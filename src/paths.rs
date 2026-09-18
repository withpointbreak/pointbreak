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
