//! Canonical operational path authority.

use std::path::{Path, PathBuf};

use crate::error::{Result, ShoreError};

/// Canonical paths rooted in one Git worktree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryPaths {
    worktree_root: PathBuf,
    config_dir: PathBuf,
    worktree_store: PathBuf,
}

impl RepositoryPaths {
    pub fn resolve(repo: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::from_worktree_root(crate::git::git_worktree_root(
            repo.as_ref(),
        )?))
    }

    pub fn from_worktree_root(worktree_root: impl Into<PathBuf>) -> Self {
        let worktree_root = worktree_root.into();
        let config_dir = worktree_root.join(".pointbreak");
        let worktree_store = config_dir.join("data");
        Self {
            worktree_root,
            config_dir,
            worktree_store,
        }
    }

    pub fn worktree_root(&self) -> &Path {
        &self.worktree_root
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn worktree_store(&self) -> &Path {
        &self.worktree_store
    }

    pub fn gitignore(&self) -> PathBuf {
        self.config_dir().join(".gitignore")
    }

    pub fn store_config(&self) -> PathBuf {
        self.config_dir().join("store.json")
    }

    pub fn store_config_local(&self) -> PathBuf {
        self.config_dir().join("store.local.json")
    }

    pub fn delegates(&self) -> PathBuf {
        self.config_dir().join("delegates.json")
    }

    pub fn delegates_local(&self) -> PathBuf {
        self.config_dir().join("delegates.local.json")
    }

    pub fn actor_attributes(&self) -> PathBuf {
        self.config_dir().join("actor-attributes.json")
    }

    pub fn actor_attributes_local(&self) -> PathBuf {
        self.config_dir().join("actor-attributes.local.json")
    }

    pub fn allowed_signers(&self) -> PathBuf {
        self.config_dir().join("allowed-signers.json")
    }

    pub fn sensitivity(&self) -> PathBuf {
        self.config_dir().join("sensitivity.json")
    }

    pub fn sensitivity_local(&self) -> PathBuf {
        self.config_dir().join("sensitivity.local.json")
    }

    #[cfg(test)]
    pub(crate) fn is_worktree_store_relative(path: &Path) -> bool {
        let store = Self::from_worktree_root(PathBuf::new()).worktree_store;
        path == store || path.starts_with(store)
    }
}

/// Reject pre-existing links in canonical repository control write paths.
/// This is a static check, not protection against concurrent path replacement.
pub(crate) fn require_plain_repository_control_write(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.file_name() != Some(std::ffi::OsStr::new(".pointbreak"))
        || !matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some(
                "store.json"
                    | "store.local.json"
                    | ".gitignore"
                    | "delegates.json"
                    | "delegates.local.json"
                    | "actor-attributes.json"
                    | "actor-attributes.local.json"
                    | "allowed-signers.json"
            )
        )
    {
        return Ok(());
    }
    require_plain_entry(parent, true)?;
    require_plain_entry(path, false)
}

/// Check only the two canonical worktree-store directory entries before setup.
pub(crate) fn require_plain_worktree_store_write_root(paths: &RepositoryPaths) -> Result<()> {
    require_plain_entry(paths.config_dir(), true)?;
    require_plain_entry(paths.worktree_store(), true)
}

fn require_plain_entry(path: &Path, directory: bool) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(ShoreError::Message(format!(
                "inspect repository-controlled write path {}: {error}",
                path.display()
            )));
        }
    };
    if is_link_or_reparse_point(&metadata) {
        return Err(ShoreError::Message(format!(
            "refuse repository-controlled write through link or reparse point {}",
            path.display()
        )));
    }
    if (directory && metadata.is_dir()) || (!directory && metadata.is_file()) {
        return Ok(());
    }
    let expected = if directory {
        "directory"
    } else {
        "regular file"
    };
    Err(ShoreError::Message(format!(
        "repository-controlled write path {} must be a plain {expected}",
        path.display()
    )))
}

fn is_link_or_reparse_point(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

/// Canonical store and binding paths rooted in one Git common directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommonDirPaths {
    common_dir: PathBuf,
}

impl CommonDirPaths {
    pub fn resolve(repo: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::from_common_dir(crate::git::git_common_dir(
            repo.as_ref(),
        )?))
    }

    pub fn from_common_dir(common_dir: impl Into<PathBuf>) -> Self {
        Self {
            common_dir: common_dir.into(),
        }
    }

    pub fn common_dir(&self) -> &Path {
        &self.common_dir
    }

    pub fn store_dir(&self) -> PathBuf {
        self.common_dir.join("pointbreak")
    }

    pub fn binding(&self) -> PathBuf {
        self.common_dir.join("pointbreak.link.json")
    }
}

/// Resolved Pointbreak user-home paths shared by keys and user-level stores.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserHomePaths {
    root: PathBuf,
}

impl UserHomePaths {
    /// Resolve the canonical user home from process environment.
    pub fn resolve() -> Result<Self> {
        Self::resolve_from(
            std::env::var_os(crate::environment::HOME).map(PathBuf::from),
            std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            std::env::var_os("HOME").map(PathBuf::from),
            std::env::var_os("APPDATA").map(PathBuf::from),
        )
    }

    /// Resolve from injectable platform inputs while retaining the existing
    /// base-directory precedence.
    pub fn resolve_from(
        explicit: Option<PathBuf>,
        xdg_data_home: Option<PathBuf>,
        home: Option<PathBuf>,
        app_data: Option<PathBuf>,
    ) -> Result<Self> {
        if let Some(root) = explicit {
            if root.as_os_str().is_empty() {
                return Err(ShoreError::Message(format!(
                    "{} must not be empty",
                    crate::environment::HOME
                )));
            }
            if !root.is_absolute() {
                return Err(ShoreError::Message(format!(
                    "{} must be an absolute path, got {}",
                    crate::environment::HOME,
                    root.display()
                )));
            }
            return Ok(Self { root });
        }
        if let Some(xdg) = xdg_data_home {
            return Ok(Self {
                root: xdg.join("pointbreak"),
            });
        }
        #[cfg(unix)]
        if let Some(home) = home {
            return Ok(Self {
                root: home.join(".pointbreak"),
            });
        }
        #[cfg(windows)]
        if let Some(app_data) = app_data {
            return Ok(Self {
                root: app_data.join("pointbreak"),
            });
        }
        let _ = (home, app_data);
        Err(ShoreError::Message(format!(
            "cannot resolve a Pointbreak user home: set {} or a platform home directory",
            crate::environment::HOME
        )))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn keys_dir(&self) -> PathBuf {
        self.root.join("keys")
    }

    pub fn stores_dir(&self) -> PathBuf {
        self.root.join("stores")
    }

    pub fn family_dir(&self, slug: &str) -> PathBuf {
        self.stores_dir().join(slug)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_write_guard_allows_absent_and_regular_entries() {
        let root = tempfile::tempdir().unwrap();
        let paths = RepositoryPaths::from_worktree_root(root.path());
        require_plain_repository_control_write(&paths.store_config()).unwrap();
        std::fs::create_dir(paths.config_dir()).unwrap();
        std::fs::write(paths.store_config(), "malformed old config").unwrap();
        require_plain_repository_control_write(&paths.store_config()).unwrap();
    }

    #[test]
    fn control_write_guard_rejects_wrong_entry_types() {
        let root = tempfile::tempdir().unwrap();
        let paths = RepositoryPaths::from_worktree_root(root.path());
        std::fs::write(paths.config_dir(), "file instead of directory").unwrap();
        let error = require_plain_repository_control_write(&paths.store_config())
            .unwrap_err()
            .to_string();
        assert!(error.contains("must be a plain directory"), "{error}");
        std::fs::remove_file(paths.config_dir()).unwrap();
        std::fs::create_dir_all(paths.store_config()).unwrap();
        let error = require_plain_repository_control_write(&paths.store_config())
            .unwrap_err()
            .to_string();
        assert!(error.contains("must be a plain regular file"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn control_write_guard_checks_exact_allowlist_before_following_links() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let paths = RepositoryPaths::from_worktree_root(root.path());
        std::fs::create_dir(paths.config_dir()).unwrap();
        let sentinel = external.path().join("sentinel");
        std::fs::write(&sentinel, b"external sentinel\n").unwrap();
        for name in [
            "store.json",
            "store.local.json",
            ".gitignore",
            "delegates.json",
            "delegates.local.json",
            "actor-attributes.json",
            "actor-attributes.local.json",
            "allowed-signers.json",
        ] {
            let path = paths.config_dir().join(name);
            symlink(&sentinel, &path).unwrap();
            let error = require_plain_repository_control_write(&path)
                .unwrap_err()
                .to_string();
            assert!(error.contains("link or reparse point"), "{error}");
            assert!(error.contains(&path.display().to_string()), "{error}");
            assert!(
                std::fs::symlink_metadata(&path)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
        for path in [
            paths.config_dir().join("custom.json"),
            root.path().join("store.json"),
        ] {
            symlink(&sentinel, &path).unwrap();
            require_plain_repository_control_write(&path).unwrap();
        }
        assert_eq!(std::fs::read(sentinel).unwrap(), b"external sentinel\n");
    }

    #[cfg(unix)]
    #[test]
    fn control_write_guard_rejects_dangling_parent() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let paths = RepositoryPaths::from_worktree_root(root.path());
        let missing = root.path().join("missing");
        symlink(&missing, paths.config_dir()).unwrap();
        let error = require_plain_repository_control_write(&paths.store_config())
            .unwrap_err()
            .to_string();
        assert!(error.contains("link or reparse point"), "{error}");
        assert!(
            error.contains(&paths.config_dir().display().to_string()),
            "{error}"
        );
        assert!(!missing.exists());
    }

    #[cfg(unix)]
    #[test]
    fn control_write_guard_allows_alias_at_repository_anchor() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let links = tempfile::tempdir().unwrap();
        let alias = links.path().join("repo");
        symlink(root.path(), &alias).unwrap();
        let paths = RepositoryPaths::from_worktree_root(alias);
        require_plain_repository_control_write(&paths.store_config()).unwrap();
        std::fs::create_dir(paths.config_dir()).unwrap();
        std::fs::write(paths.store_config(), "existing config").unwrap();
        require_plain_repository_control_write(&paths.store_config()).unwrap();
    }

    #[test]
    fn explicit_home_is_absolute_nonempty_and_owns_all_children() {
        let root = std::env::temp_dir().join("pointbreak-home");
        let paths = UserHomePaths::resolve_from(
            Some(root.clone()),
            Some(std::env::temp_dir().join("xdg-data")),
            Some(std::env::temp_dir().join("platform-home")),
            None,
        )
        .unwrap();
        assert_eq!(paths.root(), root);
        assert_eq!(paths.keys_dir(), root.join("keys"));
        assert_eq!(paths.family_dir("acme"), root.join("stores/acme"));

        for invalid in [PathBuf::new(), PathBuf::from("relative-home")] {
            assert!(UserHomePaths::resolve_from(Some(invalid), None, None, None).is_err());
        }
    }

    #[test]
    fn xdg_data_home_wins_over_platform_home_without_an_override() {
        let xdg = std::env::temp_dir().join("xdg-data");
        let paths = UserHomePaths::resolve_from(
            None,
            Some(xdg.clone()),
            Some(std::env::temp_dir().join("platform-home")),
            None,
        )
        .unwrap();
        assert_eq!(paths.root(), xdg.join("pointbreak"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_home_uses_the_pointbreak_dot_directory() {
        let paths = UserHomePaths::resolve_from(None, None, Some(PathBuf::from("/home/dev")), None)
            .unwrap();
        assert_eq!(paths.root(), Path::new("/home/dev/.pointbreak"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_app_data_uses_the_pointbreak_directory() {
        let paths = UserHomePaths::resolve_from(
            None,
            None,
            None,
            Some(PathBuf::from(r"C:\Users\dev\AppData\Roaming")),
        )
        .unwrap();
        assert_eq!(
            paths.root(),
            Path::new(r"C:\Users\dev\AppData\Roaming\pointbreak")
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_explicit_home_is_preserved() {
        use std::os::unix::ffi::OsStringExt as _;

        let root = PathBuf::from(std::ffi::OsString::from_vec(vec![
            b'/', b't', b'm', b'p', 0xff,
        ]));
        let paths = UserHomePaths::resolve_from(Some(root.clone()), None, None, None).unwrap();
        assert_eq!(paths.root(), root);
    }

    #[test]
    fn absent_platform_bases_are_an_error() {
        assert!(UserHomePaths::resolve_from(None, None, None, None).is_err());
    }
}
