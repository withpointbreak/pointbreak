#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

#[path = "support/owned_inspector_controls.rs"]
mod inspector_controls;

fn invoke_data(home: &Path, request: Value) -> Output {
    invoke_private_json(home, "__owned-data-v1", request)
}
fn invoke_private_json(home: &Path, entry: &str, request: Value) -> Output {
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
        .arg(entry)
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

fn seed_inspector_revision(
    parent: &Path,
    repo: &Path,
    home: &Path,
    key: &Path,
    change: &Value,
) -> Value {
    use std::io::Write as _;

    use sha2::{Digest as _, Sha256};
    let sha256_bytes_hex = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    let library = PathBuf::from(
        std::env::var_os("POINTBREAK_CONSUMER_LIBTEST")
            .expect("reviewed exact library test executable"),
    );
    assert!(library.is_absolute() && library.is_file());
    assert_eq!(
        sha256_bytes_hex(&std::fs::read(&library).unwrap()),
        std::env::var("POINTBREAK_CONSUMER_LIBTEST_SHA256").unwrap()
    );
    std::fs::write(
        parent.join("fixture-owner"),
        b"owned-inspector-fixture-v1\n",
    )
    .unwrap();
    let request = serde_json::to_vec(
        &serde_json::json!({"version":1,"parent":parent,"changeId":change["changeId"],"key":key}),
    )
    .unwrap();
    let request_path = parent.join("seed-request.json");
    std::fs::File::create_new(&request_path)
        .unwrap()
        .write_all(&request)
        .unwrap();
    let mut child = Command::new(&library)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", home)
        .env("POINTBREAK_HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("POINTBREAK_INSPECTOR_SEED_REQUEST", &request_path)
        .env(
            "POINTBREAK_INSPECTOR_SEED_REQUEST_SHA256",
            sha256_bytes_hex(&request),
        )
        .args([
            "--exact",
            "session::append_progress::inspector_fixture::prepare_owned_inspector_seed",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdin(Stdio::null())
        .stdout(std::fs::File::create_new(parent.join("seed.stdout")).unwrap())
        .stderr(std::fs::File::create_new(parent.join("seed.stderr")).unwrap())
        .spawn()
        .unwrap();
    std::fs::write(parent.join("seed.pid"), child.id().to_string()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "UNKNOWN fixture helper; preserve PID and stop"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(child.wait().unwrap().success());
    assert!(
        std::fs::read_to_string(parent.join("seed.stdout"))
            .unwrap()
            .contains("1 passed; 0 failed")
    );
    let bytes = std::fs::read(parent.join("seed.json")).unwrap();
    assert!(
        bytes.len() < 262_144,
        "seed JSON is {} bytes; limit is strictly below 262144",
        bytes.len()
    );
    std::fs::write(parent.join("seed.sha256"), sha256_bytes_hex(&bytes)).unwrap();
    let seed: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(seed["version"], 1);
    assert_eq!(seed["changeId"], change["changeId"]);
    let store = repo.join(".git/pointbreak");
    for artifact in seed["artifacts"].as_array().unwrap() {
        let relative = artifact["path"].as_str().unwrap();
        let parts = relative.split('/').collect::<Vec<_>>();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "artifacts");
        assert!(matches!(parts[1], "objects" | "notes"));
        let stem = parts[2].strip_suffix(".json").unwrap();
        assert_eq!(stem.len(), 64);
        assert!(
            stem.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        let bytes = serde_json::from_value::<Vec<u8>>(artifact["bytes"].clone()).unwrap();
        assert_eq!(sha256_bytes_hex(&bytes), artifact["sha256"]);
        let directory = store.join("artifacts").join(parts[1]);
        std::fs::create_dir_all(&directory).unwrap();
        assert_eq!(std::fs::canonicalize(&directory).unwrap(), directory);
        std::fs::File::create_new(store.join(relative))
            .unwrap()
            .write_all(&bytes)
            .unwrap();
    }
    assert_eq!(seed["events"].as_array().unwrap().len(), 5);
    for (index, event) in seed["events"].as_array().unwrap().iter().enumerate() {
        let request = serde_json::json!({"version":1,"root":store,"requestId":"1".repeat(64),"enrollmentActor":"actor:consumer-test",
            "operation":{"kind":"append","attemptId":format!("{:064x}",0x100+index),"event":event}});
        std::fs::write(
            parent.join(format!("append-{index}.request.json")),
            serde_json::to_vec_pretty(&request).unwrap(),
        )
        .unwrap();
        let output = invoke_private_json(home, "__owned-append-v1", request);
        std::fs::write(
            parent.join(format!("append-{index}.stdout")),
            &output.stdout,
        )
        .unwrap();
        std::fs::write(
            parent.join(format!("append-{index}.stderr")),
            &output.stderr,
        )
        .unwrap();
        let receipt = success(&output);
        assert_eq!(receipt["finalized"], true);
        assert_eq!(receipt["head"], index + 2);
        assert!(
            receipt["prefixSha256"]
                .as_str()
                .is_some_and(|value| value.len() == 64)
        );
    }
    // Removal events are retained inputs for a separately admitted negative
    // control. Completed rebuild currently refuses ArtifactRemoved; never
    // poison the positive fixture before its exact-resource/browser checks.
    assert_eq!(seed["removalEvents"].as_array().unwrap().len(), 2);
    seed
}
fn data_request(repo: &Path, operation: &str) -> Value {
    serde_json::json!({"version":1, "repository":repo, "requestId":"1".repeat(64),
        "enrollmentActor":"actor:consumer-test", "operation":operation})
}
type TreeObjects = BTreeMap<PathBuf, (u64, u64, u32, Vec<u8>)>;
fn contains_string(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(text) => text == expected,
        Value::Array(values) => values.iter().any(|value| contains_string(value, expected)),
        Value::Object(values) => values
            .values()
            .any(|value| contains_string(value, expected)),
        _ => false,
    }
}
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

/// This control is compile-only until the exact child and caller receive a
/// separate native admission. It retains its fresh fixture and all frames.
#[test]
#[ignore = "exact composed review and coordinator native readiness required; assigned empty retained parent"]
fn owned_inspector_child_pins_routes_and_refuses_stale_generation() {
    use std::io::{BufRead as _, Read as _, Write as _};
    let parent = PathBuf::from(
        std::env::var_os("POINTBREAK_CONSUMER_FIXTURE").expect("assigned fresh parent"),
    );
    assert!(parent.is_absolute() && parent.is_dir());
    assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 0);
    let repo = parent.join("repo");
    let home = parent.join("home");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&home).unwrap();
    let git = |arguments: &[&str]| {
        let output = Command::new("/usr/bin/git")
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", &home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(arguments)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(output.status.success(), "fixture Git failed: {output:?}");
    };
    git(&["init", "--quiet"]);
    std::fs::write(
        repo.join("fixture.txt"),
        b"before: exact captured fixture\n",
    )
    .unwrap();
    git(&["add", "fixture.txt"]);
    git(&[
        "-c",
        "user.name=Inspector Fixture",
        "-c",
        "user.email=inspector@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "--allow-empty",
        "--quiet",
        "-m",
        "fixture",
    ]);
    std::fs::create_dir(home.join("keys")).unwrap();
    let key = pointbreak::keys::generate_key_in(
        &home.join("keys"),
        &pointbreak::keys::KeyName::parse("consumer").unwrap(),
    )
    .unwrap();
    let first = success(&invoke(
        &home,
        "sqlite-wal-bodyless-v1",
        &owned(
            "enroll-empty",
            &args(
                &repo,
                key.private_key_path(),
                "inspector-first",
                &"61".repeat(32),
            ),
        ),
    ));
    std::fs::write(repo.join("fixture.txt"), b"after: exact captured fixture\n").unwrap();
    let seed = seed_inspector_revision(&parent, &repo, &home, key.private_key_path(), &first);
    success(&invoke_data(&home, data_request(&repo, "rebuild")));
    let store = repo.join(".git/pointbreak");
    let stable = tree_bytes(&store);
    let mut child = Command::new(env!("CARGO_BIN_EXE_pointbreak"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", &home)
        .env("POINTBREAK_HOME", &home)
        .env("POINTBREAK_ACTOR_ID", "actor:consumer-test")
        .env("POINTBREAK_DERIVED_ACCESS", "sqlite-wal-bodyless-v1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("__inspect-data-v1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(parent.join("child.stderr.log")).unwrap())
        .spawn()
        .unwrap();
    println!("retained Inspector data child PID {}", child.id());
    std::fs::write(parent.join("child.pid"), child.id().to_string()).unwrap();
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::sync_channel(2);
    let raw_path = parent.join("child.frames.jsonl");
    let reader = std::thread::spawn(move || {
        let mut input = std::io::BufReader::new(output);
        let mut raw = std::fs::File::create(raw_path).unwrap();
        loop {
            let mut bytes = Vec::new();
            match input
                .by_ref()
                .take(32 * 1024 * 1024 + 1)
                .read_until(b'\n', &mut bytes)
            {
                Ok(0) => break,
                Ok(_) => {
                    raw.write_all(&bytes).unwrap();
                    raw.flush().unwrap();
                    if bytes.len() > 32 * 1024 * 1024 || bytes.last() != Some(&b'\n') {
                        let _ = send.send(Err("invalid child frame".to_owned()));
                        break;
                    }
                    if send
                        .send(serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string()))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    let _ = send.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    let read = || {
        receive
            .recv_timeout(Duration::from_secs(90))
            .expect("UNKNOWN Inspector response; preserve PID and fixture; no kill or replay")
            .unwrap()
    };
    let write = |input: &mut std::process::ChildStdin, frame: Value| {
        input
            .write_all(&serde_json::to_vec(&frame).unwrap())
            .unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
    };
    write(
        &mut input,
        serde_json::json!({"kind":"start","epoch":7,"repo":repo,
        "selection":{"requestId":"1".repeat(64),"enrollmentActor":"actor:consumer-test"}}),
    );
    let startup = read();
    assert_eq!(startup["epoch"], 7);
    assert_eq!(startup["operation"], 0);
    let mut operation = 0u64;
    let mut request = |method: &str, target: &str| {
        operation += 1;
        let request_id = format!("{operation:032x}");
        let data = serde_json::json!({"method":method,"target":target});
        #[cfg(feature = "longitudinal-counting")]
        let data = {
            let mut data = data;
            data["counting"] = serde_json::json!([]);
            data
        };
        write(
            &mut input,
            serde_json::json!({"kind":"request","epoch":7,"operation":operation,"request_id":request_id,"request":data}),
        );
        let reply = read();
        assert_eq!(reply["epoch"], 7);
        assert_eq!(reply["operation"], operation);
        assert_eq!(reply["request_id"], request_id);
        let bytes = serde_json::from_value::<Vec<u8>>(reply["response"]["body"].clone()).unwrap();
        let body = serde_json::from_slice::<Value>(&bytes).unwrap();
        (reply, body)
    };
    for path in [
        "/api/identity",
        "/api/v2/profile",
        "/api/v2/changes?limit=50",
        "/api/v2/attention?limit=50",
        "/api/v2/history?limit=10",
    ] {
        let (reply, body) = request("GET", path);
        assert_eq!(reply["response"]["status"], "200 OK", "{path}: {body}");
        if path == "/api/v2/profile" {
            assert_eq!(
                body["minimumReaderProfile"],
                "review_change_revision_progress_v1"
            );
            assert!(body["commitGraphStamp"].is_string());
        }
        if path.starts_with("/api/v2/changes") {
            assert_eq!(body["changes"][0]["changeId"], first["changeId"]);
        }
        println!("owned Inspector route passed: {path}");
    }
    let detail = format!("/api/v2/changes/{}", first["changeId"].as_str().unwrap());
    assert_eq!(request("GET", &detail).0["response"]["status"], "200 OK");
    let exact = format!(
        "{detail}/revisions/{}",
        seed["revisionId"].as_str().unwrap()
    );
    let selector = format!("artifactHash={}", seed["artifactHash"].as_str().unwrap());
    let (reply, revision) = request("GET", &format!("{exact}?{selector}"));
    assert_eq!(reply["response"]["status"], "200 OK");
    assert!(
        revision
            .to_string()
            .contains(seed["observationId"].as_str().unwrap())
    );
    assert!(contains_string(
        &revision,
        seed["observationBody"].as_str().unwrap()
    ));
    assert!(
        revision
            .to_string()
            .contains(seed["validationId"].as_str().unwrap())
    );
    let (reply, resource) = request("GET", &format!("{exact}/resource?{selector}"));
    assert_eq!(reply["response"]["status"], "200 OK");
    let typed: pointbreak::documents::RevisionResourceDocumentV1 =
        serde_json::from_value(resource.clone()).unwrap();
    typed.validate_integrity().unwrap();
    assert_eq!(resource["availability"], "available");
    for field in ["resource", "capturedDocument", "capturedDocumentHash"] {
        assert_eq!(
            resource[field], seed["expectedResource"][field],
            "actual resource differs at {field}"
        );
    }
    let (reply, timeline) = request("GET", "/api/v2/history?limit=50");
    assert_eq!(reply["response"]["status"], "200 OK");
    assert!(contains_string(
        &timeline,
        seed["events"][2]["eventId"].as_str().unwrap()
    ));
    assert!(contains_string(
        &timeline,
        seed["events"][4]["eventId"].as_str().unwrap()
    ));
    // Each enrolled substring qualifier must execute under the unchanged native
    // read authorizer and select the actual observation and validation carriers.
    for (field, value) in [
        ("revision", seed["revisionId"].as_str().unwrap()),
        ("change", first["changeId"].as_str().unwrap()),
        ("snapshot", seed["artifactHash"].as_str().unwrap()),
    ] {
        let suffix = &value[value.len() - 12..];
        let (reply, filtered) = request(
            "GET",
            &format!(
                "/api/v2/history?limit=50&q={field}%3A{}",
                suffix.to_uppercase()
            ),
        );
        assert_eq!(
            reply["response"]["status"], "200 OK",
            "{field} filter refused"
        );
        for index in [2, 4] {
            let event_id = seed["events"][index]["eventId"].as_str().unwrap();
            assert!(
                filtered["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| contains_string(entry, event_id)),
                "{field} lost selected carrier {event_id}"
            );
        }
        for literal in ["%25", "%5F", "%21", "%5C"] {
            let (reply, filtered) = request(
                "GET",
                &format!("/api/v2/history?limit=50&q={field}%3A{literal}"),
            );
            assert_eq!(reply["response"]["status"], "200 OK");
            assert_eq!(
                filtered["matchCount"], 0,
                "{field} treated {literal} as a wildcard"
            );
            assert!(filtered["entries"].as_array().unwrap().is_empty());
        }
    }
    assert_eq!(
        tree_bytes(&store),
        stable,
        "read routes mutated retained fixture"
    );
    success(&invoke(
        &home,
        "sqlite-wal-bodyless-v1",
        &owned(
            "resume",
            &args(
                &repo,
                key.private_key_path(),
                "inspector-second",
                &"62".repeat(32),
            ),
        ),
    ));
    let stale = tree_bytes(&store);
    let (reply, body) = request("GET", "/api/v2/changes?limit=50");
    assert_eq!(reply["response"]["status"], "503 Service Unavailable");
    assert_eq!(body["schema"], "pointbreak.inspect-change-projection-error");
    assert_eq!(
        tree_bytes(&store),
        stale,
        "stale response mutated retained fixture"
    );
    assert_eq!(
        request("POST", "/api/derived-access/retry").0["response"]["status"],
        "200 OK"
    );
    let (reply, body) = request("GET", "/api/v2/changes?limit=50");
    assert_eq!(reply["response"]["status"], "200 OK");
    assert_eq!(body["changes"].as_array().unwrap().len(), 2);
    // Actual protocol rejection: keep stdin open, deliver the wrong epoch,
    // and require child exit without a reply. EOF cannot explain this exit.
    let wrong_request = serde_json::json!({"method":"GET","target":"/api/v2/profile"});
    #[cfg(feature = "longitudinal-counting")]
    let wrong_request = {
        let mut value = wrong_request;
        value["counting"] = serde_json::json!([]);
        value
    };
    write(
        &mut input,
        serde_json::json!({"kind":"request","epoch":6,"operation":999,"request_id":"wrong-epoch","request":wrong_request}),
    );
    let deadline = Instant::now() + Duration::from_secs(90);
    while child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "UNKNOWN live Inspector child {}; preserve and stop",
            child.id()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(child.wait().unwrap().success());
    drop(input);
    reader.join().unwrap();
    assert!(receive.try_recv().is_err(), "wrong epoch received a reply");
    println!("owned child refused wrong epoch and exited before stdin EOF; fixture retained");
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
