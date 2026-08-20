#!/usr/bin/env bash
# Verify the Change-first Inspector in a real browser over a disposable public L2 fixture.
# Prefer `just change-inspector-browser-verify <empty-root>`; every generated file stays below root.

set -euo pipefail

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
usage: change-inspector-browser-verify.sh --root <empty-directory>

Runs the public L2 Change matrix against an exact injected Pointbreak binary.
The root must be empty and outside this worktree. Logs, screenshots, fixture
repositories, the disposable POINTBREAK_HOME, and completion-last manifest all
remain under that root.
EOF
}

for command in git jq node rg shasum find sort wc tr mv curl cp chmod; do
  command -v "$command" >/dev/null 2>&1 || die "$command is required"
done

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
browser_program_template="$script_dir/change-inspector-browser-verify.mjs"
browser_diagnostics="$script_dir/change-inspector-browser-diagnostics.mjs"
browser_manifest_publisher="$script_dir/change-inspector-browser-manifest.mjs"
matrix_materializer="$script_dir/materialize-inspector-decision-matrix.sh"
pointbreak_binary="${POINTBREAK_BINARY:-}"
root=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --root) root="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

[ -n "$root" ] || die "--root <empty-directory> is required"
[ -n "$pointbreak_binary" ] || die "POINTBREAK_BINARY must name the exact worktree binary"
[ -x "$pointbreak_binary" ] || die "POINTBREAK_BINARY is not executable: $pointbreak_binary"
case "$pointbreak_binary" in
  /* | [A-Za-z]:/* | [A-Za-z]:\\* | \\\\*) ;;
  *) die "POINTBREAK_BINARY must be an absolute executable path" ;;
esac
case "${POINTBREAK_DERIVED_ACCESS:-}" in
  "" | sqlite-wal-bodyless-v1) ;;
  *) die "POINTBREAK_DERIVED_ACCESS must be unset or sqlite-wal-bodyless-v1" ;;
esac
[ -f "$browser_program_template" ] || die "browser program is missing: $browser_program_template"
[ -f "$browser_diagnostics" ] || die "browser diagnostics are missing: $browser_diagnostics"
[ -f "$browser_manifest_publisher" ] || die "browser manifest publisher is missing: $browser_manifest_publisher"
[ -x "$matrix_materializer" ] || die "matrix materializer is not executable: $matrix_materializer"

[ -z "$(git -C "$repo_root" status --porcelain --untracked-files=all)" ] \
  || die "source worktree must be clean so the manifest names an exact source commit"

if [ -e "$root" ]; then
  [ -d "$root" ] || die "root exists and is not a directory: $root"
  [ -z "$(find "$root" -mindepth 1 -maxdepth 1 -print -quit)" ] \
    || die "root is not empty: $root"
else
  mkdir -p "$root"
fi
root="$(cd "$root" && pwd -P)"
case "$root" in
  "$repo_root"|"$repo_root"/*) die "root must be outside the Pointbreak source worktree" ;;
esac

fixture_root="$root/public-l2-change-matrix"
fixture_repo="$fixture_root/repository"
pointbreak_home="$fixture_root/pointbreak-home"
artifact_dir="$root/browser-artifacts"
log_dir="$root/logs"
mkdir -p "$fixture_root" "$pointbreak_home" "$artifact_dir" "$log_dir"

# Own every asynchronous child from the moment it is spawned. The EXIT trap is
# installed before asynchronous work begins, while browser cleanup remains
# disabled until its session command has been resolved.
background_pids=()
pwcli=()
session=""
browser_cleanup_enabled=false

run_pw() {
  (cd "$artifact_dir" && "${pwcli[@]}" -s="$session" "$@")
}

register_background_process() {
  background_pids+=("$1")
}

forget_background_process() {
  local completed_pid="$1"
  local retained_pids=()
  local pid
  for pid in "${background_pids[@]}"; do
    [ "$pid" = "$completed_pid" ] || retained_pids+=("$pid")
  done
  background_pids=("${retained_pids[@]}")
}

stop_background_process() {
  local pid="$1"
  [ -n "$pid" ] || return 0
  if kill -0 "$pid" >/dev/null 2>&1; then
    kill "$pid" >/dev/null 2>&1 || true
  fi
  wait "$pid" >/dev/null 2>&1 || true
}

cleanup() {
  local mode="${1:-best-effort}"
  local browser_close_status=0
  local pid
  if [ "$browser_cleanup_enabled" = true ]; then
    if run_pw close >"$log_dir/browser-close.log" 2>&1; then
      browser_close_status=0
    else
      browser_close_status=$?
    fi
    browser_cleanup_enabled=false
  fi
  for pid in "${background_pids[@]}"; do
    stop_background_process "$pid"
  done
  background_pids=()
  if [ "$mode" = strict ] && [ "$browser_close_status" -ne 0 ]; then
    return "$browser_close_status"
  fi
  return 0
}
trap cleanup EXIT

source_commit="$(git -C "$repo_root" rev-parse HEAD)"
requested_binary="$pointbreak_binary"
binary_sha256="$(shasum -a 256 "$pointbreak_binary" | awk '{print $1}')"
snapshot_root="$root/harness"
snapshot_scripts="$snapshot_root/scripts"
snapshot_ready_store="$snapshot_root/tests/support/assets/change-ready-store"
snapshot_timeline_compat_store="$snapshot_root/tests/support/assets/inspector-timeline-compat-v1"
snapshot_legacy_note_store="$snapshot_root/tests/fixtures/legacy_stores/review_note_imported/store"
activation_fixture="5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json"
completion_fixture="f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json"
mkdir -p "$snapshot_scripts" "$snapshot_ready_store" \
  "$snapshot_timeline_compat_store" "$snapshot_legacy_note_store"
git -C "$repo_root" show "$source_commit:scripts/change-inspector-browser-verify.sh" \
  >"$snapshot_scripts/change-inspector-browser-verify.sh"
git -C "$repo_root" show "$source_commit:scripts/change-inspector-browser-verify.mjs" \
  >"$snapshot_scripts/change-inspector-browser-verify.mjs"
git -C "$repo_root" show "$source_commit:scripts/change-inspector-browser-diagnostics.mjs" \
  >"$snapshot_scripts/change-inspector-browser-diagnostics.mjs"
git -C "$repo_root" show "$source_commit:scripts/change-inspector-browser-manifest.mjs" \
  >"$snapshot_scripts/change-inspector-browser-manifest.mjs"
git -C "$repo_root" show "$source_commit:scripts/materialize-inspector-decision-matrix.sh" \
  >"$snapshot_scripts/materialize-inspector-decision-matrix.sh"
git -C "$repo_root" show "$source_commit:tests/support/assets/change-ready-store/$activation_fixture" \
  >"$snapshot_ready_store/$activation_fixture"
git -C "$repo_root" show "$source_commit:tests/support/assets/change-ready-store/$completion_fixture" \
  >"$snapshot_ready_store/$completion_fixture"

snapshot_git_tree() {
  local source_prefix="$1"
  local destination_root="$2"
  local source_path relative_path destination_path
  while IFS= read -r source_path; do
    relative_path="${source_path#"$source_prefix"/}"
    destination_path="$destination_root/$relative_path"
    mkdir -p "$(dirname "$destination_path")"
    git -C "$repo_root" show "$source_commit:$source_path" >"$destination_path"
  done < <(git -C "$repo_root" ls-tree -r --name-only "$source_commit" -- "$source_prefix")
}

snapshot_git_tree \
  "tests/support/assets/inspector-timeline-compat-v1" \
  "$snapshot_timeline_compat_store"
snapshot_git_tree \
  "tests/fixtures/legacy_stores/review_note_imported/store" \
  "$snapshot_legacy_note_store"
[ "$(find "$snapshot_timeline_compat_store" -maxdepth 1 -type f -name '*.json' | wc -l | tr -d '[:space:]')" -eq 9 ] \
  || die "source-bound Timeline compatibility fixture event count drifted"
chmod 0444 \
  "$snapshot_scripts/change-inspector-browser-verify.mjs" \
  "$snapshot_scripts/change-inspector-browser-diagnostics.mjs" \
  "$snapshot_scripts/change-inspector-browser-manifest.mjs" \
  "$snapshot_ready_store/$activation_fixture" \
  "$snapshot_ready_store/$completion_fixture"
find "$snapshot_timeline_compat_store" "$snapshot_legacy_note_store" -type f -exec chmod 0444 {} +
chmod 0555 \
  "$snapshot_scripts/change-inspector-browser-verify.sh" \
  "$snapshot_scripts/materialize-inspector-decision-matrix.sh"

binary_snapshot="$snapshot_root/pointbreak"
cp "$pointbreak_binary" "$binary_snapshot"
chmod 0555 "$binary_snapshot"
[ "$(shasum -a 256 "$binary_snapshot" | awk '{print $1}')" = "$binary_sha256" ] \
  || die "binary snapshot did not match the injected executable"

shell_sha256="$(shasum -a 256 "$snapshot_scripts/change-inspector-browser-verify.sh" | awk '{print $1}')"
template_sha256="$(shasum -a 256 "$snapshot_scripts/change-inspector-browser-verify.mjs" | awk '{print $1}')"
diagnostics_sha256="$(shasum -a 256 "$snapshot_scripts/change-inspector-browser-diagnostics.mjs" | awk '{print $1}')"
publisher_sha256="$(shasum -a 256 "$snapshot_scripts/change-inspector-browser-manifest.mjs" | awk '{print $1}')"
materializer_sha256="$(shasum -a 256 "$snapshot_scripts/materialize-inspector-decision-matrix.sh" | awk '{print $1}')"
activation_fixture_sha256="$(shasum -a 256 "$snapshot_ready_store/$activation_fixture" | awk '{print $1}')"
completion_fixture_sha256="$(shasum -a 256 "$snapshot_ready_store/$completion_fixture" | awk '{print $1}')"
compatibility_fixture_inventory="$(
  find "$snapshot_timeline_compat_store" "$snapshot_legacy_note_store" -type f -print \
    | LC_ALL=C sort \
    | while IFS= read -r fixture_file; do
        relative_path="${fixture_file#"$snapshot_root"/}"
        fixture_sha256="$(shasum -a 256 "$fixture_file" | awk '{print $1}')"
        jq -cn --arg path "$relative_path" --arg sha256 "$fixture_sha256" \
          '{path: $path, sha256: $sha256}'
      done \
    | jq -cs '.'
)"
[ "$(shasum -a 256 "$script_dir/change-inspector-browser-verify.sh" | awk '{print $1}')" = "$shell_sha256" ] \
  || die "running browser verifier did not match the exact source commit"
jq -n \
  --arg sourceCommit "$source_commit" \
  --arg requestedBinary "$requested_binary" \
  --arg executedBinary "$binary_snapshot" \
  --arg binarySha256 "$binary_sha256" \
  --arg shellSha256 "$shell_sha256" \
  --arg templateSha256 "$template_sha256" \
  --arg diagnosticsSha256 "$diagnostics_sha256" \
  --arg publisherSha256 "$publisher_sha256" \
  --arg materializerSha256 "$materializer_sha256" \
  --arg activationFixture "$activation_fixture" \
  --arg activationFixtureSha256 "$activation_fixture_sha256" \
  --arg completionFixture "$completion_fixture" \
  --arg completionFixtureSha256 "$completion_fixture_sha256" \
  --argjson compatibilityFixtureInventory "$compatibility_fixture_inventory" \
  '{schema: "pointbreak.change-inspector-browser-harness", version: 1,
    sourceCommit: $sourceCommit,
    binary: {requestedPath: $requestedBinary, executedPath: $executedBinary, sha256: $binarySha256},
    files: ([
      {path: "scripts/change-inspector-browser-verify.sh", sha256: $shellSha256},
      {path: "scripts/change-inspector-browser-verify.mjs", sha256: $templateSha256},
      {path: "scripts/change-inspector-browser-diagnostics.mjs", sha256: $diagnosticsSha256},
      {path: "scripts/change-inspector-browser-manifest.mjs", sha256: $publisherSha256},
      {path: "scripts/materialize-inspector-decision-matrix.sh", sha256: $materializerSha256},
      {path: ("tests/support/assets/change-ready-store/" + $activationFixture), sha256: $activationFixtureSha256},
      {path: ("tests/support/assets/change-ready-store/" + $completionFixture), sha256: $completionFixtureSha256}
    ] + $compatibilityFixtureInventory)}' >"$log_dir/harness-digests.json"
harness_record_sha256="$(shasum -a 256 "$log_dir/harness-digests.json" | awk '{print $1}')"

pointbreak_binary="$binary_snapshot"
browser_program_template="$snapshot_scripts/change-inspector-browser-verify.mjs"
browser_diagnostics="$snapshot_scripts/change-inspector-browser-diagnostics.mjs"
browser_manifest_publisher="$snapshot_scripts/change-inspector-browser-manifest.mjs"
matrix_materializer="$snapshot_scripts/materialize-inspector-decision-matrix.sh"
"$pointbreak_binary" version --format json >"$log_dir/pointbreak-version.json"
jq -e --arg source_commit "$source_commit" '
  .schema == "pointbreak.version" and .version == 1 and
  .build.source == "git" and .build.commit == $source_commit and .build.dirty == false
' "$log_dir/pointbreak-version.json" >/dev/null \
  || die "injected binary does not attest the clean exact source commit"

# The retained matrix supplies multiple topology and unavailable-resource cases.
# Add enough distinct Change captures to exercise the 363+ list contract without
# borrowing any owner records. Keep the scale input tracked and overwrite it
# for every capture so each captured diff stays constant-sized.
POINTBREAK_HOME="$pointbreak_home" POINTBREAK_BINARY="$pointbreak_binary" \
  POINTBREAK_TIMELINE_COMPAT_FIXTURE_DIR="$snapshot_timeline_compat_store" \
  POINTBREAK_LEGACY_NOTE_FIXTURE_DIR="$snapshot_legacy_note_store" \
  "$matrix_materializer" "$fixture_repo" \
  >"$log_dir/base-matrix.json" 2>"$log_dir/base-matrix.log"
printf 'pub const BROWSER_SCALE: u32 = 0;\n' >"$fixture_repo/src/browser-scale.rs"
git -C "$fixture_repo" add src/browser-scale.rs
git -C "$fixture_repo" commit --quiet -m "browser scale source"

for ordinal in $(seq 1 351); do
  printf 'pub const BROWSER_SCALE_%s: u32 = %s;\n' "$ordinal" "$ordinal" \
    >"$fixture_repo/src/browser-scale.rs"
  POINTBREAK_HOME="$pointbreak_home" \
    POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
    "$pointbreak_binary" capture --repo "$fixture_repo" \
      --summary "Browser scale Change $ordinal" --format json \
      >>"$log_dir/scale-captures.jsonl" 2>>"$log_dir/scale-captures.log"
done

# An initial Change capture mints its declaration and membership assertion from
# one operation timestamp. Bind the first supported receipt's exact event pair
# so the browser can prove the Timeline's event-id tie break without depending
# on scheduler or wall-clock coincidence between separate writer processes.
equal_timestamp_pair="$(jq -s -ce '
  .[0] as $capture
  | ($capture | .changeEvents | map(select(.outcome == "created"))) as $events
  | [$events[] | select(
      .eventType == "change_declared" or
      .eventType == "change_membership_asserted"
    )] as $pair
  | if (($pair | length) == 2 and
        ($pair | map(.eventType) | unique | length) == 2 and
        ($capture.changeId | startswith("change:sha256:")))
    then {changeId: $capture.changeId, tieBreak: "event_id_asc",
      eventIds: ($pair | map(.eventId) | sort)}
    else error("first scale capture did not emit one created declaration/membership pair") end
' "$log_dir/scale-captures.jsonl")" \
  || die "supported Change capture did not provide a deterministic equal-occurredAt pair"

# One explicit exact Revision is the removed-resource case. The removal claim
# models intentional unavailability without erasing bytes, preserving a
# replayable fixture history distinct from the recoverably missing case below.
printf 'pub const BROWSER_EXACT: &str = "exact";\n' >"$fixture_repo/src/browser-scale.rs"
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" capture --repo "$fixture_repo" \
    --summary "Browser exact Change" --format json \
    >"$log_dir/exact-capture.json" 2>"$log_dir/exact-capture.log"
exact_change="$(jq -er '.changeId' "$log_dir/exact-capture.json")"
exact_revision="$(jq -er '.revision.revisionId' "$log_dir/exact-capture.json")"
exact_artifact="$(jq -er '.revision.objectArtifactContentHash' "$log_dir/exact-capture.json")"
jq -e '
  .schema == "pointbreak.change-capture-receipt.v1" and .version == 1 and
  (.revision.revisionId | startswith("rev:sha256:")) and
  (.revision.objectArtifactContentHash | startswith("sha256:"))
' "$log_dir/exact-capture.json" >/dev/null \
  || die "direct Change capture did not emit the expected exact Revision schema"
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" store remove --repo "$fixture_repo" --revision "$exact_revision" --format json \
    >"$log_dir/exact-resource-removed.json" 2>"$log_dir/exact-resource-removed.log"

# Turn the materializer's exact missing-object Change into an honest missing-
# resource case by moving its bound artifact to a retained recovery directory.
# Refuse unexpected hashes, symlinks, and store locations before moving bytes;
# every path remains within this caller-owned disposable evidence root.
missing_change="$(jq -er '.missing_change' "$log_dir/base-matrix.json")"
missing_revision="$(jq -er '.missing_revision' "$log_dir/base-matrix.json")"
missing_artifact="$(jq -er '.missing_artifact' "$log_dir/base-matrix.json")"
[[ "$missing_artifact" =~ ^sha256:[0-9a-f]{64}$ ]] \
  || die "missing-resource fixture emitted an invalid artifact hash: $missing_artifact"
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" store paths \
  --repo "$fixture_repo" --format json >"$log_dir/store-paths.json"
common_store="$(jq -er '.commonStore' "$log_dir/store-paths.json")"
[ -d "$common_store/artifacts/objects" ] \
  || die "missing-resource object store is absent: $common_store/artifacts/objects"
artifact_objects="$(cd "$common_store/artifacts/objects" && pwd -P)"
case "$artifact_objects" in
  "$root"/*) ;;
  *) die "missing-resource object store escaped the disposable root: $artifact_objects" ;;
esac
missing_digest="${missing_artifact#sha256:}"
missing_artifact_path="$artifact_objects/$missing_digest.json"
[ -f "$missing_artifact_path" ] \
  || die "bound missing-resource artifact is absent before the fixture move"
[ ! -L "$missing_artifact_path" ] \
  || die "bound missing-resource artifact must not be a symlink"
case "$missing_artifact_path" in
  "$artifact_objects"/*) ;;
  *) die "bound missing-resource artifact escaped its object store" ;;
esac
missing_recovery_dir="$fixture_root/recoverable-missing-resource"
mkdir -p "$missing_recovery_dir"
missing_recovery_dir="$(cd "$missing_recovery_dir" && pwd -P)"
case "$missing_recovery_dir" in
  "$root"/*) ;;
  *) die "missing-resource recovery directory escaped the disposable root" ;;
esac
missing_recovery_path="$missing_recovery_dir/$missing_digest.json"
[ ! -e "$missing_recovery_path" ] \
  || die "missing-resource recovery target already exists"
mv "$missing_artifact_path" "$missing_recovery_path"
[ ! -e "$missing_artifact_path" ] && [ -f "$missing_recovery_path" ] \
  || die "missing-resource artifact move did not preserve exactly one recovery copy"

# Prove the two bodyless states before publishing derived access. Removed is
# event-authorized; missing is a physical absence with its bytes still retained
# under the evidence root. Neither exact read may substitute live Git bytes.
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change resource \
  "$exact_change" "$exact_revision" --artifact-hash "$exact_artifact" \
  --repo "$fixture_repo" --format json >"$log_dir/removed-resource-preflight.json"
jq -e --arg revision "$exact_revision" --arg artifact "$exact_artifact" '
  .schema == "pointbreak.review-revision-resource" and .version == 1 and
  .availability == "removed" and
  .resource.revision.revisionId == $revision and
  .resource.revision.objectArtifactContentHash == $artifact and
  .capturedDocument == null and .capturedDocumentHash == null and
  .diagnostics == ["captured_resource_removed"]
' "$log_dir/removed-resource-preflight.json" >/dev/null \
  || die "removed-resource preflight did not remain exact and bodyless for $exact_change"

POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change resource \
  "$missing_change" "$missing_revision" --artifact-hash "$missing_artifact" \
  --repo "$fixture_repo" --format json >"$log_dir/missing-resource-preflight.json"
jq -e --arg revision "$missing_revision" --arg artifact "$missing_artifact" '
  .schema == "pointbreak.review-revision-resource" and .version == 1 and
  .availability == "missing" and
  .resource.revision.revisionId == $revision and
  .resource.revision.objectArtifactContentHash == $artifact and
  .capturedDocument == null and .capturedDocumentHash == null and
  .diagnostics == ["captured_resource_missing"]
' "$log_dir/missing-resource-preflight.json" >/dev/null \
  || die "missing-resource preflight did not remain exact and bodyless for $missing_change"

# Extend the retained matrix exactly once with event families whose historical
# semantics cannot be inferred from the final Change cards alone. All writes
# use supported public commands against this disposable L2 root.
primary_change="$(jq -er '.topology.initial.change' "$log_dir/base-matrix.json")"
primary_revision="$(jq -er '.primary_revision' "$log_dir/base-matrix.json")"
primary_artifact="$(jq -er '.topology.initial.current.artifact' "$log_dir/base-matrix.json")"
historical_change="$(jq -er '.topology.parallel_current.change' "$log_dir/base-matrix.json")"
shared_revision="$(jq -er '.shared_revision.revision' "$log_dir/base-matrix.json")"
graph_change="$(jq -s -e -r '.[0].changeId' "$log_dir/scale-captures.jsonl")"
graph_successor_revision="$(jq -s -e -r '.[0].revision.revisionId' "$log_dir/scale-captures.jsonl")"
graph_successor_artifact="$(jq -s -e -r '.[0].revision.objectArtifactContentHash' "$log_dir/scale-captures.jsonl")"
graph_context_revision="$(jq -s -e -r '.[1].revision.revisionId' "$log_dir/scale-captures.jsonl")"
graph_context_artifact="$(jq -s -e -r '.[1].revision.objectArtifactContentHash' "$log_dir/scale-captures.jsonl")"

# Build claim-only relationship context from two otherwise clean scale
# Changes. Assert the relation while both exact Revisions are members, then
# withdraw the contextual membership. The active relation must remain visible
# as typed incomplete context without making an unavailable node actionable.
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" change join "$graph_change" "$graph_context_revision" \
    --operation-id "change-operation:browser-graph-context-join-v1" \
    --repo "$fixture_repo" --format json \
    >"$log_dir/graph-context-join.json" \
    2>"$log_dir/graph-context-join.log"
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change show \
  "$graph_change" --repo "$fixture_repo" --format json \
  >"$log_dir/graph-context-after-join.json"
graph_context_membership_claim="$(jq -er --arg revision "$graph_context_revision" '
  [.membershipClaims[] | select(.revisionId == $revision and .active == true)]
  | if length == 1 then .[0].claimId
    else error("expected one active graph context membership claim") end
' "$log_dir/graph-context-after-join.json")"
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" change assert-relation "$graph_change" \
    "$graph_successor_revision" "$graph_context_revision" \
    --successor-artifact-hash "$graph_successor_artifact" \
    --predecessor-artifact-hash "$graph_context_artifact" \
    --operation-id "change-operation:browser-graph-context-relation-v1" \
    --repo "$fixture_repo" --format json \
    >"$log_dir/graph-context-relation.json" \
    2>"$log_dir/graph-context-relation.log"
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" change withdraw-membership "$graph_context_membership_claim" \
    --operation-id "change-operation:browser-graph-context-withdraw-v1" \
    --repo "$fixture_repo" --format json \
    >"$log_dir/graph-context-withdraw.json" \
    2>"$log_dir/graph-context-withdraw.log"
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change show \
  "$graph_change" --repo "$fixture_repo" --format json \
  >"$log_dir/graph-context-final.json"

POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" observation add --repo "$fixture_repo" \
    --exact-revision "$primary_revision" --track "agent:browser-history-cases" \
    --title "Browser correction origin" \
    --body "This observation is retained as the superseded historical fact." \
    --idempotency-key "browser-history-correction-origin-v1" --format json \
    >"$log_dir/correction-origin.json" 2>"$log_dir/correction-origin.log"
correction_origin_id="$(jq -er '.observationId' "$log_dir/correction-origin.json")"

POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" observation add --repo "$fixture_repo" \
    --exact-revision "$primary_revision" --track "agent:browser-history-cases" \
    --title "Browser correction replacement" \
    --body "This observation explicitly corrects the retained historical fact." \
    --supersedes "$correction_origin_id" \
    --idempotency-key "browser-history-correction-replacement-v1" --format json \
    >"$log_dir/correction-replacement.json" 2>"$log_dir/correction-replacement.log"

# The primary exact Revision is directly owned by its original Change. Add one
# historical membership in a second Change, resolve the exact claim from the
# typed Change document, and withdraw only that claim. Historical correlation
# must remain visible even though effective current membership does not.
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" change join "$historical_change" "$primary_revision" \
    --repo "$fixture_repo" \
    --operation-id "change-operation:browser-history-membership-join-v1" --format json \
    >"$log_dir/historical-membership-join.json" \
    2>"$log_dir/historical-membership-join.log"
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change show \
  "$historical_change" --repo "$fixture_repo" --format json \
  >"$log_dir/historical-membership-after-join.json"
historical_membership_claim="$(jq -er --arg revision "$primary_revision" '
  [.membershipClaims[] | select(.revisionId == $revision and .active == true)]
  | if length == 1 then .[0].claimId
    else error("expected one active browser historical membership claim") end
' "$log_dir/historical-membership-after-join.json")"
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" change withdraw-membership "$historical_membership_claim" \
    --repo "$fixture_repo" \
    --operation-id "change-operation:browser-history-membership-withdraw-v1" --format json \
    >"$log_dir/historical-membership-withdraw.json" \
    2>"$log_dir/historical-membership-withdraw.log"

POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" store derived build \
  --repo "$fixture_repo" --format json \
  >"$log_dir/derived-build.json" 2>"$log_dir/derived-build.log"
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change list --repo "$fixture_repo" --format json \
  >"$log_dir/changes.json"
change_count="$(jq -er '.changes | length' "$log_dir/changes.json")"
[ "$change_count" -ge 363 ] || die "expected at least 363 public matrix Changes, found $change_count"

# Prove that every topology named by the browser contract is a retained final
# state at its exact fixture identity. The shared Revision must remain an
# active member of all four non-initial topology Changes, including after it is
# historical in the consolidation row.
for topology in initial replacement parallel_current replacement_divergent consolidation; do
  topology_change="$(jq -er --arg topology "$topology" '.topology[$topology].change' "$log_dir/base-matrix.json")"
  topology_current="$(jq -c --arg topology "$topology" '
    .topology[$topology].current
    | if type == "array" then . else [.] end
    | map({revisionId: .revision, objectArtifactContentHash: .artifact})
    # Matrix metadata records fixture construction order. Change documents
    # expose the canonical RevisionId order owned by the ChangeView BTreeSet.
    | sort_by(.revisionId, .objectArtifactContentHash)
  ' "$log_dir/base-matrix.json")"
  POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change show "$topology_change" \
    --repo "$fixture_repo" --format json >"$log_dir/topology-$topology.json"
  jq -e --arg change "$topology_change" --arg topology "$topology" \
    --argjson current "$topology_current" '
      .summary.changeId == $change and .summary.topology == $topology and
      .currentRevisionRefs == $current
    ' "$log_dir/topology-$topology.json" >/dev/null \
    || die "topology fixture $topology did not retain its exact final state"
done
for topology in replacement parallel_current replacement_divergent consolidation; do
  jq -e --arg revision "$shared_revision" '
    any(.memberRevisions[]?; .revision.revisionId == $revision)
  ' "$log_dir/topology-$topology.json" >/dev/null \
    || die "topology fixture $topology omitted the shared exact Revision"
done
jq -e \
  --arg change "$graph_change" \
  --arg successorRevision "$graph_successor_revision" \
  --arg successorArtifact "$graph_successor_artifact" \
  --arg predecessorRevision "$graph_context_revision" \
  --arg predecessorArtifact "$graph_context_artifact" '
    .summary.changeId == $change and
    .summary.topology == "incomplete" and
    .summary.currentRevisionRefs == [{
      revisionId: $successorRevision,
      objectArtifactContentHash: $successorArtifact
    }] and
    (.effectiveSupersedes | length) == 0 and
    any(.pendingOrConflictingEdges[]?;
      .active == true and
      .successor.revisionId == $successorRevision and
      .successor.objectArtifactContentHash == $successorArtifact and
      .predecessor.revisionId == $predecessorRevision and
      .predecessor.objectArtifactContentHash == $predecessorArtifact) and
    any(.diagnostics[]?; . == "change_relation_membership_incomplete")
  ' "$log_dir/graph-context-final.json" >/dev/null \
  || die "pending nonmember graph context changed or disappeared from typed topology"

rich_revision="$(jq -er '.primary_revision' "$log_dir/base-matrix.json")"
rich_change="$(jq -er --arg revision "$rich_revision" '
  [.changes[] | select(any(.currentRevisionRefs[]?; .revisionId == $revision))]
  | if length == 1 then .[0].changeId else error("expected one rich Change") end
' "$log_dir/changes.json")"
rich_artifact="$(jq -er --arg revision "$rich_revision" '
  [.changes[].currentRevisionRefs[]? | select(.revisionId == $revision)]
  | if length == 1 then .[0].objectArtifactContentHash else error("expected one rich Revision") end
' "$log_dir/changes.json")"

fixture_identity="public-l2-change-matrix-v1"
correction_event="$(jq -er '.eventId' "$log_dir/correction-replacement.json")"
fact_port_event="$(jq -er '.fact_port.event_id' "$log_dir/base-matrix.json")"
fact_port_id="$(jq -er '.fact_port.port_id' "$log_dir/base-matrix.json")"
historical_membership_join_event="$(jq -er '
  [.events[] | select(.eventType == "change_membership_asserted")]
  | if length == 1 then .[0].eventId else error("expected one membership assertion event") end
' "$log_dir/historical-membership-join.json")"
historical_membership_withdraw_event="$(jq -er '
  [.events[] | select(.eventType == "change_membership_withdrawn")]
  | if length == 1 then .[0].eventId else error("expected one membership withdrawal event") end
' "$log_dir/historical-membership-withdraw.json")"
jq -n \
  --arg fixture "$fixture_identity" \
  --arg sourceCommit "$source_commit" \
  --arg exactChange "$exact_change" \
  --arg exactRevision "$exact_revision" \
  --arg exactArtifact "$exact_artifact" \
  --arg missingChange "$missing_change" \
  --arg missingRevision "$missing_revision" \
  --arg missingArtifact "$missing_artifact" \
  --arg missingRecoveryPath "$missing_recovery_path" \
  --arg richChange "$rich_change" \
  --arg richRevision "$rich_revision" \
  --arg richArtifact "$rich_artifact" \
  --arg correctionOrigin "$correction_origin_id" \
  --arg correctionEvent "$correction_event" \
  --arg factPortId "$fact_port_id" \
  --arg factPortEvent "$fact_port_event" \
  --arg graphChange "$graph_change" \
  --arg graphSuccessorRevision "$graph_successor_revision" \
  --arg graphSuccessorArtifact "$graph_successor_artifact" \
  --arg graphContextRevision "$graph_context_revision" \
  --arg graphContextArtifact "$graph_context_artifact" \
  --arg directChange "$primary_change" \
  --arg historicalChange "$historical_change" \
  --arg historicalRevision "$primary_revision" \
  --arg historicalArtifact "$primary_artifact" \
  --arg historicalClaim "$historical_membership_claim" \
  --arg historicalJoinEvent "$historical_membership_join_event" \
  --arg historicalWithdrawEvent "$historical_membership_withdraw_event" \
  --argjson equalTimestamp "$equal_timestamp_pair" \
  --argjson changeCount "$change_count" \
  '{fixture: $fixture, sourceCommit: $sourceCommit, changeCount: $changeCount,
    removed: {changeId: $exactChange, revisionId: $exactRevision, artifactHash: $exactArtifact},
    missing: {changeId: $missingChange, revisionId: $missingRevision,
      artifactHash: $missingArtifact, recoverableArtifactPath: $missingRecoveryPath},
    rich: {changeId: $richChange, revisionId: $richRevision, artifactHash: $richArtifact},
    correction: {originObservationId: $correctionOrigin, eventId: $correctionEvent},
    factPort: {portId: $factPortId, eventId: $factPortEvent},
    graph: {changeId: $graphChange,
      successor: {revisionId: $graphSuccessorRevision, artifactHash: $graphSuccessorArtifact},
      context: {revisionId: $graphContextRevision, artifactHash: $graphContextArtifact}},
    historicalMembership: {directChangeId: $directChange,
      historicalChangeId: $historicalChange, revisionId: $historicalRevision,
      artifactHash: $historicalArtifact, claimId: $historicalClaim,
      joinEventId: $historicalJoinEvent, withdrawEventId: $historicalWithdrawEvent},
    equalTimestamp: $equalTimestamp}' \
  >"$log_dir/fixture.json"

# Retain three tiny reader-state roots beside the primary fixture so the real
# browser can prove readiness sequencing without borrowing owner authority.
# Each repository pins its store to worktree-local ephemeral placement.
reader_state_root="$root/reader-state-fixtures"
reader_state_home="$reader_state_root/pointbreak-home"
reader_empty_l2_repo="$reader_state_root/empty-ready-l2"
reader_l0_repo="$reader_state_root/l0"
reader_m1_repo="$reader_state_root/m1"
mkdir -p "$reader_state_home"
for reader_repo in "$reader_empty_l2_repo" "$reader_l0_repo" "$reader_m1_repo"; do
  git -C "$reader_state_root" init --quiet "$reader_repo"
  git -C "$reader_repo" config user.name "Pointbreak Browser Reader Fixture"
  git -C "$reader_repo" config user.email "pointbreak-browser@example.com"
  git -C "$reader_repo" config commit.gpgsign false
  mkdir -p "$reader_repo/.pointbreak/data/events"
  printf '%s\n' \
    '{"schema":"shore.store-config","version":1,"mode":"ephemeral"}' \
    >"$reader_repo/.pointbreak/store.local.json"
  printf '%s\n' 'public Inspector reader-state fixture' >"$reader_repo/README.md"
  git -C "$reader_repo" add README.md
  git -C "$reader_repo" commit --quiet -m "reader fixture base"
done
ready_store="$snapshot_ready_store"
activation_record="$ready_store/$activation_fixture"
completion_record="$ready_store/$completion_fixture"
[ -f "$activation_record" ] && [ -f "$completion_record" ] \
  || die "public reader-state activation fixtures are unavailable"
cp "$activation_record" "$completion_record" "$reader_empty_l2_repo/.pointbreak/data/events/"
cp "$activation_record" "$reader_m1_repo/.pointbreak/data/events/"

session="pointbreak-change-browser-$$"
if [ -n "${PLAYWRIGHT_CLI:-}" ]; then
  pwcli=("$PLAYWRIGHT_CLI")
elif command -v playwright-cli >/dev/null 2>&1; then
  pwcli=(playwright-cli)
else
  command -v npx >/dev/null 2>&1 || die "playwright-cli and npx are unavailable"
  pwcli=(npx --yes --package @playwright/cli@0.1.17 playwright-cli)
fi
browser_cleanup_enabled=true

start_reader_state_server() {
  local state="$1"
  local repo="$2"
  local startup="$log_dir/reader-$state-startup.json"
  local server_log="$log_dir/reader-$state-server.log"
  POINTBREAK_DERIVED_ACCESS=sqlite-wal-bodyless-v1 \
    POINTBREAK_HOME="$reader_state_home" "$pointbreak_binary" inspect \
    --repo "$repo" --port 0 --format json >"$startup" 2>"$server_log" &
  reader_state_started_pid=$!
  register_background_process "$reader_state_started_pid"
  for _ in $(seq 1 100); do
    [ -s "$startup" ] && break
    kill -0 "$reader_state_started_pid" >/dev/null 2>&1 \
      || die "$state Inspector exited before startup"
    sleep 0.05
  done
  jq -e '
    .schema == "pointbreak.inspect-startup" and .version == 1 and
    (.port > 0) and (.token | length > 0)
  ' "$startup" >/dev/null || die "$state Inspector did not emit valid startup JSON"
}

retry_empty_ready_l2() {
  local startup="$log_dir/reader-empty-ready-l2-startup.json"
  local base_url
  local token
  local retry_log="$log_dir/browser-empty-ready-l2-retry.json"
  local ready_log="$log_dir/browser-empty-ready-l2-ready.json"
  local ready_tmp="$ready_log.tmp"
  local response_status

  base_url="http://$(jq -r '.host' "$startup"):$(jq -r '.port' "$startup")"
  token="$(jq -r '.token' "$startup")"
  response_status="$(curl -sS -o "$retry_log" -w '%{http_code}' -X POST \
    -H "Authorization: Bearer $token" \
    "$base_url/api/derived-access/retry")"
  [ "$response_status" = "200" ] \
    || die "empty-ready-l2 derived-access retry returned HTTP $response_status"
  jq -e '
    .schema == "pointbreak.inspect-derived-access-status" and .version == 1 and
    .active == true and (.availability | type == "string") and
    (.rebuildInFlight | type == "boolean") and (.actions | type == "array")
  ' "$retry_log" >/dev/null \
    || die "empty-ready-l2 derived-access retry did not return typed status"

  for _ in $(seq 1 200); do
    response_status="$(curl -sS -o "$ready_tmp" -w '%{http_code}' \
      -H "Authorization: Bearer $token" \
      "$base_url/api/derived-access/status")"
    if [ "$response_status" = "200" ] && jq -e '
      .schema == "pointbreak.inspect-derived-access-status" and .version == 1 and
      .active == true and .servingCurrent == true and .availability == "current" and
      .rebuildInFlight == false and .rebuildPaused == false
    ' "$ready_tmp" >/dev/null; then
      mv "$ready_tmp" "$ready_log"
      return 0
    fi
    sleep 0.05
  done
  [ -f "$ready_tmp" ] && mv "$ready_tmp" "$ready_log"
  die "empty-ready-l2 did not publish a current derived generation after explicit retry"
}

retain_primary_derived_access_status() {
  local startup="$log_dir/inspect-startup.json"
  local status_log="$log_dir/browser-primary-derived-access-status.json"
  local status_tmp="$status_log.tmp"
  local base_url
  local token
  local response_status

  base_url="http://$(jq -r '.host' "$startup"):$(jq -r '.port' "$startup")"
  token="$(jq -r '.token' "$startup")"
  for _ in $(seq 1 200); do
    response_status="$(curl -sS -o "$status_tmp" -w '%{http_code}' \
      -H "Authorization: Bearer $token" \
      "$base_url/api/derived-access/status")"
    if [ "$response_status" = "200" ] && jq -e '
      .schema == "pointbreak.inspect-derived-access-status" and .version == 1 and
      .active == true and .servingCurrent == true and .availability == "current" and
      .rebuildInFlight == false and .rebuildPaused == false
    ' "$status_tmp" >/dev/null; then
      mv "$status_tmp" "$status_log"
      return 0
    fi
    sleep 0.05
  done
  [ -f "$status_tmp" ] && mv "$status_tmp" "$status_log"
  die "primary Inspector did not publish an active current derived-access status"
}

start_reader_state_server "empty-ready-l2" "$reader_empty_l2_repo"
retry_empty_ready_l2
start_reader_state_server "l0" "$reader_l0_repo"
start_reader_state_server "m1" "$reader_m1_repo"
reader_servers="$(jq -cn \
  --slurpfile empty "$log_dir/reader-empty-ready-l2-startup.json" \
  --slurpfile l0 "$log_dir/reader-l0-startup.json" \
  --slurpfile m1 "$log_dir/reader-m1-startup.json" '
    def server($startup): {
      baseUrl: ("http://" + $startup.host + ":" + ($startup.port | tostring)),
      token: $startup.token
    };
    {emptyReadyL2: server($empty[0]), l0: server($l0[0]), m1: server($m1[0])}
  ')"

POINTBREAK_DERIVED_ACCESS=sqlite-wal-bodyless-v1 \
  POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" inspect --repo "$fixture_repo" --port 0 --format json \
  >"$log_dir/inspect-startup.json" 2>"$log_dir/inspect-server.log" &
server_pid=$!
register_background_process "$server_pid"
for _ in $(seq 1 100); do
  [ -s "$log_dir/inspect-startup.json" ] && break
  kill -0 "$server_pid" >/dev/null 2>&1 || die "Inspector exited before startup"
  sleep 0.05
done
jq -e '.schema == "pointbreak.inspect-startup" and .version == 1 and (.port > 0) and (.token | length > 0)' \
  "$log_dir/inspect-startup.json" >/dev/null || die "Inspector did not emit valid startup JSON"
server="$(jq -c '{baseUrl: ("http://" + .host + ":" + (.port | tostring)), token}' "$log_dir/inspect-startup.json")"
retain_primary_derived_access_status

# L2 remains a deliberately Change-aware reader profile: the restored Timeline
# must use `/api/v2/history`, never reactivate the retired aggregate endpoint.
legacy_status="$(curl -sS -o "$log_dir/legacy-history.json" -w '%{http_code}' \
  -H "Authorization: Bearer $(jq -r '.token' "$log_dir/inspect-startup.json")" \
  "$(jq -r '.baseUrl' <<<"$server")/api/history")"
[ "$legacy_status" = "426" ] \
  || die "legacy /api/history unexpectedly returned HTTP $legacy_status instead of 426"
jq -e '.schema == "pointbreak.reader-upgrade-required" and .version == 1' \
  "$log_dir/legacy-history.json" >/dev/null \
  || die "legacy /api/history did not return the typed reader-upgrade response"

browser_config="$(jq -cn \
  --arg artifactDir "$artifact_dir" \
  --arg appendReceipt "$log_dir/timeline-append.json" \
  --argjson server "$server" \
  --argjson readerServers "$reader_servers" \
  --slurpfile fixture "$log_dir/fixture.json" \
  --slurpfile matrix "$log_dir/base-matrix.json" \
  '{artifactDir: $artifactDir, appendReceipt: $appendReceipt, server: $server,
    readerServers: $readerServers,
    fixture: ($fixture[0] + {matrix: $matrix[0]})}')"
browser_program="$log_dir/browser-program.mjs"
# shellcheck disable=SC2016 # JavaScript template literals are intentionally single-quoted from Bash.
node --input-type=module -e '
import fs from "node:fs";
import { pathToFileURL } from "node:url";
const source = fs.readFileSync(process.argv[1], "utf8");
const diagnostics = await import(pathToFileURL(process.argv[2]));
const replacements = new Map([
  ["__POINTBREAK_BROWSER_DIAGNOSTIC_FAILURE__", diagnostics.BrowserDiagnosticFailure.toString()],
  ["__POINTBREAK_BROWSER_DIAGNOSTICS__", diagnostics.createBrowserDiagnostics.toString()],
  ["__POINTBREAK_CHANGE_BROWSER_CONFIG__", process.argv[3]],
]);
let rendered = source;
for (const [marker, value] of replacements) {
  if (!rendered.includes(marker)) throw new Error(`browser program marker is missing: ${marker}`);
  rendered = rendered.replace(marker, value);
}
fs.writeFileSync(process.argv[4], rendered);
' "$browser_program_template" "$browser_diagnostics" "$browser_config" "$browser_program"

# Create the session without visiting the Inspector. The injected program installs
# console, page-error, and request-failure observers before it performs the
# capability-bearing bootstrap navigation.
run_pw open about:blank >"$log_dir/browser-open.log" 2>&1

# The browser program first parks the initial Timeline and writes the retained
# screenshot below.  Only then append one public fixture event.  This avoids a
# racy sleep while proving that a parked reader remains stable until its
# explicit catch-up action.  The worker changes only the disposable repository
# and writes its receipt below the caller-provided evidence root.
timeline_append_marker="$artifact_dir/timeline-parked-before-append.png"
(
  for _ in $(seq 1 240); do
    [ -f "$timeline_append_marker" ] && break
    sleep 0.25
  done
  [ -f "$timeline_append_marker" ] || exit 1
  printf 'pub const BROWSER_TIMELINE_APPEND: &str = "after-park";\n' \
    >"$fixture_repo/src/browser-scale.rs"
  POINTBREAK_HOME="$pointbreak_home" \
    POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
    "$pointbreak_binary" capture --repo "$fixture_repo" \
      --summary "Browser Timeline append after park" --format json \
      >"$log_dir/timeline-append.json" 2>"$log_dir/timeline-append.log"
) &
timeline_append_pid=$!
register_background_process "$timeline_append_pid"
browser_gate_status=0
run_pw run-code --filename="$browser_program" >"$log_dir/browser-gate.log" 2>&1 \
  || browser_gate_status=$?
browser_result="$log_dir/browser-result.json"
browser_result_line="$(awk '
  {
    line = $0
    sub(/\r$/, "", line)
    if (after_result) {
      result = line
      after_result = 0
    }
    if (line == "### Result") after_result = 1
  }
  END {
    if (result != "") print result
  }
' "$log_dir/browser-gate.log")"
if [ -n "$browser_result_line" ]; then
  printf '%s\n' "$browser_result_line" >"$browser_result"
  jq -e '
    .schema == "pointbreak.change-inspector-browser-report" and .version == 1 and
    (.status == "passed" or .status == "failed") and
    (.assertionCount | type == "number") and (.assertionCount >= 0) and
    (.screenshotCount | type == "number") and (.screenshotCount >= 0) and
    (.sectionCount | type == "number") and (.sectionCount > 0) and
    (.globalInvalid | type == "boolean") and
    (.sections | type == "array") and ((.sections | length) == .sectionCount) and
    (.failures | type == "array")
  ' "$browser_result" >/dev/null || die "browser emitted an invalid diagnostic report"
fi
if [ "$browser_gate_status" -ne 0 ]; then
  sed -n '1,240p' "$log_dir/browser-gate.log" >&2
  die "real-browser Change Inspector gate failed"
fi
[ -s "$browser_result" ] || die "browser did not emit its diagnostic report"
jq -e '
  .status == "passed" and .globalInvalid == false and
  (.failures | length == 0) and
  (.sections | all(.status == "passed" and .failureCount == 0))
' "$browser_result" >/dev/null \
  || {
    jq -r '
      .failures[]? |
      "[\(.section)] \(.label): \(.detail)\n  expected=\(.expected | tojson) actual=\(.actual | tojson)\n  route=\(.route) screenshot=\(.screenshot)"
    ' "$browser_result" >&2
    die "browser diagnostic report did not pass"
  }
if wait "$timeline_append_pid"; then
  forget_background_process "$timeline_append_pid"
else
  forget_background_process "$timeline_append_pid"
  die "disposable Timeline append did not complete after the parked screenshot"
fi
test -s "$log_dir/timeline-append.json" \
  || die "disposable Timeline append did not leave its receipt"
jq -e '
  .schema == "pointbreak.change-capture-receipt.v1" and .version == 1 and
  (.changeId | startswith("change:sha256:")) and
  (.revision.revisionId | startswith("rev:sha256:")) and
  (.revision.objectArtifactContentHash | startswith("sha256:"))
' "$log_dir/timeline-append.json" >/dev/null \
  || die "disposable Timeline append did not emit an exact capture receipt"
if rg -q '^### Error' "$log_dir/browser-gate.log"; then
  sed -n '1,240p' "$log_dir/browser-gate.log" >&2
  die "real-browser Change Inspector gate reported an error"
fi

screenshot_count="$(find "$artifact_dir" -maxdepth 1 -type f -name '*.png' | wc -l | tr -d ' ')"
assertion_count="$(jq -er '.assertionCount' "$browser_result")"
reported_screenshot_count="$(jq -er '.screenshotCount' "$browser_result")"
[ "$screenshot_count" -eq "$reported_screenshot_count" ] \
  || die "browser reported $reported_screenshot_count screenshots but preserved $screenshot_count"
[ "$screenshot_count" -ge 12 ] || die "expected at least 12 browser screenshots, found $screenshot_count"
[ "$(shasum -a 256 "$pointbreak_binary" | awk '{print $1}')" = "$binary_sha256" ] \
  || die "executed binary snapshot changed during browser qualification"
[ "$(shasum -a 256 "$browser_program_template" | awk '{print $1}')" = "$template_sha256" ] \
  || die "browser program snapshot changed during qualification"
[ "$(shasum -a 256 "$browser_diagnostics" | awk '{print $1}')" = "$diagnostics_sha256" ] \
  || die "browser diagnostics snapshot changed during qualification"
[ "$(shasum -a 256 "$browser_manifest_publisher" | awk '{print $1}')" = "$publisher_sha256" ] \
  || die "manifest publisher snapshot changed during qualification"
[ "$(shasum -a 256 "$matrix_materializer" | awk '{print $1}')" = "$materializer_sha256" ] \
  || die "fixture materializer snapshot changed during qualification"
[ "$(shasum -a 256 "$snapshot_ready_store/$activation_fixture" | awk '{print $1}')" = "$activation_fixture_sha256" ] \
  || die "activation fixture snapshot changed during qualification"
[ "$(shasum -a 256 "$snapshot_ready_store/$completion_fixture" | awk '{print $1}')" = "$completion_fixture_sha256" ] \
  || die "completion fixture snapshot changed during qualification"
[ "$(shasum -a 256 "$script_dir/change-inspector-browser-verify.sh" | awk '{print $1}')" = "$shell_sha256" ] \
  || die "browser verifier source changed during qualification"
tool_versions="$(jq -n \
  --arg git "$(git --version)" \
  --arg node "$(node --version)" \
  --arg playwright "$(run_pw --version 2>&1 | tr '\n' ' ')" \
  --slurpfile pointbreak "$log_dir/pointbreak-version.json" \
  '{git: $git, node: $node, playwright: $playwright, pointbreak: $pointbreak[0]}')"

# The completion marker must follow browser shutdown and every child log
# flush. Run the normally trap-owned cleanup explicitly, reap each child, then
# disarm the trap so no evidence file can be written after manifest.json.
cleanup strict || die "browser session did not close cleanly"
trap - EXIT

# The temporary file may be incomplete if serialization fails. Only the final
# atomic rename publishes manifest.json, so its presence remains the completion
# marker for fixture, browser, screenshot, identity, and cleanup verification.
manifest_tmp="$root/.manifest.json.tmp"
[ "$(shasum -a 256 "$log_dir/harness-digests.json" | awk '{print $1}')" = "$harness_record_sha256" ] \
  || die "browser harness digest record changed during qualification"
for required_evidence_path in \
  logs/browser-empty-ready-l2-retry.json \
  logs/browser-empty-ready-l2-ready.json \
  logs/browser-primary-derived-access-status.json \
  logs/browser-result.json \
  logs/browser-gate.log \
  logs/browser-program.mjs; do
  [ -f "$root/$required_evidence_path" ] \
    || die "required browser evidence is missing: $required_evidence_path"
done
evidence_inventory="$({
  for evidence_path in "$artifact_dir"/*.png; do
    [ -f "$evidence_path" ] || continue
    printf 'browser-artifacts/%s\n' "${evidence_path##*/}"
  done
  for evidence_path in "$log_dir"/browser-*; do
    [ -f "$evidence_path" ] || continue
    case "$evidence_path" in
      *.json | *.log | *.mjs) printf 'logs/%s\n' "${evidence_path##*/}" ;;
    esac
  done
} | LC_ALL=C sort | while IFS= read -r evidence_path; do
  evidence_sha256="$(shasum -a 256 "$root/$evidence_path" | awk '{print $1}')"
  jq -cn --arg path "$evidence_path" --arg sha256 "$evidence_sha256" \
    '{path: $path, sha256: $sha256}'
done | jq -s '.')"
jq -n \
  --arg sourceCommit "$source_commit" \
  --arg binary "$requested_binary" \
  --arg executedBinary "$pointbreak_binary" \
  --arg binarySha256 "$binary_sha256" \
  --arg root "$root" \
  --arg fixture "$fixture_identity" \
  --argjson fixtureData "$(cat "$log_dir/fixture.json")" \
  --argjson timelineAppend "$(cat "$log_dir/timeline-append.json")" \
  --argjson toolVersions "$tool_versions" \
  --slurpfile harness "$log_dir/harness-digests.json" \
  --slurpfile primaryDerivedAccess "$log_dir/browser-primary-derived-access-status.json" \
  --arg harnessSha256 "$harness_record_sha256" \
  --argjson assertionCount "$assertion_count" \
  --argjson screenshotCount "$screenshot_count" \
  --argjson evidenceInventory "$evidence_inventory" \
  '{gate: "change-inspector-browser-verify", status: "passed", sourceCommit: $sourceCommit,
    binary: $binary, executedBinary: $executedBinary, binarySha256: $binarySha256,
    harness: $harness[0], harnessSha256: $harnessSha256, root: $root, fixture: $fixture,
    fixtureData: $fixtureData, timelineAppend: $timelineAppend,
    primaryDerivedAccessStatus: $primaryDerivedAccess[0],
    toolVersions: $toolVersions, assertionCount: $assertionCount,
    screenshotCount: $screenshotCount, evidenceInventory: $evidenceInventory}' \
  >"$manifest_tmp"
[ "$(shasum -a 256 "$browser_manifest_publisher" | awk '{print $1}')" = "$publisher_sha256" ] \
  || die "manifest publisher snapshot changed before completion publication"
node "$browser_manifest_publisher" "$manifest_tmp" "$root/manifest.json" "$browser_result"
cat "$root/manifest.json"
