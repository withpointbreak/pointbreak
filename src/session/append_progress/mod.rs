//! Private owned enrollment foundations; public repository routing remains disabled.

mod audit;
mod live;
#[allow(dead_code, reason = "enrolled routing is not yet admitted")]
mod native;
mod owner;

#[allow(dead_code, reason = "enrolled routing is not yet admitted")]
mod ledger;
pub(crate) use ledger::c4::PublicationPermit;
pub(crate) use ledger::c4::audit::JournalAdmission;

#[allow(dead_code, reason = "private enrollment is not a product route")]
mod enrollment;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_fixture;
#[doc(hidden)]
pub use audit::AuditLimits;
#[doc(hidden)]
pub use ledger::c4::AppendExit;
#[doc(hidden)]
pub use ledger::c4::audit::AuditExit;
#[doc(hidden)]
pub use owner::{OwnedAppendBootstrap, OwnedAppendOperation, run_disposable_append};
#[doc(hidden)]
pub use owner::{OwnedAuditBootstrap, run_disposable_audit};
pub use owner::{
    OwnedProcessBootstrap, OwnedProcessExit, OwnedProcessFailure, run_disposable_owner,
};
#[cfg(all(test, windows))]
pub(super) use test_fixture::fixture_acl;
#[cfg(test)]
pub(crate) use test_fixture::test_root;

#[cfg(target_os = "macos")]
pub(in crate::session) mod consumer;
