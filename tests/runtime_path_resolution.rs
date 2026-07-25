//! Guards the convention the cross-compiled Windows lane depends on.
//!
//! Windows tests are compiled on Linux and executed from a `cargo nextest` archive on a
//! runner with no Rust toolchain. `cargo-nextest` relocates that archive with
//! `--workspace-remap`, which rewrites the paths it hands the tests *at run time*. Anything
//! that captured a path at *compile* time still points at the Linux build machine, and the
//! test fails on Windows with a bare "system cannot find the path specified".
//!
//! That failure names no cause and reproduces on no other platform, so this guard exists to
//! turn it into a message that does. See docs/ci-architecture.md#windows.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[path = "support/env.rs"]
#[allow(dead_code)]
mod env;

/// Macros that capture a build-machine path at compile time.
const COMPILE_TIME_PATHS: &[&str] = &[
    r#"env!("CARGO_MANIFEST_DIR")"#,
    r#"env!("CARGO_BIN_EXE_pointbreak")"#,
    r#"env!("CARGO")"#,
    r#"env!("OUT_DIR")"#,
];

/// The runtime resolvers themselves, each of which reads the environment first and keeps a
/// compile-time value only as the same-machine fallback. These are the intended home for
/// the macros above; everything else should call into one of them.
const RESOLVER_FILES: &[&str] = &[
    "tests/support/env.rs",
    "tests/support/git_repo.rs",
    "src/test_fixtures.rs",
    "src/bench_support.rs",
    "examples/support/review_example_pack.rs",
    // This guard, which necessarily spells the macros out to search for them.
    "tests/runtime_path_resolution.rs",
];

#[test]
fn tests_resolve_build_machine_paths_at_runtime() {
    let root = env::manifest_dir();
    let mut sources = Vec::new();
    for dir in ["src", "tests", "benches", "examples"] {
        collect_rust_sources(&root.join(dir), &mut sources);
    }
    assert!(
        sources.len() > 100,
        "source walk found only {} files; the guard is not looking where it thinks it is",
        sources.len()
    );

    let mut violations = String::new();
    for path in sources {
        let relative = relative_to(&root, &path);
        if RESOLVER_FILES.contains(&relative.as_str()) {
            continue;
        }
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));

        for needle in COMPILE_TIME_PATHS {
            for (offset, _) in source.match_indices(needle) {
                if embeds_contents_at_compile_time(&source, offset) {
                    continue;
                }
                let line = source[..offset].lines().count();
                let _ = writeln!(violations, "  {relative}:{line}  {needle}");
            }
        }
    }

    assert!(
        violations.is_empty(),
        "These capture a build-machine path at compile time:\n\n{violations}\n\
         The Windows suite is cross-compiled on Linux and executed from a relocated nextest \n\
         archive, so a compile-time path points at a machine the test is not running on. It \n\
         passes locally and on Linux and macOS, then fails only on the Windows shards with \n\
         'The system cannot find the path specified'.\n\n\
         Resolve at run time instead — `support::pointbreak_bin()`, `support::manifest_dir()`, \n\
         or `support::cargo_bin()` from tests/support/env.rs, `crate::test_fixtures::manifest_dir()` \n\
         inside the library's own tests, or `crate::bench_support::manifest_dir()` in the \n\
         benchmark harnesses. Each prefers the variable cargo-nextest rewrites and keeps the \n\
         compile-time value only as a same-machine fallback.\n\n\
         `include_str!`/`include_bytes!` are exempt and need no change: they embed the file's \n\
         bytes into the binary, so no path survives to be resolved.\n\n\
         Background: docs/ci-architecture.md#windows"
    );
}

/// True when the macro at `offset` sits inside an `include_str!`/`include_bytes!`, which
/// embeds bytes at compile time and so carries no path into the built binary.
///
/// Scoped to the enclosing statement — scanning back to the previous `;` — so an unrelated
/// `include_str!` earlier in the same block cannot excuse a later runtime path.
fn embeds_contents_at_compile_time(source: &str, offset: usize) -> bool {
    let statement_start = source[..offset].rfind(';').map_or(0, |index| index + 1);
    let statement = &source[statement_start..offset];
    statement.contains("include_str!") || statement.contains("include_bytes!")
}

fn collect_rust_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let entry = entry.expect("read directory entry");
        let path = entry.path();
        if entry.file_type().expect("stat entry").is_dir() {
            collect_rust_sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

fn relative_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
