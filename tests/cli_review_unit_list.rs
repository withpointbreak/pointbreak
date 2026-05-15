mod support;

use serde_json::Value;
use support::git_repo::GitRepo;
use support::shore;

#[test]
fn review_unit_list_emits_v1_json_with_freshness_metadata() {
    let repo = modified_repo();
    let capture =
        parse_json(&shore(["review", "capture", "--repo", repo.path().to_str().unwrap()]).stdout);

    let output = shore([
        "review",
        "unit",
        "list",
        "--repo",
        repo.path().to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_json(&output.stdout);

    assert_eq!(json["schema"], "shore.review-unit-list");
    assert_eq!(json["version"], 1);
    assert!(
        json["eventSetHash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert_eq!(json["eventCount"], 1);
    assert_eq!(json["reviewUnitCount"], 1);

    let entry = &json["entries"][0];
    assert_eq!(entry["reviewUnitId"], capture["reviewUnit"]["id"]);
    assert!(!entry["capturedAt"].as_str().unwrap().is_empty());
    assert!(entry["revisionId"].as_str().unwrap().starts_with("rev:"));
    assert!(entry["snapshotId"].as_str().unwrap().starts_with("snap:"));
    assert!(entry["source"].is_object());
    assert!(entry["base"].is_object());
    assert!(entry["target"].is_object());
    assert!(
        entry["snapshotArtifactContentHash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
}

#[test]
fn review_unit_list_does_not_expose_storage_paths() {
    let repo = modified_repo();
    shore(["review", "capture", "--repo", repo.path().to_str().unwrap()]);

    let output = shore([
        "review",
        "unit",
        "list",
        "--repo",
        repo.path().to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = parse_json(&output.stdout);

    assert!(!stdout.contains(".shore/events"));
    assert!(!stdout.contains("artifacts/"));
    assert!(json.get("statePath").is_none());
    assert!(json["entries"][0].get("payloadHash").is_none());
    assert!(json["entries"][0].get("eventId").is_none());
}

#[test]
fn review_unit_list_pretty_prints() {
    let repo = modified_repo();
    shore(["review", "capture", "--repo", repo.path().to_str().unwrap()]);

    let output = shore([
        "review",
        "unit",
        "list",
        "--repo",
        repo.path().to_str().unwrap(),
        "--pretty",
    ]);

    assert!(String::from_utf8_lossy(&output.stdout).starts_with("{\n"));
}

#[test]
fn review_unit_list_returns_multiple_entries_in_capture_order() {
    let repo = modified_repo();
    let first =
        parse_json(&shore(["review", "capture", "--repo", repo.path().to_str().unwrap()]).stdout);
    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    let second =
        parse_json(&shore(["review", "capture", "--repo", repo.path().to_str().unwrap()]).stdout);

    let output = shore([
        "review",
        "unit",
        "list",
        "--repo",
        repo.path().to_str().unwrap(),
    ]);
    let json = parse_json(&output.stdout);
    let entries = json["entries"].as_array().unwrap();

    assert_ne!(first["reviewUnit"]["id"], second["reviewUnit"]["id"]);
    assert_eq!(json["reviewUnitCount"], 2);
    assert_eq!(entries.len(), 2);
    let ids: Vec<&str> = entries
        .iter()
        .map(|entry| entry["reviewUnitId"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&first["reviewUnit"]["id"].as_str().unwrap()));
    assert!(ids.contains(&second["reviewUnit"]["id"].as_str().unwrap()));
    assert!(
        entries[0]["capturedAt"].as_str().unwrap() <= entries[1]["capturedAt"].as_str().unwrap()
    );
}

#[test]
fn review_unit_list_succeeds_without_events() {
    let repo = GitRepo::new();

    let output = shore([
        "review",
        "unit",
        "list",
        "--repo",
        repo.path().to_str().unwrap(),
    ]);
    let json = parse_json(&output.stdout);

    assert!(output.status.success());
    assert_eq!(json["eventCount"], 0);
    assert_eq!(json["reviewUnitCount"], 0);
    assert!(json["entries"].as_array().unwrap().is_empty());
}

fn parse_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("parse CLI JSON")
}

fn modified_repo() -> GitRepo {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo
}
