#!/usr/bin/env bash
# Create and push a release tag (port of the original script/release.ts).
set -euo pipefail

version="${1:?Usage: script/release.sh vX.Y.Z}"

tag="v${version#v}"

echo "Creating release $tag"

git tag -a "$tag" -m "Release $tag"
git push origin "$tag"

echo "✔ Release pushed: $tag"
