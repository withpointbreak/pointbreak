//! Private owned enrollment foundations; public repository routing remains disabled.

mod audit;
#[cfg(target_os = "macos")]
mod data;
#[cfg(target_os = "macos")]
#[doc(hidden)]
pub use data::{OwnedDataOperation, OwnedDataRequest, run_owned_data_request};
mod live;
mod read;
#[cfg(target_os = "macos")]
pub(in crate::session) use ledger::{with_published_projection, with_staging_projection};
pub(crate) use read::ProgressReadFence;
#[cfg(target_os = "macos")]
pub(in crate::session) use read::ReadContext;
#[allow(dead_code, reason = "enrolled routing is not yet admitted")]
mod native;
mod owner;

#[allow(dead_code, reason = "enrolled routing is not yet admitted")]
mod ledger;
pub(crate) use ledger::c4::PublicationPermit;
pub(crate) use ledger::c4::audit::JournalAdmission;
#[cfg(target_os = "macos")]
pub(in crate::session) use ledger::c4::audit::{RebuildEvent, completed_rebuild_inputs};

/// Invocation-local work accounting; never transfers native ownership.
#[cfg(target_os = "macos")]
pub(in crate::session) struct DataWorkScope(#[allow(dead_code)] audit::Scope);
#[cfg(target_os = "macos")]
pub(in crate::session) fn enter_data_work(
    limits: AuditLimits,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> crate::error::Result<DataWorkScope> {
    Ok(DataWorkScope(audit::Scope::enter_at(
        limits,
        cancel,
        std::time::Instant::now(),
    )?))
}
pub(in crate::session) fn data_work_checkpoint() -> crate::error::Result<()> {
    audit::check()
}

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
pub(in crate::session) use owner::data_selected;
#[doc(hidden)]
pub use owner::{OwnedAppendBootstrap, OwnedAppendOperation, run_disposable_append};
#[doc(hidden)]
pub use owner::{OwnedAuditBootstrap, run_disposable_audit};
#[doc(hidden)]
pub use owner::{OwnedDataCommand, enter_owned_data_command};
pub use owner::{
    OwnedProcessBootstrap, OwnedProcessExit, OwnedProcessFailure, run_disposable_owner,
};
#[cfg(all(test, windows))]
pub(super) use test_fixture::fixture_acl;
#[cfg(test)]
pub(crate) use test_fixture::test_root;

#[cfg(target_os = "macos")]
pub(in crate::session) mod consumer;
