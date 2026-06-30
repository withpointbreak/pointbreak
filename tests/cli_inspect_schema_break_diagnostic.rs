//! A stored event whose type/envelope was retired at a breaking change must not
//! blanket-500 the inspector. Every read endpoint skips it and returns 200 with
//! a `ProjectionDiagnostic`, so the page still renders; `/api/freshness`'s
//! event-file marker counts the skipped event's file, so the client auto-refresh
//! still fires when such an event lands.

mod support;

use serde_json::Value;
use support::common_dir_store;
use support::git_repo::GitRepo;
use support::inspect::{Inspector, capture, urlencode};

/// A repo with one captured Revision plus a raw retired-type event file dropped
/// into the resolved store, returning the captured Revision id for `/api/revision`.
fn store_with_retired_event() -> (GitRepo, String) {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");

    let revision_id = capture(repo.path());

    let events_dir = common_dir_store(repo.path()).join("events");
    std::fs::create_dir_all(&events_dir).unwrap();
    std::fs::write(
        events_dir.join(format!("{}.json", "a".repeat(64))),
        br#"{"eventType":"review_disposition_recorded"}"#,
    )
    .unwrap();

    (repo, revision_id)
}

fn has_schema_break_diagnostic(body: &Value) -> bool {
    body["diagnostics"]
        .as_array()
        .expect("diagnostics is an array")
        .iter()
        .any(|d| d["code"] == "unsupported_event_type")
}

#[test]
fn inspector_endpoints_return_200_with_schema_break_diagnostic() {
    let (repo, revision_id) = store_with_retired_event();
    let inspector = Inspector::spawn(repo.path());

    // `get_json` asserts a 200; a retired event that previously 500'd now renders.
    for path in ["/api/history", "/api/revisions", "/api/objects"] {
        let body = inspector.get_json(path);
        assert!(
            has_schema_break_diagnostic(&body),
            "{path} missing the schema break diagnostic: {body}"
        );
    }

    let revision = inspector.get_json(&format!("/api/revision?id={}", urlencode(&revision_id)));
    assert!(
        has_schema_break_diagnostic(&revision),
        "/api/revision missing the schema break diagnostic: {revision}"
    );

    // The cheap marker is the event-FILE count, so it counts the retired event's
    // file even though the lenient read drops it from history's post-skip count.
    // That bump is what fires the client auto-refresh when such an event lands.
    let history = inspector.get_json("/api/history");
    let freshness = inspector.get_json("/api/freshness");
    let marker = freshness["eventCount"].as_u64().expect("eventCount");
    let post_skip = history["eventCount"].as_u64().expect("history eventCount");
    assert!(
        marker > post_skip,
        "the marker counts the skipped event's file (marker {marker} > post-skip {post_skip}): {freshness}"
    );
}
