//! Isolation contract of the shared integration harness in `tests/support`.
//!
//! Each test here installs an ambient `POINTBREAK_HOME` and an agent
//! `POINTBREAK_ACTOR_ID` into the test process itself, exactly as an agent
//! session or a developer shell would have them exported, and then drives the
//! binary through the default helpers. The ambient home must stay empty and no
//! write may be attributed to, or signed as, the ambient actor: what a test
//! writes must not depend on who runs it.

mod support;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::Value;
use support::git_repo::GitRepo;
use support::{common_dir_store, harness_home, pointbreak, pointbreak_env};

const AMBIENT_ACTOR: &str = "actor:agent:probe";

/// Install the ambient home and actor id into this process once, before any
/// helper spawns the binary, and return the ambient home.
fn ambient_home() -> &'static Path {
    static AMBIENT: OnceLock<tempfile::TempDir> = OnceLock::new();
    AMBIENT
        .get_or_init(|| {
            let home = tempfile::tempdir().expect("create ambient scratch home");
            // SAFETY: cargo-nextest runs this test in its own process. Under plain
            // `cargo test` every test in this binary funnels through this `OnceLock`
            // before it spawns anything, so the two variables are written exactly
            // once, before any concurrent reader exists in the process.
            unsafe {
                std::env::set_var("POINTBREAK_HOME", home.path());
                std::env::set_var("POINTBREAK_ACTOR_ID", AMBIENT_ACTOR);
            }
            home
        })
        .path()
}

fn dirty_repo() -> GitRepo {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|entry| entry.expect("read directory entry").path())
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("read {}: {error}", dir.display()),
    }
}

#[track_caller]
fn assert_ambient_home_untouched() {
    let leaked = entries(ambient_home());
    assert!(
        leaked.is_empty(),
        "the default harness reached the ambient POINTBREAK_HOME: {leaked:?}"
    );
}

fn stored_events(repo: &GitRepo) -> Vec<Value> {
    let mut events = entries(&common_dir_store(repo.path()).join("events"))
        .into_iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| {
            let bytes = std::fs::read(&path).expect("read stored event");
            serde_json::from_slice::<Value>(&bytes).expect("stored event is JSON")
        })
        .collect::<Vec<_>>();
    events.sort_by_key(|event| event["recordedAt"].as_str().map(str::to_owned));
    events
}

/// The reproduced case from the issue: one capture through the default helper,
/// with an agent identity and a scratch home exported in the calling process.
#[test]
fn default_helper_capture_leaves_the_ambient_home_empty() {
    let ambient = ambient_home();
    let repo = dirty_repo();
    let repo_path = repo.path().to_str().expect("utf-8 repo path");

    let output = pointbreak(["capture", "--repo", repo_path]);
    assert!(
        output.status.success(),
        "capture failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_ambient_home_untouched();
    assert!(
        entries(&ambient.join("keys")).is_empty(),
        "no signing key may be minted under the ambient home"
    );

    // The store also holds the two frozen, pre-signed ready-Change fixture records
    // the harness installs, written by `actor:qualification`; the capture's own
    // events are the ones attributed to the fixture repository's git identity.
    let events = stored_events(&repo);
    let captured = events
        .iter()
        .filter(|event| {
            event["writer"]["actorId"]
                .as_str()
                .is_some_and(|actor| actor.starts_with("actor:git-"))
        })
        .collect::<Vec<_>>();
    assert!(
        !captured.is_empty(),
        "the capture is attributed to the fixture's git identity, got writers {:?}",
        events
            .iter()
            .map(|event| event["writer"]["actorId"].clone())
            .collect::<Vec<_>>()
    );
    for event in &captured {
        assert!(
            event["signature"].is_null(),
            "a default-helper write was signed with an ambient key: {}",
            event["signature"]
        );
    }
    for event in &events {
        assert_ne!(
            event["writer"]["actorId"], AMBIENT_ACTOR,
            "a default-helper write was attributed to the ambient actor id"
        );
    }
}

/// A test that wants an agent identity names it through `pointbreak_env`. The
/// key that identity mints lands in the harness home, never the ambient one.
#[test]
fn agent_identity_opt_in_mints_its_key_in_the_harness_home() {
    ambient_home();
    let repo = dirty_repo();
    let repo_path = repo.path().to_str().expect("utf-8 repo path");

    let output = pointbreak_env(
        ["capture", "--repo", repo_path],
        &[("POINTBREAK_ACTOR_ID", "actor:agent:opted-in")],
    );
    assert!(
        output.status.success(),
        "capture failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("generated signing key for actor:agent:opted-in"),
        "the first agent write under a fresh harness home mints a key, got:\n{stderr}"
    );

    assert_ambient_home_untouched();
    let minted = harness_home().join("keys").join("agent-opted-in");
    assert!(
        minted.is_file(),
        "the opted-in agent key lives under the harness home at {}",
        minted.display()
    );

    let events = stored_events(&repo);
    assert!(
        events
            .iter()
            .any(|event| event["writer"]["actorId"] == "actor:agent:opted-in"),
        "the opted-in identity is the one attributed"
    );
    assert!(
        events
            .iter()
            .all(|event| event["writer"]["actorId"] != AMBIENT_ACTOR),
        "the ambient actor id never reaches a write"
    );
}

/// The harness home lives for the whole test: fixture preparation, the write
/// under test and every later command share one directory. An opted-in agent
/// identity mints its key on the first write and reuses it, silently, on the
/// next, so both writes carry the same signer.
#[test]
fn harness_home_is_shared_by_every_command_in_one_test() {
    ambient_home();
    let repo = dirty_repo();
    let repo_path = repo.path().to_str().expect("utf-8 repo path");
    let env = [("POINTBREAK_ACTOR_ID", "actor:agent:lifetime")];

    let first = pointbreak_env(["capture", "--repo", repo_path], &env);
    assert!(
        first.status.success(),
        "first capture failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        String::from_utf8_lossy(&first.stderr)
            .contains("generated signing key for actor:agent:lifetime"),
        "the first write under the fresh harness home mints the key"
    );
    let minted = harness_home().join("keys").join("agent-lifetime");
    assert!(minted.is_file(), "key minted at {}", minted.display());

    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    let second = pointbreak_env(["capture", "--repo", repo_path], &env);
    assert!(
        second.status.success(),
        "second capture failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&second.stderr).contains("generated signing key"),
        "the second write reuses the key from the same harness home, got:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        entries(&harness_home().join("keys"))
            .into_iter()
            .filter(|path| path
                .file_name()
                .is_some_and(|name| name == "agent-lifetime"))
            .count(),
        1,
        "exactly one key for the identity across both writes"
    );

    let signatures = stored_events(&repo)
        .into_iter()
        .filter(|event| event["writer"]["actorId"] == "actor:agent:lifetime")
        .map(|event| event["signature"].clone())
        .collect::<Vec<_>>();
    assert!(
        signatures.len() >= 2 && signatures.iter().all(|signature| !signature.is_null()),
        "both writes are signed, got {signatures:?}"
    );
    assert_ambient_home_untouched();
}

/// An explicit `POINTBREAK_HOME` passed through `pointbreak_env` still wins over
/// the harness default, so tests that assert on home contents keep working.
#[test]
fn explicit_home_opt_in_wins_over_the_harness_home() {
    ambient_home();
    let explicit = tempfile::tempdir().expect("explicit scratch home");
    let explicit_path = explicit.path().to_str().expect("utf-8 home path");
    let repo = dirty_repo();
    let repo_path = repo.path().to_str().expect("utf-8 repo path");

    let output = pointbreak_env(
        ["capture", "--repo", repo_path],
        &[
            ("POINTBREAK_HOME", explicit_path),
            ("POINTBREAK_ACTOR_ID", "actor:agent:explicit-home"),
        ],
    );
    assert!(
        output.status.success(),
        "capture failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_ambient_home_untouched();
    assert!(
        explicit.path().join("keys/agent-explicit-home").is_file(),
        "the key is minted under the explicit home"
    );
    assert!(
        !harness_home().join("keys/agent-explicit-home").exists(),
        "the harness home is untouched when an explicit home is passed"
    );
}
