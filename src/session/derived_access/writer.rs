//! Governed product writes for an active derived-access generation.

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::cursor::AppendResolution;
use super::lifecycle::DerivedAccessLifecycle;
use super::sqlite::{AppendCrashPoint, StoreWriterLock};
use crate::error::{Result, ShoreError};
use crate::session::EventWriteOutcome;
use crate::session::event::ShoreEvent;

const MAX_DIAGNOSTICS: usize = 8;
const MAX_DIAGNOSTIC_MESSAGE_BYTES: usize = 512;
static ATTEMPT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DerivedWriteDiagnostic {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

#[derive(Debug)]
pub(crate) struct DerivedWriteCoordinator {
    lifecycle: DerivedAccessLifecycle,
    degraded: AtomicBool,
    diagnostics: Mutex<VecDeque<DerivedWriteDiagnostic>>,
}

impl DerivedWriteCoordinator {
    /// Admit the active generation with one exact authoritative-head audit.
    pub(crate) fn new(lifecycle: DerivedAccessLifecycle) -> Result<Self> {
        match lifecycle.admit_writer() {
            Ok(true) => Ok(Self {
                lifecycle,
                degraded: AtomicBool::new(false),
                diagnostics: Mutex::new(VecDeque::new()),
            }),
            Ok(false) => Err(ShoreError::Message(
                "active derived access has no current generation; rebuild before writing"
                    .to_owned(),
            )),
            Err(error) => Err(ShoreError::Message(error.to_string())),
        }
    }

    pub(crate) fn record_event_once(
        &self,
        event: &ShoreEvent,
        publish: impl FnOnce() -> Result<EventWriteOutcome>,
    ) -> Result<EventWriteOutcome> {
        if self.degraded.load(Ordering::Acquire) {
            return publish();
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
        let writer_lock = StoreWriterLock::acquire(self.lifecycle.store_root())
            .map_err(|error| ShoreError::Message(error.to_string()))?;
        let current = self
            .lifecycle
            .open_current_for_write_locked(&writer_lock)
            .map_err(|error| ShoreError::Message(error.to_string()))?
            .ok_or_else(|| {
                ShoreError::Message(
                    "active derived access has no current generation; rebuild before writing"
                        .to_owned(),
                )
            })?;
        let publication = Cell::new(None);
        let attempt_token = next_attempt_token(event);
        let append = current.service().append_event_with_publisher_locked(
            event,
            &attempt_token,
            &writer_lock,
            hook,
            || {
                let outcome = publish()?;
                publication.set(Some(outcome));
                Ok(outcome)
            },
        );

        let resolution = match append {
            Ok(resolution) => resolution,
            Err(error) if publication.get() == Some(EventWriteOutcome::Created) => {
                drop(current);
                let diagnostic = self.degrade_locked(
                    "derived_access_receipt_finalization_failed",
                    &error.to_string(),
                    &writer_lock,
                );
                drop(writer_lock);
                self.degraded.store(true, Ordering::Release);
                self.push_diagnostic(diagnostic);
                return Ok(EventWriteOutcome::Created);
            }
            Err(error) => {
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

        let catch_up_lock = match StoreWriterLock::acquire(self.lifecycle.store_root()) {
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
                self.degraded.store(true, Ordering::Release);
                self.push_diagnostic(diagnostic);
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
            .quarantine_current_locked(detail, writer_lock)
            .map(|path| format!("derived state quarantined at {}", path.display()))
            .unwrap_or_else(|error| format!("derived quarantine also failed: {error}"));
        diagnostic(code, detail, &quarantine)
    }

    fn degrade(&self, code: &'static str, detail: &str) -> DerivedWriteDiagnostic {
        let quarantine = self
            .lifecycle
            .quarantine_current(detail)
            .map(|path| format!("derived state quarantined at {}", path.display()))
            .unwrap_or_else(|error| format!("derived quarantine also failed: {error}"));
        diagnostic(code, detail, &quarantine)
    }

    fn push_diagnostic(&self, diagnostic: DerivedWriteDiagnostic) {
        tracing::warn!(
            code = diagnostic.code,
            message = diagnostic.message,
            "derived_write_degraded_after_truth"
        );
        let mut diagnostics = self
            .diagnostics
            .lock()
            .expect("derived write diagnostic lock poisoned");
        if diagnostics.len() == MAX_DIAGNOSTICS {
            diagnostics.pop_front();
        }
        diagnostics.push_back(diagnostic);
    }

    fn record_catch_up_pending(&self, detail: &str) {
        let diagnostic = diagnostic(
            "derived_access_projection_catch_up_deferred",
            detail,
            "derived generation remains published as CatchingUp",
        );
        self.push_diagnostic(diagnostic);
    }
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

    use tempfile::TempDir;

    use super::{AppendCrashPoint, DerivedWriteCoordinator};
    use crate::bench_support::longitudinal::LongitudinalCountingScopeV1;
    use crate::crypto::SignerId;
    use crate::model::JournalId;
    use crate::session::derived_access::lifecycle::{
        DerivedAccessLifecycle, LifecycleControl, LifecycleError,
    };
    use crate::session::derived_access::product_contract::{
        DerivedAccessAvailability, DerivedAccessProfile,
    };
    use crate::session::derived_access::sqlite::{CursorLedgerIdentity, SqliteCursorLedger};
    use crate::session::event::{
        EventSignature, EventTarget, EventType, ReviewInitializedPayload, ShoreEvent, Writer,
    };
    use crate::session::store::bundle::import_store_bundle_into_with_verification;
    use crate::session::store::resolution::{
        event_store_for_explicit_target, opaque_path_identity,
    };
    use crate::session::{EventStore, EventVerificationPolicy, EventWriteOutcome, TrustSet};

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

        assert_eq!(
            governed.record_event_once(&event(1)).unwrap(),
            EventWriteOutcome::Created
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
    fn exact_admission_audits_directory_once_and_append_does_not_repeat_it() {
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
        assert_eq!(admission.snapshot().counters.directory_entries_walked, 3);

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
        let error =
            event_store_for_explicit_target(root.path(), DerivedAccessProfile::SqliteWalBodylessV1)
                .unwrap_err();
        assert!(error.to_string().contains("requires rebuild"));
        assert_eq!(
            EventStore::open(root.path()).list_events().unwrap().len(),
            2
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
    fn product_writers_serialize_across_processes() {
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
    #[ignore = "spawned by product_writers_serialize_across_processes"]
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
