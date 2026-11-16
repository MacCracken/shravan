#!/usr/bin/env bash
set -euo pipefail

# check-collisions.sh: shravan's bundle against the bundles consumers put beside it.
#
# Bundles share one flat namespace. When two define the same fn with the same
# arity, cycc only warns and the last definition wins for every caller in both;
# a conflicting var or enum value is the same; a different arity, or a name
# another module declares `const`, is a hard error. Either way the damage shows
# up in a consumer, not here. This compiles dist/shravan.cyr after each of:
#
#   deps    shravan's own [deps]: its declared stdlib, sankoch and bayan-json
#   stdlib  every module of the pinned toolchain's stdlib snapshot
#           ($CYRIUS_HOME/versions/<pin>/lib), mabda, sakshi, yukti and the rest
#   abaco   at its newest release tag, fetched from GitHub (the 2.9.4 issue:
#           abaco 2.8.0's `_fft_pow2` replaced shravan's, and shravan's replaced
#           abaco's)
#
# and fails on any duplicate or redeclaration located in shravan's bundle, apart
# from ALLOWED below. Same-name globals with the same value merge silently in
# cycc and are harmless, so they are not reported.
#
# Usage:
#   scripts/check-collisions.sh                  # the three targets above
#   scripts/check-collisions.sh FILE.cyr ...     # these bundles instead of the fetched ones
#   scripts/check-collisions.sh REPO[@TAG] ...   # MacCracken/REPO's dist/REPO.cyr (newest tag by default)
#   SHRAVAN_BUNDLE=old.cyr scripts/check-collisions.sh   # check another shravan bundle
#
# Exits non-zero on any finding, or when a target could not be checked.

cd "$(dirname "$0")/.."

# Collisions that are known and accepted, as SYMBOL=FIRST-DEFINED-IN.
# - detect_format: sankoch's compression detector. shravan's wrapper is public
#   API until 3.0 (roadmap backlog); shravan itself calls shravan_detect_format,
#   and neither sankoch nor any consumer calls detect_format.
ALLOWED=("detect_format=lib/sankoch.cyr")

# Bundles fetched when no arguments are given.
DEFAULT_REPOS=(abaco)
GITHUB="https://github.com/MacCracken"
RAW="https://raw.githubusercontent.com/MacCracken"

CYRB="${CYRB:-}"
if [ -z "$CYRB" ]; then
    if command -v cyrius >/dev/null 2>&1; then CYRB=cyrius
    elif [ -x "$HOME/.cyrius/bin/cyrius" ]; then CYRB="$HOME/.cyrius/bin/cyrius"
    else echo "ERROR: cyrius not found"; exit 1; fi
fi

PIN="$(grep '^cyrius = ' cyrius.cyml | head -1 | sed 's/cyrius = "\(.*\)"/\1/')"
STDLIB="${CYRIUS_HOME:-$HOME/.cyrius}/versions/$PIN/lib"
[ -d "$STDLIB" ] || { echo "ERROR: no stdlib snapshot for the $PIN pin at $STDLIB"; exit 1; }

BUNDLE="${SHRAVAN_BUNDLE:-dist/shravan.cyr}"
[ -f "$BUNDLE" ] || { echo "ERROR: $BUNDLE not found (cyrius distlib)"; exit 1; }
for f in lib/sankoch.cyr lib/bayan-json.cyr; do
    [ -f "$f" ] || { echo "ERROR: $f not vendored (cyrius deps)"; exit 1; }
done

# shravan's declared stdlib, which a consumer must supply ahead of the bundle.
read -r -a DEPS_STDLIB <<< "$(sed -n '/^\[deps\]/,/^\[/p' cyrius.cyml | grep '^stdlib' \
    | sed 's/.*\[\(.*\)\].*/\1/; s/[",]/ /g')"

W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT
cp -r "$STDLIB" "$W/lib"
# shravan's pinned dependency copies, over the snapshot's.
cp lib/sankoch.cyr lib/bayan-json.cyr "$W/lib/"
cp "$BUNDLE" "$W/shravan.cyr"
mkdir -p "$W/other"

FAIL=0

# check NAME INCLUDE...: compile INCLUDE..., shravan's stdlib and sankoch, then
# shravan's bundle last, and report what cycc says about shravan.cyr.
check() {
    local name="$1"; shift
    {
        for inc in "$@"; do echo "include \"$inc\""; done
        for m in "${DEPS_STDLIB[@]}"; do echo "include \"lib/$m.cyr\""; done
        echo 'include "lib/sankoch.cyr"'
        echo 'include "shravan.cyr"'
        echo 'syscall(60, 0);'
    } > "$W/unit.cyr"

    local out rc=0
    out="$(cd "$W" && "$CYRB" build --no-deps unit.cyr unit 2>&1)" || rc=$?
    # Diagnostics, one per line (the first shares its line with the compile banner).
    local diags
    diags="$(echo "$out" | grep -oE '(warning|error):.*' | grep -v 'undefined function' || true)"

    local found=() other=()
    while IFS= read -r d; do
        [ -n "$d" ] || continue
        if [[ "$d" == *"shravan.cyr:"* || "$d" == *"first defined in shravan.cyr"* ]]; then
            local allowed=0
            for a in "${ALLOWED[@]}"; do
                if [[ "$d" == *"duplicate fn '${a%%=*}'"*"first defined in ${a#*=})"* ]]; then allowed=1; fi
            done
            [ "$allowed" = 1 ] || found+=("$d")
        elif [[ "$d" == error:* ]]; then
            other+=("$d")
        fi
    done <<< "$diags"

    if [ "${#found[@]}" -gt 0 ]; then
        echo "FAIL  $name: ${#found[@]} collision(s) with shravan's bundle"
        printf '        %s\n' "${found[@]}"
        FAIL=1
    elif [ "$rc" -ne 0 ]; then
        echo "FAIL  $name: could not be checked (the unit does not compile)"
        printf '        %s\n' "${other[@]:-$(echo "$out" | tail -3)}"
        FAIL=1
    else
        echo "ok    $name"
    fi
}

# fetch REPO[@TAG]: dist/REPO.cyr at TAG, or at the newest release tag (x.y.z
# with a major of at most three digits: a calendar tag such as 2026.3.18 is not
# a release). Prints the local path; the tag goes to $W/other/REPO.tag.
fetch() {
    local repo="${1%%@*}" tag=""
    [[ "$1" == *@* ]] && tag="${1#*@}"
    if [ -z "$tag" ]; then
        tag="$(git ls-remote --tags --refs "$GITHUB/$repo.git" | awk -F/ '{print $3}' \
            | grep -E '^[0-9]{1,3}\.[0-9]+\.[0-9]+$' | sort -V | tail -1)"
        [ -n "$tag" ] || { echo "ERROR: no release tag for $repo" >&2; return 1; }
    fi
    local dst="$W/other/$repo-$tag.cyr"
    curl -sSfL "$RAW/$repo/$tag/dist/$repo.cyr" -o "$dst" \
        || { echo "ERROR: could not fetch $repo $tag dist/$repo.cyr" >&2; return 1; }
    echo "$tag" > "$W/other/$repo.tag"
    echo "$dst"
}

echo "shravan bundle: $BUNDLE ($(sed -n 's/^# Version: //p' "$BUNDLE" | head -1)), stdlib snapshot $PIN"

# deps: shravan's own manifest, as a consumer resolves it.
check "deps (stdlib + sankoch + bayan-json)" "lib/bayan-json.cyr"

# stdlib: every top-level module (the platform variants are included by their
# own front modules). bayan-json is left out: it is a profile of bayan.
mods=()
for f in "$STDLIB"/*.cyr; do
    m="$(basename "$f" .cyr)"
    [[ "$m" =~ _(agnos|macos|windows|win|cx|aarch64_linux|x86_64_linux|x86_64_agnos|linux_common)$ ]] && continue
    mods+=("lib/$m.cyr")
done
check "stdlib $PIN (${#mods[@]} modules)" "${mods[@]}"

targets=("$@")
[ "${#targets[@]}" -gt 0 ] || targets=("${DEFAULT_REPOS[@]}")
for t in "${targets[@]}"; do
    if [ -f "$t" ]; then
        cp "$t" "$W/other/$(basename "$t")"
        check "$t" "other/$(basename "$t")"
    else
        path="$(fetch "$t")" || { FAIL=1; continue; }
        repo="${t%%@*}"
        check "$repo $(cat "$W/other/$repo.tag")" "other/$(basename "$path")"
    fi
done

if [ "$FAIL" -ne 0 ]; then
    echo "check-collisions: FAILED"
    exit 1
fi
echo "check-collisions: no collisions"
