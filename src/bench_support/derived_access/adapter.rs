//! Qualification-only derived-access adapter.
#![cfg_attr(not(test), allow(dead_code))]

use std::path::{Path, PathBuf};

use super::sqlite_cursor::{CursorLedgerError, CursorLedgerIdentity, SqliteCursorLedger};
use super::sqlite_locator::{LocatorInventory, SqliteLocator, SqliteLocatorError};
use super::sqlite_semantic::{SemanticInventory, SqliteSemantic, SqliteSemanticError};
use crate::canonical_hash::sha256_bytes_hex;
use crate::model::RevisionId;
use crate::session::EventStore;
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
    store_root: PathBuf,
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
    #[error("authoritative event does not match locator row at {0:?}")]
    LocatorMismatch(TruthCursor),
    #[error("authoritative event does not match semantic fact at {0:?}")]
    SemanticMismatch(TruthCursor),
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
            store_root,
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
            let delta = self.cursor.events_after(checkpoint.applied, batch_limit)?;
            if delta.receipts.is_empty() && !delta.complete {
                return Err(DerivedAccessAdapterError::EmptyIncompleteDelta(
                    delta.observed_head,
                ));
            }
            let (rows, semantic_facts) = self.derived_rows(&delta)?;
            let applied = self.semantic.apply_delta(&delta, &rows, &semantic_facts)?;
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
        match self.locator.lookup_event_id(event_id, observed)? {
            LocatorRead::Ready(row) => {
                let event = row.as_ref().map(|row| self.hydrate(row)).transpose()?;
                Ok(LocatorRead::Ready(event))
            }
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
        match self.locator.chronological_window(&request, observed)? {
            LocatorRead::Ready(window) => {
                let events = window
                    .rows
                    .iter()
                    .map(|row| self.hydrate(row))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(LocatorRead::Ready(HydratedWindow {
                    as_of: window.as_of,
                    events,
                    continuation: window.continuation,
                    has_more: window.has_more,
                }))
            }
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
            .facts_for_revision(revision_id.as_str(), observed)?
        {
            LocatorRead::Ready(facts) => facts,
            LocatorRead::CatchUpRequired { applied, observed } => {
                return Ok(LocatorRead::CatchUpRequired { applied, observed });
            }
        };
        if facts.is_empty() {
            return Ok(LocatorRead::Ready(None));
        }

        let mut authoritative_events = facts
            .iter()
            .map(|fact| self.hydrate_semantic(fact))
            .collect::<Result<Vec<_>, _>>()?;
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

    #[cfg(test)]
    pub(crate) fn catch_up_with_semantic_failure_for_test(
        &self,
        batch_limit: usize,
    ) -> Result<TruthCursor, DerivedAccessAdapterError> {
        if batch_limit == 0 {
            return Err(DerivedAccessAdapterError::ZeroBatchLimit);
        }
        let checkpoint = self.locator.checkpoint()?;
        let delta = self.cursor.events_after(checkpoint.applied, batch_limit)?;
        let (rows, semantic_facts) = self.derived_rows(&delta)?;
        Ok(self
            .semantic
            .apply_delta_with_failure(&delta, &rows, &semantic_facts)?)
    }

    fn derived_rows(
        &self,
        delta: &CursorDelta,
    ) -> Result<(Vec<LocatorRow>, Vec<SemanticFact>), DerivedAccessAdapterError> {
        let mut locator_rows = Vec::with_capacity(delta.receipts.len());
        let mut semantic_facts = Vec::with_capacity(delta.receipts.len());
        for receipt in &delta.receipts {
            let event = self.read_authoritative(&receipt.logical_reread_key)?;
            locator_rows.push(LocatorRow::from_event(
                receipt.cursor,
                &event,
                receipt.validation_witness.clone(),
            )?);
            semantic_facts.push(SemanticFact::from_event(
                receipt.cursor,
                &event,
                receipt.validation_witness.clone(),
            )?);
        }
        Ok((locator_rows, semantic_facts))
    }

    fn hydrate(&self, row: &LocatorRow) -> Result<ShoreEvent, DerivedAccessAdapterError> {
        let event = self.read_authoritative(&row.logical_reread_key)?;
        let witness = sha256_bytes_hex(
            &serde_json::to_vec(&event)
                .map_err(|error| DerivedAccessAdapterError::Truth(error.to_string()))?,
        );
        let observed = LocatorRow::from_event(row.cursor, &event, witness)?;
        if &observed != row {
            return Err(DerivedAccessAdapterError::LocatorMismatch(row.cursor));
        }
        Ok(event)
    }

    fn hydrate_semantic(
        &self,
        fact: &SemanticFact,
    ) -> Result<ShoreEvent, DerivedAccessAdapterError> {
        let event = self.read_authoritative(&fact.logical_reread_key)?;
        let witness = sha256_bytes_hex(
            &serde_json::to_vec(&event)
                .map_err(|error| DerivedAccessAdapterError::Truth(error.to_string()))?,
        );
        let observed = SemanticFact::from_event(fact.cursor, &event, witness)?;
        if &observed != fact {
            return Err(DerivedAccessAdapterError::SemanticMismatch(fact.cursor));
        }
        Ok(event)
    }

    fn read_authoritative(
        &self,
        logical_reread_key: &str,
    ) -> Result<ShoreEvent, DerivedAccessAdapterError> {
        EventStore::open(&self.store_root)
            .read_stored_event(logical_reread_key)
            .map_err(|error| DerivedAccessAdapterError::Truth(error.to_string()))
    }
}
