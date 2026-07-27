//! Qualification-only derived-access adapter.
#![cfg_attr(not(test), allow(dead_code))]

use std::path::{Path, PathBuf};

use super::sqlite_cursor::{CursorLedgerError, CursorLedgerIdentity, SqliteCursorLedger};
use super::sqlite_locator::{LocatorInventory, SqliteLocator, SqliteLocatorError};
use crate::canonical_hash::sha256_bytes_hex;
use crate::session::EventStore;
use crate::session::derived_access::cursor::{AppendResolution, TruthCursor, TruthHead};
use crate::session::derived_access::locator::{
    ChronologicalWindowRequest, HydratedWindow, LocatorModelError, LocatorRead, LocatorRow,
};
use crate::session::derived_access::semantic::state::{
    DerivedAccessFreshness, FreshnessModelError,
};
use crate::session::event::ShoreEvent;

const DEFAULT_DELTA_LIMIT: usize = 512;

#[derive(Debug)]
pub(crate) struct QualificationDerivedAccessAdapter {
    store_root: PathBuf,
    cursor: SqliteCursorLedger,
    locator: SqliteLocator,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DerivedAccessAdapterError {
    #[error(transparent)]
    Cursor(#[from] CursorLedgerError),
    #[error(transparent)]
    Locator(#[from] SqliteLocatorError),
    #[error(transparent)]
    LocatorModel(#[from] LocatorModelError),
    #[error(transparent)]
    Freshness(#[from] FreshnessModelError),
    #[error("authoritative truth read failed: {0}")]
    Truth(String),
    #[error("authoritative event does not match locator row at {0:?}")]
    LocatorMismatch(TruthCursor),
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
        Ok(Self {
            store_root,
            cursor,
            locator,
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
            let rows = delta
                .receipts
                .iter()
                .map(|receipt| {
                    let event = self.read_authoritative(&receipt.logical_reread_key)?;
                    LocatorRow::from_event(
                        receipt.cursor,
                        &event,
                        receipt.validation_witness.clone(),
                    )
                    .map_err(Into::into)
                })
                .collect::<Result<Vec<_>, DerivedAccessAdapterError>>()?;
            let applied = self.locator.apply_delta(&delta, &rows)?.applied;
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

    fn read_authoritative(
        &self,
        logical_reread_key: &str,
    ) -> Result<ShoreEvent, DerivedAccessAdapterError> {
        EventStore::open(&self.store_root)
            .read_stored_event(logical_reread_key)
            .map_err(|error| DerivedAccessAdapterError::Truth(error.to_string()))
    }
}
