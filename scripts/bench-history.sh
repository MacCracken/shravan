#!/usr/bin/env bash
set -euo pipefail

HISTORY_FILE="${1:-bench-history.csv}"
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
BRANCH=$(git branch --show-current 2>/dev/null || echo "unknown")

if [ ! -f "$HISTORY_FILE" ]; then
    echo "timestamp,commit,branch,benchmark,estimate_ns" > "$HISTORY_FILE"
fi

CYRB="${CYRB:-}"
if [ -z "$CYRB" ]; then
    if command -v cyrius >/dev/null 2>&1; then CYRB=cyrius
    elif [ -x "$HOME/.cyrius/bin/cyrius" ]; then CYRB="$HOME/.cyrius/bin/cyrius"
    elif [ -x "./build/cyrius" ]; then CYRB="./build/cyrius"
    else echo "ERROR: cyrius not found"; exit 1; fi
fi

mkdir -p build
$CYRB build src/bench.cyr build/bench 2>&1
BENCH_OUTPUT=$(./build/bench 2>&1)
echo "$BENCH_OUTPUT"

# Each unit branch used to sed off the fractional digits before scaling, so
# "1.390 ms/iter" was recorded as 1000000 ns -- a 28% error in the very file the
# project treats as its proof. Scale the full decimal instead, and round.
while IFS= read -r line; do
    case "$line" in
        *ns/iter*) UNIT=1 ;;
        *us/iter*) UNIT=1000 ;;
        *ms/iter*) UNIT=1000000 ;;
        *) continue ;;
    esac
    BENCH_NAME=$(echo "$line" | sed -E 's/\(.*//;s/^ +//;s/ +$//')
    VALUE=$(echo "$line" | sed -E 's/.*: ([0-9]+(\.[0-9]+)?) [nu m]*s\/iter.*/\1/')
    NS=$(awk -v v="$VALUE" -v u="$UNIT" 'BEGIN { printf "%.0f", v * u }')
    echo "${TIMESTAMP},${COMMIT},${BRANCH},${BENCH_NAME},${NS}" >> "$HISTORY_FILE"
done <<< "$BENCH_OUTPUT"

echo "════════════════════════════════════════════"
echo "  Benchmarks recorded to ${HISTORY_FILE}"
echo "════════════════════════════════════════════"
