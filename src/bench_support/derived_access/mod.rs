mod adapter;
mod contract;
mod evidence;
mod lifecycle;
mod materializer;
mod product_contract;

pub(crate) mod sqlite_cursor {
    pub(crate) use crate::session::derived_access::sqlite::*;
}

#[cfg(test)]
pub(crate) mod sqlite_locator {
    pub(crate) use crate::session::derived_access::sqlite::*;
}

pub use contract::*;
pub use evidence::*;
pub use lifecycle::*;
pub use materializer::*;
pub use product_contract::*;

pub(crate) use crate::session::derived_access::sqlite::{
    DERIVED_QUARANTINE_PREFIX, DERIVED_SIDECAR_DIRECTORY, DERIVED_WRITER_LOCK_FILE,
};

#[cfg(test)]
mod runner_tests;
#[cfg(test)]
mod sqlite_cursor_tests;
#[cfg(test)]
mod sqlite_locator_tests;
#[cfg(test)]
mod sqlite_semantic_tests;
