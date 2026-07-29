//! Dormant derived-access lifecycle, rebuild, and immutable-generation manager.
#![cfg_attr(not(test), allow(dead_code))]

use std::path::{Path, PathBuf};

use super::generation::{
    GenerationDescriptor, GenerationError, GenerationLayout, GenerationPublication,
    GenerationReadLease,
};
use super::locator::LocatorRead;
use super::product_contract::{DerivedAccessAvailability, DerivedAccessProfile};
use super::service::{DerivedAccessService, DerivedAccessServiceError};
use super::sqlite::{
    BootstrapControl, BootstrapProgress, CursorLedgerError, CursorLedgerIdentity,
    SqliteCursorLedger, SqliteLocatorError, SqliteSemanticError, StoreWriterLock, WriterLockError,
};
use super::verification::strict_bodyless_materialized_snapshot_at;
use crate::session::EventStore;
use crate::session::derived_access::QualificationLocalJournal;

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
    GenerationPromoted,
    CurrentPublished,
    PriorPublicationRetired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LifecycleProgress {
    pub(crate) completed: usize,
    pub(crate) total: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LifecycleStatus {
    pub(crate) availability: DerivedAccessAvailability,
    pub(crate) generation_id: Option<String>,
    pub(crate) completed: Option<usize>,
    pub(crate) total: Option<usize>,
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
    generation_id: String,
    service: DerivedAccessService,
    _lease: GenerationReadLease,
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
            paths: GenerationLayout::new(store_root),
        })
    }

    pub(crate) fn paths(&self) -> &GenerationLayout {
        &self.paths
    }

    pub(crate) fn store_root(&self) -> &Path {
        &self.store_root
    }

    pub(crate) fn status(&self) -> Result<LifecycleStatus, LifecycleError> {
        if self.profile == DerivedAccessProfile::Off || !self.paths.root().exists() {
            return Ok(status(DerivedAccessAvailability::Absent, None, None));
        }
        let publication = match self.paths.current_publication() {
            Ok(publication) => publication,
            Err(error) => return self.generation_error_status(error),
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
                completed: staging_progress.map(|progress| progress.completed),
                total: staging_progress.map(|progress| progress.total),
                detail: None,
            });
        };
        if let Some(progress) = staging_progress {
            return Ok(LifecycleStatus {
                availability: DerivedAccessAvailability::Bootstrapping,
                generation_id: Some(publication.generation_id),
                completed: Some(progress.completed),
                total: Some(progress.total),
                detail: Some("current generation remains readable during rebuild".to_owned()),
            });
        }
        let _lease = match self.paths.acquire_read_lease(&publication.generation_id) {
            Ok(lease) => lease,
            Err(error) => return self.generation_error_status(error),
        };
        let descriptor = match self.paths.descriptor(&publication) {
            Ok(descriptor) => descriptor,
            Err(error) => return self.generation_error_status(error),
        };
        if let Err(error) = self.validate_descriptor(&descriptor) {
            return self.quarantine_status(error.to_string());
        }
        let generation_root = self.paths.generation(&publication.generation_id);
        if let Err(error) = validate_wal_shape(&generation_root) {
            return self.lifecycle_error_status(error);
        }
        let service = match DerivedAccessService::open_at(
            &self.store_root,
            &generation_root,
            CursorLedgerIdentity::new(self.store_id.clone()),
        ) {
            Ok(service) => service,
            Err(error) if service_error_requires_quarantine(&error) => {
                return self.quarantine_status(error.to_string());
            }
            Err(error) => {
                return Ok(status(
                    DerivedAccessAvailability::Unavailable,
                    Some(publication.generation_id),
                    Some(error.to_string()),
                ));
            }
        };
        let observed_sequence = match service.truth_head() {
            Ok(head) => head.cursor.sequence,
            Err(error) => {
                return Ok(status(
                    DerivedAccessAvailability::Unavailable,
                    Some(publication.generation_id),
                    Some(error.to_string()),
                ));
            }
        };
        let truth_count = match self.truth_head_marker() {
            Ok(count) => count,
            Err(error) => {
                return Ok(status(
                    DerivedAccessAvailability::Unavailable,
                    Some(publication.generation_id),
                    Some(error.to_string()),
                ));
            }
        };
        if observed_sequence != truth_count {
            return Ok(status(
                DerivedAccessAvailability::RebuildRequired,
                Some(publication.generation_id),
                Some(format!(
                    "derived head {observed_sequence} does not cover truth count {truth_count}"
                )),
            ));
        }
        if let Err(error) = validate_published(&service, &descriptor, truth_count) {
            return self.quarantine_status(error.to_string());
        }
        if service.locator_checkpoint()? != service.truth_head()?.cursor {
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

    pub(crate) fn rebuild_with_hook(
        &self,
        mut progress: impl FnMut(LifecycleProgress) -> LifecycleControl,
        mut hook: impl FnMut(PublicationBoundary),
    ) -> Result<LifecycleReceipt, LifecycleError> {
        if self.profile == DerivedAccessProfile::Off {
            return Err(LifecycleError::Disabled);
        }
        let _rebuild_lease = match self.paths.try_rebuild_lease() {
            Ok(lease) => lease,
            Err(GenerationError::RebuildBusy) => return Err(LifecycleError::RebuildBusy),
            Err(error) => return Err(error.into()),
        };
        let truth_before = self.truth_events()?;
        self.paths.ensure_scaffold()?;
        self.paths.discard_all_staging()?;
        let (sequence, generation_id) = self.paths.next_generation()?;
        let staging = self.paths.staging(&generation_id);
        hook(PublicationBoundary::StagingPrepared);

        let candidate_result: Result<_, LifecycleError> = (|| {
            let mut progress_error = None;
            let bootstrap = SqliteCursorLedger::bootstrap_from_truth_at(
                &self.store_root,
                &staging,
                CursorLedgerIdentity::new(self.store_id.clone()),
                sequence,
                |update| {
                    if let Err(error) =
                        self.paths
                            .record_progress(&generation_id, update.completed, update.total)
                    {
                        progress_error = Some(error);
                        return BootstrapControl::Cancel;
                    }
                    match progress(LifecycleProgress::from(update)) {
                        LifecycleControl::Continue => BootstrapControl::Continue,
                        LifecycleControl::Cancel => BootstrapControl::Cancel,
                    }
                },
            );
            if let Some(error) = progress_error {
                return Err(error.into());
            }
            if matches!(bootstrap, Err(CursorLedgerError::BootstrapCancelled)) {
                return Err(LifecycleError::Cancelled);
            }
            bootstrap?;
            hook(PublicationBoundary::CandidatePopulated);

            let service = DerivedAccessService::open_at(
                &self.store_root,
                &staging,
                CursorLedgerIdentity::new(self.store_id.clone()),
            )?;
            service.catch_up_to_head(512)?;
            let head = service.truth_head()?.cursor;
            let semantic_snapshot = validate_candidate(
                &service,
                &GenerationDescriptor::new(
                    &generation_id,
                    &self.store_id,
                    self.profile,
                    sequence,
                    head.sequence,
                    "",
                ),
                &truth_before,
            )?;
            let semantic_receipt = semantic_snapshot.semantic_receipt;
            let descriptor = GenerationDescriptor::new(
                &generation_id,
                &self.store_id,
                self.profile,
                head.epoch,
                head.sequence,
                &semantic_receipt,
            );
            let descriptor_sha256 = self.paths.write_descriptor(&staging, &descriptor)?;
            hook(PublicationBoundary::CandidateValidated);
            drop(service);
            self.paths.clear_progress(&generation_id)?;
            Ok((head, semantic_receipt, descriptor_sha256))
        })();
        let (head, semantic_receipt, descriptor_sha256) = match candidate_result {
            Ok(candidate) => candidate,
            Err(error) => {
                self.paths.discard_staging(&generation_id)?;
                return Err(error);
            }
        };

        let _writer_lock = StoreWriterLock::acquire(&self.store_root)?;
        let truth_after = match self.truth_events() {
            Ok(events) => events,
            Err(error) => {
                self.paths.discard_staging(&generation_id)?;
                return Err(error);
            }
        };
        if truth_after != truth_before {
            self.paths.discard_staging(&generation_id)?;
            return Err(LifecycleError::TruthChanged);
        }
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
        if self.profile == DerivedAccessProfile::Off || !self.paths.root().exists() {
            return Ok(None);
        }
        let publication = match self.paths.current_publication() {
            Ok(publication) => publication,
            Err(error) => return Err(self.generation_open_error(error)),
        };
        let Some(publication) = publication else {
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
            .map_err(|error| self.quarantine_error(error.to_string()))?;
        let generation_root = self.paths.generation(&publication.generation_id);
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
            if service_error_requires_quarantine(&error) {
                self.quarantine_error(error.to_string())
            } else {
                LifecycleError::Service(error)
            }
        })?;
        let truth_count = self.truth_head_marker()?;
        if service.truth_head()?.cursor.sequence != truth_count {
            return Err(LifecycleError::RebuildRequired(format!(
                "published generation does not cover {truth_count} truth events"
            )));
        }
        validate_published(&service, &descriptor, truth_count)
            .map_err(|error| self.quarantine_error(error.to_string()))?;
        Ok(Some(CurrentGeneration {
            generation_id: publication.generation_id,
            service,
            _lease: lease,
        }))
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
        let service = DerivedAccessService::open_at(
            &self.store_root,
            &generation_root,
            CursorLedgerIdentity::new(self.store_id.clone()),
        )?;
        let derived_head = service.truth_head()?.cursor.sequence;
        validate_published(&service, &descriptor, derived_head)?;
        Ok(Some(CurrentGeneration {
            generation_id: publication.generation_id,
            service,
            _lease: lease,
        }))
    }

    /// Admit a product writer against a stable current generation. The exact
    /// loose-directory audit runs while the canonical writer lock excludes both
    /// truth publication and the separately reacquired projection catch-up phase.
    pub(crate) fn admit_writer(&self) -> Result<bool, LifecycleError> {
        let writer_lock = StoreWriterLock::acquire(&self.store_root)?;
        let Some(current) = self.open_current_for_write_locked(&writer_lock)? else {
            return Ok(false);
        };
        let truth_count = self.truth_head_marker()?;
        let derived_count = current.service().truth_head()?.cursor.sequence;
        if derived_count != truth_count {
            return Err(LifecycleError::RebuildRequired(format!(
                "published generation does not cover {truth_count} truth events"
            )));
        }
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

    fn truth_events(&self) -> Result<Vec<crate::session::event::ShoreEvent>, LifecycleError> {
        EventStore::open(&self.store_root)
            .list_events()
            .map_err(|error| LifecycleError::Truth(error.to_string()))
    }

    fn staging_progress(&self) -> Result<Option<LifecycleProgress>, LifecycleError> {
        Ok(self
            .paths
            .staging_progress()?
            .map(|progress| LifecycleProgress {
                completed: progress.completed,
                total: progress.total,
            }))
    }

    fn truth_head_marker(&self) -> Result<u64, LifecycleError> {
        QualificationLocalJournal::new(&self.store_root)
            .head_marker()
            .map_err(|error| LifecycleError::Truth(error.to_string()))
    }

    fn quarantine_status(&self, reason: String) -> Result<LifecycleStatus, LifecycleError> {
        match StoreWriterLock::try_acquire(&self.store_root) {
            Ok(_lock) => {
                self.paths.quarantine(&reason)?;
                Ok(status(
                    DerivedAccessAvailability::Quarantined,
                    None,
                    Some(reason),
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
    ) -> Result<LifecycleStatus, LifecycleError> {
        if generation_error_requires_quarantine(&error) {
            self.quarantine_status(error.to_string())
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
    ) -> Result<LifecycleStatus, LifecycleError> {
        if lifecycle_error_requires_quarantine(&error) {
            self.quarantine_status(error.to_string())
        } else {
            Ok(status(
                DerivedAccessAvailability::Unavailable,
                None,
                Some(error.to_string()),
            ))
        }
    }

    fn generation_open_error(&self, error: GenerationError) -> LifecycleError {
        if generation_error_requires_quarantine(&error) {
            self.quarantine_error(error.to_string())
        } else {
            LifecycleError::Generation(error)
        }
    }
}

impl CurrentGeneration {
    pub(crate) fn generation_id(&self) -> &str {
        &self.generation_id
    }

    pub(crate) fn service(&self) -> &DerivedAccessService {
        &self.service
    }
}

impl From<BootstrapProgress> for LifecycleProgress {
    fn from(progress: BootstrapProgress) -> Self {
        Self {
            completed: progress.completed,
            total: progress.total,
        }
    }
}

pub(crate) fn lifecycle_transition_allowed(
    current: DerivedAccessAvailability,
    successor: DerivedAccessAvailability,
) -> bool {
    current.allows(successor)
}

fn validate_candidate(
    service: &DerivedAccessService,
    descriptor: &GenerationDescriptor,
    truth_events: &[crate::session::event::ShoreEvent],
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
    service: &DerivedAccessService,
    descriptor: &GenerationDescriptor,
    truth_count: u64,
) -> Result<(), LifecycleError> {
    let head = service.truth_head()?.cursor;
    let checkpoint = service.locator_checkpoint()?;
    if head.epoch != descriptor.epoch
        || head.sequence < descriptor.head_sequence
        || head.sequence != truth_count
        || checkpoint.epoch != head.epoch
        || checkpoint.sequence > head.sequence
    {
        return Err(LifecycleError::Validation(format!(
            "published coverage mismatch: head={head:?}, checkpoint={checkpoint:?}, \
             descriptor={}:{}, truth={truth_count}",
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
        DerivedAccessServiceError::Truth(_) | DerivedAccessServiceError::ZeroBatchLimit => false,
    }
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
        completed: None,
        total: None,
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
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::model::JournalId;
    use crate::session::derived_access::product_contract::{
        DerivedAccessAvailability, DerivedAccessProfile,
    };
    use crate::session::event::{
        EventTarget, EventType, ReviewInitializedPayload, ShoreEvent, Writer,
    };
    use crate::session::{EventStore, EventWriteOutcome};

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
    fn every_publication_boundary_is_process_interruption_safe() {
        let cases = [
            (
                PublicationBoundary::StagingPrepared,
                DerivedAccessAvailability::Absent,
            ),
            (
                PublicationBoundary::CandidatePopulated,
                DerivedAccessAvailability::Bootstrapping,
            ),
            (
                PublicationBoundary::CandidateValidated,
                DerivedAccessAvailability::Bootstrapping,
            ),
            (
                PublicationBoundary::GenerationPromoted,
                DerivedAccessAvailability::RebuildRequired,
            ),
            (
                PublicationBoundary::CurrentPublished,
                DerivedAccessAvailability::Current,
            ),
            (
                PublicationBoundary::PriorPublicationRetired,
                DerivedAccessAvailability::Current,
            ),
        ];
        for (boundary, expected) in cases {
            let temp = populated_store(7);
            let lifecycle = active_lifecycle(temp.path());
            let result = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "session::derived_access::lifecycle::tests::lifecycle_publication_crash_child",
                ])
                .env("POINTBREAK_LIFECYCLE_CRASH_ROOT", temp.path())
                .env(
                    "POINTBREAK_LIFECYCLE_CRASH_BOUNDARY",
                    format!("{boundary:?}"),
                )
                .status()
                .unwrap();
            assert_eq!(result.code(), Some(91), "boundary {boundary:?}");
            assert_eq!(
                lifecycle.status().unwrap().availability,
                expected,
                "boundary {boundary:?}"
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
