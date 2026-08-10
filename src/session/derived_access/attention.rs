//! Product attention reads over the active derived generation.

use super::history::{
    CurrentRead, DerivedHistoryAccess, DerivedHistoryStatus, catching_up_status, projection_stamp,
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

impl DerivedHistoryAccess {
    pub fn attention(
        &self,
        revision: Option<&RevisionId>,
    ) -> Result<DerivedAttentionRoute, String> {
        let Some((store_identity, _)) = self.active_context() else {
            return Ok(DerivedAttentionRoute::Off);
        };
        let current = match self.current()? {
            CurrentRead::Ready(current) => current,
            CurrentRead::Unavailable(status) => {
                return Ok(DerivedAttentionRoute::Unavailable(status));
            }
        };
        let snapshot = match current
            .service()
            .semantic_materialized_attention_snapshot()
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(snapshot) => snapshot,
            LocatorRead::CatchUpRequired { .. } => {
                return Ok(DerivedAttentionRoute::Unavailable(catching_up_status()));
            }
        };
        let mut items = snapshot.attention.items;
        if let Some(revision) = revision {
            scope_attention_items(&mut items, revision, &snapshot.supersession);
        }
        super::threads::record_active_ownership();
        Ok(DerivedAttentionRoute::Ready(DerivedAttention {
            projection_stamp: projection_stamp(store_identity, snapshot.as_of)?,
            event_count: snapshot.state.event_count,
            items,
            diagnostics: snapshot.attention.diagnostics,
        }))
    }
}
