#!/usr/bin/env bash
set -euo pipefail

# test-all.sh — build + run the shravan test suite.
#
# src/main.cyr -> build/shravan runs every assertion: core codecs, interop
# fixtures, Opus decode, the CELT encoder and the SILK golden vectors. Until
# 2.9.3 the suite was split across three binaries for a ~3.15 MB code buffer
# that has been a growable 64 MiB region since Cyrius 6.4.49.
#
# Exits non-zero if the build fails or any assertion fails.

cd "$(dirname "$0")/.."

CYRB="${CYRB:-}"
if [ -z "$CYRB" ]; then
    if command -v cyrius >/dev/null 2>&1; then CYRB=cyrius
    elif [ -x "$HOME/.cyrius/bin/cyrius" ]; then CYRB="$HOME/.cyrius/bin/cyrius"
    elif [ -x "./build/cyrius" ]; then CYRB="./build/cyrius"
    else echo "ERROR: cyrius not found"; exit 1; fi
fi

mkdir -p build

echo "=== building src/main.cyr -> build/shravan ==="
FAIL=0
if ! "$CYRB" build src/main.cyr build/shravan; then
    echo "BUILD FAILED"
    FAIL=1
else
    echo "=== running build/shravan ==="
    if ! ./build/shravan; then
        echo "TESTS FAILED"
        FAIL=1
    fi
fi

if [ "$FAIL" -ne 0 ]; then
    echo "test-all: FAILURES present"
    exit 1
fi
echo "test-all: all tests passed"
