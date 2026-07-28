//! Qualification-only derived-access adapter.
#![cfg_attr(not(test), allow(dead_code))]

use std::path::Path;

#[cfg(feature = "longitudinal-counting")]
use super::sqlite_cursor::CursorLedgerInventory;
use super::sqlite_cursor::{CursorLedgerError, CursorLedgerIdentity, SqliteCursorLedger};
use super::sqlite_locator::{LocatorInventory, SqliteLocator, SqliteLocatorError};
use super::sqlite_semantic::{SemanticInventory, SqliteSemantic, SqliteSemanticError};
use crate::model::RevisionId;
use crate::session::derived_access::cursor::{
    AppendResolution, CursorDelta, TruthCursor, TruthHead,
};
use crate::session::derived_access::locator::{
    ChronologicalWindowRequest, HydratedWindow, LocatorModelError, LocatorRead, LocatorRow,
};
use crate::session::derived_access::semantic::state::{
    DerivedAccessFreshness, FreshnessModelError,
};
use crate::session::derived_access::semantic::{
    HydratedRevisionDetail, SemanticFact, SemanticModelError, SemanticSnapshot,
};
use crate::session::event::{ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload};

const DEFAULT_DELTA_LIMIT: usize = 512;

#[derive(Debug)]
pub(crate) struct QualificationDerivedAccessAdapter {
    cursor: SqliteCursorLedger,
    locator: SqliteLocator,
    semantic: SqliteSemantic,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DerivedAccessAdapterError {
    #[error(transparent)]
    Cursor(#[from] CursorLedgerError),
    #[error(transparent)]
    Locator(#[from] SqliteLocatorError),
    #[error(transparent)]
    Semantic(#[from] SqliteSemanticError),
    #[error(transparent)]
    SemanticModel(#[from] SemanticModelError),
    #[error(transparent)]
    LocatorModel(#[from] LocatorModelError),
    #[error(transparent)]
    Freshness(#[from] FreshnessModelError),
    #[error("authoritative truth read failed: {0}")]
    Truth(String),
    #[error("derived catch-up batch limit must be greater than zero")]
    ZeroBatchLimit,
    #[error("derived catch-up returned no receipts before observed head {0:?}")]
    EmptyIncompleteDelta(TruthCursor),
}

impl QualificationDerivedAccessAdapter {
    pub(crate) fn open(
        store_root: &Path,
        identity: CursorLedgerIdentity,
    ) -> Result<Self, DerivedAccessAdapterError> {
        let store_root = store_root
            .canonicalize()
            .map_err(|error| DerivedAccessAdapterError::Truth(error.to_string()))?;
        let cursor = SqliteCursorLedger::open(&store_root, identity)?;
        let locator = SqliteLocator::open(&store_root)?;
        let semantic = SqliteSemantic::open(locator.clone())?;
        Ok(Self {
            cursor,
            locator,
            semantic,
        })
    }

    pub(crate) fn append_event(
        &self,
        event: &ShoreEvent,
        attempt_token: &str,
    ) -> Result<AppendResolution, DerivedAccessAdapterError> {
        let resolution = self.cursor.append_event(event, attempt_token)?;
        self.catch_up_to_head(DEFAULT_DELTA_LIMIT)?;
        Ok(resolution)
    }

    pub(crate) fn catch_up_to_head(
        &self,
        batch_limit: usize,
    ) -> Result<TruthCursor, DerivedAccessAdapterError> {
        if batch_limit == 0 {
            return Err(DerivedAccessAdapterError::ZeroBatchLimit);
        }
        loop {
            let checkpoint = self.locator.checkpoint()?;
            let head = self.cursor.head()?.cursor;
            if checkpoint.applied == head {
                return Ok(head);
            }
            let hydrated = self
                .cursor
                .events_after_hydrated(checkpoint.applied, batch_limit)?;
            let delta = &hydrated.delta;
            if delta.receipts.is_empty() && !delta.complete {
                return Err(DerivedAccessAdapterError::EmptyIncompleteDelta(
                    delta.observed_head,
                ));
            }
            let (rows, semantic_facts) = self.derived_rows(delta, &hydrated.events)?;
            let applied = self.semantic.apply_delta(delta, &rows, &semantic_facts)?;
            if delta.complete {
                return Ok(applied);
            }
        }
    }

    pub(crate) fn freshness(&self) -> Result<DerivedAccessFreshness, DerivedAccessAdapterError> {
        let applied = self.locator.checkpoint()?.applied;
        let observed = self.cursor.head()?.cursor;
        Ok(DerivedAccessFreshness::between(applied, observed)?)
    }

    pub(crate) fn new_event_count(&self) -> Result<Option<u64>, DerivedAccessAdapterError> {
        Ok(self.freshness()?.new_event_count())
    }

    pub(crate) fn semantic_id(
        &self,
        event_id: &str,
    ) -> Result<LocatorRead<Option<ShoreEvent>>, DerivedAccessAdapterError> {
        let observed = self.cursor.head()?.cursor;
        match self.locator.lookup_event_id_hydrated(event_id, observed)? {
            LocatorRead::Ready(row) => Ok(LocatorRead::Ready(row.map(|row| row.event))),
            LocatorRead::CatchUpRequired { applied, observed } => {
                Ok(LocatorRead::CatchUpRequired { applied, observed })
            }
        }
    }

    pub(crate) fn chronological_window(
        &self,
        request: ChronologicalWindowRequest,
    ) -> Result<LocatorRead<HydratedWindow>, DerivedAccessAdapterError> {
        let observed = self.cursor.head()?.cursor;
        match self
            .locator
            .chronological_window_hydrated(&request, observed)?
        {
            LocatorRead::Ready(window) => Ok(LocatorRead::Ready(HydratedWindow {
                as_of: window.window.as_of,
                events: window.events,
                continuation: window.window.continuation,
                has_more: window.window.has_more,
            })),
            LocatorRead::CatchUpRequired { applied, observed } => {
                Ok(LocatorRead::CatchUpRequired { applied, observed })
            }
        }
    }

    pub(crate) fn truth_head(&self) -> Result<TruthHead, DerivedAccessAdapterError> {
        Ok(self.cursor.head()?)
    }

    pub(crate) fn locator_checkpoint(&self) -> Result<TruthCursor, DerivedAccessAdapterError> {
        Ok(self.locator.checkpoint()?.applied)
    }

    pub(crate) fn locator_inventory(&self) -> Result<LocatorInventory, DerivedAccessAdapterError> {
        Ok(self.locator.inventory()?)
    }

    #[cfg(feature = "longitudinal-counting")]
    pub(crate) fn cursor_inventory(
        &self,
    ) -> Result<CursorLedgerInventory, DerivedAccessAdapterError> {
        Ok(self.cursor.inventory()?)
    }

    pub(crate) fn semantic_inventory(
        &self,
    ) -> Result<SemanticInventory, DerivedAccessAdapterError> {
        Ok(self.semantic.inventory()?)
    }

    pub(crate) fn semantic_audit_snapshot(
        &self,
    ) -> Result<LocatorRead<SemanticSnapshot>, DerivedAccessAdapterError> {
        let observed = self.cursor.head()?.cursor;
        Ok(self.semantic.audit_snapshot(observed)?)
    }

    pub(crate) fn semantic_materialized_audit_snapshot(
        &self,
    ) -> Result<LocatorRead<SemanticSnapshot>, DerivedAccessAdapterError> {
        let observed = self.cursor.head()?.cursor;
        Ok(self.semantic.materialized_audit_snapshot(observed)?)
    }

    pub(crate) fn semantic_materialized_engagement_snapshot(
        &self,
        engagement_id: &str,
    ) -> Result<LocatorRead<SemanticSnapshot>, DerivedAccessAdapterError> {
        let observed = self.cursor.head()?.cursor;
        Ok(self
            .semantic
            .materialized_engagement_snapshot(engagement_id, observed)?)
    }

    pub(crate) fn revision_detail(
        &self,
        revision_id: &RevisionId,
    ) -> Result<LocatorRead<Option<HydratedRevisionDetail>>, DerivedAccessAdapterError> {
        let observed = self.cursor.head()?.cursor;
        let facts = match self
            .semantic
            .facts_for_revision_hydrated(revision_id.as_str(), observed)?
        {
            LocatorRead::Ready(facts) => facts,
            LocatorRead::CatchUpRequired { applied, observed } => {
                return Ok(LocatorRead::CatchUpRequired { applied, observed });
            }
        };
        if facts.is_empty() {
            return Ok(LocatorRead::Ready(None));
        }

        let mut authoritative_events = facts.into_iter().map(|fact| fact.event).collect::<Vec<_>>();
        authoritative_events.sort_by(|left, right| left.event_id.cmp(&right.event_id));
        let capture = authoritative_events
            .iter()
            .find(|event| event.event_type == crate::session::event::EventType::WorkObjectProposed)
            .and_then(|event| {
                serde_json::from_value::<WorkObjectProposedPayload>(event.payload.clone()).ok()
            })
            .and_then(|payload| match payload.work_object {
                WorkObjectProposal::Revision {
                    revision,
                    object_artifact_content_hash,
                    ..
                } if revision.id == *revision_id => Some(object_artifact_content_hash),
                _ => None,
            });
        let Some(object_content_hash) = capture else {
            return Ok(LocatorRead::Ready(None));
        };
        let object_content_removed = self
            .semantic
            .content_is_removed(&object_content_hash, observed)?;
        Ok(LocatorRead::Ready(Some(HydratedRevisionDetail {
            as_of: observed,
            revision_id: revision_id.clone(),
            object_content_hash,
            object_content_removed,
            authoritative_events,
        })))
    }

    pub(crate) fn catch_up_with_interruption(
        &self,
        batch_limit: usize,
    ) -> Result<TruthCursor, DerivedAccessAdapterError> {
        if batch_limit == 0 {
            return Err(DerivedAccessAdapterError::ZeroBatchLimit);
        }
        let checkpoint = self.locator.checkpoint()?;
        let hydrated = self
            .cursor
            .events_after_hydrated(checkpoint.applied, batch_limit)?;
        let delta = &hydrated.delta;
        let (rows, semantic_facts) = self.derived_rows(delta, &hydrated.events)?;
        Ok(self
            .semantic
            .apply_delta_with_failure(delta, &rows, &semantic_facts)?)
    }

    fn derived_rows(
        &self,
        delta: &CursorDelta,
        events: &[ShoreEvent],
    ) -> Result<(Vec<LocatorRow>, Vec<SemanticFact>), DerivedAccessAdapterError> {
        if events.len() != delta.receipts.len() {
            return Err(DerivedAccessAdapterError::Truth(format!(
                "{} authoritative events for {} cursor receipts",
                events.len(),
                delta.receipts.len()
            )));
        }
        let mut locator_rows = Vec::with_capacity(delta.receipts.len());
        let mut semantic_facts = Vec::with_capacity(delta.receipts.len());
        for (receipt, event) in delta.receipts.iter().zip(events) {
            locator_rows.push(LocatorRow::from_event(
                receipt.cursor,
                event,
                receipt.validation_witness.clone(),
            )?);
            semantic_facts.push(SemanticFact::from_event(
                receipt.cursor,
                event,
                receipt.validation_witness.clone(),
            )?);
        }
        Ok((locator_rows, semantic_facts))
    }
}
