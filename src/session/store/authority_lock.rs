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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::bench_support::longitudinal::{
        InteractionActorV1, InteractionLockAcquisitionV1, InteractionLockKindV1,
        InteractionLockModeV1, InteractionLockOutcomeV1, LongitudinalCountingScopeV1,
    };

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
