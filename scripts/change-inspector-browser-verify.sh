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

for command in git jq node rg shasum find wc tr mv curl; do
  command -v "$command" >/dev/null 2>&1 || die "$command is required"
done

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
browser_program_template="$script_dir/change-inspector-browser-verify.mjs"
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
[ -f "$browser_program_template" ] || die "browser program is missing: $browser_program_template"
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

source_commit="$(git -C "$repo_root" rev-parse HEAD)"
binary_sha256="$(shasum -a 256 "$pointbreak_binary" | awk '{print $1}')"
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

POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change select \
  "$historical_change" --revision "$shared_revision" --source captured \
  --repo "$fixture_repo" --format json >"$log_dir/fact-port-target-cursor.json"
fact_port_cursor="$(jq -er '.token' "$log_dir/fact-port-target-cursor.json")"
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" fact port --repo "$fixture_repo" \
    --origin-revision "$primary_revision@$primary_artifact" \
    --origin-fact "$correction_origin_id" --review-cursor "$fact_port_cursor" \
    --relation context-only --track "agent:browser-history-cases" --format json \
    >"$log_dir/fact-port.json" 2>"$log_dir/fact-port.log"

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

# Exercise the real multi-writer timestamp path instead of inventing a raw
# record. A bounded burst makes a same-millisecond pair observable while every
# event remains a valid supported observation write. The exact pair is derived
# and asserted below after the projection is rebuilt.
equal_timestamp_pids=()
for ordinal in $(seq 1 16); do
  POINTBREAK_HOME="$pointbreak_home" \
    POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
    "$pointbreak_binary" observation add --repo "$fixture_repo" \
      --exact-revision "$primary_revision" --track "agent:browser-equal-time" \
      --title "Browser equal-time writer $ordinal" \
      --idempotency-key "browser-equal-time-$ordinal-v1" --format json \
      >"$log_dir/equal-time-$ordinal.json" 2>"$log_dir/equal-time-$ordinal.log" &
  equal_timestamp_pids+=("$!")
done
for equal_timestamp_pid in "${equal_timestamp_pids[@]}"; do
  wait "$equal_timestamp_pid" \
    || die "a supported concurrent equal-timestamp fixture write failed"
done

POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" store derived build \
  --repo "$fixture_repo" --format json \
  >"$log_dir/derived-build.json" 2>"$log_dir/derived-build.log"
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" history \
  --repo "$fixture_repo" --track "agent:browser-equal-time" --format json \
  >"$log_dir/equal-time-history.json"
equal_timestamp_pair="$(jq -ce '
  [.entries[] | {eventId, occurredAt}]
  | group_by(.occurredAt)
  | map(select(length >= 2))
  | first
  | select(. != null)
  | {occurredAt: .[0].occurredAt, tieBreak: "event_id_asc",
      eventIds: ([.[0].eventId, .[1].eventId] | sort)}
' "$log_dir/equal-time-history.json")" \
  || die "supported concurrent writers did not produce an equal-occurredAt pair"
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
fact_port_event="$(jq -er '.eventId' "$log_dir/fact-port.json")"
fact_port_id="$(jq -er '.portId' "$log_dir/fact-port.json")"
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
ready_store="$repo_root/tests/support/assets/change-ready-store"
activation_record="$ready_store/5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json"
completion_record="$ready_store/f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json"
[ -f "$activation_record" ] && [ -f "$completion_record" ] \
  || die "public reader-state activation fixtures are unavailable"
cp "$activation_record" "$completion_record" "$reader_empty_l2_repo/.pointbreak/data/events/"
cp "$activation_record" "$reader_m1_repo/.pointbreak/data/events/"

server_pid=""
reader_empty_l2_pid=""
reader_l0_pid=""
reader_m1_pid=""
timeline_append_pid=""
session="pointbreak-change-browser-$$"
if [ -n "${PLAYWRIGHT_CLI:-}" ]; then
  pwcli=("$PLAYWRIGHT_CLI")
elif command -v playwright-cli >/dev/null 2>&1; then
  pwcli=(playwright-cli)
else
  command -v npx >/dev/null 2>&1 || die "playwright-cli and npx are unavailable"
  pwcli=(npx --yes --package @playwright/cli@0.1.17 playwright-cli)
fi

run_pw() {
  (cd "$artifact_dir" && "${pwcli[@]}" -s="$session" "$@")
}

cleanup() {
  run_pw close >"$log_dir/browser-close.log" 2>&1 || true
  if [ -n "$timeline_append_pid" ] && kill -0 "$timeline_append_pid" >/dev/null 2>&1; then
    kill "$timeline_append_pid" >/dev/null 2>&1 || true
    wait "$timeline_append_pid" >/dev/null 2>&1 || true
  fi
  [ -z "$server_pid" ] || kill "$server_pid" >/dev/null 2>&1 || true
  [ -z "$reader_empty_l2_pid" ] \
    || kill "$reader_empty_l2_pid" >/dev/null 2>&1 || true
  [ -z "$reader_l0_pid" ] || kill "$reader_l0_pid" >/dev/null 2>&1 || true
  [ -z "$reader_m1_pid" ] || kill "$reader_m1_pid" >/dev/null 2>&1 || true
}
trap cleanup EXIT

start_reader_state_server() {
  local state="$1"
  local repo="$2"
  local startup="$log_dir/reader-$state-startup.json"
  local server_log="$log_dir/reader-$state-server.log"
  POINTBREAK_HOME="$reader_state_home" "$pointbreak_binary" inspect \
    --repo "$repo" --port 0 --format json >"$startup" 2>"$server_log" &
  reader_state_started_pid=$!
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

start_reader_state_server "empty-ready-l2" "$reader_empty_l2_repo"
reader_empty_l2_pid="$reader_state_started_pid"
start_reader_state_server "l0" "$reader_l0_repo"
reader_l0_pid="$reader_state_started_pid"
start_reader_state_server "m1" "$reader_m1_repo"
reader_m1_pid="$reader_state_started_pid"
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

POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" inspect --repo "$fixture_repo" --port 0 --format json \
  >"$log_dir/inspect-startup.json" 2>"$log_dir/inspect-server.log" &
server_pid=$!
for _ in $(seq 1 100); do
  [ -s "$log_dir/inspect-startup.json" ] && break
  kill -0 "$server_pid" >/dev/null 2>&1 || die "Inspector exited before startup"
  sleep 0.05
done
jq -e '.schema == "pointbreak.inspect-startup" and .version == 1 and (.port > 0) and (.token | length > 0)' \
  "$log_dir/inspect-startup.json" >/dev/null || die "Inspector did not emit valid startup JSON"
server="$(jq -c '{baseUrl: ("http://" + .host + ":" + (.port | tostring)), token}' "$log_dir/inspect-startup.json")"

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
node -e '
const fs = require("node:fs");
const source = fs.readFileSync(process.argv[1], "utf8");
const marker = "__POINTBREAK_CHANGE_BROWSER_CONFIG__";
if (!source.includes(marker)) throw new Error("browser config marker is missing");
fs.writeFileSync(process.argv[3], source.replace(marker, process.argv[2]));
' "$browser_program_template" "$browser_config" "$browser_program"

# Startup intentionally has no semantic fragment: the Change-aware Inspector
# must select the Timeline monitor by default while the one-time capability is
# consumed into origin-scoped session storage.
browser_url="$(jq -r '.baseUrl + "/#/?token=" + (.token | @uri)' <<<"$server")"
run_pw open "$browser_url" >"$log_dir/browser-open.log" 2>&1

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
if ! run_pw run-code --filename="$browser_program" >"$log_dir/browser-gate.log" 2>&1; then
  sed -n '1,240p' "$log_dir/browser-gate.log" >&2
  die "real-browser Change Inspector gate failed"
fi
wait "$timeline_append_pid" \
  || die "disposable Timeline append did not complete after the parked screenshot"
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
assertion_line="$(rg -o '\{"assertionCount":[0-9]+,"screenshotCount":[0-9]+\}' "$log_dir/browser-gate.log" | tail -1)"
assertion_count="$(jq -er '.assertionCount' <<<"$assertion_line")"
reported_screenshot_count="$(jq -er '.screenshotCount' <<<"$assertion_line")"
[ "$screenshot_count" -eq "$reported_screenshot_count" ] \
  || die "browser reported $reported_screenshot_count screenshots but preserved $screenshot_count"
[ "$screenshot_count" -ge 12 ] || die "expected at least 12 browser screenshots, found $screenshot_count"
tool_versions="$(jq -n \
  --arg git "$(git --version)" \
  --arg node "$(node --version)" \
  --arg playwright "$(run_pw --version 2>&1 | tr '\n' ' ')" \
  --slurpfile pointbreak "$log_dir/pointbreak-version.json" \
  '{git: $git, node: $node, playwright: $playwright, pointbreak: $pointbreak[0]}')"

# The temporary file may be incomplete if serialization fails. Only the final
# atomic rename publishes manifest.json, so its presence remains the completion
# marker for fixture, browser, screenshot, and identity verification.
manifest_tmp="$root/.manifest.json.tmp"
jq -n \
  --arg sourceCommit "$source_commit" \
  --arg binary "$pointbreak_binary" \
  --arg binarySha256 "$binary_sha256" \
  --arg root "$root" \
  --arg fixture "$fixture_identity" \
  --argjson fixtureData "$(cat "$log_dir/fixture.json")" \
  --argjson timelineAppend "$(cat "$log_dir/timeline-append.json")" \
  --argjson toolVersions "$tool_versions" \
  --argjson assertionCount "$assertion_count" \
  --argjson screenshotCount "$screenshot_count" \
  '{gate: "change-inspector-browser-verify", status: "passed", sourceCommit: $sourceCommit,
    binary: $binary, binarySha256: $binarySha256, root: $root, fixture: $fixture,
    fixtureData: $fixtureData, timelineAppend: $timelineAppend,
    toolVersions: $toolVersions, assertionCount: $assertionCount,
    screenshotCount: $screenshotCount}' \
  >"$manifest_tmp"
mv "$manifest_tmp" "$root/manifest.json"
cat "$root/manifest.json"
