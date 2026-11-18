#!/usr/bin/env bash
# interop.sh -- build the interop tools and sweep shravan against the references.
#
#   build/interop-cli  shravan's decoders and encoders from the command line
#   build/opusref      libopus reference vectors (needs the libopus headers)
#   build/opusenc-ref  libopus's opus_encode with OPUS_APPLICATION_RESTRICTED_SILK
#                      (scripts/silkref/opusenc.c), the Opus encoder layer's reference
#
# Then scripts/interop.py runs the sweep; arguments pass through (--only, -v).
#
# OPUS_PREFIX=<dir> links opusref against <dir>/lib/libopus.a instead of the system
# library. A deep-PLC build (DRED/OSCE; `build/opusref caps` says so) conceals
# wideband SILK differently by design, so the sweep checks wideband loss only
# against a default build; scripts/interop/build-libopus.sh makes one.
set -euo pipefail
cd "$(dirname "$0")/.."

CYRB="${CYRB:-cyrius}"
mkdir -p build
"$CYRB" build scripts/interop/cli.cyr build/interop-cli
OPUS_LINK=(-lopus -lm)
OPUSENC_INC=(-I/usr/include/opus)
if [ -n "${OPUS_PREFIX:-}" ]; then
    OPUS_LINK=(-I"$OPUS_PREFIX/include" "$OPUS_PREFIX/lib/libopus.a" -lm)
    OPUSENC_INC=(-I"$OPUS_PREFIX/include/opus")
fi
${CC:-cc} -O2 -Wall -o build/opusref scripts/interop/opusref.c "${OPUS_LINK[@]}"
${CC:-cc} -O2 -Wall "${OPUSENC_INC[@]}" -o build/opusenc-ref scripts/silkref/opusenc.c "${OPUS_LINK[@]}"
./build/opusref caps
exec python3 scripts/interop.py "$@"
