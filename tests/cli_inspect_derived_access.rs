mod support;

use std::time::{Duration, Instant};

use support::common_dir_store;
use support::git_repo::GitRepo;
use support::inspect::{Inspector, capture};

#[test]
fn active_inspector_first_start_bootstraps_and_serves_history() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    capture(repo.path());

    let derived_root = common_dir_store(repo.path()).join(".pointbreak-derived");
    assert!(!derived_root.exists(), "fixture starts without a sidecar");

    let inspector = Inspector::spawn_authenticated_with_env(
        repo.path(),
        &[("POINTBREAK_DERIVED_ACCESS", "sqlite-wal-bodyless-v1")],
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let history = loop {
        let (status, body) = inspector.raw_get("/api/history");
        if status.contains("200 OK") {
            break serde_json::from_str::<serde_json::Value>(&body).expect("history JSON");
        }
        assert!(
            status.contains("503 Service Unavailable"),
            "active first start returned {status}: {body}"
        );
        let availability_body =
            serde_json::from_str::<serde_json::Value>(&body).expect("availability JSON");
        let availability = availability_body["availability"]
            .as_str()
            .expect("availability");
        assert!(
            matches!(availability, "absent" | "bootstrapping"),
            "unexpected first-start state: {body}"
        );
        assert!(
            Instant::now() < deadline,
            "active first start never published: {body}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

    assert_eq!(history["schema"], "pointbreak.inspect-history");
    assert!(history["projectionStamp"].is_string());
    assert!(history.get("eventSetHash").is_none());
    assert!(
        derived_root.is_dir(),
        "first start created the private sidecar"
    );
}
