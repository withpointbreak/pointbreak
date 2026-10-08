#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

fn invoke_data(home: &Path, request: Value) -> Output {
    use std::io::Write as _;
    let mut child = Command::new(env!("CARGO_BIN_EXE_pointbreak"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", home)
        .env("POINTBREAK_HOME", home)
        .env("POINTBREAK_ACTOR_ID", "actor:consumer-test")
        .env("POINTBREAK_DERIVED_ACCESS", "sqlite-wal-bodyless-v1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("__owned-data-v1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(&serde_json::to_vec(&request).unwrap())
        .unwrap();
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(90);
    while child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "UNKNOWN live data child {}; preserve fixture and stop",
            child.id()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    println!(
        "data={} status={} stdout={} stderr={}",
        request["operation"],
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn data_request(repo: &Path, operation: &str) -> Value {
    serde_json::json!({"version":1, "repository":repo, "requestId":"1".repeat(64),
        "enrollmentActor":"actor:consumer-test", "operation":operation})
}
type TreeObjects = BTreeMap<PathBuf, (u64, u64, u32, Vec<u8>)>;
fn tree_bytes(root: &Path) -> TreeObjects {
    fn walk(root: &Path, path: &Path, out: &mut TreeObjects) {
        use std::os::unix::fs::MetadataExt as _;
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            assert!(!meta.file_type().is_symlink(), "unexpected fixture symlink");
            let bytes = if meta.is_dir() {
                Vec::new()
            } else {
                assert!(meta.is_file());
                std::fs::read(&path).unwrap()
            };
            out.insert(
                path.strip_prefix(root).unwrap().to_path_buf(),
                (meta.dev(), meta.ino(), meta.mode(), bytes),
            );
            if meta.is_dir() {
                walk(root, &path, out);
            }
        }
    }
    let mut result = BTreeMap::new();
    walk(root, root, &mut result);
    result
}
fn refused_without_effect(home: &Path, store: &Path, request: Value, message: &str) {
    let before = tree_bytes(store);
    let output = invoke_data(home, request);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{output:?}"
    );
    assert_eq!(
        tree_bytes(store),
        before,
        "refused read mutated retained fixture"
    );
}

#[test]
#[ignore = "reviewed source/caller and coordinator native readiness required; assigned empty retained parent"]
fn owned_data_fresh_rebuild_read_and_refusals() {
    let parent = PathBuf::from(
        std::env::var_os("POINTBREAK_CONSUMER_FIXTURE").expect("assigned fresh parent"),
    );
    assert!(parent.is_absolute() && parent.is_dir());
    assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 0);
    let repo = parent.join("repo");
    let home = parent.join("home");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&home).unwrap();
    assert!(
        Command::new("/usr/bin/git")
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", &home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(["init", "--quiet"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );
    std::fs::create_dir(home.join("keys")).unwrap();
    let key = pointbreak::keys::generate_key_in(
        &home.join("keys"),
        &pointbreak::keys::KeyName::parse("consumer").unwrap(),
    )
    .unwrap();
    let first_args = args(
        &repo,
        key.private_key_path(),
        "data-first",
        &"51".repeat(32),
    );
    let first = success(&invoke(
        &home,
        "sqlite-wal-bodyless-v1",
        &owned("enroll-empty", &first_args),
    ));
    let store = repo.join(".git/pointbreak");
    let read = data_request(&repo, "read_changes");
    refused_without_effect(&home, &store, read.clone(), "projection absent");
    assert!(!store.join("derived").exists());
    let mut invalid = data_request(&repo, "rebuild");
    invalid["limits"] = serde_json::json!({"visits":0,"bytes":1024,"milliseconds":1000});
    refused_without_effect(&home, &store, invalid, "invalid limits");
    assert!(!store.join("derived").exists());

    // Writer process has exited before a wholly new readonly process starts.
    let built = success(&invoke_data(&home, data_request(&repo, "rebuild")));
    let stable = tree_bytes(&store);
    let view = success(&invoke_data(&home, read.clone()));
    assert_eq!(
        tree_bytes(&store),
        stable,
        "readonly request mutated fixture"
    );
    assert_eq!(view["head"], 1);
    assert_eq!(
        view["minimumReaderProfile"],
        "review_change_revision_progress_v1"
    );
    assert_eq!(view["changes"]["changes"].as_array().unwrap().len(), 1);
    assert_eq!(view["changes"]["changes"][0]["changeId"], first["changeId"]);
    let generation = view["generationId"].as_str().unwrap();
    assert_eq!(built["publication"]["generationId"], generation);
    let generation_dir = store.join("derived/generations").join(generation);
    assert!(generation_dir.join("cursor.sqlite3-wal").is_file());
    assert!(generation_dir.join("cursor.sqlite3-shm").is_file());
    let lease_path = store.join(format!("derived.generation-lease-{generation}.lock"));
    let busy_lease = std::fs::File::open(&lease_path).unwrap();
    busy_lease.lock().unwrap();
    refused_without_effect(&home, &store, read.clone(), "still in use");
    busy_lease.unlock().unwrap();
    drop(busy_lease);

    // Neither absent companion nor missing generation lease may be repaired.
    for path in [
        generation_dir.join("cursor.sqlite3-wal"),
        generation_dir.join("cursor.sqlite3-shm"),
        store.join(format!("derived.generation-lease-{generation}.lock")),
    ] {
        let saved = path.with_extension("retained-missing-control");
        std::fs::rename(&path, &saved).unwrap();
        let before = tree_bytes(&store);
        let output = invoke_data(&home, read.clone());
        assert!(
            !output.status.success(),
            "missing witness unexpectedly accepted"
        );
        assert_eq!(
            output.status.code(),
            Some(2),
            "ordinary refusal must confirm native finish"
        );
        assert!(!path.exists(), "reader created missing witness");
        assert_eq!(tree_bytes(&store), before);
        std::fs::rename(saved, path).unwrap();
    }
    // A legacy layout is an explicit refusal, with no migration or fallback.
    let stable_layout = store.join("derived");
    let legacy_layout = store.join(".pointbreak-derived");
    std::fs::rename(&stable_layout, &legacy_layout).unwrap();
    refused_without_effect(&home, &store, read.clone(), "legacy");
    refused_without_effect(&home, &store, data_request(&repo, "rebuild"), "legacy");
    std::fs::rename(&legacy_layout, &stable_layout).unwrap();

    let second_args = args(
        &repo,
        key.private_key_path(),
        "data-second",
        &"52".repeat(32),
    );
    success(&invoke(
        &home,
        "sqlite-wal-bodyless-v1",
        &owned("resume", &second_args),
    ));
    refused_without_effect(&home, &store, read.clone(), "stale");
    success(&invoke_data(&home, data_request(&repo, "rebuild")));
    let view2 = success(&invoke_data(&home, read.clone()));
    assert_eq!(view2["head"], 2);
    assert_ne!(view2["generationId"], view["generationId"]);
    assert_eq!(view2["changes"]["changes"].as_array().unwrap().len(), 2);
    assert!(
        generation_dir.is_dir(),
        "prior generation must remain retained"
    );

    let pending = args(
        &repo,
        key.private_key_path(),
        "data-pending",
        &"53".repeat(32),
    );
    let cut = invoke(
        &home,
        "sqlite-wal-bodyless-v1",
        &owned("resume-cut-after-presence", &pending),
    );
    assert_eq!(cut.status.code(), Some(79));
    refused_without_effect(&home, &store, read, "pending");
    refused_without_effect(&home, &store, data_request(&repo, "rebuild"), "pending");
}

fn invoke(home: &Path, profile: &str, args: &[String]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pointbreak"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", home)
        .env("POINTBREAK_HOME", home)
        .env("POINTBREAK_ACTOR_ID", "actor:consumer-test")
        .env("POINTBREAK_DERIVED_ACCESS", profile)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    while child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "UNKNOWN live native child {}; preserve fixture and stop",
            child.id()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    println!(
        "args={args:?} status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn args(repo: &Path, key: &Path, operation: &str, nonce: &str) -> Vec<String> {
    vec![
        "change".into(),
        "create".into(),
        "--repo".into(),
        repo.display().to_string(),
        "--operation-id".into(),
        format!("change-operation:{operation}"),
        "--nonce".into(),
        nonce.into(),
        "--sign-key".into(),
        key.display().to_string(),
        "--format".into(),
        "json".into(),
    ]
}
fn owned(mode: &str, args: &[String]) -> Vec<String> {
    [
        vec!["__owned-change-v1".into(), "1".repeat(64), mode.into()],
        args.to_vec(),
    ]
    .concat()
}
fn success(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn ack(output: &Output) -> Value {
    let text = String::from_utf8_lossy(&output.stderr);
    serde_json::from_str(
        text.lines()
            .find_map(|line| line.strip_prefix("private-change-ack: "))
            .expect("actual consumer acknowledgement"),
    )
    .unwrap()
}
fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            assert!(path.is_file());
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
}

#[test]
#[ignore = "explicit macOS consumer canary; assigned retained disposable parent and reviewed native caller required"]
fn owned_change_create_real_command_roundtrip() {
    let parent = PathBuf::from(
        std::env::var_os("POINTBREAK_CONSUMER_FIXTURE").expect("assigned fresh parent"),
    );
    assert!(parent.is_absolute() && parent.is_dir());
    assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 0);
    for (profile, expected_derived) in [("off", "off"), ("sqlite-wal-bodyless-v1", "not_observed")]
    {
        let base = parent.join(profile);
        let repo = base.join("repo");
        let home = base.join("home");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir(&home).unwrap();
        assert!(
            Command::new("/usr/bin/git")
                .env_clear()
                .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                .env("HOME", &home)
                .args(["init", "--quiet"])
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .current_dir(&repo)
                .status()
                .unwrap()
                .success()
        );
        std::fs::create_dir(home.join("keys")).unwrap();
        let key = pointbreak::keys::generate_key_in(
            &home.join("keys"),
            &pointbreak::keys::KeyName::parse("consumer").unwrap(),
        )
        .unwrap();
        let first_args = args(&repo, key.private_key_path(), "first", &"41".repeat(32));
        // RED: the ordinary real command cannot silently enroll an empty store.
        let red = invoke(&home, profile, &first_args);
        assert!(!red.status.success());
        assert!(String::from_utf8_lossy(&red.stderr).contains("migration_required"));
        assert!(!repo.join(".git/pointbreak/append-progress").exists());
        // Missing selection cannot fall back to an empty legacy writer.
        let absent = invoke(&home, profile, &owned("resume", &first_args));
        assert!(!absent.status.success());
        let first = invoke(&home, profile, &owned("enroll-empty", &first_args));
        let receipt = success(&first);
        assert_eq!(receipt["events"][0]["outcome"], "created");
        assert_eq!(receipt["complete"], true);
        let acknowledgement = ack(&first);
        assert_eq!(acknowledgement["appendSettlement"], "confirmed");
        assert_eq!(acknowledgement["head"], 1);
        assert_eq!(acknowledgement["derived"]["availability"], expected_derived);
        assert!(acknowledgement["derived"].get("token").is_none());
        let store = repo.join(".git/pointbreak");
        let original_plan = files(&store.join("operations"));
        let original_events = files(&store.join("events"));
        assert!(!store.join("derived").exists());
        assert!(!store.join("state.json").exists());
        let plan: Value = serde_json::from_slice(original_plan.values().next().unwrap()).unwrap();
        assert!(plan["events"][0]["signature"].is_object());
        assert_eq!(original_events.len(), 3); // signed enrollment pair + one domain event
        let retry = invoke(&home, profile, &owned("resume", &first_args));
        let retried = success(&retry);
        assert_eq!(retried["changeId"], receipt["changeId"]);
        assert_eq!(
            retried["events"][0]["eventId"],
            receipt["events"][0]["eventId"]
        );
        assert_eq!(retried["events"][0]["outcome"], "existing");
        assert_eq!(ack(&retry)["attemptId"], acknowledgement["attemptId"]);
        assert_eq!(ack(&retry)["head"], 1);
        assert_eq!(files(&store.join("operations")), original_plan);
        assert_eq!(files(&store.join("events")), original_events);
        let missing_owner = invoke(&home, profile, &first_args);
        assert!(!missing_owner.status.success());
        assert!(
            String::from_utf8_lossy(&missing_owner.stderr).contains("progress_reader_unavailable")
        );
        for command in [vec!["change", "list"], vec!["store", "derived", "status"]] {
            let mut command: Vec<String> = command.into_iter().map(str::to_owned).collect();
            command.extend(["--repo".into(), repo.display().to_string()]);
            let refused = invoke(&home, profile, &command);
            assert!(!refused.status.success());
            assert!(
                String::from_utf8_lossy(&refused.stderr).contains("progress_reader_unavailable")
            );
        }
        // Keep the original signed plan and PROVED_PRESENT obligation after a
        // real process interruption, then settle it without another publication.
        let second_args = args(
            &repo,
            key.private_key_path(),
            "interrupted",
            &"42".repeat(32),
        );
        let cut = invoke(
            &home,
            profile,
            &owned("resume-cut-after-presence", &second_args),
        );
        assert_eq!(cut.status.code(), Some(79));
        let interrupted_plans = files(&store.join("operations"));
        let interrupted_events = files(&store.join("events"));
        assert_eq!(interrupted_events.len(), 4);
        let resumed = invoke(&home, profile, &owned("resume", &second_args));
        let resumed_receipt = success(&resumed);
        assert_eq!(resumed_receipt["events"][0]["outcome"], "existing");
        assert_eq!(ack(&resumed)["head"], 2);
        assert_eq!(ack(&resumed)["appendSettlement"], "confirmed");
        assert_eq!(files(&store.join("operations")), interrupted_plans);
        assert_eq!(files(&store.join("events")), interrupted_events);
        // Same operation with changed input is rejected before another append.
        let changed = args(
            &repo,
            key.private_key_path(),
            "interrupted",
            &"43".repeat(32),
        );
        let conflict = invoke(&home, profile, &owned("resume", &changed));
        assert!(!conflict.status.success());
        assert!(String::from_utf8_lossy(&conflict.stderr).contains("different Change inputs"));
        assert_eq!(files(&store.join("events")), interrupted_events);
    }
}
