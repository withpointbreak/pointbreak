//! Dormant derived-access lifecycle, rebuild, and immutable-generation manager.
#![cfg_attr(not(test), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::cursor::{TruthAuthoritySnapshot, TruthCursor};
use super::generation::{
    GenerationDescriptor, GenerationError, GenerationLayout, GenerationProgress,
    GenerationProgressPhase, GenerationPublication, GenerationReadLease,
};
use super::locator::LocatorRead;
use super::product_contract::{DerivedAccessAvailability, DerivedAccessProfile};
use super::semantic::change::{
    CHANGE_READER_PROFILE_RESOURCE_V3, ChangeReaderContractError,
    ChangeReaderProfileReceiptProbeV1, ChangeReaderProfileReceiptV3, ReaderProjectionCheckpointV1,
    RuntimeTrustIdentityV1, build_change_reader_profile_receipt_v3,
    change_generation_stamp_sha256_v1, derived_change_generation_stamp_v1,
    initial_reader_projection_checkpoint_v1, probe_change_reader_profile_receipt,
    strict_change_generation_stamp_preimage_v1,
};
use super::service::{
    BootstrapProjectionControl, DerivedAccessService, DerivedAccessServiceError,
    PublicationValidationSnapshot,
};
use super::sqlite::{
    BootstrapControl, BootstrapProgress, CursorLedgerError, CursorLedgerIdentity,
    SqliteCursorLedger, SqliteLocatorError, SqliteSemanticError, StoreWriterLock, WriterLockError,
};
use super::verification::strict_bodyless_materialized_snapshot_at;
#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::{
    LongitudinalDerivedAccessPhaseV1 as Phase, enter_derived_access_phase_v1,
};
use crate::canonical_hash::canonical_json_bytes;
use crate::documents::{
    INSPECT_READER_PROFILE_SCHEMA, ReaderProfileAvailabilityV1, ReaderProfileDocumentV1,
};
use crate::session::EventStore;
use crate::session::derived_access::QualificationLocalJournal;
use crate::session::store::authority_lock::StoreAuthorityLock;
use crate::session::store::backend::{
    JournalChangeCheck, JournalChangeStamp, JournalChangeVerdict, StoreBackend,
};
use crate::session::store::capabilities::{
    StoreCapabilityStatus, inspect_change_reader_journal_records,
    validate_bounded_change_capability_pair,
};

const STABLE_PUBLICATION_ATTEMPTS: usize = 8;
const BOOTSTRAP_PROJECTION_BATCH: usize = 512;
const CHANGE_ACTIVATION_LOGICAL_KEY: &str =
    "store_capability_activation:review_change_revision_v1:root";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LifecycleControl {
    Continue,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PublicationBoundary {
    StagingPrepared,
    CandidatePopulated,
    CandidateValidated,
    ReaderReceiptWritten,
    GenerationPromoted,
    CurrentPublished,
    PriorPublicationRetired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LifecycleProgress {
    pub(crate) phase: GenerationProgressPhase,
    pub(crate) completed: usize,
    pub(crate) total: usize,
    pub(crate) bytes_processed: u64,
    pub(crate) elapsed_ms: u64,
    pub(crate) estimated_remaining_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LifecycleStatus {
    pub(crate) availability: DerivedAccessAvailability,
    pub(crate) generation_id: Option<String>,
    pub(crate) phase: Option<GenerationProgressPhase>,
    pub(crate) completed: Option<usize>,
    pub(crate) total: Option<usize>,
    pub(crate) bytes_processed: Option<u64>,
    pub(crate) elapsed_ms: Option<u64>,
    pub(crate) estimated_remaining_ms: Option<u64>,
    pub(crate) detail: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LifecycleReceipt {
    pub(crate) availability: DerivedAccessAvailability,
    pub(crate) generation_id: Option<String>,
    pub(crate) head_sequence: u64,
    pub(crate) semantic_receipt: Option<String>,
    pub(crate) reclaimed_generation_count: usize,
    pub(crate) retained_reader_generation_count: usize,
    pub(crate) reclaim_detail: Option<String>,
}

#[derive(Debug)]
pub(crate) struct CurrentGeneration {
    publication_identity: GenerationPublication,
    service: DerivedAccessService,
    descriptor: GenerationDescriptor,
    reader_receipt: Option<ChangeReaderProfileReceiptV3>,
    authority_head: TruthCursor,
    locator_applied: TruthCursor,
    authority_maintenance_pending: bool,
    _lease: GenerationReadLease,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CurrentGenerationValidation {
    pub(crate) authority: TruthAuthoritySnapshot,
    pub(crate) locator_applied: TruthCursor,
    pub(crate) authority_maintenance_pending: bool,
}

struct CurrentAuthorityValidation {
    snapshot: TruthAuthoritySnapshot,
    continuation_pending: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct DerivedAccessLifecycle {
    profile: DerivedAccessProfile,
    store_root: PathBuf,
    store_id: String,
    paths: GenerationLayout,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum LifecycleError {
    #[error("derived-access lifecycle is disabled")]
    Disabled,
    #[error("derived-access lifecycle was cancelled")]
    Cancelled,
    #[error("another derived-access rebuild is already running")]
    RebuildBusy,
    #[error("automatic derived rebuild stopped after Change capability activation")]
    AutomaticRebuildSuppressed,
    #[error("derived-access lifecycle requires rebuild: {0}")]
    RebuildRequired(String),
    #[error("derived-access lifecycle quarantined invalid state: {0}")]
    Quarantined(String),
    #[error("derived-access store identity is empty")]
    EmptyStoreIdentity,
    #[error("authoritative truth changed during derived rebuild")]
    TruthChanged,
    #[error("derived generation validation failed: {0}")]
    Validation(String),
    #[error(transparent)]
    Generation(#[from] GenerationError),
    #[error(transparent)]
    Cursor(#[from] CursorLedgerError),
    #[error(transparent)]
    Service(#[from] DerivedAccessServiceError),
    #[error(transparent)]
    WriterLock(#[from] WriterLockError),
    #[error("authoritative truth read failed: {0}")]
    Truth(String),
}

#[derive(Clone, Copy)]
enum RebuildExecution {
    Synchronous,
    Background { suppress_after_activation: bool },
}

impl RebuildExecution {
    fn is_background(self) -> bool {
        matches!(self, Self::Background { .. })
    }

    fn suppress_after_activation(self) -> bool {
        matches!(
            self,
            Self::Background {
                suppress_after_activation: true
            }
        )
    }
}

impl DerivedAccessLifecycle {
    pub(crate) fn new(
        profile: DerivedAccessProfile,
        store_root: &Path,
        store_id: impl Into<String>,
    ) -> Result<Self, LifecycleError> {
        let store_id = store_id.into();
        if store_id.trim().is_empty() {
            return Err(LifecycleError::EmptyStoreIdentity);
        }
        Ok(Self {
            profile,
            store_root: store_root.to_path_buf(),
            store_id,
            paths: GenerationLayout::new(store_root)?,
        })
    }

    pub(crate) fn paths(&self) -> &GenerationLayout {
        &self.paths
    }

    pub(crate) fn store_root(&self) -> &Path {
        &self.store_root
    }

    pub(crate) fn published_generation_id(&self) -> Result<Option<String>, LifecycleError> {
        self.paths
            .current_publication()
            .map(|publication| publication.map(|publication| publication.generation_id))
            .map_err(|error| self.generation_open_error(error))
    }

    /// Observe the completion-last current-publication record without opening
    /// or validating the referenced generation. Unlike lifecycle generation
    /// opening, an invalid record is reported without quarantine or mutation.
    pub(crate) fn published_generation_identity_read_only(
        &self,
    ) -> Result<Option<GenerationPublication>, LifecycleError> {
        self.paths
            .current_publication()
            .map_err(LifecycleError::Generation)
    }

    /// Metadata-only activation probe used to constrain automatic recovery.
    /// It never opens a carrier or enumerates the Journal.
    pub(crate) fn change_capability_activated(&self) -> Result<bool, LifecycleError> {
        StoreBackend::Local(self.store_root.clone())
            .journal()
            .record_exists(CHANGE_ACTIVATION_LOGICAL_KEY)
            .map_err(|error| LifecycleError::Truth(error.to_string()))
    }

    fn try_background_authority(
        &self,
        execution: RebuildExecution,
    ) -> Result<Option<StoreAuthorityLock>, LifecycleError> {
        if !execution.is_background() {
            return Ok(None);
        }
        let Some(authority) = StoreAuthorityLock::try_acquire(&self.store_root)
            .map_err(|error| LifecycleError::Truth(error.to_string()))?
        else {
            return Err(LifecycleError::RebuildBusy);
        };
        if execution.suppress_after_activation() && self.change_capability_activated()? {
            return Err(LifecycleError::AutomaticRebuildSuppressed);
        }
        Ok(Some(authority))
    }

    pub(crate) fn status(&self) -> Result<LifecycleStatus, LifecycleError> {
        self.status_with_quarantine(true)
    }

    pub(crate) fn status_read_only(&self) -> Result<LifecycleStatus, LifecycleError> {
        self.status_with_quarantine(false)
    }

    fn status_with_quarantine(
        &self,
        allow_quarantine: bool,
    ) -> Result<LifecycleStatus, LifecycleError> {
        if self.profile == DerivedAccessProfile::Off || !self.paths.root().exists() {
            return Ok(status(DerivedAccessAvailability::Absent, None, None));
        }
        let publication = match self.paths.current_publication() {
            Ok(publication) => publication,
            Err(error) => return self.generation_error_status(error, allow_quarantine),
        };
        let staging_progress = match self.staging_progress() {
            Ok(progress) => progress,
            Err(error) => {
                return Ok(status(
                    DerivedAccessAvailability::Unavailable,
                    None,
                    Some(error.to_string()),
                ));
            }
        };
        let Some(publication) = publication else {
            let availability = if staging_progress.is_some() {
                DerivedAccessAvailability::Bootstrapping
            } else if directory_has_entries(&self.paths.root().join("generations"))? {
                DerivedAccessAvailability::RebuildRequired
            } else {
                DerivedAccessAvailability::Absent
            };
            return Ok(LifecycleStatus {
                availability,
                generation_id: None,
                phase: staging_progress.map(|progress| progress.phase),
                completed: staging_progress.map(|progress| progress.completed),
                total: staging_progress.map(|progress| progress.total),
                bytes_processed: staging_progress.map(|progress| progress.bytes_processed),
                elapsed_ms: staging_progress.map(|progress| progress.elapsed_ms),
                estimated_remaining_ms: staging_progress
                    .and_then(|progress| progress.estimated_remaining_ms),
                detail: None,
            });
        };
        if let Some(progress) = staging_progress {
            return Ok(LifecycleStatus {
                availability: DerivedAccessAvailability::Bootstrapping,
                generation_id: Some(publication.generation_id),
                phase: Some(progress.phase),
                completed: Some(progress.completed),
                total: Some(progress.total),
                bytes_processed: Some(progress.bytes_processed),
                elapsed_ms: Some(progress.elapsed_ms),
                estimated_remaining_ms: progress.estimated_remaining_ms,
                detail: Some("current generation remains readable during rebuild".to_owned()),
            });
        }
        let stable_publication = match self.stable_current_publication() {
            Ok(publication) => publication,
            Err(error) => return self.generation_error_status(error, allow_quarantine),
        };
        let Some((paths, publication, _lease)) = stable_publication else {
            return Ok(status(
                DerivedAccessAvailability::RebuildRequired,
                None,
                Some("current generation changed while reading lifecycle status".to_owned()),
            ));
        };
        let descriptor = match paths.descriptor(&publication) {
            Ok(descriptor) => descriptor,
            Err(error) => return self.generation_error_status(error, allow_quarantine),
        };
        if let Err(error) = self.validate_descriptor(&descriptor) {
            return self.invalid_status(error.to_string(), allow_quarantine);
        }
        let generation_root = paths.generation(&publication.generation_id);
        if let Err(error) = validate_wal_shape(&generation_root) {
            return self.lifecycle_error_status(error, allow_quarantine);
        }
        let service = match DerivedAccessService::open_at(
            &self.store_root,
            &generation_root,
            CursorLedgerIdentity::new(self.store_id.clone()),
        ) {
            Ok(service) => service,
            Err(error) if service_error_requires_rebuild(&error) => {
                return Ok(status(
                    DerivedAccessAvailability::RebuildRequired,
                    Some(publication.generation_id),
                    Some(error.to_string()),
                ));
            }
            Err(error) if service_error_requires_quarantine(&error) => {
                return self.invalid_status(error.to_string(), allow_quarantine);
            }
            Err(error) => {
                return Ok(status(
                    DerivedAccessAvailability::Unavailable,
                    Some(publication.generation_id),
                    Some(error.to_string()),
                ));
            }
        };
        let publication_snapshot = match service.publication_validation_snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) if service_error_requires_rebuild(&error) => {
                return Ok(status(
                    DerivedAccessAvailability::RebuildRequired,
                    Some(publication.generation_id),
                    Some(error.to_string()),
                ));
            }
            Err(error) if service_error_requires_quarantine(&error) => {
                return self.invalid_status(error.to_string(), allow_quarantine);
            }
            Err(error) => {
                return Ok(status(
                    DerivedAccessAvailability::Unavailable,
                    Some(publication.generation_id),
                    Some(error.to_string()),
                ));
            }
        };
        if let Err(error) = self.validate_change_reader_publication(
            &generation_root,
            &descriptor,
            &publication_snapshot,
        ) {
            return match error {
                LifecycleError::RebuildRequired(detail) => Ok(status(
                    DerivedAccessAvailability::RebuildRequired,
                    Some(publication.generation_id),
                    Some(detail),
                )),
                LifecycleError::Validation(detail) => self.invalid_status(detail, allow_quarantine),
                error => Ok(status(
                    DerivedAccessAvailability::Unavailable,
                    Some(publication.generation_id),
                    Some(error.to_string()),
                )),
            };
        }
        let authority =
            match self.observe_current_authority_snapshot(publication_snapshot.authority.clone()) {
                Ok(authority) => authority.snapshot,
                Err(LifecycleError::RebuildRequired(detail)) => {
                    return Ok(status(
                        DerivedAccessAvailability::RebuildRequired,
                        Some(publication.generation_id),
                        Some(detail),
                    ));
                }
                Err(error) => {
                    return Ok(status(
                        DerivedAccessAvailability::Unavailable,
                        Some(publication.generation_id),
                        Some(error.to_string()),
                    ));
                }
            };
        if let Err(error) = validate_published(
            &descriptor,
            &authority,
            publication_snapshot.locator_applied,
        ) {
            return self.invalid_status(error.to_string(), allow_quarantine);
        }
        if publication_snapshot.locator_applied != authority.head.cursor {
            return Ok(status(
                DerivedAccessAvailability::CatchingUp,
                Some(publication.generation_id),
                Some("derived projections are catching up to authoritative truth".to_owned()),
            ));
        }
        Ok(status(
            DerivedAccessAvailability::Current,
            Some(publication.generation_id),
            None,
        ))
    }

    pub(crate) fn rebuild(
        &self,
        progress: impl FnMut(LifecycleProgress) -> LifecycleControl,
    ) -> Result<LifecycleReceipt, LifecycleError> {
        self.rebuild_with_hook(progress, |_| {})
    }

    pub(crate) fn try_automatic_legacy_rebuild(
        &self,
        progress: impl FnMut(LifecycleProgress) -> LifecycleControl,
    ) -> Result<LifecycleReceipt, LifecycleError> {
        self.rebuild_with_execution(
            BOOTSTRAP_PROJECTION_BATCH,
            progress,
            |_| {},
            RebuildExecution::Background {
                suppress_after_activation: true,
            },
        )
    }

    pub(crate) fn try_explicit_background_rebuild(
        &self,
        progress: impl FnMut(LifecycleProgress) -> LifecycleControl,
    ) -> Result<LifecycleReceipt, LifecycleError> {
        self.rebuild_with_execution(
            BOOTSTRAP_PROJECTION_BATCH,
            progress,
            |_| {},
            RebuildExecution::Background {
                suppress_after_activation: false,
            },
        )
    }

    pub(crate) fn rebuild_required_while_writer_idle(&self) -> Result<bool, LifecycleError> {
        // A governed append publishes loose truth before finalizing its cursor
        // receipt. `RebuildRequired` during that bounded gap is expected. The
        // writer lock makes this second observation authoritative: if the gap
        // has closed, the background worker must not start a full rebuild.
        let _writer_lock = match StoreWriterLock::try_acquire(&self.store_root) {
            Ok(writer_lock) => writer_lock,
            Err(WriterLockError::Busy) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        Ok(self.status()?.availability == DerivedAccessAvailability::RebuildRequired)
    }

    pub(crate) fn rebuild_with_hook(
        &self,
        progress: impl FnMut(LifecycleProgress) -> LifecycleControl,
        hook: impl FnMut(PublicationBoundary),
    ) -> Result<LifecycleReceipt, LifecycleError> {
        self.rebuild_with_execution(
            BOOTSTRAP_PROJECTION_BATCH,
            progress,
            hook,
            RebuildExecution::Synchronous,
        )
    }

    fn rebuild_with_hook_and_batch_limit(
        &self,
        bootstrap_batch_limit: usize,
        progress: impl FnMut(LifecycleProgress) -> LifecycleControl,
        hook: impl FnMut(PublicationBoundary),
    ) -> Result<LifecycleReceipt, LifecycleError> {
        self.rebuild_with_execution(
            bootstrap_batch_limit,
            progress,
            hook,
            RebuildExecution::Synchronous,
        )
    }

    fn rebuild_with_execution(
        &self,
        bootstrap_batch_limit: usize,
        mut progress: impl FnMut(LifecycleProgress) -> LifecycleControl,
        mut hook: impl FnMut(PublicationBoundary),
        execution: RebuildExecution,
    ) -> Result<LifecycleReceipt, LifecycleError> {
        if self.profile == DerivedAccessProfile::Off {
            return Err(LifecycleError::Disabled);
        }
        // Every rebuild acquires authority before the rebuild lease and holds
        // it through publication. Ordinary synchronous callers remain
        // fail-fast under contention; migration's required post-activation
        // build nests the authority guard it already owns and may wait for a
        // background lease that will yield on its next authority try.
        let synchronous_authority_lock = if matches!(execution, RebuildExecution::Synchronous) {
            match StoreAuthorityLock::try_acquire(&self.store_root)
                .map_err(|error| LifecycleError::Truth(error.to_string()))?
            {
                Some(authority) => Some(authority),
                None => return Err(LifecycleError::RebuildBusy),
            }
        } else {
            None
        };
        let mut background_authority_lock = self.try_background_authority(execution)?;
        let lease = if synchronous_authority_lock
            .as_ref()
            .is_some_and(StoreAuthorityLock::is_reentrant)
            && !self.paths.rebuild_lease_held_by_current_thread()
        {
            self.paths.acquire_rebuild_lease()
        } else {
            self.paths.try_rebuild_lease()
        };
        let rebuild_lease = match lease {
            Ok(lease) => lease,
            Err(GenerationError::RebuildBusy) => return Err(LifecycleError::RebuildBusy),
            Err(error) => return Err(error.into()),
        };
        self.paths.ensure_scaffold()?;
        self.paths.discard_all_staging()?;
        let (sequence, generation_id) = self.paths.next_generation()?;
        let staging = self.paths.staging(&generation_id);
        hook(PublicationBoundary::StagingPrepared);
        let rebuild_started = Instant::now();

        let candidate_result: Result<_, LifecycleError> = (|| {
            #[cfg(any(test, feature = "longitudinal-counting"))]
            let population_phase = enter_derived_access_phase_v1(Phase::BootstrapPopulation);
            let cursor_phase_started = Instant::now();
            let mut progress_error = None;
            let mut report_bootstrap = |update: BootstrapProgress| {
                let update = lifecycle_progress(
                    GenerationProgressPhase::CursorPopulation,
                    update.completed,
                    update.total,
                    update.bytes_processed,
                    rebuild_started,
                    cursor_phase_started,
                );
                let control = match record_and_report_progress(
                    &self.paths,
                    &generation_id,
                    update,
                    &mut progress,
                ) {
                    Ok(control) => control,
                    Err(error) => {
                        progress_error = Some(error);
                        return BootstrapControl::Cancel;
                    }
                };
                match control {
                    LifecycleControl::Continue => BootstrapControl::Continue,
                    LifecycleControl::Cancel => BootstrapControl::Cancel,
                }
            };
            let bootstrap = if execution.is_background() {
                let writer_lock = match StoreWriterLock::try_acquire(&self.store_root) {
                    Ok(writer_lock) => writer_lock,
                    Err(WriterLockError::Busy) => return Err(LifecycleError::RebuildBusy),
                    Err(error) => return Err(error.into()),
                };
                SqliteCursorLedger::bootstrap_population_from_truth_at_locked_with_hook(
                    &self.store_root,
                    &staging,
                    CursorLedgerIdentity::new(self.store_id.clone()),
                    sequence,
                    &writer_lock,
                    &mut report_bootstrap,
                    |_| {},
                )
            } else {
                SqliteCursorLedger::bootstrap_population_from_truth_at_with_hook(
                    &self.store_root,
                    &staging,
                    CursorLedgerIdentity::new(self.store_id.clone()),
                    sequence,
                    &mut report_bootstrap,
                    |_| {},
                )
            };
            drop(background_authority_lock.take());
            if let Some(error) = progress_error {
                return Err(error);
            }
            if matches!(bootstrap, Err(CursorLedgerError::BootstrapCancelled)) {
                return Err(LifecycleError::Cancelled);
            }
            let bootstrap = bootstrap?;

            let service = DerivedAccessService::open_writable_at(
                &self.store_root,
                &staging,
                CursorLedgerIdentity::new(self.store_id.clone()),
            )?;
            let population_total = bootstrap.entries.len();
            let population_bytes = bootstrap.entries.iter().fold(0_u64, |total, entry| {
                total.saturating_add(entry.carrier_bytes)
            });
            let projection_phase_started = Instant::now();
            let initial_projection = lifecycle_progress(
                GenerationProgressPhase::ProjectionPopulation,
                0,
                population_total,
                0,
                rebuild_started,
                projection_phase_started,
            );
            if record_and_report_progress(
                &self.paths,
                &generation_id,
                initial_projection,
                &mut progress,
            )? == LifecycleControl::Cancel
            {
                return Err(LifecycleError::Cancelled);
            }
            let mut projection_progress_error = None;
            let population_result = service.populate_bootstrap_with_hook(
                &bootstrap.entries,
                bootstrap_batch_limit,
                |projection| {
                    let update = lifecycle_progress(
                        GenerationProgressPhase::ProjectionPopulation,
                        projection.completed,
                        projection.total,
                        projection.bytes_processed,
                        rebuild_started,
                        projection_phase_started,
                    );
                    match record_and_report_progress(
                        &self.paths,
                        &generation_id,
                        update,
                        &mut progress,
                    ) {
                        Ok(LifecycleControl::Continue) => BootstrapProjectionControl::Continue,
                        Ok(LifecycleControl::Cancel) => BootstrapProjectionControl::Cancel,
                        Err(error) => {
                            projection_progress_error = Some(error);
                            BootstrapProjectionControl::Cancel
                        }
                    }
                },
            );
            if let Some(error) = projection_progress_error {
                return Err(error);
            }
            if matches!(
                population_result,
                Err(DerivedAccessServiceError::BootstrapCancelled)
            ) {
                return Err(LifecycleError::Cancelled);
            }
            population_result?;
            hook(PublicationBoundary::CandidatePopulated);
            let authority = service.truth_authority_snapshot()?;
            let head = authority.head.cursor;
            let bootstrap_stamp = authority.change_stamp.clone();
            drop(bootstrap);
            #[cfg(any(test, feature = "longitudinal-counting"))]
            drop(population_phase);

            // Background rebuilds never wait for authoritative mutation while
            // owning the rebuild lease. This second guard linearizes strict
            // replay and publication against capability activation; the long
            // semantic projection above intentionally runs without it.
            background_authority_lock = self.try_background_authority(execution)?;

            #[cfg(any(test, feature = "longitudinal-counting"))]
            let oracle_phase = enter_derived_access_phase_v1(Phase::BootstrapOracle);
            let strict_phase_started = Instant::now();
            let strict_start = lifecycle_progress(
                GenerationProgressPhase::StrictVerification,
                0,
                population_total,
                0,
                rebuild_started,
                strict_phase_started,
            );
            if record_and_report_progress(&self.paths, &generation_id, strict_start, &mut progress)?
                == LifecycleControl::Cancel
            {
                return Err(LifecycleError::Cancelled);
            }
            let strict_events = self.truth_events()?;
            let authority_backend = StoreBackend::Local(self.store_root.clone());
            let authority_journal = authority_backend.journal();
            let change_inspection = authority_journal
                .record_exists(CHANGE_ACTIVATION_LOGICAL_KEY)
                .map_err(|error| LifecycleError::Truth(error.to_string()))?
                .then(|| {
                    inspect_change_reader_journal_records(authority_journal.as_ref())
                        .map_err(|error| LifecycleError::Truth(error.to_string()))
                })
                .transpose()?;
            #[cfg(any(test, feature = "longitudinal-counting"))]
            let strict_event_ownership =
                crate::bench_support::longitudinal::RetainedDecodedEventsGuardV1::new(
                    strict_events.len(),
                );
            let semantic_snapshot = validate_candidate(
                &service,
                &GenerationDescriptor::new(
                    &generation_id,
                    &self.store_id,
                    self.profile,
                    sequence,
                    head.sequence,
                    authority.change_stamp.clone(),
                    "",
                ),
                strict_events.clone(),
            )?;
            let compact_change_projection =
                match service.semantic_materialized_change_projection()? {
                    LocatorRead::Ready(projection) => projection,
                    LocatorRead::CatchUpRequired { applied, observed } => {
                        return Err(LifecycleError::Validation(format!(
                            "candidate Change projection remained behind after rebuild: \
                             {applied:?} != {observed:?}"
                        )));
                    }
                };
            if compact_change_projection.as_of != head
                || compact_change_projection.projection != semantic_snapshot.changes
            {
                return Err(LifecycleError::Validation(
                    "materialized Change projection differs from the candidate snapshot".to_owned(),
                ));
            }
            let strict_complete = lifecycle_progress(
                GenerationProgressPhase::StrictVerification,
                population_total,
                population_total,
                population_bytes,
                rebuild_started,
                strict_phase_started,
            );
            if record_and_report_progress(
                &self.paths,
                &generation_id,
                strict_complete,
                &mut progress,
            )? == LifecycleControl::Cancel
            {
                return Err(LifecycleError::Cancelled);
            }
            #[cfg(any(test, feature = "longitudinal-counting"))]
            drop(strict_event_ownership);
            #[cfg(any(test, feature = "longitudinal-counting"))]
            drop(oracle_phase);
            hook(PublicationBoundary::CandidateValidated);
            Ok((
                service,
                head,
                semantic_snapshot,
                compact_change_projection,
                strict_events,
                change_inspection,
                bootstrap_stamp,
            ))
        })();
        let (
            service,
            head,
            semantic_snapshot,
            compact_change_projection,
            strict_events,
            change_inspection,
            bootstrap_stamp,
        ) = match candidate_result {
            Ok(candidate) => candidate,
            Err(error) => {
                self.paths.discard_staging(&generation_id)?;
                return Err(error);
            }
        };

        #[cfg(any(test, feature = "longitudinal-counting"))]
        let finalization_phase = enter_derived_access_phase_v1(Phase::BootstrapFinalization);
        let finalizing_phase_started = Instant::now();
        let finalizing_start = lifecycle_progress(
            GenerationProgressPhase::Finalizing,
            0,
            1,
            0,
            rebuild_started,
            finalizing_phase_started,
        );
        match record_and_report_progress(
            &self.paths,
            &generation_id,
            finalizing_start,
            &mut progress,
        ) {
            Ok(LifecycleControl::Continue) => {}
            Ok(LifecycleControl::Cancel) => {
                self.paths.discard_staging(&generation_id)?;
                return Err(LifecycleError::Cancelled);
            }
            Err(error) => {
                self.paths.discard_staging(&generation_id)?;
                return Err(error);
            }
        }

        let writer_lock = if execution.is_background() {
            match StoreWriterLock::try_acquire(&self.store_root) {
                Ok(writer_lock) => writer_lock,
                Err(WriterLockError::Busy) => {
                    self.paths.discard_staging(&generation_id)?;
                    return Err(LifecycleError::RebuildBusy);
                }
                Err(error) => {
                    self.paths.discard_staging(&generation_id)?;
                    return Err(error.into());
                }
            }
        } else {
            match StoreWriterLock::acquire(&self.store_root) {
                Ok(writer_lock) => writer_lock,
                Err(error) => {
                    self.paths.discard_staging(&generation_id)?;
                    return Err(error.into());
                }
            }
        };
        let authority_check = QualificationLocalJournal::new(&self.store_root)
            .changes_since(&bootstrap_stamp)
            .map_err(|error| LifecycleError::Truth(error.to_string()))?;
        if authority_check.verdict != JournalChangeVerdict::Stable {
            self.paths.discard_staging(&generation_id)?;
            return Err(LifecycleError::TruthChanged);
        }
        if let Err(error) =
            service.bind_truth_authority_stamp_locked(head, &authority_check.after, &writer_lock)
        {
            self.paths.discard_staging(&generation_id)?;
            return Err(error.into());
        }
        let semantic_receipt_result = (|| -> Result<String, LifecycleError> {
            let Some(change_inspection) = change_inspection.as_ref() else {
                return Ok(semantic_snapshot.semantic_receipt.clone());
            };
            match &change_inspection.status {
                StoreCapabilityStatus::Ready {
                    activation_id,
                    manifest_hash,
                    completion_id,
                } => {
                    let completion_logical_key =
                        format!("bulk_adoption_completion:{completion_id}");
                    let backend = StoreBackend::Local(self.store_root.clone());
                    let pair = validate_bounded_change_capability_pair(
                        backend.journal().as_ref(),
                        CHANGE_ACTIVATION_LOGICAL_KEY,
                        &completion_logical_key,
                    )
                    .map_err(|error| LifecycleError::Truth(error.to_string()))?;
                    if pair.activation_id != *activation_id
                        || pair.manifest_hash != *manifest_hash
                        || pair.completion_id != *completion_id
                    {
                        return Err(LifecycleError::TruthChanged);
                    }
                    let receipt = build_change_reader_profile_receipt_v3(
                        &generation_id,
                        &self.store_id,
                        change_inspection,
                        pair.activation_record_sha256,
                        pair.completion_record_sha256,
                        authority_check.after.clone(),
                        head,
                        &strict_events,
                        &semantic_snapshot,
                        &compact_change_projection.document_projection,
                    )
                    .map_err(|error| LifecycleError::Validation(error.to_string()))?;
                    let checkpoint = initial_reader_projection_checkpoint_v1(&receipt)
                        .map_err(|error| LifecycleError::Validation(error.to_string()))?;
                    service.seed_change_reader_publication(
                        &change_inspection.authority_record_identities,
                        &checkpoint,
                    )?;
                    let receipt_bytes = canonical_json_bytes(
                        &serde_json::to_value(&receipt)
                            .map_err(|error| LifecycleError::Validation(error.to_string()))?,
                    )
                    .map_err(|error| LifecycleError::Validation(error.to_string()))?;
                    self.paths.write_resource(
                        &staging,
                        CHANGE_READER_PROFILE_RESOURCE_V3,
                        &receipt_bytes,
                    )?;
                    hook(PublicationBoundary::ReaderReceiptWritten);
                    Ok(receipt.receipt_sha256)
                }
                StoreCapabilityStatus::MigrationRequired => {
                    Ok(semantic_snapshot.semantic_receipt.clone())
                }
                StoreCapabilityStatus::MigrationInProgress { .. } => {
                    Err(LifecycleError::RebuildRequired(
                        "partial Change capability authority cannot publish a generation"
                            .to_owned(),
                    ))
                }
            }
        })();
        let semantic_receipt = match semantic_receipt_result {
            Ok(receipt) => receipt,
            Err(error) => {
                self.paths.discard_staging(&generation_id)?;
                return Err(error);
            }
        };
        let descriptor = GenerationDescriptor::new(
            &generation_id,
            &self.store_id,
            self.profile,
            head.epoch,
            head.sequence,
            authority_check.after,
            &semantic_receipt,
        );
        let descriptor_sha256 = match self.paths.write_descriptor(&staging, &descriptor) {
            Ok(sha256) => sha256,
            Err(error) => {
                self.paths.discard_staging(&generation_id)?;
                return Err(error.into());
            }
        };
        let finalizing_complete = lifecycle_progress(
            GenerationProgressPhase::Finalizing,
            1,
            1,
            0,
            rebuild_started,
            finalizing_phase_started,
        );
        match record_and_report_progress(
            &self.paths,
            &generation_id,
            finalizing_complete,
            &mut progress,
        ) {
            Ok(LifecycleControl::Continue) => {}
            Ok(LifecycleControl::Cancel) => {
                self.paths.discard_staging(&generation_id)?;
                return Err(LifecycleError::Cancelled);
            }
            Err(error) => {
                self.paths.discard_staging(&generation_id)?;
                return Err(error);
            }
        }
        self.paths.clear_progress(&generation_id)?;
        drop(service);
        if let Err(error) = self.paths.promote_staging(&generation_id) {
            self.paths.discard_staging(&generation_id)?;
            return Err(error.into());
        }
        hook(PublicationBoundary::GenerationPromoted);
        self.paths.publish(&GenerationPublication::new(
            sequence,
            &generation_id,
            descriptor_sha256,
        ))?;
        hook(PublicationBoundary::CurrentPublished);
        self.paths.retire_prior_publications(sequence)?;
        hook(PublicationBoundary::PriorPublicationRetired);
        let (reclaimed_generation_count, retained_reader_generation_count, reclaim_detail) =
            match self.paths.reclaim_inactive_generations(&generation_id) {
                Ok(reclaim) => (
                    reclaim.reclaimed.len(),
                    reclaim.retained_by_readers.len(),
                    None,
                ),
                Err(error) => (0, 0, Some(error.to_string())),
            };
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(finalization_phase);
        drop(writer_lock);
        drop(rebuild_lease);
        drop(background_authority_lock);
        drop(synchronous_authority_lock);

        Ok(LifecycleReceipt {
            availability: DerivedAccessAvailability::Current,
            generation_id: Some(generation_id),
            head_sequence: head.sequence,
            semantic_receipt: Some(semantic_receipt),
            reclaimed_generation_count,
            retained_reader_generation_count,
            reclaim_detail,
        })
    }

    pub(crate) fn open_current(&self) -> Result<Option<CurrentGeneration>, LifecycleError> {
        if self.profile == DerivedAccessProfile::Off {
            return Ok(None);
        }
        let Some((paths, publication, lease)) = self
            .stable_current_publication()
            .map_err(|error| self.generation_open_error(error))?
        else {
            return Ok(None);
        };
        let descriptor = paths
            .descriptor(&publication)
            .map_err(|error| self.generation_open_error(error))?;
        self.validate_descriptor(&descriptor)
            .map_err(|error| self.quarantine_error(error.to_string()))?;
        let generation_root = paths.generation(&publication.generation_id);
        if let Err(error) = validate_wal_shape(&generation_root) {
            return Err(if lifecycle_error_requires_quarantine(&error) {
                self.quarantine_error(error.to_string())
            } else {
                error
            });
        }
        let service = DerivedAccessService::open_at(
            &self.store_root,
            &generation_root,
            CursorLedgerIdentity::new(self.store_id.clone()),
        )
        .map_err(|error| {
            if service_error_requires_rebuild(&error) {
                LifecycleError::RebuildRequired(error.to_string())
            } else if service_error_requires_quarantine(&error) {
                self.quarantine_error(error.to_string())
            } else {
                LifecycleError::Service(error)
            }
        })?;
        let publication_snapshot = service.publication_validation_snapshot().map_err(|error| {
            if service_error_requires_rebuild(&error) {
                LifecycleError::RebuildRequired(error.to_string())
            } else if service_error_requires_quarantine(&error) {
                self.quarantine_error(error.to_string())
            } else {
                LifecycleError::Service(error)
            }
        })?;
        let reader_receipt = self
            .validate_change_reader_publication(
                &generation_root,
                &descriptor,
                &publication_snapshot,
            )
            .map_err(|error| match error {
                LifecycleError::Validation(detail) => LifecycleError::Quarantined(detail),
                error => error,
            })?;
        let authority =
            self.observe_current_authority_snapshot(publication_snapshot.authority.clone())?;
        validate_published(
            &descriptor,
            &authority.snapshot,
            publication_snapshot.locator_applied,
        )
        .map_err(|error| self.quarantine_error(error.to_string()))?;
        Ok(Some(CurrentGeneration {
            publication_identity: publication,
            service,
            descriptor,
            reader_receipt,
            authority_head: authority.snapshot.head.cursor,
            locator_applied: publication_snapshot.locator_applied,
            authority_maintenance_pending: authority.continuation_pending,
            _lease: lease,
        }))
    }

    fn stable_current_publication(
        &self,
    ) -> Result<
        Option<(GenerationLayout, GenerationPublication, GenerationReadLease)>,
        GenerationError,
    > {
        self.stable_current_publication_with_hook(|| {})
    }

    fn stable_current_publication_with_hook(
        &self,
        mut after_publication_selected: impl FnMut(),
    ) -> Result<
        Option<(GenerationLayout, GenerationPublication, GenerationReadLease)>,
        GenerationError,
    > {
        for _ in 0..STABLE_PUBLICATION_ATTEMPTS {
            // Namespace selection and generation lease acquisition form one
            // optimistic read transaction. A transition can win after the
            // first resolution but before this reader obtains its lease. In
            // that case the second resolution observes a different root, the
            // stale lease is dropped, and the reader retries against the
            // published namespace instead of waking against a renamed path.
            let paths = GenerationLayout::new(&self.store_root)?;
            let Some(publication) = paths.current_publication()? else {
                return Ok(None);
            };
            after_publication_selected();
            let lease = paths.acquire_read_lease(&publication.generation_id)?;
            let confirmed = GenerationLayout::new(&self.store_root)?;
            if confirmed.root() == paths.root()
                && confirmed.current_publication()?.as_ref() == Some(&publication)
            {
                return Ok(Some((confirmed, publication, lease)));
            }
        }
        Err(GenerationError::PublicationUnstable)
    }

    /// Open the generation selected while the caller holds the canonical writer
    /// lock. The coordinator has already performed the exact truth-count audit at
    /// construction; this path validates the publication and its internal cursor
    /// coverage without repeating a history-proportional directory walk for every
    /// append.
    pub(crate) fn open_current_for_write_locked(
        &self,
        _writer_lock: &StoreWriterLock,
    ) -> Result<Option<CurrentGeneration>, LifecycleError> {
        if self.profile == DerivedAccessProfile::Off || !self.paths.root().exists() {
            return Ok(None);
        }
        let Some(publication) = self
            .paths
            .current_publication()
            .map_err(|error| self.generation_open_error(error))?
        else {
            return Ok(None);
        };
        let lease = self
            .paths
            .acquire_read_lease(&publication.generation_id)
            .map_err(|error| self.generation_open_error(error))?;
        let descriptor = self
            .paths
            .descriptor(&publication)
            .map_err(|error| self.generation_open_error(error))?;
        self.validate_descriptor(&descriptor)
            .map_err(|error| LifecycleError::Validation(error.to_string()))?;
        let generation_root = self.paths.generation(&publication.generation_id);
        validate_wal_shape(&generation_root)?;
        let service = DerivedAccessService::open_writable_at(
            &self.store_root,
            &generation_root,
            CursorLedgerIdentity::new(self.store_id.clone()),
        )
        .map_err(|error| {
            if service_error_requires_rebuild(&error) {
                LifecycleError::RebuildRequired(error.to_string())
            } else {
                LifecycleError::Service(error)
            }
        })?;
        let publication_snapshot = service.publication_validation_snapshot()?;
        let reader_receipt = self.validate_change_reader_publication(
            &generation_root,
            &descriptor,
            &publication_snapshot,
        )?;
        let authority = self.persist_current_authority_snapshot_locked(
            &service,
            publication_snapshot.authority.clone(),
            _writer_lock,
        )?;
        validate_published(
            &descriptor,
            &authority,
            publication_snapshot.locator_applied,
        )?;
        Ok(Some(CurrentGeneration {
            publication_identity: publication,
            service,
            descriptor,
            reader_receipt,
            authority_head: authority.head.cursor,
            locator_applied: publication_snapshot.locator_applied,
            authority_maintenance_pending: false,
            _lease: lease,
        }))
    }

    /// Run mutable maintenance against the current generation without ever
    /// replacing it. A busy canonical writer is reported as pending so the
    /// process-local worker can retry without blocking a response.
    pub(crate) fn maintain_current_generation(&self) -> Result<bool, LifecycleError> {
        self.maintain_current_generation_with(|service| {
            service
                .catch_up_to_head(BOOTSTRAP_PROJECTION_BATCH)
                .map(|_| ())
                .map_err(LifecycleError::Service)
        })
    }

    fn maintain_current_generation_with(
        &self,
        catch_up: impl FnOnce(&DerivedAccessService) -> Result<(), LifecycleError>,
    ) -> Result<bool, LifecycleError> {
        let writer_lock = match StoreWriterLock::try_acquire(&self.store_root) {
            Ok(writer_lock) => writer_lock,
            Err(WriterLockError::Busy) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let Some(current) = self.open_current_for_write_locked(&writer_lock)? else {
            return Ok(true);
        };
        catch_up(current.service())?;
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn maintain_current_generation_with_interruption(
        &self,
        batch_limit: usize,
    ) -> Result<bool, LifecycleError> {
        self.maintain_current_generation_with(|service| {
            service
                .catch_up_with_interruption(batch_limit)
                .map(|_| ())
                .map_err(LifecycleError::Service)
        })
    }

    /// Admit a product writer against a stable current generation. The bounded
    /// authority continuation runs while the canonical writer lock excludes
    /// governed truth publication; no loose-directory census is repeated. Busy
    /// admission is reported immediately so the product writer can preserve
    /// authoritative availability through its degraded-loose mode.
    pub(crate) fn admit_writer(&self) -> Result<bool, LifecycleError> {
        let writer_lock = StoreWriterLock::try_acquire(&self.store_root)?;
        let Some(_current) = self.open_current_for_write_locked(&writer_lock)? else {
            return Ok(false);
        };
        Ok(true)
    }

    pub(crate) fn quarantine_current_locked(
        &self,
        reason: &str,
        _writer_lock: &StoreWriterLock,
    ) -> Result<PathBuf, LifecycleError> {
        Ok(self.paths.quarantine(reason)?)
    }

    pub(crate) fn quarantine_current(&self, reason: &str) -> Result<PathBuf, LifecycleError> {
        let writer_lock = StoreWriterLock::acquire(&self.store_root)?;
        self.quarantine_current_locked(reason, &writer_lock)
    }

    pub(crate) fn retire(&self) -> Result<Option<PathBuf>, LifecycleError> {
        if self.profile == DerivedAccessProfile::Off {
            return Ok(None);
        }
        let _writer_lock = StoreWriterLock::acquire(&self.store_root)?;
        Ok(self.paths.retire()?)
    }

    pub(crate) fn delete(&self) -> Result<(), LifecycleError> {
        if self.profile == DerivedAccessProfile::Off {
            return Ok(());
        }
        let _rebuild_lease = self.paths.try_rebuild_lease()?;
        let _writer_lock = StoreWriterLock::acquire(&self.store_root)?;
        self.paths.delete()?;
        Ok(())
    }

    pub(crate) fn purge_disposable_root(&self, path: &Path) -> Result<(), LifecycleError> {
        if self.profile == DerivedAccessProfile::Off {
            return Err(LifecycleError::Disabled);
        }
        self.paths.purge_disposable_root(path)?;
        Ok(())
    }

    fn validate_descriptor(&self, descriptor: &GenerationDescriptor) -> Result<(), LifecycleError> {
        if descriptor.store_id != self.store_id {
            return Err(LifecycleError::Validation(format!(
                "expected store {}, observed {}",
                self.store_id, descriptor.store_id
            )));
        }
        if descriptor.profile != self.profile {
            return Err(LifecycleError::Validation(format!(
                "expected profile {}, observed {}",
                self.profile.as_str(),
                descriptor.profile.as_str()
            )));
        }
        Ok(())
    }

    fn validate_change_reader_publication(
        &self,
        generation_root: &Path,
        descriptor: &GenerationDescriptor,
        snapshot: &PublicationValidationSnapshot,
    ) -> Result<Option<ChangeReaderProfileReceiptV3>, LifecycleError> {
        let receipt_path = generation_root.join(CHANGE_READER_PROFILE_RESOURCE_V3);
        let receipt_bytes = match std::fs::read(&receipt_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let backend = StoreBackend::Local(self.store_root.clone());
                let journal = backend.journal();
                let activation_exists = journal
                    .record_exists(CHANGE_ACTIVATION_LOGICAL_KEY)
                    .map_err(|error| LifecycleError::Truth(error.to_string()))?;
                if activation_exists {
                    return Err(LifecycleError::RebuildRequired(
                        "published Change generation has no V3 reader receipt".to_owned(),
                    ));
                }
                if snapshot.reader_projection_checkpoint.is_some() {
                    return Err(LifecycleError::RebuildRequired(
                        "legacy publication has an unanchored reader checkpoint".to_owned(),
                    ));
                }
                return Ok(None);
            }
            Err(error) => {
                return Err(LifecycleError::Validation(format!(
                    "could not read Change reader receipt {}: {error}",
                    receipt_path.display()
                )));
            }
        };
        let receipt = match probe_change_reader_profile_receipt(&receipt_bytes)
            .map_err(change_reader_contract_lifecycle_error)?
        {
            ChangeReaderProfileReceiptProbeV1::Current(receipt) => receipt,
            ChangeReaderProfileReceiptProbeV1::RebuildRequired { schema, version } => {
                return Err(LifecycleError::RebuildRequired(format!(
                    "published Change reader receipt requires upgrade: {schema:?} v{version:?}"
                )));
            }
        };
        receipt
            .validate_for_descriptor(descriptor)
            .map_err(change_reader_contract_lifecycle_error)?;
        validate_live_reader_checkpoint(snapshot, Some(&receipt))?;

        let backend = StoreBackend::Local(self.store_root.clone());
        let pair = validate_bounded_change_capability_pair(
            backend.journal().as_ref(),
            &receipt.publication_activation_carrier.logical_key,
            &receipt.publication_completion_carrier.logical_key,
        )
        .map_err(|error| LifecycleError::Validation(error.to_string()))?;
        if pair.activation_id != receipt.publication_activation_id
            || pair.manifest_hash != receipt.publication_manifest_hash
            || pair.completion_id != receipt.publication_completion_id
            || pair.minimum_reader_profile != receipt.minimum_reader_profile
            || pair.activation_record_sha256 != receipt.publication_activation_carrier.record_sha256
            || pair.completion_record_sha256 != receipt.publication_completion_carrier.record_sha256
        {
            return Err(LifecycleError::Validation(
                "bounded Change capability authority differs from the published receipt".to_owned(),
            ));
        }
        Ok(Some(*receipt))
    }

    fn truth_events(&self) -> Result<Vec<crate::session::event::ShoreEvent>, LifecycleError> {
        EventStore::open(&self.store_root)
            .list_events_untracked()
            .map_err(|error| LifecycleError::Truth(error.to_string()))
    }

    fn staging_progress(&self) -> Result<Option<LifecycleProgress>, LifecycleError> {
        Ok(self
            .paths
            .staging_progress()?
            .map(|progress| LifecycleProgress {
                phase: progress.phase,
                completed: progress.completed,
                total: progress.total,
                bytes_processed: progress.bytes_processed,
                elapsed_ms: progress.elapsed_ms,
                estimated_remaining_ms: progress.estimated_remaining_ms,
            }))
    }

    /// Revalidate one cached generation without mixing SQLite transactions.
    /// The immutable descriptor and V3 receipt were checked when the generation
    /// entered the cache; only their mutable checkpoint is re-read here.
    pub(crate) fn validate_cached_current(
        &self,
        current: &CurrentGeneration,
    ) -> Result<CurrentGenerationValidation, LifecycleError> {
        let publication_snapshot = current.service.publication_validation_snapshot()?;
        validate_live_reader_checkpoint(&publication_snapshot, current.reader_receipt.as_ref())?;
        let authority =
            self.observe_current_authority_snapshot(publication_snapshot.authority.clone())?;
        validate_published(
            &current.descriptor,
            &authority.snapshot,
            publication_snapshot.locator_applied,
        )?;
        Ok(CurrentGenerationValidation {
            authority: authority.snapshot,
            locator_applied: publication_snapshot.locator_applied,
            authority_maintenance_pending: authority.continuation_pending,
        })
    }

    /// Recheck only the bounded native authority interval behind one cached
    /// generation. This does not persist a stable cursor successor or acquire
    /// the derived writer lock; response paths use it to reject loose truth
    /// publication that could not advance the live SQLite checkpoint.
    pub(crate) fn cached_current_authority_is_stable(
        &self,
        current: &CurrentGeneration,
    ) -> Result<bool, LifecycleError> {
        let snapshot = current.service.truth_authority_snapshot()?;
        let check = QualificationLocalJournal::new(&self.store_root)
            .changes_since(&snapshot.change_stamp)
            .map_err(|error| LifecycleError::Truth(error.to_string()))?;
        Ok(check.verdict == JournalChangeVerdict::Stable)
    }

    fn observe_current_authority_snapshot(
        &self,
        snapshot: TruthAuthoritySnapshot,
    ) -> Result<CurrentAuthorityValidation, LifecycleError> {
        let journal = QualificationLocalJournal::new(&self.store_root);
        self.observe_current_authority_snapshot_with(snapshot, |before| {
            journal
                .changes_since(before)
                .map_err(|error| LifecycleError::Truth(error.to_string()))
        })
    }

    fn validate_current_authority_with(
        &self,
        service: &DerivedAccessService,
        changes_since: impl FnMut(&JournalChangeStamp) -> Result<JournalChangeCheck, LifecycleError>,
    ) -> Result<TruthAuthoritySnapshot, LifecycleError> {
        Ok(self
            .observe_current_authority_with(service, changes_since)?
            .snapshot)
    }

    fn observe_current_authority_with(
        &self,
        service: &DerivedAccessService,
        changes_since: impl FnMut(&JournalChangeStamp) -> Result<JournalChangeCheck, LifecycleError>,
    ) -> Result<CurrentAuthorityValidation, LifecycleError> {
        let snapshot = service.truth_authority_snapshot()?;
        self.observe_current_authority_snapshot_with(snapshot, changes_since)
    }

    fn observe_current_authority_snapshot_with(
        &self,
        snapshot: TruthAuthoritySnapshot,
        mut changes_since: impl FnMut(&JournalChangeStamp) -> Result<JournalChangeCheck, LifecycleError>,
    ) -> Result<CurrentAuthorityValidation, LifecycleError> {
        let check = changes_since(&snapshot.change_stamp)?;
        require_stable_authority(&check)?;
        let continuation_pending = check.after != snapshot.change_stamp;
        Ok(CurrentAuthorityValidation {
            snapshot: TruthAuthoritySnapshot {
                head: snapshot.head,
                change_stamp: check.after,
            },
            continuation_pending,
        })
    }

    fn persist_current_authority_snapshot_locked(
        &self,
        service: &DerivedAccessService,
        snapshot: TruthAuthoritySnapshot,
        writer_lock: &StoreWriterLock,
    ) -> Result<TruthAuthoritySnapshot, LifecycleError> {
        let journal = QualificationLocalJournal::new(&self.store_root);
        self.persist_current_authority_snapshot_with(service, snapshot, writer_lock, |before| {
            journal
                .changes_since(before)
                .map_err(|error| LifecycleError::Truth(error.to_string()))
        })
    }

    fn persist_current_authority_with(
        &self,
        service: &DerivedAccessService,
        writer_lock: &StoreWriterLock,
        changes_since: impl FnMut(&JournalChangeStamp) -> Result<JournalChangeCheck, LifecycleError>,
    ) -> Result<TruthAuthoritySnapshot, LifecycleError> {
        let snapshot = service.truth_authority_snapshot()?;
        self.persist_current_authority_snapshot_with(service, snapshot, writer_lock, changes_since)
    }

    fn persist_current_authority_snapshot_with(
        &self,
        service: &DerivedAccessService,
        snapshot: TruthAuthoritySnapshot,
        writer_lock: &StoreWriterLock,
        changes_since: impl FnMut(&JournalChangeStamp) -> Result<JournalChangeCheck, LifecycleError>,
    ) -> Result<TruthAuthoritySnapshot, LifecycleError> {
        let observed = self.observe_current_authority_snapshot_with(snapshot, changes_since)?;
        if observed.continuation_pending {
            service.bind_truth_authority_stamp_locked(
                observed.snapshot.head.cursor,
                &observed.snapshot.change_stamp,
                writer_lock,
            )?;
        }
        Ok(observed.snapshot)
    }

    fn quarantine_status(&self, reason: String) -> Result<LifecycleStatus, LifecycleError> {
        match StoreWriterLock::try_acquire(&self.store_root) {
            Ok(_lock) => {
                // The observation that led here was made without the writer
                // lock. A normal append can temporarily expose an incomplete
                // WAL/header/checkpoint relationship, then finish before this
                // lock is acquired. Never quarantine from that stale
                // observation: re-run the complete classifier under the lock
                // and rename only if it still reports invalid state.
                let observed = self.status_with_quarantine(false)?;
                if observed.availability != DerivedAccessAvailability::Quarantined {
                    return Ok(observed);
                }
                let confirmed_reason = observed.detail.unwrap_or(reason);
                self.paths.quarantine(&confirmed_reason)?;
                Ok(status(
                    DerivedAccessAvailability::Quarantined,
                    None,
                    Some(confirmed_reason),
                ))
            }
            Err(WriterLockError::Busy) => Ok(status(
                DerivedAccessAvailability::Unavailable,
                None,
                Some("derived writer is busy while invalid state awaits quarantine".to_owned()),
            )),
            Err(error) => Err(error.into()),
        }
    }

    fn quarantine_error(&self, reason: String) -> LifecycleError {
        // This path follows an error while opening immutable metadata that was
        // made visible by atomic generation/publication renames. Unlike
        // `quarantine_status`, it is not classifying a mutable WAL/checkpoint
        // snapshot that a governed writer can repair between observation and
        // lock acquisition. The writer lock therefore protects the rename;
        // re-running the status classifier here would discard the typed open
        // error without adding a stronger observation.
        match StoreWriterLock::try_acquire(&self.store_root) {
            Ok(_lock) => match self.paths.quarantine(&reason) {
                Ok(_) => LifecycleError::Quarantined(reason),
                Err(error) => LifecycleError::Generation(error),
            },
            Err(error) => LifecycleError::WriterLock(error),
        }
    }

    fn generation_error_status(
        &self,
        error: GenerationError,
        allow_quarantine: bool,
    ) -> Result<LifecycleStatus, LifecycleError> {
        if generation_error_requires_rebuild(&error) {
            Ok(status(
                DerivedAccessAvailability::RebuildRequired,
                None,
                Some(error.to_string()),
            ))
        } else if generation_error_requires_quarantine(&error) {
            self.invalid_status(error.to_string(), allow_quarantine)
        } else {
            Ok(status(
                DerivedAccessAvailability::Unavailable,
                None,
                Some(error.to_string()),
            ))
        }
    }

    fn lifecycle_error_status(
        &self,
        error: LifecycleError,
        allow_quarantine: bool,
    ) -> Result<LifecycleStatus, LifecycleError> {
        if lifecycle_error_requires_quarantine(&error) {
            self.invalid_status(error.to_string(), allow_quarantine)
        } else {
            Ok(status(
                DerivedAccessAvailability::Unavailable,
                None,
                Some(error.to_string()),
            ))
        }
    }

    fn invalid_status(
        &self,
        reason: String,
        allow_quarantine: bool,
    ) -> Result<LifecycleStatus, LifecycleError> {
        if allow_quarantine {
            self.quarantine_status(reason)
        } else {
            Ok(status(
                DerivedAccessAvailability::Quarantined,
                None,
                Some(reason),
            ))
        }
    }

    fn generation_open_error(&self, error: GenerationError) -> LifecycleError {
        if generation_error_requires_rebuild(&error) {
            LifecycleError::RebuildRequired(error.to_string())
        } else if generation_error_requires_quarantine(&error) {
            self.quarantine_error(error.to_string())
        } else {
            LifecycleError::Generation(error)
        }
    }
}

impl CurrentGeneration {
    pub(crate) fn generation_id(&self) -> &str {
        &self.publication_identity.generation_id
    }

    pub(crate) fn publication_identity(&self) -> &GenerationPublication {
        &self.publication_identity
    }

    pub(crate) fn service(&self) -> &DerivedAccessService {
        &self.service
    }

    pub(crate) const fn authority_head(&self) -> TruthCursor {
        self.authority_head
    }

    pub(crate) const fn locator_applied(&self) -> TruthCursor {
        self.locator_applied
    }

    pub(crate) const fn authority_maintenance_pending(&self) -> bool {
        self.authority_maintenance_pending
    }

    /// Pin the exact live Change checkpoint used by one product response.
    ///
    /// The cursor, locator, and checkpoint rows come from the service's one
    /// publication-validation snapshot. The generation lease keeps immutable
    /// publication resources alive; callers still revalidate at response
    /// completion so a same-generation append cannot make a K result look like
    /// K+1.
    pub(crate) fn pin_change_reader_checkpoint(
        &self,
    ) -> Result<ReaderProjectionCheckpointV1, LifecycleError> {
        let snapshot = self.service.publication_validation_snapshot()?;
        validate_live_reader_checkpoint(&snapshot, self.reader_receipt.as_ref())?;
        let checkpoint = snapshot.reader_projection_checkpoint.ok_or_else(|| {
            LifecycleError::RebuildRequired(
                "published Change generation has no live reader checkpoint".to_owned(),
            )
        })?;
        if snapshot.authority.head.store_id != checkpoint.store_id {
            return Err(LifecycleError::Validation(
                "live Change reader checkpoint has the wrong store identity".to_owned(),
            ));
        }
        if snapshot.authority.head.cursor != checkpoint.truth_cursor {
            return Err(LifecycleError::TruthChanged);
        }
        Ok(checkpoint)
    }

    /// Reconstruct the frozen public profile from the validated immutable V3
    /// anchor and its exact live checkpoint. Commit-graph freshness remains a
    /// CLI concern because it is outside derived-store authority.
    pub(crate) fn reader_profile_document(
        &self,
        checkpoint: &ReaderProjectionCheckpointV1,
    ) -> Result<ReaderProfileDocumentV1, LifecycleError> {
        let receipt = self.reader_receipt.as_ref().ok_or_else(|| {
            LifecycleError::RebuildRequired(
                "current generation has no Change reader-profile receipt".to_owned(),
            )
        })?;
        checkpoint
            .validate_for_receipt(receipt)
            .map_err(change_reader_contract_lifecycle_error)?;
        Ok(ReaderProfileDocumentV1 {
            schema: INSPECT_READER_PROFILE_SCHEMA.to_owned(),
            version: 1,
            availability: ReaderProfileAvailabilityV1::Ready,
            authority_cursor: checkpoint.authority_cursor.clone(),
            minimum_reader_profile: Some(receipt.minimum_reader_profile.clone()),
            activation_id: Some(receipt.publication_activation_id.clone()),
            manifest_hash: Some(receipt.publication_manifest_hash.clone()),
            completion_id: Some(receipt.publication_completion_id.clone()),
            commit_graph_stamp: None,
            documents: crate::documents::change_revision_document_registry()
                .iter()
                .map(|(schema, version)| ((*schema).to_owned(), *version))
                .collect(),
        })
    }

    pub(crate) fn change_generation_stamp(
        &self,
        checkpoint: &ReaderProjectionCheckpointV1,
        semantic_projection: &crate::session::ChangeProjection,
        document_projection: &crate::session::ChangeDocumentProjectionV1,
    ) -> Result<String, LifecycleError> {
        let receipt = self.reader_receipt.as_ref().ok_or_else(|| {
            LifecycleError::RebuildRequired(
                "current generation has no Change reader-profile receipt".to_owned(),
            )
        })?;
        derived_change_generation_stamp_v1(
            receipt,
            checkpoint,
            semantic_projection,
            document_projection,
        )
        .map_err(change_reader_contract_lifecycle_error)
    }

    pub(crate) fn strict_change_generation_stamp(
        &self,
        checkpoint: &ReaderProjectionCheckpointV1,
        authority_cursor: &crate::session::AuthorityCursorV2,
        semantic_projection: &crate::session::ChangeProjection,
        document_projection: &crate::session::ChangeDocumentProjectionV1,
    ) -> Result<String, LifecycleError> {
        let receipt = self.reader_receipt.as_ref().ok_or_else(|| {
            LifecycleError::RebuildRequired(
                "current generation has no Change reader-profile receipt".to_owned(),
            )
        })?;
        let preimage = strict_change_generation_stamp_preimage_v1(
            receipt,
            checkpoint,
            authority_cursor,
            semantic_projection,
            document_projection,
            RuntimeTrustIdentityV1::NotApplicable,
        )
        .map_err(change_reader_contract_lifecycle_error)?;
        change_generation_stamp_sha256_v1(&preimage).map_err(change_reader_contract_lifecycle_error)
    }
}

fn lifecycle_progress(
    phase: GenerationProgressPhase,
    completed: usize,
    total: usize,
    bytes_processed: u64,
    rebuild_started: Instant,
    phase_started: Instant,
) -> LifecycleProgress {
    let elapsed_ms = u64::try_from(rebuild_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let phase_elapsed_ms = u64::try_from(phase_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let estimated_remaining_ms = if completed >= total {
        Some(0)
    } else if completed == 0 || phase_elapsed_ms == 0 {
        None
    } else {
        let remaining = u64::try_from(total - completed).unwrap_or(u64::MAX);
        let completed = u64::try_from(completed).unwrap_or(u64::MAX);
        Some(
            phase_elapsed_ms
                .saturating_mul(remaining)
                .checked_div(completed)
                .unwrap_or(u64::MAX),
        )
    };
    LifecycleProgress {
        phase,
        completed,
        total,
        bytes_processed,
        elapsed_ms,
        estimated_remaining_ms,
    }
}

fn require_stable_authority(check: &JournalChangeCheck) -> Result<(), LifecycleError> {
    match check.verdict {
        JournalChangeVerdict::Stable => Ok(()),
        JournalChangeVerdict::Changed | JournalChangeVerdict::Indeterminate => {
            Err(LifecycleError::RebuildRequired(format!(
                "authoritative truth freshness is {:?} via {}",
                check.verdict, check.mechanism
            )))
        }
    }
}

fn record_and_report_progress(
    paths: &GenerationLayout,
    generation_id: &str,
    update: LifecycleProgress,
    progress: &mut impl FnMut(LifecycleProgress) -> LifecycleControl,
) -> Result<LifecycleControl, LifecycleError> {
    paths.record_progress(generation_id, update.into())?;
    Ok(progress(update))
}

impl From<LifecycleProgress> for GenerationProgress {
    fn from(progress: LifecycleProgress) -> Self {
        Self::new(
            progress.phase,
            progress.completed,
            progress.total,
            progress.bytes_processed,
            progress.elapsed_ms,
            progress.estimated_remaining_ms,
        )
    }
}

pub(crate) fn lifecycle_transition_allowed(
    current: DerivedAccessAvailability,
    successor: DerivedAccessAvailability,
) -> bool {
    current.allows(successor)
}

fn change_reader_contract_lifecycle_error(error: ChangeReaderContractError) -> LifecycleError {
    if error.is_rebuild_required() {
        LifecycleError::RebuildRequired(error.to_string())
    } else {
        LifecycleError::Validation(error.to_string())
    }
}

fn validate_live_reader_checkpoint(
    snapshot: &PublicationValidationSnapshot,
    receipt: Option<&ChangeReaderProfileReceiptV3>,
) -> Result<(), LifecycleError> {
    match (receipt, snapshot.reader_projection_checkpoint.as_ref()) {
        (None, None) => Ok(()),
        (None, Some(_)) => Err(LifecycleError::RebuildRequired(
            "legacy publication has an unanchored reader checkpoint".to_owned(),
        )),
        (Some(_), None) => Err(LifecycleError::RebuildRequired(
            "published Change generation has no live reader checkpoint".to_owned(),
        )),
        (Some(receipt), Some(checkpoint)) => {
            checkpoint
                .validate_for_receipt(receipt)
                .map_err(change_reader_contract_lifecycle_error)?;
            if checkpoint.truth_cursor != snapshot.locator_applied {
                return Err(LifecycleError::Validation(
                    "live Change reader checkpoint differs from the locator checkpoint".to_owned(),
                ));
            }
            Ok(())
        }
    }
}

fn validate_candidate(
    service: &DerivedAccessService,
    descriptor: &GenerationDescriptor,
    truth_events: Vec<crate::session::event::ShoreEvent>,
) -> Result<super::semantic::SemanticSnapshot, LifecycleError> {
    let head = service.truth_head()?.cursor;
    let checkpoint = service.locator_checkpoint()?;
    let cursor = service.cursor_inventory()?;
    let locator = service.locator_inventory()?;
    let semantic = service.semantic_inventory()?;
    let expected_count = u64::try_from(truth_events.len())
        .map_err(|_| LifecycleError::Validation("truth count overflow".to_owned()))?;
    if head.epoch != descriptor.epoch
        || head.sequence != descriptor.head_sequence
        || head.sequence != expected_count
        || checkpoint != head
        || cursor.head_sequence != expected_count
        || cursor.receipt_count != expected_count
        || locator.row_count != expected_count
        || semantic.fact_count != expected_count
        || semantic.retained_body_object_bytes != 0
    {
        return Err(LifecycleError::Validation(format!(
            "coverage mismatch: head={head:?}, checkpoint={checkpoint:?}, truth={expected_count}, \
             cursor_receipts={}, locator_rows={}, semantic_facts={}, retained_body_bytes={}",
            cursor.receipt_count,
            locator.row_count,
            semantic.fact_count,
            semantic.retained_body_object_bytes
        )));
    }
    let actual = match service.semantic_materialized_audit_snapshot()? {
        LocatorRead::Ready(snapshot) => snapshot,
        LocatorRead::CatchUpRequired { applied, observed } => {
            return Err(LifecycleError::Validation(format!(
                "candidate remained behind after rebuild: {applied:?} != {observed:?}"
            )));
        }
    };
    let expected = strict_bodyless_materialized_snapshot_at(head, truth_events)
        .map_err(|error| LifecycleError::Validation(error.to_string()))?;
    if actual != expected {
        return Err(LifecycleError::Validation(
            "materialized semantic receipt differs from strict replay".to_owned(),
        ));
    }
    if !descriptor.semantic_receipt.is_empty()
        && actual.semantic_receipt != descriptor.semantic_receipt
    {
        return Err(LifecycleError::Validation(
            "semantic receipt differs from generation descriptor".to_owned(),
        ));
    }
    Ok(actual)
}

fn validate_published(
    descriptor: &GenerationDescriptor,
    authority: &TruthAuthoritySnapshot,
    checkpoint: TruthCursor,
) -> Result<(), LifecycleError> {
    let head = authority.head.cursor;
    // The descriptor freezes the publication-time authority anchor. The cursor
    // ledger may later carry that anchor forward after a bounded stable check
    // (or advance it with a governed append). Authority validation has
    // already proved the live cursor before this structural coverage check, so
    // requiring byte equality with the immutable anchor would reject valid NTFS
    // continuations that observed only unrelated volume churn.
    if head.epoch != descriptor.epoch
        || head.sequence < descriptor.head_sequence
        || checkpoint.epoch != head.epoch
        || checkpoint.sequence > head.sequence
    {
        return Err(LifecycleError::Validation(format!(
            "published coverage mismatch: head={head:?}, checkpoint={checkpoint:?}, \
             descriptor={}:{}",
            descriptor.epoch, descriptor.head_sequence
        )));
    }
    Ok(())
}

fn validate_wal_shape(generation_root: &Path) -> Result<(), LifecycleError> {
    let database = generation_root.join("cursor.sqlite3");
    let mut database_header = [0_u8; 16];
    use std::io::Read as _;
    match std::fs::File::open(&database) {
        Ok(mut file) => file
            .read_exact(&mut database_header)
            .map_err(|error| LifecycleError::Validation(error.to_string()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(LifecycleError::Validation(
                "published generation database is absent".to_owned(),
            ));
        }
        Err(error) => return Err(LifecycleError::Truth(error.to_string())),
    }
    if &database_header != b"SQLite format 3\0" {
        return Err(LifecycleError::Validation(
            "published generation database header is invalid".to_owned(),
        ));
    }

    let path = generation_root.join("cursor.sqlite3-wal");
    let length = match std::fs::metadata(&path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(LifecycleError::Truth(error.to_string())),
    };
    if length == 0 {
        return Ok(());
    }
    if length < 32 {
        return Err(LifecycleError::Validation(format!(
            "SQLite WAL is shorter than its 32-byte header: {length}"
        )));
    }
    let mut header = [0_u8; 32];
    std::fs::File::open(&path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| LifecycleError::Truth(error.to_string()))?;
    let magic = u32::from_be_bytes(header[0..4].try_into().expect("four-byte WAL magic"));
    if !matches!(magic, 0x377f_0682 | 0x377f_0683) {
        return Err(LifecycleError::Validation(format!(
            "SQLite WAL has unsupported magic 0x{magic:08x}"
        )));
    }
    let encoded_page_size =
        u32::from_be_bytes(header[8..12].try_into().expect("four-byte WAL page size"));
    let page_size = if encoded_page_size == 1 {
        65_536
    } else {
        u64::from(encoded_page_size)
    };
    if !(512..=65_536).contains(&page_size) || !page_size.is_power_of_two() {
        return Err(LifecycleError::Validation(format!(
            "SQLite WAL has invalid page size {page_size}"
        )));
    }
    let frame_size = page_size + 24;
    if (length - 32) % frame_size != 0 {
        return Err(LifecycleError::Validation(format!(
            "SQLite WAL length {length} is not an integral number of {frame_size}-byte frames"
        )));
    }
    Ok(())
}

fn generation_error_requires_quarantine(error: &GenerationError) -> bool {
    matches!(
        error,
        GenerationError::InvalidGenerationId(_) | GenerationError::Metadata { .. }
    )
}

fn generation_error_requires_rebuild(error: &GenerationError) -> bool {
    matches!(error, GenerationError::LegacyDescriptor { .. })
}

fn lifecycle_error_requires_quarantine(error: &LifecycleError) -> bool {
    matches!(error, LifecycleError::Validation(_))
        || matches!(
            error,
            LifecycleError::Generation(error) if generation_error_requires_quarantine(error)
        )
}

fn service_error_requires_quarantine(error: &DerivedAccessServiceError) -> bool {
    match error {
        DerivedAccessServiceError::Cursor(error) => matches!(
            error,
            CursorLedgerError::IdentityMismatch(_)
                | CursorLedgerError::SchemaMismatch(_)
                | CursorLedgerError::Quarantined(_)
                | CursorLedgerError::IncompleteBootstrap
                | CursorLedgerError::UnreceiptedCarrier(_)
                | CursorLedgerError::CarrierAbsent(_)
                | CursorLedgerError::WitnessMismatch(_)
        ),
        DerivedAccessServiceError::Locator(error) => locator_error_requires_quarantine(error),
        DerivedAccessServiceError::Semantic(error) => semantic_error_requires_quarantine(error),
        DerivedAccessServiceError::SemanticModel(_)
        | DerivedAccessServiceError::LocatorModel(_)
        | DerivedAccessServiceError::Freshness(_)
        | DerivedAccessServiceError::EmptyIncompleteDelta(_) => true,
        DerivedAccessServiceError::Truth(_)
        | DerivedAccessServiceError::ZeroBatchLimit
        | DerivedAccessServiceError::BootstrapCancelled => false,
    }
}

fn service_error_requires_rebuild(error: &DerivedAccessServiceError) -> bool {
    matches!(
        error,
        DerivedAccessServiceError::Cursor(CursorLedgerError::UpgradeRequired(_))
            | DerivedAccessServiceError::Locator(SqliteLocatorError::UpgradeRequired(_))
            | DerivedAccessServiceError::Semantic(SqliteSemanticError::Locator(
                SqliteLocatorError::UpgradeRequired(_)
            ))
            | DerivedAccessServiceError::Semantic(
                SqliteSemanticError::ProductHistoryUpgradeRequired(_)
            )
            | DerivedAccessServiceError::Semantic(SqliteSemanticError::UpgradeRequired(_))
    )
}

fn locator_error_requires_quarantine(error: &SqliteLocatorError) -> bool {
    matches!(
        error,
        SqliteLocatorError::MissingSidecar(_)
            | SqliteLocatorError::Metadata(_)
            | SqliteLocatorError::Delta(_)
            | SqliteLocatorError::Model(_)
            | SqliteLocatorError::CarrierMismatch(_)
    )
}

fn semantic_error_requires_quarantine(error: &SqliteSemanticError) -> bool {
    match error {
        SqliteSemanticError::Locator(error) => locator_error_requires_quarantine(error),
        SqliteSemanticError::Model(_)
        | SqliteSemanticError::Metadata(_)
        | SqliteSemanticError::Delta(_)
        | SqliteSemanticError::CarrierMismatch(_) => true,
        SqliteSemanticError::ProductHistoryUpgradeRequired(_)
        | SqliteSemanticError::UpgradeRequired(_) => false,
        SqliteSemanticError::Sqlite { .. } => false,
    }
}

fn status(
    availability: DerivedAccessAvailability,
    generation_id: Option<String>,
    detail: Option<String>,
) -> LifecycleStatus {
    LifecycleStatus {
        availability,
        generation_id,
        phase: None,
        completed: None,
        total: None,
        bytes_processed: None,
        elapsed_ms: None,
        estimated_remaining_ms: None,
        detail,
    }
}

fn directory_has_entries(path: &Path) -> Result<bool, LifecycleError> {
    if !path.exists() {
        return Ok(false);
    }
    Ok(std::fs::read_dir(path)
        .map_err(|error| LifecycleError::Truth(error.to_string()))?
        .next()
        .transpose()
        .map_err(|error| LifecycleError::Truth(error.to_string()))?
        .is_some())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::bench_support::longitudinal::LongitudinalCountingScopeV1;
    use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
    use crate::model::JournalId;
    use crate::session::derived_access::layout::{
        DerivedStorageLayout, DerivedStorageNamespace, DerivedStorageTransition,
    };
    use crate::session::derived_access::product_contract::{
        DerivedAccessAvailability, DerivedAccessProfile,
    };
    use crate::session::derived_access::semantic::change::{
        ChangeReaderProfileReceiptProbeV1, ReaderProjectionCheckpointV1,
        probe_change_reader_profile_receipt,
    };
    use crate::session::event::{
        EventTarget, EventType, ReviewInitializedPayload, ShoreEvent, Writer,
    };
    use crate::session::store::backend::StoreBackend;
    use crate::session::store::capabilities::{
        CapabilityFixtureState, write_capability_fixture_for_test,
    };
    use crate::session::{EventStore, EventWriteOutcome};

    const CHANGE_READER_PROFILE_RESOURCE: &str = "change-reader-profile.json";

    #[test]
    fn lifecycle_implements_the_frozen_transition_table() {
        for state in DerivedAccessAvailability::ALL {
            for successor in DerivedAccessAvailability::ALL {
                assert_eq!(
                    lifecycle_transition_allowed(state, successor),
                    state.allows(successor),
                    "{state:?} -> {successor:?}"
                );
            }
        }
    }

    #[test]
    fn transient_generation_io_is_unavailable_not_corruption() {
        assert!(!generation_error_requires_quarantine(
            &GenerationError::Io {
                path: PathBuf::from("publications"),
                message: "temporarily unavailable".to_owned(),
            }
        ));
        assert!(generation_error_requires_quarantine(
            &GenerationError::Metadata {
                path: PathBuf::from("publication.json"),
                message: "invalid body".to_owned(),
            }
        ));
    }

    #[test]
    fn off_is_a_real_filesystem_absence() {
        let temp = TempDir::new().unwrap();
        let store_root = temp.path().join("does-not-exist");
        let lifecycle =
            DerivedAccessLifecycle::new(DerivedAccessProfile::Off, &store_root, "store:test")
                .unwrap();

        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::Absent
        );
        assert!(!store_root.exists());
    }

    #[test]
    fn compatible_legacy_generation_moves_without_replay_and_reopens_the_same_head() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        let built = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let stable =
            DerivedStorageLayout::for_namespace(temp.path(), DerivedStorageNamespace::Stable);
        let legacy =
            DerivedStorageLayout::for_namespace(temp.path(), DerivedStorageNamespace::Legacy);
        let generation_id = built.generation_id.clone().unwrap();
        let descriptor_before = fs::read(
            stable
                .root()
                .join("generations")
                .join(&generation_id)
                .join("generation.json"),
        )
        .unwrap();
        fs::rename(stable.root(), legacy.root()).unwrap();

        let transition = DerivedStorageLayout::transition_legacy(temp.path()).unwrap();

        assert_eq!(transition.disposition, DerivedStorageTransition::Moved);
        assert_eq!(
            fs::read(
                stable
                    .root()
                    .join("generations")
                    .join(&generation_id)
                    .join("generation.json")
            )
            .unwrap(),
            descriptor_before
        );
        let reopened = active_lifecycle(temp.path());
        let status = reopened.status().unwrap();
        assert_eq!(status.availability, DerivedAccessAvailability::Current);
        assert_eq!(
            status.generation_id.as_deref(),
            Some(generation_id.as_str())
        );
        assert_eq!(
            reopened
                .open_current()
                .unwrap()
                .unwrap()
                .service()
                .locator_checkpoint()
                .unwrap()
                .sequence,
            built.head_sequence
        );
    }

    #[test]
    fn lifecycle_re_resolves_when_transition_wins_before_reader_lease() {
        let temp = populated_store(7);
        let built = active_lifecycle(temp.path())
            .rebuild(|_| LifecycleControl::Continue)
            .unwrap();
        let stable =
            DerivedStorageLayout::for_namespace(temp.path(), DerivedStorageNamespace::Stable);
        let legacy =
            DerivedStorageLayout::for_namespace(temp.path(), DerivedStorageNamespace::Legacy);
        fs::rename(stable.root(), legacy.root()).unwrap();
        let lifecycle = active_lifecycle(temp.path());
        let mut transitioned = false;

        let selected = lifecycle.stable_current_publication_with_hook(|| {
            if !transitioned {
                let receipt = DerivedStorageLayout::transition_legacy(temp.path()).unwrap();
                assert_eq!(receipt.disposition, DerivedStorageTransition::Moved);
                transitioned = true;
            }
        });

        assert!(transitioned);
        assert!(
            selected.is_ok(),
            "reader must re-resolve after the move: {selected:?}"
        );
        assert_eq!(
            selected.unwrap().unwrap().1.generation_id,
            built.generation_id.unwrap()
        );
        assert_eq!(
            lifecycle
                .open_current()
                .unwrap()
                .unwrap()
                .service()
                .locator_checkpoint()
                .unwrap()
                .sequence,
            built.head_sequence
        );
    }

    #[test]
    fn cancelled_bootstrap_never_publishes_a_partial_generation() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        let calls = Cell::new(0);

        let result = lifecycle.rebuild(|_| {
            calls.set(calls.get() + 1);
            LifecycleControl::Cancel
        });

        assert!(matches!(result, Err(LifecycleError::Cancelled)));
        assert!(calls.get() > 0);
        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::Absent
        );
        assert!(lifecycle.open_current().unwrap().is_none());
        assert_eq!(
            EventStore::open(temp.path()).list_events().unwrap().len(),
            7
        );
    }

    #[test]
    fn replacement_keeps_an_open_reader_on_the_prior_generation() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        let first = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let reader = lifecycle.open_current().unwrap().unwrap();

        let second = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();

        assert_ne!(first.generation_id, second.generation_id);
        assert_eq!(second.retained_reader_generation_count, 1);
        assert!(
            lifecycle
                .paths()
                .generation(first.generation_id.as_deref().unwrap())
                .exists()
        );
        assert_eq!(
            reader.generation_id(),
            first.generation_id.as_deref().unwrap()
        );
        assert_eq!(reader.service().locator_checkpoint().unwrap().sequence, 7);
        assert_eq!(
            lifecycle.open_current().unwrap().unwrap().generation_id(),
            second.generation_id.as_deref().unwrap()
        );
        drop(reader);

        let third = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();

        assert_eq!(third.reclaimed_generation_count, 2);
        assert!(
            !lifecycle
                .paths()
                .generation(first.generation_id.as_deref().unwrap())
                .exists()
        );
        assert!(
            !lifecycle
                .paths()
                .generation(second.generation_id.as_deref().unwrap())
                .exists()
        );
    }

    #[test]
    fn reader_retries_when_publication_changes_before_lease_acquisition() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        let first_generation = lifecycle
            .rebuild(|_| LifecycleControl::Continue)
            .unwrap()
            .generation_id
            .unwrap();
        let mut replaced = false;

        let (_paths, publication, lease) = lifecycle
            .stable_current_publication_with_hook(|| {
                if !replaced {
                    lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
                    replaced = true;
                }
            })
            .unwrap()
            .unwrap();

        assert!(replaced);
        assert_ne!(publication.generation_id, first_generation);
        assert!(
            lifecycle
                .paths()
                .generation(&publication.generation_id)
                .exists()
        );
        assert!(
            !lifecycle.paths().generation(&first_generation).exists(),
            "the replacement reclaimed the generation selected before the lease"
        );
        let delayed_reader_lease = lifecycle.paths().generation_lease_path(&first_generation);
        assert!(
            delayed_reader_lease.exists(),
            "the delayed reader can recreate the reclaimed generation's lease path"
        );
        drop(lease);

        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        assert!(
            !delayed_reader_lease.exists(),
            "the next rebuild must collect a lease recreated by a delayed reader"
        );
    }

    #[test]
    fn repeated_rebuilds_collect_reclaimed_generation_leases() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());

        for _ in 0..6 {
            lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        }

        let generations = lifecycle.paths().root().join("generations");
        let generation_count = std::fs::read_dir(&generations).unwrap().count();
        let lease_generation_ids = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                lifecycle
                    .paths()
                    .generation_lease_id(&name)
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();
        assert_eq!(generation_count, 1);
        assert!(
            lease_generation_ids
                .iter()
                .all(|generation_id| generations.join(generation_id).exists()),
            "every retained lease must still protect a generation"
        );
    }

    #[test]
    fn writer_idle_confirmation_distinguishes_current_from_rebuild_required() {
        let temp = populated_store(1);
        let lifecycle = active_lifecycle(temp.path());
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();

        assert!(!lifecycle.rebuild_required_while_writer_idle().unwrap());
        EventStore::open(temp.path())
            .record_event_once(&lifecycle_event(2))
            .unwrap();
        assert!(lifecycle.rebuild_required_while_writer_idle().unwrap());

        let _writer_lock = StoreWriterLock::acquire(temp.path()).unwrap();
        let started = std::time::Instant::now();
        assert!(!lifecycle.rebuild_required_while_writer_idle().unwrap());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "writer-idle confirmation must never wait behind an active writer"
        );
    }

    #[test]
    fn stable_authority_successors_are_deferred_to_writable_maintenance() {
        let temp = populated_store(1);
        let lifecycle = active_lifecycle(temp.path());
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let current = lifecycle.open_current().unwrap().unwrap();
        let publication = lifecycle.paths.current_publication().unwrap().unwrap();
        let descriptor = lifecycle.paths.descriptor(&publication).unwrap();
        let initial = current.service().truth_authority_snapshot().unwrap();
        let successor_one = JournalChangeStamp::Observed {
            identity_sha256: "1".repeat(64),
            change_sha256: "2".repeat(64),
            entry_count: None,
            native_cursor: None,
        };

        let continued = lifecycle
            .validate_current_authority_with(current.service(), |before| {
                assert_eq!(before, &initial.change_stamp);
                Ok(stable_change_check(successor_one.clone()))
            })
            .unwrap();
        assert_eq!(continued.change_stamp, successor_one);
        validate_published(&descriptor, &continued, current.locator_applied()).unwrap();
        assert_eq!(
            current
                .service()
                .truth_authority_snapshot()
                .unwrap()
                .change_stamp,
            initial.change_stamp,
            "the read-only response must not persist its proved successor"
        );

        let writer_lock = StoreWriterLock::acquire(temp.path()).unwrap();
        let writable = lifecycle
            .open_current_for_write_locked(&writer_lock)
            .unwrap()
            .unwrap();
        let persisted = lifecycle
            .persist_current_authority_with(writable.service(), &writer_lock, |before| {
                assert_eq!(before, &initial.change_stamp);
                Ok(stable_change_check(successor_one.clone()))
            })
            .unwrap();
        assert_eq!(persisted.change_stamp, successor_one);

        let successor_two = JournalChangeStamp::Observed {
            identity_sha256: "1".repeat(64),
            change_sha256: "3".repeat(64),
            entry_count: None,
            native_cursor: None,
        };
        let continued = lifecycle
            .validate_current_authority_with(current.service(), |before| {
                if before == &successor_one {
                    Ok(stable_change_check(successor_two.clone()))
                } else {
                    Ok(JournalChangeCheck {
                        after: before.clone(),
                        verdict: JournalChangeVerdict::Indeterminate,
                        native_bytes_examined: 0,
                        native_records_examined: 0,
                        relevant_file_references: Vec::new(),
                        mechanism: "bounded interval would be exhausted from stale cursor"
                            .to_owned(),
                    })
                }
            })
            .unwrap();
        assert_eq!(continued.change_stamp, successor_two);
        assert_eq!(
            current
                .service()
                .truth_authority_snapshot()
                .unwrap()
                .change_stamp,
            successor_one,
            "a second response still leaves persistence to maintenance"
        );
        lifecycle
            .persist_current_authority_with(writable.service(), &writer_lock, |before| {
                assert_eq!(before, &successor_one);
                Ok(stable_change_check(successor_two.clone()))
            })
            .unwrap();
        assert_eq!(
            writable
                .service()
                .truth_authority_snapshot()
                .unwrap()
                .change_stamp,
            successor_two
        );
    }

    #[test]
    fn stable_authority_successor_does_not_wait_for_a_busy_writer() {
        let temp = populated_store(1);
        let lifecycle = active_lifecycle(temp.path());
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let current = lifecycle.open_current().unwrap().unwrap();
        let initial = current.service().truth_authority_snapshot().unwrap();
        let successor = JournalChangeStamp::Observed {
            identity_sha256: "4".repeat(64),
            change_sha256: "5".repeat(64),
            entry_count: None,
            native_cursor: None,
        };
        let writer_lock = StoreWriterLock::acquire(temp.path()).unwrap();

        let continued = lifecycle
            .validate_current_authority_with(current.service(), |_| {
                Ok(stable_change_check(successor.clone()))
            })
            .unwrap();

        assert_eq!(continued.change_stamp, successor);
        assert_eq!(
            current
                .service()
                .truth_authority_snapshot()
                .unwrap()
                .change_stamp,
            initial.change_stamp,
            "the busy read may use but must not claim to persist the successor"
        );
        drop(writer_lock);
    }

    #[cfg(windows)]
    #[test]
    fn native_ntfs_stable_continuation_defers_unrelated_volume_churn_to_maintenance() {
        let temp = populated_store(1);
        let lifecycle = active_lifecycle(temp.path());
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let current = lifecycle.open_current().unwrap().unwrap();
        let before = current.service().truth_authority_snapshot().unwrap();
        std::fs::write(temp.path().join("unrelated-to-events.bin"), b"volume churn").unwrap();

        let validation = lifecycle
            .validate_cached_current(&current)
            .expect("unrelated NTFS churn remains stable");

        assert_ne!(validation.authority.change_stamp, before.change_stamp);
        assert!(validation.authority_maintenance_pending);
        assert_eq!(
            current
                .service()
                .truth_authority_snapshot()
                .unwrap()
                .change_stamp,
            before.change_stamp,
            "cached-current validation observes a successor without persisting it"
        );
    }

    #[test]
    fn bootstrap_population_and_serial_oracle_open_each_carrier_once() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        let scope = LongitudinalCountingScopeV1::new("b".repeat(64)).unwrap();
        let guard = scope.enter();
        let mut retained_by_phase = Vec::new();

        lifecycle
            .rebuild(|update| {
                retained_by_phase.push((
                    update.phase,
                    scope.snapshot().capacity_ownership.retained_decoded_events,
                ));
                LifecycleControl::Continue
            })
            .expect("bounded bootstrap");

        drop(guard);
        let observed = scope.snapshot();
        assert_eq!(
            observed
                .derived_access_phases
                .iter()
                .map(|sample| sample.phase)
                .collect::<Vec<_>>(),
            crate::bench_support::derived_access::QualificationDerivedAccessPhaseOperationV1::Bootstrap
                .expected_phases()
        );
        assert_eq!(observed.counters.carrier_opens, 14);
        assert_eq!(observed.counters.event_decodes, 14);
        assert_eq!(observed.counters.event_validations, 14);
        assert_eq!(observed.capacity_ownership.retained_decoded_events, 0);
        for phase in [
            GenerationProgressPhase::CursorPopulation,
            GenerationProgressPhase::ProjectionPopulation,
            GenerationProgressPhase::StrictVerification,
        ] {
            assert_eq!(
                retained_by_phase
                    .iter()
                    .filter(|(observed, _)| *observed == phase)
                    .map(|(_, retained)| *retained)
                    .max(),
                Some(7),
                "{phase:?} must retain exactly one decoded population"
            );
        }
        assert_eq!(
            retained_by_phase
                .iter()
                .filter(|(phase, _)| *phase == GenerationProgressPhase::Finalizing)
                .map(|(_, retained)| *retained)
                .max(),
            Some(0),
            "finalization must retain no decoded population"
        );
    }

    #[test]
    fn bootstrap_reports_durable_phase_and_resource_progress() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        let mut updates = Vec::new();

        lifecycle
            .rebuild(|update| {
                updates.push(update);
                LifecycleControl::Continue
            })
            .expect("bounded bootstrap");

        let phases = updates
            .iter()
            .map(|update| update.phase)
            .collect::<Vec<_>>();
        assert!(phases.contains(&GenerationProgressPhase::CursorPopulation));
        assert!(phases.contains(&GenerationProgressPhase::ProjectionPopulation));
        assert!(phases.contains(&GenerationProgressPhase::StrictVerification));
        assert!(phases.contains(&GenerationProgressPhase::Finalizing));
        assert!(
            phases.windows(2).all(|pair| pair[0] <= pair[1]),
            "progress phases must be monotonic: {phases:?}"
        );
        assert!(
            updates
                .iter()
                .all(|update| update.completed <= update.total)
        );
        assert!(
            updates
                .iter()
                .filter(|update| update.completed < update.total)
                .all(|update| update
                    .estimated_remaining_ms
                    .is_none_or(|_| update.completed > 0))
        );
        for phase in [
            GenerationProgressPhase::CursorPopulation,
            GenerationProgressPhase::ProjectionPopulation,
            GenerationProgressPhase::StrictVerification,
        ] {
            let completed = updates
                .iter()
                .rev()
                .find(|update| update.phase == phase)
                .expect("phase progress");
            assert_eq!((completed.completed, completed.total), (7, 7));
            assert!(completed.bytes_processed > 0);
            assert_eq!(completed.estimated_remaining_ms, Some(0));
        }
        assert_eq!(
            updates.last().map(|update| (
                update.phase,
                update.completed,
                update.total,
                update.estimated_remaining_ms,
            )),
            Some((GenerationProgressPhase::Finalizing, 1, 1, Some(0)))
        );
    }

    #[test]
    fn cancellation_between_projection_batches_restarts_from_clean_staging() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());

        let cancelled = lifecycle.rebuild_with_hook_and_batch_limit(
            4,
            |update| {
                if update.phase == GenerationProgressPhase::ProjectionPopulation
                    && update.completed == 4
                {
                    LifecycleControl::Cancel
                } else {
                    LifecycleControl::Continue
                }
            },
            |_| {},
        );

        assert!(matches!(cancelled, Err(LifecycleError::Cancelled)));
        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::Absent
        );
        assert!(lifecycle.open_current().unwrap().is_none());

        let completed = lifecycle
            .rebuild(|_| LifecycleControl::Continue)
            .expect("clean restart");
        assert_eq!(completed.head_sequence, 7);
        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::Current
        );
    }

    #[test]
    fn corrupt_visible_generation_is_quarantined_without_touching_truth() {
        let temp = populated_store(1);
        let lifecycle = active_lifecycle(temp.path());
        let receipt = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let generation = lifecycle
            .paths()
            .generation(receipt.generation_id.as_deref().unwrap());
        std::fs::write(generation.join("cursor.sqlite3"), b"not sqlite").unwrap();

        let status = lifecycle.status().unwrap();

        assert_eq!(status.availability, DerivedAccessAvailability::Quarantined);
        assert!(!lifecycle.paths().root().exists());
        assert_eq!(
            EventStore::open(temp.path()).list_events().unwrap().len(),
            1
        );
    }

    #[test]
    fn stale_quarantine_observation_is_revalidated_under_the_writer_lock() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();

        let status = lifecycle
            .quarantine_status("stale transient observation".to_owned())
            .unwrap();

        assert_eq!(status.availability, DerivedAccessAvailability::Current);
        assert!(lifecycle.paths().root().exists());
        assert_eq!(
            EventStore::open(temp.path()).list_events().unwrap().len(),
            7
        );
    }

    #[test]
    fn active_off_active_preserves_the_published_generation() {
        let temp = populated_store(1);
        let active = active_lifecycle(temp.path());
        let receipt = active.rebuild(|_| LifecycleControl::Continue).unwrap();
        let off = DerivedAccessLifecycle::new(DerivedAccessProfile::Off, temp.path(), "store:test")
            .unwrap();

        assert_eq!(
            off.status().unwrap().availability,
            DerivedAccessAvailability::Absent
        );
        assert_eq!(
            active.status().unwrap().generation_id,
            receipt.generation_id
        );
    }

    #[test]
    fn empty_d0_l1_and_l7_bootstrap_and_reopen_exactly() {
        for event_count in [0, 128, 1, 7] {
            let temp = populated_store(event_count);
            let lifecycle = active_lifecycle(temp.path());
            let receipt = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();

            assert_eq!(receipt.head_sequence, event_count as u64);
            assert_eq!(
                lifecycle.status().unwrap().availability,
                DerivedAccessAvailability::Current
            );
            assert_eq!(
                lifecycle
                    .open_current()
                    .unwrap()
                    .unwrap()
                    .service()
                    .locator_checkpoint()
                    .unwrap()
                    .sequence,
                event_count as u64
            );
        }
    }

    #[test]
    fn change_publication_binds_immutable_v3_and_its_live_checkpoint() {
        let temp = valid_change_store();
        let lifecycle = active_lifecycle(temp.path());
        let built = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let generation_id = built.generation_id.as_deref().unwrap();
        let publication = lifecycle.paths().current_publication().unwrap().unwrap();
        let descriptor = lifecycle.paths().descriptor(&publication).unwrap();
        let generation = lifecycle.paths().generation(generation_id);
        let receipt_path = generation.join(CHANGE_READER_PROFILE_RESOURCE);
        let receipt_bytes = fs::read(&receipt_path)
            .expect("published Change generation persists its immutable V3 reader receipt");
        let receipt = match probe_change_reader_profile_receipt(&receipt_bytes).unwrap() {
            ChangeReaderProfileReceiptProbeV1::Current(receipt) => receipt,
            ChangeReaderProfileReceiptProbeV1::RebuildRequired { schema, version } => {
                panic!("published Change generation used legacy receipt {schema:?} v{version:?}")
            }
        };

        receipt.validate_for_descriptor(&descriptor).unwrap();
        assert_eq!(descriptor.semantic_receipt, receipt.receipt_sha256);

        let database = generation.join("cursor.sqlite3");
        let connection = rusqlite::Connection::open(database).unwrap();
        let checkpoint_json = connection
            .query_row(
                "SELECT checkpoint_json
                 FROM reader_projection_checkpoint
                 WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("published Change generation persists one live reader checkpoint");
        let checkpoint: ReaderProjectionCheckpointV1 =
            serde_json::from_str(&checkpoint_json).unwrap();
        checkpoint.validate_for_receipt(&receipt).unwrap();
        assert_eq!(checkpoint.reader_receipt_sha256, receipt.receipt_sha256);
        drop(connection);

        lifecycle.open_current().unwrap().unwrap();
        assert_eq!(fs::read(receipt_path).unwrap(), receipt_bytes);
    }

    #[test]
    fn change_receipt_failures_are_classified_without_strict_fallback() {
        for (case, expected) in [
            ("absent", DerivedAccessAvailability::RebuildRequired),
            ("v2", DerivedAccessAvailability::RebuildRequired),
            ("malformed", DerivedAccessAvailability::Quarantined),
            ("self_hash", DerivedAccessAvailability::Quarantined),
            ("profile", DerivedAccessAvailability::Quarantined),
            (
                "checkpoint_anchor",
                DerivedAccessAvailability::RebuildRequired,
            ),
            (
                "checkpoint_schema",
                DerivedAccessAvailability::RebuildRequired,
            ),
        ] {
            let temp = valid_change_store();
            let lifecycle = active_lifecycle(temp.path());
            let built = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
            let generation = lifecycle
                .paths()
                .generation(built.generation_id.as_deref().unwrap());
            let receipt_path = generation.join(CHANGE_READER_PROFILE_RESOURCE);

            match case {
                "absent" => {
                    if receipt_path.exists() {
                        fs::remove_file(&receipt_path).unwrap();
                    }
                }
                "v2" => fs::write(
                    &receipt_path,
                    br#"{"schema":"pointbreak.change-reader-profile-receipt.v2","version":2}"#,
                )
                .unwrap(),
                "malformed" => fs::write(&receipt_path, b"{").unwrap(),
                "self_hash" | "profile" => {
                    let mut value: serde_json::Value =
                        serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
                    if case == "self_hash" {
                        value["receiptSha256"] =
                            serde_json::Value::String(format!("sha256:{}", "0".repeat(64)));
                    } else {
                        value["publicationProfile"] = serde_json::Value::String("off".to_owned());
                    }
                    fs::write(&receipt_path, canonical_json_bytes(&value).unwrap()).unwrap();
                }
                "checkpoint_anchor" | "checkpoint_schema" => {
                    let database = generation.join("cursor.sqlite3");
                    let connection = rusqlite::Connection::open(&database).unwrap();
                    let checkpoint_json: String = connection
                        .query_row(
                            "SELECT checkpoint_json
                             FROM reader_projection_checkpoint
                             WHERE singleton = 1",
                            [],
                            |row| row.get(0),
                        )
                        .unwrap();
                    let mut checkpoint: serde_json::Value =
                        serde_json::from_str(&checkpoint_json).unwrap();
                    if case == "checkpoint_anchor" {
                        checkpoint["readerReceiptSha256"] =
                            serde_json::Value::String(format!("sha256:{}", "0".repeat(64)));
                    } else {
                        checkpoint["schema"] = serde_json::Value::String(
                            "pointbreak.reader-projection-checkpoint.v2".to_owned(),
                        );
                        checkpoint["version"] = serde_json::Value::from(2);
                    }
                    connection
                        .execute(
                            "UPDATE reader_projection_checkpoint
                             SET checkpoint_json = ?1
                             WHERE singleton = 1",
                            [
                                String::from_utf8(canonical_json_bytes(&checkpoint).unwrap())
                                    .unwrap(),
                            ],
                        )
                        .unwrap();
                    connection
                        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
                        .unwrap();
                }
                _ => unreachable!(),
            }

            let scope = LongitudinalCountingScopeV1::new("d".repeat(64)).unwrap();
            let guard = scope.enter();
            let observed = lifecycle.status_read_only().unwrap();
            drop(guard);

            assert_eq!(observed.availability, expected, "receipt case {case}");
            let counters = scope.snapshot().counters;
            assert_eq!(counters.event_folds, 0, "receipt case {case}");
            assert_eq!(counters.projection_rebuilds, 0, "receipt case {case}");
            assert_eq!(counters.state_rebuilds, 0, "receipt case {case}");
            assert!(generation.exists(), "receipt case {case}");
        }
    }

    #[test]
    fn fresh_process_valid_l2_restart_is_bounded_and_receipt_backed() {
        let temp = valid_change_store();
        active_lifecycle(temp.path())
            .rebuild(|_| LifecycleControl::Continue)
            .unwrap();

        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "session::derived_access::lifecycle::tests::valid_l2_restart_child",
            ])
            .env("POINTBREAK_VALID_L2_RESTART_ROOT", temp.path())
            .status()
            .unwrap();

        assert!(result.success(), "fresh valid-L2 restart must stay bounded");
    }

    #[test]
    #[ignore = "spawned by fresh_process_valid_l2_restart_is_bounded_and_receipt_backed"]
    fn valid_l2_restart_child() {
        let root = std::env::var_os("POINTBREAK_VALID_L2_RESTART_ROOT").unwrap();
        let scope = LongitudinalCountingScopeV1::new("e".repeat(64)).unwrap();
        let guard = scope.enter();

        active_lifecycle(Path::new(&root))
            .open_current()
            .expect("valid L2 publication opens")
            .expect("valid L2 publication remains current");

        drop(guard);
        let counters = scope.snapshot().counters;
        assert_eq!(counters.directory_entries_walked, 0);
        assert_eq!(
            counters.carrier_opens, 2,
            "restart point-reads only activation and completion carriers"
        );
        assert_eq!(counters.event_folds, 0);
        assert_eq!(counters.projection_rebuilds, 0);
        assert_eq!(counters.state_rebuilds, 0);
    }

    #[test]
    fn every_publication_boundary_is_process_interruption_safe() {
        let cases = [
            ("StagingPrepared", DerivedAccessAvailability::Absent),
            (
                "CandidatePopulated",
                DerivedAccessAvailability::Bootstrapping,
            ),
            (
                "CandidateValidated",
                DerivedAccessAvailability::Bootstrapping,
            ),
            (
                "ReaderReceiptWritten",
                DerivedAccessAvailability::Bootstrapping,
            ),
            (
                "GenerationPromoted",
                DerivedAccessAvailability::RebuildRequired,
            ),
            ("CurrentPublished", DerivedAccessAvailability::Current),
            (
                "PriorPublicationRetired",
                DerivedAccessAvailability::Current,
            ),
        ];
        for (boundary, expected) in cases {
            let temp = if boundary == "ReaderReceiptWritten" {
                valid_change_store()
            } else {
                populated_store(7)
            };
            let lifecycle = active_lifecycle(temp.path());
            let result = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "session::derived_access::lifecycle::tests::lifecycle_publication_crash_child",
                ])
                .env("POINTBREAK_LIFECYCLE_CRASH_ROOT", temp.path())
                .env("POINTBREAK_LIFECYCLE_CRASH_BOUNDARY", boundary)
                .status()
                .unwrap();
            assert_eq!(result.code(), Some(91), "boundary {boundary}");
            assert_eq!(
                lifecycle.status().unwrap().availability,
                expected,
                "boundary {boundary}"
            );

            lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
            assert_eq!(
                lifecycle.status().unwrap().availability,
                DerivedAccessAvailability::Current
            );
        }
    }

    #[test]
    #[ignore = "spawned by every_publication_boundary_is_process_interruption_safe"]
    fn lifecycle_publication_crash_child() {
        let root = std::env::var_os("POINTBREAK_LIFECYCLE_CRASH_ROOT").unwrap();
        let boundary = std::env::var("POINTBREAK_LIFECYCLE_CRASH_BOUNDARY").unwrap();
        active_lifecycle(Path::new(&root))
            .rebuild_with_hook(
                |_| LifecycleControl::Continue,
                |observed| {
                    if format!("{observed:?}") == boundary {
                        std::process::exit(91);
                    }
                },
            )
            .unwrap();
        panic!("child did not observe requested publication boundary");
    }

    #[test]
    fn background_rebuild_status_does_not_wait_for_the_writer() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        let worker = lifecycle.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut reported = false;
            worker.rebuild(|progress| {
                if !reported && progress.completed == 0 {
                    reported = true;
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }
                LifecycleControl::Continue
            })
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        let status = lifecycle.status().unwrap();

        assert_eq!(
            status.availability,
            DerivedAccessAvailability::Bootstrapping
        );
        assert_eq!((status.completed, status.total), (Some(0), Some(7)));
        release_tx.send(()).unwrap();
        thread.join().unwrap().unwrap();
    }

    #[test]
    fn automatic_rebuild_stops_when_activation_wins_between_population_and_strict_replay() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        let backend = StoreBackend::Local(temp.path().to_path_buf());
        let mut activated = false;

        let result = lifecycle.try_automatic_legacy_rebuild(|progress| {
            if !activated
                && progress.phase == GenerationProgressPhase::ProjectionPopulation
                && progress.completed == 0
            {
                let _authority = StoreAuthorityLock::acquire(temp.path()).unwrap();
                write_capability_fixture_for_test(
                    backend.journal().as_ref(),
                    CapabilityFixtureState::L2,
                )
                .unwrap();
                activated = true;
            }
            LifecycleControl::Continue
        });

        assert!(activated);
        assert!(matches!(
            result,
            Err(LifecycleError::AutomaticRebuildSuppressed)
        ));
        assert_eq!(lifecycle.published_generation_id().unwrap(), None);
        assert!(lifecycle.paths().staging_progress().unwrap().is_none());
    }

    #[test]
    fn authority_held_rebuild_waits_for_a_background_lease_to_yield() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        let worker = lifecycle.clone();
        let (projection_tx, projection_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let background = std::thread::spawn(move || {
            let mut paused = false;
            worker.try_automatic_legacy_rebuild(|progress| {
                if !paused
                    && progress.phase == GenerationProgressPhase::ProjectionPopulation
                    && progress.completed == 0
                {
                    paused = true;
                    projection_tx.send(()).unwrap();
                    continue_rx.recv().unwrap();
                }
                LifecycleControl::Continue
            })
        });
        projection_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let authority = StoreAuthorityLock::acquire(temp.path()).unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            continue_tx.send(()).unwrap();
        });

        let receipt = lifecycle
            .rebuild(|_| LifecycleControl::Continue)
            .expect("the authority-owning rebuild waits for the background lease");

        assert!(matches!(
            background.join().unwrap(),
            Err(LifecycleError::RebuildBusy)
        ));
        assert_eq!(
            lifecycle.published_generation_id().unwrap(),
            receipt.generation_id
        );
        releaser.join().unwrap();
        drop(authority);
    }

    #[test]
    fn synchronous_l0_rebuild_publishes_before_a_waiting_activation() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        let root = temp.path().to_path_buf();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let mut contender = None;

        lifecycle
            .rebuild_with_hook(
                |_| LifecycleControl::Continue,
                |boundary| match boundary {
                    PublicationBoundary::CandidateValidated => {
                        let acquired_tx = acquired_tx.clone();
                        let root = root.clone();
                        contender = Some(std::thread::spawn(move || {
                            let _authority = StoreAuthorityLock::acquire(&root).unwrap();
                            acquired_tx.send(()).unwrap();
                            let backend = StoreBackend::Local(root);
                            write_capability_fixture_for_test(
                                backend.journal().as_ref(),
                                CapabilityFixtureState::L2,
                            )
                            .unwrap();
                        }));
                        assert!(
                            acquired_rx
                                .recv_timeout(Duration::from_millis(100))
                                .is_err(),
                            "activation must wait while the L0 rebuild owns authority"
                        );
                    }
                    PublicationBoundary::CurrentPublished => assert!(
                        acquired_rx
                            .recv_timeout(Duration::from_millis(100))
                            .is_err(),
                        "activation must remain excluded until current publication"
                    ),
                    _ => {}
                },
            )
            .unwrap();

        acquired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        contender.unwrap().join().unwrap();
    }

    #[test]
    fn status_does_not_wait_during_generation_publication() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let worker = lifecycle.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            worker.rebuild_with_hook(
                |_| LifecycleControl::Continue,
                |boundary| {
                    if boundary == PublicationBoundary::GenerationPromoted {
                        started_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                    }
                },
            )
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let observer = lifecycle.clone();
        let (status_tx, status_rx) = mpsc::channel();
        let status_thread = std::thread::spawn(move || status_tx.send(observer.status()).unwrap());

        let status = status_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("status must not wait for the publication lock")
            .unwrap();

        assert_eq!(status.availability, DerivedAccessAvailability::Current);
        release_tx.send(()).unwrap();
        status_thread.join().unwrap();
        thread.join().unwrap().unwrap();
    }

    #[test]
    fn a_second_rebuild_is_rejected_without_disturbing_the_first() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        let worker = lifecycle.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut reported = false;
            worker.rebuild(|progress| {
                if !reported && progress.completed == 0 {
                    reported = true;
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }
                LifecycleControl::Continue
            })
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        assert!(matches!(
            lifecycle.rebuild(|_| LifecycleControl::Continue),
            Err(LifecycleError::RebuildBusy)
        ));
        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::Bootstrapping
        );

        release_tx.send(()).unwrap();
        thread.join().unwrap().unwrap();
        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::Current
        );
    }

    #[test]
    fn same_thread_rebuild_reentry_is_rejected_without_waiting() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        let mut reentered = false;

        lifecycle
            .rebuild(|_| {
                if !reentered {
                    reentered = true;
                    assert!(matches!(
                        lifecycle.rebuild(|_| LifecycleControl::Continue),
                        Err(LifecycleError::RebuildBusy)
                    ));
                }
                LifecycleControl::Continue
            })
            .unwrap();

        assert!(reentered);
        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::Current
        );
    }

    #[test]
    fn failed_candidate_validation_discards_staging() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        let staging_root = lifecycle.paths().root().join("staging");
        let result = lifecycle.rebuild_with_hook(
            |_| LifecycleControl::Continue,
            |boundary| {
                if boundary == PublicationBoundary::CandidatePopulated {
                    let generation = std::fs::read_dir(&staging_root)
                        .unwrap()
                        .next()
                        .unwrap()
                        .unwrap()
                        .path();
                    std::fs::write(generation.join("cursor.sqlite3"), b"invalid").unwrap();
                }
            },
        );

        assert!(result.is_err());
        assert!(std::fs::read_dir(&staging_root).unwrap().next().is_none());
        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::Absent
        );
    }

    #[test]
    fn an_out_of_band_truth_append_requires_rebuild_without_serving_stale_state() {
        let temp = populated_store(1);
        let lifecycle = active_lifecycle(temp.path());
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let store = EventStore::open(temp.path());
        assert_eq!(
            store.record_event_once(&lifecycle_event(2)).unwrap(),
            EventWriteOutcome::Created
        );

        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::RebuildRequired
        );
        assert!(matches!(
            lifecycle.open_current(),
            Err(LifecycleError::RebuildRequired(_))
        ));
    }

    #[test]
    fn legacy_generation_descriptor_requires_rebuild_without_quarantine() {
        let temp = populated_store(1);
        let lifecycle = active_lifecycle(temp.path());
        let receipt = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let generation = lifecycle
            .paths()
            .generation(receipt.generation_id.as_deref().unwrap());
        let descriptor_path = generation.join("generation.json");
        let mut descriptor: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&descriptor_path).unwrap()).unwrap();
        descriptor["schema"] =
            serde_json::Value::String("pointbreak.derived-access-generation.v1".to_owned());
        descriptor.as_object_mut().unwrap().remove("authorityStamp");
        let descriptor_bytes = canonical_json_bytes(&descriptor).unwrap();
        std::fs::write(&descriptor_path, &descriptor_bytes).unwrap();

        let publication_path = std::fs::read_dir(lifecycle.paths().root().join("publications"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut publication: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&publication_path).unwrap()).unwrap();
        publication["descriptorSha256"] =
            serde_json::Value::String(sha256_bytes_hex(&descriptor_bytes));
        std::fs::write(
            publication_path,
            canonical_json_bytes(&publication).unwrap(),
        )
        .unwrap();

        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::RebuildRequired
        );
        assert!(lifecycle.paths().root().exists());
    }

    #[test]
    fn legacy_cursor_schema_requires_rebuild_without_quarantine() {
        let temp = populated_store(1);
        let lifecycle = active_lifecycle(temp.path());
        let receipt = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let database = lifecycle
            .paths()
            .generation(receipt.generation_id.as_deref().unwrap())
            .join("cursor.sqlite3");
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection.pragma_update(None, "user_version", 3).unwrap();
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        drop(connection);

        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::RebuildRequired
        );
        assert!(lifecycle.paths().root().exists());
    }

    #[test]
    fn published_generation_without_product_history_schema_requires_rebuild_without_mutation() {
        let temp = populated_store(1);
        let lifecycle = active_lifecycle(temp.path());
        let receipt = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let generation = lifecycle
            .paths()
            .generation(receipt.generation_id.as_deref().unwrap());
        let database = generation.join("cursor.sqlite3");
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "DROP TABLE product_revision_edge;
                 DROP TABLE product_revision;
                 DROP TABLE product_history_signature;
                 DROP TABLE product_history_tag;
                 DROP TABLE product_history_meta;
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .unwrap();
        drop(connection);

        let observed = lifecycle.status().unwrap();

        assert_eq!(
            observed.availability,
            DerivedAccessAvailability::RebuildRequired
        );
        let connection = rusqlite::Connection::open(database).unwrap();
        let product_tables = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema
                 WHERE type = 'table' AND name LIKE 'product_history_%'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(product_tables, 0);
    }

    #[test]
    fn published_generation_with_older_product_history_schema_requires_rebuild() {
        let temp = populated_store(1);
        let lifecycle = active_lifecycle(temp.path());
        let receipt = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let generation = lifecycle
            .paths()
            .generation(receipt.generation_id.as_deref().unwrap());
        let database = generation.join("cursor.sqlite3");
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "PRAGMA ignore_check_constraints = ON;
                 UPDATE product_history_meta SET schema_version = 1 WHERE singleton = 1;
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .unwrap();
        drop(connection);

        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::RebuildRequired
        );
        assert!(matches!(
            lifecycle.open_current(),
            Err(LifecycleError::RebuildRequired(_))
        ));
        assert!(
            generation.exists(),
            "stale generation must not be quarantined"
        );
    }

    #[test]
    fn published_generation_with_older_locator_schema_requires_rebuild() {
        let temp = populated_store(1);
        let lifecycle = active_lifecycle(temp.path());
        let receipt = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let generation = lifecycle
            .paths()
            .generation(receipt.generation_id.as_deref().unwrap());
        let database = generation.join("cursor.sqlite3");
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "PRAGMA ignore_check_constraints = ON;
                 UPDATE locator_checkpoint SET schema_version = 2 WHERE singleton = 1;
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .unwrap();
        drop(connection);

        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::RebuildRequired
        );
        assert!(matches!(
            lifecycle.open_current(),
            Err(LifecycleError::RebuildRequired(_))
        ));
        assert!(
            generation.exists(),
            "older locator generation must remain available for deliberate rebuild"
        );
    }

    #[test]
    fn published_generation_with_older_semantic_schema_requires_rebuild_before_ddl() {
        let temp = populated_store(1);
        let lifecycle = active_lifecycle(temp.path());
        let receipt = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let generation = lifecycle
            .paths()
            .generation(receipt.generation_id.as_deref().unwrap());
        let database = generation.join("cursor.sqlite3");
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "PRAGMA ignore_check_constraints = ON;
                 UPDATE semantic_meta SET schema_version = 4 WHERE singleton = 1;
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .unwrap();
        drop(connection);

        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::RebuildRequired
        );
        assert!(matches!(
            lifecycle.open_current(),
            Err(LifecycleError::RebuildRequired(_))
        ));
        assert!(
            generation.exists(),
            "upgradeable generation must not be quarantined"
        );
    }

    #[test]
    fn wrong_store_profile_and_schema_are_quarantined() {
        for mutation in ["store", "profile", "schema"] {
            let temp = populated_store(1);
            let lifecycle = active_lifecycle(temp.path());
            let receipt = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
            let database = lifecycle
                .paths()
                .generation(receipt.generation_id.as_deref().unwrap())
                .join("cursor.sqlite3");
            let connection = rusqlite::Connection::open(&database).unwrap();
            match mutation {
                "store" => {
                    connection
                        .execute(
                            "UPDATE cursor_meta SET store_id = 'store:other' WHERE singleton = 1",
                            [],
                        )
                        .unwrap();
                }
                "profile" => {
                    connection
                        .execute(
                            "UPDATE cursor_meta SET profile_id = 'profile:other' WHERE singleton = 1",
                            [],
                        )
                        .unwrap();
                }
                "schema" => connection.pragma_update(None, "user_version", 99).unwrap(),
                _ => unreachable!(),
            }
            drop(connection);

            assert_eq!(
                lifecycle.status().unwrap().availability,
                DerivedAccessAvailability::Quarantined,
                "mutation {mutation}"
            );
            assert_eq!(
                EventStore::open(temp.path()).list_events().unwrap().len(),
                1
            );
        }
    }

    #[test]
    fn corrupt_uncheckpointed_wal_is_quarantined() {
        let temp = populated_store(1);
        let lifecycle = active_lifecycle(temp.path());
        let receipt = lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let database = lifecycle
            .paths()
            .generation(receipt.generation_id.as_deref().unwrap())
            .join("cursor.sqlite3");
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "session::derived_access::lifecycle::tests::lifecycle_wal_child",
            ])
            .env("POINTBREAK_LIFECYCLE_WAL_DATABASE", &database)
            .status()
            .unwrap();
        assert_eq!(result.code(), Some(92));
        let wal = database.with_file_name("cursor.sqlite3-wal");
        assert!(wal.exists());
        std::fs::write(&wal, b"corrupt WAL").unwrap();

        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::Quarantined
        );
        assert_eq!(
            EventStore::open(temp.path()).list_events().unwrap().len(),
            1
        );
    }

    #[test]
    #[ignore = "spawned by corrupt_uncheckpointed_wal_is_quarantined"]
    fn lifecycle_wal_child() {
        let database = std::env::var_os("POINTBREAK_LIFECYCLE_WAL_DATABASE").unwrap();
        let connection = rusqlite::Connection::open(Path::new(&database)).unwrap();
        connection
            .pragma_update(None, "wal_autocheckpoint", 0)
            .unwrap();
        connection
            .execute(
                "UPDATE cursor_meta SET quarantine_reason = 'wal-child' WHERE singleton = 1",
                [],
            )
            .unwrap();
        std::process::exit(92);
    }

    #[test]
    fn retire_delete_and_rebuild_are_explicit_and_truth_preserving() {
        let temp = populated_store(7);
        let lifecycle = active_lifecycle(temp.path());
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();

        let publication = lifecycle.paths.current_publication().unwrap().unwrap();
        let lease = lifecycle
            .paths
            .acquire_read_lease(&publication.generation_id)
            .unwrap();
        let retired = lifecycle.retire().unwrap().unwrap();
        assert!(retired.exists());
        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::Absent
        );
        assert!(matches!(
            lifecycle.purge_disposable_root(&retired),
            Err(LifecycleError::Generation(GenerationError::GenerationInUse))
        ));
        drop(lease);
        lifecycle.purge_disposable_root(&retired).unwrap();
        assert!(!retired.exists());
        assert!(
            !lifecycle
                .paths
                .generation_lease_path(&publication.generation_id)
                .exists()
        );
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let publication = lifecycle.paths.current_publication().unwrap().unwrap();
        let lease_path = lifecycle
            .paths
            .generation_lease_path(&publication.generation_id);
        lifecycle.status().unwrap();
        assert!(lease_path.exists());
        lifecycle.delete().unwrap();
        assert!(!lease_path.exists());
        assert_eq!(
            lifecycle.status().unwrap().availability,
            DerivedAccessAvailability::Absent
        );
        assert_eq!(
            EventStore::open(temp.path()).list_events().unwrap().len(),
            7
        );
    }

    fn stable_change_check(after: JournalChangeStamp) -> JournalChangeCheck {
        JournalChangeCheck {
            after,
            verdict: JournalChangeVerdict::Stable,
            native_bytes_examined: 4096,
            native_records_examined: 1,
            relevant_file_references: Vec::new(),
            mechanism: "bounded stable continuation".to_owned(),
        }
    }

    fn active_lifecycle(root: &std::path::Path) -> DerivedAccessLifecycle {
        DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            root,
            "store:test",
        )
        .unwrap()
    }

    fn populated_store(event_count: usize) -> TempDir {
        let temp = TempDir::new().unwrap();
        let store = EventStore::open(temp.path());
        for index in 0..event_count {
            assert_eq!(
                store.record_event_once(&lifecycle_event(index)).unwrap(),
                EventWriteOutcome::Created
            );
        }
        temp
    }

    fn valid_change_store() -> TempDir {
        let temp = TempDir::new().unwrap();
        let backend = StoreBackend::Local(temp.path().to_path_buf());
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();
        temp
    }

    fn lifecycle_event(index: usize) -> ShoreEvent {
        let journal_id = JournalId::new(format!("journal:lifecycle:{index}"));
        ShoreEvent::new(
            EventType::ReviewInitialized,
            ReviewInitializedPayload::idempotency_key(&journal_id),
            EventTarget::for_journal(journal_id),
            Writer::shore_local("test"),
            ReviewInitializedPayload {},
            format!("2026-07-28T00:{:02}:{:02}Z", index / 60, index % 60),
        )
        .unwrap()
    }
}
