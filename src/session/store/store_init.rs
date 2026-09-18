use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Result, ShoreError};
use crate::git::{git_path_is_untracked, git_paths_are_ignored};
pub(crate) use crate::paths::RepositoryPaths;
use crate::storage::{LocalStorage, TempSweepAge};

/// The store directory reads and writes for `repo` actually resolve to — the
/// shared common-dir store by default, or the worktree-local `.pointbreak/data` when
/// the worktree is Ephemeral. Delegates to the same resolver the read/write seams
/// use, so a library caller is never pointed at a different store than the CLI.
pub fn store_dir_for_repo(repo: &Path) -> Result<PathBuf> {
    Ok(crate::session::store::resolution::resolve_store(repo)?
        .store_dir()
        .to_path_buf())
}

/// Entries that establish whether a canonical worktree-local store contains data.
pub(crate) const STORE_CONTENT_MARKERS: &[&str] = &["events", "artifacts"];

/// True when `<store_dir>` (`<root>/.pointbreak/data`) holds a real worktree-local
/// store (any flat-store marker present), as opposed to an empty/absent dir. The
/// legacy guard on the normal read/write resolution path uses this to direct the
/// user to `pointbreak store migrate` when a worktree-local store predates the shared
/// store default. A config-only `.pointbreak/` (no events/artifacts under
/// `.pointbreak/data`) is not populated, and neither is a `.pointbreak/data` holding
/// only a leftover `state.json` from an earlier version: that file is inert.
pub(crate) fn worktree_local_store_is_populated(store_dir: &Path) -> bool {
    STORE_CONTENT_MARKERS
        .iter()
        .any(|marker| store_dir.join(marker).exists())
}

pub(crate) fn ensure_store_dirs(store_dir: &Path) -> Result<()> {
    for dir in [
        store_dir.join("events"),
        store_dir.join("artifacts/notes"),
        store_dir.join("artifacts/objects"),
    ] {
        fs::create_dir_all(&dir).map_err(|error| io_error("create directory", &dir, error))?;
    }
    Ok(())
}

pub(crate) fn sweep_stale_temp_files(storage: &LocalStorage, store_dir: &Path) -> Result<()> {
    storage.sweep_temp_files(store_dir, TempSweepAge::workflow_startup())
}

/// Shared writer setup against an explicit store dir and worktree root: sweep stale temp
/// files, ensure the store directory layout, and — only when the store lands inside the
/// worktree's `.pointbreak/` (the ephemeral opt-in) — ensure the committed `.pointbreak/.gitignore`
/// covers it. The shared common-dir store lives inside `.git/`, which git already
/// ignores, so a shared-store write generates nothing: a capture must never mutate the
/// worktree it is capturing (that would fork the content-only object id between a
/// worktree capture and a range capture of identical content). The write-landing seam's
/// `prepare_write_landing` calls this with the resolved write store dir and the worktree
/// root, so every write workflow shares one exclusion body.
pub(crate) fn prepare_store_writer_at(
    storage: &LocalStorage,
    store_dir: &Path,
    worktree_root: &Path,
) -> Result<()> {
    sweep_stale_temp_files(storage, store_dir)?;
    ensure_store_dirs(store_dir)?;
    if store_dir.starts_with(RepositoryPaths::from_worktree_root(worktree_root).config_dir()) {
        ensure_pointbreak_gitignore(worktree_root)?;
    }
    Ok(())
}

/// One canonical probe → line mapping for the committed `.pointbreak/.gitignore`.
/// `data/` covers the opt-in ephemeral store; `*.local.json` covers every
/// private `.local.json` override. Probes are worktree-relative paths checked
/// against ALL standard ignore sources, so user-managed ignore files are
/// respected and never duplicated.
fn pointbreak_gitignore_specs() -> Vec<(String, &'static str)> {
    let paths = RepositoryPaths::from_worktree_root(PathBuf::new());
    [
        (paths.worktree_store().join("events"), "data/"),
        (paths.delegates_local(), "*.local.json"),
        (paths.actor_attributes_local(), "*.local.json"),
        (paths.store_config_local(), "*.local.json"),
    ]
    .into_iter()
    .map(|(path, line)| (path.to_string_lossy().into_owned(), line))
    .collect()
}

/// The canonical gitignore lines Pointbreak can write into a fresh `.pointbreak/.gitignore`,
/// in generation order and deduplicated, derived from [`pointbreak_gitignore_specs`] so
/// the generator and the capture-suppression oracle share one source of truth.
/// Today: `data/` then `*.local.json`.
fn canonical_pointbreak_gitignore_lines() -> Vec<&'static str> {
    let mut lines: Vec<&'static str> = Vec::new();
    for (_, line) in pointbreak_gitignore_specs() {
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    lines
}

/// True when `body` is byte-identical to a `.pointbreak/.gitignore` Pointbreak itself could
/// have generated: non-empty, LF-terminated, and its lines form an ordered,
/// duplicate-free subsequence of [`canonical_pointbreak_gitignore_lines`]. Pure — no
/// git-ignore probing (an existing file covers its own probes, so a live probe
/// would self-contradict). A user-edited body (extra line, comment, reorder,
/// duplicate, blank line), any non-LF line ending (Pointbreak writes LF only, so a
/// `\r` stays attached to the split line and fails the exact match), or any body
/// without a trailing newline is rejected.
fn body_is_purely_pointbreak_generated(body: &str) -> bool {
    // Strip exactly the trailing LF, then split on LF only. `str::lines()` would
    // also swallow a `\r`, letting a CRLF body pass as if it were LF — which is not
    // byte-identical to what Shore generates.
    let Some(without_trailing_newline) = body.strip_suffix('\n') else {
        return false;
    };
    if without_trailing_newline.is_empty() {
        return false;
    }
    let canonical = canonical_pointbreak_gitignore_lines();
    let mut next = 0usize; // advancing cursor into `canonical` enforces order + no dupes
    for line in without_trailing_newline.split('\n') {
        let Some(offset) = canonical[next..]
            .iter()
            .position(|candidate| *candidate == line)
        else {
            return false;
        };
        next += offset + 1;
    }
    true
}

/// Keep Pointbreak's generated/private files out of Git status via a committed
/// `.pointbreak/.gitignore` — visible in the working tree, scoped to the directory,
/// and shared through clone — never by mutating the hidden, per-clone
/// `.git/info/exclude`. A path already ignored by any standard source is
/// skipped, so this is a no-op in a repo that manages its own ignores.
pub fn ensure_pointbreak_gitignore(worktree_root: &Path) -> Result<()> {
    let specs = pointbreak_gitignore_specs();
    let probes: Vec<&str> = specs.iter().map(|(probe, _)| probe.as_str()).collect();
    let ignored = git_paths_are_ignored(worktree_root, &probes)?;
    let mut missing: Vec<&str> = Vec::new();
    for ((_, line), is_ignored) in specs.iter().zip(ignored) {
        if !is_ignored && !missing.contains(line) {
            missing.push(line);
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    append_pointbreak_gitignore_lines(worktree_root, &missing)
}

/// True when `<worktree_root>/.pointbreak/.gitignore` is an **untracked** file whose
/// bytes are byte-identical to what Pointbreak itself generates. A tracked (committed)
/// file — clean or modified, even one edited back to a canonical body — a
/// user-edited untracked file, or an absent file all report false, so a real
/// reviewable change is never hidden. Reads the bytes first (a fast NotFound
/// short-circuit for the common no-file case) and applies the pure oracle before
/// the git probe, so the subprocess runs only for a genuinely Pointbreak-shaped file.
/// Do not reorder the git probe ahead of the pure check.
pub(crate) fn generated_gitignore_is_capture_suppressible(worktree_root: &Path) -> Result<bool> {
    let path = RepositoryPaths::from_worktree_root(worktree_root).gitignore();
    let body = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(io_error("read .pointbreak/.gitignore", &path, error)),
    };
    if !body_is_purely_pointbreak_generated(&body) {
        return Ok(false);
    }
    let relative = path
        .strip_prefix(worktree_root)
        .expect("repository gitignore remains under its worktree root")
        .to_str()
        .expect("canonical Pointbreak path is UTF-8");
    git_path_is_untracked(worktree_root, relative)
}

/// Absolute paths of Pointbreak-generated files a worktree capture should filter out of
/// its inventory right now — currently just `.pointbreak/.gitignore`, and only while it
/// is untracked and byte-identical to what Pointbreak generates. Returned as absolute
/// paths ready for [`crate::git::IngestOptions::exclude_helper_path`], which records
/// nothing in provenance, so the suppression never folds into the revision id.
/// Empty when nothing is suppressible.
pub(crate) fn pointbreak_generated_excluded_paths(worktree_root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    if generated_gitignore_is_capture_suppressible(worktree_root)? {
        paths.push(RepositoryPaths::from_worktree_root(worktree_root).gitignore());
    }
    Ok(paths)
}

/// Append `lines` (each newline-terminated) to `<worktree_root>/.pointbreak/.gitignore`,
/// creating `.pointbreak/` and the file as needed and normalizing a missing trailing
/// newline on existing content. Callers pass only not-yet-ignored lines.
fn append_pointbreak_gitignore_lines(worktree_root: &Path, lines: &[&str]) -> Result<()> {
    let path = RepositoryPaths::from_worktree_root(worktree_root).gitignore();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| io_error("create .pointbreak directory", parent, error))?;
    }
    let current = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(io_error("read .pointbreak/.gitignore", &path, error)),
    };
    let mut updated = current;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    for line in lines {
        updated.push_str(line);
        updated.push('\n');
    }
    fs::write(&path, updated)
        .map_err(|error| io_error("write .pointbreak/.gitignore", &path, error))
}

fn io_error(action: &str, path: &Path, error: std::io::Error) -> ShoreError {
    ShoreError::Message(format!("{action} {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::*;
    use crate::git::git_info_exclude_path;

    #[test]
    fn pointbreak_store_paths_resolve_from_subdirectory() {
        let repo = git_repo();
        fs::create_dir_all(repo.path().join("src/nested")).unwrap();
        let paths = RepositoryPaths::resolve(repo.path().join("src/nested")).unwrap();

        assert_existing_paths_eq(paths.worktree_root(), repo.path());
        // The store dir is now <root>/.pointbreak/data.
        assert_eq!(path_file_name(paths.worktree_store()), "data");
        assert_eq!(
            path_file_name(path_parent(paths.worktree_store())),
            ".pointbreak"
        );
        assert_existing_paths_eq(
            path_parent(path_parent(paths.worktree_store())),
            repo.path(),
        );
        // The gitignore probe for the store is <root>/.pointbreak/data/events.
        let probe = paths.worktree_store().join("events");
        assert_eq!(path_file_name(path_parent(probe.as_path())), "data");
        assert_eq!(
            path_file_name(path_parent(path_parent(probe.as_path()))),
            ".pointbreak"
        );
    }

    #[test]
    fn a_lone_state_json_is_not_a_populated_worktree_local_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join(".pointbreak/data");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(store.join("state.json"), "{}").unwrap();
        assert!(!worktree_local_store_is_populated(&store));
        std::fs::create_dir_all(store.join("events")).unwrap();
        assert!(worktree_local_store_is_populated(&store));
    }

    #[test]
    fn public_store_dir_helper_resolves_the_same_store_as_the_read_write_seams() {
        let repo = git_repo();

        let from_public_helper = store_dir_for_repo(repo.path()).unwrap();
        let from_resolver = crate::session::store::resolution::resolve_store(repo.path())
            .unwrap()
            .store_dir()
            .to_path_buf();

        assert_eq!(from_public_helper, from_resolver);
        // A fresh (non-ephemeral) repo resolves the shared common-dir store, not
        // the raw worktree-local `.pointbreak/data`.
        assert_eq!(path_file_name(&from_public_helper), "pointbreak");
    }

    #[test]
    fn public_store_dir_helper_resolves_the_user_level_family_store_when_bound() {
        use crate::session::store::store_config::set_family_binding_for_repo;
        use crate::session::store::user_level::{
            ensure_family_store_scaffold, user_level_store_dir,
        };

        let repo = git_repo();
        let home = tempfile::tempdir().unwrap();
        let (family_dir, from_public_helper, from_resolver) =
            crate::environment::test_support::with_home_override(home.path(), || {
                let slug = "acme-web";
                let family_dir = user_level_store_dir(slug).unwrap();
                ensure_family_store_scaffold(&family_dir, slug, &[]).unwrap();
                set_family_binding_for_repo(repo.path(), slug, "0123abcd4567ef89").unwrap();

                let from_public_helper = store_dir_for_repo(repo.path()).unwrap();
                let from_resolver = crate::session::store::resolution::resolve_store(repo.path())
                    .unwrap()
                    .store_dir()
                    .to_path_buf();
                (family_dir, from_public_helper, from_resolver)
            });

        assert_eq!(from_public_helper, from_resolver);
        // Both are computed from the same POINTBREAK_HOME root, so they are byte-equal.
        assert_eq!(from_public_helper, family_dir);
    }

    fn assert_existing_paths_eq(actual: &Path, expected: &Path) {
        assert_eq!(
            actual.canonicalize().expect("canonicalize actual path"),
            expected.canonicalize().expect("canonicalize expected path")
        );
    }

    fn path_parent(path: &Path) -> &Path {
        path.parent().expect("path has parent")
    }

    fn path_file_name(path: &Path) -> &str {
        path.file_name()
            .and_then(|name| name.to_str())
            .expect("path has utf-8 file name")
    }

    #[test]
    fn prepare_store_writer_at_creates_store_dirs_and_shore_gitignore() {
        let repo = git_repo();
        let paths = RepositoryPaths::resolve(repo.path()).unwrap();
        let storage = LocalStorage::new(paths.worktree_store());

        prepare_store_writer_at(&storage, paths.worktree_store(), paths.worktree_root()).unwrap();

        assert!(paths.worktree_store().join("events").is_dir());
        assert!(paths.worktree_store().join("artifacts/notes").is_dir());
        assert!(paths.worktree_store().join("artifacts/objects").is_dir());

        // Exclusion rides the committed .pointbreak/.gitignore — never the hidden
        // repo-local exclude and never the root .gitignore.
        assert!(
            !repo.path().join(".gitignore").exists(),
            "writer setup must not create a root .gitignore"
        );
        let body = fs::read_to_string(repo.path().join(".pointbreak/.gitignore")).unwrap();
        assert_eq!(body, "data/\n*.local.json\n");
        let exclude = git_info_exclude_path(repo.path()).unwrap();
        if exclude.exists() {
            let exclude_body = fs::read_to_string(&exclude).unwrap();
            assert!(
                !exclude_body.contains(".pointbreak"),
                "no .pointbreak entry lands in info/exclude: {exclude_body}"
            );
        }
    }

    #[test]
    fn prepare_store_writer_appends_missing_gitignore_lines_preserving_existing_body() {
        let repo = git_repo();
        let paths = RepositoryPaths::resolve(repo.path()).unwrap();
        let storage = LocalStorage::new(paths.worktree_store());

        // Seed a user-owned .pointbreak/.gitignore with NO trailing newline so the test
        // also exercises newline normalization. Its pattern ignores none of the
        // probe paths, so both canonical lines append after it.
        fs::create_dir_all(repo.path().join(".pointbreak")).unwrap();
        fs::write(
            repo.path().join(".pointbreak/.gitignore"),
            "# existing\nvendor-cache/",
        )
        .unwrap();

        prepare_store_writer_at(&storage, paths.worktree_store(), paths.worktree_root()).unwrap();

        // Assert the FULL file body: pre-existing content survives verbatim, the
        // missing trailing newline is normalized, and the canonical lines follow.
        let body = fs::read_to_string(repo.path().join(".pointbreak/.gitignore")).unwrap();
        assert_eq!(
            body, "# existing\nvendor-cache/\ndata/\n*.local.json\n",
            "must preserve the existing body verbatim and append the missing lines \
             with exact newline handling"
        );
    }

    #[test]
    fn prepare_store_writer_at_covers_probe_paths_and_keeps_committed_config_tracked() {
        let repo = git_repo();
        let paths = RepositoryPaths::resolve(repo.path()).unwrap();
        let storage = LocalStorage::new(paths.worktree_store());

        prepare_store_writer_at(&storage, paths.worktree_store(), paths.worktree_root()).unwrap();

        // The store dir and every private .local.json override are ignored…
        let ignored = git_paths_are_ignored(
            repo.path(),
            &[
                ".pointbreak/data/state.json",
                ".pointbreak/delegates.local.json",
                ".pointbreak/actor-attributes.local.json",
                ".pointbreak/store.local.json",
            ],
        )
        .unwrap();
        assert_eq!(ignored, vec![true, true, true, true]);
        // …while the committed config siblings stay tracked.
        let committed = git_paths_are_ignored(
            repo.path(),
            &[
                ".pointbreak/store.json",
                ".pointbreak/delegates.json",
                ".pointbreak/actor-attributes.json",
            ],
        )
        .unwrap();
        assert_eq!(committed, vec![false, false, false]);
    }

    #[test]
    fn prepare_store_writer_at_generates_nothing_for_the_shared_common_dir_store() {
        let repo = git_repo();
        // The shared store lives inside .git/, which git already ignores; a
        // shared-store write must not mutate the worktree (no generated
        // .pointbreak/.gitignore) — that is what keeps a capture from forking the
        // content-only object id of the worktree it is capturing.
        let store_dir = repo.path().join(".git/pointbreak");
        let storage = LocalStorage::new(&store_dir);

        prepare_store_writer_at(&storage, &store_dir, repo.path()).unwrap();

        assert!(store_dir.join("events").is_dir());
        assert!(
            !repo.path().join(".pointbreak/.gitignore").exists(),
            "a shared-store write generates no .pointbreak/.gitignore"
        );
        assert!(
            !repo.path().join(".pointbreak").exists(),
            "a shared-store write creates nothing under the worktree"
        );
    }

    #[test]
    fn prepare_store_writer_at_is_idempotent() {
        let repo = git_repo();
        let paths = RepositoryPaths::resolve(repo.path()).unwrap();
        let storage = LocalStorage::new(paths.worktree_store());

        // The probe reads the pre-append ignore state, so a second run must see the
        // now-covered probes as already-ignored and append nothing.
        prepare_store_writer_at(&storage, paths.worktree_store(), paths.worktree_root()).unwrap();
        prepare_store_writer_at(&storage, paths.worktree_store(), paths.worktree_root()).unwrap();

        let body = fs::read_to_string(repo.path().join(".pointbreak/.gitignore")).unwrap();
        for line in ["data/", "*.local.json"] {
            let hits = body.lines().filter(|l| l.trim() == line).count();
            assert_eq!(
                hits, 1,
                "{line} must be written at most once across repeated runs"
            );
        }
    }

    #[test]
    fn body_oracle_accepts_every_body_shore_can_generate() {
        // The three non-empty ordered subsequences of [data/, *.local.json].
        assert!(body_is_purely_pointbreak_generated("data/\n*.local.json\n"));
        assert!(body_is_purely_pointbreak_generated("data/\n"));
        assert!(body_is_purely_pointbreak_generated("*.local.json\n"));
    }

    #[test]
    fn body_oracle_rejects_user_touched_or_malformed_bodies() {
        assert!(!body_is_purely_pointbreak_generated(
            "data/\n*.local.json\nmine/\n"
        )); // extra line
        assert!(!body_is_purely_pointbreak_generated(
            "# mine\ndata/\n*.local.json\n"
        )); // comment
        assert!(!body_is_purely_pointbreak_generated(
            "*.local.json\ndata/\n"
        )); // reordered
        assert!(!body_is_purely_pointbreak_generated("data/\ndata/\n")); // duplicate
        assert!(!body_is_purely_pointbreak_generated(
            "data/\n\n*.local.json\n"
        )); // blank line
        assert!(!body_is_purely_pointbreak_generated("data/\n*.local.json")); // no trailing newline
        assert!(!body_is_purely_pointbreak_generated("")); // empty
        assert!(!body_is_purely_pointbreak_generated("\n")); // lone newline
    }

    #[test]
    fn body_oracle_rejects_non_lf_line_endings() {
        // Shore writes LF only; a CRLF or bare-CR body is not byte-identical, even
        // though its visible lines match. `str::lines()` would strip the `\r` — the
        // oracle must not, or a user-touched CRLF file could be wrongly suppressed.
        assert!(!body_is_purely_pointbreak_generated(
            "data/\r\n*.local.json\r\n"
        )); // CRLF
        assert!(!body_is_purely_pointbreak_generated("data/\r\n")); // CRLF, single line
        assert!(!body_is_purely_pointbreak_generated(
            "data/\r*.local.json\n"
        )); // bare CR separator
    }

    #[test]
    fn untracked_canonical_gitignore_is_suppressible() {
        let repo = git_repo();
        ensure_pointbreak_gitignore(repo.path()).unwrap(); // writes canonical, untracked
        assert!(generated_gitignore_is_capture_suppressible(repo.path()).unwrap());
        assert_eq!(
            pointbreak_generated_excluded_paths(repo.path()).unwrap(),
            vec![repo.path().join(".pointbreak/.gitignore")]
        );
    }

    #[test]
    fn absent_gitignore_is_not_suppressible() {
        let repo = git_repo();
        assert!(!generated_gitignore_is_capture_suppressible(repo.path()).unwrap());
        assert!(
            pointbreak_generated_excluded_paths(repo.path())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn user_edited_untracked_gitignore_is_not_suppressible() {
        let repo = git_repo();
        fs::create_dir_all(repo.path().join(".pointbreak")).unwrap();
        fs::write(
            repo.path().join(".pointbreak/.gitignore"),
            "data/\n*.local.json\nmine/\n",
        )
        .unwrap();
        assert!(!generated_gitignore_is_capture_suppressible(repo.path()).unwrap());
        assert!(
            pointbreak_generated_excluded_paths(repo.path())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn committed_gitignore_is_not_suppressible_even_when_canonical() {
        let repo = git_repo();
        fs::create_dir_all(repo.path().join(".pointbreak")).unwrap();
        fs::write(
            repo.path().join(".pointbreak/.gitignore"),
            "data/\n*.local.json\n",
        )
        .unwrap();
        commit_all(&repo); // now TRACKED, byte-identical to canonical
        // Byte-oracle passes, but the untracked gate fails ⇒ not suppressible.
        assert!(!generated_gitignore_is_capture_suppressible(repo.path()).unwrap());
        assert!(
            pointbreak_generated_excluded_paths(repo.path())
                .unwrap()
                .is_empty()
        );
    }

    /// Commit everything in a `git_repo()` (which has no identity configured).
    fn commit_all(repo: &tempfile::TempDir) {
        for args in [
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
            vec!["config", "commit.gpgsign", "false"],
            vec!["add", "--all"],
            vec!["commit", "-m", "x"],
        ] {
            let out = Command::new("git")
                .args(&args)
                .current_dir(repo.path())
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?} failed");
        }
    }

    #[test]
    fn ensure_pointbreak_gitignore_writes_the_two_canonical_lines() {
        let repo = git_repo();
        ensure_pointbreak_gitignore(repo.path()).unwrap();
        let body = fs::read_to_string(repo.path().join(".pointbreak/.gitignore")).unwrap();
        assert_eq!(body, "data/\n*.local.json\n");
        // The mechanism is the committed .pointbreak/.gitignore — never the repo-local
        // exclude and never the root .gitignore. (`git init` may seed a commented
        // info/exclude template, so assert on content, not existence.)
        let exclude = git_info_exclude_path(repo.path()).unwrap();
        if exclude.exists() {
            let exclude_body = fs::read_to_string(&exclude).unwrap();
            assert!(
                !exclude_body.contains(".pointbreak"),
                "no .pointbreak entry lands in info/exclude: {exclude_body}"
            );
        }
        assert!(!repo.path().join(".gitignore").exists());
    }

    #[test]
    fn ensure_pointbreak_gitignore_is_idempotent() {
        let repo = git_repo();
        ensure_pointbreak_gitignore(repo.path()).unwrap();
        ensure_pointbreak_gitignore(repo.path()).unwrap();
        let body = fs::read_to_string(repo.path().join(".pointbreak/.gitignore")).unwrap();
        assert_eq!(
            body, "data/\n*.local.json\n",
            "each line is written at most once"
        );
    }

    #[test]
    fn ensure_pointbreak_gitignore_appends_missing_lines_preserving_user_content() {
        let repo = git_repo();
        // A user-owned .pointbreak/.gitignore that already covers the data/ store (via
        // its own spelling) but not the local overrides, with no trailing newline
        // so the append also exercises newline normalization.
        fs::create_dir_all(repo.path().join(".pointbreak")).unwrap();
        fs::write(repo.path().join(".pointbreak/.gitignore"), "# mine\ndata").unwrap();
        ensure_pointbreak_gitignore(repo.path()).unwrap();
        let body = fs::read_to_string(repo.path().join(".pointbreak/.gitignore")).unwrap();
        // The user's `data` line (no slash) is a basename pattern that matches the
        // data directory, so the .pointbreak/data/state.json probe reports ignored and
        // only *.local.json is appended; existing content survives verbatim.
        assert_eq!(body, "# mine\ndata\n*.local.json\n");
    }

    #[test]
    fn ensure_pointbreak_gitignore_is_a_noop_when_probes_are_already_ignored() {
        let repo = git_repo();
        // Any standard ignore source counts — here the root .gitignore covers both
        // the store dir and the local overrides, so nothing is written.
        fs::write(
            repo.path().join(".gitignore"),
            ".pointbreak/data/\n.pointbreak/*.local.json\n",
        )
        .unwrap();
        ensure_pointbreak_gitignore(repo.path()).unwrap();
        assert!(
            !repo.path().join(".pointbreak/.gitignore").exists(),
            "user-managed ignore files are respected; no file is generated"
        );
    }

    #[test]
    fn shore_gitignore_covers_all_four_probe_paths() {
        let repo = git_repo();
        ensure_pointbreak_gitignore(repo.path()).unwrap();
        let ignored = crate::git::git_paths_are_ignored(
            repo.path(),
            &[
                ".pointbreak/data/state.json",
                ".pointbreak/delegates.local.json",
                ".pointbreak/actor-attributes.local.json",
                ".pointbreak/store.local.json",
            ],
        )
        .unwrap();
        assert_eq!(ignored, vec![true, true, true, true]);
        // The committed config siblings are never ignored.
        let committed = crate::git::git_paths_are_ignored(
            repo.path(),
            &[".pointbreak/store.json", ".pointbreak/delegates.json"],
        )
        .unwrap();
        assert_eq!(committed, vec![false, false]);
    }

    #[test]
    fn prepare_store_writer_at_preserves_fresh_temp_files() {
        let repo = git_repo();
        let paths = RepositoryPaths::resolve(repo.path()).unwrap();
        fs::create_dir_all(paths.worktree_store().join("events")).unwrap();
        let temp = paths.worktree_store().join("events/.shore-write.fresh.tmp");
        fs::write(&temp, "in flight").unwrap();
        let storage = LocalStorage::new(paths.worktree_store());

        prepare_store_writer_at(&storage, paths.worktree_store(), paths.worktree_root()).unwrap();

        assert_eq!(fs::read_to_string(temp).unwrap(), "in flight");
    }

    #[test]
    fn migrated_nested_store_resolves_cleanly() {
        let repo = git_repo();
        // Post-migration steady state: only the nested store, no flat markers.
        fs::create_dir_all(repo.path().join(".pointbreak/data/events")).unwrap();
        let paths = RepositoryPaths::resolve(repo.path()).expect("nested store resolves");
        assert_eq!(path_file_name(paths.worktree_store()), "data");
    }

    #[test]
    fn config_only_pointbreak_dir_resolves_cleanly() {
        let repo = git_repo();
        // .pointbreak/ holds ONLY committed config (no store yet). Must NOT trip the
        // legacy guard — committed config now legitimately lives under .pointbreak/.
        fs::create_dir_all(repo.path().join(".pointbreak")).unwrap();
        fs::write(
            repo.path().join(".pointbreak/delegates.json"),
            r#"{"delegates":{}}"#,
        )
        .unwrap();
        let paths =
            RepositoryPaths::resolve(repo.path()).expect("config-only .pointbreak/ resolves");
        assert_eq!(path_file_name(paths.worktree_store()), "data");
    }

    #[test]
    fn fresh_repo_with_no_pointbreak_dir_resolves_cleanly() {
        let repo = git_repo();
        let paths = RepositoryPaths::resolve(repo.path()).expect("fresh repo resolves");
        assert_eq!(path_file_name(paths.worktree_store()), "data");
    }

    fn git_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().expect("create temp git repository directory");
        let output = Command::new("git")
            .arg("init")
            .current_dir(repo.path())
            .output()
            .expect("run git init");
        assert!(
            output.status.success(),
            "git init failed in {}:\nstdout:\n{}\nstderr:\n{}",
            repo.path().display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        repo
    }
}
