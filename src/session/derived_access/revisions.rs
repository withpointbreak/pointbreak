//! Product revision collection and exact-detail reads over the active derived generation.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rusqlite::types::Value;
use serde::{Deserialize, Serialize};

use super::history::{
    CurrentRead, DerivedHistoryAccess, DerivedHistoryMode, DerivedHistoryStatus,
    catching_up_status, hydrate_events, projection_stamp, state_diagnostics, support_event_ids,
};
use super::locator::LocatorRead;
use crate::canonical_hash::sha256_bytes_hex;
use crate::model::RevisionId;
use crate::session::event::ShoreEvent;
use crate::session::workflow::{
    RevisionListOptions, RevisionListResult, RevisionOverview, RevisionShowOptions,
    RevisionShowResult, SnapshotSummaryCache, list_revisions_from_selected_events,
    revision_overviews_from_selected_events, show_revision_from_selected_events,
};
use crate::session::{RemovalPolicy, SupersessionView, TrustSet};

pub const REVISION_PAGE_SCHEMA: &str = "pointbreak.inspect-revisions-page.v1";
pub const REVISION_PAGE_DEFAULT_LIMIT: usize = 100;
pub const REVISION_PAGE_MAXIMUM_LIMIT: usize = 500;
pub const ACTIVE_REVISION_PAGE_PROFILE: &str = "sqlite-wal-bodyless-v1";
pub const AUTHORITATIVE_REVISION_PAGE_PROFILE: &str = "authoritative-loose-v1";
const REVISION_PAGE_TOKEN_SCHEMA: &str = "pointbreak.inspect-revisions-page-token.v1";
const REVISION_PAGE_ORDER: &str = "captured_at_desc_revision_id_desc";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevisionPageRequestError {
    InvalidRequest,
    RestartRequired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionPageCursor {
    pub captured_at_millis: i64,
    pub revision_id: RevisionId,
}

#[derive(Clone, Debug)]
pub struct RevisionPageRequest {
    limit: usize,
    after: Option<RevisionPageToken>,
}

impl RevisionPageRequest {
    pub fn new(
        limit: Option<usize>,
        after: Option<&str>,
    ) -> Result<Self, RevisionPageRequestError> {
        let limit = limit.unwrap_or(REVISION_PAGE_DEFAULT_LIMIT);
        if limit == 0 || limit > REVISION_PAGE_MAXIMUM_LIMIT {
            return Err(RevisionPageRequestError::InvalidRequest);
        }
        let after = after.map(RevisionPageToken::decode).transpose()?;
        Ok(Self { limit, after })
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn cursor(
        &self,
        profile: &str,
        snapshot: &str,
    ) -> Result<Option<RevisionPageCursor>, RevisionPageRequestError> {
        let Some(token) = &self.after else {
            return Ok(None);
        };
        if token.profile != profile || token.snapshot != snapshot {
            return Err(RevisionPageRequestError::RestartRequired);
        }
        Ok(Some(RevisionPageCursor {
            captured_at_millis: token.captured_at_millis,
            revision_id: RevisionId::new(token.revision_id.clone()),
        }))
    }

    pub fn next(&self, profile: &str, snapshot: &str, cursor: &RevisionPageCursor) -> String {
        RevisionPageToken::new(
            profile,
            snapshot,
            cursor.captured_at_millis,
            &cursor.revision_id,
        )
        .encode()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RevisionPageToken {
    schema: String,
    profile: String,
    snapshot: String,
    order: String,
    captured_at_millis: i64,
    revision_id: String,
}

impl RevisionPageToken {
    fn new(
        profile: impl Into<String>,
        snapshot: impl Into<String>,
        captured_at_millis: i64,
        revision_id: &RevisionId,
    ) -> Self {
        Self {
            schema: REVISION_PAGE_TOKEN_SCHEMA.to_owned(),
            profile: profile.into(),
            snapshot: snapshot.into(),
            order: REVISION_PAGE_ORDER.to_owned(),
            captured_at_millis,
            revision_id: revision_id.as_str().to_owned(),
        }
    }

    fn encode(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("revision page token is serializable");
        URL_SAFE_NO_PAD.encode(bytes)
    }

    fn decode(token: &str) -> Result<Self, RevisionPageRequestError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(token.as_bytes())
            .map_err(|_| RevisionPageRequestError::InvalidRequest)?;
        let token: Self =
            serde_json::from_slice(&bytes).map_err(|_| RevisionPageRequestError::InvalidRequest)?;
        if token.schema != REVISION_PAGE_TOKEN_SCHEMA
            || token.order != REVISION_PAGE_ORDER
            || token.profile.is_empty()
            || token.snapshot.is_empty()
            || token.revision_id.is_empty()
        {
            return Err(RevisionPageRequestError::InvalidRequest);
        }
        Ok(token)
    }
}

#[doc(hidden)]
pub enum DerivedRevisionPageRoute {
    Off,
    Ready(DerivedRevisionPage),
    Unavailable(DerivedHistoryStatus),
    RestartRequired,
}

#[doc(hidden)]
pub enum DerivedRevisionDetailRoute {
    Off,
    Ready(Option<Box<DerivedRevisionDetail>>),
    ExactFallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RevisionPageWork {
    pub rows_selected: usize,
    pub entries_returned: usize,
}

#[doc(hidden)]
pub struct DerivedRevisionPage {
    pub projection_stamp: String,
    pub as_of: String,
    pub next: Option<String>,
    pub result: RevisionListResult,
    pub overviews: BTreeMap<RevisionId, RevisionOverview>,
    pub work: RevisionPageWork,
}

#[doc(hidden)]
pub struct DerivedRevisionDetail {
    pub projection_stamp: String,
    pub result: RevisionShowResult,
    pub supersession: SupersessionView,
}

/// Selects the authoritative events needed to render one exact revision and
/// its fork-tolerant supersession component at a frozen truth cursor.
///
/// The `CROSS JOIN` order and `INDEXED BY` clauses are deliberate planner
/// fences. Without them, SQLite is free to begin each recursive step from the
/// bounded-but-large locator range, then scan that range once per component
/// member. That turns a point read into `O(history * component)` work. The
/// fenced order advances the state machine from one known revision identity:
///
/// 1. expand backward through the selected revision's outgoing edges;
/// 2. expand forward through the target index;
/// 3. deduplicate identities through recursive `UNION`;
/// 4. scan the retained event range once and test membership in the
///    materialized component set.
///
/// Do not rewrite these joins as ordinary inner joins without checking the
/// `EXPLAIN QUERY PLAN` regression below on the bundled SQLite version.
const REVISION_COMPONENT_EVENT_IDS_SQL: &str = "WITH RECURSIVE component(revision_id) AS (
             SELECT ?3
             UNION
             SELECT edge.superseded_revision_id
             FROM component
             CROSS JOIN product_revision AS revision
               INDEXED BY product_revision_identity
             CROSS JOIN semantic_representative AS representative
             CROSS JOIN locator_event_text AS revision_locator
             CROSS JOIN product_revision_edge AS edge
             WHERE revision.revision_id = component.revision_id
               AND representative.family_id = 1
               AND representative.sequence = revision.sequence
               AND revision_locator.sequence = revision.sequence
               AND edge.sequence = revision.sequence
               AND revision_locator.epoch = ?1
               AND revision.sequence <= ?2
             UNION
             SELECT revision.revision_id
             FROM component
             CROSS JOIN product_revision_edge AS edge
               INDEXED BY product_revision_edge_target
             CROSS JOIN product_revision AS revision
             CROSS JOIN semantic_representative AS representative
             CROSS JOIN locator_event_text AS revision_locator
             WHERE edge.superseded_revision_id = component.revision_id
               AND revision.sequence = edge.sequence
               AND representative.family_id = 1
               AND representative.sequence = revision.sequence
               AND revision_locator.sequence = revision.sequence
               AND revision_locator.epoch = ?1
               AND revision.sequence <= ?2
         )
         SELECT locator.event_id
         FROM semantic_event_fact_text AS event
         JOIN locator_event_text AS locator ON locator.sequence = event.sequence
         WHERE locator.epoch = ?1
           AND locator.sequence <= ?2
           AND event.revision_id IN (SELECT revision_id FROM component)
         ORDER BY locator.replay_key, locator.event_id";

impl DerivedHistoryAccess {
    /// Read one snapshot-bound page of revision summaries from the active
    /// bodyless generation. Page selection is index-backed and examines only
    /// the accepted limit plus one look-ahead row; authoritative carriers are
    /// hydrated only for the selected summaries.
    #[doc(hidden)]
    pub fn revisions_page(
        &self,
        repo: &Path,
        trust_set: TrustSet,
        snapshot_summaries: Arc<SnapshotSummaryCache>,
        request: &RevisionPageRequest,
    ) -> Result<DerivedRevisionPageRoute, String> {
        let DerivedHistoryMode::Active {
            store_identity,
            backend,
            ..
        } = &self.mode
        else {
            return Ok(DerivedRevisionPageRoute::Off);
        };
        let current = match self.current()? {
            CurrentRead::Ready(current) => current,
            CurrentRead::Unavailable(status) => {
                return Ok(DerivedRevisionPageRoute::Unavailable(status));
            }
        };
        let service = current.service();
        let (connection, state) = match service
            .product_history_connection()
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(context) => context,
            LocatorRead::CatchUpRequired { .. } => {
                return Ok(DerivedRevisionPageRoute::Unavailable(catching_up_status()));
            }
        };
        let as_of = service
            .locator_checkpoint()
            .map_err(|error| error.to_string())?;
        let projection_stamp = projection_stamp(store_identity, as_of)?;
        let cursor = match request.cursor(ACTIVE_REVISION_PAGE_PROFILE, &projection_stamp) {
            Ok(cursor) => cursor,
            Err(RevisionPageRequestError::RestartRequired) => {
                return Ok(DerivedRevisionPageRoute::RestartRequired);
            }
            Err(RevisionPageRequestError::InvalidRequest) => {
                return Err("invalid revision page token".to_owned());
            }
        };
        let mut rows = revision_page_rows(
            &connection,
            as_of,
            cursor.as_ref(),
            request.limit().saturating_add(1),
        )?;
        let rows_selected = rows.len();
        let has_more = rows.len() > request.limit();
        rows.truncate(request.limit());
        let selected_ids = rows
            .iter()
            .map(|row| row.revision_id.clone())
            .collect::<Vec<_>>();
        let events = hydrate_revision_events(
            service,
            &connection,
            page_revision_event_ids(&connection, as_of, &selected_ids)?,
            as_of,
        )?;
        let mut result = list_revisions_from_selected_events(
            RevisionListOptions::new(repo)
                .with_read_for_display(true)
                .with_group_shared_commits(false),
            events.clone(),
        )
        .map_err(|error| error.to_string())?;
        let mut entries_by_id = result
            .entries
            .drain(..)
            .map(|entry| (entry.revision_id.clone(), entry))
            .collect::<BTreeMap<_, _>>();
        result.entries = selected_ids
            .iter()
            .map(|revision_id| {
                entries_by_id.remove(revision_id).ok_or_else(|| {
                    format!(
                        "derived revision page omitted selected revision {}",
                        revision_id.as_str()
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !entries_by_id.is_empty() {
            return Err("derived revision page returned an unselected revision".to_owned());
        }
        result.event_count = usize::try_from(as_of.sequence)
            .map_err(|_| "derived revision event count does not fit usize".to_owned())?;
        result.revision_count = indexed_revision_count(&connection)?;
        result.diagnostics.extend(state_diagnostics(&state)?);
        let revision_ids = result
            .entries
            .iter()
            .map(|entry| entry.revision_id.clone())
            .collect::<Vec<_>>();
        let mut overview_events = events;
        overview_events.extend(hydrate_events(
            service,
            &page_revision_superseder_event_ids(&connection, as_of, &revision_ids)?,
            as_of,
        )?);
        normalize_hydrated_events(&mut overview_events);
        let overviews = revision_overviews_from_selected_events(
            backend,
            overview_events,
            &revision_ids,
            &trust_set,
            RemovalPolicy::default(),
            Some(snapshot_summaries.as_ref()),
        )
        .map_err(|error| error.to_string())?;
        let next = has_more
            .then(|| rows.last())
            .flatten()
            .map(|cursor| request.next(ACTIVE_REVISION_PAGE_PROFILE, &projection_stamp, cursor));
        Ok(DerivedRevisionPageRoute::Ready(DerivedRevisionPage {
            as_of: projection_stamp.clone(),
            projection_stamp,
            next,
            work: RevisionPageWork {
                rows_selected,
                entries_returned: result.entries.len(),
            },
            result,
            overviews,
        }))
    }

    pub fn revision_detail(
        &self,
        revision_id: &RevisionId,
        options: RevisionShowOptions,
    ) -> Result<DerivedRevisionDetailRoute, String> {
        let DerivedHistoryMode::Active {
            store_identity,
            backend,
            ..
        } = &self.mode
        else {
            return Ok(DerivedRevisionDetailRoute::Off);
        };
        let current = match self.current()? {
            CurrentRead::Ready(current) => current,
            CurrentRead::Unavailable(_) => {
                return Ok(DerivedRevisionDetailRoute::ExactFallback);
            }
        };
        let service = current.service();
        let (connection, _) = match service
            .product_history_connection()
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(context) => context,
            LocatorRead::CatchUpRequired { .. } => {
                return Ok(DerivedRevisionDetailRoute::ExactFallback);
            }
        };
        let as_of = service
            .locator_checkpoint()
            .map_err(|error| error.to_string())?;
        if !revision_exists(&connection, revision_id, as_of)? {
            return Ok(DerivedRevisionDetailRoute::Ready(None));
        }
        let events = hydrate_revision_events(
            service,
            &connection,
            revision_event_ids(&connection, as_of.epoch, as_of.sequence, Some(revision_id))?,
            as_of,
        )?;
        let supersession =
            SupersessionView::from_events(&events).map_err(|error| error.to_string())?;
        let mut result = show_revision_from_selected_events(options, backend, events)
            .map_err(|error| error.to_string())?;
        result.event_count = usize::try_from(as_of.sequence)
            .map_err(|_| "derived revision event count does not fit usize".to_owned())?;
        Ok(DerivedRevisionDetailRoute::Ready(Some(Box::new(
            DerivedRevisionDetail {
                projection_stamp: projection_stamp(store_identity, as_of)?,
                result,
                supersession,
            },
        ))))
    }
}

fn revision_page_rows(
    connection: &rusqlite::Connection,
    as_of: super::cursor::TruthCursor,
    after: Option<&RevisionPageCursor>,
    limit: usize,
) -> Result<Vec<RevisionPageCursor>, String> {
    let (sql, parameters) = revision_page_query(as_of, after, limit)?;
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
            Ok(RevisionPageCursor {
                captured_at_millis: row.get(0)?,
                revision_id: RevisionId::new(row.get::<_, String>(1)?),
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn revision_page_query(
    as_of: super::cursor::TruthCursor,
    after: Option<&RevisionPageCursor>,
    limit: usize,
) -> Result<(String, Vec<Value>), String> {
    let mut sql = "SELECT revision.captured_at_millis, revision.revision_id
         FROM product_revision AS revision INDEXED BY product_revision_chronological
         JOIN semantic_representative AS representative
           ON representative.family_id = 1
          AND representative.sequence = revision.sequence
         JOIN locator_event_text AS locator ON locator.sequence = revision.sequence
         WHERE locator.epoch = ?1
           AND revision.sequence <= ?2"
        .to_owned();
    let mut parameters = vec![
        Value::Integer(to_sql_integer(as_of.epoch)?),
        Value::Integer(to_sql_integer(as_of.sequence)?),
    ];
    if let Some(after) = after {
        sql.push_str(
            " AND (revision.captured_at_millis < ?3 OR
                    (revision.captured_at_millis = ?3 AND revision.revision_id < ?4))",
        );
        parameters.push(Value::Integer(after.captured_at_millis));
        parameters.push(Value::Text(after.revision_id.as_str().to_owned()));
    }
    sql.push_str(" ORDER BY revision.captured_at_millis DESC, revision.revision_id DESC LIMIT ?");
    parameters.push(Value::Integer(to_sql_integer(limit)?));
    Ok((sql, parameters))
}

fn indexed_revision_count(connection: &rusqlite::Connection) -> Result<usize, String> {
    let count = connection
        .query_row(
            "SELECT revision_count FROM semantic_state_projection WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())?;
    usize::try_from(count).map_err(|_| "derived revision count does not fit usize".to_owned())
}

fn page_revision_event_ids(
    connection: &rusqlite::Connection,
    as_of: super::cursor::TruthCursor,
    revision_ids: &[RevisionId],
) -> Result<Vec<String>, String> {
    if revision_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (0..revision_ids.len())
        .map(|index| format!("?{}", index + 3))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT locator.event_id
         FROM semantic_event_fact_text AS event
         JOIN locator_event_text AS locator ON locator.sequence = event.sequence
         WHERE locator.epoch = ?1
           AND locator.sequence <= ?2
           AND event.revision_id IN ({placeholders})
         ORDER BY locator.replay_key, locator.event_id"
    );
    let mut parameters = vec![
        Value::Integer(to_sql_integer(as_of.epoch)?),
        Value::Integer(to_sql_integer(as_of.sequence)?),
    ];
    parameters.extend(
        revision_ids
            .iter()
            .map(|revision_id| Value::Text(revision_id.as_str().to_owned())),
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

/// Select only the proposal carriers for direct successors of the page's
/// revisions. A successor may sit on a later/earlier page, but its edge is
/// still needed to compute the selected revision's `superseded_by` and stale
/// review-fact attention readback. Keeping these carriers out of
/// `page_revision_event_ids` prevents the successor itself from leaking into
/// the page collection; the target index keeps the supplemental work bounded
/// by the selected identities and their direct edges.
fn page_revision_superseder_event_ids(
    connection: &rusqlite::Connection,
    as_of: super::cursor::TruthCursor,
    revision_ids: &[RevisionId],
) -> Result<Vec<String>, String> {
    if revision_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (0..revision_ids.len())
        .map(|index| format!("?{}", index + 3))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT locator.event_id
         FROM product_revision_edge AS edge
           INDEXED BY product_revision_edge_target
         JOIN locator_event_text AS locator ON locator.sequence = edge.sequence
         WHERE locator.epoch = ?1
           AND locator.sequence <= ?2
           AND edge.superseded_revision_id IN ({placeholders})
         ORDER BY locator.replay_key, locator.event_id"
    );
    let mut parameters = vec![
        Value::Integer(to_sql_integer(as_of.epoch)?),
        Value::Integer(to_sql_integer(as_of.sequence)?),
    ];
    parameters.extend(
        revision_ids
            .iter()
            .map(|revision_id| Value::Text(revision_id.as_str().to_owned())),
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?;
    let mut event_ids = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    event_ids.dedup();
    Ok(event_ids)
}

fn revision_event_ids(
    connection: &rusqlite::Connection,
    epoch: u64,
    sequence: u64,
    revision_id: Option<&RevisionId>,
) -> Result<Vec<String>, String> {
    let sql = if revision_id.is_some() {
        REVISION_COMPONENT_EVENT_IDS_SQL
    } else {
        "SELECT locator.event_id
         FROM semantic_event_fact_text AS event
         JOIN locator_event_text AS locator ON locator.sequence = event.sequence
         WHERE locator.epoch = ?1
           AND locator.sequence <= ?2
           AND event.revision_id IS NOT NULL
         ORDER BY locator.replay_key, locator.event_id"
    };
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let mut parameters = vec![
        Value::Integer(to_sql_integer(epoch)?),
        Value::Integer(to_sql_integer(sequence)?),
    ];
    if let Some(revision_id) = revision_id {
        parameters.push(Value::Text(revision_id.as_str().to_owned()));
    }
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn revision_exists(
    connection: &rusqlite::Connection,
    revision_id: &RevisionId,
    as_of: super::cursor::TruthCursor,
) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1
                 FROM product_revision AS revision
                 JOIN semantic_representative AS representative
                   ON representative.family_id = 1
                  AND representative.sequence = revision.sequence
                 JOIN locator_event_text AS locator ON locator.sequence = revision.sequence
                 WHERE revision.revision_id = ?1
                   AND locator.epoch = ?2
                   AND revision.sequence <= ?3
             )",
            rusqlite::params![
                revision_id.as_str(),
                to_sql_integer(as_of.epoch)?,
                to_sql_integer(as_of.sequence)?,
            ],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| error.to_string())
}

fn hydrate_revision_events(
    service: &super::service::DerivedAccessService,
    connection: &rusqlite::Connection,
    event_ids: Vec<String>,
    as_of: super::cursor::TruthCursor,
) -> Result<Vec<ShoreEvent>, String> {
    let selected = hydrate_events(service, &event_ids, as_of)?;
    let support_ids = support_event_ids(connection, &selected, as_of)?;
    let mut events = selected;
    events.extend(hydrate_events(service, &support_ids, as_of)?);
    normalize_hydrated_events(&mut events);
    Ok(events)
}

fn normalize_hydrated_events(events: &mut Vec<ShoreEvent>) {
    events.sort_by(|left, right| {
        sha256_bytes_hex(left.idempotency_key.as_bytes())
            .cmp(&sha256_bytes_hex(right.idempotency_key.as_bytes()))
    });
    events.dedup_by(|left, right| left.event_id == right.event_id);
}

fn to_sql_integer(value: impl TryInto<i64>) -> Result<i64, String> {
    value
        .try_into()
        .map_err(|_| "revision value does not fit SQLite INTEGER".to_owned())
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::Mutex;

    use tempfile::TempDir;

    use super::*;
    use crate::documents::revision_show_document;
    use crate::model::JournalId;
    use crate::session::derived_access::lifecycle::{DerivedAccessLifecycle, LifecycleControl};
    use crate::session::derived_access::product_contract::DerivedAccessProfile;
    use crate::session::derived_access::writer::DerivedWriteCoordinator;
    use crate::session::event::{
        ArtifactRemovedPayload, EventTarget, EventType, ReviewInitializedPayload, ShoreEvent,
        WorkObjectProposedPayload, Writer,
    };
    use crate::session::store::resolution::resolve_read_store;
    use crate::session::workflow::{
        CaptureOptions, RevisionOverviewsOptions, capture_worktree_review, show_revision,
        show_revision_for_inspector, show_revision_overviews,
    };
    use crate::session::{EventStore, EventWriteOutcome};

    fn git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn active_captured_repo() -> (TempDir, DerivedHistoryAccess, RevisionId) {
        let repo = TempDir::new().expect("create repository");
        git(repo.path(), &["init"]);
        git(repo.path(), &["config", "user.name", "Pointbreak Tests"]);
        git(
            repo.path(),
            &["config", "user.email", "pointbreak-tests@example.com"],
        );
        git(repo.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.path().join("source.txt"), "before\n").expect("write base");
        git(repo.path(), &["add", "--all"]);
        git(repo.path(), &["commit", "-m", "base"]);
        std::fs::write(repo.path().join("source.txt"), "after\n").expect("write change");
        let capture =
            capture_worktree_review(CaptureOptions::new(repo.path())).expect("capture revision");
        git(repo.path(), &["checkout", "--", "source.txt"]);
        std::fs::write(repo.path().join("source.txt"), "successor\n")
            .expect("write successor change");
        capture_worktree_review(
            CaptureOptions::new(repo.path()).with_supersedes(vec![capture.revision_id.clone()]),
        )
        .expect("capture successor revision");
        git(repo.path(), &["checkout", "--", "source.txt"]);
        std::fs::write(repo.path().join("source.txt"), "competing successor\n")
            .expect("write competing change");
        capture_worktree_review(
            CaptureOptions::new(repo.path()).with_supersedes(vec![capture.revision_id.clone()]),
        )
        .expect("capture competing successor revision");

        let read_store = resolve_read_store(repo.path()).expect("resolve store");
        let event_store = EventStore::open(read_store.store_dir());
        for index in 0..8 {
            let journal_id = JournalId::new(format!("journal:unrelated:{index}"));
            let event = ShoreEvent::new(
                EventType::ReviewInitialized,
                ReviewInitializedPayload::idempotency_key(&journal_id),
                EventTarget::for_journal(journal_id),
                Writer::shore_local("test"),
                ReviewInitializedPayload {},
                format!("2026-07-28T13:00:{index:02}Z"),
            )
            .unwrap();
            assert_eq!(
                event_store.record_event_once(&event).unwrap(),
                EventWriteOutcome::Created
            );
        }
        let unrelated_removal = ShoreEvent::new(
            EventType::ArtifactRemoved,
            ArtifactRemovedPayload::idempotency_key(&format!("sha256:{}", "ab".repeat(32))),
            EventTarget::for_journal(JournalId::new("journal:unrelated-removal")),
            Writer::shore_local("test"),
            ArtifactRemovedPayload {
                content_hash: format!("sha256:{}", "ab".repeat(32)),
            },
            "2026-07-28T13:01:00Z",
        )
        .unwrap();
        assert_eq!(
            event_store.record_event_once(&unrelated_removal).unwrap(),
            EventWriteOutcome::Created
        );
        let lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            read_store.store_dir(),
            "store:test",
        )
        .expect("create lifecycle");
        lifecycle
            .rebuild(|_| LifecycleControl::Continue)
            .expect("publish generation");
        let access = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Active {
            lifecycle,
            current: Mutex::new(None),
            store_identity: "store:test".to_owned(),
            backend: read_store.backend().clone(),
        });
        (repo, access, capture.revision_id)
    }

    fn active_bridged_repo() -> (TempDir, DerivedHistoryAccess, RevisionId) {
        let repo = TempDir::new().expect("create repository");
        git(repo.path(), &["init"]);
        git(repo.path(), &["config", "user.name", "Pointbreak Tests"]);
        git(
            repo.path(),
            &["config", "user.email", "pointbreak-tests@example.com"],
        );
        git(repo.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.path().join("source.txt"), "before\n").expect("write base");
        git(repo.path(), &["add", "--all"]);
        git(repo.path(), &["commit", "-m", "base"]);

        std::fs::write(repo.path().join("source.txt"), "first root\n").expect("write first root");
        let first =
            capture_worktree_review(CaptureOptions::new(repo.path())).expect("capture first root");
        git(repo.path(), &["checkout", "--", "source.txt"]);

        std::fs::write(repo.path().join("source.txt"), "second root\n").expect("write second root");
        let second =
            capture_worktree_review(CaptureOptions::new(repo.path())).expect("capture second root");
        git(repo.path(), &["checkout", "--", "source.txt"]);

        std::fs::write(repo.path().join("source.txt"), "bridge\n").expect("write bridge");
        capture_worktree_review(
            CaptureOptions::new(repo.path())
                .with_supersedes(vec![first.revision_id.clone(), second.revision_id]),
        )
        .expect("capture bridge");

        let read_store = resolve_read_store(repo.path()).expect("resolve store");
        let lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            read_store.store_dir(),
            "store:test",
        )
        .expect("create lifecycle");
        lifecycle
            .rebuild(|_| LifecycleControl::Continue)
            .expect("publish generation");
        let access = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Active {
            lifecycle,
            current: Mutex::new(None),
            store_identity: "store:test".to_owned(),
            backend: read_store.backend().clone(),
        });
        (repo, access, first.revision_id)
    }

    fn revision_proposal_at(occurred_at: &str) -> (ShoreEvent, RevisionId) {
        let donor = TempDir::new().expect("create donor repository");
        git(donor.path(), &["init"]);
        git(donor.path(), &["config", "user.name", "Pointbreak Tests"]);
        git(
            donor.path(),
            &["config", "user.email", "pointbreak-tests@example.com"],
        );
        git(donor.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(donor.path().join("donor.txt"), "before\n").expect("write donor base");
        git(donor.path(), &["add", "--all"]);
        git(donor.path(), &["commit", "-m", "base"]);
        std::fs::write(donor.path().join("donor.txt"), "after\n").expect("write donor change");
        let capture =
            capture_worktree_review(CaptureOptions::new(donor.path())).expect("capture donor");
        let donor_store = resolve_read_store(donor.path()).expect("resolve donor store");
        let proposal = EventStore::open(donor_store.store_dir())
            .list_events()
            .expect("list donor events")
            .into_iter()
            .find(|event| event.event_type == EventType::WorkObjectProposed)
            .expect("donor proposal");
        let payload: WorkObjectProposedPayload =
            serde_json::from_value(proposal.payload).expect("decode donor proposal");
        let event = ShoreEvent::new(
            EventType::WorkObjectProposed,
            proposal.idempotency_key,
            proposal.target,
            proposal.writer,
            payload,
            occurred_at,
        )
        .expect("mint backdated proposal");
        (event, capture.revision_id)
    }

    fn backdated_revision_proposal() -> (ShoreEvent, RevisionId) {
        revision_proposal_at("2000-01-01T00:00:00Z")
    }

    #[test]
    fn active_exact_detail_matches_authoritative_projection_and_supersession() {
        let (repo, access, revision_id) = active_captured_repo();
        let options = || {
            RevisionShowOptions::new(repo.path())
                .with_revision_id(revision_id.clone())
                .with_exact(true)
                .with_include_body(true)
                .with_read_for_display(true)
                .with_verification_policy(crate::session::EventVerificationPolicy::advisory())
        };
        let audit = show_revision(options()).expect("read store-wide authoritative detail");
        let authoritative =
            show_revision_for_inspector(options()).expect("read authoritative detail");
        assert!(
            audit
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "snapshot_content_removed_target_missing" })
        );
        assert!(
            !authoritative
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "snapshot_content_removed_target_missing" })
        );
        assert_eq!(authoritative.event_count, audit.event_count);
        assert_eq!(authoritative.event_set_hash, audit.event_set_hash);
        let DerivedRevisionDetailRoute::Ready(Some(derived)) = access
            .revision_detail(&revision_id, options())
            .expect("read derived detail")
        else {
            panic!("published exact revision should be current");
        };
        let mut authoritative =
            serde_json::to_value(revision_show_document(authoritative)).unwrap();
        let mut selected = serde_json::to_value(revision_show_document(derived.result)).unwrap();
        authoritative
            .as_object_mut()
            .expect("detail document")
            .remove("eventSetHash");
        selected
            .as_object_mut()
            .expect("detail document")
            .remove("eventSetHash");

        assert_eq!(selected, authoritative);
        let (events, _) =
            crate::session::read_events_for_display(repo.path()).expect("read full events");
        assert_eq!(
            derived.supersession,
            SupersessionView::from_events(&events).expect("project full supersession")
        );
    }

    #[test]
    fn active_exact_detail_follows_supersession_bridges_across_engagement_hints() {
        let (repo, access, revision_id) = active_bridged_repo();
        let options = || {
            RevisionShowOptions::new(repo.path())
                .with_revision_id(revision_id.clone())
                .with_exact(true)
                .with_include_body(true)
                .with_read_for_display(true)
                .with_verification_policy(crate::session::EventVerificationPolicy::advisory())
        };
        let authoritative = show_revision(options()).expect("read authoritative detail");
        let DerivedRevisionDetailRoute::Ready(Some(derived)) = access
            .revision_detail(&revision_id, options())
            .expect("read derived detail")
        else {
            panic!("published exact revision should be current");
        };
        let mut authoritative =
            serde_json::to_value(revision_show_document(authoritative)).unwrap();
        let mut selected = serde_json::to_value(revision_show_document(derived.result)).unwrap();
        authoritative
            .as_object_mut()
            .expect("detail document")
            .remove("eventSetHash");
        selected
            .as_object_mut()
            .expect("detail document")
            .remove("eventSetHash");

        assert_eq!(selected, authoritative);
        let (events, _) =
            crate::session::read_events_for_display(repo.path()).expect("read full events");
        assert_eq!(
            derived.supersession,
            SupersessionView::from_events(&events).expect("project full supersession")
        );
    }

    #[test]
    fn exact_detail_anchors_component_walks_before_scanning_retained_history() {
        let (_repo, access, revision_id) = active_bridged_repo();
        let CurrentRead::Ready(current) = access.current().expect("read current generation") else {
            panic!("published generation should be current");
        };
        let service = current.service();
        let LocatorRead::Ready((connection, _)) = service
            .product_history_connection()
            .expect("open product history")
        else {
            panic!("published generation should not require catch-up");
        };
        let as_of = service.locator_checkpoint().expect("read checkpoint");
        let mut statement = connection
            .prepare(&format!(
                "EXPLAIN QUERY PLAN {REVISION_COMPONENT_EVENT_IDS_SQL}"
            ))
            .expect("prepare exact-detail query plan");
        let details = statement
            .query_map(
                rusqlite::params![
                    to_sql_integer(as_of.epoch).unwrap(),
                    to_sql_integer(as_of.sequence).unwrap(),
                    revision_id.as_str(),
                ],
                |row| row.get::<_, String>(3),
            )
            .expect("query exact-detail plan")
            .collect::<Result<Vec<_>, _>>()
            .expect("read exact-detail plan");

        assert!(
            details.iter().any(|detail| {
                detail.contains("product_revision_identity") && detail.contains("revision_id=?")
            }),
            "backward component expansion must start from the selected revision: {details:?}"
        );
        assert!(
            details.iter().any(|detail| {
                detail.contains("product_revision_edge_target")
                    && detail.contains("superseded_revision_id=?")
            }),
            "forward component expansion must start from the selected revision: {details:?}"
        );
    }

    #[test]
    fn exact_miss_is_final_only_at_current_coverage() {
        let (repo, access, _) = active_captured_repo();
        let missing = RevisionId::new(format!("rev:sha256:{}", "99".repeat(32)));
        let options = || {
            RevisionShowOptions::new(repo.path())
                .with_revision_id(missing.clone())
                .with_exact(true)
                .with_include_body(true)
                .with_read_for_display(true)
        };

        assert!(matches!(
            access.revision_detail(&missing, options()).unwrap(),
            DerivedRevisionDetailRoute::Ready(None)
        ));

        let read_store = resolve_read_store(repo.path()).unwrap();
        let journal_id = JournalId::new("journal:derived-revision-miss");
        let appended = ShoreEvent::new(
            EventType::ReviewInitialized,
            ReviewInitializedPayload::idempotency_key(&journal_id),
            EventTarget::for_journal(journal_id),
            Writer::shore_local("test"),
            ReviewInitializedPayload {},
            "2026-07-28T12:00:00Z",
        )
        .unwrap();
        assert_eq!(
            EventStore::open(read_store.store_dir())
                .record_event_once(&appended)
                .unwrap(),
            EventWriteOutcome::Created
        );

        assert!(matches!(
            access.revision_detail(&missing, options()).unwrap(),
            DerivedRevisionDetailRoute::ExactFallback
        ));
    }

    #[test]
    fn revision_page_request_enforces_the_frozen_limits_and_token_syntax() {
        assert_eq!(RevisionPageRequest::new(None, None).unwrap().limit(), 100);
        assert_eq!(
            RevisionPageRequest::new(Some(500), None).unwrap().limit(),
            500
        );
        assert_eq!(
            RevisionPageRequest::new(Some(501), None).unwrap_err(),
            RevisionPageRequestError::InvalidRequest
        );
        assert_eq!(
            RevisionPageRequest::new(Some(1), Some("not-base64!!")).unwrap_err(),
            RevisionPageRequestError::InvalidRequest
        );
    }

    #[test]
    fn active_revision_pages_are_snapshot_bound_and_limit_plus_one() {
        let (repo, access, _) = active_captured_repo();
        let summaries = Arc::new(SnapshotSummaryCache::new());
        let request = RevisionPageRequest::new(Some(1), None).unwrap();
        let DerivedRevisionPageRoute::Ready(first) = access
            .revisions_page(
                repo.path(),
                TrustSet::default(),
                Arc::clone(&summaries),
                &request,
            )
            .expect("read first revision page")
        else {
            panic!("published generation should serve a revision page");
        };
        assert_eq!(first.result.entries.len(), 1);
        assert_eq!(first.work.rows_selected, 2);
        assert_eq!(first.work.entries_returned, 1);
        assert_eq!(first.result.revision_count, 3);
        let first_id = first.result.entries[0].revision_id.clone();
        let next = first.next.clone().expect("first page has a continuation");

        let request = RevisionPageRequest::new(Some(1), Some(&next)).unwrap();
        let DerivedRevisionPageRoute::Ready(second) = access
            .revisions_page(
                repo.path(),
                TrustSet::default(),
                Arc::clone(&summaries),
                &request,
            )
            .expect("read second revision page")
        else {
            panic!("same snapshot should accept its continuation");
        };
        assert_eq!(second.result.entries.len(), 1);
        assert_ne!(second.result.entries[0].revision_id, first_id);
        assert_eq!(second.as_of, first.as_of);
        assert!(second.work.rows_selected <= 2);
    }

    #[test]
    fn active_revision_page_overview_includes_off_page_superseders() {
        let (repo, access, superseded_revision_id) = active_captured_repo();
        let expected = show_revision_overviews(
            RevisionOverviewsOptions::new(repo.path())
                .with_revisions([superseded_revision_id.clone()])
                .with_read_for_display(true),
        )
        .expect("read authoritative overview")
        .remove(&superseded_revision_id)
        .expect("authoritative overview includes selected revision");
        assert!(
            !expected.superseded_by.is_empty(),
            "fixture revision must be superseded"
        );

        let summaries = Arc::new(SnapshotSummaryCache::new());
        let mut after = None;
        loop {
            let request = RevisionPageRequest::new(Some(1), after.as_deref())
                .expect("build revision page request");
            let DerivedRevisionPageRoute::Ready(page) = access
                .revisions_page(
                    repo.path(),
                    TrustSet::default(),
                    Arc::clone(&summaries),
                    &request,
                )
                .expect("read active revision page")
            else {
                panic!("published generation should serve a revision page");
            };
            if page.result.entries[0].revision_id == superseded_revision_id {
                let actual = page
                    .overviews
                    .get(&superseded_revision_id)
                    .expect("derived overview includes selected revision");
                assert_eq!(actual.superseded_by, expected.superseded_by);
                break;
            }
            after = page.next;
            assert!(
                after.is_some(),
                "selected superseded revision was not paged"
            );
        }
    }

    #[test]
    fn active_revision_page_query_uses_the_chronological_index() {
        let (_repo, access, _) = active_captured_repo();
        let CurrentRead::Ready(current) = access.current().expect("read current generation") else {
            panic!("published generation should be current");
        };
        let service = current.service();
        let LocatorRead::Ready((connection, _)) = service
            .product_history_connection()
            .expect("open product history")
        else {
            panic!("published generation should not require catch-up");
        };
        let as_of = service.locator_checkpoint().expect("read checkpoint");
        for cursor in [
            None,
            Some(RevisionPageCursor {
                captured_at_millis: 1_900_000_000_000,
                revision_id: RevisionId::new("rev:sha256:cursor"),
            }),
        ] {
            let (sql, parameters) =
                revision_page_query(as_of, cursor.as_ref(), 101).expect("build page query");
            let explain = format!("EXPLAIN QUERY PLAN {sql}");
            let details = connection
                .prepare(&explain)
                .expect("prepare production revision page plan")
                .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
                    row.get::<_, String>(3)
                })
                .expect("query revision page plan")
                .collect::<Result<Vec<_>, _>>()
                .expect("read revision page plan");

            assert!(
                details
                    .iter()
                    .any(|detail| detail.contains("product_revision_chronological")),
                "revision pages must scan their stable ordering index: {details:?}"
            );
            assert!(
                details
                    .iter()
                    .all(|detail| !detail.contains("USE TEMP B-TREE")),
                "revision pages must not sort a complete intermediate: {details:?}"
            );
        }
    }

    #[test]
    fn backdated_append_moves_the_snapshot_and_requires_page_restart() {
        let (repo, access, _) = active_captured_repo();
        let summaries = Arc::new(SnapshotSummaryCache::new());
        let first_request = RevisionPageRequest::new(Some(1), None).unwrap();
        let DerivedRevisionPageRoute::Ready(first) = access
            .revisions_page(
                repo.path(),
                TrustSet::default(),
                Arc::clone(&summaries),
                &first_request,
            )
            .expect("read first revision page")
        else {
            panic!("published generation should serve a revision page");
        };
        let token = first.next.expect("first page continuation");

        let DerivedHistoryMode::Active { lifecycle, .. } = &access.mode else {
            panic!("test access is active");
        };
        let coordinator = DerivedWriteCoordinator::new(lifecycle.clone()).expect("active writer");
        let read_store = resolve_read_store(repo.path()).expect("resolve target store");
        let target_store = EventStore::open(read_store.store_dir());
        let (backdated, _revision_id) = backdated_revision_proposal();
        assert_eq!(
            coordinator
                .record_event_once(&backdated, || target_store.record_event_once(&backdated))
                .expect("publish and catch up backdated proposal"),
            EventWriteOutcome::Created
        );

        let stale = RevisionPageRequest::new(Some(1), Some(&token)).unwrap();
        assert!(matches!(
            access
                .revisions_page(
                    repo.path(),
                    TrustSet::default(),
                    Arc::clone(&summaries),
                    &stale,
                )
                .expect("classify stale continuation"),
            DerivedRevisionPageRoute::RestartRequired
        ));
        let complete_request = RevisionPageRequest::new(Some(500), None).unwrap();
        let DerivedRevisionPageRoute::Ready(restarted) = access
            .revisions_page(
                repo.path(),
                TrustSet::default(),
                summaries,
                &complete_request,
            )
            .expect("restart revision traversal")
        else {
            panic!("caught-up generation should serve a fresh page");
        };
        assert_eq!(restarted.result.revision_count, 4);
        assert_eq!(
            restarted.result.entries.last().unwrap().captured_at,
            "2000-01-01T00:00:00Z"
        );
    }

    #[test]
    fn revision_pages_start_with_the_newest_normalized_instant() {
        let (repo, access, _) = active_captured_repo();
        let DerivedHistoryMode::Active { lifecycle, .. } = &access.mode else {
            panic!("test access is active");
        };
        let coordinator = DerivedWriteCoordinator::new(lifecycle.clone()).expect("active writer");
        let read_store = resolve_read_store(repo.path()).expect("resolve target store");
        let target_store = EventStore::open(read_store.store_dir());
        let (legacy, legacy_id) = revision_proposal_at("unix-ms:1893456000000");
        let (rfc3339, rfc3339_id) = revision_proposal_at("2040-01-01T00:00:00Z");
        for event in [&legacy, &rfc3339] {
            assert_eq!(
                coordinator
                    .record_event_once(event, || target_store.record_event_once(event))
                    .expect("publish and catch up proposal"),
                EventWriteOutcome::Created
            );
        }

        let request = RevisionPageRequest::new(Some(2), None).unwrap();
        let DerivedRevisionPageRoute::Ready(page) = access
            .revisions_page(
                repo.path(),
                TrustSet::default(),
                Arc::new(SnapshotSummaryCache::new()),
                &request,
            )
            .expect("read newest page")
        else {
            panic!("published generation should serve a revision page");
        };

        assert_eq!(
            page.result
                .entries
                .iter()
                .map(|entry| &entry.revision_id)
                .collect::<Vec<_>>(),
            vec![&rfc3339_id, &legacy_id]
        );
    }

    #[test]
    fn revision_page_token_rejects_a_different_profile_or_snapshot() {
        let token = RevisionPageToken::new(
            "sqlite-wal-bodyless-v1",
            "snapshot:a",
            1_775_000_000_000,
            &RevisionId::new("rev:sha256:a"),
        )
        .encode();
        let request = RevisionPageRequest::new(Some(1), Some(&token)).unwrap();
        assert_eq!(
            request.cursor("authoritative-loose-v1", "snapshot:a"),
            Err(RevisionPageRequestError::RestartRequired)
        );
        assert_eq!(
            request.cursor("sqlite-wal-bodyless-v1", "snapshot:b"),
            Err(RevisionPageRequestError::RestartRequired)
        );
    }
}
