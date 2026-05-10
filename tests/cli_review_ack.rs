mod support;

use support::{dump_repo, shore};

#[test]
fn ack_cli_records_acknowledgement_and_emits_v1_json() {
    let repo = dump_repo();
    let _ = shore(["review", "publish", "--repo", repo.path().to_str().unwrap()]);
    let verdict = shore([
        "review",
        "verdict",
        "--repo",
        repo.path().to_str().unwrap(),
        "--decision",
        "pass",
    ]);
    let verdict_json: serde_json::Value = serde_json::from_slice(&verdict.stdout).unwrap();
    let artifact_id = verdict_json["reviewArtifactId"].as_str().unwrap();

    let output = shore([
        "review",
        "ack",
        "--repo",
        repo.path().to_str().unwrap(),
        "--review-artifact",
        artifact_id,
        "--next-action",
        "accept",
    ]);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(json["schema"], "shore.review-ack");
    assert_eq!(json["version"], 1);
    assert!(
        json["acknowledgementId"]
            .as_str()
            .unwrap()
            .starts_with("ack:sha256:")
    );
    assert_eq!(json["eventsCreated"], 1);
}

#[test]
fn ack_cli_errors_when_artifact_unknown() {
    let repo = dump_repo();
    let _ = shore(["review", "publish", "--repo", repo.path().to_str().unwrap()]);

    let output = shore([
        "review",
        "ack",
        "--repo",
        repo.path().to_str().unwrap(),
        "--review-artifact",
        "review-artifact:sha256:nope",
        "--next-action",
        "accept",
    ]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .to_lowercase()
            .contains("unknown review artifact")
    );
}
