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
  if [ -n "$cygpath_program" ]; then
    "$cygpath_program" -u "$native_path"
  else
    printf '%s\n' "$native_path"
  fi
}

resolve_program() {
  local requested="$1"
  local explicit="$2"
  if [ -n "$explicit" ]; then
    case "$explicit" in
      /* | [A-Za-z]:/* | [A-Za-z]:\\* | \\\\*) ;;
      *) die "$explicit must be an absolute program path" ;;
    esac
  fi
  command -v "$requested" 2>/dev/null || die "$requested is required"
}

git_program="$(resolve_program "${POINTBREAK_GIT_PROGRAM:-git}" "${POINTBREAK_GIT_PROGRAM:-}")"
jq_program="$(resolve_program "${POINTBREAK_JQ_PROGRAM:-jq}" "${POINTBREAK_JQ_PROGRAM:-}")"
find_program="$(resolve_program "${POINTBREAK_FIND_PROGRAM:-find}" "${POINTBREAK_FIND_PROGRAM:-}")"
sort_program="$(resolve_program "${POINTBREAK_SORT_PROGRAM:-sort}" "${POINTBREAK_SORT_PROGRAM:-}")"
wc_program="$(resolve_program "${POINTBREAK_WC_PROGRAM:-wc}" "${POINTBREAK_WC_PROGRAM:-}")"
tr_program="$(resolve_program "${POINTBREAK_TR_PROGRAM:-tr}" "${POINTBREAK_TR_PROGRAM:-}")"
awk_program="$(resolve_program "${POINTBREAK_AWK_PROGRAM:-awk}" "${POINTBREAK_AWK_PROGRAM:-}")"
cp_program="$(resolve_program "${POINTBREAK_CP_PROGRAM:-cp}" "${POINTBREAK_CP_PROGRAM:-}")"
head_program="$(resolve_program "${POINTBREAK_HEAD_PROGRAM:-head}" "${POINTBREAK_HEAD_PROGRAM:-}")"
dirname_program="$(resolve_program "${POINTBREAK_DIRNAME_PROGRAM:-dirname}" "${POINTBREAK_DIRNAME_PROGRAM:-}")"
mkdir_program="$(resolve_program "${POINTBREAK_MKDIR_PROGRAM:-mkdir}" "${POINTBREAK_MKDIR_PROGRAM:-}")"
rm_program="$(resolve_program "${POINTBREAK_RM_PROGRAM:-rm}" "${POINTBREAK_RM_PROGRAM:-}")"
hash_program_request="${POINTBREAK_HASH_PROGRAM:-${POINTBREAK_SHASUM_PROGRAM:-}}"
hash_program_mode="${POINTBREAK_HASH_PROGRAM_MODE:-}"
if [ -n "$hash_program_request" ]; then
  hash_program="$(resolve_program "$hash_program_request" "$hash_program_request")"
  case "$hash_program_mode" in
    shasum|sha256sum) hash_mode="$hash_program_mode" ;;
    *) die "POINTBREAK_HASH_PROGRAM_MODE must be shasum or sha256sum with POINTBREAK_HASH_PROGRAM" ;;
  esac
elif [ -n "$hash_program_mode" ]; then
  die "POINTBREAK_HASH_PROGRAM_MODE requires POINTBREAK_HASH_PROGRAM"
elif command -v sha256sum >/dev/null 2>&1; then
  hash_program="$(command -v sha256sum)"
  hash_mode="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
  hash_program="$(command -v shasum)"
  hash_mode="shasum"
else
  die "sha256sum or shasum is required"
fi

# An unset binding preserves normal recipe behavior: use cygpath when it is
# available. Diagnostic callers set this explicitly to either a fixed absolute
# executable or `absent`, which prevents PATH discovery from entering their
# witness calculation.
cygpath_program=""
if [ "${POINTBREAK_CYGPATH_PROGRAM+x}" = x ]; then
  case "${POINTBREAK_CYGPATH_PROGRAM}" in
    absent) ;;
    /* | [A-Za-z]:/* | [A-Za-z]:\\* | \\\\*)
      cygpath_program="$(resolve_program "$POINTBREAK_CYGPATH_PROGRAM" "$POINTBREAK_CYGPATH_PROGRAM")"
      ;;
    *) die "POINTBREAK_CYGPATH_PROGRAM must be an absolute program path or absent" ;;
  esac
elif command -v cygpath >/dev/null 2>&1; then
  cygpath_program="$(command -v cygpath)"
fi

git() { "$git_program" "$@"; }
jq() { "$jq_program" "$@"; }
find() { "$find_program" "$@"; }
sort() { "$sort_program" "$@"; }
wc() { "$wc_program" "$@"; }
tr() { "$tr_program" "$@"; }
awk() { "$awk_program" "$@"; }
cp() { "$cp_program" "$@"; }
head() { "$head_program" "$@"; }
dirname() { "$dirname_program" "$@"; }
mkdir() { "$mkdir_program" "$@"; }
rm() { "$rm_program" "$@"; }

sha256_file() {
  if [ "$hash_mode" = "sha256sum" ]; then
    "$hash_program" -- "$1" | awk '{print $1}'
  else
    "$hash_program" -a 256 -- "$1" | awk '{print $1}'
  fi
}

sha256_stdin() {
  if [ "$hash_mode" = "sha256sum" ]; then
    "$hash_program" | awk '{print $1}'
  else
    "$hash_program" -a 256 | awk '{print $1}'
  fi
}

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
  record_validation_json "$@" >/dev/null
}

record_validation_json() {
  local revision="$1"
  local check_name="$2"
  local status="$3"
  local completed_at="$4"
  pointbreak_actor_json "actor:agent:pointbreak-matrix-validation-writer" \
    validation add --repo "$destination" --exact-revision "$revision" \
    --track "agent:matrix-validation" --check-name "$check_name" \
    --status "$status" --completed-at "$completed_at"
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

# Seed the public historical-reader compatibility records that have no current
# review CLI writer. They are deterministic outputs of the checked-in D0
# generator, while the retired imported-note pair comes from its existing
# byte-faithful legacy-store fixture. These records remain visible inputs to the
# public Inspector matrix rather than qualification-runner injections.
timeline_compat_store="${POINTBREAK_TIMELINE_COMPAT_FIXTURE_DIR:-$repo_root/tests/support/assets/inspector-timeline-compat-v1}"
legacy_note_store="${POINTBREAK_LEGACY_NOTE_FIXTURE_DIR:-$repo_root/tests/fixtures/legacy_stores/review_note_imported/store}"
[ -d "$timeline_compat_store" ] || die "public Timeline compatibility fixture is missing"
[ "$(find "$timeline_compat_store" -maxdepth 1 -type f -name '*.json' | wc -l | tr -d '[:space:]')" -eq 9 ] \
  || die "public Timeline compatibility fixture event count drifted"
for legacy_record in \
  "$legacy_note_store/events/82828b3ccf26612a9830837a3260291b95c5b4aa13451cad3c7dd271262ecd27.json" \
  "$legacy_note_store/events/f8bf79cecc3306f874829afe7b684feffb3dd0ddce388fa8bbadbe1d88044bb0.json" \
  "$legacy_note_store/artifacts/objects/d18e06368b6a96f788dc110e3628646847bc0a7367e7027e1530f6e30312afa0.json"; do
  [ -f "$legacy_record" ] || die "public imported-note compatibility record is missing"
done
cp "$timeline_compat_store"/*.json "$destination/.git/pointbreak/events/"
cp "$legacy_note_store/events/82828b3ccf26612a9830837a3260291b95c5b4aa13451cad3c7dd271262ecd27.json" \
  "$legacy_note_store/events/f8bf79cecc3306f874829afe7b684feffb3dd0ddce388fa8bbadbe1d88044bb0.json" \
  "$destination/.git/pointbreak/events/"
mkdir -p "$destination/.git/pointbreak/artifacts/notes" \
  "$destination/.git/pointbreak/artifacts/objects"
cp "$timeline_compat_store/artifacts/notes"/*.json \
  "$destination/.git/pointbreak/artifacts/notes/"
cp "$legacy_note_store/artifacts/objects/d18e06368b6a96f788dc110e3628646847bc0a7367e7027e1530f6e30312afa0.json" \
  "$destination/.git/pointbreak/artifacts/objects/"

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

signed_validation_result="$(record_validation_json \
  "$primary_revision" "passed current" passed "2026-07-17T10:00:00Z")"
signed_validation_event="$(printf '%s\n' "$signed_validation_result" | jq -er '.eventId')"
unsigned_validation_result="$(POINTBREAK_SIGNING=off record_validation_json \
  "$primary_revision" "unsigned trust witness" passed "2026-07-17T10:00:00Z")"
unsigned_validation_event="$(printf '%s\n' "$unsigned_validation_result" | jq -er '.eventId')"
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
primary_landing_selection="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-association-writer" \
  change select "$primary_change" --revision "$primary_revision" \
  --source "commit:$first_landing" --repo "$destination")"
primary_landing_cursor="$(printf '%s\n' "$primary_landing_selection" | jq -er '.token')"
first_land_result="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-association-writer" \
  association land --repo "$destination" --review-cursor "$primary_landing_cursor" \
  --track "agent:matrix-associations" --commit "$first_landing")"
first_commit_association="$(printf '%s\n' "$first_land_result" | jq -er '.commitAssociationId')"
relation_attestation_id="$(printf '%s\n' "$first_land_result" | jq -er '.relationAttestationId')"

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

# Port one exact fact while the primary Change still has a resolvable request
# state. The second conflicting response below deliberately makes that Change
# ambiguous only after the public selector and write path have revalidated the
# exact target.
fact_port_origin_revision="$topology_left_revision"
fact_port_origin_artifact="$topology_left_artifact"
fact_port_origin_id="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-fact-writer" \
  observation add --repo "$destination" --exact-revision "$fact_port_origin_revision" \
  --track "agent:matrix-facts" --title "Decision context origin" \
  --body "This exact observation is relationship-only context for the primary Revision." \
  | jq -er '.observationId')"
fact_port_target_selection="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-fact-writer" \
  change select "$primary_change" --revision "$primary_revision" --source captured \
  --repo "$destination")"
fact_port_cursor="$(printf '%s\n' "$fact_port_target_selection" | jq -er '.token')"
fact_port_result="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-fact-writer" \
  fact port --repo "$destination" \
  --origin-revision "$fact_port_origin_revision@$fact_port_origin_artifact" \
  --origin-fact "$fact_port_origin_id" --review-cursor "$fact_port_cursor" \
  --relation context-only --track "agent:matrix-facts")"
fact_port_id="$(printf '%s\n' "$fact_port_result" | jq -er '.portId')"
fact_port_event="$(printf '%s\n' "$fact_port_result" | jq -er '.eventId')"
pointbreak_actor_json "actor:agent:pointbreak-matrix-response-two" \
  input-request respond "$ambiguous_request" --repo "$destination" \
  --outcome rejected --reason "second response rejects" >/dev/null

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

# Exercise a historical relation withdrawal without changing the final
# parallel-current topology.
temporary_relation_assertion="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  change assert-relation "$topology_parallel_change" \
  "$topology_right_revision" "$topology_left_revision" \
  --successor-artifact-hash "$topology_right_artifact" \
  --predecessor-artifact-hash "$topology_left_artifact" \
  --operation-id "change-operation:decision-matrix-temporary-relation" \
  --repo "$destination")"
temporary_relation_assertion_event="$(printf '%s\n' "$temporary_relation_assertion" \
  | jq -er '.events[0].eventId')"
temporary_relation_claim="$(pointbreak_json change show "$topology_parallel_change" \
  --repo "$destination" | jq -er '.relationClaims[] | select(.active == true) | .claimId')"
temporary_relation_withdrawal="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  change withdraw-relation "$temporary_relation_claim" \
  --repo "$destination" \
  --operation-id "change-operation:decision-matrix-temporary-relation-withdrawal")"
temporary_relation_withdrawal_event="$(printf '%s\n' "$temporary_relation_withdrawal" \
  | jq -er '.events[0].eventId')"

# A disposable Change supplies one membership withdrawal and one Change link;
# its empty final membership leaves the five browser topology rows untouched.
historical_change="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  change create --repo "$destination" \
  --operation-id "change-operation:decision-matrix-historical-create" \
  --nonce "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" \
  | jq -er '.changeId')"
pointbreak_actor_json "actor:agent:pointbreak-matrix-topology-writer" \
  change join "$historical_change" "$primary_revision" \
  --repo "$destination" \
  --operation-id "change-operation:decision-matrix-historical-join" >/dev/null
historical_membership_claim="$(pointbreak_json change show "$historical_change" \
  --repo "$destination" | jq -er '.membershipClaims[] | select(.active == true) | .claimId')"
historical_membership_withdrawal="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  change withdraw-membership "$historical_membership_claim" \
  --repo "$destination" \
  --operation-id "change-operation:decision-matrix-historical-withdrawal")"
historical_membership_withdrawal_event="$(printf '%s\n' "$historical_membership_withdrawal" \
  | jq -er '.events[0].eventId')"
historical_change_link="$(pointbreak_actor_json \
  "actor:agent:pointbreak-matrix-topology-writer" \
  change link "$historical_change" "$primary_change" --relation related-work \
  --repo "$destination" \
  --operation-id "change-operation:decision-matrix-historical-link")"
historical_change_link_event="$(printf '%s\n' "$historical_change_link" \
  | jq -er '.events[0].eventId')"

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
git_object_dir="$(cd "$git_object_dir" && pwd -P)"
missing_object_path="$git_object_dir/${missing_commit:0:2}/${missing_commit:2}"
case "$missing_object_path" in
  "$git_object_dir"/*) ;;
  *) die "synthetic commit object escaped the disposable Git object directory" ;;
esac
if [ -e "$missing_object_path" ] || [ -L "$missing_object_path" ]; then
  [ -f "$missing_object_path" ] || die "synthetic commit object is not a regular file"
  [ ! -L "$missing_object_path" ] || die "synthetic commit object must not be a symlink"
  # `-f` makes this removal step retry-safe if another cleanup wins after the
  # checks above. The final object-database probe remains the authority.
  rm -f -- "$missing_object_path"
elif git -C "$destination" cat-file -e "$missing_commit^{commit}" 2>/dev/null; then
  die "synthetic commit is still readable but is not the expected loose object"
fi
if git -C "$destination" cat-file -e "$missing_commit^{commit}" 2>/dev/null; then
  die "synthetic commit remained readable after missing-object materialization"
fi

store_paths="$(pointbreak_json store paths --repo "$destination")"
common_store="$(printf '%s\n' "$store_paths" | jq -er '.commonStore')"
common_store_for_comparison="$(normalize_for_shell_comparison "$common_store")"
case "$common_store_for_comparison" in
  "$destination"/*) ;;
  *) die "generated store escaped the isolated repository: $common_store" ;;
esac

# Hash the same complete authoritative file inventory used by the qualification
# runner. Only the two governed top-level disposable namespaces are excluded;
# lookalikes or nested names remain authoritative.
is_governed_derived_entry() {
  local name="$1"
  local path="$2"
  if [ -d "$path" ]; then
    case "$name" in
      derived|.pointbreak-derived) return 0 ;;
    esac
    [[ "$name" =~ ^(derived|\.pointbreak-derived)\.(quarantine|retired)-[0-9]+-[0-9]+$ ]]
    return
  fi
  if [ -f "$path" ]; then
    case "$name" in
      derived.writer.lock|derived.rebuild.lock|.pointbreak-derived.writer.lock|.pointbreak-derived.rebuild.lock)
        return 0
        ;;
    esac
    [[ "$name" =~ ^(derived|\.pointbreak-derived)\.generation-lease-.+\.lock$ ]]
    return
  fi
  return 1
}

while IFS= read -r path; do
  relative_path="${path#"$common_store_for_comparison"/}"
  top_level_name="${relative_path%%/*}"
  if ! is_governed_derived_entry \
    "$top_level_name" "$common_store_for_comparison/$top_level_name"; then
    die "decision matrix authoritative inventory rejects non-file path: $relative_path"
  fi
done < <(find "$common_store_for_comparison" ! -type d ! -type f -print | LC_ALL=C sort)
inventory_rows="$(
  while IFS= read -r file; do
    relative_path="${file#"$common_store_for_comparison"/}"
    top_level_name="${relative_path%%/*}"
    if is_governed_derived_entry \
      "$top_level_name" "$common_store_for_comparison/$top_level_name"; then
      continue
    fi
    byte_count="$(wc -c < "$file" | tr -d '[:space:]')"
    file_sha256="$(sha256_file "$file")"
    jq -cnS \
      --arg relativePath "$relative_path" \
      --argjson bytes "$byte_count" \
      --arg sha256 "$file_sha256" \
      '{relativePath: $relativePath, bytes: $bytes, sha256: $sha256}'
  done < <(find "$common_store_for_comparison" -type f -print | LC_ALL=C sort)
)"
authoritative_inventory="$(printf '%s\n' "$inventory_rows" | jq -csS '.')"
authoritative_inventory_sha256="$(printf '%s' "$authoritative_inventory" | sha256_stdin)"
[[ "$authoritative_inventory_sha256" =~ ^[0-9a-f]{64}$ ]] \
  || die "decision matrix authoritative inventory hash is invalid"

jq -n \
  --arg authoritative_inventory_sha256 "$authoritative_inventory_sha256" \
  --arg primary_change "$primary_change" \
  --arg primary_revision "$primary_revision" \
  --arg primary_artifact "$primary_artifact" \
  --arg fact_port_origin_revision "$fact_port_origin_revision" \
  --arg fact_port_origin_artifact "$fact_port_origin_artifact" \
  --arg fact_port_origin_id "$fact_port_origin_id" \
  --arg fact_port_id "$fact_port_id" \
  --arg fact_port_event "$fact_port_event" \
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
  --arg signed_validation_event "$signed_validation_event" \
  --arg unsigned_validation_event "$unsigned_validation_event" \
  --arg relation_attestation_id "$relation_attestation_id" \
  --arg historical_membership_withdrawal_event "$historical_membership_withdrawal_event" \
  --arg historical_change_link_event "$historical_change_link_event" \
  --arg temporary_relation_assertion_event "$temporary_relation_assertion_event" \
  --arg temporary_relation_withdrawal_event "$temporary_relation_withdrawal_event" \
  --arg base_commit "$base_commit" \
  --arg first_landing "$first_landing" \
  --arg second_landing "$second_landing" \
  --arg live_landing "$live_landing" \
  '{
    schema: "pointbreak.qualification-derived-change-fixture-witness.v1",
    fixtureId: "topology-v1",
    authoritativeInventorySha256: $authoritative_inventory_sha256,
    storageForbiddenProbeHashes: {
      proposalSummarySha256: "21f749c5f166ae819a99a8ff0e303297a43685fd14cc7f1b86a90751989b167c",
      proseSha256: "da79cc8c9b04f41616275f4a6bd027acf6d0358f3605dac74ccadfeea92945a4",
      payloadDocumentSha256: "20dfd0d4e1ce81bfb753001a61c0394914d4711e84f90fb745a659dba1ff11bf"
    },
    timeline: {
      trust: {
        signedEvent: $signed_validation_event,
        unsignedEvent: $unsigned_validation_event
      },
      historicalCompatibility: {
        relationAttestation: $relation_attestation_id,
        membershipWithdrawalEvent: $historical_membership_withdrawal_event,
        changeLinkEvent: $historical_change_link_event,
        relationAssertionEvent: $temporary_relation_assertion_event,
        relationWithdrawalEvent: $temporary_relation_withdrawal_event
      }
    },
    primary_revision: $primary_revision,
    fact_port: {
      port_id: $fact_port_id,
      event_id: $fact_port_event,
      origin: {
        revision: $fact_port_origin_revision,
        artifact: $fact_port_origin_artifact,
        observation: $fact_port_origin_id
      }
    },
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
