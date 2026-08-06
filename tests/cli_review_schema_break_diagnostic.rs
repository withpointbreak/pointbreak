//! L0 stores fail at the migration fence before retired payloads are decoded.

mod support;

use support::git_repo::GitRepo;
use support::{common_dir_store, pointbreak, pointbreak_unprepared};

/// A repo with one captured Revision plus a raw retired-type event file dropped
/// into the resolved store. The probe rejects the raw file before full decode,
/// so it needs no valid signature or hash.
fn store_with_retired_event() -> GitRepo {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");

    let capture = pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);
    assert!(
        capture.status.success(),
        "capture failed:\n{}",
        String::from_utf8_lossy(&capture.stderr)
    );

    let events_dir = common_dir_store(repo.path()).join("events");
    for entry in std::fs::read_dir(&events_dir)
        .unwrap()
        .filter_map(Result::ok)
    {
        let Ok(value) = std::fs::read(entry.path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .ok_or(())
        else {
            continue;
        };
        if matches!(
            value["schema"].as_str(),
            Some("pointbreak.store-capability-activation" | "pointbreak.bulk-adoption-completion")
        ) {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
    std::fs::create_dir_all(&events_dir).unwrap();
    std::fs::write(
        events_dir.join(format!("{}.json", "a".repeat(64))),
        br#"{"schema":"shore.event","version":1,"eventType":"review_disposition_recorded"}"#,
    )
    .unwrap();

    repo
}

#[test]
fn review_history_refuses_l0_before_decoding_a_retired_event() {
    let repo = store_with_retired_event();

    let output = pointbreak_unprepared(["history", "--repo", repo.path().to_str().unwrap()]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("migration_required"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn review_revisions_refuses_l0_before_decoding_a_retired_event() {
    let repo = store_with_retired_event();

    let output =
        pointbreak_unprepared(["revision", "list", "--repo", repo.path().to_str().unwrap()]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("migration_required"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn review_show_refuses_l0_before_decoding_a_retired_event() {
    let repo = store_with_retired_event();

    let output =
        pointbreak_unprepared(["revision", "show", "--repo", repo.path().to_str().unwrap()]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("migration_required"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
