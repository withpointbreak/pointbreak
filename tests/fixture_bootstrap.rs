mod support;

use support::git_repo::{GitRepo, git_spawn_count, reset_git_spawn_count};

#[test]
fn new_repo_bootstraps_with_zero_git_spawns() {
    reset_git_spawn_count();
    let repo = GitRepo::new();
    assert_eq!(git_spawn_count(), 0, "bootstrap must not spawn git");

    // The scaffold is still a usable repo on the deterministic default branch,
    // with the fixture identity baked in.
    assert_eq!(
        repo.git(["symbolic-ref", "HEAD"]).stdout.trim(),
        "refs/heads/main"
    );
    assert_eq!(
        repo.git(["config", "user.email"]).stdout.trim(),
        "shore-tests@example.com"
    );

    // Real commits still land (these spawn git; that is expected after new()).
    repo.write("f.txt", "x\n");
    repo.commit_all("c");
    assert_eq!(
        repo.git(["rev-parse", "--abbrev-ref", "HEAD"])
            .stdout
            .trim(),
        "main"
    );
}

#[test]
fn init_at_bootstraps_with_zero_git_spawns() {
    reset_git_spawn_count();
    let outer = GitRepo::new();
    let nested = outer.path().join("nested");
    GitRepo::init_at(&nested);
    assert_eq!(git_spawn_count(), 0, "nested bootstrap must not spawn git");

    // The nested repo carries the deterministic default branch without depending
    // on the host's init.defaultBranch.
    let head = std::fs::read_to_string(nested.join(".git/HEAD")).expect("read nested HEAD");
    assert_eq!(head.trim(), "ref: refs/heads/main");
}

#[test]
fn inside_work_tree_probe_is_memoized_per_path() {
    let repo = GitRepo::new();

    let first = support::inside_work_tree(repo.path());
    assert!(first, "fixture scaffold is a work tree");
    let spawns_after_first = support::work_tree_probe_spawns();

    let second = support::inside_work_tree(repo.path());
    assert_eq!(first, second);
    assert_eq!(
        support::work_tree_probe_spawns(),
        spawns_after_first,
        "repeat probe of the same path must not spawn git again"
    );
}
