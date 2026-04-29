#!/usr/bin/env bash
# Publish a fixed set of awsim crates to crates.io with a delay between each
# publish, since crates.io rate-limits new-crate registration to roughly one
# per 10 minutes per account.
#
# Usage:
#   scripts/publish-crates.sh                # publish for real, 600s between each
#   scripts/publish-crates.sh --dry-run      # cargo publish --dry-run, no sleep
#   scripts/publish-crates.sh --delay 900    # custom delay (seconds)
#   scripts/publish-crates.sh --start awsim-pinpoint
#                                            # resume from a specific crate
#
# Behaviour:
#   - Aborts on any real failure (network, build error, auth).
#   - Detects "crate version is already uploaded" responses and skips that crate
#     (no sleep) so the script is safe to re-run after a crash mid-batch.
#   - Skips the sleep after the final crate.
#   - Uses --no-verify by default (verify build is the same one we just shipped
#     to git). Pass --verify to flip it back on.

set -euo pipefail

CRATES=(
  awsim-memorydb
  awsim-mq
  awsim-pinpoint
  awsim-pipes
  awsim-resourcegroupstagging
  awsim-servicediscovery
  awsim-sns
  awsim-sso-admin
  awsim-stepfunctions
  awsim-sts
  awsim-transfer
  awsim-xray
)

delay=600
dry_run=0
verify=0
start_at=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) dry_run=1; shift ;;
    --verify) verify=1; shift ;;
    --delay) delay="$2"; shift 2 ;;
    --start) start_at="$2"; shift 2 ;;
    --help|-h)
      sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "unknown flag: $1" >&2; exit 2 ;;
  esac
done

# Move to repo root so cargo finds the workspace.
cd "$(dirname "$0")/.."

publish_args=(--locked)
if (( verify == 0 )); then publish_args+=(--no-verify); fi
if (( dry_run == 1 )); then publish_args+=(--dry-run); fi

# If --start was passed, drop everything before it.
if [[ -n "$start_at" ]]; then
  found=0
  trimmed=()
  for c in "${CRATES[@]}"; do
    if (( found == 1 )) || [[ "$c" == "$start_at" ]]; then
      found=1
      trimmed+=("$c")
    fi
  done
  if (( ${#trimmed[@]} == 0 )); then
    echo "error: --start crate '$start_at' not found in CRATES list" >&2
    exit 2
  fi
  CRATES=("${trimmed[@]}")
fi

total=${#CRATES[@]}
echo "publishing $total crates (delay=${delay}s, dry_run=$dry_run, verify=$verify)"
echo "list: ${CRATES[*]}"
echo

i=0
for crate in "${CRATES[@]}"; do
  i=$((i + 1))
  echo "==[$i/$total]== $crate"
  log=$(mktemp)
  if cargo publish -p "$crate" "${publish_args[@]}" 2>&1 | tee "$log"; then
    echo "ok: $crate"
    rm -f "$log"
  else
    rc=${PIPESTATUS[0]}
    if grep -qE "already (uploaded|exists)" "$log"; then
      echo "skip: $crate (version already on crates.io)"
      rm -f "$log"
      # Treat as a no-op: skip the rate-limit sleep too.
      continue
    fi
    echo "FAIL: $crate (cargo publish exit $rc)" >&2
    rm -f "$log"
    exit "$rc"
  fi

  if (( i < total && dry_run == 0 )); then
    echo "sleeping ${delay}s for crates.io rate limit..."
    sleep "$delay"
  fi
done

echo
echo "done."
