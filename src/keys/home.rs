use std::path::PathBuf;

use crate::error::{Result, ShoreError};

/// Environment override for the user-level keystore root, taken verbatim. This
/// is the hermetic-test seam: tests point it at a `tempfile` directory so the
/// keystore never touches the real user home.
pub(crate) const KEYS_HOME_ENV: &str = "SHORE_HOME";

/// Resolve and create the user-level keystore's `keys/` directory, returning its
/// path. The directory tree is created if absent; on Unix it is created `0700`.
pub(crate) fn keys_dir() -> Result<PathBuf> {
    let root = resolve_keys_root(
        std::env::var_os(KEYS_HOME_ENV).map(PathBuf::from),
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("APPDATA").map(PathBuf::from),
    )?;
    let keys = root.join("keys");
    create_private_dir(&keys)?;
    Ok(keys)
}

/// Pure resolution seam (kept env-free for testing). Precedence: explicit
/// override, then `$XDG_DATA_HOME/shore`, then the platform default
/// (`$HOME/.shore` on Unix, `%APPDATA%\shore` on Windows). A missing home with
/// no override is a typed error.
fn resolve_keys_root(
    shore_home: Option<PathBuf>,
    xdg_data_home: Option<PathBuf>,
    home: Option<PathBuf>,
    app_data: Option<PathBuf>,
) -> Result<PathBuf> {
    if let Some(root) = shore_home {
        return Ok(root);
    }
    if let Some(xdg) = xdg_data_home {
        return Ok(xdg.join("shore"));
    }
    #[cfg(unix)]
    if let Some(home) = home {
        return Ok(home.join(".shore"));
    }
    #[cfg(windows)]
    if let Some(app_data) = app_data {
        return Ok(app_data.join("shore"));
    }
    // Keep both bindings live on every platform so neither triggers an
    // unused-variable warning on the leg that does not consume it.
    let _ = (home, app_data);
    Err(ShoreError::Message(
        "cannot resolve a user-level key home: set SHORE_HOME or a platform home directory"
            .to_owned(),
    ))
}

/// Create `dir` (and parents) if absent. On Unix the leaf is set to mode `0700`
/// so private keys beneath it are not world-readable; on other platforms the
/// directory inherits the default ACL (documented caveat).
fn create_private_dir(dir: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|error| {
        ShoreError::Message(format!("create key home {}: {error}", dir.display()))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|error| {
            ShoreError::Message(format!("set 0700 on {}: {error}", dir.display()))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Pure seam: (shore_home, xdg_data_home, home, app_data) -> resolved root.
    // None models an unset variable; the seam never reads the process env.

    #[test]
    fn shore_home_override_is_used_verbatim() {
        let root = resolve_keys_root(
            Some(PathBuf::from("/tmp/hermetic-store")),
            Some(PathBuf::from("/xdg/data")),
            Some(PathBuf::from("/home/dev")),
            None,
        )
        .unwrap();
        assert_eq!(root, PathBuf::from("/tmp/hermetic-store"));
    }

    #[test]
    fn xdg_data_home_wins_over_home_when_no_override() {
        let root = resolve_keys_root(
            None,
            Some(PathBuf::from("/xdg/data")),
            Some(PathBuf::from("/home/dev")),
            None,
        )
        .unwrap();
        assert_eq!(root, PathBuf::from("/xdg/data").join("shore"));
    }

    #[cfg(unix)]
    #[test]
    fn home_dot_shore_is_the_unix_default() {
        let root = resolve_keys_root(None, None, Some(PathBuf::from("/home/dev")), None).unwrap();
        assert_eq!(root, PathBuf::from("/home/dev").join(".shore"));
    }

    #[cfg(windows)]
    #[test]
    fn app_data_shore_is_the_windows_default() {
        let root = resolve_keys_root(
            None,
            None,
            None,
            Some(PathBuf::from(r"C:\Users\dev\AppData\Roaming")),
        )
        .unwrap();
        assert_eq!(
            root,
            PathBuf::from(r"C:\Users\dev\AppData\Roaming").join("shore")
        );
    }

    #[test]
    fn missing_home_with_no_override_is_a_typed_error() {
        let result = resolve_keys_root(None, None, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn keys_dir_under_override_creates_keys_subtree_deterministically() {
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: single-threaded test; the override is the documented hermetic seam.
        unsafe {
            std::env::set_var(KEYS_HOME_ENV, tmp.path());
        }
        let first = keys_dir().unwrap();
        let second = keys_dir().unwrap();
        unsafe {
            std::env::remove_var(KEYS_HOME_ENV);
        }

        assert_eq!(first, second, "resolution is deterministic under override");
        assert_eq!(first, tmp.path().join("keys"));
        assert!(first.is_dir(), "the keys/ subtree is created");
    }

    #[cfg(unix)]
    #[test]
    fn created_keys_directory_is_0700_on_unix() {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var(KEYS_HOME_ENV, tmp.path());
        }
        let dir = keys_dir().unwrap();
        unsafe {
            std::env::remove_var(KEYS_HOME_ENV);
        }

        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "keystore dir must be private");
    }
}
