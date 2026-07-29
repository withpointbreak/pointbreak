use super::product_contract::DerivedAccessProfile;
use super::service::{DerivedAccessHandle, DerivedAccessIoProbe};
use super::sqlite::{CursorLedgerIdentity, SqliteCursorLedger};

fn dependency_line<'a>(dependencies: &'a str, name: &str) -> &'a str {
    dependencies
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("{name} =")))
        .unwrap_or_else(|| panic!("{name} is absent from normal dependencies"))
}

fn directory_entries(path: &std::path::Path) -> Vec<std::ffi::OsString> {
    let mut entries = std::fs::read_dir(path)
        .expect("read directory")
        .map(|entry| entry.expect("directory entry").file_name())
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

#[test]
fn product_owns_the_only_sqlite_core_and_normal_dependency_closure() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for retired in [
        "src/bench_support/derived_access/sqlite_cursor.rs",
        "src/bench_support/derived_access/sqlite_locator.rs",
        "src/bench_support/derived_access/sqlite_semantic.rs",
        "src/bench_support/derived_access/writer_lock.rs",
    ] {
        assert!(!root.join(retired).exists(), "{retired} still owns SQL");
    }
    for product in [
        "src/session/derived_access/sqlite/cursor.rs",
        "src/session/derived_access/sqlite/locator.rs",
        "src/session/derived_access/sqlite/semantic.rs",
        "src/session/derived_access/sqlite/writer_lock.rs",
    ] {
        assert!(root.join(product).is_file(), "{product} is absent");
    }

    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("read Cargo.toml");
    let normal_dependencies = manifest
        .split_once("[dependencies]")
        .and_then(|(_, rest)| rest.split_once("[dev-dependencies]"))
        .map(|(dependencies, _)| dependencies)
        .expect("normal dependency section");
    let libsqlite3 = dependency_line(normal_dependencies, "libsqlite3-sys");
    assert!(libsqlite3.contains("features = [\"bundled\"]"));
    assert!(!libsqlite3.contains("optional = true"));

    let rusqlite = dependency_line(normal_dependencies, "rusqlite");
    assert!(rusqlite.contains("default-features = false"));
    for feature in ["backup", "bundled", "limits"] {
        assert!(rusqlite.contains(&format!("\"{feature}\"")));
    }
    assert!(!rusqlite.contains("optional = true"));
    assert!(!manifest.contains("\"dep:libsqlite3-sys\""));
    assert!(!manifest.contains("\"dep:rusqlite\""));
}

#[test]
fn off_profile_performs_no_derived_filesystem_actions() {
    let store = tempfile::tempdir().expect("temporary store");
    std::fs::write(store.path().join("sentinel"), b"unchanged").expect("write sentinel");
    let before = directory_entries(store.path());
    let probe = DerivedAccessIoProbe::default();

    let service = DerivedAccessHandle::resolve(
        DerivedAccessProfile::Off,
        store.path(),
        "store:sha256:off",
        probe.clone(),
    )
    .expect("off service");

    assert_eq!(service.profile(), DerivedAccessProfile::Off);
    assert_eq!(probe.snapshot().total(), 0);
    assert_eq!(directory_entries(store.path()), before);
}

#[test]
fn active_missing_sidecar_records_only_the_writer_lock_and_path_resolution() {
    let store = tempfile::tempdir().expect("temporary store");
    let probe = DerivedAccessIoProbe::default();

    let error = DerivedAccessHandle::resolve(
        DerivedAccessProfile::SqliteWalBodylessV1,
        store.path(),
        "store:sha256:missing-sidecar",
        probe.clone(),
    )
    .expect_err("active service requires an existing qualified sidecar");

    assert!(error.to_string().contains("incomplete"));
    let snapshot = probe.snapshot();
    assert_eq!(snapshot.root_resolutions, 1);
    assert_eq!(snapshot.sqlite_physical_opens, 0);
    assert_eq!(snapshot.total(), 1);
    assert!(!store.path().join(".pointbreak-derived").exists());
    assert_eq!(
        directory_entries(store.path()),
        vec![std::ffi::OsString::from(".pointbreak-derived.writer.lock")]
    );
}

#[test]
fn active_profile_opens_the_existing_product_core() {
    let store = tempfile::tempdir().expect("temporary store");
    SqliteCursorLedger::initialize_empty(
        store.path(),
        CursorLedgerIdentity::new("store:sha256:active"),
    )
    .expect("initialize qualified sidecar");
    let probe = DerivedAccessIoProbe::default();

    let handle = DerivedAccessHandle::resolve(
        DerivedAccessProfile::SqliteWalBodylessV1,
        store.path(),
        "store:sha256:active",
        probe.clone(),
    )
    .expect("active service");

    assert_eq!(handle.profile(), DerivedAccessProfile::SqliteWalBodylessV1);
    assert!(handle.active().is_some());
    assert_eq!(probe.snapshot().root_resolutions, 1);
    assert_eq!(probe.snapshot().sqlite_physical_opens, 3);
    assert_eq!(probe.snapshot().total(), 4);
}
