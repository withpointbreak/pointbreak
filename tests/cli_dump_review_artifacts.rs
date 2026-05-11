mod support;

use std::path::Path;
use std::process::Output;

use serde_json::Value;
use support::{dump_repo, shore};

#[test]
fn dump_omits_review_artifacts_when_no_shore_dir() {
    let repo = dump_repo();

    let output = shore(["dump", "--repo", repo.path().to_str().unwrap()]);
    let json = parse_json(&output);

    assert!(
        json.get("review_artifacts").is_none(),
        "expected no review_artifacts field; got: {json:#?}"
    );
}

#[test]
fn dump_emits_empty_review_artifacts_when_shore_dir_present_but_no_events() {
    let repo = dump_repo();
    std::fs::create_dir_all(repo.path().join(".shore/events")).unwrap();

    let output = shore(["dump", "--repo", repo.path().to_str().unwrap()]);
    let json = parse_json(&output);
    let section = &json["review_artifacts"];

    assert!(section.is_object(), "expected object, got: {section}");
    assert_eq!(section["verdicts"].as_array().unwrap().len(), 0);
    assert_eq!(section["acknowledgements"].as_array().unwrap().len(), 0);
    assert_eq!(section["current_verdict"]["status"], "none");
}

#[test]
fn dump_emits_review_artifacts_section_after_publish_verdict_ack() {
    let repo = dump_repo();
    let repo_arg = repo.path().to_str().unwrap();

    let publish = shore(["review", "publish", "--repo", repo_arg]);
    assert!(
        publish.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&publish.stderr)
    );

    let verdict = shore([
        "review",
        "verdict",
        "--repo",
        repo_arg,
        "--decision",
        "pass",
        "--summary",
        "ship it",
    ]);
    let verdict_json = parse_json(&verdict);
    let artifact_id = verdict_json["reviewArtifactId"].as_str().unwrap();

    let ack = shore([
        "review",
        "ack",
        "--repo",
        repo_arg,
        "--review-artifact",
        artifact_id,
        "--next-action",
        "accept",
        "--reason",
        "ok",
    ]);
    assert!(
        ack.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&ack.stderr)
    );

    let output = shore(["dump", "--repo", repo_arg]);
    let json = parse_json(&output);
    let section = &json["review_artifacts"];

    assert_eq!(section["verdicts"][0]["decision"], "pass");
    assert_eq!(section["verdicts"][0]["summary"], "ship it");
    assert_eq!(section["acknowledgements"][0]["next_action"], "accept");
    assert_eq!(section["acknowledgements"][0]["reason"], "ok");
    assert_eq!(section["current_verdict"]["status"], "resolved");
    assert_eq!(section["current_verdict"]["decision"], "pass");
    assert_eq!(section["summary"]["verdict_count"], 1);
    assert_eq!(section["summary"]["acknowledgement_count"], 1);
}

#[test]
fn dump_review_artifacts_current_verdict_reports_ambiguous() {
    let repo = dump_repo();
    publish_and_two_unreplaced_verdicts(repo.path());

    let output = shore(["dump", "--repo", repo.path().to_str().unwrap()]);
    let json = parse_json(&output);
    let current_verdict = &json["review_artifacts"]["current_verdict"];

    assert_eq!(current_verdict["status"], "ambiguous");
    assert!(current_verdict["decision"].is_null());
    assert_eq!(
        current_verdict["review_artifact_ids"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn dump_with_sidecar_input_still_emits_review_artifacts_section() {
    let repo = dump_repo();
    let repo_arg = repo.path().to_str().unwrap();

    let publish = shore(["review", "publish", "--repo", repo_arg]);
    assert!(
        publish.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&publish.stderr)
    );

    let verdict = shore([
        "review",
        "verdict",
        "--repo",
        repo_arg,
        "--decision",
        "pass",
        "--summary",
        "ship it",
    ]);
    assert!(
        verdict.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&verdict.stderr)
    );

    let sidecar = repo.path().join("review-notes.json");
    std::fs::write(&sidecar, native_review_notes_json()).unwrap();

    let output = shore([
        "dump",
        "--repo",
        repo_arg,
        "--review-notes",
        sidecar.to_str().unwrap(),
    ]);
    let json = parse_json(&output);

    assert!(
        json.get("review_artifacts").is_some(),
        "sidecar path must still emit review_artifacts; got: {json:#?}"
    );
}

fn publish_and_two_unreplaced_verdicts(repo: &Path) {
    let repo_arg = repo.to_str().unwrap();

    let publish = shore(["review", "publish", "--repo", repo_arg]);
    assert!(
        publish.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&publish.stderr)
    );

    let verdict_one = shore([
        "review",
        "verdict",
        "--repo",
        repo_arg,
        "--decision",
        "pass",
        "--summary",
        "first verdict",
    ]);
    assert!(
        verdict_one.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&verdict_one.stderr)
    );

    let verdict_two = shore([
        "review",
        "verdict",
        "--repo",
        repo_arg,
        "--decision",
        "request-changes",
        "--summary",
        "second verdict",
    ]);
    assert!(
        verdict_two.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&verdict_two.stderr)
    );
}

fn parse_json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn native_review_notes_json() -> &'static str {
    r#"{
  "schema": "shore.review-notes",
  "version": 1,
  "summary": "CLI review notes",
  "files": [
    {
      "path": "src/untracked.rs",
      "notes": [
        {
          "id": "note:untracked",
          "title": "Untracked note",
          "body": "Review this new file.",
          "target": {
            "side": "new",
            "startLine": 1,
            "endLine": 1
          },
          "author": "human reviewer",
          "source": "reviewer"
        }
      ]
    },
    {
      "path": "src/lib.rs",
      "notes": []
    }
  ]
}"#
}
