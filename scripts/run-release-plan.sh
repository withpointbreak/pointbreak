#!/usr/bin/env bash
#
# Trigger the exact-parent Release Plan workflow, wait for it to complete,
# download the report, and print it to stdout.
#
# Usage:
#   ./scripts/run-release-plan.sh <plan|release> <version> --expected-source <full-sha>
#   ./scripts/run-release-plan.sh plan 0.8.0 --expected-source <full-sha> -- bat --paging=always
#
# Set RELEASE_PLAN_DIR to retain release-plan.md outside the temporary directory.
# Viewer arguments are accepted only after the `--` separator.
set -euo pipefail

usage() {
  echo "usage: $0 <plan|release> <version> --expected-source <full-sha> [-- <viewer>...]" >&2
  exit 2
}

[ "$#" -ge 4 ] || usage

MODE="$1"
VERSION="${2#v}"
shift 2

case "$MODE" in
  plan | release) ;;
  *) usage ;;
esac
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?$ ]] || usage

[ "${1:-}" = "--expected-source" ] || usage
EXPECTED_SOURCE_COMMIT="${2:-}"
shift 2
[[ "$EXPECTED_SOURCE_COMMIT" =~ ^[0-9a-f]{40}$ ]] || {
  echo "error: --expected-source must be a full lowercase 40-hex commit" >&2
  exit 2
}

VIEWER=()
if [ "$#" -gt 0 ]; then
  [ "$1" = "--" ] || usage
  shift
  VIEWER=("$@")
fi

if [ "${#VIEWER[@]}" -eq 0 ]; then
  if command -v bat >/dev/null 2>&1; then
    VIEWER=(bat --paging=never)
  else
    VIEWER=(cat)
  fi
fi

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(git -C "$SCRIPT_DIR/.." rev-parse --show-toplevel)

resolve_repository() {
  local remote_url
  remote_url=$(git -C "$REPO_ROOT" remote get-url origin)
  remote_url=${remote_url#git@github.com:}
  remote_url=${remote_url#https://github.com/}
  remote_url=${remote_url%.git}

  if [[ ! "$remote_url" =~ ^[^/]+/[^/]+$ ]]; then
    echo "error: origin is not a GitHub owner/repository URL" >&2
    return 1
  fi

  printf '%s\n' "$remote_url"
}

REPO=${RELEASE_PLAN_REPO:-$(resolve_repository)}
if [[ ! "$REPO" =~ ^[^/]+/[^/]+$ ]]; then
  echo "error: RELEASE_PLAN_REPO must be an owner/repository name" >&2
  exit 1
fi

[ -z "$(git -C "$REPO_ROOT" status --porcelain --untracked-files=all)" ] || {
  echo "error: release planning requires a clean source worktree" >&2
  exit 1
}
git -C "$REPO_ROOT" fetch --quiet origin main
current_main=$(git -C "$REPO_ROOT" rev-parse origin/main)
current_head=$(git -C "$REPO_ROOT" rev-parse HEAD)
if [ "$current_main" != "$EXPECTED_SOURCE_COMMIT" ] || [ "$current_head" != "$EXPECTED_SOURCE_COMMIT" ]; then
  echo "error: expected source $EXPECTED_SOURCE_COMMIT does not match clean HEAD/origin/main ($current_head/$current_main)" >&2
  exit 1
fi

just --justfile "$REPO_ROOT/Justfile" --working-directory "$REPO_ROOT" package-archive-selftest

if [ -n "${RELEASE_PLAN_DIR:-}" ]; then
  OUTDIR="$RELEASE_PLAN_DIR"
  mkdir -p "$OUTDIR"
else
  OUTDIR=$(mktemp -d)
  trap 'rm -rf "$OUTDIR"' EXIT
fi

WORKFLOW="release-plan.yml"
flags=(
  -f "mode=${MODE}"
  -f "version=${VERSION}"
  -f "expected_source_commit=${EXPECTED_SOURCE_COMMIT}"
)

echo "Dispatching Release Plan workflow (mode=${MODE}, version=${VERSION}, expected_source=${EXPECTED_SOURCE_COMMIT})..."
gh workflow run "$WORKFLOW" --repo "$REPO" "${flags[@]}"

sleep 3
RUN_ID=$(gh run list --repo "$REPO" --workflow "$WORKFLOW" --limit 1 \
  --json databaseId --jq '.[0].databaseId')
[ -n "$RUN_ID" ] || {
  echo "error: could not find workflow run" >&2
  exit 1
}

echo "Waiting for run ${RUN_ID}..."
gh run watch "$RUN_ID" --repo "$REPO" --exit-status || {
  echo "error: workflow run failed" >&2
  gh run view "$RUN_ID" --repo "$REPO"
  exit 1
}

echo ""
gh run download "$RUN_ID" --repo "$REPO" --name release-plan --dir "$OUTDIR"

if [ -f "$OUTDIR/release-plan.md" ]; then
  echo "To re-download: gh run download ${RUN_ID} -n release-plan"
  echo ""
  "${VIEWER[@]}" "$OUTDIR/release-plan.md"
else
  echo "error: release-plan.md not found in artifacts" >&2
  exit 1
fi
