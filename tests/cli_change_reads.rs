//! Characterization floor and route contracts for the Change CLI reads: the
//! producer-reuse pages (`change profile`, `change list`, `change attention`)
//! and the per-Change selector reads (`change show`, `change interdiff`,
//! captured `change select`).
//!
//! The characterization tests freeze the authoritative bytes per format lane;
//! they are the parity oracle for the derived routing and may only change
//! where the documented stamp substitution is the specified difference. The
//! route-contract tests pin the derived lane itself: the stamp substitution,
//! carrier-proportional counters, and the fallback states.

mod support;

use std::path::Path;
use std::process::Output;

use serde_json::Value;
use support::git_repo::GitRepo;
use support::{common_dir_store, pointbreak_env, pointbreak_unprepared_env};

const ACTIVE: &[(&str, &str)] = &[("POINTBREAK_DERIVED_ACCESS", "sqlite-wal-bodyless-v1")];
const OFF: &[(&str, &str)] = &[("POINTBREAK_DERIVED_ACCESS", "off")];
const FORMAT_LANES: &[&str] = &["json", "json-pretty", "text"];
const REVIEW_TRACK: &str = "agent:change-reads-reviewer";

/// One store exercising every read shape this suite pins: a Change with two
/// parallel current revisions, an accepted Change, and a top-level
/// membership-withdrawal diagnostic carried by a raw claim-missing event.
///
/// Derived states for the route contracts:
/// - active-current: call [`ChangeReadsFixture::build_derived`], then read
///   with [`ACTIVE`];
/// - explicit off: read with [`OFF`] (never consults the derived store);
/// - unavailable: read with [`ACTIVE`] without calling `build_derived`.
struct ChangeReadsFixture {
    repo: GitRepo,
    parallel_change_id: String,
    parallel_revision_ids: (String, String),
    accepted_change_id: String,
    #[cfg_attr(not(feature = "longitudinal-counting"), allow(dead_code))]
    accepted_revision_id: String,
    withdrawn_claim_id: String,
}

impl ChangeReadsFixture {
    fn repo_arg(&self) -> &str {
        self.repo.path().to_str().expect("fixture path is UTF-8")
    }

    fn build_derived(&self) {
        let output = pointbreak_env(
            ["store", "derived", "build", "--repo", self.repo_arg()],
            ACTIVE,
        );
        assert_success(&output);
    }

    /// Append review facts that no Change read selects (observations on an
    /// existing member revision), so unrelated event history grows without
    /// changing any Change document body.
    #[cfg_attr(not(feature = "longitudinal-counting"), allow(dead_code))]
    fn grow_unrelated_history(&self, events: usize) {
        for ordinal in 0..events {
            let output = pointbreak_env(
                [
                    "observation",
                    "add",
                    "--repo",
                    self.repo_arg(),
                    "--track",
                    REVIEW_TRACK,
                    "--revision",
                    &self.accepted_revision_id,
                    "--title",
                    &format!("unrelated growth {ordinal}"),
                    "--body",
                    "unrelated to any Change read selection",
                ],
                OFF,
            );
            assert_success(&output);
        }
    }
}

fn change_reads_fixture() -> ChangeReadsFixture {
    let repo = support::dump_repo();
    let repo_arg = repo
        .path()
        .to_str()
        .expect("fixture path is UTF-8")
        .to_owned();

    let first = capture(&["capture", "--repo", &repo_arg]);
    let parallel_change_id = first["changeId"]
        .as_str()
        .expect("first capture change id")
        .to_owned();
    let first_revision = first["revision"]["revisionId"]
        .as_str()
        .expect("first revision id")
        .to_owned();
    let first_cursor = first["reviewCursor"]["token"]
        .as_str()
        .expect("first review cursor")
        .to_owned();

    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    let second = capture(&[
        "capture",
        "--repo",
        &repo_arg,
        "--review-cursor",
        &first_cursor,
        "--advance",
        "parallel",
    ]);
    assert_eq!(
        second["changeId"].as_str(),
        Some(parallel_change_id.as_str()),
        "a parallel advance stays inside the same Change"
    );
    let second_revision = second["revision"]["revisionId"]
        .as_str()
        .expect("second revision id")
        .to_owned();

    repo.write("src/lib.rs", "pub fn value() -> u32 { 4 }\n");
    let third = capture(&["capture", "--repo", &repo_arg]);
    let accepted_change_id = third["changeId"]
        .as_str()
        .expect("third capture change id")
        .to_owned();
    assert_ne!(
        accepted_change_id, parallel_change_id,
        "a cursor-less capture opens a new Change"
    );
    let accepted_revision_id = third["revision"]["revisionId"]
        .as_str()
        .expect("third revision id")
        .to_owned();
    let third_cursor = third["reviewCursor"]["token"]
        .as_str()
        .expect("third review cursor")
        .to_owned();
    let accepted = pointbreak_env(
        [
            "assessment",
            "add",
            "--repo",
            &repo_arg,
            "--review-cursor",
            &third_cursor,
            "--track",
            REVIEW_TRACK,
            "--assessment",
            "accepted",
            "--summary",
            "accepted for the change read floor",
        ],
        OFF,
    );
    assert_success(&accepted);

    let withdrawn_claim_id = format!("change-membership:sha256:{}", "a7".repeat(32));
    append_withdrawal_of_missing_claim(repo.path(), &withdrawn_claim_id);

    ChangeReadsFixture {
        repo,
        parallel_change_id,
        parallel_revision_ids: (first_revision, second_revision),
        accepted_change_id,
        accepted_revision_id,
        withdrawn_claim_id,
    }
}

fn capture(args: &[&str]) -> Value {
    let output = pointbreak_env(args, OFF);
    assert_success(&output);
    parse_json(&output.stdout)
}

/// A membership withdrawal naming a claim no event asserted. The ordinary
/// writer refuses this shape, so the carrier is appended raw; the projection
/// folds it into the one top-level withdrawal diagnostic the list document
/// carries.
fn append_withdrawal_of_missing_claim(repo_root: &Path, claim_id: &str) {
    use pointbreak::model::{ChangeMembershipClaimId, ChangeMembershipWithdrawalId, JournalId};
    use pointbreak::session::event::{
        ChangeMembershipWithdrawnPayload, EventTarget, EventType, ShoreEvent, Writer,
    };
    use sha2::{Digest, Sha256};

    let idempotency_key = "change-reads-fixture:withdrawal-claim-missing";
    let claim_nonce = "e5".repeat(32);
    // The payload validator re-derives the withdrawal id from the claim id
    // and nonce over canonical (key-sorted, compact) JSON; serde_json's
    // default map serialization already matches that form.
    let withdrawal_preimage = serde_json::json!({
        "family": "change_membership_withdrawn_v1",
        "coordinates": {"membershipClaimId": claim_id},
        "claimNonce": claim_nonce,
    });
    let withdrawal_id = format!(
        "change-membership-withdrawal:sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&withdrawal_preimage).expect("encode preimage"))
    );
    let event = ShoreEvent::new(
        EventType::ChangeMembershipWithdrawn,
        idempotency_key,
        EventTarget::for_journal(JournalId::new("journal:default")),
        Writer::shore_local("change-reads-fixture"),
        ChangeMembershipWithdrawnPayload {
            schema: "pointbreak.change-membership-withdrawn".to_owned(),
            version: 1,
            membership_withdrawal_id: ChangeMembershipWithdrawalId::new(withdrawal_id),
            membership_claim_id: ChangeMembershipClaimId::new(claim_id),
            claim_nonce,
        },
        "2027-01-01T00:00:01Z",
    )
    .expect("build raw withdrawal fixture event");
    append_raw_event(repo_root, &event);
}

/// Append one raw event file to the store, bypassing the ordinary writer.
/// The derived generation is behind afterwards; call
/// [`ChangeReadsFixture::build_derived`] before derived-lane reads.
fn append_raw_event(repo_root: &Path, event: &pointbreak::session::event::ShoreEvent) {
    use sha2::{Digest, Sha256};

    let events_dir = common_dir_store(repo_root).join("events");
    std::fs::create_dir_all(&events_dir).expect("create fixture events directory");
    let stem = format!("{:x}", Sha256::digest(event.idempotency_key.as_bytes()));
    std::fs::write(
        events_dir.join(format!("{stem}.json")),
        serde_json::to_vec(event).expect("serialize fixture event"),
    )
    .expect("write fixture event");
}

/// Append a raw second proposal carrier binding `revision_id` to a different
/// object artifact hash, so the Revision's exact reference set becomes
/// conflicted.
fn append_conflicting_proposal(repo_root: &Path, revision_id: &str, artifact_hash: &str) {
    use pointbreak::model::{
        EngagementId, EngagementType, JournalId, ObjectId, ReviewTargetRef, RevisionId, TargetRef,
    };
    use pointbreak::session::event::{
        EventTarget, EventType, Revision, ShoreEvent, WorkObjectProposal,
        WorkObjectProposedPayload, Writer,
    };

    let revision_id = RevisionId::new(revision_id);
    let event = ShoreEvent::new(
        EventType::WorkObjectProposed,
        format!("change-reads-fixture:conflicting-proposal:{artifact_hash}"),
        EventTarget::for_generative_move(
            JournalId::new("journal:default"),
            EngagementType::Review,
            TargetRef::Review(ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            }),
            None,
        )
        .expect("proposal target"),
        Writer::shore_local("change-reads-fixture"),
        WorkObjectProposedPayload {
            engagement_id: EngagementId::new(format!("engagement:sha256:{}", "b6".repeat(32))),
            work_object: WorkObjectProposal::Revision {
                revision: Revision {
                    id: revision_id,
                    object_id: ObjectId::new(format!("obj:sha256:{}", "c6".repeat(32))),
                    git_provenance: None,
                },
                summary: None,
                object_artifact_content_hash: artifact_hash.to_owned(),
                supersedes: Vec::new(),
            },
        },
        "2027-01-01T00:00:02Z",
    )
    .expect("build conflicting proposal fixture event");
    append_raw_event(repo_root, &event);
}

/// Append a raw membership claim binding a fabricated, never-proposed
/// Revision into `change_id`, so the member has no exact reference at all.
fn append_membership_of_unproposed_revision(repo_root: &Path, change_id: &str, revision_id: &str) {
    use pointbreak::model::{ChangeId, ChangeMembershipClaimId, JournalId, RevisionId};
    use pointbreak::session::event::{
        ChangeMembershipAssertedPayload, EventTarget, EventType, ShoreEvent, Writer,
    };
    use sha2::{Digest, Sha256};

    let claim_nonce = "d7".repeat(32);
    // The payload validator re-derives the claim id from (change, revision,
    // nonce) over canonical key-sorted compact JSON.
    let claim_preimage = serde_json::json!({
        "family": "change_membership_asserted_v1",
        "changeId": change_id,
        "revisionId": revision_id,
        "claimNonce": claim_nonce,
    });
    let claim_id = format!(
        "change-membership:sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&claim_preimage).expect("encode claim preimage"))
    );
    let event = ShoreEvent::new(
        EventType::ChangeMembershipAsserted,
        "change-reads-fixture:membership-unproposed",
        EventTarget::for_journal(JournalId::new("journal:default")),
        Writer::shore_local("change-reads-fixture"),
        ChangeMembershipAssertedPayload {
            schema: "pointbreak.change-membership-asserted".to_owned(),
            version: 1,
            membership_claim_id: ChangeMembershipClaimId::new(claim_id),
            change_id: ChangeId::new(change_id),
            revision_id: RevisionId::new(revision_id),
            claim_nonce,
        },
        "2027-01-01T00:00:03Z",
    )
    .expect("build unproposed membership fixture event");
    append_raw_event(repo_root, &event);
}

/// Append a raw review-domain response to `request_id`. The ordinary writer
/// refuses both shapes this builder produces — it always reconstructs the
/// response's revision from the opened request's subject — so the carrier is
/// appended raw. `subject_revision_id: Some(..)` claims that revision as the
/// response's own subject (the foreign-revision shape); `None` claims no
/// subject at all (the revision-less shape).
fn append_raw_input_request_response(
    repo_root: &Path,
    request_id: &str,
    subject_revision_id: Option<&str>,
) {
    use pointbreak::model::{
        InputRequestId, InputRequestResponseId, JournalId, ReviewTargetRef, RevisionId, TargetRef,
    };
    use pointbreak::session::event::{
        BodyContentType, EventTarget, EventType, InputRequestRespondedPayload,
        InputRequestResponseOutcome, ShoreEvent, Writer,
    };
    use sha2::{Digest, Sha256};

    let request_id = InputRequestId::new(request_id);
    let response_id = InputRequestResponseId::new(format!(
        "input-response:sha256:{:x}",
        Sha256::digest(format!("change-reads-fixture:response:{}", request_id.as_str()).as_bytes())
    ));
    let target = match subject_revision_id {
        Some(revision_id) => EventTarget::for_subject(
            JournalId::new("journal:default"),
            TargetRef::Review(ReviewTargetRef::InputRequest {
                revision_id: RevisionId::new(revision_id),
                input_request_id: request_id.clone(),
            }),
            None,
        )
        .expect("raw response target"),
        None => EventTarget::for_journal(JournalId::new("journal:default")),
    };
    let event = ShoreEvent::new(
        EventType::InputRequestResponded,
        InputRequestRespondedPayload::idempotency_key(&request_id, response_id.as_str()),
        target,
        Writer::shore_local("change-reads-fixture"),
        InputRequestRespondedPayload {
            input_request_response_id: response_id,
            input_request_id: request_id,
            revision_id: subject_revision_id.map(RevisionId::new),
            task_target: None,
            outcome: InputRequestResponseOutcome::Approved,
            reason: None,
            reason_content_type: BodyContentType::TextPlain,
            reason_artifact_path: None,
            reason_byte_size: None,
            reason_content_hash: None,
            target_fingerprint: None,
        },
        "2027-01-01T00:00:04Z",
    )
    .expect("build raw response fixture event");
    append_raw_event(repo_root, &event);
}

/// A response answering the request through a revision outside the Change
/// hosting it (issue #726's shape).
fn append_foreign_revision_response(repo_root: &Path, request_id: &str, foreign_revision_id: &str) {
    append_raw_input_request_response(repo_root, request_id, Some(foreign_revision_id));
}

/// A response carrying neither a revision nor a task target (issue #723's
/// shape).
fn append_revision_less_response(repo_root: &Path, request_id: &str) {
    append_raw_input_request_response(repo_root, request_id, None);
}

/// Open an operative review-domain input request on `revision_id` through the
/// real writer path, returning the opened request's id.
fn open_operative_request(fixture: &ChangeReadsFixture, revision_id: &str) -> String {
    let output = pointbreak_env(
        [
            "input-request",
            "open",
            "--repo",
            fixture.repo_arg(),
            "--exact-revision",
            revision_id,
            "--track",
            REVIEW_TRACK,
            "--title",
            "closure parity request",
            "--reason",
            "manual-decision-required",
        ],
        OFF,
    );
    assert_success(&output);
    parse_json(&output.stdout)["inputRequestId"]
        .as_str()
        .expect("input request open returns id")
        .to_owned()
}

// ---------------------------------------------------------------------------
// Characterization floor (must pass on current bytes)
// ---------------------------------------------------------------------------

#[test]
fn change_list_reports_every_change_with_projection_identity_in_each_format_lane() {
    let fixture = change_reads_fixture();

    for lane in FORMAT_LANES {
        let output = pointbreak_env(
            [
                "change",
                "list",
                "--repo",
                fixture.repo_arg(),
                "--format",
                lane,
            ],
            OFF,
        );
        assert_success(&output);
        assert!(output.stderr.is_empty(), "lane {lane} wrote stderr");
        let json = parse_json(&output.stdout);
        assert_eq!(json["schema"], "pointbreak.review-change-list", "{lane}");
        assert_eq!(json["version"], 1, "{lane}");
        assert!(
            !json["projectionStamp"]
                .as_str()
                .expect("list projection stamp")
                .is_empty(),
            "{lane}"
        );

        let changes = json["changes"].as_array().expect("changes array");
        let ids = changes
            .iter()
            .map(|change| change["changeId"].as_str().expect("change id"))
            .collect::<Vec<_>>();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "{lane}: changes are ChangeId-ascending");
        assert!(ids.contains(&fixture.parallel_change_id.as_str()), "{lane}");
        assert!(ids.contains(&fixture.accepted_change_id.as_str()), "{lane}");

        let parallel = changes
            .iter()
            .find(|change| change["changeId"] == fixture.parallel_change_id.as_str())
            .expect("parallel Change summary");
        let current = parallel["currentRevisionRefs"]
            .as_array()
            .expect("current revision refs");
        let current_ids = current
            .iter()
            .map(|reference| reference["revisionId"].as_str().expect("revision id"))
            .collect::<Vec<_>>();
        assert_eq!(current_ids.len(), 2, "{lane}: parallel heads stay current");
        assert!(current_ids.contains(&fixture.parallel_revision_ids.0.as_str()));
        assert!(current_ids.contains(&fixture.parallel_revision_ids.1.as_str()));

        let diagnostics = json["diagnostics"]
            .as_array()
            .expect("top-level diagnostics")
            .iter()
            .map(|value| value.as_str().expect("diagnostic string"))
            .collect::<Vec<_>>();
        assert!(
            diagnostics.contains(
                &format!(
                    "change_membership_withdrawal_claim_missing:{}",
                    fixture.withdrawn_claim_id
                )
                .as_str()
            ),
            "{lane}: withdrawal diagnostic missing from {diagnostics:?}"
        );
    }
}

#[test]
fn change_attention_excludes_accepted_changes_and_carries_presentations() {
    let fixture = change_reads_fixture();

    let output = pointbreak_env(["change", "attention", "--repo", fixture.repo_arg()], OFF);
    assert_success(&output);
    let json = parse_json(&output.stdout);
    assert_eq!(json["schema"], "pointbreak.attention-list");
    assert_eq!(json["version"], 2);
    assert!(
        !json["projectionStamp"]
            .as_str()
            .expect("attention projection stamp")
            .is_empty()
    );
    assert!(
        json.get("diagnostics").is_none(),
        "attention has no top-level diagnostics field: {json:#}"
    );

    let ids = json["changes"]
        .as_array()
        .expect("changes array")
        .iter()
        .map(|change| change["changeId"].as_str().expect("change id"))
        .collect::<Vec<_>>();
    assert!(ids.contains(&fixture.parallel_change_id.as_str()));
    assert!(
        !ids.contains(&fixture.accepted_change_id.as_str()),
        "accepted Change leaked into attention: {ids:?}"
    );

    let presentations = json["presentations"]
        .as_object()
        .expect("presentations map is present");
    assert!(presentations.contains_key(&fixture.parallel_change_id));
    assert!(
        !presentations.contains_key(&fixture.accepted_change_id),
        "accepted Change leaked into presentations"
    );
}

#[test]
fn change_profile_reports_ready_availability_with_the_authority_cursor() {
    let fixture = change_reads_fixture();

    let output = pointbreak_env(["change", "profile", "--repo", fixture.repo_arg()], OFF);
    assert_success(&output);
    let json = parse_json(&output.stdout);
    assert_eq!(json["schema"], "pointbreak.inspect-reader-profile");
    assert_eq!(json["version"], 1);
    assert_eq!(json["availability"], "ready");
    for field in ["eventSetHash", "journalRecordSetHash", "capabilitySetHash"] {
        assert!(
            json["authorityCursor"][field].is_string(),
            "authorityCursor.{field}: {json:#}"
        );
    }
    let documents = json["documents"].as_object().expect("document registry");
    assert!(!documents.is_empty());
    let keys = documents.keys().collect::<Vec<_>>();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted, "documents registry is key-ascending");
}

#[test]
fn change_reads_on_l0_emit_the_typed_documents_identically_on_both_lanes() {
    let repo = tempfile::tempdir().expect("temporary L0 repository");
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repo.path())
            .status()
            .expect("run git init")
            .success()
    );
    let repo_arg = repo.path().to_str().expect("L0 path is UTF-8");

    assert_typed_capability_documents(repo_arg, "migration_required");
}

#[test]
fn change_reads_on_m1_emit_the_typed_documents_identically_on_both_lanes() {
    let repo = GitRepo::new();
    std::fs::create_dir_all(repo.path().join(".pointbreak/data/events"))
        .expect("create disposable M1 event directory");
    std::fs::write(
        repo.path().join(".pointbreak/store.local.json"),
        b"{\"schema\":\"shore.store-config\",\"version\":1,\"mode\":\"ephemeral\"}\n",
    )
    .expect("write disposable M1 store configuration");
    std::fs::write(
        repo.path().join(
            ".pointbreak/data/events/5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json",
        ),
        include_bytes!(
            "support/assets/change-ready-store/5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json"
        ),
    )
    .expect("install M1 capability activation only");
    let repo_arg = repo.path().to_str().expect("M1 path is UTF-8");

    assert_typed_capability_documents(repo_arg, "migration_in_progress");
}

/// Both non-L2 states answer with typed stdout documents and exit success on
/// the explicit-off and the derived-selected lanes, byte-identically. The
/// selector-taking reads answer the same way before any selector is resolved.
fn assert_typed_capability_documents(repo_arg: &str, state: &str) {
    let placeholder_change = format!("change:sha256:{}", "0".repeat(64));
    let placeholder_revision = format!("revision:sha256:{}", "1".repeat(64));
    let placeholder_hash = format!("sha256:{}", "2".repeat(64));
    let commands: Vec<(&str, Vec<&str>)> = vec![
        ("profile", vec!["change", "profile", "--repo", repo_arg]),
        ("list", vec!["change", "list", "--repo", repo_arg]),
        ("attention", vec!["change", "attention", "--repo", repo_arg]),
        (
            "show",
            vec!["change", "show", &placeholder_change, "--repo", repo_arg],
        ),
        (
            "select",
            vec!["change", "select", &placeholder_change, "--repo", repo_arg],
        ),
        (
            "interdiff",
            vec![
                "change",
                "interdiff",
                &placeholder_change,
                &placeholder_revision,
                &placeholder_revision,
                "--from-artifact-hash",
                &placeholder_hash,
                "--to-artifact-hash",
                &placeholder_hash,
                "--repo",
                repo_arg,
            ],
        ),
    ];
    for (command, args) in commands {
        let off = pointbreak_unprepared_env(&args, OFF);
        let active = pointbreak_unprepared_env(&args, ACTIVE);
        for (lane, output) in [("off", &off), ("active", &active)] {
            assert!(
                output.status.success(),
                "change {command} ({lane}, {state}) exited nonzero: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert_eq!(
            off.stdout, active.stdout,
            "change {command} ({state}) stdout parity across lanes"
        );
        assert_eq!(
            off.stderr, active.stderr,
            "change {command} ({state}) stderr parity across lanes"
        );

        let json = parse_json(&off.stdout);
        if command == "profile" {
            assert_eq!(json["schema"], "pointbreak.inspect-reader-profile");
            assert_eq!(json["availability"], state, "change profile ({state})");
        } else {
            let expected_schema = match state {
                "migration_required" => "pointbreak.store-migration-required",
                "migration_in_progress" => "pointbreak.store-migration-in-progress",
                other => panic!("unexpected capability state {other}"),
            };
            assert_eq!(json["schema"], expected_schema, "change {command}");
            assert_eq!(json["state"], state, "change {command}");
        }
    }
}

#[test]
fn change_list_and_attention_bytes_are_identical_while_derived_is_unavailable() {
    let fixture = change_reads_fixture();

    for command in ["list", "attention"] {
        for lane in FORMAT_LANES {
            assert_derived_lane_byte_parity(&fixture, command, lane, "unavailable");
        }
    }
}

/// With the derived generation active-current, the derived lane preserves the
/// characterization floor's domain properties: ChangeId-ascending changes,
/// RevisionId-ascending current revision refs, the accepted-Change exclusion
/// on attention, and the list diagnostics vocabulary.
#[test]
fn change_list_and_attention_active_current_preserve_ordering_and_exclusion() {
    let fixture = change_reads_fixture();
    fixture.build_derived();

    let list_output = pointbreak_env(["change", "list", "--repo", fixture.repo_arg()], ACTIVE);
    assert_success(&list_output);
    let list = parse_json(&list_output.stdout);
    let off_output = pointbreak_env(["change", "list", "--repo", fixture.repo_arg()], OFF);
    assert_success(&off_output);
    assert_ne!(
        list["projectionStamp"],
        parse_json(&off_output.stdout)["projectionStamp"],
        "the derived lane must be serving before its ordering is meaningful"
    );

    let changes = list["changes"].as_array().expect("changes array");
    let ids = changes
        .iter()
        .map(|change| change["changeId"].as_str().expect("change id"))
        .collect::<Vec<_>>();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort_unstable();
    assert_eq!(ids, sorted_ids, "derived changes are ChangeId-ascending");
    let parallel = changes
        .iter()
        .find(|change| change["changeId"] == fixture.parallel_change_id.as_str())
        .expect("parallel Change summary");
    let current_ids = parallel["currentRevisionRefs"]
        .as_array()
        .expect("current revision refs")
        .iter()
        .map(|reference| reference["revisionId"].as_str().expect("revision id"))
        .collect::<Vec<_>>();
    let mut sorted_current = current_ids.clone();
    sorted_current.sort_unstable();
    assert_eq!(
        current_ids, sorted_current,
        "derived current revision refs are RevisionId-ascending"
    );
    assert_eq!(current_ids.len(), 2);
    assert!(
        list["diagnostics"]
            .as_array()
            .expect("list diagnostics")
            .iter()
            .any(|value| {
                value.as_str().expect("diagnostic string")
                    == format!(
                        "change_membership_withdrawal_claim_missing:{}",
                        fixture.withdrawn_claim_id
                    )
            }),
        "derived list carries the withdrawal diagnostic"
    );

    let attention_output = pointbreak_env(
        ["change", "attention", "--repo", fixture.repo_arg()],
        ACTIVE,
    );
    assert_success(&attention_output);
    let attention = parse_json(&attention_output.stdout);
    let attention_ids = attention["changes"]
        .as_array()
        .expect("attention changes")
        .iter()
        .map(|change| change["changeId"].as_str().expect("change id"))
        .collect::<Vec<_>>();
    assert!(attention_ids.contains(&fixture.parallel_change_id.as_str()));
    assert!(
        !attention_ids.contains(&fixture.accepted_change_id.as_str()),
        "the derived attention lane excludes the accepted Change"
    );
    assert!(
        attention.get("diagnostics").is_none(),
        "attention keeps no top-level diagnostics field on the derived lane"
    );
}

/// A derived `list`/`attention` document and an authoritative `show`
/// document at the same store state deliberately carry different
/// `projectionStamp` values — the derived lane binds the generation stamp,
/// the authoritative facade its presentation-fold stamp — and each lane stays
/// internally consistent. The derived `show` route binds its own seek stamp,
/// pinned three-way in
/// [`mixed_lane_stamps_are_three_way_distinct_and_internally_consistent`];
/// the authoritative facade stamp is read on the explicit-off lane here.
#[test]
fn derived_list_and_authoritative_show_carry_distinct_consistent_stamps() {
    let fixture = change_reads_fixture();
    fixture.build_derived();

    let list = parse_json(
        &pointbreak_env(["change", "list", "--repo", fixture.repo_arg()], ACTIVE).stdout,
    );
    let attention = parse_json(
        &pointbreak_env(
            ["change", "attention", "--repo", fixture.repo_arg()],
            ACTIVE,
        )
        .stdout,
    );
    let show = parse_json(
        &pointbreak_env(
            [
                "change",
                "show",
                &fixture.parallel_change_id,
                "--repo",
                fixture.repo_arg(),
            ],
            OFF,
        )
        .stdout,
    );

    let derived_stamp = list["projectionStamp"].as_str().expect("derived stamp");
    assert_eq!(
        derived_stamp,
        attention["projectionStamp"]
            .as_str()
            .expect("attention stamp"),
        "the derived lane shares one generation stamp"
    );
    for summary in list["changes"].as_array().expect("list changes") {
        assert_eq!(
            summary["projectionStamp"].as_str().expect("summary stamp"),
            derived_stamp,
            "every derived summary carries the shared generation stamp"
        );
    }
    let show_stamp = show["projectionStamp"].as_str().expect("show stamp");
    assert_ne!(
        derived_stamp, show_stamp,
        "the authoritative show facade stamp differs from the derived generation stamp"
    );
    let off_list =
        parse_json(&pointbreak_env(["change", "list", "--repo", fixture.repo_arg()], OFF).stdout);
    assert_eq!(
        show_stamp,
        off_list["projectionStamp"].as_str().expect("off stamp"),
        "the authoritative lane stays internally consistent"
    );
}

/// The other change subcommands never consult the derived lane: their output
/// stays byte-identical between the derived-selected and explicit-off lanes
/// at the same store state, including with an active-current generation.
#[test]
fn neighbor_change_subcommands_are_untouched_by_the_derived_routing() {
    let fixture = change_reads_fixture();
    fixture.build_derived();

    let list =
        parse_json(&pointbreak_env(["change", "list", "--repo", fixture.repo_arg()], OFF).stdout);
    let accepted = list["changes"]
        .as_array()
        .expect("changes array")
        .iter()
        .find(|change| change["changeId"] == fixture.accepted_change_id.as_str())
        .expect("accepted Change summary");
    let accepted_ref = &accepted["currentRevisionRefs"][0];
    let accepted_revision = accepted_ref["revisionId"].as_str().expect("revision id");
    let accepted_hash = accepted_ref["objectArtifactContentHash"]
        .as_str()
        .expect("artifact hash");
    let parallel = list["changes"]
        .as_array()
        .expect("changes array")
        .iter()
        .find(|change| change["changeId"] == fixture.parallel_change_id.as_str())
        .expect("parallel Change summary");
    assert_eq!(
        parallel["currentRevisionRefs"]
            .as_array()
            .expect("parallel heads")
            .len(),
        2
    );

    let revision = vec![
        "revision".to_owned(),
        fixture.accepted_change_id.clone(),
        accepted_revision.to_owned(),
        "--artifact-hash".to_owned(),
        accepted_hash.to_owned(),
    ];
    let resource = vec![
        "resource".to_owned(),
        fixture.accepted_change_id.clone(),
        accepted_revision.to_owned(),
        "--artifact-hash".to_owned(),
        accepted_hash.to_owned(),
    ];
    // Deliberate, reviewed narrowing: `show`, `select`, and `interdiff` are
    // now derived-routed, so the untouched-neighbor set is exactly the two
    // commands the derived lane must never consult.
    for subcommand in [revision, resource] {
        let mut args = vec!["change".to_owned()];
        args.extend(subcommand.clone());
        args.extend(["--repo".to_owned(), fixture.repo_arg().to_owned()]);
        let active = pointbreak_env(&args, ACTIVE);
        let off = pointbreak_env(&args, OFF);
        assert_eq!(
            active.status.code(),
            off.status.code(),
            "change {}: exit parity",
            subcommand[0]
        );
        assert_eq!(
            active.stdout, off.stdout,
            "change {}: stdout parity",
            subcommand[0]
        );
        assert_eq!(
            active.stderr, off.stderr,
            "change {}: stderr parity",
            subcommand[0]
        );
        assert_success(&active);
    }
}

#[test]
fn change_profile_bytes_are_identical_across_derived_states() {
    let fixture = change_reads_fixture();

    for lane in FORMAT_LANES {
        assert_derived_lane_byte_parity(&fixture, "profile", lane, "unavailable");
    }
    fixture.build_derived();
    for lane in FORMAT_LANES {
        assert_derived_lane_byte_parity(&fixture, "profile", lane, "active-current");
    }
}

/// The derived-selected lane and the explicit-off lane answer byte-identically
/// (exit, stdout, stderr) for one command and format lane at the current
/// store state.
fn assert_derived_lane_byte_parity(
    fixture: &ChangeReadsFixture,
    command: &str,
    lane: &str,
    state: &str,
) {
    let args = [
        "change",
        command,
        "--repo",
        fixture.repo_arg(),
        "--format",
        lane,
    ];
    let active = pointbreak_env(args, ACTIVE);
    let off = pointbreak_env(args, OFF);
    assert_eq!(
        active.status.code(),
        off.status.code(),
        "change {command} ({lane}, {state}): exit parity"
    );
    assert_eq!(
        active.stdout, off.stdout,
        "change {command} ({lane}, {state}): stdout parity"
    );
    assert_eq!(
        active.stderr, off.stderr,
        "change {command} ({lane}, {state}): stderr parity"
    );
}

// ---------------------------------------------------------------------------
// Characterization floor: per-Change seek reads (show / interdiff / select)
// ---------------------------------------------------------------------------

/// Extend the shared fixture with a replacement edge inside the accepted
/// Change and an explicit link between the two Changes, so a detail document
/// carries members, claims, an effective supersedes pair, a link, and a
/// qualification entry. Returns the replacement Revision id.
fn enriched_detail_fixture() -> (ChangeReadsFixture, String) {
    let fixture = change_reads_fixture();

    let link = pointbreak_env(
        [
            "change",
            "link",
            &fixture.parallel_change_id,
            &fixture.accepted_change_id,
            "--relation",
            "same-work",
            "--repo",
            fixture.repo_arg(),
            "--operation-id",
            "change-operation:detail-floor-link",
        ],
        OFF,
    );
    assert_success(&link);

    let select = pointbreak_env(
        [
            "change",
            "select",
            &fixture.accepted_change_id,
            "--repo",
            fixture.repo_arg(),
        ],
        OFF,
    );
    assert_success(&select);
    let token = parse_json(&select.stdout)["token"]
        .as_str()
        .expect("selection token")
        .to_owned();
    fixture
        .repo
        .write("src/lib.rs", "pub fn value() -> u32 { 5 }\n");
    let replaced = capture(&[
        "capture",
        "--repo",
        fixture.repo_arg(),
        "--review-cursor",
        &token,
        "--advance",
        "replace",
    ]);
    let replacement_revision = replaced["revision"]["revisionId"]
        .as_str()
        .expect("replacement revision id")
        .to_owned();
    (fixture, replacement_revision)
}

fn change_show(fixture: &ChangeReadsFixture, change_id: &str, lane: &str) -> Value {
    let output = pointbreak_env(
        [
            "change",
            "show",
            change_id,
            "--repo",
            fixture.repo_arg(),
            "--format",
            lane,
        ],
        OFF,
    );
    assert_success(&output);
    assert!(output.stderr.is_empty(), "show ({lane}) wrote stderr");
    parse_json(&output.stdout)
}

fn ascending_strings(values: &[&str], label: &str) {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    assert_eq!(values, sorted.as_slice(), "{label} is ascending");
}

#[test]
fn change_show_reports_one_change_detail_in_each_format_lane() {
    let (fixture, replacement_revision) = enriched_detail_fixture();

    const DETAIL_FIELDS: &[&str] = &[
        "summary",
        "memberRevisions",
        "unavailableMemberRevisions",
        "membershipClaims",
        "membershipWithdrawals",
        "relationClaims",
        "relationWithdrawals",
        "links",
        "effectiveSupersedes",
        "pendingOrConflictingEdges",
        "currentRevisionRefs",
        "perCurrentRevisionQualification",
        "operativeObligations",
        "diagnostics",
        "projectionStamp",
    ];

    for lane in FORMAT_LANES {
        for change_id in [&fixture.parallel_change_id, &fixture.accepted_change_id] {
            let json = change_show(&fixture, change_id, lane);
            assert_eq!(json["schema"], "pointbreak.review-change", "{lane}");
            assert_eq!(json["version"], 1, "{lane}");
            for field in DETAIL_FIELDS {
                assert!(
                    json.get(field).is_some(),
                    "{lane}: change {change_id} detail field {field} missing: {json:#}"
                );
            }
            assert_eq!(json["summary"]["changeId"], change_id.as_str(), "{lane}");
            assert!(
                !json["projectionStamp"]
                    .as_str()
                    .expect("show projection stamp")
                    .is_empty(),
                "{lane}"
            );

            let members = json["memberRevisions"]
                .as_array()
                .expect("member revisions")
                .iter()
                .map(|member| {
                    member["revision"]["revisionId"]
                        .as_str()
                        .expect("member revision id")
                })
                .collect::<Vec<_>>();
            ascending_strings(&members, &format!("{lane}: memberRevisions"));
            let claims = json["membershipClaims"]
                .as_array()
                .expect("membership claims")
                .iter()
                .map(|claim| claim["claimId"].as_str().expect("claim id"))
                .collect::<Vec<_>>();
            ascending_strings(&claims, &format!("{lane}: membershipClaims"));
            let supersedes = json["effectiveSupersedes"]
                .as_array()
                .expect("effective supersedes")
                .iter()
                .map(|pair| {
                    pair[0]["revisionId"]
                        .as_str()
                        .expect("successor revision id")
                })
                .collect::<Vec<_>>();
            ascending_strings(&supersedes, &format!("{lane}: effectiveSupersedes"));
            let current = json["currentRevisionRefs"]
                .as_array()
                .expect("current revision refs")
                .iter()
                .map(|reference| reference["revisionId"].as_str().expect("revision id"))
                .collect::<Vec<_>>();
            ascending_strings(&current, &format!("{lane}: currentRevisionRefs"));
        }

        let parallel = change_show(&fixture, &fixture.parallel_change_id, lane);
        assert_eq!(
            parallel["memberRevisions"]
                .as_array()
                .expect("parallel members")
                .len(),
            2,
            "{lane}: both parallel members are exact"
        );
        assert_eq!(
            parallel["currentRevisionRefs"]
                .as_array()
                .expect("parallel current")
                .len(),
            2,
            "{lane}: both parallel heads stay current"
        );

        let accepted = change_show(&fixture, &fixture.accepted_change_id, lane);
        let supersedes = accepted["effectiveSupersedes"]
            .as_array()
            .expect("accepted supersedes");
        assert_eq!(supersedes.len(), 1, "{lane}: one effective replacement");
        assert_eq!(
            supersedes[0][0]["revisionId"], replacement_revision,
            "{lane}: replacement Revision is the successor"
        );
        assert_eq!(
            supersedes[0][1]["revisionId"], fixture.accepted_revision_id,
            "{lane}: replaced Revision is the predecessor"
        );
        let current = accepted["currentRevisionRefs"]
            .as_array()
            .expect("accepted current");
        assert_eq!(current.len(), 1, "{lane}");
        assert_eq!(current[0]["revisionId"], replacement_revision, "{lane}");
        // Link endpoints are canonically ordered (left < right), not
        // argument-ordered.
        let mut endpoints = [
            fixture.parallel_change_id.as_str(),
            fixture.accepted_change_id.as_str(),
        ];
        endpoints.sort_unstable();
        for change_document in [&parallel, &accepted] {
            let links = change_document["links"].as_array().expect("links");
            assert_eq!(links.len(), 1, "{lane}: the explicit link is present");
            assert_eq!(links[0]["leftChangeId"], endpoints[0], "{lane}");
            assert_eq!(links[0]["rightChangeId"], endpoints[1], "{lane}");
            assert_eq!(links[0]["relation"], "same_work", "{lane}");
        }
        let qualification = accepted["perCurrentRevisionQualification"]
            .as_array()
            .expect("qualification entries");
        assert_eq!(qualification.len(), 1, "{lane}");
        assert_eq!(
            qualification[0]["revision"]["revisionId"], replacement_revision,
            "{lane}: qualification tracks the current Revision"
        );
    }
}

#[test]
fn change_show_unknown_change_fails_with_the_authoritative_message() {
    let fixture = change_reads_fixture();
    let missing = format!("change:sha256:{}", "f".repeat(64));

    let output = pointbreak_env(
        ["change", "show", &missing, "--repo", fixture.repo_arg()],
        OFF,
    );
    assert!(!output.status.success(), "unknown Change must fail");
    assert!(
        output.stdout.is_empty(),
        "unknown Change writes no document: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("Change {missing} is unavailable")),
        "authoritative unknown-Change message missing: {stderr}"
    );
}

/// The detail document's `diagnostics` field carries the per-Change
/// vocabulary only; the store-scoped withdrawal diagnostic stays a list
/// document concern and never leaks into `show`.
#[test]
fn change_show_diagnostics_are_the_per_change_vocabulary() {
    let fixture = change_reads_fixture();

    let withdrawal_diagnostic = format!(
        "change_membership_withdrawal_claim_missing:{}",
        fixture.withdrawn_claim_id
    );
    for change_id in [&fixture.parallel_change_id, &fixture.accepted_change_id] {
        let json = change_show(&fixture, change_id, "json");
        let diagnostics = json["diagnostics"]
            .as_array()
            .expect("detail diagnostics")
            .iter()
            .map(|value| value.as_str().expect("diagnostic string"))
            .collect::<Vec<_>>();
        assert!(
            !diagnostics.contains(&withdrawal_diagnostic.as_str()),
            "store-scoped withdrawal diagnostic leaked into change {change_id}: {diagnostics:?}"
        );
    }

    let list =
        parse_json(&pointbreak_env(["change", "list", "--repo", fixture.repo_arg()], OFF).stdout);
    assert!(
        list["diagnostics"]
            .as_array()
            .expect("list diagnostics")
            .iter()
            .any(|value| value.as_str() == Some(withdrawal_diagnostic.as_str())),
        "the withdrawal diagnostic stays a store-scoped list concern"
    );
}

/// Two exact parallel heads of one Change, with their authoritative artifact
/// hashes, straight from the list document.
fn parallel_heads(fixture: &ChangeReadsFixture) -> (Value, Value) {
    let list =
        parse_json(&pointbreak_env(["change", "list", "--repo", fixture.repo_arg()], OFF).stdout);
    let parallel = list["changes"]
        .as_array()
        .expect("changes array")
        .iter()
        .find(|change| change["changeId"] == fixture.parallel_change_id.as_str())
        .expect("parallel Change summary")
        .clone();
    let heads = parallel["currentRevisionRefs"]
        .as_array()
        .expect("parallel heads");
    assert_eq!(heads.len(), 2, "both parallel heads stay current");
    (heads[0].clone(), heads[1].clone())
}

#[test]
fn change_interdiff_emits_the_unavailable_first_cohort_contract_in_each_format_lane() {
    let fixture = change_reads_fixture();
    let (from, to) = parallel_heads(&fixture);

    for lane in FORMAT_LANES {
        let output = pointbreak_env(
            [
                "change",
                "interdiff",
                &fixture.parallel_change_id,
                from["revisionId"].as_str().expect("from revision id"),
                to["revisionId"].as_str().expect("to revision id"),
                "--from-artifact-hash",
                from["objectArtifactContentHash"]
                    .as_str()
                    .expect("from hash"),
                "--to-artifact-hash",
                to["objectArtifactContentHash"].as_str().expect("to hash"),
                "--repo",
                fixture.repo_arg(),
                "--format",
                lane,
            ],
            OFF,
        );
        assert_success(&output);
        assert!(output.stderr.is_empty(), "interdiff ({lane}) wrote stderr");
        let json = parse_json(&output.stdout);
        assert_eq!(
            json["schema"], "pointbreak.review-revision-interdiff",
            "{lane}"
        );
        assert_eq!(json["version"], 1, "{lane}");
        assert!(
            json.get("projectionStamp").is_none(),
            "{lane}: the cold CLI interdiff document carries no stamp"
        );
        assert_eq!(json["interdiff"]["from"], from, "{lane}");
        assert_eq!(json["interdiff"]["to"], to, "{lane}");
        assert_eq!(json["interdiff"]["algorithmVersion"], "unavailable-v1");
        assert_eq!(
            json["interdiff"]["scope"],
            Value::Array(Vec::new()),
            "{lane}"
        );
        assert_eq!(json["availability"], "unavailable", "{lane}");
        assert!(
            json.get("comparison").is_none(),
            "{lane}: no comparison material in the first cohort"
        );
        assert_eq!(
            json["diagnostics"],
            serde_json::json!(["revision_interdiff_not_available"]),
            "{lane}"
        );
        assert!(
            !json["cacheKey"].as_str().expect("cache key").is_empty(),
            "{lane}"
        );
    }
}

/// Endpoint validation order is observable: an invalid `from` surfaces its
/// error even when `to` is also invalid with a different failure class.
#[test]
fn change_interdiff_validates_from_before_to() {
    let fixture = change_reads_fixture();
    let (first_head, second_head) = parallel_heads(&fixture);

    let output = pointbreak_env(
        [
            "change",
            "interdiff",
            &fixture.parallel_change_id,
            // Not a member of the parallel Change: the accepted Change's head.
            &fixture.accepted_revision_id,
            second_head["revisionId"].as_str().expect("to revision id"),
            "--from-artifact-hash",
            first_head["objectArtifactContentHash"]
                .as_str()
                .expect("borrowed hash"),
            "--to-artifact-hash",
            // Mismatched hash: the other head's artifact hash.
            first_head["objectArtifactContentHash"]
                .as_str()
                .expect("mismatched hash"),
            "--repo",
            fixture.repo_arg(),
        ],
        OFF,
    );
    assert!(!output.status.success(), "invalid endpoints must fail");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("exact Revision is not an active member of the Change"),
        "the from endpoint's failure surfaces first: {stderr}"
    );
    assert!(
        !stderr.contains("does not match authoritative state"),
        "the to endpoint is not validated before from: {stderr}"
    );
}

fn decode_cursor_token(token: &str) -> Value {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token)
        .expect("cursor token is url-safe base64");
    serde_json::from_slice(&bytes).expect("cursor token wire JSON")
}

#[test]
fn change_select_captured_emits_a_cursor_in_each_format_lane() {
    let fixture = change_reads_fixture();

    for lane in FORMAT_LANES {
        let output = pointbreak_env(
            [
                "change",
                "select",
                &fixture.accepted_change_id,
                "--source",
                "captured",
                "--repo",
                fixture.repo_arg(),
                "--format",
                lane,
            ],
            OFF,
        );
        assert_success(&output);
        assert!(output.stderr.is_empty(), "select ({lane}) wrote stderr");
        let json = parse_json(&output.stdout);
        let cursor = &json["cursor"];
        assert_eq!(cursor["schema"], "pointbreak.review-cursor.v1", "{lane}");
        assert_eq!(cursor["changeId"], fixture.accepted_change_id, "{lane}");
        assert_eq!(
            cursor["revision"]["revisionId"], fixture.accepted_revision_id,
            "{lane}"
        );
        assert_eq!(cursor["sourceBinding"]["kind"], "captured", "{lane}");
        assert_eq!(
            cursor["blockingDiagnostics"],
            Value::Array(Vec::new()),
            "{lane}"
        );
        assert_eq!(
            cursor["selectedCurrentRevisions"]
                .as_array()
                .expect("selected current revisions")
                .len(),
            1,
            "{lane}"
        );

        let token = json["token"].as_str().expect("selection token");
        let wire = decode_cursor_token(token);
        assert_eq!(wire["cursor"], *cursor, "{lane}: token binds the cursor");
        assert!(
            !wire["selfHash"].as_str().expect("self hash").is_empty(),
            "{lane}"
        );
    }
}

#[test]
fn change_select_refusals_are_json_on_stderr_with_nonzero_exit() {
    let fixture = change_reads_fixture();

    let output = pointbreak_env(
        [
            "change",
            "select",
            &fixture.parallel_change_id,
            "--repo",
            fixture.repo_arg(),
        ],
        OFF,
    );
    assert!(
        !output.status.success(),
        "a parallel-current Change refuses implicit selection"
    );
    assert!(
        output.stdout.is_empty(),
        "a refusal writes no stdout document: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let refusal = parse_json(&output.stderr);
    assert_eq!(refusal["code"], "explicit_revision_required");
    assert!(
        !refusal["message"].as_str().expect("message").is_empty(),
        "{refusal:#}"
    );
    assert_eq!(
        refusal["exactCandidates"]
            .as_array()
            .expect("exact candidates")
            .len(),
        2,
        "both parallel heads are exact candidates"
    );
    assert_eq!(refusal["diagnostics"], Value::Array(Vec::new()));
}

#[test]
fn change_select_cursor_revalidation_round_trips_through_the_cli() {
    let fixture = change_reads_fixture();

    let first = pointbreak_env(
        [
            "change",
            "select",
            &fixture.accepted_change_id,
            "--repo",
            fixture.repo_arg(),
        ],
        OFF,
    );
    assert_success(&first);
    let first_json = parse_json(&first.stdout);
    let first_token = first_json["token"].as_str().expect("first token");

    let second = pointbreak_env(
        [
            "change",
            "select",
            &fixture.accepted_change_id,
            "--cursor",
            first_token,
            "--repo",
            fixture.repo_arg(),
        ],
        OFF,
    );
    assert_success(&second);
    let second_json = parse_json(&second.stdout);
    assert_eq!(
        second_json["cursor"], first_json["cursor"],
        "revalidation at an unchanged graph reissues the identical cursor"
    );
    assert_eq!(
        second_json["token"].as_str().expect("second token"),
        first_token,
        "the reissued token is byte-identical"
    );
}

#[test]
fn change_select_allow_historical_admits_a_non_current_member() {
    let repo = support::dump_repo();
    let repo_arg = repo
        .path()
        .to_str()
        .expect("fixture path is UTF-8")
        .to_owned();
    let first = capture(&["capture", "--repo", &repo_arg]);
    let change_id = first["changeId"].as_str().expect("change id").to_owned();
    let historical_revision = first["revision"]["revisionId"]
        .as_str()
        .expect("first revision id")
        .to_owned();
    let cursor = first["reviewCursor"]["token"]
        .as_str()
        .expect("first review cursor")
        .to_owned();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    let second = capture(&[
        "capture",
        "--repo",
        &repo_arg,
        "--review-cursor",
        &cursor,
        "--advance",
        "replace",
    ]);
    let current_revision = second["revision"]["revisionId"]
        .as_str()
        .expect("replacement revision id")
        .to_owned();

    let refused = pointbreak_env(
        [
            "change",
            "select",
            &change_id,
            "--revision",
            &historical_revision,
            "--repo",
            &repo_arg,
        ],
        OFF,
    );
    assert!(
        !refused.status.success(),
        "a historical member requires --allow-historical"
    );
    assert_eq!(
        parse_json(&refused.stderr)["code"],
        "historical_revision_not_authorable"
    );

    let output = pointbreak_env(
        [
            "change",
            "select",
            &change_id,
            "--revision",
            &historical_revision,
            "--allow-historical",
            "--repo",
            &repo_arg,
        ],
        OFF,
    );
    assert_success(&output);
    let json = parse_json(&output.stdout);
    assert_eq!(
        json["cursor"]["revision"]["revisionId"],
        historical_revision
    );
    assert_eq!(json["cursor"]["sourceBinding"]["kind"], "captured");
    let selected = json["cursor"]["selectedCurrentRevisions"]
        .as_array()
        .expect("selected current revisions");
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0]["revisionId"], current_revision,
        "the current set names the replacement, not the historical member"
    );
}

// ---------------------------------------------------------------------------
// Route contracts (red until the reads route through the derived producers)
// ---------------------------------------------------------------------------

/// With the derived generation active and exactly current, the derived lane
/// substitutes the derived generation stamp for the authoritative
/// presentation-fold stamp; every other byte matches the explicit-off lane,
/// and both derived documents at one store state share one stamp value.
#[test]
fn change_list_and_attention_active_current_substitute_one_derived_stamp() {
    let fixture = change_reads_fixture();
    fixture.build_derived();

    for lane in FORMAT_LANES {
        let mut active_stamps = Vec::new();
        for command in ["list", "attention"] {
            let args = [
                "change",
                command,
                "--repo",
                fixture.repo_arg(),
                "--format",
                lane,
            ];
            let active = pointbreak_env(args, ACTIVE);
            let off = pointbreak_env(args, OFF);
            assert_success(&active);
            assert_success(&off);

            let active_stamp = parse_json(&active.stdout)["projectionStamp"]
                .as_str()
                .expect("active projection stamp")
                .to_owned();
            let off_stamp = parse_json(&off.stdout)["projectionStamp"]
                .as_str()
                .expect("authoritative projection stamp")
                .to_owned();
            assert_ne!(
                active_stamp, off_stamp,
                "change {command} ({lane}): the derived lane substitutes the \
                 generation stamp for the presentation-fold stamp"
            );

            let normalized = String::from_utf8(active.stdout.clone())
                .expect("active stdout is UTF-8")
                .replace(&active_stamp, &off_stamp);
            assert_eq!(
                normalized.into_bytes(),
                off.stdout,
                "change {command} ({lane}): byte parity modulo the stamp substitution"
            );
            assert_eq!(
                active.stderr, off.stderr,
                "change {command} ({lane}): stderr parity"
            );
            active_stamps.push(active_stamp);
        }
        assert_eq!(
            active_stamps[0], active_stamps[1],
            "{lane}: derived list and attention share one stamp at one store state"
        );
    }
}

/// With the derived generation active and exactly current, `change show`
/// substitutes the seek-scoped stamp value for the authoritative
/// presentation-fold stamp; every other byte matches the explicit-off lane
/// per format lane.
#[test]
fn derived_change_show_is_byte_identical_modulo_the_seek_stamp() {
    let fixture = change_reads_fixture();
    fixture.build_derived();

    for lane in FORMAT_LANES {
        for change_id in [&fixture.parallel_change_id, &fixture.accepted_change_id] {
            let args = [
                "change",
                "show",
                change_id,
                "--repo",
                fixture.repo_arg(),
                "--format",
                lane,
            ];
            let active = pointbreak_env(args, ACTIVE);
            let off = pointbreak_env(args, OFF);
            assert_success(&active);
            assert_success(&off);

            let active_stamp = parse_json(&active.stdout)["projectionStamp"]
                .as_str()
                .expect("active projection stamp")
                .to_owned();
            let off_stamp = parse_json(&off.stdout)["projectionStamp"]
                .as_str()
                .expect("authoritative projection stamp")
                .to_owned();
            assert_ne!(
                active_stamp, off_stamp,
                "change show ({lane}): the derived lane substitutes the seek stamp"
            );
            // One stamp VALUE appears at two JSON paths (`projectionStamp`
            // and `summary.projectionStamp`); the value replacement covers
            // both.
            let normalized = String::from_utf8(active.stdout.clone())
                .expect("active stdout is UTF-8")
                .replace(&active_stamp, &off_stamp);
            assert_eq!(
                normalized.into_bytes(),
                off.stdout,
                "change show ({lane}): byte parity modulo the stamp substitution"
            );
            assert_eq!(
                active.stderr, off.stderr,
                "change show ({lane}): stderr parity"
            );
        }
    }
}

/// One Change's `change show` under ACTIVE vs OFF: byte-identical modulo the
/// documented stamp substitution, with `operativeObligations` empty on both
/// lanes — the end-to-end shape both response-closure parity tests assert.
fn assert_show_obligations_cleared_on_both_lanes(fixture: &ChangeReadsFixture, change_id: &str) {
    for lane in FORMAT_LANES {
        let args = [
            "change",
            "show",
            change_id,
            "--repo",
            fixture.repo_arg(),
            "--format",
            lane,
        ];
        let active = pointbreak_env(args, ACTIVE);
        let off = pointbreak_env(args, OFF);
        assert_success(&active);
        assert_success(&off);

        let active_json = parse_json(&active.stdout);
        let off_json = parse_json(&off.stdout);
        assert_eq!(
            off_json["operativeObligations"]
                .as_array()
                .expect("authoritative operative obligations"),
            &Vec::<Value>::new(),
            "change show ({lane}): the authoritative lane clears the answered obligation"
        );
        assert_eq!(
            active_json["operativeObligations"], off_json["operativeObligations"],
            "change show ({lane}): both lanes agree on operative obligations"
        );

        let active_stamp = active_json["projectionStamp"]
            .as_str()
            .expect("active projection stamp")
            .to_owned();
        let off_stamp = off_json["projectionStamp"]
            .as_str()
            .expect("authoritative projection stamp")
            .to_owned();
        assert_ne!(
            active_stamp, off_stamp,
            "change show ({lane}): the derived lane substitutes the seek stamp"
        );
        let normalized = String::from_utf8(active.stdout.clone())
            .expect("active stdout is UTF-8")
            .replace(&active_stamp, &off_stamp);
        assert_eq!(
            normalized.into_bytes(),
            off.stdout,
            "change show ({lane}): byte parity modulo the stamp substitution"
        );
        assert_eq!(
            active.stderr, off.stderr,
            "change show ({lane}): stderr parity"
        );
    }
}

/// End-to-end #726 parity: an operative request opened through the real
/// writer, answered by a raw response bound to another Change's revision,
/// reads identically on the derived and authoritative lanes.
#[test]
fn derived_change_show_matches_authoritative_obligations_for_a_foreign_revision_response() {
    let fixture = change_reads_fixture();
    let request_id = open_operative_request(&fixture, &fixture.parallel_revision_ids.0);
    append_foreign_revision_response(
        fixture.repo.path(),
        &request_id,
        &fixture.accepted_revision_id,
    );
    fixture.build_derived();

    assert_show_obligations_cleared_on_both_lanes(&fixture, &fixture.parallel_change_id);
}

/// End-to-end #723 parity: the same arrangement with a revision-less
/// response. Pre-fix this store could not even build a derived generation
/// (the shape quarantined the apply); now it builds and both lanes agree.
#[test]
fn derived_change_show_matches_authoritative_obligations_for_a_revision_less_response() {
    let fixture = change_reads_fixture();
    let request_id = open_operative_request(&fixture, &fixture.parallel_revision_ids.0);
    append_revision_less_response(fixture.repo.path(), &request_id);
    fixture.build_derived();

    assert_show_obligations_cleared_on_both_lanes(&fixture, &fixture.parallel_change_id);
}

/// Unknown and malformed Change ids produce byte-identical outcomes on both
/// lanes: a lookup miss is never a derived-lane hazard.
#[test]
fn derived_show_unknown_and_malformed_change_match_the_authoritative_lane() {
    let fixture = change_reads_fixture();
    fixture.build_derived();

    let unknown = format!("change:sha256:{}", "f".repeat(64));
    for selector in [unknown.as_str(), "not-a-change-id"] {
        let args = ["change", "show", selector, "--repo", fixture.repo_arg()];
        let active = pointbreak_env(args, ACTIVE);
        let off = pointbreak_env(args, OFF);
        assert_eq!(
            active.status.code(),
            off.status.code(),
            "show {selector}: exit parity"
        );
        assert!(!off.status.success(), "show {selector} must fail");
        assert_eq!(active.stdout, off.stdout, "show {selector}: stdout parity");
        assert_eq!(active.stderr, off.stderr, "show {selector}: stderr parity");
        assert!(
            String::from_utf8_lossy(&off.stderr)
                .contains(&format!("Change {selector} is unavailable")),
            "show {selector}: authoritative message"
        );
    }
}

/// The three stamp families at one store state are pairwise distinct and each
/// internally consistent: the authoritative facade stamp, the derived page
/// (generation) stamp, and the derived seek stamp.
#[test]
fn mixed_lane_stamps_are_three_way_distinct_and_internally_consistent() {
    let fixture = change_reads_fixture();
    fixture.build_derived();

    let stamp = |output: &Output| {
        parse_json(&output.stdout)["projectionStamp"]
            .as_str()
            .expect("projection stamp")
            .to_owned()
    };
    let list_args = ["change", "list", "--repo", fixture.repo_arg()];
    let show_args = [
        "change",
        "show",
        &fixture.parallel_change_id,
        "--repo",
        fixture.repo_arg(),
    ];
    let page_stamp = stamp(&pointbreak_env(list_args, ACTIVE));
    let seek_stamp = stamp(&pointbreak_env(show_args, ACTIVE));
    let facade_stamp = stamp(&pointbreak_env(show_args, OFF));

    assert_ne!(
        facade_stamp, page_stamp,
        "facade stamp != derived page stamp"
    );
    assert_ne!(
        facade_stamp, seek_stamp,
        "facade stamp != derived seek stamp"
    );
    assert_ne!(
        page_stamp, seek_stamp,
        "derived page stamp != derived seek stamp"
    );
    assert_eq!(
        seek_stamp,
        stamp(&pointbreak_env(show_args, ACTIVE)),
        "two derived show reads at one store state agree"
    );
}

/// The interdiff CLI document carries no stamp field, so the derived lane is
/// full byte parity with the explicit-off lane — no substitution.
#[test]
fn derived_change_interdiff_is_byte_identical_modulo_no_stamp() {
    let fixture = change_reads_fixture();
    fixture.build_derived();
    let (from, to) = parallel_heads(&fixture);

    for lane in FORMAT_LANES {
        let args = [
            "change",
            "interdiff",
            &fixture.parallel_change_id,
            from["revisionId"].as_str().expect("from revision id"),
            to["revisionId"].as_str().expect("to revision id"),
            "--from-artifact-hash",
            from["objectArtifactContentHash"]
                .as_str()
                .expect("from hash"),
            "--to-artifact-hash",
            to["objectArtifactContentHash"].as_str().expect("to hash"),
            "--repo",
            fixture.repo_arg(),
            "--format",
            lane,
        ];
        let active = pointbreak_env(args, ACTIVE);
        let off = pointbreak_env(args, OFF);
        assert_success(&active);
        assert_eq!(
            active.status.code(),
            off.status.code(),
            "interdiff ({lane}): exit parity"
        );
        assert_eq!(
            active.stdout, off.stdout,
            "interdiff ({lane}): full stdout byte parity"
        );
        assert_eq!(
            active.stderr, off.stderr,
            "interdiff ({lane}): stderr parity"
        );
        assert!(
            parse_json(&active.stdout).get("projectionStamp").is_none(),
            "interdiff ({lane}): the CLI document carries no stamp"
        );
    }
}

/// The floor's endpoint-ordering contract holds unchanged on the derived
/// lane, byte-identically.
#[test]
fn derived_interdiff_validates_from_before_to() {
    let fixture = change_reads_fixture();
    fixture.build_derived();
    let (first_head, second_head) = parallel_heads(&fixture);

    let args = [
        "change",
        "interdiff",
        &fixture.parallel_change_id,
        &fixture.accepted_revision_id,
        second_head["revisionId"].as_str().expect("to revision id"),
        "--from-artifact-hash",
        first_head["objectArtifactContentHash"]
            .as_str()
            .expect("borrowed hash"),
        "--to-artifact-hash",
        first_head["objectArtifactContentHash"]
            .as_str()
            .expect("mismatched hash"),
        "--repo",
        fixture.repo_arg(),
    ];
    let active = pointbreak_env(args, ACTIVE);
    let off = pointbreak_env(args, OFF);
    assert!(!active.status.success());
    assert_eq!(active.status.code(), off.status.code());
    assert_eq!(active.stdout, off.stdout);
    assert_eq!(active.stderr, off.stderr);
    assert!(
        String::from_utf8_lossy(&active.stderr)
            .contains("exact Revision is not an active member of the Change"),
        "the from endpoint's failure surfaces first on the derived lane"
    );
}

fn assert_select_parity(
    fixture: &ChangeReadsFixture,
    extra: &[&str],
    label: &str,
) -> (Output, Output) {
    let mut args = vec!["change", "select"];
    args.extend_from_slice(extra);
    args.extend_from_slice(&["--repo", fixture.repo_arg()]);
    let active = pointbreak_env(&args, ACTIVE);
    let off = pointbreak_env(&args, OFF);
    assert_eq!(
        active.status.code(),
        off.status.code(),
        "{label}: exit parity"
    );
    assert_eq!(active.stdout, off.stdout, "{label}: stdout parity");
    assert_eq!(active.stderr, off.stderr, "{label}: stderr parity");
    (active, off)
}

/// The captured cursor document binds `change_graph_token`, per-Change and
/// lane-identical — no projection stamp — so the derived lane is full byte
/// parity, and tokens round-trip across lanes.
#[test]
fn derived_captured_select_output_is_fully_byte_identical() {
    let fixture = change_reads_fixture();
    fixture.build_derived();

    let mut tokens = Vec::new();
    for lane in FORMAT_LANES {
        let args = [
            "change",
            "select",
            &fixture.accepted_change_id,
            "--repo",
            fixture.repo_arg(),
            "--format",
            lane,
        ];
        let active = pointbreak_env(args, ACTIVE);
        let off = pointbreak_env(args, OFF);
        assert_success(&active);
        assert_eq!(
            active.stdout, off.stdout,
            "select ({lane}): full stdout byte parity"
        );
        assert_eq!(active.stderr, off.stderr, "select ({lane}): stderr parity");
        tokens.push(
            parse_json(&off.stdout)["token"]
                .as_str()
                .expect("selection token")
                .to_owned(),
        );
    }

    // A token minted on either lane revalidates identically on both.
    let token = tokens[0].as_str();
    let (revalidated_active, _) = assert_select_parity(
        &fixture,
        &[&fixture.accepted_change_id, "--cursor", token],
        "cursor revalidation",
    );
    assert_success(&revalidated_active);
    assert_eq!(
        parse_json(&revalidated_active.stdout)["token"]
            .as_str()
            .expect("revalidated token"),
        token,
        "revalidation at an unchanged graph reissues the identical token"
    );
}

/// Every refusal code reachable on the derived captured arm is byte-identical
/// JSON on stderr with a non-zero exit on both lanes.
#[test]
fn derived_select_refusals_are_byte_identical_json_on_stderr() {
    let refusal_code = |output: &Output| {
        assert!(!output.status.success(), "refusal exits non-zero");
        assert!(
            output.stdout.is_empty(),
            "refusal writes no stdout document"
        );
        parse_json(&output.stderr)["code"]
            .as_str()
            .expect("refusal code")
            .to_owned()
    };

    // explicit_revision_required + revision_not_a_change_member on the shared
    // fixture's parallel Change.
    let fixture = change_reads_fixture();
    fixture.build_derived();
    let (active, _) = assert_select_parity(
        &fixture,
        &[&fixture.parallel_change_id],
        "explicit_revision_required",
    );
    assert_eq!(refusal_code(&active), "explicit_revision_required");
    let (active, _) = assert_select_parity(
        &fixture,
        &[
            &fixture.parallel_change_id,
            "--revision",
            &fixture.accepted_revision_id,
        ],
        "revision_not_a_change_member",
    );
    assert_eq!(refusal_code(&active), "revision_not_a_change_member");

    // historical_revision_not_authorable: a replaced member without
    // --allow-historical.
    let historical = {
        let repo = support::dump_repo();
        let repo_arg = repo
            .path()
            .to_str()
            .expect("fixture path is UTF-8")
            .to_owned();
        let first = capture(&["capture", "--repo", &repo_arg]);
        let change_id = first["changeId"].as_str().expect("change id").to_owned();
        let historical_revision = first["revision"]["revisionId"]
            .as_str()
            .expect("first revision id")
            .to_owned();
        let cursor = first["reviewCursor"]["token"]
            .as_str()
            .expect("first review cursor")
            .to_owned();
        repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
        capture(&[
            "capture",
            "--repo",
            &repo_arg,
            "--review-cursor",
            &cursor,
            "--advance",
            "replace",
        ]);
        (repo, change_id, historical_revision)
    };
    let build = pointbreak_env(
        [
            "store",
            "derived",
            "build",
            "--repo",
            historical.0.path().to_str().unwrap(),
        ],
        ACTIVE,
    );
    assert_success(&build);
    let args = [
        "change",
        "select",
        &historical.1,
        "--revision",
        &historical.2,
        "--repo",
        historical.0.path().to_str().unwrap(),
    ];
    let active = pointbreak_env(args, ACTIVE);
    let off = pointbreak_env(args, OFF);
    assert_eq!(active.stderr, off.stderr, "historical: stderr parity");
    assert_eq!(active.status.code(), off.status.code());
    assert_eq!(refusal_code(&active), "historical_revision_not_authorable");

    // change_graph_stale: a captured-bound token minted before the graph
    // changed stays eligible and refuses identically.
    let fixture = change_reads_fixture();
    let stale_token = parse_json(
        &pointbreak_env(
            [
                "change",
                "select",
                &fixture.accepted_change_id,
                "--repo",
                fixture.repo_arg(),
            ],
            OFF,
        )
        .stdout,
    )["token"]
        .as_str()
        .expect("pre-advance token")
        .to_owned();
    fixture
        .repo
        .write("src/lib.rs", "pub fn value() -> u32 { 6 }\n");
    capture(&[
        "capture",
        "--repo",
        fixture.repo_arg(),
        "--review-cursor",
        &stale_token,
        "--advance",
        "parallel",
    ]);
    fixture.build_derived();
    let (active, _) = assert_select_parity(
        &fixture,
        &[&fixture.accepted_change_id, "--cursor", &stale_token],
        "change_graph_stale",
    );
    assert_eq!(refusal_code(&active), "change_graph_stale");

    // review_cursor_selection_stale: a crafted captured-bound token whose
    // graph token still matches but whose recorded selection differs.
    let fixture = change_reads_fixture();
    fixture.build_derived();
    let selection = parse_json(
        &pointbreak_env(
            [
                "change",
                "select",
                &fixture.parallel_change_id,
                "--revision",
                &fixture.parallel_revision_ids.0,
                "--repo",
                fixture.repo_arg(),
            ],
            OFF,
        )
        .stdout,
    );
    let token = selection["token"].as_str().expect("parallel token");
    let mut crafted =
        pointbreak::session::ReviewCursorV1::decode_token(token).expect("decode parallel token");
    assert_eq!(crafted.selected_current_revisions.len(), 2);
    crafted.selected_current_revisions.truncate(1);
    let crafted_token = crafted.encode_token().expect("re-encode crafted token");
    let (active, _) = assert_select_parity(
        &fixture,
        &[
            &fixture.parallel_change_id,
            "--revision",
            &fixture.parallel_revision_ids.0,
            "--cursor",
            &crafted_token,
        ],
        "review_cursor_selection_stale",
    );
    assert_eq!(refusal_code(&active), "review_cursor_selection_stale");

    // change_state_unresolved: replacement divergence — two successors
    // asserted for one predecessor through the low-level relation command.
    {
        let repo = support::dump_repo();
        let repo_arg = repo
            .path()
            .to_str()
            .expect("fixture path is UTF-8")
            .to_owned();
        let first = capture(&["capture", "--repo", &repo_arg]);
        let change_id = first["changeId"].as_str().expect("change id").to_owned();
        let replaced = first["revision"]["revisionId"]
            .as_str()
            .expect("first revision id")
            .to_owned();
        let first_cursor = first["reviewCursor"]["token"]
            .as_str()
            .expect("first cursor")
            .to_owned();
        repo.write("src/lib.rs", "pub fn value() -> u32 { 21 }\n");
        let second = capture(&[
            "capture",
            "--repo",
            &repo_arg,
            "--review-cursor",
            &first_cursor,
            "--advance",
            "replace",
        ]);
        let second_cursor = second["reviewCursor"]["token"]
            .as_str()
            .expect("second cursor")
            .to_owned();
        let second_revision = second["revision"]["revisionId"]
            .as_str()
            .expect("second revision id")
            .to_owned();
        repo.write("src/lib.rs", "pub fn value() -> u32 { 22 }\n");
        let third = capture(&[
            "capture",
            "--repo",
            &repo_arg,
            "--review-cursor",
            &second_cursor,
            "--advance",
            "parallel",
        ]);
        let third_revision = third["revision"]["revisionId"]
            .as_str()
            .expect("third revision id")
            .to_owned();

        let show = parse_json(
            &pointbreak_env(["change", "show", &change_id, "--repo", &repo_arg], OFF).stdout,
        );
        let hash_of = |revision: &str| {
            show["memberRevisions"]
                .as_array()
                .expect("member revisions")
                .iter()
                .find(|member| member["revision"]["revisionId"] == revision)
                .expect("member present")["revision"]["objectArtifactContentHash"]
                .as_str()
                .expect("artifact hash")
                .to_owned()
        };
        let diverging = pointbreak_env(
            [
                "change",
                "assert-relation",
                &change_id,
                &third_revision,
                &replaced,
                "--successor-artifact-hash",
                &hash_of(&third_revision),
                "--predecessor-artifact-hash",
                &hash_of(&replaced),
                "--repo",
                &repo_arg,
                "--operation-id",
                "change-operation:divergence",
            ],
            OFF,
        );
        assert_success(&diverging);
        let build = pointbreak_env(["store", "derived", "build", "--repo", &repo_arg], ACTIVE);
        assert_success(&build);
        let args = [
            "change",
            "select",
            &change_id,
            "--revision",
            &second_revision,
            "--repo",
            &repo_arg,
        ];
        let active = pointbreak_env(args, ACTIVE);
        let off = pointbreak_env(args, OFF);
        assert_eq!(active.status.code(), off.status.code(), "unresolved");
        assert_eq!(active.stdout, off.stdout, "unresolved: stdout parity");
        assert_eq!(active.stderr, off.stderr, "unresolved: stderr parity");
        assert_eq!(refusal_code(&active), "change_state_unresolved");
    }

    // revision_artifact_conflicted: a raw second proposal carrier binds a
    // current member to a second artifact hash. A store holding this shape
    // fails the derived build's strict verification, and no governed writer
    // produces it, so a derived-current generation cannot observe it: the
    // generation here is built BEFORE the carrier arrives, the active lane
    // falls back behind the moved truth, and both lanes stay byte-identical.
    // The derived arm shares the one pure selection helper, so the refusal
    // itself cannot diverge structurally.
    let fixture = change_reads_fixture();
    fixture.build_derived();
    append_conflicting_proposal(
        fixture.repo.path(),
        &fixture.accepted_revision_id,
        &format!("sha256:{}", "9c".repeat(32)),
    );
    let (active, _) = assert_select_parity(
        &fixture,
        &[&fixture.accepted_change_id],
        "revision_artifact_conflicted",
    );
    assert_eq!(refusal_code(&active), "revision_artifact_conflicted");

    // revision_artifact_missing: a raw membership claim plus a proposal whose
    // artifact hash is not a canonical reference — the member resolves into
    // the unavailable set, so its exact reference is absent.
    // As with the conflicted carrier, a raw proposal fails the derived
    // build's strict verification, so the generation is built first and the
    // active lane falls back behind the moved truth, byte-identically.
    let fixture = change_reads_fixture();
    fixture.build_derived();
    let fabricated = format!("rev:sha256:{}", "8d".repeat(32));
    append_membership_of_unproposed_revision(
        fixture.repo.path(),
        &fixture.accepted_change_id,
        &fabricated,
    );
    append_conflicting_proposal(fixture.repo.path(), &fabricated, "not-a-canonical-hash");
    let (active, _) = assert_select_parity(
        &fixture,
        &[&fixture.accepted_change_id],
        "revision_artifact_missing",
    );
    assert_eq!(refusal_code(&active), "revision_artifact_missing");
}

/// Mutable sources and mutable-bound prior cursors never enter the derived
/// lane: their bytes and error ordering are today's authoritative behavior,
/// including under an explicit `--source captured` override.
#[test]
fn mutable_sources_and_mutable_bound_cursors_never_enter_the_derived_lane() {
    let fixture = change_reads_fixture();
    fixture.build_derived();

    // Explicit mutable sources.
    let (active, _) = assert_select_parity(
        &fixture,
        &[&fixture.accepted_change_id, "--source", "worktree"],
        "--source worktree",
    );
    // The fixture worktree still matches the accepted capture, so the
    // worktree arm succeeds authoritatively on both lanes and emits a
    // worktree-bound cursor; the parity above is the contract.
    assert_success(&active);
    assert_eq!(
        parse_json(&active.stdout)["cursor"]["sourceBinding"]["kind"],
        "worktree_match_v1",
        "the worktree arm emits a worktree-bound cursor"
    );
    assert_select_parity(
        &fixture,
        &[&fixture.accepted_change_id, "--source", "commit:HEAD"],
        "--source commit:HEAD",
    );

    // A worktree-bound prior cursor: mint one on a fresh capture whose
    // worktree still matches.
    let repo = support::dump_repo();
    let repo_arg = repo
        .path()
        .to_str()
        .expect("fixture path is UTF-8")
        .to_owned();
    let first = capture(&["capture", "--repo", &repo_arg]);
    let change_id = first["changeId"].as_str().expect("change id").to_owned();
    let bound = pointbreak_env(
        [
            "change", "select", &change_id, "--source", "worktree", "--repo", &repo_arg,
        ],
        OFF,
    );
    assert_success(&bound);
    let worktree_token = parse_json(&bound.stdout)["token"]
        .as_str()
        .expect("worktree-bound token")
        .to_owned();
    let build = pointbreak_env(["store", "derived", "build", "--repo", &repo_arg], ACTIVE);
    assert_success(&build);
    for (label, extra) in [
        (
            "worktree-bound cursor",
            vec![change_id.as_str(), "--cursor", worktree_token.as_str()],
        ),
        (
            "worktree-bound cursor with explicit --source captured",
            vec![
                change_id.as_str(),
                "--cursor",
                worktree_token.as_str(),
                "--source",
                "captured",
            ],
        ),
    ] {
        let mut args = vec!["change", "select"];
        args.extend_from_slice(&extra);
        args.extend_from_slice(&["--repo", &repo_arg]);
        let active = pointbreak_env(&args, ACTIVE);
        let off = pointbreak_env(&args, OFF);
        assert_eq!(active.status.code(), off.status.code(), "{label}");
        assert_eq!(active.stdout, off.stdout, "{label}: stdout parity");
        assert_eq!(active.stderr, off.stderr, "{label}: stderr parity");
    }

    // A mutable-bound cursor whose worktree moved: today's authoritative
    // error form, identical on both lanes (source_binding_mismatch is not
    // derived-reachable).
    repo.write("src/lib.rs", "pub fn value() -> u32 { 9 }\n");
    let args = [
        "change",
        "select",
        &change_id,
        "--cursor",
        &worktree_token,
        "--repo",
        &repo_arg,
    ];
    let active = pointbreak_env(args, ACTIVE);
    let off = pointbreak_env(args, OFF);
    assert!(!off.status.success());
    assert_eq!(active.status.code(), off.status.code());
    assert_eq!(active.stdout, off.stdout);
    assert_eq!(active.stderr, off.stderr);

    // An undecodable cursor is ineligible; the authoritative decode error
    // surfaces in place (invalid_review_cursor is not derived-reachable as a
    // selection refusal).
    let (active, _) = assert_select_parity(
        &fixture,
        &[&fixture.accepted_change_id, "--cursor", "not-a-token"],
        "undecodable cursor",
    );
    assert!(!active.status.success());
    assert!(
        String::from_utf8_lossy(&active.stderr).contains("invalid Review cursor token"),
        "the in-facade decode error surfaces in its existing position"
    );
}

/// The R08 ordering pins: capability documents precede cursor decoding on
/// L0, and a bad `--source` value surfaces from the authoritative arm in its
/// existing position — after a bad cursor, never earlier.
#[test]
fn l0_with_a_malformed_cursor_still_emits_the_typed_document() {
    let repo = tempfile::tempdir().expect("temporary L0 repository");
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repo.path())
            .status()
            .expect("run git init")
            .success()
    );
    let repo_arg = repo.path().to_str().expect("L0 path is UTF-8");

    for extra in [
        vec!["--cursor", "not-a-token"],
        vec!["--source", "bogus"],
        vec!["--cursor", "not-a-token", "--source", "bogus"],
    ] {
        let mut args = vec!["change", "select", "change:sha256:l0-selector"];
        args.extend_from_slice(&extra);
        args.extend_from_slice(&["--repo", repo_arg]);
        let active = pointbreak_unprepared_env(&args, ACTIVE);
        let off = pointbreak_unprepared_env(&args, OFF);
        assert!(
            off.status.success(),
            "L0 select answers with the typed document: {}",
            String::from_utf8_lossy(&off.stderr)
        );
        assert_eq!(active.stdout, off.stdout, "{extra:?}: stdout parity");
        assert_eq!(active.stderr, off.stderr, "{extra:?}: stderr parity");
        let json = parse_json(&off.stdout);
        assert_eq!(json["schema"], "pointbreak.store-migration-required");
    }
}

/// A bad `--source` value surfaces from the authoritative arm in its
/// existing position on a live store; combined with a bad cursor, the
/// in-facade cursor decode error stays first.
#[test]
fn invalid_source_errors_surface_in_their_existing_position() {
    let fixture = change_reads_fixture();
    fixture.build_derived();

    let (active, _) = assert_select_parity(
        &fixture,
        &[&fixture.accepted_change_id, "--source", "bogus"],
        "bad source",
    );
    assert!(!active.status.success());
    assert!(
        String::from_utf8_lossy(&active.stderr)
            .contains("--source must be captured, worktree, or commit:<rev>"),
        "the parse error surfaces in its existing position"
    );

    let (active, _) = assert_select_parity(
        &fixture,
        &[
            &fixture.accepted_change_id,
            "--cursor",
            "not-a-token",
            "--source",
            "bogus",
        ],
        "bad cursor + bad source",
    );
    assert!(!active.status.success());
    assert!(
        String::from_utf8_lossy(&active.stderr).contains("invalid Review cursor token"),
        "the cursor decode error surfaces before the source parse error"
    );
}

/// The `--allow-historical` SUCCESS document is byte-identical across lanes
/// (the floor pinned only the refusal path cross-lane).
#[test]
fn derived_allow_historical_success_is_byte_identical() {
    let repo = support::dump_repo();
    let repo_arg = repo
        .path()
        .to_str()
        .expect("fixture path is UTF-8")
        .to_owned();
    let first = capture(&["capture", "--repo", &repo_arg]);
    let change_id = first["changeId"].as_str().expect("change id").to_owned();
    let historical_revision = first["revision"]["revisionId"]
        .as_str()
        .expect("first revision id")
        .to_owned();
    let cursor = first["reviewCursor"]["token"]
        .as_str()
        .expect("first review cursor")
        .to_owned();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    capture(&[
        "capture",
        "--repo",
        &repo_arg,
        "--review-cursor",
        &cursor,
        "--advance",
        "replace",
    ]);
    let build = pointbreak_env(["store", "derived", "build", "--repo", &repo_arg], ACTIVE);
    assert_success(&build);

    for lane in FORMAT_LANES {
        let args = [
            "change",
            "select",
            &change_id,
            "--revision",
            &historical_revision,
            "--allow-historical",
            "--repo",
            &repo_arg,
            "--format",
            lane,
        ];
        let active = pointbreak_env(args, ACTIVE);
        let off = pointbreak_env(args, OFF);
        assert_success(&active);
        assert_eq!(
            active.stdout, off.stdout,
            "allow-historical ({lane}): full stdout byte parity"
        );
        assert_eq!(
            active.stderr, off.stderr,
            "allow-historical ({lane}): stderr parity"
        );
    }
}

#[cfg(feature = "longitudinal-counting")]
mod counted {
    use std::path::Path;

    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use pointbreak::bench_support::longitudinal::{
        LONGITUDINAL_COUNTER_RECEIPT_HEADER_V1, LONGITUDINAL_COUNTING_REQUEST_HEADER_V1,
        LongitudinalCounterReceiptV1,
    };
    use serde_json::Value;
    use sha2::{Digest as _, Sha256};

    use super::support::inspect::{Inspector, urlencode};
    use super::{
        ACTIVE, ChangeReadsFixture, assert_success, change_reads_fixture, parse_json,
        pointbreak_env,
    };

    /// Distinct proposal-independent history large enough that a complete
    /// fold's per-event cost is visibly proportional to it.
    const UNRELATED_EVENTS: usize = 12;

    fn counted_counters(
        fixture: &ChangeReadsFixture,
        subcommand: &str,
        receipt_dir: &Path,
        ordinal: u64,
    ) -> Value {
        counted_counters_for(fixture, &[subcommand], receipt_dir, ordinal)
    }

    fn counted_counters_for(
        fixture: &ChangeReadsFixture,
        subcommand: &[&str],
        receipt_dir: &Path,
        ordinal: u64,
    ) -> Value {
        let (output, counters) = counted_outcome_for(fixture, subcommand, receipt_dir, ordinal);
        assert_success(&output);
        counters
    }

    /// Like [`counted_counters_for`], but tolerates a failing exit: the
    /// eligibility falsifier asserts seek counters on refused shapes too.
    fn counted_outcome_for(
        fixture: &ChangeReadsFixture,
        subcommand: &[&str],
        receipt_dir: &Path,
        ordinal: u64,
    ) -> (std::process::Output, Value) {
        let receipt_path = receipt_dir.join(format!("receipt-{ordinal}.json"));
        let request = serde_json::json!({
            "runIdentity": format!("{:064x}", ordinal + 1),
            "context": {
                "rootIdentity": "2".repeat(64),
                "operation": "CHANGE_READ_CONTRACT",
                "phase": format!("case-{ordinal}"),
                "baseExecutionIdentitySha256": "3".repeat(64),
                "derivativeExecutionIdentitySha256": "4".repeat(64),
                "manifestSha256": "5".repeat(64),
                "scheduleSha256": "6".repeat(64),
                "success": false,
                "semanticResultSha256": "7".repeat(64),
                "includeCapacityOwnership": false
            },
            "receiptPath": receipt_path,
        });
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).expect("encode request"));
        let mut arguments = vec!["--longitudinal-counting", &encoded, "change"];
        arguments.extend_from_slice(subcommand);
        arguments.extend_from_slice(&["--repo", fixture.repo_arg()]);
        let output = pointbreak_env(&arguments, ACTIVE);
        let counters =
            parse_json(&std::fs::read(receipt_path).expect("read counted receipt"))["counters"]
                .clone();
        (output, counters)
    }

    fn counter(counters: &Value, name: &str) -> u64 {
        counters
            .get(name)
            .map_or(0, |value| value.as_u64().expect("counter is a u64"))
    }

    fn counted_http_get(
        inspector: &Inspector,
        path: &str,
        ordinal: u64,
    ) -> (String, LongitudinalCounterReceiptV1) {
        let baseline = inspector.get_text(path);
        let semantic_result_sha256 = format!("{:x}", Sha256::digest(baseline.as_bytes()));
        let request = serde_json::json!({
            "runIdentity": format!("{:064x}", ordinal + 1),
            "context": {
                "rootIdentity": "2".repeat(64),
                "operation": "EXACT_CHANGE_HTTP",
                "phase": format!("case-{ordinal}"),
                "baseExecutionIdentitySha256": "3".repeat(64),
                "derivativeExecutionIdentitySha256": "4".repeat(64),
                "manifestSha256": "5".repeat(64),
                "scheduleSha256": "6".repeat(64),
                "success": true,
                "semanticResultSha256": semantic_result_sha256,
                "includeCapacityOwnership": false
            }
        });
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).expect("encode request"));
        let authorization = format!(
            "Bearer {}",
            inspector.token().expect("authenticated Inspector token")
        );
        let (head, body) = inspector.raw_request(
            "GET",
            path,
            &[
                ("Host", inspector.canonical_host()),
                ("Authorization", &authorization),
                (LONGITUDINAL_COUNTING_REQUEST_HEADER_V1, &encoded),
            ],
        );
        assert!(head.starts_with("HTTP/1.1 200"), "{head}: {body}");
        assert_eq!(body, baseline, "counting must not change response bytes");
        let encoded_receipt = head
            .lines()
            .skip(1)
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case(LONGITUDINAL_COUNTER_RECEIPT_HEADER_V1)
                    .then(|| value.trim())
            })
            .expect("counted response receipt header");
        let receipt: LongitudinalCounterReceiptV1 = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(encoded_receipt)
                .expect("decode counter receipt"),
        )
        .expect("parse counter receipt");
        receipt.validate().expect("valid counter receipt");
        assert_eq!(receipt.semantic_result_sha256, semantic_result_sha256);
        (body, receipt)
    }

    fn assert_route_pins(counters: &Value, label: &str) {
        for pin in [
            "strictJournalInspections",
            "bodyArtifactReads",
            "objectArtifactReads",
        ] {
            assert_eq!(counter(counters, pin), 0, "{label}: {pin} stays zero");
        }
    }

    /// Post-Green end-to-end verification of the exact derived HTTP dispatch.
    #[test]
    fn exact_inspector_derived_routes_open_no_authoritative_carriers() {
        let fixture = change_reads_fixture();
        fixture.build_derived();
        let inspector = Inspector::spawn_current(fixture.repo.path());
        let changes = inspector.get_json("/api/v2/changes");
        let change = changes["changes"]
            .as_array()
            .expect("Change list")
            .iter()
            .find(|change| change["changeId"] == fixture.parallel_change_id)
            .expect("parallel Change summary");
        let heads = change["currentRevisionRefs"]
            .as_array()
            .expect("parallel current Revisions");
        assert_eq!(heads.len(), 2, "fixture keeps two parallel heads");

        let detail_path = format!("/api/v2/changes/{}", urlencode(&fixture.parallel_change_id));
        let interdiff_path = format!(
            "/api/v2/changes/{}/interdiff/{}/{}?fromArtifactHash={}&toArtifactHash={}",
            urlencode(&fixture.parallel_change_id),
            urlencode(heads[0]["revisionId"].as_str().expect("first Revision id")),
            urlencode(heads[1]["revisionId"].as_str().expect("second Revision id")),
            urlencode(
                heads[0]["objectArtifactContentHash"]
                    .as_str()
                    .expect("first artifact hash")
            ),
            urlencode(
                heads[1]["objectArtifactContentHash"]
                    .as_str()
                    .expect("second artifact hash")
            ),
        );

        for (ordinal, (label, path)) in [
            ("detail", detail_path.as_str()),
            ("interdiff", interdiff_path.as_str()),
        ]
        .into_iter()
        .enumerate()
        {
            let (_, receipt) = counted_http_get(&inspector, path, 100 + ordinal as u64);
            assert_eq!(
                receipt.counters.event_decodes, 0,
                "{label} decodes no event"
            );
            assert_eq!(
                receipt.counters.change_proposal_carriers_opened, 0,
                "{label} opens no proposal carrier"
            );
            assert_eq!(
                receipt.counters.change_support_carriers_opened, 0,
                "{label} opens no support carrier"
            );
        }
    }

    /// The derived profile answers from the pinned checkpoint plus the two
    /// capability carriers: no event decode, no proposal or support carrier
    /// open, even with unrelated history present.
    #[test]
    fn change_profile_active_current_opens_only_capability_carriers() {
        let fixture = change_reads_fixture();
        fixture.grow_unrelated_history(UNRELATED_EVENTS);
        fixture.build_derived();
        let receipt_dir = tempfile::tempdir().expect("receipt directory");

        let counters = counted_counters(&fixture, "profile", receipt_dir.path(), 0);
        assert_eq!(
            counter(&counters, "eventDecodes"),
            0,
            "derived profile decodes no event"
        );
        assert_eq!(
            counter(&counters, "changeProposalCarriersOpened"),
            0,
            "derived profile opens no proposal carrier"
        );
        assert_eq!(
            counter(&counters, "changeSupportCarriersOpened"),
            0,
            "derived profile opens no support carrier"
        );
        assert_route_pins(&counters, "profile");
    }

    /// The derived page reads open work proportional to the selected proposal
    /// and support carriers: growing unrelated event history changes neither
    /// the decode count nor the carrier-open count, and the existing
    /// Change-page counters are recorded.
    #[test]
    fn change_list_and_attention_active_current_stay_carrier_proportional() {
        let fixture = change_reads_fixture();
        fixture.build_derived();
        let receipt_dir = tempfile::tempdir().expect("receipt directory");

        for (ordinal, subcommand) in ["list", "attention"].into_iter().enumerate() {
            let ordinal = ordinal as u64 * 2;
            let before = counted_counters(&fixture, subcommand, receipt_dir.path(), ordinal);
            fixture.grow_unrelated_history(UNRELATED_EVENTS);
            fixture.build_derived();
            let after = counted_counters(&fixture, subcommand, receipt_dir.path(), ordinal + 1);

            for invariant in ["eventDecodes", "carrierOpens"] {
                assert_eq!(
                    counter(&before, invariant),
                    counter(&after, invariant),
                    "change {subcommand}: {invariant} is invariant under unrelated growth \
                     (before {before:#}, after {after:#})"
                );
            }
            for recorded in [
                "changeCandidates",
                "changeProposalCarriersOpened",
                "changeRowsEmitted",
            ] {
                assert!(
                    counter(&after, recorded) > 0,
                    "change {subcommand}: derived page records {recorded}"
                );
            }
            assert_route_pins(&after, subcommand);
        }
    }

    /// The R05 seek falsifier: with unrelated history and unrelated Changes
    /// present, the derived seek lane opens zero authoritative material —
    /// no event decode, no proposal/support carrier, no body or object
    /// artifact, no strict journal inspection — and its one honest counter,
    /// `changeSeekFactRowsSelected`, is proportional to the target Change's
    /// correlated fact rows and invariant as unrelated Changes and history
    /// grow. Zero-construction assertions are deliberately absent (the
    /// narrowed fold still constructs), and `factSqliteRowsSelected` belongs
    /// to exact-Revision selections, never asserted here.
    #[test]
    fn change_seek_reads_active_current_stay_zero_pin_and_row_invariant() {
        let fixture = change_reads_fixture();
        fixture.build_derived();
        let receipt_dir = tempfile::tempdir().expect("receipt directory");

        let list = super::parse_json(
            &pointbreak_env(["change", "list", "--repo", fixture.repo_arg()], ACTIVE).stdout,
        );
        let parallel = list["changes"]
            .as_array()
            .expect("changes array")
            .iter()
            .find(|change| change["changeId"] == fixture.parallel_change_id.as_str())
            .expect("parallel Change summary")
            .clone();
        let heads = parallel["currentRevisionRefs"]
            .as_array()
            .expect("parallel heads")
            .clone();

        let show: Vec<String> = vec!["show".into(), fixture.accepted_change_id.clone()];
        let select: Vec<String> = vec!["select".into(), fixture.accepted_change_id.clone()];
        let interdiff: Vec<String> = vec![
            "interdiff".into(),
            fixture.parallel_change_id.clone(),
            heads[0]["revisionId"].as_str().expect("head id").into(),
            heads[1]["revisionId"].as_str().expect("head id").into(),
            "--from-artifact-hash".into(),
            heads[0]["objectArtifactContentHash"]
                .as_str()
                .expect("head hash")
                .into(),
            "--to-artifact-hash".into(),
            heads[1]["objectArtifactContentHash"]
                .as_str()
                .expect("head hash")
                .into(),
        ];
        let shapes: [(&str, &[String]); 3] = [
            ("show", &show),
            ("select", &select),
            ("interdiff", &interdiff),
        ];

        let assert_seek_pins = |counters: &Value, label: &str| {
            for pin in [
                "eventDecodes",
                "changeProposalCarriersOpened",
                "changeProposalCarriersValidated",
                "changeSupportCarriersOpened",
                "bodyArtifactReads",
                "objectArtifactReads",
                "strictJournalInspections",
            ] {
                assert_eq!(counter(counters, pin), 0, "{label}: {pin} stays zero");
            }
            assert!(
                counter(counters, "changeSeekFactRowsSelected") > 0,
                "{label}: the seek records its selected fact rows"
            );
        };

        let mut before = Vec::new();
        for (ordinal, (label, shape)) in shapes.iter().enumerate() {
            let arguments = shape.iter().map(String::as_str).collect::<Vec<_>>();
            let counters =
                counted_counters_for(&fixture, &arguments, receipt_dir.path(), ordinal as u64);
            assert_seek_pins(&counters, label);
            before.push(counter(&counters, "changeSeekFactRowsSelected"));
        }

        // Unrelated growth: review facts on an existing member plus two whole
        // unrelated Changes with their own captures.
        fixture.grow_unrelated_history(UNRELATED_EVENTS);
        for ordinal in 0..2 {
            fixture.repo.write(
                "src/lib.rs",
                format!("pub fn value() -> u32 {{ {} }}\n", 40 + ordinal),
            );
            let unrelated = pointbreak_env(["capture", "--repo", fixture.repo_arg()], super::OFF);
            super::assert_success(&unrelated);
        }
        fixture.build_derived();

        for (ordinal, (label, shape)) in shapes.iter().enumerate() {
            let arguments = shape.iter().map(String::as_str).collect::<Vec<_>>();
            let counters = counted_counters_for(
                &fixture,
                &arguments,
                receipt_dir.path(),
                10 + ordinal as u64,
            );
            assert_seek_pins(&counters, label);
            assert_eq!(
                counter(&counters, "changeSeekFactRowsSelected"),
                before[ordinal],
                "{label}: selected fact rows are invariant under unrelated growth"
            );
        }
    }

    /// The R08 eligibility gate's falsifier: every ineligible `select` shape
    /// records zero seek rows under the counted harness — byte parity alone
    /// cannot catch a regression that widens eligibility, because both lanes
    /// share one pure selection helper. The eligible captured shapes record
    /// seek rows as the positive contrast, which also proves the generation
    /// is active-current before the zero assertions run. The counter sums
    /// selected rows, so the exact guarantee is "no seek rows were read";
    /// both target Changes demonstrably have rows, which the contrast pins.
    #[test]
    fn ineligible_select_shapes_never_touch_the_seek() {
        let fixture = change_reads_fixture();

        // A Change whose capture matches the live worktree, so a
        // worktree-bound cursor can be minted.
        fixture
            .repo
            .write("src/lib.rs", "pub fn value() -> u32 { 50 }\n");
        let fresh = super::capture(&["capture", "--repo", fixture.repo_arg()]);
        let fresh_change = fresh["changeId"]
            .as_str()
            .expect("fresh change id")
            .to_owned();
        let worktree_bound = pointbreak_env(
            [
                "change",
                "select",
                &fresh_change,
                "--source",
                "worktree",
                "--repo",
                fixture.repo_arg(),
            ],
            super::OFF,
        );
        assert_success(&worktree_bound);
        let worktree_token = parse_json(&worktree_bound.stdout)["token"]
            .as_str()
            .expect("worktree-bound token")
            .to_owned();
        fixture.build_derived();
        let captured_token = parse_json(
            &pointbreak_env(
                [
                    "change",
                    "select",
                    &fixture.accepted_change_id,
                    "--repo",
                    fixture.repo_arg(),
                ],
                super::OFF,
            )
            .stdout,
        )["token"]
            .as_str()
            .expect("captured-bound token")
            .to_owned();
        let receipt_dir = tempfile::tempdir().expect("receipt directory");

        for (ordinal, eligible) in [
            vec!["select", fixture.accepted_change_id.as_str()],
            vec![
                "select",
                fixture.accepted_change_id.as_str(),
                "--cursor",
                captured_token.as_str(),
            ],
        ]
        .into_iter()
        .enumerate()
        {
            let counters =
                counted_counters_for(&fixture, &eligible, receipt_dir.path(), ordinal as u64);
            assert!(
                counter(&counters, "changeSeekFactRowsSelected") > 0,
                "{eligible:?}: the eligible captured shape reads through the seek"
            );
        }

        for (ordinal, ineligible) in [
            vec![
                "select",
                fixture.accepted_change_id.as_str(),
                "--source",
                "worktree",
            ],
            vec![
                "select",
                fixture.accepted_change_id.as_str(),
                "--source",
                "commit:HEAD",
            ],
            vec![
                "select",
                fixture.accepted_change_id.as_str(),
                "--source",
                "bogus",
            ],
            vec![
                "select",
                fixture.accepted_change_id.as_str(),
                "--cursor",
                "not-a-token",
            ],
            vec![
                "select",
                fresh_change.as_str(),
                "--cursor",
                worktree_token.as_str(),
            ],
            vec![
                "select",
                fresh_change.as_str(),
                "--cursor",
                worktree_token.as_str(),
                "--source",
                "captured",
            ],
        ]
        .into_iter()
        .enumerate()
        {
            let (_output, counters) = counted_outcome_for(
                &fixture,
                &ineligible,
                receipt_dir.path(),
                10 + ordinal as u64,
            );
            assert_eq!(
                counter(&counters, "changeSeekFactRowsSelected"),
                0,
                "{ineligible:?}: an ineligible shape must never touch the seek"
            );
        }
    }

    /// The first catching-up CLI fallback case: with the generation behind a
    /// raw append, the derived lane answers nothing — zero seek rows, the
    /// authoritative replay decodes events — and the output stays
    /// byte-identical to the explicit-off lane.
    #[test]
    fn change_show_catching_up_falls_back_with_authoritative_bytes() {
        let fixture = change_reads_fixture();
        fixture.build_derived();
        super::append_membership_of_unproposed_revision(
            fixture.repo.path(),
            &fixture.accepted_change_id,
            &format!("rev:sha256:{}", "7e".repeat(32)),
        );

        let args = [
            "change",
            "show",
            &fixture.parallel_change_id,
            "--repo",
            fixture.repo_arg(),
        ];
        let active = pointbreak_env(args, ACTIVE);
        let off = pointbreak_env(args, super::OFF);
        super::assert_success(&active);
        assert_eq!(active.stdout, off.stdout, "catching-up show stdout parity");
        assert_eq!(active.stderr, off.stderr, "catching-up show stderr parity");

        let receipt_dir = tempfile::tempdir().expect("receipt directory");
        let show: Vec<&str> = vec!["show", &fixture.parallel_change_id];
        let counters = counted_counters_for(&fixture, &show, receipt_dir.path(), 0);
        assert_eq!(
            counter(&counters, "changeSeekFactRowsSelected"),
            0,
            "the seek selects nothing behind a moved truth"
        );
        assert!(
            counter(&counters, "eventDecodes") > 0,
            "the authoritative replay serves the catching-up read"
        );
    }
}

fn parse_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("valid JSON")
}

#[track_caller]
fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
