mod support;

use serde_json::Value;
use support::git_repo::GitRepo;
use support::{pointbreak, pointbreak_env};

#[test]
fn revision_help_lists_show() {
    let output = pointbreak(["revision", "--help"]);

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("show"));
}

/// #445 option 2: `revision show` liveness defaults to "did this land on the
/// default branch?" — narrow against the detected default branch — not broad
/// reachability. The anchored commit here is `main`'s own tip, so with the
/// tip-equality fix (#447) it reads `merged` and its label is the default branch.
#[test]
fn revision_show_liveness_defaults_to_the_integration_branch() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo.commit_all("change");
    let path = repo.path().to_str().unwrap();

    // A commit-range capture anchors the target (HEAD) commit, which is main's tip.
    let capture = pointbreak(["capture", "--repo", path, "--base", "HEAD~1"]);
    assert!(
        capture.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&capture.stderr)
    );

    let json = parse_json(&pointbreak(["revision", "show", "--repo", path]).stdout);
    let per_commit = json["commitRange"]["liveness"]["perCommit"]
        .as_array()
        .unwrap();
    assert_eq!(per_commit.len(), 1);
    assert_eq!(
        per_commit[0]["condition"], "merged",
        "the anchored commit is main's tip → landed on the default branch"
    );
    assert_eq!(per_commit[0]["liveBranch"], "main");
}

/// An explicit `--integration-ref` overrides the default: narrowed against a side
/// branch that main's tip has not landed on, the commit is not `merged`. It stays
/// a live tip (`live`), never unreachable — the integration check only decides
/// "merged into that ref".
#[test]
fn revision_show_accepts_explicit_integration_ref() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo.commit_all("change");
    // A side branch forking before HEAD: its tip does not reach main's tip.
    repo.git(["checkout", "-b", "side", "HEAD~1"]);
    repo.write("src/lib.rs", "pub fn value() -> u32 { 9 }\n");
    repo.commit_all("side");
    repo.git(["checkout", "main"]);
    let path = repo.path().to_str().unwrap();

    let capture = pointbreak(["capture", "--repo", path, "--base", "HEAD~1"]);
    assert!(
        capture.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&capture.stderr)
    );

    let show = pointbreak([
        "revision",
        "show",
        "--repo",
        path,
        "--integration-ref",
        "refs/heads/side",
    ]);
    assert!(
        show.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&show.stderr)
    );
    let json = parse_json(&show.stdout);
    let per_commit = json["commitRange"]["liveness"]["perCommit"]
        .as_array()
        .unwrap();
    assert_eq!(per_commit.len(), 1);
    assert_eq!(
        per_commit[0]["condition"], "live",
        "main's tip has not landed on the side branch, but it is still a live tip"
    );
}

/// #445 regression (dangling origin/HEAD): a symbolic `origin/HEAD` whose target
/// does not resolve must not suppress the whole liveness block — detection falls
/// through to local `main`, so `revision show` still emits liveness against it.
#[test]
fn revision_show_liveness_survives_a_dangling_origin_head() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo.commit_all("change");
    // A dangling origin/HEAD: symbolic ref to a remote-tracking branch that does
    // not exist.
    repo.git([
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/missing",
    ]);
    let path = repo.path().to_str().unwrap();

    let capture = pointbreak(["capture", "--repo", path, "--base", "HEAD~1"]);
    assert!(
        capture.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&capture.stderr)
    );

    let json = parse_json(&pointbreak(["revision", "show", "--repo", path]).stdout);
    let per_commit = json["commitRange"]["liveness"]["perCommit"]
        .as_array()
        .expect("liveness block present despite dangling origin/HEAD");
    assert_eq!(per_commit.len(), 1);
    assert_eq!(per_commit[0]["condition"], "merged");
    assert_eq!(per_commit[0]["liveBranch"], "main");
}

#[test]
fn revision_show_positional_accepts_and_omits() {
    let repo = modified_repo();
    pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);
    let path = repo.path().to_str().unwrap();

    // Omitted: shows the current capture (the flag-absent behavior).
    let current = pointbreak(["revision", "show", "--repo", path]);
    assert!(
        current.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&current.stderr)
    );
    let json = parse_json(&current.stdout);
    assert_eq!(json["schema"], "pointbreak.review-revision");
    let id = json["revision"]["id"].as_str().unwrap().to_owned();

    // Positional full id: selects that revision.
    let selected = pointbreak(["revision", "show", &id, "--repo", path]);
    assert!(
        selected.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&selected.stderr)
    );
    assert_eq!(parse_json(&selected.stdout)["revision"]["id"], id);
}

#[test]
fn revision_show_positional_resolves_a_prefixed_short_id() {
    let repo = modified_repo();
    pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);
    let path = repo.path().to_str().unwrap();

    let current = parse_json(&pointbreak(["revision", "show", "--repo", path]).stdout);
    let id = current["revision"]["id"].as_str().unwrap().to_owned();
    // id = "rev:…sha256:<hex>"; the bare-prefixed short form resolves it.
    let digest = id.rsplit_once("sha256:").unwrap().1;
    let prefixed_short = format!("rev:{}", &digest[..8]);

    let selected = pointbreak(["revision", "show", &prefixed_short, "--repo", path]);
    assert!(
        selected.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&selected.stderr)
    );
    assert_eq!(parse_json(&selected.stdout)["revision"]["id"], id);
}

#[test]
fn revision_show_emits_v2_json() {
    let repo = modified_repo();
    pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);

    let output = pointbreak(["revision", "show", "--repo", repo.path().to_str().unwrap()]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_json(&output.stdout);

    assert_eq!(json["schema"], "pointbreak.review-revision");
    assert_eq!(json["version"], 2);
    assert!(json.get("adapterNotes").is_none());
    assert!(json["summary"].get("adapterNoteCount").is_none());
    assert!(
        json["eventSetHash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert_eq!(json["eventCount"], 2);
    assert_eq!(json["revision"]["id"], json["filters"]["revisionId"]);
    assert_eq!(json["currentAssessment"]["status"], "unassessed");
    assert!(json["currentAssessment"].get("assessment").is_none());
    assert!(json["currentAssessment"].get("assessmentId").is_none());
    assert!(json.get("statePath").is_none());
}

#[test]
fn revision_show_rejects_invalid_track_before_json_output() {
    let repo = modified_repo();
    pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);

    let output = pointbreak([
        "revision",
        "show",
        "--repo",
        repo.path().to_str().unwrap(),
        "--track",
        "Agent Codex",
    ]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("track"));
}

#[test]
fn revision_show_json_pretty_prints() {
    let repo = modified_repo();
    pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);

    let output = pointbreak([
        "revision",
        "show",
        "--repo",
        repo.path().to_str().unwrap(),
        "--format",
        "json-pretty",
    ]);

    assert!(String::from_utf8_lossy(&output.stdout).starts_with("{\n"));
}

#[test]
fn revision_show_rejects_removed_pretty_flag() {
    let repo = modified_repo();
    pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);

    let output = pointbreak([
        "revision",
        "show",
        "--repo",
        repo.path().to_str().unwrap(),
        "--pretty",
    ]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--pretty"));
}

#[test]
fn revision_show_supports_explicit_revision_when_ambiguous() {
    let repo = modified_repo();
    let first =
        parse_json(&pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]).stdout);
    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    let second =
        parse_json(&pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]).stdout);

    let ambiguous = pointbreak(["revision", "show", "--repo", repo.path().to_str().unwrap()]);
    assert!(!ambiguous.status.success());
    assert!(String::from_utf8_lossy(&ambiguous.stderr).contains("multiple captured revisions"));

    let explicit = pointbreak([
        "revision",
        "show",
        first["revision"]["id"].as_str().unwrap(),
        "--repo",
        repo.path().to_str().unwrap(),
    ]);
    let json = parse_json(&explicit.stdout);

    assert_ne!(first["revision"]["id"], second["revision"]["id"]);
    assert_eq!(json["revision"]["id"], first["revision"]["id"]);
}

#[test]
fn revision_show_include_body_hydrates_without_internal_paths() {
    let repo = modified_repo();
    pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);
    add_observation_with_body(&repo, "agent:codex", "Body", "visible body");

    let output = pointbreak([
        "revision",
        "show",
        "--repo",
        repo.path().to_str().unwrap(),
        "--include-body",
    ]);
    let json = parse_json(&output.stdout);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(json["filters"]["includeBody"], true);
    assert!(stdout.contains("visible body"));
    assert!(!stdout.contains("artifacts/notes/"));
    assert!(!stdout.contains("artifacts/objects/"));
    assert!(!stdout.contains(".pointbreak/data/events"));
    assert!(json.get("statePath").is_none());
    assert!(json.get("snapshotArtifactPath").is_none());
}

#[test]
fn revision_show_includes_input_requests_and_omits_legacy_fields() {
    let repo = modified_repo();
    pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);
    let requested = add_input_request_with_body(&repo, "visible request body");
    respond_to_input_request(
        &repo,
        requested["inputRequestId"].as_str().unwrap(),
        "approved",
    );

    let output = pointbreak([
        "revision",
        "show",
        "--repo",
        repo.path().to_str().unwrap(),
        "--include-body",
    ]);
    let json = parse_json(&output.stdout);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let input_request_id = requested["inputRequestId"].as_str().unwrap();

    assert_eq!(json["inputRequests"].as_array().unwrap().len(), 1);
    assert_eq!(json["inputRequests"][0]["id"], input_request_id);
    assert_eq!(json["inputRequests"][0]["mode"], "operative");
    assert_eq!(json["inputRequests"][0]["body"], "visible request body");
    assert_eq!(
        json["inputRequests"][0]["responses"][0]["reason"],
        "approved"
    );
    assert_eq!(json["summary"]["inputRequestCount"], 1);
    assert!(!stdout.contains("artifacts/notes/"));
    assert!(!stdout.contains("artifacts/objects/"));
    assert!(!stdout.contains(".pointbreak/data/events"));
    assert!(!stdout.contains("\"blocking\""));
    assert!(json.get("interventions").is_none());
    assert!(json["summary"].get("interventionCount").is_none());
    assert!(json["inputRequests"][0].get("resolutions").is_none());
    assert!(json["rows"].as_array().unwrap().iter().any(|row| {
        row["kind"] == "input_request"
            && row["relatedInputRequestIds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|id| id == input_request_id)
            && row.get("relatedInterventionIds").is_none()
    }));
}

#[test]
fn revision_show_rows_are_narrative_first_and_snapshot_complete() {
    let repo = modified_repo();
    pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);
    add_observation(&repo, "agent:codex", "Narrative");

    let json = parse_json(
        &pointbreak(["revision", "show", "--repo", repo.path().to_str().unwrap()]).stdout,
    );

    let rows = json["rows"].as_array().unwrap();
    let first_remainder = rows
        .iter()
        .position(|row| row["projectionPhase"] == "snapshot_remainder")
        .unwrap();
    let narrative = rows
        .iter()
        .position(|row| row["projectionPhase"] == "narrative")
        .unwrap();

    assert!(narrative < first_remainder);
    assert!(
        json["summary"]["snapshotRemainderRowCount"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row["snapshotOrder"].is_object())
            .count() as u64,
        json["summary"]["snapshotRowCount"].as_u64().unwrap()
    );
}

#[test]
fn revision_show_track_filter_echoes_and_narrows_narrative_only() {
    let repo = multi_file_repo();
    pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);
    add_observation(&repo, "agent:codex", "Codex");
    add_observation(&repo, "agent:claude", "Claude");

    let all = parse_json(
        &pointbreak(["revision", "show", "--repo", repo.path().to_str().unwrap()]).stdout,
    );
    let codex = parse_json(
        &pointbreak([
            "revision",
            "show",
            "--repo",
            repo.path().to_str().unwrap(),
            "--track",
            "agent:codex",
        ])
        .stdout,
    );

    assert_eq!(codex["filters"]["trackId"], "agent:codex");
    assert_eq!(codex["observations"].as_array().unwrap().len(), 1);
    assert_eq!(codex["observations"][0]["title"], "Codex");
    assert!(
        all["summary"]["narrativeRowCount"].as_u64().unwrap()
            > codex["summary"]["narrativeRowCount"].as_u64().unwrap()
    );
    assert_eq!(
        all["summary"]["snapshotRemainderRowCount"],
        codex["summary"]["snapshotRemainderRowCount"]
    );
}

#[test]
fn revision_show_includes_current_assessment_status() {
    let repo = modified_repo();
    pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);
    add_assessment(&repo);

    let json = parse_json(
        &pointbreak(["revision", "show", "--repo", repo.path().to_str().unwrap()]).stdout,
    );

    assert_eq!(json["currentAssessment"]["status"], "resolved");
    assert_eq!(json["currentAssessment"]["assessment"], "accepted");
    assert_eq!(json["assessments"].as_array().unwrap().len(), 1);
}

#[test]
fn unit_show_projects_range_capture_with_bound_snapshot() {
    let repo = support::committed_repo();
    let capture = parse_json(
        &pointbreak([
            "capture",
            "--repo",
            repo.path().to_str().unwrap(),
            "--base",
            "HEAD~1",
        ])
        .stdout,
    );
    let revision_id = capture["revision"]["id"].as_str().unwrap();

    let output = pointbreak([
        "revision",
        "show",
        revision_id,
        "--repo",
        repo.path().to_str().unwrap(),
    ]);

    // The command succeeding proves load_bound_object_artifact validated the
    // bound snapshot against the range identity.
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = parse_json(&output.stdout);
    assert_eq!(json["revision"]["source"]["kind"], "git_commit_range");
    assert_eq!(json["revision"]["base"]["kind"], "git_commit");
    assert_eq!(json["revision"]["target"]["kind"], "git_commit");
    assert!(json["summary"]["snapshotRowCount"].as_u64().unwrap() > 0);
    assert!(
        stdout.contains("src/lib.rs"),
        "snapshot rows must include the committed file"
    );
}

#[test]
fn unit_show_disambiguates_worktree_and_range_units() {
    let repo = support::committed_repo();
    // A dirty worktree yields a worktree unit; --base yields a range unit.
    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    pointbreak(["capture", "--repo", repo.path().to_str().unwrap()]);
    let range = parse_json(
        &pointbreak([
            "capture",
            "--repo",
            repo.path().to_str().unwrap(),
            "--base",
            "HEAD~1",
        ])
        .stdout,
    );

    let ambiguous = pointbreak(["revision", "show", "--repo", repo.path().to_str().unwrap()]);
    assert!(!ambiguous.status.success());
    assert!(
        String::from_utf8_lossy(&ambiguous.stderr).contains("multiple captured revisions"),
        "stderr:\n{}",
        String::from_utf8_lossy(&ambiguous.stderr)
    );

    let json = parse_json(
        &pointbreak([
            "revision",
            "show",
            range["revision"]["id"].as_str().unwrap(),
            "--repo",
            repo.path().to_str().unwrap(),
        ])
        .stdout,
    );
    assert_eq!(json["revision"]["id"], range["revision"]["id"]);
    assert_eq!(json["revision"]["source"]["kind"], "git_commit_range");
}

#[test]
fn unit_show_renders_verification_status_on_members_and_capture() {
    let home = tempfile::tempdir().unwrap();
    let env_home = home.path().to_str().unwrap();
    let env: [(&str, &str); 1] = [("POINTBREAK_HOME", env_home)];
    // A present-but-unenrolled key → signs, verifies untrusted_key under the empty trust set.
    assert!(
        pointbreak_env(["key", "init", "--name", "default"], &env)
            .status
            .success()
    );

    let repo = modified_repo();
    let repo_arg = repo.path().to_str().unwrap();
    assert!(
        pointbreak_env(["capture", "--repo", repo_arg], &env)
            .status
            .success()
    );
    assert!(
        pointbreak_env(
            [
                "observation",
                "add",
                "--repo",
                repo_arg,
                "--track",
                "agent:codex",
                "--title",
                "t",
                "--body",
                "b",
            ],
            &env,
        )
        .status
        .success()
    );

    let out = pointbreak_env(["revision", "show", "--repo", repo_arg], &env);
    assert!(
        out.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    // The capture identity (the captured event) carries the status.
    assert_eq!(doc["revision"]["verificationStatus"], "untrusted_key");
    // Each narrative member carries the status of its own event.
    assert_eq!(
        doc["observations"][0]["verificationStatus"],
        "untrusted_key"
    );
}

#[test]
fn unit_show_renders_endorsement_on_capture_identity() {
    let home = tempfile::tempdir().unwrap();
    let env_home = home.path().to_str().unwrap();
    let env: [(&str, &str); 1] = [("POINTBREAK_HOME", env_home)];
    assert!(
        pointbreak_env(["key", "init", "--name", "default"], &env)
            .status
            .success()
    );
    let repo = modified_repo();
    let repo_arg = repo.path().to_str().unwrap();
    // Enroll the default key under kevin + attest kind/roles (reader config).
    assert!(
        pointbreak_env(
            [
                "key",
                "enroll",
                "default",
                "--actor",
                "actor:git-email:kevin@swiber.dev",
                "--repo",
                repo_arg,
            ],
            &env,
        )
        .status
        .success()
    );
    assert!(
        pointbreak_env(
            [
                "identity",
                "attest",
                "actor:git-email:kevin@swiber.dev",
                "--kind",
                "human",
                "--role",
                "reviewer",
                "--repo",
                repo_arg,
            ],
            &[],
        )
        .status
        .success()
    );
    // Capture UNSIGNED so the detached endorsement carrier is not deduped.
    assert!(
        pointbreak_env(
            ["capture", "--repo", repo_arg],
            &[("POINTBREAK_HOME", env_home), ("POINTBREAK_SIGNING", "off")],
        )
        .status
        .success()
    );
    let target = captured_event_id(repo.path());
    assert!(
        pointbreak_env(
            ["endorse", &target, "--repo", repo_arg],
            &[
                ("POINTBREAK_HOME", env_home),
                ("POINTBREAK_ACTOR_ID", "actor:git-email:kevin@swiber.dev"),
            ],
        )
        .status
        .success()
    );

    let out = pointbreak_env(
        ["revision", "show", "--repo", repo_arg],
        &[("POINTBREAK_HOME", env_home)],
    );
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    let endorsement = &doc["revision"]["endorsements"][0];
    assert_eq!(endorsement["classification"], "endorsement-trusted");
    assert_eq!(endorsement["endorser"], "actor:git-email:kevin@swiber.dev");
    assert_eq!(endorsement["endorserAttributes"]["kind"], "human");
}

#[test]
fn text_digest_is_bounded_and_never_renders_rows() {
    let repo = modified_repo();
    let repo_arg = repo.path().to_str().unwrap();
    pointbreak(["capture", "--repo", repo_arg]);
    add_observation(&repo, "agent:codex", "Narrative");
    add_input_request_with_body(&repo, "please decide");
    add_assessment(&repo);

    let output = pointbreak(["revision", "show", "--repo", repo_arg, "--format", "text"]);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("current call"), "stdout:\n{stdout}");
    assert!(stdout.contains("rev:"), "stdout:\n{stdout}");
    assert!(stdout.contains("observation"), "stdout:\n{stdout}");
    assert!(!stdout.contains("file_header"), "stdout:\n{stdout}");
    assert!(!stdout.contains("\"rows\""), "stdout:\n{stdout}");
    assert!(
        stdout.lines().count() < 40,
        "digest must be bounded, got {} lines:\n{stdout}",
        stdout.lines().count()
    );
}

#[test]
fn text_digest_reports_signed_by_enrolled_key() {
    const ENROLLED: &str = "actor:git-email:kevin@swiber.dev";

    // Enrolled key signs the assessment → the current call verifies valid.
    let yes_home = tempfile::tempdir().unwrap();
    let yes_home_s = yes_home.path().to_str().unwrap();
    let yes_env: [(&str, &str); 1] = [("POINTBREAK_HOME", yes_home_s)];
    assert!(
        pointbreak_env(["key", "init", "--name", "default"], &yes_env)
            .status
            .success()
    );
    let yes_repo = modified_repo();
    let yes_repo_arg = yes_repo.path().to_str().unwrap();
    assert!(
        pointbreak_env(
            [
                "key",
                "enroll",
                "default",
                "--actor",
                ENROLLED,
                "--repo",
                yes_repo_arg,
            ],
            &yes_env,
        )
        .status
        .success()
    );
    assert!(
        pointbreak_env(["capture", "--repo", yes_repo_arg], &yes_env)
            .status
            .success()
    );
    assert!(
        pointbreak_env(
            [
                "assessment",
                "add",
                "--repo",
                yes_repo_arg,
                "--track",
                "human:kevin",
                "--assessment",
                "accepted",
                "--summary",
                "ship it",
            ],
            &[
                ("POINTBREAK_HOME", yes_home_s),
                ("POINTBREAK_ACTOR_ID", ENROLLED)
            ],
        )
        .status
        .success()
    );
    let yes_out = pointbreak_env(
        [
            "revision",
            "show",
            "--repo",
            yes_repo_arg,
            "--format",
            "text",
        ],
        &yes_env,
    );
    let yes_stdout = String::from_utf8_lossy(&yes_out.stdout);
    assert!(
        yes_stdout.contains("signed by enrolled key: yes"),
        "stdout:\n{yes_stdout}"
    );

    // Unsigned assessment → not signed by an enrolled key.
    let no_home = tempfile::tempdir().unwrap();
    let no_home_s = no_home.path().to_str().unwrap();
    let no_repo = modified_repo();
    let no_repo_arg = no_repo.path().to_str().unwrap();
    assert!(
        pointbreak_env(
            ["capture", "--repo", no_repo_arg],
            &[
                ("POINTBREAK_HOME", no_home_s),
                ("POINTBREAK_SIGNING", "off")
            ],
        )
        .status
        .success()
    );
    assert!(
        pointbreak_env(
            [
                "assessment",
                "add",
                "--repo",
                no_repo_arg,
                "--track",
                "human:kevin",
                "--assessment",
                "accepted",
                "--summary",
                "ship it",
            ],
            &[
                ("POINTBREAK_HOME", no_home_s),
                ("POINTBREAK_SIGNING", "off")
            ],
        )
        .status
        .success()
    );
    let no_out = pointbreak_env(
        [
            "revision",
            "show",
            "--repo",
            no_repo_arg,
            "--format",
            "text",
        ],
        &[("POINTBREAK_HOME", no_home_s)],
    );
    let no_stdout = String::from_utf8_lossy(&no_out.stdout);
    assert!(
        no_stdout.contains("signed by enrolled key: no"),
        "stdout:\n{no_stdout}"
    );
}

#[test]
fn text_digest_clamps_long_open_request_titles() {
    let repo = modified_repo();
    let repo_arg = repo.path().to_str().unwrap();
    pointbreak(["capture", "--repo", repo_arg]);
    let long_title = "x".repeat(320);
    pointbreak([
        "input-request",
        "open",
        "--repo",
        repo_arg,
        "--track",
        "agent:codex",
        "--title",
        &long_title,
        "--reason",
        "manual-decision-required",
    ]);

    let output = pointbreak(["revision", "show", "--repo", repo_arg, "--format", "text"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let longest = stdout
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap();

    assert!(stdout.contains("open input requests"), "stdout:\n{stdout}");
    // The one user-controlled string in the digest must not blow the line bound.
    assert!(
        !stdout.contains(&long_title),
        "the full 320-char title must be clamped:\n{stdout}"
    );
    assert!(
        longest < 150,
        "digest lines must stay bounded, longest was {longest}:\n{stdout}"
    );
}

#[test]
fn text_digest_groups_fact_counts_by_track() {
    let repo = multi_file_repo();
    let repo_arg = repo.path().to_str().unwrap();
    pointbreak(["capture", "--repo", repo_arg]);
    add_observation(&repo, "agent:codex", "Codex finding");
    add_observation(&repo, "agent:claude", "Claude finding");

    let output = pointbreak(["revision", "show", "--repo", repo_arg, "--format", "text"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("tracks:"), "stdout:\n{stdout}");
    assert!(stdout.contains("agent:codex"), "stdout:\n{stdout}");
    assert!(stdout.contains("agent:claude"), "stdout:\n{stdout}");
}

/// A repo whose captured branch tip is about to be rewritten: `main` holds a
/// base commit, `feat/amend` holds one captured commit. Returns
/// `(repo, revision_id, recorded_head_oid)`.
fn amendable_capture_repo() -> (GitRepo, String, String) {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.git(["checkout", "-b", "feat/amend"]);
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo.commit_all("captured change");

    let captured = parse_json(
        &pointbreak([
            "capture",
            "--repo",
            repo.path().to_str().unwrap(),
            "--base",
            "main",
        ])
        .stdout,
    );
    let revision_id = captured["revision"]["id"].as_str().unwrap().to_owned();
    let recorded_head = captured["revision"]["target"]["commitOid"]
        .as_str()
        .unwrap()
        .to_owned();
    (repo, revision_id, recorded_head)
}

/// Amend the tip of the current branch with new content, returning the new tip.
fn amend_with_content(repo: &GitRepo, contents: &str) -> String {
    repo.write("src/lib.rs", contents);
    repo.git(["add", "--all"]);
    repo.git(["commit", "--amend", "--no-edit"]);
    repo.git(["rev-parse", "HEAD"]).stdout.trim().to_owned()
}

fn shown_document(repo: &GitRepo, revision_id: &str) -> Value {
    let output = pointbreak([
        "revision",
        "show",
        revision_id,
        "--repo",
        repo.path().to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    parse_json(&output.stdout)
}

fn shown_liveness(repo: &GitRepo, revision_id: &str) -> Value {
    shown_document(repo, revision_id)["commitRange"]["liveness"].clone()
}

/// An amended-away capture target is `unreachable` — never `orphaned` — and the
/// recorded ref association is diagnosed as rewritten, naming the recorded and
/// current OIDs plus the reflog action, without touching the durable record.
#[test]
fn revision_show_reads_amended_capture_as_unreachable_with_rewrite_diagnosis() {
    let (repo, revision_id, recorded_head) = amendable_capture_repo();
    let amended_tip = amend_with_content(&repo, "pub fn value() -> u32 { 3 }\n");
    assert_ne!(recorded_head, amended_tip);

    let document = shown_document(&repo, &revision_id);
    let liveness = document["commitRange"]["liveness"].clone();

    let per_commit = liveness["perCommit"].as_array().unwrap();
    assert_eq!(per_commit.len(), 1);
    assert_eq!(per_commit[0]["condition"], "unreachable");
    assert!(
        per_commit[0].get("reason").is_none(),
        "unreachable is a first-class condition, not an orphan reason: {per_commit:?}"
    );
    assert_eq!(
        per_commit[0]["retention"], "reflog",
        "the amended-away object is still reflog-retained"
    );
    assert_eq!(liveness["headline"]["condition"], "unreachable");

    let continuity = liveness["refContinuity"].as_array().unwrap();
    let entry = continuity
        .iter()
        .find(|entry| entry["refName"] == "refs/heads/feat/amend")
        .expect("the captured branch ref has a continuity entry");
    assert_eq!(entry["continuity"], "rewritten");
    assert_eq!(entry["recordedHeadOid"], recorded_head);
    assert_eq!(entry["currentTipOid"], amended_tip);
    assert!(
        entry["rewriteAction"]
            .as_str()
            .unwrap()
            .starts_with("commit (amend)"),
        "the reflog action names the rewrite: {entry:?}"
    );
    assert_eq!(
        entry["sameTree"], false,
        "a changed-content amend is not a same-tree rewrite"
    );

    // Enrichment diagnostics surface in the document's top-level array, the
    // same place divergence diagnostics land.
    let diagnostics = document["diagnostics"].as_array().unwrap();
    let rewritten = diagnostics
        .iter()
        .find(|diagnostic| diagnostic["code"] == "ref_rewritten")
        .expect("a rewritten recorded ref surfaces a diagnostic");
    let message = rewritten["message"].as_str().unwrap();
    assert!(
        message.contains(&recorded_head[..12]) && message.contains(&amended_tip[..12]),
        "the diagnostic names the recorded and current OIDs: {message}"
    );
}

/// A message-only amend keeps the tree: the rewrite is diagnosed with same-tree
/// confidence.
#[test]
fn revision_show_marks_a_same_tree_rewrite() {
    let (repo, revision_id, recorded_head) = amendable_capture_repo();
    repo.git(["commit", "--amend", "-m", "captured change, reworded"]);
    let amended_tip = repo.git(["rev-parse", "HEAD"]).stdout.trim().to_owned();
    assert_ne!(recorded_head, amended_tip);

    let liveness = shown_liveness(&repo, &revision_id);
    let entry = &liveness["refContinuity"][0];

    assert_eq!(entry["continuity"], "rewritten");
    assert_eq!(entry["sameTree"], true);
}

/// A multi-amend chain still diagnoses the recorded head against the final tip.
#[test]
fn revision_show_diagnoses_a_multi_amend_chain() {
    let (repo, revision_id, recorded_head) = amendable_capture_repo();
    amend_with_content(&repo, "pub fn value() -> u32 { 3 }\n");
    amend_with_content(&repo, "pub fn value() -> u32 { 4 }\n");
    let final_tip = amend_with_content(&repo, "pub fn value() -> u32 { 5 }\n");

    let liveness = shown_liveness(&repo, &revision_id);
    let entry = &liveness["refContinuity"][0];

    assert_eq!(entry["continuity"], "rewritten");
    assert_eq!(entry["recordedHeadOid"], recorded_head);
    assert_eq!(entry["currentTipOid"], final_tip);
}

/// An amend performed in a linked worktree writes the shared branch reflog, so
/// the rewrite is diagnosed from the main worktree too.
#[test]
fn revision_show_diagnoses_an_amend_from_a_linked_worktree() {
    let (repo, revision_id, recorded_head) = amendable_capture_repo();
    repo.git(["checkout", "main"]);
    let linked_parent = tempfile::tempdir().unwrap();
    let linked = linked_parent
        .path()
        .join("linked-amend-wt")
        .to_str()
        .unwrap()
        .to_owned();
    repo.git(["worktree", "add", &linked, "feat/amend"]);
    std::fs::write(
        std::path::Path::new(&linked).join("src/lib.rs"),
        "pub fn value() -> u32 { 7 }\n",
    )
    .unwrap();
    let git_at = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&linked)
            .status()
            .unwrap();
        assert!(status.success());
    };
    git_at(&["add", "--all"]);
    git_at(&["commit", "--amend", "--no-edit"]);

    let liveness = shown_liveness(&repo, &revision_id);
    let entry = &liveness["refContinuity"][0];

    assert_eq!(entry["continuity"], "rewritten");
    assert_eq!(entry["recordedHeadOid"], recorded_head);
    repo.git(["worktree", "remove", "--force", &linked]);
}

/// Expired reflog evidence degrades the diagnosis to `moved` — never a false
/// `rewritten`, never an error, and never a change to the durable record.
#[test]
fn revision_show_degrades_to_ref_moved_when_reflog_evidence_expired() {
    let (repo, revision_id, recorded_head) = amendable_capture_repo();
    let amended_tip = amend_with_content(&repo, "pub fn value() -> u32 { 3 }\n");
    repo.git(["reflog", "expire", "--expire=now", "--all"]);

    let liveness = shown_liveness(&repo, &revision_id);

    let per_commit = liveness["perCommit"].as_array().unwrap();
    assert_eq!(per_commit[0]["condition"], "unreachable");
    assert_eq!(
        per_commit[0]["retention"], "none",
        "no reflog retains the amended-away object any more"
    );

    let entry = &liveness["refContinuity"][0];
    assert_eq!(entry["continuity"], "moved");
    assert_eq!(entry["recordedHeadOid"], recorded_head);
    assert_eq!(entry["currentTipOid"], amended_tip);
    assert!(entry.get("rewriteAction").is_none());
}

/// A gc'd capture target reads `missing` — object availability stays
/// distinguishable from live-ref reachability — while the revision stays shown.
#[test]
fn revision_show_reads_pruned_capture_target_as_missing() {
    let (repo, revision_id, recorded_head) = amendable_capture_repo();
    amend_with_content(&repo, "pub fn value() -> u32 { 3 }\n");
    repo.git(["reflog", "expire", "--expire=now", "--all"]);
    repo.git(["prune", "--expire=now"]);
    let gone = std::process::Command::new("git")
        .args(["cat-file", "-e", &recorded_head])
        .current_dir(repo.path())
        .status()
        .unwrap();
    assert!(
        !gone.success(),
        "the recorded target object must be pruned for this fixture"
    );

    let liveness = shown_liveness(&repo, &revision_id);

    let per_commit = liveness["perCommit"].as_array().unwrap();
    assert_eq!(per_commit[0]["condition"], "missing");
    assert!(
        per_commit[0].get("retention").is_none(),
        "retention only qualifies a present-but-unreachable object"
    );
    assert_eq!(liveness["headline"]["condition"], "missing");
    assert_eq!(liveness["refContinuity"][0]["continuity"], "moved");
}

/// A deleted recorded ref is diagnosed `deleted`, with no fabricated tip.
#[test]
fn revision_show_reads_deleted_ref_continuity() {
    let (repo, revision_id, recorded_head) = amendable_capture_repo();
    repo.git(["checkout", "main"]);
    repo.git(["branch", "-D", "feat/amend"]);

    let liveness = shown_liveness(&repo, &revision_id);

    let entry = &liveness["refContinuity"][0];
    assert_eq!(entry["continuity"], "deleted");
    assert_eq!(entry["recordedHeadOid"], recorded_head);
    assert!(entry.get("currentTipOid").is_none());
    assert_eq!(liveness["perCommit"][0]["condition"], "unreachable");
}

/// A ref that advanced normally reads `advanced`, and the captured commit —
/// now interior to the live branch — reads `live`, not unreachable: it is
/// reachable from a live ref even though it has not landed on the integration
/// branch.
#[test]
fn revision_show_reads_advanced_ref_and_carried_commit_as_live() {
    let (repo, revision_id, recorded_head) = amendable_capture_repo();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    repo.commit_all("follow-up on the branch");
    let advanced_tip = repo.git(["rev-parse", "HEAD"]).stdout.trim().to_owned();

    let liveness = shown_liveness(&repo, &revision_id);

    let per_commit = liveness["perCommit"].as_array().unwrap();
    assert_eq!(
        per_commit[0]["condition"], "live",
        "a commit carried by a live branch is live, not unreachable"
    );
    assert_eq!(per_commit[0]["liveBranch"], "feat/amend");

    let entry = &liveness["refContinuity"][0];
    assert_eq!(entry["continuity"], "advanced");
    assert_eq!(entry["recordedHeadOid"], recorded_head);
    assert_eq!(entry["currentTipOid"], advanced_tip);
}

/// The still-current ref association reads `current`.
#[test]
fn revision_show_reads_current_ref_continuity() {
    let (repo, revision_id, recorded_head) = amendable_capture_repo();

    let liveness = shown_liveness(&repo, &revision_id);

    let entry = &liveness["refContinuity"][0];
    assert_eq!(entry["continuity"], "current");
    assert_eq!(entry["recordedHeadOid"], recorded_head);
    assert_eq!(entry["currentTipOid"], recorded_head);
    assert!(entry.get("rewriteAction").is_none());
}

/// Find the captured Revision event id via the public read path (`read_events`).
fn captured_event_id(repo_path: &std::path::Path) -> String {
    pointbreak::session::read_events(repo_path)
        .unwrap()
        .iter()
        .find(|e| e.event_type == pointbreak::session::event::EventType::WorkObjectProposed)
        .expect("a captured review unit event")
        .event_id
        .as_str()
        .to_owned()
}

fn modified_repo() -> GitRepo {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo
}

fn multi_file_repo() -> GitRepo {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.write("src/other.rs", "pub fn other() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo.write("src/other.rs", "pub fn other() -> u32 { 2 }\n");
    repo
}

fn add_observation(repo: &GitRepo, track: &str, title: &str) -> Value {
    parse_json(
        &pointbreak([
            "observation",
            "add",
            "--repo",
            repo.path().to_str().unwrap(),
            "--track",
            track,
            "--title",
            title,
        ])
        .stdout,
    )
}

fn add_observation_with_body(repo: &GitRepo, track: &str, title: &str, body: &str) -> Value {
    parse_json(
        &pointbreak([
            "observation",
            "add",
            "--repo",
            repo.path().to_str().unwrap(),
            "--track",
            track,
            "--title",
            title,
            "--body",
            body,
        ])
        .stdout,
    )
}

fn add_assessment(repo: &GitRepo) -> Value {
    parse_json(
        &pointbreak([
            "assessment",
            "add",
            "--repo",
            repo.path().to_str().unwrap(),
            "--track",
            "human:kevin",
            "--assessment",
            "accepted",
            "--summary",
            "ship it",
        ])
        .stdout,
    )
}

fn add_input_request_with_body(repo: &GitRepo, body: &str) -> Value {
    parse_json(
        &pointbreak([
            "input-request",
            "open",
            "--repo",
            repo.path().to_str().unwrap(),
            "--track",
            "agent:codex",
            "--title",
            "Need decision",
            "--reason",
            "manual-decision-required",
            "--body",
            body,
        ])
        .stdout,
    )
}

fn respond_to_input_request(repo: &GitRepo, input_request_id: &str, reason: &str) -> Value {
    parse_json(
        &pointbreak([
            "input-request",
            "respond",
            "--repo",
            repo.path().to_str().unwrap(),
            input_request_id,
            "--outcome",
            "approved",
            "--reason",
            reason,
        ])
        .stdout,
    )
}

fn parse_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("valid json")
}
