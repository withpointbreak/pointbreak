use shore::git::{git_worktree_root, ingest_tracked_diff};
use shore::session::{capture_worktree_fingerprint, ensure_shore_ignored, shore_dir_for_repo};

use crate::support::git_repo::GitRepo;

#[test]
fn shore_dir_resolves_to_git_worktree_root_from_subdirectory() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn demo() {}\n");
    let subdir = repo.path().join("src");

    let root = git_worktree_root(&subdir).expect("git root resolves");
    let shore_dir = shore_dir_for_repo(&subdir).expect("shore dir resolves");

    let expected_root = repo.path().canonicalize().expect("canonical repo root");
    assert_eq!(root, expected_root);
    assert_eq!(shore_dir, expected_root.join(".shore"));
}

#[test]
fn ensure_shore_ignored_creates_or_updates_root_gitignore_without_duplicates() {
    let repo = GitRepo::new();

    ensure_shore_ignored(repo.path()).expect("ignore entry is written");
    ensure_shore_ignored(repo.path()).expect("ignore entry is idempotent");

    let gitignore = repo.read(".gitignore");
    assert_eq!(
        gitignore
            .lines()
            .filter(|line| line.trim_end() == ".shore/")
            .count(),
        1
    );
}

#[test]
fn ensure_shore_ignored_appends_to_existing_gitignore_with_separator_newline() {
    let repo = GitRepo::new();
    repo.write(".gitignore", "target/\n!.keep");

    ensure_shore_ignored(repo.path()).expect("ignore entry is appended");

    assert_eq!(repo.read(".gitignore"), "target/\n!.keep\n.shore/\n");
}

#[test]
fn ensure_shore_ignored_treats_bare_shore_entry_as_existing_ignore() {
    let repo = GitRepo::new();
    repo.write(
        ".gitignore",
        "# .shore/ is intentionally ignored below\n.shore\n",
    );

    ensure_shore_ignored(repo.path()).expect("bare ignore entry is recognized");

    assert_eq!(
        repo.read(".gitignore"),
        "# .shore/ is intentionally ignored below\n.shore\n"
    );
}

#[test]
fn nested_git_repo_uses_its_own_worktree_root() {
    let outer = GitRepo::new();
    outer.write("nested/.keep", "");
    let nested = outer.path().join("nested");
    GitRepo::init_at(&nested);

    assert_eq!(
        git_worktree_root(&nested).unwrap(),
        nested.canonicalize().expect("canonical nested repo")
    );
}

#[test]
fn same_working_tree_diff_produces_same_revision_and_snapshot_ids() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");

    let first = capture_worktree_fingerprint(repo.path()).expect("first capture");
    let second = capture_worktree_fingerprint(repo.path()).expect("second capture");

    assert_eq!(first.revision_id, second.revision_id);
    assert_eq!(first.snapshot_id, second.snapshot_id);
    assert!(
        first
            .revision_id
            .as_str()
            .starts_with("rev:worktree:sha256:")
    );
    assert!(first.snapshot_id.as_str().starts_with("snap:git:sha256:"));
}

#[test]
fn sidecar_input_does_not_affect_revision_fingerprint() {
    let repo = modified_repo();
    ensure_shore_ignored(repo.path()).expect("ignore shore state");

    let before = capture_worktree_fingerprint(repo.path()).expect("capture before shore state");
    repo.write(".shore/state.json", "changed notes");
    let after = capture_worktree_fingerprint(repo.path()).expect("capture after shore state");

    assert_eq!(before.revision_id, after.revision_id);
    assert_eq!(before.snapshot_id, after.snapshot_id);
}

#[test]
fn tracked_and_untracked_content_changes_change_revision_id() {
    let repo = modified_repo();
    let before = capture_worktree_fingerprint(repo.path()).expect("capture before untracked");

    repo.write("untracked.rs", "pub fn new() {}\n");
    let after_untracked =
        capture_worktree_fingerprint(repo.path()).expect("capture after untracked");
    assert_ne!(before.revision_id, after_untracked.revision_id);
    assert_ne!(before.snapshot_id, after_untracked.snapshot_id);

    repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    let after_tracked = capture_worktree_fingerprint(repo.path()).expect("capture after tracked");
    assert_ne!(after_untracked.revision_id, after_tracked.revision_id);
    assert_ne!(after_untracked.snapshot_id, after_tracked.snapshot_id);
}

#[test]
fn git_ingestion_uses_content_derived_snapshot_id() {
    let repo = modified_repo();

    let fingerprint = capture_worktree_fingerprint(repo.path()).expect("capture fingerprint");
    let snapshot = ingest_tracked_diff(repo.path()).expect("ingest snapshot");

    assert_eq!(snapshot.snapshot_id, fingerprint.snapshot_id);
}

fn modified_repo() -> GitRepo {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo
}
