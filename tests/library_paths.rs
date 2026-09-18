mod support;

use std::process::Command;

use support::assert_existing_paths_eq;
use support::git_repo::GitRepo;

#[test]
fn repository_and_common_dir_objects_own_canonical_paths() {
    let repo = GitRepo::new();
    let repository = pointbreak::paths::RepositoryPaths::resolve(repo.path()).unwrap();
    let common = pointbreak::paths::CommonDirPaths::resolve(repo.path()).unwrap();
    let root = repository.worktree_root();

    assert_existing_paths_eq(root, repo.path());
    assert_eq!(repository.config_dir(), root.join(".pointbreak"));
    assert_eq!(repository.worktree_store(), root.join(".pointbreak/data"));
    assert_eq!(
        repository.allowed_signers(),
        root.join(".pointbreak/allowed-signers.json")
    );
    assert_eq!(repository.gitignore(), root.join(".pointbreak/.gitignore"));
    assert_eq!(
        repository.store_config(),
        root.join(".pointbreak/store.json")
    );
    assert_eq!(
        repository.store_config_local(),
        root.join(".pointbreak/store.local.json")
    );
    assert_eq!(
        repository.delegates(),
        root.join(".pointbreak/delegates.json")
    );
    assert_eq!(
        repository.delegates_local(),
        root.join(".pointbreak/delegates.local.json")
    );
    assert_eq!(
        repository.actor_attributes(),
        root.join(".pointbreak/actor-attributes.json")
    );
    assert_eq!(
        repository.actor_attributes_local(),
        root.join(".pointbreak/actor-attributes.local.json")
    );
    assert_eq!(
        repository.sensitivity(),
        root.join(".pointbreak/sensitivity.json")
    );
    assert_eq!(
        repository.sensitivity_local(),
        root.join(".pointbreak/sensitivity.local.json")
    );
    assert_existing_paths_eq(common.common_dir(), &repo.path().join(".git"));
    assert_eq!(common.store_dir(), common.common_dir().join("pointbreak"));
    assert_eq!(
        common.binding(),
        common.common_dir().join("pointbreak.link.json")
    );
}

#[test]
fn linked_worktrees_share_common_paths_but_keep_worktree_paths_distinct() {
    let repo = GitRepo::new();
    repo.write("tracked.txt", "base\n");
    repo.commit_all("base");
    let parent = tempfile::tempdir().unwrap();
    let linked = parent.path().join("linked");
    let output = Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(&linked)
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let main_repository = pointbreak::paths::RepositoryPaths::resolve(repo.path()).unwrap();
    let linked_repository = pointbreak::paths::RepositoryPaths::resolve(&linked).unwrap();
    let main_common = pointbreak::paths::CommonDirPaths::resolve(repo.path()).unwrap();
    let linked_common = pointbreak::paths::CommonDirPaths::resolve(&linked).unwrap();

    assert_ne!(main_repository.config_dir(), linked_repository.config_dir());
    assert_eq!(main_common, linked_common);
    assert_eq!(main_common.store_dir(), linked_common.store_dir());
    assert_eq!(main_common.binding(), linked_common.binding());
}

#[test]
fn separate_git_dir_is_the_common_path_authority() {
    let root = tempfile::tempdir().unwrap();
    let worktree = root.path().join("checkout");
    let metadata = root.path().join("metadata");
    std::fs::create_dir_all(&worktree).unwrap();
    let output = Command::new("git")
        .arg("init")
        .arg("--separate-git-dir")
        .arg(&metadata)
        .arg(&worktree)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let repository = pointbreak::paths::RepositoryPaths::resolve(&worktree).unwrap();
    let common = pointbreak::paths::CommonDirPaths::resolve(&worktree).unwrap();
    assert_existing_paths_eq(repository.worktree_root(), &worktree);
    assert_eq!(
        repository.config_dir(),
        repository.worktree_root().join(".pointbreak")
    );
    assert_existing_paths_eq(common.common_dir(), &metadata);
    assert_eq!(common.store_dir(), common.common_dir().join("pointbreak"));
}

#[test]
fn bare_repo_exposes_common_paths_and_has_no_repository_paths() {
    let bare = tempfile::tempdir().unwrap();
    let output = Command::new("git")
        .args(["init", "--bare"])
        .current_dir(bare.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    assert!(pointbreak::paths::RepositoryPaths::resolve(bare.path()).is_err());
    let common = pointbreak::paths::CommonDirPaths::resolve(bare.path()).unwrap();
    assert_existing_paths_eq(common.common_dir(), bare.path());
    assert_eq!(common.store_dir(), common.common_dir().join("pointbreak"));
    assert_eq!(
        common.binding(),
        common.common_dir().join("pointbreak.link.json")
    );
}

#[test]
fn canonical_layout_presence_and_old_layouts_do_not_redirect_paths() {
    let repo = GitRepo::new();
    for path in [
        repo.path().join(".pointbreak/delegates.json"),
        repo.path().join(".pointbreak/data/events/event.json"),
        repo.path().join(".git/pointbreak/events/event.json"),
        repo.path().join(".git/pointbreak.link.json"),
        repo.path().join(".shore/data/events/old.json"),
        repo.path().join(".git/shore/events/old.json"),
        repo.path().join(".git/shore.link.json"),
    ] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "{}\n").unwrap();
    }

    let repository = pointbreak::paths::RepositoryPaths::resolve(repo.path()).unwrap();
    let common = pointbreak::paths::CommonDirPaths::resolve(repo.path()).unwrap();
    assert_existing_paths_eq(repository.worktree_root(), repo.path());
    assert_existing_paths_eq(common.common_dir(), &repo.path().join(".git"));
    assert_eq!(
        repository.worktree_store(),
        repository.worktree_root().join(".pointbreak/data")
    );
    assert_eq!(common.store_dir(), common.common_dir().join("pointbreak"));
    assert_eq!(
        common.binding(),
        common.common_dir().join("pointbreak.link.json")
    );
}

#[test]
fn supported_allowed_signers_seam_matches_repository_authority() {
    let repo = GitRepo::new();
    let authority = pointbreak::paths::RepositoryPaths::resolve(repo.path()).unwrap();
    assert_eq!(
        pointbreak::session::allowed_signers_path_for_repo(repo.path()).unwrap(),
        authority.allowed_signers()
    );
}
