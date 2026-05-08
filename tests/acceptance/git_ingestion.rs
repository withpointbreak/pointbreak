use crate::support::git_repo::GitRepo;
use crate::support::snapshots::normalize_path;

#[test]
fn scratch_repo_can_create_commit_modify_and_report_status() {
    let repo = GitRepo::new();

    repo.write("src/lib.rs", "pub fn original() {}\n");
    repo.commit_all("initial commit");
    repo.write("src/lib.rs", "pub fn changed() {}\n");
    repo.write("obsolete.rs", "remove me\n");
    repo.remove("obsolete.rs");

    let status = repo.git(["status", "--porcelain=v2"]);

    assert!(repo.path().join("src/lib.rs").exists());
    assert!(!repo.path().join("obsolete.rs").exists());
    assert!(!normalize_path(repo.path()).is_empty());
    assert!(
        status.stderr.is_empty(),
        "status command should not write stderr:\n{}",
        status.stderr
    );
    assert!(
        status.stdout.contains("src/lib.rs"),
        "status output should mention modified file:\n{}",
        status.stdout
    );
}
