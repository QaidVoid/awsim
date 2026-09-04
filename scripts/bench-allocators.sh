#!/usr/bin/env bash
#
# bench-allocators.sh
#
# Compare the three allocator choices for a glibc AWSim build under the
# server's own HTTP load: the system allocator (glibc malloc), mimalloc,
# and jemalloc. The script builds one release binary per variant, then
# runs the same workload matrix against each one and reports throughput,
# latency, and resident memory side by side.
#
# The allocator is chosen at build time by the awsim crate's
# `alloc-mimalloc` / `alloc-jemalloc` features, so each binary links the
# allocator the same way a shipped build would.
#
# Requires: cargo, oha, jq, curl, python3. Linux only (reads /proc).
#
# Usage:
#   scripts/bench-allocators.sh            full matrix
#   scripts/bench-allocators.sh --quick    short smoke run
#   scripts/bench-allocators.sh --help

set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)

# ---------------------------------------------------------------------------
# defaults
# ---------------------------------------------------------------------------

ALL_VARIANTS=(system mimalloc jemalloc)
ALL_WORKLOADS=(sts-tiny ddb-put-1k ddb-query s3-put-256k s3-get-256k sqs-grow-8k)

VARIANTS=("${ALL_VARIANTS[@]}")
WORKLOADS=("${ALL_WORKLOADS[@]}")
CONNS=(16 128)
DURATION=10s
WARMUP=3s
IDLE_SETTLE=5
REPEATS=3
BASE_PORT=4699
JOBS=4
BASELINE=system
SKIP_BUILD=0
PIN=1
OUT_DIR=""
REPORT_ONLY=""
SCRATCH=""

BIN_DIR="$REPO_ROOT/target/bench-allocators/bin"
PAYLOAD_DIR="$REPO_ROOT/target/bench-allocators/payloads"

# ---------------------------------------------------------------------------
# output helpers
# ---------------------------------------------------------------------------

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  C_RESET=$'\033[0m'; C_BOLD=$'\033[1m'; C_DIM=$'\033[2m'
  C_RED=$'\033[31m'; C_GREEN=$'\033[32m'; C_YELLOW=$'\033[33m'
  C_BLUE=$'\033[34m'; C_CYAN=$'\033[36m'
  COLOR=1
else
  C_RESET=''; C_BOLD=''; C_DIM=''
  C_RED=''; C_GREEN=''; C_YELLOW=''; C_BLUE=''; C_CYAN=''
  COLOR=0
fi

say()  { printf '%s\n' "$*"; }
step() { printf '%s==>%s %s%s%s\n' "$C_BLUE" "$C_RESET" "$C_BOLD" "$*" "$C_RESET"; }
info() { printf '    %s%s%s\n' "$C_DIM" "$*" "$C_RESET"; }
warn() { printf '%s!!%s  %s\n' "$C_YELLOW" "$C_RESET" "$*" >&2; }
die()  { printf '%sxx%s  %s\n' "$C_RED" "$C_RESET" "$*" >&2; exit 1; }

rule() {
  local title=${1:-} width=78 line pad
  line=$(printf '%*s' "$width" '' | tr ' ' '=')
  if [ -n "$title" ]; then
    pad=$((width - ${#title} - 6))
    [ "$pad" -lt 0 ] && pad=0
    printf '\n%s%s %s %s%s\n' "$C_CYAN" '===' "$title" "${line:0:$pad}" "$C_RESET"
  else
    printf '%s%s%s\n' "$C_CYAN" "$line" "$C_RESET"
  fi
}

usage() {
  cat <<'USAGE'
bench-allocators.sh - compare glibc malloc, mimalloc, and jemalloc

Options:
  --variants LIST     Comma-separated: system,mimalloc,jemalloc  (default: all)
  --workloads LIST    Comma-separated workload names             (default: all)
  --conns LIST        Comma-separated concurrency levels         (default: 16,128)
  --duration DUR      Measured load duration per cell            (default: 10s)
  --warmup DUR        Discarded warmup duration per cell         (default: 3s)
  --repeats N         Repeats of the whole matrix                (default: 3)
  --baseline VARIANT  Variant the deltas are measured against    (default: system)
  --port N            First TCP port to use                      (default: 4699)
  --jobs N            cargo -j value                             (default: 4)
  --out DIR           Results directory (default: target/bench-allocators/<stamp>)
  --scratch DIR       Where the server keeps its scratch state. Defaults to the
                      first RAM-backed directory found (/dev/shm, then
                      $XDG_RUNTIME_DIR). AWSim without --data-dir still puts the
                      DynamoDB SQLite file and the S3 blob store under TMPDIR, so
                      a disk-backed one measures the filesystem, not the allocator.
  --skip-build        Reuse binaries already in target/bench-allocators/bin
  --no-pin            Do not pin server and load generator to separate cores
  --quick             Short smoke run: 3s, 1 repeat, 2 workloads, c=16
  --report DIR        Re-render the report for a finished run and exit
  --list              List the available workloads and exit
  -h, --help          Show this help

Examples:
  scripts/bench-allocators.sh --quick
  scripts/bench-allocators.sh --workloads s3-put-256k,ddb-query --conns 16,64,256
  scripts/bench-allocators.sh --duration 30s --repeats 5
USAGE
}

workload_desc() {
  case "$1" in
    sts-tiny)     echo "STS GetCallerIdentity, tiny bodies (routing and parse overhead)" ;;
    ddb-put-1k)   echo "DynamoDB PutItem, 1 KiB item (JSON parse plus SQLite write)" ;;
    ddb-query)    echo "DynamoDB Query returning 200 items (read path, ~200 KiB responses)" ;;
    s3-put-256k)  echo "S3 PutObject, 256 KiB body (large upload buffering)" ;;
    s3-get-256k)  echo "S3 GetObject, 256 KiB body (large download path)" ;;
    sqs-grow-8k)  echo "SQS SendMessage, 8 KiB body, fixed 30k requests (heap growth and retention)" ;;
    *)            echo "unknown workload" ;;
  esac
}

# ---------------------------------------------------------------------------
# argument parsing
# ---------------------------------------------------------------------------

split_list() { echo "$1" | tr ',' ' '; }

while [ $# -gt 0 ]; do
  case "$1" in
    --variants)  read -r -a VARIANTS  <<< "$(split_list "$2")"; shift 2 ;;
    --workloads) read -r -a WORKLOADS <<< "$(split_list "$2")"; shift 2 ;;
    --conns)     read -r -a CONNS     <<< "$(split_list "$2")"; shift 2 ;;
    --duration)  DURATION=$2; shift 2 ;;
    --warmup)    WARMUP=$2; shift 2 ;;
    --repeats)   REPEATS=$2; shift 2 ;;
    --baseline)  BASELINE=$2; shift 2 ;;
    --port)      BASE_PORT=$2; shift 2 ;;
    --jobs)      JOBS=$2; shift 2 ;;
    --out)       OUT_DIR=$2; shift 2 ;;
    --scratch)   SCRATCH=$2; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    --no-pin)    PIN=0; shift ;;
    --report)    REPORT_ONLY=$2; shift 2 ;;
    --quick)
      DURATION=3s; WARMUP=1s; IDLE_SETTLE=2; REPEATS=1
      CONNS=(16); WORKLOADS=(ddb-put-1k s3-put-256k)
      shift ;;
    --list)
      say "Workloads:"
      for w in "${ALL_WORKLOADS[@]}"; do printf '  %-14s %s\n' "$w" "$(workload_desc "$w")"; done
      exit 0 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
done

for v in "${VARIANTS[@]}"; do
  case "$v" in
    system|mimalloc|jemalloc) ;;
    *) die "unknown variant: $v" ;;
  esac
done
for w in "${WORKLOADS[@]}"; do
  case " ${ALL_WORKLOADS[*]} " in
    *" $w "*) ;;
    *) die "unknown workload: $w (try --list)" ;;
  esac
done

# ---------------------------------------------------------------------------
# report renderer
# ---------------------------------------------------------------------------

render_report() {
  local dir=$1
  [ -f "$dir/results.csv" ] || die "no results.csv in $dir"
  python3 - "$dir" "$BASELINE" "$COLOR" <<'PY'
import csv, math, sys, os
from collections import defaultdict

out_dir, baseline, use_color = sys.argv[1], sys.argv[2], sys.argv[3] == "1"

RESET = "\033[0m" if use_color else ""
BOLD = "\033[1m" if use_color else ""
DIM = "\033[2m" if use_color else ""
GREEN = "\033[32m" if use_color else ""
RED = "\033[31m" if use_color else ""
CYAN = "\033[36m" if use_color else ""
YELLOW = "\033[33m" if use_color else ""

DESCS = {
    "sts-tiny": "STS GetCallerIdentity, tiny bodies",
    "ddb-put-1k": "DynamoDB PutItem, 1 KiB item",
    "ddb-query": "DynamoDB Query, 200 items per response",
    "s3-put-256k": "S3 PutObject, 256 KiB body",
    "s3-get-256k": "S3 GetObject, 256 KiB body",
    "sqs-grow-8k": "SQS SendMessage, 8 KiB body, fixed request count",
}

rows = []
with open(os.path.join(out_dir, "results.csv")) as fh:
    for r in csv.DictReader(fh):
        rows.append(r)
if not rows:
    sys.exit("results.csv is empty")


def median(xs):
    xs = sorted(xs)
    n = len(xs)
    if n == 0:
        return float("nan")
    return xs[n // 2] if n % 2 else (xs[n // 2 - 1] + xs[n // 2]) / 2


cells = defaultdict(lambda: defaultdict(list))
for r in rows:
    key = (r["workload"], int(r["conns"]), r["variant"])
    cells[key]["rps"].append(float(r["rps"]))
    cells[key]["p50"].append(float(r["p50_ms"]))
    cells[key]["p99"].append(float(r["p99_ms"]))
    cells[key]["peak"].append(float(r["peak_rss_kib"]) / 1024.0)
    cells[key]["idle"].append(float(r["idle_rss_kib"]) / 1024.0)
    cells[key]["ok"].append(float(r["success"]))

workloads, conns_seen, variants = [], [], []
for r in rows:
    if r["workload"] not in workloads:
        workloads.append(r["workload"])
    if int(r["conns"]) not in conns_seen:
        conns_seen.append(int(r["conns"]))
    if r["variant"] not in variants:
        variants.append(r["variant"])
conns_seen.sort()


def agg(workload, conns, variant):
    c = cells.get((workload, conns, variant))
    if not c:
        return None
    rps = c["rps"]
    mid = median(rps)
    # Half the observed range, as a percentage of the median. Treat any
    # difference smaller than this as indistinguishable from noise.
    spread = ((max(rps) - min(rps)) / 2.0 / mid * 100.0) if (len(rps) > 1 and mid) else None
    return {
        "rps": mid,
        "spread": spread,
        "p50": median(c["p50"]),
        "p99": median(c["p99"]),
        "peak": median(c["peak"]),
        "idle": median(c["idle"]),
        "ok": min(c["ok"]),
        "n": len(rps),
    }


def pct(value, base):
    if base is None or base == 0:
        return None
    return (value / base - 1.0) * 100.0


def cell(text, width, color=""):
    return "{}{:>{w}}{}".format(color, text, RESET if color else "", w=width)


def fmt_delta(p, noise):
    if p is None:
        return cell("baseline", 9, DIM)
    txt = "{:+.1f}%".format(p)
    # Dim anything inside the measured noise band: it is not a result.
    if noise is not None and abs(p) <= noise:
        return cell(txt, 9, DIM)
    if p >= 2.0:
        return cell(txt, 9, GREEN)
    if p <= -2.0:
        return cell(txt, 9, RED)
    return cell(txt, 9, DIM)


def fmt_spread(a):
    if a["spread"] is None:
        return cell("-", 7, DIM)
    return cell("+-{:.1f}%".format(a["spread"]), 7, DIM)


HEADER = "  {:>5}  {:<10} {:>10} {:>7}  {:>9}  {:>8}  {:>8}  {:>10}  {:>10}".format(
    "conc", "allocator", "rps", "spread", "vs base", "p50 ms", "p99 ms",
    "peak RSS", "idle RSS")

md = ["# Allocator benchmark\n"]
meta_path = os.path.join(out_dir, "meta.txt")
if os.path.exists(meta_path):
    md.append("```\n" + open(meta_path).read().strip() + "\n```\n")
md.append("`spread` is half the range across repeats, as a percentage of the "
          "median. A `vs base` delta smaller than the spread of either row is "
          "noise, not a result.\n")

lines = []
for w in workloads:
    title = DESCS.get(w, w)
    bar = "=" * max(3, 74 - len(w) - len(title))
    lines.append("")
    lines.append("{}== {}: {} {}{}".format(CYAN, w, title, bar, RESET))
    lines.append("{}{}{}".format(BOLD, HEADER, RESET))
    md.append("\n## {}\n\n{}\n".format(w, title))
    md.append("| conc | allocator | rps | spread | vs base | p50 ms | p99 ms | peak RSS | idle RSS |")
    md.append("|---:|---|---:|---:|---:|---:|---:|---:|---:|")
    for c in conns_seen:
        base = agg(w, c, baseline)
        base_rps = base["rps"] if base else None
        for v in variants:
            a = agg(w, c, v)
            if not a:
                continue
            delta = None if v == baseline else pct(a["rps"], base_rps)
            noise = None
            if base and base["spread"] is not None and a["spread"] is not None:
                noise = base["spread"] + a["spread"]
            flag = "" if a["ok"] >= 0.99 else " {}(errors){}".format(YELLOW, RESET)
            lines.append(
                "  {:>5}  {:<10} {:>10} {}  {}  {:>8.2f}  {:>8.2f}  {:>9.1f}M  {:>9.1f}M{}".format(
                    c, v, "{:,.0f}".format(a["rps"]), fmt_spread(a),
                    fmt_delta(delta, noise), a["p50"], a["p99"],
                    a["peak"], a["idle"], flag))
            md.append("| {} | {} | {:,.0f} | {} | {} | {:.2f} | {:.2f} | {:.1f} MiB | {:.1f} MiB |".format(
                c, v, a["rps"],
                "-" if a["spread"] is None else "+-{:.1f}%".format(a["spread"]),
                "baseline" if delta is None else "{:+.1f}%".format(delta),
                a["p50"], a["p99"], a["peak"], a["idle"]))
        if c != conns_seen[-1]:
            lines.append("")


def geomean(xs):
    xs = [x for x in xs if x and x > 0]
    if not xs:
        return None
    return math.exp(sum(math.log(x) for x in xs) / len(xs))


summary_rows = []
for v in variants:
    rps_r, p99_r, peak_r, idle_r = [], [], [], []
    for w in workloads:
        for c in conns_seen:
            b, a = agg(w, c, baseline), agg(w, c, v)
            if not b or not a:
                continue
            rps_r.append(a["rps"] / b["rps"] if b["rps"] else None)
            p99_r.append(a["p99"] / b["p99"] if b["p99"] else None)
            peak_r.append(a["peak"] / b["peak"] if b["peak"] else None)
            idle_r.append(a["idle"] / b["idle"] if b["idle"] else None)
    summary_rows.append((v, geomean(rps_r), geomean(p99_r),
                         geomean(peak_r), geomean(idle_r)))


def fmt_ratio(r, width, higher_is_better):
    if r is None:
        return cell("n/a", width, DIM)
    p = (r - 1.0) * 100.0
    txt = "{:+.1f}%".format(p)
    good = p >= 2.0 if higher_is_better else p <= -2.0
    bad = p <= -2.0 if higher_is_better else p >= 2.0
    return cell(txt, width, GREEN if good else (RED if bad else DIM))


lines.append("")
lines.append("{}== summary {}{}".format(CYAN, "=" * 67, RESET))
lines.append("{}  {:<10} {:>12}  {:>10}  {:>10}  {:>10}{}".format(
    BOLD, "allocator", "throughput", "p99", "peak RSS", "idle RSS", RESET))
md.append("\n## Summary\n")
md.append("Geometric mean across every workload and concurrency level, "
          "relative to `{}`.\n".format(baseline))
md.append("| allocator | throughput | p99 | peak RSS | idle RSS |")
md.append("|---|---:|---:|---:|---:|")

for v, rps_g, p99_g, peak_g, idle_g in summary_rows:
    mark = "  (baseline)" if v == baseline else ""
    lines.append("  {:<10} {}  {}  {}  {}{}".format(
        v, fmt_ratio(rps_g, 12, True), fmt_ratio(p99_g, 10, False),
        fmt_ratio(peak_g, 10, False), fmt_ratio(idle_g, 10, False),
        DIM + mark + RESET))

    def md_r(r):
        return "n/a" if r is None else "{:+.1f}%".format((r - 1.0) * 100.0)

    md.append("| {}{} | {} | {} | {} | {} |".format(
        v, " (baseline)" if v == baseline else "",
        md_r(rps_g), md_r(p99_g), md_r(peak_g), md_r(idle_g)))

ranked = [s for s in summary_rows if s[1]]
if ranked:
    best = max(ranked, key=lambda s: s[1])
    lines.append("")
    lines.append("  {}fastest overall: {}{} ({:+.1f}% throughput vs {})".format(
        BOLD, best[0], RESET, (best[1] - 1.0) * 100.0, baseline))
    if len(rows) // max(1, len(cells)) < 2:
        lines.append("  {}single repeat: no spread was measured, so treat small "
                     "deltas as noise{}".format(DIM, RESET))

print("\n".join(lines))
with open(os.path.join(out_dir, "report.md"), "w") as fh:
    fh.write("\n".join(md) + "\n")
PY
  say ""
  info "markdown report: $dir/report.md"
}

if [ -n "$REPORT_ONLY" ]; then
  render_report "$REPORT_ONLY"
  exit 0
fi

# ---------------------------------------------------------------------------
# preflight
# ---------------------------------------------------------------------------

[ "$(uname -s)" = "Linux" ] || die "this script reads /proc, so it is Linux only"

for tool in cargo oha jq curl python3; do
  command -v "$tool" >/dev/null 2>&1 || die "missing required tool: $tool"
done

NCPU=$(nproc)
SERVER_CPUS=""
LOAD_CPUS=""
if [ "$PIN" = 1 ]; then
  if command -v taskset >/dev/null 2>&1 && [ "$NCPU" -ge 8 ]; then
    half=$((NCPU / 2))
    SERVER_CPUS="0-$((half - 1))"
    LOAD_CPUS="$half-$((NCPU - 1))"
  else
    PIN=0
    warn "cpu pinning disabled (needs taskset and at least 8 cores)"
  fi
fi

# AWSim run without --data-dir still writes the DynamoDB SQLite file and
# the S3 / SQS blob stores under TMPDIR. On a disk-backed filesystem those
# flushes dominate and swamp the allocator differences, so prefer tmpfs.
fstype_of() { findmnt -n -o FSTYPE -T "$1" 2>/dev/null || echo unknown; }

if [ -z "$SCRATCH" ]; then
  for cand in /dev/shm "${XDG_RUNTIME_DIR:-}"; do
    [ -n "$cand" ] && [ -d "$cand" ] && [ -w "$cand" ] || continue
    if [ "$(fstype_of "$cand")" = tmpfs ]; then SCRATCH=$cand; break; fi
  done
fi
if [ -z "$SCRATCH" ]; then
  SCRATCH=${TMPDIR:-/tmp}
  warn "no tmpfs found for server scratch state, falling back to $SCRATCH ($(fstype_of "$SCRATCH"))"
  warn "results will include filesystem noise, pass --scratch DIR to point at a RAM-backed one"
fi
[ -d "$SCRATCH" ] && [ -w "$SCRATCH" ] || die "scratch directory is not writable: $SCRATCH"

if [ -z "$OUT_DIR" ]; then
  OUT_DIR="$REPO_ROOT/target/bench-allocators/$(date +%Y%m%d-%H%M%S)"
fi
mkdir -p "$OUT_DIR/raw" "$OUT_DIR/logs" "$BIN_DIR" "$PAYLOAD_DIR"

CSV="$OUT_DIR/results.csv"
echo "rep,variant,workload,conns,rps,p50_ms,p99_ms,success,peak_rss_kib,idle_rss_kib,status" > "$CSV"

dur_secs() { echo "${1%s}"; }
TOTAL_CELLS=$(( ${#VARIANTS[@]} * ${#WORKLOADS[@]} * ${#CONNS[@]} * REPEATS ))
PER_CELL=$(( $(dur_secs "$DURATION") + $(dur_secs "$WARMUP") + IDLE_SETTLE + 5 ))
ETA_MIN=$(( (TOTAL_CELLS * PER_CELL + 59) / 60 ))

{
  echo "host          $(uname -srm)"
  echo "cpus          $NCPU"
  echo "rustc         $(rustc --version 2>/dev/null || echo unknown)"
  echo "commit        $(cd "$REPO_ROOT" && (jj log -r @ --no-graph -T 'commit_id.short()' 2>/dev/null || git rev-parse --short HEAD 2>/dev/null) || echo unknown)"
  echo "variants      ${VARIANTS[*]}"
  echo "workloads     ${WORKLOADS[*]}"
  echo "concurrency   ${CONNS[*]}"
  echo "duration      $DURATION (warmup $WARMUP, idle settle ${IDLE_SETTLE}s)"
  echo "repeats       $REPEATS"
  echo "pinning       $([ "$PIN" = 1 ] && echo "server $SERVER_CPUS / load $LOAD_CPUS" || echo off)"
  echo "scratch       $SCRATCH ($(fstype_of "$SCRATCH"))"
  echo "started       $(date -Is)"
} > "$OUT_DIR/meta.txt"

rule "AWSim allocator benchmark"
sed 's/^/    /' "$OUT_DIR/meta.txt"
info "cells         $TOTAL_CELLS (about $ETA_MIN min of measurement, builds on top)"
info "results       $OUT_DIR"
say ""

# ---------------------------------------------------------------------------
# build
# ---------------------------------------------------------------------------

variant_features() {
  case "$1" in
    system)   printf '' ;;
    mimalloc) printf 'alloc-mimalloc' ;;
    jemalloc) printf 'alloc-jemalloc' ;;
  esac
}

# Release profile uses fat LTO, so switching features relinks the whole
# binary. Build every variant up front and keep the binaries around.
build_variant() {
  local variant=$1 feat args bin
  feat=$(variant_features "$variant")
  args=(build --release -p awsim -j "$JOBS")
  [ -n "$feat" ] && args+=(--features "$feat")
  step "building $variant $([ -n "$feat" ] && echo "(--features $feat)" || echo "(default features)")"
  ( cd "$REPO_ROOT" && cargo "${args[@]}" ) || die "build failed for variant $variant"
  cp "$REPO_ROOT/target/release/awsim" "$BIN_DIR/awsim-$variant"
}

# The release profile strips symbols, so look for the allocator's own
# string constants instead. A mismatch means the wrong binary is staged.
verify_variant() {
  local variant=$1
  local bin="$BIN_DIR/awsim-$variant"
  local has_mi has_je
  has_mi=$(strings -a "$bin" 2>/dev/null | grep -c -i 'mimalloc' || true)
  has_je=$(strings -a "$bin" 2>/dev/null | grep -c -i 'jemalloc' || true)
  case "$variant" in
    system)   [ "$has_mi" -eq 0 ] && [ "$has_je" -eq 0 ] || warn "$variant binary mentions an allocator (mi=$has_mi je=$has_je)" ;;
    mimalloc) [ "$has_mi" -gt 0 ] || warn "$variant binary has no mimalloc strings" ;;
    jemalloc) [ "$has_je" -gt 0 ] || warn "$variant binary has no jemalloc strings" ;;
  esac
}

if [ "$SKIP_BUILD" = 1 ]; then
  for v in "${VARIANTS[@]}"; do
    [ -x "$BIN_DIR/awsim-$v" ] || die "--skip-build set but $BIN_DIR/awsim-$v is missing"
  done
  info "reusing binaries in $BIN_DIR"
else
  for v in "${VARIANTS[@]}"; do build_variant "$v"; done
fi
for v in "${VARIANTS[@]}"; do
  command -v strings >/dev/null 2>&1 && verify_variant "$v"
done

# ---------------------------------------------------------------------------
# payloads
# ---------------------------------------------------------------------------

make_payloads() {
  [ -f "$PAYLOAD_DIR/blob-256k.bin" ] || head -c 262144 /dev/urandom > "$PAYLOAD_DIR/blob-256k.bin"
  python3 - "$PAYLOAD_DIR" <<'PY'
import json, os, sys
d = sys.argv[1]
blob = "x" * 900
with open(os.path.join(d, "ddb-put.json"), "w") as fh:
    json.dump({"TableName": "bench",
               "Item": {"pk": {"S": "p0"}, "sk": {"S": "hot"},
                        "data": {"S": blob}, "n": {"N": "1"}}}, fh)
with open(os.path.join(d, "ddb-query.json"), "w") as fh:
    json.dump({"TableName": "bench",
               "KeyConditionExpression": "pk = :p",
               "ExpressionAttributeValues": {":p": {"S": "p0"}}}, fh)
PY
}

# ---------------------------------------------------------------------------
# server lifecycle
# ---------------------------------------------------------------------------

SERVER_PID=""

cleanup() {
  if [ -n "$SERVER_PID" ]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

start_server() {
  local bin=$1 port=$2 log=$3 scratch=$4
  local cmd i
  cmd=("$bin" --port "$port" --bind 127.0.0.1 --log-level warn)
  [ "$PIN" = 1 ] && cmd=(taskset -c "$SERVER_CPUS" "${cmd[@]}")
  env -u AWSIM_DATA_DIR -u AWSIM_PORT -u AWSIM_LOG_LEVEL \
      TMPDIR="$scratch" TMP="$scratch" "${cmd[@]}" > "$log" 2>&1 &
  SERVER_PID=$!
  for i in $(seq 1 300); do
    if curl -sf -m 1 "http://127.0.0.1:$port/_awsim/health" > /dev/null 2>&1; then
      return 0
    fi
    kill -0 "$SERVER_PID" 2>/dev/null || { tail -n 20 "$log" >&2; die "server exited during startup"; }
    sleep 0.1
  done
  die "server on port $port never became healthy (see $log)"
}

stop_server() {
  [ -n "$SERVER_PID" ] || return 0
  kill "$SERVER_PID" 2>/dev/null || true
  wait "$SERVER_PID" 2>/dev/null || true
  SERVER_PID=""
}

rss_kib()      { awk '/^VmRSS:/  {print $2}' "/proc/$1/status" 2>/dev/null || echo 0; }
peak_rss_kib() { awk '/^VmHWM:/  {print $2}' "/proc/$1/status" 2>/dev/null || echo 0; }

# Drop the peak-RSS watermark so warmup and seeding do not inflate it.
reset_peak_rss() { echo 5 > "/proc/$1/clear_refs" 2>/dev/null || true; }

# ---------------------------------------------------------------------------
# workloads
# ---------------------------------------------------------------------------

# AWSim resolves the target service from the SigV4 credential scope for
# protocols that carry no X-Amz-Target header (S3, and the query
# protocols). The signature itself is not checked by default.
sigv4() {
  printf 'Authorization: AWS4-HMAC-SHA256 Credential=bench/20250101/us-east-1/%s/aws4_request, SignedHeaders=host, Signature=00' "$1"
}

ddb_post() {
  local base=$1 target=$2 body=$3
  curl -sf -o /dev/null -X POST "$base/" \
    -H "X-Amz-Target: DynamoDB_20120810.$target" \
    -H 'Content-Type: application/x-amz-json-1.0' \
    -d "$body" || die "DynamoDB $target failed during seeding"
}

seed_workload() {
  local wl=$1 base=$2 tmp=$3
  case "$wl" in
    sts-tiny) ;;

    ddb-put-1k|ddb-query)
      ddb_post "$base" CreateTable '{"TableName":"bench",
        "KeySchema":[{"AttributeName":"pk","KeyType":"HASH"},{"AttributeName":"sk","KeyType":"RANGE"}],
        "AttributeDefinitions":[{"AttributeName":"pk","AttributeType":"S"},{"AttributeName":"sk","AttributeType":"S"}],
        "BillingMode":"PAY_PER_REQUEST"}'
      if [ "$wl" = ddb-query ]; then
        python3 - "$base" 200 <<'PY'
import json, sys, urllib.request
base, n = sys.argv[1], int(sys.argv[2])
blob = "x" * 900
items = [{"PutRequest": {"Item": {"pk": {"S": "p0"},
                                  "sk": {"S": "s%06d" % i},
                                  "data": {"S": blob}}}} for i in range(n)]
for i in range(0, n, 25):
    body = json.dumps({"RequestItems": {"bench": items[i:i + 25]}}).encode()
    req = urllib.request.Request(base + "/", data=body, headers={
        "X-Amz-Target": "DynamoDB_20120810.BatchWriteItem",
        "Content-Type": "application/x-amz-json-1.0"})
    urllib.request.urlopen(req).read()
PY
      fi
      ;;

    s3-put-256k|s3-get-256k)
      curl -sf -o /dev/null -X PUT -H "$(sigv4 s3)" "$base/bench-bucket" \
        || die "S3 CreateBucket failed during seeding"
      if [ "$wl" = s3-get-256k ]; then
        curl -sf -o /dev/null -X PUT -H "$(sigv4 s3)" \
          --data-binary "@$PAYLOAD_DIR/blob-256k.bin" "$base/bench-bucket/obj" \
          || die "S3 PutObject failed during seeding"
      fi
      ;;

    sqs-grow-8k)
      local queue_url
      queue_url=$(curl -sf -X POST "$base/" \
        -H 'X-Amz-Target: AmazonSQS.CreateQueue' \
        -H 'Content-Type: application/x-amz-json-1.0' \
        -d '{"QueueName":"bench-q"}' | jq -r '.QueueUrl') \
        || die "SQS CreateQueue failed during seeding"
      [ -n "$queue_url" ] && [ "$queue_url" != null ] || die "SQS CreateQueue returned no QueueUrl"
      python3 - "$queue_url" "$tmp/sqs-send.json" <<'PY'
import json, sys
url, out = sys.argv[1], sys.argv[2]
with open(out, "w") as fh:
    json.dump({"QueueUrl": url, "MessageBody": "y" * 8192}, fh)
PY
      ;;
  esac
}

# Workloads that grow the heap are run for a fixed number of requests
# rather than a fixed time. Otherwise the faster allocator enqueues more
# messages and its resident memory looks worse for that reason alone.
workload_count() {
  case "$1" in
    sqs-grow-8k) echo 30000 ;;
    *)           echo "" ;;
  esac
}

# Fills OHA_ARGS with everything after the shared oha flags.
build_oha_args() {
  local wl=$1 base=$2 tmp=$3
  case "$wl" in
    sts-tiny)
      OHA_ARGS=(-m POST -H "$(sigv4 sts)"
                -T 'application/x-www-form-urlencoded'
                -d 'Action=GetCallerIdentity&Version=2011-06-15'
                "$base/") ;;
    ddb-put-1k)
      OHA_ARGS=(-m POST -H 'X-Amz-Target: DynamoDB_20120810.PutItem'
                -T 'application/x-amz-json-1.0'
                -D "$PAYLOAD_DIR/ddb-put.json" "$base/") ;;
    ddb-query)
      OHA_ARGS=(-m POST -H 'X-Amz-Target: DynamoDB_20120810.Query'
                -T 'application/x-amz-json-1.0'
                -D "$PAYLOAD_DIR/ddb-query.json" "$base/") ;;
    s3-put-256k)
      OHA_ARGS=(-m PUT -H "$(sigv4 s3)"
                -D "$PAYLOAD_DIR/blob-256k.bin" "$base/bench-bucket/obj") ;;
    s3-get-256k)
      OHA_ARGS=(-m GET -H "$(sigv4 s3)" "$base/bench-bucket/obj") ;;
    sqs-grow-8k)
      OHA_ARGS=(-m POST -H 'X-Amz-Target: AmazonSQS.SendMessage'
                -T 'application/x-amz-json-1.0'
                -D "$tmp/sqs-send.json" "$base/") ;;
  esac
}

oha_run() {
  local flag=$1 limit=$2 conns=$3 out=$4
  local cmd
  cmd=(oha --no-tui --output-format json --disable-compression
       "$flag" "$limit" -c "$conns" "${OHA_ARGS[@]}")
  [ "$PIN" = 1 ] && cmd=(taskset -c "$LOAD_CPUS" "${cmd[@]}")
  "${cmd[@]}" > "$out" 2>> "$OUT_DIR/logs/oha.err" || die "oha failed (see $OUT_DIR/logs/oha.err)"
}

# ---------------------------------------------------------------------------
# the matrix
# ---------------------------------------------------------------------------

make_payloads

TMP_ROOT=$(mktemp -d -t awsim-bench-XXXXXX)
cleanup_tmp() { rm -rf "$TMP_ROOT"; }
trap 'cleanup; cleanup_tmp' EXIT INT TERM

PORT_OFFSET=0
CELL=0

run_cell() {
  local rep=$1 variant=$2 wl=$3 conns=$4
  local port=$((BASE_PORT + PORT_OFFSET)); PORT_OFFSET=$((PORT_OFFSET + 1))
  local base="http://127.0.0.1:$port"
  local tag="$variant-$wl-c$conns-r$rep"
  local log="$OUT_DIR/logs/$tag.log"
  local json="$OUT_DIR/raw/$tag.json"

  CELL=$((CELL + 1))
  printf '  %s[%2d/%2d]%s %-10s %-13s c=%-4s ' \
    "$C_DIM" "$CELL" "$TOTAL_CELLS" "$C_RESET" "$variant" "$wl" "$conns"

  local scratch
  scratch=$(mktemp -d "$SCRATCH/awsim-bench-XXXXXX")
  start_server "$BIN_DIR/awsim-$variant" "$port" "$log" "$scratch"
  seed_workload "$wl" "$base" "$TMP_ROOT"
  build_oha_args "$wl" "$base" "$TMP_ROOT"

  local count
  count=$(workload_count "$wl")
  if [ -n "$count" ]; then
    oha_run -n "$((count / 10))" "$conns" /dev/null
    reset_peak_rss "$SERVER_PID"
    oha_run -n "$count" "$conns" "$json"
  else
    oha_run -z "$WARMUP" "$conns" /dev/null
    reset_peak_rss "$SERVER_PID"
    oha_run -z "$DURATION" "$conns" "$json"
  fi

  local peak idle rps p50 p99 ok status
  peak=$(peak_rss_kib "$SERVER_PID")
  sleep "$IDLE_SETTLE"
  idle=$(rss_kib "$SERVER_PID")
  stop_server
  rm -rf "$scratch"

  read -r rps p50 p99 ok <<< "$(jq -r '[.metrics.requests_per_sec,
                                        .metrics.latency_ms.p50,
                                        .metrics.latency_ms.p99,
                                        .metrics.success_rate] | @tsv' "$json")"
  status=$(jq -r '.statusCodeDistribution | to_entries
                  | map("\(.key)x\(.value)") | join("+")' "$json")
  case "$rps" in
    ''|null) die "could not read throughput from oha output ($json). oha 1.15 or newer is expected." ;;
  esac

  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "$rep" "$variant" "$wl" "$conns" "$rps" "$p50" "$p99" "$ok" "$peak" "$idle" "$status" >> "$CSV"

  printf '%9.0f rps  p99 %6.2f ms  peak %6.1f MiB\n' \
    "$rps" "$p99" "$(echo "$peak" | awk '{print $1/1024}')"

  case "$status" in
    2*) ;;
    *) warn "$tag returned non-2xx responses: $status" ;;
  esac
}

rule "running"
for rep in $(seq 1 "$REPEATS"); do
  # Rotate the variant order between repeats so thermal drift and
  # background noise do not systematically favour whoever runs first.
  ordered=()
  n=${#VARIANTS[@]}
  for ((i = 0; i < n; i++)); do
    ordered+=("${VARIANTS[$(( (i + rep - 1) % n ))]}")
  done
  say "  ${C_DIM}repeat $rep/$REPEATS, order: ${ordered[*]}${C_RESET}"
  for wl in "${WORKLOADS[@]}"; do
    for conns in "${CONNS[@]}"; do
      for variant in "${ordered[@]}"; do
        run_cell "$rep" "$variant" "$wl" "$conns"
      done
    done
  done
done

echo "finished      $(date -Is)" >> "$OUT_DIR/meta.txt"

rule "results"
render_report "$OUT_DIR"
