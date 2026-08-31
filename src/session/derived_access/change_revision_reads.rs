//! Change-scoped exact-Revision reads over one fact snapshot.
//!
//! Session preparation pins the generation and builds the narrowed Change
//! facade without opening a transaction. Consuming the session selects each
//! requested Revision component and its support carriers on one transaction,
//! reads content through the resolved authoritative backend, closes the
//! transaction, and proves that the pinned generation did not move.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::change_seek_reads::prepare_narrowed_facade;
use super::changes::{
    DerivedChangeAccess, DerivedChangeOutcomeV1, DerivedExactRevisionReadV1,
    DerivedExactRevisionSessionV1, DerivedProjectionFailureCodeV1, ExactRevisionReadPlanV1,
    lifecycle_failure_outcome,
};
use super::detail_reads::validate_selected_component_events;
use super::fact_reads::{normalize_events, validate_support_events};
use super::history::hydrate_events;
use super::lifecycle::LifecycleError;
use super::locator::LocatorRead;
use super::runtime::RuntimeCurrentRead;
use super::service::DerivedAccessService;
use super::sqlite::ExactRevisionFactReadSnapshot;
use super::support::support_event_plan;
#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::{
    LongitudinalDerivedAccessPhaseV1 as Phase, enter_derived_access_phase_v1,
};
use crate::documents::{FactPortApplicabilityV1, normalize_fact_presentations};
use crate::error::{Result, ShoreError};
use crate::model::{ChangeId, RevisionRefV1};
use crate::session::derived_access::cursor::TruthCursor;
use crate::session::event::FactRefV1;
use crate::session::store::backend::StoreBackend;
use crate::session::workflow::show_revision_from_selected_events;
use crate::session::{RevisionShowOptions, RevisionShowResult};

/// Observable producer boundaries used by deterministic snapshot tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExactRevisionReadBoundary {
    SessionPrepared,
    SnapshotOpened,
    BaseHydrated,
    OriginsPlanned,
    SnapshotFinished,
}

enum ComponentReadFailure {
    BeforeSelection(String),
    AfterSelection(String),
}

impl ComponentReadFailure {
    fn message(self) -> String {
        match self {
            Self::BeforeSelection(message) | Self::AfterSelection(message) => message,
        }
    }
}

/// Pin a proven-current generation and prepare its narrowed Change facade.
/// The returned session owns no open transaction.
pub(crate) fn exact_revision_session_v1_inner<'a>(
    access: &'a DerivedChangeAccess,
    change_id: &ChangeId,
    mut hook: impl FnMut(ExactRevisionReadBoundary),
) -> Result<DerivedChangeOutcomeV1<DerivedExactRevisionSessionV1<'a>>> {
    #[cfg(any(test, feature = "longitudinal-counting"))]
    let snapshot_phase = enter_derived_access_phase_v1(Phase::ChangeSeekSnapshotAcquisition);
    let current = match access.runtime().current() {
        Ok(RuntimeCurrentRead::Ready(current)) => current,
        Ok(RuntimeCurrentRead::Unavailable(status)) => {
            return Ok(access.page_control_outcome(status));
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
        Err(error) => return Ok(access.page_receipt_failure_outcome(error)),
    };
    if let Err(error) = current.reader_profile_document(&checkpoint) {
        return Ok(access.page_receipt_failure_outcome(error));
    }
    #[cfg(any(test, feature = "longitudinal-counting"))]
    drop(snapshot_phase);

    let prepared = match prepare_narrowed_facade(&current, &checkpoint, change_id)? {
        DerivedChangeOutcomeV1::Ready(prepared) => prepared,
        other => return Ok(other.map_ready(|_| unreachable!("matched non-Ready outcome"))),
    };
    hook(ExactRevisionReadBoundary::SessionPrepared);
    Ok(DerivedChangeOutcomeV1::Ready(
        DerivedExactRevisionSessionV1::new(access, current, generation_id, checkpoint, prepared),
    ))
}

/// Consume a prepared session through one exact fact snapshot and one
/// terminal generation proof.
pub(crate) fn exact_revision_read_v1_inner(
    session: DerivedExactRevisionSessionV1<'_>,
    plan: &ExactRevisionReadPlanV1,
    mut hook: impl FnMut(ExactRevisionReadBoundary),
) -> Result<DerivedChangeOutcomeV1<DerivedExactRevisionReadV1>> {
    let (access, current, generation_id, checkpoint, prepared) = session.into_parts();
    let Some(repo) = access.repo() else {
        return Ok(DerivedChangeOutcomeV1::projection_unavailable(
            DerivedProjectionFailureCodeV1::ProjectionInvalid,
            "derived exact-Revision session has no resolved repository",
        ));
    };
    let Some((_, backend)) = access.runtime().active_context() else {
        return Ok(DerivedChangeOutcomeV1::retryable(
            DerivedProjectionFailureCodeV1::ProjectionUnstable,
            "derived exact-Revision authority moved before snapshot acquisition",
        ));
    };
    let backend = backend.clone();
    let observed = checkpoint.truth_cursor;
    let service = current.service();
    let snapshot = match service.exact_revision_fact_read_snapshot_at(observed) {
        Ok(LocatorRead::Ready(snapshot)) => snapshot,
        Ok(LocatorRead::CatchUpRequired { .. }) => {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionStale,
                "derived exact-Revision facts moved while their checkpoint was pinned",
            ));
        }
        Err(error) => {
            return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                error.to_string(),
            ));
        }
    };
    hook(ExactRevisionReadBoundary::SnapshotOpened);

    let mut selection_started = false;
    let prepared_results = prepare_results(
        &snapshot,
        service,
        &backend,
        repo,
        observed,
        &prepared,
        plan,
        &mut selection_started,
        &mut hook,
    );
    let finished = snapshot.finish().map_err(|error| error.to_string());
    hook(ExactRevisionReadBoundary::SnapshotFinished);

    if let Err(message) = finished {
        return if selection_started {
            Err(ShoreError::Message(message))
        } else {
            Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                message,
            ))
        };
    }
    let results = match prepared_results {
        Ok(results) => results,
        Err(error) if selection_started => {
            return Err(ShoreError::Message(error.message()));
        }
        Err(ComponentReadFailure::BeforeSelection(message)) => {
            return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                message,
            ));
        }
        Err(error @ ComponentReadFailure::AfterSelection(_)) => {
            return Err(ShoreError::Message(error.message()));
        }
    };

    let final_current = match access.runtime().current() {
        Ok(RuntimeCurrentRead::Ready(current)) => current,
        Ok(RuntimeCurrentRead::Unavailable(_)) | Err(_) => {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "derived exact-Revision generation moved before response completion",
            ));
        }
    };
    if final_current.generation_id() != generation_id {
        return Ok(DerivedChangeOutcomeV1::retryable(
            DerivedProjectionFailureCodeV1::ProjectionUnstable,
            "derived exact-Revision generation changed before response completion",
        ));
    }
    let final_checkpoint = match final_current.pin_change_reader_checkpoint() {
        Ok(checkpoint) => checkpoint,
        Err(LifecycleError::TruthChanged) => {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "derived exact-Revision checkpoint moved before response completion",
            ));
        }
        Err(error) => return Ok(lifecycle_failure_outcome(error)),
    };
    if final_checkpoint.checkpoint_sha256 != checkpoint.checkpoint_sha256 {
        return Ok(DerivedChangeOutcomeV1::retryable(
            DerivedProjectionFailureCodeV1::ProjectionUnstable,
            "derived exact-Revision checkpoint changed before response completion",
        ));
    }

    Ok(DerivedChangeOutcomeV1::Ready(
        DerivedExactRevisionReadV1::new(
            prepared.view,
            prepared.document_projection,
            prepared.facade,
            prepared.stamp,
            results,
        ),
    ))
}

#[allow(clippy::too_many_arguments)]
fn prepare_results(
    snapshot: &ExactRevisionFactReadSnapshot,
    service: &DerivedAccessService,
    backend: &StoreBackend,
    repo: &Path,
    observed: TruthCursor,
    prepared: &super::change_seek_reads::PreparedNarrowedFacadeV1,
    plan: &ExactRevisionReadPlanV1,
    selection_started: &mut bool,
    hook: &mut impl FnMut(ExactRevisionReadBoundary),
) -> std::result::Result<BTreeMap<RevisionRefV1, RevisionShowResult>, ComponentReadFailure> {
    let mut planned = plan.revisions.iter().cloned().collect::<BTreeSet<_>>();
    if let Some(context) = &plan.fact_port_context {
        planned.insert(context.clone());
    }

    let mut results = BTreeMap::new();
    for revision in planned {
        let result = component_read(
            snapshot,
            service,
            backend,
            repo,
            observed,
            &revision,
            plan,
            selection_started,
        )?;
        results.insert(revision, result);
    }
    hook(ExactRevisionReadBoundary::BaseHydrated);

    let mut origins = BTreeSet::new();
    if let Some(base) = &plan.fact_port_context {
        let base_result = results.get(base).ok_or_else(|| {
            ComponentReadFailure::AfterSelection(
                "derived exact-Revision context result is absent".to_owned(),
            )
        })?;
        let (base_facts, _) = normalize_fact_presentations(base_result, base);
        let ports = prepared
            .facade
            .fact_port_presentations(&prepared.view.change_id, base)
            .map_err(|error| ComponentReadFailure::AfterSelection(error.to_string()))?;
        for port in ports {
            if port.applicability != FactPortApplicabilityV1::Applicable {
                continue;
            }
            if port.target_fact.as_ref().is_some_and(|target| {
                !base_facts.iter().any(|fact| {
                    fact.origin_revision == *base
                        && fact.fact_id == fact_ref_id(target)
                        && fact.family == fact_ref_family(target)
                })
            }) {
                continue;
            }
            if !results.contains_key(&port.origin_revision) {
                origins.insert(port.origin_revision);
            }
        }
    }
    hook(ExactRevisionReadBoundary::OriginsPlanned);

    for origin in origins {
        let result = component_read(
            snapshot,
            service,
            backend,
            repo,
            observed,
            &origin,
            plan,
            selection_started,
        )?;
        if result.revision.object_artifact_content_hash != origin.object_artifact_content_hash {
            continue;
        }
        results.insert(origin, result);
    }
    Ok(results)
}

#[allow(clippy::too_many_arguments)]
fn component_read(
    snapshot: &ExactRevisionFactReadSnapshot,
    service: &DerivedAccessService,
    backend: &StoreBackend,
    repo: &Path,
    observed: TruthCursor,
    revision: &RevisionRefV1,
    plan: &ExactRevisionReadPlanV1,
    selection_started: &mut bool,
) -> std::result::Result<RevisionShowResult, ComponentReadFailure> {
    #[cfg(any(test, feature = "longitudinal-counting"))]
    let selection_phase = enter_derived_access_phase_v1(Phase::RevisionDetailSqlSelection);
    let selected_event_ids = snapshot
        .revision_component_event_ids(&revision.revision_id, observed)
        .map_err(|error| ComponentReadFailure::BeforeSelection(error.to_string()))?;
    *selection_started = true;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    drop(selection_phase);

    #[cfg(any(test, feature = "longitudinal-counting"))]
    let selected_phase =
        enter_derived_access_phase_v1(Phase::RevisionDetailSelectedCarrierHydrationValidation);
    let selected = hydrate_events(service, &selected_event_ids, observed)
        .map_err(ComponentReadFailure::AfterSelection)?;
    validate_selected_component_events(&selected, &revision.revision_id)
        .map_err(ComponentReadFailure::AfterSelection)?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    drop(selected_phase);

    #[cfg(any(test, feature = "longitudinal-counting"))]
    let support_phase =
        enter_derived_access_phase_v1(Phase::RevisionDetailSupportCarrierHydrationValidation);
    let support_plan = support_event_plan(&snapshot.connection, &selected, observed)
        .map_err(ComponentReadFailure::AfterSelection)?;
    let support_event_ids = support_plan.all_event_ids();
    let support = hydrate_events(service, &support_event_ids, observed)
        .map_err(ComponentReadFailure::AfterSelection)?;
    validate_support_events(&support_plan, &selected, &support)
        .map_err(ComponentReadFailure::AfterSelection)?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    drop(support_phase);

    let mut events = selected;
    events.extend(support);
    normalize_events(&mut events);
    show_revision_from_selected_events(
        RevisionShowOptions::new(repo)
            .with_revision_id(revision.revision_id.clone())
            .with_exact(true)
            .with_include_body(plan.include_body)
            .with_read_for_display(plan.read_for_display),
        backend,
        events,
    )
    .map_err(|error| ComponentReadFailure::AfterSelection(error.to_string()))
}

fn fact_ref_id(fact: &FactRefV1) -> &str {
    match fact {
        FactRefV1::Observation { observation_id } => observation_id.as_str(),
        FactRefV1::InputRequest { input_request_id } => input_request_id.as_str(),
    }
}

fn fact_ref_family(fact: &FactRefV1) -> &'static str {
    match fact {
        FactRefV1::Observation { .. } => "observation",
        FactRefV1::InputRequest { .. } => "input_request",
    }
}
