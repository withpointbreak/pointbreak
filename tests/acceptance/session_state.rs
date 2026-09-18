use pointbreak::git::{git_worktree_root, ingest_tracked_diff};
use pointbreak::session::event::EventType;
use pointbreak::session::{
    CaptureOptions, SessionState, capture_worktree_fingerprint, capture_worktree_review,
    ensure_pointbreak_gitignore, read_events, read_object_artifact, store_dir_for_repo,
};

use crate::support::git_repo::GitRepo;
use crate::support::{assert_existing_paths_eq, common_dir_store};

#[test]
fn store_dir_resolves_to_git_worktree_root_from_subdirectory() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn demo() {}\n");
    let subdir = repo.path().join("src");

    let root = git_worktree_root(&subdir).expect("git root resolves");
    let store_dir = store_dir_for_repo(&subdir).expect("store dir resolves");

    assert_existing_paths_eq(&root, repo.path());
    // From a subdirectory, the public helper resolves the repo's shared common-dir
    // store (`.git/pointbreak`), the same store the read/write seams use.
    assert_eq!(path_file_name(&store_dir), "pointbreak");
    assert_eq!(path_file_name(path_parent(&store_dir)), ".git");
    assert_existing_paths_eq(path_parent(path_parent(&store_dir)), repo.path());
}

#[test]
fn ensure_pointbreak_gitignore_writes_the_pointbreak_scoped_ignore_file() {
    let repo = GitRepo::new();

    ensure_pointbreak_gitignore(repo.path()).expect("gitignore is written");
    ensure_pointbreak_gitignore(repo.path()).expect("gitignore write is idempotent");

    // The committed .pointbreak/.gitignore carries the canonical body, even across repeats.
    assert_eq!(
        repo.read(".pointbreak/.gitignore"),
        "data/\n*.local.json\n",
        "each line is written at most once"
    );
    // No root .gitignore is created, and nothing lands in the hidden local exclude.
    assert!(
        !repo.path().join(".gitignore").exists(),
        "ensure must not create a root .gitignore"
    );
    assert!(
        !read_local_exclude(&repo)
            .lines()
            .any(|line| line.trim().contains(".pointbreak")),
        "ensure must not write .git/info/exclude"
    );
    // The generated file is deliberately VISIBLE — it is a repo file the user
    // commits — so it is the only working-tree entry.
    let status = repo.git(["status", "--short"]).stdout;
    assert_eq!(
        status.trim(),
        "?? .pointbreak/",
        "the generated .pointbreak/.gitignore is the only untracked entry"
    );
    // `.pointbreak/data/` is now effectively ignored.
    assert!(shore_is_ignored(&repo));
}

#[test]
fn ensure_pointbreak_gitignore_leaves_tracked_root_gitignore_untouched() {
    let repo = GitRepo::new();
    repo.write(".gitignore", "target/\n");
    repo.commit_all("add gitignore");

    ensure_pointbreak_gitignore(repo.path()).expect("gitignore is written");

    // The tracked root .gitignore is never rewritten.
    assert_eq!(repo.read(".gitignore"), "target/\n");
    // The .pointbreak-scoped file carries the exclusions instead, and they work.
    assert_eq!(repo.read(".pointbreak/.gitignore"), "data/\n*.local.json\n");
    assert!(shore_is_ignored(&repo));
}

#[test]
fn ensure_pointbreak_gitignore_is_noop_when_ignores_are_already_covered() {
    let repo = GitRepo::new();
    repo.write(
        ".gitignore",
        "# pointbreak paths are intentionally ignored below\n.pointbreak/data\n.pointbreak/*.local.json\n",
    );
    repo.commit_all("ignore pointbreak paths in gitignore");

    ensure_pointbreak_gitignore(repo.path()).expect("existing ignore is respected");

    // The user's .gitignore choice is respected: no generated file, no local entry.
    assert!(
        !repo.path().join(".pointbreak/.gitignore").exists(),
        "must not generate a redundant .pointbreak/.gitignore"
    );
    assert!(
        !read_local_exclude(&repo)
            .lines()
            .any(|line| line.trim().contains(".pointbreak")),
        "must not add a redundant local exclude entry"
    );
}

#[test]
fn ensure_pointbreak_gitignore_is_noop_against_legacy_local_exclude_entries() {
    let repo = GitRepo::new();
    // A pre-existing clone carries the entries the retired mechanism wrote to the
    // repo-local exclude; they still count as coverage, so nothing new is written.
    let exclude_path = repo.path().join(".git/info/exclude");
    std::fs::write(
        &exclude_path,
        "# local excludes\n.pointbreak/delegates.local.json\n.pointbreak/actor-attributes.local.json\n\
         .pointbreak/store.local.json\n.pointbreak/data/\n",
    )
    .expect("seed local exclude");

    ensure_pointbreak_gitignore(repo.path()).expect("existing local exclude is respected");

    assert!(
        !repo.path().join(".pointbreak/.gitignore").exists(),
        "legacy narrow exclude entries already cover the probes"
    );
    assert_eq!(
        read_local_exclude(&repo),
        "# local excludes\n.pointbreak/delegates.local.json\n.pointbreak/actor-attributes.local.json\n\
         .pointbreak/store.local.json\n.pointbreak/data/\n",
        "the legacy exclude body is never rewritten"
    );
}

#[test]
fn read_events_uses_worktree_store_dir_from_subdirectory() {
    let repo = modified_repo();
    capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();

    let events = read_events(repo.path().join("src")).unwrap();

    assert!(!events.is_empty());
}

#[test]
fn nested_git_repo_uses_its_own_worktree_root() {
    let outer = GitRepo::new();
    outer.write("nested/.keep", "");
    let nested = outer.path().join("nested");
    GitRepo::init_at(&nested);

    assert_existing_paths_eq(&git_worktree_root(&nested).unwrap(), &nested);
}

fn path_parent(path: &std::path::Path) -> &std::path::Path {
    path.parent().expect("path has parent")
}

fn path_file_name(path: &std::path::Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .expect("path has utf-8 file name")
}

#[test]
fn same_working_tree_diff_produces_same_revision_and_snapshot_ids() {
    let repo = modified_repo();

    let first = capture_worktree_fingerprint(repo.path()).expect("first capture");
    let second = capture_worktree_fingerprint(repo.path()).expect("second capture");

    assert_eq!(first.revision_id, second.revision_id);
    assert_eq!(first.object_id, second.object_id);
    assert!(
        first
            .revision_id
            .as_str()
            .starts_with("rev:worktree:sha256:")
    );
    assert!(first.object_id.as_str().starts_with("obj:git:sha256:"));
}

#[test]
fn shore_state_does_not_affect_revision_fingerprint() {
    let repo = modified_repo();
    ensure_pointbreak_gitignore(repo.path()).expect("ignore pointbreak state");

    let before =
        capture_worktree_fingerprint(repo.path()).expect("capture before pointbreak state");
    repo.write(".pointbreak/data/state.json", "changed notes");
    let after = capture_worktree_fingerprint(repo.path()).expect("capture after pointbreak state");

    assert_eq!(before.revision_id, after.revision_id);
    assert_eq!(before.object_id, after.object_id);
}

#[test]
fn tracked_and_untracked_content_changes_change_revision_id() {
    let repo = modified_repo();
    let before = capture_worktree_fingerprint(repo.path()).expect("capture before untracked");

    repo.write("untracked.rs", "pub fn new() {}\n");
    let after_untracked =
        capture_worktree_fingerprint(repo.path()).expect("capture after untracked");
    assert_ne!(before.revision_id, after_untracked.revision_id);
    assert_ne!(before.object_id, after_untracked.object_id);

    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    let after_tracked = capture_worktree_fingerprint(repo.path()).expect("capture after tracked");
    assert_ne!(after_untracked.revision_id, after_tracked.revision_id);
    assert_ne!(after_untracked.object_id, after_tracked.object_id);
}

#[test]
fn git_ingestion_uses_content_derived_snapshot_id() {
    let repo = modified_repo();

    let fingerprint = capture_worktree_fingerprint(repo.path()).expect("capture fingerprint");
    let snapshot = ingest_tracked_diff(repo.path()).expect("ingest snapshot");

    assert_eq!(snapshot.object_id, fingerprint.object_id);
}

#[test]
fn first_capture_creates_shore_store_events_artifacts_and_no_state_projection() {
    let repo = modified_repo();

    let result =
        capture_worktree_review(CaptureOptions::new(repo.path())).expect("capture succeeds");

    let store = common_dir_store(repo.path());
    assert!(store.join("events").is_dir());
    assert!(store.join("artifacts/objects").is_dir());
    assert!(
        !store.join("state.json").exists(),
        "no write creates a store-root state projection"
    );
    // The shared store lives inside .git/, which git already ignores, so a
    // shared-store capture writes NO ignore entries anywhere: no generated
    // .pointbreak/.gitignore, nothing in the repo-local exclude, no root .gitignore.
    assert!(
        !repo.path().join(".pointbreak/.gitignore").exists(),
        "a shared-store capture generates no .pointbreak/.gitignore"
    );
    assert!(
        !read_local_exclude(&repo)
            .lines()
            .any(|line| line.trim().contains(".pointbreak")),
        "capture must not write .git/info/exclude"
    );
    assert!(
        !repo.path().join(".gitignore").exists(),
        "capture must not create a root .gitignore"
    );
    assert_eq!(result.events_created_by_type["work_object_proposed"], 1);

    let state = SessionState::from_events(&read_events(repo.path()).expect("events list"))
        .expect("state folds from events");
    assert_eq!(state.current_revision_id, Some(result.revision_id));
    assert_eq!(state.revision_count, 1);
    assert_eq!(state.event_count, 2);
}

#[test]
fn capture_does_not_dirty_worktree_or_leak_storage_into_snapshot() {
    let repo = GitRepo::new();
    repo.write("src.txt", "alpha\n");
    repo.commit_all("base");

    // The worktree is clean before any Pointbreak command runs.
    assert!(
        repo.git(["status", "--short"]).stdout.trim().is_empty(),
        "worktree should start clean"
    );

    capture_worktree_review(CaptureOptions::new(repo.path()).with_allow_empty())
        .expect("capture succeeds");

    // A shared-store capture must never mutate the worktree it is capturing:
    // no generated .pointbreak/.gitignore (the shared store lives inside .git/),
    // no root .gitignore, nothing in git status. Mutating here would fork the
    // content-only object id between a worktree capture and a range capture
    // of identical content.
    assert!(
        !repo.path().join(".gitignore").exists(),
        "capture must not create a root .gitignore"
    );
    let status = repo.git(["status", "--short"]).stdout;
    assert!(
        status.trim().is_empty(),
        "capture must keep the worktree clean, got:\n{status}"
    );

    // The captured snapshot carries no Pointbreak storage or ignore-file rows.
    let snapshot = ingest_tracked_diff(repo.path()).expect("ingest snapshot");
    assert!(
        snapshot.files.iter().all(|file| {
            let mentions_shore_state = |path: &str| {
                path == ".gitignore" || path == ".pointbreak" || path.starts_with(".pointbreak/")
            };
            !file.new_path.as_deref().is_some_and(mentions_shore_state)
                && !file.old_path.as_deref().is_some_and(mentions_shore_state)
        }),
        "snapshot must not include Pointbreak storage or .gitignore rows, got: {:?}",
        snapshot.files
    );
}

#[test]
fn capture_unchanged_worktree_is_idempotent() {
    let repo = modified_repo();

    let first = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
    let second = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();

    assert_eq!(first.revision_id, second.revision_id);
    assert_eq!(second.events_created, 0);
    assert!(second.events_existing >= 1);
}

#[test]
fn capture_writer_identity_prefers_git_config_email() {
    let repo = modified_repo();

    let result = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
    let events = read_events(repo.path()).expect("events list");
    let event = events
        .iter()
        .find(|event| {
            event.event_type == EventType::WorkObjectProposed
                && event.payload["workObject"]["revision"]["id"] == result.revision_id.as_str()
        })
        .expect("review unit event exists");

    assert_eq!(
        event.writer.actor_id.as_str(),
        "actor:git-email:shore-tests@example.com"
    );
}

#[test]
fn state_event_set_hash_changes_when_events_change() {
    let repo = modified_repo();
    capture_worktree_review(CaptureOptions::new(repo.path())).expect("capture succeeds");
    let capture_state = SessionState::from_events(&read_events(repo.path()).expect("events list"))
        .expect("capture state folds");

    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    capture_worktree_review(CaptureOptions::new(repo.path())).expect("second capture succeeds");
    let second_state = SessionState::from_events(&read_events(repo.path()).expect("events list"))
        .expect("second capture state folds");

    assert_eq!(capture_state.event_count, 2);
    assert!(capture_state.event_set_hash.is_some());
    assert_ne!(capture_state.event_set_hash, second_state.event_set_hash);
}

#[test]
fn event_store_detects_corrupted_event_payload_hash() {
    let repo = bounded_journal_repo();
    corrupt_first_event_payload(&common_dir_store(repo.path()));

    let error = read_events(repo.path()).expect_err("corrupt event is rejected");

    assert!(error.to_string().contains("payload"));
}

#[test]
fn worktree_capture_excludes_untracked_shore_generated_gitignore() {
    // #349 acceptance: on current main this worktree captured as a 2-file review; it
    // must now capture as a ONE-file review, identity unchanged from the no-file
    // capture. Exercises the public capture entry point end to end.
    let repo = modified_repo();

    // Baseline: capture the code change with no generated file present.
    let baseline = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();

    // Generate the ephemeral-mode .pointbreak/.gitignore (untracked, canonical) and recapture.
    ensure_pointbreak_gitignore(repo.path()).unwrap();
    assert_eq!(repo.read(".pointbreak/.gitignore"), "data/\n*.local.json\n");
    let with_generated = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();

    // Identity-neutral: same revision id and object id, and an idempotent recapture.
    assert_eq!(with_generated.revision_id, baseline.revision_id);
    assert_eq!(with_generated.object_id, baseline.object_id);
    assert_eq!(with_generated.events_created, 0);

    // The reviewed snapshot is the code change alone.
    let artifact = read_object_artifact(repo.path(), &with_generated.object_id).unwrap();
    let paths: Vec<&str> = artifact
        .snapshot
        .files
        .iter()
        .filter_map(|file| file.new_path.as_deref())
        .collect();
    assert_eq!(paths, vec!["src/lib.rs"]);
}

fn modified_repo() -> GitRepo {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo
}

fn bounded_journal_repo() -> GitRepo {
    let repo = modified_repo();
    capture_worktree_review(CaptureOptions::new(repo.path())).expect("capture succeeds");
    repo
}

fn read_local_exclude(repo: &GitRepo) -> String {
    std::fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap_or_default()
}

fn shore_is_ignored(repo: &GitRepo) -> bool {
    // `git check-ignore` prints the path when it is ignored and exits 1 (no
    // output) otherwise, so a non-empty stdout means storage is excluded.
    let output = std::process::Command::new("git")
        .args(["check-ignore", ".pointbreak/data/state.json"])
        .current_dir(repo.path())
        .output()
        .expect("run git check-ignore");
    !output.stdout.is_empty()
}

fn corrupt_first_event_payload(store: &std::path::Path) {
    let mut event_files = std::fs::read_dir(store.join("events"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    event_files.sort();

    let event_path = event_files.first().expect("event exists");
    let mut event: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(event_path).unwrap()).unwrap();
    event["payload"]["tampered"] = serde_json::Value::Bool(true);
    std::fs::write(event_path, serde_json::to_string_pretty(&event).unwrap()).unwrap();
}
