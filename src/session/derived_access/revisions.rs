//! Product revision collection and exact-detail reads over the active derived generation.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use rusqlite::types::Value;

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

#[doc(hidden)]
pub enum DerivedRevisionCollectionRoute {
    Off,
    Ready(DerivedRevisionCollection),
    Unavailable(DerivedHistoryStatus),
}

#[doc(hidden)]
pub enum DerivedRevisionDetailRoute {
    Off,
    Ready(Option<Box<DerivedRevisionDetail>>),
    ExactFallback,
}

#[doc(hidden)]
pub struct DerivedRevisionCollection {
    pub projection_stamp: String,
    pub result: RevisionListResult,
    pub overviews: BTreeMap<RevisionId, RevisionOverview>,
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
    pub fn revisions(
        &self,
        repo: &Path,
        trust_set: TrustSet,
        snapshot_summaries: Arc<SnapshotSummaryCache>,
    ) -> Result<DerivedRevisionCollectionRoute, String> {
        let DerivedHistoryMode::Active {
            store_identity,
            backend,
            ..
        } = &self.mode
        else {
            return Ok(DerivedRevisionCollectionRoute::Off);
        };
        let current = match self.current()? {
            CurrentRead::Ready(current) => current,
            CurrentRead::Unavailable(status) => {
                return Ok(DerivedRevisionCollectionRoute::Unavailable(status));
            }
        };
        let service = current.service();
        let (connection, state) = match service
            .product_history_connection()
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(context) => context,
            LocatorRead::CatchUpRequired { .. } => {
                return Ok(DerivedRevisionCollectionRoute::Unavailable(
                    catching_up_status(),
                ));
            }
        };
        let as_of = service
            .locator_checkpoint()
            .map_err(|error| error.to_string())?;
        let events = hydrate_revision_events(
            service,
            &connection,
            revision_event_ids(&connection, as_of.epoch, as_of.sequence, None)?,
            as_of,
        )?;
        let mut result = list_revisions_from_selected_events(
            RevisionListOptions::new(repo).with_read_for_display(true),
            events.clone(),
        )
        .map_err(|error| error.to_string())?;
        result.event_count = usize::try_from(as_of.sequence)
            .map_err(|_| "derived revision event count does not fit usize".to_owned())?;
        result.diagnostics.extend(state_diagnostics(&state)?);
        let revision_ids = result
            .entries
            .iter()
            .map(|entry| entry.revision_id.clone())
            .collect::<Vec<_>>();
        let overviews = revision_overviews_from_selected_events(
            backend,
            events,
            &revision_ids,
            &trust_set,
            RemovalPolicy::default(),
            Some(snapshot_summaries.as_ref()),
        )
        .map_err(|error| error.to_string())?;
        Ok(DerivedRevisionCollectionRoute::Ready(
            DerivedRevisionCollection {
                projection_stamp: projection_stamp(store_identity, as_of)?,
                result,
                overviews,
            },
        ))
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
    events.sort_by(|left, right| {
        sha256_bytes_hex(left.idempotency_key.as_bytes())
            .cmp(&sha256_bytes_hex(right.idempotency_key.as_bytes()))
    });
    events.dedup_by(|left, right| left.event_id == right.event_id);
    Ok(events)
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
    use crate::session::event::{
        ArtifactRemovedPayload, EventTarget, EventType, ReviewInitializedPayload, ShoreEvent,
        Writer,
    };
    use crate::session::store::resolution::resolve_read_store;
    use crate::session::workflow::{
        CaptureOptions, RevisionOverviewsOptions, capture_worktree_review, list_revisions,
        show_revision, show_revision_for_inspector, show_revision_overviews,
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

    #[test]
    fn active_collection_matches_authoritative_revision_and_overview_projections() {
        let (repo, access, _revision_id) = active_captured_repo();
        let trust_set = TrustSet::default();
        let summaries = Arc::new(SnapshotSummaryCache::new());
        let scope =
            crate::bench_support::longitudinal::LongitudinalCountingScopeV1::new("d".repeat(64))
                .unwrap();
        let _guard = scope.enter();
        let DerivedRevisionCollectionRoute::Ready(derived) = access
            .revisions(repo.path(), trust_set.clone(), Arc::clone(&summaries))
            .expect("read derived revisions")
        else {
            panic!("published generation should be current");
        };
        let counters = scope.snapshot();
        assert!(counters.counters.carrier_opens < derived.result.event_count as u64);
        assert!(counters.counters.event_decodes < derived.result.event_count as u64);
        assert!(derived.result.event_count >= 10);
        let authoritative =
            list_revisions(RevisionListOptions::new(repo.path()).with_read_for_display(true))
                .expect("read authoritative revisions");
        let authoritative_overviews = show_revision_overviews(
            RevisionOverviewsOptions::new(repo.path())
                .with_revisions(
                    authoritative
                        .entries
                        .iter()
                        .map(|entry| entry.revision_id.clone()),
                )
                .with_read_for_display(true)
                .with_trust_set(trust_set)
                .with_snapshot_summary_cache(summaries),
        )
        .expect("read authoritative overviews");

        assert_eq!(
            serde_json::to_value(&derived.result.entries).unwrap(),
            serde_json::to_value(&authoritative.entries).unwrap()
        );
        assert_eq!(derived.result.event_count, authoritative.event_count);
        assert_eq!(derived.result.revision_count, authoritative.revision_count);
        assert_eq!(derived.result.diagnostics, authoritative.diagnostics);
        assert_eq!(derived.overviews, authoritative_overviews);
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
}
