//! Product-domain contract for Change-aware derived reads.
#![cfg_attr(not(test), allow(dead_code))]
#![deny(private_bounds, private_interfaces)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use super::change_revision_reads::ExactRevisionSessionStateV1;
use super::lifecycle::LifecycleError;
use super::locator::LocatorRead;
use super::product_contract::{DerivedAccessAvailability, DerivedAccessProfile};
use super::runtime::{
    DerivedAccessMode, DerivedAccessRuntime, RuntimeCurrentRead, RuntimeCurrentStatus,
};
#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::{
    LongitudinalDerivedAccessPhaseV1 as Phase, enter_derived_access_phase_v1,
    record_change_candidate_current_revisions, record_change_candidates, record_change_matches,
    record_change_proposal_carriers_opened, record_change_proposal_carriers_validated,
    record_change_rows_emitted, record_change_support_carriers_opened,
};
use crate::canonical_hash::sha256_bytes_hex;
use crate::documents::{
    ChangeAttentionPresentationDocumentV2, ChangeAttentionPresentationV1,
    ChangeAttentionReasonPresentationV1, ChangeAttentionReasonV1, ChangeDocumentFacadeV1,
    ChangeListDocumentV1, ChangeListPresentationDocumentV1, ChangeQueryUnavailableDocumentV1,
    ChangeSummaryV1, ReaderProfileDocumentV1, ReaderUpgradeRequiredDocumentV1,
    attention_presentation_for_change,
};
use crate::error::{Result, ShoreError};
use crate::model::{ChangeId, RevisionRefV1};
use crate::session::event::{EventType, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload};
use crate::session::store::capabilities::{
    StoreCapabilityInspection, StoreCapabilityStatus, change_reader_activation_exists,
    inspect_change_reader_journal_records,
};
use crate::session::store::resolution::resolve_change_read_backend;
use crate::session::{
    AuthorityCursorV2, ChangeDocumentProjectionV1, ChangeLifecycleV1, ChangeProjection,
    ChangeTopologyV1, ChangeView, RevisionShowResult,
};

const AUTHORITY_ERROR_SCHEMA: &str = "pointbreak.inspect-change-authority-error";
const PROJECTION_ERROR_SCHEMA: &str = "pointbreak.inspect-change-projection-error";
const ERROR_DOCUMENT_VERSION: u32 = 1;
const DEFAULT_PAGE_LIMIT: usize = 50;
const MAXIMUM_PAGE_LIMIT: usize = 100;
const MAXIMUM_SUMMARY_QUERY_BYTES: usize = 256;

/// Thin product facade consumed by the Inspector binary.
#[doc(hidden)]
#[derive(Clone)]
pub struct DerivedChangeAccess {
    runtime: Arc<DerivedAccessRuntime>,
    repo: Option<PathBuf>,
}

impl DerivedChangeAccess {
    pub(crate) fn from_runtime(runtime: Arc<DerivedAccessRuntime>) -> Self {
        Self {
            runtime,
            repo: None,
        }
    }

    pub fn resolve_for_inspector(repo: impl AsRef<Path>) -> Result<Self> {
        Self::resolve_for_read(repo)
    }

    /// CLI-side sibling of [`Self::resolve_for_inspector`]. Both constructors
    /// share one resolution body; the distinct names keep each caller's
    /// surface explicit without renaming the shipped Inspector entry point.
    pub fn resolve_for_command(repo: impl AsRef<Path>) -> Result<Self> {
        Self::resolve_for_read(repo)
    }

    fn resolve_for_read(repo: impl AsRef<Path>) -> Result<Self> {
        let repo = repo.as_ref().to_path_buf();
        let profile = DerivedAccessProfile::from_environment()
            .map_err(|error| ShoreError::Message(error.to_string()))?;
        if profile == DerivedAccessProfile::Off {
            return Ok(Self {
                runtime: DerivedAccessRuntime::from_mode(DerivedAccessMode::Off),
                repo: Some(repo),
            });
        }
        let read_store = resolve_change_read_backend(&repo)
            .map_err(|error| ShoreError::Message(error.to_string()))?;
        let runtime =
            DerivedAccessRuntime::from_read_store(read_store).map_err(ShoreError::Message)?;
        Ok(Self {
            runtime,
            repo: Some(repo),
        })
    }

    pub fn profile(&self) -> Result<DerivedChangeOutcomeV1<ReaderProfileDocumentV1>> {
        Ok(self
            .profile_with_source()?
            .map_ready(|(document, _)| document))
    }

    /// [`Self::profile`] plus where a Ready answer came from, so a caller that
    /// labels its route can tell the proven-current generation apart from the
    /// authoritative capability control path (non-L2 stores).
    pub fn profile_with_source(
        &self,
    ) -> Result<DerivedChangeOutcomeV1<(ReaderProfileDocumentV1, DerivedReadSourceV1)>> {
        let control = |outcome: DerivedChangeOutcomeV1<ReaderProfileDocumentV1>| {
            outcome.map_ready(|document| (document, DerivedReadSourceV1::CapabilityControlPath))
        };
        let current = match self.runtime.current() {
            Ok(RuntimeCurrentRead::Ready(current)) => current,
            Ok(RuntimeCurrentRead::Unavailable(status)) => {
                return Ok(control(self.profile_control_outcome(status)));
            }
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error,
                ));
            }
        };
        let generation_id = current.generation_id().to_owned();
        let checkpoint = match current.pin_change_reader_checkpoint() {
            Ok(checkpoint) => checkpoint,
            Err(error) => return Ok(control(self.profile_receipt_failure_outcome(error))),
        };
        let document = match current.reader_profile_document(&checkpoint) {
            Ok(document) => document,
            Err(error) => return Ok(control(self.profile_receipt_failure_outcome(error))),
        };

        let final_current = match self.runtime.current() {
            Ok(RuntimeCurrentRead::Ready(current)) => current,
            Ok(RuntimeCurrentRead::Unavailable(_)) | Err(_) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionUnstable,
                    "derived Change profile moved before response completion",
                ));
            }
        };
        if final_current.generation_id() != generation_id {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "derived Change profile generation changed before response completion",
            ));
        }
        let final_checkpoint = match final_current.pin_change_reader_checkpoint() {
            Ok(checkpoint) => checkpoint,
            Err(LifecycleError::TruthChanged) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionUnstable,
                    "derived Change profile checkpoint moved before response completion",
                ));
            }
            Err(error) => return Ok(lifecycle_failure_outcome(error)),
        };
        if final_checkpoint.checkpoint_sha256 != checkpoint.checkpoint_sha256 {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "derived Change profile checkpoint changed before response completion",
            ));
        }
        Ok(DerivedChangeOutcomeV1::Ready((
            document,
            DerivedReadSourceV1::Generation,
        )))
    }

    fn profile_control_outcome(
        &self,
        status: RuntimeCurrentStatus,
    ) -> DerivedChangeOutcomeV1<ReaderProfileDocumentV1> {
        match self.control_path_capability() {
            Ok(inspection)
                if matches!(
                    inspection.status,
                    StoreCapabilityStatus::MigrationRequired
                        | StoreCapabilityStatus::MigrationInProgress { .. }
                ) =>
            {
                DerivedChangeOutcomeV1::Ready(ReaderProfileDocumentV1::from(&inspection))
            }
            Ok(_) => runtime_unavailable_outcome(status),
            Err(error) => DerivedChangeOutcomeV1::authority_invalid(error),
        }
    }

    fn profile_receipt_failure_outcome(
        &self,
        error: LifecycleError,
    ) -> DerivedChangeOutcomeV1<ReaderProfileDocumentV1> {
        match self.change_cohort_is_activated() {
            Ok(true) => lifecycle_failure_outcome(error),
            Ok(false) => match self.control_path_capability() {
                Ok(inspection)
                    if matches!(inspection.status, StoreCapabilityStatus::MigrationRequired) =>
                {
                    DerivedChangeOutcomeV1::Ready(ReaderProfileDocumentV1::from(&inspection))
                }
                Ok(_) => lifecycle_failure_outcome(error),
                Err(detail) => DerivedChangeOutcomeV1::authority_invalid(detail),
            },
            Err(detail) => DerivedChangeOutcomeV1::authority_invalid(detail),
        }
    }

    pub(crate) fn page_control_outcome<T>(
        &self,
        status: RuntimeCurrentStatus,
    ) -> DerivedChangeOutcomeV1<T> {
        self.capability_unavailable_or(status)
    }

    pub(crate) fn page_receipt_failure_outcome<T>(
        &self,
        error: LifecycleError,
    ) -> DerivedChangeOutcomeV1<T> {
        match self.change_cohort_is_activated() {
            Ok(true) => lifecycle_failure_outcome(error),
            Ok(false) => match self.control_path_capability() {
                Ok(inspection) => capability_unavailable_outcome(&inspection)
                    .unwrap_or_else(|| lifecycle_failure_outcome(error)),
                Err(detail) => DerivedChangeOutcomeV1::authority_invalid(detail),
            },
            Err(detail) => DerivedChangeOutcomeV1::authority_invalid(detail),
        }
    }

    fn capability_unavailable_or<T>(
        &self,
        fallback: RuntimeCurrentStatus,
    ) -> DerivedChangeOutcomeV1<T> {
        match self.control_path_capability() {
            Ok(inspection) => capability_unavailable_outcome(&inspection)
                .unwrap_or_else(|| runtime_unavailable_outcome(fallback)),
            Err(detail) => DerivedChangeOutcomeV1::authority_invalid(detail),
        }
    }

    fn control_path_capability(&self) -> std::result::Result<StoreCapabilityInspection, String> {
        let Some((_, backend)) = self.runtime.active_context() else {
            return Err("derived Change authority has no resolved store backend".to_owned());
        };
        let inspection = inspect_change_reader_journal_records(backend.journal().as_ref())
            .map_err(|error| error.to_string())?;
        Ok(StoreCapabilityInspection {
            status: inspection.status,
            cursor: inspection.cursor,
            minimum_reader_profile: inspection.minimum_reader_profile,
        })
    }

    fn change_cohort_is_activated(&self) -> std::result::Result<bool, String> {
        let Some((_, backend)) = self.runtime.active_context() else {
            return Err("derived Change authority has no resolved store backend".to_owned());
        };
        change_reader_activation_exists(backend.journal().as_ref())
            .map_err(|error| error.to_string())
    }

    /// Recovery controls and the supported legacy facade wrap the same runtime
    /// instead of resolving an independent generation slot or worker.
    #[doc(hidden)]
    pub fn recovery_access(&self) -> super::history::DerivedHistoryAccess {
        super::history::DerivedHistoryAccess::from_runtime(Arc::clone(&self.runtime))
    }

    /// Bind strict Change projections to the checkpoint already selected by
    /// this process without giving strict reads a derived content dependency.
    #[doc(hidden)]
    pub fn strict_stamp_binder(&self) -> StrictChangeStampBinder {
        StrictChangeStampBinder {
            runtime: Arc::clone(&self.runtime),
        }
    }

    #[doc(hidden)]
    pub fn is_active(&self) -> bool {
        self.runtime.is_active()
    }

    pub(crate) fn runtime(&self) -> &DerivedAccessRuntime {
        &self.runtime
    }

    pub(crate) fn repo(&self) -> Option<&Path> {
        self.repo.as_deref()
    }

    /// Compose one Change's review-schema detail document through the
    /// per-Change seek, mirroring [`Self::review_list_document`]'s finished
    /// return shape for the CLI.
    pub fn review_detail_document(
        &self,
        change: &ChangeId,
    ) -> Result<DerivedChangeOutcomeV1<crate::documents::ChangeDetailDocumentV1>> {
        use super::change_seek_reads::{
            ChangeSeekCompositionTarget, PreparedChangeSeek, change_seek_read_v1_inner,
        };
        Ok(
            change_seek_read_v1_inner(self, change, ChangeSeekCompositionTarget::Detail)?
                .map_ready(|prepared| match prepared {
                    PreparedChangeSeek::Detail(document) => *document,
                    PreparedChangeSeek::Selector(_) => {
                        unreachable!("the Detail target composes a detail document")
                    }
                }),
        )
    }

    /// Select one Change's narrowed view, exact references, and seek stamp
    /// through the per-Change seek, for the selector-consuming commands.
    pub fn change_seek(
        &self,
        change: &ChangeId,
    ) -> Result<DerivedChangeOutcomeV1<DerivedChangeSeekV1>> {
        use super::change_seek_reads::{
            ChangeSeekCompositionTarget, PreparedChangeSeek, change_seek_read_v1_inner,
        };
        Ok(
            change_seek_read_v1_inner(self, change, ChangeSeekCompositionTarget::Selector)?
                .map_ready(|prepared| match prepared {
                    PreparedChangeSeek::Selector(seek) => *seek,
                    PreparedChangeSeek::Detail(_) => {
                        unreachable!("the Selector target composes a seek carrier")
                    }
                }),
        )
    }

    /// Prepare one Change-scoped exact-Revision session without opening its
    /// fact snapshot. [`DerivedExactRevisionSessionV1::read`] owns that one
    /// snapshot and its terminal currentness proof.
    pub fn exact_revision_session(
        &self,
        change: &ChangeId,
    ) -> Result<DerivedChangeOutcomeV1<DerivedExactRevisionSessionV1<'_>>> {
        super::change_revision_reads::exact_revision_session_v1_inner(self, change, |_| {})
    }

    /// Read the complete materialized Change generation at one proven-current
    /// checkpoint for exact document composition.
    pub fn review_generation(&self) -> Result<DerivedChangeOutcomeV1<DerivedChangeGenerationV1>> {
        self.review_generation_with_hook(|| {})
    }

    /// Compose one Change's review-schema detail document from the complete
    /// materialized generation and bind it to that generation's stamp.
    pub fn review_generation_detail_document(
        &self,
        change: &ChangeId,
    ) -> Result<DerivedChangeOutcomeV1<crate::documents::ChangeDetailDocumentV1>> {
        let generation = self.review_generation()?;
        Ok(match generation {
            DerivedChangeOutcomeV1::Ready(generation) => {
                let document = ChangeDocumentFacadeV1::new(
                    generation.projection().clone(),
                    generation.document_projection().clone(),
                )?
                .with_generation_stamp(generation.stamp().to_owned())?
                .detail_document(change)?;
                DerivedChangeOutcomeV1::Ready(document)
            }
            DerivedChangeOutcomeV1::AuthorityUnavailable(document) => {
                DerivedChangeOutcomeV1::AuthorityUnavailable(document)
            }
            DerivedChangeOutcomeV1::AuthorityConflicted(document) => {
                DerivedChangeOutcomeV1::AuthorityConflicted(document)
            }
            DerivedChangeOutcomeV1::AuthorityInvalid(document) => {
                DerivedChangeOutcomeV1::AuthorityInvalid(document)
            }
            DerivedChangeOutcomeV1::ReaderUpgradeRequired(document) => {
                DerivedChangeOutcomeV1::ReaderUpgradeRequired(document)
            }
            DerivedChangeOutcomeV1::ProjectionUnavailable(document) => {
                DerivedChangeOutcomeV1::ProjectionUnavailable(document)
            }
            DerivedChangeOutcomeV1::Retryable(document) => {
                DerivedChangeOutcomeV1::Retryable(document)
            }
        })
    }

    fn review_generation_with_hook(
        &self,
        hook: impl FnOnce(),
    ) -> Result<DerivedChangeOutcomeV1<DerivedChangeGenerationV1>> {
        // Keep this acquisition and terminal re-proof aligned with the sibling
        // Change page producer while their composition targets remain distinct.
        let current = match self.runtime.current() {
            Ok(RuntimeCurrentRead::Ready(current)) => current,
            Ok(RuntimeCurrentRead::Unavailable(status)) => {
                return Ok(self.page_control_outcome(status));
            }
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error,
                ));
            }
        };
        let generation_id = current.generation_id().to_owned();
        let checkpoint = match current.pin_change_reader_checkpoint() {
            Ok(checkpoint) => checkpoint,
            Err(error) => return Ok(self.page_receipt_failure_outcome(error)),
        };
        if let Err(error) = current.reader_profile_document(&checkpoint) {
            return Ok(self.page_receipt_failure_outcome(error));
        }
        let as_of = checkpoint.truth_cursor;
        let materialized = match current
            .service()
            .semantic_materialized_change_projection_at(as_of)
        {
            Ok(LocatorRead::Ready(materialized)) => materialized,
            Ok(LocatorRead::CatchUpRequired { .. }) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionStale,
                    "derived Change projection moved while its checkpoint was pinned",
                ));
            }
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error.to_string(),
                ));
            }
        };
        if let Some(outcome) = generation_checkpoint_mismatch_outcome(as_of, materialized.as_of) {
            return Ok(outcome);
        }
        let stamp = match current.change_generation_stamp(
            &checkpoint,
            &materialized.projection,
            &materialized.document_projection,
        ) {
            Ok(stamp) => stamp,
            Err(error) => return Ok(lifecycle_failure_outcome(error)),
        };
        let generation = DerivedChangeGenerationV1 {
            projection: materialized.projection,
            document_projection: materialized.document_projection,
            stamp,
        };

        hook();

        let final_current = match self.runtime.current() {
            Ok(RuntimeCurrentRead::Ready(current)) => current,
            Ok(RuntimeCurrentRead::Unavailable(_)) | Err(_) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionUnstable,
                    "derived Change generation moved before response completion",
                ));
            }
        };
        if final_current.generation_id() != generation_id {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "derived Change generation changed before response completion",
            ));
        }
        let final_checkpoint = match final_current.pin_change_reader_checkpoint() {
            Ok(checkpoint) => checkpoint,
            Err(LifecycleError::TruthChanged) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionUnstable,
                    "derived Change checkpoint moved before response completion",
                ));
            }
            Err(error) => return Ok(lifecycle_failure_outcome(error)),
        };
        if final_checkpoint.checkpoint_sha256 != checkpoint.checkpoint_sha256 {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "derived Change checkpoint changed before response completion",
            ));
        }
        Ok(DerivedChangeOutcomeV1::Ready(generation))
    }

    pub fn changes(
        &self,
        request: &DerivedChangePageRequestV1,
    ) -> Result<DerivedChangeOutcomeV1<DerivedChangePageV1>> {
        Ok(self
            .read_page(ChangePageLens::Changes, request, |_| {})?
            .map_ready(|page| match page {
                PreparedChangePage::Changes(page) => page,
                PreparedChangePage::Attention(_)
                | PreparedChangePage::ReviewList(_)
                | PreparedChangePage::ReviewAttention(_) => {
                    unreachable!("Changes lens constructs a Changes page")
                }
            }))
    }

    pub fn attention(
        &self,
        request: &DerivedChangePageRequestV1,
    ) -> Result<DerivedChangeOutcomeV1<DerivedAttentionPageV1>> {
        Ok(self
            .read_page(ChangePageLens::Attention, request, |_| {})?
            .map_ready(|page| match page {
                PreparedChangePage::Attention(page) => page,
                PreparedChangePage::Changes(_)
                | PreparedChangePage::ReviewList(_)
                | PreparedChangePage::ReviewAttention(_) => {
                    unreachable!("Attention lens constructs an Attention page")
                }
            }))
    }

    /// Compose the un-paged review-schema Change list over the bare page
    /// selection, mirroring [`Self::profile`]'s finished-document return
    /// shape for the CLI.
    pub fn review_list_document(&self) -> Result<DerivedChangeOutcomeV1<ChangeListDocumentV1>> {
        Ok(self
            .read_page_with_hook(
                ChangePageLens::Changes,
                ChangeCompositionTarget::Review,
                &DerivedChangePageRequestV1::Bare,
                |_| {},
            )?
            .map_ready(|page| match page {
                PreparedChangePage::ReviewList(document) => document,
                PreparedChangePage::Changes(_)
                | PreparedChangePage::Attention(_)
                | PreparedChangePage::ReviewAttention(_) => {
                    unreachable!("the Review Changes target composes a review list")
                }
            }))
    }

    /// Compose the review-schema Attention document (`inspect = false`) over
    /// the bare page selection for the CLI.
    pub fn review_attention_document(
        &self,
    ) -> Result<DerivedChangeOutcomeV1<ChangeAttentionPresentationDocumentV2>> {
        Ok(self
            .read_page_with_hook(
                ChangePageLens::Attention,
                ChangeCompositionTarget::Review,
                &DerivedChangePageRequestV1::Bare,
                |_| {},
            )?
            .map_ready(|page| match page {
                PreparedChangePage::ReviewAttention(document) => document,
                PreparedChangePage::Changes(_)
                | PreparedChangePage::Attention(_)
                | PreparedChangePage::ReviewList(_) => {
                    unreachable!("the Review Attention target composes a review document")
                }
            }))
    }

    pub fn timeline(
        &self,
        request: &super::timeline::DerivedTimelinePageRequestV1,
        trust_set: &crate::session::TrustSet,
    ) -> Result<DerivedChangeOutcomeV1<super::timeline::DerivedTimelinePageV1>> {
        self.timeline_with_hook(request, trust_set, |_| {})
    }

    fn timeline_with_hook(
        &self,
        request: &super::timeline::DerivedTimelinePageRequestV1,
        trust_set: &crate::session::TrustSet,
        mut hook: impl FnMut(super::timeline::TimelineReadBoundary),
    ) -> Result<DerivedChangeOutcomeV1<super::timeline::DerivedTimelinePageV1>> {
        let current = match self.runtime.current() {
            Ok(RuntimeCurrentRead::Ready(current)) => current,
            Ok(RuntimeCurrentRead::Unavailable(status)) => {
                return Ok(self.page_control_outcome(status));
            }
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error,
                ));
            }
        };
        let generation_id = current.generation_id().to_owned();
        let checkpoint = match current.pin_change_reader_checkpoint() {
            Ok(checkpoint) => checkpoint,
            Err(error) => return Ok(self.page_receipt_failure_outcome(error)),
        };
        if let Err(error) = current.reader_profile_document(&checkpoint) {
            return Ok(self.page_receipt_failure_outcome(error));
        }
        let as_of = checkpoint.truth_cursor;
        let snapshot = match current.service().product_history_read_snapshot_at(as_of) {
            Ok(LocatorRead::Ready(snapshot)) => snapshot,
            Ok(LocatorRead::CatchUpRequired { .. }) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionStale,
                    "derived Timeline product history moved while its checkpoint was pinned",
                ));
            }
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error.to_string(),
                ));
            }
        };
        if snapshot.changes.as_of != as_of
            || u64::try_from(snapshot.state.event_count).ok()
                != Some(checkpoint.authority_cursor.event_count)
        {
            return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                "derived Timeline snapshot disagrees with its pinned authority cursor",
            ));
        }
        let source_change_projection_stamp = match current.change_generation_stamp(
            &checkpoint,
            &snapshot.changes.projection,
            &snapshot.changes.document_projection,
        ) {
            Ok(stamp) => stamp,
            Err(error) => return Ok(lifecycle_failure_outcome(error)),
        };
        let trust_set_sha256 = match trust_set.identity_sha256() {
            Ok(identity) => identity,
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error.to_string(),
                ));
            }
        };
        let timeline_projection_stamp =
            match crate::session::projection::event_history::timeline_projection_stamp_v1(
                &source_change_projection_stamp,
                &trust_set_sha256,
            ) {
                Ok(stamp) => stamp,
                Err(error) => {
                    return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                        DerivedProjectionFailureCodeV1::ProjectionInvalid,
                        error.to_string(),
                    ));
                }
            };
        if request
            .position()
            .expected_projection_stamp()
            .is_some_and(|expected| expected != timeline_projection_stamp)
        {
            return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionStale,
                "derived Timeline continuation belongs to a different live checkpoint or TrustSet",
            ));
        }
        hook(super::timeline::TimelineReadBoundary::SnapshotPinned);
        let prepared = super::timeline::prepare_timeline_page(
            current.service(),
            &snapshot.connection,
            &snapshot.changes.document_projection,
            as_of,
            checkpoint.authority_cursor.clone(),
            source_change_projection_stamp,
            timeline_projection_stamp,
            request,
            trust_set,
            &mut hook,
        );
        if let Err(error) = snapshot.finish() {
            return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                error.to_string(),
            ));
        }
        let prepared = match prepared {
            Ok(page) => page,
            Err(super::timeline::TimelinePageError::RequestInvalid(message)) => {
                return Err(ShoreError::WorkflowInputInvalid { reason: message });
            }
            Err(super::timeline::TimelinePageError::Stale(message)) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionStale,
                    message,
                ));
            }
            Err(super::timeline::TimelinePageError::Invalid(message)) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    message,
                ));
            }
        };

        let final_current = match self.runtime.current() {
            Ok(RuntimeCurrentRead::Ready(current)) => current,
            Ok(RuntimeCurrentRead::Unavailable(_)) | Err(_) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionUnstable,
                    "derived Timeline projection moved before response completion",
                ));
            }
        };
        if final_current.generation_id() != generation_id {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "derived Timeline generation changed before response completion",
            ));
        }
        let final_checkpoint = match final_current.pin_change_reader_checkpoint() {
            Ok(checkpoint) => checkpoint,
            Err(LifecycleError::TruthChanged) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionUnstable,
                    "derived Timeline checkpoint moved before response completion",
                ));
            }
            Err(error) => return Ok(lifecycle_failure_outcome(error)),
        };
        if final_checkpoint.checkpoint_sha256 != checkpoint.checkpoint_sha256 {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "derived Timeline checkpoint changed before response completion",
            ));
        }
        Ok(DerivedChangeOutcomeV1::Ready(prepared))
    }

    fn read_page(
        &self,
        lens: ChangePageLens,
        request: &DerivedChangePageRequestV1,
        hook: impl FnMut(ChangeReadBoundary),
    ) -> Result<DerivedChangeOutcomeV1<PreparedChangePage>> {
        self.read_page_with_hook(lens, ChangeCompositionTarget::Inspector, request, hook)
    }

    fn read_page_with_hook(
        &self,
        lens: ChangePageLens,
        target: ChangeCompositionTarget,
        request: &DerivedChangePageRequestV1,
        mut hook: impl FnMut(ChangeReadBoundary),
    ) -> Result<DerivedChangeOutcomeV1<PreparedChangePage>> {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let snapshot_phase = enter_derived_access_phase_v1(Phase::ChangePageSnapshotAcquisition);
        let current = match self.runtime.current() {
            Ok(RuntimeCurrentRead::Ready(current)) => current,
            Ok(RuntimeCurrentRead::Unavailable(status)) => {
                return Ok(self.page_control_outcome(status));
            }
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error,
                ));
            }
        };
        let generation_id = current.generation_id().to_owned();
        let checkpoint = match current.pin_change_reader_checkpoint() {
            Ok(checkpoint) => checkpoint,
            Err(error) => return Ok(self.page_receipt_failure_outcome(error)),
        };
        if let Err(error) = current.reader_profile_document(&checkpoint) {
            return Ok(self.page_receipt_failure_outcome(error));
        }
        let as_of = checkpoint.truth_cursor;
        let materialized = match current
            .service()
            .semantic_materialized_change_projection_at(as_of)
        {
            Ok(LocatorRead::Ready(materialized)) => materialized,
            Ok(LocatorRead::CatchUpRequired { .. }) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionStale,
                    "derived Change projection moved while its checkpoint was pinned",
                ));
            }
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error.to_string(),
                ));
            }
        };
        if materialized.as_of != as_of {
            return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                "materialized Change projection has the wrong live checkpoint",
            ));
        }
        let generation_stamp = match current.change_generation_stamp(
            &checkpoint,
            &materialized.projection,
            &materialized.document_projection,
        ) {
            Ok(stamp) => stamp,
            Err(error) => return Ok(lifecycle_failure_outcome(error)),
        };
        if matches!(
            request,
            DerivedChangePageRequestV1::Bounded(selection)
                if selection.after().is_some_and(|continuation| {
                    continuation.expected_projection_stamp() != generation_stamp
                })
        ) {
            return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionStale,
                "derived Change continuation belongs to a different live checkpoint",
            ));
        }
        let facade = match ChangeDocumentFacadeV1::new(
            materialized.projection,
            materialized.document_projection,
        ) {
            Ok(facade) => facade,
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error.to_string(),
                ));
            }
        };
        hook(ChangeReadBoundary::SnapshotPinned);
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(snapshot_phase);

        #[cfg(any(test, feature = "longitudinal-counting"))]
        let selection_phase = enter_derived_access_phase_v1(Phase::ChangePageBodylessSelection);
        let summaries = facade.list_document_for_inspector().changes;
        let candidate_ids =
            match select_bodyless_change_candidates(lens, &summaries, &generation_stamp, request) {
                Ok(candidates) => candidates,
                Err(error) => {
                    return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                        DerivedProjectionFailureCodeV1::ProjectionInvalid,
                        error.to_string(),
                    ));
                }
            };
        let candidate_id_set = candidate_ids.iter().cloned().collect::<BTreeSet<_>>();
        let candidate_revisions = summaries
            .iter()
            .filter(|summary| candidate_id_set.contains(&summary.change_id))
            .flat_map(|summary| summary.current_revision_refs.iter().cloned())
            .collect::<BTreeSet<_>>();
        #[cfg(any(test, feature = "longitudinal-counting"))]
        {
            record_change_candidates(candidate_ids.len());
            record_change_candidate_current_revisions(candidate_revisions.len());
        }
        let summary_query = match request {
            DerivedChangePageRequestV1::Bare => None,
            DerivedChangePageRequestV1::Bounded(selection) => selection.summary_query(),
        };
        let (hydration_plan, revisions_to_hydrate) = match summary_query {
            None => {
                let selection = match paginate_bodyless_change_candidates(
                    &candidate_ids,
                    &generation_stamp,
                    request,
                ) {
                    Ok(selection) => selection,
                    Err(error) => {
                        return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                            DerivedProjectionFailureCodeV1::ProjectionInvalid,
                            error.to_string(),
                        ));
                    }
                };
                let selected_ids = selection
                    .change_ids
                    .iter()
                    .cloned()
                    .collect::<BTreeSet<_>>();
                let revisions = summaries
                    .iter()
                    .filter(|summary| selected_ids.contains(&summary.change_id))
                    .flat_map(|summary| summary.current_revision_refs.iter().cloned())
                    .collect::<BTreeSet<_>>();
                (ChangeProposalHydrationPlan::Ordinary(selection), revisions)
            }
            Some(normalized_query) => (
                ChangeProposalHydrationPlan::ExhaustiveSearch { normalized_query },
                candidate_revisions,
            ),
        };
        hook(ChangeReadBoundary::BodylessSelectionComplete);
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(selection_phase);

        #[cfg(any(test, feature = "longitudinal-counting"))]
        let locator_phase =
            enter_derived_access_phase_v1(Phase::ChangePageProposalLocatorExpansion);
        let proposal_locators = match current
            .service()
            .proposal_carrier_locators_for_exact_revisions(&revisions_to_hydrate, as_of)
        {
            Ok(LocatorRead::Ready(locators)) => locators,
            Ok(LocatorRead::CatchUpRequired { .. }) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionStale,
                    "derived Change proposal locators moved during selection",
                ));
            }
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error.to_string(),
                ));
            }
        };
        if let Some(missing) = proposal_locators
            .iter()
            .find_map(|(revision, locators)| locators.is_empty().then_some(revision))
        {
            return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                format!(
                    "selected current exact Revision {} has no authoritative proposal carrier",
                    missing.revision_id.as_str()
                ),
            ));
        }
        let located = proposal_locators
            .iter()
            .flat_map(|(revision, locators)| {
                locators.iter().map(move |locator| (revision, locator))
            })
            .collect::<Vec<_>>();
        let proposal_event_ids = located
            .iter()
            .map(|(_, locator)| locator.event_id.as_str().to_owned())
            .collect::<Vec<_>>();
        hook(ChangeReadBoundary::ProposalLocatorsSelected);
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(locator_phase);

        #[cfg(any(test, feature = "longitudinal-counting"))]
        let hydration_phase =
            enter_derived_access_phase_v1(Phase::ChangePageCarrierHydrationValidation);
        let hydrated = match current
            .service()
            .semantic_ids_hydrated_at(&proposal_event_ids, as_of)
        {
            Ok(LocatorRead::Ready(hydrated)) => hydrated,
            Ok(LocatorRead::CatchUpRequired { .. }) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionStale,
                    "derived Change proposal carriers moved during hydration",
                ));
            }
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error.to_string(),
                ));
            }
        };
        #[cfg(any(test, feature = "longitudinal-counting"))]
        record_change_proposal_carriers_opened(proposal_event_ids.len());
        if hydrated.len() != located.len() {
            return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                "authoritative proposal hydration returned the wrong carrier count",
            ));
        }
        let mut proposal_events = Vec::with_capacity(hydrated.len());
        for ((expected_revision, locator), hydrated) in located.into_iter().zip(hydrated) {
            let Some(hydrated) = hydrated else {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    format!(
                        "selected authoritative proposal carrier {} is absent",
                        locator.event_id.as_str()
                    ),
                ));
            };
            if hydrated.row.cursor != locator.cursor
                || sha256_bytes_hex(hydrated.row.logical_reread_key.as_bytes())
                    != locator.logical_reread_key_hash
                || hydrated.row.replay_key != locator.replay_key
                || hydrated.row.event_id != locator.event_id.as_str()
                || hydrated.row.event_type != locator.event_type
                || hydrated.row.payload_hash != locator.payload_hash
                || hydrated.row.validation_witness != locator.validation_witness
                || hydrated.event.event_id != locator.event_id
                || hydrated.event.payload_hash != locator.payload_hash
            {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    format!(
                        "authoritative proposal carrier {} differs from its compact locator",
                        locator.event_id.as_str()
                    ),
                ));
            }
            let actual_revision = match exact_revision_from_proposal(&hydrated.event) {
                Ok(revision) => revision,
                Err(error) => {
                    return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                        DerivedProjectionFailureCodeV1::ProjectionInvalid,
                        error.to_string(),
                    ));
                }
            };
            if actual_revision != *expected_revision || locator.revision != *expected_revision {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    format!(
                        "authoritative proposal carrier {} has the wrong exact Revision binding",
                        locator.event_id.as_str()
                    ),
                ));
            }
            #[cfg(any(test, feature = "longitudinal-counting"))]
            record_change_proposal_carriers_validated(1);
            proposal_events.push(hydrated.event);
        }
        hook(ChangeReadBoundary::ProposalHydrationComplete);
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(hydration_phase);

        let selection = match hydration_plan {
            ChangeProposalHydrationPlan::Ordinary(selection) => selection,
            ChangeProposalHydrationPlan::ExhaustiveSearch { normalized_query } => {
                #[cfg(any(test, feature = "longitudinal-counting"))]
                let search_phase =
                    enter_derived_access_phase_v1(Phase::ChangePageExhaustiveProposalSearch);
                let matching_ids = match facade.search_change_ids_with_proposal_presentations(
                    &candidate_ids,
                    &proposal_events,
                    normalized_query,
                ) {
                    Ok(matching_ids) => matching_ids,
                    Err(error) => {
                        return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                            DerivedProjectionFailureCodeV1::ProjectionInvalid,
                            error.to_string(),
                        ));
                    }
                };
                #[cfg(any(test, feature = "longitudinal-counting"))]
                record_change_matches(matching_ids.len());
                let selection = match paginate_bodyless_change_candidates(
                    &matching_ids,
                    &generation_stamp,
                    request,
                ) {
                    Ok(selection) => selection,
                    Err(error) => {
                        return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                            DerivedProjectionFailureCodeV1::ProjectionInvalid,
                            error.to_string(),
                        ));
                    }
                };
                #[cfg(any(test, feature = "longitudinal-counting"))]
                drop(search_phase);
                selection
            }
        };
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let emitted_row_count = selection.change_ids.len();
        let selected_ids = selection
            .change_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let selected_revisions = summaries
            .iter()
            .filter(|summary| selected_ids.contains(&summary.change_id))
            .flat_map(|summary| summary.current_revision_refs.iter().cloned())
            .collect::<BTreeSet<_>>();
        let mut selected_proposal_events = Vec::new();
        for event in proposal_events {
            let revision = match exact_revision_from_proposal(&event) {
                Ok(revision) => revision,
                Err(error) => {
                    return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                        DerivedProjectionFailureCodeV1::ProjectionInvalid,
                        error.to_string(),
                    ));
                }
            };
            if selected_revisions.contains(&revision) {
                selected_proposal_events.push(event);
            }
        }
        let proposal_events = selected_proposal_events;

        #[cfg(any(test, feature = "longitudinal-counting"))]
        let support_phase = enter_derived_access_phase_v1(Phase::ChangePageSupportExpansion);
        let support_ids = match current
            .service()
            .support_event_ids_at(&proposal_events, as_of)
        {
            Ok(LocatorRead::Ready(event_ids)) => event_ids,
            Ok(LocatorRead::CatchUpRequired { .. }) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionStale,
                    "derived Change support set moved during expansion",
                ));
            }
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error.to_string(),
                ));
            }
        };
        match current.service().semantic_ids_at(&support_ids, as_of) {
            Ok(LocatorRead::Ready(events)) => {
                #[cfg(any(test, feature = "longitudinal-counting"))]
                record_change_support_carriers_opened(support_ids.len());
                if events.len() != support_ids.len() {
                    return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                        DerivedProjectionFailureCodeV1::ProjectionInvalid,
                        "authoritative support hydration returned the wrong carrier count",
                    ));
                }
                for (event_id, event) in support_ids.iter().zip(events) {
                    let Some(_event) = event else {
                        return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                            DerivedProjectionFailureCodeV1::ProjectionInvalid,
                            format!("selected authoritative support carrier {event_id} is absent"),
                        ));
                    };
                }
            }
            Ok(LocatorRead::CatchUpRequired { .. }) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionStale,
                    "derived Change support carriers moved during hydration",
                ));
            }
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error.to_string(),
                ));
            }
        }
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(support_phase);

        #[cfg(any(test, feature = "longitudinal-counting"))]
        let presentation_phase =
            enter_derived_access_phase_v1(Phase::ChangePagePresentationProjection);
        let prepared = match (lens, target) {
            (ChangePageLens::Changes, ChangeCompositionTarget::Review) => facade
                .selected_list_document_with_presentations(
                    &selection.change_ids,
                    &proposal_events,
                    &generation_stamp,
                )
                .map(PreparedChangePage::ReviewList),
            (ChangePageLens::Attention, ChangeCompositionTarget::Review) => facade
                .selected_attention_document_with_presentations(
                    &selection.change_ids,
                    &proposal_events,
                    &generation_stamp,
                )
                .map(PreparedChangePage::ReviewAttention),
            (ChangePageLens::Changes, ChangeCompositionTarget::Inspector) => facade
                .selected_list_document_for_inspector_with_presentations(
                    &selection.change_ids,
                    &proposal_events,
                    &generation_stamp,
                )
                .map(|document| {
                    PreparedChangePage::Changes(DerivedChangePageV1 {
                        document,
                        window: selection.window.clone(),
                    })
                }),
            (ChangePageLens::Attention, ChangeCompositionTarget::Inspector) => (|| {
                let document = facade
                    .selected_attention_document_for_inspector_with_presentations(
                        &selection.change_ids,
                        &proposal_events,
                        &generation_stamp,
                    )?;
                let attention_presentations = selection
                    .change_ids
                    .iter()
                    .map(|change_id| {
                        let detail = facade.detail_document(change_id)?;
                        attention_presentation_for_change(&detail.detail)
                            .map(|presentation| (change_id.clone(), presentation))
                    })
                    .collect::<Result<BTreeMap<_, _>>>()?;
                Ok(PreparedChangePage::Attention(DerivedAttentionPageV1 {
                    document,
                    attention_presentations,
                    window: selection.window.clone(),
                }))
            })(),
        };
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    error.to_string(),
                ));
            }
        };
        hook(ChangeReadBoundary::ResponseConstructed);
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(presentation_phase);

        let final_current = match self.runtime.current() {
            Ok(RuntimeCurrentRead::Ready(current)) => current,
            Ok(RuntimeCurrentRead::Unavailable(_)) | Err(_) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionUnstable,
                    "derived Change projection moved before response completion",
                ));
            }
        };
        if final_current.generation_id() != generation_id {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "derived Change generation changed before response completion",
            ));
        }
        let final_checkpoint = match final_current.pin_change_reader_checkpoint() {
            Ok(checkpoint) => checkpoint,
            Err(LifecycleError::TruthChanged) => {
                return Ok(DerivedChangeOutcomeV1::retryable(
                    DerivedProjectionFailureCodeV1::ProjectionUnstable,
                    "derived Change checkpoint moved before response completion",
                ));
            }
            Err(error) => return Ok(lifecycle_failure_outcome(error)),
        };
        if final_checkpoint.checkpoint_sha256 != checkpoint.checkpoint_sha256 {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "derived Change checkpoint changed before response completion",
            ));
        }
        // Emission is a request outcome, not work performed inside an earlier
        // phase. Failed or unstable responses therefore never claim rows.
        #[cfg(any(test, feature = "longitudinal-counting"))]
        record_change_rows_emitted(emitted_row_count);
        Ok(DerivedChangeOutcomeV1::Ready(prepared))
    }
}

fn generation_checkpoint_mismatch_outcome(
    expected: super::cursor::TruthCursor,
    materialized: super::cursor::TruthCursor,
) -> Option<DerivedChangeOutcomeV1<DerivedChangeGenerationV1>> {
    (materialized != expected).then(|| {
        DerivedChangeOutcomeV1::projection_unavailable(
            DerivedProjectionFailureCodeV1::ProjectionInvalid,
            "materialized Change projection has the wrong live checkpoint",
        )
    })
}

/// Metadata-only bridge between strict Change content and one cached reader
/// checkpoint. The strict projections remain the complete content authority.
#[doc(hidden)]
#[derive(Clone)]
pub struct StrictChangeStampBinder {
    runtime: Arc<DerivedAccessRuntime>,
}

/// Result of attempting to bind strict Change content to cached generation
/// metadata.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StrictChangeStampBindingV1 {
    Unavailable,
    Bound(String),
    Moving,
}

impl StrictChangeStampBinder {
    pub fn bind(
        &self,
        authority_cursor: &AuthorityCursorV2,
        semantic_projection: &ChangeProjection,
        document_projection: &ChangeDocumentProjectionV1,
    ) -> Result<StrictChangeStampBindingV1> {
        self.bind_with_hook(
            authority_cursor,
            semantic_projection,
            document_projection,
            || {},
        )
    }

    fn bind_with_hook(
        &self,
        authority_cursor: &AuthorityCursorV2,
        semantic_projection: &ChangeProjection,
        document_projection: &ChangeDocumentProjectionV1,
        hook: impl FnOnce(),
    ) -> Result<StrictChangeStampBindingV1> {
        bind_strict_change_stamp_with_hook(
            &self.runtime,
            authority_cursor,
            semantic_projection,
            document_projection,
            hook,
        )
    }
}

fn bind_strict_change_stamp_with_hook(
    runtime: &DerivedAccessRuntime,
    authority_cursor: &AuthorityCursorV2,
    semantic_projection: &ChangeProjection,
    document_projection: &ChangeDocumentProjectionV1,
    hook: impl FnOnce(),
) -> Result<StrictChangeStampBindingV1> {
    let Some(current) = runtime.cached_current() else {
        return Ok(StrictChangeStampBindingV1::Unavailable);
    };
    let initial_publication = match runtime.current_publication_identity() {
        Ok(Some(publication)) => publication,
        Ok(None) | Err(_) => return Ok(StrictChangeStampBindingV1::Unavailable),
    };
    if current.publication_identity() != &initial_publication {
        return Ok(StrictChangeStampBindingV1::Moving);
    }
    let checkpoint = match current.pin_change_reader_checkpoint() {
        Ok(checkpoint) => checkpoint,
        Err(LifecycleError::TruthChanged) => return Ok(StrictChangeStampBindingV1::Moving),
        Err(_) => return Ok(StrictChangeStampBindingV1::Unavailable),
    };
    if &checkpoint.authority_cursor != authority_cursor {
        return Ok(StrictChangeStampBindingV1::Moving);
    }
    let stamp = current
        .strict_change_generation_stamp(
            &checkpoint,
            authority_cursor,
            semantic_projection,
            document_projection,
        )
        .map_err(|error| ShoreError::Message(error.to_string()))?;

    hook();

    let Some(final_current) = runtime.cached_current() else {
        return Ok(StrictChangeStampBindingV1::Moving);
    };
    if !Arc::ptr_eq(&current, &final_current) {
        return Ok(StrictChangeStampBindingV1::Moving);
    }
    let final_checkpoint = match final_current.pin_change_reader_checkpoint() {
        Ok(checkpoint) => checkpoint,
        Err(LifecycleError::TruthChanged) => return Ok(StrictChangeStampBindingV1::Moving),
        Err(_) => return Ok(StrictChangeStampBindingV1::Unavailable),
    };
    if final_checkpoint.checkpoint_sha256 != checkpoint.checkpoint_sha256
        || &final_checkpoint.authority_cursor != authority_cursor
    {
        return Ok(StrictChangeStampBindingV1::Moving);
    }
    let final_publication = match runtime.current_publication_identity() {
        Ok(Some(publication)) => publication,
        Ok(None) => return Ok(StrictChangeStampBindingV1::Moving),
        Err(_) => return Ok(StrictChangeStampBindingV1::Unavailable),
    };
    if final_publication != initial_publication
        || final_current.publication_identity() != &final_publication
    {
        return Ok(StrictChangeStampBindingV1::Moving);
    }
    match runtime.cached_current_authority_is_stable(&final_current) {
        Ok(true) => {}
        Ok(false) => return Ok(StrictChangeStampBindingV1::Moving),
        Err(_) => return Ok(StrictChangeStampBindingV1::Unavailable),
    }
    Ok(StrictChangeStampBindingV1::Bound(stamp))
}

/// One complete materialized Change generation for exact document reads.
///
/// Accessors only: `document_projection().diagnostics` is store-scoped and
/// must never be read or serialized by a product surface. Documents read the
/// per-Change `view.diagnostics` vocabulary instead.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct DerivedChangeGenerationV1 {
    projection: ChangeProjection,
    document_projection: ChangeDocumentProjectionV1,
    stamp: String,
}

impl DerivedChangeGenerationV1 {
    pub fn projection(&self) -> &ChangeProjection {
        &self.projection
    }

    pub fn document_projection(&self) -> &ChangeDocumentProjectionV1 {
        &self.document_projection
    }

    pub fn stamp(&self) -> &str {
        &self.stamp
    }
}

/// One Change's narrowed seek result for the selector-consuming CLI reads.
///
/// Accessors only: `document_projection().diagnostics` is the narrowed
/// store-scoped diagnostics field and must never be read or serialized by a
/// product surface — documents read the per-Change `view.diagnostics`
/// vocabulary instead.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct DerivedChangeSeekV1 {
    view: crate::session::ChangeView,
    document_projection: ChangeDocumentProjectionV1,
    stamp: String,
}

impl DerivedChangeSeekV1 {
    /// The producer lives in the sibling `change_seek_reads` module and
    /// cannot build a private-field struct without this constructor.
    pub(crate) fn new(
        view: crate::session::ChangeView,
        document_projection: ChangeDocumentProjectionV1,
        stamp: String,
    ) -> Self {
        Self {
            view,
            document_projection,
            stamp,
        }
    }

    pub fn change_view(&self) -> &crate::session::ChangeView {
        &self.view
    }

    pub fn document_projection(&self) -> &ChangeDocumentProjectionV1 {
        &self.document_projection
    }

    pub fn stamp(&self) -> &str {
        &self.stamp
    }
}

/// Inputs for one snapshot-bound exact-Revision read.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactRevisionReadPlanV1 {
    pub revisions: Vec<RevisionRefV1>,
    pub include_body: bool,
    pub read_for_display: bool,
    pub fact_port_context: Option<RevisionRefV1>,
}

/// Prepared Change-scoped session. Construction stays library-side; callers
/// may inspect the narrowed facade before consuming the session with `read`.
#[doc(hidden)]
pub struct DerivedExactRevisionSessionV1<'a> {
    access: &'a DerivedChangeAccess,
    state: ExactRevisionSessionStateV1,
}

impl<'a> DerivedExactRevisionSessionV1<'a> {
    pub(crate) fn new(access: &'a DerivedChangeAccess, state: ExactRevisionSessionStateV1) -> Self {
        Self { access, state }
    }

    pub(crate) fn into_parts(self) -> (&'a DerivedChangeAccess, ExactRevisionSessionStateV1) {
        (self.access, self.state)
    }

    pub fn change_view(&self) -> &ChangeView {
        self.state.change_view()
    }

    pub fn document_projection(&self) -> &ChangeDocumentProjectionV1 {
        self.state.document_projection()
    }

    pub fn facade(&self) -> &ChangeDocumentFacadeV1 {
        self.state.facade()
    }

    pub fn stamp(&self) -> &str {
        self.state.stamp()
    }

    pub fn read(
        self,
        plan: &ExactRevisionReadPlanV1,
    ) -> Result<DerivedChangeOutcomeV1<DerivedExactRevisionReadV1>> {
        super::change_revision_reads::exact_revision_read_v1_inner(self, plan, |_| {})
    }
}

/// One completed exact-Revision session read, paired with the narrowed
/// Change projection and the exact facade used for fact-port discovery.
#[doc(hidden)]
pub struct DerivedExactRevisionReadV1 {
    view: ChangeView,
    document_projection: ChangeDocumentProjectionV1,
    facade: ChangeDocumentFacadeV1,
    stamp: String,
    results: BTreeMap<RevisionRefV1, RevisionShowResult>,
}

impl DerivedExactRevisionReadV1 {
    pub(crate) fn new(
        view: ChangeView,
        document_projection: ChangeDocumentProjectionV1,
        facade: ChangeDocumentFacadeV1,
        stamp: String,
        results: BTreeMap<RevisionRefV1, RevisionShowResult>,
    ) -> Self {
        Self {
            view,
            document_projection,
            facade,
            stamp,
            results,
        }
    }

    pub fn change_view(&self) -> &ChangeView {
        &self.view
    }

    pub fn document_projection(&self) -> &ChangeDocumentProjectionV1 {
        &self.document_projection
    }

    pub fn facade(&self) -> &ChangeDocumentFacadeV1 {
        &self.facade
    }

    pub fn stamp(&self) -> &str {
        &self.stamp
    }

    pub fn result(&self, revision: &RevisionRefV1) -> Option<&RevisionShowResult> {
        self.results.get(revision)
    }
}

/// Independent authority, compatibility, and projection outcomes.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DerivedChangeOutcomeV1<T> {
    Ready(T),
    AuthorityUnavailable(ChangeQueryUnavailableDocumentV1),
    AuthorityConflicted(DerivedAuthorityFailureDocumentV1),
    AuthorityInvalid(DerivedAuthorityFailureDocumentV1),
    ReaderUpgradeRequired(ReaderUpgradeRequiredDocumentV1),
    ProjectionUnavailable(DerivedProjectionUnavailableDocumentV1),
    Retryable(DerivedProjectionUnavailableDocumentV1),
}

impl<T> DerivedChangeOutcomeV1<T> {
    pub fn map_ready<U>(self, map: impl FnOnce(T) -> U) -> DerivedChangeOutcomeV1<U> {
        match self {
            Self::Ready(value) => DerivedChangeOutcomeV1::Ready(map(value)),
            Self::AuthorityUnavailable(document) => {
                DerivedChangeOutcomeV1::AuthorityUnavailable(document)
            }
            Self::AuthorityConflicted(document) => {
                DerivedChangeOutcomeV1::AuthorityConflicted(document)
            }
            Self::AuthorityInvalid(document) => DerivedChangeOutcomeV1::AuthorityInvalid(document),
            Self::ReaderUpgradeRequired(document) => {
                DerivedChangeOutcomeV1::ReaderUpgradeRequired(document)
            }
            Self::ProjectionUnavailable(document) => {
                DerivedChangeOutcomeV1::ProjectionUnavailable(document)
            }
            Self::Retryable(document) => DerivedChangeOutcomeV1::Retryable(document),
        }
    }

    pub(crate) fn authority_conflicted(message: impl Into<String>) -> Self {
        Self::AuthorityConflicted(DerivedAuthorityFailureDocumentV1::new(
            DerivedAuthorityFailureCodeV1::AuthorityConflicted,
            message,
        ))
    }

    pub(crate) fn authority_invalid(message: impl Into<String>) -> Self {
        Self::AuthorityInvalid(DerivedAuthorityFailureDocumentV1::new(
            DerivedAuthorityFailureCodeV1::AuthorityInvalid,
            message,
        ))
    }

    pub(crate) fn projection_unavailable(
        code: DerivedProjectionFailureCodeV1,
        message: impl Into<String>,
    ) -> Self {
        Self::ProjectionUnavailable(DerivedProjectionUnavailableDocumentV1::new(
            code, message, false,
        ))
    }

    pub(crate) fn retryable(
        code: DerivedProjectionFailureCodeV1,
        message: impl Into<String>,
    ) -> Self {
        Self::Retryable(DerivedProjectionUnavailableDocumentV1::new(
            code, message, true,
        ))
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedAuthorityFailureCodeV1 {
    AuthorityConflicted,
    AuthorityInvalid,
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DerivedAuthorityFailureDocumentV1 {
    schema: String,
    version: u32,
    code: DerivedAuthorityFailureCodeV1,
    message: String,
}

impl DerivedAuthorityFailureDocumentV1 {
    pub fn code(&self) -> DerivedAuthorityFailureCodeV1 {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    fn new(code: DerivedAuthorityFailureCodeV1, message: impl Into<String>) -> Self {
        Self {
            schema: AUTHORITY_ERROR_SCHEMA.to_owned(),
            version: ERROR_DOCUMENT_VERSION,
            code,
            message: message.into(),
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedProjectionFailureCodeV1 {
    ProjectionAbsent,
    ProjectionRebuildRequired,
    ProjectionStale,
    ProjectionInvalid,
    ProjectionUnstable,
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DerivedProjectionUnavailableDocumentV1 {
    schema: String,
    version: u32,
    code: DerivedProjectionFailureCodeV1,
    message: String,
    retryable: bool,
}

impl DerivedProjectionUnavailableDocumentV1 {
    pub fn code(&self) -> DerivedProjectionFailureCodeV1 {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn is_retryable(&self) -> bool {
        self.retryable
    }

    fn new(
        code: DerivedProjectionFailureCodeV1,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            schema: PROJECTION_ERROR_SCHEMA.to_owned(),
            version: ERROR_DOCUMENT_VERSION,
            code,
            message: message.into(),
            retryable,
        }
    }
}

/// Authenticated and normalized page request. Token bytes and signatures stay
/// in the Inspector adapter.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DerivedChangePageRequestV1 {
    Bare,
    Bounded(DerivedChangePageSelectionV1),
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedChangePageSelectionV1 {
    limit: usize,
    after: Option<DerivedChangePageContinuationV1>,
    summary_query: Option<String>,
    topology: Option<ChangeTopologyV1>,
    lifecycle: Option<ChangeLifecycleV1>,
    attention: Option<DerivedChangeAttentionFilterV1>,
    availability: Option<DerivedChangeAvailabilityFilterV1>,
}

impl DerivedChangePageSelectionV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        limit: usize,
        after: Option<DerivedChangePageContinuationV1>,
        summary_query: Option<String>,
        topology: Option<ChangeTopologyV1>,
        lifecycle: Option<ChangeLifecycleV1>,
        attention: Option<DerivedChangeAttentionFilterV1>,
        availability: Option<DerivedChangeAvailabilityFilterV1>,
    ) -> Result<Self> {
        if !(1..=MAXIMUM_PAGE_LIMIT).contains(&limit) {
            return Err(ShoreError::Message(
                "derived Change page limit must be between 1 and 100".to_owned(),
            ));
        }
        let summary_query = summary_query
            .map(|query| {
                let query = query.trim();
                if query.is_empty() {
                    return Err(ShoreError::Message(
                        "derived Change summary query is empty".to_owned(),
                    ));
                }
                if query.len() > MAXIMUM_SUMMARY_QUERY_BYTES {
                    return Err(ShoreError::Message(
                        "derived Change summary query exceeds 256 bytes".to_owned(),
                    ));
                }
                Ok(query.to_lowercase())
            })
            .transpose()?;
        Ok(Self {
            limit,
            after,
            summary_query,
            topology,
            lifecycle,
            attention,
            availability,
        })
    }

    pub fn default_page() -> Self {
        Self {
            limit: DEFAULT_PAGE_LIMIT,
            after: None,
            summary_query: None,
            topology: None,
            lifecycle: None,
            attention: None,
            availability: None,
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn after(&self) -> Option<&DerivedChangePageContinuationV1> {
        self.after.as_ref()
    }

    pub fn summary_query(&self) -> Option<&str> {
        self.summary_query.as_deref()
    }

    pub fn topology(&self) -> Option<ChangeTopologyV1> {
        self.topology
    }

    pub fn lifecycle(&self) -> Option<ChangeLifecycleV1> {
        self.lifecycle
    }

    pub fn attention_filter(&self) -> Option<DerivedChangeAttentionFilterV1> {
        self.attention
    }

    pub fn availability_filter(&self) -> Option<DerivedChangeAvailabilityFilterV1> {
        self.availability
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedChangePageContinuationV1 {
    expected_projection_stamp: String,
    boundary: DerivedChangePageBoundaryV1,
}

impl DerivedChangePageContinuationV1 {
    pub fn new(
        expected_projection_stamp: impl Into<String>,
        boundary: DerivedChangePageBoundaryV1,
    ) -> Result<Self> {
        let expected_projection_stamp = expected_projection_stamp.into();
        if expected_projection_stamp.is_empty() {
            return Err(ShoreError::Message(
                "derived Change continuation has no projection stamp".to_owned(),
            ));
        }
        Ok(Self {
            expected_projection_stamp,
            boundary,
        })
    }

    pub fn expected_projection_stamp(&self) -> &str {
        &self.expected_projection_stamp
    }

    pub fn boundary(&self) -> &DerivedChangePageBoundaryV1 {
        &self.boundary
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedChangePageBoundaryV1 {
    last_change_id: Option<ChangeId>,
}

impl DerivedChangePageBoundaryV1 {
    pub fn page_one() -> Self {
        Self {
            last_change_id: None,
        }
    }

    pub fn after(last_change_id: ChangeId) -> Self {
        Self {
            last_change_id: Some(last_change_id),
        }
    }

    pub fn last_change_id(&self) -> Option<&ChangeId> {
        self.last_change_id.as_ref()
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DerivedChangeAttentionFilterV1 {
    Clear,
    InProgress,
    Incomplete,
    Conflicted,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DerivedChangeAvailabilityFilterV1 {
    Available,
    Incomplete,
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedChangePageWindowV1 {
    pub projection_stamp: String,
    pub previous: Option<DerivedChangePageBoundaryV1>,
    pub next: Option<DerivedChangePageBoundaryV1>,
    pub last: Option<DerivedChangePageBoundaryV1>,
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedChangePageV1 {
    pub document: ChangeListPresentationDocumentV1,
    pub window: Option<DerivedChangePageWindowV1>,
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedAttentionPageV1 {
    pub document: ChangeAttentionPresentationDocumentV2,
    pub attention_presentations: BTreeMap<ChangeId, DerivedAttentionPresentationV1>,
    pub window: Option<DerivedChangePageWindowV1>,
}

#[doc(hidden)]
pub type DerivedAttentionPresentationV1 = ChangeAttentionPresentationV1;

#[doc(hidden)]
pub type DerivedAttentionReasonV1 = ChangeAttentionReasonV1;

#[doc(hidden)]
pub type DerivedAttentionReasonPresentationV1 = ChangeAttentionReasonPresentationV1;

#[derive(Clone, Debug, Eq, PartialEq)]
enum PreparedChangePage {
    Changes(DerivedChangePageV1),
    Attention(DerivedAttentionPageV1),
    ReviewList(ChangeListDocumentV1),
    ReviewAttention(ChangeAttentionPresentationDocumentV2),
}

/// Document family composed at the page boundary. The Inspector target keeps
/// the shipped page shapes; the Review target composes the CLI review-schema
/// documents from the same prepared inputs at the same point in the flow, so
/// the terminal stability re-proof covers both.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChangeCompositionTarget {
    Inspector,
    Review,
}

/// Where a Ready derived answer came from: the proven-current generation, or
/// the authoritative capability-carrier control path a non-L2 store answers
/// through. Callers that label their route use this to keep the route-state
/// counters truthful.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DerivedReadSourceV1 {
    Generation,
    CapabilityControlPath,
}

enum ChangeProposalHydrationPlan<'a> {
    Ordinary(BodylessChangePageSelection),
    ExhaustiveSearch { normalized_query: &'a str },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChangeReadBoundary {
    SnapshotPinned,
    BodylessSelectionComplete,
    ProposalLocatorsSelected,
    ProposalHydrationComplete,
    ResponseConstructed,
}

fn capability_unavailable_outcome<T>(
    inspection: &StoreCapabilityInspection,
) -> Option<DerivedChangeOutcomeV1<T>> {
    ChangeQueryUnavailableDocumentV1::for_inspection(inspection)
        .map(DerivedChangeOutcomeV1::AuthorityUnavailable)
}

fn runtime_unavailable_outcome<T>(status: RuntimeCurrentStatus) -> DerivedChangeOutcomeV1<T> {
    let detail = status
        .detail
        .unwrap_or_else(|| "derived Change projection is unavailable".to_owned());
    match status.availability {
        DerivedAccessAvailability::Absent => DerivedChangeOutcomeV1::projection_unavailable(
            DerivedProjectionFailureCodeV1::ProjectionAbsent,
            detail,
        ),
        DerivedAccessAvailability::Bootstrapping | DerivedAccessAvailability::CatchingUp => {
            DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionStale,
                detail,
            )
        }
        DerivedAccessAvailability::RebuildRequired => {
            DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionRebuildRequired,
                detail,
            )
        }
        DerivedAccessAvailability::Quarantined | DerivedAccessAvailability::Unavailable => {
            DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                detail,
            )
        }
        DerivedAccessAvailability::Current => DerivedChangeOutcomeV1::retryable(
            DerivedProjectionFailureCodeV1::ProjectionUnstable,
            "derived Change runtime returned current without a generation",
        ),
    }
}

pub(crate) fn lifecycle_failure_outcome<T>(error: LifecycleError) -> DerivedChangeOutcomeV1<T> {
    let detail = error.to_string();
    match error {
        LifecycleError::TruthChanged
        | LifecycleError::Cancelled
        | LifecycleError::RebuildBusy
        | LifecycleError::WriterLock(_)
        | LifecycleError::Truth(_) => DerivedChangeOutcomeV1::retryable(
            DerivedProjectionFailureCodeV1::ProjectionUnstable,
            detail,
        ),
        LifecycleError::RebuildRequired(_) | LifecycleError::AutomaticRebuildSuppressed => {
            DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionRebuildRequired,
                detail,
            )
        }
        LifecycleError::Disabled
        | LifecycleError::Quarantined(_)
        | LifecycleError::EmptyStoreIdentity
        | LifecycleError::Validation(_)
        | LifecycleError::Generation(_)
        | LifecycleError::Cursor(_)
        | LifecycleError::Service(_) => DerivedChangeOutcomeV1::projection_unavailable(
            DerivedProjectionFailureCodeV1::ProjectionInvalid,
            detail,
        ),
    }
}

pub(super) fn exact_revision_from_proposal(event: &ShoreEvent) -> Result<RevisionRefV1> {
    if event.event_type != EventType::WorkObjectProposed {
        return Err(ShoreError::Message(format!(
            "selected proposal carrier {} has family {}",
            event.event_id.as_str(),
            event.event_type.as_str()
        )));
    }
    let payload: WorkObjectProposedPayload = serde_json::from_value(event.payload.clone())?;
    let WorkObjectProposal::Revision {
        revision,
        object_artifact_content_hash,
        ..
    } = payload.work_object
    else {
        return Err(ShoreError::Message(format!(
            "selected proposal carrier {} is not a Revision proposal",
            event.event_id.as_str()
        )));
    };
    RevisionRefV1::new(revision.id, object_artifact_content_hash)
}

/// Product lens applied before structured filters and window selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChangePageLens {
    Changes,
    Attention,
}

/// Bodyless selection result. Proposal carriers are opened only for these
/// Change identities after this pure step completes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BodylessChangePageSelection {
    pub(crate) change_ids: Vec<ChangeId>,
    pub(crate) window: Option<DerivedChangePageWindowV1>,
}

/// Select an ordinary Changes or Attention page without consulting proposal
/// prose. Summary search deliberately belongs to the exhaustive proposal path.
pub(crate) fn select_bodyless_change_page(
    lens: ChangePageLens,
    summaries: &[ChangeSummaryV1],
    projection_stamp: &str,
    request: &DerivedChangePageRequestV1,
) -> Result<BodylessChangePageSelection> {
    let candidates = select_bodyless_change_candidates(lens, summaries, projection_stamp, request)?;
    if matches!(request, DerivedChangePageRequestV1::Bounded(selection) if selection.summary_query().is_some())
    {
        return Err(ShoreError::Message(
            "derived Change summary query requires exhaustive proposal selection".to_owned(),
        ));
    }
    paginate_bodyless_change_candidates(&candidates, projection_stamp, request)
}

/// Apply the lens and every prose-independent filter without windowing. The
/// exhaustive summary path validates all proposal carriers for this complete
/// candidate set before matching or pagination.
pub(crate) fn select_bodyless_change_candidates(
    lens: ChangePageLens,
    summaries: &[ChangeSummaryV1],
    projection_stamp: &str,
    request: &DerivedChangePageRequestV1,
) -> Result<Vec<ChangeId>> {
    let mut candidates = summaries.iter().collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.change_id.as_str().cmp(right.change_id.as_str()));
    if candidates
        .windows(2)
        .any(|pair| pair[0].change_id == pair[1].change_id)
    {
        return Err(ShoreError::Message(
            "bodyless Change selection contains a duplicate Change identity".to_owned(),
        ));
    }

    if lens == ChangePageLens::Attention {
        candidates.retain(|summary| summary.lifecycle != ChangeLifecycleV1::Accepted);
    }

    if let DerivedChangePageRequestV1::Bounded(selection) = request {
        if selection.after().is_some_and(|continuation| {
            continuation.expected_projection_stamp() != projection_stamp
        }) {
            return Err(ShoreError::Message(
                "derived Change continuation belongs to a different projection".to_owned(),
            ));
        }
        candidates.retain(|summary| {
            selection
                .topology()
                .is_none_or(|topology| summary.topology == topology)
                && selection
                    .lifecycle()
                    .is_none_or(|lifecycle| summary.lifecycle == lifecycle)
                && selection.attention_filter().is_none_or(|attention| {
                    summary.attention_summary == attention_filter_name(attention)
                })
                && selection.availability_filter().is_none_or(|availability| {
                    summary.availability_summary == availability_filter_name(availability)
                })
        });
    }

    Ok(candidates
        .into_iter()
        .map(|summary| summary.change_id.clone())
        .collect())
}

fn paginate_bodyless_change_candidates(
    candidates: &[ChangeId],
    projection_stamp: &str,
    request: &DerivedChangePageRequestV1,
) -> Result<BodylessChangePageSelection> {
    if candidates
        .windows(2)
        .any(|pair| pair[0].as_str() >= pair[1].as_str())
    {
        return Err(ShoreError::Message(
            "bodyless Change candidates are not strictly ordered".to_owned(),
        ));
    }

    let DerivedChangePageRequestV1::Bounded(selection) = request else {
        return Ok(BodylessChangePageSelection {
            change_ids: candidates.to_vec(),
            window: None,
        });
    };

    let start = selection
        .after()
        .and_then(|continuation| continuation.boundary().last_change_id())
        .map_or(0, |boundary| {
            candidates.partition_point(|change_id| change_id.as_str() <= boundary.as_str())
        });
    let end = start
        .saturating_add(selection.limit())
        .min(candidates.len());
    let change_ids = candidates[start..end].to_vec();

    let previous = (start > 0).then(|| {
        let previous_start = start.saturating_sub(selection.limit());
        if previous_start == 0 {
            DerivedChangePageBoundaryV1::page_one()
        } else {
            DerivedChangePageBoundaryV1::after(candidates[previous_start - 1].clone())
        }
    });
    let next = (end < candidates.len())
        .then(|| DerivedChangePageBoundaryV1::after(candidates[end - 1].clone()));
    let last_page_start = candidates
        .len()
        .checked_sub(1)
        .map_or(0, |last| (last / selection.limit()) * selection.limit());
    let last = (last_page_start != start).then(|| {
        if last_page_start == 0 {
            DerivedChangePageBoundaryV1::page_one()
        } else {
            DerivedChangePageBoundaryV1::after(candidates[last_page_start - 1].clone())
        }
    });

    Ok(BodylessChangePageSelection {
        change_ids,
        window: Some(DerivedChangePageWindowV1 {
            projection_stamp: projection_stamp.to_owned(),
            previous,
            next,
            last,
        }),
    })
}

fn attention_filter_name(filter: DerivedChangeAttentionFilterV1) -> &'static str {
    match filter {
        DerivedChangeAttentionFilterV1::Clear => "clear",
        DerivedChangeAttentionFilterV1::InProgress => "in_progress",
        DerivedChangeAttentionFilterV1::Incomplete => "incomplete",
        DerivedChangeAttentionFilterV1::Conflicted => "conflicted",
    }
}

fn availability_filter_name(filter: DerivedChangeAvailabilityFilterV1) -> &'static str {
    match filter {
        DerivedChangeAvailabilityFilterV1::Available => "available",
        DerivedChangeAvailabilityFilterV1::Incomplete => "incomplete",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use rusqlite::params;
    use tempfile::TempDir;

    use super::*;
    use crate::bench_support::longitudinal::LongitudinalCountingScopeV1;
    use crate::crypto::{EventSigner, TestEd25519Signer};
    use crate::documents::{
        ChangeDeclarationStateV1, ReaderProfileAvailabilityV1, change_presentation_projection,
    };
    use crate::model::{
        ChangeIdentityDescriptorV1, EngagementId, JournalId, ObjectId, ObservationId,
        ReviewEndpoint, ReviewId, ReviewTargetRef, RevisionId, RevisionSource, TrackId,
        WorktreeCaptureMode,
    };
    use crate::session::derived_access::layout::{
        DerivedStorageLayout, DerivedStorageNamespace, DerivedStorageTransition,
    };
    use crate::session::derived_access::lifecycle::{DerivedAccessLifecycle, LifecycleControl};
    use crate::session::derived_access::product_contract::DerivedAccessProfile;
    use crate::session::derived_access::runtime::{DerivedAccessMaintenance, DerivedAccessMode};
    use crate::session::derived_access::semantic::change::CHANGE_READER_PROFILE_RESOURCE_V3;
    use crate::session::derived_access::sqlite::StoreWriterLock;
    use crate::session::derived_access::writer::DerivedWriteCoordinator;
    use crate::session::event::{
        ArtifactRemovedPayload, BodyContentType, EventSignature, EventSignatureRecordedPayload,
        EventTarget, EventToBeSigned, FactPortRelationV1, FactRefV1, ReviewFactPortDraftV1,
        ReviewInitializedPayload, ReviewObservationRecordedPayload, Revision, Writer,
        build_change_declared, build_membership_asserted, build_membership_withdrawn,
        build_review_fact_ported, build_revision_relation_asserted,
        build_revision_relation_withdrawn, event_signature_pre_authentication_encoding,
    };
    use crate::session::projection::freshness::event_set_hash_for_events;
    use crate::session::store::backend::StoreBackend;
    use crate::session::store::capabilities::{
        CapabilityFixtureState, inspect_journal_records, write_capability_fixture_for_test,
    };
    use crate::session::store::resolution::{opaque_path_identity, resolve_store};
    use crate::session::{
        AUTHORITY_CURSOR_SCHEMA_V2, AuthorityCursorV2, EventStore, EventWriteOutcome,
        StoreCapabilityInspection,
    };

    const PAGE_TEST_STAMP: &str = "sha256:bodyless-page-test";
    type WindowShape = (
        Option<Option<String>>,
        Option<Option<String>>,
        Option<Option<String>>,
    );

    #[derive(Clone)]
    struct FixtureChange {
        change_id: ChangeId,
        revision: RevisionRefV1,
        proposal_events: Vec<ShoreEvent>,
    }

    struct ActiveChangeFixture {
        _temp: TempDir,
        lifecycle: DerivedAccessLifecycle,
        runtime: Arc<DerivedAccessRuntime>,
        access: DerivedChangeAccess,
        store: EventStore,
        changes: Vec<FixtureChange>,
    }

    impl ActiveChangeFixture {
        fn new(proposal_summaries: &[&[Option<&str>]]) -> Self {
            let temp = TempDir::new().expect("create disposable Change root");
            let backend = StoreBackend::Local(temp.path().to_path_buf());
            write_capability_fixture_for_test(
                backend.journal().as_ref(),
                CapabilityFixtureState::EmptyL2,
            )
            .expect("activate disposable Change root");
            let store_identity =
                opaque_path_identity("store", temp.path()).expect("derive fixture store identity");
            let lifecycle = DerivedAccessLifecycle::new(
                DerivedAccessProfile::SqliteWalBodylessV1,
                temp.path(),
                store_identity.clone(),
            )
            .expect("create Change lifecycle");
            lifecycle
                .rebuild(|_| LifecycleControl::Continue)
                .expect("publish empty L2 generation");
            let coordinator =
                DerivedWriteCoordinator::new(lifecycle.clone()).expect("admit derived writer");
            let store = EventStore::from_backend(&backend).with_coordinator(coordinator);

            let changes = proposal_summaries
                .iter()
                .enumerate()
                .map(|(index, summaries)| {
                    let marker = u8::try_from(index + 1).expect("small fixture");
                    let descriptor = ChangeIdentityDescriptorV1::opaque_nonce([marker; 32]);
                    let declaration =
                        build_change_declared(descriptor, [marker.saturating_add(32); 32])
                            .expect("build Change declaration");
                    let change_id = declaration.change_id.clone();
                    record_fixture_event(
                        &store,
                        ShoreEvent::new(
                            EventType::ChangeDeclared,
                            format!("fixture:change-declared:{index}"),
                            EventTarget::for_journal(JournalId::new("journal:change-endpoint")),
                            Writer::shore_local("change-endpoint-test"),
                            declaration,
                            format!("2026-08-10T01:{index:02}:00Z"),
                        )
                        .expect("build Change declaration event"),
                    );

                    let digest_character = char::from(b'a' + marker);
                    let revision_id = RevisionId::new(format!(
                        "rev:sha256:{}",
                        digest_character.to_string().repeat(64)
                    ));
                    let artifact_hash = format!(
                        "sha256:{}",
                        char::from(b'0' + marker).to_string().repeat(64)
                    );
                    let revision = RevisionRefV1::new(revision_id.clone(), artifact_hash.clone())
                        .expect("build exact Revision");
                    let proposal_events = summaries
                        .iter()
                        .enumerate()
                        .map(|(duplicate, summary)| {
                            let event = ShoreEvent::new(
                                EventType::WorkObjectProposed,
                                format!("fixture:proposal:{index}:{duplicate}"),
                                EventTarget::for_revision(
                                    JournalId::new("journal:change-endpoint"),
                                    revision_id.clone(),
                                    None,
                                )
                                .expect("build proposal target"),
                                Writer::shore_local("change-endpoint-test"),
                                WorkObjectProposedPayload {
                                    engagement_id: EngagementId::new(format!(
                                        "engagement:sha256:{}",
                                        digest_character.to_string().repeat(64)
                                    )),
                                    work_object: WorkObjectProposal::Revision {
                                        revision: Revision {
                                            id: revision_id.clone(),
                                            object_id: ObjectId::new(format!(
                                                "obj:sha256:{}",
                                                digest_character.to_string().repeat(64)
                                            )),
                                            git_provenance: None,
                                        },
                                        summary: summary.map(str::to_owned),
                                        object_artifact_content_hash: artifact_hash.clone(),
                                        supersedes: Vec::new(),
                                    },
                                },
                                format!("2026-08-10T01:{index:02}:{:02}Z", duplicate + 1),
                            )
                            .expect("build proposal event");
                            record_fixture_event(&store, event.clone());
                            event
                        })
                        .collect::<Vec<_>>();

                    let membership = build_membership_asserted(
                        &change_id,
                        &revision_id,
                        [marker.saturating_add(64); 32],
                    )
                    .expect("build membership");
                    record_fixture_event(
                        &store,
                        ShoreEvent::new(
                            EventType::ChangeMembershipAsserted,
                            format!("fixture:membership:{index}"),
                            EventTarget::for_journal(JournalId::new("journal:change-endpoint")),
                            Writer::shore_local("change-endpoint-test"),
                            membership,
                            format!("2026-08-10T01:{index:02}:59Z"),
                        )
                        .expect("build membership event"),
                    );
                    FixtureChange {
                        change_id,
                        revision,
                        proposal_events,
                    }
                })
                .collect();

            let runtime = DerivedAccessRuntime::from_mode(DerivedAccessMode::Active {
                lifecycle: lifecycle.clone(),
                current: Mutex::new(None),
                store_identity,
                backend,
            });
            let access = DerivedChangeAccess::from_runtime(Arc::clone(&runtime));
            Self {
                _temp: temp,
                lifecycle,
                runtime,
                access,
                store,
                changes,
            }
        }

        fn repo_bound_access(&self) -> DerivedChangeAccess {
            if !self._temp.path().join(".git").is_dir() {
                assert!(
                    std::process::Command::new("git")
                        .args(["init", "--quiet"])
                        .current_dir(self._temp.path())
                        .status()
                        .expect("initialize fixture repository")
                        .success()
                );
            }
            DerivedChangeAccess {
                runtime: Arc::clone(&self.runtime),
                repo: Some(self._temp.path().to_path_buf()),
            }
        }

        fn append_unrelated(&self, suffix: &str) {
            let journal_id = JournalId::new(format!("journal:change-endpoint:{suffix}"));
            record_fixture_event(
                &self.store,
                ShoreEvent::new(
                    EventType::ReviewInitialized,
                    ReviewInitializedPayload::idempotency_key(&journal_id),
                    EventTarget::for_journal(journal_id),
                    Writer::shore_local("change-endpoint-test"),
                    ReviewInitializedPayload {},
                    "2026-08-10T02:00:00Z",
                )
                .expect("build unrelated append"),
            );
        }

        fn append_removal_support(&self, revision: &RevisionRefV1) -> (ShoreEvent, ShoreEvent) {
            let removal = ShoreEvent::new(
                EventType::ArtifactRemoved,
                ArtifactRemovedPayload::idempotency_key(&revision.object_artifact_content_hash),
                EventTarget::for_journal(JournalId::new("journal:change-endpoint")),
                Writer::shore_local("change-endpoint-test"),
                ArtifactRemovedPayload {
                    content_hash: revision.object_artifact_content_hash.clone(),
                },
                "2026-08-10T02:01:00Z",
            )
            .expect("build removal support");
            record_fixture_event(&self.store, removal.clone());

            let signer = TestEd25519Signer::from_seed([91; 32]);
            let to_be_signed = EventToBeSigned::from_event(&removal, signer.signer_id())
                .expect("build removal signature message");
            let signature = signer
                .sign_event_message(
                    &event_signature_pre_authentication_encoding(&to_be_signed)
                        .expect("encode removal signature message"),
                )
                .expect("sign removal support");
            let payload = EventSignatureRecordedPayload {
                target_event_id: removal.event_id.clone(),
                target_event_record_hash: removal
                    .event_record_hash()
                    .expect("hash removal support"),
                attesting_signer: signer.signer_id().clone(),
                attestation: EventSignature::ed25519_v1(signature),
                inclusion_proof: None,
            };
            let carrier = ShoreEvent::new(
                EventType::EventSignatureRecorded,
                EventSignatureRecordedPayload::idempotency_key(
                    &payload.target_event_record_hash,
                    &payload.attesting_signer,
                    payload.attestation.sig.as_str(),
                ),
                EventTarget::for_journal(JournalId::new("journal:change-endpoint")),
                Writer::shore_local("change-endpoint-test"),
                payload,
                "2026-08-10T02:01:01Z",
            )
            .expect("build detached removal signature");
            record_fixture_event(&self.store, carrier.clone());
            (removal, carrier)
        }

        fn append_historical_membership_then_withdraw(
            &self,
            change_id: &ChangeId,
            revision: &RevisionRefV1,
        ) -> (ShoreEvent, ShoreEvent) {
            let membership = build_membership_asserted(change_id, &revision.revision_id, [121; 32])
                .expect("build historical membership");
            let membership_event = ShoreEvent::new(
                EventType::ChangeMembershipAsserted,
                "fixture:historical-membership",
                EventTarget::for_journal(JournalId::new("journal:change-endpoint")),
                Writer::shore_local("change-endpoint-test"),
                membership.clone(),
                "2026-08-10T02:02:00Z",
            )
            .expect("build historical membership event");
            record_fixture_event(&self.store, membership_event.clone());

            let withdrawal = build_membership_withdrawn(&membership.membership_claim_id, [122; 32])
                .expect("build historical membership withdrawal");
            let withdrawal_event = ShoreEvent::new(
                EventType::ChangeMembershipWithdrawn,
                "fixture:historical-membership-withdrawal",
                EventTarget::for_journal(JournalId::new("journal:change-endpoint")),
                Writer::shore_local("change-endpoint-test"),
                withdrawal,
                "2026-08-10T02:02:01Z",
            )
            .expect("build historical membership withdrawal event");
            record_fixture_event(&self.store, withdrawal_event.clone());
            (membership_event, withdrawal_event)
        }

        fn append_conflicting_proposal(&self, revision_id: &RevisionId) -> ShoreEvent {
            let artifact_hash = format!("sha256:{}", "9".repeat(64));
            let event = ShoreEvent::new(
                EventType::WorkObjectProposed,
                "fixture:conflicting-proposal",
                EventTarget::for_revision(
                    JournalId::new("journal:change-endpoint"),
                    revision_id.clone(),
                    None,
                )
                .expect("build conflicting proposal target"),
                Writer::shore_local("change-endpoint-test"),
                WorkObjectProposedPayload {
                    engagement_id: EngagementId::new(format!(
                        "engagement:sha256:{}",
                        "9".repeat(64)
                    )),
                    work_object: WorkObjectProposal::Revision {
                        revision: Revision {
                            id: revision_id.clone(),
                            object_id: ObjectId::new(format!("obj:sha256:{}", "9".repeat(64))),
                            git_provenance: None,
                        },
                        summary: Some("conflicting proposal".to_owned()),
                        object_artifact_content_hash: artifact_hash,
                        supersedes: Vec::new(),
                    },
                },
                "2026-08-10T02:03:00Z",
            )
            .expect("build conflicting proposal event");
            record_fixture_event(&self.store, event.clone());
            event
        }

        fn append_inline_signed_review_initialized(
            &self,
            invalid_algorithm: bool,
        ) -> (ShoreEvent, String) {
            let signer = TestEd25519Signer::from_seed([92; 32]);
            let mut event = ShoreEvent::new(
                EventType::ReviewInitialized,
                if invalid_algorithm {
                    "fixture:inline-signed-invalid"
                } else {
                    "fixture:inline-signed-valid"
                },
                EventTarget::for_journal(JournalId::new("journal:change-endpoint")),
                Writer::shore_local("change-endpoint-test"),
                ReviewInitializedPayload {},
                if invalid_algorithm {
                    "2026-08-10T02:04:01Z"
                } else {
                    "2026-08-10T02:04:00Z"
                },
            )
            .expect("build inline-signed Timeline event");
            let to_be_signed = EventToBeSigned::from_event(&event, signer.signer_id())
                .expect("build inline Timeline signature message");
            let signature = signer
                .sign_event_message(
                    &event_signature_pre_authentication_encoding(&to_be_signed)
                        .expect("encode inline Timeline signature message"),
                )
                .expect("sign inline Timeline event");
            event.signer = Some(signer.signer_id().clone());
            event.signature = Some(EventSignature::ed25519_v1(signature));
            if invalid_algorithm {
                event.signature.as_mut().expect("attached signature").alg = "invalid".to_owned();
            }
            let signer_id = signer.signer_id().as_str().to_owned();
            record_fixture_event(&self.store, event.clone());
            (event, signer_id)
        }

        fn append_relation_then_withdraw(
            &self,
            change_id: &ChangeId,
            successor: RevisionRefV1,
            predecessor: RevisionRefV1,
        ) -> (ShoreEvent, ShoreEvent) {
            let relation =
                build_revision_relation_asserted(change_id, successor, predecessor, [123; 32])
                    .expect("build historical Revision relation");
            let relation_event = ShoreEvent::new(
                EventType::ChangeRevisionRelationAsserted,
                "fixture:historical-relation",
                EventTarget::for_journal(JournalId::new("journal:change-endpoint")),
                Writer::shore_local("change-endpoint-test"),
                relation.clone(),
                "2026-08-10T02:05:00Z",
            )
            .expect("build historical Revision relation event");
            record_fixture_event(&self.store, relation_event.clone());
            let withdrawal =
                build_revision_relation_withdrawn(&relation.relation_claim_id, [124; 32])
                    .expect("build historical Revision relation withdrawal");
            let withdrawal_event = ShoreEvent::new(
                EventType::ChangeRevisionRelationWithdrawn,
                "fixture:historical-relation-withdrawal",
                EventTarget::for_journal(JournalId::new("journal:change-endpoint")),
                Writer::shore_local("change-endpoint-test"),
                withdrawal,
                "2026-08-10T02:05:01Z",
            )
            .expect("build historical Revision relation withdrawal event");
            record_fixture_event(&self.store, withdrawal_event.clone());
            (relation_event, withdrawal_event)
        }

        fn database_path(&self) -> PathBuf {
            let generation_id = self
                .lifecycle
                .published_generation_id()
                .expect("read fixture publication")
                .expect("fixture generation is published");
            self.lifecycle
                .paths()
                .generation(&generation_id)
                .join("cursor.sqlite3")
        }

        fn receipt_path(&self) -> PathBuf {
            let generation_id = self
                .lifecycle
                .published_generation_id()
                .expect("read fixture publication")
                .expect("fixture generation is published");
            self.lifecycle
                .paths()
                .generation(&generation_id)
                .join(CHANGE_READER_PROFILE_RESOURCE_V3)
        }

        fn publication_path(&self) -> PathBuf {
            fs::read_dir(self.lifecycle.paths().root().join("publications"))
                .expect("read disposable publication directory")
                .map(|entry| entry.expect("read disposable publication entry").path())
                .find(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
                .expect("fixture has a current publication record")
        }

        fn proposal_sequence(&self, event: &ShoreEvent) -> i64 {
            rusqlite::Connection::open(self.database_path())
                .expect("open fixture sidecar")
                .query_row(
                    "SELECT sequence FROM locator_event_text WHERE event_id = ?1",
                    [event.event_id.as_str()],
                    |row| row.get(0),
                )
                .expect("locate fixture proposal sequence")
        }

        fn mutate_database(&self, mutate: impl FnOnce(&rusqlite::Connection)) {
            let connection =
                rusqlite::Connection::open(self.database_path()).expect("open fixture sidecar");
            mutate(&connection);
            connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
                .expect("checkpoint fixture mutation");
        }

        fn fresh_access(&self) -> DerivedChangeAccess {
            let runtime = DerivedAccessRuntime::from_mode(DerivedAccessMode::Active {
                lifecycle: self.lifecycle.clone(),
                current: Mutex::new(None),
                store_identity: opaque_path_identity("store", self._temp.path())
                    .expect("derive fresh fixture store identity"),
                backend: StoreBackend::Local(self._temp.path().to_path_buf()),
            });
            DerivedChangeAccess::from_runtime(runtime)
        }
    }

    fn record_fixture_event(store: &EventStore, event: ShoreEvent) {
        assert_eq!(
            store
                .record_event_once(&event)
                .expect("record fixture event"),
            EventWriteOutcome::Created
        );
    }

    fn append_artifact_backed_change(
        fixture: &ActiveChangeFixture,
        with_external_body: bool,
    ) -> FixtureChange {
        let backend = StoreBackend::Local(fixture._temp.path().to_path_buf());
        let revision_id = RevisionId::new(format!("rev:sha256:{}", "8".repeat(64)));
        let object_id = ObjectId::new(format!("obj:sha256:{}", "8".repeat(64)));
        let engagement_id = EngagementId::new(format!("engagement:sha256:{}", "8".repeat(64)));
        let snapshot = crate::model::DiffSnapshot::new(
            ReviewId::new("review:exact-session-artifact"),
            object_id.clone(),
            Vec::new(),
        );
        let artifact = crate::session::build_object_artifact_v2(snapshot)
            .expect("build exact-session object artifact");
        let fingerprint = crate::session::RevisionFingerprint {
            revision_id: revision_id.clone(),
            object_id: object_id.clone(),
            engagement_id: engagement_id.clone(),
            source: RevisionSource::GitWorktree {
                mode: WorktreeCaptureMode::CombinedHeadToWorkingTree,
                include_untracked: false,
                pathspecs: Vec::new(),
            },
            base: ReviewEndpoint::GitWorkingTree {
                worktree_root: fixture._temp.path().display().to_string(),
            },
            target: ReviewEndpoint::GitWorkingTree {
                worktree_root: fixture._temp.path().display().to_string(),
            },
        };
        let artifact = crate::session::object_artifact::write_prepared_object_artifact_to(
            &backend,
            &fingerprint,
            artifact,
        )
        .expect("store exact-session object artifact");
        let revision = RevisionRefV1::new(revision_id.clone(), artifact.content_hash)
            .expect("build artifact-backed exact Revision");

        let declaration = build_change_declared(
            ChangeIdentityDescriptorV1::opaque_nonce([188; 32]),
            [189; 32],
        )
        .expect("build artifact-backed Change declaration");
        let change_id = declaration.change_id.clone();
        record_fixture_event(
            &fixture.store,
            ShoreEvent::new(
                EventType::ChangeDeclared,
                "fixture:artifact-backed-change",
                EventTarget::for_journal(JournalId::new("journal:exact-session-artifact")),
                Writer::shore_local("exact-session-test"),
                declaration,
                "2026-08-10T06:00:00Z",
            )
            .expect("build artifact-backed declaration event"),
        );
        let proposal = ShoreEvent::new(
            EventType::WorkObjectProposed,
            "fixture:artifact-backed-proposal",
            EventTarget::for_revision(
                JournalId::new("journal:exact-session-artifact"),
                revision_id.clone(),
                None,
            )
            .expect("build artifact-backed proposal target"),
            Writer::shore_local("exact-session-test"),
            WorkObjectProposedPayload {
                engagement_id,
                work_object: WorkObjectProposal::Revision {
                    revision: Revision {
                        id: revision_id.clone(),
                        object_id,
                        git_provenance: None,
                    },
                    summary: Some("artifact-backed exact Revision".to_owned()),
                    object_artifact_content_hash: revision.object_artifact_content_hash.clone(),
                    supersedes: Vec::new(),
                },
            },
            "2026-08-10T06:00:01Z",
        )
        .expect("build artifact-backed proposal event");
        record_fixture_event(&fixture.store, proposal.clone());

        if with_external_body {
            let body = "external exact-Revision body\n".repeat(200);
            let crate::session::body_artifact::BodyArtifactOutcome::Artifact {
                relative_path,
                byte_size,
                body_envelope,
            } = crate::session::body_artifact::stage_body_artifact(body.as_bytes())
                .expect("stage external exact-session body")
            else {
                panic!("the exact-session body must cross the artifact threshold");
            };
            backend
                .content_store()
                .put_once(
                    &relative_path,
                    &body_envelope
                        .to_json_bytes()
                        .expect("encode exact-session body envelope"),
                )
                .expect("store exact-session body artifact");
            let body_content_hash =
                crate::session::body_artifact::note_body_content_hash_from_path(&relative_path)
                    .expect("derive exact-session body hash");
            let track_id = TrackId::new("track:exact-session-artifact");
            record_fixture_event(
                &fixture.store,
                ShoreEvent::new(
                    EventType::ReviewObservationRecorded,
                    ReviewObservationRecordedPayload::idempotency_key(
                        &revision_id,
                        &track_id,
                        "external-body",
                    ),
                    EventTarget::for_revision(
                        JournalId::new("journal:exact-session-artifact"),
                        revision_id.clone(),
                        Some(track_id),
                    )
                    .expect("build body observation target"),
                    Writer::shore_local("exact-session-test"),
                    ReviewObservationRecordedPayload {
                        observation_id: ObservationId::new(format!(
                            "observation:sha256:{}",
                            "8".repeat(64)
                        )),
                        target: ReviewTargetRef::Revision {
                            revision_id: revision_id.clone(),
                        },
                        title: "external body".to_owned(),
                        body: None,
                        body_content_type: BodyContentType::TextPlain,
                        body_artifact_path: Some(relative_path),
                        body_byte_size: Some(byte_size),
                        body_content_hash: Some(body_content_hash),
                        tags: Vec::new(),
                        confidence: None,
                        supersedes_observation_ids: Vec::new(),
                        responds_to_observation_ids: Vec::new(),
                    },
                    "2026-08-10T06:00:02Z",
                )
                .expect("build external-body observation event"),
            );
        }

        let membership = build_membership_asserted(&change_id, &revision_id, [190; 32])
            .expect("build artifact-backed membership");
        record_fixture_event(
            &fixture.store,
            ShoreEvent::new(
                EventType::ChangeMembershipAsserted,
                "fixture:artifact-backed-membership",
                EventTarget::for_journal(JournalId::new("journal:exact-session-artifact")),
                Writer::shore_local("exact-session-test"),
                membership,
                "2026-08-10T06:00:03Z",
            )
            .expect("build artifact-backed membership event"),
        );
        FixtureChange {
            change_id,
            revision,
            proposal_events: vec![proposal],
        }
    }

    fn normalize_list_projection_stamp(
        document: &mut ChangeListPresentationDocumentV1,
        projection_stamp: &str,
    ) {
        document.document.projection_stamp = projection_stamp.to_owned();
        for change in &mut document.document.changes {
            change.projection_stamp = projection_stamp.to_owned();
        }
    }

    fn normalize_attention_projection_stamp(
        document: &mut ChangeAttentionPresentationDocumentV2,
        projection_stamp: &str,
    ) {
        document.document.projection_stamp = projection_stamp.to_owned();
        for change in &mut document.document.changes {
            change.projection_stamp = projection_stamp.to_owned();
        }
    }

    fn derived_change_outcome_class<T>(outcome: &DerivedChangeOutcomeV1<T>) -> &'static str {
        match outcome {
            DerivedChangeOutcomeV1::Ready(_) => "ready",
            DerivedChangeOutcomeV1::AuthorityUnavailable(_) => "authority-unavailable",
            DerivedChangeOutcomeV1::AuthorityConflicted(_) => "authority-conflicted",
            DerivedChangeOutcomeV1::AuthorityInvalid(_) => "authority-invalid",
            DerivedChangeOutcomeV1::ReaderUpgradeRequired(_) => "reader-upgrade-required",
            DerivedChangeOutcomeV1::ProjectionUnavailable(_) => "projection-unavailable",
            DerivedChangeOutcomeV1::Retryable(_) => "retryable",
        }
    }

    fn assert_projection_invalid<T>(outcome: DerivedChangeOutcomeV1<T>, expected: &str) {
        let DerivedChangeOutcomeV1::ProjectionUnavailable(document) = outcome else {
            panic!("invalid selected carrier state must fail the complete response");
        };
        assert_eq!(
            document.code(),
            DerivedProjectionFailureCodeV1::ProjectionInvalid
        );
        assert!(!document.is_retryable());
        assert!(
            document.message().contains(expected),
            "expected {expected:?} in {:?}",
            document.message()
        );
    }

    fn assert_m1_unavailable<T>(outcome: DerivedChangeOutcomeV1<T>) {
        assert!(matches!(
            outcome,
            DerivedChangeOutcomeV1::AuthorityUnavailable(
                ChangeQueryUnavailableDocumentV1::MigrationInProgress { .. }
            )
        ));
    }

    fn bodyless_summary(
        change_id: impl Into<String>,
        topology: ChangeTopologyV1,
        lifecycle: ChangeLifecycleV1,
        attention: DerivedChangeAttentionFilterV1,
        availability: DerivedChangeAvailabilityFilterV1,
    ) -> ChangeSummaryV1 {
        ChangeSummaryV1 {
            change_id: ChangeId::new(change_id),
            declaration_state: ChangeDeclarationStateV1::Authoritative,
            title_assertions: Vec::new(),
            member_count: 0,
            current_revision_refs: Vec::new(),
            topology,
            lifecycle,
            attention_summary: attention_filter_name(attention).to_owned(),
            availability_summary: availability_filter_name(availability).to_owned(),
            diagnostics: Vec::new(),
            projection_stamp: PAGE_TEST_STAMP.to_owned(),
        }
    }

    fn bodyless_sequence(count: usize) -> Vec<ChangeSummaryV1> {
        (1..=count)
            .rev()
            .map(|index| {
                bodyless_summary(
                    format!("change:{index:03}"),
                    ChangeTopologyV1::Initial,
                    ChangeLifecycleV1::InProgress,
                    DerivedChangeAttentionFilterV1::InProgress,
                    DerivedChangeAvailabilityFilterV1::Available,
                )
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn bounded_request(
        limit: usize,
        boundary: Option<DerivedChangePageBoundaryV1>,
        topology: Option<ChangeTopologyV1>,
        lifecycle: Option<ChangeLifecycleV1>,
        attention: Option<DerivedChangeAttentionFilterV1>,
        availability: Option<DerivedChangeAvailabilityFilterV1>,
    ) -> DerivedChangePageRequestV1 {
        DerivedChangePageRequestV1::Bounded(
            DerivedChangePageSelectionV1::new(
                limit,
                boundary.map(|boundary| {
                    DerivedChangePageContinuationV1::new(PAGE_TEST_STAMP, boundary).unwrap()
                }),
                None,
                topology,
                lifecycle,
                attention,
                availability,
            )
            .unwrap(),
        )
    }

    fn bounded_search_request(
        query: &str,
        limit: usize,
        lifecycle: Option<ChangeLifecycleV1>,
    ) -> DerivedChangePageRequestV1 {
        DerivedChangePageRequestV1::Bounded(
            DerivedChangePageSelectionV1::new(
                limit,
                None,
                Some(query.to_owned()),
                None,
                lifecycle,
                None,
                None,
            )
            .expect("build bounded Change search"),
        )
    }

    fn fixture_proposal_summary(change: &FixtureChange) -> String {
        let payload: WorkObjectProposedPayload = serde_json::from_value(
            change
                .proposal_events
                .first()
                .expect("fixture Change has a proposal")
                .payload
                .clone(),
        )
        .expect("decode fixture proposal");
        let WorkObjectProposal::Revision {
            summary: Some(summary),
            ..
        } = payload.work_object
        else {
            panic!("fixture proposal must carry a Revision summary")
        };
        summary
    }

    fn selected_ids(selection: &BodylessChangePageSelection) -> Vec<String> {
        selection
            .change_ids
            .iter()
            .map(|change_id| change_id.as_str().to_owned())
            .collect()
    }

    fn boundary_shape(boundary: &Option<DerivedChangePageBoundaryV1>) -> Option<Option<String>> {
        boundary.as_ref().map(|boundary| {
            boundary
                .last_change_id()
                .map(|change_id| change_id.as_str().to_owned())
        })
    }

    fn window_shapes(selection: &BodylessChangePageSelection) -> WindowShape {
        let window = selection.window.as_ref().expect("bounded page window");
        assert_eq!(window.projection_stamp, PAGE_TEST_STAMP);
        (
            boundary_shape(&window.previous),
            boundary_shape(&window.next),
            boundary_shape(&window.last),
        )
    }

    fn attention_filter_name(filter: DerivedChangeAttentionFilterV1) -> &'static str {
        match filter {
            DerivedChangeAttentionFilterV1::Clear => "clear",
            DerivedChangeAttentionFilterV1::InProgress => "in_progress",
            DerivedChangeAttentionFilterV1::Incomplete => "incomplete",
            DerivedChangeAttentionFilterV1::Conflicted => "conflicted",
        }
    }

    fn availability_filter_name(filter: DerivedChangeAvailabilityFilterV1) -> &'static str {
        match filter {
            DerivedChangeAvailabilityFilterV1::Available => "available",
            DerivedChangeAvailabilityFilterV1::Incomplete => "incomplete",
        }
    }

    fn empty_authority_cursor() -> AuthorityCursorV2 {
        AuthorityCursorV2 {
            schema: AUTHORITY_CURSOR_SCHEMA_V2.to_owned(),
            journal_record_count: 0,
            event_count: 0,
            journal_record_set_hash: format!("sha256:{}", "0".repeat(64)),
            event_set_hash: format!("sha256:{}", "0".repeat(64)),
            capability_set_hash: format!("sha256:{}", "0".repeat(64)),
        }
    }

    fn inspector_http_status<T>(outcome: &DerivedChangeOutcomeV1<T>) -> u16 {
        match outcome {
            DerivedChangeOutcomeV1::Ready(_) => 200,
            DerivedChangeOutcomeV1::AuthorityUnavailable(_)
            | DerivedChangeOutcomeV1::AuthorityConflicted(_)
            | DerivedChangeOutcomeV1::AuthorityInvalid(_) => 409,
            DerivedChangeOutcomeV1::ReaderUpgradeRequired(_) => 426,
            DerivedChangeOutcomeV1::ProjectionUnavailable(_)
            | DerivedChangeOutcomeV1::Retryable(_) => 503,
        }
    }

    #[test]
    fn m1_control_path_invalidates_a_preactivation_current_without_semantic_fallback() {
        let temp = TempDir::new().expect("create disposable pre-activation root");
        let backend = StoreBackend::Local(temp.path().to_path_buf());
        let store_identity =
            opaque_path_identity("store", temp.path()).expect("derive disposable store identity");
        let lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            temp.path(),
            store_identity.clone(),
        )
        .expect("create pre-activation lifecycle");
        lifecycle
            .rebuild(|_| LifecycleControl::Continue)
            .expect("publish pre-activation derived generation");
        assert!(
            lifecycle
                .open_current()
                .expect("open pre-activation current")
                .is_some(),
            "the fixture must begin with a published pre-activation generation"
        );

        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::M1)
            .expect("write M1 capability fixture");
        let runtime = DerivedAccessRuntime::from_mode(DerivedAccessMode::Active {
            lifecycle,
            current: Mutex::new(None),
            store_identity,
            backend,
        });
        let access = DerivedChangeAccess::from_runtime(runtime);

        let scope = LongitudinalCountingScopeV1::new("f".repeat(64)).unwrap();
        let guard = scope.enter();
        let profile = access.profile().expect("classify M1 profile");
        let changes = access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("classify M1 Changes");
        let attention = access
            .attention(&DerivedChangePageRequestV1::Bare)
            .expect("classify M1 Attention");
        drop(guard);

        let DerivedChangeOutcomeV1::Ready(profile) = profile else {
            panic!("M1 Profile must remain a control document");
        };
        assert_eq!(
            profile.availability,
            ReaderProfileAvailabilityV1::MigrationInProgress
        );
        assert_m1_unavailable(changes);
        assert_m1_unavailable(attention);

        let counters = scope.snapshot().counters;
        assert_eq!(counters.authoritative_fallbacks, 0);
        assert_eq!(counters.full_history_fallbacks, 0);
    }

    #[test]
    fn l0_control_path_survives_a_preactivation_generation_without_a_live_checkpoint() {
        let temp = TempDir::new().expect("create disposable pre-activation root");
        let backend = StoreBackend::Local(temp.path().to_path_buf());
        let store_identity =
            opaque_path_identity("store", temp.path()).expect("derive disposable store identity");
        let lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            temp.path(),
            store_identity.clone(),
        )
        .expect("create pre-activation lifecycle");
        lifecycle
            .rebuild(|_| LifecycleControl::Continue)
            .expect("publish pre-activation derived generation");

        let runtime = DerivedAccessRuntime::from_mode(DerivedAccessMode::Active {
            lifecycle,
            current: Mutex::new(None),
            store_identity,
            backend,
        });
        let access = DerivedChangeAccess::from_runtime(runtime);

        let profile = access.profile().expect("classify L0 profile");
        let changes = access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("classify L0 Changes");
        let attention = access
            .attention(&DerivedChangePageRequestV1::Bare)
            .expect("classify L0 Attention");

        let DerivedChangeOutcomeV1::Ready(profile) = profile else {
            panic!("L0 Profile must remain a control document");
        };
        assert_eq!(
            profile.availability,
            ReaderProfileAvailabilityV1::MigrationRequired
        );
        for outcome in [changes.map_ready(|_| ()), attention.map_ready(|_| ())] {
            assert!(matches!(
                outcome,
                DerivedChangeOutcomeV1::AuthorityUnavailable(
                    ChangeQueryUnavailableDocumentV1::MigrationRequired { .. }
                )
            ));
        }
    }

    #[test]
    fn derived_change_outcomes_keep_failure_axes_distinct() {
        let ready = DerivedChangeOutcomeV1::Ready(());
        let unavailable = DerivedChangeOutcomeV1::<()>::AuthorityUnavailable(
            ChangeQueryUnavailableDocumentV1::MigrationRequired {
                schema: "pointbreak.store-migration-required".to_owned(),
                version: 1,
                authority_cursor: empty_authority_cursor(),
            },
        );
        let conflicted = DerivedChangeOutcomeV1::<()>::authority_conflicted("ambiguous authority");
        let invalid = DerivedChangeOutcomeV1::<()>::authority_invalid("invalid authority");
        let projection = DerivedChangeOutcomeV1::<()>::projection_unavailable(
            DerivedProjectionFailureCodeV1::ProjectionInvalid,
            "invalid projection",
        );
        let retryable = DerivedChangeOutcomeV1::<()>::retryable(
            DerivedProjectionFailureCodeV1::ProjectionUnstable,
            "projection moved",
        );
        let upgrade = DerivedChangeOutcomeV1::<()>::ReaderUpgradeRequired(
            ReaderUpgradeRequiredDocumentV1::new(
                "unsupported_reader_profile",
                Some("review_change_revision_v1".to_owned()),
            ),
        );

        assert_eq!(inspector_http_status(&ready), 200);
        assert_eq!(inspector_http_status(&unavailable), 409);
        assert_eq!(inspector_http_status(&conflicted), 409);
        assert_eq!(inspector_http_status(&invalid), 409);
        assert_eq!(inspector_http_status(&upgrade), 426);
        assert_eq!(inspector_http_status(&projection), 503);
        assert_eq!(inspector_http_status(&retryable), 503);

        let DerivedChangeOutcomeV1::AuthorityConflicted(document) = conflicted else {
            panic!("authority conflict changed axes");
        };
        assert_eq!(
            serde_json::to_value(document).unwrap(),
            serde_json::json!({
                "schema": AUTHORITY_ERROR_SCHEMA,
                "version": 1,
                "code": "authority_conflicted",
                "message": "ambiguous authority",
            })
        );
        let DerivedChangeOutcomeV1::AuthorityInvalid(document) = invalid else {
            panic!("invalid authority changed axes");
        };
        assert_eq!(
            document.code(),
            DerivedAuthorityFailureCodeV1::AuthorityInvalid
        );
        let DerivedChangeOutcomeV1::ProjectionUnavailable(document) = projection else {
            panic!("projection failure changed axes");
        };
        assert_eq!(
            serde_json::to_value(document).unwrap(),
            serde_json::json!({
                "schema": PROJECTION_ERROR_SCHEMA,
                "version": 1,
                "code": "projection_invalid",
                "message": "invalid projection",
                "retryable": false,
            })
        );
        let DerivedChangeOutcomeV1::Retryable(document) = retryable else {
            panic!("retryable projection state changed axes");
        };
        assert_eq!(
            document.code(),
            DerivedProjectionFailureCodeV1::ProjectionUnstable
        );
        assert!(document.is_retryable());
        assert_eq!(serde_json::to_value(document).unwrap()["retryable"], true);
    }

    #[test]
    fn derived_failure_codes_have_exact_wire_names() {
        for (code, expected) in [
            (
                DerivedAuthorityFailureCodeV1::AuthorityConflicted,
                "authority_conflicted",
            ),
            (
                DerivedAuthorityFailureCodeV1::AuthorityInvalid,
                "authority_invalid",
            ),
        ] {
            assert_eq!(serde_json::to_value(code).unwrap(), expected);
        }
        for (code, expected) in [
            (
                DerivedProjectionFailureCodeV1::ProjectionAbsent,
                "projection_absent",
            ),
            (
                DerivedProjectionFailureCodeV1::ProjectionRebuildRequired,
                "projection_rebuild_required",
            ),
            (
                DerivedProjectionFailureCodeV1::ProjectionStale,
                "projection_stale",
            ),
            (
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                "projection_invalid",
            ),
            (
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "projection_unstable",
            ),
        ] {
            assert_eq!(serde_json::to_value(code).unwrap(), expected);
        }
    }

    #[test]
    fn derived_change_selection_is_normalized_and_token_free() {
        let selection = DerivedChangePageSelectionV1::new(
            25,
            Some(
                DerivedChangePageContinuationV1::new(
                    "sha256:current",
                    DerivedChangePageBoundaryV1::page_one(),
                )
                .unwrap(),
            ),
            Some("  Mixed CASE  ".to_owned()),
            Some(ChangeTopologyV1::ParallelCurrent),
            Some(ChangeLifecycleV1::InProgress),
            Some(DerivedChangeAttentionFilterV1::InProgress),
            Some(DerivedChangeAvailabilityFilterV1::Available),
        )
        .unwrap();

        assert_eq!(selection.limit(), 25);
        assert_eq!(selection.summary_query(), Some("mixed case"));
        assert_eq!(
            selection.topology(),
            Some(ChangeTopologyV1::ParallelCurrent)
        );
        assert_eq!(
            selection
                .after()
                .expect("continuation")
                .boundary()
                .last_change_id(),
            None
        );
        assert!(DerivedChangePageSelectionV1::new(0, None, None, None, None, None, None).is_err());
        assert!(
            DerivedChangePageSelectionV1::new(101, None, None, None, None, None, None).is_err()
        );
        assert!(
            DerivedChangePageSelectionV1::new(
                50,
                None,
                Some("  ".to_owned()),
                None,
                None,
                None,
                None,
            )
            .is_err()
        );
        let unicode_boundary = "İ".repeat(128);
        assert_eq!(unicode_boundary.len(), MAXIMUM_SUMMARY_QUERY_BYTES);
        let normalized = DerivedChangePageSelectionV1::new(
            50,
            None,
            Some(unicode_boundary),
            None,
            None,
            None,
            None,
        )
        .expect("length is checked before Unicode lowercase expansion");
        assert!(normalized.summary_query().unwrap().len() > MAXIMUM_SUMMARY_QUERY_BYTES);
    }

    #[test]
    fn ordinary_bodyless_change_pages_freeze_bare_empty_and_window_navigation() {
        let rows = bodyless_sequence(7);

        let bare = select_bodyless_change_page(
            ChangePageLens::Changes,
            &rows,
            PAGE_TEST_STAMP,
            &DerivedChangePageRequestV1::Bare,
        )
        .unwrap();
        assert_eq!(
            selected_ids(&bare),
            (1..=7)
                .map(|index| format!("change:{index:03}"))
                .collect::<Vec<_>>()
        );
        assert!(
            bare.window.is_none(),
            "bare output has no page capabilities"
        );

        let first = select_bodyless_change_page(
            ChangePageLens::Changes,
            &rows,
            PAGE_TEST_STAMP,
            &bounded_request(2, None, None, None, None, None),
        )
        .unwrap();
        assert_eq!(selected_ids(&first), ["change:001", "change:002"]);
        assert_eq!(
            window_shapes(&first),
            (
                None,
                Some(Some("change:002".to_owned())),
                Some(Some("change:006".to_owned()))
            )
        );

        let middle = select_bodyless_change_page(
            ChangePageLens::Changes,
            &rows,
            PAGE_TEST_STAMP,
            &bounded_request(
                2,
                Some(DerivedChangePageBoundaryV1::after(ChangeId::new(
                    "change:002",
                ))),
                None,
                None,
                None,
                None,
            ),
        )
        .unwrap();
        assert_eq!(selected_ids(&middle), ["change:003", "change:004"]);
        assert_eq!(
            window_shapes(&middle),
            (
                Some(None),
                Some(Some("change:004".to_owned())),
                Some(Some("change:006".to_owned()))
            )
        );

        let previous_boundary = middle
            .window
            .as_ref()
            .unwrap()
            .previous
            .clone()
            .expect("middle page previous boundary");
        let previous = select_bodyless_change_page(
            ChangePageLens::Changes,
            &rows,
            PAGE_TEST_STAMP,
            &bounded_request(2, Some(previous_boundary), None, None, None, None),
        )
        .unwrap();
        assert_eq!(selected_ids(&previous), ["change:001", "change:002"]);

        let next_boundary = first
            .window
            .as_ref()
            .unwrap()
            .next
            .clone()
            .expect("first page next boundary");
        let next = select_bodyless_change_page(
            ChangePageLens::Changes,
            &rows,
            PAGE_TEST_STAMP,
            &bounded_request(2, Some(next_boundary), None, None, None, None),
        )
        .unwrap();
        assert_eq!(selected_ids(&next), ["change:003", "change:004"]);

        let last_boundary = first
            .window
            .as_ref()
            .unwrap()
            .last
            .clone()
            .expect("first page last boundary");
        let last = select_bodyless_change_page(
            ChangePageLens::Changes,
            &rows,
            PAGE_TEST_STAMP,
            &bounded_request(2, Some(last_boundary), None, None, None, None),
        )
        .unwrap();
        assert_eq!(selected_ids(&last), ["change:007"]);
        assert_eq!(
            window_shapes(&last),
            (Some(Some("change:004".to_owned())), None, None)
        );

        let absent_boundary = select_bodyless_change_page(
            ChangePageLens::Changes,
            &rows,
            PAGE_TEST_STAMP,
            &bounded_request(
                2,
                Some(DerivedChangePageBoundaryV1::after(ChangeId::new(
                    "change:003x",
                ))),
                None,
                None,
                None,
                None,
            ),
        )
        .unwrap();
        assert_eq!(selected_ids(&absent_boundary), ["change:004", "change:005"]);
        assert_eq!(
            window_shapes(&absent_boundary),
            (
                Some(Some("change:001".to_owned())),
                Some(Some("change:005".to_owned())),
                Some(Some("change:006".to_owned()))
            )
        );

        let beyond_tail = select_bodyless_change_page(
            ChangePageLens::Changes,
            &rows,
            PAGE_TEST_STAMP,
            &bounded_request(
                2,
                Some(DerivedChangePageBoundaryV1::after(ChangeId::new(
                    "change:999",
                ))),
                None,
                None,
                None,
                None,
            ),
        )
        .unwrap();
        assert!(selected_ids(&beyond_tail).is_empty());
        assert_eq!(
            window_shapes(&beyond_tail),
            (
                Some(Some("change:005".to_owned())),
                None,
                Some(Some("change:006".to_owned()))
            )
        );

        let empty = select_bodyless_change_page(
            ChangePageLens::Changes,
            &[],
            PAGE_TEST_STAMP,
            &bounded_request(2, None, None, None, None, None),
        )
        .unwrap();
        assert!(selected_ids(&empty).is_empty());
        assert_eq!(window_shapes(&empty), (None, None, None));
    }

    #[test]
    fn ordinary_bodyless_change_pages_freeze_limit_defaults_maximum_and_tail_truncation() {
        for (count, limit, expected_len, next_boundary, last_boundary) in [
            (3, 1, 1, "change:001", "change:002"),
            (
                51,
                DEFAULT_PAGE_LIMIT,
                DEFAULT_PAGE_LIMIT,
                "change:050",
                "change:050",
            ),
            (
                101,
                MAXIMUM_PAGE_LIMIT,
                MAXIMUM_PAGE_LIMIT,
                "change:100",
                "change:100",
            ),
        ] {
            let rows = bodyless_sequence(count);
            let page = select_bodyless_change_page(
                ChangePageLens::Changes,
                &rows,
                PAGE_TEST_STAMP,
                &bounded_request(limit, None, None, None, None, None),
            )
            .unwrap();
            assert_eq!(selected_ids(&page).len(), expected_len);
            assert_eq!(
                window_shapes(&page),
                (
                    None,
                    Some(Some(next_boundary.to_owned())),
                    Some(Some(last_boundary.to_owned()))
                )
            );
        }

        let tail = select_bodyless_change_page(
            ChangePageLens::Changes,
            &bodyless_sequence(7),
            PAGE_TEST_STAMP,
            &bounded_request(
                3,
                Some(DerivedChangePageBoundaryV1::after(ChangeId::new(
                    "change:006",
                ))),
                None,
                None,
                None,
                None,
            ),
        )
        .unwrap();
        assert_eq!(selected_ids(&tail), ["change:007"]);
        assert_eq!(
            window_shapes(&tail),
            (Some(Some("change:003".to_owned())), None, None)
        );
    }

    #[test]
    fn ordinary_bodyless_change_pages_refuse_summary_search_and_stale_continuations() {
        let rows = bodyless_sequence(1);
        let searched = DerivedChangePageRequestV1::Bounded(
            DerivedChangePageSelectionV1::new(
                DEFAULT_PAGE_LIMIT,
                None,
                Some("proposal words".to_owned()),
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        );
        let search_error =
            select_bodyless_change_page(ChangePageLens::Changes, &rows, PAGE_TEST_STAMP, &searched)
                .expect_err("summary search must use the exhaustive proposal path");
        assert!(
            search_error
                .to_string()
                .contains("requires exhaustive proposal selection")
        );

        let stale = DerivedChangePageRequestV1::Bounded(
            DerivedChangePageSelectionV1::new(
                DEFAULT_PAGE_LIMIT,
                Some(
                    DerivedChangePageContinuationV1::new(
                        "sha256:stale",
                        DerivedChangePageBoundaryV1::page_one(),
                    )
                    .unwrap(),
                ),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        );
        let stale_error =
            select_bodyless_change_page(ChangePageLens::Changes, &rows, PAGE_TEST_STAMP, &stale)
                .expect_err("stale continuation cannot select against another projection");
        assert!(
            stale_error
                .to_string()
                .contains("belongs to a different projection")
        );
    }

    #[test]
    fn ordinary_bodyless_change_filters_freeze_every_value_and_and_combination() {
        let topologies = [
            ChangeTopologyV1::Initial,
            ChangeTopologyV1::Replacement,
            ChangeTopologyV1::ReplacementDivergent,
            ChangeTopologyV1::Consolidation,
            ChangeTopologyV1::ParallelCurrent,
            ChangeTopologyV1::Mixed,
            ChangeTopologyV1::Incomplete,
            ChangeTopologyV1::CycleConflicted,
        ];
        let lifecycles = [
            ChangeLifecycleV1::Incomplete,
            ChangeLifecycleV1::Conflicted,
            ChangeLifecycleV1::InProgress,
            ChangeLifecycleV1::Accepted,
        ];
        let attentions = [
            DerivedChangeAttentionFilterV1::Clear,
            DerivedChangeAttentionFilterV1::InProgress,
            DerivedChangeAttentionFilterV1::Incomplete,
            DerivedChangeAttentionFilterV1::Conflicted,
        ];
        let availabilities = [
            DerivedChangeAvailabilityFilterV1::Available,
            DerivedChangeAvailabilityFilterV1::Incomplete,
        ];
        let rows = topologies
            .iter()
            .enumerate()
            .map(|(index, topology)| {
                bodyless_summary(
                    format!("change:{:03}", index + 1),
                    *topology,
                    lifecycles[index % lifecycles.len()],
                    attentions[index % attentions.len()],
                    availabilities[index % availabilities.len()],
                )
            })
            .collect::<Vec<_>>();

        let topology_filters = std::iter::once(None)
            .chain(topologies.into_iter().map(Some))
            .collect::<Vec<_>>();
        let lifecycle_filters = std::iter::once(None)
            .chain(lifecycles.into_iter().map(Some))
            .collect::<Vec<_>>();
        let attention_filters = std::iter::once(None)
            .chain(attentions.into_iter().map(Some))
            .collect::<Vec<_>>();
        let availability_filters = std::iter::once(None)
            .chain(availabilities.into_iter().map(Some))
            .collect::<Vec<_>>();

        for topology in &topology_filters {
            for lifecycle in &lifecycle_filters {
                for attention in &attention_filters {
                    for availability in &availability_filters {
                        let page = select_bodyless_change_page(
                            ChangePageLens::Changes,
                            &rows,
                            PAGE_TEST_STAMP,
                            &bounded_request(
                                MAXIMUM_PAGE_LIMIT,
                                None,
                                *topology,
                                *lifecycle,
                                *attention,
                                *availability,
                            ),
                        )
                        .unwrap();
                        let expected = rows
                            .iter()
                            .filter(|row| topology.is_none_or(|value| row.topology == value))
                            .filter(|row| lifecycle.is_none_or(|value| row.lifecycle == value))
                            .filter(|row| {
                                attention.is_none_or(|value| {
                                    row.attention_summary == attention_filter_name(value)
                                })
                            })
                            .filter(|row| {
                                availability.is_none_or(|value| {
                                    row.availability_summary == availability_filter_name(value)
                                })
                            })
                            .map(|row| row.change_id.as_str().to_owned())
                            .collect::<Vec<_>>();
                        assert_eq!(
                            selected_ids(&page),
                            expected,
                            "topology={topology:?} lifecycle={lifecycle:?} attention={attention:?} availability={availability:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn ordinary_attention_prefilters_accepted_changes_before_filter_and_window_selection() {
        let rows = vec![
            bodyless_summary(
                "change:001",
                ChangeTopologyV1::Initial,
                ChangeLifecycleV1::Accepted,
                DerivedChangeAttentionFilterV1::Clear,
                DerivedChangeAvailabilityFilterV1::Available,
            ),
            bodyless_summary(
                "change:002",
                ChangeTopologyV1::ParallelCurrent,
                ChangeLifecycleV1::InProgress,
                DerivedChangeAttentionFilterV1::InProgress,
                DerivedChangeAvailabilityFilterV1::Available,
            ),
            bodyless_summary(
                "change:003",
                ChangeTopologyV1::Replacement,
                ChangeLifecycleV1::Accepted,
                DerivedChangeAttentionFilterV1::Clear,
                DerivedChangeAvailabilityFilterV1::Available,
            ),
            bodyless_summary(
                "change:004",
                ChangeTopologyV1::Incomplete,
                ChangeLifecycleV1::Incomplete,
                DerivedChangeAttentionFilterV1::Incomplete,
                DerivedChangeAvailabilityFilterV1::Incomplete,
            ),
            bodyless_summary(
                "change:005",
                ChangeTopologyV1::CycleConflicted,
                ChangeLifecycleV1::Conflicted,
                DerivedChangeAttentionFilterV1::Conflicted,
                DerivedChangeAvailabilityFilterV1::Incomplete,
            ),
        ];

        let bare = select_bodyless_change_page(
            ChangePageLens::Attention,
            &rows,
            PAGE_TEST_STAMP,
            &DerivedChangePageRequestV1::Bare,
        )
        .unwrap();
        assert_eq!(
            selected_ids(&bare),
            ["change:002", "change:004", "change:005"]
        );
        assert!(bare.window.is_none());

        let first = select_bodyless_change_page(
            ChangePageLens::Attention,
            &rows,
            PAGE_TEST_STAMP,
            &bounded_request(2, None, None, None, None, None),
        )
        .unwrap();
        assert_eq!(selected_ids(&first), ["change:002", "change:004"]);
        assert_eq!(
            window_shapes(&first),
            (
                None,
                Some(Some("change:004".to_owned())),
                Some(Some("change:004".to_owned()))
            )
        );

        let next = select_bodyless_change_page(
            ChangePageLens::Attention,
            &rows,
            PAGE_TEST_STAMP,
            &bounded_request(
                2,
                Some(DerivedChangePageBoundaryV1::after(ChangeId::new(
                    "change:004",
                ))),
                None,
                None,
                None,
                None,
            ),
        )
        .unwrap();
        assert_eq!(selected_ids(&next), ["change:005"]);

        let accepted_filter = select_bodyless_change_page(
            ChangePageLens::Attention,
            &rows,
            PAGE_TEST_STAMP,
            &bounded_request(
                MAXIMUM_PAGE_LIMIT,
                None,
                None,
                Some(ChangeLifecycleV1::Accepted),
                None,
                None,
            ),
        )
        .unwrap();
        assert!(
            selected_ids(&accepted_filter).is_empty(),
            "Attention excludes accepted Changes before applying lifecycle filters"
        );
    }

    #[test]
    fn derived_change_endpoints_hydrate_bare_and_bounded_presentations() {
        let fixture = ActiveChangeFixture::new(&[
            &[Some("first exact state"), Some("first exact state")],
            &[Some("second exact state"), Some("second exact state")],
            &[Some("third exact state"), Some("third exact state")],
        ]);

        let DerivedChangeOutcomeV1::Ready(bare) = fixture
            .access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("read bare Changes")
        else {
            panic!("bare Changes must be ready");
        };
        assert_eq!(bare.document.document.changes.len(), 3);
        assert_eq!(bare.document.presentations.len(), 3);
        assert!(bare.window.is_none());
        let events = fixture.store.list_events().expect("read strict event set");
        let strict_semantic =
            crate::session::project_changes(&events).expect("strict Change replay");
        let strict_documents =
            crate::session::project_change_documents(&events).expect("strict document replay");
        let presentations = change_presentation_projection(
            &strict_semantic,
            &strict_documents,
            &events,
            &event_set_hash_for_events(&events).expect("strict event-set hash"),
        )
        .expect("strict presentation replay");
        let strict = ChangeDocumentFacadeV1::new(strict_semantic, strict_documents)
            .expect("strict Change facade")
            .with_presentations(presentations)
            .expect("bind strict presentations");
        let mut expected_bare = strict
            .list_document_for_inspector_with_presentations()
            .expect("strict bare Changes");
        normalize_list_projection_stamp(
            &mut expected_bare,
            &bare.document.document.projection_stamp,
        );
        assert_eq!(bare.document, expected_bare);

        let RuntimeCurrentRead::Ready(current) = fixture
            .runtime
            .current()
            .expect("read current fixture generation")
        else {
            panic!("fixture generation must remain current");
        };
        let checkpoint = current
            .pin_change_reader_checkpoint()
            .expect("pin current fixture checkpoint");
        let LocatorRead::Ready(materialized) = current
            .service()
            .semantic_materialized_change_projection_at(checkpoint.truth_cursor)
            .expect("read current fixture projection")
        else {
            panic!("fixture projection must be caught up");
        };
        assert_eq!(
            current
                .change_generation_stamp(
                    &checkpoint,
                    &materialized.projection,
                    &materialized.document_projection,
                )
                .expect("mint current generation stamp"),
            bare.document.document.projection_stamp
        );

        let DerivedChangeOutcomeV1::Ready(page) = fixture
            .access
            .changes(&bounded_request(1, None, None, None, None, None))
            .expect("read bounded Changes")
        else {
            panic!("bounded Changes must be ready");
        };
        assert_eq!(page.document.document.changes.len(), 1);
        assert_eq!(page.document.presentations.len(), 1);
        assert!(page.window.is_some());
        let selected_change = &page.document.document.changes[0].change_id;
        assert!(page.document.presentations.contains_key(selected_change));
        let mut expected_page = expected_bare.clone();
        expected_page
            .document
            .changes
            .retain(|change| &change.change_id == selected_change);
        expected_page
            .presentations
            .retain(|change_id, _| change_id == selected_change);
        normalize_list_projection_stamp(
            &mut expected_page,
            &page.document.document.projection_stamp,
        );
        assert_eq!(page.document, expected_page);

        let DerivedChangeOutcomeV1::Ready(attention) = fixture
            .access
            .attention(&bounded_request(1, None, None, None, None, None))
            .expect("read bounded Attention")
        else {
            panic!("bounded Attention must be ready");
        };
        assert_eq!(attention.document.document.changes.len(), 1);
        assert_eq!(attention.document.presentations.len(), 1);
        assert_eq!(
            attention.document.document.changes[0],
            page.document.document.changes[0]
        );
        assert_eq!(
            attention.document.presentations,
            page.document.presentations
        );
        let mut expected_attention = strict
            .attention_document_with_presentations(true)
            .expect("strict Attention");
        expected_attention
            .document
            .changes
            .retain(|change| &change.change_id == selected_change);
        expected_attention
            .presentations
            .retain(|change_id, _| change_id == selected_change);
        normalize_attention_projection_stamp(
            &mut expected_attention,
            &attention.document.document.projection_stamp,
        );
        assert_eq!(attention.document, expected_attention);
        let expected_attention_presentation = attention_presentation_for_change(
            &strict
                .detail_document(selected_change)
                .expect("strict selected Attention detail")
                .detail,
        )
        .expect("strict selected Attention explanation");
        assert_eq!(
            attention.attention_presentations,
            BTreeMap::from([(selected_change.clone(), expected_attention_presentation)])
        );
    }

    #[test]
    fn strict_stamp_binder_matches_the_bodyless_generation_stamp() {
        let fixture =
            ActiveChangeFixture::new(&[&[Some("shared stamp state"), Some("shared stamp state")]]);
        let DerivedChangeOutcomeV1::Ready(page) = fixture
            .access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("warm the cached Change generation")
        else {
            panic!("fixture Changes must be ready");
        };
        let events = fixture.store.list_events().expect("read strict events");
        let strict_projection = crate::session::project_changes(&events).expect("project Changes");
        let strict_documents =
            crate::session::project_change_documents(&events).expect("project Change documents");
        let authority = inspect_journal_records(
            StoreBackend::Local(fixture._temp.path().to_path_buf())
                .journal()
                .as_ref(),
        )
        .expect("inspect strict authority")
        .cursor;

        assert_eq!(
            fixture
                .access
                .strict_stamp_binder()
                .bind(&authority, &strict_projection, &strict_documents)
                .expect("bind strict projection stamp"),
            StrictChangeStampBindingV1::Bound(page.document.document.projection_stamp)
        );

        let mut invalid_strict_documents = strict_documents;
        invalid_strict_documents.projection_stamp = "not-a-projection-hash".to_owned();
        assert!(
            fixture
                .access
                .strict_stamp_binder()
                .bind(&authority, &strict_projection, &invalid_strict_documents,)
                .is_err(),
            "invalid strict projection input must not be mistaken for cached metadata absence"
        );
    }

    #[test]
    fn review_generation_matches_the_page_stamp_and_full_projections() {
        let fixture = ActiveChangeFixture::new(&[
            &[
                Some("first generation state"),
                Some("first generation state"),
            ],
            &[
                Some("second generation state"),
                Some("second generation state"),
            ],
        ]);
        let DerivedChangeOutcomeV1::Ready(page) = fixture
            .access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("read the Change page")
        else {
            panic!("the fixture Change page must be ready");
        };
        let DerivedChangeOutcomeV1::Ready(generation) = fixture
            .access
            .review_generation()
            .expect("read the whole Change generation")
        else {
            panic!("the fixture Change generation must be ready");
        };

        let events = fixture.store.list_events().expect("read strict events");
        let strict_projection =
            crate::session::project_changes(&events).expect("project strict Changes");
        let strict_documents = crate::session::project_change_documents(&events)
            .expect("project strict Change documents");
        assert_eq!(generation.projection(), &strict_projection);
        assert_eq!(generation.document_projection(), &strict_documents);
        assert_eq!(
            generation.stamp(),
            page.document.document.projection_stamp,
            "the carrier and staged page must bind the same whole generation"
        );

        let RuntimeCurrentRead::Ready(current) = fixture.runtime.current().unwrap() else {
            panic!("the fixture generation must remain current");
        };
        let checkpoint = current
            .pin_change_reader_checkpoint()
            .expect("pin the current generation");
        let mut invalid_documents = strict_documents;
        invalid_documents.projection_stamp = "not-a-projection-hash".to_owned();
        assert!(
            current
                .change_generation_stamp(&checkpoint, &strict_projection, &invalid_documents)
                .is_err(),
            "invalid projection input must not be mistaken for absent generation metadata"
        );
    }

    #[test]
    fn review_generation_decodes_no_events_and_opens_no_carriers() {
        let fixture = ActiveChangeFixture::new(&[
            &[Some("first counted state"), Some("first counted state")],
            &[Some("second counted state"), Some("second counted state")],
        ]);
        fixture
            .runtime
            .current()
            .expect("warm the current generation before counting");
        let scope = LongitudinalCountingScopeV1::new("a".repeat(64)).unwrap();
        let guard = scope.enter();
        let outcome = fixture
            .access
            .review_generation()
            .expect("read the counted Change generation");
        drop(guard);
        assert!(matches!(outcome, DerivedChangeOutcomeV1::Ready(_)));

        let counters = scope.snapshot().counters;
        assert_eq!(counters.event_decodes, 0);
        assert_eq!(counters.carrier_opens, 0);
        assert_eq!(counters.change_proposal_carriers_opened, 0);
        assert_eq!(counters.change_support_carriers_opened, 0);
        assert_eq!(counters.body_artifact_reads, 0);
        assert_eq!(counters.object_artifact_reads, 0);
    }

    #[test]
    fn review_generation_maps_lifecycle_failures_to_outcomes() {
        let inactive = DerivedChangeAccess::from_runtime(DerivedAccessRuntime::from_mode(
            DerivedAccessMode::Off,
        ));
        let inactive_page = inactive
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("classify the inactive page read");
        let inactive_generation = inactive
            .review_generation()
            .expect("classify the inactive generation read");
        assert_ne!(derived_change_outcome_class(&inactive_page), "ready");
        assert_eq!(
            derived_change_outcome_class(&inactive_generation),
            derived_change_outcome_class(&inactive_page)
        );

        let temp = TempDir::new().expect("create an unpublished Change root");
        let backend = StoreBackend::Local(temp.path().to_path_buf());
        write_capability_fixture_for_test(
            backend.journal().as_ref(),
            CapabilityFixtureState::EmptyL2,
        )
        .expect("activate the unpublished Change root");
        let store_identity =
            opaque_path_identity("store", temp.path()).expect("derive unpublished store identity");
        let lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            temp.path(),
            store_identity.clone(),
        )
        .expect("create unpublished Change lifecycle");
        let empty = DerivedChangeAccess::from_runtime(DerivedAccessRuntime::from_mode(
            DerivedAccessMode::Active {
                lifecycle,
                current: Mutex::new(None),
                store_identity,
                backend,
            },
        ));
        let empty_page = empty
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("classify the unpublished page read");
        let empty_generation = empty
            .review_generation()
            .expect("classify the unpublished generation read");
        assert_ne!(derived_change_outcome_class(&empty_page), "ready");
        assert_eq!(
            derived_change_outcome_class(&empty_generation),
            derived_change_outcome_class(&empty_page)
        );

        let mismatch = generation_checkpoint_mismatch_outcome(
            super::super::cursor::TruthCursor::new(7, 11),
            super::super::cursor::TruthCursor::new(7, 10),
        )
        .expect("the post-load checkpoint mismatch must map to an outcome");
        let DerivedChangeOutcomeV1::ProjectionUnavailable(document) = mismatch else {
            panic!("a wrong materialized checkpoint must fail the generation read");
        };
        assert_eq!(
            document.code(),
            DerivedProjectionFailureCodeV1::ProjectionInvalid
        );
        assert!(!document.is_retryable());
    }

    #[test]
    fn review_generation_maps_terminal_repin_drift_to_retryable() {
        let fixture = ActiveChangeFixture::new(&[&[
            Some("moving generation state"),
            Some("moving generation state"),
        ]]);
        let mut appended = false;
        let outcome = fixture
            .access
            .review_generation_with_hook(|| {
                fixture.append_unrelated("review-generation-movement");
                appended = true;
            })
            .expect("read across generation movement");
        assert!(appended, "the terminal re-pin hook must run");
        let DerivedChangeOutcomeV1::Retryable(document) = outcome else {
            panic!("terminal checkpoint movement must be retryable");
        };
        assert_eq!(
            document.code(),
            DerivedProjectionFailureCodeV1::ProjectionUnstable
        );
        assert!(document.is_retryable());
    }

    #[test]
    fn review_generation_detail_document_matches_the_page_stamp_and_strict_detail() {
        let fixture = ActiveChangeFixture::new(&[&[
            Some("whole generation detail"),
            Some("whole generation detail"),
        ]]);
        let change_id = fixture.changes[0].change_id.clone();
        let DerivedChangeOutcomeV1::Ready(page) = fixture
            .access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("read the staged Change page")
        else {
            panic!("the staged Change page must be ready");
        };
        let DerivedChangeOutcomeV1::Ready(detail) = fixture
            .access
            .review_generation_detail_document(&change_id)
            .expect("read the whole-generation detail")
        else {
            panic!("the whole-generation detail must be ready");
        };

        let events = fixture.store.list_events().expect("read strict events");
        let strict_projection =
            crate::session::project_changes(&events).expect("project strict Changes");
        let strict_documents = crate::session::project_change_documents(&events)
            .expect("project strict Change documents");
        let expected = ChangeDocumentFacadeV1::new(strict_projection, strict_documents)
            .expect("build strict Change facade")
            .with_generation_stamp(page.document.document.projection_stamp.clone())
            .expect("bind the staged generation stamp")
            .detail_document(&change_id)
            .expect("compose strict Change detail");
        assert_eq!(detail, expected);
        assert_eq!(
            detail.detail.projection_stamp,
            page.document.document.projection_stamp
        );

        let missing = ChangeId::new(format!("change:sha256:{}", "f".repeat(64)));
        let generation_error = fixture
            .access
            .review_generation_detail_document(&missing)
            .expect_err("the whole-generation detail must reject an unknown Change");
        let seek_error = fixture
            .access
            .review_detail_document(&missing)
            .expect_err("the seek detail must reject an unknown Change");
        assert_eq!(generation_error.to_string(), seek_error.to_string());
    }

    #[test]
    fn strict_stamp_binder_leaves_an_empty_runtime_cache_unavailable() {
        let fixture = ActiveChangeFixture::new(&[]);
        let access = fixture.fresh_access();
        let publication_path = fixture.publication_path();
        fs::write(&publication_path, b"{")
            .expect("invalidate publication metadata behind an empty cache");
        let scope = LongitudinalCountingScopeV1::new("8".repeat(64)).unwrap();
        let guard = scope.enter();
        let binding = access
            .strict_stamp_binder()
            .bind(
                &empty_authority_cursor(),
                &crate::session::ChangeProjection::default(),
                &crate::session::ChangeDocumentProjectionV1::default(),
            )
            .expect("inspect an empty cached-current slot");
        drop(guard);

        assert_eq!(binding, StrictChangeStampBindingV1::Unavailable);
        assert!(access.runtime.cached_current().is_none());
        assert_eq!(
            fs::read(&publication_path)
                .expect("empty-cache binding must not move publication state"),
            b"{"
        );
        let counters = scope.snapshot().counters;
        assert_eq!(counters.directory_entries_walked, 0);
        assert_eq!(counters.event_folds, 0);
        assert_eq!(counters.projection_rebuilds, 0);
        assert_eq!(counters.state_rebuilds, 0);
        assert_eq!(counters.full_history_fallbacks, 0);
    }

    #[test]
    fn strict_stamp_binder_never_binds_a_mismatched_or_moving_checkpoint() {
        let fixture =
            ActiveChangeFixture::new(&[&[Some("moving stamp state"), Some("moving stamp state")]]);
        fixture
            .access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("warm the cached Change generation");
        let events = fixture.store.list_events().expect("read strict events");
        let strict_projection = crate::session::project_changes(&events).expect("project Changes");
        let strict_documents =
            crate::session::project_change_documents(&events).expect("project Change documents");
        let authority = inspect_journal_records(
            StoreBackend::Local(fixture._temp.path().to_path_buf())
                .journal()
                .as_ref(),
        )
        .expect("inspect strict authority")
        .cursor;
        let mut mismatched_authority = authority.clone();
        mismatched_authority.event_count += 1;
        assert_eq!(
            fixture
                .access
                .strict_stamp_binder()
                .bind(&mismatched_authority, &strict_projection, &strict_documents,)
                .expect("classify mismatched authority"),
            StrictChangeStampBindingV1::Moving
        );

        assert_eq!(
            fixture
                .access
                .strict_stamp_binder()
                .bind_with_hook(&authority, &strict_projection, &strict_documents, || {
                    fixture.append_unrelated("strict-stamp-movement");
                })
                .expect("classify a moving live checkpoint"),
            StrictChangeStampBindingV1::Moving
        );
    }

    #[test]
    fn strict_stamp_binder_rejects_a_loose_append_while_the_derived_writer_is_busy() {
        let fixture =
            ActiveChangeFixture::new(&[&[Some("busy writer state"), Some("busy writer state")]]);
        fixture
            .access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("warm the cached Change generation");
        let events = fixture.store.list_events().expect("read strict events");
        let strict_projection = crate::session::project_changes(&events).expect("project Changes");
        let strict_documents =
            crate::session::project_change_documents(&events).expect("project Change documents");
        let authority = inspect_journal_records(
            StoreBackend::Local(fixture._temp.path().to_path_buf())
                .journal()
                .as_ref(),
        )
        .expect("inspect strict authority")
        .cursor;
        let _writer_lock =
            StoreWriterLock::acquire(fixture._temp.path()).expect("hold the derived writer lock");

        let binding = fixture
            .access
            .strict_stamp_binder()
            .bind_with_hook(&authority, &strict_projection, &strict_documents, || {
                fixture.append_unrelated("busy-writer-strict-stamp-movement");
            })
            .expect("classify a loose append during stamp binding");

        assert_eq!(
            binding,
            StrictChangeStampBindingV1::Moving,
            "a loose authoritative append must invalidate the cached strict stamp"
        );
        assert_eq!(
            fixture.store.take_write_diagnostics()[0].code,
            "derived_access_generation_unavailable"
        );
    }

    #[test]
    fn strict_stamp_binder_rejects_a_cached_generation_after_publication_advances() {
        let fixture = ActiveChangeFixture::new(&[&[
            Some("publication stamp state"),
            Some("publication stamp state"),
        ]]);
        let DerivedChangeOutcomeV1::Ready(old_page) = fixture
            .access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("warm the original cached Change generation")
        else {
            panic!("original fixture Changes must be ready");
        };
        let old_generation_id = fixture
            .runtime
            .cached_current()
            .expect("original generation is cached")
            .generation_id()
            .to_owned();
        let events = fixture.store.list_events().expect("read strict events");
        let strict_projection = crate::session::project_changes(&events).expect("project Changes");
        let strict_documents =
            crate::session::project_change_documents(&events).expect("project Change documents");
        let authority = inspect_journal_records(
            StoreBackend::Local(fixture._temp.path().to_path_buf())
                .journal()
                .as_ref(),
        )
        .expect("inspect strict authority")
        .cursor;

        assert_eq!(
            fixture
                .access
                .strict_stamp_binder()
                .bind_with_hook(&authority, &strict_projection, &strict_documents, || {
                    fixture
                        .lifecycle
                        .rebuild(|_| LifecycleControl::Continue)
                        .expect("publish a replacement generation during stamp binding");
                })
                .expect("classify publication movement during binding"),
            StrictChangeStampBindingV1::Moving,
            "one response must not mix N with a live pointer that advances to N+1"
        );
        let replacement_generation_id = fixture
            .lifecycle
            .published_generation_id()
            .expect("read replacement publication")
            .expect("replacement generation is current");
        assert_ne!(replacement_generation_id, old_generation_id);
        assert_eq!(
            fixture
                .access
                .strict_stamp_binder()
                .bind(&authority, &strict_projection, &strict_documents)
                .expect("classify the retained cached generation"),
            StrictChangeStampBindingV1::Moving,
            "a retained reader for N must not bind after the live pointer publishes N+1"
        );

        let replacement_access = fixture.fresh_access();
        let DerivedChangeOutcomeV1::Ready(replacement_page) = replacement_access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("read through a fresh replacement-generation cache")
        else {
            panic!("replacement fixture Changes must be ready");
        };
        assert_ne!(
            replacement_page.document.document.projection_stamp,
            old_page.document.document.projection_stamp
        );
        assert_eq!(
            replacement_access
                .strict_stamp_binder()
                .bind(&authority, &strict_projection, &strict_documents)
                .expect("bind the replacement generation"),
            StrictChangeStampBindingV1::Bound(replacement_page.document.document.projection_stamp)
        );
    }

    #[test]
    fn strict_stamp_binder_observes_the_refreshed_publication_namespace() {
        let fixture = ActiveChangeFixture::new(&[&[
            Some("transitioned publication state"),
            Some("transitioned publication state"),
        ]]);
        let stable = DerivedStorageLayout::for_namespace(
            fixture._temp.path(),
            DerivedStorageNamespace::Stable,
        );
        let legacy = DerivedStorageLayout::for_namespace(
            fixture._temp.path(),
            DerivedStorageNamespace::Legacy,
        );
        fs::rename(stable.root(), legacy.root()).expect("move fixture into the legacy namespace");
        let store_identity = opaque_path_identity("store", fixture._temp.path())
            .expect("derive transitioned fixture identity");
        let configured_lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            fixture._temp.path(),
            store_identity.clone(),
        )
        .expect("configure runtime while the legacy namespace is selected");
        let runtime = DerivedAccessRuntime::new(
            DerivedAccessMode::Active {
                lifecycle: configured_lifecycle,
                current: Mutex::new(None),
                store_identity: store_identity.clone(),
                backend: StoreBackend::Local(fixture._temp.path().to_path_buf()),
            },
            Some(DerivedAccessMaintenance {
                profile: DerivedAccessProfile::SqliteWalBodylessV1,
                store_root: fixture._temp.path().to_path_buf(),
                store_identity,
            }),
        );
        let transition = DerivedStorageLayout::transition_legacy(fixture._temp.path())
            .expect("transition the publication into the stable namespace");
        assert_eq!(transition.disposition, DerivedStorageTransition::Moved);
        let access = DerivedChangeAccess::from_runtime(runtime);
        let DerivedChangeOutcomeV1::Ready(page) = access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("open the refreshed stable publication")
        else {
            panic!("refreshed stable publication must be ready");
        };
        let events = fixture.store.list_events().expect("read strict events");
        let strict_projection = crate::session::project_changes(&events).expect("project Changes");
        let strict_documents =
            crate::session::project_change_documents(&events).expect("project Change documents");
        let authority = inspect_journal_records(
            StoreBackend::Local(fixture._temp.path().to_path_buf())
                .journal()
                .as_ref(),
        )
        .expect("inspect strict authority")
        .cursor;

        assert_eq!(
            access
                .strict_stamp_binder()
                .bind(&authority, &strict_projection, &strict_documents)
                .expect("bind through the refreshed publication namespace"),
            StrictChangeStampBindingV1::Bound(page.document.document.projection_stamp)
        );
    }

    #[test]
    fn strict_stamp_binder_treats_invalid_cached_metadata_as_unavailable() {
        let fixture =
            ActiveChangeFixture::new(&[&[Some("cached stamp state"), Some("cached stamp state")]]);
        fixture
            .access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("warm the cached Change generation");
        let events = fixture.store.list_events().expect("read strict events");
        let strict_projection = crate::session::project_changes(&events).expect("project Changes");
        let strict_documents =
            crate::session::project_change_documents(&events).expect("project Change documents");
        let authority = inspect_journal_records(
            StoreBackend::Local(fixture._temp.path().to_path_buf())
                .journal()
                .as_ref(),
        )
        .expect("inspect strict authority")
        .cursor;
        fixture.mutate_database(|connection| {
            connection
                .execute(
                    "UPDATE reader_projection_checkpoint
                     SET checkpoint_json = '{'
                     WHERE singleton = 1",
                    [],
                )
                .expect("invalidate the disposable cached checkpoint");
        });

        assert_eq!(
            fixture
                .access
                .strict_stamp_binder()
                .bind(&authority, &strict_projection, &strict_documents)
                .expect("classify invalid cached metadata"),
            StrictChangeStampBindingV1::Unavailable
        );
    }

    #[test]
    fn strict_stamp_binder_treats_invalid_publication_metadata_as_unavailable() {
        let fixture = ActiveChangeFixture::new(&[&[
            Some("publication metadata state"),
            Some("publication metadata state"),
        ]]);
        fixture
            .access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("warm the cached Change generation");
        let events = fixture.store.list_events().expect("read strict events");
        let strict_projection = crate::session::project_changes(&events).expect("project Changes");
        let strict_documents =
            crate::session::project_change_documents(&events).expect("project Change documents");
        let authority = inspect_journal_records(
            StoreBackend::Local(fixture._temp.path().to_path_buf())
                .journal()
                .as_ref(),
        )
        .expect("inspect strict authority")
        .cursor;
        let publication_path = fixture.publication_path();
        fs::write(&publication_path, b"{").expect("invalidate disposable publication metadata");

        assert_eq!(
            fixture
                .access
                .strict_stamp_binder()
                .bind(&authority, &strict_projection, &strict_documents)
                .expect("classify invalid publication metadata"),
            StrictChangeStampBindingV1::Unavailable
        );
        assert!(
            fixture.runtime.cached_current().is_some(),
            "metadata observation must not mutate the cached reader slot"
        );
        assert_eq!(
            fs::read(&publication_path)
                .expect("publication observation must not move invalid metadata"),
            b"{"
        );
    }

    #[test]
    fn strict_stamp_binder_source_has_no_query_hydration_or_activation_path() {
        let source = include_str!("changes.rs");
        let start = source
            .find("fn bind_strict_change_stamp_with_hook")
            .expect("binder implementation marker");
        let end = source[start..]
            .find("\n}\n\n/// Independent authority")
            .map(|offset| start + offset)
            .expect("binder implementation terminator");
        let implementation = &source[start..end];
        assert!(
            implementation
                .find("runtime.cached_current()")
                .expect("cached reader observation")
                < implementation
                    .find("runtime.current_publication_identity()")
                    .expect("initial publication observation"),
            "an empty cached-reader slot must return before publication metadata I/O"
        );
        for forbidden in [
            ".current(",
            "open_current",
            "semantic_materialized",
            "read_page",
            "LocatorRead",
            "hydrate",
            "query",
            "rebuild",
            "maintenance",
        ] {
            assert!(
                !implementation.contains(forbidden),
                "strict stamp binding must not contain {forbidden:?}"
            );
        }

        let runtime_source = include_str!("runtime.rs");
        let runtime_start = runtime_source
            .find("pub(super) fn current_publication_identity")
            .expect("runtime publication observation marker");
        let runtime_end = runtime_source[runtime_start..]
            .find("\n    /// Clone the process-local reader")
            .map(|offset| runtime_start + offset)
            .expect("runtime publication observation terminator");
        let runtime_observation = &runtime_source[runtime_start..runtime_end];
        for forbidden in [
            ".current(",
            "open_current",
            "rebuild",
            "maintenance(",
            "maintain_current_generation",
            "start_background",
            "semantic",
            "hydrate",
            "query",
            "lock(current)",
        ] {
            assert!(
                !runtime_observation.contains(forbidden),
                "publication observation must not contain {forbidden:?}"
            );
        }

        let lifecycle_source = include_str!("lifecycle.rs");
        let lifecycle_start = lifecycle_source
            .find("pub(crate) fn published_generation_identity_read_only")
            .expect("lifecycle publication observation marker");
        let lifecycle_end = lifecycle_source[lifecycle_start..]
            .find("\n    /// Metadata-only activation probe")
            .map(|offset| lifecycle_start + offset)
            .expect("lifecycle publication observation terminator");
        let lifecycle_observation = &lifecycle_source[lifecycle_start..lifecycle_end];
        for forbidden in [
            "open_current",
            "rebuild",
            "maintain",
            "semantic",
            "hydrate",
            "query",
            "EventStore",
            "StoreBackend",
            "generation_open_error",
            "quarantine",
            "try_acquire",
            "rename",
            "remove",
            "write",
        ] {
            assert!(
                !lifecycle_observation.contains(forbidden),
                "publication metadata read must not contain {forbidden:?}"
            );
        }
    }

    #[test]
    fn derived_change_carrier_work_is_page_proportional_and_hydrates_required_support() {
        let fixture = ActiveChangeFixture::new(&[
            &[Some("first state"), Some("first state")],
            &[Some("second state"), Some("second state")],
            &[Some("third state"), Some("third state")],
        ]);
        let selected = fixture
            .changes
            .iter()
            .min_by(|left, right| left.change_id.cmp(&right.change_id))
            .expect("fixture has a selected Change")
            .clone();
        let (_removal, _signature) = fixture.append_removal_support(&selected.revision);

        fixture
            .runtime
            .current()
            .expect("warm the current generation before counting");
        let bounded_scope = LongitudinalCountingScopeV1::new("1".repeat(64)).unwrap();
        let bounded_guard = bounded_scope.enter();
        let bounded = fixture
            .access
            .changes(&bounded_request(1, None, None, None, None, None))
            .expect("read selected page with support");
        drop(bounded_guard);
        let DerivedChangeOutcomeV1::Ready(bounded) = bounded else {
            panic!("selected page with required support must be ready");
        };
        assert_eq!(
            bounded.document.document.changes[0].change_id,
            selected.change_id
        );
        let bounded_snapshot = bounded_scope.snapshot();
        let bounded_counters = &bounded_snapshot.counters;
        assert_eq!(
            bounded_counters.carrier_opens, 4,
            "two equal selected proposals plus removal and detached signature"
        );
        assert_eq!(bounded_counters.event_decodes, 4);
        assert_eq!(bounded_counters.directory_entries_walked, 0);
        assert_eq!(bounded_counters.event_folds, 0);
        assert_eq!(bounded_counters.projection_rebuilds, 0);
        assert_eq!(bounded_counters.state_rebuilds, 0);
        assert_eq!(bounded_counters.authoritative_fallbacks, 0);
        assert_eq!(bounded_counters.full_history_fallbacks, 0);
        assert_eq!(bounded_counters.change_candidates, 3);
        assert_eq!(bounded_counters.change_candidate_current_revisions, 3);
        assert_eq!(bounded_counters.change_proposal_carriers_opened, 2);
        assert_eq!(bounded_counters.change_proposal_carriers_validated, 2);
        assert_eq!(bounded_counters.change_support_carriers_opened, 2);
        assert_eq!(bounded_counters.change_matches, 0);
        assert_eq!(bounded_counters.change_rows_emitted, 1);
        assert_eq!(
            bounded_snapshot
                .derived_access_phases
                .iter()
                .map(|sample| sample.phase)
                .collect::<Vec<_>>(),
            vec![
                Phase::ChangePageSnapshotAcquisition,
                Phase::CheckpointAndWal,
                Phase::ChangePageBodylessSelection,
                Phase::ChangePageProposalLocatorExpansion,
                Phase::CheckpointAndWal,
                Phase::ChangePageCarrierHydrationValidation,
                Phase::CheckpointAndWal,
                Phase::ChangePageSupportExpansion,
                Phase::CheckpointAndWal,
                Phase::CheckpointAndWal,
                Phase::ChangePagePresentationProjection,
            ]
        );

        let bare_scope = LongitudinalCountingScopeV1::new("2".repeat(64)).unwrap();
        let bare_guard = bare_scope.enter();
        let bare = fixture
            .access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("read bare Change page");
        drop(bare_guard);
        assert!(matches!(bare, DerivedChangeOutcomeV1::Ready(_)));
        let bare_counters = bare_scope.snapshot().counters;
        assert_eq!(
            bare_counters.carrier_opens, 8,
            "bare output reopens six proposal carriers and two selected support carriers"
        );
        assert_eq!(bare_counters.directory_entries_walked, 0);
        assert_eq!(bare_counters.event_folds, 0);
        assert_eq!(bare_counters.projection_rebuilds, 0);
        assert_eq!(bare_counters.state_rebuilds, 0);
        assert_eq!(bare_counters.authoritative_fallbacks, 0);
        assert_eq!(bare_counters.full_history_fallbacks, 0);
        assert_eq!(bare_counters.change_candidates, 3);
        assert_eq!(bare_counters.change_candidate_current_revisions, 3);
        assert_eq!(bare_counters.change_proposal_carriers_opened, 6);
        assert_eq!(bare_counters.change_proposal_carriers_validated, 6);
        assert_eq!(bare_counters.change_support_carriers_opened, 2);
        assert_eq!(bare_counters.change_matches, 0);
        assert_eq!(bare_counters.change_rows_emitted, 3);

        let cold_scope = LongitudinalCountingScopeV1::new("3".repeat(64)).unwrap();
        let cold_guard = cold_scope.enter();
        let cold = fixture
            .fresh_access()
            .changes(&bounded_request(1, None, None, None, None, None))
            .expect("read selected page from a cold runtime");
        drop(cold_guard);
        assert!(matches!(cold, DerivedChangeOutcomeV1::Ready(_)));
        let cold_counters = cold_scope.snapshot().counters;
        assert!(
            (4..=6).contains(&cold_counters.carrier_opens),
            "cold carrier work is two capability carriers plus four selected carriers: {cold_counters:?}"
        );
        assert_eq!(cold_counters.directory_entries_walked, 0);
        assert_eq!(cold_counters.event_folds, 0);
        assert_eq!(cold_counters.projection_rebuilds, 0);
        assert_eq!(cold_counters.state_rebuilds, 0);
        assert_eq!(cold_counters.authoritative_fallbacks, 0);
        assert_eq!(cold_counters.full_history_fallbacks, 0);
        assert_eq!(cold_counters.change_candidates, 3);
        assert_eq!(cold_counters.change_candidate_current_revisions, 3);
        assert_eq!(cold_counters.change_proposal_carriers_opened, 2);
        assert_eq!(cold_counters.change_proposal_carriers_validated, 2);
        assert_eq!(cold_counters.change_support_carriers_opened, 2);
        assert_eq!(cold_counters.change_matches, 0);
        assert_eq!(cold_counters.change_rows_emitted, 1);
    }

    #[test]
    fn derived_change_selected_proposal_failures_are_typed_and_fail_closed() {
        let conflicting = ActiveChangeFixture::new(&[&[Some("present"), None]]);
        assert_projection_invalid(
            conflicting
                .access
                .changes(&DerivedChangePageRequestV1::Bare)
                .expect("read conflicting duplicate proposals"),
            "conflicting proposal summaries for exact Revision",
        );

        let empty = ActiveChangeFixture::new(&[&[Some("present"), Some("present")]]);
        let exact = empty.changes[0].revision.clone();
        empty.mutate_database(|connection| {
            connection
                .execute(
                    "DELETE FROM semantic_revision_proposal_carrier
                     WHERE revision_id = ?1 AND object_artifact_content_hash = ?2",
                    params![
                        exact.revision_id.as_str(),
                        exact.object_artifact_content_hash
                    ],
                )
                .expect("remove compact proposal group");
        });
        assert_projection_invalid(
            empty
                .access
                .changes(&DerivedChangePageRequestV1::Bare)
                .expect("read empty compact proposal group"),
            "has no authoritative proposal carrier",
        );

        let absent = ActiveChangeFixture::new(&[&[Some("present"), Some("present")]]);
        let absent_event = absent.changes[0].proposal_events[0].clone();
        let mut removed = false;
        let outcome = absent
            .access
            .read_page(
                ChangePageLens::Changes,
                &DerivedChangePageRequestV1::Bare,
                |boundary| {
                    if boundary == ChangeReadBoundary::ProposalLocatorsSelected {
                        std::fs::remove_file(
                            absent
                                .store
                                .event_path_for_idempotency_key(&absent_event.idempotency_key),
                        )
                        .expect("remove selected authoritative proposal carrier");
                        removed = true;
                    }
                },
            )
            .expect("read selected proposal removed after locator selection");
        assert!(removed);
        assert_projection_invalid(outcome, "is absent");

        let changed_witness = ActiveChangeFixture::new(&[&[Some("present"), Some("present")]]);
        let changed_sequence =
            changed_witness.proposal_sequence(&changed_witness.changes[0].proposal_events[0]);
        changed_witness.mutate_database(|connection| {
            connection
                .execute(
                    "UPDATE cursor_receipt SET validation_witness_hash = zeroblob(32)
                     WHERE sequence = ?1",
                    [changed_sequence],
                )
                .expect("change selected proposal witness");
        });
        assert_projection_invalid(
            changed_witness
                .access
                .changes(&DerivedChangePageRequestV1::Bare)
                .expect("read changed proposal witness"),
            "does not match persisted row",
        );
    }

    #[test]
    fn derived_change_compact_proposal_mismatches_fail_closed() {
        let wrong_family = ActiveChangeFixture::new(&[&[Some("present"), Some("present")]]);
        let family_sequence =
            wrong_family.proposal_sequence(&wrong_family.changes[0].proposal_events[0]);
        wrong_family.mutate_database(|connection| {
            connection
                .execute(
                    "UPDATE locator_event_type SET value = 'review_initialized'
                     WHERE id = (
                         SELECT event_type_id FROM locator_event WHERE sequence = ?1
                     )",
                    [family_sequence],
                )
                .expect("change compact proposal family");
        });
        assert_projection_invalid(
            wrong_family
                .access
                .changes(&DerivedChangePageRequestV1::Bare)
                .expect("read wrong-family proposal locator"),
            "does not match its indexed exact Revision",
        );

        let wrong_binding = ActiveChangeFixture::new(&[
            &[Some("first"), Some("first")],
            &[Some("second"), Some("second")],
        ]);
        let moved_sequence =
            wrong_binding.proposal_sequence(&wrong_binding.changes[0].proposal_events[0]);
        let other = wrong_binding.changes[1].revision.clone();
        wrong_binding.mutate_database(|connection| {
            connection
                .execute(
                    "UPDATE semantic_revision_proposal_carrier
                     SET revision_id = ?1, object_artifact_content_hash = ?2
                     WHERE sequence = ?3",
                    params![
                        other.revision_id.as_str(),
                        other.object_artifact_content_hash,
                        moved_sequence
                    ],
                )
                .expect("change compact exact Revision binding");
        });
        assert_projection_invalid(
            wrong_binding
                .access
                .changes(&DerivedChangePageRequestV1::Bare)
                .expect("read wrong exact Revision binding"),
            "wrong exact Revision binding",
        );
    }

    #[test]
    fn derived_change_reads_retry_when_the_checkpoint_moves_mid_read() {
        for (index, boundary) in [
            ChangeReadBoundary::BodylessSelectionComplete,
            ChangeReadBoundary::ProposalLocatorsSelected,
            ChangeReadBoundary::ProposalHydrationComplete,
            ChangeReadBoundary::ResponseConstructed,
        ]
        .into_iter()
        .enumerate()
        {
            let fixture =
                ActiveChangeFixture::new(&[&[Some("checkpoint state"), Some("checkpoint state")]]);
            let scope =
                LongitudinalCountingScopeV1::new(format!("{value:064x}", value = index + 10))
                    .expect("count checkpoint movement");
            let guard = scope.enter();
            let mut appended = false;
            let outcome = fixture
                .access
                .read_page(
                    ChangePageLens::Changes,
                    &DerivedChangePageRequestV1::Bare,
                    |observed| {
                        if observed == boundary {
                            fixture.append_unrelated(&format!("movement-{index}"));
                            appended = true;
                        }
                    },
                )
                .expect("read across same-generation checkpoint movement");
            drop(guard);
            assert!(appended, "requested read boundary must be observed");
            let DerivedChangeOutcomeV1::Retryable(document) = outcome else {
                panic!("checkpoint movement must not return an old or mixed Ready page");
            };
            assert_eq!(
                document.code(),
                DerivedProjectionFailureCodeV1::ProjectionUnstable
            );
            assert!(document.is_retryable());
            assert_eq!(scope.snapshot().counters.change_rows_emitted, 0);
        }
    }

    #[test]
    fn derived_change_reads_retry_when_publication_moves_mid_read() {
        let fixture =
            ActiveChangeFixture::new(&[&[Some("published state"), Some("published state")]]);
        let before = fixture
            .lifecycle
            .published_generation_id()
            .expect("read initial publication")
            .expect("fixture generation is published");
        let mut rebuilt = false;
        let outcome = fixture
            .access
            .read_page(
                ChangePageLens::Changes,
                &DerivedChangePageRequestV1::Bare,
                |boundary| {
                    if boundary == ChangeReadBoundary::BodylessSelectionComplete {
                        fixture
                            .lifecycle
                            .rebuild(|_| LifecycleControl::Continue)
                            .expect("publish replacement generation");
                        rebuilt = true;
                    }
                },
            )
            .expect("read across generation publication");
        assert!(rebuilt);
        assert_ne!(
            fixture
                .lifecycle
                .published_generation_id()
                .expect("read replacement publication")
                .expect("replacement generation is published"),
            before
        );
        let DerivedChangeOutcomeV1::Retryable(document) = outcome else {
            panic!("generation movement must not return an old or mixed Ready page");
        };
        assert_eq!(
            document.code(),
            DerivedProjectionFailureCodeV1::ProjectionUnstable
        );
        assert!(document.is_retryable());
    }

    #[test]
    fn derived_change_summary_search_is_exhaustive_before_pagination_and_measured() {
        let fixture = ActiveChangeFixture::new(&[
            &[
                Some("first searchable state"),
                Some("first searchable state"),
            ],
            &[
                Some("second searchable state"),
                Some("second searchable state"),
            ],
            &[
                Some("ÄPFEL searchable state"),
                Some("ÄPFEL searchable state"),
            ],
        ]);
        let target = fixture
            .changes
            .iter()
            .max_by(|left, right| left.change_id.cmp(&right.change_id))
            .expect("fixture has a last Change")
            .clone();
        let target_summary = fixture_proposal_summary(&target);
        let unselected = fixture
            .changes
            .iter()
            .find(|change| change.change_id != target.change_id)
            .expect("fixture has an unselected Change");
        fixture.append_removal_support(&unselected.revision);
        fixture.append_removal_support(&target.revision);
        fixture
            .runtime
            .current()
            .expect("warm the current generation before counting");

        let scope = LongitudinalCountingScopeV1::new("4".repeat(64)).unwrap();
        let guard = scope.enter();
        let outcome = fixture
            .access
            .changes(&bounded_search_request(&target_summary, 1, None))
            .expect("search derived Changes");
        drop(guard);

        let DerivedChangeOutcomeV1::Ready(page) = outcome else {
            panic!("exhaustive derived Change search must be ready: {outcome:?}");
        };
        assert_eq!(page.document.document.changes.len(), 1);
        assert_eq!(
            page.document.document.changes[0].change_id,
            target.change_id
        );
        assert_eq!(page.document.presentations.len(), 1);
        assert!(page.window.is_some());

        let snapshot = scope.snapshot();
        let counters = &snapshot.counters;
        assert_eq!(counters.change_candidates, 3);
        assert_eq!(counters.change_candidate_current_revisions, 3);
        assert_eq!(counters.change_proposal_carriers_opened, 6);
        assert_eq!(counters.change_proposal_carriers_validated, 6);
        assert_eq!(counters.change_support_carriers_opened, 2);
        assert_eq!(counters.change_matches, 1);
        assert_eq!(counters.change_rows_emitted, 1);
        assert_eq!(counters.carrier_opens, 8);
        assert_eq!(counters.full_history_fallbacks, 0);
        let phases = &snapshot.derived_access_phases;
        assert_eq!(
            phases.iter().map(|sample| sample.phase).collect::<Vec<_>>(),
            vec![
                Phase::ChangePageSnapshotAcquisition,
                Phase::CheckpointAndWal,
                Phase::ChangePageBodylessSelection,
                Phase::ChangePageProposalLocatorExpansion,
                Phase::CheckpointAndWal,
                Phase::ChangePageCarrierHydrationValidation,
                Phase::CheckpointAndWal,
                Phase::ChangePageExhaustiveProposalSearch,
                Phase::ChangePageSupportExpansion,
                Phase::CheckpointAndWal,
                Phase::CheckpointAndWal,
                Phase::ChangePagePresentationProjection,
            ]
        );
        assert_eq!(phases[2].counters.change_candidates, 3);
        assert_eq!(phases[2].counters.change_candidate_current_revisions, 3);
        assert_eq!(phases[5].counters.change_proposal_carriers_opened, 6);
        assert_eq!(phases[5].counters.change_proposal_carriers_validated, 6);
        assert_eq!(phases[7].counters.change_matches, 1);
        assert_eq!(phases[8].counters.change_support_carriers_opened, 2);
        assert!(
            phases
                .iter()
                .all(|phase| phase.counters.change_rows_emitted == 0),
            "successful row emission is recorded only after final response validation"
        );

        for query in [
            target.change_id.as_str().to_owned(),
            target.revision.revision_id.as_str().to_owned(),
            format!("\u{a0}{}\u{3000}", target_summary.to_uppercase()),
        ] {
            let DerivedChangeOutcomeV1::Ready(page) = fixture
                .access
                .changes(&bounded_search_request(&query, 1, None))
                .expect("search by frozen Change field")
            else {
                panic!("identity and Unicode summary search must be ready");
            };
            assert_eq!(
                page.document.document.changes[0].change_id,
                target.change_id
            );
        }

        let unicode_target = &fixture.changes[2];
        let DerivedChangeOutcomeV1::Ready(page) = fixture
            .access
            .changes(&bounded_search_request("\u{a0}\u{c4}PFEL\u{3000}", 1, None))
            .expect("search by normalized Unicode proposal summary")
        else {
            panic!("normalized Unicode proposal search must be ready");
        };
        assert_eq!(
            page.document.document.changes[0].change_id,
            unicode_target.change_id
        );

        let DerivedChangeOutcomeV1::Ready(attention) = fixture
            .access
            .attention(&bounded_search_request(&target_summary, 1, None))
            .expect("search derived Attention")
        else {
            panic!("derived Attention search must be ready");
        };
        assert_eq!(attention.document.document.changes.len(), 1);
        assert_eq!(
            attention.document.document.changes[0].change_id,
            target.change_id
        );
        assert_eq!(
            attention
                .attention_presentations
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec![target.change_id]
        );
    }

    #[test]
    fn derived_change_summary_search_filters_before_hydration_and_conflicts_fail_globally() {
        let filtered = ActiveChangeFixture::new(&[
            &[Some("first"), Some("first")],
            &[Some("second"), Some("second")],
        ]);
        filtered
            .runtime
            .current()
            .expect("warm filtered fixture before counting");
        let scope = LongitudinalCountingScopeV1::new("5".repeat(64)).unwrap();
        let guard = scope.enter();
        let outcome = filtered
            .access
            .changes(&bounded_search_request(
                "does not matter",
                1,
                Some(ChangeLifecycleV1::Accepted),
            ))
            .expect("search empty bodyless candidate set");
        drop(guard);
        let DerivedChangeOutcomeV1::Ready(page) = outcome else {
            panic!("an empty filtered search must be ready");
        };
        assert!(page.document.document.changes.is_empty());
        let counters = scope.snapshot().counters;
        assert_eq!(counters.change_candidates, 0);
        assert_eq!(counters.change_candidate_current_revisions, 0);
        assert_eq!(counters.change_proposal_carriers_opened, 0);
        assert_eq!(counters.change_proposal_carriers_validated, 0);
        assert_eq!(counters.change_matches, 0);
        assert_eq!(counters.change_rows_emitted, 0);
        assert_eq!(counters.carrier_opens, 0);

        let conflicting = ActiveChangeFixture::new(&[
            &[Some("nonmatching state"), None],
            &[Some("target state"), Some("target state")],
        ]);
        assert_projection_invalid(
            conflicting
                .access
                .changes(&bounded_search_request("target state", 1, None))
                .expect("search across a conflicting candidate"),
            "conflicting proposal summaries for exact Revision",
        );
    }

    #[test]
    fn derived_timeline_projects_validated_change_aware_entries() {
        let fixture = ActiveChangeFixture::new(&[&[Some("first proposal")], &[None]]);
        let request = crate::session::DerivedTimelinePageRequestV1::new(
            100,
            crate::session::DerivedTimelineOrderV1::Asc,
            None,
            Vec::new(),
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::Initial,
        )
        .expect("build Timeline request");

        let DerivedChangeOutcomeV1::Ready(page) = fixture
            .access
            .timeline(&request, &crate::session::TrustSet::default())
            .expect("read derived Timeline")
        else {
            panic!("derived Timeline fixture must be ready");
        };
        assert_eq!(page.document().match_count, 6);
        assert_eq!(page.document().entries.len(), 6);
        assert!(page.document().previous.is_none());
        assert!(page.document().next.is_none());
        for change in &fixture.changes {
            assert!(page.document().entries.iter().any(|entry| {
                entry.change_ids.contains(&change.change_id)
                    && entry.revision_refs.contains(&change.revision)
            }));
        }
    }

    #[test]
    fn derived_timeline_preserves_historical_withdrawn_change_correlation() {
        let fixture =
            ActiveChangeFixture::new(&[&[Some("first proposal")], &[Some("second proposal")]]);
        let first_change = fixture.changes[0].clone();
        let second_change = fixture.changes[1].clone();
        let (membership, withdrawal) = fixture.append_historical_membership_then_withdraw(
            &first_change.change_id,
            &second_change.revision,
        );
        let request = crate::session::DerivedTimelinePageRequestV1::new(
            100,
            crate::session::DerivedTimelineOrderV1::Asc,
            None,
            Vec::new(),
            None,
            Some(first_change.change_id),
            None,
            crate::session::DerivedTimelinePagePositionV1::Initial,
        )
        .expect("build historical-correlation request");

        let outcome = fixture
            .access
            .timeline(&request, &crate::session::TrustSet::default())
            .expect("read historical-correlation Timeline");
        let DerivedChangeOutcomeV1::Ready(page) = outcome else {
            panic!("historical-correlation Timeline must be ready: {outcome:?}");
        };
        let event_ids = page
            .document()
            .entries
            .iter()
            .map(|entry| &entry.event_id)
            .collect::<Vec<_>>();
        assert!(event_ids.contains(&&second_change.proposal_events[0].event_id));
        assert!(event_ids.contains(&&membership.event_id));
        assert!(event_ids.contains(&&withdrawal.event_id));
    }

    #[test]
    fn derived_timeline_entries_match_the_strict_projector_with_support_and_history() {
        let fixture = ActiveChangeFixture::new(&[
            &[Some("strict parity first")],
            &[Some("strict parity second")],
        ]);
        let first = fixture.changes[0].clone();
        let second = fixture.changes[1].clone();
        fixture.append_historical_membership_then_withdraw(&first.change_id, &second.revision);
        fixture.append_relation_then_withdraw(
            &first.change_id,
            second.revision.clone(),
            first.revision.clone(),
        );
        fixture.append_removal_support(&first.revision);
        let request = crate::session::DerivedTimelinePageRequestV1::new(
            100,
            crate::session::DerivedTimelineOrderV1::Asc,
            None,
            Vec::new(),
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::Initial,
        )
        .unwrap();
        let DerivedChangeOutcomeV1::Ready(derived) = fixture
            .access
            .timeline(&request, &crate::session::TrustSet::default())
            .unwrap()
        else {
            panic!("derived parity Timeline must be ready");
        };
        let source_change_projection_stamp =
            derived.document().source_change_projection_stamp.clone();

        let events = fixture
            .store
            .list_events()
            .expect("read strict parity events");
        let change_projection =
            crate::session::project_change_documents(&events).expect("project strict Changes");
        let authority_cursor = inspect_journal_records(
            StoreBackend::Local(fixture._temp.path().to_path_buf())
                .journal()
                .as_ref(),
        )
        .expect("inspect strict parity authority")
        .cursor;
        let strict = crate::session::project_event_history(
            &events,
            &change_projection,
            authority_cursor,
            source_change_projection_stamp,
            &crate::session::TrustSet::default(),
        )
        .expect("project strict parity Timeline")
        .document();
        let derived = derived.document();

        assert_eq!(derived, &strict);
    }

    #[test]
    fn derived_timeline_recomputes_ambiguous_revision_candidates_from_all_carriers() {
        let fixture = ActiveChangeFixture::new(&[&[Some("original proposal")]]);
        let change = fixture.changes[0].clone();
        fixture.append_conflicting_proposal(&change.revision.revision_id);
        let request = crate::session::DerivedTimelinePageRequestV1::new(
            100,
            crate::session::DerivedTimelineOrderV1::Asc,
            None,
            Vec::new(),
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::Initial,
        )
        .unwrap();
        let DerivedChangeOutcomeV1::Ready(derived) = fixture
            .access
            .timeline(&request, &crate::session::TrustSet::default())
            .unwrap()
        else {
            panic!("ambiguous derived Timeline must be ready");
        };
        let membership = derived
            .document()
            .entries
            .iter()
            .find(|entry| entry.event_type == EventType::ChangeMembershipAsserted)
            .cloned()
            .expect("ambiguous membership entry");
        assert!(membership.revision_refs.is_empty());
        assert_eq!(
            membership.unresolved_revision_ids,
            vec![change.revision.revision_id.clone()]
        );

        let events = fixture
            .store
            .list_events()
            .expect("read ambiguous strict events");
        let change_projection =
            crate::session::project_change_documents(&events).expect("project ambiguous Changes");
        let authority_cursor = inspect_journal_records(
            StoreBackend::Local(fixture._temp.path().to_path_buf())
                .journal()
                .as_ref(),
        )
        .expect("inspect ambiguous authority")
        .cursor;
        let strict = crate::session::project_event_history(
            &events,
            &change_projection,
            authority_cursor,
            "strict-ambiguous-source-stamp".to_owned(),
            &crate::session::TrustSet::default(),
        )
        .expect("project ambiguous strict Timeline")
        .document();
        assert_eq!(derived.document().entries, strict.entries);

        let exact_request = crate::session::DerivedTimelinePageRequestV1::new(
            100,
            crate::session::DerivedTimelineOrderV1::Asc,
            None,
            Vec::new(),
            None,
            None,
            Some(crate::session::DerivedTimelineExactRevisionV1::new(
                change.revision.clone(),
            )),
            crate::session::DerivedTimelinePagePositionV1::Initial,
        )
        .unwrap();
        let DerivedChangeOutcomeV1::Ready(exact) = fixture
            .access
            .timeline(&exact_request, &crate::session::TrustSet::default())
            .unwrap()
        else {
            panic!("exact ambiguous Timeline filter must be ready");
        };
        assert_eq!(exact.document().entries.len(), 1);
        assert_eq!(
            exact.document().entries[0].event_id,
            change.proposal_events[0].event_id
        );
    }

    #[test]
    fn derived_timeline_selected_and_signature_support_fail_closed() {
        use crate::session::derived_access::timeline::TimelineReadBoundary;

        let request_for = |revision: RevisionRefV1| {
            crate::session::DerivedTimelinePageRequestV1::new(
                100,
                crate::session::DerivedTimelineOrderV1::Asc,
                None,
                vec![EventType::WorkObjectProposed],
                None,
                None,
                Some(crate::session::DerivedTimelineExactRevisionV1::new(
                    revision,
                )),
                crate::session::DerivedTimelinePagePositionV1::Initial,
            )
            .unwrap()
        };

        let absent_selected = ActiveChangeFixture::new(&[&[Some("selected carrier")]]);
        let selected_event = absent_selected.changes[0].proposal_events[0].clone();
        let mut removed = false;
        let outcome = absent_selected
            .access
            .timeline_with_hook(
                &request_for(absent_selected.changes[0].revision.clone()),
                &crate::session::TrustSet::default(),
                |boundary| {
                    if boundary == TimelineReadBoundary::CarrierLocatorsSelected && !removed {
                        fs::remove_file(
                            absent_selected
                                .store
                                .event_path_for_idempotency_key(&selected_event.idempotency_key),
                        )
                        .expect("remove selected Timeline carrier");
                        removed = true;
                    }
                },
            )
            .expect("read a removed selected Timeline carrier");
        assert!(removed);
        assert_projection_invalid(outcome, "absent");

        let absent_signature = ActiveChangeFixture::new(&[&[Some("signature support")]]);
        let (_, signature) =
            absent_signature.append_removal_support(&absent_signature.changes[0].revision);
        let mut removed = false;
        let outcome = absent_signature
            .access
            .timeline_with_hook(
                &request_for(absent_signature.changes[0].revision.clone()),
                &crate::session::TrustSet::default(),
                |boundary| {
                    if boundary == TimelineReadBoundary::CarrierHydrationMidpoint && !removed {
                        fs::remove_file(
                            absent_signature
                                .store
                                .event_path_for_idempotency_key(&signature.idempotency_key),
                        )
                        .expect("remove Timeline signature support carrier");
                        removed = true;
                    }
                },
            )
            .expect("read a removed Timeline signature support carrier");
        assert!(removed);
        assert_projection_invalid(outcome, "absent");

        let changed_signature = ActiveChangeFixture::new(&[&[Some("changed signature support")]]);
        let (_, signature) =
            changed_signature.append_removal_support(&changed_signature.changes[0].revision);
        changed_signature.mutate_database(|connection| {
            connection
                .execute(
                    "UPDATE product_history_signature
                     SET target_event_id = ?1
                     WHERE sequence = ?2",
                    params![
                        changed_signature.changes[0].proposal_events[0]
                            .event_id
                            .as_str(),
                        changed_signature.proposal_sequence(&signature),
                    ],
                )
                .expect("change normalized Timeline signature target");
        });
        assert_projection_invalid(
            changed_signature
                .access
                .timeline(
                    &request_for(changed_signature.changes[0].revision.clone()),
                    &crate::session::TrustSet::default(),
                )
                .expect("read changed Timeline signature support"),
            "wrong target",
        );

        let wrong_family = ActiveChangeFixture::new(&[&[Some("wrong signature family")]]);
        let (_, signature) = wrong_family.append_removal_support(&wrong_family.changes[0].revision);
        let signature_sequence = wrong_family.proposal_sequence(&signature);
        wrong_family.mutate_database(|connection| {
            connection
                .execute(
                    "UPDATE locator_event_type SET value = 'review_initialized'
                     WHERE id = (
                         SELECT event_type_id FROM locator_event WHERE sequence = ?1
                     )",
                    [signature_sequence],
                )
                .expect("change compact Timeline signature family");
        });
        assert_projection_invalid(
            wrong_family
                .access
                .timeline(
                    &request_for(wrong_family.changes[0].revision.clone()),
                    &crate::session::TrustSet::default(),
                )
                .expect("read wrong-family Timeline signature support"),
            "does not match persisted row",
        );
    }

    #[test]
    fn derived_timeline_applies_runtime_trust_only_after_carrier_validation() {
        use crate::crypto::EventVerificationStatus;

        let fixture = ActiveChangeFixture::new(&[&[Some("trust fixture")]]);
        let (signed, signer_id) = fixture.append_inline_signed_review_initialized(false);
        let request = crate::session::DerivedTimelinePageRequestV1::new(
            100,
            crate::session::DerivedTimelineOrderV1::Asc,
            None,
            vec![EventType::ReviewInitialized],
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::Initial,
        )
        .unwrap();
        let DerivedChangeOutcomeV1::Ready(untrusted) = fixture
            .access
            .timeline(&request, &crate::session::TrustSet::default())
            .unwrap()
        else {
            panic!("untrusted Timeline must be ready");
        };
        assert_eq!(untrusted.document().entries.len(), 1);
        assert_eq!(
            untrusted.document().entries[0].verification_status,
            EventVerificationStatus::UntrustedKey
        );

        let trusted = crate::session::event_signature_trust_set(serde_json::json!({
            "allowedSigners": {
                signed.writer.actor_id.as_str(): [signer_id]
            }
        }))
        .unwrap();
        let DerivedChangeOutcomeV1::Ready(valid) =
            fixture.access.timeline(&request, &trusted).unwrap()
        else {
            panic!("trusted Timeline must be ready");
        };
        assert_eq!(
            valid.document().entries[0].verification_status,
            EventVerificationStatus::Valid
        );
        assert_ne!(
            valid.document().timeline_projection_stamp,
            untrusted.document().timeline_projection_stamp
        );

        let invalid = ActiveChangeFixture::new(&[&[Some("invalid trust fixture")]]);
        invalid.append_inline_signed_review_initialized(true);
        let DerivedChangeOutcomeV1::Ready(invalid_page) = invalid
            .access
            .timeline(&request, &crate::session::TrustSet::default())
            .unwrap()
        else {
            panic!("invalid-signature Timeline must be ready");
        };
        assert_eq!(
            invalid_page.document().entries[0].verification_status,
            EventVerificationStatus::Invalid
        );

        let unsigned_request = crate::session::DerivedTimelinePageRequestV1::new(
            100,
            crate::session::DerivedTimelineOrderV1::Asc,
            None,
            vec![EventType::ChangeDeclared],
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::Initial,
        )
        .unwrap();
        let DerivedChangeOutcomeV1::Ready(unsigned) = fixture
            .access
            .timeline(&unsigned_request, &trusted)
            .unwrap()
        else {
            panic!("unsigned Timeline must be ready");
        };
        assert_eq!(
            unsigned.document().entries[0].verification_status,
            EventVerificationStatus::Unsigned
        );
    }

    #[test]
    fn derived_timeline_preserves_filters_exhaustive_search_and_bidirectional_windows() {
        let fixture = ActiveChangeFixture::new(&[
            &[Some("alpha proposal prose")],
            &[Some("beta proposal prose")],
            &[Some("gamma proposal prose")],
        ]);
        let first_change = fixture.changes[0].clone();
        let request =
            |query: Option<String>,
             change: Option<ChangeId>,
             revision: Option<RevisionRefV1>,
             position: crate::session::DerivedTimelinePagePositionV1| {
                crate::session::DerivedTimelinePageRequestV1::new(
                    2,
                    crate::session::DerivedTimelineOrderV1::Asc,
                    query,
                    Vec::new(),
                    None,
                    change,
                    revision.map(crate::session::DerivedTimelineExactRevisionV1::new),
                    position,
                )
                .expect("build Timeline request")
            };

        let DerivedChangeOutcomeV1::Ready(first) = fixture
            .access
            .timeline(
                &request(
                    None,
                    Some(first_change.change_id.clone()),
                    Some(first_change.revision.clone()),
                    crate::session::DerivedTimelinePagePositionV1::Initial,
                ),
                &crate::session::TrustSet::default(),
            )
            .unwrap()
        else {
            panic!("filtered Timeline must be ready");
        };
        assert_eq!(first.document().match_count, 2);
        assert_eq!(first.document().entries.len(), 2);
        assert!(first.document().entries.iter().all(|entry| {
            entry.change_ids.contains(&first_change.change_id)
                && entry.revision_refs.contains(&first_change.revision)
        }));

        let DerivedChangeOutcomeV1::Ready(search) = fixture
            .access
            .timeline(
                &request(
                    Some("alpha proposal prose".to_owned()),
                    None,
                    None,
                    crate::session::DerivedTimelinePagePositionV1::Initial,
                ),
                &crate::session::TrustSet::default(),
            )
            .unwrap()
        else {
            panic!("exhaustive Timeline search must be ready");
        };
        assert_eq!(search.document().match_count, 1);
        assert_eq!(
            search.document().entries[0].event_type,
            EventType::WorkObjectProposed
        );

        let DerivedChangeOutcomeV1::Ready(after) = fixture
            .access
            .timeline(
                &request(
                    Some("after:2026-08-10T01:00:30Z".to_owned()),
                    None,
                    None,
                    crate::session::DerivedTimelinePagePositionV1::Initial,
                ),
                &crate::session::TrustSet::default(),
            )
            .unwrap()
        else {
            panic!("structured after Timeline must be ready");
        };
        assert_eq!(after.document().match_count, 7);
        let DerivedChangeOutcomeV1::Ready(before) = fixture
            .access
            .timeline(
                &request(
                    Some("before:2026-08-10T01:00:30Z".to_owned()),
                    None,
                    None,
                    crate::session::DerivedTimelinePagePositionV1::Initial,
                ),
                &crate::session::TrustSet::default(),
            )
            .unwrap()
        else {
            panic!("structured before Timeline must be ready");
        };
        assert_eq!(before.document().match_count, 2);

        let unfiltered = request(
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::Initial,
        );
        let DerivedChangeOutcomeV1::Ready(page_one) = fixture
            .access
            .timeline(&unfiltered, &crate::session::TrustSet::default())
            .unwrap()
        else {
            panic!("first Timeline page must be ready");
        };
        let next = page_one
            .adjacent()
            .next()
            .cloned()
            .expect("first Timeline page has a next boundary");
        let page_two_request = request(
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::continuation(
                crate::session::DerivedTimelineTraversalV1::Next,
                next,
                page_one.document().timeline_projection_stamp.clone(),
            )
            .unwrap(),
        );
        let DerivedChangeOutcomeV1::Ready(page_two) = fixture
            .access
            .timeline(&page_two_request, &crate::session::TrustSet::default())
            .unwrap()
        else {
            panic!("second Timeline page must be ready");
        };
        assert_eq!(page_two.document().offset, 2);
        assert!(page_one.document().entries.iter().all(|left| {
            page_two
                .document()
                .entries
                .iter()
                .all(|right| left.event_id != right.event_id)
        }));
        let previous = page_two
            .adjacent()
            .previous()
            .cloned()
            .expect("second Timeline page has a previous boundary");
        let back_request = request(
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::continuation(
                crate::session::DerivedTimelineTraversalV1::Previous,
                previous,
                page_two.document().timeline_projection_stamp.clone(),
            )
            .unwrap(),
        );
        let DerivedChangeOutcomeV1::Ready(back) = fixture
            .access
            .timeline(&back_request, &crate::session::TrustSet::default())
            .unwrap()
        else {
            panic!("previous Timeline page must be ready");
        };
        assert_eq!(back.document().entries, page_one.document().entries);
    }

    #[test]
    fn timeline_continuation_stamp_is_bound_to_runtime_trust() {
        let fixture = ActiveChangeFixture::new(&[&[None], &[None]]);
        let request = crate::session::DerivedTimelinePageRequestV1::new(
            2,
            crate::session::DerivedTimelineOrderV1::Asc,
            None,
            Vec::new(),
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::Initial,
        )
        .unwrap();
        let DerivedChangeOutcomeV1::Ready(first) = fixture
            .access
            .timeline(&request, &crate::session::TrustSet::default())
            .unwrap()
        else {
            panic!("first Timeline page must be ready");
        };
        let changed_trust = crate::session::event_signature_trust_set(serde_json::json!({
            "allowedSigners": {
                "actor:agent:changed": [
                    "did:key:z6MkehRgf7yJbgaGfYsdoAsKdBPE3dj2CYhowQdcjqSJgvVd"
                ]
            }
        }))
        .unwrap();
        let continuation = crate::session::DerivedTimelinePageRequestV1::new(
            2,
            crate::session::DerivedTimelineOrderV1::Asc,
            None,
            Vec::new(),
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::continuation(
                crate::session::DerivedTimelineTraversalV1::Next,
                first.adjacent().next().cloned().unwrap(),
                first.document().timeline_projection_stamp.clone(),
            )
            .unwrap(),
        )
        .unwrap();
        let outcome = fixture
            .access
            .timeline(&continuation, &changed_trust)
            .unwrap();
        assert!(matches!(
            outcome,
            DerivedChangeOutcomeV1::ProjectionUnavailable(ref document)
                if document.code() == DerivedProjectionFailureCodeV1::ProjectionStale
        ));
    }

    #[test]
    fn derived_timeline_reports_absent_locators_and_boundaries_as_request_errors() {
        let fixture = ActiveChangeFixture::new(&[&[Some("request error")]]);
        let absent = crate::session::DerivedTimelinePageRequestV1::new(
            1,
            crate::session::DerivedTimelineOrderV1::Asc,
            None,
            Vec::new(),
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::At(crate::model::EventId::new(
                "evt:sha256:absent",
            )),
        )
        .unwrap();
        assert!(matches!(
            fixture
                .access
                .timeline(&absent, &crate::session::TrustSet::default()),
            Err(ShoreError::WorkflowInputInvalid { ref reason })
                if reason.contains("does not match")
        ));

        let initial = crate::session::DerivedTimelinePageRequestV1::new(
            1,
            crate::session::DerivedTimelineOrderV1::Asc,
            None,
            Vec::new(),
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::Initial,
        )
        .unwrap();
        let DerivedChangeOutcomeV1::Ready(first) = fixture
            .access
            .timeline(&initial, &crate::session::TrustSet::default())
            .unwrap()
        else {
            panic!("initial request-error Timeline must be ready");
        };
        let next = first
            .adjacent()
            .next()
            .expect("first page has a next boundary");
        let crate::session::DerivedTimelinePageBoundaryV1::Key(next) = next else {
            panic!("next boundary must be keyed");
        };
        let mismatched = crate::session::DerivedTimelinePageRequestV1::new(
            1,
            crate::session::DerivedTimelineOrderV1::Asc,
            None,
            Vec::new(),
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::continuation(
                crate::session::DerivedTimelineTraversalV1::Next,
                crate::session::DerivedTimelinePageBoundaryV1::Key(
                    crate::session::DerivedTimelinePageKeyV1::new(
                        "2099-01-01T00:00:00.000Z",
                        next.event_id().clone(),
                    )
                    .unwrap(),
                ),
                first.document().timeline_projection_stamp.clone(),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            fixture
                .access
                .timeline(&mismatched, &crate::session::TrustSet::default()),
            Err(ShoreError::WorkflowInputInvalid { ref reason })
                if reason.contains("absent")
        ));
    }

    #[test]
    fn derived_timeline_refuses_generation_movement_at_every_read_boundary() {
        use crate::session::derived_access::timeline::TimelineReadBoundary;

        for (index, boundary) in [
            TimelineReadBoundary::SnapshotPinned,
            TimelineReadBoundary::SupportExpansionStarted,
            TimelineReadBoundary::CarrierHydrationMidpoint,
            TimelineReadBoundary::TrustBindingComplete,
        ]
        .into_iter()
        .enumerate()
        {
            let fixture = ActiveChangeFixture::new(&[&[Some("moving Timeline")]]);
            let request = crate::session::DerivedTimelinePageRequestV1::initial();
            let mut moved = false;
            let outcome = fixture
                .access
                .timeline_with_hook(&request, &crate::session::TrustSet::default(), |observed| {
                    if observed == boundary && !moved {
                        fixture.append_unrelated(&format!("timeline-boundary-{index}"));
                        moved = true;
                    }
                })
                .expect("classify a moving Timeline read");
            assert!(moved, "the requested Timeline boundary must be observed");
            assert!(matches!(
                outcome,
                DerivedChangeOutcomeV1::Retryable(ref document)
                    if document.code() == DerivedProjectionFailureCodeV1::ProjectionUnstable
            ));
        }
    }

    #[test]
    fn derived_timeline_does_not_mix_retained_n_with_published_n_plus_one() {
        use crate::session::derived_access::timeline::TimelineReadBoundary;

        let fixture = ActiveChangeFixture::new(&[&[Some("replacement generation")]]);
        let original_generation = fixture
            .lifecycle
            .published_generation_id()
            .unwrap()
            .unwrap();
        let mut rebuilt = false;
        let outcome = fixture
            .access
            .timeline_with_hook(
                &crate::session::DerivedTimelinePageRequestV1::initial(),
                &crate::session::TrustSet::default(),
                |boundary| {
                    if boundary == TimelineReadBoundary::SnapshotPinned && !rebuilt {
                        fixture
                            .lifecycle
                            .rebuild(|_| LifecycleControl::Continue)
                            .expect("publish N+1 during retained N Timeline read");
                        rebuilt = true;
                    }
                },
            )
            .expect("classify N/N+1 Timeline overlap");
        assert!(rebuilt);
        assert_ne!(
            fixture
                .lifecycle
                .published_generation_id()
                .unwrap()
                .unwrap(),
            original_generation
        );
        assert!(matches!(
            outcome,
            DerivedChangeOutcomeV1::Retryable(ref document)
                if document.code() == DerivedProjectionFailureCodeV1::ProjectionUnstable
        ));
    }

    #[test]
    fn derived_timeline_counters_separate_bounded_and_exhaustive_work() {
        let structured = ActiveChangeFixture::new(&[&[Some("bounded proposal prose")]]);
        structured
            .runtime
            .current()
            .expect("warm the structured fixture before counting");
        let structured_scope = LongitudinalCountingScopeV1::new("a".repeat(64)).unwrap();
        let structured_guard = structured_scope.enter();
        let structured_request = crate::session::DerivedTimelinePageRequestV1::new(
            1,
            crate::session::DerivedTimelineOrderV1::Desc,
            None,
            Vec::new(),
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::Initial,
        )
        .unwrap();
        let DerivedChangeOutcomeV1::Ready(structured_page) = structured
            .access
            .timeline(&structured_request, &crate::session::TrustSet::default())
            .unwrap()
        else {
            panic!("structured Timeline must be ready");
        };
        drop(structured_guard);
        assert_eq!(structured_page.document().entries.len(), 1);
        let structured_counters = structured_scope.snapshot().counters;
        assert_eq!(structured_counters.timeline_sqlite_candidates, 3);
        assert_eq!(structured_counters.timeline_sqlite_window_rows, 1);
        assert_eq!(structured_counters.timeline_sqlite_facet_rows, 3);
        assert_eq!(structured_counters.timeline_selected_carriers, 1);
        assert_eq!(structured_counters.timeline_revision_candidate_carriers, 1);
        assert_eq!(structured_counters.timeline_correlation_support_carriers, 0);
        assert_eq!(structured_counters.timeline_removal_support_carriers, 0);
        assert_eq!(structured_counters.timeline_signature_support_carriers, 0);
        assert_eq!(structured_counters.timeline_trust_support_carriers, 1);
        assert_eq!(structured_counters.timeline_exhaustive_candidates, 0);
        assert_eq!(structured_counters.timeline_entries_emitted, 1);
        assert_eq!(structured_counters.carrier_opens, 2);
        assert_eq!(structured_counters.directory_entries_walked, 0);
        assert_eq!(structured_counters.event_folds, 0);
        assert_eq!(structured_counters.authoritative_fallbacks, 0);
        assert_eq!(structured_counters.full_history_fallbacks, 0);

        let exhaustive = ActiveChangeFixture::new(&[&[Some("exhaustive proposal prose")]]);
        exhaustive
            .runtime
            .current()
            .expect("warm the exhaustive fixture before counting");
        let exhaustive_scope = LongitudinalCountingScopeV1::new("b".repeat(64)).unwrap();
        let exhaustive_guard = exhaustive_scope.enter();
        let exhaustive_request = crate::session::DerivedTimelinePageRequestV1::new(
            1,
            crate::session::DerivedTimelineOrderV1::Asc,
            Some("exhaustive proposal prose".to_owned()),
            Vec::new(),
            None,
            None,
            None,
            crate::session::DerivedTimelinePagePositionV1::Initial,
        )
        .unwrap();
        let DerivedChangeOutcomeV1::Ready(exhaustive_page) = exhaustive
            .access
            .timeline(&exhaustive_request, &crate::session::TrustSet::default())
            .unwrap()
        else {
            panic!("exhaustive Timeline must be ready");
        };
        drop(exhaustive_guard);
        assert_eq!(exhaustive_page.document().entries.len(), 1);
        let exhaustive_counters = exhaustive_scope.snapshot().counters;
        assert_eq!(exhaustive_counters.timeline_sqlite_candidates, 3);
        assert_eq!(exhaustive_counters.timeline_sqlite_window_rows, 3);
        assert_eq!(exhaustive_counters.timeline_sqlite_facet_rows, 0);
        assert_eq!(exhaustive_counters.timeline_selected_carriers, 3);
        assert_eq!(exhaustive_counters.timeline_exhaustive_candidates, 3);
        assert_eq!(exhaustive_counters.timeline_trust_support_carriers, 3);
        assert_eq!(exhaustive_counters.timeline_entries_emitted, 1);
        assert_eq!(exhaustive_counters.carrier_opens, 3);
        assert_eq!(exhaustive_counters.directory_entries_walked, 0);
        assert_eq!(exhaustive_counters.event_folds, 0);
        assert_eq!(exhaustive_counters.authoritative_fallbacks, 0);
        assert_eq!(exhaustive_counters.full_history_fallbacks, 0);
    }

    #[test]
    fn derived_timeline_window_plan_uses_change_revision_and_tag_indexes() {
        let fixture = ActiveChangeFixture::new(&[&[Some("query plan")]]);
        let change = fixture.changes[0].clone();
        let request = crate::session::DerivedTimelinePageRequestV1::new(
            10,
            crate::session::DerivedTimelineOrderV1::Asc,
            Some("tag:correctness revision:sha256".to_owned()),
            vec![EventType::WorkObjectProposed],
            None,
            Some(change.change_id),
            Some(crate::session::DerivedTimelineExactRevisionV1::new(
                change.revision,
            )),
            crate::session::DerivedTimelinePagePositionV1::Initial,
        )
        .unwrap();
        let RuntimeCurrentRead::Ready(current) = fixture.runtime.current().unwrap() else {
            panic!("query-plan generation must be current");
        };
        let checkpoint = current
            .pin_change_reader_checkpoint()
            .expect("pin query-plan checkpoint");
        let connection =
            rusqlite::Connection::open(fixture.database_path()).expect("open query-plan sidecar");
        let plan = crate::session::derived_access::timeline::timeline_window_query_plan_for_test(
            &connection,
            checkpoint.truth_cursor,
            &request,
        )
        .expect("explain Timeline window query");
        for index in [
            "product_history_change_correlation_change",
            "product_history_revision_reference_exact",
        ] {
            assert!(
                plan.iter().any(|detail| detail.contains(index)),
                "Timeline query plan must use {index}: {plan:#?}"
            );
        }
        assert!(
            plan.iter()
                .any(|detail| detail.contains("SEARCH tag USING PRIMARY KEY")),
            "Timeline tag predicate must use the sequence/tag primary key: {plan:#?}"
        );
    }

    #[test]
    fn inspector_resolution_connects_one_runtime_without_complete_change_classification() {
        let repo = TempDir::new().expect("create disposable Inspector repository");
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .expect("initialize disposable Inspector repository")
                .success()
        );
        let resolved = resolve_store(repo.path()).expect("resolve disposable Inspector store");
        write_capability_fixture_for_test(
            resolved.backend().journal().as_ref(),
            CapabilityFixtureState::EmptyL2,
        )
        .expect("activate disposable Inspector store");
        let store_identity = opaque_path_identity("store", resolved.store_dir())
            .expect("derive disposable Inspector store identity");
        DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            resolved.store_dir(),
            store_identity,
        )
        .expect("create disposable Inspector lifecycle")
        .rebuild(|_| LifecycleControl::Continue)
        .expect("publish disposable Inspector generation");

        let scope = LongitudinalCountingScopeV1::new("6".repeat(64)).unwrap();
        let guard = scope.enter();
        let access = DerivedChangeAccess::resolve_for_inspector(repo.path())
            .expect("resolve the Change-aware Inspector runtime");
        let recovery = access.recovery_access();
        assert!(recovery.is_active());
        drop(guard);

        let counters = scope.snapshot().counters;
        assert_eq!(counters.directory_entries_walked, 0);
        assert_eq!(counters.event_folds, 0);
        assert_eq!(counters.projection_rebuilds, 0);
        assert_eq!(counters.state_rebuilds, 0);
        assert_eq!(counters.full_history_fallbacks, 0);
    }

    #[test]
    fn command_resolution_shares_the_inspector_resolution_body() {
        let repo = TempDir::new().expect("create disposable command repository");
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .expect("initialize disposable command repository")
                .success()
        );
        let resolved = resolve_store(repo.path()).expect("resolve disposable command store");
        write_capability_fixture_for_test(
            resolved.backend().journal().as_ref(),
            CapabilityFixtureState::EmptyL2,
        )
        .expect("activate disposable command store");
        let store_identity = opaque_path_identity("store", resolved.store_dir())
            .expect("derive disposable command store identity");
        DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            resolved.store_dir(),
            store_identity,
        )
        .expect("create disposable command lifecycle")
        .rebuild(|_| LifecycleControl::Continue)
        .expect("publish disposable command generation");

        let command = DerivedChangeAccess::resolve_for_command(repo.path())
            .expect("resolve the Change-aware command runtime");
        let inspector = DerivedChangeAccess::resolve_for_inspector(repo.path())
            .expect("resolve the Change-aware Inspector runtime");
        assert!(command.is_active());
        assert!(inspector.is_active());

        let DerivedChangeOutcomeV1::Ready(command_profile) =
            command.profile().expect("read the command profile")
        else {
            panic!("command resolution must reach the ready derived profile");
        };
        let DerivedChangeOutcomeV1::Ready(inspector_profile) =
            inspector.profile().expect("read the Inspector profile")
        else {
            panic!("Inspector resolution must reach the ready derived profile");
        };
        assert_eq!(
            command_profile, inspector_profile,
            "both constructors resolve the same store root and runtime state"
        );
    }

    #[test]
    fn review_documents_compose_the_cli_schemas_over_the_bare_page() {
        let fixture = ActiveChangeFixture::new(&[&[Some("review state"), Some("review state")]]);

        let DerivedChangeOutcomeV1::Ready(list) = fixture
            .access
            .review_list_document()
            .expect("read the review list document")
        else {
            panic!("review list must be ready on an active-current generation");
        };
        let DerivedChangeOutcomeV1::Ready(page) = fixture
            .access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("read the Inspector page oracle")
        else {
            panic!("Inspector page oracle must be ready");
        };
        assert_eq!(list.schema, crate::documents::REVIEW_CHANGE_LIST_SCHEMA);
        assert_eq!(list.version, 1);
        assert_eq!(
            list.projection_stamp, page.document.document.projection_stamp,
            "the review document binds the same generation stamp"
        );
        assert_eq!(list.changes, page.document.document.changes);
        assert_eq!(list.diagnostics, page.document.document.diagnostics);

        let DerivedChangeOutcomeV1::Ready(attention) = fixture
            .access
            .review_attention_document()
            .expect("read the review attention document")
        else {
            panic!("review attention must be ready on an active-current generation");
        };
        let DerivedChangeOutcomeV1::Ready(attention_page) = fixture
            .access
            .attention(&DerivedChangePageRequestV1::Bare)
            .expect("read the Inspector attention oracle")
        else {
            panic!("Inspector attention oracle must be ready");
        };
        assert_eq!(
            attention.document.schema,
            crate::documents::ATTENTION_LIST_SCHEMA_V2
        );
        assert_eq!(attention.document.version, 2);
        assert_eq!(attention.document.projection_stamp, list.projection_stamp);
        assert_eq!(
            attention.document.changes,
            attention_page.document.document.changes
        );
        assert_eq!(
            attention.presentations,
            attention_page.document.presentations
        );
    }

    #[test]
    fn profile_source_distinguishes_the_generation_from_the_control_path() {
        let fixture = ActiveChangeFixture::new(&[&[Some("source state"), Some("source state")]]);
        let DerivedChangeOutcomeV1::Ready((document, source)) = fixture
            .access
            .profile_with_source()
            .expect("read the sourced profile")
        else {
            panic!("an active-current generation answers the sourced profile");
        };
        assert_eq!(document.availability, ReaderProfileAvailabilityV1::Ready);
        assert_eq!(source, DerivedReadSourceV1::Generation);
        let DerivedChangeOutcomeV1::Ready(unsourced) =
            fixture.access.profile().expect("read the plain profile")
        else {
            panic!("the plain profile mirrors the sourced read");
        };
        assert_eq!(unsourced, document);

        let control = TempDir::new().expect("create disposable control-path repository");
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(control.path())
                .status()
                .expect("initialize disposable control-path repository")
                .success()
        );
        let resolved = resolve_store(control.path()).expect("resolve disposable control store");
        write_capability_fixture_for_test(
            resolved.backend().journal().as_ref(),
            CapabilityFixtureState::M1,
        )
        .expect("install the M1 capability fixture");
        let access = DerivedChangeAccess::resolve_for_command(control.path())
            .expect("resolve the M1 command runtime");
        let DerivedChangeOutcomeV1::Ready((document, source)) = access
            .profile_with_source()
            .expect("read the M1 sourced profile")
        else {
            panic!("the M1 control path answers with the typed profile document");
        };
        assert_eq!(
            document.availability,
            ReaderProfileAvailabilityV1::MigrationInProgress
        );
        assert_eq!(source, DerivedReadSourceV1::CapabilityControlPath);
    }

    #[test]
    fn review_documents_fail_closed_with_the_profile_outcome_classes() {
        fn outcome_class<T>(outcome: &DerivedChangeOutcomeV1<T>) -> &'static str {
            match outcome {
                DerivedChangeOutcomeV1::Ready(_) => "ready",
                DerivedChangeOutcomeV1::AuthorityUnavailable(_) => "authority-unavailable",
                DerivedChangeOutcomeV1::AuthorityConflicted(_) => "authority-conflicted",
                DerivedChangeOutcomeV1::AuthorityInvalid(_) => "authority-invalid",
                DerivedChangeOutcomeV1::ReaderUpgradeRequired(_) => "reader-upgrade-required",
                DerivedChangeOutcomeV1::ProjectionUnavailable(_) => "projection-unavailable",
                DerivedChangeOutcomeV1::Retryable(_) => "retryable",
            }
        }

        let access = DerivedChangeAccess::from_runtime(DerivedAccessRuntime::from_mode(
            DerivedAccessMode::Off,
        ));
        let profile_class = outcome_class(&access.profile().expect("classify the profile outcome"));
        assert_ne!(
            profile_class, "ready",
            "an inactive runtime must not answer a derived read"
        );
        assert_eq!(
            outcome_class(
                &access
                    .review_list_document()
                    .expect("classify the review list outcome")
            ),
            profile_class,
            "the review list fails closed with the profile's outcome class"
        );
        assert_eq!(
            outcome_class(
                &access
                    .review_attention_document()
                    .expect("classify the review attention outcome")
            ),
            profile_class,
            "the review attention fails closed with the profile's outcome class"
        );
    }

    #[test]
    fn receipt_backed_profile_matches_strict_oracle_at_the_live_checkpoint() {
        let fixture = ActiveChangeFixture::new(&[&[Some("profile state"), Some("profile state")]]);
        let inspection = inspect_journal_records(
            StoreBackend::Local(fixture._temp.path().to_path_buf())
                .journal()
                .as_ref(),
        )
        .expect("classify the strict profile oracle outside the measured read");
        let expected = ReaderProfileDocumentV1::from(&StoreCapabilityInspection {
            status: inspection.status,
            cursor: inspection.cursor,
            minimum_reader_profile: inspection.minimum_reader_profile,
        });
        assert!(expected.authority_cursor.event_count > 0);

        let scope = LongitudinalCountingScopeV1::new("7".repeat(64)).unwrap();
        let guard = scope.enter();
        let outcome = fixture
            .access
            .profile()
            .expect("read the receipt-backed Inspector profile");
        drop(guard);

        let DerivedChangeOutcomeV1::Ready(actual) = outcome else {
            panic!("a valid L2 V3 profile must be ready: {outcome:?}");
        };
        assert_eq!(actual, expected);
        let RuntimeCurrentRead::Ready(current) = fixture.runtime.current().unwrap() else {
            panic!("the fixture generation must remain current");
        };
        assert_eq!(
            actual.authority_cursor,
            current
                .pin_change_reader_checkpoint()
                .expect("pin the exact live reader checkpoint")
                .authority_cursor
        );

        let counters = scope.snapshot().counters;
        assert_eq!(counters.directory_entries_walked, 0);
        assert_eq!(counters.event_folds, 0);
        assert_eq!(counters.projection_rebuilds, 0);
        assert_eq!(counters.state_rebuilds, 0);
        assert_eq!(counters.full_history_fallbacks, 0);
    }

    #[test]
    fn missing_and_incompatible_v3_profiles_are_typed_without_strict_fallback() {
        for case in ["missing", "incompatible"] {
            let fixture = ActiveChangeFixture::new(&[]);
            let receipt_path = fixture.receipt_path();
            match case {
                "missing" => fs::remove_file(&receipt_path).expect("remove disposable V3 receipt"),
                "incompatible" => fs::write(
                    &receipt_path,
                    br#"{"schema":"pointbreak.derived-change-reader-profile-receipt.v2","version":2}"#,
                )
                .expect("replace disposable V3 receipt"),
                _ => unreachable!(),
            }
            let access = fixture.fresh_access();
            let scope = LongitudinalCountingScopeV1::new(match case {
                "missing" => "8".repeat(64),
                "incompatible" => "9".repeat(64),
                _ => unreachable!(),
            })
            .unwrap();
            let guard = scope.enter();
            let outcome = access.profile().expect("classify the damaged V3 profile");
            drop(guard);

            let DerivedChangeOutcomeV1::ProjectionUnavailable(document) = outcome else {
                panic!("{case} V3 state must be a typed projection failure: {outcome:?}");
            };
            assert_eq!(
                document.code(),
                DerivedProjectionFailureCodeV1::ProjectionRebuildRequired,
                "V3 case {case}"
            );
            assert!(!document.is_retryable(), "V3 case {case}");
            let counters = scope.snapshot().counters;
            assert_eq!(counters.event_folds, 0, "V3 case {case}");
            assert_eq!(counters.projection_rebuilds, 0, "V3 case {case}");
            assert_eq!(counters.state_rebuilds, 0, "V3 case {case}");
            assert_eq!(counters.full_history_fallbacks, 0, "V3 case {case}");
        }
    }

    #[test]
    fn derived_failure_documents_do_not_change_the_shared_reader_registry() {
        let registry = crate::documents::change_revision_document_registry();
        assert!(
            !registry
                .iter()
                .any(|(schema, _)| *schema == AUTHORITY_ERROR_SCHEMA)
        );
        assert!(
            !registry
                .iter()
                .any(|(schema, _)| *schema == PROJECTION_ERROR_SCHEMA)
        );
    }

    #[test]
    fn change_seek_stamp_is_seek_scoped_and_distinct_from_the_generation_stamp() {
        let fixture = ActiveChangeFixture::new(&[&[None], &[None]]);
        let RuntimeCurrentRead::Ready(current) = fixture.runtime.current().unwrap() else {
            panic!("fixture generation must remain current");
        };
        let checkpoint = current.pin_change_reader_checkpoint().unwrap();
        let LocatorRead::Ready(materialized) = current
            .service()
            .semantic_materialized_change_projection_at(checkpoint.truth_cursor)
            .unwrap()
        else {
            panic!("fixture projection must be caught up");
        };

        let narrowed = |change_id: &ChangeId| {
            let LocatorRead::Ready(facts) = current
                .service()
                .semantic_change_seek_facts_at(change_id, checkpoint.truth_cursor)
                .unwrap()
            else {
                panic!("fixture seek must be caught up");
            };
            let semantic = crate::session::projection::change::project_changes_from_facts(
                &facts
                    .iter()
                    .map(|fact| fact.change.clone())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            let document =
                crate::session::projection::change::project_change_documents_from_facts(&facts)
                    .unwrap();
            (semantic, document)
        };

        let first = &fixture.changes[0].change_id;
        let second = &fixture.changes[1].change_id;
        let (first_semantic, first_document) = narrowed(first);
        let (second_semantic, second_document) = narrowed(second);

        let first_stamp = current
            .change_seek_stamp(&checkpoint, first, &first_semantic, &first_document)
            .unwrap();
        assert_eq!(
            first_stamp,
            current
                .change_seek_stamp(&checkpoint, first, &first_semantic, &first_document)
                .unwrap(),
            "two seek invocations at one store state agree"
        );
        assert_ne!(
            first_stamp,
            current
                .change_seek_stamp(&checkpoint, second, &second_semantic, &second_document)
                .unwrap(),
            "seek stamps are scoped to their Change"
        );
        assert_ne!(
            first_stamp,
            current
                .change_generation_stamp(
                    &checkpoint,
                    &materialized.projection,
                    &materialized.document_projection,
                )
                .unwrap(),
            "the seek stamp never equals the whole-generation stamp"
        );
    }

    fn narrowed_seek_folds(
        current: &Arc<super::super::lifecycle::CurrentGeneration>,
        change_id: &ChangeId,
        as_of: super::super::cursor::TruthCursor,
    ) -> (ChangeProjection, ChangeDocumentProjectionV1) {
        let LocatorRead::Ready(facts) = current
            .service()
            .semantic_change_seek_facts_at(change_id, as_of)
            .unwrap()
        else {
            panic!("fixture seek must be caught up");
        };
        let semantic = crate::session::projection::change::project_changes_from_facts(
            &facts
                .iter()
                .map(|fact| fact.change.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let document =
            crate::session::projection::change::project_change_documents_from_facts(&facts)
                .unwrap();
        (semantic, document)
    }

    #[test]
    fn review_detail_document_returns_ready_detail_on_an_active_current_generation() {
        let fixture = ActiveChangeFixture::new(&[&[None]]);
        let change_id = fixture.changes[0].change_id.clone();
        let DerivedChangeOutcomeV1::Ready(detail) =
            fixture.access.review_detail_document(&change_id).unwrap()
        else {
            panic!("active-current detail must be ready");
        };

        let RuntimeCurrentRead::Ready(current) = fixture.runtime.current().unwrap() else {
            panic!("fixture generation must remain current");
        };
        let checkpoint = current.pin_change_reader_checkpoint().unwrap();
        let LocatorRead::Ready(materialized) = current
            .service()
            .semantic_materialized_change_projection_at(checkpoint.truth_cursor)
            .unwrap()
        else {
            panic!("fixture projection must be caught up");
        };
        let authoritative =
            ChangeDocumentFacadeV1::new(materialized.projection, materialized.document_projection)
                .unwrap();
        let mut expected = authoritative.detail_document(&change_id).unwrap();
        let mut actual = detail.clone();
        assert_ne!(
            actual.detail.projection_stamp, expected.detail.projection_stamp,
            "the derived detail substitutes the seek stamp"
        );
        let (narrowed_semantic, narrowed_document) =
            narrowed_seek_folds(&current, &change_id, checkpoint.truth_cursor);
        assert_eq!(
            actual.detail.projection_stamp,
            current
                .change_seek_stamp(
                    &checkpoint,
                    &change_id,
                    &narrowed_semantic,
                    &narrowed_document
                )
                .unwrap(),
            "the bound stamp is the lifecycle carrier's seek stamp"
        );
        expected.detail.projection_stamp = String::new();
        expected.detail.summary.projection_stamp = String::new();
        actual.detail.projection_stamp = String::new();
        actual.detail.summary.projection_stamp = String::new();
        assert_eq!(
            actual, expected,
            "every other detail byte equals the authoritative composition"
        );
    }

    #[test]
    fn change_seek_returns_the_narrowed_view_and_refs() {
        let fixture = ActiveChangeFixture::new(&[&[None], &[None]]);
        let change_id = fixture.changes[0].change_id.clone();
        let DerivedChangeOutcomeV1::Ready(seek) = fixture.access.change_seek(&change_id).unwrap()
        else {
            panic!("active-current seek must be ready");
        };

        let RuntimeCurrentRead::Ready(current) = fixture.runtime.current().unwrap() else {
            panic!("fixture generation must remain current");
        };
        let checkpoint = current.pin_change_reader_checkpoint().unwrap();
        let LocatorRead::Ready(materialized) = current
            .service()
            .semantic_materialized_change_projection_at(checkpoint.truth_cursor)
            .unwrap()
        else {
            panic!("fixture projection must be caught up");
        };
        let authoritative_view = &materialized.projection.changes[&change_id];
        assert_eq!(seek.change_view(), authoritative_view);
        for member in &authoritative_view.members {
            assert_eq!(
                seek.document_projection().revision_refs.get(member),
                materialized.document_projection.revision_refs.get(member),
                "member {member:?} resolves the authoritative exact reference"
            );
        }
        assert!(!seek.stamp().is_empty());
    }

    #[test]
    fn exact_revision_session_carriers_are_available_to_product_callers() {
        let plan = ExactRevisionReadPlanV1 {
            revisions: Vec::new(),
            include_body: true,
            read_for_display: true,
            fact_port_context: None,
        };
        let method = DerivedChangeAccess::exact_revision_session;
        let mapped = DerivedChangeOutcomeV1::Ready(1usize).map_ready(|value| value + 1);
        assert_eq!(mapped, DerivedChangeOutcomeV1::Ready(2));
        let _ = (plan, method);
    }

    #[test]
    fn exact_revision_session_reads_the_context_revision_and_binds_the_repository() {
        let fixture = ActiveChangeFixture::new(&[&[Some("exact session")]]);
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(fixture._temp.path())
                .status()
                .expect("initialize exact-session repository")
                .success()
        );
        let access = DerivedChangeAccess {
            runtime: Arc::clone(&fixture.runtime),
            repo: Some(fixture._temp.path().to_path_buf()),
        };
        let change = &fixture.changes[0];
        let DerivedChangeOutcomeV1::Ready(session) = access
            .exact_revision_session(&change.change_id)
            .expect("prepare exact-Revision session")
        else {
            panic!("active exact-Revision session must be ready");
        };
        let DerivedChangeOutcomeV1::Ready(read) = session
            .read(&ExactRevisionReadPlanV1 {
                revisions: Vec::new(),
                include_body: true,
                read_for_display: true,
                fact_port_context: Some(change.revision.clone()),
            })
            .expect("read exact-Revision session")
        else {
            panic!("active exact-Revision read must be ready");
        };
        let result = read
            .result(&change.revision)
            .expect("the unplanned context revision is auto-read");
        assert_eq!(result.revision.id, change.revision.revision_id);

        let DerivedChangeOutcomeV1::Ready(unbound) = fixture
            .access
            .exact_revision_session(&change.change_id)
            .expect("prepare runtime-only session")
        else {
            panic!("runtime-only session preparation remains available");
        };
        assert!(matches!(
            unbound.read(&ExactRevisionReadPlanV1 {
                revisions: vec![change.revision.clone()],
                include_body: false,
                read_for_display: true,
                fact_port_context: None,
            }),
            Ok(DerivedChangeOutcomeV1::ProjectionUnavailable(_))
        ));
    }

    #[test]
    fn session_read_matches_the_authoritative_component_show() {
        let fixture = ActiveChangeFixture::new(&[&[Some("authoritative parity")]]);
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(fixture._temp.path())
                .status()
                .expect("initialize parity repository")
                .success()
        );
        let access = DerivedChangeAccess {
            runtime: Arc::clone(&fixture.runtime),
            repo: Some(fixture._temp.path().to_path_buf()),
        };
        let change = &fixture.changes[0];
        let DerivedChangeOutcomeV1::Ready(session) = access
            .exact_revision_session(&change.change_id)
            .expect("prepare parity session")
        else {
            panic!("parity session must be ready");
        };
        let DerivedChangeOutcomeV1::Ready(read) = session
            .read(&ExactRevisionReadPlanV1 {
                revisions: vec![change.revision.clone()],
                include_body: true,
                read_for_display: true,
                fact_port_context: None,
            })
            .expect("read parity session")
        else {
            panic!("parity read must be ready");
        };
        let mut actual = read
            .result(&change.revision)
            .expect("planned Revision result")
            .clone();

        let backend = StoreBackend::Local(fixture._temp.path().to_path_buf());
        let state = crate::session::workflow::change_reader_state_from_backend_for_test(&backend)
            .expect("build authoritative parity generation");
        let mut expected = crate::session::show_revision_for_change_reader_ready(
            crate::session::RevisionShowOptions::new(fixture._temp.path())
                .with_revision_id(change.revision.revision_id.clone())
                .with_exact(true)
                .with_include_body(true)
                .with_read_for_display(true),
            state.ready().expect("L2 fixture is authoritative-ready"),
        )
        .expect("read authoritative component");
        actual.event_set_hash.clear();
        actual.event_count = 0;
        expected.event_set_hash.clear();
        expected.event_count = 0;
        assert_eq!(actual, expected);
    }

    fn active_fact_port_fixture() -> (
        ActiveChangeFixture,
        ChangeId,
        RevisionRefV1,
        Vec<ShoreEvent>,
    ) {
        let fixture = ActiveChangeFixture::new(&[&[None], &[None]]);
        let change_id = fixture.changes[0].change_id.clone();
        let target = fixture.changes[0].revision.clone();
        let origin = fixture.changes[1].revision.clone();
        let membership =
            build_membership_asserted(&change_id, &origin.revision_id, [117; 32]).unwrap();
        record_fixture_event(
            &fixture.store,
            ShoreEvent::new(
                EventType::ChangeMembershipAsserted,
                "fixture:fact-port-origin-membership",
                EventTarget::for_journal(JournalId::new("journal:change-endpoint")),
                Writer::shore_local("change-endpoint-test"),
                membership,
                "2026-08-10T03:00:00Z",
            )
            .unwrap(),
        );

        let writer = Writer::shore_local("change-endpoint-test");
        let track_id = TrackId::new("track:fact-port-seek");
        let explicit = build_review_fact_ported(
            ReviewFactPortDraftV1 {
                origin_revision: origin.clone(),
                origin_fact: FactRefV1::Observation {
                    observation_id: ObservationId::new("observation:sha256:explicit"),
                },
                target_revision: target.clone(),
                relation: FactPortRelationV1::ContextOnly,
                target_fact: None,
                rationale_content_hash: None,
                context_change_id: Some(change_id.clone()),
            },
            &writer.actor_id,
            &track_id,
        )
        .unwrap();
        let unscoped = build_review_fact_ported(
            ReviewFactPortDraftV1 {
                origin_revision: origin.clone(),
                origin_fact: FactRefV1::Observation {
                    observation_id: ObservationId::new("observation:sha256:unscoped"),
                },
                target_revision: target.clone(),
                relation: FactPortRelationV1::ContextOnly,
                target_fact: None,
                rationale_content_hash: None,
                context_change_id: None,
            },
            &writer.actor_id,
            &track_id,
        )
        .unwrap();
        let events = [
            (
                "fixture:fact-port-explicit-one",
                explicit.clone(),
                "2026-08-10T03:00:01Z",
            ),
            (
                "fixture:fact-port-explicit-two",
                explicit,
                "2026-08-10T03:00:02Z",
            ),
            (
                "fixture:fact-port-unscoped",
                unscoped,
                "2026-08-10T03:00:03Z",
            ),
        ]
        .into_iter()
        .map(|(key, payload, occurred_at)| {
            ShoreEvent::new(
                EventType::ReviewFactPorted,
                key,
                EventTarget::for_revision(
                    JournalId::new("journal:change-endpoint"),
                    origin.revision_id.clone(),
                    Some(track_id.clone()),
                )
                .unwrap(),
                writer.clone(),
                payload,
                occurred_at,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
        for event in &events {
            record_fixture_event(&fixture.store, event.clone());
        }
        (fixture, change_id, target, events)
    }

    #[test]
    fn session_read_discovers_applicable_origins_in_one_snapshot() {
        use crate::session::derived_access::change_revision_reads::{
            ExactRevisionReadBoundary, exact_revision_read_v1_inner,
            exact_revision_session_v1_inner,
        };

        let (fixture, change_id, target, _) = active_fact_port_fixture();
        let origin = fixture.changes[1].revision.clone();
        let access = fixture.repo_bound_access();
        let mut session_boundaries = Vec::new();
        let DerivedChangeOutcomeV1::Ready(session) =
            exact_revision_session_v1_inner(&access, &change_id, |boundary| {
                session_boundaries.push(boundary)
            })
            .expect("prepare contextual session")
        else {
            panic!("contextual session must be ready");
        };
        assert_eq!(
            session_boundaries,
            vec![ExactRevisionReadBoundary::SessionPrepared]
        );

        let mut read_boundaries = Vec::new();
        let DerivedChangeOutcomeV1::Ready(read) = exact_revision_read_v1_inner(
            session,
            &ExactRevisionReadPlanV1 {
                revisions: Vec::new(),
                include_body: false,
                read_for_display: true,
                fact_port_context: Some(target.clone()),
            },
            |boundary| read_boundaries.push(boundary),
        )
        .expect("read contextual session") else {
            panic!("contextual session read must be ready");
        };
        assert!(read.result(&target).is_some());
        assert!(read.result(&origin).is_some());
        assert_eq!(
            read_boundaries
                .iter()
                .filter(|boundary| **boundary == ExactRevisionReadBoundary::SnapshotOpened)
                .count(),
            1
        );
        assert_eq!(
            read_boundaries
                .iter()
                .filter(|boundary| **boundary == ExactRevisionReadBoundary::SnapshotFinished)
                .count(),
            1
        );

        let wrong_hash = format!("sha256:{}", "f".repeat(64));
        let mismatched_origin =
            RevisionRefV1::new(origin.revision_id.clone(), wrong_hash.clone()).unwrap();
        let writer = Writer::shore_local("change-endpoint-test");
        let track_id = TrackId::new("track:fact-port-seek");
        let mismatched_port = build_review_fact_ported(
            ReviewFactPortDraftV1 {
                origin_revision: mismatched_origin.clone(),
                origin_fact: FactRefV1::Observation {
                    observation_id: ObservationId::new("observation:sha256:mismatched"),
                },
                target_revision: target.clone(),
                relation: FactPortRelationV1::ContextOnly,
                target_fact: None,
                rationale_content_hash: None,
                context_change_id: Some(change_id.clone()),
            },
            &writer.actor_id,
            &track_id,
        )
        .unwrap();
        let mismatched_port_fact = serde_json::json!({
            "kind": "fact_port",
            "port": mismatched_port,
        });
        fixture.mutate_database(|connection| {
            connection
                .execute(
                    "UPDATE semantic_change_fact
                     SET fact_json = replace(fact_json, ?1, ?2)
                     WHERE json_extract(fact_json, '$.kind') = 'revision'
                       AND instr(fact_json, ?1) > 0",
                    params![origin.object_artifact_content_hash, wrong_hash],
                )
                .expect("inject mismatched materialized origin hash");
            connection
                .execute(
                    "UPDATE semantic_change_fact
                     SET fact_json = ?1
                     WHERE json_extract(fact_json, '$.kind') = 'fact_port'",
                    [mismatched_port_fact.to_string()],
                )
                .expect("bind fact ports to the mismatched materialized origin");
        });
        let DerivedChangeOutcomeV1::Ready(session) = access
            .exact_revision_session(&change_id)
            .expect("prepare mismatched-origin session")
        else {
            panic!("mismatched-origin session preparation stays ready");
        };
        let DerivedChangeOutcomeV1::Ready(read) = session
            .read(&ExactRevisionReadPlanV1 {
                revisions: Vec::new(),
                include_body: false,
                read_for_display: true,
                fact_port_context: Some(target.clone()),
            })
            .expect("read mismatched-origin session")
        else {
            panic!("an origin hash mismatch is an omission, not a failed read");
        };
        assert!(read.result(&target).is_some());
        assert!(read.result(&mismatched_origin).is_none());
    }

    #[test]
    fn session_read_never_selects_the_removal_audit_closure() {
        use crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1 as Phase;

        let fixture = ActiveChangeFixture::new(&[&[Some("phase attribution")]]);
        let access = fixture.repo_bound_access();
        let change = &fixture.changes[0];
        let DerivedChangeOutcomeV1::Ready(session) = access
            .exact_revision_session(&change.change_id)
            .expect("prepare phase-attribution session")
        else {
            panic!("phase-attribution session must be ready");
        };
        let scope = LongitudinalCountingScopeV1::new("e".repeat(64)).unwrap();
        let guard = scope.enter();
        let outcome = session
            .read(&ExactRevisionReadPlanV1 {
                revisions: vec![change.revision.clone()],
                include_body: false,
                read_for_display: true,
                fact_port_context: None,
            })
            .expect("read phase-attribution session");
        drop(guard);
        assert!(matches!(outcome, DerivedChangeOutcomeV1::Ready(_)));
        let snapshot = scope.snapshot();
        let phases = snapshot
            .derived_access_phases
            .iter()
            .map(|sample| sample.phase)
            .collect::<BTreeSet<_>>();
        for phase in [
            Phase::RevisionDetailSqlSelection,
            Phase::RevisionDetailSelectedCarrierHydrationValidation,
            Phase::RevisionDetailSupportCarrierHydrationValidation,
        ] {
            assert!(phases.contains(&phase), "missing phase {phase:?}");
        }
        assert!(!phases.contains(&Phase::RevisionDetailAuditCarrierHydrationValidation));
        assert_eq!(snapshot.counters.strict_journal_inspections, 0);
    }

    #[test]
    fn session_read_reads_content_from_the_authoritative_backend() {
        let fixture = ActiveChangeFixture::new(&[]);
        let change = append_artifact_backed_change(&fixture, true);
        let access = fixture.repo_bound_access();

        let read_with_body = |include_body: bool, identity: char| {
            let DerivedChangeOutcomeV1::Ready(session) = access
                .exact_revision_session(&change.change_id)
                .expect("prepare artifact-backed session")
            else {
                panic!("artifact-backed session must be ready");
            };
            let scope = LongitudinalCountingScopeV1::new(identity.to_string().repeat(64)).unwrap();
            let guard = scope.enter();
            let outcome = session
                .read(&ExactRevisionReadPlanV1 {
                    revisions: vec![change.revision.clone()],
                    include_body,
                    read_for_display: include_body,
                    fact_port_context: None,
                })
                .expect("read artifact-backed session");
            drop(guard);
            let DerivedChangeOutcomeV1::Ready(read) = outcome else {
                panic!("artifact-backed read must be ready");
            };
            let result = read
                .result(&change.revision)
                .expect("artifact-backed result");
            assert_eq!(
                result.snapshot_content_state,
                crate::session::SnapshotContentState::Present
            );
            (
                scope.snapshot().counters,
                result.observations[0].body.clone(),
            )
        };

        let (bodyless, absent_body) = read_with_body(false, 'a');
        assert_eq!(bodyless.object_artifact_reads, 1);
        assert_eq!(bodyless.body_artifact_reads, 0);
        assert!(absent_body.is_none());

        let (with_body, present_body) = read_with_body(true, 'b');
        assert_eq!(with_body.object_artifact_reads, 1);
        assert!(with_body.body_artifact_reads > 0);
        assert!(present_body.is_some());
    }

    #[test]
    fn session_read_fails_closed_on_moved_generation() {
        use crate::session::derived_access::change_revision_reads::{
            ExactRevisionReadBoundary, exact_revision_read_v1_inner,
        };

        let fixture = ActiveChangeFixture::new(&[&[Some("moving session")]]);
        let access = fixture.repo_bound_access();
        let change = &fixture.changes[0];
        let DerivedChangeOutcomeV1::Ready(session) = access
            .exact_revision_session(&change.change_id)
            .expect("prepare moving session")
        else {
            panic!("moving session must initially be ready");
        };
        let mut moved = false;
        let outcome = exact_revision_read_v1_inner(
            session,
            &ExactRevisionReadPlanV1 {
                revisions: vec![change.revision.clone()],
                include_body: false,
                read_for_display: true,
                fact_port_context: None,
            },
            |boundary| {
                if boundary == ExactRevisionReadBoundary::SnapshotFinished && !moved {
                    fixture.append_unrelated("exact-session-moved");
                    moved = true;
                }
            },
        )
        .expect("classify moving exact-Revision session");
        assert!(moved);
        assert!(matches!(
            outcome,
            DerivedChangeOutcomeV1::Retryable(ref document)
                if document.code() == DerivedProjectionFailureCodeV1::ProjectionUnstable
        ));
    }

    #[test]
    fn session_read_labels_post_selection_failures() {
        use crate::session::derived_access::change_revision_reads::{
            ExactRevisionReadBoundary, exact_revision_read_v1_inner,
        };

        let fixture = ActiveChangeFixture::new(&[&[Some("selected carrier")]]);
        let access = fixture.repo_bound_access();
        let change = &fixture.changes[0];
        let selected_path = fixture
            .store
            .event_path_for_idempotency_key(&change.proposal_events[0].idempotency_key);
        let DerivedChangeOutcomeV1::Ready(session) = access
            .exact_revision_session(&change.change_id)
            .expect("prepare selected-corruption session")
        else {
            panic!("selected-corruption session must be ready");
        };
        let scope = LongitudinalCountingScopeV1::new("d".repeat(64)).unwrap();
        let guard = scope.enter();
        let mut removed = false;
        let result = exact_revision_read_v1_inner(
            session,
            &ExactRevisionReadPlanV1 {
                revisions: vec![change.revision.clone()],
                include_body: false,
                read_for_display: true,
                fact_port_context: None,
            },
            |boundary| {
                if boundary == ExactRevisionReadBoundary::SnapshotOpened && !removed {
                    fs::remove_file(&selected_path)
                        .expect("remove disposable selected carrier after snapshot open");
                    removed = true;
                }
            },
        );
        drop(guard);
        assert!(removed);
        let Err(error) = result else {
            panic!("post-selection carrier loss must be a terminal error");
        };
        assert!(!error.to_string().is_empty());
        assert!(scope.snapshot().observed_route_states.is_empty());

        let stale = ActiveChangeFixture::new(&[&[Some("stale before selection")]]);
        let access = stale.repo_bound_access();
        let change = &stale.changes[0];
        let DerivedChangeOutcomeV1::Ready(session) = access
            .exact_revision_session(&change.change_id)
            .expect("prepare pre-selection stale session")
        else {
            panic!("pre-selection stale session must initially be ready");
        };
        stale.append_unrelated("exact-session-pre-selection");
        let outcome = session
            .read(&ExactRevisionReadPlanV1 {
                revisions: vec![change.revision.clone()],
                include_body: false,
                read_for_display: true,
                fact_port_context: None,
            })
            .expect("pre-selection failure is a typed non-Ready outcome");
        assert!(!matches!(outcome, DerivedChangeOutcomeV1::Ready(_)));
    }

    #[test]
    fn session_refuses_unknown_change_like_the_authoritative_arm() {
        let fixture = ActiveChangeFixture::new(&[&[None]]);
        let access = fixture.repo_bound_access();
        let unknown = ChangeId::new(format!("change:sha256:{}", "0".repeat(64)));
        let Err(error) = access.exact_revision_session(&unknown) else {
            panic!("unknown Change must be refused before session construction");
        };
        assert_eq!(
            error.to_string(),
            format!("Change {} is unavailable", unknown.as_str())
        );
    }

    #[test]
    fn seek_facade_presents_fact_ports_like_the_authoritative_facade() {
        let (fixture, change_id, target, _) = active_fact_port_fixture();

        let RuntimeCurrentRead::Ready(current) = fixture.runtime.current().unwrap() else {
            panic!("fixture generation must be current");
        };
        let checkpoint = current.pin_change_reader_checkpoint().unwrap();
        let DerivedChangeOutcomeV1::Ready(prepared) =
            super::super::change_seek_reads::prepare_narrowed_facade(
                &current,
                &checkpoint,
                &change_id,
            )
            .unwrap()
        else {
            panic!("narrowed facade preparation must be ready");
        };

        let events = fixture.store.list_change_events().unwrap();
        let semantic = crate::session::project_changes(&events).unwrap();
        let provenance = crate::session::project_change_documents(&events).unwrap();
        let event_set_hash = event_set_hash_for_events(&events).unwrap();
        let authoritative = ChangeDocumentFacadeV1::new(semantic.clone(), provenance.clone())
            .unwrap()
            .with_presentations(
                change_presentation_projection(&semantic, &provenance, &events, &event_set_hash)
                    .unwrap(),
            )
            .unwrap();
        let expected = authoritative
            .fact_port_presentations(&change_id, &target)
            .unwrap();
        let actual = prepared
            .facade
            .fact_port_presentations(&change_id, &target)
            .unwrap();

        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 2);
        assert!(actual.iter().any(|port| port.context_change_id.is_none()));
        assert!(actual.iter().any(|port| port.source_event_ids.len() == 2));
    }

    #[test]
    fn seek_consumers_fall_back_when_fact_port_sources_are_malformed() {
        for malformed in ["missing-track", "conflicting-carrier"] {
            let (fixture, change_id, _, port_events) = active_fact_port_fixture();
            match malformed {
                "missing-track" => fixture.mutate_database(|connection| {
                    connection
                        .execute(
                            "UPDATE locator_event SET track_id = NULL WHERE sequence = (\
                             SELECT sequence FROM locator_event_text WHERE event_id = ?1)",
                            [port_events[0].event_id.as_str()],
                        )
                        .unwrap();
                }),
                "conflicting-carrier" => fixture.mutate_database(|connection| {
                    let mut port = port_events[1].payload.clone();
                    port["relation"] = serde_json::json!("resolved_by");
                    let fact = serde_json::json!({
                        "kind": "fact_port",
                        "port": port,
                    });
                    connection
                        .execute(
                            "UPDATE semantic_change_fact SET fact_json = ?1 WHERE sequence = (\
                             SELECT sequence FROM locator_event_text WHERE event_id = ?2)",
                            params![fact.to_string(), port_events[1].event_id.as_str()],
                        )
                        .unwrap();
                }),
                _ => unreachable!(),
            }

            let RuntimeCurrentRead::Ready(current) = fixture.runtime.current().unwrap() else {
                panic!("fixture generation must be current");
            };
            let checkpoint = current.pin_change_reader_checkpoint().unwrap();
            let expected = match malformed {
                "missing-track" => "materialized fact port carries no review track",
                "conflicting-carrier" => "fact-port identity or attribution mismatch",
                _ => unreachable!(),
            };
            assert_projection_invalid(
                super::super::change_seek_reads::prepare_narrowed_facade(
                    &current,
                    &checkpoint,
                    &change_id,
                )
                .unwrap(),
                expected,
            );
            assert_projection_invalid(
                fixture.access.review_detail_document(&change_id).unwrap(),
                expected,
            );
            assert_projection_invalid(fixture.access.change_seek(&change_id).unwrap(), expected);
        }
    }

    fn normalize_projection_stamps(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                if let Some(stamp) = fields.get_mut("projectionStamp") {
                    *stamp = serde_json::Value::String("<seek-stamp>".to_owned());
                }
                for value in fields.values_mut() {
                    normalize_projection_stamps(value);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    normalize_projection_stamps(value);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn seek_producer_output_is_unchanged_by_shared_preparation() {
        let fixture = ActiveChangeFixture::new(&[&[None]]);
        let change_id = fixture.changes[0].change_id.clone();
        let DerivedChangeOutcomeV1::Ready(detail) =
            fixture.access.review_detail_document(&change_id).unwrap()
        else {
            panic!("fixture detail must be ready");
        };
        let DerivedChangeOutcomeV1::Ready(seek) = fixture.access.change_seek(&change_id).unwrap()
        else {
            panic!("fixture seek must be ready");
        };
        let mut snapshot = serde_json::json!({
            "detail": detail,
            "seek": {
                "changeView": seek.change_view(),
                "documentProjection": seek.document_projection(),
                "projectionStamp": seek.stamp(),
            },
        });
        normalize_projection_stamps(&mut snapshot);
        let actual = serde_json::to_string_pretty(&snapshot).unwrap();
        const EXPECTED: &str = r#"{
  "detail": {
    "currentRevisionRefs": [
      {
        "objectArtifactContentHash": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        "revisionId": "rev:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
      }
    ],
    "diagnostics": [],
    "effectiveSupersedes": [],
    "links": [],
    "memberRevisions": [
      {
        "revision": {
          "objectArtifactContentHash": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
          "revisionId": "rev:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        },
        "supportingClaimIds": [
          "change-membership:sha256:5e8ace7ca9625e75ae16988a4bac9e1b8e97355572578e721df8375ffb67a43b"
        ]
      }
    ],
    "membershipClaims": [
      {
        "active": true,
        "changeId": "change:sha256:94e12f1e0a87f6a5c34d8a201588b1dabc40690cc1ce34859131335550198e32",
        "claimId": "change-membership:sha256:5e8ace7ca9625e75ae16988a4bac9e1b8e97355572578e721df8375ffb67a43b",
        "diagnostics": [],
        "revisionId": "rev:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "supports": [
          {
            "actorId": "actor:local",
            "eventId": "evt:sha256:39ecbb2b0e87e97e07ec44a83d6caa476857a413d54adfa246c1a095f58eef59"
          }
        ],
        "withdrawals": []
      }
    ],
    "membershipWithdrawals": [],
    "operativeObligations": [],
    "pendingOrConflictingEdges": [],
    "perCurrentRevisionQualification": [
      {
        "qualified": false,
        "revision": {
          "objectArtifactContentHash": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
          "revisionId": "rev:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        }
      }
    ],
    "projectionStamp": "<seek-stamp>",
    "relationClaims": [],
    "relationWithdrawals": [],
    "schema": "pointbreak.review-change",
    "summary": {
      "attentionSummary": "in_progress",
      "availabilitySummary": "available",
      "changeId": "change:sha256:94e12f1e0a87f6a5c34d8a201588b1dabc40690cc1ce34859131335550198e32",
      "currentRevisionRefs": [
        {
          "objectArtifactContentHash": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
          "revisionId": "rev:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        }
      ],
      "declarationState": "authoritative",
      "diagnostics": [],
      "lifecycle": "in_progress",
      "memberCount": 1,
      "projectionStamp": "<seek-stamp>",
      "titleAssertions": [],
      "topology": "initial"
    },
    "unavailableMemberRevisions": [],
    "version": 1
  },
  "seek": {
    "changeView": {
      "changeId": "change:sha256:94e12f1e0a87f6a5c34d8a201588b1dabc40690cc1ce34859131335550198e32",
      "currentRevisions": [
        "rev:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
      ],
      "diagnostics": [],
      "lifecycle": "in_progress",
      "members": [
        "rev:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
      ],
      "operativeObligations": [],
      "qualifiedCurrentRevisions": [],
      "supersedes": [],
      "topology": "initial"
    },
    "documentProjection": {
      "diagnostics": [],
      "membershipClaims": [
        {
          "active": true,
          "changeId": "change:sha256:94e12f1e0a87f6a5c34d8a201588b1dabc40690cc1ce34859131335550198e32",
          "claimId": "change-membership:sha256:5e8ace7ca9625e75ae16988a4bac9e1b8e97355572578e721df8375ffb67a43b",
          "diagnostics": [],
          "revisionId": "rev:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
          "supports": [
            {
              "actorId": "actor:local",
              "eventId": "evt:sha256:39ecbb2b0e87e97e07ec44a83d6caa476857a413d54adfa246c1a095f58eef59"
            }
          ],
          "withdrawals": []
        }
      ],
      "projectionStamp": "<seek-stamp>",
      "relationClaims": [],
      "revisionRefs": {
        "rev:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb": [
          {
            "objectArtifactContentHash": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
            "revisionId": "rev:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
          }
        ]
      },
      "unavailableRevisionRefs": {}
    },
    "projectionStamp": "<seek-stamp>"
  }
}"#;

        assert_eq!(actual, EXPECTED);
    }

    #[test]
    fn seek_producer_maps_an_unknown_change_to_the_authoritative_refusal_path() {
        let fixture = ActiveChangeFixture::new(&[&[None]]);
        let missing = ChangeId::new(format!("change:sha256:{}", "f".repeat(64)));
        let expected = format!("Change {} is unavailable", missing.as_str());

        let error = fixture.access.review_detail_document(&missing).unwrap_err();
        assert!(
            error.to_string().contains(&expected),
            "detail refusal must carry the authoritative message: {error}"
        );
        let error = fixture.access.change_seek(&missing).unwrap_err();
        assert!(
            error.to_string().contains(&expected),
            "selector refusal must carry the authoritative message: {error}"
        );
    }

    #[test]
    fn seek_producer_reproves_stability_and_retries_on_drift() {
        use crate::session::derived_access::change_seek_reads::{
            ChangeSeekCompositionTarget, ChangeSeekReadBoundary,
            change_seek_read_v1_inner_with_hook,
        };

        let fixture = ActiveChangeFixture::new(&[&[None]]);
        let change_id = fixture.changes[0].change_id.clone();
        let outcome = change_seek_read_v1_inner_with_hook(
            &fixture.access,
            &change_id,
            ChangeSeekCompositionTarget::Detail,
            |boundary| {
                if boundary == ChangeSeekReadBoundary::Composed {
                    let declaration = build_change_declared(
                        ChangeIdentityDescriptorV1::opaque_nonce([201; 32]),
                        [202; 32],
                    )
                    .expect("build drift declaration");
                    record_fixture_event(
                        &fixture.store,
                        ShoreEvent::new(
                            EventType::ChangeDeclared,
                            "fixture:change-declared:drift",
                            EventTarget::for_journal(JournalId::new("journal:change-endpoint")),
                            Writer::shore_local("change-endpoint-test"),
                            declaration,
                            "2026-08-10T02:00:00Z",
                        )
                        .expect("build drift declaration event"),
                    );
                }
            },
        )
        .unwrap();
        let DerivedChangeOutcomeV1::Retryable(document) = outcome else {
            panic!("post-composition drift must be retryable, got a different outcome");
        };
        assert_eq!(
            document.code(),
            DerivedProjectionFailureCodeV1::ProjectionUnstable
        );
    }

    #[test]
    fn seek_producer_validates_narrowed_keys() {
        use crate::session::derived_access::change_seek_reads::validate_narrowed_seek_scope;

        let target = ChangeId::new(format!("change:sha256:{}", "1".repeat(64)));
        let foreign = ChangeId::new(format!("change:sha256:{}", "2".repeat(64)));
        let empty = ChangeProjection::default();
        assert!(validate_narrowed_seek_scope(&target, &empty, 0).is_ok());
        assert!(
            validate_narrowed_seek_scope(&target, &empty, 3).is_err(),
            "selected rows without the target view fail closed"
        );

        let mut foreign_projection = ChangeProjection::default();
        foreign_projection.changes.insert(
            foreign.clone(),
            crate::session::ChangeView {
                change_id: foreign,
                members: BTreeSet::new(),
                current_revisions: BTreeSet::new(),
                supersedes: BTreeSet::new(),
                topology: ChangeTopologyV1::Initial,
                lifecycle: ChangeLifecycleV1::Incomplete,
                qualified_current_revisions: BTreeSet::new(),
                operative_obligations: BTreeSet::new(),
                diagnostics: Vec::new(),
            },
        );
        assert!(
            validate_narrowed_seek_scope(&target, &foreign_projection, 1).is_err(),
            "a foreign folded Change fails closed"
        );
    }
}
