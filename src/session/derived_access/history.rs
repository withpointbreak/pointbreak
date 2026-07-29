//! Product history and freshness reads over the derived-access profile.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rusqlite::OptionalExtension;
use rusqlite::types::Value;
use serde::Serialize;

use super::cursor::TruthCursor;
use super::lifecycle::{CurrentGeneration, DerivedAccessLifecycle};
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

const PRODUCT_HISTORY_SCHEMA_V1: &str = "pointbreak.sqlite-derived-access-history.v1";
const PROJECTION_STAMP_SCHEMA_V1: &str = "pointbreak.derived-access-projection-stamp.v1";
const ACTIVE_PROFILE: &str = "sqlite-wal-bodyless-v1";
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
          locator.event_type != 'work_object_proposed'
          OR event.revision_id IS NOT NULL
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
    mode: DerivedHistoryMode,
}

enum DerivedHistoryMode {
    Off,
    Active {
        lifecycle: DerivedAccessLifecycle,
        current: Mutex<Option<Arc<CurrentGeneration>>>,
        store_identity: String,
        backend: StoreBackend,
    },
}

impl DerivedHistoryAccess {
    pub fn resolve(repo: impl AsRef<Path>) -> Result<Self, String> {
        let profile =
            DerivedAccessProfile::from_environment().map_err(|error| error.to_string())?;
        if profile == DerivedAccessProfile::Off {
            return Ok(Self {
                mode: DerivedHistoryMode::Off,
            });
        }
        let read_store = resolve_read_store(repo).map_err(|error| error.to_string())?;
        let store_identity = opaque_path_identity("store", read_store.store_dir())
            .map_err(|error| error.to_string())?;
        let lifecycle =
            DerivedAccessLifecycle::new(profile, read_store.store_dir(), store_identity.clone())
                .map_err(|error| error.to_string())?;
        Ok(Self {
            mode: DerivedHistoryMode::Active {
                lifecycle,
                current: Mutex::new(None),
                store_identity,
                backend: read_store.backend().clone(),
            },
        })
    }

    pub const fn is_active(&self) -> bool {
        matches!(self.mode, DerivedHistoryMode::Active { .. })
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
                return Ok(DerivedHistoryRoute::Unavailable(status(
                    DerivedHistoryAvailability::CatchingUp,
                    "derived history is catching up to authoritative truth",
                )));
            }
        };
        let as_of = service
            .locator_checkpoint()
            .map_err(|error| error.to_string())?;
        let selection = select_history_rows(&connection, query, page)?;
        let selected = hydrate_events(service, &selection.event_ids)?;
        let support_ids = support_event_ids(&connection, &selected, as_of)?;
        let mut support = selected.clone();
        support.extend(hydrate_events(service, &support_ids)?);
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
                return Ok(DerivedHistoryRoute::Unavailable(status(
                    DerivedHistoryAvailability::CatchingUp,
                    "derived history is catching up to authoritative truth",
                )));
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
        let as_of = current
            .service()
            .locator_checkpoint()
            .map_err(|error| error.to_string())?;
        Ok(DerivedHistoryRoute::Ready(DerivedHistoryFreshness {
            projection_stamp: projection_stamp(store_identity, as_of)?,
            event_count: as_of.sequence,
        }))
    }

    fn current(&self) -> Result<CurrentRead, String> {
        let DerivedHistoryMode::Active {
            lifecycle,
            current,
            backend,
            ..
        } = &self.mode
        else {
            return Err("derived history is disabled".to_owned());
        };
        let truth_count = backend
            .journal()
            .head_marker()
            .map_err(|error| error.to_string())?;
        let published_generation_id = lifecycle
            .published_generation_id()
            .map_err(|error| error.to_string())?;
        let mut guard = lock(current);
        if let Some(existing) = guard.as_ref()
            && published_generation_id.as_deref() != Some(existing.generation_id())
        {
            *guard = None;
        }
        if let Some(existing) = guard.as_ref() {
            let head = existing
                .service()
                .truth_head()
                .map_err(|error| error.to_string())?
                .cursor;
            let applied = existing
                .service()
                .locator_checkpoint()
                .map_err(|error| error.to_string())?;
            if head.sequence == truth_count && applied == head {
                return Ok(CurrentRead::Ready(Arc::clone(existing)));
            }
            *guard = None;
            if head.sequence != truth_count {
                return Ok(CurrentRead::Unavailable(status(
                    DerivedHistoryAvailability::RebuildRequired,
                    "derived cursor does not exactly cover authoritative truth",
                )));
            }
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
                let observed = lifecycle.status().map_err(|error| error.to_string())?;
                Ok(CurrentRead::Unavailable(DerivedHistoryStatus {
                    availability: map_availability(observed.availability),
                    detail: observed.detail,
                }))
            }
            Err(error) => {
                let observed = lifecycle.status().map_err(|_| error.to_string())?;
                Ok(CurrentRead::Unavailable(DerivedHistoryStatus {
                    availability: map_availability(observed.availability),
                    detail: observed.detail.or_else(|| Some(error.to_string())),
                }))
            }
        }
    }
}

enum CurrentRead {
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
    let (offset, match_index, at_absent) =
        resolve_history_offset(connection, query, page, &page_predicate, &page_parameters)?;
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
) -> Result<(usize, Option<usize>, bool), String> {
    if let Some(after) = &page.after {
        if query.order == HistoryOrder::Desc {
            return Err("descending history does not support continuation cursors".to_owned());
        }
        let occurred_at = normalize_history_cursor(after)?;
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
        return Ok((page.offset.unwrap_or(0), None, false));
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
        let occurred_at = normalize_history_cursor(after)?;
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
            sql.push_str(" LIMIT ? OFFSET ?");
            page_parameters.push(to_sql_integer(limit)?.into());
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
    let occurred_at = normalize_history_cursor(since)?;
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

fn normalize_history_cursor(cursor: &HistoryCursor) -> Result<String, String> {
    normalize_occurred_at(&cursor.occurred_at).map_err(|error| error.to_string())
}

fn support_event_ids(
    connection: &rusqlite::Connection,
    selected: &[ShoreEvent],
    as_of: TruthCursor,
) -> Result<Vec<String>, String> {
    let mut targets = selected
        .iter()
        .map(|event| event.event_id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let content_hashes = selected
        .iter()
        .flat_map(event_content_hashes)
        .collect::<BTreeSet<_>>();
    let mut support = BTreeSet::new();
    if !content_hashes.is_empty() {
        let placeholders = std::iter::repeat_n("?", content_hashes.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT locator.event_id
             FROM semantic_event_fact_text AS event
             JOIN locator_event_text AS locator ON locator.sequence = event.sequence
             WHERE locator.event_type = 'artifact_removed'
               AND locator.epoch = ?
               AND locator.sequence <= ?
               AND event.content_hash IN ({placeholders})
             ORDER BY locator.event_id"
        );
        let mut parameters = vec![
            to_sql_integer(as_of.epoch)?.into(),
            to_sql_integer(as_of.sequence)?.into(),
        ];
        parameters.extend(content_hashes.into_iter().map(Value::from));
        for event_id in query_string_rows(connection, &sql, &parameters)? {
            targets.insert(event_id.clone());
            support.insert(event_id);
        }
    }
    if !targets.is_empty() {
        let placeholders = std::iter::repeat_n("?", targets.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT locator.event_id
             FROM product_history_signature AS signature
             JOIN locator_event_text AS locator ON locator.sequence = signature.sequence
             WHERE locator.epoch = ?
               AND locator.sequence <= ?
               AND signature.target_event_id IN ({placeholders})
             ORDER BY locator.event_id"
        );
        let mut parameters = vec![
            to_sql_integer(as_of.epoch)?.into(),
            to_sql_integer(as_of.sequence)?.into(),
        ];
        parameters.extend(targets.into_iter().map(Value::from));
        support.extend(query_string_rows(connection, &sql, &parameters)?);
    }
    Ok(support.into_iter().collect())
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

fn hydrate_events(
    service: &super::service::DerivedAccessService,
    event_ids: &[String],
) -> Result<Vec<ShoreEvent>, String> {
    event_ids
        .iter()
        .map(|event_id| {
            match service
                .semantic_id(event_id)
                .map_err(|error| error.to_string())?
            {
                LocatorRead::Ready(Some(event)) => Ok(event),
                LocatorRead::Ready(None) => {
                    Err(format!("selected authoritative event {event_id} is absent"))
                }
                LocatorRead::CatchUpRequired { .. } => {
                    Err("derived history became stale during selected hydration".to_owned())
                }
            }
        })
        .collect()
}

fn event_content_hashes(event: &ShoreEvent) -> Vec<String> {
    const HASH_FIELDS: [&str; 5] = [
        "bodyContentHash",
        "summaryContentHash",
        "reasonContentHash",
        "objectArtifactContentHash",
        "contentHash",
    ];
    let Some(payload) = event.payload.as_object() else {
        return Vec::new();
    };
    HASH_FIELDS
        .iter()
        .filter_map(|field| payload.get(*field).and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect()
}

fn state_diagnostics(state: &SemanticStateSnapshot) -> Result<Vec<ProjectionDiagnostic>, String> {
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

fn status(
    availability: DerivedHistoryAvailability,
    detail: impl Into<String>,
) -> DerivedHistoryStatus {
    DerivedHistoryStatus {
        availability,
        detail: Some(detail.into()),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn projection_stamp(store_identity: &str, cursor: TruthCursor) -> Result<String, String> {
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
        schema_version: PRODUCT_HISTORY_SCHEMA_V1,
        epoch: cursor.epoch,
        applied_sequence: cursor.sequence,
    })
    .map_err(|error| error.to_string())?;
    sha256_json_prefixed(&material).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::model::{
        EngagementId, JournalId, ObjectId, ObservationId, ReviewTargetRef, RevisionId, TrackId,
    };
    use crate::session::derived_access::lifecycle::LifecycleControl;
    use crate::session::event::{
        EventTarget, EventType, ReviewInitializedPayload, ReviewObservationRecordedPayload,
        Revision, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload, Writer,
    };
    use crate::session::workflow::history_base_from_events;
    use crate::session::{EventStore, EventWriteOutcome, apply_history_query, count_new_since};

    fn active_history(event_count: usize) -> (TempDir, DerivedHistoryAccess) {
        active_history_from_events((0..event_count).map(review_initialized).collect::<Vec<_>>())
    }

    fn active_history_from_events(events: Vec<ShoreEvent>) -> (TempDir, DerivedHistoryAccess) {
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
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        let access = DerivedHistoryAccess {
            mode: DerivedHistoryMode::Active {
                lifecycle,
                current: Mutex::new(None),
                store_identity: "store:test".to_owned(),
                backend: StoreBackend::Local(temp.path().to_path_buf()),
            },
        };
        (temp, access)
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
    fn out_of_band_truth_append_never_serves_stale_history_as_current() {
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
