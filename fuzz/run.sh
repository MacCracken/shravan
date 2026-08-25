#!/usr/bin/env bash
# Run all shravan fuzz harnesses.
#
#   ./fuzz/run.sh            # quick pass  (10000 iters codecs, 4 x 500 decode)
#   ./fuzz/run.sh 90000      # roadmap gate: >=90K calls per harness
#
# fuzz_codecs runs in one process. fuzz_decode is swept across several seeded
# processes instead, because alloc() is a bump allocator that never frees:
# ~34K calls costs ~230 MB RSS, so one process cannot reach a high call count.
# Each batch is a fresh process (memory reclaimed by exit) aimed at a different
# part of the input space; a failing batch prints the seed that reproduces it.
set -euo pipefail

ITERS="${1:-10000}"
CYRIUS="${CYRIUS_HOME:-$HOME/.cyrius}/bin/cyrius"
command -v cyrius >/dev/null 2>&1 && CYRIUS=cyrius

cd "$(dirname "$0")/.."
mkdir -p build

echo "=== fuzz_codecs ($ITERS iters) ==="
"$CYRIUS" build fuzz/fuzz_codecs.cyr build/fuzz_codecs 2>&1 | tail -2
./build/fuzz_codecs "$ITERS"

# fuzz_decode makes 17 calls per iteration; size each batch to stay well under
# the memory ceiling and use as many batches as needed to reach ITERS calls.
BATCH_ITERS=500
BATCHES=$(( (ITERS + (BATCH_ITERS * 17) - 1) / (BATCH_ITERS * 17) ))
[ "$BATCHES" -lt 4 ] && BATCHES=4

echo ""
echo "=== fuzz_decode ($BATCHES batches x $BATCH_ITERS iters, seeds 1..$BATCHES) ==="
"$CYRIUS" build fuzz/fuzz_decode.cyr build/fuzz_decode 2>&1 | tail -2

DECODE_TOTAL=0
for seed in $(seq 1 "$BATCHES"); do
    OUT=$(./build/fuzz_decode "$BATCH_ITERS" "$seed") || {
        echo "FAILED in batch seed=$seed — reproduce with:"
        echo "  ./build/fuzz_decode $BATCH_ITERS $seed"
        exit 1
    }
    CALLS=$(echo "$OUT" | sed -n 's/^fuzz_decode: \([0-9]*\) calls.*/\1/p')
    DECODE_TOTAL=$(( DECODE_TOTAL + ${CALLS:-0} ))
    printf "  seed %-3s %s calls  %s\n" "$seed" "${CALLS:-?}" \
        "$(echo "$OUT" | grep -oE 'stream [0-9]+')"
done
echo "  fuzz_decode: $DECODE_TOTAL calls total, 0 crashes"

echo ""
echo "all fuzzers passed"
