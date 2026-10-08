use std::collections::HashSet;
use std::fs::File;
use std::marker::PhantomData;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

use crate::error::{Result, ShoreError};
use crate::paths::CommonDirPaths;
use crate::session::store::authority_lock::{
    PhysicalFileIdentity, file_identity, open_identity_path,
};

#[allow(
    dead_code,
    reason = "the repository-bound consumer follows this guard producer"
)]
pub(in crate::session) const CLONE_BINDING_BUSY_PREFIX: &str = "clone_binding_busy;";

type PhysicalKey = (u64, u128);

static HELD_CLONE_DIRECTORIES: OnceLock<Mutex<HashSet<PhysicalKey>>> = OnceLock::new();

fn held_clone_directories() -> &'static Mutex<HashSet<PhysicalKey>> {
    HELD_CLONE_DIRECTORIES.get_or_init(|| Mutex::new(HashSet::new()))
}

struct DirectoryWitness {
    path: PathBuf,
    handle: File,
    identity: PhysicalFileIdentity,
}

/// Fail-fast admission for one physical Git common directory's binding.
#[allow(
    dead_code,
    reason = "the repository-bound consumer follows this guard producer"
)]
pub(in crate::session) struct CloneBindingGuard {
    common_dir: PathBuf,
    directories: Vec<DirectoryWitness>,
    lock: File,
    lock_identity: PhysicalFileIdentity,
    key: PhysicalKey,
    _not_send: PhantomData<Rc<()>>,
}

#[allow(
    dead_code,
    reason = "the repository-bound consumer follows this guard producer"
)]
impl CloneBindingGuard {
    pub(in crate::session) fn try_acquire(common_dir: &Path) -> Result<Option<Self>> {
        Self::acquire(common_dir, true)
    }

    /// Reader admission requires the stable leaf provisioned by an authorized writer.
    pub(in crate::session) fn try_acquire_existing(common_dir: &Path) -> Result<Option<Self>> {
        Self::acquire(common_dir, false)
    }

    fn acquire(common_dir: &Path, create: bool) -> Result<Option<Self>> {
        let (common_dir, directories) = capture_directories(common_dir)?;
        let key = directories
            .last()
            .expect("an absolute path has a root directory")
            .identity
            .parts();
        let lock_path = CommonDirPaths::from_common_dir(common_dir.as_path()).binding_lock();
        // First use may create only the stable lock leaf in an observed parent.
        let lock = open_identity_path(&lock_path, false, create)?;
        let lock_identity = file_identity(&lock)?;
        ensure_supported_filesystem(&lock)?;

        let mut held = held_clone_directories()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if held.contains(&key) {
            return Ok(None);
        }
        match lock.try_lock() {
            Ok(()) => {
                held.insert(key);
                drop(held);
                let guard = Self {
                    common_dir,
                    directories,
                    lock,
                    lock_identity,
                    key,
                    _not_send: PhantomData,
                };
                guard.validate(&guard.common_dir)?;
                Ok(Some(guard))
            }
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => Err(ShoreError::Message(format!(
                "could not acquire clone binding lock {}: {error}",
                lock_path.display()
            ))),
        }
    }

    /// Borrow N from the actual held admission, only for synchronous identity checks.
    pub(in crate::session) fn retained_directories(&self) -> impl Iterator<Item = (&Path, &File)> {
        self.directories
            .iter()
            .map(|w| (w.path.as_path(), &w.handle))
    }

    pub(in crate::session) fn retained_lock(&self) -> &File {
        &self.lock
    }

    pub(in crate::session) fn validate(&self, common_dir: &Path) -> Result<()> {
        if normalize_absolute(common_dir)? != self.common_dir {
            return Err(physical_mismatch());
        }
        for witness in &self.directories {
            if file_identity(&witness.handle)? != witness.identity {
                return Err(physical_mismatch());
            }
            let named = open_identity_path(&witness.path, true, false)?;
            if file_identity(&named)? != witness.identity {
                return Err(physical_mismatch());
            }
        }
        let named_lock = open_identity_path(
            &CommonDirPaths::from_common_dir(self.common_dir.as_path()).binding_lock(),
            false,
            false,
        )?;
        if file_identity(&named_lock)? != self.lock_identity
            || file_identity(&self.lock)? != self.lock_identity
        {
            return Err(physical_mismatch());
        }
        Ok(())
    }
}

impl Drop for CloneBindingGuard {
    fn drop(&mut self) {
        let _ = self.lock.unlock();
        held_clone_directories()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.key);
    }
}

fn physical_mismatch() -> ShoreError {
    ShoreError::Message("physical clone binding root or stable lock changed".to_owned())
}

fn normalize_absolute(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(ShoreError::Message(
            "clone binding common directory must be absolute".to_owned(),
        ));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => return Err(physical_mismatch()),
        }
    }
    Ok(normalized)
}

fn capture_directories(path: &Path) -> Result<(PathBuf, Vec<DirectoryWitness>)> {
    let normalized = normalize_absolute(path)?;
    let mut prefix = PathBuf::new();
    let mut directories = Vec::new();
    for component in normalized.components() {
        prefix.push(component.as_os_str());
        // On Windows, a volume prefix alone is not a directory; its root is.
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let handle = open_identity_path(&prefix, true, false)?;
        ensure_supported_filesystem(&handle)?;
        let identity = file_identity(&handle)?;
        directories.push(DirectoryWitness {
            path: prefix.clone(),
            handle,
            identity,
        });
    }
    Ok((normalized, directories))
}

#[cfg(target_os = "macos")]
fn ensure_supported_filesystem(file: &File) -> Result<()> {
    use std::os::fd::AsRawFd as _;
    let mut info = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: the retained fd stays live, and fstatfs fills the output on success.
    if unsafe { libc::fstatfs(file.as_raw_fd(), info.as_mut_ptr()) } != 0 {
        return Err(ShoreError::Message(format!(
            "could not inspect clone binding filesystem: {}",
            std::io::Error::last_os_error()
        )));
    }
    // SAFETY: successful fstatfs initialized the statfs value.
    let info = unsafe { info.assume_init() };
    let fs_name: Vec<u8> = info
        .f_fstypename
        .iter()
        .take_while(|&&byte| byte != 0)
        .map(|&byte| byte as u8)
        .collect();
    if info.f_flags & (libc::MNT_LOCAL as u32) == 0 || fs_name != b"apfs" {
        return Err(ShoreError::Message(
            "clone binding requires a local APFS filesystem".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn ensure_supported_filesystem(_file: &File) -> Result<()> {
    Err(ShoreError::Message(
        "clone binding physical filesystem policy is not qualified on this platform".to_owned(),
    ))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::io::{BufRead, Read, Write};
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;

    use super::*;

    fn acquire(dir: &Path) -> CloneBindingGuard {
        CloneBindingGuard::try_acquire(dir).unwrap().unwrap()
    }

    fn disposable_common_parent() -> tempfile::TempDir {
        tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap()
    }

    #[test]
    fn stable_lock_reentry_and_drop() {
        let root = disposable_common_parent();
        let first = acquire(root.path());
        assert!(root.path().join("pointbreak.link.lock").is_file());
        assert!(
            CloneBindingGuard::try_acquire(root.path())
                .unwrap()
                .is_none()
        );
        first.validate(root.path()).unwrap();
        drop(first);
        let second = acquire(root.path());
        second.validate(root.path()).unwrap();
    }

    #[test]
    fn case_folded_alias_when_available_and_independent_clone_admission() {
        let parent = disposable_common_parent();
        let first_root = parent.path().join("clone-binding-case");
        std::fs::create_dir(&first_root).unwrap();
        let alias = parent.path().join("CLONE-BINDING-CASE");
        let other_root = disposable_common_parent();
        let first = acquire(&first_root);
        // Case-insensitive APFS exposes distinct names for the same directory.
        // Case-sensitive APFS has no such alias, so retain the independent-root control.
        if alias.is_dir() {
            let original = open_identity_path(&first_root, true, false).unwrap();
            let alternate = open_identity_path(&alias, true, false).unwrap();
            assert_ne!(
                normalize_absolute(&first_root).unwrap(),
                normalize_absolute(&alias).unwrap()
            );
            assert_eq!(
                file_identity(&original).unwrap(),
                file_identity(&alternate).unwrap()
            );
            assert!(CloneBindingGuard::try_acquire(&alias).unwrap().is_none());
        } else {
            eprintln!("case-folded APFS alias unavailable on this volume");
        }
        let other = acquire(other_root.path());
        other.validate(other_root.path()).unwrap();
        first.validate(&first_root).unwrap();
    }

    #[test]
    fn another_thread_refuses_then_reacquires() {
        let root = disposable_common_parent();
        let first = acquire(root.path());
        let path = root.path().to_path_buf();
        let (sender, receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            sender
                .send(CloneBindingGuard::try_acquire(&path).unwrap().is_none())
                .unwrap();
        });
        assert!(receiver.recv().unwrap());
        thread.join().unwrap();
        drop(first);
        acquire(root.path()).validate(root.path()).unwrap();
    }

    #[test]
    fn another_process_refuses_while_holder_is_live() {
        let root = disposable_common_parent();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "session::store::family_topology::tests::lock_holder_child",
                "--nocapture",
            ])
            .env("POINTBREAK_CLONE_GUARD_CHILD_ROOT", root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        let mut ready = false;
        while output.read_line(&mut line).unwrap() != 0 {
            if line.contains("CLONE_GUARD_READY") {
                ready = true;
                break;
            }
            line.clear();
        }
        assert!(ready, "holder exited before acquiring the guard: {line}");
        let busy = CloneBindingGuard::try_acquire(root.path())
            .unwrap()
            .is_none();
        child.stdin.take().unwrap().write_all(b"x").unwrap();
        assert!(child.wait().unwrap().success());
        assert!(busy);
        acquire(root.path()).validate(root.path()).unwrap();
    }

    #[test]
    #[ignore = "executed by the parent test with an assigned disposable root"]
    fn lock_holder_child() {
        let Ok(root) = std::env::var("POINTBREAK_CLONE_GUARD_CHILD_ROOT") else {
            return;
        };
        let _guard = acquire(Path::new(&root));
        println!("CLONE_GUARD_READY");
        std::io::stdout().flush().unwrap();
        let mut byte = [0];
        std::io::stdin().read_exact(&mut byte).unwrap();
    }

    #[test]
    fn root_and_lock_replacement_refuse_validation() {
        let outer = disposable_common_parent();
        let common = outer.path().join("common");
        std::fs::create_dir(&common).unwrap();
        let guard = acquire(&common);
        let lock = common.join("pointbreak.link.lock");
        let old_lock = common.join("old.lock");
        std::fs::rename(&lock, &old_lock).unwrap();
        std::fs::write(&lock, b"replacement").unwrap();
        assert!(guard.validate(&common).is_err());
        drop(guard);

        let guard = acquire(&common);
        let old_common = outer.path().join("old-common");
        std::fs::rename(&common, &old_common).unwrap();
        std::fs::create_dir(&common).unwrap();
        assert!(guard.validate(&common).is_err());
    }

    #[test]
    fn missing_parent_and_symlink_ancestor_refuse_without_store_creation() {
        let root = disposable_common_parent();
        let missing = root.path().join("missing").join("common");
        assert!(CloneBindingGuard::try_acquire(&missing).is_err());
        assert!(!missing.exists());
        #[cfg(unix)]
        {
            let actual = root.path().join("actual");
            std::fs::create_dir(&actual).unwrap();
            let alias = root.path().join("alias");
            std::os::unix::fs::symlink(&actual, &alias).unwrap();
            assert!(CloneBindingGuard::try_acquire(&alias).is_err());
            assert!(!actual.join("pointbreak").exists());
        }
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod unsupported_platform_tests {
    use super::*;

    #[test]
    fn admission_refuses_without_creating_a_lock() {
        let root = tempfile::tempdir().unwrap();
        let lock = root.path().join("pointbreak.link.lock");
        let error = match CloneBindingGuard::try_acquire(root.path()) {
            Ok(_) => panic!("unsupported platform admitted clone binding"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("not qualified on this platform"));
        assert!(!lock.exists());
    }
}
