#![cfg_attr(not(test), allow(dead_code))]

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::{
    InteractionLockKindV1, InteractionLockModeV1, InteractionLockOutcomeV1,
    InteractionPhysicalLockHoldRecorderV1, begin_interaction_lock_attempt_v1,
};
use crate::session::derived_access::layout::DerivedStorageLayout;

#[derive(Debug, thiserror::Error)]
pub(crate) enum WriterLockError {
    #[error("derived-access writer is busy")]
    Busy,
    #[error("writer-lock I/O failed at {path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error("writer-lock layout resolution failed: {0}")]
    Layout(String),
}

#[derive(Debug)]
pub(crate) struct StoreWriterLock {
    file: File,
    #[cfg(any(test, feature = "longitudinal-counting"))]
    _hold_recorder: Option<InteractionPhysicalLockHoldRecorderV1>,
}

impl StoreWriterLock {
    pub(crate) fn acquire(store_root: &Path) -> Result<Self, WriterLockError> {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let attempt = begin_interaction_lock_attempt_v1(
            InteractionLockKindV1::Derived,
            InteractionLockModeV1::Blocking,
        );
        let (file, path) = match open_lock_file(store_root) {
            Ok(opened) => opened,
            Err(error) => {
                #[cfg(any(test, feature = "longitudinal-counting"))]
                attempt.record_not_acquired(InteractionLockOutcomeV1::Failed);
                return Err(error);
            }
        };
        match file.lock() {
            Ok(()) => Ok(Self {
                file,
                #[cfg(any(test, feature = "longitudinal-counting"))]
                _hold_recorder: attempt.record_physical_acquired(),
            }),
            Err(error) => {
                #[cfg(any(test, feature = "longitudinal-counting"))]
                attempt.record_not_acquired(InteractionLockOutcomeV1::Failed);
                Err(io_error(&path, error))
            }
        }
    }

    pub(crate) fn try_acquire(store_root: &Path) -> Result<Self, WriterLockError> {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let attempt = begin_interaction_lock_attempt_v1(
            InteractionLockKindV1::Derived,
            InteractionLockModeV1::Try,
        );
        let (file, path) = match open_lock_file(store_root) {
            Ok(opened) => opened,
            Err(error) => {
                #[cfg(any(test, feature = "longitudinal-counting"))]
                attempt.record_not_acquired(InteractionLockOutcomeV1::Failed);
                return Err(error);
            }
        };
        match file.try_lock() {
            Ok(()) => Ok(Self {
                file,
                #[cfg(any(test, feature = "longitudinal-counting"))]
                _hold_recorder: attempt.record_physical_acquired(),
            }),
            Err(std::fs::TryLockError::WouldBlock) => {
                #[cfg(any(test, feature = "longitudinal-counting"))]
                attempt.record_not_acquired(InteractionLockOutcomeV1::Deferred);
                Err(WriterLockError::Busy)
            }
            Err(std::fs::TryLockError::Error(error)) => {
                #[cfg(any(test, feature = "longitudinal-counting"))]
                attempt.record_not_acquired(InteractionLockOutcomeV1::Failed);
                Err(io_error(&path, error))
            }
        }
    }
}

impl Drop for StoreWriterLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn open_lock_file(store_root: &Path) -> Result<(File, PathBuf), WriterLockError> {
    std::fs::create_dir_all(store_root).map_err(|error| io_error(store_root, error))?;
    let canonical_root = store_root
        .canonicalize()
        .map_err(|error| io_error(store_root, error))?;
    let path = DerivedStorageLayout::resolve(&canonical_root)
        .map_err(|error| WriterLockError::Layout(error.to_string()))?
        .writer_lock();
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| io_error(&path, error))?;
    Ok((file, path))
}

fn io_error(path: &Path, error: std::io::Error) -> WriterLockError {
    WriterLockError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use crate::bench_support::longitudinal::{
        InteractionActorV1, InteractionLockAcquisitionV1, InteractionLockKindV1,
        InteractionLockOutcomeV1, LongitudinalCountingScopeV1,
    };

    #[test]
    fn derived_lock_facts_distinguish_physical_hold_from_deferred_try() {
        let root = tempfile::tempdir().unwrap();
        let counting = LongitudinalCountingScopeV1::new("8".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let _scope = counting.enter();
        let _actor = counting.enter_actor_scope(InteractionActorV1::BackgroundMaintenance);

        let held = StoreWriterLock::acquire(root.path()).unwrap();
        assert!(matches!(
            StoreWriterLock::try_acquire(root.path()),
            Err(WriterLockError::Busy)
        ));
        drop(held);

        let locks = counting.snapshot().lock_facts;
        assert_eq!(locks.len(), 2);
        assert_eq!(locks[0].kind, InteractionLockKindV1::Derived);
        assert_eq!(locks[0].outcome, InteractionLockOutcomeV1::Acquired);
        assert_eq!(locks[0].acquisition, InteractionLockAcquisitionV1::Physical);
        assert!(locks[0].hold_nanos.is_some());
        assert_eq!(locks[1].kind, InteractionLockKindV1::Derived);
        assert_eq!(locks[1].outcome, InteractionLockOutcomeV1::Deferred);
        assert_eq!(
            locks[1].acquisition,
            InteractionLockAcquisitionV1::NotAcquired
        );
        assert_eq!(locks[1].hold_nanos, None);
    }
}
