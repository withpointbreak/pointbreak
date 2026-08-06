mod support;

use support::git_repo::GitRepo;
use support::{
    install_empty_ready_change_store, pointbreak, pointbreak_env, pointbreak_unprepared_env,
};

fn repo() -> GitRepo {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo
}

#[test]
fn store_link_refuses_l0_before_any_placement_write() {
    let repo = repo();
    let home = tempfile::tempdir().unwrap();
    let repo_arg = repo.path().to_str().unwrap();
    let output = pointbreak_unprepared_env(
        ["store", "link", "acme", "--repo", repo_arg],
        &[("POINTBREAK_HOME", home.path().to_str().unwrap())],
    );

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("migration_required"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!repo.path().join(".git/pointbreak.link.json").exists());
    assert!(!home.path().join("stores/acme").exists());
}

#[test]
fn store_link_and_preview_refuse_l2_until_exact_transfer_is_activated() {
    let repo = repo();
    let home = tempfile::tempdir().unwrap();
    let repo_arg = repo.path().to_str().unwrap();
    let env = [("POINTBREAK_HOME", home.path().to_str().unwrap())];
    install_empty_ready_change_store(repo.path());

    for args in [
        vec!["store", "link", "acme", "--repo", repo_arg],
        vec!["store", "link", "acme", "--dry-run", "--repo", repo_arg],
    ] {
        let output = pointbreak_env(args, &env);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("change_store_transfer_unavailable"),
            "stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(!repo.path().join(".git/pointbreak.link.json").exists());
    assert!(!home.path().join("stores/acme").exists());
}

#[test]
fn store_link_without_a_slug_still_surfaces_the_syntax_error() {
    let repo = repo();
    install_empty_ready_change_store(repo.path());
    let output = pointbreak(["store", "link", "--repo", repo.path().to_str().unwrap()]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("slug"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn store_unlink_remains_an_idempotent_placement_operation() {
    let repo = repo();
    let output = pointbreak([
        "store",
        "unlink",
        "--repo",
        repo.path().to_str().unwrap(),
        "--format",
        "text",
    ]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("not linked"), "stdout:\n{stdout}");
}
