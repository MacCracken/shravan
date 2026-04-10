#!/usr/bin/env bash
set -euo pipefail

NEW_VERSION="${1:?Usage: $0 <new-version>}"

echo "$NEW_VERSION" > VERSION
sed -i "s/^version = \".*\"/version = \"${NEW_VERSION}\"/" cyrius.toml

echo "Bumped to $NEW_VERSION"
echo ""
echo "Next steps:"
echo "  git add VERSION cyrius.toml"
echo "  git commit -m 'release: $NEW_VERSION'"
echo "  git tag $NEW_VERSION"
echo "  git push origin main --tags"
