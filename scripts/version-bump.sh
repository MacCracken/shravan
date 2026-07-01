#!/usr/bin/env bash
set -euo pipefail

NEW_VERSION="${1:?Usage: $0 <new-version>}"

# VERSION is the single source of truth. cyrius.cyml derives the package
# version via `version = "${file:VERSION}"`, so it needs no edit here.
echo "$NEW_VERSION" > VERSION

echo "Bumped VERSION to $NEW_VERSION"
echo ""
echo "Still manual (not machine-safe to sed):"
echo "  - CHANGELOG.md   : add a '## [$NEW_VERSION] - <date>' section"
echo "  - src/main.cyr   : update the 'shravan vX.Y.Z' startup banner (byte-length syscall arg)"
echo "  - src/bench.cyr  : update the 'shravan vX.Y.Z benchmarks' banner"
echo "  - cyrius.cyml    : bump [package].cyrius only when changing toolchain, then: cyrius lib sync"
echo ""
echo "Next steps:"
echo "  git add -A"
echo "  git commit -m 'release: $NEW_VERSION'"
echo "  git tag $NEW_VERSION"
echo "  git push origin main --tags"
