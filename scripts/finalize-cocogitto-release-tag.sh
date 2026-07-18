#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 <vMAJOR.MINOR.PATCH[-prerelease]>" >&2
  exit 2
}

[ "$#" -eq 1 ] || usage
TAG="$1"
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?$ ]] || usage

TAG_REF="refs/tags/${TAG}"

tag_type=$(git cat-file -t "$TAG_REF" 2>/dev/null) || {
  echo "error: Cocogitto did not create the expected local tag: $TAG" >&2
  exit 1
}
[ "$tag_type" = commit ] || {
  echo "error: expected Cocogitto's local lightweight tag, got object type: $tag_type" >&2
  exit 1
}

if git ls-remote --exit-code --tags origin "$TAG_REF" >/dev/null 2>&1; then
  echo "error: release tag already exists on origin: $TAG" >&2
  exit 1
else
  remote_status=$?
  [ "$remote_status" -eq 2 ] || {
    echo "error: failed to determine whether release tag exists on origin: $TAG" >&2
    exit "$remote_status"
  }
fi

COG_TAG_COMMIT=$(git rev-parse "$TAG_REF")
RELEASE_COMMIT=$(git rev-parse HEAD)
[ "$COG_TAG_COMMIT" != "$RELEASE_COMMIT" ] || {
  echo "error: release commit was not amended after Cocogitto created its tag" >&2
  exit 1
}

COG_TAG_TREE=$(git rev-parse "${COG_TAG_COMMIT}^{tree}")
RELEASE_TREE=$(git rev-parse "${RELEASE_COMMIT}^{tree}")
[ "$COG_TAG_TREE" = "$RELEASE_TREE" ] || {
  echo "error: amended release commit changed the Cocogitto release tree" >&2
  exit 1
}

COG_TAG_PARENT=$(git rev-parse "${COG_TAG_COMMIT}^")
RELEASE_PARENT=$(git rev-parse "${RELEASE_COMMIT}^")
[ "$COG_TAG_PARENT" = "$RELEASE_PARENT" ] || {
  echo "error: amended release commit changed the reviewed source parent" >&2
  exit 1
}

[ "$(git show -s --format=%s "$RELEASE_COMMIT")" = "chore: $TAG" ] || {
  echo "error: amended release commit has an unexpected subject" >&2
  exit 1
}
git verify-commit "$RELEASE_COMMIT" >/dev/null

# Cocogitto 6.5.0 creates a local lightweight tag before post-bump hooks. Delete
# only that exact, verified local ref before the first push, then create the
# signed annotated publication tag at the amended release commit. update-ref's
# old-value guard prevents replacing any other local tag object.
git update-ref -d "$TAG_REF" "$COG_TAG_COMMIT"
git tag -s -m "$TAG" "$TAG"

[ "$(git cat-file -t "$TAG_REF")" = tag ]
[ "$(git rev-parse "${TAG_REF}^{}")" = "$RELEASE_COMMIT" ]
git verify-tag "$TAG" >/dev/null

echo "finalized signed annotated $TAG at $RELEASE_COMMIT"
