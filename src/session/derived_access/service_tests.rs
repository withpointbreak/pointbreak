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

fn rust_sources(root: &std::path::Path) -> Vec<(std::path::PathBuf, String)> {
    fn collect(path: &std::path::Path, sources: &mut Vec<(std::path::PathBuf, String)>) {
        let mut entries = std::fs::read_dir(path)
            .expect("read source directory")
            .map(|entry| entry.expect("source directory entry").path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                collect(&entry, sources);
            } else if entry.extension().and_then(std::ffi::OsStr::to_str) == Some("rs") {
                let source = std::fs::read_to_string(&entry).expect("read Rust source");
                sources.push((entry, source));
            }
        }
    }

    let mut sources = Vec::new();
    collect(root, &mut sources);
    sources
}

fn production_source(source: &str) -> &str {
    source
        .split_once("\n#[cfg(test)]\nmod tests")
        .map_or(source, |(production, _)| production)
}

fn owners_with_all(
    sources: &[(std::path::PathBuf, String)],
    needles: &[&str],
) -> std::collections::BTreeSet<std::path::PathBuf> {
    sources
        .iter()
        .filter_map(|(path, source)| {
            let source = production_source(source);
            needles
                .iter()
                .all(|needle| source.contains(needle))
                .then(|| path.clone())
        })
        .collect()
}

fn public_use_material(source: &str) -> String {
    let mut material = String::new();
    let mut collecting = false;
    for line in source.lines() {
        if !collecting && line.trim_start().starts_with("pub use ") {
            collecting = true;
        }
        if collecting {
            material.push_str(line);
            material.push('\n');
            if line.contains(';') {
                collecting = false;
            }
        }
    }
    material
}

fn rust_string_literals(source: &str) -> Vec<&str> {
    let bytes = source.as_bytes();
    let mut literals = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"//") {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            index += 2;
            let mut depth = 1_u32;
            while index < bytes.len() && depth > 0 {
                if bytes[index..].starts_with(b"/*") {
                    depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            continue;
        }
        if bytes[index] == b'r' {
            let mut quote = index + 1;
            while quote < bytes.len() && bytes[quote] == b'#' {
                quote += 1;
            }
            if quote < bytes.len() && bytes[quote] == b'"' {
                let hashes = quote - index - 1;
                let start = quote + 1;
                let mut end = start;
                while end < bytes.len() {
                    if bytes[end] == b'"'
                        && (hashes == 0
                            || (end + hashes < bytes.len()
                                && bytes[end + 1..=end + hashes]
                                    .iter()
                                    .all(|byte| *byte == b'#')))
                    {
                        literals.push(&source[start..end]);
                        index = end + hashes + 1;
                        break;
                    }
                    end += 1;
                }
                if end < bytes.len() {
                    continue;
                }
            }
        }
        if bytes[index] == b'"' {
            let start = index + 1;
            index = start;
            while index < bytes.len() {
                match bytes[index] {
                    b'\\' => index = index.saturating_add(2),
                    b'"' => {
                        literals.push(&source[start..index]);
                        index += 1;
                        break;
                    }
                    _ => index += 1,
                }
            }
            continue;
        }
        index += 1;
    }
    literals
}

fn looks_like_sql(literal: &str) -> bool {
    let literal = literal.to_ascii_lowercase();
    literal
        .find("select ")
        .is_some_and(|start| literal[start..].contains(" from "))
        || literal.contains("insert into ")
        || literal
            .find("update ")
            .is_some_and(|start| literal[start..].contains(" set "))
        || literal.contains("delete from ")
        || literal.contains("create table ")
        || literal.contains("create temp table ")
        || literal.contains("pragma ")
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
fn change_reader_reuses_the_single_derived_access_engine() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let derived_root = root.join("src/session/derived_access");
    let sources = rust_sources(&derived_root)
        .into_iter()
        .filter(|(path, _)| {
            path.file_name().and_then(std::ffi::OsStr::to_str) != Some("service_tests.rs")
        })
        .collect::<Vec<_>>();

    for (owner, primitives) in [
        (
            "generation publication",
            &[
                "struct GenerationLayout",
                "publication-staging",
                "fn promote_staging(",
                "fn publish(",
                "fn current_publication(",
            ][..],
        ),
        (
            "generation lifecycle",
            &["struct DerivedAccessLifecycle", "struct CurrentGeneration"][..],
        ),
        (
            "current-generation slot",
            &["Mutex<Option<Arc<CurrentGeneration", "current:"][..],
        ),
        (
            "rebuild worker",
            &[
                "background_rebuild_in_flight",
                "start_background_rebuild",
                ".spawn(",
            ][..],
        ),
        (
            "SQLite service",
            &[
                "struct DerivedAccessService",
                "SqliteCursorLedger",
                "SqliteLocator",
                "SqliteSemantic",
            ][..],
        ),
        ("shared runtime", &["struct DerivedAccessRuntime"][..]),
    ] {
        assert_eq!(
            owners_with_all(&sources, primitives).len(),
            1,
            "{owner} primitives must have one production owner"
        );
    }
    for hydrator in [
        "fn hydrate_events(",
        "fn lookup_event_ids_hydrated(",
        "fn hydrate_locator_row(",
    ] {
        assert_eq!(
            owners_with_all(&sources, &[hydrator]).len(),
            1,
            "{hydrator} must have one authoritative implementation"
        );
    }

    let facade = std::fs::read_to_string(derived_root.join("changes.rs"))
        .expect("read derived Change facade");
    assert!(facade.contains("runtime: Arc<DerivedAccessRuntime>"));
    for forbidden in [
        "GenerationLayout",
        "DerivedAccessLifecycle",
        "CurrentGeneration",
        "JoinHandle",
        "SqliteLocator",
        "SqliteSemantic",
        "QualificationLocalJournal",
        "EventStore",
        "rusqlite",
        "std::fs",
        "read_change_semantics_for_qualification",
        "ExhaustiveSearchFallback",
    ] {
        assert!(
            !facade.contains(forbidden),
            "Change facade must reuse the shared engine instead of naming {forbidden}"
        );
    }

    let adapter = std::fs::read_to_string(root.join("src/bench_support/derived_access/adapter.rs"))
        .expect("read qualification adapter");
    assert!(adapter.contains("DerivedAccessService as QualificationDerivedAccessAdapter"));
}

#[test]
fn cli_keeps_derived_storage_behind_the_session_facade() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = rust_sources(&root.join("src/cli"));
    for (path, source) in &sources {
        let normalized = source.to_ascii_lowercase();
        for forbidden in [
            "rusqlite",
            "libsqlite3_sys",
            "libsqlite3-sys",
            "session::derived_access",
            "SqliteCursor",
            "SqliteLocator",
            "SqliteSemantic",
            "GenerationLayout",
            "CurrentGeneration",
            "DerivedAccessLifecycle",
            "DerivedAccessRuntime",
            "DerivedAccessService",
        ] {
            assert!(
                !normalized.contains(&forbidden.to_ascii_lowercase()),
                "{} leaks {forbidden} into the CLI",
                path.display()
            );
        }
        for literal in rust_string_literals(source) {
            assert!(
                !looks_like_sql(literal),
                "{} owns derived SQL in string literal {literal:?}",
                path.display()
            );
        }
    }

    let session_exports =
        std::fs::read_to_string(root.join("src/session/mod.rs")).expect("read session exports");
    assert!(session_exports.contains("pub(crate) mod derived_access;"));
    let public_exports = public_use_material(&session_exports);
    for storage_type in [
        "DerivedAccessRuntime",
        "GenerationLayout",
        "CurrentGeneration",
        "DerivedAccessLifecycle",
        "DerivedAccessService",
        "SqliteCursorLedger",
        "SqliteLocator",
        "SqliteSemantic",
    ] {
        assert!(
            !public_exports.contains(storage_type),
            "session exports the storage/runtime type {storage_type}"
        );
    }
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
    let layout = super::layout::DerivedStorageLayout::resolve(store.path()).unwrap();
    assert!(!layout.root().exists());
    assert_eq!(
        directory_entries(store.path()),
        vec![layout.writer_lock().file_name().unwrap().to_owned()]
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
