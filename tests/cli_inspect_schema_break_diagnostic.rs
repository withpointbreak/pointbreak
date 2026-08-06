//! A stored event whose type/envelope was retired at a breaking change must not
//! blanket-500 the legacy Inspector routes. The capable profile still reports
//! L0 migration state, while each historical route skips the retired payload
//! and includes a diagnostic.

mod support;

use support::common_dir_store;
use support::git_repo::GitRepo;
use support::inspect::{Inspector, capture, urlencode};

fn has_schema_break_diagnostic(body: &serde_json::Value) -> bool {
    body["diagnostics"]
        .as_array()
        .expect("diagnostics is an array")
        .iter()
        .any(|diagnostic| diagnostic["code"] == "unsupported_event_type")
}

/// A repo with one captured Revision plus a raw retired-type event file dropped
/// into the resolved store, returning the captured Revision id for `/api/revisions/{id}`.
fn store_with_retired_event() -> (GitRepo, String) {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");

    let revision_id = capture(repo.path());

    let events_dir = common_dir_store(repo.path()).join("events");
    for entry in std::fs::read_dir(&events_dir)
        .unwrap()
        .filter_map(Result::ok)
    {
        let Some(schema) = std::fs::read(entry.path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| value["schema"].as_str().map(str::to_owned))
        else {
            continue;
        };
        if matches!(
            schema.as_str(),
            "pointbreak.store-capability-activation" | "pointbreak.bulk-adoption-completion"
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

    (repo, revision_id)
}

#[test]
fn inspector_legacy_endpoints_diagnose_a_retired_l0_event() {
    let (repo, revision_id) = store_with_retired_event();
    let inspector = Inspector::spawn_authenticated_with_env(
        repo.path(),
        &[("POINTBREAK_DERIVED_ACCESS", "off")],
    );

    let profile = inspector.get_json("/api/v2/profile");
    assert_eq!(profile["availability"], "migration_required");
    assert!(
        profile["authorityCursor"]["journalRecordCount"].as_u64()
            > profile["authorityCursor"]["eventCount"].as_u64(),
        "the typed cursor retains the retired raw record without treating it as a supported event: {profile}"
    );
    let (status, unavailable) = inspector.get_error("/api/v2/changes");
    assert!(status.contains("409"), "unexpected status: {status}");
    assert_eq!(unavailable["state"], "migration_required");

    for path in ["/api/history", "/api/revisions", "/api/threads"] {
        let body = inspector.get_json(path);
        assert!(
            has_schema_break_diagnostic(&body),
            "{path} missing the schema-break diagnostic: {body}"
        );
    }

    let revision = inspector.get_json(&format!("/api/revisions/{}", urlencode(&revision_id)));
    assert!(has_schema_break_diagnostic(&revision));

    let history = inspector.get_json("/api/history");
    let freshness = inspector.get_json("/api/freshness");
    assert!(
        freshness["eventCount"]
            .as_u64()
            .expect("freshness eventCount")
            > history["eventCount"].as_u64().expect("history eventCount")
    );
}
