#!/usr/bin/env bash
# Materialize canonical and synthetic decision-continuity records beneath one empty destination.
# Prefer `just review-decision-matrix-materialize`; writes stay inside the disposable destination.

set -euo pipefail

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

normalize_for_shell_comparison() {
  local native_path="${1//\\//}"
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -u "$native_path"
  else
    printf '%s\n' "$native_path"
  fi
}

command -v git >/dev/null 2>&1 || die "git is required"
command -v jq >/dev/null 2>&1 || die "jq is required"

[ "$#" -eq 1 ] || die "usage: $0 <empty-destination>"

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
destination="$1"
pointbreak_binary="${POINTBREAK_BINARY:-$repo_root/target/debug/pointbreak}"

[ -x "$pointbreak_binary" ] \
  || die "POINTBREAK_BINARY is not executable; provide an absolute installed binary or run 'just build'"
case "$pointbreak_binary" in
  /* | [A-Za-z]:/* | [A-Za-z]:\\* | \\\\*) ;;
  *) die "POINTBREAK_BINARY must resolve to an absolute path" ;;
esac

if [ -e "$destination" ]; then
  [ -d "$destination" ] || die "destination exists and is not a directory: $destination"
  [ -z "$(find "$destination" -mindepth 1 -maxdepth 1 -print -quit)" ] \
    || die "destination is not empty: $destination"
else
  mkdir -p "$destination"
fi

destination="$(cd "$destination" && pwd -P)"
case "$destination" in
  "$repo_root"|"$repo_root"/*)
    die "destination must be outside the Pointbreak source worktree"
    ;;
esac

pointbreak_home="${POINTBREAK_HOME:-$destination/.git/pointbreak-home}"
mkdir -p "$pointbreak_home"
pointbreak_home="$(cd "$pointbreak_home" && pwd -P)"
if [ -n "${POINTBREAK_HOME:-}" ]; then
  destination_parent="$(dirname "$destination")"
  case "$pointbreak_home" in
    "$destination_parent"/*) ;;
    *) die "POINTBREAK_HOME must remain beneath the destination's temporary parent" ;;
  esac
  [ -z "$(find "$pointbreak_home" -mindepth 1 -maxdepth 1 -print -quit)" ] \
    || die "POINTBREAK_HOME must be empty for deterministic materialization: $pointbreak_home"
fi

pointbreak_json() {
  POINTBREAK_HOME="$pointbreak_home" \
    "$pointbreak_binary" "$@" --format json
}

pointbreak_actor_json() {
  local actor="$1"
  shift
  POINTBREAK_HOME="$pointbreak_home" \
    POINTBREAK_ACTOR_ID="$actor" "$pointbreak_binary" "$@" --format json
}

capture_revision() {
  local actor="$1"
  shift
  pointbreak_actor_json "$actor" capture --repo "$destination" "$@" \
    | jq -er '.revision.revisionId'
}

record_validation() {
  local revision="$1"
  local check_name="$2"
  local status="$3"
  local completed_at="$4"
  pointbreak_actor_json "actor:agent:pointbreak-matrix-validation-writer" \
    validation add --repo "$destination" --exact-revision "$revision" \
    --track "agent:matrix-validation" --check-name "$check_name" \
    --status "$status" --completed-at "$completed_at" >/dev/null
}

git -C "$destination" init --quiet
git -C "$destination" symbolic-ref HEAD refs/heads/main
git -C "$destination" config user.name "Pointbreak Matrix"
git -C "$destination" config user.email "pointbreak-matrix@example.com"
git -C "$destination" config commit.gpgsign false
# The decision matrix is an L2-only product fixture.  Seed the two frozen,
# public activation records before its first writer command; production stores
# are never consulted or modified by this developer-only materializer.
ready_store="${POINTBREAK_CHANGE_READY_FIXTURE_DIR:-$repo_root/tests/support/assets/change-ready-store}"
[ -f "$ready_store/5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json" ] \
  || die "public L2 activation fixture is missing"
[ -f "$ready_store/f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json" ] \
  || die "public L2 completion fixture is missing"
mkdir -p "$destination/.git/pointbreak/events"
cp "$ready_store"/*.json "$destination/.git/pointbreak/events/"

mkdir -p "$destination/src"
printf 'pub fn matrix_value() -> u32 { 1 }\n' > "$destination/src/lib.rs"
git -C "$destination" add --all
git -C "$destination" commit --quiet -m "matrix base"
base_commit="$(git -C "$destination" rev-parse HEAD)"

git -C "$destination" switch --quiet -c feat/decision-matrix
printf 'pub fn matrix_value() -> u32 { 2 }\n' > "$destination/src/lib.rs"
primary_capture="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-capture-writer" \
  capture --repo "$destination" --summary "Decision continuity matrix")"
primary_change="$(printf '%s\n' "$primary_capture" | jq -er '.changeId')"
primary_revision="$(printf '%s\n' "$primary_capture" | jq -er '.revision.revisionId')"
primary_artifact="$(printf '%s\n' "$primary_capture" | jq -er '.revision.objectArtifactContentHash')"

pointbreak_actor_json "actor:agent:pointbreak-matrix-fact-writer" \
  observation add --repo "$destination" --exact-revision "$primary_revision" \
  --track "agent:matrix-facts" --title "Matrix fact" \
  --body "The matrix keeps evidence classes distinct." >/dev/null

pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-participant-opener" \
  input-request open --repo "$destination" --revision "$primary_revision" \
  --track "agent:matrix-requests" --title "Open decision" \
  --reason insufficient-evidence --body "More evidence is required." >/dev/null

responded_request="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-participant-opener" \
  input-request open --repo "$destination" --revision "$primary_revision" \
  --track "agent:matrix-requests" --title "Responded decision" \
  --reason manual-decision-required --body "Is the evidence sufficient?" \
  | jq -er '.inputRequestId')"
pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-participant-responder" \
  input-request respond "$responded_request" --repo "$destination" \
  --outcome approved --reason "the evidence is sufficient" >/dev/null

ambiguous_request="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-request-opener" \
  input-request open --repo "$destination" --revision "$primary_revision" \
  --track "agent:matrix-requests" --title "Ambiguous decision" \
  --reason conflicting-event --body "The responses may conflict." \
  | jq -er '.inputRequestId')"
pointbreak_actor_json "actor:agent:pointbreak-matrix-response-one" \
  input-request respond "$ambiguous_request" --repo "$destination" \
  --outcome approved --reason "first response approves" >/dev/null
pointbreak_actor_json "actor:agent:pointbreak-matrix-response-two" \
  input-request respond "$ambiguous_request" --repo "$destination" \
  --outcome rejected --reason "second response rejects" >/dev/null

replaced_assessment="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-assessment-writer-one" \
  assessment add --repo "$destination" --exact-revision "$primary_revision" \
  --track "agent:matrix-assessment" --assessment needs-changes \
  --summary "The matrix is incomplete." | jq -er '.assessmentId')"
pointbreak_actor_json "actor:agent:pointbreak-matrix-assessment-writer-two" \
  assessment add --repo "$destination" --exact-revision "$primary_revision" \
  --track "agent:matrix-assessment" --assessment accepted-with-follow-up \
  --summary "The matrix is complete with bounded follow-up." \
  --replaces "$replaced_assessment" >/dev/null

record_validation "$primary_revision" "passed current" passed "2026-07-17T10:00:00Z"
record_validation "$primary_revision" "failed current" failed "2026-07-17T10:00:00Z"
record_validation "$primary_revision" "errored current" errored "2026-07-17T10:00:00Z"
record_validation "$primary_revision" "skipped only" skipped "2026-07-17T10:00:00Z"
record_validation "$primary_revision" "failed then passed" failed "2026-07-17T10:00:00Z"
record_validation "$primary_revision" "failed then passed" passed "2026-07-17T10:01:00Z"
record_validation "$primary_revision" "errored then passed" errored "2026-07-17T10:00:00Z"
record_validation "$primary_revision" "errored then passed" passed "2026-07-17T10:01:00Z"
record_validation "$primary_revision" "equal time" failed "2026-07-17T10:00:00Z"
record_validation "$primary_revision" "equal time" passed "2026-07-17T10:00:00Z"
record_validation "$primary_revision" "regression" passed "2026-07-17T10:00:00Z"
record_validation "$primary_revision" "regression" failed "2026-07-17T10:01:00Z"
record_validation "$primary_revision" "failure followed by skip" failed "2026-07-17T10:00:00Z"
record_validation "$primary_revision" "failure followed by skip" skipped "2026-07-17T10:01:00Z"

git -C "$destination" add --all
git -C "$destination" commit --quiet -m "first matrix landing"
first_landing="$(git -C "$destination" rev-parse HEAD)"
first_commit_association="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-association-writer" \
  association record --repo "$destination" --revision "$primary_revision" \
  --track "agent:matrix-associations" --commit "$first_landing" \
  | jq -er '.commitAssociationId')"

printf 'pub fn matrix_value() -> u32 { 3 }\n' > "$destination/src/lib.rs"
git -C "$destination" add --all
git -C "$destination" commit --quiet -m "second matrix landing"
second_landing="$(git -C "$destination" rev-parse HEAD)"
pointbreak_actor_json "actor:agent:pointbreak-matrix-association-writer" \
  association record --repo "$destination" --revision "$primary_revision" \
  --track "agent:matrix-associations" --commit "$second_landing" >/dev/null
pointbreak_actor_json "actor:agent:pointbreak-matrix-association-writer" \
  association withdraw "$first_commit_association" --repo "$destination" \
  --revision "$primary_revision" --track "agent:matrix-associations" >/dev/null

git -C "$destination" branch withdrawn-matrix "$first_landing"
withdrawn_ref_association="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-association-writer" \
  association record --repo "$destination" --revision "$primary_revision" \
  --track "agent:matrix-associations" --ref withdrawn-matrix \
  --head "$first_landing" | jq -er '.refAssociationId')"
pointbreak_actor_json "actor:agent:pointbreak-matrix-association-writer" \
  association withdraw "$withdrawn_ref_association" --repo "$destination" \
  --revision "$primary_revision" --track "agent:matrix-associations" >/dev/null
git -C "$destination" branch live-matrix "$second_landing"
pointbreak_actor_json "actor:agent:pointbreak-matrix-association-writer" \
  association record --repo "$destination" --revision "$primary_revision" \
  --track "agent:matrix-associations" --ref live-matrix \
  --head "$second_landing" >/dev/null

git -C "$destination" switch --quiet main
git -C "$destination" merge --quiet --ff-only feat/decision-matrix

git -C "$destination" switch --quiet -c feat/live-matrix
printf 'pub fn matrix_value() -> u32 { 4 }\n' > "$destination/src/lib.rs"
live_revision="$(capture_revision \
  "actor:agent:pointbreak-matrix-capture-writer" \
  --summary "Live landing matrix")"
git -C "$destination" add --all
git -C "$destination" commit --quiet -m "live matrix landing"
live_landing="$(git -C "$destination" rev-parse HEAD)"
pointbreak_actor_json "actor:agent:pointbreak-matrix-association-writer" \
  association record --repo "$destination" --revision "$live_revision" \
  --track "agent:matrix-associations" --commit "$live_landing" >/dev/null
pointbreak_actor_json "actor:agent:pointbreak-matrix-association-writer" \
  association record --repo "$destination" --revision "$live_revision" \
  --track "agent:matrix-associations" --ref feat/live-matrix \
  --head "$live_landing" >/dev/null

git -C "$destination" switch --quiet --detach main
printf 'pub fn matrix_value() -> u32 { 5 }\n' > "$destination/src/lib.rs"
unassessed_revision="$(capture_revision \
  "actor:agent:pointbreak-matrix-capture-writer" \
  --summary "Unassessed matrix")"
git -C "$destination" reset --quiet --hard main

# Keep separate final-state Changes for the browser contract. Reuse the exact
# A/B/C Revisions across explicit Change memberships so replacement,
# parallel-current, divergent replacement, consolidation, and many-to-many
# membership remain simultaneously observable instead of becoming transient
# states in one graph.
git -C "$destination" switch --quiet -c feat/topology-matrix main
printf 'pub fn topology_value() -> u32 { 1 }\n' > "$destination/src/topology.rs"
git -C "$destination" add src/topology.rs
topology_root_capture="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  capture --repo "$destination" --summary "Topology initial matrix")"
topology_root_cursor="$(printf '%s\n' "$topology_root_capture" | jq -er '.reviewCursor.token')"
topology_replacement_change="$(printf '%s\n' "$topology_root_capture" | jq -er '.changeId')"
topology_root_revision="$(printf '%s\n' "$topology_root_capture" | jq -er '.revision.revisionId')"
topology_root_artifact="$(printf '%s\n' "$topology_root_capture" | jq -er '.revision.objectArtifactContentHash')"

printf 'pub fn topology_value() -> u32 { 2 }\n' > "$destination/src/topology.rs"
topology_left_capture="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  capture --repo "$destination" --summary "Topology replacement matrix" \
  --review-cursor "$topology_root_cursor" --advance replace)"
topology_left_revision="$(printf '%s\n' "$topology_left_capture" | jq -er '.revision.revisionId')"
topology_left_artifact="$(printf '%s\n' "$topology_left_capture" | jq -er '.revision.objectArtifactContentHash')"

# A second Change adopts A and B, then C advances in parallel before asserting
# its own replacement of A. Reselection after B -> A is required because every
# Change mutation invalidates older graph tokens.
topology_divergent_change="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  change create --repo "$destination" \
  --operation-id "change-operation:decision-matrix-divergent-create" \
  --nonce "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd" \
  | jq -er '.changeId')"
pointbreak_actor_json "actor:agent:pointbreak-matrix-topology-writer" \
  change join "$topology_divergent_change" "$topology_root_revision" \
  --repo "$destination" \
  --operation-id "change-operation:decision-matrix-divergent-join-root" >/dev/null
pointbreak_actor_json "actor:agent:pointbreak-matrix-topology-writer" \
  change join "$topology_divergent_change" "$topology_left_revision" \
  --repo "$destination" \
  --operation-id "change-operation:decision-matrix-divergent-join-left" >/dev/null
pointbreak_actor_json "actor:agent:pointbreak-matrix-topology-writer" \
  change assert-relation "$topology_divergent_change" \
  "$topology_left_revision" "$topology_root_revision" \
  --successor-artifact-hash "$topology_left_artifact" \
  --predecessor-artifact-hash "$topology_root_artifact" \
  --operation-id "change-operation:decision-matrix-divergent-left-root" \
  --repo "$destination" >/dev/null
topology_divergent_cursor="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  change select "$topology_divergent_change" \
  --revision "$topology_left_revision" --source captured --repo "$destination" \
  | jq -er '.token')"

printf 'pub fn topology_value() -> u32 { 3 }\n' > "$destination/src/topology.rs"
topology_right_capture="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  capture --repo "$destination" --summary "Topology divergent matrix" \
  --review-cursor "$topology_divergent_cursor" --advance parallel)"
topology_right_revision="$(printf '%s\n' "$topology_right_capture" | jq -er '.revision.revisionId')"
topology_right_artifact="$(printf '%s\n' "$topology_right_capture" | jq -er '.revision.objectArtifactContentHash')"
pointbreak_actor_json "actor:agent:pointbreak-matrix-topology-writer" \
  change assert-relation "$topology_divergent_change" \
  "$topology_right_revision" "$topology_root_revision" \
  --successor-artifact-hash "$topology_right_artifact" \
  --predecessor-artifact-hash "$topology_root_artifact" \
  --operation-id "change-operation:decision-matrix-divergent-right-root" \
  --repo "$destination" >/dev/null

# A relation-free Change over B and C preserves a pure parallel-current row.
topology_parallel_change="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  change create --repo "$destination" \
  --operation-id "change-operation:decision-matrix-parallel-create" \
  --nonce "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" \
  | jq -er '.changeId')"
pointbreak_actor_json "actor:agent:pointbreak-matrix-topology-writer" \
  change join "$topology_parallel_change" "$topology_left_revision" \
  --repo "$destination" \
  --operation-id "change-operation:decision-matrix-parallel-join-left" >/dev/null
pointbreak_actor_json "actor:agent:pointbreak-matrix-topology-writer" \
  change join "$topology_parallel_change" "$topology_right_revision" \
  --repo "$destination" \
  --operation-id "change-operation:decision-matrix-parallel-join-right" >/dev/null

# A fourth Change starts with the same B/C current pair and captures one
# Revision that atomically supersedes both exact predecessors.
topology_consolidation_change="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  change create --repo "$destination" \
  --operation-id "change-operation:decision-matrix-consolidation-create" \
  --nonce "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc" \
  | jq -er '.changeId')"
pointbreak_actor_json "actor:agent:pointbreak-matrix-topology-writer" \
  change join "$topology_consolidation_change" "$topology_left_revision" \
  --repo "$destination" \
  --operation-id "change-operation:decision-matrix-consolidation-join-left" >/dev/null
pointbreak_actor_json "actor:agent:pointbreak-matrix-topology-writer" \
  change join "$topology_consolidation_change" "$topology_right_revision" \
  --repo "$destination" \
  --operation-id "change-operation:decision-matrix-consolidation-join-right" >/dev/null
topology_consolidation_cursor="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  change select "$topology_consolidation_change" \
  --revision "$topology_left_revision" --source captured --repo "$destination" \
  | jq -er '.token')"
printf 'pub fn topology_value() -> u32 { 4 }\n' > "$destination/src/topology.rs"
topology_consolidated_capture="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  capture --repo "$destination" --summary "Topology consolidation matrix" \
  --review-cursor "$topology_consolidation_cursor" --advance replace \
  --also-supersedes "$topology_right_revision@$topology_right_artifact")"
topology_consolidated_revision="$(printf '%s\n' "$topology_consolidated_capture" | jq -er '.revision.revisionId')"
topology_consolidated_artifact="$(printf '%s\n' "$topology_consolidated_capture" | jq -er '.revision.objectArtifactContentHash')"
git -C "$destination" reset --quiet --hard main

git -C "$destination" switch --quiet -c feat/competing-heads
printf 'pub fn matrix_value() -> u32 { 6 }\n' > "$destination/src/lib.rs"
superseded_capture="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-capture-writer" \
  capture --repo "$destination" --summary "Supersession root")"
superseded_revision="$(printf '%s\n' "$superseded_capture" | jq -er '.revision.revisionId')"
superseded_change="$(printf '%s\n' "$superseded_capture" | jq -er '.changeId')"
superseded_cursor="$(printf '%s\n' "$superseded_capture" | jq -er '.reviewCursor.token')"
superseded_artifact="$(printf '%s\n' "$superseded_capture" | jq -er '.revision.objectArtifactContentHash')"
pointbreak_actor_json "actor:agent:pointbreak-matrix-fact-writer" \
  observation add --repo "$destination" --exact-revision "$superseded_revision" \
  --track "agent:matrix-facts" --title "Stale predecessor fact" \
  --body "This fact remains on the addressed predecessor." >/dev/null

printf 'pub fn matrix_value() -> u32 { 7 }\n' > "$destination/src/lib.rs"
ambiguous_assessment_capture="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-capture-writer" \
  capture --repo "$destination" --summary "Competing head A" \
  --review-cursor "$superseded_cursor" --advance replace)"
ambiguous_assessment_revision="$(printf '%s\n' "$ambiguous_assessment_capture" | jq -er '.revision.revisionId')"
ambiguous_assessment_cursor="$(printf '%s\n' "$ambiguous_assessment_capture" | jq -er '.reviewCursor.token')"
pointbreak_actor_json "actor:agent:pointbreak-matrix-assessment-writer-one" \
  assessment add --repo "$destination" \
  --exact-revision "$ambiguous_assessment_revision" \
  --track "agent:matrix-assessment-a" --assessment accepted \
  --summary "Candidate A accepts." >/dev/null
pointbreak_actor_json "actor:agent:pointbreak-matrix-assessment-writer-two" \
  assessment add --repo "$destination" \
  --exact-revision "$ambiguous_assessment_revision" \
  --track "agent:matrix-assessment-b" --assessment needs-changes \
  --summary "Candidate B requests changes." >/dev/null

printf 'pub fn matrix_value() -> u32 { 8 }\n' > "$destination/src/lib.rs"
competing_capture="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-capture-writer" \
  capture --repo "$destination" --summary "Competing head B" \
  --review-cursor "$ambiguous_assessment_cursor" --advance parallel)"
competing_revision="$(printf '%s\n' "$competing_capture" | jq -er '.revision.revisionId')"
competing_artifact="$(printf '%s\n' "$competing_capture" | jq -er '.revision.objectArtifactContentHash')"
pointbreak_actor_json "actor:agent:pointbreak-matrix-capture-writer" \
  change assert-relation "$superseded_change" "$competing_revision" "$superseded_revision" \
  --successor-artifact-hash "$competing_artifact" \
  --predecessor-artifact-hash "$superseded_artifact" \
  --operation-id "change-operation:decision-matrix-competing-relation" \
  --repo "$destination" >/dev/null
git -C "$destination" reset --quiet --hard main

git -C "$destination" switch --quiet -c feat/source-matrix
printf 'pub fn matrix_value() -> u32 { 9 }\n' > "$destination/src/lib.rs"
git -C "$destination" add --all
git -C "$destination" commit --quiet -m "range matrix target"
range_revision="$(capture_revision \
  "actor:agent:pointbreak-matrix-capture-writer" \
  --base HEAD~1 --target HEAD --summary "Range matrix")"
root_revision="$(capture_revision \
  "actor:agent:pointbreak-matrix-capture-writer" \
  --root --target HEAD --summary "Root matrix")"

printf 'pub fn staged_value() -> u32 { 10 }\n' > "$destination/src/staged.rs"
git -C "$destination" add src/staged.rs
staged_revision="$(capture_revision \
  "actor:agent:pointbreak-matrix-capture-writer" \
  --staged --summary "Staged matrix")"
git -C "$destination" branch second-current-ref HEAD
pointbreak_actor_json "actor:agent:pointbreak-matrix-association-writer" \
  association record --repo "$destination" --revision "$staged_revision" \
  --track "agent:matrix-associations" --ref second-current-ref \
  --head "$(git -C "$destination" rev-parse HEAD)" >/dev/null

git -C "$destination" reset --quiet HEAD -- src/staged.rs
rm "$destination/src/staged.rs"
printf 'pub fn matrix_value() -> u32 { 10 }\n' > "$destination/src/lib.rs"
unstaged_revision="$(capture_revision \
  "actor:agent:pointbreak-matrix-capture-writer" \
  --unstaged --summary "Unstaged matrix")"
git -C "$destination" reset --quiet --hard HEAD

git -C "$destination" switch --quiet --detach HEAD
printf 'pub fn matrix_value() -> u32 { 11 }\n' > "$destination/src/lib.rs"
detached_revision="$(capture_revision \
  "actor:agent:pointbreak-matrix-capture-writer" \
  --summary "Detached worktree matrix")"
git -C "$destination" reset --quiet --hard HEAD

git -C "$destination" switch --quiet -c feat/missing-object main
printf 'pub fn matrix_value() -> u32 { 12 }\n' > "$destination/src/lib.rs"
git -C "$destination" add --all
git -C "$destination" commit --quiet -m "missing object matrix target"
missing_commit="$(git -C "$destination" rev-parse HEAD)"
missing_capture="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-capture-writer" \
  capture --repo "$destination" --base HEAD~1 --target HEAD \
  --summary "Missing object matrix")"
missing_change="$(printf '%s\n' "$missing_capture" | jq -er '.changeId')"
missing_revision="$(printf '%s\n' "$missing_capture" | jq -er '.revision.revisionId')"
missing_artifact="$(printf '%s\n' "$missing_capture" | jq -er '.revision.objectArtifactContentHash')"
git -C "$destination" switch --quiet main
git -C "$destination" branch --delete --force feat/missing-object >/dev/null
git -C "$destination" reflog expire --expire=now --all
git_object_dir="$(git -C "$destination" rev-parse --path-format=absolute --git-path objects)"
missing_object_path="$git_object_dir/${missing_commit:0:2}/${missing_commit:2}"
[ -f "$missing_object_path" ] || die "expected a loose synthetic commit object"
rm "$missing_object_path"

store_paths="$(pointbreak_json store paths --repo "$destination")"
common_store="$(printf '%s\n' "$store_paths" | jq -er '.commonStore')"
common_store_for_comparison="$(normalize_for_shell_comparison "$common_store")"
case "$common_store_for_comparison" in
  "$destination"/*) ;;
  *) die "generated store escaped the isolated repository: $common_store" ;;
esac

jq -n \
  --arg primary_change "$primary_change" \
  --arg primary_revision "$primary_revision" \
  --arg primary_artifact "$primary_artifact" \
  --arg live_revision "$live_revision" \
  --arg unassessed_revision "$unassessed_revision" \
  --arg superseded_revision "$superseded_revision" \
  --arg ambiguous_assessment_revision "$ambiguous_assessment_revision" \
  --arg competing_revision "$competing_revision" \
  --arg range_revision "$range_revision" \
  --arg root_revision "$root_revision" \
  --arg staged_revision "$staged_revision" \
  --arg unstaged_revision "$unstaged_revision" \
  --arg detached_revision "$detached_revision" \
  --arg missing_change "$missing_change" \
  --arg missing_revision "$missing_revision" \
  --arg missing_artifact "$missing_artifact" \
  --arg topology_root_revision "$topology_root_revision" \
  --arg topology_root_artifact "$topology_root_artifact" \
  --arg topology_replacement_change "$topology_replacement_change" \
  --arg topology_left_revision "$topology_left_revision" \
  --arg topology_left_artifact "$topology_left_artifact" \
  --arg topology_parallel_change "$topology_parallel_change" \
  --arg topology_divergent_change "$topology_divergent_change" \
  --arg topology_right_revision "$topology_right_revision" \
  --arg topology_right_artifact "$topology_right_artifact" \
  --arg topology_consolidation_change "$topology_consolidation_change" \
  --arg topology_consolidated_revision "$topology_consolidated_revision" \
  --arg topology_consolidated_artifact "$topology_consolidated_artifact" \
  --arg base_commit "$base_commit" \
  --arg first_landing "$first_landing" \
  --arg second_landing "$second_landing" \
  --arg live_landing "$live_landing" \
  '{
    primary_revision: $primary_revision,
    live_revision: $live_revision,
    unassessed_revision: $unassessed_revision,
    superseded_revision: $superseded_revision,
    ambiguous_assessment_revision: $ambiguous_assessment_revision,
    competing_revision: $competing_revision,
    range_revision: $range_revision,
    root_revision: $root_revision,
    staged_revision: $staged_revision,
    unstaged_revision: $unstaged_revision,
    detached_revision: $detached_revision,
    missing_change: $missing_change,
    missing_revision: $missing_revision,
    missing_artifact: $missing_artifact,
    topology: {
      initial: {
        change: $primary_change,
        current: {revision: $primary_revision, artifact: $primary_artifact}
      },
      replacement: {
        change: $topology_replacement_change,
        current: {revision: $topology_left_revision, artifact: $topology_left_artifact},
        predecessor: {revision: $topology_root_revision, artifact: $topology_root_artifact}
      },
      parallel_current: {
        change: $topology_parallel_change,
        current: [
          {revision: $topology_left_revision, artifact: $topology_left_artifact},
          {revision: $topology_right_revision, artifact: $topology_right_artifact}
        ]
      },
      replacement_divergent: {
        change: $topology_divergent_change,
        current: [
          {revision: $topology_left_revision, artifact: $topology_left_artifact},
          {revision: $topology_right_revision, artifact: $topology_right_artifact}
        ]
      },
      consolidation: {
        change: $topology_consolidation_change,
        current: {revision: $topology_consolidated_revision, artifact: $topology_consolidated_artifact},
        predecessors: [
          {revision: $topology_left_revision, artifact: $topology_left_artifact},
          {revision: $topology_right_revision, artifact: $topology_right_artifact}
        ]
      }
    },
    shared_revision: {
      revision: $topology_left_revision,
      artifact: $topology_left_artifact,
      changes: [
        $topology_replacement_change,
        $topology_divergent_change,
        $topology_parallel_change,
        $topology_consolidation_change
      ]
    },
    base_commit: $base_commit,
    first_landing: $first_landing,
    second_landing: $second_landing,
    live_landing: $live_landing
  }'
