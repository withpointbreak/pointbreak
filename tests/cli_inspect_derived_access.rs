mod support;

use std::time::{Duration, Instant};

use support::common_dir_store;
use support::git_repo::GitRepo;
use support::inspect::{Inspector, capture, urlencode};

fn assert_revision_page_parity(active: &serde_json::Value, authoritative: &serde_json::Value) {
    for field in [
        "schema",
        "eventCount",
        "revisionCount",
        "entries",
        "diagnostics",
    ] {
        assert_eq!(
            active[field], authoritative[field],
            "revision page field {field} diverged"
        );
    }
    assert!(active["projectionStamp"].is_string());
    assert!(active["eventSetHash"].is_null());
    assert!(authoritative["projectionStamp"].is_null());
    assert!(authoritative["eventSetHash"].is_string());
}

#[test]
fn active_inspector_first_start_bootstraps_and_serves_history() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    let revision_id = capture(repo.path());

    let derived_root = common_dir_store(repo.path()).join("derived");
    assert!(!derived_root.exists(), "fixture starts without a sidecar");
    let rebuild_lock_path = common_dir_store(repo.path()).join("derived.rebuild.lock");
    let rebuild_lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&rebuild_lock_path)
        .unwrap();
    rebuild_lock.lock().unwrap();

    let inspector = Inspector::spawn_authenticated_with_env(
        repo.path(),
        &[("POINTBREAK_DERIVED_ACCESS", "sqlite-wal-bodyless-v1")],
    );
    let (status_head, status_body) = inspector.raw_get("/api/derived-access/status");
    assert!(
        status_head.contains("200 OK"),
        "derived status must remain available during first bootstrap: {status_head}: {status_body}"
    );
    let status = serde_json::from_str::<serde_json::Value>(&status_body)
        .expect("derived-access status JSON");
    assert_eq!(status["schema"], "pointbreak.inspect-derived-access-status");
    assert_eq!(status["version"], 1);
    assert_eq!(status["active"], true);
    assert!(status["availability"].is_string());
    assert!(status["rebuildInFlight"].is_boolean());
    assert!(status["actions"].is_array());

    let authorization = format!("Bearer {}", inspector.token().expect("authenticated token"));
    let (cancel_head, cancel_body) = inspector.raw_request(
        "POST",
        "/api/derived-access/cancel",
        &[
            ("Host", inspector.canonical_host()),
            ("Authorization", authorization.as_str()),
        ],
    );
    assert!(
        cancel_head.contains("200 OK"),
        "{cancel_head}: {cancel_body}"
    );
    let cancelled =
        serde_json::from_str::<serde_json::Value>(&cancel_body).expect("cancel status JSON");
    assert_eq!(cancelled["servingCurrent"], false);
    assert_eq!(cancelled["rebuildInFlight"], false);
    assert_eq!(cancelled["rebuildPaused"], true);
    assert!(
        cancelled["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action == "retry")
    );
    assert!(
        !cancelled["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action == "cancel")
    );

    drop(rebuild_lock);
    std::thread::sleep(Duration::from_millis(150));
    let (_, still_cancelled_body) = inspector.raw_get("/api/derived-access/status");
    let still_cancelled = serde_json::from_str::<serde_json::Value>(&still_cancelled_body)
        .expect("latched cancel status JSON");
    assert_eq!(still_cancelled["rebuildInFlight"], false);
    assert_eq!(still_cancelled["rebuildPaused"], true);

    let (retry_head, retry_body) = inspector.raw_request(
        "POST",
        "/api/derived-access/retry",
        &[
            ("Host", inspector.canonical_host()),
            ("Authorization", authorization.as_str()),
        ],
    );
    assert!(retry_head.contains("200 OK"), "{retry_head}: {retry_body}");

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
            matches!(
                availability,
                "absent" | "bootstrapping" | "rebuild_required"
            ),
            "unexpected first-start state: {body}"
        );
        let (progress_head, progress_body) = inspector.raw_get("/api/derived-access/status");
        assert!(progress_head.contains("200 OK"), "{progress_head}");
        let progress = serde_json::from_str::<serde_json::Value>(&progress_body)
            .expect("derived progress JSON");
        assert_eq!(progress["active"], true);
        assert!(progress["availability"].is_string());
        if progress["availability"] == "bootstrapping" {
            assert!(progress["phase"].is_string());
            assert!(progress["completedEvents"].is_number());
            assert!(progress["totalEvents"].is_number());
            assert!(progress["elapsedMilliseconds"].is_number());
        }
        assert!(
            Instant::now() < deadline,
            "active first start never published: {body}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

    assert_eq!(history["schema"], "pointbreak.inspect-history");
    assert!(history["projectionStamp"].is_string());
    assert!(history.get("eventSetHash").is_none());
    let initial_projection_stamp = history["projectionStamp"].clone();
    assert!(
        derived_root.is_dir(),
        "first start created the private sidecar"
    );

    let (fallback_head, fallback_body) =
        inspector.raw_get("/api/history?limit=100&access=authoritative");
    assert!(fallback_head.contains("200 OK"), "{fallback_head}");
    assert!(
        fallback_head.contains("X-Pointbreak-Access-Source: authoritative-fallback"),
        "explicit fallback is visibly labeled: {fallback_head}"
    );
    let fallback =
        serde_json::from_str::<serde_json::Value>(&fallback_body).expect("fallback history JSON");
    assert!(fallback["eventSetHash"].is_string());
    assert!(fallback.get("projectionStamp").is_none());

    let (detail_head, detail_body) = inspector.raw_get(&format!(
        "/api/revisions/{revision_id}?access=authoritative"
    ));
    assert!(
        detail_head.contains("200 OK"),
        "{detail_head}: {detail_body}"
    );
    assert!(
        detail_head.contains("X-Pointbreak-Access-Source: authoritative-fallback"),
        "explicit detail fallback is visibly labeled: {detail_head}"
    );
    let detail =
        serde_json::from_str::<serde_json::Value>(&detail_body).expect("fallback detail JSON");
    assert_eq!(detail["revision"]["id"], revision_id);

    let (invalid_head, _) = inspector.raw_get("/api/history?access=surprise");
    assert!(invalid_head.contains("400 Bad Request"), "{invalid_head}");

    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    capture(repo.path());

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (status, body) = inspector.raw_get("/api/history");
        if status.contains("200 OK") {
            let history = serde_json::from_str::<serde_json::Value>(&body).expect("history JSON");
            assert_ne!(history["projectionStamp"], initial_projection_stamp);
            break;
        }
        assert!(
            status.contains("503 Service Unavailable"),
            "out-of-band append returned {status}: {body}"
        );
        assert!(
            Instant::now() < deadline,
            "same Inspector process never rebuilt after an out-of-band append: {body}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn active_and_authoritative_revision_routes_match_across_page_boundaries() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    let mut revision_ids = Vec::new();
    for value in 2..=4 {
        repo.write(
            "src/lib.rs",
            format!("pub fn value() -> u32 {{ {value} }}\n"),
        );
        revision_ids.push(capture(repo.path()));
    }

    let inspector = Inspector::spawn_authenticated_with_env(
        repo.path(),
        &[("POINTBREAK_DERIVED_ACCESS", "sqlite-wal-bodyless-v1")],
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let active_first = loop {
        let (status, body) = inspector.raw_get("/api/revisions?limit=1");
        if status.contains("200 OK") {
            break serde_json::from_str::<serde_json::Value>(&body)
                .expect("active first page JSON");
        }
        assert!(
            status.contains("503 Service Unavailable"),
            "active first page returned {status}: {body}"
        );
        assert!(
            Instant::now() < deadline,
            "active revision page never became available: {body}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let (fallback_head, fallback_body) =
        inspector.raw_get("/api/revisions?limit=1&access=authoritative");
    assert!(fallback_head.contains("200 OK"), "{fallback_head}");
    assert!(
        fallback_head.contains("X-Pointbreak-Access-Source: authoritative-fallback"),
        "explicit fallback is visibly labeled: {fallback_head}"
    );
    let authoritative_first = serde_json::from_str::<serde_json::Value>(&fallback_body)
        .expect("authoritative first page JSON");
    assert_revision_page_parity(&active_first, &authoritative_first);
    assert_eq!(
        active_first["entries"][0]["revisionId"],
        revision_ids.last().unwrap().as_str(),
        "page one starts with the newest capture"
    );

    let active_next = active_first["next"].as_str().expect("active continuation");
    let authoritative_next = authoritative_first["next"]
        .as_str()
        .expect("authoritative continuation");
    let active_second = inspector.get_json(&format!(
        "/api/revisions?limit=1&after={}",
        urlencode(active_next)
    ));
    let authoritative_second = inspector.get_json(&format!(
        "/api/revisions?limit=1&after={}&access=authoritative",
        urlencode(authoritative_next)
    ));
    assert_revision_page_parity(&active_second, &authoritative_second);
    assert_ne!(
        active_first["entries"][0]["revisionId"],
        active_second["entries"][0]["revisionId"]
    );
}
