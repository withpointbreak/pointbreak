use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Result, ShoreError};
use crate::git::git_worktree_root;
use crate::storage::{LocalStorage, TempSweepAge};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ShoreStorePaths {
    worktree_root: PathBuf,
    shore_dir: PathBuf,
}

impl ShoreStorePaths {
    pub(crate) fn resolve(repo: impl AsRef<Path>) -> Result<Self> {
        let worktree_root = git_worktree_root(repo.as_ref())?;
        let shore_dir = worktree_root.join(".shore");
        Ok(Self {
            worktree_root,
            shore_dir,
        })
    }

    pub(crate) fn worktree_root(&self) -> &Path {
        &self.worktree_root
    }

    pub(crate) fn shore_dir(&self) -> &Path {
        &self.shore_dir
    }

    pub(crate) fn state_path(&self) -> PathBuf {
        self.shore_dir.join("state.json")
    }
}

pub fn shore_dir_for_repo(repo: &Path) -> Result<PathBuf> {
    Ok(ShoreStorePaths::resolve(repo)?.shore_dir().to_path_buf())
}

pub(crate) fn ensure_store_dirs(shore_dir: &Path) -> Result<()> {
    for dir in [
        shore_dir.join("events"),
        shore_dir.join("artifacts/notes"),
        shore_dir.join("artifacts/revisions"),
        shore_dir.join("artifacts/snapshots"),
    ] {
        fs::create_dir_all(&dir).map_err(|error| io_error("create directory", &dir, error))?;
    }
    Ok(())
}

pub(crate) fn sweep_stale_temp_files(storage: &LocalStorage, shore_dir: &Path) -> Result<()> {
    storage.sweep_temp_files(shore_dir, TempSweepAge::workflow_startup())
}

pub(crate) fn prepare_shore_writer(paths: &ShoreStorePaths, storage: &LocalStorage) -> Result<()> {
    sweep_stale_temp_files(storage, paths.shore_dir())?;
    ensure_store_dirs(paths.shore_dir())?;
    ensure_shore_ignored(paths.worktree_root())
}

pub fn ensure_shore_ignored(worktree_root: &Path) -> Result<()> {
    let gitignore_path = worktree_root.join(".gitignore");
    let current = match fs::read_to_string(&gitignore_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(io_error("read .gitignore", &gitignore_path, error));
        }
    };

    if has_shore_ignore_entry(&current) {
        return Ok(());
    }

    let mut updated = current;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(".shore/\n");

    fs::write(&gitignore_path, updated)
        .map_err(|error| io_error("write .gitignore", &gitignore_path, error))
}

fn has_shore_ignore_entry(contents: &str) -> bool {
    contents
        .lines()
        .map(str::trim)
        .any(|line| matches!(line, ".shore" | ".shore/" | "/.shore" | "/.shore/"))
}

fn io_error(action: &str, path: &Path, error: std::io::Error) -> ShoreError {
    ShoreError::Message(format!("{action} {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    #[test]
    fn shore_store_paths_resolve_from_subdirectory() {
        let repo = git_repo();
        fs::create_dir_all(repo.path().join("src/nested")).unwrap();
        let expected_root = repo.path().canonicalize().unwrap();

        let paths = ShoreStorePaths::resolve(repo.path().join("src/nested")).unwrap();

        assert_eq!(paths.worktree_root(), expected_root.as_path());
        assert_eq!(paths.shore_dir(), expected_root.join(".shore").as_path());
        assert_eq!(paths.state_path(), expected_root.join(".shore/state.json"));
    }

    #[test]
    fn public_shore_dir_helper_delegates_to_store_paths() {
        let repo = git_repo();

        let from_public_helper = shore_dir_for_repo(repo.path()).unwrap();
        let from_paths = ShoreStorePaths::resolve(repo.path())
            .unwrap()
            .shore_dir()
            .to_path_buf();

        assert_eq!(from_public_helper, from_paths);
    }

    #[test]
    fn prepare_shore_writer_creates_current_store_dirs_and_ignore_entry() {
        let repo = git_repo();
        let paths = ShoreStorePaths::resolve(repo.path()).unwrap();
        let storage = LocalStorage::new(paths.shore_dir());

        prepare_shore_writer(&paths, &storage).unwrap();

        assert!(paths.shore_dir().join("events").is_dir());
        assert!(paths.shore_dir().join("artifacts/notes").is_dir());
        assert!(paths.shore_dir().join("artifacts/revisions").is_dir());
        assert!(paths.shore_dir().join("artifacts/snapshots").is_dir());
        assert_eq!(
            fs::read_to_string(repo.path().join(".gitignore")).unwrap(),
            ".shore/\n"
        );
    }

    #[test]
    fn prepare_shore_writer_preserves_fresh_temp_files() {
        let repo = git_repo();
        let paths = ShoreStorePaths::resolve(repo.path()).unwrap();
        fs::create_dir_all(paths.shore_dir().join("events")).unwrap();
        let temp = paths.shore_dir().join("events/.shore-write.fresh.tmp");
        fs::write(&temp, "in flight").unwrap();
        let storage = LocalStorage::new(paths.shore_dir());

        prepare_shore_writer(&paths, &storage).unwrap();

        assert_eq!(fs::read_to_string(temp).unwrap(), "in flight");
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
