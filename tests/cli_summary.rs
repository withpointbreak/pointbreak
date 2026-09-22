mod support;

use std::path::Path;

use pointbreak::model::ActorId;
use pointbreak::session::CaptureOptions;
use serde_json::Value;
use support::git_repo::GitRepo;
use support::{common_dir_store, dump_repo, pointbreak, pointbreak_env, pointbreak_unprepared_env};

fn parse_json(stdout: &[u8]) -> Value {
    serde_json::from_slice(stdout).expect("stdout is valid JSON")
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repo_arg(repo: &GitRepo) -> String {
    repo.path().to_str().expect("utf-8 repo path").to_owned()
}

/// A repo with one captured Revision in one Change, assessed once.
fn assessed_repo() -> GitRepo {
    let repo = dump_repo();
    let repo_arg = repo_arg(&repo);
    assert_success(&pointbreak(["capture", "--repo", &repo_arg]));
    assert_success(&pointbreak([
        "assessment",
        "add",
        "--repo",
        &repo_arg,
        "--track",
        "human:reviewer",
        "--assessment",
        "accepted",
    ]));
    repo
}

fn summary(repo: &GitRepo, extra: &[&str]) -> std::process::Output {
    let repo_arg = repo_arg(repo);
    let mut args = vec!["summary", "show", "--repo", repo_arg.as_str()];
    args.extend_from_slice(extra);
    pointbreak(args)
}

fn store_file_count(store: &Path) -> usize {
    ["events", "artifacts"]
        .iter()
        .map(|dir| count_files(&store.join(dir)))
        .sum()
}

fn count_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .map(|entry| {
            let path = entry.expect("store entry").path();
            if path.is_dir() { count_files(&path) } else { 1 }
        })
        .sum()
}

#[test]
fn summary_show_emits_the_review_summary_document() {
    let repo = assessed_repo();

    let output = summary(&repo, &["--format", "json"]);
    assert_success(&output);
    let json = parse_json(&output.stdout);

    assert_eq!(json["schema"], "pointbreak.review-summary");
    assert_eq!(json["version"], 1);
    assert_eq!(json["provenance"]["basis"], "factSet");
    assert!(json["provenance"]["eventSetHash"].is_string());
    assert!(json["provenance"].get("projectionStamp").is_none());
    assert!(json["provenance"]["countedInputs"].get("entries").is_none());
    assert_eq!(json["population"]["changesCounted"], 1);
    assert_eq!(
        json["reviewRounds"]["firstCapture"]["accepted"], 1,
        "the one captured Revision carries an accepting verdict"
    );
}

#[test]
fn summary_show_receipt_entries_inlines_every_counted_input() {
    let repo = assessed_repo();

    let output = summary(&repo, &["--receipt", "entries"]);
    assert_success(&output);
    let json = parse_json(&output.stdout);
    let counted = &json["provenance"]["countedInputs"];
    let entries = counted["entries"].as_array().expect("entries inlined");

    assert!(!entries.is_empty());
    assert_eq!(entries.len() as u64, counted["count"].as_u64().unwrap());
    for entry in entries {
        for field in [
            "eventId",
            "payloadHash",
            "eventRecordHash",
            "verificationStatus",
        ] {
            assert!(entry[field].is_string(), "entry field {field}: {entry}");
        }
    }
}

#[test]
fn summary_show_text_renders_every_section() {
    let repo = assessed_repo();

    let output = summary(&repo, &["--format", "text"]);
    assert_success(&output);
    let text = String::from_utf8(output.stdout).expect("utf-8 text");

    for heading in [
        "Population",
        "Exclusions",
        "Review rounds",
        "First capture",
        "Record-internal measures",
        "Measured against the record itself; says nothing about landed work.",
        "Landed-work coverage",
        "Not computed by this read: needs the integration branch history.",
        "Provenance",
        "Proves which facts were counted, not that the store is complete.",
    ] {
        assert!(text.contains(heading), "missing {heading:?}:\n{text}");
    }
    for rung in [
        "tipExact",
        "associatedRange",
        "assessedRange",
        "acceptingVerdictRange",
        "distinctIdentity",
        "provedLanding",
    ] {
        assert!(text.contains(rung), "missing rung {rung}:\n{text}");
    }
    assert!(!text.contains("actor:"), "text names an actor:\n{text}");
}

#[test]
fn summary_show_records_nothing() {
    let repo = assessed_repo();
    let store = common_dir_store(repo.path());
    let before = store_file_count(&store);

    let output = summary(&repo, &["--receipt", "entries"]);
    assert_success(&output);

    assert_eq!(
        store_file_count(&store),
        before,
        "the summary writes nothing"
    );
    let json = parse_json(&output.stdout);
    assert!(json.get("eventsCreated").is_none());
}

#[test]
fn summary_show_rejects_an_unknown_receipt_detail() {
    let repo = assessed_repo();

    let output = summary(&repo, &["--receipt", "everything"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--receipt"),
        "clap names the flag:\n{stderr}"
    );
}

const AUTHOR: &str = "actor:git-email:author@example.com";
const REVIEWER: &str = "actor:human:reviewer";
const REVIEW_TRACK: &str = "human:reviewer";
const AUTHOR_TRACK: &str = "human:author";
/// 2025-10-09T08:53:20.000Z, written in the legacy `unix-ms:` form.
const LEGACY_INSTANT: &str = "unix-ms:1760000000000";

/// A store holding every case the summary definitions turn on, built through
/// the CLI wherever the CLI can express the case.
///
/// Changes (by role):
/// - `lineage`: two migrated legacy Revisions (one replacing the other) plus
///   one Revision captured after migration — a review Change whose backfill
///   memberships count. Its first capture is unassessed.
/// - `backfill_only`: one migrated legacy Revision and nothing else; excluded as
///   migration backfill.
/// - `two_rounds`: first capture needs changes, second accepted, and the second
///   carries a commit association.
/// - `replaced_verdict`: one capture whose needs-changes verdict is replaced by an
///   accepting one.
/// - `unassessed`: one capture, never assessed, plus a withdrawn membership of
///   the backfill-only Revision.
/// - `same_actor`: one capture assessed under the capture's own actor id.
/// - `withdrawn_only`: a created Change whose only membership is withdrawn.
/// - `clarification`: one capture whose only verdict is needs-clarification.
/// - `legacy_instant`: a raw capture stamped in the legacy `unix-ms:` form that
///   precedes its sibling's RFC 3339 instant while sorting after it as a string.
/// - `file_scoped`: one capture whose only assessment targets a file.
struct ReviewSummaryFixture {
    repo: GitRepo,
    home: tempfile::TempDir,
    backfill_only_revision: String,
    manifest_hash: String,
}

impl ReviewSummaryFixture {
    fn repo_arg(&self) -> String {
        repo_arg(&self.repo)
    }

    fn home_arg(&self) -> &str {
        self.home.path().to_str().expect("utf-8 home path")
    }

    fn run(&self, actor: &str, args: &[&str]) -> Value {
        let env = [
            ("POINTBREAK_HOME", self.home_arg()),
            ("POINTBREAK_DERIVED_ACCESS", "off"),
            ("POINTBREAK_ACTOR_ID", actor),
        ];
        let output = pointbreak_env(args, &env);
        assert_success(&output);
        parse_json(&output.stdout)
    }

    fn summary(&self, extra: &[&str]) -> std::process::Output {
        let repo_arg = self.repo_arg();
        let mut args = vec!["summary", "show", "--repo", repo_arg.as_str()];
        args.extend_from_slice(extra);
        let env = [
            ("POINTBREAK_HOME", self.home_arg()),
            ("POINTBREAK_DERIVED_ACCESS", "off"),
        ];
        pointbreak_env(args, &env)
    }

    fn capture(&self, content: &str, extra: &[&str]) -> Value {
        self.repo.write("src/lib.rs", content);
        let repo_arg = self.repo_arg();
        let mut args = vec!["capture", "--repo", repo_arg.as_str()];
        args.extend_from_slice(extra);
        self.run(AUTHOR, &args)
    }

    fn assess(
        &self,
        actor: &str,
        track: &str,
        revision: &str,
        verdict: &str,
        extra: &[&str],
    ) -> String {
        let repo_arg = self.repo_arg();
        let mut args = vec![
            "assessment",
            "add",
            "--repo",
            repo_arg.as_str(),
            "--exact-revision",
            revision,
            "--track",
            track,
            "--assessment",
            verdict,
        ];
        args.extend_from_slice(extra);
        self.run(actor, &args)["assessmentId"]
            .as_str()
            .expect("assessment id")
            .to_owned()
    }

    fn select(&self, change: &str, revision: &str) -> String {
        let repo_arg = self.repo_arg();
        self.run(
            AUTHOR,
            &[
                "change",
                "select",
                change,
                "--revision",
                revision,
                "--repo",
                &repo_arg,
            ],
        )["token"]
            .as_str()
            .expect("review cursor")
            .to_owned()
    }

    fn join(&self, change: &str, revision: &str, operation: &str) -> Value {
        let repo_arg = self.repo_arg();
        self.run(
            AUTHOR,
            &[
                "change",
                "join",
                change,
                revision,
                "--repo",
                &repo_arg,
                "--operation-id",
                operation,
            ],
        )
    }

    fn withdraw(&self, claim: &str, operation: &str) {
        let repo_arg = self.repo_arg();
        self.run(
            AUTHOR,
            &[
                "change",
                "withdraw-membership",
                claim,
                "--repo",
                &repo_arg,
                "--operation-id",
                operation,
            ],
        );
    }
}

fn revision_of(capture: &Value) -> String {
    capture["revision"]["revisionId"]
        .as_str()
        .expect("captured revision id")
        .to_owned()
}

fn change_of(capture: &Value) -> String {
    capture["changeId"]
        .as_str()
        .expect("captured change id")
        .to_owned()
}

/// The membership claim a `change join` wrote, read from its stored event.
fn membership_claim_of(repo_root: &Path, joined: &Value) -> String {
    let event_id = joined["events"][0]["eventId"]
        .as_str()
        .unwrap_or_else(|| panic!("join reports its event: {joined}"));
    let stored = stored_record(repo_root, event_id.trim_start_matches("evt:sha256:"));
    stored["payload"]["membershipClaimId"]
        .as_str()
        .expect("stored membership claim id")
        .to_owned()
}

/// One stored record, addressed by the hex digest of its key.
fn stored_record(repo_root: &Path, key_digest: &str) -> Value {
    let path = common_dir_store(repo_root)
        .join("events")
        .join(format!("{key_digest}.json"));
    parse_json(&std::fs::read(path).expect("read stored record"))
}

/// The migration manifest hash carried by the store's root activation record.
fn activation_manifest_hash(repo_root: &Path) -> String {
    let digest = sha256_hex(b"store_capability_activation:review_change_revision_v1:root");
    stored_record(repo_root, &digest)["bulkAdoptionManifestHash"]
        .as_str()
        .expect("activation manifest hash")
        .to_owned()
}

/// Capture legacy Revisions through the library (the CLI cannot write a legacy
/// capture), then run the real migration so the activation manifest and the
/// backfill it names are genuine.
fn migrated_legacy_repo(home: &tempfile::TempDir) -> (GitRepo, Vec<String>, String) {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    let legacy = |content: &str, supersedes: Vec<pointbreak::model::RevisionId>| {
        repo.write("src/lib.rs", content);
        pointbreak::session::capture_review(
            CaptureOptions::new(repo.path())
                .with_summary("legacy revision")
                .with_actor_id(ActorId::new(AUTHOR))
                .with_supersedes(supersedes),
        )
        .expect("legacy capture")
        .revision_id
    };
    let first = legacy("pub fn value() -> u32 { 2 }\n", Vec::new());
    let second = legacy("pub fn value() -> u32 { 3 }\n", vec![first.clone()]);
    let unrelated = legacy("pub fn value() -> u32 { 4 }\n", Vec::new());

    let home_arg = home.path().to_str().expect("utf-8 home path");
    let env = [
        ("POINTBREAK_HOME", home_arg),
        ("POINTBREAK_DERIVED_ACCESS", "off"),
    ];
    let repo_arg = repo_arg(&repo);
    assert_success(&pointbreak_unprepared_env(
        ["key", "init", "--name", "migration"],
        &env,
    ));
    let dry = pointbreak_unprepared_env(["change", "migrate-dry-run", "--repo", &repo_arg], &env);
    assert_success(&dry);
    let dry_json = parse_json(&dry.stdout);
    let dry_path = home.path().join("approved-dry-run.json");
    std::fs::write(&dry_path, &dry.stdout).expect("write dry run");
    let dry_run_hash = dry_json["manifestHash"]
        .as_str()
        .expect("dry run hash")
        .to_owned();
    let cohort = dry_json["roots"][0]["cohortManifestHash"]
        .as_str()
        .expect("cohort manifest hash")
        .to_owned();
    let backup = home.path().join("migration-backup");
    assert_success(&pointbreak_unprepared_env(
        [
            "change",
            "migrate",
            "--repo",
            &repo_arg,
            "--dry-run",
            dry_path.to_str().expect("utf-8 dry run path"),
            "--ack-manifest",
            &dry_run_hash,
            "--ack-cohort-manifest",
            &cohort,
            "--ack-minimum-reader",
            "review_change_revision_v1",
            "--ack-v0-9-unsupported",
            "--backup",
            backup.to_str().expect("utf-8 backup path"),
            "--operation-id",
            "summary-migration",
            "--sign-key",
            "migration",
        ],
        &env,
    ));
    let manifest_hash = activation_manifest_hash(repo.path());
    (
        repo,
        vec![
            first.as_str().to_owned(),
            second.as_str().to_owned(),
            unrelated.as_str().to_owned(),
        ],
        manifest_hash,
    )
}

fn review_summary_fixture() -> ReviewSummaryFixture {
    let home = tempfile::tempdir().expect("isolated home");
    let (repo, legacy, manifest_hash) = migrated_legacy_repo(&home);
    let fixture = ReviewSummaryFixture {
        repo,
        home,
        backfill_only_revision: legacy[2].clone(),
        manifest_hash,
    };
    let repo_arg = fixture.repo_arg();

    // The lineage Change gains a post-migration round.
    let changes = fixture.run(AUTHOR, &["change", "list", "--repo", &repo_arg]);
    let lineage_change = find_change_holding(&changes, &legacy[1]);
    let cursor = fixture.select(&lineage_change, &legacy[1]);
    fixture.capture(
        "pub fn value() -> u32 { 5 }\n",
        &["--review-cursor", &cursor, "--advance", "replace"],
    );

    // Two rounds: needs changes, then accepted; the second is commit-associated.
    let first = fixture.capture("pub fn value() -> u32 { 6 }\n", &[]);
    let two_rounds = change_of(&first);
    fixture.assess(
        REVIEWER,
        REVIEW_TRACK,
        &revision_of(&first),
        "needs-changes",
        &[],
    );
    let cursor = fixture.select(&two_rounds, &revision_of(&first));
    let second = fixture.capture(
        "pub fn value() -> u32 { 7 }\n",
        &["--review-cursor", &cursor, "--advance", "replace"],
    );
    fixture.assess(
        REVIEWER,
        REVIEW_TRACK,
        &revision_of(&second),
        "accepted",
        &[],
    );
    let head = fixture
        .repo
        .git(["rev-parse", "HEAD"])
        .stdout
        .trim()
        .to_owned();
    fixture.run(
        REVIEWER,
        &[
            "association",
            "record",
            "--repo",
            &repo_arg,
            "--exact-revision",
            &revision_of(&second),
            "--track",
            REVIEW_TRACK,
            "--commit",
            &head,
        ],
    );

    // A replaced verdict.
    let replaced = fixture.capture("pub fn value() -> u32 { 8 }\n", &[]);
    let needs_changes = fixture.assess(
        REVIEWER,
        REVIEW_TRACK,
        &revision_of(&replaced),
        "needs-changes",
        &[],
    );
    fixture.assess(
        REVIEWER,
        REVIEW_TRACK,
        &revision_of(&replaced),
        "accepted",
        &["--replaces", &needs_changes],
    );

    // Unassessed, with a withdrawn membership of the backfill-only Revision.
    let unassessed = fixture.capture("pub fn value() -> u32 { 9 }\n", &[]);
    let joined = fixture.join(
        &change_of(&unassessed),
        &fixture.backfill_only_revision,
        "change-operation:summary-join-backfill",
    );
    fixture.withdraw(
        &membership_claim_of(fixture.repo.path(), &joined),
        "change-operation:summary-withdraw-backfill",
    );

    // Assessed under the capture's own actor id.
    let same_actor = fixture.capture("pub fn value() -> u32 { 10 }\n", &[]);
    fixture.assess(
        AUTHOR,
        AUTHOR_TRACK,
        &revision_of(&same_actor),
        "accepted",
        &[],
    );

    // A Change whose only membership is withdrawn.
    let created = fixture.run(
        AUTHOR,
        &[
            "change",
            "create",
            "--repo",
            &repo_arg,
            "--nonce",
            &"5e".repeat(32),
            "--operation-id",
            "change-operation:summary-create-emptied",
        ],
    );
    let emptied = created["changeId"]
        .as_str()
        .expect("created change id")
        .to_owned();
    let joined = fixture.join(
        &emptied,
        &revision_of(&same_actor),
        "change-operation:summary-join-emptied",
    );
    fixture.withdraw(
        &membership_claim_of(fixture.repo.path(), &joined),
        "change-operation:summary-withdraw-emptied",
    );

    // A needs-clarification-only first capture.
    let clarification = fixture.capture("pub fn value() -> u32 { 11 }\n", &[]);
    fixture.assess(
        REVIEWER,
        REVIEW_TRACK,
        &revision_of(&clarification),
        "needs-clarification",
        &[],
    );

    // The legacy instant decides the first capture.
    let sibling = fixture.capture("pub fn value() -> u32 { 12 }\n", &[]);
    fixture.assess(
        REVIEWER,
        REVIEW_TRACK,
        &revision_of(&sibling),
        "accepted",
        &[],
    );
    let legacy_revision = append_legacy_instant_capture(fixture.repo.path(), &sibling);
    fixture.join(
        &change_of(&sibling),
        &legacy_revision,
        "change-operation:summary-join-legacy-instant",
    );
    fixture.assess(
        REVIEWER,
        REVIEW_TRACK,
        &legacy_revision,
        "needs-changes",
        &[],
    );

    // A file-scoped assessment is the Revision's only assessment.
    let file_scoped = fixture.capture("pub fn value() -> u32 { 13 }\n", &[]);
    fixture.assess(
        REVIEWER,
        REVIEW_TRACK,
        &revision_of(&file_scoped),
        "accepted",
        &["--file", "src/lib.rs"],
    );

    fixture
}

fn find_change_holding(changes: &Value, revision: &str) -> String {
    let text = changes.to_string();
    assert!(
        text.contains(revision),
        "change list names {revision}: {text}"
    );
    changes["changes"]
        .as_array()
        .expect("change rows")
        .iter()
        .find(|row| row.to_string().contains(revision))
        .and_then(|row| row["changeId"].as_str())
        .expect("the Change holding the Revision")
        .to_owned()
}

/// Append a capture of a fresh Revision over the sibling's stored object, stamped
/// with a legacy `unix-ms:` instant that precedes the sibling's capture.
fn append_legacy_instant_capture(repo_root: &Path, sibling: &Value) -> String {
    use pointbreak::model::{
        EngagementId, EngagementType, JournalId, ObjectId, ReviewTargetRef, RevisionId, TargetRef,
    };
    use pointbreak::session::event::{
        EventTarget, EventType, Revision, ShoreEvent, WorkObjectProposal,
        WorkObjectProposedPayload, Writer, WriterProducer,
    };
    use sha2::{Digest, Sha256};

    let revision_id = RevisionId::new(format!("rev:sha256:{}", "1e".repeat(32)));
    let event = ShoreEvent::new(
        EventType::WorkObjectProposed,
        "summary-fixture:legacy-instant-capture",
        EventTarget::for_generative_move(
            JournalId::new("journal:default"),
            EngagementType::Review,
            TargetRef::Review(ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            }),
            None,
        )
        .expect("capture target"),
        Writer {
            actor_id: ActorId::new(AUTHOR),
            producer: WriterProducer {
                name: "pointbreak".to_owned(),
                version: "fixture".to_owned(),
            },
        },
        WorkObjectProposedPayload {
            engagement_id: EngagementId::new(format!("engagement:sha256:{}", "1f".repeat(32))),
            work_object: WorkObjectProposal::Revision {
                revision: Revision {
                    id: revision_id.clone(),
                    object_id: ObjectId::new(
                        sibling["revision"]["objectId"]
                            .as_str()
                            .expect("sibling object id"),
                    ),
                    git_provenance: None,
                },
                summary: Some("legacy instant capture".to_owned()),
                object_artifact_content_hash: sibling["revision"]["objectArtifactContentHash"]
                    .as_str()
                    .expect("sibling artifact hash")
                    .to_owned(),
                supersedes: Vec::new(),
            },
        },
        LEGACY_INSTANT,
    )
    .expect("legacy instant capture event");
    let events = common_dir_store(repo_root).join("events");
    let stem = format!("{:x}", Sha256::digest(event.idempotency_key.as_bytes()));
    std::fs::write(
        events.join(format!("{stem}.json")),
        serde_json::to_vec(&event).expect("serialize capture event"),
    )
    .expect("write capture event");
    revision_id.as_str().to_owned()
}

/// Every stored event naming `revision` in its payload, as `(eventType, eventId)`.
fn events_naming(repo_root: &Path, revision: &str) -> Vec<(String, String)> {
    let events = common_dir_store(repo_root).join("events");
    std::fs::read_dir(events)
        .expect("read events")
        .filter_map(|entry| {
            let bytes = std::fs::read(entry.expect("event entry").path()).ok()?;
            let event: Value = serde_json::from_slice(&bytes).ok()?;
            let id = event["eventId"].as_str()?.to_owned();
            event["payload"].to_string().contains(revision).then(|| {
                (
                    event["eventType"].as_str().unwrap_or_default().to_owned(),
                    id,
                )
            })
        })
        .collect()
}

fn walk(value: &Value, key: Option<&str>, visit: &mut impl FnMut(Option<&str>, &Value)) {
    visit(key, value);
    match value {
        Value::Object(map) => {
            for (child_key, child) in map {
                walk(child, Some(child_key), visit);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk(item, key, visit);
            }
        }
        _ => {}
    }
}

fn sorted_keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    keys
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn summary_counts_the_fixture_store_exactly() {
    let fixture = review_summary_fixture();

    let output = fixture.summary(&["--receipt", "entries"]);
    assert_success(&output);
    let json = parse_json(&output.stdout);
    let population = &json["population"];
    let rounds = &json["reviewRounds"];
    let measures = &json["recordInternalMeasures"];

    // Rounds per counted Change: replaced_verdict, unassessed, same_actor,
    // clarification, file_scoped have one; two_rounds and legacy_instant have
    // two; lineage has its two migrated Revisions plus one new round.
    assert_eq!(
        rounds["profile"],
        serde_json::json!({
            "state": "computed",
            "of": 8,
            "distribution": [
                {"rounds": 1, "changes": 5},
                {"rounds": 2, "changes": 2},
                {"rounds": 3, "changes": 1}
            ]
        })
    );
    // Seen: all ten Changes with a membership. Counted: the eight review
    // Changes with a current member (not backfill_only, not withdrawn_only).
    assert_eq!(population["changesSeen"], 10);
    assert_eq!(population["changesCounted"], 8);
    // Current: lineage 3, two_rounds 2, legacy_instant 2, and one each for the
    // other five. Historical adds the withdrawn membership in unassessed.
    assert_eq!(
        population["memberships"],
        serde_json::json!({"current": 12, "historical": 13})
    );
    // Every current membership names a distinct Revision; the withdrawn one
    // names the backfill-only Revision, which no counted Change holds currently.
    assert_eq!(
        population["capturedRevisions"],
        serde_json::json!({"current": 12, "historical": 13})
    );
    assert_eq!(
        population["observedWindow"]["from"], "2025-10-09T08:53:20.000Z",
        "the legacy-instant capture is the earliest counted capture"
    );
    assert_eq!(
        population["exclusions"],
        serde_json::json!([
            {
                "reason": "migrationBackfill",
                "basis": "storeActivationManifest",
                "manifestHash": fixture.manifest_hash,
                "changes": 1,
                "memberships": 1
            },
            {"reason": "noCurrentMembers", "changes": 1}
        ])
    );

    // First captures: lineage (its first legacy Revision) and unassessed have no
    // verdict; two_rounds and legacy_instant need changes; replaced_verdict,
    // same_actor and file_scoped are accepted; clarification is other.
    assert_eq!(
        rounds["firstCapture"],
        serde_json::json!({
            "state": "computed",
            "of": 8,
            "accepted": 3,
            "needsChanges": 2,
            "otherVerdict": 1,
            "unassessed": 2
        })
    );
    // Ordering by the raw timestamp string would pick the sibling (RFC 3339
    // sorts before `unix-ms:`), turning legacy_instant's needs-changes into an
    // accepted first capture: accepted 4, needsChanges 1.
    assert_eq!(
        rounds["firstCapture"]["needsChanges"], 2,
        "first capture must order by instant, not by the occurredAt string"
    );

    // Assessed: both two_rounds Revisions, replaced_verdict, same_actor,
    // clarification, both legacy_instant Revisions, file_scoped. Unassessed:
    // lineage's three Revisions and unassessed.
    assert_eq!(
        measures["assessedCaptureShare"],
        serde_json::json!({"state": "computed", "assessed": 8, "of": 12})
    );
    assert_eq!(
        measures["commitAssociatedCaptureShare"],
        serde_json::json!({"state": "computed", "associated": 1, "of": 12})
    );
    assert_eq!(
        measures["firstCaptureAcceptance"],
        serde_json::json!({
            "state": "computed",
            "accepting": 3,
            "of": 6,
            "unit": "assessedFirstCaptures"
        })
    );
    // Distinct: every assessed Revision except same_actor's. Undetermined: the
    // four Revisions without an assessment.
    assert_eq!(
        measures["actorDistinctAssessmentShare"],
        serde_json::json!({
            "state": "computed",
            "actorDistinct": 7,
            "actorSame": 1,
            "undetermined": 4,
            "of": 12,
            "keyEvidence": {"state": "unavailable", "reasons": ["signingKeysNotCompared"]}
        })
    );

    let counted = &json["provenance"]["countedInputs"];
    // Claims: 13 memberships of counted Changes. Withdrawals: the one naming
    // unassessed's claim. Captures: one per captured Revision. Assessments:
    // nine on captured Revisions (replaced_verdict holds two). One association.
    assert_eq!(
        counted["byKind"],
        serde_json::json!({
            "membershipClaims": 13,
            "membershipWithdrawals": 1,
            "captures": 12,
            "assessments": 9,
            "commitAssociations": 1
        })
    );
    assert_eq!(counted["count"], 36);
    let entries = counted["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 36);
    let lines: String = entries
        .iter()
        .map(|entry| {
            format!(
                "{} {} {}\n",
                entry["eventId"].as_str().unwrap(),
                entry["payloadHash"].as_str().unwrap(),
                entry["eventRecordHash"].as_str().unwrap()
            )
        })
        .collect();
    let mut sorted_entries = entries.clone();
    sorted_entries.sort_by_key(|entry| {
        (
            entry["eventId"].as_str().unwrap().to_owned(),
            entry["eventRecordHash"].as_str().unwrap().to_owned(),
        )
    });
    assert_eq!(&sorted_entries, entries, "entries are sorted");
    assert_eq!(
        counted["digest"],
        format!("sha256:{}", sha256_hex(lines.as_bytes())),
        "digest recomputed from the entry lines"
    );
    let verification = &counted["verification"];
    let tallied: u64 = ["valid", "untrustedKey", "invalid", "unsigned"]
        .iter()
        .map(|key| verification[*key].as_u64().unwrap())
        .sum();
    assert_eq!(tallied, 36);
    assert_eq!(verification["allowedSignersConfigured"], false);

    // The backfill-only Revision: its migrated membership and its capture are
    // not counted; the withdrawn membership from unassessed is.
    let entry_ids: Vec<&str> = entries
        .iter()
        .map(|entry| entry["eventId"].as_str().unwrap())
        .collect();
    let naming = events_naming(fixture.repo.path(), &fixture.backfill_only_revision);
    let counted_naming: Vec<_> = naming
        .iter()
        .filter(|(_, id)| entry_ids.contains(&id.as_str()))
        .collect();
    assert_eq!(
        counted_naming.len(),
        1,
        "only the withdrawn membership naming the backfill-only Revision is counted: {naming:?}"
    );
}

#[test]
fn summary_document_contracts_hold_on_the_fixture_store() {
    let fixture = review_summary_fixture();

    let entries_output = fixture.summary(&["--receipt", "entries"]);
    let digest_output = fixture.summary(&[]);
    let text_output = fixture.summary(&["--format", "text"]);
    for output in [&entries_output, &digest_output, &text_output] {
        assert_success(output);
    }
    let with_entries = parse_json(&entries_output.stdout);
    let digest_only = parse_json(&digest_output.stdout);
    let text = String::from_utf8(text_output.stdout).expect("utf-8 text");

    walk(&with_entries, None, &mut |key, node| {
        if let Value::Number(number) = node {
            assert!(number.is_u64() || number.is_i64(), "float at {key:?}");
        }
    });
    let coverage = &with_entries["landedWorkCoverage"];
    assert_eq!(
        sorted_keys(coverage),
        [
            "acceptingVerdictRange",
            "assessedRange",
            "associatedRange",
            "distinctIdentity",
            "provedLanding",
            "tipExact"
        ]
    );
    for rung in sorted_keys(coverage) {
        assert_eq!(coverage[&rung]["state"], "unavailable", "{rung}");
    }
    let measure_keys = sorted_keys(&with_entries["recordInternalMeasures"]);
    assert!(
        sorted_keys(coverage)
            .iter()
            .all(|key| !measure_keys.contains(key))
    );

    let serialized = with_entries.to_string();
    for rendered in [serialized.as_str(), text.as_str()] {
        for needle in ["actor:", "did:key:", "@"] {
            assert!(!rendered.contains(needle), "rendering names {needle}");
        }
        let lowered = rendered.to_lowercase();
        for word in ["waste", "independent", "trust score"] {
            assert!(!lowered.contains(word), "rendering uses {word:?}");
        }
    }

    let strip = |mut value: Value| {
        let provenance = value["provenance"].as_object_mut().unwrap();
        provenance.remove("computedAt");
        provenance["countedInputs"]
            .as_object_mut()
            .unwrap()
            .remove("entries");
        value
    };
    assert_eq!(strip(with_entries), strip(digest_only));
}

#[test]
fn summary_of_a_backfill_only_store_is_unavailable_not_zero() {
    let home = tempfile::tempdir().expect("isolated home");
    let (repo, _, manifest_hash) = migrated_legacy_repo(&home);
    let env = [
        ("POINTBREAK_HOME", home.path().to_str().unwrap()),
        ("POINTBREAK_DERIVED_ACCESS", "off"),
    ];
    let repo_arg = repo_arg(&repo);

    let output = pointbreak_env(["summary", "show", "--repo", repo_arg.as_str()], &env);
    assert_success(&output);
    let json = parse_json(&output.stdout);

    // Two migrated Changes: the two-Revision lineage and the unrelated one.
    assert_eq!(json["population"]["changesSeen"], 2);
    assert_eq!(json["population"]["changesCounted"], 0);
    assert_eq!(
        json["population"]["exclusions"][0],
        serde_json::json!({
            "reason": "migrationBackfill",
            "basis": "storeActivationManifest",
            "manifestHash": manifest_hash,
            "changes": 2,
            "memberships": 3
        })
    );
    assert_all_measures_unavailable(&json);
}

#[test]
fn summary_of_a_store_with_no_changes_is_unavailable_not_zero() {
    let repo = dump_repo();

    let output = summary(&repo, &[]);
    assert_success(&output);
    let json = parse_json(&output.stdout);

    assert_eq!(json["population"]["changesSeen"], 0);
    // The test harness activates every fresh store with an empty migration
    // manifest, so the basis is the (empty) activation manifest.
    assert_eq!(
        json["population"]["exclusions"][0]["basis"],
        "storeActivationManifest"
    );
    assert_eq!(json["population"]["exclusions"][0]["changes"], 0);
    assert_all_measures_unavailable(&json);
}

fn assert_all_measures_unavailable(json: &Value) {
    let unavailable = serde_json::json!({"state": "unavailable", "reasons": ["noCountedChanges"]});
    for measure in [
        &json["reviewRounds"]["profile"],
        &json["reviewRounds"]["firstCapture"],
        &json["recordInternalMeasures"]["assessedCaptureShare"],
        &json["recordInternalMeasures"]["commitAssociatedCaptureShare"],
        &json["recordInternalMeasures"]["firstCaptureAcceptance"],
        &json["recordInternalMeasures"]["actorDistinctAssessmentShare"],
    ] {
        assert_eq!(measure, &unavailable);
    }
    assert_eq!(json["provenance"]["countedInputs"]["count"], 0);
    assert!(json["population"].get("observedWindow").is_none());
}

/// A small store with a captured, assessed Revision, built under an isolated
/// home with non-agent actor ids.
struct CheckStore {
    repo: GitRepo,
    home: tempfile::TempDir,
}

impl CheckStore {
    fn new() -> Self {
        let home = tempfile::tempdir().expect("isolated home");
        let repo = dump_repo();
        let store = Self { repo, home };
        let repo_arg = repo_arg(&store.repo);
        assert_success(&store.run(
            AUTHOR,
            &["capture", "--repo", &repo_arg, "--summary", "checked"],
        ));
        assert_success(&store.run(
            REVIEWER,
            &[
                "assessment",
                "add",
                "--repo",
                &repo_arg,
                "--track",
                REVIEW_TRACK,
                "--assessment",
                "accepted",
            ],
        ));
        store
    }

    fn run(&self, actor: &str, args: &[&str]) -> std::process::Output {
        let env = [
            (
                "POINTBREAK_HOME",
                self.home.path().to_str().expect("utf-8 home path"),
            ),
            ("POINTBREAK_DERIVED_ACCESS", "off"),
            ("POINTBREAK_ACTOR_ID", actor),
        ];
        pointbreak_env(args, &env)
    }

    fn saved_receipt(&self) -> std::path::PathBuf {
        let repo_arg = repo_arg(&self.repo);
        let output = self.run(
            AUTHOR,
            &[
                "summary",
                "show",
                "--repo",
                &repo_arg,
                "--receipt",
                "entries",
            ],
        );
        assert_success(&output);
        let path = self.home.path().join("saved-summary.json");
        std::fs::write(&path, &output.stdout).expect("save summary");
        path
    }

    fn check(&self, receipt: &Path, extra: &[&str]) -> std::process::Output {
        let repo_arg = repo_arg(&self.repo);
        let mut args = vec![
            "summary",
            "check",
            receipt.to_str().expect("utf-8 receipt path"),
            "--repo",
            repo_arg.as_str(),
        ];
        args.extend_from_slice(extra);
        self.run(AUTHOR, &args)
    }

    fn counted_event_ids(&self, receipt: &Path) -> Vec<String> {
        let saved = parse_json(&std::fs::read(receipt).expect("read receipt"));
        saved["provenance"]["countedInputs"]["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .map(|entry| entry["eventId"].as_str().expect("event id").to_owned())
            .collect()
    }
}

/// The stored file of the event with `event_id`.
fn stored_event_path(repo_root: &Path, event_id: &str) -> std::path::PathBuf {
    std::fs::read_dir(common_dir_store(repo_root).join("events"))
        .expect("read events")
        .map(|entry| entry.expect("event entry").path())
        .find(|path| {
            std::fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .is_some_and(|event| event["eventId"] == event_id)
        })
        .unwrap_or_else(|| panic!("no stored event {event_id}"))
}

/// The payload hash the store computes: sha256 over the payload with object
/// keys sorted at every level.
fn payload_hash_of(payload: &Value) -> String {
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            Value::Object(map) => {
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort_unstable();
                Value::Object(
                    keys.into_iter()
                        .map(|key| (key.clone(), canonical(&map[key])))
                        .collect(),
                )
            }
            other => other.clone(),
        }
    }
    let bytes = serde_json::to_vec(&canonical(payload)).expect("serialize payload");
    format!("sha256:{}", sha256_hex(&bytes))
}

fn rewrite_stored_event(path: &Path, edit: impl FnOnce(&mut Value)) {
    let mut event = parse_json(&std::fs::read(path).expect("read event"));
    edit(&mut event);
    std::fs::write(path, serde_json::to_vec(&event).expect("serialize event"))
        .expect("write event");
}

fn only_difference(json: &Value) -> &Value {
    let differing = json["differing"].as_array().expect("differing");
    assert_eq!(differing.len(), 1, "exactly one difference: {differing:?}");
    &differing[0]
}

#[test]
fn summary_check_matches_every_entry_of_a_fresh_receipt() {
    let fixture = review_summary_fixture();
    let saved = fixture.summary(&["--receipt", "entries"]);
    assert_success(&saved);
    let receipt = fixture.home.path().join("saved-summary.json");
    std::fs::write(&receipt, &saved.stdout).expect("save summary");
    let store = common_dir_store(fixture.repo.path());
    let before = store_file_count(&store);

    let repo_arg = fixture.repo_arg();
    let env = [
        ("POINTBREAK_HOME", fixture.home_arg()),
        ("POINTBREAK_DERIVED_ACCESS", "off"),
    ];
    let output = pointbreak_env(
        [
            "summary",
            "check",
            receipt.to_str().expect("utf-8 path"),
            "--repo",
            repo_arg.as_str(),
        ],
        &env,
    );
    assert_success(&output);
    let json = parse_json(&output.stdout);

    let count = parse_json(&saved.stdout)["provenance"]["countedInputs"]["count"]
        .as_u64()
        .expect("count");
    assert!(count > 0);
    assert_eq!(json["schema"], "pointbreak.review-summary-check");
    assert_eq!(json["version"], 1);
    assert_eq!(json["matched"], count);
    assert_eq!(json["changed"], 0);
    assert_eq!(json["missing"], 0);
    assert_eq!(json["differing"], serde_json::json!([]));
    assert_eq!(json["digestMatchesEntries"], true);
    assert_eq!(json["completeness"], "notProven");
    assert_eq!(json["receipt"]["count"], count);
    assert_eq!(
        json["receipt"]["digest"],
        parse_json(&saved.stdout)["provenance"]["countedInputs"]["digest"]
    );
    assert_eq!(store_file_count(&store), before, "the check writes nothing");
    assert!(json.get("eventsCreated").is_none());
}

#[test]
fn summary_check_reports_a_rewritten_payload_as_changed() {
    let store = CheckStore::new();
    let receipt = store.saved_receipt();
    let event_id = store.counted_event_ids(&receipt)[0].clone();
    let path = stored_event_path(store.repo.path(), &event_id);
    let before = parse_json(&std::fs::read(&path).expect("read event"));
    // A self-consistent rewrite: the payload changes and the payload hash is
    // recomputed, so the record still loads.
    rewrite_stored_event(&path, |event| {
        event["payload"]["rewritten"] = Value::Bool(true);
        event["payloadHash"] = Value::String(payload_hash_of(&event["payload"]));
    });

    let output = store.check(&receipt, &[]);

    assert_success(&output);
    let json = parse_json(&output.stdout);
    assert_eq!((&json["changed"], &json["missing"]), (&1.into(), &0.into()));
    let difference = only_difference(&json);
    assert_eq!(difference["eventId"], event_id);
    assert_eq!(difference["outcome"], "changed");
    assert_eq!(difference["recordedPayloadHash"], before["payloadHash"]);
    assert_ne!(difference["currentPayloadHash"], before["payloadHash"]);
    assert!(difference["recordedEventRecordHash"].is_string());
    assert!(difference["currentEventRecordHash"].is_string());
    assert_ne!(
        difference["currentEventRecordHash"],
        difference["recordedEventRecordHash"]
    );
}

#[test]
fn summary_check_reports_a_record_edit_with_the_payload_untouched_as_changed() {
    let store = CheckStore::new();
    let receipt = store.saved_receipt();
    let event_id = store.counted_event_ids(&receipt)[0].clone();
    let path = stored_event_path(store.repo.path(), &event_id);
    rewrite_stored_event(&path, |event| {
        event["writer"]["producer"]["version"] = Value::String("edited".to_owned());
    });

    let output = store.check(&receipt, &[]);

    assert_success(&output);
    let json = parse_json(&output.stdout);
    assert_eq!(json["changed"], 1);
    let difference = only_difference(&json);
    assert_eq!(difference["eventId"], event_id);
    assert_eq!(
        difference["currentPayloadHash"], difference["recordedPayloadHash"],
        "only the record hash moved"
    );
    assert_ne!(
        difference["currentEventRecordHash"],
        difference["recordedEventRecordHash"]
    );
}

#[test]
fn summary_check_reports_a_deleted_event_as_missing() {
    let store = CheckStore::new();
    let receipt = store.saved_receipt();
    let event_id = store.counted_event_ids(&receipt)[0].clone();
    std::fs::remove_file(stored_event_path(store.repo.path(), &event_id))
        .expect("delete stored event");

    let output = store.check(&receipt, &[]);

    assert_success(&output);
    let json = parse_json(&output.stdout);
    assert_eq!((&json["changed"], &json["missing"]), (&0.into(), &1.into()));
    let difference = only_difference(&json);
    assert_eq!(difference["eventId"], event_id);
    assert_eq!(difference["outcome"], "missing");
    assert!(difference.get("currentPayloadHash").is_none());
    assert!(difference.get("currentEventRecordHash").is_none());
}

#[test]
fn summary_check_flags_a_saved_file_that_no_longer_reproduces_its_digest() {
    let store = CheckStore::new();
    let receipt = store.saved_receipt();
    let mut saved = parse_json(&std::fs::read(&receipt).expect("read receipt"));
    saved["provenance"]["countedInputs"]["entries"][0]["payloadHash"] =
        Value::String("sha256:tampered".to_owned());
    std::fs::write(&receipt, serde_json::to_vec(&saved).expect("serialize")).expect("save");

    let output = store.check(&receipt, &[]);

    assert_success(&output);
    let json = parse_json(&output.stdout);
    assert_eq!(json["digestMatchesEntries"], false);
    assert_eq!(json["changed"], 1, "the edited entry no longer matches");
}

#[test]
fn summary_check_of_a_digest_only_document_says_to_rerun_with_entries() {
    let store = CheckStore::new();
    let repo_arg = repo_arg(&store.repo);
    let digest_only = store.run(AUTHOR, &["summary", "show", "--repo", &repo_arg]);
    assert_success(&digest_only);
    let receipt = store.home.path().join("digest-only.json");
    std::fs::write(&receipt, &digest_only.stdout).expect("save summary");

    let output = store.check(&receipt, &[]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--receipt entries"), "{stderr}");
}

#[test]
fn summary_check_of_a_file_that_is_not_a_summary_fails() {
    let store = CheckStore::new();
    let receipt = store.home.path().join("other.json");
    std::fs::write(
        &receipt,
        br#"{"schema":"pointbreak.review-history","version":1}"#,
    )
    .expect("write file");

    let output = store.check(&receipt, &[]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("pointbreak.review-summary"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn summary_check_text_states_what_it_proves_and_names_no_actor() {
    let store = CheckStore::new();
    let receipt = store.saved_receipt();
    let event_id = store.counted_event_ids(&receipt)[0].clone();
    std::fs::remove_file(stored_event_path(store.repo.path(), &event_id))
        .expect("delete stored event");

    let json_output = store.check(&receipt, &["--format", "json"]);
    let text_output = store.check(&receipt, &["--format", "text"]);
    assert_success(&json_output);
    assert_success(&text_output);
    let text = String::from_utf8(text_output.stdout).expect("utf-8 text");

    assert!(
        text.contains("missing"),
        "the missing fact is named in the text:\n{text}"
    );
    assert!(text.contains(&event_id), "{text}");
    assert!(
        text.trim_end().ends_with(
            "Checks the facts this receipt lists. Facts added or removed elsewhere in the store \
             are out of its reach."
        ),
        "{text}"
    );
    for rendered in [
        String::from_utf8_lossy(&json_output.stdout).into_owned(),
        text,
    ] {
        for needle in ["actor:", "did:key:", "@"] {
            assert!(!rendered.contains(needle), "rendering names {needle}");
        }
    }
}

/// The derived-access profile a current generation serves reads from.
const DERIVED_ACTIVE: &str = "sqlite-wal-bodyless-v1";
/// The one recovery action the fallback hint names.
const DERIVED_HINT: &str = "pointbreak store derived build";

impl ReviewSummaryFixture {
    fn summary_with_access(&self, access: &str, extra: &[&str]) -> std::process::Output {
        let repo_arg = self.repo_arg();
        let mut args = vec!["summary", "show", "--repo", repo_arg.as_str()];
        args.extend_from_slice(extra);
        let env = [
            ("POINTBREAK_HOME", self.home_arg()),
            ("POINTBREAK_DERIVED_ACCESS", access),
        ];
        pointbreak_env(args, &env)
    }

    fn build_derived(&self) {
        let repo_arg = self.repo_arg();
        let env = [
            ("POINTBREAK_HOME", self.home_arg()),
            ("POINTBREAK_DERIVED_ACCESS", DERIVED_ACTIVE),
        ];
        assert_success(&pointbreak_env(
            ["store", "derived", "build", "--repo", repo_arg.as_str()],
            &env,
        ));
    }
}

impl CheckStore {
    fn summary_with_access(&self, access: &str, extra: &[&str]) -> std::process::Output {
        let repo_arg = repo_arg(&self.repo);
        let mut args = vec!["summary", "show", "--repo", repo_arg.as_str()];
        args.extend_from_slice(extra);
        let env = [
            (
                "POINTBREAK_HOME",
                self.home.path().to_str().expect("utf-8 home path"),
            ),
            ("POINTBREAK_DERIVED_ACCESS", access),
        ];
        pointbreak_env(args, &env)
    }

    fn build_derived(&self) {
        let repo_arg = repo_arg(&self.repo);
        let env = [
            (
                "POINTBREAK_HOME",
                self.home.path().to_str().expect("utf-8 home path"),
            ),
            ("POINTBREAK_DERIVED_ACCESS", DERIVED_ACTIVE),
        ];
        assert_success(&pointbreak_env(
            ["store", "derived", "build", "--repo", repo_arg.as_str()],
            &env,
        ));
    }
}

/// The fixture store plus a range-scoped and an observation-scoped assessment,
/// each its Revision's only assessment, with a current derived generation built
/// over the result.
fn parity_fixture() -> ReviewSummaryFixture {
    let fixture = review_summary_fixture();
    let repo_arg = fixture.repo_arg();

    let range_scoped = fixture.capture("pub fn value() -> u32 { 14 }\n", &[]);
    fixture.assess(
        REVIEWER,
        REVIEW_TRACK,
        &revision_of(&range_scoped),
        "needs-changes",
        &[
            "--file",
            "src/lib.rs",
            "--start-line",
            "1",
            "--end-line",
            "1",
        ],
    );

    let observed = fixture.capture("pub fn value() -> u32 { 15 }\n", &[]);
    let observation = fixture.run(
        REVIEWER,
        &[
            "observation",
            "add",
            "--repo",
            &repo_arg,
            "--exact-revision",
            &revision_of(&observed),
            "--track",
            REVIEW_TRACK,
            "--title",
            "the value needs a test",
        ],
    );
    let observation_id = observation["observationId"]
        .as_str()
        .unwrap_or_else(|| panic!("observation id: {observation}"))
        .to_owned();
    fixture.assess(
        REVIEWER,
        REVIEW_TRACK,
        &revision_of(&observed),
        "accepted",
        &["--observation", &observation_id],
    );

    fixture.build_derived();
    fixture
}

/// The document without the four fields that name its basis.
fn without_basis_fields(mut document: Value) -> Value {
    let provenance = document["provenance"]
        .as_object_mut()
        .expect("provenance block");
    for key in ["basis", "eventSetHash", "projectionStamp", "computedAt"] {
        provenance.remove(key);
    }
    document
}

/// Every file under `root`, relative to it, with the hash of its bytes.
fn files_under(root: &Path) -> std::collections::BTreeMap<String, String> {
    fn walk_files(dir: &Path, root: &Path, files: &mut std::collections::BTreeMap<String, String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("store entry").path();
            if path.is_dir() {
                walk_files(&path, root, files);
            } else {
                files.insert(
                    path.strip_prefix(root)
                        .expect("path under root")
                        .to_string_lossy()
                        .replace('\\', "/"),
                    sha256_hex(&std::fs::read(&path).expect("read store file")),
                );
            }
        }
    }
    let mut files = std::collections::BTreeMap::new();
    walk_files(root, root, &mut files);
    files
}

/// Files any derived read may leave beside the generation it served from: the
/// reader's generation lease and SQLite's write-ahead index pair. Neither holds a
/// fact or a projection row.
fn is_derived_reader_residue(path: &str) -> bool {
    (path.starts_with("derived.generation-lease-") && path.ends_with(".lock"))
        || (path.starts_with("derived/generations/")
            && (path.ends_with("/cursor.sqlite3-shm") || path.ends_with("/cursor.sqlite3-wal")))
}

/// The generation's SQLite files. SQLite may fold its own write-ahead log into
/// the database file when a connection closes (it does on Windows), so these are
/// compared by logical content through [`generation_contents`], not by bytes.
fn is_generation_database(path: &str) -> bool {
    path.starts_with("derived/generations/")
        && [
            "/cursor.sqlite3",
            "/cursor.sqlite3-wal",
            "/cursor.sqlite3-shm",
        ]
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

/// The directory of the store's one published derived generation.
fn the_generation(store: &CheckStore) -> std::path::PathBuf {
    let root = common_dir_store(store.repo.path());
    let generations = std::fs::read_dir(root.join("derived/generations"))
        .expect("generations")
        .map(|entry| entry.expect("generation").path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    assert_eq!(generations.len(), 1, "one published generation");
    generations.into_iter().next().expect("one generation")
}

fn open_the_generation(store: &CheckStore) -> rusqlite::Connection {
    rusqlite::Connection::open(the_generation(store).join("cursor.sqlite3"))
        .expect("open generation")
}

/// The generation's logical content: its schema, then every row of every table,
/// each table's rows sorted, independent of how SQLite lays out its files.
fn generation_contents(connection: &rusqlite::Connection) -> Vec<(String, Vec<String>)> {
    let mut schema = connection
        .prepare(
            "SELECT type, name, coalesce(sql, '') FROM sqlite_schema
             WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )
        .expect("read schema");
    let entries = schema
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("query schema")
        .collect::<Result<Vec<_>, _>>()
        .expect("schema rows");
    let mut contents = vec![(
        "schema".to_owned(),
        entries
            .iter()
            .map(|(kind, name, sql)| format!("{kind} {name} {sql}"))
            .collect(),
    )];
    for (_, table, _) in entries.iter().filter(|(kind, _, _)| kind == "table") {
        let mut statement = connection
            .prepare(&format!("SELECT * FROM \"{table}\""))
            .expect("select table");
        let columns = statement.column_count();
        let mut rows = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|index| {
                        row.get::<_, rusqlite::types::Value>(index)
                            .map(|value| format!("{value:?}"))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .expect("query table")
            .map(|cells| cells.expect("table row").join("\u{1f}"))
            .collect::<Vec<_>>();
        rows.sort();
        contents.push((table.clone(), rows));
    }
    contents
}

/// The generation's applied sequence.
fn generation_applied_sequence(connection: &rusqlite::Connection) -> i64 {
    connection
        .query_row(
            "SELECT applied_sequence FROM locator_checkpoint WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .expect("applied sequence")
}

#[test]
fn generation_contents_see_a_changed_cell_but_not_a_checkpoint() {
    let store = CheckStore::new();
    store.build_derived();
    let connection = open_the_generation(&store);
    let built = generation_contents(&connection);

    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .expect("checkpoint");
    assert_eq!(generation_contents(&connection), built);

    let changed = connection
        .execute(
            "UPDATE product_revision SET captured_at = '2000-01-01T00:00:00Z'",
            [],
        )
        .expect("rewrite one cell");
    assert_eq!(changed, 1);
    assert_ne!(generation_contents(&connection), built);
}

#[test]
fn summary_show_projection_and_fact_set_documents_agree_outside_the_basis() {
    let fixture = parity_fixture();

    for extra in [&[][..], &["--receipt", "entries"][..]] {
        let fact_set = fixture.summary_with_access("off", extra);
        let projection = fixture.summary_with_access(DERIVED_ACTIVE, extra);
        assert_success(&fact_set);
        assert_success(&projection);
        assert!(
            projection.stderr.is_empty(),
            "a current generation answers without a hint:\n{}",
            String::from_utf8_lossy(&projection.stderr)
        );
        let fact_set = parse_json(&fact_set.stdout);
        let projection = parse_json(&projection.stdout);

        assert_eq!(fact_set["provenance"]["basis"], "factSet");
        assert!(fact_set["provenance"]["eventSetHash"].is_string());
        assert!(fact_set["provenance"].get("projectionStamp").is_none());
        assert_eq!(projection["provenance"]["basis"], "projection");
        assert!(projection["provenance"]["projectionStamp"].is_string());
        assert!(projection["provenance"].get("eventSetHash").is_none());
        assert_eq!(
            projection["provenance"]["countedInputs"]["digest"],
            fact_set["provenance"]["countedInputs"]["digest"]
        );
        assert_eq!(
            without_basis_fields(projection),
            without_basis_fields(fact_set),
            "receipt detail {extra:?}"
        );
    }
}

#[test]
fn summary_show_without_a_built_generation_falls_back_with_one_hint_per_process() {
    let store = CheckStore::new();

    for _ in 0..2 {
        let output = store.summary_with_access(DERIVED_ACTIVE, &[]);
        assert_success(&output);
        let json = parse_json(&output.stdout);
        assert_eq!(json["provenance"]["basis"], "factSet");
        assert!(json["provenance"]["eventSetHash"].is_string());
        assert!(json["provenance"].get("projectionStamp").is_none());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(stderr.matches(DERIVED_HINT).count(), 1, "{stderr}");
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
    }
}

#[test]
fn summary_show_with_derived_access_off_reads_facts_without_a_hint() {
    let store = CheckStore::new();
    store.build_derived();

    let output = store.summary_with_access("off", &[]);

    assert_success(&output);
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(parse_json(&output.stdout)["provenance"]["basis"], "factSet");
}

#[test]
fn summary_show_on_a_projection_writes_nothing_and_leaves_the_generation_unmoved() {
    let store = CheckStore::new();
    store.build_derived();
    let root = common_dir_store(store.repo.path());
    let generation_before = generation_contents(&open_the_generation(&store));
    let before = files_under(&root);

    let first = store.summary_with_access(DERIVED_ACTIVE, &["--receipt", "entries"]);
    let second = store.summary_with_access(DERIVED_ACTIVE, &[]);

    assert_success(&first);
    assert_success(&second);
    let after = files_under(&root);
    for (path, hash) in before
        .iter()
        .filter(|(path, _)| !is_generation_database(path))
    {
        assert_eq!(after.get(path), Some(hash), "{path} is unchanged");
    }
    let added = after
        .keys()
        .filter(|path| !before.contains_key(*path))
        .collect::<Vec<_>>();
    assert!(
        added.iter().all(|path| is_derived_reader_residue(path)),
        "the summary adds no fact, artifact, or projection file: {added:?}"
    );
    let first = parse_json(&first.stdout);
    let second = parse_json(&second.stdout);
    assert_eq!(first["provenance"]["basis"], "projection");
    assert_eq!(
        first["provenance"]["projectionStamp"],
        second["provenance"]["projectionStamp"]
    );
    let generation = open_the_generation(&store);
    let generation_after = generation_contents(&generation);
    let changed = generation_before
        .iter()
        .zip(&generation_after)
        .filter(|(before, after)| before != after)
        .map(|((table, before), (_, after))| {
            let removed = before.iter().filter(|row| !after.contains(row));
            let added = after.iter().filter(|row| !before.contains(row));
            format!("{table}: removed {removed:?} added {added:?}")
        })
        .collect::<Vec<_>>();
    assert!(
        changed.is_empty() && generation_after.len() == generation_before.len(),
        "the generation's schema and every table row are unchanged: {changed:#?}"
    );
    assert_eq!(
        Some(generation_applied_sequence(&generation)),
        first["provenance"]["eventCount"].as_i64(),
        "the generation still applies exactly the events it was built over"
    );
}

/// Apply `sql` to the store's one published derived generation, as damage to the
/// index would, and fold the change into the database file.
fn tamper_with_the_generation(store: &CheckStore, sql: &str) -> usize {
    let connection = open_the_generation(store);
    let rewritten = connection.execute(sql, []).expect("rewrite the index");
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .expect("checkpoint");
    rewritten
}

#[test]
fn summary_show_fails_when_a_counted_row_disagrees_with_its_recorded_payload() {
    let store = CheckStore::new();
    store.build_derived();
    let rewritten = tamper_with_the_generation(
        &store,
        "UPDATE locator_event SET payload_hash = zeroblob(32)
         WHERE sequence = (SELECT sequence FROM product_revision LIMIT 1)",
    );
    assert_eq!(rewritten, 1);

    let output = store.summary_with_access(DERIVED_ACTIVE, &[]);

    assert!(
        !output.status.success(),
        "a disagreeing counted row fails the read instead of falling back:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not match"), "{stderr}");
    assert!(!stderr.contains(DERIVED_HINT), "{stderr}");
}

#[test]
fn summary_show_fails_when_a_counted_verdict_row_disagrees_with_its_recorded_event() {
    let store = CheckStore::new();
    store.build_derived();
    // The carrier and its payload hash stay intact; only the indexed verdict moves.
    let rewritten = tamper_with_the_generation(
        &store,
        "UPDATE semantic_assessment_fact SET assessment = 'needs_changes'
         WHERE assessment = 'accepted'",
    );
    assert_eq!(rewritten, 1);

    let projection = store.summary_with_access(DERIVED_ACTIVE, &["--receipt", "entries"]);
    let fact_set = store.summary_with_access("off", &["--receipt", "entries"]);

    assert!(
        !projection.status.success(),
        "a counted row that disagrees with its recorded event fails the read:\n{}",
        String::from_utf8_lossy(&projection.stdout)
    );
    assert!(projection.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&projection.stderr);
    assert!(
        stderr.contains("disagrees with its recorded event"),
        "{stderr}"
    );
    assert!(!stderr.contains(DERIVED_HINT), "{stderr}");
    assert_success(&fact_set);
    assert_eq!(
        parse_json(&fact_set.stdout)["reviewRounds"]["firstCapture"]["accepted"],
        1,
        "the recorded facts are intact"
    );
}
