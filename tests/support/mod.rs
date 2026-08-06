use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[allow(dead_code)]
pub mod event_signature_fixtures;
#[allow(dead_code)]
pub mod git_repo;
#[allow(dead_code)]
pub mod inspect;
#[allow(dead_code)]
pub mod snapshots;

#[allow(dead_code)]
pub fn pointbreak<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();
    let args = prepare_change_cli_fixture(args);
    let output = Command::new(env!("CARGO_BIN_EXE_pointbreak"))
        .args(&args)
        .env_remove("POINTBREAK_LOG")
        .env_remove("RUST_LOG")
        // Isolate byte-asserting tests from a developer's ambient output-lane
        // selector; tests that exercise POINTBREAK_FORMAT set it explicitly via pointbreak_env.
        .env_remove("POINTBREAK_FORMAT")
        // Isolate color-asserting tests from an ambient NO_COLOR / CLICOLOR_FORCE;
        // color tests select the lane explicitly with `--color`.
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE")
        // Isolate theme-asserting tests from a developer's ambient theme
        // selection; theme tests set these explicitly via pointbreak_env.
        .env_remove("POINTBREAK_THEME")
        .env_remove("BAT_THEME")
        .output()
        .expect("run pointbreak binary");
    remember_change_capture(&args, &output);
    if output.status.success()
        && let Some(repo) = repo_argument(&args)
    {
        inspect::sync_legacy_mirrors(&repo);
    }
    output
}

/// Run the binary without installing the ordinary ready-Change integration fixture.
/// Reserved for capability-fence tests that deliberately exercise L0 or M1.
#[allow(dead_code)]
pub fn pointbreak_unprepared<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_pointbreak"))
        .args(args)
        .env_remove("POINTBREAK_LOG")
        .env_remove("RUST_LOG")
        .env_remove("POINTBREAK_FORMAT")
        .env_remove("POINTBREAK_THEME")
        .env_remove("BAT_THEME")
        .output()
        .expect("run unprepared pointbreak binary")
}

#[allow(dead_code)]
pub fn pointbreak_unprepared_env<I, S>(args: I, env: &[(&str, &str)]) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(env!("CARGO_BIN_EXE_pointbreak"));
    command
        .args(args)
        .env_remove("POINTBREAK_LOG")
        .env_remove("RUST_LOG")
        .env_remove("POINTBREAK_FORMAT")
        .env_remove("POINTBREAK_THEME")
        .env_remove("BAT_THEME");
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().expect("run unprepared pointbreak binary")
}

/// Run `pointbreak` with extra environment variables — e.g. `POINTBREAK_ACTOR_ID` to
/// attribute a write to a specific actor.
#[allow(dead_code)]
pub fn pointbreak_env<I, S>(args: I, env: &[(&str, &str)]) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();
    let args = prepare_change_cli_fixture(args);
    let mut command = Command::new(env!("CARGO_BIN_EXE_pointbreak"));
    command
        .args(&args)
        .env_remove("POINTBREAK_LOG")
        .env_remove("RUST_LOG")
        // Clear ambient selectors first; a caller that passes POINTBREAK_FORMAT or
        // a theme variable in `env` re-sets it below and still wins.
        .env_remove("POINTBREAK_FORMAT")
        .env_remove("POINTBREAK_THEME")
        .env_remove("BAT_THEME");
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command.output().expect("run pointbreak binary");
    remember_change_capture(&args, &output);
    if output.status.success()
        && let Some(repo) = repo_argument(&args)
    {
        inspect::sync_legacy_mirrors(&repo);
    }
    output
}

/// Ordinary integration fixtures start from an empty, already-qualified L2
/// authority. Tests that exercise L0/M1 construct those states explicitly and
/// bypass this capture helper. The two frozen records were emitted by the
/// test-only capability fixture producer and contain no repository data.
#[derive(Clone)]
struct FixtureCapture {
    revision_id: String,
    change_id: String,
    artifact_hash: String,
}

#[derive(Default)]
struct FixtureChangeState {
    latest_cursor: Option<String>,
    captures: HashMap<String, FixtureCapture>,
}

fn fixture_change_state() -> &'static Mutex<HashMap<std::path::PathBuf, FixtureChangeState>> {
    static STATE: OnceLock<Mutex<HashMap<std::path::PathBuf, FixtureChangeState>>> =
        OnceLock::new();
    STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn repo_argument(args: &[OsString]) -> Option<std::path::PathBuf> {
    let index = args.iter().position(|arg| arg == "--repo")?;
    Some(std::path::PathBuf::from(args.get(index + 1)?))
}

fn prepare_change_cli_fixture(args: Vec<OsString>) -> Vec<OsString> {
    let Some(repo) = repo_argument(&args) else {
        return args;
    };
    let command = args.first().and_then(|arg| arg.to_str());
    let subcommand = args.get(1).and_then(|arg| arg.to_str());
    let migration_dry_run = command == Some("change") && subcommand == Some("migrate-dry-run");
    if !migration_dry_run {
        maybe_install_empty_ready_change_store(&repo);
    }
    args
}

fn maybe_install_empty_ready_change_store(repo_root: &Path) {
    let git = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(repo_root)
        .output();
    if !git.is_ok_and(|output| output.status.success()) {
        return;
    }
    let events = fixture_store_dir(repo_root).join("events");
    let has_authority = events.exists()
        && std::fs::read_dir(&events)
            .expect("read fixture event directory")
            .next()
            .is_some();
    if !has_authority {
        install_empty_ready_change_store(repo_root);
    }
}

fn remember_change_capture(args: &[OsString], output: &Output) {
    if args.first().and_then(|arg| arg.to_str()) != Some("capture") || !output.status.success() {
        return;
    }
    let Some(repo) = repo_argument(args) else {
        return;
    };
    let Ok(document) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return;
    };
    let revision = document["revision"]["revisionId"]
        .as_str()
        .expect("capture fixture Revision id")
        .to_owned();
    let change_id = document["changeId"]
        .as_str()
        .expect("capture fixture Change id")
        .to_owned();
    let cursor = document["reviewCursor"]["token"]
        .as_str()
        .expect("capture fixture review cursor")
        .to_owned();
    let artifact_hash = document["revision"]["objectArtifactContentHash"]
        .as_str()
        .expect("capture fixture artifact hash")
        .to_owned();
    let mut state = fixture_change_state().lock().expect("fixture state lock");
    let repo_state = state.entry(repo).or_default();
    repo_state.latest_cursor = Some(cursor.clone());
    repo_state.captures.insert(
        revision,
        FixtureCapture {
            revision_id: document["revision"]["revisionId"]
                .as_str()
                .expect("capture fixture Revision id")
                .to_owned(),
            change_id,
            artifact_hash,
        },
    );
}

fn fixture_capture<'a>(state: &'a FixtureChangeState, selector: &str) -> &'a FixtureCapture {
    if let Some(capture) = state.captures.get(selector) {
        return capture;
    }
    let mut matches = state
        .captures
        .iter()
        .filter(|(revision, _)| {
            revision.starts_with(selector)
                || revision
                    .strip_prefix("rev:sha256:")
                    .is_some_and(|digest| digest.starts_with(selector))
        })
        .map(|(_, capture)| capture);
    let capture = matches
        .next()
        .expect("superseded fixture Revision was captured through this helper");
    assert!(
        matches.next().is_none(),
        "fixture Revision selector must be unambiguous"
    );
    capture
}

fn fresh_fixture_cursor(repo: &Path, revision: &str, change_id: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_pointbreak"))
        .args([
            "change",
            "select",
            change_id,
            "--revision",
            revision,
            "--allow-historical",
            "--repo",
            repo.to_str().expect("fixture repo path is utf-8"),
        ])
        .env_remove("POINTBREAK_LOG")
        .env_remove("RUST_LOG")
        .env_remove("POINTBREAK_FORMAT")
        .output()
        .expect("select fresh fixture review cursor");
    assert!(
        output.status.success(),
        "fresh fixture cursor selection failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("review cursor selection is JSON");
    document["token"]
        .as_str()
        .expect("review cursor selection token")
        .to_owned()
}

#[allow(dead_code)]
pub fn install_empty_ready_change_store(repo_root: &Path) {
    let events = fixture_store_dir(repo_root).join("events");
    let activation =
        events.join("5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json");
    if activation.exists() {
        return;
    }
    if events.exists()
        && std::fs::read_dir(&events)
            .expect("read fixture event directory")
            .next()
            .is_some()
    {
        panic!("cannot install the empty L2 fixture over existing L0 authority");
    }
    std::fs::create_dir_all(&events).expect("create fixture event directory");
    for (name, bytes) in [
        (
            "5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json",
            include_bytes!(
                "assets/change-ready-store/5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json"
            )
            .as_slice(),
        ),
        (
            "f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json",
            include_bytes!(
                "assets/change-ready-store/f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json"
            )
            .as_slice(),
        ),
    ] {
        std::fs::write(events.join(name), bytes).expect("write frozen empty L2 fixture record");
    }
}

fn fixture_store_dir(repo_root: &Path) -> std::path::PathBuf {
    for config in [
        repo_root.join(".pointbreak/store.local.json"),
        repo_root.join(".pointbreak/store.json"),
    ] {
        let Ok(bytes) = std::fs::read(config) else {
            continue;
        };
        let Ok(document) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        if document["mode"] == "ephemeral" {
            return repo_root.join(".pointbreak/data");
        }
        break;
    }
    common_dir_store(repo_root)
}

#[allow(dead_code)]
pub fn dump_repo() -> git_repo::GitRepo {
    let repo = git_repo::GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    install_empty_ready_change_store(repo.path());
    repo
}

/// Capture two worktree states where the second supersedes the first, returning
/// the repository and both full revision ids for selector-behavior tests.
#[allow(dead_code)]
pub fn superseded_dump_repo() -> (git_repo::GitRepo, String, String) {
    let repo = dump_repo();
    let repo_arg = repo.path().to_str().expect("temporary path is utf-8");
    let first: serde_json::Value =
        serde_json::from_slice(&pointbreak(["capture", "--repo", repo_arg]).stdout)
            .expect("first capture emits JSON");
    let first_id = first["revision"]["id"]
        .as_str()
        .expect("first revision id")
        .to_owned();
    let first_cursor = first["reviewCursor"]["token"]
        .as_str()
        .expect("first review cursor")
        .to_owned();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    let second: serde_json::Value = serde_json::from_slice(
        &pointbreak([
            "capture",
            "--repo",
            repo_arg,
            "--review-cursor",
            &first_cursor,
            "--advance",
            "replace",
        ])
        .stdout,
    )
    .expect("second capture emits JSON");
    let second_id = second["revision"]["id"]
        .as_str()
        .expect("second revision id")
        .to_owned();
    (repo, first_id, second_id)
}

/// A repository with two commits (clean worktree), so `--base HEAD~1` captures
/// the committed range. Shared by the commit-range read-surface suites.
#[allow(dead_code)]
pub fn committed_repo() -> git_repo::GitRepo {
    let repo = git_repo::GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo.commit_all("change");
    repo
}

/// The shared common-dir store a clone resolves by default
/// (`<git-common-dir>/pointbreak`, i.e. `.git/pointbreak`). Every non-ephemeral worktree of
/// a clone — main and linked — reads and writes here, with no `store link`. Use
/// this for store-path assertions after a `pointbreak` write instead of the raw
/// worktree-local `.pointbreak/data`.
#[allow(dead_code)]
pub fn common_dir_store(repo_root: &Path) -> std::path::PathBuf {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo_root)
        .output()
        .expect("run git rev-parse --git-common-dir");
    assert!(
        output.status.success(),
        "git rev-parse --git-common-dir failed in {}",
        repo_root.display()
    );
    let common_dir = String::from_utf8(output.stdout)
        .expect("git-common-dir is utf-8")
        .trim()
        .to_owned();
    Path::new(&common_dir).join("pointbreak")
}

/// Append a model-valid provenance-free revision that reuses an already stored
/// object artifact. This is a read-surface fixture only: production capture is
/// intentionally Git-backed, while longitudinal/generated producers may propose
/// revisions without Git coordinates.
#[allow(dead_code)]
pub fn append_provenance_free_revision(
    repo_root: &Path,
    object_id: &str,
    object_artifact_content_hash: &str,
) -> String {
    use pointbreak::model::{
        EngagementId, JournalId, ObjectId, ReviewTargetRef, RevisionId, TargetRef,
    };
    use pointbreak::session::event::{
        EventTarget, EventType, Revision, ShoreEvent, WorkObjectProposal,
        WorkObjectProposedPayload, Writer,
    };
    use sha2::{Digest, Sha256};

    let revision_id = RevisionId::new(format!("rev:sha256:{}", "15".repeat(32)));
    let target = TargetRef::Review(ReviewTargetRef::Revision {
        revision_id: revision_id.clone(),
    });
    let payload = WorkObjectProposedPayload {
        engagement_id: EngagementId::new(format!("engagement:sha256:{}", "16".repeat(32))),
        work_object: WorkObjectProposal::Revision {
            revision: Revision {
                id: revision_id.clone(),
                object_id: ObjectId::new(object_id),
                git_provenance: None,
            },
            summary: Some("generated revision".to_owned()),
            object_artifact_content_hash: object_artifact_content_hash.to_owned(),
            supersedes: Vec::new(),
        },
    };
    let idempotency_key = format!("fixture:provenance-free:{}", revision_id.as_str());
    let event = ShoreEvent::new(
        EventType::WorkObjectProposed,
        &idempotency_key,
        EventTarget::for_generative_move(
            JournalId::new("journal:default"),
            pointbreak::model::EngagementType::Review,
            target,
            None,
        )
        .expect("fixture event target"),
        Writer::shore_local("test"),
        payload,
        "2026-07-24T00:00:00Z",
    )
    .expect("fixture event");

    let events_dir = common_dir_store(repo_root).join("events");
    std::fs::create_dir_all(&events_dir).expect("create fixture events directory");
    let stem = Sha256::digest(idempotency_key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    std::fs::write(
        events_dir.join(format!("{stem}.json")),
        serde_json::to_vec(&event).expect("serialize fixture event"),
    )
    .expect("write fixture event");

    revision_id.as_str().to_owned()
}

#[track_caller]
#[allow(dead_code)]
pub fn assert_existing_paths_eq(actual: &Path, expected: &Path) {
    assert_eq!(
        actual.canonicalize().expect("canonicalize actual path"),
        expected.canonicalize().expect("canonicalize expected path")
    );
}
