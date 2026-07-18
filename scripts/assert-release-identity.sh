#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <pointbreak-binary> <version> <tag> <full-commit>" >&2
  exit 2
fi

binary="$1"
version="${2#v}"
tag="$3"
commit="$4"

[ -x "$binary" ] || { echo "release binary is not executable: $binary" >&2; exit 1; }
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?$ ]] \
  || { echo "invalid release version: $version" >&2; exit 1; }
[ "$tag" = "v${version}" ] || { echo "release tag does not match version: $tag" >&2; exit 1; }
[[ "$commit" =~ ^[0-9a-f]{40}$ ]] || { echo "release commit is not full lowercase hex" >&2; exit 1; }

document=$("$binary" version --format json)
printf '%s\n' "$document" | jq -e \
  --arg version "$version" \
  --arg tag "$tag" \
  --arg commit "$commit" \
  '.schema == "pointbreak.version"
   and .version == 1
   and .cliVersion == $version
   and .documents["pointbreak.version"] == 1
   and (.diagnostics | type == "array" and length == 0)
   and .build.source == "git"
   and .build.commit == $commit
   and .build.describe == $tag
   and .build.dirty == false' >/dev/null \
  || { echo "release binary does not report the required clean exact-tag identity" >&2; exit 1; }

if command -v sha256sum >/dev/null 2>&1; then
  binary_sha256=$(sha256sum "$binary" | awk '{print $1}')
else
  binary_sha256=$(shasum -a 256 "$binary" | awk '{print $1}')
fi

jq -n \
  --arg binary "$binary" \
  --arg binarySha256 "$binary_sha256" \
  --arg version "$version" \
  --arg tag "$tag" \
  --arg commit "$commit" \
  --argjson versionDocument "$document" \
  '{status:"passed", binary:$binary, binarySha256:$binarySha256, version:$version, tag:$tag, commit:$commit, versionDocument:$versionDocument}'
