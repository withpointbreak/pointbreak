#![cfg_attr(not(test), allow(dead_code))]

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use super::DERIVED_WRITER_LOCK_FILE;

#[derive(Debug, thiserror::Error)]
pub(crate) enum WriterLockError {
    #[error("derived-access writer is busy")]
    Busy,
    #[error("writer-lock I/O failed at {path}: {message}")]
    Io { path: PathBuf, message: String },
}

#[derive(Debug)]
pub(crate) struct StoreWriterLock {
    file: File,
}

impl StoreWriterLock {
    pub(crate) fn acquire(store_root: &Path) -> Result<Self, WriterLockError> {
        let (file, path) = open_lock_file(store_root)?;
        file.lock().map_err(|error| io_error(&path, error))?;
        Ok(Self { file })
    }

    pub(crate) fn try_acquire(store_root: &Path) -> Result<Self, WriterLockError> {
        let (file, path) = open_lock_file(store_root)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { file }),
            Err(std::fs::TryLockError::WouldBlock) => Err(WriterLockError::Busy),
            Err(std::fs::TryLockError::Error(error)) => Err(io_error(&path, error)),
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
    let path = canonical_root.join(DERIVED_WRITER_LOCK_FILE);
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
