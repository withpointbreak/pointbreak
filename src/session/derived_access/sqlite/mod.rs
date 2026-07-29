//! One SQLite-WAL bodyless implementation shared by product and qualification callers.

mod cursor;
mod locator;
mod semantic;
mod writer_lock;

pub(crate) const DERIVED_QUARANTINE_PREFIX: &str = ".pointbreak-derived.quarantine-";
pub(crate) const DERIVED_SIDECAR_DIRECTORY: &str = ".pointbreak-derived";
pub(crate) const DERIVED_WRITER_LOCK_FILE: &str = ".pointbreak-derived.writer.lock";

pub(crate) use cursor::*;
pub(crate) use locator::*;
pub(crate) use semantic::*;
pub(crate) use writer_lock::*;
