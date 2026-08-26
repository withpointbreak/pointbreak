//! Product history and freshness reads over the derived-access profile.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

use rusqlite::OptionalExtension;
use rusqlite::types::Value;
use serde::Serialize;

use super::cursor::TruthCursor;
use super::interaction::{AUTHORITATIVE_FALLBACK_HINT, claim_unavailable_hint};
use super::layout::{
    DerivedStorageDiscovery, DerivedStorageLayout, DerivedStorageNamespace,
    DerivedStorageTransition,
};
use super::lifecycle::{
    CurrentGeneration, DerivedAccessLifecycle, LifecycleControl, LifecycleProgress,
};
use super::locator::{LocatorRead, normalize_occurred_at};
use super::product_contract::{DerivedAccessAvailability, DerivedAccessProfile};
pub(super) use super::runtime::DerivedAccessMode as DerivedHistoryMode;
use super::runtime::{
    DerivedAccessMaintenance as DerivedHistoryMaintenance, DerivedAccessRuntime, RuntimeCurrentRead,
};
use super::support::support_event_ids;
use crate::canonical_hash::sha256_json_prefixed;
use crate::session::ProjectionDiagnostic;
use crate::session::derived_access::semantic::state::SemanticStateSnapshot;
use crate::session::event::ShoreEvent;
use crate::session::store::backend::StoreBackend;
use crate::session::store::resolution::{
    activated_store_capability_for_repo, resolve_read_store_with_derived_access_profile,
};
use crate::session::workflow::{
    BaseProjectionConfig, DistinctValues, HistoryCursor, HistoryOrder, HistoryPage, HistoryQuery,
    QueryDiagnostic, ReviewHistoryEntry, history_entries_from_selected_events,
};

const PRODUCT_HISTORY_SCHEMA_V5: &str = "pointbreak.sqlite-derived-access-history.v5";
const PROJECTION_STAMP_SCHEMA_V1: &str = "pointbreak.derived-access-projection-stamp.v1";
const ACTIVE_PROFILE: &str = "sqlite-wal-bodyless-v1";

#[cfg(test)]
pub(crate) fn product_history_stamp_schema() -> &'static str {
    PRODUCT_HISTORY_SCHEMA_V5
}
const REVIEW_EVENT_CTE: &str = "
WITH revision_object_ranked AS (
    SELECT event.revision_id, revision.object_id,
           row_number() OVER (
               PARTITION BY event.revision_id
               ORDER BY locator.normalized_occurred_at DESC, locator.event_id DESC
           ) AS rank
    FROM semantic_revision_fact AS revision
    JOIN semantic_event_fact_text AS event ON event.sequence = revision.sequence
    JOIN locator_event_text AS locator ON locator.sequence = revision.sequence
),
revision_object AS (
    SELECT revision_id, object_id
    FROM revision_object_ranked
    WHERE rank = 1
),
review_event AS (
    SELECT locator.sequence, locator.event_id, locator.normalized_occurred_at,
           locator.event_type, locator.track_id, event.revision_id, event.actor_id,
           revision_object.object_id
    FROM locator_event_text AS locator
    JOIN semantic_event_fact_text AS event ON event.sequence = locator.sequence
    LEFT JOIN revision_object ON revision_object.revision_id = event.revision_id
    WHERE locator.event_type NOT IN (
        'task_checkpoint_captured',
        'task_observation_recorded',
        'event_signature_recorded',
        'artifact_removed',
        'change_declared',
        'change_membership_asserted',
        'change_membership_withdrawn',
        'change_link_asserted',
        'change_revision_relation_asserted',
        'change_revision_relation_withdrawn',
        'revision_relation_attested',
        'review_fact_ported'
    )
      AND (
          event.revision_id IS NOT NULL
          OR locator.event_type NOT IN (
              'work_object_proposed',
              'input_request_opened',
              'input_request_responded'
          )
      )
)";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[doc(hidden)]
pub enum DerivedHistoryAvailability {
    Absent,
    Bootstrapping,
    Current,
    CatchingUp,
    RebuildRequired,
    Quarantined,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[doc(hidden)]
pub struct DerivedHistoryStatus {
    pub availability: DerivedHistoryAvailability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[doc(hidden)]
pub enum DerivedHistoryProgressPhase {
    CursorPopulation,
    ProjectionPopulation,
    StrictVerification,
    Finalizing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[doc(hidden)]
pub struct DerivedHistoryLifecycleStatus {
    pub active: bool,
    pub availability: DerivedHistoryAvailability,
    pub namespace: DerivedHistoryNamespace,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<DerivedHistoryProgressPhase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_events: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_events: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_milliseconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_milliseconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub rebuild_in_flight: bool,
    pub rebuild_paused: bool,
    #[serde(skip)]
    pub conflict_paths: Option<DerivedHistoryConflictPaths>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[doc(hidden)]
pub enum DerivedHistoryNamespace {
    Absent,
    Stable,
    Legacy,
    Conflict,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub struct DerivedHistoryConflictPaths {
    pub stable: PathBuf,
    pub legacy: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[doc(hidden)]
pub enum DerivedHistoryTransition {
    NotNeeded,
    Deferred,
    Moved,
    Conflict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum DerivedHistoryControl {
    Continue,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub struct DerivedHistoryProgress {
    pub phase: DerivedHistoryProgressPhase,
    pub completed_events: usize,
    pub total_events: usize,
    pub completed_bytes: u64,
    pub elapsed_milliseconds: u64,
    pub eta_milliseconds: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[doc(hidden)]
pub struct DerivedHistoryLifecycleReceipt {
    pub availability: DerivedHistoryAvailability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_sequence: Option<u64>,
    pub rebuilt: bool,
    pub transition: DerivedHistoryTransition,
    pub moved_artifact_count: usize,
    pub reclaimed_generation_count: usize,
    pub retained_reader_generation_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[doc(hidden)]
pub enum DerivedHistoryRoute<T> {
    Off,
    Ready(T),
    ExhaustiveSearchFallback,
    Unavailable(DerivedHistoryStatus),
}

#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct DerivedHistoryPage {
    pub projection_stamp: String,
    pub event_count: usize,
    pub entries: Vec<ReviewHistoryEntry>,
    pub facets: BTreeMap<String, usize>,
    pub match_count: usize,
    pub offset: usize,
    pub match_index: Option<usize>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
    pub query_notices: Vec<QueryDiagnostic>,
    pub distinct_values: DistinctValues,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub struct DerivedHistoryNewCount {
    pub projection_stamp: String,
    pub new_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub struct DerivedHistoryFreshness {
    pub projection_stamp: String,
    pub event_count: u64,
}

#[doc(hidden)]
pub struct DerivedHistoryAccess {
    runtime: Arc<DerivedAccessRuntime>,
}

impl DerivedHistoryAccess {
    pub(super) fn from_mode(mode: DerivedHistoryMode) -> Self {
        Self {
            runtime: DerivedAccessRuntime::from_mode(mode),
        }
    }

    pub(crate) fn from_runtime(runtime: Arc<DerivedAccessRuntime>) -> Self {
        Self { runtime }
    }

    pub(super) fn active_context(&self) -> Option<(&str, &StoreBackend)> {
        self.runtime.active_context()
    }

    pub(super) fn lifecycle(&self) -> Option<&DerivedAccessLifecycle> {
        self.runtime.lifecycle()
    }

    fn rebuild_in_flight(&self) -> bool {
        self.runtime.rebuild_in_flight()
    }

    #[cfg(test)]
    fn maintenance_in_flight(&self) -> bool {
        self.runtime.maintenance_in_flight()
    }

    fn rebuild_paused(&self) -> bool {
        self.runtime.rebuild_paused()
    }

    #[cfg(test)]
    fn rebuild_worker_joined(&self) -> bool {
        self.runtime.rebuild_worker_joined()
    }

    #[cfg(test)]
    pub(in crate::session::derived_access) fn pause_background_worker_for_test(&self) {
        self.runtime.pause_background_worker_for_test();
    }

    pub fn resolve(repo: impl AsRef<Path>) -> Result<Self, String> {
        let profile =
            DerivedAccessProfile::from_environment().map_err(|error| error.to_string())?;
        Self::resolve_with_profile(repo.as_ref(), profile)
    }

    pub(crate) fn from_public_read_store(
        read_store: crate::session::store::resolution::ReadStore,
    ) -> Result<Self, String> {
        Ok(Self::from_runtime(DerivedAccessRuntime::from_read_store(
            read_store,
        )?))
    }

    fn resolve_with_profile(repo: &Path, profile: DerivedAccessProfile) -> Result<Self, String> {
        if profile == DerivedAccessProfile::Off {
            return Ok(Self::from_mode(DerivedHistoryMode::Off));
        }
        let read_store = resolve_read_store_with_derived_access_profile(repo, profile)
            .map_err(|error| error.to_string())?;
        Ok(Self::from_runtime(DerivedAccessRuntime::from_read_store(
            read_store,
        )?))
    }

    /// Resolve the legacy derived-history service for a mixed-cohort Inspector.
    ///
    /// Once a Change/Revision capability activation exists, legacy aggregate
    /// routes are unavailable by contract and the Inspector gates them before
    /// dispatch. Keeping this service explicitly off prevents its event-only
    /// bootstrap from blocking the Change-capable server or mutating a stale
    /// pre-activation sidecar. The Change reader retains its own complete,
    /// capability-validated authority path. An explicit off profile retains
    /// highest precedence and returns before repository capability discovery.
    #[doc(hidden)]
    pub fn resolve_for_inspector(repo: impl AsRef<Path>) -> Result<Self, String> {
        let profile =
            DerivedAccessProfile::from_environment().map_err(|error| error.to_string())?;
        Self::resolve_for_inspector_with_profile(repo.as_ref(), profile)
    }

    fn resolve_for_inspector_with_profile(
        repo: &Path,
        profile: DerivedAccessProfile,
    ) -> Result<Self, String> {
        if profile == DerivedAccessProfile::Off {
            return Ok(Self::from_mode(DerivedHistoryMode::Off));
        }
        if activated_store_capability_for_repo(repo)
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Ok(Self::from_mode(DerivedHistoryMode::Off));
        }
        Self::resolve_with_profile(repo, profile)
    }

    pub fn is_active(&self) -> bool {
        self.runtime.is_active()
    }

    /// Claim the shared process-local fallback hint for this exact store.
    ///
    /// Returning `None` means this access is explicitly off or another read or
    /// write already surfaced the same recovery action for this store.
    #[doc(hidden)]
    pub fn claim_authoritative_fallback_hint(&self) -> Option<&'static str> {
        let store_root = &self.runtime.maintenance()?.store_root;
        claim_unavailable_hint(store_root).then_some(AUTHORITATIVE_FALLBACK_HINT)
    }

    /// Report lifecycle progress without requiring a current generation.
    ///
    /// This is the Inspector's recovery-plane read: it never opens a derived
    /// generation and remains available while a first or replacement build is
    /// staging. The normal data routes still decide independently whether a
    /// validated old generation can be served.
    #[doc(hidden)]
    pub fn lifecycle_status(&self) -> DerivedHistoryLifecycleStatus {
        if let Some(maintenance) = self.runtime.maintenance() {
            return maintenance.status_read_only(self.rebuild_in_flight(), self.rebuild_paused());
        }
        let Some(lifecycle) = self.runtime.lifecycle() else {
            return DerivedHistoryLifecycleStatus {
                active: false,
                availability: DerivedHistoryAvailability::Absent,
                namespace: DerivedHistoryNamespace::Absent,
                generation_id: None,
                phase: None,
                completed_events: None,
                total_events: None,
                completed_bytes: None,
                elapsed_milliseconds: None,
                eta_milliseconds: None,
                detail: Some("derived access is disabled".to_owned()),
                rebuild_in_flight: false,
                rebuild_paused: false,
                conflict_paths: None,
            };
        };
        match lifecycle.status_read_only() {
            Ok(observed) => DerivedHistoryLifecycleStatus {
                active: true,
                availability: map_availability(observed.availability),
                namespace: namespace_for_layout(lifecycle.paths()),
                generation_id: observed.generation_id,
                phase: observed.phase.map(map_progress_phase),
                completed_events: observed.completed,
                total_events: observed.total,
                completed_bytes: observed.bytes_processed,
                elapsed_milliseconds: observed.elapsed_ms,
                eta_milliseconds: observed.estimated_remaining_ms,
                detail: observed.detail,
                rebuild_in_flight: self.rebuild_in_flight(),
                rebuild_paused: self.rebuild_paused(),
                conflict_paths: None,
            },
            Err(error) => DerivedHistoryLifecycleStatus {
                active: true,
                availability: DerivedHistoryAvailability::Unavailable,
                namespace: namespace_for_layout(lifecycle.paths()),
                generation_id: None,
                phase: None,
                completed_events: None,
                total_events: None,
                completed_bytes: None,
                elapsed_milliseconds: None,
                eta_milliseconds: None,
                detail: Some(error.to_string()),
                rebuild_in_flight: self.rebuild_in_flight(),
                rebuild_paused: self.rebuild_paused(),
                conflict_paths: None,
            },
        }
    }

    /// Build a missing or unusable generation synchronously. A current
    /// generation is returned unchanged.
    #[doc(hidden)]
    pub fn build(
        &self,
        progress: impl FnMut(DerivedHistoryProgress) -> DerivedHistoryControl,
    ) -> Result<DerivedHistoryLifecycleReceipt, String> {
        self.run_lifecycle(false, progress)
    }

    /// Publish a replacement generation synchronously while preserving any
    /// previously valid generation until completion.
    #[doc(hidden)]
    pub fn rebuild(
        &self,
        progress: impl FnMut(DerivedHistoryProgress) -> DerivedHistoryControl,
    ) -> Result<DerivedHistoryLifecycleReceipt, String> {
        self.run_lifecycle(true, progress)
    }

    fn run_lifecycle(
        &self,
        force: bool,
        mut progress: impl FnMut(DerivedHistoryProgress) -> DerivedHistoryControl,
    ) -> Result<DerivedHistoryLifecycleReceipt, String> {
        let maintenance = self
            .runtime
            .maintenance()
            .ok_or_else(|| "derived-access lifecycle is disabled".to_owned())?;
        if maintenance.profile == DerivedAccessProfile::Off {
            return Err("derived-access lifecycle is disabled".to_owned());
        }
        let transition = DerivedStorageLayout::transition_legacy(&maintenance.store_root)
            .map_err(|error| error.to_string())?;
        let mapped_transition = map_transition(transition.disposition);
        if matches!(
            transition.disposition,
            DerivedStorageTransition::Deferred | DerivedStorageTransition::Conflict
        ) {
            let status = maintenance.status_read_only(false, false);
            return Ok(DerivedHistoryLifecycleReceipt {
                availability: status.availability,
                generation_id: status.generation_id,
                head_sequence: None,
                rebuilt: false,
                transition: mapped_transition,
                moved_artifact_count: 0,
                reclaimed_generation_count: 0,
                retained_reader_generation_count: 0,
                detail: status.detail,
            });
        }

        // The transition may have changed the selected namespace. Construct a
        // fresh lifecycle only after it releases both namespace lock sets.
        let lifecycle = maintenance.lifecycle()?;
        if !force {
            let status = lifecycle.status().map_err(|error| error.to_string())?;
            if status.availability == DerivedAccessAvailability::Current {
                let head_sequence = lifecycle
                    .open_current()
                    .map_err(|error| error.to_string())?
                    .and_then(|current| current.service().locator_checkpoint().ok())
                    .map(|cursor| cursor.sequence);
                return Ok(DerivedHistoryLifecycleReceipt {
                    availability: DerivedHistoryAvailability::Current,
                    generation_id: status.generation_id,
                    head_sequence,
                    rebuilt: false,
                    transition: mapped_transition,
                    moved_artifact_count: transition.moved_artifacts.len(),
                    reclaimed_generation_count: 0,
                    retained_reader_generation_count: 0,
                    detail: status.detail,
                });
            }
        }

        let receipt = lifecycle
            .rebuild(|update| match progress(map_lifecycle_progress(update)) {
                DerivedHistoryControl::Continue => LifecycleControl::Continue,
                DerivedHistoryControl::Cancel => LifecycleControl::Cancel,
            })
            .map_err(|error| error.to_string())?;
        Ok(DerivedHistoryLifecycleReceipt {
            availability: map_availability(receipt.availability),
            generation_id: receipt.generation_id,
            head_sequence: Some(receipt.head_sequence),
            rebuilt: true,
            transition: mapped_transition,
            moved_artifact_count: transition.moved_artifacts.len(),
            reclaimed_generation_count: receipt.reclaimed_generation_count,
            retained_reader_generation_count: receipt.retained_reader_generation_count,
            detail: receipt.reclaim_detail,
        })
    }

    /// Whether an active data route can truthfully serve a validated current
    /// generation now. During a replacement build this may remain true even
    /// though lifecycle availability is `bootstrapping`.
    #[doc(hidden)]
    pub fn current_readable(&self) -> bool {
        self.is_active() && matches!(self.current(), Ok(CurrentRead::Ready(_)))
    }

    /// Start rebuilding a non-current active profile without delaying the
    /// caller. The lifecycle's store-scoped lease deduplicates concurrent
    /// Inspector processes; readers continue to report the typed lifecycle
    /// state until the immutable generation is published.
    #[doc(hidden)]
    pub fn start_background_rebuild(&self) -> Result<(), String> {
        self.runtime.start_background_rebuild()
    }

    /// Cooperatively cancel and join this process's rebuild worker.
    ///
    /// Explicit cancellation stays latched so status and data-route discovery
    /// cannot immediately start a replacement worker. Only an explicit retry
    /// clears the latch.
    ///
    /// Cancellation is observed at bounded bootstrap progress boundaries and
    /// between retry waits. Once the completion-last publication critical
    /// section begins, publication wins and the join returns the new Current
    /// state rather than abandoning a half-published generation.
    #[doc(hidden)]
    pub fn cancel_background_rebuild(&self) -> Result<(), String> {
        self.runtime.cancel_background_rebuild()
    }

    /// Cancel any local worker, join it, and start one fresh lifecycle attempt.
    #[doc(hidden)]
    pub fn restart_background_rebuild(&self) -> Result<(), String> {
        self.runtime.restart_background_rebuild()
    }

    pub fn history(
        &self,
        query: &HistoryQuery,
        page: &HistoryPage,
        config: &BaseProjectionConfig,
    ) -> Result<DerivedHistoryRoute<DerivedHistoryPage>, String> {
        let Some((store_identity, backend)) = self.runtime.active_context() else {
            return Ok(DerivedHistoryRoute::Off);
        };
        if !query.q.trim().is_empty() {
            return Ok(DerivedHistoryRoute::ExhaustiveSearchFallback);
        }
        let current = match self.current()? {
            CurrentRead::Ready(current) => current,
            CurrentRead::Unavailable(status) => {
                return Ok(DerivedHistoryRoute::Unavailable(status));
            }
        };
        let service = current.service();
        let (connection, state) = match service
            .product_history_connection()
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(context) => context,
            LocatorRead::CatchUpRequired { .. } => {
                return Ok(DerivedHistoryRoute::Unavailable(catching_up_status()));
            }
        };
        let as_of = service
            .locator_checkpoint()
            .map_err(|error| error.to_string())?;
        let selection = select_history_rows(&connection, query, page)?;
        let selected = hydrate_events(service, &selection.event_ids, as_of)?;
        let support_ids = support_event_ids(&connection, &selected, as_of)?;
        let mut support = selected.clone();
        support.extend(hydrate_events(service, &support_ids, as_of)?);
        support.sort_by(|left, right| left.event_id.cmp(&right.event_id));
        support.dedup_by(|left, right| left.event_id == right.event_id);
        let (entries, body_diagnostics) =
            history_entries_from_selected_events(&selected, &support, config, backend)
                .map_err(|error| error.to_string())?;
        let mut diagnostics = state_diagnostics(&state)?;
        diagnostics.extend(body_diagnostics);
        record_active_ownership(&entries);
        Ok(DerivedHistoryRoute::Ready(DerivedHistoryPage {
            projection_stamp: projection_stamp(store_identity, as_of)?,
            event_count: state.event_count,
            entries,
            facets: selection.facets,
            match_count: selection.match_count,
            offset: selection.offset,
            match_index: selection.match_index,
            diagnostics,
            query_notices: Vec::new(),
            distinct_values: selection.distinct_values,
        }))
    }

    pub fn new_count(
        &self,
        query: &HistoryQuery,
        since: &HistoryCursor,
    ) -> Result<DerivedHistoryRoute<DerivedHistoryNewCount>, String> {
        let Some((store_identity, _)) = self.runtime.active_context() else {
            return Ok(DerivedHistoryRoute::Off);
        };
        if !query.q.trim().is_empty() {
            return Ok(DerivedHistoryRoute::ExhaustiveSearchFallback);
        }
        let current = match self.current()? {
            CurrentRead::Ready(current) => current,
            CurrentRead::Unavailable(status) => {
                return Ok(DerivedHistoryRoute::Unavailable(status));
            }
        };
        let service = current.service();
        let (connection, _) = match service
            .product_history_connection()
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(context) => context,
            LocatorRead::CatchUpRequired { .. } => {
                return Ok(DerivedHistoryRoute::Unavailable(catching_up_status()));
            }
        };
        let as_of = service
            .locator_checkpoint()
            .map_err(|error| error.to_string())?;
        let new_count = count_new_rows(&connection, query, since)?;
        Ok(DerivedHistoryRoute::Ready(DerivedHistoryNewCount {
            projection_stamp: projection_stamp(store_identity, as_of)?,
            new_count,
        }))
    }

    pub fn freshness(&self) -> Result<DerivedHistoryRoute<DerivedHistoryFreshness>, String> {
        let Some((store_identity, _)) = self.runtime.active_context() else {
            return Ok(DerivedHistoryRoute::Off);
        };
        let current = match self.current()? {
            CurrentRead::Ready(current) => current,
            CurrentRead::Unavailable(status) => {
                return Ok(DerivedHistoryRoute::Unavailable(status));
            }
        };
        let service = current.service();
        let observed = service
            .truth_head()
            .map_err(|error| error.to_string())?
            .cursor;
        let as_of = service
            .locator_checkpoint()
            .map_err(|error| error.to_string())?;
        if as_of != observed {
            return Ok(DerivedHistoryRoute::Unavailable(catching_up_status()));
        }
        Ok(DerivedHistoryRoute::Ready(DerivedHistoryFreshness {
            projection_stamp: projection_stamp(store_identity, as_of)?,
            event_count: as_of.sequence,
        }))
    }

    pub(super) fn current(&self) -> Result<CurrentRead, String> {
        match self.runtime.current()? {
            RuntimeCurrentRead::Ready(current) => Ok(CurrentRead::Ready(current)),
            RuntimeCurrentRead::Unavailable(status) => {
                Ok(CurrentRead::Unavailable(DerivedHistoryStatus {
                    availability: map_availability(status.availability),
                    detail: status.detail,
                }))
            }
        }
    }
}

impl DerivedHistoryMaintenance {
    fn status_read_only(
        &self,
        rebuild_in_flight: bool,
        rebuild_paused: bool,
    ) -> DerivedHistoryLifecycleStatus {
        let discovery = DerivedStorageLayout::discover(&self.store_root);
        let DerivedStorageDiscovery::Selected(layout) = discovery else {
            let DerivedStorageDiscovery::Conflict { stable, legacy } = discovery else {
                unreachable!("derived storage discovery is exhaustive")
            };
            return DerivedHistoryLifecycleStatus {
                active: true,
                availability: DerivedHistoryAvailability::Unavailable,
                namespace: DerivedHistoryNamespace::Conflict,
                generation_id: None,
                phase: None,
                completed_events: None,
                total_events: None,
                completed_bytes: None,
                elapsed_milliseconds: None,
                eta_milliseconds: None,
                detail: Some(
                    "both stable and legacy derived-access roots exist; move one disposable root aside or select explicit off"
                        .to_owned(),
                ),
                rebuild_in_flight,
                rebuild_paused,
                conflict_paths: Some(DerivedHistoryConflictPaths {
                    stable: stable.root(),
                    legacy: legacy.root(),
                }),
            };
        };
        let namespace = map_namespace(layout.namespace());
        match self.lifecycle().and_then(|lifecycle| {
            lifecycle
                .status_read_only()
                .map_err(|error| error.to_string())
        }) {
            Ok(observed) => DerivedHistoryLifecycleStatus {
                active: true,
                availability: map_availability(observed.availability),
                namespace,
                generation_id: observed.generation_id,
                phase: observed.phase.map(map_progress_phase),
                completed_events: observed.completed,
                total_events: observed.total,
                completed_bytes: observed.bytes_processed,
                elapsed_milliseconds: observed.elapsed_ms,
                eta_milliseconds: observed.estimated_remaining_ms,
                detail: observed.detail,
                rebuild_in_flight,
                rebuild_paused,
                conflict_paths: None,
            },
            Err(error) => DerivedHistoryLifecycleStatus {
                active: true,
                availability: DerivedHistoryAvailability::Unavailable,
                namespace,
                generation_id: None,
                phase: None,
                completed_events: None,
                total_events: None,
                completed_bytes: None,
                elapsed_milliseconds: None,
                eta_milliseconds: None,
                detail: Some(error),
                rebuild_in_flight,
                rebuild_paused,
                conflict_paths: None,
            },
        }
    }
}

pub(super) enum CurrentRead {
    Ready(Arc<CurrentGeneration>),
    Unavailable(DerivedHistoryStatus),
}

struct HistorySelection {
    event_ids: Vec<String>,
    facets: BTreeMap<String, usize>,
    match_count: usize,
    offset: usize,
    match_index: Option<usize>,
    distinct_values: DistinctValues,
}

fn select_history_rows(
    connection: &rusqlite::Connection,
    query: &HistoryQuery,
    page: &HistoryPage,
) -> Result<HistorySelection, String> {
    let (page_predicate, page_parameters) = history_predicate(query, true);
    let (facet_predicate, facet_parameters) = history_predicate(query, false);
    let match_count = query_count(connection, &page_predicate, &page_parameters)?;
    let facets = query_facets(connection, &facet_predicate, &facet_parameters)?;
    let distinct_values = query_distinct_values(connection)?;
    let (offset, match_index, at_absent) = resolve_history_offset(
        connection,
        query,
        page,
        &page_predicate,
        &page_parameters,
        match_count,
    )?;
    if at_absent {
        return Ok(HistorySelection {
            event_ids: Vec::new(),
            facets,
            match_count,
            offset: 0,
            match_index: None,
            distinct_values,
        });
    }
    let event_ids = query_page_ids(
        connection,
        query,
        page,
        &page_predicate,
        &page_parameters,
        offset,
        match_count,
    )?;
    Ok(HistorySelection {
        event_ids,
        facets,
        match_count,
        offset,
        match_index,
        distinct_values,
    })
}

fn history_predicate(query: &HistoryQuery, include_types: bool) -> (String, Vec<Value>) {
    let mut predicates = Vec::new();
    let mut parameters = Vec::new();
    if let Some(track) = &query.track {
        predicates.push("lower(coalesce(track_id, '')) = lower(?)".to_owned());
        parameters.push(track.clone().into());
    }
    if let Some(snapshot) = &query.snapshot {
        predicates.push("object_id = ?".to_owned());
        parameters.push(snapshot.clone().into());
    }
    if let Some(revision) = &query.revision {
        predicates.push("revision_id = ?".to_owned());
        parameters.push(revision.as_str().to_owned().into());
    }
    if let Some(revisions) = &query.revisions {
        push_set_predicate(
            &mut predicates,
            &mut parameters,
            "revision_id",
            revisions
                .iter()
                .map(|revision| revision.as_str().to_owned()),
        );
    }
    if include_types && let Some(types) = &query.types {
        push_set_predicate(
            &mut predicates,
            &mut parameters,
            "event_type",
            types.iter().cloned(),
        );
    }
    if predicates.is_empty() {
        ("1 = 1".to_owned(), parameters)
    } else {
        (predicates.join(" AND "), parameters)
    }
}

fn push_set_predicate(
    predicates: &mut Vec<String>,
    parameters: &mut Vec<Value>,
    column: &str,
    values: impl IntoIterator<Item = String>,
) {
    let values = values.into_iter().collect::<Vec<_>>();
    if values.is_empty() {
        predicates.push("0 = 1".to_owned());
        return;
    }
    predicates.push(format!(
        "{column} IN ({})",
        std::iter::repeat_n("?", values.len())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    parameters.extend(values.into_iter().map(Value::from));
}

fn query_count(
    connection: &rusqlite::Connection,
    predicate: &str,
    parameters: &[Value],
) -> Result<usize, String> {
    query_selected_count(
        connection,
        REVIEW_EVENT_CTE,
        "review_event",
        predicate,
        parameters,
    )
}

fn query_facets(
    connection: &rusqlite::Connection,
    predicate: &str,
    parameters: &[Value],
) -> Result<BTreeMap<String, usize>, String> {
    query_selected_facets(
        connection,
        REVIEW_EVENT_CTE,
        "review_event",
        predicate,
        parameters,
    )
}

/// Shared count primitive for the legacy and Change-aware product-history
/// selectors. Callers own predicate compilation; this function owns the one
/// count shape and checked integer conversion.
pub(super) fn query_selected_count(
    connection: &rusqlite::Connection,
    cte: &str,
    relation: &str,
    predicate: &str,
    parameters: &[Value],
) -> Result<usize, String> {
    let sql = format!("{cte} SELECT count(*) FROM {relation} WHERE {predicate}");
    query_count_sql(connection, &sql, parameters)
}

/// Shared event-type facet primitive. The caller supplies the predicate with
/// only the URL event-type set removed, preserving the public facet contract.
pub(super) fn query_selected_facets(
    connection: &rusqlite::Connection,
    cte: &str,
    relation: &str,
    predicate: &str,
    parameters: &[Value],
) -> Result<BTreeMap<String, usize>, String> {
    let sql = format!(
        "{cte}
         SELECT event_type, count(*)
         FROM {relation}
         WHERE {predicate}
         GROUP BY event_type
         ORDER BY event_type"
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let mut facets = BTreeMap::new();
    for row in rows {
        let (event_type, count) = row.map_err(|error| error.to_string())?;
        facets.insert(
            event_type,
            usize::try_from(count).map_err(|_| "negative facet count".to_owned())?,
        );
    }
    Ok(facets)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SelectedHistoryKey {
    pub(super) occurred_at: String,
    pub(super) event_id: String,
}

/// Resolve one selected event key from the same predicate snapshot.
pub(super) fn query_selected_key(
    connection: &rusqlite::Connection,
    cte: &str,
    relation: &str,
    predicate: &str,
    parameters: &[Value],
    event_id: &str,
) -> Result<Option<SelectedHistoryKey>, String> {
    let sql = format!(
        "{cte}
         SELECT normalized_occurred_at, event_id
         FROM {relation}
         WHERE {predicate} AND event_id = ?"
    );
    let mut values = parameters.to_vec();
    values.push(event_id.to_owned().into());
    connection
        .query_row(&sql, rusqlite::params_from_iter(values.iter()), |row| {
            Ok(SelectedHistoryKey {
                occurred_at: row.get(0)?,
                event_id: row.get(1)?,
            })
        })
        .optional()
        .map_err(|error| error.to_string())
}

/// Count rows that precede `key` in one normalized selected order.
#[allow(clippy::too_many_arguments)]
pub(super) fn query_selected_index(
    connection: &rusqlite::Connection,
    cte: &str,
    relation: &str,
    predicate: &str,
    parameters: &[Value],
    key: &SelectedHistoryKey,
    descending: bool,
    inclusive: bool,
) -> Result<usize, String> {
    let (occurred_comparison, event_comparison) = match (descending, inclusive) {
        (false, false) => ("<", "<"),
        (false, true) => ("<", "<="),
        (true, false) => (">", ">"),
        (true, true) => (">", ">="),
    };
    let sql = format!(
        "{cte}
         SELECT count(*) FROM {relation}
         WHERE {predicate}
           AND (
               normalized_occurred_at {occurred_comparison} ?
               OR (
                   normalized_occurred_at = ?
                   AND event_id {event_comparison} ?
               )
           )"
    );
    let mut values = parameters.to_vec();
    values.extend([
        key.occurred_at.clone().into(),
        key.occurred_at.clone().into(),
        key.event_id.clone().into(),
    ]);
    query_count_sql(connection, &sql, &values)
}

/// Select one normalized window from a caller-supplied predicate. The same
/// primitive backs both product History and Change-aware Timeline paging.
#[allow(clippy::too_many_arguments)]
pub(super) fn query_selected_window(
    connection: &rusqlite::Connection,
    cte: &str,
    relation: &str,
    predicate: &str,
    parameters: &[Value],
    descending: bool,
    limit: usize,
    offset: usize,
) -> Result<Vec<SelectedHistoryKey>, String> {
    let direction = if descending { "DESC" } else { "ASC" };
    let sql = format!(
        "{cte}
         SELECT normalized_occurred_at, event_id
         FROM {relation}
         WHERE {predicate}
         ORDER BY normalized_occurred_at {direction}, event_id {direction}
         LIMIT ? OFFSET ?"
    );
    let mut values = parameters.to_vec();
    values.extend([
        to_sql_integer(limit)?.into(),
        to_sql_integer(offset)?.into(),
    ]);
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(values.iter()), |row| {
            Ok(SelectedHistoryKey {
                occurred_at: row.get(0)?,
                event_id: row.get(1)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn query_distinct_values(connection: &rusqlite::Connection) -> Result<DistinctValues, String> {
    fn strings(connection: &rusqlite::Connection, sql: &str) -> Result<Vec<String>, String> {
        let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }
    Ok(DistinctValues {
        track: strings(
            connection,
            &format!(
                "{REVIEW_EVENT_CTE}
                 SELECT DISTINCT lower(track_id)
                 FROM review_event
                 WHERE track_id IS NOT NULL AND track_id != ''
                 ORDER BY lower(track_id)"
            ),
        )?,
        actor: strings(
            connection,
            &format!(
                "{REVIEW_EVENT_CTE}
                 SELECT DISTINCT lower(actor_id)
                 FROM review_event
                 WHERE actor_id != ''
                 ORDER BY lower(actor_id)"
            ),
        )?,
        tag: strings(
            connection,
            "SELECT DISTINCT tag_key FROM product_history_tag ORDER BY tag_key",
        )?,
    })
}

fn resolve_history_offset(
    connection: &rusqlite::Connection,
    query: &HistoryQuery,
    page: &HistoryPage,
    predicate: &str,
    parameters: &[Value],
    match_count: usize,
) -> Result<(usize, Option<usize>, bool), String> {
    if let Some(after) = &page.after {
        if query.order == HistoryOrder::Desc {
            return Err("descending history does not support continuation cursors".to_owned());
        }
        let occurred_at = normalized_history_cursor(after);
        let sql = format!(
            "{REVIEW_EVENT_CTE}
             SELECT count(*) FROM review_event
             WHERE {predicate}
               AND (
                   normalized_occurred_at < ?
                   OR (normalized_occurred_at = ? AND event_id <= ?)
               )"
        );
        let mut before_parameters = parameters.to_vec();
        before_parameters.extend([
            occurred_at.clone().into(),
            occurred_at.into(),
            after.event_id.as_str().to_owned().into(),
        ]);
        return Ok((
            query_count_sql(connection, &sql, &before_parameters)?,
            None,
            false,
        ));
    }
    let Some(at) = &page.at else {
        return Ok((page.offset.unwrap_or(0).min(match_count), None, false));
    };
    let sql = format!(
        "{REVIEW_EVENT_CTE}
         SELECT normalized_occurred_at, event_id
         FROM review_event
         WHERE {predicate} AND event_id = ?"
    );
    let mut target_parameters = parameters.to_vec();
    target_parameters.push(at.as_str().to_owned().into());
    let target = connection
        .query_row(
            &sql,
            rusqlite::params_from_iter(target_parameters.iter()),
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((occurred_at, event_id)) = target else {
        return Ok((0, None, true));
    };
    let comparison = match query.order {
        HistoryOrder::Asc => {
            "(normalized_occurred_at < ? OR \
              (normalized_occurred_at = ? AND event_id < ?))"
        }
        HistoryOrder::Desc => {
            "(normalized_occurred_at > ? OR \
              (normalized_occurred_at = ? AND event_id > ?))"
        }
    };
    let count_sql = format!(
        "{REVIEW_EVENT_CTE}
         SELECT count(*) FROM review_event
         WHERE {predicate} AND {comparison}"
    );
    let mut count_parameters = parameters.to_vec();
    count_parameters.extend([
        occurred_at.clone().into(),
        occurred_at.into(),
        event_id.into(),
    ]);
    let index = query_count_sql(connection, &count_sql, &count_parameters)?;
    let offset = match page.limit {
        Some(0) => 0,
        Some(limit) => (index / limit) * limit,
        None => 0,
    };
    Ok((offset, Some(index), false))
}

fn query_count_sql(
    connection: &rusqlite::Connection,
    sql: &str,
    parameters: &[Value],
) -> Result<usize, String> {
    let count = connection
        .query_row(sql, rusqlite::params_from_iter(parameters.iter()), |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| error.to_string())?;
    usize::try_from(count).map_err(|_| "negative history count".to_owned())
}

fn query_page_ids(
    connection: &rusqlite::Connection,
    query: &HistoryQuery,
    page: &HistoryPage,
    predicate: &str,
    parameters: &[Value],
    offset: usize,
    match_count: usize,
) -> Result<Vec<String>, String> {
    let mut page_predicate = predicate.to_owned();
    let mut page_parameters = parameters.to_vec();
    if let Some(after) = &page.after {
        if query.order == HistoryOrder::Desc {
            return Err("descending history does not support continuation cursors".to_owned());
        }
        let occurred_at = normalized_history_cursor(after);
        page_predicate.push_str(
            " AND (normalized_occurred_at > ? OR \
             (normalized_occurred_at = ? AND event_id > ?))",
        );
        page_parameters.extend([
            occurred_at.clone().into(),
            occurred_at.into(),
            after.event_id.as_str().to_owned().into(),
        ]);
    }
    let selected_offset = if page.after.is_some() { 0 } else { offset };
    let limit = page
        .limit
        .unwrap_or_else(|| match_count.saturating_sub(selected_offset))
        .min(match_count);
    query_selected_window(
        connection,
        REVIEW_EVENT_CTE,
        "review_event",
        &page_predicate,
        &page_parameters,
        query.order == HistoryOrder::Desc,
        limit,
        selected_offset,
    )
    .map(|rows| rows.into_iter().map(|row| row.event_id).collect())
}

fn count_new_rows(
    connection: &rusqlite::Connection,
    query: &HistoryQuery,
    since: &HistoryCursor,
) -> Result<usize, String> {
    let (predicate, mut parameters) = history_predicate(query, true);
    let occurred_at = normalized_history_cursor(since);
    let sql = format!(
        "{REVIEW_EVENT_CTE}
         SELECT count(*)
         FROM review_event
         WHERE {predicate}
           AND (
               normalized_occurred_at > ?
               OR (normalized_occurred_at = ? AND event_id > ?)
           )"
    );
    parameters.extend([
        occurred_at.clone().into(),
        occurred_at.into(),
        since.event_id.as_str().to_owned().into(),
    ]);
    query_count_sql(connection, &sql, &parameters)
}

fn normalized_history_cursor(cursor: &HistoryCursor) -> String {
    // The authoritative in-memory order places unparseable instants before all
    // parsed instants. Derived rows are always normalized and non-empty, so the
    // empty SQLite key preserves that tolerant legacy behavior.
    normalize_occurred_at(&cursor.occurred_at).unwrap_or_default()
}

pub(super) fn hydrate_events(
    service: &super::service::DerivedAccessService,
    event_ids: &[String],
    as_of: TruthCursor,
) -> Result<Vec<ShoreEvent>, String> {
    match service
        .semantic_ids_at(event_ids, as_of)
        .map_err(|error| error.to_string())?
    {
        LocatorRead::Ready(events) => event_ids
            .iter()
            .zip(events)
            .map(|(event_id, event)| {
                event.ok_or_else(|| format!("selected authoritative event {event_id} is absent"))
            })
            .collect(),
        LocatorRead::CatchUpRequired { .. } => {
            Err("derived history became stale during selected hydration".to_owned())
        }
    }
}

pub(super) fn state_diagnostics(
    state: &SemanticStateSnapshot,
) -> Result<Vec<ProjectionDiagnostic>, String> {
    state
        .diagnostics
        .iter()
        .cloned()
        .map(|diagnostic| serde_json::from_value(diagnostic).map_err(|error| error.to_string()))
        .collect()
}

fn record_active_ownership(entries: &[ReviewHistoryEntry]) {
    #[cfg(any(test, feature = "longitudinal-counting"))]
    {
        crate::bench_support::longitudinal::set_retained_hydrated_history_entries(entries.len());
        crate::bench_support::longitudinal::set_retained_search_record_strings(0);
        crate::bench_support::longitudinal::set_retained_search_record_field_bytes(0);
        crate::bench_support::longitudinal::set_retained_decoded_events(entries.len());
    }
    #[cfg(not(any(test, feature = "longitudinal-counting")))]
    let _ = entries;
}

fn to_sql_integer(value: impl TryInto<i64>) -> Result<i64, String> {
    value
        .try_into()
        .map_err(|_| "history value does not fit SQLite INTEGER".to_owned())
}

fn map_availability(value: DerivedAccessAvailability) -> DerivedHistoryAvailability {
    match value {
        DerivedAccessAvailability::Absent => DerivedHistoryAvailability::Absent,
        DerivedAccessAvailability::Bootstrapping => DerivedHistoryAvailability::Bootstrapping,
        DerivedAccessAvailability::Current => DerivedHistoryAvailability::Current,
        DerivedAccessAvailability::CatchingUp => DerivedHistoryAvailability::CatchingUp,
        DerivedAccessAvailability::RebuildRequired => DerivedHistoryAvailability::RebuildRequired,
        DerivedAccessAvailability::Quarantined => DerivedHistoryAvailability::Quarantined,
        DerivedAccessAvailability::Unavailable => DerivedHistoryAvailability::Unavailable,
    }
}

fn map_namespace(value: DerivedStorageNamespace) -> DerivedHistoryNamespace {
    match value {
        DerivedStorageNamespace::Stable => DerivedHistoryNamespace::Stable,
        DerivedStorageNamespace::Legacy => DerivedHistoryNamespace::Legacy,
    }
}

fn namespace_for_layout(layout: &super::generation::GenerationLayout) -> DerivedHistoryNamespace {
    map_namespace(layout.namespace())
}

fn map_transition(value: DerivedStorageTransition) -> DerivedHistoryTransition {
    match value {
        DerivedStorageTransition::NotNeeded => DerivedHistoryTransition::NotNeeded,
        DerivedStorageTransition::Deferred => DerivedHistoryTransition::Deferred,
        DerivedStorageTransition::Moved => DerivedHistoryTransition::Moved,
        DerivedStorageTransition::Conflict => DerivedHistoryTransition::Conflict,
    }
}

fn map_lifecycle_progress(value: LifecycleProgress) -> DerivedHistoryProgress {
    DerivedHistoryProgress {
        phase: map_progress_phase(value.phase),
        completed_events: value.completed,
        total_events: value.total,
        completed_bytes: value.bytes_processed,
        elapsed_milliseconds: value.elapsed_ms,
        eta_milliseconds: value.estimated_remaining_ms,
    }
}

fn map_progress_phase(
    value: super::generation::GenerationProgressPhase,
) -> DerivedHistoryProgressPhase {
    match value {
        super::generation::GenerationProgressPhase::CursorPopulation => {
            DerivedHistoryProgressPhase::CursorPopulation
        }
        super::generation::GenerationProgressPhase::ProjectionPopulation => {
            DerivedHistoryProgressPhase::ProjectionPopulation
        }
        super::generation::GenerationProgressPhase::StrictVerification => {
            DerivedHistoryProgressPhase::StrictVerification
        }
        super::generation::GenerationProgressPhase::Finalizing => {
            DerivedHistoryProgressPhase::Finalizing
        }
    }
}

fn unavailable_lifecycle_status(
    observed: super::lifecycle::LifecycleStatus,
    fallback_detail: &str,
) -> DerivedHistoryStatus {
    let availability = map_availability(observed.availability);
    DerivedHistoryStatus {
        availability: if availability == DerivedHistoryAvailability::Current {
            DerivedHistoryAvailability::Unavailable
        } else {
            availability
        },
        detail: observed.detail.or_else(|| Some(fallback_detail.to_owned())),
    }
}

fn status(
    availability: DerivedHistoryAvailability,
    detail: impl Into<String>,
) -> DerivedHistoryStatus {
    DerivedHistoryStatus {
        availability,
        detail: Some(detail.into()),
    }
}

pub(super) fn catching_up_status() -> DerivedHistoryStatus {
    status(
        DerivedHistoryAvailability::CatchingUp,
        "derived history is catching up to authoritative truth",
    )
}

pub(super) fn projection_stamp(
    store_identity: &str,
    cursor: TruthCursor,
) -> Result<String, String> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct StampMaterial<'a> {
        schema: &'static str,
        store_identity: &'a str,
        profile: &'static str,
        schema_version: &'static str,
        epoch: u64,
        applied_sequence: u64,
    }

    let material = serde_json::to_value(StampMaterial {
        schema: PROJECTION_STAMP_SCHEMA_V1,
        store_identity,
        profile: ACTIVE_PROFILE,
        schema_version: PRODUCT_HISTORY_SCHEMA_V5,
        epoch: cursor.epoch,
        applied_sequence: cursor.sequence,
    })
    .map_err(|error| error.to_string())?;
    sha256_json_prefixed(&material).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use rusqlite::limits::Limit;
    use tempfile::TempDir;

    use super::*;
    use crate::model::{
        EngagementId, InputRequestId, InputRequestResponseId, JournalId, ObjectId, ObservationId,
        ReviewTargetRef, RevisionId, TargetRef, TaskTargetRef, TrackId, WorkObjectId,
    };
    use crate::session::derived_access::generation::{GenerationProgress, GenerationProgressPhase};
    use crate::session::derived_access::lifecycle::LifecycleControl;
    use crate::session::derived_access::sqlite::StoreWriterLock;
    use crate::session::event::{
        AssertionMode, EventTarget, EventType, InputRequestResponseOutcome,
        ReviewInitializedPayload, ReviewObservationRecordedPayload, Revision, ShoreEvent,
        WorkObjectProposal, WorkObjectProposedPayload, Writer,
    };
    use crate::session::projection::test_support::{
        task_input_request_event_with_target, user_response_event,
    };
    use crate::session::store::authority_lock::StoreAuthorityLock;
    use crate::session::store::capabilities::{
        CapabilityFixtureState, write_capability_fixture_for_test,
    };
    use crate::session::store::resolution::resolve_store;
    use crate::session::workflow::history_base_from_events;
    use crate::session::{EventStore, EventWriteOutcome, apply_history_query, count_new_since};

    fn active_history(event_count: usize) -> (TempDir, DerivedHistoryAccess) {
        active_history_from_events((0..event_count).map(review_initialized).collect::<Vec<_>>())
    }

    fn active_history_from_events(events: Vec<ShoreEvent>) -> (TempDir, DerivedHistoryAccess) {
        let (temp, access) = unbuilt_active_history_from_events(events);
        let lifecycle = access.lifecycle().expect("test access is active");
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        (temp, access)
    }

    #[test]
    fn unchanged_publication_reuses_the_same_current_generation_arc() {
        let (_temp, access) = active_history(1);
        let CurrentRead::Ready(first) = access.current().unwrap() else {
            panic!("published generation should be readable");
        };
        let CurrentRead::Ready(second) = access.current().unwrap() else {
            panic!("unchanged publication should remain readable");
        };

        assert!(
            Arc::ptr_eq(&first, &second),
            "unchanged reads must reuse the process-local generation lease"
        );
    }

    #[test]
    fn ready_retry_may_preserve_the_current_projection_stamp() {
        let (_temp, access) = active_history(1);
        let DerivedHistoryRoute::Ready(before) = access.freshness().unwrap() else {
            panic!("initial generation should be current");
        };
        let lifecycle = access.lifecycle().expect("test access should be active");
        let before_generation = lifecycle.published_generation_id().unwrap();

        access.restart_background_rebuild().unwrap();
        wait_for_background_rebuild(&access, "ready retry maintenance");

        let DerivedHistoryRoute::Ready(after) = access.freshness().unwrap() else {
            panic!("ready retry should preserve a current generation");
        };
        assert_eq!(after.projection_stamp, before.projection_stamp);
        assert_eq!(
            lifecycle.published_generation_id().unwrap(),
            before_generation
        );
    }

    fn unbuilt_active_history_from_events(
        events: Vec<ShoreEvent>,
    ) -> (TempDir, DerivedHistoryAccess) {
        let temp = TempDir::new().unwrap();
        let store = EventStore::open(temp.path());
        for event in events {
            assert_eq!(
                store.record_event_once(&event).unwrap(),
                EventWriteOutcome::Created
            );
        }
        let lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            temp.path(),
            "store:test",
        )
        .unwrap();
        let mode = DerivedHistoryMode::Active {
            lifecycle,
            current: Mutex::new(None),
            store_identity: "store:test".to_owned(),
            backend: StoreBackend::Local(temp.path().to_path_buf()),
        };
        let maintenance = DerivedHistoryMaintenance {
            profile: DerivedAccessProfile::SqliteWalBodylessV1,
            store_root: temp.path().to_path_buf(),
            store_identity: "store:test".to_owned(),
        };
        let access =
            DerivedHistoryAccess::from_runtime(DerivedAccessRuntime::new(mode, Some(maintenance)));
        (temp, access)
    }

    #[test]
    fn activated_change_roots_do_not_start_the_legacy_inspector_sidecar() {
        let l0_repo = TempDir::new().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(l0_repo.path())
                .status()
                .unwrap()
                .success()
        );
        let l0_resolution = resolve_store(l0_repo.path()).unwrap();
        let l0_access = DerivedHistoryAccess::resolve_for_inspector_with_profile(
            l0_repo.path(),
            DerivedAccessProfile::SqliteWalBodylessV1,
        )
        .unwrap();
        assert!(l0_access.is_active());
        l0_access
            .build(|_| DerivedHistoryControl::Continue)
            .unwrap();
        assert!(l0_resolution.store_dir().join("derived").is_dir());

        for state in [CapabilityFixtureState::M1, CapabilityFixtureState::L2] {
            let repo = TempDir::new().unwrap();
            assert!(
                std::process::Command::new("git")
                    .args(["init", "--quiet"])
                    .current_dir(repo.path())
                    .status()
                    .unwrap()
                    .success()
            );
            let resolution = resolve_store(repo.path()).unwrap();
            write_capability_fixture_for_test(resolution.backend().journal().as_ref(), state)
                .unwrap();

            let access = DerivedHistoryAccess::resolve_for_inspector_with_profile(
                repo.path(),
                DerivedAccessProfile::SqliteWalBodylessV1,
            )
            .unwrap();
            assert!(!access.is_active());
            assert_eq!(
                access.lifecycle_status().availability,
                DerivedHistoryAvailability::Absent
            );
            assert!(!resolution.store_dir().join("derived").exists());
        }
    }

    #[test]
    fn synchronous_build_cancellation_preserves_truth_and_publishes_nothing() {
        let (temp, access) =
            unbuilt_active_history_from_events((0..7).map(review_initialized).collect::<Vec<_>>());
        let mut progress_calls = 0;

        let result = access.build(|_| {
            progress_calls += 1;
            DerivedHistoryControl::Cancel
        });

        assert!(result.unwrap_err().contains("cancelled"));
        assert!(progress_calls > 0);
        assert_eq!(
            EventStore::open(temp.path()).list_events().unwrap().len(),
            7
        );
        assert_eq!(
            access.lifecycle_status().availability,
            DerivedHistoryAvailability::Absent
        );
    }

    fn wait_for_background_rebuild(access: &DerivedHistoryAccess, context: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while access.maintenance_in_flight() {
            assert!(
                std::time::Instant::now() < deadline,
                "{context} worker did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn wait_for_current_generation(access: &DerivedHistoryAccess, context: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match access.current().unwrap() {
                CurrentRead::Ready(_) => return,
                CurrentRead::Unavailable(status) => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "{context} did not recover: {status:?}"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
    }

    #[test]
    fn unavailable_route_never_serializes_lifecycle_current() {
        let status = unavailable_lifecycle_status(
            super::super::lifecycle::LifecycleStatus {
                availability: DerivedAccessAvailability::Current,
                generation_id: Some("g-current".to_owned()),
                phase: None,
                completed: None,
                total: None,
                bytes_processed: None,
                elapsed_ms: None,
                estimated_remaining_ms: None,
                detail: None,
            },
            "publication handoff requires a retry",
        );

        assert_eq!(status.availability, DerivedHistoryAvailability::Unavailable);
        assert_eq!(
            status.detail.as_deref(),
            Some("publication handoff requires a retry")
        );
    }

    #[test]
    fn active_access_bootstraps_an_absent_generation_in_the_background() {
        let (_temp, access) = unbuilt_active_history_from_events(vec![review_initialized(0)]);
        assert!(matches!(
            access.current().unwrap(),
            CurrentRead::Unavailable(DerivedHistoryStatus {
                availability: DerivedHistoryAvailability::Absent,
                ..
            })
        ));

        access.start_background_rebuild().unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match access.current().unwrap() {
                CurrentRead::Ready(_) => break,
                CurrentRead::Unavailable(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                CurrentRead::Unavailable(status) => {
                    panic!("background bootstrap did not publish: {status:?}");
                }
            }
        }
    }

    #[test]
    fn background_rebuild_reserves_propagates_and_completes_one_child_scope() {
        use crate::bench_support::longitudinal::{
            InteractionActorV1, InteractionScopeCoverageV1, LongitudinalCountingScopeV1,
        };

        let (_temp, access) = unbuilt_active_history_from_events(vec![review_initialized(0)]);
        let counting = LongitudinalCountingScopeV1::new("9".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let _scope = counting.enter();

        access.start_background_rebuild().unwrap();
        wait_for_background_rebuild(&access, "interaction background rebuild");

        let snapshot = counting.snapshot();
        assert_eq!(
            snapshot.child_reservations,
            vec![(0, InteractionActorV1::BackgroundRebuild)]
        );
        assert_eq!(snapshot.child_terminals.len(), 1);
        assert_eq!(snapshot.child_terminals[0].ordinal, 0);
        assert_eq!(
            snapshot.child_terminals[0].actor,
            InteractionActorV1::BackgroundRebuild
        );
        assert_eq!(
            snapshot.child_terminals[0].coverage,
            InteractionScopeCoverageV1::Complete
        );
        assert!(!snapshot.derived_access_phases.is_empty());
        assert!(
            snapshot
                .derived_access_phases
                .iter()
                .all(|sample| { sample.actor == Some(InteractionActorV1::BackgroundRebuild) })
        );
    }

    #[test]
    fn synchronous_build_attributes_existing_work_to_explicit_recovery() {
        use crate::bench_support::longitudinal::{InteractionActorV1, LongitudinalCountingScopeV1};

        let (_temp, access) = unbuilt_active_history_from_events(vec![review_initialized(0)]);
        let counting = LongitudinalCountingScopeV1::new("a".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let _scope = counting.enter();

        access
            .build(|_| DerivedHistoryControl::Continue)
            .expect("explicit build");

        let snapshot = counting.snapshot();
        assert!(!snapshot.derived_access_phases.is_empty());
        assert!(
            snapshot
                .derived_access_phases
                .iter()
                .all(|sample| { sample.actor == Some(InteractionActorV1::ExplicitRecovery) })
        );
        assert!(!snapshot.lock_facts.is_empty());
        assert!(
            snapshot
                .lock_facts
                .iter()
                .all(|fact| fact.actor == InteractionActorV1::ExplicitRecovery)
        );
    }

    #[test]
    fn activated_store_without_a_current_generation_only_schedules_maintenance() {
        let temp = TempDir::new().unwrap();
        let backend = StoreBackend::Local(temp.path().to_path_buf());
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();
        let lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            temp.path(),
            "store:test",
        )
        .unwrap();
        let scope =
            crate::bench_support::longitudinal::LongitudinalCountingScopeV1::new("b".repeat(64))
                .unwrap();
        let guard = scope.enter();
        assert!(lifecycle.change_capability_activated().unwrap());
        drop(guard);
        let counters = scope.snapshot().counters;
        assert_eq!(counters.directory_entries_walked, 0);
        assert_eq!(counters.carrier_opens, 0);
        assert_eq!(counters.event_folds, 0);
        lifecycle.paths().ensure_scaffold().unwrap();
        let rebuild_lease = lifecycle.paths().try_rebuild_lease().unwrap();
        let access = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Active {
            lifecycle: lifecycle.clone(),
            current: Mutex::new(None),
            store_identity: "store:test".to_owned(),
            backend,
        });

        let interaction =
            crate::bench_support::longitudinal::LongitudinalCountingScopeV1::new("e".repeat(64))
                .unwrap();
        interaction.record_execution_actor_once(
            crate::bench_support::longitudinal::InteractionActorV1::RequestReader,
        );
        let interaction_guard = interaction.enter();
        assert!(matches!(
            access.current().unwrap(),
            CurrentRead::Unavailable(_)
        ));
        wait_for_background_rebuild(&access, "activated cold-store maintenance");
        drop(interaction_guard);
        let interaction_snapshot = interaction.snapshot();
        assert_eq!(
            interaction_snapshot.child_reservations,
            vec![(
                0,
                crate::bench_support::longitudinal::InteractionActorV1::BackgroundMaintenance
            )]
        );
        assert_eq!(interaction_snapshot.child_terminals.len(), 1);
        assert_eq!(
            interaction_snapshot.child_terminals[0].coverage,
            crate::bench_support::longitudinal::InteractionScopeCoverageV1::Complete
        );
        assert!(
            interaction_snapshot
                .derived_access_phases
                .iter()
                .all(|sample| {
                    sample.actor
                == Some(
                    crate::bench_support::longitudinal::InteractionActorV1::BackgroundMaintenance,
                )
                })
        );
        assert!(interaction_snapshot.lock_facts.iter().all(|fact| {
            fact.actor
                == crate::bench_support::longitudinal::InteractionActorV1::BackgroundMaintenance
        }));
        assert_eq!(lifecycle.published_generation_id().unwrap(), None);
        assert!(!access.rebuild_in_flight());
        drop(rebuild_lease);
    }

    #[test]
    fn activated_store_reader_error_does_not_replace_the_current_generation() {
        use crate::session::derived_access::semantic::change::CHANGE_READER_PROFILE_RESOURCE_V3;

        let temp = TempDir::new().unwrap();
        let backend = StoreBackend::Local(temp.path().to_path_buf());
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();
        let lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            temp.path(),
            "store:test",
        )
        .unwrap();
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let generation_id = lifecycle
            .published_generation_id()
            .unwrap()
            .expect("explicit setup rebuild publishes a generation");
        std::fs::remove_file(
            lifecycle
                .paths()
                .generation(&generation_id)
                .join(CHANGE_READER_PROFILE_RESOURCE_V3),
        )
        .unwrap();
        let rebuild_lease = lifecycle.paths().try_rebuild_lease().unwrap();
        let access = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Active {
            lifecycle: lifecycle.clone(),
            current: Mutex::new(None),
            store_identity: "store:test".to_owned(),
            backend,
        });

        assert!(matches!(
            access.current().unwrap(),
            CurrentRead::Unavailable(DerivedHistoryStatus {
                availability: DerivedHistoryAvailability::RebuildRequired,
                ..
            })
        ));
        wait_for_background_rebuild(&access, "activated reader-error maintenance");
        assert_eq!(
            lifecycle.published_generation_id().unwrap().as_deref(),
            Some(generation_id.as_str())
        );
        assert!(!access.rebuild_in_flight());
        drop(rebuild_lease);

        access.restart_background_rebuild().unwrap();
        assert!(
            access.rebuild_in_flight(),
            "explicit retry reports rebuild admission during confirmation"
        );
        wait_for_background_rebuild(&access, "explicit activated-store retry");
        assert_ne!(
            lifecycle.published_generation_id().unwrap().as_deref(),
            Some(generation_id.as_str()),
            "an explicit retry remains authorized to replace the invalid generation"
        );
        assert!(matches!(access.current().unwrap(), CurrentRead::Ready(_)));
    }

    #[test]
    fn active_access_joins_a_contended_background_rebuild_without_restart() {
        let (_temp, access) = unbuilt_active_history_from_events(vec![review_initialized(0)]);
        let lifecycle = access.lifecycle().expect("test access is active");
        let rebuild_lease = lifecycle.paths().try_rebuild_lease().unwrap();

        access.start_background_rebuild().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(rebuild_lease);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match access.current().unwrap() {
                CurrentRead::Ready(_) => break,
                CurrentRead::Unavailable(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                CurrentRead::Unavailable(status) => {
                    panic!("contended background bootstrap did not publish: {status:?}");
                }
            }
        }
    }

    #[test]
    fn lifecycle_status_maps_staging_progress_before_a_generation_is_current() {
        let (_temp, access) = unbuilt_active_history_from_events(vec![review_initialized(0)]);
        let lifecycle = access.lifecycle().expect("test access is active");
        lifecycle.paths().ensure_scaffold().unwrap();
        let (_, generation_id) = lifecycle.paths().next_generation().unwrap();
        std::fs::create_dir_all(lifecycle.paths().staging(&generation_id)).unwrap();
        lifecycle
            .paths()
            .record_progress(
                &generation_id,
                GenerationProgress::new(
                    GenerationProgressPhase::ProjectionPopulation,
                    256,
                    512,
                    4096,
                    250,
                    Some(350),
                ),
            )
            .unwrap();
        assert!(lifecycle.paths().root().is_dir());
        assert!(lifecycle.paths().staging_progress().unwrap().is_some());

        let status = access.lifecycle_status();
        assert!(status.active);
        assert_eq!(
            status.availability,
            DerivedHistoryAvailability::Bootstrapping
        );
        assert_eq!(
            status.phase,
            Some(DerivedHistoryProgressPhase::ProjectionPopulation)
        );
        assert_eq!(status.completed_events, Some(256));
        assert_eq!(status.total_events, Some(512));
        assert_eq!(status.completed_bytes, Some(4096));
        assert_eq!(status.elapsed_milliseconds, Some(250));
        assert_eq!(status.eta_milliseconds, Some(350));
    }

    #[test]
    fn valid_old_generation_serves_during_replacement_but_stamp_drift_fails_closed() {
        let (temp, access) = active_history(1);
        let lifecycle = access.lifecycle().expect("test access is active").clone();
        lifecycle.paths().ensure_scaffold().unwrap();
        let (_, generation_id) = lifecycle.paths().next_generation().unwrap();
        std::fs::create_dir_all(lifecycle.paths().staging(&generation_id)).unwrap();
        lifecycle
            .paths()
            .record_progress(
                &generation_id,
                GenerationProgress::new(
                    GenerationProgressPhase::CursorPopulation,
                    0,
                    1,
                    0,
                    0,
                    None,
                ),
            )
            .unwrap();

        assert_eq!(
            access.lifecycle_status().availability,
            DerivedHistoryAvailability::Bootstrapping
        );
        assert!(
            access.current_readable(),
            "unchanged old current remains readable"
        );

        let rebuild_lease = lifecycle.paths().try_rebuild_lease().unwrap();
        EventStore::open(temp.path())
            .record_event_once(&review_initialized(2))
            .unwrap();
        assert!(
            !access.current_readable(),
            "authority drift invalidates the old generation"
        );
        access.cancel_background_rebuild().unwrap();
        drop(rebuild_lease);
    }

    #[test]
    fn cancellation_joins_a_contended_worker_and_retry_can_publish() {
        let (_temp, access) = unbuilt_active_history_from_events(vec![review_initialized(0)]);
        let lifecycle = access.lifecycle().expect("test access is active").clone();
        let rebuild_lease = lifecycle.paths().try_rebuild_lease().unwrap();
        for _ in 0..100 {
            access.restart_background_rebuild().unwrap();
            assert!(access.maintenance_in_flight());
            assert!(access.rebuild_in_flight());

            access.cancel_background_rebuild().unwrap();
            assert!(!access.maintenance_in_flight());
            assert!(!access.rebuild_in_flight());
            assert!(access.rebuild_worker_joined());
            assert!(!access.current_readable());
            assert!(
                !access.maintenance_in_flight(),
                "status/read discovery must honor explicit cancellation"
            );
        }

        drop(rebuild_lease);
        access.restart_background_rebuild().unwrap();
        wait_for_background_rebuild(&access, "retry after cancellation");
        assert!(access.current_readable());
    }

    #[test]
    fn dropping_access_cancels_and_joins_a_contended_worker() {
        let (_temp, access) = unbuilt_active_history_from_events(vec![review_initialized(0)]);
        let lifecycle = access.lifecycle().expect("test access is active").clone();
        let _rebuild_lease = lifecycle.paths().try_rebuild_lease().unwrap();
        access.start_background_rebuild().unwrap();

        let started = std::time::Instant::now();
        drop(access);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "drop interrupts retry sleep and joins promptly"
        );
    }

    #[test]
    fn cancellation_does_not_wait_for_a_busy_derived_writer() {
        let (temp, access) = active_history(1);
        EventStore::open(temp.path())
            .record_event_once(&review_initialized(2))
            .unwrap();
        let _writer_lock = StoreWriterLock::acquire(temp.path()).unwrap();

        access.restart_background_rebuild().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(access.maintenance_in_flight());

        let started = std::time::Instant::now();
        access.cancel_background_rebuild().unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "cancel must interrupt writer-busy confirmation without waiting for the writer"
        );
        assert!(!access.maintenance_in_flight());
        assert!(access.rebuild_worker_joined());
    }

    #[test]
    fn absent_generation_cancellation_does_not_wait_for_a_busy_derived_writer() {
        let (temp, access) = unbuilt_active_history_from_events(vec![review_initialized(0)]);
        let _writer_lock = StoreWriterLock::acquire(temp.path()).unwrap();

        access.start_background_rebuild().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(access.maintenance_in_flight());

        let started = std::time::Instant::now();
        access.cancel_background_rebuild().unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "cancel must not wait for bootstrap writer admission"
        );
        assert!(!access.maintenance_in_flight());
        assert!(access.rebuild_worker_joined());
    }

    #[test]
    fn automatic_l0_worker_cannot_publish_after_activation() {
        let (temp, access) = unbuilt_active_history_from_events(vec![review_initialized(0)]);
        let lifecycle = access.lifecycle().expect("test access is active").clone();
        let authority = StoreAuthorityLock::acquire(temp.path()).unwrap();

        access.start_background_rebuild().unwrap();
        let backend = StoreBackend::Local(temp.path().to_path_buf());
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();
        drop(authority);

        wait_for_background_rebuild(&access, "activation-interlocked automatic rebuild");
        assert_eq!(lifecycle.published_generation_id().unwrap(), None);
        assert!(!access.current_readable());
    }

    fn review_initialized(index: usize) -> ShoreEvent {
        let journal_id = JournalId::new(format!("journal:history:{index}"));
        ShoreEvent::new(
            EventType::ReviewInitialized,
            ReviewInitializedPayload::idempotency_key(&journal_id),
            EventTarget::for_journal(journal_id),
            Writer::shore_local("test"),
            ReviewInitializedPayload {},
            format!("2026-07-28T00:00:{index:02}Z"),
        )
        .unwrap()
    }

    fn captured_revision(
        revision_id: &RevisionId,
        object_id: &ObjectId,
        occurred_at: &str,
    ) -> ShoreEvent {
        ShoreEvent::new(
            EventType::WorkObjectProposed,
            format!("capture:{}", revision_id.as_str()),
            EventTarget::for_revision(JournalId::new("journal:history"), revision_id.clone(), None)
                .unwrap(),
            Writer::shore_local("test"),
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new(format!("engagement:sha256:{}", "11".repeat(32))),
                work_object: WorkObjectProposal::Revision {
                    revision: Revision {
                        id: revision_id.clone(),
                        object_id: object_id.clone(),
                        git_provenance: None,
                    },
                    summary: None,
                    object_artifact_content_hash: format!("sha256:{}", "22".repeat(32)),
                    supersedes: Vec::new(),
                },
            },
            occurred_at,
        )
        .unwrap()
    }

    fn observation(revision_id: &RevisionId, track: &str, occurred_at: &str) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewObservationRecorded,
            format!("observation:{track}:{occurred_at}"),
            EventTarget::for_revision(
                JournalId::new("journal:history"),
                revision_id.clone(),
                Some(TrackId::new(track)),
            )
            .unwrap(),
            Writer::shore_local("test"),
            ReviewObservationRecordedPayload {
                observation_id: ObservationId::new(format!("obs:sha256:{}", "33".repeat(32))),
                target: ReviewTargetRef::Revision {
                    revision_id: revision_id.clone(),
                },
                title: "selected observation".to_owned(),
                body: None,
                body_content_type: Default::default(),
                body_artifact_path: None,
                body_byte_size: None,
                body_content_hash: None,
                tags: vec!["Issue:158".to_owned()],
                confidence: None,
                supersedes_observation_ids: Vec::new(),
                responds_to_observation_ids: Vec::new(),
            },
            occurred_at,
        )
        .unwrap()
    }

    #[test]
    fn projection_stamp_binds_store_profile_schema_epoch_and_sequence() {
        let base = projection_stamp("store:one", TruthCursor::new(3, 8)).unwrap();

        assert_eq!(
            base,
            "sha256:582d8e32c6d3b3c04e5b21a7a92de8e0b41d6233c40d26c4b381c1f0f2dad8a8"
        );
        assert_eq!(
            base,
            projection_stamp("store:one", TruthCursor::new(3, 8)).unwrap()
        );
        assert_ne!(
            base,
            projection_stamp("store:two", TruthCursor::new(3, 8)).unwrap()
        );
        assert_ne!(
            base,
            projection_stamp("store:one", TruthCursor::new(4, 8)).unwrap()
        );
        assert_ne!(
            base,
            projection_stamp("store:one", TruthCursor::new(3, 9)).unwrap()
        );
    }

    #[test]
    fn selected_support_includes_validation_log_artifact_hashes() {
        let mut event = review_initialized(1);
        event.payload = serde_json::json!({
            "summaryContentHash": "sha256:summary",
            "logArtifactContentHashes": ["sha256:log-a", "sha256:log-b"],
        });

        assert_eq!(
            crate::session::workflow::selected_support_content_hashes(&[event])
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![
                "sha256:log-a".to_owned(),
                "sha256:log-b".to_owned(),
                "sha256:summary".to_owned(),
            ]
        );
    }

    #[test]
    fn selected_support_stays_within_the_portable_sqlite_variable_limit() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE locator_event_text (
                     sequence INTEGER NOT NULL,
                     event_id TEXT NOT NULL,
                     event_type TEXT NOT NULL,
                     epoch INTEGER NOT NULL
                 );
                 CREATE TABLE semantic_event_fact_text (
                     sequence INTEGER NOT NULL,
                     content_hash TEXT
                 );
                 CREATE TABLE product_history_signature (
                     sequence INTEGER NOT NULL,
                     target_event_id TEXT NOT NULL
                 );",
            )
            .unwrap();
        connection
            .set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 999)
            .unwrap();

        let selected = (0..1_100)
            .map(|index| {
                let mut event = review_initialized(index);
                event.payload = serde_json::json!({
                    "summaryContentHash": format!("sha256:summary-{index:04}")
                });
                event
            })
            .collect::<Vec<_>>();
        let removed_hash = "sha256:summary-1099";
        connection
            .execute(
                "INSERT INTO locator_event_text
                     (sequence, event_id, event_type, epoch)
                 VALUES (1, 'event:removed', 'artifact_removed', 0)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO semantic_event_fact_text (sequence, content_hash)
                 VALUES (1, ?1)",
                [removed_hash],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO locator_event_text
                     (sequence, event_id, event_type, epoch)
                 VALUES (2, 'event:signature', 'event_signature_recorded', 0)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO product_history_signature (sequence, target_event_id)
                 VALUES (2, ?1)",
                ["event:removed"],
            )
            .unwrap();

        assert_eq!(
            support_event_ids(&connection, &selected, TruthCursor::new(0, 2)).unwrap(),
            vec!["event:removed".to_owned(), "event:signature".to_owned()]
        );
    }

    #[test]
    fn selected_support_uses_indexed_semantic_content_for_nested_payloads() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE locator_event_text (
                     sequence INTEGER NOT NULL,
                     event_id TEXT NOT NULL,
                     event_type TEXT NOT NULL,
                     epoch INTEGER NOT NULL
                 );
                 CREATE TABLE semantic_event_fact_text (
                     sequence INTEGER NOT NULL,
                     content_hash TEXT
                 );
                 CREATE TABLE product_history_signature (
                     sequence INTEGER NOT NULL,
                     target_event_id TEXT NOT NULL
                 );",
            )
            .unwrap();
        let selected = review_initialized(1);
        let selected_id = selected.event_id.as_str();
        let removed_hash = "sha256:nested-object-content";
        connection
            .execute(
                "INSERT INTO locator_event_text
                     (sequence, event_id, event_type, epoch)
                 VALUES (1, ?1, 'work_object_proposed', 0)",
                [selected_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO semantic_event_fact_text (sequence, content_hash)
                 VALUES (1, ?1)",
                [removed_hash],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO locator_event_text
                     (sequence, event_id, event_type, epoch)
                 VALUES (2, 'event:removed', 'artifact_removed', 0)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO semantic_event_fact_text (sequence, content_hash)
                 VALUES (2, ?1)",
                [removed_hash],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO locator_event_text
                     (sequence, event_id, event_type, epoch)
                 VALUES (3, 'event:signature', 'event_signature_recorded', 0)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO product_history_signature (sequence, target_event_id)
                 VALUES (3, 'event:removed')",
                [],
            )
            .unwrap();

        assert_eq!(
            support_event_ids(&connection, &[selected], TruthCursor::new(0, 3)).unwrap(),
            vec!["event:removed".to_owned(), "event:signature".to_owned()]
        );
    }

    #[test]
    fn active_history_pages_hydrate_only_selected_authoritative_carriers() {
        let (_temp, access) = active_history(7);
        let scope =
            crate::bench_support::longitudinal::LongitudinalCountingScopeV1::new("a".repeat(64))
                .unwrap();
        let _guard = scope.enter();
        let result = access
            .history(
                &HistoryQuery {
                    order: HistoryOrder::Desc,
                    ..HistoryQuery::default()
                },
                &HistoryPage {
                    limit: Some(2),
                    ..HistoryPage::default()
                },
                &BaseProjectionConfig::default(),
            )
            .unwrap();
        let DerivedHistoryRoute::Ready(page) = result else {
            panic!("active history should be current");
        };

        assert_eq!(page.event_count, 7);
        assert_eq!(page.match_count, 7);
        assert_eq!(page.entries.len(), 2);
        assert!(page.entries[0].occurred_at > page.entries[1].occurred_at);
        assert_eq!(page.facets.get("review_initialized"), Some(&7));
        let counters = scope.snapshot();
        assert_eq!(counters.counters.carrier_opens, 2);
        assert_eq!(counters.counters.event_decodes, 2);
        assert_eq!(counters.counters.directory_entries_walked, 0);
        assert_eq!(
            counters
                .capacity_ownership
                .retained_hydrated_history_entries,
            2
        );
        assert_eq!(
            counters.capacity_ownership.retained_search_record_strings,
            0
        );
    }

    #[test]
    fn explicit_search_is_the_only_exhaustive_history_fallback() {
        let (_temp, access) = active_history(1);
        let result = access
            .history(
                &HistoryQuery {
                    q: "body text".to_owned(),
                    ..HistoryQuery::default()
                },
                &HistoryPage::default(),
                &BaseProjectionConfig::default(),
            )
            .unwrap();

        assert!(matches!(
            result,
            DerivedHistoryRoute::ExhaustiveSearchFallback
        ));
    }

    #[test]
    fn out_of_band_truth_append_rebuilds_without_restarting_the_reader() {
        let (temp, access) = active_history(1);
        assert!(matches!(
            access.freshness().unwrap(),
            DerivedHistoryRoute::Ready(_)
        ));
        EventStore::open(temp.path())
            .record_event_once(&review_initialized(2))
            .unwrap();

        let DerivedHistoryRoute::Unavailable(status) = access.freshness().unwrap() else {
            panic!("legacy truth append must not serve stale derived state");
        };
        assert_eq!(
            status.availability,
            DerivedHistoryAvailability::RebuildRequired
        );

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match access.freshness().unwrap() {
                DerivedHistoryRoute::Ready(freshness) => {
                    assert_eq!(freshness.event_count, 2);
                    break;
                }
                DerivedHistoryRoute::Unavailable(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                DerivedHistoryRoute::Unavailable(status) => {
                    panic!("out-of-band append did not rebuild: {status:?}");
                }
                DerivedHistoryRoute::Off | DerivedHistoryRoute::ExhaustiveSearchFallback => {
                    panic!("active freshness returned the wrong route");
                }
            }
        }
    }

    #[test]
    fn invalid_publication_is_typed_unavailable_instead_of_failing_the_reader() {
        let (_temp, access) = active_history(1);
        let lifecycle = access.lifecycle().expect("test access is active");
        let publications = lifecycle.paths().root().join("publications");
        let publication = std::fs::read_dir(publications)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::write(publication, b"not a publication").unwrap();

        let CurrentRead::Unavailable(status) = access.current().unwrap() else {
            panic!("invalid publication must not be served");
        };
        assert!(matches!(
            status.availability,
            DerivedHistoryAvailability::Quarantined | DerivedHistoryAvailability::Unavailable
        ));
        wait_for_background_rebuild(&access, "invalid publication recovery");
        assert!(matches!(access.current().unwrap(), CurrentRead::Ready(_)));
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_sidecar_does_not_block_background_startup() {
        use std::os::unix::fs::PermissionsExt;

        let (_temp, access) = active_history(1);
        let lifecycle = access.lifecycle().expect("test access is active");
        let publications = lifecycle.paths().root().join("publications");
        std::fs::set_permissions(&publications, std::fs::Permissions::from_mode(0o000)).unwrap();

        let started = access.start_background_rebuild();

        std::fs::set_permissions(&publications, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            started.is_ok(),
            "sidecar status must be read in the worker: {started:?}"
        );
        wait_for_background_rebuild(&access, "unreadable sidecar status");
    }

    #[test]
    fn published_rebuild_replaces_the_cached_reader_and_projection_stamp() {
        let (_temp, access) = active_history(2);
        let DerivedHistoryRoute::Ready(before) = access.freshness().unwrap() else {
            panic!("initial generation should be current");
        };
        let lifecycle = access.lifecycle().expect("test access should be active");

        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();

        let DerivedHistoryRoute::Ready(after) = access.freshness().unwrap() else {
            panic!("replacement generation should be current");
        };
        assert_eq!(after.event_count, before.event_count);
        assert_ne!(after.projection_stamp, before.projection_stamp);
    }

    #[test]
    fn active_bodyless_history_matrix_matches_the_authoritative_projection() {
        let revision_id = RevisionId::new(format!("rev:sha256:{}", "44".repeat(32)));
        let object_id = ObjectId::new(format!("object:sha256:{}", "55".repeat(32)));
        let capture = captured_revision(&revision_id, &object_id, "2026-07-28T00:00:01Z");
        let selected = observation(&revision_id, "CoDe", "2026-07-28T00:00:02Z");
        let initialized = review_initialized(3);
        let selected_id = selected.event_id.clone();
        let capture_cursor = HistoryCursor {
            occurred_at: capture.occurred_at.clone(),
            event_id: capture.event_id.clone(),
        };
        let events = vec![initialized, selected, capture];
        let (_temp, access) = active_history_from_events(events.clone());
        let config = BaseProjectionConfig::default();
        let base = history_base_from_events(&events, &config, None).unwrap();
        let matrix = vec![
            (
                HistoryQuery::default(),
                HistoryPage {
                    limit: Some(2),
                    ..HistoryPage::default()
                },
            ),
            (
                HistoryQuery {
                    order: HistoryOrder::Desc,
                    ..HistoryQuery::default()
                },
                HistoryPage {
                    limit: Some(2),
                    offset: Some(1),
                    ..HistoryPage::default()
                },
            ),
            (
                HistoryQuery::default(),
                HistoryPage {
                    limit: Some(2),
                    at: Some(selected_id),
                    ..HistoryPage::default()
                },
            ),
            (
                HistoryQuery::default(),
                HistoryPage {
                    limit: Some(2),
                    after: Some(capture_cursor),
                    ..HistoryPage::default()
                },
            ),
            (
                HistoryQuery {
                    track: Some("code".to_owned()),
                    ..HistoryQuery::default()
                },
                HistoryPage::default(),
            ),
            (
                HistoryQuery {
                    snapshot: Some(object_id.as_str().to_owned()),
                    ..HistoryQuery::default()
                },
                HistoryPage::default(),
            ),
            (
                HistoryQuery {
                    revision: Some(revision_id),
                    ..HistoryQuery::default()
                },
                HistoryPage::default(),
            ),
            (
                HistoryQuery {
                    types: Some(BTreeSet::from(["review_observation_recorded".to_owned()])),
                    ..HistoryQuery::default()
                },
                HistoryPage::default(),
            ),
        ];

        for (query, page) in matrix {
            let expected = apply_history_query(&base, &query, &page);
            let DerivedHistoryRoute::Ready(actual) =
                access.history(&query, &page, &config).unwrap()
            else {
                panic!("active matrix row should be current");
            };
            assert_eq!(
                serde_json::to_value(&actual.entries).unwrap(),
                serde_json::to_value(&expected.entries).unwrap()
            );
            assert_eq!(actual.event_count, expected.event_count);
            assert_eq!(actual.facets, expected.facets);
            assert_eq!(actual.match_count, expected.match_count);
            assert_eq!(actual.offset, expected.offset);
            assert_eq!(actual.match_index, expected.match_index);
            assert_eq!(actual.diagnostics, expected.diagnostics);
            assert_eq!(actual.query_notices, expected.query_notices);
            assert_eq!(actual.distinct_values, expected.distinct_values);
        }
    }

    #[test]
    fn active_history_excludes_task_subject_input_requests() {
        let task_attempt_id = WorkObjectId::new("task-attempt:sha256:history");
        let journal_id = JournalId::new("journal:history:task");
        let input_request_id = InputRequestId::new("input-request:sha256:history");
        let response_id = InputRequestResponseId::new("input-request-response:sha256:history");
        let task_target = TaskTargetRef::TaskAttempt {
            task_attempt_id: task_attempt_id.clone(),
        };
        let mut request = task_input_request_event_with_target(
            &task_attempt_id,
            &journal_id,
            &input_request_id,
            "history-task-request",
            "2026-07-28T00:00:01Z",
            TargetRef::Task(task_target),
            "task-only request",
        );
        request.target.track_id = Some(TrackId::new("agent:test"));
        let mut response = user_response_event(
            &input_request_id,
            &response_id,
            InputRequestResponseOutcome::Approved,
            AssertionMode::Operative,
            "2026-07-28T00:00:02Z",
        );
        response.target.track_id = Some(TrackId::new("agent:test"));
        let review = review_initialized(3);
        let events = vec![review.clone(), request, response];
        let (_temp, access) = active_history_from_events(events.clone());
        let config = BaseProjectionConfig::default();
        let expected = apply_history_query(
            &history_base_from_events(&events, &config, None).unwrap(),
            &HistoryQuery::default(),
            &HistoryPage::default(),
        );

        let DerivedHistoryRoute::Ready(actual) = access
            .history(&HistoryQuery::default(), &HistoryPage::default(), &config)
            .unwrap()
        else {
            panic!("active history should be current");
        };
        assert_eq!(actual.match_count, 1);
        assert_eq!(actual.entries.len(), 1);
        assert_eq!(actual.entries[0].event_id, review.event_id);
        assert_eq!(
            serde_json::to_value(&actual.entries).unwrap(),
            serde_json::to_value(&expected.entries).unwrap()
        );
        assert_eq!(actual.facets, expected.facets);
        assert_eq!(actual.distinct_values, expected.distinct_values);
    }

    #[test]
    fn active_new_count_matches_the_authoritative_bodyless_matrix() {
        let revision_id = RevisionId::new(format!("rev:sha256:{}", "66".repeat(32)));
        let object_id = ObjectId::new(format!("object:sha256:{}", "77".repeat(32)));
        let capture = captured_revision(&revision_id, &object_id, "2026-07-28T00:00:01Z");
        let selected = observation(&revision_id, "CoDe", "2026-07-28T00:00:02Z");
        let since = HistoryCursor {
            occurred_at: capture.occurred_at.clone(),
            event_id: capture.event_id.clone(),
        };
        let events = vec![review_initialized(3), selected, capture];
        let (_temp, access) = active_history_from_events(events.clone());
        let config = BaseProjectionConfig::default();
        let base = history_base_from_events(&events, &config, None).unwrap();
        let matrix = [
            HistoryQuery::default(),
            HistoryQuery {
                track: Some("code".to_owned()),
                ..HistoryQuery::default()
            },
            HistoryQuery {
                snapshot: Some(object_id.as_str().to_owned()),
                ..HistoryQuery::default()
            },
            HistoryQuery {
                revision: Some(revision_id),
                ..HistoryQuery::default()
            },
            HistoryQuery {
                types: Some(BTreeSet::from(["review_observation_recorded".to_owned()])),
                ..HistoryQuery::default()
            },
        ];

        for query in matrix {
            let expected = count_new_since(&base, &query, &since);
            let DerivedHistoryRoute::Ready(actual) = access.new_count(&query, &since).unwrap()
            else {
                panic!("active new-count row should be current");
            };
            assert_eq!(actual.new_count, expected);
        }
    }

    #[test]
    fn active_history_clamps_offsets_and_tolerates_edge_cursor_inputs() {
        let (_temp, access) = active_history(7);
        let config = BaseProjectionConfig::default();

        for page in [
            HistoryPage {
                limit: Some(2),
                offset: Some(99),
                ..HistoryPage::default()
            },
            HistoryPage {
                limit: Some(usize::MAX),
                offset: Some(usize::MAX),
                ..HistoryPage::default()
            },
        ] {
            let DerivedHistoryRoute::Ready(actual) = access
                .history(&HistoryQuery::default(), &page, &config)
                .unwrap()
            else {
                panic!("active history should be current");
            };
            assert_eq!(actual.offset, 7);
            assert!(actual.entries.is_empty());
        }

        let malformed = HistoryCursor {
            occurred_at: "garbage".to_owned(),
            event_id: crate::model::EventId::new("evt:sha256:before-all-legal-instants"),
        };
        let DerivedHistoryRoute::Ready(actual) = access
            .new_count(&HistoryQuery::default(), &malformed)
            .unwrap()
        else {
            panic!("active new-count should be current");
        };
        assert_eq!(actual.new_count, 7);
    }

    #[test]
    fn freshness_rejects_a_semantic_checkpoint_behind_the_cursor_head() {
        let (temp, access) = active_history(2);
        let lifecycle = access.lifecycle().expect("test access should be active");
        let generation_id = lifecycle
            .published_generation_id()
            .unwrap()
            .expect("rebuild should publish a generation");
        let database = lifecycle
            .paths()
            .generation(&generation_id)
            .join("cursor.sqlite3");
        let connection = rusqlite::Connection::open(database).unwrap();
        connection
            .execute_batch(
                "UPDATE locator_checkpoint SET applied_sequence = 1 WHERE singleton = 1;
                 UPDATE semantic_meta SET applied_sequence = 1 WHERE singleton = 1;
                 UPDATE product_history_meta SET applied_sequence = 1 WHERE singleton = 1;",
            )
            .unwrap();
        drop(connection);

        let cold = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Active {
            lifecycle: lifecycle.clone(),
            current: Mutex::new(None),
            store_identity: "store:test".to_owned(),
            backend: StoreBackend::Local(temp.path().to_path_buf()),
        });
        let DerivedHistoryRoute::Unavailable(status) = cold.freshness().unwrap() else {
            panic!("a lagging semantic checkpoint must not look current");
        };
        assert_eq!(status.availability, DerivedHistoryAvailability::CatchingUp);

        let rebuild_lease = lifecycle.paths().try_rebuild_lease().unwrap();
        let DerivedHistoryRoute::Unavailable(status) = cold.freshness().unwrap() else {
            panic!("a cached lagging checkpoint must not look current");
        };
        assert_eq!(status.availability, DerivedHistoryAvailability::CatchingUp);
        assert!(
            !cold.rebuild_in_flight(),
            "current-generation maintenance must not report an N+1 rebuild"
        );
        wait_for_background_rebuild(&cold, "lagging checkpoint maintenance");
        assert_eq!(
            lifecycle.published_generation_id().unwrap().as_deref(),
            Some(generation_id.as_str()),
            "lagging maintenance must not publish a replacement generation"
        );
        drop(rebuild_lease);
    }

    #[test]
    fn cached_generation_failure_is_typed_and_recovers_without_restart() {
        let (_temp, access) = active_history(1);
        assert!(matches!(
            access.freshness().unwrap(),
            DerivedHistoryRoute::Ready(_)
        ));
        let lifecycle = access.lifecycle().expect("test access is active");
        let generation_id = lifecycle
            .published_generation_id()
            .unwrap()
            .expect("generation is published");
        let database = lifecycle
            .paths()
            .generation(&generation_id)
            .join("cursor.sqlite3");
        rusqlite::Connection::open(database)
            .unwrap()
            .execute_batch("DROP TABLE cursor_meta;")
            .unwrap();

        let CurrentRead::Unavailable(status) = access.current().unwrap() else {
            panic!("a damaged cached generation must not be served");
        };
        assert_eq!(status.availability, DerivedHistoryAvailability::Unavailable);
        wait_for_current_generation(&access, "cached generation recovery");
    }

    #[test]
    fn active_cursor_comparisons_normalize_legacy_unix_millis() {
        let revision_id = RevisionId::new(format!("rev:sha256:{}", "88".repeat(32)));
        let object_id = ObjectId::new(format!("object:sha256:{}", "99".repeat(32)));
        let older = captured_revision(&revision_id, &object_id, "unix-ms:0");
        let newer = observation(&revision_id, "code", "1970-01-01T00:00:01Z");
        let cursor = HistoryCursor {
            occurred_at: older.occurred_at.clone(),
            event_id: older.event_id.clone(),
        };
        let events = vec![review_initialized(3), newer.clone(), older];
        let (_temp, access) = active_history_from_events(events);

        let DerivedHistoryRoute::Ready(page) = access
            .history(
                &HistoryQuery::default(),
                &HistoryPage {
                    limit: Some(1),
                    after: Some(cursor.clone()),
                    ..HistoryPage::default()
                },
                &BaseProjectionConfig::default(),
            )
            .unwrap()
        else {
            panic!("active history should be current");
        };
        assert_eq!(page.offset, 1);
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].event_id, newer.event_id);

        let DerivedHistoryRoute::Ready(new_count) =
            access.new_count(&HistoryQuery::default(), &cursor).unwrap()
        else {
            panic!("active new-count should be current");
        };
        assert_eq!(new_count.new_count, 2);
    }
}
