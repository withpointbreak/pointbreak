use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread::ThreadId;

#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::{
    InteractionLockKindV1, InteractionLockModeV1, InteractionLockOutcomeV1,
    InteractionPhysicalLockHoldRecorderV1, begin_interaction_lock_attempt_v1,
};
use crate::error::{Result, ShoreError};

pub(crate) const STORE_AUTHORITY_LOCK_FILE: &str = "authority.writer.lock";

/// Process-wide and cross-process exclusion for mutations of one authoritative
/// store. Keeping the handle alive holds the operating-system lock; dropping it
/// releases the lock without deleting the stable lock file.
#[derive(Debug)]
struct StoreAuthorityLockState {
    _file: File,
    #[cfg(any(test, feature = "longitudinal-counting"))]
    _hold_recorder: Option<InteractionPhysicalLockHoldRecorderV1>,
}

#[derive(Debug)]
pub(crate) struct StoreAuthorityLock {
    _state: Arc<StoreAuthorityLockState>,
    key: (ThreadId, PathBuf),
    reentrant: bool,
    _not_send: PhantomData<Rc<()>>,
}

type HeldAuthorityLocks = HashMap<(ThreadId, PathBuf), Weak<StoreAuthorityLockState>>;

static HELD_AUTHORITY_LOCKS: OnceLock<Mutex<HeldAuthorityLocks>> = OnceLock::new();

impl StoreAuthorityLock {
    pub(crate) fn acquire(store_root: &Path) -> Result<Self> {
        let (key, path) = lock_key(store_root)?;
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let attempt = begin_interaction_lock_attempt_v1(
            InteractionLockKindV1::Authority,
            InteractionLockModeV1::Blocking,
        );
        if let Some(state) = held_state(&key) {
            #[cfg(any(test, feature = "longitudinal-counting"))]
            attempt.record_reentrant_acquired();
            return Ok(Self::new(key, state, true));
        }
        let file = match open_lock_file(&path) {
            Ok(file) => file,
            Err(error) => {
                #[cfg(any(test, feature = "longitudinal-counting"))]
                attempt.record_not_acquired(InteractionLockOutcomeV1::Failed);
                return Err(error);
            }
        };
        match file.lock() {
            Ok(()) => Ok(register_lock(
                key,
                file,
                #[cfg(any(test, feature = "longitudinal-counting"))]
                attempt.record_physical_acquired(),
            )),
            Err(error) => {
                #[cfg(any(test, feature = "longitudinal-counting"))]
                attempt.record_not_acquired(InteractionLockOutcomeV1::Failed);
                Err(lock_error(&path, "acquire", error))
            }
        }
    }

    pub(crate) fn try_acquire(store_root: &Path) -> Result<Option<Self>> {
        let (key, path) = lock_key(store_root)?;
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let attempt = begin_interaction_lock_attempt_v1(
            InteractionLockKindV1::Authority,
            InteractionLockModeV1::Try,
        );
        if let Some(state) = held_state(&key) {
            #[cfg(any(test, feature = "longitudinal-counting"))]
            attempt.record_reentrant_acquired();
            return Ok(Some(Self::new(key, state, true)));
        }
        let file = match open_lock_file(&path) {
            Ok(file) => file,
            Err(error) => {
                #[cfg(any(test, feature = "longitudinal-counting"))]
                attempt.record_not_acquired(InteractionLockOutcomeV1::Failed);
                return Err(error);
            }
        };
        match file.try_lock() {
            Ok(()) => Ok(Some(register_lock(
                key,
                file,
                #[cfg(any(test, feature = "longitudinal-counting"))]
                attempt.record_physical_acquired(),
            ))),
            Err(std::fs::TryLockError::WouldBlock) => {
                #[cfg(any(test, feature = "longitudinal-counting"))]
                attempt.record_not_acquired(InteractionLockOutcomeV1::Busy);
                Ok(None)
            }
            Err(std::fs::TryLockError::Error(error)) => {
                #[cfg(any(test, feature = "longitudinal-counting"))]
                attempt.record_not_acquired(InteractionLockOutcomeV1::Failed);
                Err(lock_error(&path, "acquire", error))
            }
        }
    }

    pub(crate) fn is_reentrant(&self) -> bool {
        self.reentrant
    }

    /// Queries the retained lock object without reopening or releasing authority.
    #[allow(
        dead_code,
        reason = "physical identity consumers are not yet connected"
    )]
    pub(in crate::session) fn physical_lock_identity(&self) -> Result<PhysicalFileIdentity> {
        file_identity(&self._state._file)
    }

    fn new(key: (ThreadId, PathBuf), state: Arc<StoreAuthorityLockState>, reentrant: bool) -> Self {
        Self {
            _state: state,
            key,
            reentrant,
            _not_send: PhantomData,
        }
    }
}

impl Drop for StoreAuthorityLock {
    fn drop(&mut self) {
        if Arc::strong_count(&self._state) != 1 {
            return;
        }
        let mut held = HELD_AUTHORITY_LOCKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if held
            .get(&self.key)
            .is_some_and(|entry| entry.ptr_eq(&Arc::downgrade(&self._state)))
        {
            held.remove(&self.key);
        }
    }
}

fn lock_key(store_root: &Path) -> Result<((ThreadId, PathBuf), PathBuf)> {
    std::fs::create_dir_all(store_root).map_err(|error| {
        ShoreError::Message(format!(
            "could not create store authority directory {}: {error}",
            store_root.display()
        ))
    })?;
    let canonical_root = store_root.canonicalize().map_err(|error| {
        ShoreError::Message(format!(
            "could not resolve store authority directory {}: {error}",
            store_root.display()
        ))
    })?;
    let path = canonical_root.join(STORE_AUTHORITY_LOCK_FILE);
    Ok(((std::thread::current().id(), path.clone()), path))
}

fn open_lock_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| lock_error(path, "open", error))?;
    Ok(file)
}

fn held_state(key: &(ThreadId, PathBuf)) -> Option<Arc<StoreAuthorityLockState>> {
    let mut held = HELD_AUTHORITY_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let state = held.get(key).and_then(Weak::upgrade);
    if state.is_none() {
        held.remove(key);
    }
    state
}

fn register_lock(
    key: (ThreadId, PathBuf),
    file: File,
    #[cfg(any(test, feature = "longitudinal-counting"))] hold_recorder: Option<
        InteractionPhysicalLockHoldRecorderV1,
    >,
) -> StoreAuthorityLock {
    let state = Arc::new(StoreAuthorityLockState {
        _file: file,
        #[cfg(any(test, feature = "longitudinal-counting"))]
        _hold_recorder: hold_recorder,
    });
    HELD_AUTHORITY_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key.clone(), Arc::downgrade(&state));
    StoreAuthorityLock::new(key, state, false)
}

fn lock_error(path: &Path, action: &str, error: std::io::Error) -> ShoreError {
    ShoreError::Message(format!(
        "could not {action} store authority lock {}: {error}",
        path.display()
    ))
}

#[allow(
    dead_code,
    reason = "physical identity consumers are not yet connected"
)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::session) struct PhysicalFileIdentity {
    volume: u64,
    file: u128,
}

#[allow(
    dead_code,
    reason = "physical identity consumers are not yet connected"
)]
impl PhysicalFileIdentity {
    pub(in crate::session) fn parts(&self) -> (u64, u128) {
        (self.volume, self.file)
    }
}

#[allow(
    dead_code,
    reason = "physical identity consumers are not yet connected"
)]
fn physical_error(error: std::io::Error) -> ShoreError {
    ShoreError::Message(format!(
        "could not validate physical store authority: {error}"
    ))
}

#[allow(
    dead_code,
    reason = "physical identity consumers are not yet connected"
)]
fn physical_mismatch() -> ShoreError {
    ShoreError::Message("physical store root or stable authority lock changed".to_owned())
}

/// Opens the final component without following it; ancestors remain caller-owned.
/// Creation is limited to a regular leaf in an existing parent, without truncation.
#[cfg(any(unix, windows))]
#[allow(
    dead_code,
    reason = "physical identity consumers are not yet connected"
)]
pub(in crate::session) fn open_identity_path(
    path: &Path,
    directory: bool,
    create: bool,
) -> Result<File> {
    if directory && create {
        return Err(ShoreError::Message(
            "physical identity opens cannot create directories".to_owned(),
        ));
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(create)
        .create(create)
        .truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | if directory { libc::O_DIRECTORY } else { 0 });
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        // Observe the final object itself; never follow a reparse point.
        options.custom_flags(0x0020_0000 | if directory { 0x0200_0000 } else { 0 });
    }
    let file = options.open(path).map_err(physical_error)?;
    let metadata = file.metadata().map_err(physical_error)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(physical_mismatch());
        }
    }
    if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(physical_mismatch());
    }
    Ok(file)
}

#[cfg(unix)]
#[allow(
    dead_code,
    reason = "physical identity consumers are not yet connected"
)]
pub(in crate::session) fn file_identity(file: &File) -> Result<PhysicalFileIdentity> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = file.metadata().map_err(physical_error)?;
    Ok(PhysicalFileIdentity {
        volume: metadata.dev(),
        file: u128::from(metadata.ino()),
    })
}

/// Query only: the caller retains ownership and must keep the fd live.
#[cfg(unix)]
#[allow(dead_code, reason = "borrowed native consumers are not yet admitted")]
pub(in crate::session) fn borrowed_file_stat(
    file: std::os::fd::BorrowedFd<'_>,
) -> std::io::Result<libc::stat> {
    use std::os::fd::AsRawFd as _;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: the borrowed fd and correctly sized writable output stay live
    // through fstat. No owner, duplicate or path-based handle is constructed.
    if unsafe { libc::fstat(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful fstat initialized the complete output.
    Ok(unsafe { stat.assume_init() })
}

#[cfg(unix)]
#[allow(dead_code, reason = "borrowed native consumers are not yet admitted")]
#[allow(
    clippy::unnecessary_cast,
    reason = "stat field widths vary across Unix targets"
)]
pub(in crate::session) fn identity_from_stat(stat: &libc::stat) -> PhysicalFileIdentity {
    PhysicalFileIdentity {
        volume: stat.st_dev as u64,
        file: stat.st_ino as u128,
    }
}

#[cfg(unix)]
#[allow(dead_code, reason = "borrowed native consumers are not yet admitted")]
pub(in crate::session) fn borrowed_file_identity(
    file: std::os::fd::BorrowedFd<'_>,
) -> Result<PhysicalFileIdentity> {
    let stat = borrowed_file_stat(file).map_err(physical_error)?;
    Ok(identity_from_stat(&stat))
}

#[cfg(windows)]
#[allow(
    dead_code,
    reason = "physical identity consumers are not yet connected"
)]
pub(in crate::session) fn file_identity(file: &File) -> Result<PhysicalFileIdentity> {
    use std::os::windows::io::AsHandle as _;
    borrowed_file_identity(file.as_handle())
}

/// Query only: the caller retains ownership and must keep the HANDLE live.
#[cfg(windows)]
#[allow(
    dead_code,
    reason = "physical identity consumers are not yet connected"
)]
pub(in crate::session) fn borrowed_file_identity(
    file: std::os::windows::io::BorrowedHandle<'_>,
) -> Result<PhysicalFileIdentity> {
    use std::os::windows::io::AsRawHandle as _;
    #[repr(C)]
    struct FileIdInfo {
        volume: u64,
        file: [u8; 16],
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandleEx(
            file: *mut std::ffi::c_void,
            class: i32,
            info: *mut std::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    let mut info = std::mem::MaybeUninit::<FileIdInfo>::zeroed();
    // SAFETY: the open handle and correctly sized writable output live through
    // the synchronous FILE_ID_INFO query, matching the existing NTFS adapter.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            18,
            info.as_mut_ptr().cast(),
            std::mem::size_of::<FileIdInfo>() as u32,
        )
    };
    if ok == 0 {
        return Err(physical_error(std::io::Error::last_os_error()));
    }
    // SAFETY: successful query initialized the complete FILE_ID_INFO.
    let info = unsafe { info.assume_init() };
    Ok(PhysicalFileIdentity {
        volume: info.volume,
        file: u128::from_le_bytes(info.file),
    })
}

#[cfg(not(any(unix, windows)))]
#[allow(
    dead_code,
    reason = "physical identity consumers are not yet connected"
)]
pub(in crate::session) fn file_identity(_file: &File) -> Result<PhysicalFileIdentity> {
    Err(ShoreError::Message(
        "unsupported platform: physical store identity unavailable".to_owned(),
    ))
}

#[cfg(not(any(unix, windows)))]
#[allow(
    dead_code,
    reason = "physical identity consumers are not yet connected"
)]
pub(in crate::session) fn open_identity_path(
    _path: &Path,
    _directory: bool,
    _create: bool,
) -> Result<File> {
    Err(ShoreError::Message(
        "unsupported platform: physical store identity unavailable".to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::bench_support::longitudinal::{
        InteractionActorV1, InteractionLockAcquisitionV1, InteractionLockKindV1,
        InteractionLockModeV1, InteractionLockOutcomeV1, LongitudinalCountingScopeV1,
    };

    #[cfg(any(unix, windows))]
    #[test]
    fn physical_identity_borrow_matches_owner_without_closing_it() {
        let file = tempfile::tempfile().unwrap();
        let expected = file_identity(&file).unwrap();
        #[cfg(unix)]
        let observed = {
            use std::os::fd::AsFd as _;
            let stat = borrowed_file_stat(file.as_fd()).unwrap();
            assert_eq!(identity_from_stat(&stat), expected);
            borrowed_file_identity(file.as_fd()).unwrap()
        };
        #[cfg(windows)]
        let observed = {
            use std::os::windows::io::AsHandle as _;
            borrowed_file_identity(file.as_handle()).unwrap()
        };
        assert_eq!(observed, expected);
        assert!(file.metadata().unwrap().is_file());
        assert_eq!(file_identity(&file).unwrap(), expected);
    }

    #[test]
    fn physical_identity_parts_preserve_full_identifier_width() {
        let identity = PhysicalFileIdentity {
            volume: u64::MAX,
            file: (1_u128 << 127) | 7,
        };
        assert_eq!(identity.parts(), (u64::MAX, (1_u128 << 127) | 7));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn physical_identity_open_preserves_kind_and_creation_boundaries() {
        let root = tempfile::tempdir().unwrap();
        let leaf = root.path().join("identity-file");
        assert!(open_identity_path(&leaf, false, false).is_err());
        assert!(!leaf.exists());
        let missing_directory = root.path().join("missing-directory");
        assert!(open_identity_path(&missing_directory, true, false).is_err());
        assert!(open_identity_path(&missing_directory, true, true).is_err());
        assert!(!missing_directory.exists());
        let missing_parent = root.path().join("missing-parent");
        assert!(open_identity_path(&missing_parent.join("lock"), false, true).is_err());
        assert!(!missing_parent.exists());
        let created = open_identity_path(&leaf, false, true).unwrap();
        assert!(created.metadata().unwrap().is_file());
        drop(created);
        std::fs::write(&leaf, b"retain these bytes").unwrap();
        drop(open_identity_path(&leaf, false, true).unwrap());
        assert_eq!(std::fs::read(&leaf).unwrap(), b"retain these bytes");
        assert!(open_identity_path(&leaf, true, false).is_err());
        assert!(open_identity_path(root.path(), false, false).is_err());
        assert!(open_identity_path(root.path(), true, false).is_ok());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn physical_identity_open_refuses_leaf_links_including_dangling_create() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("file");
        let file_link = root.path().join("file-link");
        let dir_link = root.path().join("dir-link");
        let absent = root.path().join("absent");
        let dangling = root.path().join("dangling");
        std::fs::write(&file, b"unchanged").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&file, &file_link).unwrap();
            std::os::unix::fs::symlink(root.path(), &dir_link).unwrap();
            std::os::unix::fs::symlink(&absent, &dangling).unwrap();
        }
        #[cfg(windows)]
        {
            // This native fixture requires Windows symbolic-link creation rights.
            std::os::windows::fs::symlink_file(&file, &file_link).unwrap();
            std::os::windows::fs::symlink_dir(root.path(), &dir_link).unwrap();
            std::os::windows::fs::symlink_file(&absent, &dangling).unwrap();
        }
        assert!(open_identity_path(&file_link, false, false).is_err());
        assert!(open_identity_path(&file_link, false, true).is_err());
        assert!(open_identity_path(&dir_link, true, false).is_err());
        assert!(open_identity_path(&dangling, false, true).is_err());
        assert!(!absent.exists());
        assert_eq!(std::fs::read(&file).unwrap(), b"unchanged");
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn physical_lock_identity_observes_held_file_after_name_replacement() {
        let root = tempfile::tempdir().unwrap();
        let guard = StoreAuthorityLock::acquire(root.path()).unwrap();
        let named = root.path().join(STORE_AUTHORITY_LOCK_FILE);
        let before = open_identity_path(&named, false, false).unwrap();
        let original = file_identity(&before).unwrap();
        assert_eq!(guard.physical_lock_identity().unwrap(), original);
        let retained = root.path().join("retained-lock");
        std::fs::rename(&named, &retained).unwrap();
        let replacement = open_identity_path(&named, false, true).unwrap();
        assert_ne!(file_identity(&replacement).unwrap(), original);
        assert_eq!(file_identity(&before).unwrap(), original);
        assert_eq!(guard.physical_lock_identity().unwrap(), original);
        assert_eq!(
            file_identity(&open_identity_path(&retained, false, false).unwrap()).unwrap(),
            original
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn physical_lock_identity_query_preserves_lock_and_nested_lifetime() {
        let root = tempfile::tempdir().unwrap();
        let guard = StoreAuthorityLock::acquire(root.path()).unwrap();
        let expected = guard.physical_lock_identity().unwrap();
        let nested = StoreAuthorityLock::try_acquire(root.path())
            .unwrap()
            .unwrap();
        assert!(nested.is_reentrant());
        assert_eq!(nested.physical_lock_identity().unwrap(), expected);
        drop(guard);
        let path = root.path().to_owned();
        assert!(
            std::thread::spawn(move || {
                StoreAuthorityLock::try_acquire(&path).unwrap().is_none()
            })
            .join()
            .unwrap()
        );
        drop(nested);
        let reopened = StoreAuthorityLock::try_acquire(root.path())
            .unwrap()
            .unwrap();
        assert!(!reopened.is_reentrant());
        assert_eq!(reopened.physical_lock_identity().unwrap(), expected);
        drop(reopened);
        assert!(root.path().join(STORE_AUTHORITY_LOCK_FILE).is_file());
    }

    #[test]
    fn identity_helpers_do_not_change_legacy_root_creation_or_errors() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("legacy-root");
        let guard = StoreAuthorityLock::try_acquire(&missing).unwrap().unwrap();
        assert!(missing.is_dir());
        drop(guard);
        assert!(missing.join(STORE_AUTHORITY_LOCK_FILE).is_file());
        let file = root.path().join("not-a-directory");
        std::fs::write(&file, b"keep").unwrap();
        let error = StoreAuthorityLock::try_acquire(&file).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("could not create store authority directory")
        );
        assert_eq!(std::fs::read(&file).unwrap(), b"keep");
    }

    #[cfg(not(any(unix, windows)))]
    #[test]
    fn physical_identity_refuses_unsupported_platform_before_path_creation() {
        let file = tempfile::tempfile().unwrap();
        assert!(file_identity(&file).is_err());
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("lock");
        assert!(open_identity_path(&missing, false, true).is_err());
        assert!(!missing.exists());
    }

    #[test]
    fn authority_lock_serializes_independent_store_writers() {
        let root = tempfile::tempdir().unwrap();
        let first = StoreAuthorityLock::acquire(root.path()).unwrap();
        let path = root.path().to_path_buf();
        let (started_tx, started_rx) = mpsc::channel();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let contender = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let _second = StoreAuthorityLock::acquire(&path).unwrap();
            acquired_tx.send(()).unwrap();
        });
        started_rx.recv().unwrap();
        assert!(
            acquired_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "a second store writer must remain blocked while authority is held"
        );
        drop(first);
        acquired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        contender.join().unwrap();
    }

    #[test]
    fn authority_lock_try_acquire_reports_contention_without_blocking() {
        let root = tempfile::tempdir().unwrap();
        let first = StoreAuthorityLock::acquire(root.path()).unwrap();
        let path = root.path().to_path_buf();
        let contender =
            std::thread::spawn(move || StoreAuthorityLock::try_acquire(&path).unwrap().is_none());

        assert!(contender.join().unwrap());

        drop(first);
        assert!(
            StoreAuthorityLock::try_acquire(root.path())
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn authority_lock_is_reentrant_only_for_the_owning_thread() {
        let root = tempfile::tempdir().unwrap();
        let key = lock_key(root.path()).unwrap().0;
        let first = StoreAuthorityLock::acquire(root.path()).unwrap();
        let nested = StoreAuthorityLock::try_acquire(root.path())
            .unwrap()
            .expect("the owning thread may nest authority");
        let path = root.path().to_path_buf();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let contender = std::thread::spawn(move || {
            let _guard = StoreAuthorityLock::acquire(&path).unwrap();
            acquired_tx.send(()).unwrap();
        });

        assert!(
            acquired_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "another thread must remain excluded"
        );
        drop(nested);
        assert!(
            acquired_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "the outer guard must retain the operating-system lock"
        );
        drop(first);
        acquired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        contender.join().unwrap();
        assert!(
            !HELD_AUTHORITY_LOCKS
                .get()
                .unwrap()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_key(&key),
            "the final owning-thread drop removes its stale registry key"
        );
    }

    #[test]
    fn authority_lock_facts_follow_physical_release_and_reentrant_lifetime() {
        let root = tempfile::tempdir().unwrap();
        let counting = LongitudinalCountingScopeV1::new("7".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let _scope = counting.enter();
        let _actor = counting.enter_actor_scope(InteractionActorV1::ExplicitRecovery);

        let outer = StoreAuthorityLock::acquire(root.path()).unwrap();
        let nested = StoreAuthorityLock::try_acquire(root.path())
            .unwrap()
            .expect("same-thread authority reentry");
        assert!(!outer.is_reentrant());
        assert!(nested.is_reentrant());

        drop(outer);
        let before_final_release = counting.snapshot();
        assert_eq!(before_final_release.lock_facts.len(), 1);
        assert_eq!(
            before_final_release.lock_facts[0].acquisition,
            InteractionLockAcquisitionV1::Reentrant
        );
        assert_eq!(before_final_release.lock_facts[0].wait_nanos, 0);
        assert_eq!(before_final_release.lock_facts[0].hold_nanos, None);

        drop(nested);
        let after_final_release = counting.snapshot();
        assert_eq!(after_final_release.lock_facts.len(), 2);
        assert_eq!(after_final_release.lock_facts[0].ordinal, 0);
        assert_eq!(
            after_final_release.lock_facts[0].actor,
            InteractionActorV1::ExplicitRecovery
        );
        assert_eq!(
            after_final_release.lock_facts[0].kind,
            InteractionLockKindV1::Authority
        );
        assert_eq!(
            after_final_release.lock_facts[0].mode,
            InteractionLockModeV1::Blocking
        );
        assert_eq!(
            after_final_release.lock_facts[0].outcome,
            InteractionLockOutcomeV1::Acquired
        );
        assert_eq!(
            after_final_release.lock_facts[0].acquisition,
            InteractionLockAcquisitionV1::Physical
        );
        assert!(after_final_release.lock_facts[0].hold_nanos.is_some());
        assert_eq!(after_final_release.lock_facts[1].ordinal, 1);
        assert_eq!(
            after_final_release.lock_facts[1].acquisition,
            InteractionLockAcquisitionV1::Reentrant
        );
    }

    #[test]
    fn authority_contender_preserves_busy_and_physical_wait_facts() {
        let root = tempfile::tempdir().unwrap();
        let counting = LongitudinalCountingScopeV1::new("c".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let _scope = counting.enter();
        let _actor = counting.enter_actor_scope(InteractionActorV1::ExplicitRecovery);
        let held = StoreAuthorityLock::acquire(root.path()).unwrap();

        let contender_scope = counting.clone();
        let contender_root = root.path().to_path_buf();
        let (busy_tx, busy_rx) = mpsc::channel();
        let contender = std::thread::spawn(move || {
            let _scope = contender_scope.enter();
            let _actor = contender_scope.enter_actor_scope(InteractionActorV1::ProductWriter);
            assert!(
                StoreAuthorityLock::try_acquire(&contender_root)
                    .unwrap()
                    .is_none()
            );
            busy_tx.send(()).unwrap();
            let acquired = StoreAuthorityLock::acquire(&contender_root).unwrap();
            drop(acquired);
        });

        busy_rx.recv().unwrap();
        drop(held);
        contender.join().unwrap();

        let locks = counting.snapshot().lock_facts;
        assert_eq!(locks.len(), 3);
        assert_eq!(locks[0].acquisition, InteractionLockAcquisitionV1::Physical);
        assert_eq!(locks[0].actor, InteractionActorV1::ExplicitRecovery);
        assert_eq!(locks[1].outcome, InteractionLockOutcomeV1::Busy);
        assert_eq!(
            locks[1].acquisition,
            InteractionLockAcquisitionV1::NotAcquired
        );
        assert_eq!(locks[1].actor, InteractionActorV1::ProductWriter);
        assert_eq!(locks[2].outcome, InteractionLockOutcomeV1::Acquired);
        assert_eq!(locks[2].acquisition, InteractionLockAcquisitionV1::Physical);
        assert_eq!(locks[2].actor, InteractionActorV1::ProductWriter);
        assert!(locks[2].wait_nanos > 0);
        assert!(locks[2].hold_nanos.is_some());
    }
}
