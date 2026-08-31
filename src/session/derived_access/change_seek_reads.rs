//! Derived per-Change seek read producer.
//!
//! Answers `change show`, `change interdiff`, and captured `change select`
//! from the materialized fact tables through one Change-scoped seek: the
//! correlated-sequence selection narrows the fact rows to the target Change,
//! the existing folds rebuild the narrowed projections, and the existing
//! facade composes the documents. No authoritative carrier is opened, no
//! event is decoded, no body or presentation is hydrated, and no eager
//! complete-Change scan runs on this path.
//!
//! Response closure: the correlated-sequence seek keys on each fact's own
//! revision, but the authoritative Change fold clears operative obligations
//! by request identity over the global stream, so a response bound to a
//! foreign revision (issue #726) or to no revision at all (issue #723 —
//! writable only by pre-2026-07 builds or peer ingest; the current writer
//! always reconstructs the request's revision) would never be selected and
//! its obligation would survive on this lane alone. After the correlated
//! seek, the snapshot therefore collects the selected requests' identities
//! and unions in every response answering them — read-side and
//! order-independent, so existing generations are fixed without rebuild.
//!
//! Residual Timeline-lane gaps (issues #723/#726 — the closure fixes this
//! seek, not the Timeline relation): `inspect event-history --change <C>`
//! still does not list a foreign-revision response under the Change whose
//! obligation it clears, and a revision-less response stays entirely absent
//! from the Timeline relation (its subject-less shape is deliberately kept
//! out of `product_history_event`). The intended long-term endpoint is a
//! completed correlation index built at materialization time (recorded on
//! both issues), at which point the closure statements become pure no-ops
//! and are removed. The eager page reads are unaffected (they scan every
//! Change fact row).
#![cfg_attr(not(test), allow(dead_code))]

use super::changes::{
    DerivedChangeAccess, DerivedChangeOutcomeV1, DerivedChangeSeekV1,
    DerivedProjectionFailureCodeV1, lifecycle_failure_outcome,
};
use super::lifecycle::{CurrentGeneration, LifecycleError};
use super::locator::LocatorRead;
use super::runtime::RuntimeCurrentRead;
use super::semantic::change::ReaderProjectionCheckpointV1;
#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::{
    LongitudinalDerivedAccessPhaseV1 as Phase, enter_derived_access_phase_v1,
};
use crate::documents::{ChangeDetailDocumentV1, ChangeDocumentFacadeV1, FactPortCarrierSourceV1};
use crate::error::{Result, ShoreError};
use crate::model::ChangeId;
use crate::session::projection::change::{
    ChangeProjectionFact, project_change_documents_from_facts, project_changes_from_facts,
};
use crate::session::{ChangeDocumentProjectionV1, ChangeProjection, ChangeView};

/// Which composed carrier a seek read produces.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChangeSeekCompositionTarget {
    Detail,
    Selector,
}

/// One composed seek answer, matched exhaustively by the thin delegates.
pub(crate) enum PreparedChangeSeek {
    Detail(Box<ChangeDetailDocumentV1>),
    Selector(Box<DerivedChangeSeekV1>),
}

pub(crate) struct PreparedNarrowedFacadeV1 {
    pub(crate) view: ChangeView,
    pub(crate) document_projection: ChangeDocumentProjectionV1,
    pub(crate) facade: ChangeDocumentFacadeV1,
    pub(crate) stamp: String,
}

/// Observable read boundaries for deterministic drift tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChangeSeekReadBoundary {
    SnapshotPinned,
    Composed,
}

/// The narrowed fold must stay inside the seek scope: any foreign Change view
/// is cross-Change leakage, and selected rows without the target view mean
/// the correlation and the fold disagree. Both fail closed — never a partial
/// answer.
pub(crate) fn validate_narrowed_seek_scope(
    change_id: &ChangeId,
    narrowed: &ChangeProjection,
    selected_fact_rows: usize,
) -> std::result::Result<(), String> {
    if let Some(foreign) = narrowed.changes.keys().find(|key| *key != change_id) {
        return Err(format!(
            "derived Change seek folded foreign Change {}",
            foreign.as_str()
        ));
    }
    if selected_fact_rows > 0 && !narrowed.changes.contains_key(change_id) {
        return Err(
            "derived Change seek selected fact rows without the target Change view".to_owned(),
        );
    }
    Ok(())
}

/// Seek, fold, validate scope, mint the seek stamp, and compose the narrowed
/// facade with its fact-port carriers at one pinned checkpoint.
pub(crate) fn prepare_narrowed_facade(
    current: &CurrentGeneration,
    checkpoint: &ReaderProjectionCheckpointV1,
    change_id: &ChangeId,
) -> Result<DerivedChangeOutcomeV1<PreparedNarrowedFacadeV1>> {
    let as_of = checkpoint.truth_cursor;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    let selection_phase = enter_derived_access_phase_v1(Phase::ChangeSeekCorrelatedSelection);
    let facts = match current
        .service()
        .semantic_change_seek_facts_at(change_id, as_of)
    {
        Ok(LocatorRead::Ready(facts)) => facts,
        Ok(LocatorRead::CatchUpRequired { .. }) => {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionStale,
                "derived Change seek moved while its checkpoint was pinned",
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
    drop(selection_phase);

    #[cfg(any(test, feature = "longitudinal-counting"))]
    let fold_phase = enter_derived_access_phase_v1(Phase::ChangeSeekProjectionFold);
    let narrowed_semantic = match project_changes_from_facts(
        &facts
            .iter()
            .map(|fact| fact.change.clone())
            .collect::<Vec<_>>(),
    ) {
        Ok(projection) => projection,
        Err(error) => {
            return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                error.to_string(),
            ));
        }
    };
    let narrowed_document = match project_change_documents_from_facts(&facts) {
        Ok(projection) => projection,
        Err(error) => {
            return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                error.to_string(),
            ));
        }
    };
    if let Err(message) = validate_narrowed_seek_scope(change_id, &narrowed_semantic, facts.len()) {
        return Ok(DerivedChangeOutcomeV1::projection_unavailable(
            DerivedProjectionFailureCodeV1::ProjectionInvalid,
            message,
        ));
    }
    #[cfg(any(test, feature = "longitudinal-counting"))]
    drop(fold_phase);

    // An unknown or malformed Change id is a lookup miss with zero correlated
    // rows: surface the authoritative refusal so every caller's fallback lane
    // answers with the identical bytes on the identical path.
    let Some(view) = narrowed_semantic.changes.get(change_id).cloned() else {
        return Err(ShoreError::Message(format!(
            "Change {} is unavailable",
            change_id.as_str()
        )));
    };

    let sources = facts
        .iter()
        .filter_map(|fact| match &fact.change {
            ChangeProjectionFact::FactPort { port } => Some((port, &fact.support)),
            _ => None,
        })
        .map(|(port, support)| {
            support
                .track_id
                .clone()
                .map(|track_id| FactPortCarrierSourceV1 {
                    payload: port.clone(),
                    event_id: support.event_id.clone(),
                    actor_id: support.actor_id.clone(),
                    track_id,
                })
                .ok_or_else(|| "materialized fact port carries no review track".to_owned())
        })
        .collect::<std::result::Result<Vec<_>, String>>();
    let sources = match sources {
        Ok(sources) => sources,
        Err(message) => {
            return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                message,
            ));
        }
    };

    #[cfg(any(test, feature = "longitudinal-counting"))]
    let composition_phase = enter_derived_access_phase_v1(Phase::ChangeSeekComposition);
    let stamp = match current.change_seek_stamp(
        checkpoint,
        change_id,
        &narrowed_semantic,
        &narrowed_document,
    ) {
        Ok(stamp) => stamp,
        Err(error) => return Ok(lifecycle_failure_outcome(error)),
    };
    let facade = match ChangeDocumentFacadeV1::new(narrowed_semantic, narrowed_document.clone())
        .and_then(|facade| facade.with_generation_stamp(stamp.clone()))
        .and_then(|facade| facade.with_fact_port_sources(sources))
    {
        Ok(facade) => facade,
        Err(error) => {
            return Ok(DerivedChangeOutcomeV1::projection_unavailable(
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                error.to_string(),
            ));
        }
    };
    #[cfg(any(test, feature = "longitudinal-counting"))]
    drop(composition_phase);

    Ok(DerivedChangeOutcomeV1::Ready(PreparedNarrowedFacadeV1 {
        view,
        document_projection: narrowed_document,
        facade,
        stamp,
    }))
}

pub(crate) fn change_seek_read_v1_inner(
    access: &DerivedChangeAccess,
    change_id: &ChangeId,
    target: ChangeSeekCompositionTarget,
) -> Result<DerivedChangeOutcomeV1<PreparedChangeSeek>> {
    change_seek_read_v1_inner_with_hook(access, change_id, target, |_| {})
}

pub(crate) fn change_seek_read_v1_inner_with_hook(
    access: &DerivedChangeAccess,
    change_id: &ChangeId,
    target: ChangeSeekCompositionTarget,
    mut hook: impl FnMut(ChangeSeekReadBoundary),
) -> Result<DerivedChangeOutcomeV1<PreparedChangeSeek>> {
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
    hook(ChangeSeekReadBoundary::SnapshotPinned);
    #[cfg(any(test, feature = "longitudinal-counting"))]
    drop(snapshot_phase);

    let narrowed = match prepare_narrowed_facade(&current, &checkpoint, change_id)? {
        DerivedChangeOutcomeV1::Ready(prepared) => prepared,
        DerivedChangeOutcomeV1::AuthorityUnavailable(document) => {
            return Ok(DerivedChangeOutcomeV1::AuthorityUnavailable(document));
        }
        DerivedChangeOutcomeV1::AuthorityConflicted(document) => {
            return Ok(DerivedChangeOutcomeV1::AuthorityConflicted(document));
        }
        DerivedChangeOutcomeV1::AuthorityInvalid(document) => {
            return Ok(DerivedChangeOutcomeV1::AuthorityInvalid(document));
        }
        DerivedChangeOutcomeV1::ReaderUpgradeRequired(document) => {
            return Ok(DerivedChangeOutcomeV1::ReaderUpgradeRequired(document));
        }
        DerivedChangeOutcomeV1::ProjectionUnavailable(document) => {
            return Ok(DerivedChangeOutcomeV1::ProjectionUnavailable(document));
        }
        DerivedChangeOutcomeV1::Retryable(document) => {
            return Ok(DerivedChangeOutcomeV1::Retryable(document));
        }
    };
    let prepared = match target {
        ChangeSeekCompositionTarget::Detail => {
            PreparedChangeSeek::Detail(Box::new(narrowed.facade.detail_document(change_id)?))
        }
        ChangeSeekCompositionTarget::Selector => PreparedChangeSeek::Selector(Box::new(
            DerivedChangeSeekV1::new(narrowed.view, narrowed.document_projection, narrowed.stamp),
        )),
    };
    hook(ChangeSeekReadBoundary::Composed);

    let final_current = match access.runtime().current() {
        Ok(RuntimeCurrentRead::Ready(current)) => current,
        Ok(RuntimeCurrentRead::Unavailable(_)) | Err(_) => {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "derived Change seek moved before response completion",
            ));
        }
    };
    if final_current.generation_id() != generation_id {
        return Ok(DerivedChangeOutcomeV1::retryable(
            DerivedProjectionFailureCodeV1::ProjectionUnstable,
            "derived Change seek generation changed before response completion",
        ));
    }
    let final_checkpoint = match final_current.pin_change_reader_checkpoint() {
        Ok(checkpoint) => checkpoint,
        Err(LifecycleError::TruthChanged) => {
            return Ok(DerivedChangeOutcomeV1::retryable(
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "derived Change seek checkpoint moved before response completion",
            ));
        }
        Err(error) => return Ok(lifecycle_failure_outcome(error)),
    };
    if final_checkpoint.checkpoint_sha256 != checkpoint.checkpoint_sha256 {
        return Ok(DerivedChangeOutcomeV1::retryable(
            DerivedProjectionFailureCodeV1::ProjectionUnstable,
            "derived Change seek checkpoint changed before response completion",
        ));
    }
    Ok(DerivedChangeOutcomeV1::Ready(prepared))
}
