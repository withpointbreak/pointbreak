mod support;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use shoreline::model::SnapshotId;
use shoreline::session::{
    ArtifactKind, ArtifactRef, ImportArtifactOptions, LineageListOptions, ReviewHistoryOptions,
    ReviewUnitListOptions, ReviewUnitShowOptions, export_artifact, import_artifact, list_lineages,
    list_review_units, read_events, read_snapshot_artifact, referenced_artifacts, review_history,
    show_review_unit,
};
use support::git_repo::GitRepo;
use support::shore;

/// Linked review fixture: the seed worktree captures one review unit and
/// links it into the clone-local store, then the reader worktree links with
/// nothing local to push. Reads from the reader exercise the linked store.
struct LinkedFixture {
    main: GitRepo,
    _worktree_parent: tempfile::TempDir,
    seed: PathBuf,
    reader: PathBuf,
    seed_review_unit_id: String,
    seed_snapshot_id: String,
    seed_snapshot_artifact_content_hash: String,
}

impl LinkedFixture {
    fn new() -> Self {
        let main = GitRepo::new();
        main.write("README.md", "base\n");
        main.commit_all("base");

        let worktree_parent = tempfile::tempdir().expect("create worktree parent");
        let seed = worktree_parent.path().join("seed");
        add_worktree(main.path(), &seed, "seed");
        let reader = worktree_parent.path().join("reader");
        add_worktree(main.path(), &reader, "reader");

        let mut fixture = Self {
            main,
            _worktree_parent: worktree_parent,
            seed,
            reader,
            seed_review_unit_id: String::new(),
            seed_snapshot_id: String::new(),
            seed_snapshot_artifact_content_hash: String::new(),
        };
        fs::write(fixture.seed.join("README.md"), "changed in seed\n").unwrap();
        let capture = fixture.capture(&fixture.seed);
        fixture.seed_review_unit_id = capture["reviewUnit"]["id"]
            .as_str()
            .expect("capture has review unit id")
            .to_owned();
        fixture.seed_snapshot_id = capture["reviewUnit"]["snapshotId"]
            .as_str()
            .expect("capture has snapshot id")
            .to_owned();
        fixture.seed_snapshot_artifact_content_hash =
            capture["reviewUnit"]["snapshotArtifactContentHash"]
                .as_str()
                .expect("capture has snapshot artifact content hash")
                .to_owned();
        fixture.link(&fixture.seed);
        fixture.link(&fixture.reader);
        fixture
    }

    fn capture(&self, worktree: &Path) -> Value {
        run_shore_json(&["review", "capture", "--repo", worktree.to_str().unwrap()])
    }

    fn link(&self, worktree: &Path) -> Value {
        run_shore_json(&["store", "link", "--repo", worktree.to_str().unwrap()])
    }

    fn observation_add(&self, worktree: &Path, review_unit_id: &str, body: &str) -> Value {
        run_shore_json(&[
            "review",
            "observation",
            "add",
            "--repo",
            worktree.to_str().unwrap(),
            "--review-unit",
            review_unit_id,
            "--track",
            "agent:test-fixture",
            "--title",
            "linked body artifact",
            "--body",
            body,
        ])
    }

    fn linked_store_dir(&self) -> PathBuf {
        self.main.path().join(".git/shoreline")
    }

    fn lineage_attach(&self, worktree: &Path, lineage_id: &str, review_unit_id: &str) -> Value {
        run_shore_json(&[
            "review",
            "lineage",
            "attach",
            "--repo",
            worktree.to_str().unwrap(),
            "--lineage",
            lineage_id,
            "--review-unit",
            review_unit_id,
        ])
    }

    fn history_json(&self, worktree: &Path, include_body: bool) -> Value {
        let mut args = vec!["review", "history", "--repo", worktree.to_str().unwrap()];
        if include_body {
            args.push("--include-body");
        }
        run_shore_json(&args)
    }

    fn unit_show_json(&self, worktree: &Path, review_unit_id: &str) -> Value {
        run_shore_json(&[
            "review",
            "unit",
            "show",
            "--repo",
            worktree.to_str().unwrap(),
            "--review-unit",
            review_unit_id,
            "--include-body",
        ])
    }

    /// Record one of each reviewer-facing fact on the seed's review unit.
    /// Returns the opened input request's id.
    fn seed_full_facts(&self, body: &str) -> String {
        self.observation_add(&self.seed, &self.seed_review_unit_id, body);
        let seed = self.seed.to_str().unwrap();
        let opened = run_shore_json(&[
            "review",
            "input-request",
            "open",
            "--repo",
            seed,
            "--track",
            "agent:test-fixture",
            "--title",
            "Need approval",
            "--reason",
            "manual-decision-required",
            "--body",
            "approve this path?",
        ]);
        run_shore_json(&[
            "review",
            "assessment",
            "add",
            "--repo",
            seed,
            "--track",
            "human:kevin",
            "--assessment",
            "accepted",
            "--summary",
            "ship it",
        ]);
        run_shore_json(&[
            "review",
            "validation",
            "add",
            "--repo",
            seed,
            "--track",
            "agent:test-fixture",
            "--check-name",
            "cargo test",
            "--status",
            "passed",
        ]);
        opened["inputRequestId"]
            .as_str()
            .expect("input request open returns id")
            .to_owned()
    }

    fn respond_input_request(&self, worktree: &Path, input_request_id: &str) -> Value {
        run_shore_json(&[
            "review",
            "input-request",
            "respond",
            input_request_id,
            "--repo",
            worktree.to_str().unwrap(),
            "--outcome",
            "approved",
            "--reason",
            "approved locally",
        ])
    }

    /// Force-remove the seed worktree; its review record survives only in the
    /// linked clone-local store.
    fn remove_seed(&self) {
        run_git_os(
            self.main.path(),
            [
                OsString::from("worktree"),
                OsString::from("remove"),
                OsString::from("--force"),
                self.seed.as_os_str().to_owned(),
            ],
        );
        assert!(!self.seed.exists());
    }

    fn unit_list_json(&self, worktree: &Path) -> Value {
        run_shore_json(&[
            "review",
            "unit",
            "list",
            "--repo",
            worktree.to_str().unwrap(),
        ])
    }
}

/// One fully populated seed unit (every fact kind + response + lineage),
/// linked, with the seed worktree force-removed. The shared arrangement for
/// the deleted-source-worktree matrix.
fn populated_fixture_with_deleted_seed(body: &str, lineage_id: &str) -> (LinkedFixture, String) {
    let fixture = LinkedFixture::new();
    let input_request_id = fixture.seed_full_facts(body);
    fixture.respond_input_request(&fixture.seed, &input_request_id);
    fixture.lineage_attach(&fixture.seed, lineage_id, &fixture.seed_review_unit_id);
    fixture.link(&fixture.seed);
    fixture.remove_seed();
    (fixture, input_request_id)
}

fn assert_no_deleted_path_in_diagnostics(fixture: &LinkedFixture, json: &Value) {
    let diagnostics = json["diagnostics"].to_string();
    assert!(
        !diagnostics.contains(fixture.seed.to_str().unwrap()),
        "diagnostics mention the deleted worktree path: {diagnostics}"
    );
}

#[test]
fn deleted_worktree_unit_list_lists_unit() {
    let (fixture, _) = populated_fixture_with_deleted_seed("m1", "review-unit-lineage:random:m1");

    let json = fixture.unit_list_json(&fixture.reader);

    assert_eq!(json["reviewUnitCount"], 1);
    assert_eq!(
        json["entries"][0]["reviewUnitId"],
        Value::String(fixture.seed_review_unit_id.clone())
    );
    assert_no_deleted_path_in_diagnostics(&fixture, &json);
}

#[test]
fn deleted_worktree_unit_show_renders_composite_with_snapshot() {
    let body = "m".repeat(5000);
    let (fixture, _) = populated_fixture_with_deleted_seed(&body, "review-unit-lineage:random:m2");

    let json = fixture.unit_show_json(&fixture.reader, &fixture.seed_review_unit_id);

    assert_eq!(json["summary"]["observationCount"], 1);
    assert_eq!(json["summary"]["inputRequestCount"], 1);
    assert_eq!(json["summary"]["assessmentCount"], 1);
    assert_eq!(json["summary"]["validationCheckCount"], 1);
    assert!(json["summary"]["snapshotRowCount"].as_u64().unwrap() > 0);
    assert_eq!(json["observations"][0]["body"], Value::String(body));
    assert_no_deleted_path_in_diagnostics(&fixture, &json);
}

#[test]
fn deleted_worktree_history_renders_timeline_with_bodies() {
    let body = "n".repeat(5000);
    let (fixture, _) = populated_fixture_with_deleted_seed(&body, "review-unit-lineage:random:m3");

    let json = fixture.history_json(&fixture.reader, true);

    assert!(json["eventCount"].as_u64().unwrap() > 0);
    assert!(
        json.to_string().contains(&body),
        "hydrated observation body loads from the linked store"
    );
    assert_no_deleted_path_in_diagnostics(&fixture, &json);
}

#[test]
fn deleted_worktree_observation_list_renders_with_hydrated_body() {
    let body = "p".repeat(5000);
    let (fixture, _) = populated_fixture_with_deleted_seed(&body, "review-unit-lineage:random:m4");

    let json = run_shore_json(&[
        "review",
        "observation",
        "list",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
        "--include-body",
    ]);

    assert_eq!(json["observations"].as_array().unwrap().len(), 1);
    assert_eq!(json["observations"][0]["body"], Value::String(body));
    assert_no_deleted_path_in_diagnostics(&fixture, &json);
}

#[test]
fn deleted_worktree_input_request_list_renders_with_response() {
    let (fixture, input_request_id) =
        populated_fixture_with_deleted_seed("m5", "review-unit-lineage:random:m5");

    let json = run_shore_json(&[
        "review",
        "input-request",
        "list",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
        "--status",
        "all",
    ]);

    assert_eq!(json["inputRequests"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["inputRequests"][0]["id"],
        Value::String(input_request_id)
    );
    assert_eq!(
        json["inputRequests"][0]["responses"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_no_deleted_path_in_diagnostics(&fixture, &json);
}

#[test]
fn deleted_worktree_assessment_show_renders() {
    let (fixture, _) = populated_fixture_with_deleted_seed("m6", "review-unit-lineage:random:m6");

    let json = run_shore_json(&[
        "review",
        "assessment",
        "show",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
    ]);

    assert_eq!(json["assessments"].as_array().unwrap().len(), 1);
    assert_eq!(json["assessments"][0]["assessment"], "accepted");
    assert_no_deleted_path_in_diagnostics(&fixture, &json);
}

#[test]
fn deleted_worktree_validation_list_renders() {
    let (fixture, _) = populated_fixture_with_deleted_seed("m7", "review-unit-lineage:random:m7");

    let json = run_shore_json(&[
        "review",
        "validation",
        "list",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
    ]);

    assert_eq!(json["validationChecks"].as_array().unwrap().len(), 1);
    assert_no_deleted_path_in_diagnostics(&fixture, &json);
}

#[test]
fn deleted_worktree_lineage_list_and_show_render() {
    let lineage_id = "review-unit-lineage:random:m8";
    let (fixture, _) = populated_fixture_with_deleted_seed("m8", lineage_id);

    let list = list_lineages(LineageListOptions::new(&fixture.reader)).unwrap();
    assert_eq!(list.lineage_count, 1);
    assert_eq!(list.entries[0].lineage_id.as_str(), lineage_id);

    let show = run_shore_json(&[
        "review",
        "lineage",
        "show",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--lineage",
        lineage_id,
    ]);
    assert_eq!(
        show["headReviewUnitId"],
        Value::String(fixture.seed_review_unit_id.clone())
    );
    assert_eq!(show["rounds"].as_array().unwrap().len(), 1);
    assert_no_deleted_path_in_diagnostics(&fixture, &show);
}

#[test]
fn linked_reads_agree_on_event_set_hash_across_surfaces() {
    let fixture = LinkedFixture::new();
    fixture.seed_full_facts("short body");
    fixture.link(&fixture.seed);

    let reader_units = fixture.unit_list_json(&fixture.reader);
    let reader_history = fixture.history_json(&fixture.reader, false);
    let seed_units = fixture.unit_list_json(&fixture.seed);
    let seed_history = fixture.history_json(&fixture.seed, false);

    // Issue #140's regression signal, inverted into the standing guard: every
    // read surface in every linked checkout reports one eventSetHash.
    let hash = event_set_hash(&reader_units);
    assert!(hash.starts_with("sha256:"));
    assert_eq!(event_set_hash(&reader_history), hash);
    assert_eq!(event_set_hash(&seed_units), hash);
    assert_eq!(event_set_hash(&seed_history), hash);
    assert_eq!(reader_units["eventCount"], reader_history["eventCount"]);
    assert_eq!(reader_units["eventCount"], seed_units["eventCount"]);
}

#[test]
fn divergence_diagnostic_appears_then_clears_after_store_link() {
    let fixture = LinkedFixture::new();
    let code = "clone_local_unsynced_local_events";

    // Synced: no divergence diagnostic on any reader surface.
    let units = fixture.unit_list_json(&fixture.reader);
    let history = fixture.history_json(&fixture.reader, false);
    assert!(!has_diagnostic(&units, code));
    assert!(!has_diagnostic(&history, code));
    let synced_hash = event_set_hash(&units).to_owned();

    // The reader captures locally (writes land worktree-local): both surfaces
    // report the gap, the local unit stays invisible (store-only), and the
    // hash still reflects the linked store alone.
    fs::write(fixture.reader.join("README.md"), "changed in reader\n").unwrap();
    let local_capture = fixture.capture(&fixture.reader);
    let local_unit_id = local_capture["reviewUnit"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let units = fixture.unit_list_json(&fixture.reader);
    let history = fixture.history_json(&fixture.reader, false);
    assert!(has_diagnostic(&units, code), "{}", units["diagnostics"]);
    assert!(has_diagnostic(&history, code), "{}", history["diagnostics"]);
    assert!(diagnostic_message(&units, code).contains("1 local event"));
    assert_eq!(units["reviewUnitCount"], 1);
    assert!(!units["entries"].to_string().contains(&local_unit_id));
    assert_eq!(event_set_hash(&units), synced_hash);
    assert_eq!(event_set_hash(&history), synced_hash);

    // After store link from the reader: diagnostic gone, the unit appears,
    // and the hash advances in step on both surfaces.
    fixture.link(&fixture.reader);
    let units = fixture.unit_list_json(&fixture.reader);
    let history = fixture.history_json(&fixture.reader, false);
    assert!(!has_diagnostic(&units, code), "{}", units["diagnostics"]);
    assert!(
        !has_diagnostic(&history, code),
        "{}",
        history["diagnostics"]
    );
    assert_eq!(units["reviewUnitCount"], 2);
    assert!(units["entries"].to_string().contains(&local_unit_id));
    let advanced_hash = event_set_hash(&units);
    assert_ne!(advanced_hash, synced_hash);
    assert_eq!(event_set_hash(&history), advanced_hash);
}

/// Research-0008 Q5's lock-free claim, made executable: a reader racing a
/// `store link` import may see fewer events, never a broken set. Per-event
/// exclusive-create writes and artifact-before-event publication mean every
/// intermediate read parses, hydrates every surfaced body, and resolves every
/// listed unit's bound snapshot.
#[test]
fn reads_racing_store_link_always_see_consistent_event_sets() {
    let main = GitRepo::new();
    main.write("README.md", "base\n");
    main.commit_all("base");

    let parent = tempfile::tempdir().expect("worktree parent");
    let seed = parent.path().join("seed");
    add_worktree(main.path(), &seed, "seed");
    let reader = parent.path().join("reader");
    add_worktree(main.path(), &reader, "reader");

    // The reader registers first, so its reads resolve the still-empty
    // clone-local family store while the seed's record is unlinked.
    run_shore_json(&["store", "link", "--repo", reader.to_str().unwrap()]);

    // A meaty unlinked event set: two captures with snapshot artifacts plus a
    // spread of facts, most with distinct body artifacts.
    let seed_arg = seed.to_str().unwrap().to_owned();
    fs::write(seed.join("README.md"), "changed in seed\n").unwrap();
    let first_capture = run_shore_json(&["review", "capture", "--repo", &seed_arg]);
    let unit_a = first_capture["reviewUnit"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for index in 0..12 {
        let title = format!("observation {index}");
        let body = format!("{}{index}", "r".repeat(5000));
        run_shore_json(&[
            "review",
            "observation",
            "add",
            "--repo",
            &seed_arg,
            "--review-unit",
            &unit_a,
            "--track",
            "agent:racer",
            "--title",
            &title,
            "--body",
            &body,
        ]);
    }
    for index in 0..6 {
        let title = format!("input request {index}");
        run_shore_json(&[
            "review",
            "input-request",
            "open",
            "--repo",
            &seed_arg,
            "--track",
            "agent:racer",
            "--title",
            &title,
            "--reason",
            "manual-decision-required",
            "--body",
            "answer?",
        ]);
    }
    for track in ["human:kevin", "human:other"] {
        run_shore_json(&[
            "review",
            "assessment",
            "add",
            "--repo",
            &seed_arg,
            "--track",
            track,
            "--assessment",
            "accepted",
            "--summary",
            "ship it",
        ]);
    }
    for index in 0..6 {
        let check_name = format!("check {index}");
        run_shore_json(&[
            "review",
            "validation",
            "add",
            "--repo",
            &seed_arg,
            "--track",
            "agent:racer",
            "--check-name",
            &check_name,
            "--status",
            "passed",
        ]);
    }
    fs::write(seed.join("README.md"), "changed in seed again\n").unwrap();
    run_shore_json(&["review", "capture", "--repo", &seed_arg]);

    let seed_before_link = list_review_units(ReviewUnitListOptions::new(&seed)).unwrap();
    let expected_count = seed_before_link.event_count;
    assert!(expected_count >= 20, "meaty seed set: {expected_count}");

    let mut child = Command::new(env!("CARGO_BIN_EXE_shore"))
        .args(["store", "link", "--repo", &seed_arg])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn shore store link");

    let mut last_count = 0usize;
    let mut iterations = 0usize;
    let link_status = loop {
        iterations += 1;
        let list =
            list_review_units(ReviewUnitListOptions::new(&reader)).expect("unit list during race");
        review_history(ReviewHistoryOptions::new(&reader).with_include_body(true))
            .expect("history with bodies during race");
        assert!(
            list.event_count >= last_count,
            "event count regressed: {last_count} -> {}",
            list.event_count
        );
        last_count = list.event_count;
        for entry in &list.entries {
            let show = show_review_unit(
                ReviewUnitShowOptions::new(&reader)
                    .with_review_unit_id(entry.review_unit_id.clone())
                    .with_include_body(true),
            )
            .expect("listed unit resolves its bound snapshot during race");
            assert_eq!(show.review_unit.id, entry.review_unit_id);
        }
        if let Some(status) = child.try_wait().expect("poll store link subprocess") {
            break status;
        }
        assert!(iterations < 5000, "store link subprocess did not finish");
    };
    assert!(link_status.success(), "store link failed during race");

    let final_list = list_review_units(ReviewUnitListOptions::new(&reader)).unwrap();
    assert_eq!(final_list.event_count, expected_count);
    assert_eq!(final_list.review_unit_count, 2);
    let seed_after_link = list_review_units(ReviewUnitListOptions::new(&seed)).unwrap();
    assert_eq!(final_list.event_set_hash, seed_after_link.event_set_hash);
}

#[test]
fn linked_local_capture_file_target_observation_resolves_artifact() {
    let fixture = LinkedFixture::new();

    // The reader captures locally: unit C and its snapshot artifact land in the
    // reader's worktree-local .shore and are NOT yet copied by store link.
    fs::write(fixture.reader.join("README.md"), "changed in reader\n").unwrap();
    let capture = fixture.capture(&fixture.reader);
    let local_unit_id = capture["reviewUnit"]["id"]
        .as_str()
        .expect("local capture has review unit id")
        .to_owned();

    // A file-targeted observation against that locally captured unit. Before the
    // fallback fix this is RED: resolve_observation_target reads the snapshot
    // artifact from the linked store, where store link has not copied it, so it
    // fails with "missing artifact for snapshot ...". After the fix it succeeds,
    // reading the artifact from the reader's worktree-local .shore.
    let json = run_shore_json(&[
        "review",
        "observation",
        "add",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &local_unit_id,
        "--track",
        "agent:test-fixture",
        "--title",
        "file targeted on local capture",
        "--file",
        "README.md",
    ]);

    assert_eq!(json["target"]["kind"], "file");
    assert_eq!(json["target"]["filePath"], "README.md");
    assert_eq!(json["target"]["reviewUnitId"], Value::String(local_unit_id));
}

#[test]
fn linked_reader_attaches_observation_to_linked_only_unit() {
    let fixture = LinkedFixture::new();

    // The reader attaches an observation to the seed's linked-only unit. Before
    // the migration this is RED: record_observation validates against the
    // reader's empty worktree-local store and fails with "unknown review unit".
    let result = fixture.observation_add(
        &fixture.reader,
        &fixture.seed_review_unit_id,
        "cross-worktree note",
    );

    assert_eq!(result["eventsCreated"], 1);
    assert!(
        result["observationId"].as_str().is_some(),
        "result carries an observation id: {result}"
    );
}

#[test]
fn linked_reader_observation_result_carries_fact_batch_only_diagnostic() {
    let fixture = LinkedFixture::new();

    let result = fixture.observation_add(&fixture.reader, &fixture.seed_review_unit_id, "note");

    assert!(
        diagnostic_codes(&result).contains(&"clone_local_fact_batch_only"),
        "diagnostics: {}",
        result["diagnostics"]
    );
}

#[test]
fn worktree_local_observation_add_has_no_fact_batch_only_diagnostic() {
    // Unlinked repo: the batch-only diagnostic is linked-mode only, so it must
    // be ABSENT here (additive contract).
    let repo = GitRepo::new();
    repo.write("README.md", "base\n");
    repo.commit_all("base");
    repo.write("README.md", "changed locally\n");
    let capture = run_shore_json(&["review", "capture", "--repo", repo.path().to_str().unwrap()]);
    let unit_id = capture["reviewUnit"]["id"].as_str().unwrap().to_owned();

    let result = run_shore_json(&[
        "review",
        "observation",
        "add",
        "--repo",
        repo.path().to_str().unwrap(),
        "--review-unit",
        &unit_id,
        "--track",
        "agent:test-fixture",
        "--title",
        "local note",
        "--body",
        "body",
    ]);

    assert_eq!(result["eventsCreated"], 1);
    assert!(
        !diagnostic_codes(&result).contains(&"clone_local_fact_batch_only"),
        "diagnostics: {}",
        result["diagnostics"]
    );
}

#[test]
fn linked_reader_opens_input_request_against_linked_only_unit() {
    let fixture = LinkedFixture::new();

    // RED today: open_input_request validates against the reader's empty local
    // store and fails with "unknown review unit".
    let result = run_shore_json(&[
        "review",
        "input-request",
        "open",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
        "--track",
        "agent:test-fixture",
        "--title",
        "cross-worktree question",
        "--reason",
        "manual-decision-required",
        "--body",
        "approve?",
    ]);

    assert_eq!(result["eventsCreated"], 1);
    assert!(result["inputRequestId"].as_str().is_some());
    assert!(
        diagnostic_codes(&result).contains(&"clone_local_fact_batch_only"),
        "diagnostics: {}",
        result["diagnostics"]
    );
}

#[test]
fn linked_reader_opens_input_request_with_observation_ref_target() {
    let fixture = LinkedFixture::new();

    // The seed records an observation on its unit and links it; the observation
    // now lives only in the linked store.
    let observation = fixture.observation_add(
        &fixture.seed,
        &fixture.seed_review_unit_id,
        "observation to reference",
    );
    let observation_id = observation["observationId"]
        .as_str()
        .expect("seed observation has an id")
        .to_owned();
    fixture.link(&fixture.seed);

    // RED today: resolve_input_request_target cannot see the linked-only
    // observation from the reader's local store.
    let result = run_shore_json(&[
        "review",
        "input-request",
        "open",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
        "--observation",
        &observation_id,
        "--track",
        "agent:test-fixture",
        "--title",
        "question about that observation",
        "--reason",
        "manual-decision-required",
        "--body",
        "is this right?",
    ]);

    assert_eq!(result["eventsCreated"], 1);
    assert_eq!(result["target"]["kind"], "observation");
    assert_eq!(
        result["target"]["observationId"],
        Value::String(observation_id)
    );
}

#[test]
fn linked_reader_responds_to_linked_only_input_request() {
    let fixture = LinkedFixture::new();
    let request_id = fixture.seed_full_facts("seed body");
    // The opened request lives in the seed's local store until link; copy it to
    // the linked store so the reader can see it.
    fixture.link(&fixture.seed);

    // RED today: respond_input_request reads the reader's empty local store and
    // fails with "unknown input request".
    let result = fixture.respond_input_request(&fixture.reader, &request_id);

    assert_eq!(result["eventsCreated"], 1);
    assert_eq!(result["outcome"], "approved");
    assert!(
        diagnostic_codes(&result).contains(&"clone_local_fact_batch_only"),
        "diagnostics: {}",
        result["diagnostics"]
    );
}

#[test]
fn linked_reader_respond_copies_request_event_target_fields() {
    let fixture = LinkedFixture::new();
    let request_id = fixture.seed_full_facts("seed body");
    fixture.link(&fixture.seed);

    fixture.respond_input_request(&fixture.reader, &request_id);

    // The reader had nothing local; responding writes exactly the response event
    // to its worktree-local store. Its EventTarget must be copied verbatim from
    // the union-read request, not fabricated.
    let events = read_events(&fixture.reader).expect("read reader worktree events");
    assert_eq!(events.len(), 1, "only the response event is local");
    let response = &events[0];
    assert_eq!(
        response
            .target
            .review_unit_id
            .as_ref()
            .map(|id| id.as_str()),
        Some(fixture.seed_review_unit_id.as_str())
    );
    assert_eq!(
        response.target.snapshot_id.as_ref().map(|id| id.as_str()),
        Some(fixture.seed_snapshot_id.as_str())
    );
    assert_eq!(
        response.target.track_id.as_ref().map(|id| id.as_str()),
        Some("agent:test-fixture")
    );
}

#[test]
fn linked_reader_records_assessment_on_linked_only_unit() {
    let fixture = LinkedFixture::new();

    // RED today: record_assessment validates against the reader's empty local
    // store and fails with "unknown review unit".
    let result = run_shore_json(&[
        "review",
        "assessment",
        "add",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
        "--track",
        "human:kevin",
        "--assessment",
        "accepted",
        "--summary",
        "ship it",
    ]);

    assert_eq!(result["eventsCreated"], 1);
    assert!(result["assessmentId"].as_str().is_some());
    assert!(
        diagnostic_codes(&result).contains(&"clone_local_fact_batch_only"),
        "diagnostics: {}",
        result["diagnostics"]
    );
}

#[test]
fn linked_reader_assessment_relates_linked_only_observation() {
    let fixture = LinkedFixture::new();
    let observation = fixture.observation_add(
        &fixture.seed,
        &fixture.seed_review_unit_id,
        "observation to relate",
    );
    let observation_id = observation["observationId"]
        .as_str()
        .expect("seed observation id")
        .to_owned();
    fixture.link(&fixture.seed);

    // RED today: relationship validation reads the reader's local store and
    // fails with "unknown observation".
    let result = run_shore_json(&[
        "review",
        "assessment",
        "add",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
        "--track",
        "human:kevin",
        "--assessment",
        "accepted",
        "--summary",
        "looks good",
        "--related-observation",
        &observation_id,
    ]);

    assert_eq!(result["eventsCreated"], 1);
}

#[test]
fn linked_reader_assessment_replaces_linked_only_assessment() {
    let fixture = LinkedFixture::new();
    let seed_assessment = run_shore_json(&[
        "review",
        "assessment",
        "add",
        "--repo",
        fixture.seed.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
        "--track",
        "human:kevin",
        "--assessment",
        "accepted",
        "--summary",
        "first pass",
    ]);
    let assessment_id = seed_assessment["assessmentId"]
        .as_str()
        .expect("seed assessment id")
        .to_owned();
    fixture.link(&fixture.seed);

    // RED today: --replaces validation reads the reader's local store and fails
    // with "unknown assessment".
    let result = run_shore_json(&[
        "review",
        "assessment",
        "add",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
        "--track",
        "human:kevin",
        "--assessment",
        "needs-changes",
        "--summary",
        "second pass",
        "--replaces",
        &assessment_id,
    ]);

    assert_eq!(result["eventsCreated"], 1);
}

#[test]
fn linked_reader_records_validation_on_linked_only_unit() {
    let fixture = LinkedFixture::new();

    // RED today: record_validation_check validates against the reader's empty
    // local store and fails with "unknown review unit".
    let result = run_shore_json(&[
        "review",
        "validation",
        "add",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
        "--track",
        "agent:test-fixture",
        "--check-name",
        "cargo test",
        "--status",
        "passed",
    ]);

    assert_eq!(result["eventsCreated"], 1);
    assert!(
        diagnostic_codes(&result).contains(&"clone_local_fact_batch_only"),
        "diagnostics: {}",
        result["diagnostics"]
    );
}

#[test]
fn linked_reader_attaches_linked_only_unit_to_lineage() {
    let fixture = LinkedFixture::new();

    // RED today: stored_capture_payload reads the reader's empty local store and
    // fails with "unknown review unit".
    let result = fixture.lineage_attach(
        &fixture.reader,
        "review-unit-lineage:random:wv-attach",
        &fixture.seed_review_unit_id,
    );

    assert_eq!(
        result["headReviewUnitId"],
        Value::String(fixture.seed_review_unit_id.clone())
    );
    assert_eq!(result["eventsCreated"], 2);
    assert!(
        diagnostic_codes(&result).contains(&"clone_local_fact_batch_only"),
        "diagnostics: {}",
        result["diagnostics"]
    );
}

#[test]
fn linked_reader_lineage_result_projection_uses_union_for_head() {
    let fixture = LinkedFixture::new();
    let lineage_id = "review-unit-lineage:random:wv-union-head";
    let unit_a = fixture.seed_review_unit_id.clone();

    // The seed attaches A to the lineage, then captures a successor B; linking
    // puts A, B, and A's lineage round in the linked store.
    fixture.lineage_attach(&fixture.seed, lineage_id, &unit_a);
    fs::write(fixture.seed.join("README.md"), "changed in seed again\n").unwrap();
    let capture_b = fixture.capture(&fixture.seed);
    let unit_b = capture_b["reviewUnit"]["id"]
        .as_str()
        .expect("successor capture id")
        .to_owned();
    fixture.link(&fixture.seed);

    // The reader attaches B with predecessor A — a clean extension. The result
    // projection must see A's prior round (linked) plus the two new local events,
    // so B is a clean head, not a spurious fork.
    let result = run_shore_json(&[
        "review",
        "lineage",
        "attach",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--lineage",
        lineage_id,
        "--review-unit",
        &unit_b,
        "--predecessor",
        &unit_a,
    ]);

    assert_eq!(result["headReviewUnitId"], Value::String(unit_b));
    assert!(
        !diagnostic_codes(&result).contains(&"lineage_forked_successor"),
        "spurious fork diagnostic: {}",
        result["diagnostics"]
    );
    assert!(
        !diagnostic_codes(&result).contains(&"lineage_multiple_heads"),
        "spurious multiple-heads diagnostic: {}",
        result["diagnostics"]
    );
}

fn event_set_hash(json: &Value) -> &str {
    json["eventSetHash"].as_str().expect("eventSetHash present")
}

fn has_diagnostic(json: &Value, code: &str) -> bool {
    diagnostic_codes(json).contains(&code)
}

fn run_shore_json(args: &[&str]) -> Value {
    let output = shore(args.iter().copied());
    assert!(
        output.status.success(),
        "shore {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("shore stdout is json")
}

fn diagnostic_codes(json: &Value) -> Vec<&str> {
    json["diagnostics"]
        .as_array()
        .map(|diagnostics| {
            diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic["code"].as_str())
                .collect()
        })
        .unwrap_or_default()
}

fn diagnostic_message(json: &Value, code: &str) -> String {
    json["diagnostics"]
        .as_array()
        .and_then(|diagnostics| {
            diagnostics
                .iter()
                .find(|diagnostic| diagnostic["code"] == code)
        })
        .and_then(|diagnostic| diagnostic["message"].as_str())
        .unwrap_or_else(|| panic!("no diagnostic with code {code}"))
        .to_owned()
}

fn add_worktree(repo: &Path, path: &Path, branch: &str) {
    run_git_os(
        repo,
        [
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("-b"),
            OsString::from(branch),
            path.as_os_str().to_owned(),
        ],
    );
}

fn run_git_os<I>(cwd: &Path, args: I)
where
    I: IntoIterator<Item = OsString>,
{
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|error| panic!("run git in {}: {error}", cwd.display()));
    assert!(
        output.status.success(),
        "git failed in {}\nstdout:\n{}\nstderr:\n{}",
        cwd.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn linked_unit_list_reports_unsynced_local_events_diagnostic() {
    let fixture = LinkedFixture::new();
    fs::write(fixture.reader.join("README.md"), "changed in reader\n").unwrap();
    fixture.capture(&fixture.reader);

    let json = fixture.unit_list_json(&fixture.reader);

    // Store-only: the reader's local capture is not listed; only the seed's
    // linked unit is.
    assert_eq!(json["reviewUnitCount"], 1);
    assert_eq!(
        json["entries"][0]["reviewUnitId"],
        Value::String(fixture.seed_review_unit_id.clone())
    );
    let codes = diagnostic_codes(&json);
    assert!(
        codes.contains(&"clone_local_unsynced_local_events"),
        "diagnostics: {}",
        json["diagnostics"]
    );
    let message = diagnostic_message(&json, "clone_local_unsynced_local_events");
    assert!(message.contains("1 local event"), "message: {message}");
    assert!(message.contains("shore store link"), "message: {message}");
}

#[test]
fn linked_unit_list_without_local_events_has_no_divergence_diagnostic() {
    let fixture = LinkedFixture::new();

    let json = fixture.unit_list_json(&fixture.reader);

    assert_eq!(json["reviewUnitCount"], 1);
    assert_eq!(json["eventCount"], 1);
    assert_eq!(
        json["entries"][0]["reviewUnitId"],
        Value::String(fixture.seed_review_unit_id.clone())
    );
    assert!(
        json["eventSetHash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert!(
        !diagnostic_codes(&json).contains(&"clone_local_unsynced_local_events"),
        "diagnostics: {}",
        json["diagnostics"]
    );
}

#[test]
fn linked_history_reads_full_timeline_from_linked_store() {
    let fixture = LinkedFixture::new();
    let body = "h".repeat(5000);
    fixture.observation_add(&fixture.seed, &fixture.seed_review_unit_id, &body);
    fixture.link(&fixture.seed);

    let json = fixture.history_json(&fixture.reader, true);

    assert_eq!(json["eventCount"], 2);
    let event_types: Vec<&str> = json["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["eventType"].as_str())
        .collect();
    assert!(
        event_types.contains(&"review_unit_captured"),
        "{event_types:?}"
    );
    assert!(
        event_types.contains(&"review_observation_recorded"),
        "{event_types:?}"
    );
    assert!(
        json.to_string().contains(&body),
        "hydrated observation body loads from the linked store"
    );
}

#[test]
fn linked_history_emits_divergence_diagnostic_with_local_only_events() {
    let fixture = LinkedFixture::new();
    fs::write(fixture.reader.join("README.md"), "changed in reader\n").unwrap();
    let local_capture = fixture.capture(&fixture.reader);
    let local_unit_id = local_capture["reviewUnit"]["id"].as_str().unwrap();

    let json = fixture.history_json(&fixture.reader, false);

    assert!(
        diagnostic_codes(&json).contains(&"clone_local_unsynced_local_events"),
        "diagnostics: {}",
        json["diagnostics"]
    );
    // Store-only: the reader's unsynced local capture is not in the timeline.
    assert_eq!(json["eventCount"], 1);
    assert!(!json["entries"].to_string().contains(local_unit_id));
}

#[test]
fn linked_unit_show_resolves_linked_only_unit() {
    let fixture = LinkedFixture::new();
    let body = "o".repeat(5000);
    fixture.seed_full_facts(&body);
    fixture.link(&fixture.seed);

    let json = fixture.unit_show_json(&fixture.reader, &fixture.seed_review_unit_id);

    assert_eq!(
        json["reviewUnit"]["id"],
        Value::String(fixture.seed_review_unit_id.clone())
    );
    assert_eq!(json["summary"]["observationCount"], 1);
    assert_eq!(json["summary"]["inputRequestCount"], 1);
    assert_eq!(json["summary"]["assessmentCount"], 1);
    assert_eq!(json["summary"]["validationCheckCount"], 1);
    assert_eq!(
        json["observations"][0]["body"],
        Value::String(body),
        "observation body hydrates from the linked store"
    );
}

#[test]
fn linked_unit_show_loads_bound_snapshot_from_linked_store() {
    let fixture = LinkedFixture::new();

    let json = fixture.unit_show_json(&fixture.reader, &fixture.seed_review_unit_id);

    assert_eq!(
        json["reviewUnit"]["snapshotArtifactContentHash"],
        Value::String(fixture.seed_snapshot_artifact_content_hash.clone())
    );
    assert!(
        json["summary"]["snapshotRowCount"].as_u64().unwrap() > 0,
        "bound snapshot rows project from the linked artifact"
    );
}

#[test]
fn linked_unit_show_emits_divergence_diagnostic_with_local_only_events() {
    let fixture = LinkedFixture::new();
    fs::write(fixture.reader.join("README.md"), "changed in reader\n").unwrap();
    fixture.capture(&fixture.reader);

    let json = fixture.unit_show_json(&fixture.reader, &fixture.seed_review_unit_id);

    assert!(
        diagnostic_codes(&json).contains(&"clone_local_unsynced_local_events"),
        "diagnostics: {}",
        json["diagnostics"]
    );
}

#[test]
fn linked_observation_list_resolves_linked_unit() {
    let fixture = LinkedFixture::new();
    let body = "b".repeat(5000);
    fixture.seed_full_facts(&body);
    fixture.link(&fixture.seed);

    let json = run_shore_json(&[
        "review",
        "observation",
        "list",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
        "--include-body",
    ]);

    assert_eq!(
        json["reviewUnitId"],
        Value::String(fixture.seed_review_unit_id.clone())
    );
    assert_eq!(json["observations"].as_array().unwrap().len(), 1);
    assert_eq!(json["observations"][0]["body"], Value::String(body));
}

#[test]
fn linked_observation_list_emits_divergence_diagnostic_with_local_only_events() {
    let fixture = LinkedFixture::new();
    let body = "c".repeat(64);
    fixture.seed_full_facts(&body);
    fixture.link(&fixture.seed);
    fs::write(fixture.reader.join("README.md"), "changed in reader\n").unwrap();
    fixture.capture(&fixture.reader);

    let json = run_shore_json(&[
        "review",
        "observation",
        "list",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
    ]);

    assert!(
        diagnostic_codes(&json).contains(&"clone_local_unsynced_local_events"),
        "diagnostics: {}",
        json["diagnostics"]
    );
}

#[test]
fn linked_input_request_list_resolves_linked_unit() {
    let fixture = LinkedFixture::new();
    fixture.seed_full_facts("short body");
    fixture.link(&fixture.seed);

    let json = run_shore_json(&[
        "review",
        "input-request",
        "list",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
    ]);

    assert_eq!(
        json["reviewUnitId"],
        Value::String(fixture.seed_review_unit_id.clone())
    );
    assert_eq!(json["inputRequests"].as_array().unwrap().len(), 1);
    assert_eq!(json["inputRequests"][0]["title"], "Need approval");
}

#[test]
fn linked_input_request_fetch_resolves_linked_request() {
    let fixture = LinkedFixture::new();
    let input_request_id = fixture.seed_full_facts("short body");
    fixture.link(&fixture.seed);

    let json = run_shore_json(&[
        "review",
        "input-request",
        "fetch",
        &input_request_id,
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--include-body",
    ]);

    assert_eq!(json["inputRequest"]["id"], Value::String(input_request_id));
    assert_eq!(json["inputRequest"]["title"], "Need approval");
}

#[test]
fn linked_assessment_show_resolves_linked_unit() {
    let fixture = LinkedFixture::new();
    fixture.seed_full_facts("short body");
    fixture.link(&fixture.seed);

    let json = run_shore_json(&[
        "review",
        "assessment",
        "show",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
    ]);

    assert_eq!(
        json["reviewUnitId"],
        Value::String(fixture.seed_review_unit_id.clone())
    );
    assert_eq!(json["assessments"].as_array().unwrap().len(), 1);
    assert_eq!(json["assessments"][0]["assessment"], "accepted");
}

#[test]
fn linked_validation_list_resolves_linked_unit() {
    let fixture = LinkedFixture::new();
    fixture.seed_full_facts("short body");
    fixture.link(&fixture.seed);

    let json = run_shore_json(&[
        "review",
        "validation",
        "list",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--review-unit",
        &fixture.seed_review_unit_id,
    ]);

    assert_eq!(
        json["reviewUnitId"],
        Value::String(fixture.seed_review_unit_id.clone())
    );
    assert_eq!(json["validationChecks"].as_array().unwrap().len(), 1);
    assert_eq!(json["validationChecks"][0]["checkName"], "cargo test");
}

#[test]
fn linked_lineage_list_sees_linked_lineage() {
    let fixture = LinkedFixture::new();
    let lineage_id = "review-unit-lineage:random:linked-test";
    fixture.lineage_attach(&fixture.seed, lineage_id, &fixture.seed_review_unit_id);
    fixture.link(&fixture.seed);

    let result = list_lineages(LineageListOptions::new(&fixture.reader)).unwrap();

    assert_eq!(result.lineage_count, 1);
    assert_eq!(result.entries[0].lineage_id.as_str(), lineage_id);
    assert_eq!(
        result.entries[0]
            .head_review_unit_id
            .as_ref()
            .map(|id| id.as_str()),
        Some(fixture.seed_review_unit_id.as_str())
    );
}

#[test]
fn linked_lineage_list_emits_divergence_diagnostic_with_local_only_events() {
    let fixture = LinkedFixture::new();
    let lineage_id = "review-unit-lineage:random:diag-test";
    fixture.lineage_attach(&fixture.seed, lineage_id, &fixture.seed_review_unit_id);
    fixture.link(&fixture.seed);
    fs::write(fixture.reader.join("README.md"), "changed in reader\n").unwrap();
    fixture.capture(&fixture.reader);

    let result = list_lineages(LineageListOptions::new(&fixture.reader)).unwrap();

    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "clone_local_unsynced_local_events"),
        "diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn linked_lineage_show_resolves_rounds_from_linked_store() {
    let fixture = LinkedFixture::new();
    let lineage_id = "review-unit-lineage:random:show-test";
    fixture.lineage_attach(&fixture.seed, lineage_id, &fixture.seed_review_unit_id);
    fixture.link(&fixture.seed);

    let json = run_shore_json(&[
        "review",
        "lineage",
        "show",
        "--repo",
        fixture.reader.to_str().unwrap(),
        "--lineage",
        lineage_id,
    ]);

    assert_eq!(json["lineageId"], lineage_id);
    assert_eq!(
        json["headReviewUnitId"],
        Value::String(fixture.seed_review_unit_id.clone())
    );
    let rounds = json["rounds"].as_array().unwrap();
    assert_eq!(rounds.len(), 1);
    assert_eq!(
        rounds[0]["reviewUnitId"],
        Value::String(fixture.seed_review_unit_id.clone())
    );
    assert_eq!(rounds[0]["isHead"], true);
}

#[test]
fn snapshot_artifact_reads_from_linked_store() {
    let fixture = LinkedFixture::new();
    let snapshot_id = SnapshotId::new(fixture.seed_snapshot_id.clone());

    let artifact = read_snapshot_artifact(&fixture.reader, &snapshot_id)
        .expect("snapshot artifact reads from the linked store");

    assert_eq!(artifact.snapshot.snapshot_id, snapshot_id);
    assert_eq!(
        artifact.review_unit_id.as_str(),
        fixture.seed_review_unit_id
    );
}

#[test]
fn export_artifact_body_reads_from_linked_store() {
    let fixture = LinkedFixture::new();
    let body = "x".repeat(5000);
    fixture.observation_add(&fixture.seed, &fixture.seed_review_unit_id, &body);
    fixture.link(&fixture.seed);

    let body_ref = seed_body_artifact_ref(&fixture);
    let bytes = export_artifact(&fixture.reader, &body_ref)
        .expect("body artifact exports from the linked store");

    assert!(!bytes.is_empty());
}

#[test]
fn import_artifact_still_writes_worktree_local_in_linked_mode() {
    let fixture = LinkedFixture::new();
    let body = "y".repeat(5000);
    fixture.observation_add(&fixture.seed, &fixture.seed_review_unit_id, &body);

    let body_ref = seed_body_artifact_ref(&fixture);
    let artifact_relative_path = format!(
        "artifacts/notes/{}.json",
        body_ref
            .content_hash()
            .strip_prefix("sha256:")
            .expect("body content hash is sha256-prefixed")
    );
    let bytes = fs::read(fixture.seed.join(".shore").join(&artifact_relative_path)).unwrap();

    import_artifact(ImportArtifactOptions::new(&fixture.reader, body_ref, bytes))
        .expect("import into the linked reader succeeds");

    // Writes stay worktree-local until shared-store writes land: the artifact
    // lands in the reader's own .shore, not the linked clone-local store.
    assert!(
        fixture
            .reader
            .join(".shore")
            .join(&artifact_relative_path)
            .is_file()
    );
    assert!(
        !fixture
            .linked_store_dir()
            .join(&artifact_relative_path)
            .exists()
    );
}

fn seed_body_artifact_ref(fixture: &LinkedFixture) -> ArtifactRef {
    let events = read_events(&fixture.seed).expect("read seed worktree events");
    referenced_artifacts(&events)
        .expect("derive artifact refs from seed events")
        .into_iter()
        .find(|artifact| artifact.kind() == ArtifactKind::Body)
        .expect("seed events reference a body artifact")
}

#[test]
fn worktree_local_unit_list_is_unchanged() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    run_shore_json(&["review", "capture", "--repo", repo.path().to_str().unwrap()]);

    let json = run_shore_json(&[
        "review",
        "unit",
        "list",
        "--repo",
        repo.path().to_str().unwrap(),
    ]);

    assert_eq!(json["schema"], "shore.review-unit-list");
    assert_eq!(json["version"], 1);
    assert_eq!(json["eventCount"], 1);
    assert_eq!(json["reviewUnitCount"], 1);
    assert!(
        !diagnostic_codes(&json).contains(&"clone_local_unsynced_local_events"),
        "diagnostics: {}",
        json["diagnostics"]
    );
}
