//! Private owned enrollment foundations; public repository routing remains disabled.

mod audit;
#[cfg(target_os = "macos")]
mod data;
#[cfg(target_os = "macos")]
#[doc(hidden)]
pub use data::{
    OwnedDataOperation, OwnedDataRequest, run_owned_data_request, with_owned_inspector_request,
};
mod live;
mod read;
#[cfg(target_os = "macos")]
pub(in crate::session) use ledger::{
    ProjectionRequestWitnesses, with_projection_phase, with_published_projection,
    with_staging_projection,
};
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
#[cfg(target_os = "macos")]
pub(in crate::session) fn data_work_charge(
    visits: usize,
    bytes: usize,
) -> crate::error::Result<()> {
    for _ in 0..visits {
        audit::visit()?;
    }
    audit::bytes(bytes as u64)
}
#[cfg(target_os = "macos")]
pub(in crate::session) fn require_quiescent_data() -> crate::error::Result<()> {
    owner::require_quiescent()?;
    if !owner::data_selected() {
        return Err(crate::error::ShoreError::Message(
            "owned_process_required".into(),
        ));
    }
    audit::check()
}

#[allow(dead_code, reason = "private enrollment is not a product route")]
mod enrollment;

#[cfg(test)]
mod tests;

#[cfg(all(test, target_os = "macos"))]
mod inspector_fixture;
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

/// Meter resource bytes only during an admitted owned data invocation. Ordinary
/// callers retain their existing LocalContentStore implementation and behavior.
#[cfg(target_os = "macos")]
pub(in crate::session) fn owned_content_read(
    root: &std::path::Path,
    key: &str,
) -> Option<crate::error::Result<Option<Vec<u8>>>> {
    if !owner::data_selected() || !audit::active() {
        return None;
    }
    Some((|| {
        require_quiescent_data()?;
        let relative = std::path::Path::new(key);
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(crate::error::ShoreError::Message(
                "invalid owned content key".into(),
            ));
        }
        let path = root.join(relative);
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(crate::error::ShoreError::Message(error.to_string())),
            Ok(_) => {}
        }
        let file = crate::session::store::authority_lock::open_identity_path(&path, false, false)?;
        let length = file
            .metadata()
            .map_err(|e| crate::error::ShoreError::Message(e.to_string()))?
            .len();
        if length > 32 * 1024 * 1024 {
            return Err(crate::error::ShoreError::Message(
                "owned resource byte limit".into(),
            ));
        }
        Ok(Some(audit::read(&file, length)?))
    })())
}

/// Logical SQL output is charged before allocation/decoding. Other profiles
/// have no owned data budget and preserve their existing behavior.
#[cfg(target_os = "macos")]
pub(in crate::session) fn data_row_charge(row: &rusqlite::Row<'_>) -> rusqlite::Result<()> {
    if owner::data_selected() {
        audit::row(row)
    } else {
        Ok(())
    }
}
#[cfg(target_os = "macos")]
pub(in crate::session) fn data_file_bytes(
    path: &std::path::Path,
    maximum: u64,
) -> crate::error::Result<Vec<u8>> {
    require_quiescent_data()?;
    audit::visit()?;
    let file = crate::session::store::authority_lock::open_identity_path(path, false, false)?;
    let length = file
        .metadata()
        .map_err(|e| crate::error::ShoreError::Message(e.to_string()))?
        .len();
    if length > maximum {
        return Err(crate::error::ShoreError::Message(
            "owned input byte limit".into(),
        ));
    }
    audit::read(&file, length)
}
