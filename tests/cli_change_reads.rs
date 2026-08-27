//! Characterization floor and route contracts for the three producer-reuse
//! Change CLI reads: `change profile`, `change list`, and `change attention`.
//!
//! The passing tests freeze today's authoritative bytes per format lane; they
//! are the parity oracle for the derived routing and may only change where the
//! documented stamp substitution is the specified difference. The ignored
//! tests are the derived-route contract: they fail while the commands still
//! replay the complete authoritative fold and become the Green targets when
//! the reads route through the existing derived producers.

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
/// the explicit-off and the derived-selected lanes, byte-identically.
fn assert_typed_capability_documents(repo_arg: &str, state: &str) {
    for command in ["profile", "list", "attention"] {
        let off = pointbreak_unprepared_env(["change", command, "--repo", repo_arg], OFF);
        let active = pointbreak_unprepared_env(["change", command, "--repo", repo_arg], ACTIVE);
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

// ---------------------------------------------------------------------------
// Route contracts (red until the reads route through the derived producers)
// ---------------------------------------------------------------------------

/// With the derived generation active and exactly current, the derived lane
/// substitutes the derived generation stamp for the authoritative
/// presentation-fold stamp; every other byte matches the explicit-off lane,
/// and both derived documents at one store state share one stamp value.
#[test]
#[ignore = "red until the change reads route through the derived producers"]
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
    #[ignore = "red until the change reads route through the derived producers"]
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
    #[ignore = "red until the change reads route through the derived producers"]
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
