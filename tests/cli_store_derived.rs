mod support;

use std::fs::{self, OpenOptions};

use serde_json::Value;
use support::git_repo::GitRepo;
use support::{common_dir_store, pointbreak, pointbreak_env};

const ACTIVE: &[(&str, &str)] = &[("POINTBREAK_DERIVED_ACCESS", "sqlite-wal-bodyless-v1")];

#[test]
fn derived_status_is_side_effect_free_when_off_or_absent() {
    let repo = GitRepo::new();
    let repo_arg = repo.path().to_str().unwrap();
    let store = common_dir_store(repo.path());

    let off = pointbreak(["store", "derived", "status", "--repo", repo_arg]);
    assert!(
        off.status.success(),
        "{}",
        String::from_utf8_lossy(&off.stderr)
    );
    let off_json = parse_json(&off.stdout);
    assert_eq!(off_json["schema"], "pointbreak.store-derived-status");
    assert_eq!(off_json["version"], 1);
    assert_eq!(off_json["active"], false);
    assert_eq!(off_json["availability"], "absent");
    assert!(off.stderr.is_empty());
    assert!(!store.join("derived").exists());
    assert!(!store.join(".pointbreak-derived").exists());

    let active = pointbreak_env(["store", "derived", "status", "--repo", repo_arg], ACTIVE);
    assert!(
        active.status.success(),
        "{}",
        String::from_utf8_lossy(&active.stderr)
    );
    let active_json = parse_json(&active.stdout);
    assert_eq!(active_json["active"], true);
    assert_eq!(active_json["availability"], "absent");
    assert_eq!(active_json["namespace"], "stable");
    assert!(active.stderr.is_empty());
    assert!(!store.join("derived").exists());
    assert!(!store.join("derived.rebuild.lock").exists());

    fs::create_dir_all(store.join("derived/generations/orphaned")).unwrap();
    let rebuild_required =
        pointbreak_env(["store", "derived", "status", "--repo", repo_arg], ACTIVE);
    assert!(rebuild_required.status.success());
    let rebuild_required = parse_single_json(&rebuild_required.stdout);
    assert_eq!(rebuild_required["availability"], "rebuild_required");
    assert_eq!(rebuild_required["namespace"], "stable");
    assert!(
        !store.join("derived.rebuild.lock").exists(),
        "status must not acquire or create the rebuild lock"
    );
}

#[test]
fn derived_build_and_rebuild_are_synchronous_and_publish_one_document() {
    let repo = populated_repo();
    let repo_arg = repo.path().to_str().unwrap();
    let store = common_dir_store(repo.path());

    let built = pointbreak_env(["store", "derived", "build", "--repo", repo_arg], ACTIVE);
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let built_json = parse_single_json(&built.stdout);
    assert_eq!(built_json["schema"], "pointbreak.store-derived-build");
    assert_eq!(built_json["version"], 1);
    assert_eq!(built_json["availability"], "current");
    assert_eq!(built_json["transition"], "not_needed");
    assert_eq!(built_json["rebuilt"], true);
    let first_generation = built_json["generationId"].as_str().unwrap().to_owned();
    assert!(store.join("derived").is_dir());
    assert_progress_is_stderr_only(&built);

    let no_op = pointbreak_env(["store", "derived", "build", "--repo", repo_arg], ACTIVE);
    assert!(no_op.status.success());
    let no_op_json = parse_single_json(&no_op.stdout);
    assert_eq!(no_op_json["generationId"], first_generation);
    assert_eq!(no_op_json["rebuilt"], false);

    let current_status = pointbreak_env(
        [
            "store", "derived", "status", "--repo", repo_arg, "--format", "text",
        ],
        ACTIVE,
    );
    assert!(current_status.status.success());
    let current_status = String::from_utf8(current_status.stdout).unwrap();
    assert!(
        current_status.contains("availability: Current"),
        "{current_status}"
    );
    assert!(
        current_status.contains("namespace: Stable"),
        "{current_status}"
    );

    let rebuilt = pointbreak_env(["store", "derived", "rebuild", "--repo", repo_arg], ACTIVE);
    assert!(
        rebuilt.status.success(),
        "{}",
        String::from_utf8_lossy(&rebuilt.stderr)
    );
    let rebuilt_json = parse_single_json(&rebuilt.stdout);
    assert_eq!(rebuilt_json["schema"], "pointbreak.store-derived-rebuild");
    assert_eq!(rebuilt_json["availability"], "current");
    assert_eq!(rebuilt_json["rebuilt"], true);
    assert_ne!(rebuilt_json["generationId"], first_generation);
    assert_progress_is_stderr_only(&rebuilt);
}

#[test]
fn compatible_legacy_generation_transitions_before_build_without_replay() {
    let repo = populated_repo();
    let repo_arg = repo.path().to_str().unwrap();
    let store = common_dir_store(repo.path());
    let first = pointbreak_env(["store", "derived", "build", "--repo", repo_arg], ACTIVE);
    assert!(first.status.success());
    let generation = parse_json(&first.stdout)["generationId"]
        .as_str()
        .unwrap()
        .to_owned();
    fs::rename(store.join("derived"), store.join(".pointbreak-derived")).unwrap();

    let transitioned = pointbreak_env(["store", "derived", "build", "--repo", repo_arg], ACTIVE);
    assert!(
        transitioned.status.success(),
        "{}",
        String::from_utf8_lossy(&transitioned.stderr)
    );
    let json = parse_single_json(&transitioned.stdout);
    assert_eq!(json["transition"], "moved");
    assert_eq!(json["generationId"], generation);
    assert_eq!(json["rebuilt"], false);
    assert!(store.join("derived").is_dir());
    assert!(!store.join(".pointbreak-derived").exists());
}

#[test]
fn conflict_status_names_both_local_roots_without_mutating_them() {
    let repo = GitRepo::new();
    let repo_arg = repo.path().to_str().unwrap();
    let store = common_dir_store(repo.path());
    let stable = store.join("derived");
    let legacy = store.join(".pointbreak-derived");
    fs::create_dir_all(&stable).unwrap();
    fs::create_dir_all(&legacy).unwrap();
    fs::write(stable.join("stable-only"), b"stable").unwrap();
    fs::write(legacy.join("legacy-only"), b"legacy").unwrap();

    let json_output = pointbreak_env(["store", "derived", "status", "--repo", repo_arg], ACTIVE);
    assert!(json_output.status.success());
    let json = parse_single_json(&json_output.stdout);
    assert_eq!(json["active"], true);
    assert_eq!(json["availability"], "unavailable");
    assert_eq!(json["namespace"], "conflict");
    assert!(!String::from_utf8_lossy(&json_output.stdout).contains(repo_arg));

    let text = pointbreak_env(
        [
            "store", "derived", "status", "--repo", repo_arg, "--format", "text",
        ],
        ACTIVE,
    );
    assert!(text.status.success());
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains(stable.to_str().unwrap()), "{text}");
    assert!(text.contains(legacy.to_str().unwrap()), "{text}");
    assert_eq!(fs::read(stable.join("stable-only")).unwrap(), b"stable");
    assert_eq!(fs::read(legacy.join("legacy-only")).unwrap(), b"legacy");

    for command in ["build", "rebuild"] {
        let refused = pointbreak_env(["store", "derived", command, "--repo", repo_arg], ACTIVE);
        assert!(!refused.status.success());
        let receipt = parse_single_json(&refused.stdout);
        assert_eq!(receipt["transition"], "conflict");
        assert_eq!(receipt["rebuilt"], false);
        assert!(String::from_utf8_lossy(&refused.stderr).contains("Conflict"));
    }
    assert_eq!(fs::read(stable.join("stable-only")).unwrap(), b"stable");
    assert_eq!(fs::read(legacy.join("legacy-only")).unwrap(), b"legacy");
}

#[test]
fn deferred_transition_returns_a_receipt_and_a_failure_exit() {
    let repo = populated_repo();
    let repo_arg = repo.path().to_str().unwrap();
    let store = common_dir_store(repo.path());
    let built = pointbreak_env(["store", "derived", "build", "--repo", repo_arg], ACTIVE);
    assert!(built.status.success());
    let generation = parse_single_json(&built.stdout)["generationId"]
        .as_str()
        .unwrap()
        .to_owned();
    fs::rename(store.join("derived"), store.join(".pointbreak-derived")).unwrap();
    let lease_path = store.join(format!(
        ".pointbreak-derived.generation-lease-{generation}.lock"
    ));
    let lease = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lease_path)
        .unwrap();
    lease.lock_shared().unwrap();

    let deferred = pointbreak_env(["store", "derived", "build", "--repo", repo_arg], ACTIVE);

    assert!(!deferred.status.success());
    let receipt = parse_single_json(&deferred.stdout);
    assert_eq!(receipt["transition"], "deferred");
    assert_eq!(receipt["rebuilt"], false);
    assert!(String::from_utf8_lossy(&deferred.stderr).contains("Deferred"));
    assert!(store.join(".pointbreak-derived").is_dir());
    assert!(!store.join("derived").exists());
}

#[test]
fn explicit_off_and_busy_rebuild_fail_without_a_completion_document() {
    let repo = populated_repo();
    let repo_arg = repo.path().to_str().unwrap();
    let store = common_dir_store(repo.path());

    let off = pointbreak_env(
        ["store", "derived", "build", "--repo", repo_arg],
        &[("POINTBREAK_DERIVED_ACCESS", "off")],
    );
    assert!(!off.status.success());
    assert!(off.stdout.is_empty());
    assert!(String::from_utf8_lossy(&off.stderr).contains("disabled"));
    assert!(!store.join("derived").exists());

    fs::create_dir_all(&store).unwrap();
    let rebuild_lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(store.join("derived.rebuild.lock"))
        .unwrap();
    rebuild_lock.lock().unwrap();
    let busy = pointbreak_env(["store", "derived", "build", "--repo", repo_arg], ACTIVE);

    assert!(!busy.status.success());
    assert!(busy.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&busy.stderr).contains("already running"),
        "{}",
        String::from_utf8_lossy(&busy.stderr)
    );
    assert!(!store.join("derived").exists());
}

#[test]
fn active_first_write_falls_back_to_truth_with_one_actionable_hint() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    let repo_arg = repo.path().to_str().unwrap();
    let store = common_dir_store(repo.path());

    let capture = pointbreak_env(["capture", "--repo", repo_arg], ACTIVE);

    assert!(
        capture.status.success(),
        "{}",
        String::from_utf8_lossy(&capture.stderr)
    );
    let document = parse_single_json(&capture.stdout);
    assert_eq!(document["schema"], "pointbreak.review-capture");
    let stderr = String::from_utf8_lossy(&capture.stderr);
    assert_eq!(
        stderr
            .matches("derived acceleration is unavailable")
            .count(),
        1
    );
    assert!(stderr.contains("store derived status"), "{stderr}");
    assert!(stderr.contains("store derived build"), "{stderr}");
    assert!(
        !store.join("derived").exists(),
        "ordinary writes never bootstrap disposable history"
    );
    assert!(
        store.join("events").read_dir().unwrap().next().is_some(),
        "authoritative event bytes were published"
    );
}

#[test]
fn namespace_conflict_preserves_authoritative_capture_and_both_roots() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    let repo_arg = repo.path().to_str().unwrap();
    let store = common_dir_store(repo.path());
    let stable = store.join("derived");
    let legacy = store.join(".pointbreak-derived");
    fs::create_dir_all(&stable).unwrap();
    fs::create_dir_all(&legacy).unwrap();
    fs::write(stable.join("stable-only"), b"stable").unwrap();
    fs::write(legacy.join("legacy-only"), b"legacy").unwrap();

    let capture = pointbreak_env(["capture", "--repo", repo_arg], ACTIVE);

    assert!(
        capture.status.success(),
        "{}",
        String::from_utf8_lossy(&capture.stderr)
    );
    assert_eq!(
        parse_single_json(&capture.stdout)["schema"],
        "pointbreak.review-capture"
    );
    assert!(
        String::from_utf8_lossy(&capture.stderr).contains("derived acceleration is unavailable")
    );
    assert_eq!(fs::read(stable.join("stable-only")).unwrap(), b"stable");
    assert_eq!(fs::read(legacy.join("legacy-only")).unwrap(), b"legacy");
}

#[test]
fn busy_governed_writer_degrades_without_blocking_or_bootstrapping() {
    let repo = populated_repo();
    let repo_arg = repo.path().to_str().unwrap();
    let store = common_dir_store(repo.path());
    let built = pointbreak_env(["store", "derived", "build", "--repo", repo_arg], ACTIVE);
    assert!(built.status.success());
    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    let writer_lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(store.join("derived.writer.lock"))
        .unwrap();
    writer_lock.lock().unwrap();

    let capture = pointbreak_env(["capture", "--repo", repo_arg], ACTIVE);

    assert!(
        capture.status.success(),
        "{}",
        String::from_utf8_lossy(&capture.stderr)
    );
    assert_eq!(
        parse_single_json(&capture.stdout)["schema"],
        "pointbreak.review-capture"
    );
    let stderr = String::from_utf8_lossy(&capture.stderr);
    assert_eq!(
        stderr
            .matches("derived acceleration is unavailable")
            .count(),
        1
    );
    assert!(
        stderr
            .lines()
            .find(|line| line.starts_with("advisory:"))
            .is_some_and(|line| line.contains("derived-access writer is busy")),
        "the operator advisory should retain the specific cause: {stderr}"
    );
    assert!(stderr.contains("store derived status"), "{stderr}");
    assert!(store.join("derived").is_dir());
}

#[test]
fn explicit_off_write_creates_no_derived_artifact_or_hint() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    let repo_arg = repo.path().to_str().unwrap();
    let store = common_dir_store(repo.path());

    let capture = pointbreak_env(
        ["capture", "--repo", repo_arg],
        &[("POINTBREAK_DERIVED_ACCESS", "off")],
    );

    assert!(capture.status.success());
    assert_eq!(
        parse_single_json(&capture.stdout)["schema"],
        "pointbreak.review-capture"
    );
    assert!(!String::from_utf8_lossy(&capture.stderr).contains("derived acceleration"));
    for path in [
        "derived",
        "derived.writer.lock",
        "derived.rebuild.lock",
        ".pointbreak-derived",
    ] {
        assert!(!store.join(path).exists(), "explicit off created {path}");
    }
}

fn populated_repo() -> GitRepo {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    let capture = pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);
    assert!(capture.status.success());
    repo
}

fn parse_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or_else(|error| {
        panic!("expected JSON: {error}: {}", String::from_utf8_lossy(bytes))
    })
}

fn parse_single_json(bytes: &[u8]) -> Value {
    let text = String::from_utf8_lossy(bytes);
    assert_eq!(
        text.lines().count(),
        1,
        "stdout must contain one document: {text}"
    );
    parse_json(bytes)
}

fn assert_progress_is_stderr_only(output: &std::process::Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("derived"),
        "expected progress on stderr: {stderr}"
    );
    assert!(
        stderr.contains("events"),
        "expected event progress on stderr: {stderr}"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("progress"));
}
