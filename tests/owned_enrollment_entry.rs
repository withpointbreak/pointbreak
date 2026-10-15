#![cfg(all(
    feature = "experimental-enrolled-progress",
    any(target_os = "macos", windows)
))]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

// The operator assigns and retains these fresh roots. This integration test
// exercises the real binary branch; the test process never acquires native SQL.
fn assigned(name: &str) -> std::path::PathBuf {
    let path = std::path::PathBuf::from(std::env::var_os(name).expect("assigned native fixture"));
    assert!(path.is_absolute());
    assert!(path.is_dir());
    path.canonicalize().unwrap()
}
fn invoke(mode: &str, input: &Value) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pointbreak"))
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("owned binary spawn");
    println!("owned diagnostic child {} mode={mode}", child.id());
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(input).unwrap())
        .unwrap();
    let until = Instant::now() + Duration::from_secs(180);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            Instant::now() < until,
            "UNKNOWN live child {}; preserve process and fixture, stop dependent native work",
            child.id()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    println!(
        "status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn snapshot(path: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(path: &Path, out: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for item in std::fs::read_dir(path).unwrap() {
            let item = item.unwrap();
            if item.file_type().unwrap().is_dir() {
                visit(&item.path(), out);
            } else {
                out.insert(item.path(), std::fs::read(item.path()).unwrap());
            }
        }
    }
    let mut out = Default::default();
    visit(path, &mut out);
    out
}
#[test]
#[ignore = "requires exact source/caller review, assigned retained root/key directories, and native readiness"]
fn owned_enrollment_binary_roundtrip() {
    let root = assigned("POINTBREAK_NATIVE_ENTRY_ROOT");
    let keys = assigned("POINTBREAK_NATIVE_ENTRY_KEYS");
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    let key = pointbreak::keys::generate_key_in(
        &keys,
        &pointbreak::keys::KeyName::parse("foundation").unwrap(),
    )
    .unwrap();
    let mut input = json!({"version":1,"root":root,"requestId":"1".repeat(64),
        "writer":pointbreak::session::event::Writer::shore_local("test"),
        "occurredAt":"2026-09-26T00:00:00Z","signingKey":key.private_key_path(),"selectedOnly":false});
    let enrolled = invoke("__owned-enrollment-v1", &input);
    assert!(enrolled.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&enrolled.stdout).unwrap()["selected"],
        true
    );
    let before = snapshot(&root);
    // Same original signed plan survives a complete repeated enrollment/resume.
    assert!(invoke("__owned-enrollment-v1", &input).status.success());
    let resumed = snapshot(&root);
    for (path, bytes) in &before {
        if path.extension().is_some_and(|v| v == "json") {
            assert_eq!(resumed.get(path), Some(bytes));
        }
    }
    input["selectedOnly"] = true.into();
    assert!(invoke("__owned-enrollment-v1", &input).status.success());
    assert_eq!(
        resumed,
        snapshot(&root),
        "Selected must not change persistent bytes"
    );
    assert_eq!(std::fs::read_dir(root.join("events")).unwrap().count(), 2);
}
#[test]
fn private_bootstrap_refuses_before_key_or_root_access() {
    let root = std::env::temp_dir().join("pointbreak-native-uncreated-bootstrap-20260926");
    let key = root.join("absent-key");
    assert!(!root.exists());
    assert!(!key.exists());
    let input = json!({"version":1,"root":root,"requestId":"../bad",
        "writer":pointbreak::session::event::Writer::shore_local("test"),
        "occurredAt":"2026-09-26T00:00:00Z","signingKey":key,"selectedOnly":false});
    let rejected = invoke("__owned-enrollment-v1", &input);
    assert_eq!(rejected.status.code(), Some(2));
    assert!(rejected.stdout.is_empty());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("invalid private bootstrap"));
    assert!(!root.exists());
    assert!(!key.exists());

    // macOS rejects this version before data work or repository/native acquisition.
    // Windows rejects the unsupported private entry before reading the request.
    let data_input = json!({"version":0,"repository":root,"requestId":"1".repeat(64),
        "enrollmentActor":"actor:consumer-test","operation":"identity"});
    let data_rejected = invoke("__owned-data-v1", &data_input);
    assert_eq!(data_rejected.status.code(), Some(2));
    assert!(data_rejected.stdout.is_empty());
    #[cfg(target_os = "macos")]
    assert!(
        String::from_utf8_lossy(&data_rejected.stderr)
            .contains("owned_process_required; invalid data request")
    );
    #[cfg(windows)]
    assert!(
        String::from_utf8_lossy(&data_rejected.stderr)
            .contains("owned_process_required; native data entry unsupported on this platform")
    );
    assert!(!root.exists());
    assert!(!key.exists());
}
