#!/usr/bin/env bash
# Run all shravan fuzz harnesses.
# Usage: ./fuzz/run.sh [iters]   (default: 10000 per harness)
set -euo pipefail

ITERS="${1:-10000}"
CYRIUS="${CYRIUS_HOME:-$HOME/.cyrius}/bin/cyrius"

mkdir -p build

echo "=== fuzz_codecs ($ITERS iters) ==="
"$CYRIUS" build fuzz/fuzz_codecs.cyr build/fuzz_codecs 2>&1 | tail -2
./build/fuzz_codecs "$ITERS"

echo ""
echo "all fuzzers passed"
