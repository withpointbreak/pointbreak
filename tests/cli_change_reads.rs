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
    let events_dir = common_dir_store(repo_root).join("events");
    std::fs::create_dir_all(&events_dir).expect("create fixture events directory");
    let stem = format!("{:x}", Sha256::digest(idempotency_key.as_bytes()));
    std::fs::write(
        events_dir.join(format!("{stem}.json")),
        serde_json::to_vec(&event).expect("serialize fixture event"),
    )
    .expect("write fixture event");
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
    let heads = parallel["currentRevisionRefs"]
        .as_array()
        .expect("parallel heads");
    let (first_head, second_head) = (&heads[0], &heads[1]);

    let select = vec![
        "select".to_owned(),
        fixture.accepted_change_id.clone(),
        "--revision".to_owned(),
        accepted_revision.to_owned(),
    ];
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
    let interdiff = vec![
        "interdiff".to_owned(),
        fixture.parallel_change_id.clone(),
        first_head["revisionId"]
            .as_str()
            .expect("first head id")
            .to_owned(),
        second_head["revisionId"]
            .as_str()
            .expect("second head id")
            .to_owned(),
        "--from-artifact-hash".to_owned(),
        first_head["objectArtifactContentHash"]
            .as_str()
            .expect("first head hash")
            .to_owned(),
        "--to-artifact-hash".to_owned(),
        second_head["objectArtifactContentHash"]
            .as_str()
            .expect("second head hash")
            .to_owned(),
    ];

    // `show` left the neighbor set when its derived route landed; `select`
    // and `interdiff` leave with theirs.
    for subcommand in [select, revision, resource, interdiff] {
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

#[cfg(feature = "longitudinal-counting")]
mod counted {
    use std::path::Path;

    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde_json::Value;

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
        let output = pointbreak_env(
            [
                "--longitudinal-counting",
                &encoded,
                "change",
                subcommand,
                "--repo",
                fixture.repo_arg(),
            ],
            ACTIVE,
        );
        assert_success(&output);
        parse_json(&std::fs::read(receipt_path).expect("read counted receipt"))["counters"].clone()
    }

    fn counter(counters: &Value, name: &str) -> u64 {
        counters
            .get(name)
            .map_or(0, |value| value.as_u64().expect("counter is a u64"))
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
