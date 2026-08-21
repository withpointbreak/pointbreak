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
///
/// The header may carry parameters (`test-full *args:`), so it is matched on the
/// recipe name plus a boundary rather than on an exact line.
fn recipe_body(justfile: &str, name: &str) -> String {
    let lines = justfile.lines().collect::<Vec<_>>();
    let header = lines
        .iter()
        .position(|line| {
            line.ends_with(':')
                && line
                    .strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with([' ', ':']))
        })
        .unwrap_or_else(|| panic!("Justfile has no `{name}` recipe"));

    lines[header + 1..]
        .iter()
        .take_while(|line| line.trim().is_empty() || line.starts_with(char::is_whitespace))
        .copied()
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

const BENCH_GATE: &str = "#[cfg(feature = \"bench\")]";
const COUNTING_GATE: &str = "#[cfg(any(test, feature = \"longitudinal-counting\"))]";

/// Whether `attribute` is the line immediately above `declaration`.
fn gate_precedes(source: &str, attribute: &str, declaration: &str) -> bool {
    let lines = source.lines().map(str::trim).collect::<Vec<_>>();
    lines
        .windows(2)
        .any(|window| window == [attribute, declaration])
}

/// Whether `declaration` appears with no attribute on the line above it.
fn declaration_is_ungated(source: &str, declaration: &str) -> bool {
    let lines = source.lines().map(str::trim).collect::<Vec<_>>();
    lines
        .windows(2)
        .any(|window| window[1] == declaration && !window[0].starts_with("#[cfg"))
}

/// The gate a module declaration must carry, and the declarations that must carry none.
struct GateTopology {
    source: &'static str,
    gated: &'static [(&'static str, &'static str)],
    ungated: &'static [&'static str],
}

#[test]
fn harness_submodules_stay_behind_the_bench_feature() {
    let expectations = [
        GateTopology {
            source: "src/bench_support.rs",
            gated: &[
                (BENCH_GATE, "pub mod derived_access;"),
                (BENCH_GATE, "pub mod foundation;"),
            ],
            ungated: &["pub mod longitudinal;"],
        },
        GateTopology {
            source: "src/bench_support/longitudinal/mod.rs",
            gated: &[
                (BENCH_GATE, "mod builder;"),
                (BENCH_GATE, "mod evidence;"),
                (BENCH_GATE, "pub use builder::*;"),
                (BENCH_GATE, "pub use evidence::*;"),
                (COUNTING_GATE, "mod counters;"),
                (COUNTING_GATE, "pub use counters::*;"),
            ],
            ungated: &[
                "mod contract;",
                "mod process;",
                "pub use contract::*;",
                "pub use process::*;",
            ],
        },
        GateTopology {
            source: "src/session/mod.rs",
            gated: &[(BENCH_GATE, "pub(crate) mod benchmark;")],
            ungated: &[],
        },
    ];

    for topology in expectations {
        let relative = topology.source;
        let source = std::fs::read_to_string(repo_path(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let windows_checkout = source.replace("\r\n", "\n").replace('\n', "\r\n");
        let checkouts = [("LF", source.as_str()), ("CRLF", windows_checkout.as_str())];

        for (attribute, declaration) in topology.gated {
            for (checkout, text) in checkouts {
                assert!(
                    gate_precedes(text, attribute, declaration),
                    "{relative} ({checkout}) must gate `{declaration}` with `{attribute}` \
                     so the evidence harness stays out of the default test lane"
                );
            }
        }

        for declaration in topology.ungated {
            for (checkout, text) in checkouts {
                assert!(
                    declaration_is_ungated(text, declaration),
                    "{relative} ({checkout}) must leave `{declaration}` in the default lane: \
                     the counting sites and the control-registry tests depend on it"
                );
            }
        }
    }
}

#[test]
fn ci_profile_exclusions_name_tests_that_still_exist_and_spare_the_full_lane() {
    let profile = std::fs::read_to_string(repo_path(".config/nextest.toml"))
        .expect("read nextest configuration");

    // The ci profile excludes named slow tests from the per-push lane; the
    // scheduled full-suite lane still runs them through the default profile.
    let names = filter_predicate_names(&profile);
    assert!(
        names.contains(&"cargo_install_exposes_only_pointbreak_executable".to_owned()),
        "the slow package-identity install test must be excluded from the per-push ci profile"
    );
    assert_eq!(
        profile.matches("default-filter").count(),
        1,
        "only the ci profile may carry a default-filter; one on the default \
         profile would silently drop the excluded tests from every lane"
    );

    // A renamed or deleted test leaves a stale exclusion silently filtering
    // nothing; every excluded name must still resolve to a real function.
    let sources = read_sources(&tracked_rust_sources());
    for name in &names {
        assert!(
            sources
                .iter()
                .any(|(_, source)| declares_function(source, name)),
            "ci-profile exclusion names a test no tracked Rust source declares: {name}"
        );
    }
}

#[test]
fn test_full_recipe_runs_the_pinned_feature_set_unfiltered() {
    let justfile = std::fs::read_to_string(repo_path("Justfile")).expect("read Justfile");
    let body = recipe_body(&justfile, "test-full");

    assert!(
        body.contains("--features longitudinal-counting"),
        "test-full must select the feature set the qualification argvs pin"
    );
    assert!(
        !body.contains("-E "),
        "test-full must stay unfiltered — a filterset here recreates the \
         silent-partial-match hazard the pin checks exist to close"
    );
    assert!(
        !body.contains("--all-features"),
        "test-full must not enable lmdb-proof or gix-parity, which no pin uses"
    );
}

/// Name and body of every `just` recipe, in file order.
fn recipes(justfile: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = justfile.lines().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        if line.starts_with(char::is_whitespace)
            || line.starts_with('#')
            || line.starts_with('[')
            || line.contains(":=")
        {
            continue;
        }
        let Some(colon) = line.find(':') else {
            continue;
        };
        let Some(name) = line[..colon].split_whitespace().next() else {
            continue;
        };
        let name = name.to_string();
        let body_start = index;
        while index < lines.len()
            && (lines[index].trim().is_empty() || lines[index].starts_with(char::is_whitespace))
        {
            index += 1;
        }
        found.push((name, lines[body_start..index].join("\n")));
    }
    found
}

/// Filter-bearing recipes whose zero-match policy is not yet pinned. Each entry
/// can still report success while running nothing if its filter stops matching;
/// the goal state is an empty list. Growing it is a deliberate, reviewed
/// decision, and an entry that stops naming an unguarded filter-bearing recipe
/// is stale and must be removed.
const ZERO_MATCH_GUARD_PENDING: &[&str] = &["git-bench", "git-parity"];

#[test]
fn every_filtered_recipe_refuses_a_zero_match_run() {
    let justfile = std::fs::read_to_string(repo_path("Justfile")).expect("read Justfile");
    let filtered: Vec<(String, String)> = recipes(&justfile)
        .into_iter()
        .filter(|(_, body)| body.contains("-E '"))
        .collect();

    for pending in ZERO_MATCH_GUARD_PENDING {
        assert!(
            filtered
                .iter()
                .any(|(name, body)| name == pending && !body.contains("--no-tests fail")),
            "{pending} is listed as pending a zero-match guard but is no longer \
             an unguarded filter-bearing recipe; remove the stale entry"
        );
    }

    let offenders: Vec<&str> = filtered
        .iter()
        .filter(|(name, body)| {
            !body.contains("--no-tests fail") && !ZERO_MATCH_GUARD_PENDING.contains(&name.as_str())
        })
        .map(|(name, _)| name.as_str())
        .collect();

    assert!(
        offenders.is_empty(),
        "filter-bearing recipes must refuse a zero-match run with `--no-tests fail`, \
         or the lane can report success while running nothing: {offenders:?}"
    );
}
