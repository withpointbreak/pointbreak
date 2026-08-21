//! Machine checks over the qualification pin surface.
//!
//! Two independent sets of test names are load-bearing for the qualification
//! campaign: the predicates the `derived-access-tests` recipe filters on, and the
//! module paths the control binaries execute with `--exact`. Both fail silently
//! when a test is renamed or moved — a nextest filter expression exits zero when
//! only some of its predicates match, and a stale `--exact` path simply runs
//! nothing. These checks read the recipe and the contract as text and require
//! every pinned name to resolve to a real function, so a rename goes loud here
//! instead of quietly hollowing out a campaign lane.
//!
//! Reading source text rather than linking the harness keeps this file in the
//! default lane whatever features a run selects.

use std::path::{Path, PathBuf};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// The indented body of a `just` recipe, ending at the next unindented line.
fn recipe_body(justfile: &str, name: &str) -> String {
    let marker = format!("\n{name}:\n");
    let suffix = justfile
        .split(&marker)
        .nth(1)
        .unwrap_or_else(|| panic!("Justfile has no `{name}` recipe"));
    suffix
        .lines()
        .take_while(|line| line.trim().is_empty() || line.starts_with(char::is_whitespace))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The single-quoted argument of the first `-E` flag in a recipe body.
fn filter_expression(body: &str) -> &str {
    let start = body
        .find("-E '")
        .expect("recipe has an -E filter expression")
        + "-E '".len();
    let length = body[start..]
        .find('\'')
        .expect("filter expression is single-quoted");
    &body[start..start + length]
}

/// Every `test(<name>)` predicate in a nextest filter expression, in source order.
fn filter_predicate_names(expression: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = expression;
    while let Some(offset) = rest.find("test(") {
        let after = &rest[offset + "test(".len()..];
        let Some(end) = after.find(')') else {
            break;
        };
        names.push(after[..end].to_owned());
        rest = &after[end..];
    }
    names
}

fn derived_access_filter_names(justfile: &str) -> Vec<String> {
    let body = recipe_body(justfile, "derived-access-tests");
    let expression = filter_expression(&body);
    let names = filter_predicate_names(expression);

    // A second `-E` would be parsed by nextest but ignored here, silently
    // shrinking the checked surface.
    let outside = body.replace(expression, "");
    assert!(
        !outside.contains("test("),
        "derived-access-tests has a `test(` predicate outside the checked filter expression"
    );

    names
}

/// Repository-relative paths of the tracked Rust sources the pins can name.
fn tracked_rust_sources() -> Vec<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["ls-files", "-z", "--", "src", "tests"])
        .current_dir(repo_path("."))
        .output()
        .expect("run git ls-files");
    assert!(output.status.success(), "git ls-files failed");

    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| PathBuf::from(String::from_utf8_lossy(entry).into_owned()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect()
}

fn read_sources(paths: &[PathBuf]) -> Vec<(PathBuf, String)> {
    paths
        .iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(repo_path(&path.to_string_lossy())).ok()?;
            Some((path.clone(), text))
        })
        .collect()
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Whether `source` declares a function named exactly `name`.
fn declares_function(source: &str, name: &str) -> bool {
    let needle = format!("fn {name}");
    let bytes = source.as_bytes();
    let mut searched = 0;
    while let Some(offset) = source[searched..].find(&needle) {
        let start = searched + offset;
        let end = start + needle.len();
        let opens_word = start == 0 || !is_identifier_byte(bytes[start - 1]);
        let closes_word = end >= bytes.len() || !is_identifier_byte(bytes[end]);
        if opens_word && closes_word {
            return true;
        }
        searched = start + 1;
    }
    false
}

#[test]
fn filter_predicates_in_the_derived_access_recipe_all_resolve() {
    let justfile = std::fs::read_to_string(repo_path("Justfile")).expect("read Justfile");
    let names = derived_access_filter_names(&justfile);

    assert!(
        names.len() >= 33,
        "derived-access filter looks truncated: found {} predicates",
        names.len()
    );

    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        names.len(),
        "derived-access filter repeats a predicate: {names:?}"
    );

    let sources = read_sources(&tracked_rust_sources());
    let unresolved = names
        .iter()
        .filter(|name| {
            !sources
                .iter()
                .any(|(_, source)| declares_function(source, name))
        })
        .cloned()
        .collect::<Vec<_>>();

    assert!(
        unresolved.is_empty(),
        "derived-access filter names {} test(s) that no tracked Rust source declares: {unresolved:#?}\n\
         A renamed or deleted test leaves the recipe passing while it silently stops running.",
        unresolved.len()
    );
}

/// Whether a string literal looks like a lowercase Rust module path.
fn is_module_path(candidate: &str) -> bool {
    let segments = candidate.split("::").collect::<Vec<_>>();
    segments.len() >= 2
        && segments.iter().all(|segment| {
            let mut characters = segment.chars();
            matches!(characters.next(), Some(first) if first.is_ascii_lowercase() || first == '_')
                && characters.all(|character| {
                    character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
                })
        })
}

/// The `--exact` module paths the qualification control binaries execute.
///
/// A path is a control-registry entry when its second-to-last segment names a
/// test module — `tests`, or any `*_tests` sibling module. Anchoring on `::tests::`
/// alone silently drops the `service_tests` entry.
fn control_registry_paths(contract: &str) -> Vec<String> {
    let mut paths = contract
        .split('"')
        .filter(|segment| is_module_path(segment))
        .filter(|segment| {
            let segments = segment.split("::").collect::<Vec<_>>();
            segments.len() >= 3 && {
                let module = segments[segments.len() - 2];
                module == "tests" || module.ends_with("_tests")
            }
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

/// Source files that could declare the test a control-registry path names, most
/// specific first.
fn control_path_candidates(path: &str) -> Vec<PathBuf> {
    let segments = path.split("::").collect::<Vec<_>>();
    let module = segments[segments.len() - 2];
    let owners = &segments[..segments.len() - 2];

    let mut module_root = PathBuf::from("src");
    for owner in owners {
        module_root = module_root.join(owner);
    }
    let mut parent_root = PathBuf::from("src");
    for owner in &owners[..owners.len() - 1] {
        parent_root = parent_root.join(owner);
    }

    vec![
        module_root.join(format!("{module}.rs")),
        module_root.join(module).join("mod.rs"),
        parent_root.join(format!("{}.rs", owners[owners.len() - 1])),
        module_root.join("mod.rs"),
    ]
}

#[test]
fn control_registry_paths_all_resolve_to_their_declared_module() {
    let contract =
        std::fs::read_to_string(repo_path("src/bench_support/derived_access/contract.rs"))
            .expect("read derived-access contract");
    let paths = control_registry_paths(&contract);

    assert!(
        paths.len() >= 24,
        "control registry looks truncated: found {} paths",
        paths.len()
    );

    let mut unresolved = Vec::new();
    for path in &paths {
        let name = path.rsplit("::").next().expect("path has a leaf name");
        let candidates = control_path_candidates(path);
        let Some(declaring) = candidates
            .iter()
            .find(|candidate| repo_path(&candidate.to_string_lossy()).exists())
        else {
            unresolved.push(format!(
                "{path}\n    no candidate file exists: {candidates:?}"
            ));
            continue;
        };
        let source = std::fs::read_to_string(repo_path(&declaring.to_string_lossy()))
            .unwrap_or_else(|error| panic!("read {}: {error}", declaring.display()));
        if !declares_function(&source, name) {
            unresolved.push(format!(
                "{path}\n    {} does not declare `fn {name}` (also tried {candidates:?})",
                declaring.display()
            ));
        }
    }

    assert!(
        unresolved.is_empty(),
        "{} control-registry path(s) do not resolve to their declared module:\n{}\n\
         An `--exact` path that names nothing runs no test and still exits zero.",
        unresolved.len(),
        unresolved.join("\n")
    );
}

#[test]
fn pinned_filter_names_and_control_registry_paths_stay_disjoint() {
    let justfile = std::fs::read_to_string(repo_path("Justfile")).expect("read Justfile");
    let contract =
        std::fs::read_to_string(repo_path("src/bench_support/derived_access/contract.rs"))
            .expect("read derived-access contract");

    let filtered = derived_access_filter_names(&justfile);
    let registered = control_registry_paths(&contract)
        .into_iter()
        .map(|path| {
            path.rsplit("::")
                .next()
                .expect("path has a leaf name")
                .to_owned()
        })
        .collect::<Vec<_>>();

    let shared = filtered
        .iter()
        .filter(|name| registered.contains(name))
        .collect::<Vec<_>>();

    assert!(
        shared.is_empty(),
        "these tests are pinned by both the recipe filter and the control registry: {shared:#?}\n\
         Overlap lets one lane silently stand in for the other.",
    );
}
