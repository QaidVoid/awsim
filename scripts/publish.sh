#!/bin/sh
# Publish every workspace crate to crates.io.
#
# Crates whose current version is already in the crates.io index are
# excluded, so the script is safe to rerun after any failure. When
# crates.io rate limits an upload, the script sleeps until the time it
# names and then publishes whatever is left.
#
# Usage: scripts/publish.sh [cargo publish args, e.g. --dry-run]

set -eu

cd "$(dirname "$0")/.."
root=$(pwd -P)
mkdir -p target
log=target/publish.log
status=target/publish.status

: "${CARGO_BUILD_JOBS:=4}"
export CARGO_BUILD_JOBS
if [ -t 1 ]; then
    : "${CARGO_TERM_COLOR:=always}"
    export CARGO_TERM_COLOR
fi

# Path of a crate's file in the crates.io sparse index.
index_path() {
    name=$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')
    case ${#name} in
        1 | 2) echo "${#name}/$name" ;;
        3) echo "3/$(printf '%s' "$name" | cut -c1)/$name" ;;
        *) echo "$(printf '%s' "$name" | cut -c1-2)/$(printf '%s' "$name" | cut -c3-4)/$name" ;;
    esac
}

is_published() {
    curl -fsS "https://index.crates.io/$(index_path "$1")" 2>/dev/null |
        grep -q "\"vers\":\"$2\""
}

# Set $excludes to --exclude flags for published crates and $pending to
# the crates that still need publishing.
scan() {
    excludes=
    pending=
    published=0
    echo "Checking crates.io for already published crates..."
    cargo tree --workspace --depth 0 --prefix none --format '{p}' >"$log"
    while read -r name version path; do
        [ -n "$name" ] || continue
        path=${path#\(}
        path=${path%\)}
        case $path in "$root"/*) ;; *) continue ;; esac
        if grep -q '^publish[[:space:]]*=[[:space:]]*false' "$path/Cargo.toml"; then
            continue
        fi
        if is_published "$name" "${version#v}"; then
            excludes="$excludes --exclude $name"
            published=$((published + 1))
        else
            pending="$pending $name"
        fi
    done <"$log"
    echo "$published crates already published."
}

# Seconds from now until an HTTP date such as "Sat, 17 Oct 2026 03:53:45 GMT",
# plus a small margin.
seconds_until() {
    date -u '+%Y %m %d %H %M %S' | awk -v t="$1" '
        function days(y, m, d) {
            if (m <= 2) { y--; m += 12 }
            return 365 * y + int(y / 4) - int(y / 100) + int(y / 400) \
                + int((153 * (m - 3) + 2) / 5) + d
        }
        function secs(y, m, d, hh, mm, ss) {
            return days(y, m, d) * 86400 + hh * 3600 + mm * 60 + ss
        }
        {
            split(t, f, /[ :,]+/)
            mon = (index("JanFebMarAprMayJunJulAugSepOctNovDec", f[3]) + 2) / 3
            w = secs(f[4], mon, f[2], f[5], f[6], f[7]) - secs($1, $2, $3, $4, $5, $6)
            print (w > 0 ? w : 0) + 5
        }'
}

while :; do
    scan
    if [ -z "$pending" ]; then
        echo "All crates are already published."
        exit 0
    fi
    echo "Publishing:$pending"

    echo 0 >"$status"
    # shellcheck disable=SC2086 # $excludes is a list of flags.
    { cargo publish --workspace $excludes "$@" || echo $? >"$status"; } 2>&1 | tee "$log"
    [ "$(cat "$status")" -ne 0 ] || exit 0

    retry=$(sed -n 's/.*try again after \(.* GMT\).*/\1/p' "$log" | head -n 1)
    if [ -n "$retry" ]; then
        wait=$(seconds_until "$retry")
    elif grep -q '429 Too Many Requests' "$log"; then
        wait=60
    else
        exit 1
    fi
    echo "Rate limited by crates.io, retrying in ${wait}s." >&2
    sleep "$wait"
done
