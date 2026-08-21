use std::path::{Path, PathBuf};

#[test]
fn readme_teaches_the_pointbreak_package_and_command() {
    let readme = std::fs::read_to_string("README.md").expect("read README");

    assert!(readme.contains("cargo install pointbreak"));
    assert!(readme.contains("provides the `pointbreak` command"));
    assert!(readme.contains("0.7.0"));
    assert!(!readme.contains("cargo install shore\n"));
    assert!(!readme.contains("cargo install shore "));
}

#[test]
fn installation_documents_the_one_release_hard_cutover() {
    let installation = std::fs::read_to_string("docs/installation.md").expect("read installation");

    for required in [
        "Release `0.7.0` is a one-release hard cutover",
        "Stop every process that can write Review state",
        "Move owner-controlled state and config offline",
        "POINTBREAK_HOME",
        "<repo>/.pointbreak/",
        "<git-common-dir>/pointbreak/",
        "<git-common-dir>/pointbreak.link.json",
        "pointbreak store paths --repo <path> --format json",
        "verify readback",
        "Rollback is the inverse filesystem move",
        "no runtime fallback, compatibility alias, automatic migration, migration CLI",
    ] {
        assert!(
            installation.contains(required),
            "installation guide is missing hard-cutover guidance: {required:?}"
        );
    }

    assert!(!installation.contains("pointbreak store migrate-paths"));
    assert!(!installation.contains("pointbreak review"));
}

#[test]
fn retired_documentation_host_is_never_presented_as_live() {
    for path in LIVING_OPERATIONAL_SOURCES {
        let contents =
            std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {path}: {error}"));
        if contents.contains("docs.withpointbreak.com") {
            assert!(
                contents.contains("archived") || contents.contains("retired"),
                "{path} presents docs.withpointbreak.com without an archived/retired label"
            );
        }
    }
}

#[test]
fn living_sources_teach_only_the_pointbreak_operational_contract() {
    for path in LIVING_OPERATIONAL_SOURCES {
        let contents =
            std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {path}: {error}"));

        for (index, line) in contents.lines().enumerate() {
            let line_number = index + 1;
            for (needle, purpose) in FORBIDDEN_LIVING_PATTERNS {
                if line.contains(needle) && classify_retained_reference(path, line).is_none() {
                    panic!("{path}:{line_number} presents {purpose}: {line:?}");
                }
            }
        }
    }
}

#[test]
fn generic_store_guidance_does_not_present_a_literal_path_as_universal() {
    for (path, forbidden) in [
        ("CONTRIBUTING.md", "raw `.shore/data/` files"),
        ("docs/agent-authoring.md", "same `.pointbreak/data/` store"),
        (
            "docs/assessment-model.md",
            "`show` replays `.pointbreak/data/events/`",
        ),
        (
            "docs/input-request-model.md",
            "`list` and `fetch` replay `.pointbreak/data/events/`",
        ),
        (
            "docs/input-request-model.md",
            "authoritative store is the `.pointbreak/data/events/`",
        ),
    ] {
        let contents =
            std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {path}: {error}"));
        assert!(
            !contents.contains(forbidden),
            "{path} should describe the resolved store instead of {forbidden:?}"
        );
    }
}

#[test]
fn legacy_product_word_detection_is_case_insensitive_and_word_bounded() {
    assert!(contains_legacy_reference("matched by Shore at scan time"));
    assert!(contains_legacy_reference("run SHORE_CONFIG=/tmp/config"));
    assert!(!contains_legacy_reference("shoreline fixture"));
}

#[test]
fn every_retained_public_legacy_reference_has_a_narrow_classification() {
    for path in auditable_files() {
        let audit_path = public_audit_path(&path);
        let Some(contents) = auditable_text(&path) else {
            continue;
        };
        for (index, line) in contents.lines().enumerate() {
            if contains_legacy_reference(line)
                && classify_retained_reference(&audit_path, line).is_none()
            {
                panic!(
                    "{}:{} has an unclassified legacy reference: {:?}",
                    audit_path,
                    index + 1,
                    line
                );
            }
        }
    }
}

const LIVING_OPERATIONAL_SOURCES: &[&str] = &[
    "CONTRIBUTING.md",
    "README.md",
    "docs/agent-authoring.md",
    "docs/assessment-model.md",
    "docs/benchmarking.md",
    "docs/cli-reference.md",
    "docs/getting-started.md",
    "docs/id-prefixes.md",
    "docs/input-request-model.md",
    "docs/installation.md",
    "docs/library-api.md",
    "docs/manual-testing.md",
    "docs/releasing.md",
    "docs/review-workflow.md",
    "docs/signing-ux.md",
    "docs/storage-model.md",
    "docs/substrate-language.md",
    "docs/substrate-thesis-summary.md",
    "Justfile",
    "benches/store_backend.rs",
    "scripts/capture-inspector-screenshots.sh",
    "scripts/worktree-to-fixture.sh",
    "skills/README.md",
    "skills/pointbreak-author/SKILL.md",
    "skills/pointbreak-author-response/SKILL.md",
    "skills/pointbreak-reviewer/SKILL.md",
];

const FORBIDDEN_LIVING_PATTERNS: &[(&str, &str)] = &[
    ("shore ", "a legacy executable command"),
    (
        "SHORE_",
        "a legacy environment variable as current guidance",
    ),
    (".shore", "a legacy path as current placement"),
    ("pointbreak review", "the rejected review command prefix"),
    ("cargo install shore", "the retired package/install name"),
    ("cargo binstall shore", "the retired package/install name"),
    ("store migrate-paths", "a migration CLI that does not exist"),
    ("automatically migrates", "automatic migration behavior"),
    ("automatically migrate", "automatic migration behavior"),
];

/// Directory trees the public-language audit reads in full.
const AUDIT_ROOTS: &[&str] = &[
    "docs",
    "skills",
    "scripts",
    "benches",
    "tests/fixtures/event_signatures",
    "tests/fixtures/legacy_stores",
    "tests/fixtures/naming-cutover",
    "tests/fixtures/packages",
    "tests/fixtures/review_documents",
];

/// Individual files the audit reads outside the walked trees.
const AUDIT_FILES: &[&str] = &[
    "CONTRIBUTING.md",
    "README.md",
    "Justfile",
    "CHANGELOG.md",
    "tests/agent_skill_validation_evidence.rs",
    "tests/docs_open_source_readiness.rs",
    "tests/docs_package_identity.rs",
];

/// Every file the public-language audit reads, sorted and de-duplicated.
///
/// The tree contents come from git so that build output and untracked local
/// artifacts — a macOS `.DS_Store`, a scratch note — can never change the audit's
/// verdict. A checkout without git falls back to a plain recursive walk.
fn auditable_files() -> Vec<PathBuf> {
    let mut paths = AUDIT_FILES
        .iter()
        .map(Path::new)
        .filter(|path| path.exists())
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();

    match tracked_files(Path::new("."), AUDIT_ROOTS) {
        Some(tracked) => paths.extend(tracked),
        None => {
            for root in AUDIT_ROOTS {
                collect_files(Path::new(root), &mut paths);
            }
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

/// Read a file the audit inspects, yielding `None` for content that is not text.
fn auditable_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    String::from_utf8(bytes).ok()
}

/// Enumerate the git-tracked files under `pathspecs`, relative to `root`.
///
/// Returns `None` when git cannot answer — a packaged checkout without a
/// repository — so callers can fall back to walking the directories.
fn tracked_files(root: &Path, pathspecs: &[&str]) -> Option<Vec<PathBuf>> {
    let present = pathspecs
        .iter()
        .filter(|pathspec| root.join(pathspec).exists())
        .collect::<Vec<_>>();
    if present.is_empty() {
        return None;
    }

    let output = std::process::Command::new("git")
        .args(["ls-files", "-z", "--"])
        .args(&present)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    Some(
        output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
            .map(|entry| PathBuf::from(String::from_utf8_lossy(entry).into_owned()))
            .collect(),
    )
}

fn collect_files(root: &Path, paths: &mut Vec<PathBuf>) {
    for entry in
        std::fs::read_dir(root).unwrap_or_else(|error| panic!("read {}: {error}", root.display()))
    {
        let entry = entry.unwrap_or_else(|error| panic!("read directory entry: {error}"));
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("read {} file type: {error}", entry.path().display()));
        if file_type.is_dir() {
            collect_files(&entry.path(), paths);
        } else if file_type.is_file() {
            paths.push(entry.path());
        }
    }
}

fn public_audit_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn contains_legacy_reference(line: &str) -> bool {
    line.contains(".shore")
        || line.contains("SHORE_")
        || contains_ascii_word(&line.to_ascii_lowercase(), "shore")
}

#[test]
fn public_audit_paths_are_platform_independent() {
    let path = public_audit_path(Path::new(r"docs\adr\adr-0001-example.md"));

    assert_eq!(path, "docs/adr/adr-0001-example.md");
    assert_eq!(
        classify_retained_reference(&path, "frozen shore.note-body identifier"),
        Some("accepted ADR history")
    );
}

fn contains_ascii_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(start, _)| {
        let end = start + needle.len();
        let before = haystack[..start].chars().next_back();
        let after = haystack[end..].chars().next();
        !before.is_some_and(is_word_character) && !after.is_some_and(is_word_character)
    })
}

fn is_word_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn classify_retained_reference(path: &str, line: &str) -> Option<&'static str> {
    if path.starts_with("docs/adr/") {
        return Some("accepted ADR history");
    }
    if path == "CHANGELOG.md" {
        return Some("published changelog history");
    }
    if [
        "tests/fixtures/event_signatures/",
        "tests/fixtures/legacy_stores/",
        "tests/fixtures/naming-cutover/",
        "tests/fixtures/packages/",
        "tests/fixtures/review_documents/",
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix))
    {
        return Some("frozen fixture or captured machine document");
    }
    if [
        "tests/agent_skill_validation_evidence.rs",
        "tests/docs_open_source_readiness.rs",
        "tests/docs_package_identity.rs",
    ]
    .contains(&path)
    {
        return Some("test intentionally quoting rejected or historical strings");
    }

    match path {
        "README.md" if line.contains("assets/shore-inspector-") => {
            Some("checked-in screenshot basename")
        }
        "docs/installation.md"
            if line.starts_with("   |")
                && [
                    "<repo>/.shore/",
                    "<git-common-dir>/shore/",
                    "<git-common-dir>/shore.link.json",
                    "$XDG_DATA_HOME/shore",
                    "$HOME/.shore",
                    "%APPDATA%\\shore",
                ]
                .iter()
                .any(|old_path| line.contains(old_path)) =>
        {
            Some("explicit pre-0.7.0 location in the cutover table")
        }
        "docs/assessment-model.md"
        | "docs/cli-reference.md"
        | "docs/input-request-model.md"
        | "docs/library-api.md"
            if line.contains("shore.") =>
        {
            Some("frozen persisted protocol identifier")
        }
        "docs/storage-model.md" if line.contains("shore.") => {
            Some("frozen persisted protocol identifier")
        }
        "docs/storage-model.md" if line.contains(".shore-write") => {
            Some("frozen atomic-write temporary filename")
        }
        "Justfile" if line.contains("shore(\\.exe)?|--bin shore") => {
            Some("negative release-surface assertion")
        }
        "scripts/capture-inspector-screenshots.sh"
            if line.contains("shore-inspector-") || line.contains("shore-inspect-") =>
        {
            Some("checked-in screenshot basename or inspector preference key")
        }
        "scripts/change-inspector-browser-verify.sh"
            if line.contains("\"schema\":\"shore.store-config\"") =>
        {
            Some("frozen persisted protocol identifier in a disposable browser fixture")
        }
        "scripts/install-selftest.sh"
            if line.contains("neighbor=\"${install_dir}/shore\"")
                || line.contains("grep -i 'shore'") =>
        {
            Some("negative installer assertion and untouched-neighbor sentinel")
        }
        "scripts/install-selftest.ps1"
            if line.contains("$neighbor = Join-Path $installDir \"shore.exe\"")
                || line.contains("-match \"(?i)shore\"") =>
        {
            Some("negative installer assertion and untouched-neighbor sentinel")
        }
        "scripts/package-release-selftest.sh"
            if line.contains("payload_dir/shore")
                || line.contains("-C \"$payload_dir\" shore")
                || line.contains("shore.exe")
                || line.contains("ln -s shore") =>
        {
            Some("intentionally invalid archive or alias fixture")
        }
        _ => None,
    }
}

#[test]
fn skills_distribution_uses_the_canonical_repository_route() {
    let skills_readme =
        std::fs::read_to_string("skills/README.md").expect("read skills distribution README");

    assert!(
        skills_readme.contains("npx skills add withpointbreak/pointbreak"),
        "skills distribution names the canonical supported install route"
    );
    assert!(
        !skills_readme.contains("pointbreak review"),
        "skills distribution never teaches the rejected review command prefix"
    );
}

#[test]
fn readme_has_release_badges_for_pointbreak() {
    let readme = std::fs::read_to_string("README.md").expect("read README");

    assert!(readme.contains("https://crates.io/crates/pointbreak"));
    assert!(readme.contains("https://img.shields.io/crates/v/pointbreak"));
    assert!(readme.contains("https://docs.rs/pointbreak"));
    assert!(readme.contains("https://docs.rs/pointbreak/badge.svg"));
    assert!(
        readme.contains("https://github.com/withpointbreak/pointbreak/actions/workflows/ci.yml")
    );
    assert!(readme.contains("actions/workflows/ci.yml/badge.svg"));
}

#[test]
fn cargo_metadata_points_to_pointbreak_repository() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("read Cargo manifest");

    assert!(manifest.contains(r#"homepage = "https://github.com/withpointbreak/pointbreak""#));
    assert!(manifest.contains(r#"repository = "https://github.com/withpointbreak/pointbreak""#));
}

#[test]
fn living_metadata_uses_the_canonical_organization_repository() {
    let stale_repository = ["kevinswiber", "pointbreak"].join("/");
    let canonical_repository = "withpointbreak/pointbreak";
    let paths = [
        ".github/ISSUE_TEMPLATE/config.yml",
        "CONTRIBUTING.md",
        "Cargo.toml",
        "README.md",
        "docs/adr/adr-0014-reviewunit-commit-range-lifecycle.md",
        "docs/id-prefixes.md",
        "docs/installation.md",
        "docs/storage-model.md",
        "extensions/vscode/package.json",
        "scripts/install-selftest.ps1",
        "scripts/install-selftest.sh",
        "scripts/install.ps1",
        "scripts/install.sh",
        "skills/README.md",
        "src/cli/inspect/web/test/css-coverage.test.ts",
    ];

    for path in paths {
        let contents = std::fs::read_to_string(path).unwrap_or_else(|error| {
            panic!("read {path}: {error}");
        });
        assert!(
            !contents.contains(&stale_repository),
            "{path} still uses the personal repository owner"
        );
        assert!(
            contents.contains(canonical_repository),
            "{path} does not name the canonical organization repository"
        );
    }
}

#[test]
fn vscode_metadata_keeps_its_identity_and_uses_canonical_support_urls() {
    let package = std::fs::read_to_string("extensions/vscode/package.json")
        .expect("read VS Code package manifest");

    assert!(package.contains(r#""publisher": "pointbreak""#));
    assert!(package.contains(r#""name": "pointbreak""#));
    assert!(package.contains("https://github.com/withpointbreak/pointbreak.git"));
    assert!(package.contains("https://github.com/withpointbreak/pointbreak/issues"));
    assert!(package.contains("https://github.com/withpointbreak/pointbreak#readme"));
}

#[test]
fn readme_drops_branded_hunk_origin_references() {
    let readme = std::fs::read_to_string("README.md").expect("read README");

    for stale in [
        "modem-dev/hunk",
        "kevinswiber/hunk",
        "docs/hunk-feedback.md",
        "Hunk is the practical inspiration",
        "real Hunk review session",
        "hunk fork",
    ] {
        assert!(
            !readme.contains(stale),
            "README still contains stale Hunk reference: {stale}"
        );
    }
    assert!(!Path::new("docs/hunk-feedback.md").exists());
}

#[test]
fn just_run_targets_the_pointbreak_binary() {
    let justfile = std::fs::read_to_string("Justfile").expect("read Justfile");

    assert!(justfile.contains("cargo +stable run --bin pointbreak --"));
}

#[test]
fn tracked_file_enumeration_excludes_ignored_and_untracked_files() {
    let temp = tempfile::tempdir().expect("temporary repository");
    let root = temp.path();
    let docs = root.join("docs");
    std::fs::create_dir(&docs).expect("create docs directory");
    std::fs::write(root.join(".gitignore"), "*.ignored\n").expect("write gitignore");
    std::fs::write(docs.join("kept.md"), "kept\n").expect("write tracked doc");
    std::fs::write(docs.join("build.ignored"), "ignored\n").expect("write ignored doc");
    std::fs::write(docs.join("scratch.md"), "scratch\n").expect("write untracked doc");

    for args in [
        vec!["init", "--quiet"],
        vec!["add", "docs/kept.md", ".gitignore"],
    ] {
        let status = std::process::Command::new("git")
            .args(&args)
            .current_dir(root)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    let tracked = tracked_files(root, &["docs"]).expect("enumerate tracked files");

    assert!(tracked.contains(&Path::new("docs").join("kept.md")));
    assert!(!tracked.iter().any(|path| path.ends_with("build.ignored")));
    assert!(!tracked.iter().any(|path| path.ends_with("scratch.md")));
}

#[test]
fn audit_sources_exclude_untracked_files() {
    let files = auditable_files();

    assert!(files.iter().any(|path| path.ends_with("README.md")));
    assert!(
        files
            .iter()
            .any(|path| path.ends_with("docs/installation.md"))
    );
    assert!(
        !files
            .iter()
            .any(|path| path.components().any(|part| part.as_os_str() == "target")),
        "audited set must not reach into build output"
    );
}

#[test]
fn auditable_text_skips_non_utf8_bytes() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let binary = temp.path().join("junk.bin");
    std::fs::write(&binary, [0xff, 0xfe, 0x00]).expect("write non-UTF-8 bytes");
    let text = temp.path().join("readable.md");
    std::fs::write(&text, "readable\n").expect("write text");

    assert!(auditable_text(&binary).is_none());
    assert_eq!(auditable_text(&text).as_deref(), Some("readable\n"));
}
