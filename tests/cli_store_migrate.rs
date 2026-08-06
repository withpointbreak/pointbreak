mod support;

use support::git_repo::GitRepo;
use support::{install_empty_ready_change_store, pointbreak, pointbreak_unprepared};

fn repo() -> GitRepo {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo
}

#[test]
fn store_migrate_refuses_l0_with_typed_change_migration_guidance() {
    let repo = repo();
    let output =
        pointbreak_unprepared(["store", "migrate", "--repo", repo.path().to_str().unwrap()]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("migration_required"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn store_migrate_refuses_l2_until_exact_transfer_is_activated_without_mutation() {
    let repo = repo();
    install_empty_ready_change_store(repo.path());
    let marker = repo.path().join(".pointbreak/data/keep.txt");
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(&marker, b"keep").unwrap();

    let output = pointbreak([
        "store",
        "migrate",
        "--retire-source",
        "--repo",
        repo.path().to_str().unwrap(),
    ]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("change_store_transfer_unavailable"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(marker).unwrap(), b"keep");
}
