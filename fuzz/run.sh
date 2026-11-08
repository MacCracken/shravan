#!/usr/bin/env bash
# Run all shravan fuzz harnesses.
#
#   ./fuzz/run.sh                    # quick pass: >= 10000 calls per harness
#   ./fuzz/run.sh 90000              # roadmap gate: >= 90K calls per harness
#   ./fuzz/run.sh 90000 --poison     # the same under cyrius's allocator poison mode
#   ./fuzz/run.sh 10000 --bounds     # ... or its bounds-checked build
#
# fuzz_codecs runs in one process. fuzz_decode and fuzz_opus are swept across
# seeded processes instead, because alloc() is a bump allocator that never frees:
# a process holds every decode's output until it exits. Each batch is a fresh
# process aimed at a different part of the input space, batches run until the
# harness has made the requested number of calls, and a failing batch prints the
# command that reproduces it.
#
# --poison builds with CYRIUS_POISON: every allocation gets redzones, a hit exits
# 86 at once, and each harness ends with a full sweep. --bounds builds with
# CYRIUS_BOUNDS.
set -euo pipefail

ITERS=10000
BUILD_FLAGS=()
TAG=""
for arg in "$@"; do
    case "$arg" in
        --poison) BUILD_FLAGS+=(-D CYRIUS_POISON); TAG=" (poison)" ;;
        --bounds) BUILD_FLAGS+=(--bounds); TAG=" (bounds)" ;;
        *) ITERS="$arg" ;;
    esac
done
CYRIUS="${CYRIUS_HOME:-$HOME/.cyrius}/bin/cyrius"
command -v cyrius >/dev/null 2>&1 && CYRIUS=cyrius

cd "$(dirname "$0")/.."
mkdir -p build

build() {
    rm -f "build/$1"
    "$CYRIUS" build "${BUILD_FLAGS[@]}" "fuzz/$1.cyr" "build/$1" 2>&1 | tail -1
    [ -x "build/$1" ] || { echo "build of fuzz/$1.cyr failed"; exit 1; }
}

echo "=== fuzz_codecs$TAG ($ITERS iters) ==="
build fuzz_codecs
./build/fuzz_codecs "$ITERS"

# sweep NAME BATCH_ARG MIN_BATCHES: run seeded batches until ITERS calls.
sweep() {
    local name="$1" batch="$2" min="$3" total=0 seed=0
    echo ""
    echo "=== $name$TAG (batches of $batch until $ITERS calls) ==="
    build "$name"
    while [ "$total" -lt "$ITERS" ] || [ "$seed" -lt "$min" ]; do
        seed=$((seed + 1))
        local out
        out=$(./build/"$name" "$batch" "$seed") || {
            echo "FAILED in batch seed=$seed (exit $?) — reproduce with:"
            echo "  ./build/$name $batch $seed"
            echo "$out" | tail -5
            exit 1
        }
        local calls
        calls=$(echo "$out" | sed -n "s/^$name: \([0-9]*\) calls.*/\1/p" | tail -1)
        total=$((total + ${calls:-0}))
        printf "  seed %-3s %6s calls  %s\n" "$seed" "${calls:-?}" \
            "$(echo "$out" | grep -oE 'stream [0-9]+' | head -1)"
    done
    echo "  $name: $total calls total, 0 crashes"
}

# fuzz_decode: ~30 calls per iteration, so 300 iterations make ~9K calls and stay
# near 300 MB. fuzz_opus: 2000 calls (one to four packets each), about as much.
sweep fuzz_decode 300 4
sweep fuzz_opus 2000 2

# Cold starts: one call per process, so every first touch of a codec runs before
# anything has built its tables. Every codec initialises lazily (consumers call
# only alloc_init and shravan_init_constants), and a warm process hides a reader
# that runs ahead of its initialiser: 2.9.3's SILK LBRR crash was one.
cold() {
    local name="$1" iters="$2" n="$3" seed
    echo ""
    echo "=== $name$TAG cold starts ($n processes) ==="
    for seed in $(seq 1 "$n"); do
        ./build/"$name" "$iters" "$seed" > /dev/null 2>&1 || {
            echo "FAILED cold start seed=$seed — reproduce with:"
            echo "  ./build/$name $iters $seed"
            exit 1
        }
    done
    echo "  $name: $n cold starts, 0 crashes"
}
COLD_OPUS=$(( ITERS / 45 ));   [ "$COLD_OPUS" -gt 2000 ] && COLD_OPUS=2000
COLD_DECODE=$(( ITERS / 450 )); [ "$COLD_DECODE" -gt 200 ] && COLD_DECODE=200
[ "$COLD_DECODE" -lt 10 ] && COLD_DECODE=10
cold fuzz_opus 1 "$COLD_OPUS"
cold fuzz_decode 1 "$COLD_DECODE"

echo ""
echo "all fuzzers passed$TAG"
