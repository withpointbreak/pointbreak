#![cfg(all(
    feature = "experimental-enrolled-progress",
    any(target_os = "macos", windows)
))]
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use pointbreak::crypto::EventSigner;
use pointbreak::model::JournalId;
use pointbreak::session::event::{
    EventSignature, EventTarget, EventToBeSigned, EventType, ReviewInitializedPayload, ShoreEvent,
    Writer, event_signature_pre_authentication_encoding,
};
use serde_json::{Value, json};
fn invoke(mode: &str, input: &Value) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pointbreak"))
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    println!("C4 binary child pid={} mode={mode}", child.id());
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(input).unwrap())
        .unwrap();
    let until = Instant::now() + Duration::from_secs(180);
    while child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < until,
            "UNKNOWN live child {}; preserve fixture and stop dependent work",
            child.id()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    println!(
        "C4 binary terminal {} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
#[test]
#[ignore = "operator-assigned private native roots; actual binary ownership canary"]
fn private_owned_append_binary_canary() {
    let parent = PathBuf::from(
        std::env::var_os("POINTBREAK_C4_FIXTURE_PARENT").expect("assigned C4 parent"),
    );
    assert!(parent.is_absolute());
    let root = parent.join("binary-canary").canonicalize().unwrap();
    let keys = parent.join("keys").canonicalize().unwrap();
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(&keys).unwrap().count(), 0);
    let key = pointbreak::keys::generate_key_in(
        &keys,
        &pointbreak::keys::KeyName::parse("private-c4").unwrap(),
    )
    .unwrap();
    let signer = pointbreak::keys::load_signer_from_path(key.private_key_path()).unwrap();
    let request = "1".repeat(64);
    let writer = Writer::shore_local("test");
    let enrollment_actor = writer.actor_id.as_str().to_owned();
    let enrollment = json!({"version":1,"root":root,"requestId":request,"writer":writer,"occurredAt":"2026-10-01T00:00:00Z","signingKey":key.private_key_path(),"selectedOnly":false});
    assert!(
        invoke("__owned-enrollment-v1", &enrollment)
            .status
            .success()
    );
    let mut event = ShoreEvent::new(
        EventType::ReviewInitialized,
        "private-binary:c4",
        EventTarget::for_journal(JournalId::new("journal:c4")),
        writer,
        ReviewInitializedPayload {},
        "2026-10-01T00:00:00Z",
    )
    .unwrap();
    event.signer = Some(signer.signer_id().clone());
    let view = EventToBeSigned::from_event(&event, signer.signer_id()).unwrap();
    event.signature = Some(EventSignature::ed25519_v1(
        signer
            .sign_event_message(&event_signature_pre_authentication_encoding(&view).unwrap())
            .unwrap(),
    ));
    let input = json!({"version":1,"root":root,"requestId":request,"enrollmentActor":enrollment_actor,"operation":{"kind":"append","attemptId":"2".repeat(64),"event":event}});
    let result = invoke("__owned-append-v1", &input);
    assert!(result.status.success());
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(result["outcome"], "created");
    assert_eq!(result["finalized"], true);
    assert_eq!(result["head"], 1);
    let result = invoke("__owned-append-v1", &input);
    assert!(result.status.success());
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(result["outcome"], "existing");
    assert_eq!(result["head"], 1);
    let result = invoke(
        "__owned-append-v1",
        &json!({"version":1,"root":root,"requestId":request,"enrollmentActor":enrollment_actor,"operation":{"kind":"reconcile"}}),
    );
    assert!(result.status.success());
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(result["finalized"], true);
    assert_eq!(result["head"], 1);
    assert_eq!(std::fs::read_dir(root.join("events")).unwrap().count(), 3);
    // No process/environment bit turns the unimplemented shared transport on.
    assert_eq!(invoke("__owned-data-v1", &json!({})).status.code(), Some(2));
}
#[test]
#[ignore = "actual binary private-input refusal control"]
fn private_append_bad_input_has_no_filesystem_effect() {
    let parent = PathBuf::from(
        std::env::var_os("POINTBREAK_C4_FIXTURE_PARENT").expect("assigned C4 parent"),
    );
    let root = parent.join("uncreated-invalid-input");
    assert!(!root.exists());
    for operation in [
        json!({"kind":"reconcile","unexpected":true}),
        json!({"kind":"unknown"}),
    ] {
        let result = invoke(
            "__owned-append-v1",
            &json!({"version":1,"root":root,"requestId":"1".repeat(64),"enrollmentActor":"test","operation":operation}),
        );
        assert_eq!(result.status.code(), Some(2));
        assert!(!root.exists());
    }
    let result = invoke(
        "__owned-append-v1",
        &json!({"version":1,"root":root,"requestId":"../invalid","enrollmentActor":"test","operation":{"kind":"reconcile"}}),
    );
    assert_eq!(result.status.code(), Some(2));
    assert!(!root.exists());
}
