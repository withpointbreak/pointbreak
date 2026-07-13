//! The repo/store identity the inspector chrome renders (issue #391).
//!
//! A deliberately lightweight, path-private sibling of [`store_status`]: it reports
//! *which* repository and store the inspector is serving, without the full
//! store-inventory and worktree-sensitivity scans `store_status` runs. Identity is a
//! chrome cue, so it stays cheap.
//!
//! Path-privacy is the load-bearing constraint (issue #391): no field ever carries an
//! absolute or repo-relative filesystem path. Path **basenames** (final components),
//! the opaque family slug, and one-way store/context path hashes cross the boundary —
//! the same label convention the inspector's revision endpoints already follow, plus
//! opaque equality keys that reveal no path (`src/cli/inspect/api.rs`).
//!
//! [`store_status`]: super::store_status::store_status

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{Result, ShoreError};
use crate::git::{git_common_dir, git_worktree_root};
use crate::session::store::resolution::resolve_store;

/// Floor label when no basename can be derived (unusual git layouts). Mirrors the
/// `WORKING_TREE_FLOOR` idea in `src/cli/inspect/api.rs`; kept as a lib-local copy
/// because that constant is private to the binary crate.
const REPOSITORY_FLOOR: &str = "repository";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreIdentityOptions {
    repo: PathBuf,
}

impl StoreIdentityOptions {
    pub fn new(repo: impl AsRef<Path>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
        }
    }
}

/// The path-private repo/store identity document the inspector renders.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreIdentity {
    /// Opaque identity of the resolved store directory. Equality is the contract;
    /// callers must not parse the digest.
    pub store_identity: String,
    /// Opaque identity of the current Git worktree root. Distinguishes contexts
    /// that share one store without exposing either path.
    pub context_identity: String,
    /// Stable repository label: the main-worktree-root basename (path-free).
    pub repository: String,
    /// The current worktree-root basename; present ONLY when it differs from
    /// `repository` (a linked worktree — the common-dir store serves several).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// Which store tier is being read.
    pub placement: StorePlacement,
    /// The repository family; present ONLY under the user-level (family) tier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<StoreFamily>,
}

/// The resolved store placement tier.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorePlacement {
    /// Domain-named tier tag: `"clone"` | `"family"` | `"ephemeral"`.
    pub tier: &'static str,
    /// Human label: `"clone store"` | `"family store"` | `"ephemeral store"`.
    pub label: &'static str,
}

/// The repository family a user-level store serves.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreFamily {
    /// The family slug (`repository_family_ref`) — a non-identity placement label.
    pub id: String,
}

/// Derive the [`StoreIdentity`] for `repo`. Reuses the same store resolver every read
/// surface uses, plus git basenames; performs no store-inventory or sensitivity scan.
pub fn store_identity(options: StoreIdentityOptions) -> Result<StoreIdentity> {
    let worktree_root = git_worktree_root(&options.repo)?;
    let resolution = resolve_store(&options.repo)?;
    let store_identity = opaque_path_identity("store", resolution.store_dir())?;
    let context_identity = opaque_path_identity("context", &worktree_root)?;
    let view = resolution.command_view();
    let placement = placement_for(view.mode);
    let family = view.repository_family_ref.map(|id| StoreFamily { id });

    // `repository` is the stable main-clone basename; `worktree` names the current
    // checkout only when it differs (a linked worktree — the shared store serves
    // several). Both are basenames, never paths.
    let current = basename(&worktree_root).unwrap_or_else(|| REPOSITORY_FLOOR.to_owned());
    let repository = main_worktree_basename(&options.repo).unwrap_or_else(|| current.clone());
    let worktree = (current != repository).then_some(current);

    Ok(StoreIdentity {
        store_identity,
        context_identity,
        repository,
        worktree,
        placement,
        family,
    })
}

/// Map `command_view().mode` to a placement. The single site for this mapping.
fn placement_for(mode: &str) -> StorePlacement {
    match mode {
        "user-level" => StorePlacement {
            tier: "family",
            label: "family store",
        },
        "ephemeral" => StorePlacement {
            tier: "ephemeral",
            label: "ephemeral store",
        },
        // "local" and any unexpected value floor to the clone-local default.
        _ => StorePlacement {
            tier: "clone",
            label: "clone store",
        },
    }
}

/// `basename(parent(git_common_dir(repo)))` — the stable main-clone name. `None` when
/// the common dir has no parent basename (unusual layouts); the caller falls back.
fn main_worktree_basename(repo: &Path) -> Option<String> {
    let common = git_common_dir(repo).ok()?; // <main>/.git (absolute)
    basename(common.parent()?) // <main>
}

/// Final non-empty path component, or `None` when the path has none.
fn basename(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

/// Hash a normalized path into the opaque identity shared by `store status` and
/// `/api/identity`. This stays workflow-private: equality, not path recovery or
/// digest parsing, is the public contract.
pub(super) fn opaque_path_identity(namespace: &str, path: &Path) -> Result<String> {
    let normalized = normalize_path_without_requiring_leaf(path)?;
    let digest = Sha256::digest(normalized.as_os_str().as_encoded_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").expect("writing to a string cannot fail");
    }
    Ok(format!("{namespace}:sha256:{hex}"))
}

fn normalize_path_without_requiring_leaf(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| ShoreError::Message(format!("resolve current directory: {error}")))?
            .join(path)
    };
    let mut existing = absolute.as_path();
    let mut missing = Vec::<OsString>::new();

    while !existing.try_exists().map_err(|error| {
        ShoreError::Message(format!(
            "inspect identity path {}: {error}",
            existing.display()
        ))
    })? {
        let name = existing.file_name().ok_or_else(|| {
            ShoreError::Message(format!(
                "cannot find an existing ancestor for identity path {}",
                absolute.display()
            ))
        })?;
        missing.push(name.to_owned());
        existing = existing.parent().ok_or_else(|| {
            ShoreError::Message(format!(
                "cannot find an existing ancestor for identity path {}",
                absolute.display()
            ))
        })?;
    }

    let mut normalized = existing.canonicalize().map_err(|error| {
        ShoreError::Message(format!(
            "canonicalize identity path ancestor {}: {error}",
            existing.display()
        ))
    })?;
    for component in missing.into_iter().rev() {
        normalized.push(component);
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::session::store::store_config::{StoreMode, write_store_config};
    use crate::session::{StoreLinkOptions, link_store_to_family};

    struct TestRepo {
        root: TempDir,
    }

    impl TestRepo {
        fn new() -> Self {
            let root = TempDir::new().expect("create temp git repository directory");
            let repo = Self { root };
            repo.git(["init"]);
            repo.git(["config", "user.name", "Shore Tests"]);
            repo.git(["config", "user.email", "shore-tests@example.com"]);
            repo.git(["config", "commit.gpgsign", "false"]);
            repo
        }
        fn path(&self) -> &Path {
            self.root.path()
        }
        fn write(&self, path: &str, contents: &str) {
            let path = self.root.path().join(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }
        fn commit_all(&self, message: &str) {
            self.git(["add", "--all"]);
            self.git(["commit", "-m", message]);
        }
        fn git<I, S>(&self, args: I)
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            run_git(self.root.path(), args);
        }
    }

    fn run_git<I, S>(cwd: &Path, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed in {}\nstderr:\n{}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A main clone plus one real linked worktree on a fresh branch, sharing the
    /// common-dir store.
    struct LinkedWorktreeFixture {
        main: TestRepo,
        _parent: TempDir,
        linked_path: PathBuf,
    }

    impl LinkedWorktreeFixture {
        fn new(dir_name: &str, branch: &str) -> Self {
            let main = TestRepo::new();
            main.write("README.md", "base\n");
            main.commit_all("base");

            let parent = TempDir::new().expect("worktree parent");
            let linked_path = parent.path().join(dir_name);
            main.git([
                OsString::from("worktree"),
                OsString::from("add"),
                OsString::from("-b"),
                OsString::from(branch),
                linked_path.clone().into_os_string(),
            ]);

            Self {
                main,
                _parent: parent,
                linked_path,
            }
        }
    }

    /// Set `SHORE_HOME` for the duration of `f`. nextest's process-per-test keeps the
    /// mutation contained (the `keys/home.rs` seam). SAFETY: single-threaded test.
    fn with_shore_home<T>(home: &Path, f: impl FnOnce() -> T) -> T {
        unsafe {
            std::env::set_var("SHORE_HOME", home);
        }
        let out = f();
        unsafe {
            std::env::remove_var("SHORE_HOME");
        }
        out
    }

    #[test]
    fn clone_local_identity_reports_clone_placement_and_no_family() {
        let repo = TestRepo::new();
        repo.write("README.md", "base\n");
        repo.commit_all("base");

        let id = store_identity(StoreIdentityOptions::new(repo.path())).unwrap();

        assert_eq!(id.placement.tier, "clone");
        assert_eq!(id.placement.label, "clone store");
        assert!(id.family.is_none());
        // The main worktree: `worktree` is suppressed (equals `repository`).
        assert!(id.worktree.is_none());
        // `repository` is a basename — never an absolute path.
        assert!(!id.repository.is_empty());
        assert!(!id.repository.contains(std::path::MAIN_SEPARATOR));
        // The basename matches the repo directory name.
        let expected = repo.path().file_name().unwrap().to_str().unwrap();
        assert_eq!(id.repository, expected);
    }

    #[test]
    fn ephemeral_mode_reports_ephemeral_placement() {
        let repo = TestRepo::new();
        repo.write("README.md", "base\n");
        repo.commit_all("base");
        write_store_config(repo.path(), StoreMode::Ephemeral).unwrap();

        let id = store_identity(StoreIdentityOptions::new(repo.path())).unwrap();

        assert_eq!(id.placement.tier, "ephemeral");
        assert_eq!(id.placement.label, "ephemeral store");
        assert!(id.family.is_none());
    }

    #[test]
    fn user_level_identity_reports_family_placement_and_slug() {
        let repo = TestRepo::new();
        repo.write("README.md", "base\n");
        repo.commit_all("base");

        let home = TempDir::new().unwrap();
        let id = with_shore_home(home.path(), || {
            link_store_to_family(StoreLinkOptions::new(
                repo.path(),
                Some("acme-web".to_owned()),
            ))
            .expect("link a clean, non-ephemeral, non-sensitive worktree");
            store_identity(StoreIdentityOptions::new(repo.path()))
        })
        .unwrap();

        assert_eq!(id.placement.tier, "family");
        assert_eq!(id.placement.label, "family store");
        assert_eq!(id.family.as_ref().map(|f| f.id.as_str()), Some("acme-web"));
    }

    #[test]
    fn linked_worktree_surfaces_the_worktree_basename_distinct_from_repository() {
        // A linked worktree shares the main clone's common-dir store, so `repository`
        // is the MAIN clone basename and `worktree` is the linked-worktree basename.
        let fixture = LinkedWorktreeFixture::new("feat-foo", "feat/foo");
        let id = store_identity(StoreIdentityOptions::new(&fixture.linked_path)).unwrap();

        let main_basename = fixture.main.path().file_name().unwrap().to_str().unwrap();
        assert_eq!(id.repository, main_basename);
        assert_eq!(id.worktree.as_deref(), Some("feat-foo"));
        assert_ne!(id.repository, id.worktree.clone().unwrap());
    }

    #[test]
    fn serialized_identity_carries_no_absolute_path() {
        let repo = TestRepo::new();
        repo.write("README.md", "base\n");
        repo.commit_all("base");

        let id = store_identity(StoreIdentityOptions::new(repo.path())).unwrap();
        let json = serde_json::to_string(&id).unwrap();
        let abs = repo.path().to_str().unwrap();
        assert!(
            !json.contains(abs),
            "identity JSON leaked an absolute path: {json}"
        );
        assert!(json.contains("\"placement\""));
        assert!(json.contains("\"tier\":\"clone\""));
    }
}
