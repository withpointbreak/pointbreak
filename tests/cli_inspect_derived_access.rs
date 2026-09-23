mod support;

use std::time::{Duration, Instant};

use support::git_repo::GitRepo;
use support::inspect::{Inspector, capture, legacy_reader_clone, urlencode};

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
fn legacy_reader_snapshot_excludes_transient_git_lock_files() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    capture(repo.path());

    let source_lock = repo.path().join(".git/index.lock");
    std::fs::write(&source_lock, b"transient fixture lock\n").unwrap();
    let (_legacy_root, legacy_repo) = legacy_reader_clone(repo.path());

    assert!(source_lock.exists(), "the source fixture remains untouched");
    assert!(
        !legacy_repo.join(".git/index.lock").exists(),
        "transient Git lock files are not repository snapshot state"
    );
}

#[test]
fn unset_inspector_first_start_builds_the_default_projection() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    capture(repo.path());

    let (_legacy_root, legacy_repo) = legacy_reader_clone(repo.path());
    let derived_root = legacy_repo.join(".pointbreak/data/derived");
    assert!(
        !derived_root.exists(),
        "writes do not synchronously bootstrap"
    );

    let inspector = Inspector::spawn_authenticated(&legacy_repo);
    let status = inspector.get_json("/api/derived-access/status");
    let history = inspector.get_json("/api/history");

    assert_eq!(status["active"], true);
    assert_eq!(status["availability"], "current");
    assert!(history["projectionStamp"].is_string());
    assert!(history.get("eventSetHash").is_none());
    assert!(
        derived_root.is_dir(),
        "first Inspector use built the sidecar"
    );
}

#[test]
fn active_inspector_first_start_bootstraps_and_serves_history() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    let revision_id = capture(repo.path());

    let (_legacy_root, legacy_repo) = legacy_reader_clone(repo.path());
    let derived_root = legacy_repo.join(".pointbreak/data/derived");
    assert!(!derived_root.exists(), "fixture starts without a sidecar");
    let rebuild_lock_path = legacy_repo.join(".pointbreak/data/derived.rebuild.lock");
    let rebuild_lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&rebuild_lock_path)
        .unwrap();
    rebuild_lock.lock().unwrap();

    let inspector = Inspector::spawn_authenticated_with_env(
        &legacy_repo,
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

#[test]
fn v2_entry_routes_serve_the_elected_authoritative_fallback_with_the_header() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    capture(repo.path());

    let inspector = Inspector::spawn_current_unready(repo.path());
    for path in [
        "/api/v2/profile",
        "/api/v2/changes",
        "/api/v2/history",
        "/api/v2/attention",
    ] {
        let (head, body) = inspector.raw_get(&format!("{path}?access=authoritative"));
        assert!(head.starts_with("HTTP/1.1 200"), "{path}: {head}\n{body}");
        assert!(
            head.contains("X-Pointbreak-Access-Source: authoritative-fallback"),
            "{path} must label the elected fallback: {head}"
        );
        let (derived_head, _) = inspector.raw_get(&format!("{path}?access=derived"));
        let (bare_head, _) = inspector.raw_get(path);
        assert_eq!(
            derived_head.lines().next(),
            bare_head.lines().next(),
            "{path}: access=derived is the default route"
        );
    }
    let (_, changes) = inspector.raw_get("/api/v2/changes?access=authoritative");
    let changes: serde_json::Value = serde_json::from_str(&changes).expect("changes page json");
    assert_eq!(changes["schema"], "pointbreak.inspect-changes-page");
    assert!(
        changes["changes"]
            .as_array()
            .is_some_and(|list| !list.is_empty()),
        "the elected page lists the captured Change: {changes}"
    );
}

/// Three independent Changes on a ready store with a current derived generation.
fn three_change_ready_repo() -> GitRepo {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 0 }\n");
    repo.commit_all("base");
    for value in 1..=3 {
        repo.write(
            "src/lib.rs",
            format!("pub fn value() -> u32 {{ {value} }}\n"),
        );
        capture(repo.path());
    }
    let build = support::pointbreak([
        "store",
        "derived",
        "build",
        "--repo",
        repo.path().to_str().unwrap(),
    ]);
    assert!(
        build.status.success(),
        "derived build stderr: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    repo
}

fn page(inspector: &Inspector, path: &str) -> serde_json::Value {
    let (head, body) = inspector.raw_get(path);
    assert!(head.starts_with("HTTP/1.1 200"), "{path}: {head}\n{body}");
    serde_json::from_str(&body).unwrap_or_else(|error| panic!("{path}: {error}: {body}"))
}

fn first_change_id(page: &serde_json::Value) -> String {
    page["changes"][0]["changeId"]
        .as_str()
        .or_else(|| page["changes"][0]["id"].as_str())
        .unwrap_or_else(|| panic!("page lists at least one Change: {page}"))
        .to_owned()
}

#[test]
fn v2_changes_and_attention_pages_continue_across_the_elected_and_default_lanes() {
    let repo = three_change_ready_repo();
    let inspector = Inspector::spawn_current(repo.path());
    for lens in ["changes", "attention"] {
        let path = format!("/api/v2/{lens}");
        let default = page(&inspector, &format!("{path}?limit=1"));
        let elected = page(&inspector, &format!("{path}?limit=1&access=authoritative"));
        assert_eq!(
            default["projectionStamp"], elected["projectionStamp"],
            "{lens}: an elected page binds the same generation stamp as the derived lane"
        );
        let default_next = default["next"]
            .as_str()
            .unwrap_or_else(|| panic!("{lens}: default next: {default}"));
        let elected_next = elected["next"]
            .as_str()
            .unwrap_or_else(|| panic!("{lens}: elected next: {elected}"));
        let same_lane = page(
            &inspector,
            &format!("{path}?limit=1&after={}", urlencode(default_next)),
        );
        let default_to_elected = page(
            &inspector,
            &format!(
                "{path}?limit=1&after={}&access=authoritative",
                urlencode(default_next)
            ),
        );
        let elected_to_default = page(
            &inspector,
            &format!("{path}?limit=1&after={}", urlencode(elected_next)),
        );
        let expected = first_change_id(&same_lane);
        assert_eq!(
            first_change_id(&default_to_elected),
            expected,
            "{lens}: default → elected continues at the same row"
        );
        assert_eq!(
            first_change_id(&elected_to_default),
            expected,
            "{lens}: elected → default continues at the same row"
        );
        assert_ne!(
            first_change_id(&default),
            expected,
            "{lens}: the continuation moved past the first row"
        );
    }
}

#[test]
fn v2_elected_first_page_survives_a_later_derived_read_on_a_fresh_inspector() {
    for lens in ["changes", "attention"] {
        let repo = three_change_ready_repo();
        // No harness profile pre-read: the process has not selected a generation yet.
        let inspector = Inspector::spawn_current_unready(repo.path());
        let path = format!("/api/v2/{lens}");
        let elected = page(&inspector, &format!("{path}?limit=1&access=authoritative"));
        let token = elected["next"]
            .as_str()
            .unwrap_or_else(|| panic!("{lens}: elected next: {elected}"))
            .to_owned();
        let same_lane_cold = page(
            &inspector,
            &format!(
                "{path}?limit=1&after={}&access=authoritative",
                urlencode(&token)
            ),
        );
        let derived = page(&inspector, &format!("{path}?limit=1"));
        assert_eq!(
            derived["projectionStamp"], elected["projectionStamp"],
            "{lens}: the elected page already carried the generation stamp before any derived read"
        );
        let elected_after = page(
            &inspector,
            &format!(
                "{path}?limit=1&after={}&access=authoritative",
                urlencode(&token)
            ),
        );
        let default_after = page(
            &inspector,
            &format!("{path}?limit=1&after={}", urlencode(&token)),
        );
        let expected = first_change_id(&same_lane_cold);
        assert_eq!(
            first_change_id(&elected_after),
            expected,
            "{lens}: elected continuation survives the warm-up"
        );
        assert_eq!(
            first_change_id(&default_after),
            expected,
            "{lens}: default continuation accepts the elected token"
        );
    }
}

#[test]
fn port_exhaustion_is_classified_by_error_kind() {
    use std::io::{Error, ErrorKind};

    assert!(support::inspect::is_port_exhaustion(&Error::from(
        ErrorKind::AddrNotAvailable
    )));
    assert!(!support::inspect::is_port_exhaustion(&Error::from(
        ErrorKind::ConnectionReset
    )));
    assert!(!support::inspect::is_port_exhaustion(&Error::from(
        ErrorKind::ConnectionRefused
    )));
}

#[test]
fn poll_backoff_doubles_from_20ms_and_caps_at_250ms() {
    let got: Vec<u64> = (0..7)
        .map(|a| support::timing::poll_backoff(a).as_millis() as u64)
        .collect();
    assert_eq!(got, vec![20, 40, 80, 160, 250, 250, 250]);
}

/// Re-executed by `inspector_stderr_is_readable_while_the_server_runs` as a
/// child that writes a marker to stderr and then stays alive until its stdin
/// closes. Without the child environment variable it does nothing.
#[test]
fn stderr_drain_child_entrypoint() {
    use std::io::{Read, Write};

    if std::env::var("POINTBREAK_TEST_STDERR_CHILD").as_deref() != Ok("1") {
        return;
    }
    let mut stderr = std::io::stderr().lock();
    stderr
        .write_all(b"stderr-drain-marker\n")
        .expect("write the stderr marker");
    stderr.flush().expect("flush the stderr marker");
    drop(stderr);
    let mut stdin = Vec::new();
    std::io::stdin()
        .read_to_end(&mut stdin)
        .expect("read stdin until the parent closes it");
}

#[test]
fn inspector_stderr_is_readable_while_the_server_runs() {
    use std::process::{Command, Stdio};

    use support::timing::HANG_GUARD;

    let mut child = Command::new(std::env::current_exe().expect("test binary path"))
        .args(["--exact", "stderr_drain_child_entrypoint", "--nocapture"])
        .env("POINTBREAK_TEST_STDERR_CHILD", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the stderr drain child");
    let buffer = support::inspect::spawn_stderr_drain(child.stderr.take().expect("child stderr"));

    let deadline = Instant::now() + HANG_GUARD;
    loop {
        let seen =
            String::from_utf8_lossy(&buffer.lock().expect("stderr buffer lock")).into_owned();
        if seen.contains("stderr-drain-marker") {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!(
                "stderr drain did not surface the child's marker within {HANG_GUARD:?}; buffer: {seen:?}"
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        child.try_wait().expect("poll the child").is_none(),
        "the marker must be readable while the child is still running"
    );

    drop(child.stdin.take());
    let status = child.wait().expect("wait for the child");
    assert!(status.success(), "stderr drain child failed: {status}");
}
