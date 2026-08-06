mod support;

use pointbreak::session::CaptureOptions;
use serde_json::Value;
use support::git_repo::GitRepo;
use support::pointbreak_unprepared_env;

#[test]
fn explicit_change_migration_requires_exact_acks_and_reaches_ready() {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    let home = tempfile::tempdir().unwrap();
    let home_value = home.path().to_str().unwrap();
    let env = [
        ("POINTBREAK_HOME", home_value),
        ("POINTBREAK_DERIVED_ACCESS", "off"),
    ];
    let key = pointbreak_unprepared_env(["key", "init", "--name", "migration"], &env);
    assert!(
        key.status.success(),
        "key init stderr: {}",
        String::from_utf8_lossy(&key.stderr)
    );
    pointbreak::session::capture_review(
        CaptureOptions::new(repo.path()).with_summary("legacy revision"),
    )
    .unwrap();

    let dry = pointbreak_unprepared_env(
        [
            "change",
            "migrate-dry-run",
            "--repo",
            repo.path().to_str().unwrap(),
        ],
        &env,
    );
    assert!(
        dry.status.success(),
        "dry run stderr: {}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let dry_json: Value = serde_json::from_slice(&dry.stdout).unwrap();
    let dry_path = home.path().join("approved-dry-run.json");
    std::fs::write(&dry_path, &dry.stdout).unwrap();
    let manifest = dry_json["manifestHash"].as_str().unwrap();
    let cohort = dry_json["roots"][0]["cohortManifestHash"].as_str().unwrap();
    let backup = home.path().join("migration-backup");

    let missing_ack = pointbreak_unprepared_env(
        [
            "change",
            "migrate",
            "--repo",
            repo.path().to_str().unwrap(),
            "--dry-run",
            dry_path.to_str().unwrap(),
            "--ack-manifest",
            manifest,
            "--ack-cohort-manifest",
            cohort,
            "--ack-minimum-reader",
            "review_change_revision_v1",
            "--backup",
            backup.to_str().unwrap(),
            "--operation-id",
            "cli-migration",
            "--sign-key",
            "migration",
        ],
        &env,
    );
    assert!(!missing_ack.status.success());
    assert!(
        String::from_utf8_lossy(&missing_ack.stderr).contains("v0.9 readers"),
        "missing-ack stderr: {}",
        String::from_utf8_lossy(&missing_ack.stderr)
    );
    assert!(!backup.exists());

    let migrated = pointbreak_unprepared_env(
        [
            "change",
            "migrate",
            "--repo",
            repo.path().to_str().unwrap(),
            "--dry-run",
            dry_path.to_str().unwrap(),
            "--ack-manifest",
            manifest,
            "--ack-cohort-manifest",
            cohort,
            "--ack-minimum-reader",
            "review_change_revision_v1",
            "--ack-v0-9-unsupported",
            "--backup",
            backup.to_str().unwrap(),
            "--operation-id",
            "cli-migration",
            "--sign-key",
            "migration",
        ],
        &env,
    );
    assert!(
        migrated.status.success(),
        "migration stderr: {}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    let receipt: Value = serde_json::from_slice(&migrated.stdout).unwrap();
    assert_eq!(receipt["disposition"], "created");
    assert_eq!(receipt["minimumReaderProfile"], "review_change_revision_v1");
    assert!(backup.join("backup-receipt.json").is_file());
    assert!(backup.join("migration-plan.json").is_file());

    let profile = pointbreak_unprepared_env(
        ["change", "profile", "--repo", repo.path().to_str().unwrap()],
        &env,
    );
    assert!(profile.status.success());
    let profile: Value = serde_json::from_slice(&profile.stdout).unwrap();
    assert_eq!(profile["availability"], "ready");

    let recovery = GitRepo::new();
    let restored = pointbreak_unprepared_env(
        [
            "change",
            "migrate-restore",
            "--backup",
            backup.to_str().unwrap(),
            "--target-repo",
            recovery.path().to_str().unwrap(),
        ],
        &env,
    );
    assert!(
        restored.status.success(),
        "restore stderr: {}",
        String::from_utf8_lossy(&restored.stderr)
    );
    let restored: Value = serde_json::from_slice(&restored.stdout).unwrap();
    assert_eq!(restored["disposition"], "created");
    let recovery_profile = pointbreak_unprepared_env(
        [
            "change",
            "profile",
            "--repo",
            recovery.path().to_str().unwrap(),
        ],
        &env,
    );
    let recovery_profile: Value = serde_json::from_slice(&recovery_profile.stdout).unwrap();
    assert_eq!(recovery_profile["availability"], "migration_required");
}
