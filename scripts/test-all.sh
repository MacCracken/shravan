#!/usr/bin/env bash
set -euo pipefail

# test-all.sh — build + run every shravan test-harness binary.
#
# The suite is split across three harness entry points so no single build
# overflows the Cyrius code buffer (~3.15 MB):
#   src/main.cyr          -> build/shravan          core codecs + Opus decode + API
#   src/main_encoder.cyr  -> build/shravan-encoder  Opus CELT encoder (RFC-6716)
#   src/main_silk.cyr     -> build/shravan-silk      SILK decode golden vectors
#
# Each binary shares the same library + codec modules (via include); only the
# test subset differs. Exits non-zero if any binary fails to build or any
# assertion fails.

cd "$(dirname "$0")/.."

CYRB="${CYRB:-}"
if [ -z "$CYRB" ]; then
    if command -v cyrius >/dev/null 2>&1; then CYRB=cyrius
    elif [ -x "$HOME/.cyrius/bin/cyrius" ]; then CYRB="$HOME/.cyrius/bin/cyrius"
    elif [ -x "./build/cyrius" ]; then CYRB="./build/cyrius"
    else echo "ERROR: cyrius not found"; exit 1; fi
fi

mkdir -p build

# name  src                   out
SUITES=(
    "main             src/main.cyr           build/shravan"
    "encoder          src/main_encoder.cyr   build/shravan-encoder"
    "silk             src/main_silk.cyr      build/shravan-silk"
)

FAIL=0
TOTAL=0
for row in "${SUITES[@]}"; do
    read -r name src out <<<"$row"
    echo "=== building $name ($src -> $out) ==="
    if ! "$CYRB" build "$src" "$out"; then
        echo "BUILD FAILED: $name"
        FAIL=1
        continue
    fi
    echo "=== running $name ==="
    if ! "./$out"; then
        echo "TESTS FAILED: $name"
        FAIL=1
    fi
    echo
done

if [ "$FAIL" -ne 0 ]; then
    echo "test-all: FAILURES present"
    exit 1
fi
echo "test-all: all suites passed"
