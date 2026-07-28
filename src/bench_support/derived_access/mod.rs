mod adapter;
mod contract;
mod evidence;
mod lifecycle;
mod materializer;
mod sqlite_cursor;
mod sqlite_locator;
mod sqlite_semantic;
mod writer_lock;

pub(crate) const DERIVED_QUARANTINE_PREFIX: &str = ".pointbreak-derived.quarantine-";
pub(crate) const DERIVED_SIDECAR_DIRECTORY: &str = ".pointbreak-derived";
pub(crate) const DERIVED_WRITER_LOCK_FILE: &str = ".pointbreak-derived.writer.lock";

pub use contract::*;
pub use evidence::*;
pub use lifecycle::*;
pub use materializer::*;

#[cfg(test)]
mod runner_tests;
#[cfg(test)]
mod sqlite_cursor_tests;
#[cfg(test)]
mod sqlite_locator_tests;
#[cfg(test)]
mod sqlite_semantic_tests;
