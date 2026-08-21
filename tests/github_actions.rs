#[test]
fn ci_workflow_runs_project_lint_and_tests() {
    let ci = std::fs::read_to_string(".github/workflows/ci.yml").expect("read CI workflow");

    assert!(ci.contains("name: CI"));
    assert!(ci.contains("branches: [main]"));
    assert!(ci.contains("pull_request:"));
    assert!(ci.contains("actions/checkout@v6"));
    assert!(ci.contains("dtolnay/rust-toolchain@stable"));
    assert!(ci.contains("dtolnay/rust-toolchain@nightly"));
    assert!(ci.contains("taiki-e/install-action@just"));
    assert!(ci.contains("taiki-e/install-action@nextest"));
    assert!(ci.contains("ubuntu-latest"));
    assert!(ci.contains("macos-latest"));
    assert!(ci.contains("windows-latest"));
    assert!(ci.contains("run: just lint"));
    assert!(ci.contains("run: just test-ci"));
}

#[test]
fn release_workflows_target_single_pointbreak_crate() {
    let release_plan =
        std::fs::read_to_string(".github/workflows/release-plan.yml").expect("read release plan");
    let release = std::fs::read_to_string(".github/workflows/release.yml").expect("read release");
    let release_script =
        std::fs::read_to_string("scripts/run-release-plan.sh").expect("read release script");
    let finalize_tag = std::fs::read_to_string("scripts/finalize-cocogitto-release-tag.sh")
        .expect("read Cocogitto tag finalizer");
    let finalize_tag_selftest =
        std::fs::read_to_string("scripts/finalize-cocogitto-release-tag-selftest.sh")
            .expect("read Cocogitto tag finalizer selftest");
    let cog = std::fs::read_to_string("cog.toml").expect("read cog config");

    assert!(release_plan.contains("select(.name == \"pointbreak\")"));
    assert!(release_plan.contains("sort_by(.createdAt)"));
    assert!(release_plan.contains("RELEASE_COG_CONFIG"));
    assert!(release.contains("cargo publish -p pointbreak --locked"));
    assert!(release.contains("https://crates.io/api/v1/crates/pointbreak/${VERSION}"));
    assert!(release_script.contains("RELEASE_PLAN_REPO"));
    assert!(release_script.contains("remote get-url origin"));
    assert!(release_script.contains(
        r#"just --justfile "$REPO_ROOT/Justfile" --working-directory "$REPO_ROOT" package-archive-selftest"#
    ));
    let stale_repository = ["kevinswiber", "pointbreak"].join("/");
    assert!(!release_script.contains(&stale_repository));
    assert!(!release.contains("boardwalk"));
    assert!(cog.contains(r#""git commit --amend -m 'chore: v{{version}}'""#));
    assert!(cog.contains(r#""scripts/finalize-cocogitto-release-tag.sh v{{version}}""#));
    assert!(!cog.contains(r#""git tag -m 'v{{version}}' v{{version}}""#));
    assert!(!cog.contains("git tag -f"));
    assert!(cog.contains(r#""git push origin HEAD:main""#));
    assert!(cog.contains(r#""git push origin refs/tags/v{{version}}""#));
    assert!(!cog.contains("gh workflow run release.yml"));
    assert!(!cog.contains("gh workflow run release-binaries.yml"));
    for required in [
        "git cat-file -t \"$TAG_REF\"",
        "git ls-remote --exit-code --tags origin \"$TAG_REF\"",
        "git rev-parse \"${COG_TAG_COMMIT}^{tree}\"",
        "git rev-parse \"${RELEASE_COMMIT}^{tree}\"",
        "git rev-parse \"${COG_TAG_COMMIT}^\"",
        "git rev-parse \"${RELEASE_COMMIT}^\"",
        "git update-ref -d \"$TAG_REF\" \"$COG_TAG_COMMIT\"",
        "git tag -s -m \"$TAG\" \"$TAG\"",
        "git verify-tag \"$TAG\"",
    ] {
        assert!(
            finalize_tag.contains(required),
            "tag finalizer is missing: {required}"
        );
    }
    assert!(!finalize_tag.contains("git tag -f"));
    assert!(finalize_tag_selftest.contains("finalize-cocogitto-release-tag.sh"));
    assert!(finalize_tag_selftest.contains("verify-commit"));
    assert!(finalize_tag_selftest.contains("verify-tag"));
    assert!(finalize_tag_selftest.contains("refs/tags/v0.8.0"));
    assert!(cog.contains(r#"repository = "pointbreak""#));
    assert!(cog.contains(r#"owner = "withpointbreak""#));
    let stale_owner = ["owner = \"", "kevinswiber", "\""].join("");
    assert!(!cog.contains(&stale_owner));
}

#[test]
fn release_workflows_bind_the_reviewed_parent_and_keep_published_state_immutable() {
    let release_plan =
        std::fs::read_to_string(".github/workflows/release-plan.yml").expect("read release plan");
    let release = std::fs::read_to_string(".github/workflows/release.yml").expect("read release");
    let binaries = std::fs::read_to_string(".github/workflows/release-binaries.yml")
        .expect("read release binaries");
    let verify = std::fs::read_to_string(".github/workflows/verify-release.yml")
        .expect("read post-publish verification workflow");
    let helper =
        std::fs::read_to_string("scripts/run-release-plan.sh").expect("read release helper");
    let identity = std::fs::read_to_string("scripts/assert-release-identity.sh")
        .expect("read release identity assertion");
    let docs = std::fs::read_to_string("docs/releasing.md").expect("read release docs");

    assert!(release_plan.contains("expected_source_commit:"));
    assert!(release_plan.contains("required: true"));
    assert!(release_plan.contains("EXPECTED_SOURCE_COMMIT"));
    assert!(release_plan.contains("git rev-parse origin/main"));
    assert!(release_plan.contains("git rev-parse HEAD^"));
    assert!(release_plan.contains("git rev-parse \"refs/tags/${TAG}^{}\""));
    assert!(release_plan.contains("Existing tag conflict"));
    assert!(release_plan.contains("Existing crate conflict"));
    assert!(release_plan.contains("Existing GitHub release conflict"));
    assert!(
        release_plan.contains(
            "actions/create-github-app-token@bcd2ba49218906704ab6c1aa796996da409d3eb1 # v3"
        )
    );
    assert!(release_plan.contains("client-id: ${{ secrets.RELEASE_APP_CLIENT_ID }}"));
    assert!(release_plan.contains("private-key: ${{ secrets.RELEASE_APP_PRIVATE_KEY }}"));
    assert!(release_plan.contains("permission-contents: write"));
    assert!(release_plan.contains("token: ${{ steps.release-token.outputs.token }}"));
    assert_eq!(release_plan.matches("Verify release bump hooks").count(), 2);
    assert_eq!(
        release_plan.matches("just release-bump-selftest").count(),
        2
    );
    assert!(!release_plan.contains("RELEASE_PUSH_TOKEN"));
    assert!(release_plan.contains("select(.headSha == $expected)"));
    assert!(release_plan.contains("unexpected crates.io status"));
    assert!(release_plan.contains("pointbreak-release-workflow/1.0"));
    assert!(release_plan.contains("unexpected GitHub release status"));
    assert!(docs.contains("RELEASE_APP_CLIENT_ID"));
    assert!(docs.contains("RELEASE_APP_PRIVATE_KEY"));
    assert!(!docs.contains("RELEASE_PUSH_TOKEN"));

    assert!(helper.contains("<plan|release> <version> --expected-source <full-sha>"));
    assert!(helper.contains("expected_source_commit=${EXPECTED_SOURCE_COMMIT}"));
    assert!(helper.contains("git -C \"$REPO_ROOT\" rev-parse origin/main"));

    for tag_only_workflow in [&release, &binaries] {
        assert!(!tag_only_workflow.contains("workflow_dispatch:"));
        assert!(tag_only_workflow.contains("fetch-depth: 0"));
    }
    assert!(release.contains("GitHub release already exists"));
    assert!(release.contains("unexpected crates.io status"));
    assert!(release.contains("unexpected GitHub release status"));
    assert!(!release.contains("already published; skipping"));
    assert!(binaries.contains("fetch-tags: true"));
    assert!(binaries.contains("scripts/assert-release-identity.sh"));
    assert!(binaries.contains("overwrite_files: false"));
    assert!(!binaries.contains("overwrite_files: true"));

    assert!(identity.contains(".build.source == \"git\""));
    assert!(identity.contains(".build.commit == $commit"));
    assert!(identity.contains(".build.describe == $tag"));
    assert!(identity.contains(".build.dirty == false"));

    for required in [
        "ubuntu-latest",
        "macos-latest",
        "windows-latest",
        "alpine",
        "scripts/install.sh",
        "scripts/install.ps1",
        "release-verification",
        "isPrerelease == $prerelease",
    ] {
        assert!(
            verify.contains(required),
            "missing verification term: {required}"
        );
    }
}

/// Read a workflow file from `.github/workflows/` by file name.
fn read_workflow(file_name: &str) -> String {
    let path = format!(".github/workflows/{file_name}");
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {path}: {err}"))
}

/// Name and contents of every workflow file in `.github/workflows/`.
fn workflow_sources() -> Vec<(String, String)> {
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(".github/workflows").expect("list workflow directory") {
        let path = entry.expect("read workflow directory entry").path();
        if path.extension().is_some_and(|extension| extension == "yml") {
            let name = path
                .file_name()
                .expect("workflow file name")
                .to_string_lossy()
                .into_owned();
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
            sources.push((name, text));
        }
    }
    sources
}

/// Whether a line declares a top-level job key: exactly two spaces of indent, a
/// lowercase kebab-case name, and a trailing colon with nothing after it.
fn is_job_key_line(line: &str) -> bool {
    let Some(key) = line
        .strip_prefix("  ")
        .and_then(|rest| rest.strip_suffix(':'))
    else {
        return false;
    };
    key.starts_with(|c: char| c.is_ascii_lowercase())
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The byte range of the top-level `jobs:` mapping: from the line after `jobs:` to
/// the next column-zero key or end of file. Anchoring here is what keeps two-space
/// keys under `on:` or `env:` from ever masquerading as jobs.
fn jobs_section(workflow: &str) -> &str {
    let start = if let Some(rest) = workflow.strip_prefix("jobs:\n") {
        workflow.len() - rest.len()
    } else {
        let marker = "\njobs:\n";
        let at = workflow.find(marker).expect("workflow has a jobs: mapping");
        at + marker.len()
    };
    let section = &workflow[start..];
    let mut offset = 0;
    for line in section.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if !trimmed.is_empty() && !trimmed.starts_with(' ') && !trimmed.starts_with('#') {
            return &section[..offset];
        }
        offset += line.len();
    }
    section
}

/// The top-level job keys a workflow declares, in file order.
fn job_keys(workflow: &str) -> Vec<String> {
    jobs_section(workflow)
        .lines()
        .filter(|line| is_job_key_line(line))
        .map(|line| line.trim().trim_end_matches(':').to_string())
        .collect()
}

/// The text of one top-level job's block, from its key line to the next top-level
/// job key, so a per-job assertion cannot be satisfied by a match elsewhere in the
/// workflow file.
fn job_block<'a>(workflow: &'a str, job_key: &str) -> &'a str {
    let section = jobs_section(workflow);
    let header = format!("  {job_key}:");
    let mut start = None;
    let mut offset = 0;
    for line in section.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if let Some(block_start) = start {
            if is_job_key_line(trimmed) {
                return &section[block_start..offset];
            }
        } else if trimmed == header {
            start = Some(offset);
        }
        offset += line.len();
    }
    let start = start.unwrap_or_else(|| panic!("job {job_key} not declared in the workflow"));
    &section[start..]
}

/// Where a CI job is declared to run. `Nightly` jobs live in a scheduled workflow
/// that must also be runnable on demand; every other job is declared in `ci.yml`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tier {
    PerPush,
    /// Constructed once a job leaves the per-push tier; declared ahead of that so
    /// moving a job is a one-line table edit rather than new test machinery.
    #[allow(dead_code)]
    Nightly,
}

/// Every CI job and the tier it belongs to. Changing a job's tier is a deliberate
/// edit to this table, reviewed alongside the workflow change that causes it.
const CI_JOB_TIERS: &[(&str, Tier)] = &[
    ("installer-selftest", Tier::PerPush),
    ("validate-skills", Tier::PerPush),
    ("lint-workflows", Tier::PerPush),
    ("web-check", Tier::PerPush),
    ("extension-check", Tier::PerPush),
    ("test", Tier::PerPush),
    ("store-foundation-qualification", Tier::PerPush),
    ("git-parity", Tier::PerPush),
];

#[test]
fn ci_workflow_declares_every_job_in_the_tier_table() {
    let ci = read_workflow("ci.yml");
    let declared = job_keys(&ci);
    let declared: Vec<&str> = declared.iter().map(String::as_str).collect();
    let expected: Vec<&str> = CI_JOB_TIERS
        .iter()
        .filter(|(_, tier)| *tier != Tier::Nightly)
        .map(|(job, _)| *job)
        .collect();

    let missing: Vec<&&str> = expected
        .iter()
        .filter(|job| !declared.contains(job))
        .collect();
    let untiered: Vec<&&str> = declared
        .iter()
        .filter(|job| !expected.contains(job))
        .collect();
    assert!(
        missing.is_empty() && untiered.is_empty(),
        "ci.yml jobs and the tier table diverge; \
         in the table but missing from ci.yml: {missing:?}; \
         in ci.yml but not in the table: {untiered:?}"
    );
}

#[test]
fn every_job_that_left_the_per_push_tier_has_a_home() {
    for (job, tier) in CI_JOB_TIERS {
        if *tier == Tier::PerPush {
            continue;
        }
        let home = workflow_sources().into_iter().find(|(_, text)| {
            text.contains("workflow_dispatch:") && job_keys(text).iter().any(|key| key == job)
        });
        assert!(
            home.is_some(),
            "job {job} left the per-push tier but no workflow with workflow_dispatch declares it"
        );
    }
}

#[test]
fn ci_pins_the_nextest_version_and_retains_timing_reports() {
    let ci = read_workflow("ci.yml");
    assert!(
        ci.contains("tool: nextest@"),
        "ci.yml must pin the nextest version; an unpinned install silently \
         changes the runner between timing measurements"
    );
    assert!(
        ci.contains("actions/upload-artifact"),
        "ci.yml must upload the per-run test timing report"
    );

    let profile =
        std::fs::read_to_string(".config/nextest.toml").expect("read nextest configuration");
    assert!(
        profile.contains("[profile.ci.junit]"),
        "the ci nextest profile must record per-test timings"
    );
}

#[test]
fn every_ci_job_declares_a_timeout() {
    for (file, text) in workflow_sources() {
        for job in job_keys(&text) {
            assert!(
                job_block(&text, &job).contains("timeout-minutes:"),
                "{file} job {job} declares no timeout-minutes, \
                 so a hang runs to the six-hour default"
            );
        }
    }
}

#[test]
fn nightly_workflow_runs_the_feature_on_suite_and_is_dispatchable() {
    let nightly = read_workflow("nightly.yml");
    assert!(nightly.contains("schedule:"));
    assert!(
        nightly.contains("workflow_dispatch:"),
        "a scheduled lane nobody can trigger by hand is a lane nobody can debug"
    );
    assert!(nightly.contains("run: just test-full"));
    assert!(nightly.contains("ubuntu-latest"));
    assert!(
        !nightly.contains("--include-ignored"),
        "ignored tests are ignored deliberately; the marker on the test is the \
         only record of why, and this lane must not override it"
    );
}

#[test]
fn development_guide_names_the_command_ci_actually_runs() {
    let guide = std::fs::read_to_string("docs/development.md").expect("read development guide");
    let ci = read_workflow("ci.yml");

    assert!(ci.contains("just workflow-lint-assertions"));
    assert!(
        guide.contains("just workflow-lint-assertions"),
        "the guide must name the assertions recipe the CI lint-workflows job runs, \
         not only the local umbrella recipe"
    );
    assert!(
        guide.contains("nightly.yml") && guide.contains("workflow_dispatch"),
        "the scheduled full-suite lane must be discoverable from the gate guide, \
         including how to trigger it by hand"
    );
}

#[test]
fn job_block_stops_at_the_next_job_key() {
    let workflow = "name: sample\njobs:\n  first:\n    runs-on: ubuntu-latest\n    steps:\n      - run: alpha\n  second:\n    runs-on: ubuntu-latest\n    steps:\n      - run: beta\n";

    let first = job_block(workflow, "first");
    assert!(first.contains("run: alpha"));
    assert!(!first.contains("second:"));
    assert!(!first.contains("run: beta"));

    let second = job_block(workflow, "second");
    assert!(second.contains("run: beta"));
    assert!(!second.contains("run: alpha"));
}

#[test]
fn changelog_has_cocogitto_insertion_separator() {
    let changelog = std::fs::read_to_string("CHANGELOG.md").expect("read changelog");

    assert!(
        changelog.lines().any(|line| line == "- - -"),
        "Cocogitto release bumps need the default insertion separator"
    );
}

#[test]
fn commit_check_workflow_reports_shore_examples() {
    let commit_check =
        std::fs::read_to_string(".github/workflows/commit-check.yml").expect("read commit check");

    assert!(commit_check.contains("name: Commit Check"));
    assert!(commit_check.contains("cog check \"${RANGE}\""));
    assert!(commit_check.contains("feat: add review unit discovery"));
    assert!(commit_check.contains("fix: correct input request projection"));
    assert!(!commit_check.contains("boardwalk"));
}
