mod adapter;
mod authority_stamp;
mod change_read;
mod contract;
mod diagnostic;
mod evidence;
mod lifecycle;
mod materializer;
mod product_contract;
mod storage_witness;

pub(crate) mod sqlite_cursor {
    pub(crate) use crate::session::derived_access::sqlite::*;
}

#[cfg(test)]
pub(crate) mod sqlite_locator {
    pub(crate) use crate::session::derived_access::sqlite::*;
}

pub use authority_stamp::*;
pub use change_read::*;
pub use contract::*;
pub use diagnostic::*;
pub use evidence::*;
pub use lifecycle::*;
pub use materializer::*;
pub use product_contract::*;
pub use storage_witness::*;

pub(crate) use crate::session::derived_access::layout::DerivedStorageLayout;
#[cfg(test)]
pub(crate) use crate::session::derived_access::layout::DerivedStorageNamespace;

#[cfg(test)]
mod runner_tests;
#[cfg(test)]
mod sqlite_cursor_tests;
#[cfg(test)]
mod sqlite_locator_tests;
#[cfg(test)]
mod sqlite_semantic_tests;
