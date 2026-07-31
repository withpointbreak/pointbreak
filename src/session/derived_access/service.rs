//! Dormant product service for the SQLite-WAL bodyless derived-access profile.
#![cfg_attr(not(test), allow(dead_code))]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::product_contract::DerivedAccessProfile;
use super::sqlite::{
    AppendCrashPoint, CursorLedgerError, CursorLedgerIdentity, CursorLedgerInventory,
    LocatorInventory, ProductHistoryFact, SemanticInventory, SqliteCursorLedger, SqliteLocator,
    SqliteLocatorError, SqliteSemantic, SqliteSemanticError, StoreWriterLock,
};
use crate::error::Result as ShoreResult;
use crate::model::RevisionId;
use crate::session::EventWriteOutcome;
use crate::session::derived_access::cursor::{
    AppendResolution, CursorDelta, TruthAuthoritySnapshot, TruthCursor, TruthHead,
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
use crate::session::store::backend::JournalChangeStamp;

const DEFAULT_DELTA_LIMIT: usize = 512;
type DerivedRows = (Vec<LocatorRow>, Vec<SemanticFact>, Vec<ProductHistoryFact>);

#[derive(Debug)]
pub(crate) struct DerivedAccessService {
    cursor: SqliteCursorLedger,
    locator: SqliteLocator,
    semantic: SqliteSemantic,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DerivedAccessServiceError {
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

#[derive(Clone, Debug, Default)]
pub(crate) struct DerivedAccessIoProbe {
    counters: Arc<DerivedAccessIoCounters>,
}

#[derive(Debug, Default)]
struct DerivedAccessIoCounters {
    root_resolutions: AtomicU64,
    sqlite_physical_opens: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct DerivedAccessIoSnapshot {
    pub(crate) root_resolutions: u64,
    pub(crate) sqlite_physical_opens: u64,
}

impl DerivedAccessIoSnapshot {
    pub(crate) const fn total(self) -> u64 {
        self.root_resolutions + self.sqlite_physical_opens
    }
}

impl DerivedAccessIoProbe {
    pub(crate) fn snapshot(&self) -> DerivedAccessIoSnapshot {
        DerivedAccessIoSnapshot {
            root_resolutions: self.counters.root_resolutions.load(Ordering::Relaxed),
            sqlite_physical_opens: self.counters.sqlite_physical_opens.load(Ordering::Relaxed),
        }
    }

    fn record_root_resolution(&self) {
        self.counters
            .root_resolutions
            .fetch_add(1, Ordering::Relaxed);
    }

    fn record_sqlite_physical_open(&self) {
        self.counters
            .sqlite_physical_opens
            .fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub(crate) enum DerivedAccessHandle {
    Off,
    SqliteWalBodylessV1(Box<DerivedAccessService>),
}

impl DerivedAccessHandle {
    pub(crate) fn resolve(
        profile: DerivedAccessProfile,
        store_root: &Path,
        store_id: impl Into<String>,
        probe: DerivedAccessIoProbe,
    ) -> Result<Self, DerivedAccessServiceError> {
        match profile {
            DerivedAccessProfile::Off => Ok(Self::Off),
            DerivedAccessProfile::SqliteWalBodylessV1 => DerivedAccessService::open_observed(
                store_root,
                CursorLedgerIdentity::new(store_id),
                &probe,
            )
            .map(Box::new)
            .map(Self::SqliteWalBodylessV1),
        }
    }

    pub(crate) const fn profile(&self) -> DerivedAccessProfile {
        match self {
            Self::Off => DerivedAccessProfile::Off,
            Self::SqliteWalBodylessV1(_) => DerivedAccessProfile::SqliteWalBodylessV1,
        }
    }

    pub(crate) const fn active(&self) -> Option<&DerivedAccessService> {
        match self {
            Self::Off => None,
            Self::SqliteWalBodylessV1(service) => Some(service),
        }
    }
}

impl DerivedAccessService {
    pub(crate) fn open(
        store_root: &Path,
        identity: CursorLedgerIdentity,
    ) -> Result<Self, DerivedAccessServiceError> {
        Self::open_inner(store_root, None, identity, None)
    }

    pub(crate) fn open_at(
        store_root: &Path,
        sidecar_root: &Path,
        identity: CursorLedgerIdentity,
    ) -> Result<Self, DerivedAccessServiceError> {
        Self::open_inner(store_root, Some(sidecar_root), identity, None)
    }

    fn open_observed(
        store_root: &Path,
        identity: CursorLedgerIdentity,
        probe: &DerivedAccessIoProbe,
    ) -> Result<Self, DerivedAccessServiceError> {
        Self::open_inner(store_root, None, identity, Some(probe))
    }

    fn open_inner(
        store_root: &Path,
        sidecar_root: Option<&Path>,
        identity: CursorLedgerIdentity,
        probe: Option<&DerivedAccessIoProbe>,
    ) -> Result<Self, DerivedAccessServiceError> {
        let store_root = store_root
            .canonicalize()
            .map_err(|error| DerivedAccessServiceError::Truth(error.to_string()))?;
        if let Some(probe) = probe {
            probe.record_root_resolution();
        }
        let cursor = match sidecar_root {
            Some(sidecar_root) => {
                SqliteCursorLedger::open_immutable_at(&store_root, sidecar_root, identity)?
            }
            None => SqliteCursorLedger::open(&store_root, identity)?,
        };
        if let Some(probe) = probe {
            probe.record_sqlite_physical_open();
        }
        let locator = match sidecar_root {
            Some(sidecar_root) => SqliteLocator::open_at(&store_root, sidecar_root)?,
            None => SqliteLocator::open(&store_root)?,
        };
        if let Some(probe) = probe {
            probe.record_sqlite_physical_open();
        }
        let semantic = SqliteSemantic::open(locator.clone())?;
        if let Some(probe) = probe {
            probe.record_sqlite_physical_open();
        }
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
    ) -> Result<AppendResolution, DerivedAccessServiceError> {
        let resolution = self.cursor.append_event(event, attempt_token)?;
        self.catch_up_to_head(DEFAULT_DELTA_LIMIT)?;
        Ok(resolution)
    }

    pub(crate) fn append_event_with_publisher_locked(
        &self,
        event: &ShoreEvent,
        attempt_token: &str,
        writer_lock: &StoreWriterLock,
        hook: impl FnMut(AppendCrashPoint),
        publish: impl FnOnce() -> ShoreResult<EventWriteOutcome>,
    ) -> Result<AppendResolution, DerivedAccessServiceError> {
        Ok(self.cursor.append_event_with_publisher_locked(
            event,
            attempt_token,
            writer_lock,
            hook,
            publish,
        )?)
    }

    pub(crate) fn catch_up_to_head(
        &self,
        batch_limit: usize,
    ) -> Result<TruthCursor, DerivedAccessServiceError> {
        if batch_limit == 0 {
            return Err(DerivedAccessServiceError::ZeroBatchLimit);
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
                return Err(DerivedAccessServiceError::EmptyIncompleteDelta(
                    delta.observed_head,
                ));
            }
            let (rows, semantic_facts, product_history_facts) =
                self.derived_rows(delta, &hydrated.events)?;
            let applied =
                self.semantic
                    .apply_delta(delta, &rows, &semantic_facts, &product_history_facts)?;
            if delta.complete {
                return Ok(applied);
            }
        }
    }

    pub(crate) fn freshness(&self) -> Result<DerivedAccessFreshness, DerivedAccessServiceError> {
        let applied = self.locator.checkpoint()?.applied;
        let observed = self.cursor.head()?.cursor;
        Ok(DerivedAccessFreshness::between(applied, observed)?)
    }

    pub(crate) fn new_event_count(&self) -> Result<Option<u64>, DerivedAccessServiceError> {
        Ok(self.freshness()?.new_event_count())
    }

    pub(crate) fn semantic_id(
        &self,
        event_id: &str,
    ) -> Result<LocatorRead<Option<ShoreEvent>>, DerivedAccessServiceError> {
        let observed = self.cursor.head()?.cursor;
        match self.locator.lookup_event_id_hydrated(event_id, observed)? {
            LocatorRead::Ready(row) => Ok(LocatorRead::Ready(row.map(|row| row.event))),
            LocatorRead::CatchUpRequired { applied, observed } => {
                Ok(LocatorRead::CatchUpRequired { applied, observed })
            }
        }
    }

    /// Hydrate an ordered event-id set against one already-selected truth
    /// cursor. The locator owns the one-connection batch and validates every
    /// carrier individually; callers never trade correctness for fewer opens.
    pub(crate) fn semantic_ids_at(
        &self,
        event_ids: &[String],
        observed: TruthCursor,
    ) -> Result<LocatorRead<Vec<Option<ShoreEvent>>>, DerivedAccessServiceError> {
        Ok(
            match self
                .locator
                .lookup_event_ids_hydrated(event_ids, observed)?
            {
                LocatorRead::Ready(rows) => LocatorRead::Ready(
                    rows.into_iter()
                        .map(|row| row.map(|row| row.event))
                        .collect(),
                ),
                LocatorRead::CatchUpRequired { applied, observed } => {
                    LocatorRead::CatchUpRequired { applied, observed }
                }
            },
        )
    }

    pub(super) fn product_history_connection(
        &self,
    ) -> Result<
        LocatorRead<(
            rusqlite::Connection,
            crate::session::derived_access::semantic::state::SemanticStateSnapshot,
        )>,
        DerivedAccessServiceError,
    > {
        let observed = self.cursor.head()?.cursor;
        Ok(self.semantic.product_history_connection(observed)?)
    }

    pub(crate) fn chronological_window(
        &self,
        request: ChronologicalWindowRequest,
    ) -> Result<LocatorRead<HydratedWindow>, DerivedAccessServiceError> {
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

    pub(crate) fn truth_head(&self) -> Result<TruthHead, DerivedAccessServiceError> {
        Ok(self.cursor.head()?)
    }

    pub(crate) fn truth_authority_snapshot(
        &self,
    ) -> Result<TruthAuthoritySnapshot, DerivedAccessServiceError> {
        Ok(self.cursor.authority_snapshot()?)
    }

    pub(crate) fn bind_truth_authority_stamp_locked(
        &self,
        expected: TruthCursor,
        authority_stamp: &JournalChangeStamp,
        writer_lock: &StoreWriterLock,
    ) -> Result<(), DerivedAccessServiceError> {
        Ok(self
            .cursor
            .bind_authority_stamp_locked(expected, authority_stamp, writer_lock)?)
    }

    pub(crate) fn locator_checkpoint(&self) -> Result<TruthCursor, DerivedAccessServiceError> {
        Ok(self.locator.checkpoint()?.applied)
    }

    pub(crate) fn locator_inventory(&self) -> Result<LocatorInventory, DerivedAccessServiceError> {
        Ok(self.locator.inventory()?)
    }

    pub(crate) fn cursor_inventory(
        &self,
    ) -> Result<CursorLedgerInventory, DerivedAccessServiceError> {
        Ok(self.cursor.inventory()?)
    }

    pub(crate) fn semantic_inventory(
        &self,
    ) -> Result<SemanticInventory, DerivedAccessServiceError> {
        Ok(self.semantic.inventory()?)
    }

    pub(crate) fn semantic_audit_snapshot(
        &self,
    ) -> Result<LocatorRead<SemanticSnapshot>, DerivedAccessServiceError> {
        let observed = self.cursor.head()?.cursor;
        Ok(self.semantic.audit_snapshot(observed)?)
    }

    pub(crate) fn semantic_materialized_audit_snapshot(
        &self,
    ) -> Result<LocatorRead<SemanticSnapshot>, DerivedAccessServiceError> {
        let observed = self.cursor.head()?.cursor;
        Ok(self.semantic.materialized_audit_snapshot(observed)?)
    }

    pub(crate) fn semantic_materialized_attention_snapshot(
        &self,
    ) -> Result<
        LocatorRead<crate::session::derived_access::semantic::MaterializedAttentionSnapshot>,
        DerivedAccessServiceError,
    > {
        let observed = self.cursor.head()?.cursor;
        Ok(self.semantic.materialized_attention_snapshot(observed)?)
    }

    pub(crate) fn semantic_materialized_engagement_snapshot(
        &self,
        engagement_id: &str,
    ) -> Result<LocatorRead<SemanticSnapshot>, DerivedAccessServiceError> {
        let observed = self.cursor.head()?.cursor;
        Ok(self
            .semantic
            .materialized_engagement_snapshot(engagement_id, observed)?)
    }

    pub(crate) fn revision_detail(
        &self,
        revision_id: &RevisionId,
    ) -> Result<LocatorRead<Option<HydratedRevisionDetail>>, DerivedAccessServiceError> {
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
    ) -> Result<TruthCursor, DerivedAccessServiceError> {
        if batch_limit == 0 {
            return Err(DerivedAccessServiceError::ZeroBatchLimit);
        }
        let checkpoint = self.locator.checkpoint()?;
        let hydrated = self
            .cursor
            .events_after_hydrated(checkpoint.applied, batch_limit)?;
        let delta = &hydrated.delta;
        let (rows, semantic_facts, product_history_facts) =
            self.derived_rows(delta, &hydrated.events)?;
        Ok(self.semantic.apply_delta_with_failure(
            delta,
            &rows,
            &semantic_facts,
            &product_history_facts,
        )?)
    }

    fn derived_rows(
        &self,
        delta: &CursorDelta,
        events: &[ShoreEvent],
    ) -> Result<DerivedRows, DerivedAccessServiceError> {
        if events.len() != delta.receipts.len() {
            return Err(DerivedAccessServiceError::Truth(format!(
                "{} authoritative events for {} cursor receipts",
                events.len(),
                delta.receipts.len()
            )));
        }
        let mut locator_rows = Vec::with_capacity(delta.receipts.len());
        let mut semantic_facts = Vec::with_capacity(delta.receipts.len());
        let mut product_history_facts = Vec::with_capacity(delta.receipts.len());
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
            product_history_facts.push(ProductHistoryFact::from_event(
                receipt.cursor.sequence,
                event,
            )?);
        }
        Ok((locator_rows, semantic_facts, product_history_facts))
    }
}
