//! The binary, never a safe library caller, owns the disposable process contract.
use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Result, ShoreError};

#[derive(Clone, Copy, Debug)]
pub enum OwnedProcessFailure {
    NativeOwnershipUnknown,
}
static ACTIVE: AtomicBool = AtomicBool::new(false);
thread_local! {
    static TERMINATE: Cell<Option<fn(OwnedProcessFailure) -> !>> = const { Cell::new(None) };
    static SQL_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

pub(super) struct OwnedProcessPermit(PhantomData<Rc<()>>);
impl OwnedProcessPermit {
    /// The enclosing binary entry owns the entire process until this is dropped.
    pub(super) unsafe fn enter(terminate: fn(OwnedProcessFailure) -> !) -> Result<Self> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                ShoreError::Message("owned_process_required; owner already active".into())
            })?;
        TERMINATE.with(|value| value.set(Some(terminate)));
        Ok(Self(PhantomData))
    }
}
impl Drop for OwnedProcessPermit {
    fn drop(&mut self) {
        if SQL_ACTIVE.with(Cell::get) {
            terminal();
        }
        TERMINATE.with(|value| value.set(None));
        ACTIVE.store(false, Ordering::Release);
    }
}
pub(super) fn require_owned() -> Result<()> {
    if TERMINATE.with(Cell::get).is_none() {
        return Err(ShoreError::Message(
            "owned_process_required; configured binary owner required before native acquisition"
                .into(),
        ));
    }
    Ok(())
}
pub(super) fn require_quiescent() -> Result<()> {
    require_owned()?;
    if SQL_ACTIVE.with(Cell::get) {
        return Err(ShoreError::Message(
            "overlapping native owner refused before evidence acquisition".into(),
        ));
    }
    Ok(())
}
pub(super) struct SqlPermit(PhantomData<Rc<()>>);
impl SqlPermit {
    pub(super) fn enter() -> Result<Self> {
        require_quiescent()?;
        SQL_ACTIVE.with(|value| value.set(true));
        Ok(Self(PhantomData))
    }
}
impl Drop for SqlPermit {
    fn drop(&mut self) {
        SQL_ACTIVE.with(|value| value.set(false));
    }
}
pub(super) fn terminal() -> ! {
    if let Some(terminate) = TERMINATE.with(Cell::get) {
        terminate(OwnedProcessFailure::NativeOwnershipUnknown)
    }
    // No safe API can reach uncertain native ownership without a live permit.
    // Abort is an invariant violation, not a recoverable library exit policy.
    std::process::abort()
}

thread_local! {
    static DATA_COMMAND: Cell<bool> = const { Cell::new(false) };
}

#[doc(hidden)]
pub struct OwnedDataCommand {
    _permit: OwnedProcessPermit,
}
impl Drop for OwnedDataCommand {
    fn drop(&mut self) {
        DATA_COMMAND.with(|active| active.set(false));
    }
}

/// Enter the binary-owned data executor before creating services or worker state.
///
/// # Safety
/// The caller owns the entire disposable process and must run data operations
/// serially on this thread until the returned non-Send owner is dropped. No
/// embedding host or sibling data operation may require continued execution.
/// The terminal callback exits immediately without unwinding; arguments,
/// environment, a PID and a timeout do not establish this contract.
#[doc(hidden)]
pub unsafe fn enter_owned_data_command(
    terminate: fn(OwnedProcessFailure) -> !,
) -> Result<OwnedDataCommand> {
    // SAFETY: inherited from this function's binary-only lifetime contract.
    let permit = unsafe { OwnedProcessPermit::enter(terminate) }?;
    DATA_COMMAND.with(|active| active.set(true));
    Ok(OwnedDataCommand { _permit: permit })
}

#[cfg(target_os = "macos")]
pub(in crate::session) fn data_selected() -> bool {
    DATA_COMMAND.with(Cell::get)
}

/// Private raw-root foundation input. Constructing it grants no native authority.
#[doc(hidden)]
pub struct OwnedProcessBootstrap {
    pub root: std::path::PathBuf,
    pub request_id: String,
    pub writer: crate::session::event::Writer,
    pub occurred_at: String,
    pub signer: Box<dyn crate::crypto::EventSigner + Send + Sync>,
    pub selected_only: bool,
}
#[doc(hidden)]
#[derive(Debug, serde::Serialize)]
pub struct OwnedProcessExit {
    pub request_id: String,
    pub selected: bool,
}

/// Execute one complete private enrollment or selected-H0 validation.
///
/// # Safety
/// The caller owns the entire disposable process and operation lifetime. No
/// embedding caller, sibling data operation, or prior native owner may require
/// continued execution. The callback must immediately terminate the process,
/// without unwinding, if ownership/retirement becomes uncertain. Environment,
/// PID, nonce, executable arguments and a timeout do not prove these conditions.
/// Only the audited binary whole-command/private bootstrap branches may call
/// this entry in production. Public repository routing remains disabled until
/// the real clone guard and later process/Inspector integration are qualified.
#[doc(hidden)]
pub unsafe fn run_disposable_owner(
    bootstrap: OwnedProcessBootstrap,
    terminate: fn(OwnedProcessFailure) -> !,
) -> Result<OwnedProcessExit> {
    use super::{enrollment, native};
    use crate::session::store::authority_lock::{
        StoreAuthorityLock, file_identity, open_identity_path,
    };
    enrollment::validate_request(
        &bootstrap.request_id,
        true,
        "review_change_revision_progress_v1",
    )?;
    if !bootstrap.root.is_absolute() {
        return Err(ShoreError::Message(
            "private owner requires an absolute canonical root".into(),
        ));
    }
    // SAFETY: inherited from this function's explicit process-wide contract.
    let _permit = unsafe { OwnedProcessPermit::enter(terminate) }?;
    let root = native::physical_root(&bootstrap.root)?;
    let canonical = native::root_snapshot(&root)?.canonical;
    // Both the root/ancestor set and the named authority object precede locking.
    let named_authority =
        open_identity_path(&canonical.join("authority.writer.lock"), false, true)?;
    let witness = file_identity(&named_authority)?;
    root.validate_live()?;
    let guard = StoreAuthorityLock::acquire_until(
        &canonical,
        std::time::Instant::now() + std::time::Duration::from_secs(2),
        None,
    )?;
    if guard.physical_lock_identity()? != witness {
        return Err(ShoreError::Message(
            "authority changed across acquisition".into(),
        ));
    }
    root.validate_live()?;
    if bootstrap.selected_only {
        enrollment::selected(
            &root,
            &guard,
            &bootstrap.request_id,
            bootstrap.writer.actor_id.as_str(),
        )?;
    } else {
        enrollment::enroll(
            &root,
            &guard,
            &bootstrap.request_id,
            true,
            "review_change_revision_progress_v1",
            bootstrap.signer.as_ref(),
            bootstrap.writer,
            &bootstrap.occurred_at,
        )?;
    }
    root.validate_live()?;
    // SAFETY: the whole-command owner and root/authority descriptors are live.
    let named = unsafe {
        native::namespace::observe_private_worker(
            native::retained_root_file(&root)?,
            std::ffi::OsStr::new("authority.writer.lock"),
            native::namespace::Role::Authority,
        )
    }?;
    if !named.is_some_and(|value| value.identity == witness)
        || file_identity(&named_authority)? != guard.physical_lock_identity()?
    {
        return Err(ShoreError::Message(
            "authority identity changed before result".into(),
        ));
    }
    Ok(OwnedProcessExit {
        request_id: bootstrap.request_id,
        selected: true,
    })
}

// Internal controls run one test per disposable process under the finite native
// inventory. This unsafe test entry is not used as production admission proof.
#[cfg(test)]
pub(super) unsafe fn isolated_test_owner() -> OwnedProcessPermit {
    fn terminate(_: OwnedProcessFailure) -> ! {
        // SAFETY: the test runner owns this isolated diagnostic process.
        unsafe { libc::_exit(90) }
    }
    // SAFETY: forwarded explicit contract of this test-only entry.
    unsafe { OwnedProcessPermit::enter(terminate) }.expect("one owner per test process")
}

#[cfg(test)]
mod tests {
    #[test]
    fn shared_embedding_refuses_before_native_acquisition() {
        let absent = std::env::temp_dir().join("pointbreak-uncreated-shared-owner-20260926");
        assert!(!absent.exists());
        assert!(
            super::require_owned()
                .unwrap_err()
                .to_string()
                .contains("owned_process_required")
        );
        assert!(
            super::super::native::physical_root(&absent)
                .unwrap_err()
                .to_string()
                .contains("owned_process_required")
        );
        assert!(!absent.exists());
    }
}

/// The only non-test caller is the binary's private whole-command append branch.
#[doc(hidden)]
pub struct OwnedAppendBootstrap {
    pub root: std::path::PathBuf,
    pub request_id: String,
    pub enrollment_actor: String,
    pub operation: OwnedAppendOperation,
}
#[doc(hidden)]
#[derive(serde::Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OwnedAppendOperation {
    Append {
        attempt_id: String,
        event: Box<crate::session::event::ShoreEvent>,
    },
    Reconcile,
}

/// Execute a single private C4 operation in a binary-owned disposable process.
///
/// # Safety
/// Same whole-process lifetime and immediate non-unwinding terminal callback
/// contract as `run_disposable_owner`. No ordinary embedding caller may invoke
/// this in its host. No public repository resolution/routing is admitted here.
#[doc(hidden)]
pub unsafe fn run_disposable_append(
    bootstrap: OwnedAppendBootstrap,
    terminate: fn(OwnedProcessFailure) -> !,
) -> Result<super::ledger::c4::AppendExit> {
    use super::ledger::c4;
    use super::native;
    use crate::session::store::authority_lock::{
        StoreAuthorityLock, file_identity, open_identity_path,
    };
    super::enrollment::validate_request(
        &bootstrap.request_id,
        true,
        "review_change_revision_progress_v1",
    )?;
    if !bootstrap.root.is_absolute() || bootstrap.enrollment_actor.is_empty() {
        return Err(ShoreError::Message(
            "invalid private append root/actor".into(),
        ));
    }
    let prepared: Option<(String, crate::session::store::PreparedProgressEvent)> =
        match bootstrap.operation {
            OwnedAppendOperation::Append { attempt_id, event } => {
                if attempt_id.len() != 64
                    || !attempt_id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(ShoreError::Message("invalid private append attempt".into()));
                }
                Some((attempt_id, c4::prepare(*event)?))
            }
            OwnedAppendOperation::Reconcile => None,
        };
    // SAFETY: inherited from the explicit audited whole-command caller.
    let _permit = unsafe { OwnedProcessPermit::enter(terminate) }?;
    let root = native::physical_root(&bootstrap.root)?;
    let canonical = native::root_snapshot(&root)?.canonical;
    let named_authority =
        open_identity_path(&canonical.join("authority.writer.lock"), false, true)?;
    let witness = file_identity(&named_authority)?;
    root.validate_live()?;
    let guard = StoreAuthorityLock::acquire_until(
        &canonical,
        std::time::Instant::now() + std::time::Duration::from_secs(2),
        None,
    )?;
    if guard.physical_lock_identity()? != witness {
        return Err(ShoreError::Message(
            "authority changed across acquisition".into(),
        ));
    }
    root.validate_live()?;
    let result = match prepared {
        Some((attempt, event)) => c4::append(
            &root,
            &guard,
            &bootstrap.request_id,
            &bootstrap.enrollment_actor,
            &attempt,
            event,
        ),
        None => c4::reconcile(
            &root,
            &guard,
            &bootstrap.request_id,
            &bootstrap.enrollment_actor,
        ),
    };
    let postflight = (|| -> Result<()> {
        root.validate_live()?;
        // SAFETY: root/native owner and held authority descriptors remain live.
        let named = unsafe {
            native::namespace::observe_private_worker(
                native::retained_root_file(&root)?,
                std::ffi::OsStr::new("authority.writer.lock"),
                native::namespace::Role::Authority,
            )
        }?;
        if !named.is_some_and(|value| value.identity == witness)
            || file_identity(&named_authority)? != guard.physical_lock_identity()?
        {
            return Err(ShoreError::Message(
                "authority identity changed before append result".into(),
            ));
        }
        Ok(())
    })();
    // Failed physical/authority attribution is an unknown operation result,
    // not a confirmed publication with only metadata finalization outstanding.
    postflight?;
    result
}

/// Inputs to the private whole-command audit, never a library-host permission.
#[doc(hidden)]
pub struct OwnedAuditBootstrap {
    pub root: std::path::PathBuf,
    pub request_id: String,
    pub enrollment_actor: String,
    pub audit_request: String,
    pub limits: super::audit::AuditLimits,
    pub cancellation: std::sync::Arc<AtomicBool>,
    /// Monotonic invocation start, captured before bootstrap input is read.
    pub started_at: std::time::Instant,
    /// Actual bounded bootstrap bytes already consumed by the binary.
    pub bootstrap_bytes: u64,
}
/// Execute one explicit census and settlement under the existing Reconcile authority.
///
/// # Safety
/// Same disposable whole-process contract as `run_disposable_owner`. The caller
/// owns every operation in this process and must immediately terminate without
/// unwinding if native ownership is uncertain. No shared host may call this.
#[doc(hidden)]
pub unsafe fn run_disposable_audit(
    bootstrap: OwnedAuditBootstrap,
    terminate: fn(OwnedProcessFailure) -> !,
) -> Result<super::ledger::c4::audit::AuditExit> {
    use super::ledger::c4;
    use super::{audit, native};
    use crate::session::store::authority_lock::{
        StoreAuthorityLock, file_identity, open_identity_path,
    };
    let scope = audit::Scope::enter_at(
        bootstrap.limits,
        bootstrap.cancellation.clone(),
        bootstrap.started_at,
    )?;
    audit::bytes(bootstrap.bootstrap_bytes)?;
    super::enrollment::validate_request(
        &bootstrap.request_id,
        true,
        "review_change_revision_progress_v1",
    )?;
    super::enrollment::validate_request(
        &bootstrap.audit_request,
        true,
        "review_change_revision_progress_v1",
    )?;
    if !bootstrap.root.is_absolute()
        || bootstrap.enrollment_actor.is_empty()
        || bootstrap.request_id == bootstrap.audit_request
    {
        return Err(ShoreError::Message(
            "invalid private audit root/identity".into(),
        ));
    }
    // SAFETY: inherited explicit whole-command lifetime and terminal contract.
    let _permit = unsafe { OwnedProcessPermit::enter(terminate) }?;
    let root = native::physical_root(&bootstrap.root)?;
    let canonical = native::root_snapshot(&root)?.canonical;
    let named_authority =
        open_identity_path(&canonical.join("authority.writer.lock"), false, true)?;
    let witness = file_identity(&named_authority)?;
    root.validate_live()?;
    audit::check()?;
    let deadline = scope
        .deadline()
        .min(std::time::Instant::now() + std::time::Duration::from_secs(2));
    let guard =
        StoreAuthorityLock::acquire_until(&canonical, deadline, Some(&bootstrap.cancellation))?;
    if guard.physical_lock_identity()? != witness {
        return Err(ShoreError::Message(
            "audit authority changed across acquisition".into(),
        ));
    }
    root.validate_live()?;
    let result = c4::audit::run(
        &root,
        &guard,
        &bootstrap.request_id,
        &bootstrap.enrollment_actor,
        &bootstrap.audit_request,
        &scope,
    );
    // Regardless of census failure, check the intended physical namespace before
    // exposing an attributable result. Cleanup never depends on remaining budget.
    root.validate_live()?;
    // SAFETY: native owner, held root and authority descriptors remain live.
    let named = unsafe {
        native::namespace::observe_private_worker(
            native::retained_root_file(&root)?,
            std::ffi::OsStr::new("authority.writer.lock"),
            native::namespace::Role::Authority,
        )
    }?;
    if !named.is_some_and(|value| value.identity == witness)
        || file_identity(&named_authority)? != guard.physical_lock_identity()?
    {
        return Err(ShoreError::Message(
            "audit outcome unknown: final authority binding changed".into(),
        ));
    }
    result
}
