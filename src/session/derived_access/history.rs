//! Product history and freshness reads over the derived-access profile.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use rusqlite::OptionalExtension;
use rusqlite::types::Value;
use serde::Serialize;

use super::cursor::TruthCursor;
use super::lifecycle::{
    CurrentGeneration, DerivedAccessLifecycle, LifecycleControl, LifecycleError,
};
use super::locator::{LocatorRead, normalize_occurred_at};
use super::product_contract::{DerivedAccessAvailability, DerivedAccessProfile};
use crate::canonical_hash::sha256_json_prefixed;
use crate::session::ProjectionDiagnostic;
use crate::session::derived_access::semantic::state::SemanticStateSnapshot;
use crate::session::event::ShoreEvent;
use crate::session::store::backend::StoreBackend;
use crate::session::store::resolution::{opaque_path_identity, resolve_read_store};
use crate::session::workflow::{
    BaseProjectionConfig, DistinctValues, HistoryCursor, HistoryOrder, HistoryPage, HistoryQuery,
    QueryDiagnostic, ReviewHistoryEntry, history_entries_from_selected_events,
};

const PRODUCT_HISTORY_SCHEMA_V2: &str = "pointbreak.sqlite-derived-access-history.v2";
const PROJECTION_STAMP_SCHEMA_V1: &str = "pointbreak.derived-access-projection-stamp.v1";
const ACTIVE_PROFILE: &str = "sqlite-wal-bodyless-v1";
const BACKGROUND_REBUILD_RETRY_INTERVAL: Duration = Duration::from_millis(100);
const BACKGROUND_REBUILD_REQUIRED_CONFIRMATION: Duration = Duration::from_millis(250);
const BACKGROUND_TRUTH_CHANGED_MAX_INTERVAL: Duration = Duration::from_secs(5);
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
        'artifact_removed'
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
    pub(super) mode: DerivedHistoryMode,
    background_rebuild_in_flight: Arc<AtomicBool>,
}

pub(super) enum DerivedHistoryMode {
    Off,
    Active {
        lifecycle: DerivedAccessLifecycle,
        current: Mutex<Option<Arc<CurrentGeneration>>>,
        store_identity: String,
        backend: StoreBackend,
    },
}

impl DerivedHistoryAccess {
    pub(super) fn from_mode(mode: DerivedHistoryMode) -> Self {
        Self {
            mode,
            background_rebuild_in_flight: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn resolve(repo: impl AsRef<Path>) -> Result<Self, String> {
        let profile =
            DerivedAccessProfile::from_environment().map_err(|error| error.to_string())?;
        if profile == DerivedAccessProfile::Off {
            return Ok(Self::from_mode(DerivedHistoryMode::Off));
        }
        let read_store = resolve_read_store(repo).map_err(|error| error.to_string())?;
        let store_identity = opaque_path_identity("store", read_store.store_dir())
            .map_err(|error| error.to_string())?;
        let lifecycle =
            DerivedAccessLifecycle::new(profile, read_store.store_dir(), store_identity.clone())
                .map_err(|error| error.to_string())?;
        Ok(Self::from_mode(DerivedHistoryMode::Active {
            lifecycle,
            current: Mutex::new(None),
            store_identity,
            backend: read_store.backend().clone(),
        }))
    }

    pub const fn is_active(&self) -> bool {
        matches!(self.mode, DerivedHistoryMode::Active { .. })
    }

    /// Start rebuilding a non-current active profile without delaying the
    /// caller. The lifecycle's store-scoped lease deduplicates concurrent
    /// Inspector processes; readers continue to report the typed lifecycle
    /// state until the immutable generation is published.
    #[doc(hidden)]
    pub fn start_background_rebuild(&self) -> Result<(), String> {
        let DerivedHistoryMode::Active { lifecycle, .. } = &self.mode else {
            return Ok(());
        };
        if self
            .background_rebuild_in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(());
        }
        let lifecycle = lifecycle.clone();
        let in_flight = Arc::clone(&self.background_rebuild_in_flight);
        let spawned = std::thread::Builder::new()
            .name("pointbreak-derived-rebuild".to_owned())
            .spawn(move || background_rebuild(lifecycle, in_flight));
        match spawned {
            Ok(_) => Ok(()),
            Err(error) => {
                self.background_rebuild_in_flight
                    .store(false, Ordering::Release);
                Err(format!("could not start derived-access rebuild: {error}"))
            }
        }
    }

    pub fn history(
        &self,
        query: &HistoryQuery,
        page: &HistoryPage,
        config: &BaseProjectionConfig,
    ) -> Result<DerivedHistoryRoute<DerivedHistoryPage>, String> {
        let DerivedHistoryMode::Active {
            store_identity,
            backend,
            ..
        } = &self.mode
        else {
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
        let DerivedHistoryMode::Active { store_identity, .. } = &self.mode else {
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
        let DerivedHistoryMode::Active { store_identity, .. } = &self.mode else {
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
        self.current_with_publication_retry(true)
    }

    fn current_with_publication_retry(
        &self,
        retry_current_transition: bool,
    ) -> Result<CurrentRead, String> {
        let DerivedHistoryMode::Active {
            lifecycle, current, ..
        } = &self.mode
        else {
            return Err("derived history is disabled".to_owned());
        };
        let published_generation_id = match lifecycle.published_generation_id() {
            Ok(generation_id) => generation_id,
            Err(error) => {
                self.request_background_rebuild();
                return Ok(CurrentRead::Unavailable(status(
                    DerivedHistoryAvailability::Unavailable,
                    error.to_string(),
                )));
            }
        };
        let mut guard = lock(current);
        if let Some(existing) = guard.as_ref()
            && published_generation_id.as_deref() != Some(existing.generation_id())
        {
            *guard = None;
        }
        if let Some(existing) = guard.as_ref() {
            let head = match lifecycle.validate_current_authority(existing.service()) {
                Ok(authority) => authority.head.cursor,
                Err(LifecycleError::RebuildRequired(detail)) => {
                    drop(guard);
                    self.request_background_rebuild();
                    return Ok(CurrentRead::Unavailable(status(
                        DerivedHistoryAvailability::RebuildRequired,
                        detail,
                    )));
                }
                Err(error) => {
                    *guard = None;
                    drop(guard);
                    self.request_background_rebuild();
                    return Ok(CurrentRead::Unavailable(status(
                        DerivedHistoryAvailability::Unavailable,
                        error.to_string(),
                    )));
                }
            };
            let applied = match existing.service().locator_checkpoint() {
                Ok(applied) => applied,
                Err(error) => {
                    *guard = None;
                    drop(guard);
                    self.request_background_rebuild();
                    return Ok(CurrentRead::Unavailable(status(
                        DerivedHistoryAvailability::Unavailable,
                        error.to_string(),
                    )));
                }
            };
            if applied == head {
                return Ok(CurrentRead::Ready(Arc::clone(existing)));
            }
            drop(guard);
            return Ok(CurrentRead::Unavailable(status(
                DerivedHistoryAvailability::CatchingUp,
                "derived history is catching up to authoritative truth",
            )));
        }
        match lifecycle.open_current() {
            Ok(Some(opened)) => {
                let opened = Arc::new(opened);
                *guard = Some(Arc::clone(&opened));
                Ok(CurrentRead::Ready(opened))
            }
            Ok(None) => {
                let observed = lifecycle.status();
                drop(guard);
                if retry_current_transition
                    && matches!(
                        observed.as_ref(),
                        Ok(status)
                            if status.availability == DerivedAccessAvailability::Current
                    )
                {
                    // Publication completed after `open_current` selected its
                    // input. Retry once so a usable Current generation becomes
                    // a Ready payload, never a 503 carrying "current".
                    return self.current_with_publication_retry(false);
                }
                self.request_background_rebuild();
                Ok(CurrentRead::Unavailable(match observed {
                    Ok(observed) => unavailable_lifecycle_status(
                        observed,
                        "current generation was not openable after publication",
                    ),
                    Err(error) => {
                        status(DerivedHistoryAvailability::Unavailable, error.to_string())
                    }
                }))
            }
            Err(error) => {
                let observed = lifecycle.status();
                drop(guard);
                if retry_current_transition
                    && matches!(
                        observed.as_ref(),
                        Ok(status)
                            if status.availability == DerivedAccessAvailability::Current
                    )
                {
                    return self.current_with_publication_retry(false);
                }
                self.request_background_rebuild();
                match observed {
                    Ok(observed) => Ok(CurrentRead::Unavailable(unavailable_lifecycle_status(
                        observed,
                        &error.to_string(),
                    ))),
                    Err(status_error) => Ok(CurrentRead::Unavailable(status(
                        DerivedHistoryAvailability::Unavailable,
                        format!("{error}; derived status also failed: {status_error}"),
                    ))),
                }
            }
        }
    }

    fn request_background_rebuild(&self) {
        if let Err(error) = self.start_background_rebuild() {
            tracing::warn!(error = %error, "derived_access_background_rebuild_start_failed");
        }
    }
}

struct BackgroundRebuildGuard(Arc<AtomicBool>);

impl Drop for BackgroundRebuildGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn background_rebuild(lifecycle: DerivedAccessLifecycle, in_flight: Arc<AtomicBool>) {
    let _guard = BackgroundRebuildGuard(in_flight);
    let mut truth_changed_retry_interval = BACKGROUND_REBUILD_RETRY_INTERVAL;
    let mut rebuild_required_confirmed = false;
    // The availability state is also the recovery state machine:
    //
    // - Current/CatchingUp: serve or finish bounded in-place projection work;
    //   never replace the generation.
    // - RebuildRequired: observe twice, then confirm once more while the
    //   canonical writer is idle. A governed append temporarily enters this
    //   state between loose truth publication and cursor receipt finalization.
    // - Absent/Bootstrapping/Unavailable/Quarantined: attempt or join the
    //   disposable full rebuild.
    //
    // RebuildBusy is another process making progress. TruthChanged means this
    // worker lost a race to a writer and backs off without publishing stale
    // state.
    loop {
        match lifecycle.status() {
            Ok(status)
                if matches!(
                    status.availability,
                    DerivedAccessAvailability::Current | DerivedAccessAvailability::CatchingUp
                ) =>
            {
                return;
            }
            Ok(status)
                if status.availability == DerivedAccessAvailability::RebuildRequired
                    && !rebuild_required_confirmed =>
            {
                rebuild_required_confirmed = true;
                std::thread::sleep(BACKGROUND_REBUILD_REQUIRED_CONFIRMATION);
                continue;
            }
            Ok(status) if status.availability == DerivedAccessAvailability::RebuildRequired => {
                match lifecycle.rebuild_required_while_writer_idle() {
                    Ok(true) => {}
                    Ok(false) => {
                        // A governed writer closed the transient pre-receipt
                        // gap while we acquired its lock. Throttle repeated
                        // confirmations so a sustained append stream cannot
                        // turn recovery into writer-lock contention.
                        std::thread::sleep(BACKGROUND_REBUILD_REQUIRED_CONFIRMATION);
                        continue;
                    }
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "derived_access_background_rebuild_confirmation_failed"
                        );
                        return;
                    }
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(error = %error, "derived_access_background_status_failed");
                return;
            }
        }
        match lifecycle.rebuild(|_| LifecycleControl::Continue) {
            Ok(_) => return,
            Err(LifecycleError::RebuildBusy) => {
                std::thread::sleep(BACKGROUND_REBUILD_RETRY_INTERVAL);
            }
            Err(LifecycleError::TruthChanged) => {
                std::thread::sleep(truth_changed_retry_interval);
                truth_changed_retry_interval = truth_changed_retry_interval
                    .saturating_mul(2)
                    .min(BACKGROUND_TRUTH_CHANGED_MAX_INTERVAL);
            }
            Err(error) => {
                tracing::warn!(error = %error, "derived_access_background_rebuild_failed");
                return;
            }
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
    let sql = format!(
        "{REVIEW_EVENT_CTE}
         SELECT count(*) FROM review_event WHERE {predicate}"
    );
    let count = connection
        .query_row(&sql, rusqlite::params_from_iter(parameters.iter()), |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| error.to_string())?;
    usize::try_from(count).map_err(|_| "negative history count".to_owned())
}

fn query_facets(
    connection: &rusqlite::Connection,
    predicate: &str,
    parameters: &[Value],
) -> Result<BTreeMap<String, usize>, String> {
    let sql = format!(
        "{REVIEW_EVENT_CTE}
         SELECT event_type, count(*)
         FROM review_event
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
    let direction = match query.order {
        HistoryOrder::Asc => "ASC",
        HistoryOrder::Desc => "DESC",
    };
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
    let mut sql = format!(
        "{REVIEW_EVENT_CTE}
         SELECT event_id
         FROM review_event
         WHERE {page_predicate}
         ORDER BY normalized_occurred_at {direction}, event_id {direction}"
    );
    match page.limit {
        Some(limit) => {
            let effective_limit = limit.min(match_count);
            sql.push_str(" LIMIT ? OFFSET ?");
            page_parameters.push(to_sql_integer(effective_limit)?.into());
            page_parameters
                .push(to_sql_integer(if page.after.is_some() { 0 } else { offset })?.into());
        }
        None if offset > 0 && page.after.is_none() => {
            sql.push_str(" LIMIT -1 OFFSET ?");
            page_parameters.push(to_sql_integer(offset)?.into());
        }
        None => {}
    }
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(page_parameters.iter()), |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
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

/// Find the authoritative carriers needed to interpret selected product rows.
///
/// Support closure has three ordered phases:
///
/// 1. Payload extraction retains multi-value references such as validation
///    logs, while the indexed semantic fact for every selected carrier adds
///    canonical references nested inside typed payloads (notably a captured
///    revision's object artifact).
/// 2. Referenced content hashes select `artifact_removed` carriers. Those
///    carriers are also targets because a detached signature can attest to a
///    removal event.
/// 3. The original selection plus those removal carriers select detached
///    `event_signature_recorded` carriers.
///
/// Every phase may contain more values than SQLite can bind in one statement at
/// retained scale. A connection-local TEMP table carries each complete set into
/// a set-oriented join without changing the immutable generation. Complete the
/// removal phase before replacing that table with the signature targets;
/// otherwise signatures on removal carriers would be omitted. Each product
/// read owns its connection, so the TEMP table is isolated from other requests;
/// the transaction batches all populations, while `BTreeSet` preserves
/// deterministic, duplicate-free output.
pub(super) fn support_event_ids(
    connection: &rusqlite::Connection,
    selected: &[ShoreEvent],
    as_of: TruthCursor,
) -> Result<Vec<String>, String> {
    let mut targets = selected
        .iter()
        .map(|event| event.event_id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let mut content_hashes = crate::session::workflow::selected_support_content_hashes(selected)
        .map_err(|error| error.to_string())?;
    let mut support = BTreeSet::new();
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS pointbreak_product_support_lookup (
                 value TEXT PRIMARY KEY
             ) STRICT, WITHOUT ROWID;",
        )
        .map_err(|error| error.to_string())?;
    if !targets.is_empty() {
        replace_support_lookup_values(&transaction, targets.iter())?;
        let sql = "SELECT DISTINCT event.content_hash
                   FROM semantic_event_fact_text AS event
                   JOIN locator_event_text AS locator ON locator.sequence = event.sequence
                   JOIN temp.pointbreak_product_support_lookup AS selected
                     ON selected.value = locator.event_id
                   WHERE event.content_hash IS NOT NULL
                     AND locator.epoch = ?
                     AND locator.sequence <= ?
                   ORDER BY event.content_hash";
        let parameters = [
            Value::from(to_sql_integer(as_of.epoch)?),
            Value::from(to_sql_integer(as_of.sequence)?),
        ];
        content_hashes.extend(query_string_rows(&transaction, sql, &parameters)?);
    }
    if !content_hashes.is_empty() {
        replace_support_lookup_values(&transaction, content_hashes.iter())?;
        let sql = "SELECT locator.event_id
                   FROM semantic_event_fact_text AS event
                   JOIN locator_event_text AS locator ON locator.sequence = event.sequence
                   JOIN temp.pointbreak_product_support_lookup AS selected
                     ON selected.value = event.content_hash
                   WHERE locator.event_type = 'artifact_removed'
                     AND locator.epoch = ?
                     AND locator.sequence <= ?
                   ORDER BY locator.event_id";
        let parameters = [
            Value::from(to_sql_integer(as_of.epoch)?),
            Value::from(to_sql_integer(as_of.sequence)?),
        ];
        for event_id in query_string_rows(&transaction, sql, &parameters)? {
            targets.insert(event_id.clone());
            support.insert(event_id);
        }
    }
    if !targets.is_empty() {
        replace_support_lookup_values(&transaction, targets.iter())?;
        let sql = "SELECT locator.event_id
                   FROM product_history_signature AS signature
                   JOIN locator_event_text AS locator ON locator.sequence = signature.sequence
                   JOIN temp.pointbreak_product_support_lookup AS selected
                     ON selected.value = signature.target_event_id
                   WHERE locator.epoch = ?
                     AND locator.sequence <= ?
                   ORDER BY locator.event_id";
        let parameters = [
            Value::from(to_sql_integer(as_of.epoch)?),
            Value::from(to_sql_integer(as_of.sequence)?),
        ];
        support.extend(query_string_rows(&transaction, sql, &parameters)?);
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(support.into_iter().collect())
}

fn replace_support_lookup_values<'a>(
    transaction: &rusqlite::Transaction<'_>,
    values: impl IntoIterator<Item = &'a String>,
) -> Result<(), String> {
    transaction
        .execute("DELETE FROM temp.pointbreak_product_support_lookup", [])
        .map_err(|error| error.to_string())?;
    let mut insert = transaction
        .prepare("INSERT INTO temp.pointbreak_product_support_lookup (value) VALUES (?1)")
        .map_err(|error| error.to_string())?;
    for value in values {
        insert.execute([value]).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn query_string_rows(
    connection: &rusqlite::Connection,
    sql: &str,
    parameters: &[Value],
) -> Result<Vec<String>, String> {
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
        schema_version: PRODUCT_HISTORY_SCHEMA_V2,
        epoch: cursor.epoch,
        applied_sequence: cursor.sequence,
    })
    .map_err(|error| error.to_string())?;
    sha256_json_prefixed(&material).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use rusqlite::limits::Limit;
    use tempfile::TempDir;

    use super::*;
    use crate::model::{
        EngagementId, InputRequestId, InputRequestResponseId, JournalId, ObjectId, ObservationId,
        ReviewTargetRef, RevisionId, TargetRef, TaskTargetRef, TrackId, WorkObjectId,
    };
    use crate::session::derived_access::lifecycle::LifecycleControl;
    use crate::session::event::{
        AssertionMode, EventTarget, EventType, InputRequestResponseOutcome,
        ReviewInitializedPayload, ReviewObservationRecordedPayload, Revision, ShoreEvent,
        WorkObjectProposal, WorkObjectProposedPayload, Writer,
    };
    use crate::session::projection::test_support::{
        task_input_request_event_with_target, user_response_event,
    };
    use crate::session::workflow::history_base_from_events;
    use crate::session::{EventStore, EventWriteOutcome, apply_history_query, count_new_since};

    fn active_history(event_count: usize) -> (TempDir, DerivedHistoryAccess) {
        active_history_from_events((0..event_count).map(review_initialized).collect::<Vec<_>>())
    }

    fn active_history_from_events(events: Vec<ShoreEvent>) -> (TempDir, DerivedHistoryAccess) {
        let (temp, access) = unbuilt_active_history_from_events(events);
        let DerivedHistoryMode::Active { lifecycle, .. } = &access.mode else {
            unreachable!("test access is active");
        };
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        (temp, access)
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
        let access = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Active {
            lifecycle,
            current: Mutex::new(None),
            store_identity: "store:test".to_owned(),
            backend: StoreBackend::Local(temp.path().to_path_buf()),
        });
        (temp, access)
    }

    fn wait_for_background_rebuild(access: &DerivedHistoryAccess, context: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while access.background_rebuild_in_flight.load(Ordering::Acquire) {
            assert!(
                std::time::Instant::now() < deadline,
                "{context} worker did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
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
    fn active_access_joins_a_contended_background_rebuild_without_restart() {
        let (_temp, access) = unbuilt_active_history_from_events(vec![review_initialized(0)]);
        let DerivedHistoryMode::Active { lifecycle, .. } = &access.mode else {
            unreachable!("test access is active");
        };
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
            "sha256:c50da80445d7cd5848e72ef2a5bec07331051ac2c205f00dfe0c7be26779b742"
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
        let DerivedHistoryMode::Active { lifecycle, .. } = &access.mode else {
            unreachable!("test access is active");
        };
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
        let DerivedHistoryMode::Active { lifecycle, .. } = &access.mode else {
            unreachable!("test access is active");
        };
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
        let DerivedHistoryMode::Active { lifecycle, .. } = &access.mode else {
            panic!("test access should be active");
        };

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
        let DerivedHistoryMode::Active { lifecycle, .. } = &access.mode else {
            panic!("test access should be active");
        };
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
            !cold.background_rebuild_in_flight.load(Ordering::Acquire),
            "bounded governed catch-up must not start a full rebuild"
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
        let DerivedHistoryMode::Active { lifecycle, .. } = &access.mode else {
            unreachable!("test access is active");
        };
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
        wait_for_background_rebuild(&access, "cached generation recovery");
        assert!(matches!(access.current().unwrap(), CurrentRead::Ready(_)));
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
