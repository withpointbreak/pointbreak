use crate::bench_support::derived_access::adapter::QualificationDerivedAccessAdapter;
use crate::bench_support::derived_access::sqlite_cursor::{
    CursorLedgerIdentity, SqliteCursorLedger,
};
use crate::bench_support::derived_access::sqlite_locator::SqliteLocator;
use crate::bench_support::longitudinal::LongitudinalCountingScopeV1;
use crate::canonical_hash::{sha256_bytes_hex, sha256_json_prefixed};
use crate::model::JournalId;
use crate::session::EventStore;
use crate::session::derived_access::cursor::{AppendResolution, TruthCursor};
use crate::session::derived_access::locator::{
    ChronologicalWindowRequest, LocatorRead, normalize_occurred_at,
};
use crate::session::derived_access::semantic::state::{
    DerivedAccessFreshness, FreshnessModelError,
};
use crate::session::event::{
    ArtifactRemovedPayload, EventTarget, EventType, ReviewInitializedPayload, ShoreEvent, Writer,
};

const STORE_ID: &str = "store:locator-test";

fn event(index: usize, occurred_at: &str) -> ShoreEvent {
    let journal = format!("journal:locator-{index}");
    ShoreEvent::new(
        EventType::ReviewInitialized,
        format!("review_initialized:{journal}:work:default"),
        EventTarget::for_journal(JournalId::new(journal)),
        Writer::shore_local("0.8.0"),
        ReviewInitializedPayload {},
        occurred_at,
    )
    .expect("valid event")
}

fn open_adapter(root: &std::path::Path) -> QualificationDerivedAccessAdapter {
    SqliteCursorLedger::initialize_empty(root, CursorLedgerIdentity::new(STORE_ID))
        .expect("initialize cursor");
    QualificationDerivedAccessAdapter::open(root, CursorLedgerIdentity::new(STORE_ID))
        .expect("open adapter")
}

fn append(adapter: &QualificationDerivedAccessAdapter, event: &ShoreEvent, attempt: usize) {
    assert!(matches!(
        adapter
            .append_event(event, &format!("attempt:{attempt}"))
            .expect("append and index"),
        AppendResolution::Created(_)
    ));
}

fn ready<T>(read: LocatorRead<T>) -> T {
    match read {
        LocatorRead::Ready(value) => value,
        LocatorRead::CatchUpRequired { applied, observed } => {
            panic!("unexpected lag: applied={applied:?}, observed={observed:?}")
        }
    }
}

#[test]
fn cursor_and_projection_connections_use_distinct_wal_durability_policies() {
    let root = tempfile::tempdir().expect("root");
    let cursor =
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new(STORE_ID))
            .expect("initialize cursor");
    let locator = SqliteLocator::open(root.path()).expect("open locator");

    assert_eq!(
        cursor.connection_policy_for_test().expect("cursor policy"),
        ("wal".to_owned(), 2),
        "intent, receipt, and head commits remain power-loss durable"
    );
    assert_eq!(
        locator
            .connection_policy_for_test()
            .expect("projection policy"),
        ("wal".to_owned(), 1),
        "rebuildable locator and semantic commits use WAL NORMAL"
    );
}

#[test]
fn semantic_id_is_one_validated_authoritative_point_read() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let selected = event(1, "2026-07-27T01:00:00Z");
    append(&adapter, &selected, 1);

    let scope = LongitudinalCountingScopeV1::new("1".repeat(64)).expect("scope");
    let result = {
        let _guard = scope.enter();
        ready(
            adapter
                .semantic_id(selected.event_id.as_str())
                .expect("semantic-id lookup"),
        )
    };

    assert_eq!(result, Some(selected));
    let counters = scope.snapshot().counters;
    assert_eq!(counters.directory_entries_walked, 0);
    assert_eq!(counters.carrier_opens, 1);
    assert_eq!(counters.event_decodes, 1);
    assert_eq!(counters.event_validations, 1);
    assert_eq!(counters.event_folds, 0);
    assert_eq!(counters.chronological_sort_items, 0);
}

#[test]
fn backdated_append_does_not_duplicate_or_gap_an_as_of_page() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let first = event(1, "2026-07-27T01:00:00Z");
    let second = event(2, "2026-07-27T02:00:00Z");
    let third = event(3, "2026-07-27T03:00:00Z");
    for (attempt, item) in [&first, &second, &third].into_iter().enumerate() {
        append(&adapter, item, attempt);
    }

    let first_page = ready(
        adapter
            .chronological_window(ChronologicalWindowRequest::head(2))
            .expect("first page"),
    );
    assert_eq!(
        first_page
            .events
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<Vec<_>>(),
        vec![first.event_id.as_str(), second.event_id.as_str()]
    );
    let continuation = first_page.continuation.clone().expect("continuation");

    let backdated = event(4, "2026-07-27T01:30:00Z");
    append(&adapter, &backdated, 4);

    let second_page = ready(
        adapter
            .chronological_window(ChronologicalWindowRequest::continue_from(continuation, 2))
            .expect("stable continuation"),
    );
    assert_eq!(second_page.events, vec![third.clone()]);

    let refreshed = ready(
        adapter
            .chronological_window(ChronologicalWindowRequest::head(4))
            .expect("refreshed page"),
    );
    assert_eq!(
        refreshed
            .events
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            first.event_id.as_str(),
            backdated.event_id.as_str(),
            second.event_id.as_str(),
            third.event_id.as_str(),
        ]
    );
}

#[test]
fn locator_hit_never_bypasses_authoritative_truth_validation() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let selected = event(1, "2026-07-27T01:00:00Z");
    append(&adapter, &selected, 1);

    let path =
        EventStore::open(root.path()).event_path_for_idempotency_key(&selected.idempotency_key);
    std::fs::write(path, b"{\"corrupt\":true}").expect("tamper truth");

    assert!(
        adapter.semantic_id(selected.event_id.as_str()).is_err(),
        "a locator hit cannot return unvalidated or mismatched truth"
    );
}

#[test]
fn selected_locator_row_is_revalidated_against_its_cursor_receipt() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let selected = event(1, "2026-07-27T01:00:00Z");
    append(&adapter, &selected, 1);

    let connection = rusqlite::Connection::open(
        root.path()
            .join(".pointbreak-derived")
            .join("cursor.sqlite3"),
    )
    .expect("open sidecar");
    connection
        .execute(
            "UPDATE cursor_receipt
             SET logical_reread_key_hash = zeroblob(32)
             WHERE sequence = 1",
            [],
        )
        .expect("tamper selected receipt");

    assert!(
        adapter.semantic_id(selected.event_id.as_str()).is_err(),
        "a selected locator row must match its cursor receipt before hydration"
    );
}

#[test]
fn locator_miss_is_not_authoritative_while_checkpoint_lags_truth() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let pending = event(1, "2026-07-27T01:00:00Z");
    SqliteCursorLedger::open(root.path(), CursorLedgerIdentity::new(STORE_ID))
        .expect("open cursor")
        .append_event(&pending, "attempt:truth-only")
        .expect("publish truth without locator catch-up");

    assert!(matches!(
        adapter
            .semantic_id(pending.event_id.as_str())
            .expect("lag is a typed result"),
        LocatorRead::CatchUpRequired {
            applied: TruthCursor {
                epoch: 1,
                sequence: 0
            },
            observed: TruthCursor {
                epoch: 1,
                sequence: 1
            }
        }
    ));
}

#[test]
fn locator_schema_is_bodyless_and_does_not_persist_overlay_state() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    append(&adapter, &event(1, "2026-07-27T01:00:00Z"), 1);

    let inventory = adapter.locator_inventory().expect("locator inventory");
    assert_eq!(
        inventory.profile_id,
        "pointbreak.sqlite-derived-access-locator.v1"
    );
    assert_eq!(inventory.schema_version, 2);
    assert_eq!(inventory.row_count, 1);
    assert_eq!(
        inventory.indexes,
        vec![
            "locator_event_cursor",
            "locator_event_display",
            "locator_event_target",
            "sqlite_autoindex_locator_event_1",
            "sqlite_autoindex_locator_event_2",
        ]
    );
    for forbidden in [
        "event_bytes",
        "payload_json",
        "body",
        "summary",
        "reason",
        "trust_generation",
        "removed_content",
        "logical_reread_key",
        "validation_witness",
    ] {
        assert!(
            !inventory.columns.iter().any(|column| column == forbidden),
            "forbidden persisted locator column {forbidden}"
        );
    }
}

#[test]
fn equal_conflicting_and_removal_events_preserve_gap_free_locator_state() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let original = event(1, "2026-07-27T01:00:00Z");
    append(&adapter, &original, 1);

    assert_eq!(
        adapter
            .append_event(&original, "attempt:equal")
            .expect("equal duplicate"),
        AppendResolution::Existing(TruthCursor::new(1, 1))
    );
    let mut conflict = original.clone();
    conflict.payload = serde_json::json!({"conflict": true});
    conflict.payload_hash = sha256_json_prefixed(&conflict.payload).expect("payload hash");
    assert_eq!(
        adapter
            .append_event(&conflict, "attempt:conflict")
            .expect("classified conflict"),
        AppendResolution::Conflict(TruthCursor::new(1, 1))
    );

    let content_hash = "sha256:removed";
    let removal = ShoreEvent::new(
        EventType::ArtifactRemoved,
        ArtifactRemovedPayload::idempotency_key(content_hash),
        EventTarget::for_journal(JournalId::new("journal:locator-removal")),
        Writer::shore_local("0.8.0"),
        ArtifactRemovedPayload {
            content_hash: content_hash.to_owned(),
        },
        "2026-07-27T02:00:00Z",
    )
    .expect("removal event");
    append(&adapter, &removal, 4);

    assert_eq!(
        adapter.locator_checkpoint().expect("checkpoint"),
        TruthCursor::new(1, 2)
    );
    assert_eq!(
        adapter
            .locator_inventory()
            .expect("locator inventory")
            .row_count,
        2
    );
    let selected = ready(
        adapter
            .semantic_id(removal.event_id.as_str())
            .expect("removal locator"),
    )
    .expect("removal event");
    assert_eq!(selected, removal);
}

#[test]
fn a_locator_ahead_of_an_observed_head_serves_that_stable_snapshot() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let first = event(1, "2026-07-27T01:00:00Z");
    let second = event(2, "2026-07-27T02:00:00Z");
    append(&adapter, &first, 1);
    append(&adapter, &second, 2);
    let locator = SqliteLocator::open(root.path()).expect("open locator");

    let first_row = match locator
        .lookup_event_id(first.event_id.as_str(), TruthCursor::new(1, 1))
        .expect("read older snapshot")
    {
        LocatorRead::Ready(Some(row)) => row,
        other => panic!("expected selected older row, got {other:?}"),
    };
    assert_eq!(
        first_row.replay_key,
        sha256_bytes_hex(first.idempotency_key.as_bytes())
    );
    assert_eq!(
        locator
            .lookup_event_id(second.event_id.as_str(), TruthCursor::new(1, 1))
            .expect("newer event is absent from older snapshot"),
        LocatorRead::Ready(None)
    );
}

#[test]
fn no_change_freshness_and_zero_new_count_touch_no_event_carriers() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    append(&adapter, &event(1, "2026-07-27T01:00:00Z"), 1);
    let scope = LongitudinalCountingScopeV1::new("2".repeat(64)).expect("scope");

    let (freshness, new_count) = {
        let _guard = scope.enter();
        (
            adapter.freshness().expect("freshness"),
            adapter.new_event_count().expect("new count"),
        )
    };

    assert!(matches!(
        freshness,
        DerivedAccessFreshness::Current {
            as_of: TruthCursor {
                epoch: 1,
                sequence: 1
            }
        }
    ));
    assert_eq!(new_count, Some(0));
    let counters = scope.snapshot().counters;
    assert_eq!(counters.directory_entries_walked, 0);
    assert_eq!(counters.carrier_opens, 0);
    assert_eq!(counters.event_decodes, 0);
    assert_eq!(counters.event_validations, 0);
    assert_eq!(counters.event_folds, 0);
}

#[test]
fn fixed_output_window_work_is_independent_of_history_cardinality() {
    fn measure(event_count: usize, run_identity: char) -> (u64, u64, u64, u64) {
        let root = tempfile::tempdir().expect("root");
        let adapter = open_adapter(root.path());
        for index in 0..event_count {
            append(
                &adapter,
                &event(
                    index,
                    &format!("unix-ms:{}", 1_800_000_000_000_u64 + index as u64),
                ),
                index,
            );
        }
        let scope =
            LongitudinalCountingScopeV1::new(run_identity.to_string().repeat(64)).expect("scope");
        let page = {
            let _guard = scope.enter();
            ready(
                adapter
                    .chronological_window(ChronologicalWindowRequest::tail(10))
                    .expect("tail window"),
            )
        };
        assert_eq!(page.events.len(), 10);
        let counters = scope.snapshot().counters;
        (
            counters.directory_entries_walked,
            counters.carrier_opens,
            counters.event_decodes,
            counters.chronological_sort_items,
        )
    }

    assert_eq!(measure(100, '3'), (0, 10, 10, 0));
    assert_eq!(measure(262, '4'), (0, 10, 10, 0));
}

#[test]
fn chronological_sort_counter_observes_a_regressed_sqlite_plan() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    for index in 0..20 {
        append(
            &adapter,
            &event(
                index,
                &format!("unix-ms:{}", 1_800_000_000_000_u64 + index as u64),
            ),
            index,
        );
    }
    let connection = rusqlite::Connection::open(
        root.path()
            .join(".pointbreak-derived")
            .join("cursor.sqlite3"),
    )
    .expect("open sidecar");
    connection
        .execute_batch(
            "DROP INDEX locator_event_display;
             CREATE INDEX locator_event_display ON locator_event(epoch, sequence);",
        )
        .expect("install deliberately regressed display index");

    let scope = LongitudinalCountingScopeV1::new("5".repeat(64)).expect("scope");
    {
        let _guard = scope.enter();
        ready(
            adapter
                .chronological_window(ChronologicalWindowRequest::tail(5))
                .expect("tail query still returns"),
        );
    }
    assert!(
        scope.snapshot().counters.chronological_sort_items > 0,
        "SQLite sort work must be visible to the qualification counter"
    );
}

#[test]
fn append_checkpoint_and_all_window_families_equal_full_replay() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let events = [
        event(1, "2026-07-27T04:00:00Z"),
        event(2, "2026-07-27T01:00:00Z"),
        event(3, "2026-07-27T03:00:00Z"),
        event(4, "2026-07-27T02:00:00Z"),
    ];
    for (attempt, event) in events.iter().enumerate() {
        append(&adapter, event, attempt);
    }

    let head = adapter.truth_head().expect("truth head");
    assert_eq!(
        adapter.locator_checkpoint().expect("checkpoint"),
        head.cursor
    );

    let mut expected = EventStore::open(root.path())
        .list_events()
        .expect("full replay");
    expected.sort_by(|left, right| {
        normalize_occurred_at(&left.occurred_at)
            .expect("left timestamp")
            .cmp(&normalize_occurred_at(&right.occurred_at).expect("right timestamp"))
            .then_with(|| left.event_id.as_str().cmp(right.event_id.as_str()))
    });

    let head_page = ready(
        adapter
            .chronological_window(ChronologicalWindowRequest::head(2))
            .expect("head"),
    );
    let middle_page = ready(
        adapter
            .chronological_window(ChronologicalWindowRequest::continue_from(
                head_page.continuation.clone().expect("head continuation"),
                2,
            ))
            .expect("middle"),
    );
    let tail_page = ready(
        adapter
            .chronological_window(ChronologicalWindowRequest::tail(2))
            .expect("tail"),
    );
    let before_tail = ready(
        adapter
            .chronological_window(ChronologicalWindowRequest::continue_from(
                tail_page.continuation.clone().expect("tail continuation"),
                2,
            ))
            .expect("before tail"),
    );

    assert_eq!(head_page.events, expected[..2]);
    assert_eq!(middle_page.events, expected[2..]);
    assert_eq!(tail_page.events, expected[2..]);
    assert_eq!(before_tail.events, expected[..2]);
    assert!(!before_tail.has_more);
}

#[test]
fn every_window_family_uses_a_bounded_index_plan() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    for index in 0..100 {
        append(
            &adapter,
            &event(
                index,
                &format!("unix-ms:{}", 1_800_000_000_000_u64 + index as u64),
            ),
            index,
        );
    }
    let locator = SqliteLocator::open(root.path()).expect("open locator");
    let observed = TruthCursor::new(1, 100);

    let (head, head_status) = locator
        .chronological_window_with_status(&ChronologicalWindowRequest::head(10), observed)
        .expect("head query");
    let head = ready(head);
    let (after, after_status) = locator
        .chronological_window_with_status(
            &ChronologicalWindowRequest::continue_from(
                head.continuation.clone().expect("head continuation"),
                10,
            ),
            observed,
        )
        .expect("after query");
    let after = ready(after);
    let (tail, tail_status) = locator
        .chronological_window_with_status(&ChronologicalWindowRequest::tail(10), observed)
        .expect("tail query");
    let tail = ready(tail);
    let (_, before_status) = locator
        .chronological_window_with_status(
            &ChronologicalWindowRequest::continue_from(
                tail.continuation.clone().expect("tail continuation"),
                10,
            ),
            observed,
        )
        .expect("before query");

    for status in [head_status, after_status, tail_status, before_status] {
        assert_eq!(status.fullscan_steps, 0);
        assert_eq!(status.sort_operations, 0);
    }
    assert_eq!(after.rows.len(), 10);
}

#[test]
fn continuation_requests_are_snapshot_pinned_and_epoch_checked() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let selected = event(1, "2026-07-27T01:00:00Z");
    append(&adapter, &selected, 1);
    let locator = SqliteLocator::open(root.path()).expect("open locator");
    let anchor = crate::session::derived_access::locator::DisplayKey {
        normalized_occurred_at: normalize_occurred_at(&selected.occurred_at).expect("timestamp"),
        event_id: selected.event_id.as_str().to_owned(),
    };

    let wrong_epoch = locator
        .chronological_window(
            &ChronologicalWindowRequest::after(anchor.clone(), 10, TruthCursor::new(2, 1)),
            TruthCursor::new(1, 1),
        )
        .expect_err("wrong epoch must fail");
    assert!(matches!(
        wrong_epoch,
        super::sqlite_locator::SqliteLocatorError::Model(
            crate::session::derived_access::locator::LocatorModelError::AsOfEpochMismatch { .. }
        )
    ));

    let ahead = locator
        .chronological_window(
            &ChronologicalWindowRequest::before(anchor, 10, TruthCursor::new(1, 2)),
            TruthCursor::new(1, 1),
        )
        .expect_err("future snapshot must fail");
    assert!(matches!(
        ahead,
        super::sqlite_locator::SqliteLocatorError::Model(
            crate::session::derived_access::locator::LocatorModelError::AsOfAhead { .. }
        )
    ));

    assert!(matches!(
        DerivedAccessFreshness::between(TruthCursor::new(1, 1), TruthCursor::new(2, 1)),
        Ok(DerivedAccessFreshness::EpochMismatch { .. })
    ));
    assert!(matches!(
        DerivedAccessFreshness::between(TruthCursor::new(1, 2), TruthCursor::new(1, 1)),
        Err(FreshnessModelError::AppliedAhead { .. })
    ));
}
