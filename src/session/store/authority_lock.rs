use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread::ThreadId;

use crate::error::{Result, ShoreError};

pub(crate) const STORE_AUTHORITY_LOCK_FILE: &str = "authority.writer.lock";

/// Process-wide and cross-process exclusion for mutations of one authoritative
/// store. Keeping the handle alive holds the operating-system lock; dropping it
/// releases the lock without deleting the stable lock file.
#[derive(Debug)]
struct StoreAuthorityLockState {
    _file: File,
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
        if let Some(state) = held_state(&key) {
            return Ok(Self::new(key, state, true));
        }
        let file = open_lock_file(&path)?;
        file.lock()
            .map_err(|error| lock_error(&path, "acquire", error))?;
        Ok(register_lock(key, file))
    }

    pub(crate) fn try_acquire(store_root: &Path) -> Result<Option<Self>> {
        let (key, path) = lock_key(store_root)?;
        if let Some(state) = held_state(&key) {
            return Ok(Some(Self::new(key, state, true)));
        }
        let file = open_lock_file(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(register_lock(key, file))),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => Err(lock_error(&path, "acquire", error)),
        }
    }

    pub(crate) fn is_reentrant(&self) -> bool {
        self.reentrant
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

fn register_lock(key: (ThreadId, PathBuf), file: File) -> StoreAuthorityLock {
    let state = Arc::new(StoreAuthorityLockState { _file: file });
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

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
}
