mod support;

use support::{dump_repo, shore};

#[test]
fn verdict_cli_records_verdict_and_emits_v1_json() {
    let repo = dump_repo();
    let publish = shore(["review", "publish", "--repo", repo.path().to_str().unwrap()]);
    assert!(publish.status.success());

    let output = shore([
        "review",
        "verdict",
        "--repo",
        repo.path().to_str().unwrap(),
        "--decision",
        "pass",
        "--summary",
        "looks good",
    ]);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(json["schema"], "shore.review-verdict");
    assert_eq!(json["version"], 1);
    assert!(
        json["reviewArtifactId"]
            .as_str()
            .unwrap()
            .starts_with("review-artifact:sha256:")
    );
    assert_eq!(json["eventsCreated"], 1);
    assert_eq!(json["eventsExisting"], 0);
}

#[test]
fn verdict_cli_summary_and_summary_file_are_mutually_exclusive() {
    let repo = dump_repo();
    let _ = shore(["review", "publish", "--repo", repo.path().to_str().unwrap()]);
    let path = repo.path().join("summary.txt");
    std::fs::write(&path, "via file").unwrap();

    let output = shore([
        "review",
        "verdict",
        "--repo",
        repo.path().to_str().unwrap(),
        "--decision",
        "pass",
        "--summary",
        "inline",
        "--summary-file",
        path.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
}

#[test]
fn verdict_cli_errors_clearly_when_no_current_revision() {
    let repo = dump_repo();

    let output = shore([
        "review",
        "verdict",
        "--repo",
        repo.path().to_str().unwrap(),
        "--decision",
        "pass",
    ]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .to_lowercase()
            .contains("no current revision")
    );
}

#[test]
fn verdict_cli_is_idempotent_on_re_run() {
    let repo = dump_repo();
    let _ = shore(["review", "publish", "--repo", repo.path().to_str().unwrap()]);
    let args = [
        "review",
        "verdict",
        "--repo",
        repo.path().to_str().unwrap(),
        "--decision",
        "pass",
        "--summary",
        "x",
    ];

    let first = shore(args);
    let second = shore(args);
    assert!(second.status.success());
    let first_json: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    let second_json: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(
        first_json["reviewArtifactId"],
        second_json["reviewArtifactId"]
    );
    assert_eq!(second_json["eventsCreated"], 0);
    assert_eq!(second_json["eventsExisting"], 1);
}
