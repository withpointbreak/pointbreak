use std::fs::{File, OpenOptions};
use std::path::Path;

use crate::error::{Result, ShoreError};

pub(crate) const STORE_AUTHORITY_LOCK_FILE: &str = "authority.writer.lock";

/// Process-wide and cross-process exclusion for mutations of one authoritative
/// store. Keeping the handle alive holds the operating-system lock; dropping it
/// releases the lock without deleting the stable lock file.
#[derive(Debug)]
pub(crate) struct StoreAuthorityLock {
    _file: File,
}

impl StoreAuthorityLock {
    pub(crate) fn acquire(store_root: &Path) -> Result<Self> {
        std::fs::create_dir_all(store_root).map_err(|error| {
            ShoreError::Message(format!(
                "could not create store authority directory {}: {error}",
                store_root.display()
            ))
        })?;
        let path = store_root.join(STORE_AUTHORITY_LOCK_FILE);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| lock_error(&path, "open", error))?;
        file.lock()
            .map_err(|error| lock_error(&path, "acquire", error))?;
        Ok(Self { _file: file })
    }
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
}
