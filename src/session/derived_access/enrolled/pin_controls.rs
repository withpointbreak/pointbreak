//! Ignored diagnostic: one fresh fixture and one owner process per mutation.
use super::*;
use crate::session::append_progress::{
    AuditLimits, enter_owned_data_command, require_quiescent_data, with_owned_inspector_request,
};

#[test]
#[ignore = "exact caller review and native admission; fresh assigned pin fixture only"]
fn finished_request_refuses_between_phase_mutation() {
    let parent = std::path::PathBuf::from(std::env::var_os("POINTBREAK_CONSUMER_FIXTURE").unwrap());
    assert_eq!(std::fs::canonicalize(&parent).unwrap(), parent);
    assert_eq!(
        std::fs::read(parent.join("pin-control-owner")).unwrap(),
        b"owned-pin-control-v1\n"
    );
    let mode = std::env::var("POINTBREAK_PIN_CONTROL").unwrap();
    assert!(["cancel", "selection", "publication", "journal"].contains(&mode.as_str()));
    let repo = parent.join("repo");
    let store = repo.join(".git/pointbreak");
    let cancelled = Arc::new(AtomicBool::new(false));
    fn terminal(_: crate::session::OwnedProcessFailure) -> ! {
        // SAFETY: this explicitly admitted diagnostic owns this process.
        unsafe { libc::_exit(90) }
    }
    // SAFETY: isolated exact ignored test, serial data executor, no shared host.
    let _owner = unsafe { enter_owned_data_command(terminal) }.unwrap();
    let reached = std::cell::Cell::new(false);
    let second_query = std::cell::Cell::new(false);
    let result = with_owned_inspector_request(
        &_owner,
        &repo,
        &"1".repeat(64),
        "actor:consumer-test",
        AuditLimits::default(),
        cancelled.clone(),
        |reader| {
            reader.generation()?;
            let document = reader.status_document();
            assert_eq!(document["servingCurrent"], true);
            require_quiescent_data()?;
            reached.set(true);
            if mode == "cancel" {
                // A legitimate writer cannot advance the selected progress
                // while this request retains the clone admission lease.
                let product = std::path::PathBuf::from(
                    std::env::var_os("POINTBREAK_CONSUMER_PRODUCT").unwrap(),
                );
                assert_eq!(
                    crate::canonical_hash::sha256_bytes_hex(&std::fs::read(&product).unwrap()),
                    std::env::var("POINTBREAK_CONSUMER_PRODUCT_SHA256").unwrap()
                );
                let mut writer = std::process::Command::new(product)
                    .args([
                        "__owned-change-v1",
                        &"1".repeat(64),
                        "resume",
                        "change",
                        "create",
                        "--repo",
                    ])
                    .arg(&repo)
                    .args([
                        "--operation-id",
                        "change-operation:pin-exclusion",
                        "--nonce",
                        &"72".repeat(32),
                        "--sign-key",
                    ])
                    .arg(parent.join("home/keys/consumer"))
                    .args(["--format", "json"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::fs::File::create_new(parent.join("writer.stdout")).unwrap())
                    .stderr(std::fs::File::create_new(parent.join("writer.stderr")).unwrap())
                    .spawn()
                    .unwrap();
                std::fs::write(parent.join("writer.pid"), writer.id().to_string()).unwrap();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while writer.try_wait().unwrap().is_none() {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "writer outcome UNKNOWN; preserve process"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                assert!(!writer.wait().unwrap().success());
                assert!(
                    std::fs::read_to_string(parent.join("writer.stderr"))
                        .unwrap()
                        .contains("clone_binding_busy")
                );
                assert_eq!(
                    reader.pin.context.observe()?,
                    reader.pin.fence,
                    "writer changed progress despite admission exclusion"
                );
                cancelled.store(true, Ordering::Release);
            } else {
                let path = match mode.as_str() {
                    "selection" => store.join("append-progress/active.json"),
                    "publication" => std::fs::read_dir(store.join("derived/publications"))
                        .unwrap()
                        .map(|entry| entry.unwrap().path())
                        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
                        .unwrap(),
                    "journal" => std::fs::read_dir(store.join("events"))
                        .unwrap()
                        .map(|entry| entry.unwrap().path())
                        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
                        .unwrap(),
                    _ => unreachable!(),
                };
                let bytes = std::fs::read(&path).unwrap();
                let permissions = std::fs::metadata(&path).unwrap().permissions();
                std::fs::rename(&path, parent.join("retained-original")).unwrap();
                use std::io::Write as _;
                let mut replacement = std::fs::File::create_new(&path).unwrap();
                replacement.set_permissions(permissions).unwrap();
                replacement.write_all(&bytes).unwrap();
                replacement.sync_all().unwrap();
            }
            // Alternate the explicit next-phase and final-only paths. Removing
            // either admission must let this actual currentness document escape.
            if mode == "selection" || mode == "journal" {
                reader.pin.phase(|_, _, _| {
                    second_query.set(true);
                    Ok(())
                })?;
            }
            Ok(document)
        },
    );
    assert!(reached.get(), "fixture never reached the mutation boundary");
    assert!(
        !second_query.get(),
        "changed pin admitted another SQL query"
    );
    let error = result.expect_err("currentness document escaped changed/cancelled pin");
    println!(
        "pin {mode}: reached real generation, native scopes finished, response refused: {error}"
    );
    std::fs::write(parent.join("pin-control-result.json"), serde_json::to_vec(&serde_json::json!({"mode":mode,"reached":true,"documentEscaped":false,"secondQueryEntered":false,"error":error.to_string()})).unwrap()).unwrap();
}
