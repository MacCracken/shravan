#!/usr/bin/env bash
# build-libopus.sh -- a default-configuration static libopus for the interop tools.
#
#   scripts/interop/build-libopus.sh <prefix> [tag | source-dir]
#
# Distributions may build libopus with deep PLC (DRED or OSCE), which skips the
# SILK PLC fade-in at 16 kHz: against such a build, wideband SILK and hybrid
# concealment cannot be checked (`build/opusref caps` says deep_plc 1). This
# fetches libopus (tag v1.6.1 by default, from github.com/xiph/opus) or takes a
# source tree, builds it with every CMake default (static, DRED and OSCE off) and
# installs it under <prefix>. Then:
#
#   OPUS_PREFIX=<prefix> ./scripts/interop.sh
#   OPUS_PREFIX=<prefix> ./scripts/gen-interop-fixtures.py
#
# Needs git (for a tag), cmake and a C compiler.
set -euo pipefail

PREFIX="${1:?usage: build-libopus.sh <prefix> [tag | source-dir]}"
SOURCE="${2:-v1.6.1}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [ -d "$SOURCE" ]; then
    SRC="$SOURCE"
else
    git clone --quiet --depth 1 --branch "$SOURCE" https://github.com/xiph/opus.git "$WORK/opus"
    SRC="$WORK/opus"
fi

step() {
    local log="$WORK/$1.log"
    shift
    "$@" > "$log" 2>&1 || { echo "build-libopus: $* failed:"; tail -20 "$log"; exit 1; }
}
step configure cmake -S "$SRC" -B "$WORK/build" -DCMAKE_BUILD_TYPE=Release \
    -DOPUS_DRED=OFF -DOPUS_OSCE=OFF -DCMAKE_INSTALL_LIBDIR=lib
step build cmake --build "$WORK/build" -j"$(nproc 2>/dev/null || echo 4)"
step install cmake --install "$WORK/build" --prefix "$PREFIX"
echo "libopus installed under $PREFIX ($PREFIX/lib/libopus.a)"
