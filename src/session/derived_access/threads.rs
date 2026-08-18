//! Product supersession-thread reads over the active derived generation.

use std::collections::BTreeMap;

use super::history::{
    CurrentRead, DerivedHistoryAccess, DerivedHistoryStatus, catching_up_status, projection_stamp,
    state_diagnostics,
};
use super::locator::LocatorRead;
use crate::model::RevisionId;
use crate::session::{ProjectionDiagnostic, SupersessionView};

#[doc(hidden)]
pub enum DerivedThreadsRoute {
    Off,
    Ready(DerivedThreads),
    Unavailable(DerivedHistoryStatus),
}

#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct DerivedThreads {
    pub projection_stamp: String,
    pub event_count: usize,
    pub supersession: SupersessionView,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

impl DerivedHistoryAccess {
    pub fn threads(&self) -> Result<DerivedThreadsRoute, String> {
        let Some((store_identity, _)) = self.active_context() else {
            return Ok(DerivedThreadsRoute::Off);
        };
        let current = match self.current()? {
            CurrentRead::Ready(current) => current,
            CurrentRead::Unavailable(status) => {
                return Ok(DerivedThreadsRoute::Unavailable(status));
            }
        };
        let service = current.service();
        let (connection, state) = match service
            .product_history_connection()
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(context) => context,
            LocatorRead::CatchUpRequired { .. } => {
                return Ok(DerivedThreadsRoute::Unavailable(catching_up_status()));
            }
        };
        let as_of = service
            .locator_checkpoint()
            .map_err(|error| error.to_string())?;
        let supersession = materialized_supersession(&connection, as_of)?;
        let mut diagnostics = supersession.diagnostics.clone();
        diagnostics.extend(state_diagnostics(&state)?);
        record_active_ownership();
        Ok(DerivedThreadsRoute::Ready(DerivedThreads {
            projection_stamp: projection_stamp(store_identity, as_of)?,
            event_count: state.event_count,
            supersession,
            diagnostics,
        }))
    }
}

fn materialized_supersession(
    connection: &rusqlite::Connection,
    as_of: super::cursor::TruthCursor,
) -> Result<SupersessionView, String> {
    let mut statement = connection
        .prepare(
            "SELECT revision.revision_id, edge.superseded_revision_id
             FROM product_revision AS revision
             JOIN semantic_representative AS representative
               ON representative.family_id = 1
              AND representative.sequence = revision.sequence
             JOIN locator_event_text AS locator ON locator.sequence = revision.sequence
             LEFT JOIN product_revision_edge AS edge ON edge.sequence = revision.sequence
             WHERE locator.epoch = ?1
               AND revision.sequence <= ?2
             ORDER BY revision.revision_id, edge.superseded_revision_id",
        )
        .map_err(|error| error.to_string())?;
    let mut rows = statement
        .query_map(
            rusqlite::params![
                to_sql_integer(as_of.epoch)?,
                to_sql_integer(as_of.sequence)?
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let mut edges = BTreeMap::<RevisionId, Vec<RevisionId>>::new();
    rows.try_for_each(|row| {
        let (revision, superseded) = row.map_err(|error| error.to_string())?;
        let supersedes = edges.entry(RevisionId::new(revision)).or_default();
        if let Some(superseded) = superseded {
            supersedes.push(RevisionId::new(superseded));
        }
        Ok::<(), String>(())
    })?;
    Ok(SupersessionView::from_edges(edges))
}

fn to_sql_integer(value: u64) -> Result<i64, String> {
    value
        .try_into()
        .map_err(|_| "thread value does not fit SQLite INTEGER".to_owned())
}

pub(super) fn record_active_ownership() {
    #[cfg(feature = "longitudinal-counting")]
    {
        crate::bench_support::longitudinal::set_retained_decoded_events(0);
        crate::bench_support::longitudinal::set_retained_hydrated_history_entries(0);
        crate::bench_support::longitudinal::set_retained_hydrated_body_bytes(0);
        crate::bench_support::longitudinal::set_retained_search_record_strings(0);
        crate::bench_support::longitudinal::set_retained_search_record_field_bytes(0);
        crate::bench_support::longitudinal::set_retained_serialized_response_cache_bytes(0);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;
    use std::sync::{Arc, Barrier, Mutex};

    use tempfile::TempDir;

    use super::*;
    use crate::model::{EngagementId, JournalId, ObjectId, RevisionId};
    use crate::session::derived_access::attention::DerivedAttentionRoute;
    use crate::session::derived_access::history::DerivedHistoryMode;
    use crate::session::derived_access::lifecycle::{DerivedAccessLifecycle, LifecycleControl};
    use crate::session::derived_access::product_contract::DerivedAccessProfile;
    use crate::session::derived_access::writer::DerivedWriteCoordinator;
    use crate::session::event::{
        EventTarget, EventType, ReviewInitializedPayload, Revision, ShoreEvent, WorkObjectProposal,
        WorkObjectProposedPayload, Writer,
    };
    use crate::session::store::resolution::resolve_read_store;
    use crate::session::workflow::{
        AttentionListOptions, CaptureOptions, capture_worktree_review, list_attention,
    };
    use crate::session::{
        EventStore, EventWriteOutcome, SupersessionView, read_events_for_display,
    };

    fn git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn active_forked_repo() -> (TempDir, DerivedHistoryAccess) {
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

        std::fs::write(repo.path().join("source.txt"), "root\n").expect("write root");
        let root = capture_worktree_review(CaptureOptions::new(repo.path())).expect("capture root");
        git(repo.path(), &["checkout", "--", "source.txt"]);
        std::fs::write(repo.path().join("source.txt"), "head b\n").expect("write head b");
        capture_worktree_review(CaptureOptions::new(repo.path()).with_supersedes(vec![
            root.revision_id.clone(),
            RevisionId::new("rev:sha256:dangling-plan-0158"),
        ]))
        .expect("capture head b");
        git(repo.path(), &["checkout", "--", "source.txt"]);
        std::fs::write(repo.path().join("source.txt"), "head c\n").expect("write head c");
        capture_worktree_review(
            CaptureOptions::new(repo.path()).with_supersedes(vec![root.revision_id]),
        )
        .expect("capture head c");

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
            .expect("build unrelated event");
            assert_eq!(
                event_store.record_event_once(&event).expect("record event"),
                EventWriteOutcome::Created
            );
        }
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
        (repo, access)
    }

    #[test]
    fn active_threads_match_authoritative_fork_projection() {
        let (repo, access) = active_forked_repo();
        let DerivedThreadsRoute::Ready(derived) = access.threads().expect("read derived threads")
        else {
            panic!("published generation should serve derived threads");
        };
        let (events, store_diagnostics) =
            read_events_for_display(repo.path()).expect("read authoritative events");
        let authoritative =
            SupersessionView::from_events(&events).expect("project authoritative threads");
        let mut diagnostics = authoritative.diagnostics.clone();
        diagnostics.extend(store_diagnostics);

        assert_eq!(derived.supersession, authoritative);
        assert_eq!(derived.event_count, events.len());
        assert_eq!(derived.diagnostics, diagnostics);
        assert!(!derived.projection_stamp.is_empty());
    }

    #[test]
    fn active_attention_matches_authoritative_fork_projection() {
        let (repo, access) = active_forked_repo();
        let DerivedAttentionRoute::Ready(derived) =
            access.attention(None).expect("read derived attention")
        else {
            panic!("published generation should serve derived attention");
        };
        let authoritative =
            list_attention(AttentionListOptions::new(repo.path())).expect("read attention");

        assert_eq!(derived.items, authoritative.items);
        assert_eq!(derived.event_count, authoritative.event_count);
        assert_eq!(derived.diagnostics, authoritative.diagnostics);
        assert!(!derived.projection_stamp.is_empty());
    }

    #[test]
    fn concurrent_thread_and_attention_reads_remain_available_across_governed_append() {
        let (repo, access) = active_forked_repo();
        let access = Arc::new(access);
        let DerivedThreadsRoute::Ready(initial_threads) =
            access.threads().expect("read initial threads")
        else {
            panic!("published generation should serve initial threads");
        };
        let initial_event_count = initial_threads.event_count;
        let initial_thread_count = initial_threads.supersession.components.len();
        let read_store = resolve_read_store(repo.path()).expect("resolve store");
        let lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            read_store.store_dir(),
            "store:test",
        )
        .expect("open lifecycle");
        let coordinator = DerivedWriteCoordinator::new(lifecycle).expect("admit writer");
        let governed = EventStore::open(read_store.store_dir()).with_coordinator(coordinator);
        let revision_id = RevisionId::new(format!("review-unit:sha256:{}", "ab".repeat(32)));
        let event = ShoreEvent::new(
            EventType::WorkObjectProposed,
            format!("work_object_proposed:{}", revision_id.as_str()),
            EventTarget::for_revision(
                JournalId::new("journal:concurrent"),
                revision_id.clone(),
                None,
            )
            .expect("build revision target"),
            Writer::shore_local("test"),
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new(format!("engagement:sha256:{}", "cd".repeat(32))),
                work_object: WorkObjectProposal::Revision {
                    revision: Revision {
                        id: revision_id.clone(),
                        object_id: ObjectId::new(format!("obj:sha256:{}", "ef".repeat(32))),
                        git_provenance: None,
                    },
                    summary: None,
                    object_artifact_content_hash: format!("sha256:{}", "12".repeat(32)),
                    supersedes: Vec::new(),
                },
            },
            "2026-07-28T14:00:00Z",
        )
        .expect("build concurrent event");
        let barrier = Arc::new(Barrier::new(2));

        std::thread::scope(|scope| {
            let access = Arc::clone(&access);
            let reader_barrier = Arc::clone(&barrier);
            let reader = scope.spawn(move || {
                reader_barrier.wait();
                for _ in 0..16 {
                    assert!(matches!(
                        access.threads().expect("read threads"),
                        DerivedThreadsRoute::Ready(_) | DerivedThreadsRoute::Unavailable(_)
                    ));
                    assert!(matches!(
                        access.attention(None).expect("read attention"),
                        DerivedAttentionRoute::Ready(_) | DerivedAttentionRoute::Unavailable(_)
                    ));
                }
            });
            barrier.wait();
            assert_eq!(
                governed.record_event_once(&event).expect("governed append"),
                EventWriteOutcome::Created
            );
            reader.join().expect("reader completes");
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let threads = loop {
            match access.threads().expect("read current threads") {
                DerivedThreadsRoute::Ready(threads) => break threads,
                DerivedThreadsRoute::Unavailable(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                DerivedThreadsRoute::Unavailable(_) => {
                    panic!("governed append should leave current threads available")
                }
                DerivedThreadsRoute::Off => panic!("active access switched off"),
            }
        };
        let attention = loop {
            match access.attention(None).expect("read current attention") {
                DerivedAttentionRoute::Ready(attention) => break attention,
                DerivedAttentionRoute::Unavailable(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                DerivedAttentionRoute::Unavailable(_) => {
                    panic!("governed append should leave current attention available")
                }
                DerivedAttentionRoute::Off => panic!("active access switched off"),
            }
        };
        assert_eq!(threads.event_count, initial_event_count + 1);
        assert_eq!(attention.event_count, initial_event_count + 1);
        assert_eq!(threads.projection_stamp, attention.projection_stamp);
        assert_eq!(
            threads.supersession.components.len(),
            initial_thread_count + 1
        );
        assert!(
            threads
                .supersession
                .components
                .iter()
                .any(|component| component.contains(&revision_id))
        );
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn active_thread_and_attention_reads_retain_no_complete_history_or_response_cache() {
        let (_repo, access) = active_forked_repo();
        let scope =
            crate::bench_support::longitudinal::LongitudinalCountingScopeV1::new("d".repeat(64))
                .unwrap();
        let _guard = scope.enter();

        let DerivedThreadsRoute::Ready(threads) = access.threads().unwrap() else {
            panic!("published generation should serve threads");
        };
        assert!(matches!(
            access.attention(None).unwrap(),
            DerivedAttentionRoute::Ready(_)
        ));

        let snapshot = scope.snapshot();
        assert!(threads.event_count > 0);
        assert_eq!(snapshot.counters.carrier_opens, 0);
        assert_eq!(snapshot.counters.event_decodes, 0);
        assert_eq!(snapshot.counters.event_folds, 0);
        assert_eq!(snapshot.counters.projection_rebuilds, 0);
        assert_eq!(snapshot.counters.state_rebuilds, 0);
        let ownership = snapshot.capacity_ownership;
        assert_eq!(ownership.retained_decoded_events, 0);
        assert_eq!(ownership.retained_hydrated_history_entries, 0);
        assert_eq!(ownership.retained_hydrated_body_bytes, 0);
        assert_eq!(ownership.retained_search_record_strings, 0);
        assert_eq!(ownership.retained_search_record_field_bytes, 0);
        assert_eq!(ownership.retained_serialized_response_cache_bytes, 0);
    }
}
