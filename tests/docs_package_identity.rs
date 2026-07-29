//! Durable package- and product-identity contracts for the public surfaces.
//!
//! The `shore` -> `pointbreak` cutover guards that used to live here (a legacy-word
//! scanner over every doc, skill, and script, plus the 0.7.0 hard-cutover narrative)
//! were retired once that rename landed. What remains are the invariants that stay
//! true release to release: the crate and command identity, the canonical
//! organization repository, and the packaging metadata.

#[test]
fn readme_teaches_the_pointbreak_package_and_command() {
    let readme = std::fs::read_to_string("README.md").expect("read README");

    assert!(readme.contains("cargo install pointbreak"));
    assert!(readme.contains("provides the `pointbreak` command"));
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
fn just_run_targets_the_pointbreak_binary() {
    let justfile = std::fs::read_to_string("Justfile").expect("read Justfile");

    assert!(justfile.contains("{{ cargo_stable }} run --bin pointbreak -- {{ args }}"));
}
