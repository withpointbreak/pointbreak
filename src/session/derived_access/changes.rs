//! Product-domain contract for Change-aware derived reads.
#![cfg_attr(not(test), allow(dead_code))]
#![deny(private_bounds, private_interfaces)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

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
    ChangeListPresentationDocumentV1, ChangeQueryUnavailableDocumentV1, ChangeSummaryV1,
    ReaderProfileDocumentV1, ReaderUpgradeRequiredDocumentV1, attention_presentation_for_change,
};
use crate::error::{Result, ShoreError};
use crate::model::{ChangeId, RevisionRefV1};
use crate::session::event::{EventType, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload};
use crate::session::store::capabilities::{
    StoreCapabilityInspection, StoreCapabilityStatus, change_reader_activation_exists,
    inspect_change_reader_journal_records,
};
use crate::session::store::resolution::resolve_change_read_backend;
use crate::session::{ChangeLifecycleV1, ChangeTopologyV1};

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
}

impl DerivedChangeAccess {
    pub(crate) fn from_runtime(runtime: Arc<DerivedAccessRuntime>) -> Self {
        Self { runtime }
    }

    pub fn resolve_for_inspector(repo: impl AsRef<Path>) -> Result<Self> {
        let profile = DerivedAccessProfile::from_environment()
            .map_err(|error| ShoreError::Message(error.to_string()))?;
        if profile == DerivedAccessProfile::Off {
            return Ok(Self::from_runtime(DerivedAccessRuntime::from_mode(
                DerivedAccessMode::Off,
            )));
        }
        let read_store = resolve_change_read_backend(repo)
            .map_err(|error| ShoreError::Message(error.to_string()))?;
        let runtime =
            DerivedAccessRuntime::from_read_store(read_store).map_err(ShoreError::Message)?;
        Ok(Self::from_runtime(runtime))
    }

    pub fn profile(&self) -> Result<DerivedChangeOutcomeV1<ReaderProfileDocumentV1>> {
        let current = match self.runtime.current() {
            Ok(RuntimeCurrentRead::Ready(current)) => current,
            Ok(RuntimeCurrentRead::Unavailable(status)) => {
                return Ok(self.profile_control_outcome(status));
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
            Err(error) => return Ok(lifecycle_failure_outcome(error)),
        };
        let document = match current.reader_profile_document(&checkpoint) {
            Ok(document) => document,
            Err(error) => return Ok(self.profile_receipt_failure_outcome(error)),
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
        Ok(DerivedChangeOutcomeV1::Ready(document))
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

    fn page_control_outcome<T>(&self, status: RuntimeCurrentStatus) -> DerivedChangeOutcomeV1<T> {
        self.capability_unavailable_or(status)
    }

    fn page_receipt_failure_outcome<T>(&self, error: LifecycleError) -> DerivedChangeOutcomeV1<T> {
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

    #[doc(hidden)]
    pub fn is_active(&self) -> bool {
        self.runtime.is_active()
    }

    pub fn changes(
        &self,
        request: &DerivedChangePageRequestV1,
    ) -> Result<DerivedChangeOutcomeV1<DerivedChangePageV1>> {
        Ok(self
            .read_page(ChangePageLens::Changes, request, |_| {})?
            .map_ready(|page| match page {
                PreparedChangePage::Changes(page) => page,
                PreparedChangePage::Attention(_) => {
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
                PreparedChangePage::Changes(_) => {
                    unreachable!("Attention lens constructs an Attention page")
                }
            }))
    }

    fn read_page(
        &self,
        lens: ChangePageLens,
        request: &DerivedChangePageRequestV1,
        hook: impl FnMut(ChangeReadBoundary),
    ) -> Result<DerivedChangeOutcomeV1<PreparedChangePage>> {
        self.read_page_with_hook(lens, request, hook)
    }

    fn read_page_with_hook(
        &self,
        lens: ChangePageLens,
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
            Err(error) => return Ok(lifecycle_failure_outcome(error)),
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
        let prepared = match lens {
            ChangePageLens::Changes => facade
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
            ChangePageLens::Attention => (|| {
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
    fn map_ready<U>(self, map: impl FnOnce(T) -> U) -> DerivedChangeOutcomeV1<U> {
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

fn lifecycle_failure_outcome<T>(error: LifecycleError) -> DerivedChangeOutcomeV1<T> {
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

fn exact_revision_from_proposal(event: &ShoreEvent) -> Result<RevisionRefV1> {
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
    use crate::model::{ChangeIdentityDescriptorV1, EngagementId, JournalId, ObjectId, RevisionId};
    use crate::session::derived_access::lifecycle::{DerivedAccessLifecycle, LifecycleControl};
    use crate::session::derived_access::product_contract::DerivedAccessProfile;
    use crate::session::derived_access::runtime::DerivedAccessMode;
    use crate::session::derived_access::semantic::change::CHANGE_READER_PROFILE_RESOURCE_V3;
    use crate::session::derived_access::writer::DerivedWriteCoordinator;
    use crate::session::event::{
        ArtifactRemovedPayload, EventSignature, EventSignatureRecordedPayload, EventTarget,
        EventToBeSigned, ReviewInitializedPayload, Revision, Writer, build_change_declared,
        build_membership_asserted, event_signature_pre_authentication_encoding,
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
                Phase::ChangePageBodylessSelection,
                Phase::ChangePageProposalLocatorExpansion,
                Phase::ChangePageCarrierHydrationValidation,
                Phase::ChangePageSupportExpansion,
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
                Phase::ChangePageBodylessSelection,
                Phase::ChangePageProposalLocatorExpansion,
                Phase::ChangePageCarrierHydrationValidation,
                Phase::ChangePageExhaustiveProposalSearch,
                Phase::ChangePageSupportExpansion,
                Phase::ChangePagePresentationProjection,
            ]
        );
        assert_eq!(phases[1].counters.change_candidates, 3);
        assert_eq!(phases[1].counters.change_candidate_current_revisions, 3);
        assert_eq!(phases[3].counters.change_proposal_carriers_opened, 6);
        assert_eq!(phases[3].counters.change_proposal_carriers_validated, 6);
        assert_eq!(phases[4].counters.change_matches, 1);
        assert_eq!(phases[5].counters.change_support_carriers_opened, 2);
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
}
