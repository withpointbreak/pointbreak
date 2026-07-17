use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use crate::error::{Result, ShoreError};

#[derive(Debug)]
pub(crate) struct GitOutput {
    pub stdout: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitWorktree {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
}

/// Three-valued ancestry from `merge-base --is-ancestor`, which signals only via
/// exit code with empty stdout: 0 ancestor, 1 not, 128 a missing/bad object. A
/// gc'd or absent object is a value here, never an error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Ancestry {
    Ancestor,
    NotAncestor,
    MissingObject,
}

/// One ref tip from `for-each-ref`: the full ref name and the OID it points at.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RefEntry {
    pub name: String,
    pub oid: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitInventoryPath {
    bytes: Vec<u8>,
}

impl GitInventoryPath {
    fn new(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.to_vec(),
        }
    }

    pub(crate) fn into_utf8_string(self, description: &str) -> Result<String> {
        String::from_utf8(self.bytes)
            .map_err(|error| ShoreError::Message(format!("{description} is not utf-8: {error}")))
    }
}

pub(crate) fn run_git<I, S>(cwd: &Path, args: I) -> Result<GitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run_git_allowing_statuses(cwd, args, &[0])
}

/// Invariant repository facts that Git resolves from disk but that never change
/// for a live repository within a single process: the worktree root, the common
/// Git directory, and the path to `info/exclude`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RepoFact {
    WorktreeRoot,
    CommonDir,
    #[cfg(test)]
    InfoExcludePath,
}

/// Memoizes [`RepoFact`] lookups keyed by the working directory passed to Git.
///
/// Pointbreak re-derives these facts many times across one capture/ingest — store
/// resolution alone resolves the worktree root ~10 times for a single repository
/// — and each call previously spawned a fresh `git rev-parse`. Process spawning
/// is the dominant cost in the `sys`-bound test suite and in every CLI
/// invocation, so collapsing the repeats to one spawn per repository is a real
/// latency win for both.
///
/// The memo is sound because these three facts are immutable for a given
/// repository as long as it exists: Pointbreak never relocates a repository
/// mid-process, and the `info/exclude` *path* (not its mutable contents) is fixed
/// by the layout. Only successful lookups are cached, so a transient failure is
/// never memoized. Keys are canonicalized absolute paths (see [`repo_fact_key`]),
/// so different spellings of one repository — a relative `.`, a symlinked
/// temporary directory — collapse to a single entry and can never alias two
/// distinct repositories. Concurrent first callers may each resolve once: the
/// lock is released across the (subprocess) lookup rather than single-flighting
/// it, which at worst duplicates a spawn and never returns a wrong value.
fn repo_fact_cache() -> &'static Mutex<HashMap<(PathBuf, RepoFact), PathBuf>> {
    static CACHE: OnceLock<Mutex<HashMap<(PathBuf, RepoFact), PathBuf>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Canonicalizes `repo` into a cache key so every spelling of one repository — a
/// relative `.`, a symlinked temporary directory — maps to the same entry and two
/// distinct repositories never collide. Falls back to the raw path when
/// canonicalization fails (e.g. the directory does not exist yet), in which case
/// the lookup fails and nothing is cached.
fn repo_fact_key(repo: &Path, fact: RepoFact) -> (PathBuf, RepoFact) {
    let path = std::fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
    (path, fact)
}

fn cached_repo_fact(
    repo: &Path,
    fact: RepoFact,
    resolve: impl FnOnce() -> Result<PathBuf>,
) -> Result<PathBuf> {
    let key = repo_fact_key(repo, fact);
    {
        let cache = repo_fact_cache()
            .lock()
            .expect("repo fact cache mutex is not poisoned");
        if let Some(hit) = cache.get(&key) {
            return Ok(hit.clone());
        }
    }

    // Resolve outside the lock: the guard above is dropped with its block, so the
    // (process-spawning) lookup never runs while holding the mutex.
    let value = resolve()?;
    repo_fact_cache()
        .lock()
        .expect("repo fact cache mutex is not poisoned")
        .insert(key, value.clone());
    Ok(value)
}

pub fn git_worktree_root(repo: &Path) -> Result<PathBuf> {
    cached_repo_fact(repo, RepoFact::WorktreeRoot, || {
        let output = run_git(repo, ["rev-parse", "--show-toplevel"])?;
        git_stdout_path(repo, &output.stdout, "worktree root")
    })
}

pub(crate) fn git_common_dir(repo: &Path) -> Result<PathBuf> {
    cached_repo_fact(repo, RepoFact::CommonDir, || {
        let output = match run_git(
            repo,
            ["rev-parse", "--path-format=absolute", "--git-common-dir"],
        ) {
            Ok(output) => output,
            Err(error) if git_path_format_is_unsupported(&error) => {
                return git_common_dir_without_path_format(repo);
            }
            Err(error) => return Err(error),
        };
        git_stdout_path(repo, &output.stdout, "git common-dir")
    })
}

fn git_common_dir_without_path_format(repo: &Path) -> Result<PathBuf> {
    let output = run_git(repo, ["rev-parse", "--git-common-dir"])?;
    let path = git_stdout_path(repo, &output.stdout, "git common-dir")?;
    absolute_git_cwd_path(repo, path)
}

fn git_path_format_is_unsupported(error: &ShoreError) -> bool {
    let ShoreError::GitCommand { stderr, .. } = error else {
        return false;
    };

    stderr.contains("--path-format")
        || stderr.contains("unknown option")
        || stderr.contains("unknown switch")
}

fn absolute_git_cwd_path(repo: &Path, path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }

    let cwd = if repo.is_absolute() {
        repo.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| ShoreError::Message(format!("resolve current directory: {error}")))?
            .join(repo)
    };
    let candidate = cwd.join(path);
    candidate.canonicalize().map_err(|error| {
        ShoreError::Message(format!(
            "canonicalize git common-dir {}: {error}",
            candidate.display()
        ))
    })
}

#[cfg(test)]
pub fn git_info_exclude_path(repo: &Path) -> Result<PathBuf> {
    cached_repo_fact(repo, RepoFact::InfoExcludePath, || {
        let output = run_git(repo, ["rev-parse", "--git-path", "info/exclude"])?;
        let relative = git_stdout_path(repo, &output.stdout, "info/exclude path")?;

        // `git rev-parse --git-path` resolves against the working directory we ran
        // it from (the worktree root). Joining keeps relative results anchored to
        // `repo` while preserving absolute results (linked worktrees share the
        // common `info/exclude`), since `Path::join` discards the base for an
        // absolute child.
        Ok(repo.join(relative))
    })
}

/// Reports, for each pathspec, whether it is ignored by the standard Git
/// exclude sources (the worktree `.gitignore`, the global excludes file, and
/// the repository `.git/info/exclude`), in a single `git check-ignore`
/// invocation — mirroring the `--exclude-standard` rules used when Pointbreak
/// discovers untracked files. Returns one bool per input path, in input order.
///
/// Pathspecs are passed as arguments (not via `--stdin`), so plain check-ignore echoes
/// the ignored subset one per `\n`-delimited line; this is exact for newline-free
/// pathspecs, which the store-exclude paths are. (`-z` is rejected outside `--stdin`
/// mode, so it cannot be used here.)
pub(crate) fn git_paths_are_ignored(repo: &Path, pathspecs: &[&str]) -> Result<Vec<bool>> {
    if pathspecs.is_empty() {
        return Ok(Vec::new());
    }
    let git_pathspecs: Vec<String> = pathspecs
        .iter()
        .map(|path| git_pathspec_for_separator(path, std::path::MAIN_SEPARATOR))
        .collect();
    // Plain (non-verbose) check-ignore prints only the SUBSET that is ignored, each
    // echoed as given on its own line, and exits 1 when none match — both 0 and 1 are
    // non-error.
    let mut args: Vec<&str> = Vec::with_capacity(git_pathspecs.len() + 1);
    args.push("check-ignore");
    args.extend(git_pathspecs.iter().map(String::as_str));
    let output = run_git_allowing_statuses(repo, args, &[0, 1])?;

    let ignored: std::collections::HashSet<&[u8]> = output
        .stdout
        .split(|byte| *byte == b'\n')
        .map(|token| token.strip_suffix(b"\r").unwrap_or(token))
        .filter(|token| !token.is_empty())
        .collect();
    Ok(git_pathspecs
        .iter()
        .map(|path| ignored.contains(path.as_bytes()))
        .collect())
}

/// Convert native path separators to Git's slash-form pathspec spelling.
/// Backslashes remain literal filename characters on Unix.
fn git_pathspec_for_separator(path: &str, separator: char) -> String {
    if separator == '/' {
        path.to_owned()
    } else {
        path.replace(separator, "/")
    }
}

/// Read one Git config value with the fallback semantics writer identity needs:
/// missing keys, empty values, non-zero Git status, and spawn failures all mean
/// "no value" rather than aborting actor resolution.
pub(crate) fn git_config_get(repo: &Path, key: &str) -> Option<String> {
    let (code, stdout) = run_git_status(repo, ["config", "--get", key], &[0, 1]).ok()?;
    if code != 0 {
        return None;
    }

    let value = String::from_utf8_lossy(&stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

/// Read one Git config value using Git's path expansion rules. Missing keys,
/// empty values, non-zero Git status, and spawn failures all mean "no value".
pub(crate) fn git_config_path_get(repo: &Path, key: &str) -> Option<String> {
    let (code, stdout) =
        run_git_status(repo, ["config", "--type=path", "--get", key], &[0, 1]).ok()?;
    if code != 0 {
        return None;
    }

    let value = String::from_utf8_lossy(&stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

pub(crate) fn git_untracked_inventory(repo: &Path) -> Result<Vec<GitInventoryPath>> {
    git_ls_files_inventory(
        repo,
        ["ls-files", "--others", "--exclude-standard", "-z", "--"],
    )
}

pub(crate) fn git_tracked_and_untracked_inventory(repo: &Path) -> Result<Vec<GitInventoryPath>> {
    git_ls_files_inventory(repo, ["ls-files", "-co", "--exclude-standard", "-z", "--"])
}

/// True when `relative_path` is present in the worktree as an **untracked** file
/// (git `--others`, honoring the standard excludes). A tracked path — clean or
/// modified — reports false, as does an absent or git-ignored path. Scoped to the
/// single path via a trailing pathspec, so it never lists the whole worktree.
pub(crate) fn git_path_is_untracked(repo: &Path, relative_path: &str) -> Result<bool> {
    let output = run_git(
        repo,
        [
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            relative_path,
        ],
    )?;
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .any(|field| !field.is_empty()))
}

fn git_ls_files_inventory<const N: usize>(
    repo: &Path,
    args: [&str; N],
) -> Result<Vec<GitInventoryPath>> {
    let output = run_git(repo, args)?;
    Ok(output
        .stdout
        .split(|byte| *byte == b'\0')
        .filter(|field| !field.is_empty())
        .map(GitInventoryPath::new)
        .collect())
}

/// Three-valued reachability: is `ancestor_oid` an ancestor of `descendant_oid`?
/// `merge-base --is-ancestor` reports only via exit code with empty stdout, and a
/// missing/bad object (exit 128) is returned as [`Ancestry::MissingObject`]
/// rather than an error so liveness can keep folding.
pub(crate) fn git_is_ancestor(
    repo: &Path,
    ancestor_oid: &str,
    descendant_oid: &str,
) -> Result<Ancestry> {
    let (code, _) = run_git_status(
        repo,
        ["merge-base", "--is-ancestor", ancestor_oid, descendant_oid],
        &[0, 1, 128],
    )?;
    Ok(match code {
        0 => Ancestry::Ancestor,
        1 => Ancestry::NotAncestor,
        _ => Ancestry::MissingObject,
    })
}

/// The maximal (mutually independent) commits among `oids`: the subset not
/// reachable from any other member, via one `merge-base --independent` call.
/// A chain collapses to its tip; only genuinely incomparable commits survive.
/// Callers pass only OIDs whose objects exist (liveness classifies missing
/// objects first); a bad object errors like any other git failure. Zero or one
/// input echoes back without spawning git.
pub(crate) fn git_independent_commits(repo: &Path, oids: &[String]) -> Result<Vec<String>> {
    if oids.len() <= 1 {
        return Ok(oids.to_vec());
    }
    let mut args = vec!["merge-base".to_owned(), "--independent".to_owned()];
    args.extend(oids.iter().cloned());
    let output = run_git(repo, args)?;
    let text = git_field_string(&output.stdout, "merge-base --independent output")?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

/// The paths `commit_oid` touches relative to its parent(s)
/// (`diff-tree --no-commit-id --name-only -z -r --root -m`). A merge commit
/// lists the union of its per-parent diffs; a root commit lists its full tree;
/// a rename lists both sides (no rename detection). NUL-delimited so exotic
/// path bytes never corrupt the split; a non-UTF-8 path is skipped rather than
/// erroring — the sole consumer is an advisory overlap check.
pub(crate) fn git_commit_changed_paths(repo: &Path, commit_oid: &str) -> Result<Vec<String>> {
    let output = run_git(
        repo,
        [
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-z",
            "-r",
            "--root",
            "-m",
            commit_oid,
        ],
    )?;
    Ok(output
        .stdout
        .split(|byte| *byte == b'\0')
        .filter(|field| !field.is_empty())
        .filter_map(|field| std::str::from_utf8(field).ok())
        .map(str::to_owned)
        .collect())
}

/// Read the non-empty first message line for an explicit, bounded set of commit
/// OIDs through one `cat-file --batch` process. Missing, non-commit, or
/// non-UTF-8 objects are omitted so display callers can use their recorded
/// source fallback without turning an unreadable object into a hard failure.
/// The input set and returned map are ordered for deterministic callers.
pub fn git_commit_subjects(
    repo: &Path,
    commit_oids: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>> {
    if commit_oids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let mut input = commit_oids.iter().cloned().collect::<Vec<_>>().join("\n");
    input.push('\n');
    let output = run_git_with_stdin(repo, ["cat-file", "--batch"], input.as_bytes(), &[0])?;
    parse_commit_subject_batch(commit_oids, &output.stdout)
}

fn parse_commit_subject_batch(
    commit_oids: &BTreeSet<String>,
    output: &[u8],
) -> Result<BTreeMap<String, String>> {
    let mut subjects = BTreeMap::new();
    let mut cursor = 0;

    for requested_oid in commit_oids {
        let header_end = output[cursor..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| cursor + offset)
            .ok_or_else(|| ShoreError::Message("truncated git cat-file batch header".to_owned()))?;
        let header = std::str::from_utf8(&output[cursor..header_end]).map_err(|error| {
            ShoreError::Message(format!("git returned non-utf8 cat-file header: {error}"))
        })?;
        cursor = header_end + 1;

        if header.ends_with(" missing") || header.ends_with(" ambiguous") {
            continue;
        }

        let mut fields = header.rsplitn(3, ' ');
        let size = fields
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| ShoreError::Message(format!("invalid git cat-file header: {header}")))?;
        let object_type = fields
            .next()
            .ok_or_else(|| ShoreError::Message(format!("invalid git cat-file header: {header}")))?;
        if cursor + size > output.len() {
            return Err(ShoreError::Message(
                "truncated git cat-file batch object".to_owned(),
            ));
        }
        let object = &output[cursor..cursor + size];
        cursor += size;
        if output.get(cursor) == Some(&b'\n') {
            cursor += 1;
        }

        if object_type != "commit" {
            continue;
        }
        let Some(message_start) = object
            .windows(2)
            .position(|window| window == b"\n\n")
            .map(|position| position + 2)
        else {
            continue;
        };
        let message = &object[message_start..];
        let first_line_end = message
            .iter()
            .position(|byte| *byte == b'\n')
            .unwrap_or(message.len());
        let Ok(subject) = std::str::from_utf8(&message[..first_line_end]) else {
            continue;
        };
        let subject = subject.trim();
        if !subject.is_empty() {
            subjects.insert(requested_oid.clone(), subject.to_owned());
        }
    }

    Ok(subjects)
}

/// Ref tips matching `patterns` (e.g. `&["refs/heads/*"]`), as `(oid, full ref)`
/// pairs. Empty `patterns` lists every ref.
pub(crate) fn git_for_each_ref(repo: &Path, patterns: &[&str]) -> Result<Vec<RefEntry>> {
    let mut args = vec![
        "for-each-ref".to_owned(),
        "--format=%(objectname) %(refname)".to_owned(),
    ];
    args.extend(patterns.iter().map(|pattern| (*pattern).to_owned()));
    let output = run_git(repo, args)?;
    let text = git_field_string(&output.stdout, "for-each-ref output")?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (oid, name) = line.split_once(' ')?;
            Some(RefEntry {
                name: name.to_owned(),
                oid: oid.to_owned(),
            })
        })
        .collect())
}

/// The raw branch/remote ref state, one `<oid> <refname> <symref-target>` line
/// per ref, for change detection: this is every ref input the commit-graph
/// liveness reads — branch and remote tips (including `origin/HEAD`, whose
/// symref target drives default-branch detection). Returned as git emits it
/// (sorted by refname), so equal ref states always produce equal text.
pub(crate) fn git_ref_state_lines(repo: &Path) -> Result<String> {
    let output = run_git(
        repo,
        [
            "for-each-ref",
            "--format=%(objectname) %(refname) %(symref)",
            "refs/heads/",
            "refs/remotes/",
        ],
    )?;
    git_field_string(&output.stdout, "for-each-ref state output")
}

/// Whether `oid` names an object present in the repository (`cat-file -e`).
pub(crate) fn git_object_exists(repo: &Path, oid: &str) -> Result<bool> {
    let (code, _) = run_git_status(repo, ["cat-file", "-e", oid], &[0, 1])?;
    Ok(code == 0)
}

/// The canonical full ref of HEAD (e.g. `refs/heads/feat/x`), or `None` when HEAD
/// is detached. The full ref — never the short name — is the canonical stored
/// `ref_name` spelling for association identity.
pub(crate) fn git_head_ref(repo: &Path) -> Result<Option<String>> {
    let (code, stdout) = run_git_status(repo, ["symbolic-ref", "--quiet", "HEAD"], &[0, 1])?;
    if code != 0 {
        return Ok(None);
    }
    let trimmed = trim_git_stdout(&stdout);
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(git_field_string(trimmed, "HEAD symbolic ref")?))
}

pub fn git_head_oid(repo: &Path) -> Result<String> {
    let output = run_git(repo, ["rev-parse", "HEAD"])?;
    git_stdout_string(repo, &output.stdout, "HEAD oid")
}

/// The repository's integration/default branch as a full ref, best-effort: the
/// target of `refs/remotes/origin/HEAD` when the remote publishes one, else a
/// local `refs/heads/main` or `refs/heads/master` when present, else `None`.
///
/// Never fabricates a branch — a repository with none of these simply has no
/// detectable default, and callers fall back to their own ordering. Name-agnostic
/// by construction: `origin/HEAD` names whatever the remote's default is, and the
/// local fallback tries the two conventional names in order (`main` before
/// `master`) so a repo carrying both prefers `main`.
pub(crate) fn git_default_branch_ref(repo: &Path) -> Result<Option<String>> {
    let (code, stdout) = run_git_status(
        repo,
        ["symbolic-ref", "refs/remotes/origin/HEAD"],
        &[0, 1, 128],
    )?;
    if code == 0 {
        let trimmed = trim_git_stdout(&stdout);
        if !trimmed.is_empty() {
            let target = git_field_string(trimmed, "origin/HEAD target")?;
            // Only accept origin/HEAD when its target still resolves to a commit; a
            // dangling symbolic ref (the remote-tracking branch was pruned) would
            // otherwise shadow a valid local default and, downstream, make a narrow
            // integration ref unresolvable and suppress the whole liveness block.
            if git_ref_resolves_to_commit(repo, &target)? {
                return Ok(Some(target));
            }
        }
    }

    for candidate in ["refs/heads/main", "refs/heads/master"] {
        if git_ref_resolves_to_commit(repo, candidate)? {
            return Ok(Some(candidate.to_owned()));
        }
    }

    Ok(None)
}

/// Whether `reference` resolves to a commit object (`rev-parse --verify --quiet
/// <reference>^{commit}`), the two-valued check the default-branch resolution
/// needs: exit 0 resolves, exit 1 does not (missing ref, or a non-commit).
fn git_ref_resolves_to_commit(repo: &Path, reference: &str) -> Result<bool> {
    let (code, _) = run_git_status(
        repo,
        [
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{reference}^{{commit}}"),
        ],
        &[0, 1],
    )?;
    Ok(code == 0)
}

pub(crate) fn git_head_commit_oid_optional(repo: &Path) -> Result<Option<String>> {
    let (code, stdout) = run_git_status(
        repo,
        ["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
        &[0, 1],
    )?;
    if code == 0 {
        git_stdout_string(repo, &stdout, "HEAD oid").map(Some)
    } else {
        Ok(None)
    }
}

#[cfg(test)]
pub fn git_head_tree_oid(repo: &Path) -> Result<String> {
    let output = run_git(repo, ["rev-parse", "HEAD^{tree}"])?;
    git_stdout_string(repo, &output.stdout, "HEAD tree oid")
}

/// Resolve `rev` to a full commit OID, peeling annotated tags.
///
/// Rejects revs that do not exist or do not peel to a commit (blobs, trees)
/// with an error that names the rev, so CLI flags can surface it verbatim.
/// Resolution runs in the workflow (not the CLI) so library callers get the
/// same honest errors. `--end-of-options` keeps a rev that looks like a flag
/// (user input) from being parsed as an option.
pub(crate) fn git_rev_parse_commit_oid(repo: &Path, rev: &str) -> Result<String> {
    git_rev_parse_peeled(repo, rev, "commit", "commit oid")
}

/// Resolve a commit OID to its tree OID. Callers pass an already-resolved
/// commit OID (from [`git_rev_parse_commit_oid`]), never a raw user rev.
pub(crate) fn git_commit_tree_oid(repo: &Path, commit_oid: &str) -> Result<String> {
    git_rev_parse_peeled(repo, commit_oid, "tree", "commit tree oid")
}

/// Compute the empty tree OID using the repository's configured object format.
/// This deliberately asks Git instead of embedding the SHA-1 empty-tree
/// constant, so SHA-256 repositories use their own empty-tree identity.
pub(crate) fn git_empty_tree_oid(repo: &Path) -> Result<String> {
    let output = run_git_with_stdin(repo, ["hash-object", "-t", "tree", "--stdin"], b"", &[0])?;
    git_stdout_string(repo, &output.stdout, "empty tree oid")
}

pub(crate) fn git_write_index_tree_oid(repo: &Path) -> Result<String> {
    let output = run_git(repo, ["write-tree"]).map_err(|error| match error {
        ShoreError::GitCommand { stderr, .. } => ShoreError::Message(format!(
            "cannot capture the index as a tree; resolve unmerged paths first: {}",
            stderr.trim()
        )),
        other => other,
    })?;
    git_stdout_string(repo, &output.stdout, "index tree oid")
}

/// List the full commit OIDs reachable in a `<a>..<b>` revision range via
/// `git rev-list`.
///
/// Returns the commits the range selects, in `rev-list` order (newest first); an
/// empty range yields an empty vec, not an error. The argument must denote a
/// range (contain `..`): a bare rev like `HEAD` would make `git rev-list` list
/// the whole reachable history, far broader than the `<a>..<b>` contract, so it
/// is refused. `--end-of-options` keeps a range expression that looks like a flag
/// (user input) from being parsed as an option. An unresolvable range surfaces an
/// honest, range-naming error so a CLI flag can echo it verbatim.
/// Every commit reachable from any of `tips` — the tips themselves plus all their
/// ancestors — as a set of full OIDs, in a single `git rev-list` invocation. An
/// empty `tips` yields an empty set without spawning git.
///
/// This is the batched reachability the liveness fold uses instead of one
/// ancestry probe per (commit, tip) pair: one `rev-list` answers "is this commit
/// reachable from the live tips?" for an entire revision list by in-memory set
/// membership, turning an O(revisions × tips) spawn count into O(1).
pub(crate) fn git_rev_list_reachable(repo: &Path, tips: &[String]) -> Result<HashSet<String>> {
    if tips.is_empty() {
        return Ok(HashSet::new());
    }
    let mut args = vec!["rev-list".to_owned(), "--end-of-options".to_owned()];
    args.extend(tips.iter().cloned());
    let output = run_git(repo, args)?;
    let listing = git_field_string(trim_git_stdout(&output.stdout), "rev-list output")?;
    Ok(listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

pub(crate) fn git_rev_list_range(repo: &Path, range: &str) -> Result<Vec<String>> {
    if !range.contains("..") {
        return Err(ShoreError::Message(format!(
            "'{range}' is not a commit range; expected the form '<a>..<b>'"
        )));
    }
    let output = run_git(repo, ["rev-list", "--end-of-options", range]).map_err(|_| {
        ShoreError::Message(format!(
            "cannot resolve commit range '{range}' in this repository"
        ))
    })?;
    let listing = git_field_string(trim_git_stdout(&output.stdout), "rev-list output")?;
    Ok(listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Resolve `rev` peeled to `peel` (e.g. `commit`, `tree`) via
/// `git rev-parse --verify --end-of-options <rev>^{<peel>}`.
///
/// Substitutes an honest, rev-naming error for git's noisy stderr on failure:
/// one message covers both unknown and non-`peel` objects ("cannot resolve
/// '<rev>' to a <peel>").
fn git_rev_parse_peeled(repo: &Path, rev: &str, peel: &str, description: &str) -> Result<String> {
    let output = run_git(
        repo,
        [
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{rev}^{{{peel}}}"),
        ],
    )
    .map_err(|_| {
        ShoreError::Message(format!(
            "cannot resolve '{rev}' to a {peel} in this repository"
        ))
    })?;
    git_stdout_string(repo, &output.stdout, description)
}

pub(crate) fn git_worktree_list(repo: &Path) -> Result<Vec<GitWorktree>> {
    let output = run_git(repo, ["worktree", "list", "--porcelain", "-z"])?;
    parse_git_worktree_list_z(&output.stdout)
}

fn parse_git_worktree_list_z(output: &[u8]) -> Result<Vec<GitWorktree>> {
    let mut worktrees = Vec::new();
    let mut current = None;

    for field in output.split(|byte| *byte == b'\0') {
        if field.is_empty() {
            if let Some(worktree) = current.take() {
                worktrees.push(worktree);
            }
            continue;
        }

        if let Some(path) = field.strip_prefix(b"worktree ") {
            if let Some(worktree) = current.replace(GitWorktree {
                path: git_path_from_bytes(path)?,
                head: None,
                branch: None,
                detached: false,
                bare: false,
            }) {
                worktrees.push(worktree);
            }
            continue;
        }

        let Some(worktree) = current.as_mut() else {
            return Err(ShoreError::Message(
                "git worktree list returned field before worktree path".to_owned(),
            ));
        };

        if let Some(head) = field.strip_prefix(b"HEAD ") {
            worktree.head = Some(git_field_string(head, "worktree HEAD")?);
        } else if let Some(branch) = field.strip_prefix(b"branch ") {
            worktree.branch = Some(git_field_string(branch, "worktree branch")?);
        } else if field == b"detached" {
            worktree.detached = true;
        } else if field == b"bare" {
            worktree.bare = true;
        }
    }

    if let Some(worktree) = current {
        worktrees.push(worktree);
    }

    Ok(worktrees)
}

pub(crate) fn run_git_allowing_statuses<I, S>(
    cwd: &Path,
    args: I,
    allowed_statuses: &[i32],
) -> Result<GitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let (_, stdout) = run_git_status(cwd, args, allowed_statuses)?;
    Ok(GitOutput { stdout })
}

/// Runs git and surfaces both the exit code and stdout, erroring only when the
/// code is outside `allowed_statuses`. Unlike [`run_git_allowing_statuses`],
/// this keeps the exit code, which is the only signal some plumbing commands
/// emit (`merge-base --is-ancestor`, `cat-file -e`, `symbolic-ref --quiet`).
pub(crate) fn run_git_status<I, S>(
    cwd: &Path,
    args: I,
    allowed_statuses: &[i32],
) -> Result<(i32, Vec<u8>)>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();
    let output = Command::new("git")
        .args(&args)
        .current_dir(cwd)
        .output()
        .map_err(|error| ShoreError::Message(format!("run git {:?}: {error}", args)))?;

    let status_code = output.status.code();
    if !status_code.is_some_and(|code| allowed_statuses.contains(&code)) {
        return Err(ShoreError::GitCommand {
            command: format!("{args:?}"),
            status: output.status.to_string(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    Ok((
        status_code.expect("an allowed status implies a concrete exit code"),
        output.stdout,
    ))
}

fn run_git_with_stdin<I, S>(
    cwd: &Path,
    args: I,
    stdin: &[u8],
    allowed_statuses: &[i32],
) -> Result<GitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();
    let mut child = Command::new("git")
        .args(&args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| ShoreError::Message(format!("run git {:?}: {error}", args)))?;

    child
        .stdin
        .as_mut()
        .expect("git stdin is piped")
        .write_all(stdin)
        .map_err(|error| ShoreError::Message(format!("write git {:?} stdin: {error}", args)))?;

    let output = child
        .wait_with_output()
        .map_err(|error| ShoreError::Message(format!("wait for git {:?}: {error}", args)))?;
    let status_code = output.status.code();
    if !status_code.is_some_and(|code| allowed_statuses.contains(&code)) {
        return Err(ShoreError::GitCommand {
            command: format!("{args:?}"),
            status: output.status.to_string(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    Ok(GitOutput {
        stdout: output.stdout,
    })
}

fn git_stdout_path(repo: &Path, stdout: &[u8], description: &str) -> Result<PathBuf> {
    let trimmed = trim_git_stdout(stdout);
    if trimmed.is_empty() {
        return Err(ShoreError::Message(format!(
            "git rev-parse returned empty {description} for {}",
            repo.display()
        )));
    }

    git_path_from_bytes(trimmed)
}

fn git_stdout_string(repo: &Path, stdout: &[u8], description: &str) -> Result<String> {
    let trimmed = trim_git_stdout(stdout);
    if trimmed.is_empty() {
        return Err(ShoreError::Message(format!(
            "git rev-parse returned empty {description} for {}",
            repo.display()
        )));
    }

    git_field_string(trimmed, description)
}

fn trim_git_stdout(stdout: &[u8]) -> &[u8] {
    let mut end = stdout.len();
    while end > 0 && matches!(stdout[end - 1], b'\r' | b'\n') {
        end -= 1;
    }

    &stdout[..end]
}

fn git_field_string(bytes: &[u8], description: &str) -> Result<String> {
    String::from_utf8(bytes.to_vec()).map_err(|error| {
        ShoreError::Message(format!("git returned non-utf8 {description}: {error}"))
    })
}

#[cfg(unix)]
fn git_path_from_bytes(bytes: &[u8]) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    Ok(std::ffi::OsString::from_vec(bytes.to_vec()).into())
}

#[cfg(not(unix))]
fn git_path_from_bytes(bytes: &[u8]) -> Result<PathBuf> {
    let path = String::from_utf8(bytes.to_vec()).map_err(|error| {
        ShoreError::Message(format!("git returned non-utf8 path bytes: {error}"))
    })?;
    Ok(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::io::Write;
    use std::process::Stdio;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn git_common_dir_is_shared_across_worktrees() {
        let fixture = LinkedWorktreeFixture::new();

        let main_common_dir = git_common_dir(fixture.main.path()).unwrap();
        let linked_common_dir = git_common_dir(&fixture.linked_path).unwrap();
        assert_eq!(
            canonicalize(&main_common_dir),
            canonicalize(&linked_common_dir)
        );

        let worktrees = git_worktree_list(fixture.main.path()).unwrap();
        let worktree_paths = worktrees
            .iter()
            .map(|worktree| canonicalize(&worktree.path))
            .collect::<Vec<_>>();
        assert!(worktree_paths.contains(&canonicalize(fixture.main.path())));
        assert!(worktree_paths.contains(&canonicalize(&fixture.linked_path)));
    }

    #[test]
    fn rev_parse_commit_oid_resolves_branches_relative_revs_and_annotated_tags() {
        let repo = TwoCommitRepo::new();

        let first_via_helper = git_rev_parse_commit_oid(repo.path(), "HEAD~1").unwrap();
        let first_expected = rev_parse(repo.path(), "HEAD~1");
        assert_eq!(first_via_helper, first_expected);

        let first_via_tag = git_rev_parse_commit_oid(repo.path(), "v1").unwrap();
        assert_eq!(
            first_via_tag, first_expected,
            "annotated tag must peel to its commit"
        );

        // Full-width oid (not abbreviated); width depends on object format.
        assert_eq!(first_via_helper, rev_parse(repo.path(), "HEAD~1"));
        assert!(!first_via_helper.is_empty());
    }

    #[test]
    fn rev_parse_commit_oid_rejects_unknown_rev_with_honest_error() {
        let repo = TwoCommitRepo::new();

        let error = git_rev_parse_commit_oid(repo.path(), "no-such-rev").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("no-such-rev"), "message: {message}");
        assert!(message.contains("commit"), "message: {message}");
    }

    #[test]
    fn rev_parse_commit_oid_rejects_non_commit_object() {
        let repo = TwoCommitRepo::new();

        let error = git_rev_parse_commit_oid(repo.path(), "HEAD:file.txt").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("HEAD:file.txt"), "message: {message}");
    }

    #[test]
    fn commit_tree_oid_resolves_tree_for_commit() {
        let repo = TwoCommitRepo::new();
        let head_oid = git_head_oid(repo.path()).unwrap();

        let tree_via_commit = git_commit_tree_oid(repo.path(), &head_oid).unwrap();
        let tree_via_head = git_head_tree_oid(repo.path()).unwrap();

        assert_eq!(tree_via_commit, tree_via_head);
        assert_ne!(tree_via_commit, head_oid);
    }

    #[test]
    fn commit_subjects_batch_is_deterministic_and_omits_unreadable_oids() {
        let repo = TwoCommitRepo::new();
        let first = rev_parse(repo.path(), "HEAD~1");
        let second = rev_parse(repo.path(), "HEAD");
        let missing = "0".repeat(second.len());
        let requested = BTreeSet::from([second.clone(), missing.clone(), first.clone()]);

        let subjects = git_commit_subjects(repo.path(), &requested).unwrap();

        assert_eq!(
            subjects,
            BTreeMap::from([(first, "first".to_owned()), (second, "second".to_owned())])
        );
        assert!(
            !git_commit_subjects(repo.path(), &BTreeSet::new())
                .unwrap()
                .contains_key(&missing)
        );
    }

    #[test]
    fn empty_tree_oid_matches_git_stdin_hash_object() {
        let repo = TwoCommitRepo::new();
        let oid = git_empty_tree_oid(repo.path()).unwrap();
        let expected = git_hash_object_tree_from_stdin(repo.path(), b"").unwrap();

        assert_eq!(oid, expected);
        assert!(git_rev_parse_peeled(repo.path(), &oid, "tree", "tree oid").is_ok());
    }

    #[test]
    fn empty_tree_oid_uses_repository_hash_algorithm_when_sha256_is_supported() {
        let Some(repo) = maybe_sha256_repo() else {
            return;
        };

        let oid = git_empty_tree_oid(repo.path()).unwrap();
        let expected = git_hash_object_tree_from_stdin(repo.path(), b"").unwrap();

        assert_eq!(oid, expected);
        assert_ne!(oid, "4b825dc642cb6eb9a060e54bf8d69288fbee4904");
        assert_eq!(oid.len(), 64);
    }

    fn rev_parse(repo: &Path, rev: &str) -> String {
        let output = run_git(repo, ["rev-parse", rev]).unwrap();
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn git_hash_object_tree_from_stdin(repo: &Path, input: &[u8]) -> Result<String> {
        let mut child = Command::new("git")
            .args(["hash-object", "-t", "tree", "--stdin"])
            .current_dir(repo)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| ShoreError::Message(format!("run git hash-object: {error}")))?;
        child
            .stdin
            .as_mut()
            .expect("hash-object stdin is piped")
            .write_all(input)
            .map_err(|error| {
                ShoreError::Message(format!("write git hash-object stdin: {error}"))
            })?;
        let output = child
            .wait_with_output()
            .map_err(|error| ShoreError::Message(format!("wait for git hash-object: {error}")))?;
        if !output.status.success() {
            return Err(ShoreError::Message(format!(
                "git hash-object -t tree --stdin failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn maybe_sha256_repo() -> Option<TempDir> {
        let repo = TempDir::new().expect("create sha256 test repository directory");
        let output = Command::new("git")
            .args(["init", "--object-format=sha256"])
            .current_dir(repo.path())
            .output()
            .expect("run git init --object-format=sha256");
        output.status.success().then_some(repo)
    }

    #[test]
    fn rev_list_range_lists_commits_in_the_range() {
        let repo = TwoCommitRepo::new();
        let head = rev_parse(repo.path(), "HEAD");
        let base = rev_parse(repo.path(), "HEAD~1");

        // `base..HEAD` excludes base, includes HEAD.
        let range = git_rev_list_range(repo.path(), &format!("{base}..{head}")).unwrap();
        assert_eq!(range, vec![head.clone()]);

        // An empty range (nothing reachable from base that is not reachable from
        // HEAD's first parent) yields an empty list, not an error.
        let empty = git_rev_list_range(repo.path(), &format!("{head}..{base}")).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn rev_list_range_rejects_an_unresolvable_range_with_honest_error() {
        let repo = TwoCommitRepo::new();

        let error = git_rev_list_range(repo.path(), "no-such-rev..HEAD").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("no-such-rev..HEAD"), "message: {message}");
    }

    #[test]
    fn rev_list_range_rejects_a_bare_rev_that_is_not_a_range() {
        let repo = TwoCommitRepo::new();

        // A bare rev like `HEAD` is not a range: `git rev-list HEAD` would list the
        // whole reachable history, far broader than the `<a>..<b>` contract.
        let error = git_rev_list_range(repo.path(), "HEAD").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("HEAD"), "message: {message}");
        assert!(
            message.contains(".."),
            "message names the expected range form: {message}"
        );
    }

    #[test]
    fn is_ancestor_is_three_valued() {
        let repo = TwoCommitRepo::new();
        let base = rev_parse(repo.path(), "HEAD~1");
        let tip = rev_parse(repo.path(), "HEAD");

        assert_eq!(
            git_is_ancestor(repo.path(), &base, &tip).unwrap(),
            Ancestry::Ancestor
        );
        assert_eq!(
            git_is_ancestor(repo.path(), &tip, &base).unwrap(),
            Ancestry::NotAncestor
        );
        let absent = "0".repeat(tip.len());
        assert_eq!(
            git_is_ancestor(repo.path(), &absent, &tip).unwrap(),
            Ancestry::MissingObject
        );
    }

    #[test]
    fn for_each_ref_lists_tips_including_nested_branches() {
        let repo = TwoCommitRepo::new();
        git(repo.path(), ["branch", "feat/x"]);

        // The `refs/heads/` prefix matches nested branch names; `refs/heads/*`
        // would not, because for-each-ref globs with WM_PATHNAME so `*` stops at
        // a slash.
        let entries = git_for_each_ref(repo.path(), &["refs/heads/"]).unwrap();
        let tip = rev_parse(repo.path(), "HEAD");

        assert!(
            entries
                .iter()
                .any(|entry| entry.name == "refs/heads/feat/x"),
            "for-each-ref must list the nested branch: {entries:?}"
        );
        assert!(entries.iter().any(|entry| entry.oid == tip));
    }

    #[test]
    fn object_exists_and_head_ref() {
        let repo = TwoCommitRepo::new();
        let head_oid = rev_parse(repo.path(), "HEAD");

        assert!(git_object_exists(repo.path(), &head_oid).unwrap());
        assert!(!git_object_exists(repo.path(), &"0".repeat(head_oid.len())).unwrap());

        let head_ref = git_head_ref(repo.path()).unwrap();
        assert!(
            head_ref
                .as_deref()
                .is_some_and(|name| name.starts_with("refs/heads/")),
            "attached HEAD must resolve to a full ref, got {head_ref:?}"
        );

        git(repo.path(), ["checkout", "--detach"]);
        assert_eq!(git_head_ref(repo.path()).unwrap(), None);
    }

    /// Default-branch detection is name-agnostic: a non-main local default
    /// (`master`) is detected, `main` wins the local fallback order when both
    /// exist, and a published `origin/HEAD` takes precedence over any local
    /// fallback. CI runners default to `master`, so the branch names are forced
    /// explicitly rather than left to `init.defaultBranch`.
    #[test]
    fn default_branch_ref_prefers_origin_head_then_local_main_then_master() {
        let repo = TempDir::new().expect("create temp repository directory");
        git(repo.path(), ["init"]);
        git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/master"]);
        git(repo.path(), ["config", "user.name", "Shore Tests"]);
        git(
            repo.path(),
            ["config", "user.email", "shore-tests@example.com"],
        );
        git(repo.path(), ["config", "commit.gpgsign", "false"]);
        fs::write(repo.path().join("file.txt"), "one\n").unwrap();
        git(repo.path(), ["add", "--all"]);
        git(repo.path(), ["commit", "-m", "first"]);

        assert_eq!(
            git_default_branch_ref(repo.path()).unwrap().as_deref(),
            Some("refs/heads/master"),
            "a repo whose only conventional default is master detects master"
        );

        // `main` alongside `master`: main wins the local fallback order.
        git(repo.path(), ["branch", "main"]);
        assert_eq!(
            git_default_branch_ref(repo.path()).unwrap().as_deref(),
            Some("refs/heads/main"),
            "main is preferred over master when both exist"
        );

        // A published `origin/HEAD` whose target resolves takes precedence over the
        // local fallback and names whatever the remote's default is.
        git(
            repo.path(),
            ["update-ref", "refs/remotes/origin/trunk", "refs/heads/main"],
        );
        git(
            repo.path(),
            [
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/trunk",
            ],
        );
        assert_eq!(
            git_default_branch_ref(repo.path()).unwrap().as_deref(),
            Some("refs/remotes/origin/trunk"),
            "a resolvable origin/HEAD wins over the local fallback"
        );
    }

    /// A dangling `origin/HEAD` (a symbolic ref whose target does not resolve to a
    /// commit) must not be returned: detection falls through to a valid local
    /// `main`/`master`, so a pruned remote default does not suppress liveness
    /// downstream.
    #[test]
    fn default_branch_ref_skips_a_dangling_origin_head() {
        let repo = TempDir::new().expect("create temp repository directory");
        git(repo.path(), ["init"]);
        git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
        git(repo.path(), ["config", "user.name", "Shore Tests"]);
        git(
            repo.path(),
            ["config", "user.email", "shore-tests@example.com"],
        );
        git(repo.path(), ["config", "commit.gpgsign", "false"]);
        fs::write(repo.path().join("file.txt"), "one\n").unwrap();
        git(repo.path(), ["add", "--all"]);
        git(repo.path(), ["commit", "-m", "first"]);

        // origin/HEAD points at a remote-tracking ref that does not exist.
        git(
            repo.path(),
            [
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/missing",
            ],
        );

        assert_eq!(
            git_default_branch_ref(repo.path()).unwrap().as_deref(),
            Some("refs/heads/main"),
            "a dangling origin/HEAD falls through to the valid local main"
        );
    }

    /// No conventional default and no origin: `None`, so the caller falls back to
    /// its own ordering rather than a fabricated branch.
    #[test]
    fn default_branch_ref_is_none_without_a_conventional_default() {
        let repo = TempDir::new().expect("create temp repository directory");
        git(repo.path(), ["init"]);
        git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/trunk"]);
        git(repo.path(), ["config", "user.name", "Shore Tests"]);
        git(
            repo.path(),
            ["config", "user.email", "shore-tests@example.com"],
        );
        git(repo.path(), ["config", "commit.gpgsign", "false"]);
        fs::write(repo.path().join("file.txt"), "one\n").unwrap();
        git(repo.path(), ["add", "--all"]);
        git(repo.path(), ["commit", "-m", "first"]);

        assert_eq!(git_default_branch_ref(repo.path()).unwrap(), None);
    }

    #[test]
    fn git_paths_are_ignored_reports_each_path_in_input_order() {
        let repo = TwoCommitRepo::new();
        // Write a repo-local exclude so exactly one of the probed paths is ignored.
        let exclude = repo.path().join(".git/info/exclude");
        fs::create_dir_all(exclude.parent().unwrap()).unwrap();
        fs::write(&exclude, ".pointbreak/data/\n").unwrap();

        let verdicts = git_paths_are_ignored(
            repo.path(),
            &[
                ".pointbreak/data/state.json", // ignored (matches `.pointbreak/data/`)
                ".pointbreak/delegates.local.json", // not ignored
            ],
        )
        .unwrap();

        assert_eq!(verdicts, vec![true, false]);
    }

    #[test]
    fn git_pathspecs_use_forward_slashes_for_windows_git() {
        assert_eq!(
            git_pathspec_for_separator(r".pointbreak\data\state.json", '\\'),
            ".pointbreak/data/state.json"
        );
        assert_eq!(
            git_pathspec_for_separator(r"literal\backslash", '/'),
            r"literal\backslash"
        );
    }

    #[test]
    fn git_config_get_returns_values_needed_for_writer_fallback() {
        let repo = TempDir::new().expect("create temp git repository directory");
        git(repo.path(), ["init"]);

        git(repo.path(), ["config", "user.email", ""]);
        assert_eq!(git_config_get(repo.path(), "user.email"), None);

        git(
            repo.path(),
            ["config", "user.email", "reviewer@example.com"],
        );
        assert_eq!(
            git_config_get(repo.path(), "user.email"),
            Some("reviewer@example.com".to_owned())
        );

        git(repo.path(), ["config", "user.name", ""]);
        assert_eq!(git_config_get(repo.path(), "user.name"), None);
    }

    #[test]
    fn untracked_inventory_lists_unignored_untracked_paths_in_git_order() {
        let repo = TwoCommitRepo::new();
        fs::create_dir_all(repo.path().join("notes")).unwrap();
        fs::write(repo.path().join("b.txt"), "b\n").unwrap();
        fs::write(repo.path().join("notes/a.txt"), "a\n").unwrap();
        fs::write(repo.path().join("ignored.log"), "ignored\n").unwrap();
        fs::write(repo.path().join(".git/info/exclude"), "ignored.log\n").unwrap();

        let paths = inventory_path_strings(git_untracked_inventory(repo.path()).unwrap());

        assert_eq!(paths, vec!["b.txt", "notes/a.txt"]);
    }

    fn inventory_path_strings(paths: Vec<GitInventoryPath>) -> Vec<String> {
        paths
            .into_iter()
            .map(|path| path.into_utf8_string("test inventory path").unwrap())
            .collect()
    }

    #[test]
    fn git_path_is_untracked_distinguishes_untracked_tracked_and_absent() {
        let repo = TwoCommitRepo::new();

        // Absent path → false.
        assert!(!git_path_is_untracked(repo.path(), "nope.txt").unwrap());

        // Tracked, clean → false.
        assert!(!git_path_is_untracked(repo.path(), "file.txt").unwrap());

        // Tracked, modified in the worktree → still tracked, so false.
        fs::write(repo.path().join("file.txt"), "three\n").unwrap();
        assert!(!git_path_is_untracked(repo.path(), "file.txt").unwrap());

        // Untracked, present → true.
        fs::write(repo.path().join("new.txt"), "x\n").unwrap();
        assert!(git_path_is_untracked(repo.path(), "new.txt").unwrap());

        // Untracked but git-ignored → excluded-standard, so false.
        fs::write(repo.path().join(".git/info/exclude"), "ignored.txt\n").unwrap();
        fs::write(repo.path().join("ignored.txt"), "y\n").unwrap();
        assert!(!git_path_is_untracked(repo.path(), "ignored.txt").unwrap());
    }

    struct TwoCommitRepo {
        root: TempDir,
    }

    impl TwoCommitRepo {
        fn new() -> Self {
            let root = TempDir::new().expect("create temp git repository directory");
            let repo = Self { root };

            git(repo.path(), ["init"]);
            git(repo.path(), ["config", "user.name", "Shore Tests"]);
            git(
                repo.path(),
                ["config", "user.email", "shore-tests@example.com"],
            );
            git(repo.path(), ["config", "commit.gpgsign", "false"]);

            fs::write(repo.path().join("file.txt"), "one\n").expect("write first file");
            git(repo.path(), ["add", "--all"]);
            git(repo.path(), ["commit", "-m", "first"]);
            git(repo.path(), ["tag", "-a", "v1", "-m", "v1", "HEAD"]);

            fs::write(repo.path().join("file.txt"), "two\n").expect("write second file");
            git(repo.path(), ["add", "--all"]);
            git(repo.path(), ["commit", "-m", "second"]);

            repo
        }

        fn path(&self) -> &Path {
            self.root.path()
        }
    }

    #[cfg(unix)]
    #[test]
    fn worktree_list_parser_preserves_non_utf8_paths() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let raw_path = b"/tmp/shoreline-\xff-worktree";
        let output = [
            b"worktree ".as_slice(),
            raw_path.as_slice(),
            b"\0HEAD 0123456789012345678901234567890123456789\0branch refs/heads/main\0\0",
        ]
        .concat();

        let worktrees = parse_git_worktree_list_z(&output).unwrap();

        assert_eq!(worktrees.len(), 1);
        assert_eq!(
            worktrees[0].path.as_os_str().as_bytes(),
            OsString::from_vec(raw_path.to_vec()).as_os_str().as_bytes()
        );
    }

    struct LinkedWorktreeFixture {
        main: TempDir,
        _linked_parent: TempDir,
        linked_path: PathBuf,
    }

    impl LinkedWorktreeFixture {
        fn new() -> Self {
            let main = TempDir::new().expect("create main repository directory");
            git(main.path(), ["init"]);
            git(main.path(), ["config", "user.name", "Shore Tests"]);
            git(
                main.path(),
                ["config", "user.email", "shore-tests@example.com"],
            );
            git(main.path(), ["config", "commit.gpgsign", "false"]);
            fs::write(main.path().join("README.md"), "base\n").expect("write base file");
            git(main.path(), ["add", "--all"]);
            git(main.path(), ["commit", "-m", "base"]);

            let linked_parent = TempDir::new().expect("create linked worktree parent");
            let linked_path = linked_parent.path().join("linked");
            git_os(
                main.path(),
                [
                    OsString::from("worktree"),
                    OsString::from("add"),
                    OsString::from("-b"),
                    OsString::from("linked"),
                    linked_path.as_os_str().to_owned(),
                ],
            );

            Self {
                main,
                _linked_parent: linked_parent,
                linked_path,
            }
        }
    }

    fn repo_fact_is_cached(repo: &Path, fact: RepoFact) -> bool {
        repo_fact_cache()
            .lock()
            .expect("repo fact cache mutex is not poisoned")
            .contains_key(&repo_fact_key(repo, fact))
    }

    #[test]
    fn invariant_repo_facts_are_resolved_once_and_memoized() {
        let repo = TwoCommitRepo::new();

        // A freshly created repository (unique temp dir) starts cold for every
        // fact, so the first lookup is a genuine miss that spawns Git.
        for fact in [
            RepoFact::WorktreeRoot,
            RepoFact::CommonDir,
            RepoFact::InfoExcludePath,
        ] {
            assert!(
                !repo_fact_is_cached(repo.path(), fact),
                "{fact:?} must be cold before the first lookup"
            );
        }

        let root_first = git_worktree_root(repo.path()).unwrap();
        let common_first = git_common_dir(repo.path()).unwrap();
        let exclude_first = git_info_exclude_path(repo.path()).unwrap();

        // After one lookup each fact is memoized, so subsequent calls are served
        // from the cache rather than spawning Git again.
        for fact in [
            RepoFact::WorktreeRoot,
            RepoFact::CommonDir,
            RepoFact::InfoExcludePath,
        ] {
            assert!(
                repo_fact_is_cached(repo.path(), fact),
                "{fact:?} must be memoized after the first lookup"
            );
        }

        // The memoized value matches the freshly resolved one — caching changes
        // cost, never the answer.
        assert_eq!(git_worktree_root(repo.path()).unwrap(), root_first);
        assert_eq!(git_common_dir(repo.path()).unwrap(), common_first);
        assert_eq!(git_info_exclude_path(repo.path()).unwrap(), exclude_first);
    }

    fn git<I, S>(cwd: &Path, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        run_git(cwd, args).unwrap();
    }

    fn git_os<I>(cwd: &Path, args: I)
    where
        I: IntoIterator<Item = OsString>,
    {
        run_git(cwd, args).unwrap();
    }

    fn canonicalize(path: &Path) -> PathBuf {
        path.canonicalize()
            .unwrap_or_else(|error| panic!("canonicalize {}: {error}", path.display()))
    }
}
