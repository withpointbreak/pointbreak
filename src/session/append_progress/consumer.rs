//! The single admitted repository consumer. Public/shared-host routing stays closed.
use std::cell::RefCell;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::ledger::c4;
use super::{enrollment, native, owner};
use crate::canonical_hash::sha256_json_hex;
use crate::crypto::EventSigner;
use crate::error::{Result, ShoreError};
use crate::paths::CommonDirPaths;
use crate::session::derived_access::product_contract::DerivedAccessProfile;
use crate::session::event::{ShoreEvent, Writer};
use crate::session::store::authority_lock::{
    StoreAuthorityLock, file_identity, open_identity_path,
};
use crate::session::store::family_topology::CloneBindingGuard;
use crate::session::store::resolution::resolve_owned_change_store;
use crate::session::{DerivedWriteAcknowledgementV1, EventWriteOutcome};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConsumerAcknowledgement {
    authority_outcome: EventWriteOutcome,
    append_settlement: AppendSettlement,
    derived: DerivedWriteAcknowledgementV1,
    attempt_id: String,
    head: Option<u64>,
    detail: Option<String>,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum AppendSettlement {
    Confirmed,
    Unsettled,
}

struct CommandState {
    request: String,
    enroll_empty: bool,
    used: bool,
    cut_after_presence: bool,
    binding: Option<(Rc<CloneBindingGuard>, PathBuf, PathBuf)>,
    diagnostic: Option<String>,
}
thread_local! {
    static COMMAND: RefCell<Option<CommandState>> = const { RefCell::new(None) };
}

/// A non-Send, whole-command owner; only main's private Change branch creates it.
#[doc(hidden)]
pub struct OwnedChangeCommand {
    _permit: owner::OwnedProcessPermit,
}
impl Drop for OwnedChangeCommand {
    fn drop(&mut self) {
        COMMAND.with(|slot| *slot.borrow_mut() = None);
    }
}

/// Select one private Change-create command before parsing, tracing or workers.
///
/// # Safety
/// The caller owns the entire disposable process and must run only the audited
/// Change-create parser/handler while this value lives. No embedding host,
/// sibling operation or background worker may need continued execution. The
/// terminal callback must exit immediately without unwinding. Arguments or
/// environment alone never grant this authority.
#[doc(hidden)]
pub unsafe fn enter_owned_change_command(
    request: String,
    enroll_empty: bool,
    cut_after_presence: bool,
    terminate: fn(owner::OwnedProcessFailure) -> !,
) -> Result<OwnedChangeCommand> {
    enrollment::validate_request(&request, true, "review_change_revision_progress_v1")?;
    // SAFETY: inherited from the audited main branch's whole-process contract.
    let permit = unsafe { owner::OwnedProcessPermit::enter(terminate) }?;
    COMMAND.with(|slot| {
        *slot.borrow_mut() = Some(CommandState {
            request,
            enroll_empty,
            used: false,
            cut_after_presence,
            binding: None,
            diagnostic: None,
        });
    });
    Ok(OwnedChangeCommand { _permit: permit })
}

pub(in crate::session) fn selected() -> bool {
    COMMAND.with(|slot| slot.borrow().is_some())
}

#[doc(hidden)]
pub fn take_owned_change_diagnostic() -> Option<String> {
    COMMAND.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .and_then(|state| state.diagnostic.take())
    })
}

/// Runs at the EventStore publication boundary, after C4's durable intent.
/// Raw-root foundation controls have no command state and retain their contract.
pub(in crate::session) fn validate_publication(root: &Path) -> Result<()> {
    COMMAND.with(|slot| {
        let state = slot.borrow();
        let Some(state) = state.as_ref() else {
            return Ok(());
        };
        let (clone, common, expected) = state
            .binding
            .as_ref()
            .ok_or_else(|| refuse("missing clone binding"))?;
        if expected != root {
            return Err(refuse("publication root changed"));
        }
        clone.validate(common)
    })
}
fn refuse(message: &str) -> ShoreError {
    ShoreError::Message(format!("owned_process_required; {message}"))
}

pub(in crate::session) struct Consumer {
    // Drop authority/native handles before releasing the clone admission.
    authority: StoreAuthorityLock,
    named_authority: File,
    root: native::NativeRootHandle,
    clone: Rc<CloneBindingGuard>,
    common: PathBuf,
    canonical: PathBuf,
    request: String,
    actor: String,
    profile: DerivedAccessProfile,
}
impl Consumer {
    pub(in crate::session) fn begin(
        repo: &Path,
        signer: &(dyn EventSigner + Send + Sync),
        writer: Writer,
    ) -> Result<Self> {
        owner::require_quiescent()?;
        let (request, enroll_empty) = COMMAND.with(|slot| {
            let mut state = slot.borrow_mut();
            let state = state
                .as_mut()
                .ok_or_else(|| refuse("private command owner missing"))?;
            if state.used {
                return Err(refuse("one Change operation per process"));
            }
            state.used = true;
            Ok((state.request.clone(), state.enroll_empty))
        })?;
        let common = CommonDirPaths::resolve(repo)?;
        let clone = Rc::new(
            CloneBindingGuard::try_acquire(common.common_dir())?.ok_or_else(|| {
                ShoreError::Message("clone_binding_busy; retry original operation".into())
            })?,
        );
        // No store resolution or creation precedes clone admission.
        let resolved = resolve_owned_change_store(repo, &clone, common.common_dir())?;
        if resolved.store_dir() != common.store_dir() {
            return Err(refuse(
                "private development entry admits clone-local disposable stores only",
            ));
        }
        clone.validate(common.common_dir())?;
        let canonical = resolved.store_dir().to_path_buf();
        match fs::symlink_metadata(&canonical) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && enroll_empty => {
                fs::create_dir(&canonical)
                    .map_err(|error| ShoreError::Message(error.to_string()))?;
                File::open(common.common_dir())
                    .and_then(|file| file.sync_all())
                    .map_err(|error| ShoreError::Message(error.to_string()))?;
            }
            Err(error) => return Err(ShoreError::Message(error.to_string())),
        }
        let root = native::physical_root(&canonical)?;
        let canonical = native::root_snapshot(&root)?.canonical;
        let named_authority =
            open_identity_path(&canonical.join("authority.writer.lock"), false, true)?;
        let witness = file_identity(&named_authority)?;
        root.validate_live()?;
        let authority = StoreAuthorityLock::acquire_until(
            &canonical,
            Instant::now() + Duration::from_secs(2),
            None,
        )?;
        if authority.physical_lock_identity()? != witness {
            return Err(refuse("authority changed across acquisition"));
        }
        clone.validate(common.common_dir())?;
        root.validate_live()?;
        COMMAND.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .expect("live command owner")
                .binding = Some((
                clone.clone(),
                common.common_dir().to_path_buf(),
                canonical.clone(),
            ));
        });
        let consumer = Self {
            authority,
            named_authority,
            root,
            clone,
            common: common.common_dir().to_path_buf(),
            canonical,
            request,
            actor: writer.actor_id.as_str().to_owned(),
            profile: resolved.derived_access_profile(),
        };
        // A selector is only a reason to validate selected state; it grants no
        // authority itself. Missing selector never admits an ordinary writer.
        let has_selector =
            match fs::symlink_metadata(consumer.canonical.join("append-progress/active.json")) {
                Ok(_) => true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => return Err(ShoreError::Message(error.to_string())),
            };
        if enroll_empty && !has_selector {
            enrollment::enroll(
                &consumer.root,
                &consumer.authority,
                &consumer.request,
                true,
                "review_change_revision_progress_v1",
                signer,
                writer,
                &crate::session::current_timestamp(),
            )?;
        } else {
            // Enrollment selection proves H0 only. Operational admission must
            // also accept retained domain events and an unsettled obligation;
            // the original operation is validated before append reconciles it.
            c4::admit(
                &consumer.root,
                &consumer.authority,
                &consumer.request,
                &consumer.actor,
            )?;
        }
        consumer.validate()?;
        Ok(consumer)
    }
    pub(in crate::session) fn store_dir(&self) -> &Path {
        &self.canonical
    }
    fn validate(&self) -> Result<()> {
        self.clone.validate(&self.common)?;
        self.root.validate_live()?;
        self.authority.validate_physical_root(&self.canonical)?;
        if file_identity(&self.named_authority)? != self.authority.physical_lock_identity()? {
            return Err(refuse("authority identity changed"));
        }
        Ok(())
    }
    pub(in crate::session) fn append(
        &self,
        operation: &str,
        event: &ShoreEvent,
    ) -> Result<(EventWriteOutcome, bool)> {
        self.validate()?;
        // The original operation and logical event determine one stable attempt.
        let attempt = sha256_json_hex(&serde_json::json!({
            "domain": "pointbreak.change-create-progress-attempt.v1",
            "operationId": operation, "eventKey": event.idempotency_key,
        }))?;
        let prepared = c4::prepare(event.clone())?;
        let result = c4::append(
            &self.root,
            &self.authority,
            &self.request,
            &self.actor,
            &attempt,
            prepared,
        )?;
        self.validate()?;
        let outcome = result
            .outcome
            .ok_or_else(|| refuse("append returned no authority outcome"))?;
        let derived = if self.profile == DerivedAccessProfile::Off {
            DerivedWriteAcknowledgementV1::off()
        } else {
            DerivedWriteAcknowledgementV1::not_observed()
        };
        let diagnostic = serde_json::to_string(&ConsumerAcknowledgement {
            authority_outcome: outcome,
            append_settlement: if result.finalized {
                AppendSettlement::Confirmed
            } else {
                AppendSettlement::Unsettled
            },
            derived,
            attempt_id: attempt,
            head: result.head,
            detail: result.detail,
        })?;
        COMMAND.with(|slot| {
            slot.borrow_mut().as_mut().expect("live owner").diagnostic = Some(diagnostic)
        });
        Ok((outcome, result.finalized))
    }
}

// One explicit private interruption control: after the durable presence proof,
// before settling its original obligation. No environment grants this control.
pub(super) fn boundary(at: &str) -> Result<()> {
    let cut = COMMAND.with(|slot| slot.borrow().as_ref().is_some_and(|s| s.cut_after_presence));
    if cut && at == "after-presence-owner-finish" {
        owner::require_quiescent()?;
        // SAFETY: only the audited disposable command can install this control;
        // C4 has closed its SQL owner and established the presence-proof barrier.
        unsafe { libc::_exit(79) }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn change_consumer_requires_owner_before_repository_resolution() {
        let parent = tempfile::tempdir().unwrap();
        let repo = parent.path().join("must-not-exist");
        let signer = crate::crypto::TestEd25519Signer::from_seed([73; 32]);
        let result = Consumer::begin(&repo, &signer, Writer::shore_local("test"));
        let error = match result {
            Ok(_) => panic!("safe library caller admitted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("owned_process_required"));
        assert!(!repo.exists());
    }
}
