//! Product write coordination around disposable derived access.
//!
//! The coordinator is a two-state machine. `Governed` owns the existing
//! current-generation admission, truth publication, receipt finalization, and
//! catch-up protocol. `DegradedLoose` deliberately bypasses all derived work and
//! invokes the authoritative publisher exactly once. Missing, stale, corrupt,
//! busy, or ambiguous derived state selects the latter state; it can reduce
//! acceleration but cannot make an otherwise valid loose write unavailable.
//!
//! A governed coordinator may transition to degraded before truth publication
//! if its admitted generation disappears, or after publication if receipt
//! finalization fails. It never transitions in the other direction: rebuilding
//! and admitting a new immutable generation requires a fresh coordinator.

use std::cell::Cell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use super::cursor::AppendResolution;
use super::interaction::{RECOVERY_ACTION, claim_unavailable_hint};
use super::lifecycle::DerivedAccessLifecycle;
use super::sqlite::{AppendCrashPoint, StoreWriterLock};
#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::{
    LongitudinalDerivedAccessPhaseV1 as Phase, enter_derived_access_phase_v1,
};
use crate::error::{Result, ShoreError};
use crate::session::EventWriteOutcome;
use crate::session::event::ShoreEvent;

const MAX_DIAGNOSTICS: usize = 8;
const MAX_DIAGNOSTIC_MESSAGE_BYTES: usize = 512;
static ATTEMPT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static PROCESS_DIAGNOSTICS: OnceLock<Mutex<VecDeque<DerivedWriteDiagnostic>>> = OnceLock::new();

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DerivedWriteDiagnostic {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum DerivedWriteMode {
    Governed = 0,
    DegradedLoose = 1,
}

impl DerivedWriteMode {
    fn load(value: &AtomicU8) -> Self {
        match value.load(Ordering::Acquire) {
            0 => Self::Governed,
            1 => Self::DegradedLoose,
            _ => unreachable!("derived write mode has a closed representation"),
        }
    }
}

#[derive(Debug)]
pub(crate) struct DerivedWriteCoordinator {
    store_root: PathBuf,
    lifecycle: Option<DerivedAccessLifecycle>,
    mode: AtomicU8,
    process_hint: Mutex<DerivedWriteDiagnostic>,
    diagnostics: Mutex<VecDeque<DerivedWriteDiagnostic>>,
}

impl DerivedWriteCoordinator {
    /// Admit the active generation with one exact authoritative-head audit.
    pub(crate) fn new(lifecycle: DerivedAccessLifecycle) -> Result<Self> {
        let store_root = lifecycle.store_root().to_path_buf();
        let admission = lifecycle.admit_writer();
        let (mode, unavailable_detail) = match admission {
            Ok(true) => (DerivedWriteMode::Governed, None),
            Ok(false) => (
                DerivedWriteMode::DegradedLoose,
                Some("no usable derived generation is current".to_owned()),
            ),
            Err(error) => (DerivedWriteMode::DegradedLoose, Some(error.to_string())),
        };
        let process_hint = unavailable_diagnostic(
            unavailable_detail
                .as_deref()
                .unwrap_or("no usable derived generation is current"),
        );
        let coordinator = Self {
            store_root,
            lifecycle: Some(lifecycle),
            mode: AtomicU8::new(mode as u8),
            process_hint: Mutex::new(process_hint.clone()),
            diagnostics: Mutex::new(VecDeque::new()),
        };
        if unavailable_detail.is_some() {
            coordinator.push_diagnostic(process_hint);
        }
        Ok(coordinator)
    }

    pub(crate) fn degraded(store_root: impl Into<PathBuf>, detail: &str) -> Self {
        let process_hint = unavailable_diagnostic(detail);
        let coordinator = Self {
            store_root: store_root.into(),
            lifecycle: None,
            mode: AtomicU8::new(DerivedWriteMode::DegradedLoose as u8),
            process_hint: Mutex::new(process_hint.clone()),
            diagnostics: Mutex::new(VecDeque::new()),
        };
        coordinator.push_diagnostic(process_hint);
        coordinator
    }

    pub(crate) fn record_event_once(
        &self,
        event: &ShoreEvent,
        publish: impl FnOnce() -> Result<EventWriteOutcome>,
    ) -> Result<EventWriteOutcome> {
        if DerivedWriteMode::load(&self.mode) == DerivedWriteMode::DegradedLoose {
            return self.publish_degraded(publish);
        }
        self.record_event_once_with_hook(event, |_| {}, publish, catch_up_after_publication)
    }

    fn record_event_once_with_hook(
        &self,
        event: &ShoreEvent,
        hook: impl FnMut(AppendCrashPoint),
        publish: impl FnOnce() -> Result<EventWriteOutcome>,
        catch_up: impl FnOnce(&super::service::DerivedAccessService) -> std::result::Result<(), String>,
    ) -> Result<EventWriteOutcome> {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let admission_phase = enter_derived_access_phase_v1(Phase::GovernedWriteAdmission);
        let lifecycle = self
            .lifecycle
            .as_ref()
            .expect("governed derived writes retain their admitted lifecycle");
        let writer_lock = match StoreWriterLock::try_acquire(&self.store_root) {
            Ok(lock) => lock,
            Err(error) => {
                self.record_unavailable(&error.to_string());
                return self.publish_degraded(publish);
            }
        };
        let current = match lifecycle.open_current_for_write_locked(&writer_lock) {
            Ok(Some(current)) => current,
            Ok(None) => {
                drop(writer_lock);
                self.record_unavailable("no usable derived generation is current");
                return self.publish_degraded(publish);
            }
            Err(error) => {
                drop(writer_lock);
                self.record_unavailable(&error.to_string());
                return self.publish_degraded(publish);
            }
        };
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(admission_phase);
        let publication = Cell::new(None);
        let attempt_token = next_attempt_token(event);
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let truth_phase = enter_derived_access_phase_v1(Phase::GovernedWriteTruth);
        let mut publish = Some(publish);
        let append = current.service().append_event_with_publisher_locked(
            event,
            &attempt_token,
            &writer_lock,
            hook,
            || {
                let publish = publish
                    .take()
                    .expect("authoritative publisher is invoked at most once");
                let outcome = publish()?;
                publication.set(Some(outcome));
                Ok(outcome)
            },
        );
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(truth_phase);

        let resolution = match append {
            Ok(resolution) => resolution,
            Err(error) => {
                if let Some(outcome) = publication.get() {
                    drop(current);
                    let diagnostic = self.degrade_locked(
                        "derived_access_receipt_finalization_failed",
                        &error.to_string(),
                        &writer_lock,
                    );
                    drop(writer_lock);
                    self.enter_degraded(diagnostic.clone());
                    enqueue_process_diagnostic(diagnostic);
                    return Ok(outcome);
                }
                if let Some(publish) = publish.take() {
                    drop(current);
                    let quarantine = self.degrade_locked(
                        "derived_access_generation_unavailable",
                        &error.to_string(),
                        &writer_lock,
                    );
                    drop(writer_lock);
                    let diagnostic = unavailable_diagnostic(&quarantine.message);
                    self.enter_degraded(diagnostic);
                    return self.publish_degraded(publish);
                }
                return Err(ShoreError::Message(error.to_string()));
            }
        };
        let outcome = match resolution {
            AppendResolution::Created(_) => EventWriteOutcome::Created,
            AppendResolution::Existing(_) => {
                publication.get().unwrap_or(EventWriteOutcome::Existing)
            }
            AppendResolution::Conflict(_) => {
                return Err(ShoreError::Message(format!(
                    "event conflict for idempotency key {}",
                    event.idempotency_key
                )));
            }
        };
        drop(writer_lock);

        #[cfg(any(test, feature = "longitudinal-counting"))]
        let catch_up_phase = enter_derived_access_phase_v1(Phase::GovernedWriteCatchUp);
        let catch_up_lock = match StoreWriterLock::try_acquire(&self.store_root) {
            Ok(lock) => lock,
            Err(error) => {
                drop(current);
                self.record_catch_up_pending(&error.to_string());
                return Ok(outcome);
            }
        };
        if let Err(error) = catch_up(current.service()) {
            drop(catch_up_lock);
            drop(current);
            self.record_catch_up_pending(&error);
        }
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(catch_up_phase);
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let response_phase = enter_derived_access_phase_v1(Phase::GovernedWriteResponse);
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(response_phase);
        Ok(outcome)
    }

    #[cfg(test)]
    fn record_event_once_with_forced_post_truth_interruption(
        &self,
        event: &ShoreEvent,
        publish: impl FnOnce() -> Result<EventWriteOutcome>,
    ) -> Result<EventWriteOutcome> {
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.record_event_once_with_hook(
                event,
                |point| {
                    if point == AppendCrashPoint::AfterEventPublication {
                        panic!("forced post-truth interruption");
                    }
                },
                publish,
                catch_up_after_publication,
            )
        }));
        match interrupted {
            Ok(result) => result,
            Err(_) => {
                let detail = "derived writer interrupted after authoritative truth publication";
                let diagnostic = self.degrade("derived_access_receipt_finalization_failed", detail);
                self.enter_degraded(diagnostic.clone());
                enqueue_process_diagnostic(diagnostic);
                Ok(EventWriteOutcome::Created)
            }
        }
    }

    pub(crate) fn take_diagnostics(&self) -> Vec<DerivedWriteDiagnostic> {
        self.diagnostics
            .lock()
            .expect("derived write diagnostic lock poisoned")
            .drain(..)
            .collect()
    }

    fn degrade_locked(
        &self,
        code: &'static str,
        detail: &str,
        writer_lock: &StoreWriterLock,
    ) -> DerivedWriteDiagnostic {
        let quarantine = self
            .lifecycle
            .as_ref()
            .expect("governed derived writes retain their admitted lifecycle")
            .quarantine_current_locked(detail, writer_lock)
            .map(|path| format!("derived state quarantined at {}", path.display()))
            .unwrap_or_else(|error| format!("derived quarantine also failed: {error}"));
        diagnostic(code, detail, &quarantine)
    }

    fn degrade(&self, code: &'static str, detail: &str) -> DerivedWriteDiagnostic {
        let quarantine = self
            .lifecycle
            .as_ref()
            .expect("governed derived writes retain their admitted lifecycle")
            .quarantine_current(detail)
            .map(|path| format!("derived state quarantined at {}", path.display()))
            .unwrap_or_else(|error| format!("derived quarantine also failed: {error}"));
        diagnostic(code, detail, &quarantine)
    }

    fn push_diagnostic(&self, diagnostic: DerivedWriteDiagnostic) {
        tracing::warn!(
            code = diagnostic.code,
            message = diagnostic.message,
            "derived_write_diagnostic"
        );
        let mut diagnostics = self
            .diagnostics
            .lock()
            .expect("derived write diagnostic lock poisoned");
        push_bounded_diagnostic(&mut diagnostics, diagnostic);
    }

    fn publish_degraded(
        &self,
        publish: impl FnOnce() -> Result<EventWriteOutcome>,
    ) -> Result<EventWriteOutcome> {
        let outcome = publish();
        if outcome.is_ok() {
            let process_hint = self
                .process_hint
                .lock()
                .expect("derived process hint lock poisoned")
                .clone();
            enqueue_unavailable_process_hint(&self.store_root, process_hint);
        }
        outcome
    }

    fn enter_degraded(&self, diagnostic: DerivedWriteDiagnostic) {
        self.mode
            .store(DerivedWriteMode::DegradedLoose as u8, Ordering::Release);
        *self
            .process_hint
            .lock()
            .expect("derived process hint lock poisoned") = diagnostic.clone();
        self.push_diagnostic(diagnostic);
    }

    fn record_unavailable(&self, detail: &str) {
        self.enter_degraded(unavailable_diagnostic(detail));
    }

    fn record_catch_up_pending(&self, detail: &str) {
        let diagnostic = diagnostic(
            "derived_access_projection_catch_up_deferred",
            detail,
            "derived generation remains published as CatchingUp",
        );
        self.push_diagnostic(diagnostic.clone());
        enqueue_process_diagnostic(diagnostic);
    }
}

pub(crate) fn take_process_diagnostics() -> Vec<DerivedWriteDiagnostic> {
    PROCESS_DIAGNOSTICS
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .expect("derived process diagnostic lock poisoned")
        .drain(..)
        .collect()
}

fn enqueue_unavailable_process_hint(
    store_root: &std::path::Path,
    diagnostic: DerivedWriteDiagnostic,
) {
    if !claim_unavailable_hint(store_root) {
        return;
    }
    enqueue_process_diagnostic(diagnostic);
}

fn enqueue_process_diagnostic(diagnostic: DerivedWriteDiagnostic) {
    let mut diagnostics = PROCESS_DIAGNOSTICS
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .expect("derived process diagnostic lock poisoned");
    push_bounded_diagnostic(&mut diagnostics, diagnostic);
}

fn push_bounded_diagnostic(
    diagnostics: &mut VecDeque<DerivedWriteDiagnostic>,
    diagnostic: DerivedWriteDiagnostic,
) {
    if diagnostics.len() == MAX_DIAGNOSTICS {
        diagnostics.pop_front();
    }
    diagnostics.push_back(diagnostic);
}

fn catch_up_after_publication(
    service: &super::service::DerivedAccessService,
) -> std::result::Result<(), String> {
    let mut last_error = None;
    for _ in 0..8 {
        match service.catch_up_to_head(512) {
            Ok(_) => return Ok(()),
            Err(error) => {
                let caught_up = service
                    .locator_checkpoint()
                    .and_then(|checkpoint| {
                        service.truth_head().map(|head| checkpoint == head.cursor)
                    })
                    .unwrap_or(false);
                if caught_up {
                    return Ok(());
                }
                last_error = Some(error.to_string());
                std::thread::yield_now();
            }
        }
    }
    Err(last_error.unwrap_or_else(|| "derived catch-up did not run".to_owned()))
}

fn next_attempt_token(event: &ShoreEvent) -> String {
    format!(
        "product:{}:{}:{}",
        std::process::id(),
        ATTEMPT_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        event.event_id.as_str()
    )
}

fn diagnostic(code: &'static str, detail: &str, quarantine: &str) -> DerivedWriteDiagnostic {
    let message = format!("{detail}; {quarantine}");
    DerivedWriteDiagnostic {
        code,
        message: truncate_utf8(&message, MAX_DIAGNOSTIC_MESSAGE_BYTES),
    }
}

fn unavailable_diagnostic(detail: &str) -> DerivedWriteDiagnostic {
    const PREFIX: &str = "derived acceleration is unavailable (";
    let action = format!("); {RECOVERY_ACTION}");
    let detail = truncate_utf8(
        detail,
        MAX_DIAGNOSTIC_MESSAGE_BYTES - PREFIX.len() - action.len(),
    );
    let message = format!("{PREFIX}{detail}{action}");
    DerivedWriteDiagnostic {
        code: "derived_access_generation_unavailable",
        message: truncate_utf8(&message, MAX_DIAGNOSTIC_MESSAGE_BYTES),
    }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::Mutex;

    use rusqlite::{Connection, OpenFlags};
    use tempfile::TempDir;

    use super::{AppendCrashPoint, DerivedWriteCoordinator, catch_up_after_publication};
    use crate::bench_support::longitudinal::{
        LongitudinalCountingScopeV1, LongitudinalDerivedAccessPhaseOwnershipV1,
        LongitudinalDerivedAccessPhaseV1,
    };
    use crate::canonical_hash::sha256_json_prefixed;
    use crate::crypto::SignerId;
    use crate::model::JournalId;
    use crate::session::derived_access::history::{
        CurrentRead, DerivedHistoryAccess, DerivedHistoryAvailability, DerivedHistoryMode,
        DerivedHistoryStatus,
    };
    use crate::session::derived_access::lifecycle::{
        DerivedAccessLifecycle, LifecycleControl, LifecycleError,
    };
    use crate::session::derived_access::product_contract::{
        DerivedAccessAvailability, DerivedAccessProfile,
    };
    use crate::session::derived_access::semantic::change::{
        ReaderProjectionCheckpointV1, reader_projection_checkpoint_sha256_v1,
    };
    use crate::session::derived_access::sqlite::{CursorLedgerIdentity, SqliteCursorLedger};
    use crate::session::event::{
        EventSignature, EventTarget, EventType, ReviewInitializedPayload, ShoreEvent, Writer,
    };
    use crate::session::store::backend::StoreBackend;
    use crate::session::store::bundle::import_store_bundle_into_with_verification;
    use crate::session::store::capabilities::{
        AUTHORITY_CURSOR_SCHEMA_V2, CapabilityFixtureState, inspect_journal_records,
        write_capability_fixture_for_test,
    };
    use crate::session::store::resolution::{
        event_store_for_explicit_target, opaque_path_identity,
    };
    use crate::session::{
        AuthorityCursorV2, EventStore, EventVerificationPolicy, EventWriteOutcome, TrustSet,
    };

    #[test]
    fn governed_write_advances_once_and_out_of_band_append_requires_rebuild() {
        let root = TempDir::new().unwrap();
        let store = EventStore::open(root.path());
        store.record_event_once(&event(0)).unwrap();

        let lifecycle = active_lifecycle(&root);
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let coordinator =
            DerivedWriteCoordinator::new(active_lifecycle(&root)).expect("current generation");
        let governed = EventStore::open(root.path()).with_coordinator(coordinator);
        let scope = LongitudinalCountingScopeV1::new("d".repeat(64)).unwrap();
        let guard = scope.enter();

        assert_eq!(
            governed.record_event_once(&event(1)).unwrap(),
            EventWriteOutcome::Created
        );
        drop(guard);
        assert_eq!(
            scope
                .snapshot()
                .derived_access_phases
                .iter()
                .map(|sample| sample.phase)
                .collect::<Vec<_>>(),
            crate::bench_support::derived_access::QualificationDerivedAccessPhaseOperationV1::GovernedWrite
                .expected_phases()
        );
        assert_eq!(
            governed.record_event_once(&event(1)).unwrap(),
            EventWriteOutcome::Existing
        );
        assert_eq!(
            active_lifecycle(&root)
                .open_current()
                .unwrap()
                .unwrap()
                .service()
                .truth_head()
                .unwrap()
                .cursor
                .sequence,
            2
        );

        assert_eq!(
            EventStore::open(root.path())
                .record_event_once(&event(2))
                .unwrap(),
            EventWriteOutcome::Created
        );
        assert!(matches!(
            active_lifecycle(&root).open_current(),
            Err(LifecycleError::RebuildRequired(_))
        ));
    }

    #[test]
    fn generation_lost_after_admission_degrades_before_truth_publication() {
        let root = TempDir::new().unwrap();
        let truth = EventStore::open(root.path());
        truth.record_event_once(&event(0)).unwrap();
        active_lifecycle(&root)
            .rebuild(|_| LifecycleControl::Continue)
            .unwrap();
        let coordinator =
            DerivedWriteCoordinator::new(active_lifecycle(&root)).expect("current generation");
        active_lifecycle(&root)
            .quarantine_current("forced pre-truth unavailability")
            .unwrap();

        assert_eq!(
            coordinator
                .record_event_once(&event(1), || truth.record_event_once(&event(1)))
                .unwrap(),
            EventWriteOutcome::Created
        );
        assert!(truth.event_exists(&event(1).idempotency_key).unwrap());
        let diagnostics = coordinator.take_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "derived_access_generation_unavailable");
    }

    #[test]
    fn post_truth_derived_failure_returns_created_and_degrades_following_writes() {
        let root = TempDir::new().unwrap();
        let truth = EventStore::open(root.path());
        truth.record_event_once(&event(0)).unwrap();
        let lifecycle = active_lifecycle(&root);
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let coordinator =
            DerivedWriteCoordinator::new(active_lifecycle(&root)).expect("current generation");

        let outcome = coordinator
            .record_event_once_with_forced_post_truth_interruption(&event(1), || {
                truth.record_event_once(&event(1))
            })
            .unwrap();

        assert_eq!(outcome, EventWriteOutcome::Created);
        assert!(truth.event_exists(&event(1).idempotency_key).unwrap());
        assert_eq!(
            coordinator.take_diagnostics()[0].code,
            "derived_access_receipt_finalization_failed"
        );
        assert_eq!(
            coordinator
                .record_event_once(&event(2), || truth.record_event_once(&event(2)))
                .unwrap(),
            EventWriteOutcome::Created
        );
    }

    #[test]
    fn out_of_band_create_during_stamp_publication_cannot_be_absorbed() {
        let root = TempDir::new().unwrap();
        let truth = EventStore::open(root.path());
        truth.record_event_once(&event(0)).unwrap();
        let lifecycle = active_lifecycle(&root);
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let coordinator =
            DerivedWriteCoordinator::new(active_lifecycle(&root)).expect("current generation");
        let raced = std::cell::Cell::new(false);

        let outcome = coordinator
            .record_event_once_with_hook(
                &event(1),
                |point| {
                    if point == AppendCrashPoint::AfterEventPublication && !raced.replace(true) {
                        assert_eq!(
                            truth.record_event_once(&event(2)).unwrap(),
                            EventWriteOutcome::Created
                        );
                    }
                },
                || truth.record_event_once(&event(1)),
                catch_up_after_publication,
            )
            .unwrap();

        assert_eq!(outcome, EventWriteOutcome::Created);
        assert_ne!(
            active_lifecycle(&root).status().unwrap().availability,
            DerivedAccessAvailability::Current
        );
        assert_eq!(truth.list_events().unwrap().len(), 3);
        assert_eq!(
            coordinator.take_diagnostics()[0].code,
            "derived_access_receipt_finalization_failed"
        );
    }

    #[test]
    fn production_event_publishers_use_the_resolved_event_store_factory() {
        const WRITERS: &[(&str, &str)] = &[
            ("capture", include_str!("../workflow/capture.rs")),
            ("ingest", include_str!("../workflow/ingest.rs")),
            (
                "observation",
                include_str!("../workflow/observation/add.rs"),
            ),
            ("assessment", include_str!("../workflow/assessment/add.rs")),
            ("validation", include_str!("../workflow/validation/add.rs")),
            (
                "input request open",
                include_str!("../workflow/input_request/open.rs"),
            ),
            (
                "input request respond",
                include_str!("../workflow/input_request/respond.rs"),
            ),
            (
                "association",
                include_str!("../workflow/association/mod.rs"),
            ),
            ("change", include_str!("../workflow/change.rs")),
            ("fact port", include_str!("../workflow/fact_port.rs")),
            ("landing", include_str!("../workflow/landing.rs")),
            (
                "artifact removal",
                include_str!("../workflow/artifact_removal/mod.rs"),
            ),
            (
                "event signature",
                include_str!("../workflow/event_signature/mod.rs"),
            ),
        ];
        for (name, source) in WRITERS {
            assert!(
                source.contains("write_store.event_store()?"),
                "{name} bypasses the resolved event-store factory"
            );
            assert!(
                !source.contains("EventStore::from_backend(write_store.backend())"),
                "{name} still constructs a backend-only product writer"
            );
        }

        let bundle = include_str!("../store/bundle.rs");
        let migration = include_str!("../workflow/store_migrate_common_dir.rs");
        let link = include_str!("../workflow/store_link.rs");
        assert!(bundle.contains("import_store_bundle_into_with_verification"));
        assert!(migration.contains("event_store_for_explicit_target"));
        assert!(link.contains("event_store_for_explicit_target"));
    }

    #[test]
    fn bounded_admission_and_append_do_not_walk_event_directory_entries() {
        let root = TempDir::new().unwrap();
        let truth = EventStore::open(root.path());
        for index in 0..3 {
            truth.record_event_once(&event(index)).unwrap();
        }
        let lifecycle = active_lifecycle(&root);
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let admission = counting_scope('a');
        let coordinator = {
            let _guard = admission.enter();
            DerivedWriteCoordinator::new(active_lifecycle(&root)).unwrap()
        };
        assert_eq!(admission.snapshot().counters.directory_entries_walked, 0);

        let append = counting_scope('b');
        {
            let _guard = append.enter();
            coordinator
                .record_event_once(&event(3), || truth.record_event_once(&event(3)))
                .unwrap();
        }
        assert_eq!(append.snapshot().counters.directory_entries_walked, 0);
    }

    #[test]
    fn rebuilt_change_generation_seeds_only_bodyless_authority_record_identities() {
        let (root, _backend, lifecycle) = ready_change_lifecycle();
        let connection = current_projection_connection(&lifecycle);
        let columns = connection
            .prepare("PRAGMA table_info(authority_record_identity)")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(
            columns,
            vec![
                ("key_digest".to_owned(), "BLOB".to_owned()),
                ("record_hash".to_owned(), "BLOB".to_owned()),
                ("event_payload_hash".to_owned(), "BLOB".to_owned()),
            ],
            "the published generation must persist the normalized bodyless authority identity set"
        );

        let (record_count, event_count, malformed_digest_count): (i64, i64, i64) = connection
            .query_row(
                "SELECT COUNT(*),
                        SUM(CASE WHEN event_payload_hash IS NOT NULL THEN 1 ELSE 0 END),
                        SUM(CASE
                                WHEN length(key_digest) != 32
                                  OR length(record_hash) != 32
                                  OR (event_payload_hash IS NOT NULL
                                      AND length(event_payload_hash) != 32)
                                THEN 1 ELSE 0
                            END)
                 FROM authority_record_identity",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let strict = inspect_journal_records(
            StoreBackend::Local(root.path().to_path_buf())
                .journal()
                .as_ref(),
        )
        .unwrap();
        assert_eq!(
            u64::try_from(record_count).unwrap(),
            strict.cursor.journal_record_count
        );
        assert_eq!(
            u64::try_from(event_count).unwrap(),
            strict.cursor.event_count
        );
        assert_eq!(malformed_digest_count, 0);
    }

    #[test]
    fn governed_change_prefixes_reproduce_the_exact_frozen_authority_cursor() {
        let (_root, backend, lifecycle) = ready_change_lifecycle();
        assert_bodyless_authority_matches_strict(&backend, &lifecycle);

        let coordinator = DerivedWriteCoordinator::new(lifecycle.clone()).unwrap();
        let governed = EventStore::from_backend(&backend).with_coordinator(coordinator);
        for index in 40..43 {
            assert_eq!(
                governed.record_event_once(&event(index)).unwrap(),
                EventWriteOutcome::Created
            );
            assert_bodyless_authority_matches_strict(&backend, &lifecycle);
        }
    }

    #[test]
    fn governed_change_catch_up_counts_bodyless_authority_maintenance_separately() {
        let (_root, backend, lifecycle) = ready_change_lifecycle();
        let coordinator = DerivedWriteCoordinator::new(lifecycle.clone()).unwrap();
        let governed = EventStore::from_backend(&backend).with_coordinator(coordinator);
        let scope = counting_scope('c');
        {
            let _guard = scope.enter();
            assert_eq!(
                governed.record_event_once(&event(49)).unwrap(),
                EventWriteOutcome::Created
            );
        }

        let snapshot = scope.snapshot();
        let phase = snapshot
            .derived_access_phases
            .iter()
            .find(|sample| {
                sample.phase
                    == LongitudinalDerivedAccessPhaseV1::GovernedWriteAuthorityCursorMaintenance
            })
            .expect("Change catch-up records authority cursor maintenance");
        let parent = snapshot
            .derived_access_phases
            .iter()
            .find(|sample| Some(sample.ordinal) == phase.parent_ordinal)
            .expect("authority maintenance retains its catch-up parent");

        assert_eq!(
            phase.ownership,
            LongitudinalDerivedAccessPhaseOwnershipV1::DerivedAccess
        );
        assert_eq!(
            parent.phase,
            LongitudinalDerivedAccessPhaseV1::GovernedWriteCatchUp
        );
        assert_eq!(
            phase.counters.authority_identity_rows_scanned,
            authority_identity_count(&lifecycle)
        );
        assert_eq!(phase.counters.directory_entries_walked, 0);
        assert_eq!(phase.counters.carrier_opens, 0);
        assert_eq!(phase.counters.carrier_bytes_read, 0);
        assert_eq!(phase.counters.event_decodes, 0);
        assert_eq!(phase.counters.event_validations, 0);
        assert_eq!(phase.counters.event_folds, 0);
    }

    #[test]
    fn current_change_read_does_not_run_authority_cursor_maintenance() {
        let (root, backend, lifecycle) = ready_change_lifecycle();
        let access = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Active {
            lifecycle,
            current: Mutex::new(None),
            store_identity: opaque_path_identity("store", root.path()).unwrap(),
            backend,
        });
        let scope = counting_scope('e');
        {
            let _guard = scope.enter();
            assert!(matches!(access.current().unwrap(), CurrentRead::Ready(_)));
        }

        let snapshot = scope.snapshot();
        assert_eq!(snapshot.counters.authority_identity_rows_scanned, 0);
        assert_eq!(snapshot.counters.directory_entries_walked, 0);
        assert_eq!(snapshot.counters.event_decodes, 0);
        assert_eq!(snapshot.counters.event_validations, 0);
        assert!(snapshot.derived_access_phases.iter().all(|sample| {
            sample.phase
                != LongitudinalDerivedAccessPhaseV1::GovernedWriteAuthorityCursorMaintenance
        }));
    }

    #[test]
    fn governed_change_catch_up_atomically_advances_the_live_checkpoint() {
        let (_root, backend, lifecycle) = ready_change_lifecycle();
        let before = read_live_checkpoint(&lifecycle);
        let before_hash = before.checkpoint_sha256.clone();
        let before_anchor = before.reader_receipt_sha256.clone();

        let coordinator = DerivedWriteCoordinator::new(lifecycle.clone()).unwrap();
        let governed = EventStore::from_backend(&backend).with_coordinator(coordinator);
        assert_eq!(
            governed.record_event_once(&event(50)).unwrap(),
            EventWriteOutcome::Created
        );

        let after = read_live_checkpoint(&lifecycle);
        assert_eq!(after.reader_receipt_sha256, before_anchor);
        assert_ne!(after.checkpoint_sha256, before_hash);
        assert_eq!(
            after.checkpoint_sha256,
            reader_projection_checkpoint_sha256_v1(&after).unwrap()
        );
        assert_eq!(
            after.truth_cursor.sequence,
            before.truth_cursor.sequence + 1
        );
        assert_eq!(
            after.authority_cursor.event_count,
            after.truth_cursor.sequence
        );
        assert_eq!(after.locator_checkpoint.applied_cursor, after.truth_cursor);
        assert_eq!(after.locator_checkpoint.observed_cursor, after.truth_cursor);
        assert_eq!(after.semantic_checkpoint.applied_cursor, after.truth_cursor);
        assert_eq!(after.product_checkpoint.applied_cursor, after.truth_cursor);
        assert_eq!(
            after.authority_cursor,
            inspect_journal_records(backend.journal().as_ref())
                .unwrap()
                .cursor
        );
    }

    #[test]
    fn interrupted_change_catch_up_rolls_back_identity_and_checkpoint_then_resumes() {
        let (_root, backend, lifecycle) = ready_change_lifecycle();
        let before_checkpoint = read_live_checkpoint_bytes(&lifecycle);
        let before_identity_count = authority_identity_count(&lifecycle);
        let truth = EventStore::from_backend(&backend);
        let interrupted_event = event(51);
        let coordinator = DerivedWriteCoordinator::new(lifecycle.clone()).unwrap();

        assert_eq!(
            coordinator
                .record_event_once_with_hook(
                    &interrupted_event,
                    |_| {},
                    || truth.record_event_once(&interrupted_event),
                    |_| Err("forced catch-up deferral before semantic application".to_owned()),
                )
                .unwrap(),
            EventWriteOutcome::Created
        );
        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::CatchingUp
        );

        assert!(
            lifecycle
                .maintain_current_generation_with_interruption(512)
                .is_err()
        );
        assert_eq!(read_live_checkpoint_bytes(&lifecycle), before_checkpoint);
        assert_eq!(authority_identity_count(&lifecycle), before_identity_count);

        assert!(lifecycle.maintain_current_generation().unwrap());
        let recovered = lifecycle
            .open_current()
            .unwrap()
            .unwrap()
            .service()
            .locator_checkpoint()
            .unwrap();
        let checkpoint = read_live_checkpoint(&lifecycle);
        assert_eq!(checkpoint.truth_cursor, recovered);
        assert_eq!(
            authority_identity_count(&lifecycle),
            before_identity_count + 1
        );
        assert_eq!(
            checkpoint.authority_cursor,
            inspect_journal_records(backend.journal().as_ref())
                .unwrap()
                .cursor
        );
        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::Current
        );
    }

    #[test]
    fn cold_reader_defers_interrupted_catch_up_to_the_shared_maintenance_worker() {
        let (root, backend, lifecycle) = ready_change_lifecycle();
        let generation_id = lifecycle
            .published_generation_id()
            .unwrap()
            .expect("ready fixture has a current generation");
        let truth = EventStore::from_backend(&backend);
        let interrupted_event = event(52);
        let coordinator = DerivedWriteCoordinator::new(lifecycle.clone()).unwrap();
        assert_eq!(
            coordinator
                .record_event_once_with_hook(
                    &interrupted_event,
                    |_| {},
                    || truth.record_event_once(&interrupted_event),
                    |_| Err("forced catch-up deferral before reader recovery".to_owned()),
                )
                .unwrap(),
            EventWriteOutcome::Created
        );

        let access = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Active {
            lifecycle: lifecycle.clone(),
            current: Mutex::new(None),
            store_identity: opaque_path_identity("store", root.path()).unwrap(),
            backend,
        });
        assert!(matches!(
            access.current().unwrap(),
            CurrentRead::Unavailable(DerivedHistoryStatus {
                availability: DerivedHistoryAvailability::CatchingUp,
                ..
            })
        ));

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while lifecycle.status().unwrap().availability != DerivedAccessAvailability::Current {
            assert!(
                std::time::Instant::now() < deadline,
                "maintenance worker did not resume the interrupted projection"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(matches!(access.current().unwrap(), CurrentRead::Ready(_)));
        assert_eq!(
            lifecycle.published_generation_id().unwrap().as_deref(),
            Some(generation_id.as_str()),
            "catch-up must preserve the current generation"
        );
    }

    #[test]
    fn explicit_active_target_factory_uses_the_product_store_identity() {
        let root = TempDir::new().unwrap();
        let identity = opaque_path_identity("store", root.path()).unwrap();
        let lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            root.path(),
            identity,
        )
        .unwrap();
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();

        let store =
            event_store_for_explicit_target(root.path(), DerivedAccessProfile::SqliteWalBodylessV1)
                .unwrap();
        assert_eq!(
            store.record_event_once(&event(0)).unwrap(),
            EventWriteOutcome::Created
        );
    }

    #[test]
    fn off_factory_is_sidecar_free_and_active_reopen_detects_the_off_write() {
        let root = TempDir::new().unwrap();
        let identity = opaque_path_identity("store", root.path()).unwrap();
        DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            root.path(),
            identity,
        )
        .unwrap()
        .rebuild(|_| LifecycleControl::Continue)
        .unwrap();
        let active =
            event_store_for_explicit_target(root.path(), DerivedAccessProfile::SqliteWalBodylessV1)
                .unwrap();
        active.record_event_once(&event(0)).unwrap();

        let off = event_store_for_explicit_target(root.path(), DerivedAccessProfile::Off).unwrap();
        assert_eq!(
            off.record_event_once(&event(1)).unwrap(),
            EventWriteOutcome::Created
        );
        let reopened =
            event_store_for_explicit_target(root.path(), DerivedAccessProfile::SqliteWalBodylessV1)
                .expect("stale disposable state degrades instead of blocking truth");
        assert_eq!(
            reopened.record_event_once(&event(2)).unwrap(),
            EventWriteOutcome::Created
        );
        assert_eq!(
            reopened.take_write_diagnostics()[0].code,
            "derived_access_generation_unavailable"
        );
        assert_eq!(
            EventStore::open(root.path()).list_events().unwrap().len(),
            3
        );
    }

    #[test]
    fn governed_duplicate_preserves_divergent_signature_semantics_without_advancing() {
        let root = TempDir::new().unwrap();
        active_product_lifecycle(&root)
            .rebuild(|_| LifecycleControl::Continue)
            .unwrap();
        let store =
            event_store_for_explicit_target(root.path(), DerivedAccessProfile::SqliteWalBodylessV1)
                .unwrap();
        assert_eq!(
            store.record_event_once(&signed_event(0, 'A')).unwrap(),
            EventWriteOutcome::Created
        );
        assert_eq!(
            store.record_event_once(&signed_event(0, 'B')).unwrap(),
            EventWriteOutcome::ExistingDivergentSignature
        );
        assert_eq!(
            active_product_lifecycle(&root)
                .open_current()
                .unwrap()
                .unwrap()
                .service()
                .truth_head()
                .unwrap()
                .cursor
                .sequence,
            1
        );
    }

    #[test]
    fn bundle_batch_uses_the_same_governed_writer() {
        let source = TempDir::new().unwrap();
        let source_store = EventStore::open(source.path());
        for index in 0..3 {
            source_store.record_event_once(&event(index)).unwrap();
        }
        let target = TempDir::new().unwrap();
        active_product_lifecycle(&target)
            .rebuild(|_| LifecycleControl::Continue)
            .unwrap();
        let target_store = event_store_for_explicit_target(
            target.path(),
            DerivedAccessProfile::SqliteWalBodylessV1,
        )
        .unwrap();

        let result = import_store_bundle_into_with_verification(
            source.path(),
            target.path(),
            &target_store,
            EventVerificationPolicy::advisory(),
            TrustSet::default(),
        )
        .unwrap();

        assert_eq!(result.events_created, 3);
        assert_eq!(
            active_product_lifecycle(&target)
                .open_current()
                .unwrap()
                .unwrap()
                .service()
                .truth_head()
                .unwrap()
                .cursor
                .sequence,
            3
        );
    }

    #[test]
    fn duplicate_after_head_crash_recovers_and_catches_projections_up() {
        let root = TempDir::new().unwrap();
        let lifecycle = active_product_lifecycle(&root);
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let publication = lifecycle.paths().current_publication().unwrap().unwrap();
        let generation = lifecycle.paths().generation(&publication.generation_id);
        let identity = opaque_path_identity("store", root.path()).unwrap();
        let ledger = SqliteCursorLedger::open_immutable_at(
            root.path(),
            &generation,
            CursorLedgerIdentity::new(identity),
        )
        .unwrap();
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ledger
                .append_event_with_hook(&event(0), "crash:after-head", |point| {
                    if point == AppendCrashPoint::AfterHeadBeforeIntentRetirement {
                        panic!("forced crash after head");
                    }
                })
                .unwrap();
        }));
        assert!(interrupted.is_err());

        let store =
            event_store_for_explicit_target(root.path(), DerivedAccessProfile::SqliteWalBodylessV1)
                .unwrap();
        assert_eq!(
            store.record_event_once(&event(0)).unwrap(),
            EventWriteOutcome::Existing
        );
        assert_eq!(
            active_product_lifecycle(&root)
                .status()
                .unwrap()
                .availability,
            DerivedAccessAvailability::Current
        );
    }

    #[test]
    fn catch_up_failure_remains_retryable_without_quarantining_the_generation() {
        let root = TempDir::new().unwrap();
        let truth = EventStore::open(root.path());
        active_product_lifecycle(&root)
            .rebuild(|_| LifecycleControl::Continue)
            .unwrap();
        let coordinator = DerivedWriteCoordinator::new(active_product_lifecycle(&root))
            .expect("current generation");

        assert_eq!(
            coordinator
                .record_event_once_with_hook(
                    &event(0),
                    |_| {},
                    || truth.record_event_once(&event(0)),
                    |_| Err("forced projection catch-up failure".to_owned()),
                )
                .unwrap(),
            EventWriteOutcome::Created
        );
        assert_eq!(
            coordinator.take_diagnostics()[0].code,
            "derived_access_projection_catch_up_deferred"
        );
        assert_eq!(
            active_product_lifecycle(&root)
                .status()
                .unwrap()
                .availability,
            DerivedAccessAvailability::CatchingUp
        );

        assert_eq!(
            coordinator
                .record_event_once(&event(1), || truth.record_event_once(&event(1)))
                .unwrap(),
            EventWriteOutcome::Created
        );
        assert_eq!(
            active_product_lifecycle(&root)
                .status()
                .unwrap()
                .availability,
            DerivedAccessAvailability::Current
        );
    }

    #[test]
    fn concurrent_product_writers_preserve_authoritative_events() {
        let root = TempDir::new().unwrap();
        let identity = opaque_path_identity("store", root.path()).unwrap();
        DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            root.path(),
            identity,
        )
        .unwrap()
        .rebuild(|_| LifecycleControl::Continue)
        .unwrap();

        let mut first = product_writer_child(root.path(), 0);
        let mut second = product_writer_child(root.path(), 1);
        assert!(first.wait().unwrap().success());
        assert!(second.wait().unwrap().success());
        assert_eq!(
            EventStore::open(root.path()).list_events().unwrap().len(),
            2
        );
        assert_eq!(
            event_store_for_explicit_target(
                root.path(),
                DerivedAccessProfile::SqliteWalBodylessV1,
            )
            .unwrap()
            .record_event_once(&event(1))
            .unwrap(),
            EventWriteOutcome::Existing
        );
    }

    #[test]
    #[ignore = "spawned by concurrent_product_writers_preserve_authoritative_events"]
    fn product_writer_child_entrypoint() {
        let root =
            std::path::PathBuf::from(std::env::var_os("POINTBREAK_TEST_WRITER_ROOT").unwrap());
        let index = std::env::var("POINTBREAK_TEST_WRITER_INDEX")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let store =
            event_store_for_explicit_target(&root, DerivedAccessProfile::SqliteWalBodylessV1)
                .unwrap();
        assert_eq!(
            store.record_event_once(&event(index)).unwrap(),
            EventWriteOutcome::Created
        );
    }

    fn product_writer_child(root: &std::path::Path, index: usize) -> std::process::Child {
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "session::derived_access::writer::tests::product_writer_child_entrypoint",
                "--nocapture",
            ])
            .env("POINTBREAK_TEST_WRITER_ROOT", root)
            .env("POINTBREAK_TEST_WRITER_INDEX", index.to_string())
            .spawn()
            .unwrap()
    }

    fn counting_scope(discriminator: char) -> LongitudinalCountingScopeV1 {
        LongitudinalCountingScopeV1::new(discriminator.to_string().repeat(64)).unwrap()
    }

    fn active_lifecycle(root: &TempDir) -> DerivedAccessLifecycle {
        DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            root.path(),
            "store:test",
        )
        .unwrap()
    }

    fn active_product_lifecycle(root: &TempDir) -> DerivedAccessLifecycle {
        DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            root.path(),
            opaque_path_identity("store", root.path()).unwrap(),
        )
        .unwrap()
    }

    fn ready_change_lifecycle() -> (TempDir, StoreBackend, DerivedAccessLifecycle) {
        let root = TempDir::new().unwrap();
        let backend = StoreBackend::Local(root.path().to_path_buf());
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();
        let lifecycle = active_product_lifecycle(&root);
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        (root, backend, lifecycle)
    }

    fn current_projection_connection(lifecycle: &DerivedAccessLifecycle) -> Connection {
        let publication = lifecycle.paths().current_publication().unwrap().unwrap();
        let database = lifecycle
            .paths()
            .generation(&publication.generation_id)
            .join("cursor.sqlite3");
        Connection::open_with_flags(
            database,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap()
    }

    fn authority_identity_count(lifecycle: &DerivedAccessLifecycle) -> u64 {
        let count = current_projection_connection(lifecycle)
            .query_row(
                "SELECT COUNT(*) FROM authority_record_identity",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        u64::try_from(count).unwrap()
    }

    fn read_live_checkpoint_bytes(lifecycle: &DerivedAccessLifecycle) -> Vec<u8> {
        current_projection_connection(lifecycle)
            .query_row(
                "SELECT checkpoint_json
                 FROM reader_projection_checkpoint
                 WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0).map(String::into_bytes),
            )
            .unwrap()
    }

    fn read_live_checkpoint(lifecycle: &DerivedAccessLifecycle) -> ReaderProjectionCheckpointV1 {
        serde_json::from_slice(&read_live_checkpoint_bytes(lifecycle)).unwrap()
    }

    fn assert_bodyless_authority_matches_strict(
        backend: &StoreBackend,
        lifecycle: &DerivedAccessLifecycle,
    ) {
        let strict = inspect_journal_records(backend.journal().as_ref()).unwrap();
        let derived = bodyless_authority_cursor(
            &current_projection_connection(lifecycle),
            strict.cursor.capability_set_hash.clone(),
        );
        assert_eq!(derived, strict.cursor);
    }

    fn bodyless_authority_cursor(
        connection: &Connection,
        capability_set_hash: String,
    ) -> AuthorityCursorV2 {
        let mut statement = connection
            .prepare(
                "SELECT lower(hex(key_digest)),
                        lower(hex(record_hash)),
                        CASE WHEN event_payload_hash IS NULL THEN NULL
                             ELSE lower(hex(event_payload_hash)) END
                 FROM authority_record_identity
                 ORDER BY key_digest, event_payload_hash",
            )
            .unwrap();
        let identities = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let records = identities
            .iter()
            .map(|(key_digest, record_hash, _)| {
                serde_json::json!({
                    "keyDigest": key_digest,
                    "recordHash": format!("sha256:{record_hash}"),
                })
            })
            .collect::<Vec<_>>();
        let events = identities
            .iter()
            .filter_map(|(key_digest, _, payload_hash)| {
                payload_hash.as_ref().map(|payload_hash| {
                    serde_json::json!({
                        "eventId": format!("evt:sha256:{key_digest}"),
                        "payloadHash": format!("sha256:{payload_hash}"),
                    })
                })
            })
            .collect::<Vec<_>>();
        AuthorityCursorV2 {
            schema: AUTHORITY_CURSOR_SCHEMA_V2.to_owned(),
            journal_record_count: identities.len() as u64,
            event_count: events.len() as u64,
            journal_record_set_hash: sha256_json_prefixed(&serde_json::json!({
                "schema": "pointbreak.journal-record-set.v1",
                "records": records,
            }))
            .unwrap(),
            event_set_hash: sha256_json_prefixed(&serde_json::json!({
                "schema": "shore.event-set.v1",
                "events": events,
            }))
            .unwrap(),
            capability_set_hash,
        }
    }

    fn event(index: usize) -> ShoreEvent {
        let journal_id = JournalId::new(format!("journal:governed:{index}"));
        ShoreEvent::new(
            EventType::ReviewInitialized,
            ReviewInitializedPayload::idempotency_key(&journal_id),
            EventTarget::for_journal(journal_id),
            Writer::shore_local("test"),
            ReviewInitializedPayload {},
            format!("2026-07-29T00:00:{index:02}Z"),
        )
        .unwrap()
    }

    fn signed_event(index: usize, signature_byte: char) -> ShoreEvent {
        let mut event = event(index);
        event.signer = Some(
            SignerId::parse("did:key:z6MkehRgf7yJbgaGfYsdoAsKdBPE3dj2CYhowQdcjqSJgvVd").unwrap(),
        );
        event.signature =
            Some(EventSignature::new_ed25519_v1(signature_byte.to_string().repeat(86)).unwrap());
        event
    }
}
