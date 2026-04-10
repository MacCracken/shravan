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

while IFS= read -r line; do
    if [[ "$line" == *"ns/iter"* ]]; then
        BENCH_NAME=$(echo "$line" | sed -E 's/\(.*//;s/^ +//;s/ +$//')
        NS=$(echo "$line" | sed -E 's/.*: ([0-9]+) ns.*/\1/')
        echo "${TIMESTAMP},${COMMIT},${BRANCH},${BENCH_NAME},${NS}" >> "$HISTORY_FILE"
    elif [[ "$line" == *"us/iter"* ]]; then
        BENCH_NAME=$(echo "$line" | sed -E 's/\(.*//;s/^ +//;s/ +$//')
        US=$(echo "$line" | sed -E 's/.*: ([0-9]+)\..*/\1/')
        NS=$((US * 1000))
        echo "${TIMESTAMP},${COMMIT},${BRANCH},${BENCH_NAME},${NS}" >> "$HISTORY_FILE"
    elif [[ "$line" == *"ms/iter"* ]]; then
        BENCH_NAME=$(echo "$line" | sed -E 's/\(.*//;s/^ +//;s/ +$//')
        MS=$(echo "$line" | sed -E 's/.*: ([0-9]+)\..*/\1/')
        NS=$((MS * 1000000))
        echo "${TIMESTAMP},${COMMIT},${BRANCH},${BENCH_NAME},${NS}" >> "$HISTORY_FILE"
    fi
done <<< "$BENCH_OUTPUT"

echo "════════════════════════════════════════════"
echo "  Benchmarks recorded to ${HISTORY_FILE}"
echo "════════════════════════════════════════════"
