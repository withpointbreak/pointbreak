//! Product attention reads over the active derived generation.

use super::history::{
    CurrentRead, DerivedHistoryAccess, DerivedHistoryStatus, catching_up_status, legacy_terminal,
    projection_stamp,
};
use super::locator::LocatorRead;
use crate::model::RevisionId;
use crate::session::workflow::attention::scope_attention_items;
use crate::session::{AttentionItem, ProjectionDiagnostic};

#[doc(hidden)]
pub enum DerivedAttentionRoute {
    Off,
    Ready(DerivedAttention),
    Unavailable(DerivedHistoryStatus),
}

#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct DerivedAttention {
    pub projection_stamp: String,
    pub event_count: usize,
    pub items: Vec<AttentionItem>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

/// Read boundary inside the legacy attention route where an authoritative
/// write can land between the truth-head observation and the semantic read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::session) enum LegacyAttentionReadBoundary {
    HeadObserved,
    SnapshotRead,
}

impl DerivedHistoryAccess {
    pub fn attention(
        &self,
        revision: Option<&RevisionId>,
    ) -> Result<DerivedAttentionRoute, String> {
        self.attention_inner(revision, |_| {})
    }

    #[cfg(test)]
    pub(in crate::session) fn attention_with_hook(
        &self,
        revision: Option<&RevisionId>,
        hook: impl FnMut(LegacyAttentionReadBoundary),
    ) -> Result<DerivedAttentionRoute, String> {
        self.attention_inner(revision, hook)
    }

    fn attention_inner(
        &self,
        revision: Option<&RevisionId>,
        mut hook: impl FnMut(LegacyAttentionReadBoundary),
    ) -> Result<DerivedAttentionRoute, String> {
        let Some((store_identity, _)) = self.active_context() else {
            return Ok(DerivedAttentionRoute::Off);
        };
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let _generation_phase =
            crate::bench_support::longitudinal::enter_derived_access_phase_v1(
                crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::GenerationLeaseAndRetention,
            );
        let current = match self.current()? {
            CurrentRead::Ready(current) => current,
            CurrentRead::Unavailable(status) => {
                return Ok(DerivedAttentionRoute::Unavailable(status));
            }
        };
        let snapshot = match current
            .service()
            .semantic_materialized_attention_snapshot_inner(|| {
                hook(LegacyAttentionReadBoundary::HeadObserved)
            })
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(snapshot) => snapshot,
            LocatorRead::CatchUpRequired { .. } => {
                return Ok(DerivedAttentionRoute::Unavailable(catching_up_status()));
            }
        };
        hook(LegacyAttentionReadBoundary::SnapshotRead);
        let as_of = snapshot.as_of;
        let mut items = snapshot.attention.items;
        if let Some(revision) = revision {
            scope_attention_items(&mut items, revision, &snapshot.supersession);
        }
        let outcome = projection_stamp(store_identity, as_of).map(|projection_stamp| {
            DerivedAttentionRoute::Ready(DerivedAttention {
                projection_stamp,
                event_count: snapshot.state.event_count,
                items,
                diagnostics: snapshot.attention.diagnostics,
            })
        });
        let route = legacy_terminal(current.service(), as_of, outcome, || {
            DerivedAttentionRoute::Unavailable(catching_up_status())
        })?;
        if matches!(route, DerivedAttentionRoute::Ready(_)) {
            super::threads::record_active_ownership();
        }
        Ok(route)
    }
}
