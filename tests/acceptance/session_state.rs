use shore::git::git_worktree_root;
use shore::session::{ensure_shore_ignored, shore_dir_for_repo};

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
