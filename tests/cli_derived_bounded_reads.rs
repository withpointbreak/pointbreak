mod support;

#[cfg(feature = "longitudinal-counting")]
use base64::Engine as _;
#[cfg(feature = "longitudinal-counting")]
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;
use support::git_repo::GitRepo;
use support::{pointbreak_env, superseded_dump_repo};

const ACTIVE: &[(&str, &str)] = &[("POINTBREAK_DERIVED_ACCESS", "sqlite-wal-bodyless-v1")];
const OFF: &[(&str, &str)] = &[("POINTBREAK_DERIVED_ACCESS", "off")];

#[test]
fn active_bounded_history_attention_and_revision_pages_use_projection_identity() {
    let (repo, _, _) = superseded_dump_repo();
    build(&repo);
    let repo_arg = repo.path().to_str().unwrap();

    for args in [
        vec!["history", "--repo", repo_arg, "--limit", "1"],
        vec!["history", "--repo", repo_arg, "--tail", "1"],
        vec!["attention", "list", "--repo", repo_arg],
        vec!["revision", "list", "--repo", repo_arg, "--limit", "1"],
    ] {
        let output = pointbreak_env(args, ACTIVE);
        assert_success(&output);
        let json = parse_json(&output.stdout);
        assert!(json["projectionStamp"].is_string(), "{json:#}");
        assert!(json.get("eventSetHash").is_none(), "{json:#}");
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn bounded_active_documents_preserve_authoritative_domain_content() {
    let (repo, _, _) = superseded_dump_repo();
    build(&repo);
    let repo_arg = repo.path().to_str().unwrap();

    for args in [
        vec!["history", "--repo", repo_arg, "--limit", "2"],
        vec!["history", "--repo", repo_arg, "--tail", "2"],
        vec!["attention", "list", "--repo", repo_arg],
        vec!["revision", "list", "--repo", repo_arg, "--limit", "2"],
    ] {
        let active = pointbreak_env(&args, ACTIVE);
        let loose = pointbreak_env(&args, OFF);
        assert_success(&active);
        assert_success(&loose);
        let mut active = parse_json(&active.stdout);
        let mut loose = parse_json(&loose.stdout);
        active.as_object_mut().unwrap().remove("projectionStamp");
        loose.as_object_mut().unwrap().remove("eventSetHash");
        assert_eq!(active, loose, "command: {args:?}");
    }
}

#[test]
fn bounded_history_preserves_typed_filters_and_body_redaction_policy() {
    let (repo, _, revision) = superseded_dump_repo();
    let repo_arg = repo.path().to_str().unwrap();
    let added = pointbreak_env(
        [
            "observation",
            "add",
            "--repo",
            repo_arg,
            "--track",
            "agent:bounded-read",
            "--revision",
            &revision,
            "--title",
            "Bounded read",
            "--body",
            "selected body",
        ],
        OFF,
    );
    assert_success(&added);
    build(&repo);

    for include_body in [false, true] {
        let mut args = vec![
            "history",
            "--repo",
            repo_arg,
            "--limit",
            "10",
            "--track",
            "agent:bounded-read",
            "--event-type",
            "review-observation-recorded",
        ];
        if include_body {
            args.push("--include-body");
        }
        let active = pointbreak_env(&args, ACTIVE);
        let loose = pointbreak_env(&args, OFF);
        assert_success(&active);
        assert_success(&loose);
        let mut active = parse_json(&active.stdout);
        let mut loose = parse_json(&loose.stdout);
        active.as_object_mut().unwrap().remove("projectionStamp");
        loose.as_object_mut().unwrap().remove("eventSetHash");
        assert_eq!(active, loose);
        assert_eq!(
            active["entries"][0]["summary"]["body"].as_str(),
            include_body.then_some("selected body")
        );
    }
}

#[test]
fn ineligible_or_unbounded_reads_remain_authoritative() {
    let (repo, _, _) = superseded_dump_repo();
    build(&repo);
    let repo_arg = repo.path().to_str().unwrap();

    for args in [
        vec!["history", "--repo", repo_arg],
        vec![
            "history",
            "--repo",
            repo_arg,
            "--limit",
            "1",
            "--filter",
            "type:revision",
        ],
        vec![
            "history", "--repo", repo_arg, "--limit", "1", "--ref", "HEAD",
        ],
        vec!["revision", "list", "--repo", repo_arg],
        vec![
            "revision",
            "list",
            "--repo",
            repo_arg,
            "--filter",
            "is:superseded",
        ],
    ] {
        let output = pointbreak_env(args, ACTIVE);
        assert_success(&output);
        let json = parse_json(&output.stdout);
        assert!(json["eventSetHash"].is_string(), "{json:#}");
        assert!(json.get("projectionStamp").is_none(), "{json:#}");
    }
}

#[test]
fn bounded_revision_selectors_remain_authoritative_and_still_page() {
    let (repo, _, _) = superseded_dump_repo();
    build(&repo);
    let repo_arg = repo.path().to_str().unwrap();

    for extra in [vec!["--filter", "is:superseded"], vec!["--ref", "main"]] {
        let mut args = vec!["revision", "list", "--repo", repo_arg, "--limit", "1"];
        args.extend(extra);
        let output = pointbreak_env(args, ACTIVE);
        assert_success(&output);
        let json = parse_json(&output.stdout);
        assert!(json["eventSetHash"].is_string(), "{json:#}");
        assert!(json.get("projectionStamp").is_none(), "{json:#}");
        assert!(json["entries"].as_array().unwrap().len() <= 1);
    }
}

#[test]
fn revision_page_rejects_invalid_limits_and_continues_with_an_opaque_cursor() {
    let (repo, _, _) = superseded_dump_repo();
    build(&repo);
    let repo_arg = repo.path().to_str().unwrap();

    for limit in ["0", "501"] {
        let output = pointbreak_env(
            ["revision", "list", "--repo", repo_arg, "--limit", limit],
            ACTIVE,
        );
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("--limit must be between 1 and 500"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let malformed = pointbreak_env(
        [
            "revision",
            "list",
            "--repo",
            repo_arg,
            "--limit",
            "1",
            "--cursor",
            "not-a-cursor",
        ],
        ACTIVE,
    );
    assert!(!malformed.status.success());
    assert!(
        String::from_utf8_lossy(&malformed.stderr).contains("invalid --cursor"),
        "{}",
        String::from_utf8_lossy(&malformed.stderr)
    );

    let first = pointbreak_env(
        ["revision", "list", "--repo", repo_arg, "--limit", "1"],
        ACTIVE,
    );
    assert_success(&first);
    let first = parse_json(&first.stdout);
    let cursor = first["nextCursor"].as_str().expect("first page cursor");
    let second = pointbreak_env(
        [
            "revision", "list", "--repo", repo_arg, "--limit", "1", "--cursor", cursor,
        ],
        ACTIVE,
    );
    assert_success(&second);
    let second = parse_json(&second.stdout);
    assert_ne!(
        first["entries"][0]["revisionId"],
        second["entries"][0]["revisionId"]
    );
}

#[test]
fn bounded_revision_text_names_the_page_and_continuation() {
    let (repo, _, _) = superseded_dump_repo();
    build(&repo);
    let repo_arg = repo.path().to_str().unwrap();

    let output = pointbreak_env(
        [
            "revision", "list", "--repo", repo_arg, "--limit", "1", "--format", "text",
        ],
        ACTIVE,
    );
    assert_success(&output);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.lines().next().unwrap().starts_with("1 of "), "{text}");
    assert!(
        text.contains("… more revisions remain (continue with --cursor)"),
        "{text}"
    );
}

#[test]
fn revision_page_profile_or_snapshot_changes_require_a_cursor_restart() {
    let (repo, _, _) = superseded_dump_repo();
    build(&repo);
    let repo_arg = repo.path().to_str().unwrap();

    let active_first = run_revision_page(repo_arg, ACTIVE, None);
    let active_cursor = active_first["nextCursor"].as_str().unwrap();
    assert_restart_required(&pointbreak_env(
        [
            "revision",
            "list",
            "--repo",
            repo_arg,
            "--limit",
            "1",
            "--cursor",
            active_cursor,
        ],
        OFF,
    ));

    let loose_first = run_revision_page(repo_arg, OFF, None);
    let loose_cursor = loose_first["nextCursor"].as_str().unwrap();
    assert_restart_required(&pointbreak_env(
        [
            "revision",
            "list",
            "--repo",
            repo_arg,
            "--limit",
            "1",
            "--ref",
            "main",
            "--cursor",
            loose_cursor,
        ],
        OFF,
    ));
    assert_restart_required(&pointbreak_env(
        [
            "revision",
            "list",
            "--repo",
            repo_arg,
            "--limit",
            "1",
            "--cursor",
            loose_cursor,
        ],
        ACTIVE,
    ));

    let rebuilt = pointbreak_env(["store", "derived", "rebuild", "--repo", repo_arg], ACTIVE);
    assert_success(&rebuilt);
    assert_restart_required(&pointbreak_env(
        [
            "revision",
            "list",
            "--repo",
            repo_arg,
            "--limit",
            "1",
            "--cursor",
            active_cursor,
        ],
        ACTIVE,
    ));
}

#[test]
fn unavailable_active_reads_fall_back_once_with_an_actionable_hint() {
    let (repo, _, _) = superseded_dump_repo();
    let repo_arg = repo.path().to_str().unwrap();

    for args in [
        vec!["history", "--repo", repo_arg, "--limit", "1"],
        vec!["attention", "list", "--repo", repo_arg],
        vec!["revision", "list", "--repo", repo_arg, "--limit", "1"],
    ] {
        let output = pointbreak_env(args, ACTIVE);
        assert_success(&output);
        let json = parse_json(&output.stdout);
        assert!(json["eventSetHash"].is_string(), "{json:#}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            stderr.matches("pointbreak store derived build").count(),
            1,
            "{stderr}"
        );
    }
}

#[cfg(feature = "longitudinal-counting")]
#[test]
fn eligible_active_cli_routes_never_walk_event_directory_entries() {
    let (repo, _, _) = superseded_dump_repo();
    build(&repo);
    let repo_arg = repo.path().to_str().unwrap();
    let receipt_dir = tempfile::tempdir().unwrap();

    for (ordinal, args) in [
        vec!["history", "--repo", repo_arg, "--limit", "1"],
        vec!["attention", "list", "--repo", repo_arg],
        vec!["revision", "list", "--repo", repo_arg, "--limit", "1"],
    ]
    .into_iter()
    .enumerate()
    {
        let receipt_path = receipt_dir.path().join(format!("receipt-{ordinal}.json"));
        let request = serde_json::json!({
            "runIdentity": format!("{:064x}", ordinal + 1),
            "context": {
                "rootIdentity": "2".repeat(64),
                "operation": "BOUNDED_CLI_READ",
                "phase": format!("route-{ordinal}"),
                "baseExecutionIdentitySha256": "3".repeat(64),
                "derivativeExecutionIdentitySha256": "4".repeat(64),
                "manifestSha256": "5".repeat(64),
                "scheduleSha256": "6".repeat(64),
                "success": false,
                "semanticResultSha256": "7".repeat(64),
                "includeCapacityOwnership": false
            },
            "receiptPath": receipt_path,
        });
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).unwrap());
        let mut counted = vec!["--longitudinal-counting".to_owned(), encoded];
        counted.extend(args.into_iter().map(str::to_owned));
        let output = pointbreak_env(counted, ACTIVE);
        assert_success(&output);
        let receipt = parse_json(&std::fs::read(receipt_path).unwrap());
        assert_eq!(
            receipt["counters"]["directoryEntriesWalked"], 0,
            "{receipt:#}"
        );
    }
}

fn build(repo: &GitRepo) {
    let output = pointbreak_env(
        [
            "store",
            "derived",
            "build",
            "--repo",
            repo.path().to_str().unwrap(),
        ],
        ACTIVE,
    );
    assert_success(&output);
}

fn run_revision_page(repo: &str, env: &[(&str, &str)], cursor: Option<&str>) -> Value {
    let mut args = vec!["revision", "list", "--repo", repo, "--limit", "1"];
    if let Some(cursor) = cursor {
        args.extend(["--cursor", cursor]);
    }
    let output = pointbreak_env(args, env);
    assert_success(&output);
    parse_json(&output.stdout)
}

fn assert_restart_required(output: &std::process::Output) {
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("revision page changed; retry without --cursor"),
        "{stderr}"
    );
}

fn parse_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("valid JSON")
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
