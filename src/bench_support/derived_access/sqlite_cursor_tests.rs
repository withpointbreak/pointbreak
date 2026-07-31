use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

use crate::bench_support::derived_access::sqlite_cursor::{
    AppendCrashPoint, BootstrapControl, BootstrapCrashPoint, CursorLedgerError,
    CursorLedgerIdentity, SqliteCursorLedger, full_chain_query_count_for_test,
};
use crate::bench_support::longitudinal::LongitudinalCountingScopeV1;
use crate::canonical_hash::{sha256_bytes_hex, sha256_json_prefixed};
use crate::model::JournalId;
use crate::session::EventStore;
use crate::session::derived_access::cursor::{
    AppendResolution, CarrierState, CursorIntent, IntentRecoveryAuthority, RecoveryResolution,
    ReferenceCursorLedger, TruthCursor,
};
use crate::session::event::{EventTarget, EventType, ReviewInitializedPayload, ShoreEvent, Writer};

const CHILD_TEST: &str =
    "bench_support::derived_access::sqlite_cursor_tests::sqlite_cursor_subprocess_entrypoint";
const CHILD_MODE: &str = "POINTBREAK_CURSOR_LEDGER_CHILD_MODE";
const CHILD_ROOT: &str = "POINTBREAK_CURSOR_LEDGER_CHILD_ROOT";
const CHILD_POINT: &str = "POINTBREAK_CURSOR_LEDGER_CHILD_POINT";
const CHILD_RESULT: &str = "POINTBREAK_CURSOR_LEDGER_CHILD_RESULT";
const CHILD_READY: &str = "POINTBREAK_CURSOR_LEDGER_CHILD_READY";
const CHILD_RELEASE: &str = "POINTBREAK_CURSOR_LEDGER_CHILD_RELEASE";
const CHILD_INDEX: &str = "POINTBREAK_CURSOR_LEDGER_CHILD_INDEX";
const CHILD_ATTEMPT: &str = "POINTBREAK_CURSOR_LEDGER_CHILD_ATTEMPT";

fn event(index: usize) -> ShoreEvent {
    let session = format!("session:cursor-{index}");
    ShoreEvent::new(
        EventType::ReviewInitialized,
        format!("review_initialized:{session}:work:default"),
        EventTarget::for_journal(JournalId::new(session.as_str())),
        Writer::shore_local("0.8.0"),
        ReviewInitializedPayload {},
        "2026-07-27T00:00:00Z",
    )
    .expect("valid test event")
}

fn event_witness(event: &ShoreEvent) -> String {
    sha256_bytes_hex(&serde_json::to_vec(event).expect("serialize event"))
}

#[test]
fn physical_cursor_ledger_matches_reference_for_unique_equal_and_conflicting_writes() {
    let root = tempfile::tempdir().expect("root");
    let ledger =
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("initialize ledger");
    let mut reference = ReferenceCursorLedger::new("store:test", 1);
    let first = event(1);
    let first_witness = event_witness(&first);
    let initial_authority = ledger.authority_snapshot().expect("initial authority");

    assert_eq!(
        ledger.append_event(&first, "attempt:1").expect("create"),
        reference
            .append(&first.idempotency_key, &first_witness, "attempt:1")
            .expect("reference create")
    );
    let created_authority = ledger.authority_snapshot().expect("created authority");
    assert_eq!(created_authority.head.cursor, TruthCursor::new(1, 1));
    assert_ne!(
        created_authority.change_stamp,
        initial_authority.change_stamp
    );
    assert_eq!(
        ledger
            .append_event(&first, "attempt:2")
            .expect("equal duplicate"),
        reference
            .append(&first.idempotency_key, &first_witness, "attempt:2")
            .expect("reference equal duplicate")
    );
    assert_eq!(
        ledger.authority_snapshot().expect("duplicate authority"),
        created_authority
    );

    let mut conflict = first.clone();
    conflict.payload = serde_json::json!({"conflict": true});
    conflict.payload_hash = sha256_json_prefixed(&conflict.payload).expect("payload hash");
    let conflict_witness = event_witness(&conflict);
    assert_eq!(
        ledger
            .append_event(&conflict, "attempt:3")
            .expect("conflicting duplicate"),
        reference
            .append(&conflict.idempotency_key, &conflict_witness, "attempt:3",)
            .expect("reference conflicting duplicate")
    );
    assert_eq!(
        ledger.authority_snapshot().expect("conflict authority"),
        created_authority
    );
    assert_eq!(ledger.head().expect("head"), reference.head());
    assert_eq!(
        ledger
            .events_after(TruthCursor::new(1, 0), 8)
            .expect("physical delta"),
        reference
            .events_after(TruthCursor::new(1, 0), 8)
            .expect("reference delta")
    );
    let checkpoint = ledger.checkpoint().expect("passive checkpoint");
    assert!(!checkpoint.busy);
    assert!(checkpoint.checkpointed_frames <= checkpoint.log_frames);
    let inventory = ledger.inventory().expect("lifecycle inventory");
    assert_eq!(
        inventory.profile_id,
        "pointbreak.sqlite-derived-access-cursor.v1"
    );
    assert_eq!(inventory.schema_version, 4);
    assert_eq!(inventory.epoch, 1);
    assert_eq!(inventory.head_sequence, 1);
    assert_eq!(inventory.receipt_count, 1);
    assert_eq!(inventory.attempt_count, 3);
    assert!(!inventory.active_intent);
    assert!(inventory.database_bytes > 0);
    ledger.integrity_check().expect("integrity");
    drop(ledger);
    let reopened = SqliteCursorLedger::open(root.path(), CursorLedgerIdentity::new("store:test"))
        .expect("close and reopen");
    assert_eq!(
        reopened.head().expect("reopened head").cursor,
        TruthCursor::new(1, 1)
    );
}

#[test]
fn cursor_receipts_persist_only_digest_addressed_carrier_identity() {
    let root = tempfile::tempdir().expect("root");
    let ledger =
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("initialize ledger");
    ledger
        .append_event(&event(1), "attempt:1")
        .expect("append event");
    drop(ledger);

    let connection =
        rusqlite::Connection::open(root.path().join(".pointbreak-derived/cursor.sqlite3"))
            .expect("open sidecar");
    let columns = connection
        .prepare("PRAGMA table_info(cursor_receipt)")
        .expect("prepare receipt columns")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query receipt columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("read receipt columns");
    assert_eq!(
        columns,
        vec![
            "sequence",
            "epoch",
            "logical_reread_key_hash",
            "validation_witness_hash",
            "attempt_hash",
            "attempt_token",
        ]
    );
    let (key_type, key_bytes) = connection
        .query_row(
            "SELECT typeof(logical_reread_key_hash), length(logical_reread_key_hash)
             FROM cursor_receipt WHERE sequence = 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .expect("read digest carrier identity");
    assert_eq!(key_type, "blob");
    assert_eq!(key_bytes, 32);
}

#[test]
fn attempt_tokens_are_single_use_and_failed_reuse_does_not_advance_truth() {
    let root = tempfile::tempdir().expect("root");
    let ledger =
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("initialize");
    ledger
        .append_event(&event(1), "attempt:one")
        .expect("first append");
    assert!(matches!(
        ledger.append_event(&event(2), "attempt:one"),
        Err(CursorLedgerError::AttemptTokenUsed(token)) if token == "attempt:one"
    ));
    assert_eq!(ledger.head().expect("head").cursor, TruthCursor::new(1, 1));
    assert_eq!(
        EventStore::open(root.path())
            .list_events()
            .expect("truth list")
            .len(),
        1
    );
}

#[test]
fn bounded_delta_rereads_only_selected_authoritative_carriers() {
    let root = tempfile::tempdir().expect("root");
    let ledger =
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("initialize ledger");
    for index in 0..4 {
        ledger
            .append_event(&event(index), &format!("attempt:{index}"))
            .expect("append");
    }

    let delta = ledger
        .events_after(TruthCursor::new(1, 1), 2)
        .expect("bounded delta");
    assert_eq!(delta.receipts.len(), 2);
    assert_eq!(delta.receipts[0].cursor, TruthCursor::new(1, 2));
    assert_eq!(delta.receipts[1].cursor, TruthCursor::new(1, 3));
    assert!(!delta.complete);
}

#[test]
fn bootstrap_is_explicit_completion_last_and_cancellable() {
    let root = tempfile::tempdir().expect("root");
    let store = EventStore::open(root.path());
    for index in 0..3 {
        store.record_event_once(&event(index)).expect("seed truth");
    }

    let result = SqliteCursorLedger::bootstrap_from_truth(
        root.path(),
        CursorLedgerIdentity::new("store:test"),
        1,
        |progress| {
            if progress.completed == 2 {
                BootstrapControl::Cancel
            } else {
                BootstrapControl::Continue
            }
        },
    );
    assert!(result.is_err(), "cancelled bootstrap must not publish");
    assert!(
        SqliteCursorLedger::open(root.path(), CursorLedgerIdentity::new("store:test")).is_err(),
        "incomplete bootstrap is not readable"
    );

    let ledger = SqliteCursorLedger::bootstrap_from_truth(
        root.path(),
        CursorLedgerIdentity::new("store:test"),
        1,
        |_| BootstrapControl::Continue,
    )
    .expect("restart bootstrap");
    assert_eq!(ledger.head().expect("head").cursor, TruthCursor::new(1, 3));
}

#[test]
fn wrong_root_identity_is_rejected_without_touching_truth() {
    let root = tempfile::tempdir().expect("root");
    let truth = event(1);
    EventStore::open(root.path())
        .record_event_once(&truth)
        .expect("seed truth");
    SqliteCursorLedger::bootstrap_from_truth(
        root.path(),
        CursorLedgerIdentity::new("store:a"),
        1,
        |_| BootstrapControl::Continue,
    )
    .expect("bootstrap");

    assert!(
        SqliteCursorLedger::open(root.path(), CursorLedgerIdentity::new("store:b")).is_err(),
        "wrong store identity must fail closed"
    );
    assert_eq!(
        EventStore::open(root.path())
            .list_events()
            .expect("truth remains readable"),
        vec![truth]
    );
    assert!(matches!(
        SqliteCursorLedger::open(root.path(), CursorLedgerIdentity::new("store:a")),
        Err(CursorLedgerError::Quarantined(_))
    ));
}

#[test]
fn no_change_head_and_bounded_delta_never_enumerate_the_event_directory() {
    let root = tempfile::tempdir().expect("root");
    let ledger =
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("initialize ledger");
    for index in 0..3 {
        ledger
            .append_event(&event(index), &format!("attempt:{index}"))
            .expect("append");
    }
    let scope = LongitudinalCountingScopeV1::new("a".repeat(64)).expect("scope");
    let guard = scope.enter();
    let full_chain_queries_before = full_chain_query_count_for_test();
    assert_eq!(ledger.head().expect("head").cursor, TruthCursor::new(1, 3));
    let delta = ledger
        .events_after(TruthCursor::new(1, 1), 1)
        .expect("bounded delta");
    let full_chain_queries_after = full_chain_query_count_for_test();
    drop(guard);

    assert_eq!(delta.receipts.len(), 1);
    assert_eq!(
        full_chain_queries_after, full_chain_queries_before,
        "hot reads must not run full receipt-chain queries"
    );
    let counters = scope.snapshot().counters;
    assert_eq!(counters.directory_entries_walked, 0);
    assert_eq!(counters.event_decodes, 1);
    assert_eq!(counters.event_validations, 1);
    assert_eq!(counters.event_folds, 0);
    assert_eq!(counters.carrier_opens, 1);
}

#[test]
fn hot_reads_keep_one_snapshot_across_atomic_receipt_and_head_publication() {
    let root = tempfile::tempdir().expect("root");
    let ledger =
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("initialize ledger");
    let published = event(1);

    let observed = ledger
        .head_with_snapshot_hook(|| {
            assert_eq!(
                ledger
                    .append_event(&published, "attempt:concurrent")
                    .expect("publish while reader holds its snapshot"),
                AppendResolution::Created(TruthCursor::new(1, 1))
            );
        })
        .expect("read the pre-publication snapshot");

    assert_eq!(observed.cursor, TruthCursor::new(1, 0));
    assert_eq!(
        ledger.head().expect("read the published snapshot").cursor,
        TruthCursor::new(1, 1)
    );
}

#[test]
fn every_append_crash_point_recovers_to_the_reference_result() {
    let cases = [
        AppendCrashPoint::BeforeIntentCommit,
        AppendCrashPoint::AfterIntentCommit,
        AppendCrashPoint::AfterEventPublication,
        AppendCrashPoint::AfterReceiptBeforeHead,
        AppendCrashPoint::AfterHeadBeforeIntentRetirement,
    ];

    for point in cases {
        let (expected_recovery, expected_head) =
            reference_recovery_after_crash(point, &event(1), "attempt:crash");
        let root = tempfile::tempdir().expect("root");
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("initialize ledger");
        let result = root.path().join("child-result");
        let mut child = spawn_child(
            "crash-append",
            root.path(),
            Some(point_code(point)),
            &result,
            1,
            &format!("attempt:{point:?}"),
        );
        let status = child.wait().expect("wait for crash child");
        assert_eq!(status.code(), Some(86), "{point:?} must crash at its hook");

        let ledger = SqliteCursorLedger::open(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("reopen after crash");
        assert_eq!(
            ledger.recover().expect("targeted recovery"),
            expected_recovery,
            "{point:?}"
        );
        assert_eq!(
            ledger.head().expect("head").cursor,
            expected_head,
            "{point:?}"
        );
        ledger.integrity_check().expect("integrity after recovery");
    }
}

#[test]
fn live_writer_intent_is_not_recovered_and_process_death_releases_the_lock() {
    let root = tempfile::tempdir().expect("root");
    let ledger =
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("initialize ledger");
    let ready = root.path().join("ready-a");
    let release = root.path().join("release-a");
    let result = root.path().join("result-a");
    let mut writer_a = spawn_paused_child(root.path(), &ready, &release, &result, 1, "attempt:a");
    wait_for_file(&ready);
    assert_eq!(
        ledger.head().expect("reader sees old complete head").cursor,
        TruthCursor::new(1, 0)
    );
    let old_snapshot = ledger
        .events_after(TruthCursor::new(1, 0), 1)
        .expect("reader sees old complete delta");
    assert!(old_snapshot.complete);
    assert!(old_snapshot.receipts.is_empty());
    assert!(matches!(
        ledger.try_append_event(&event(2), "attempt:b"),
        Err(CursorLedgerError::WriterBusy)
    ));
    let mut reference = ReferenceCursorLedger::new("store:test", 1);
    reference
        .set_abandoned_intent(CursorIntent::new(
            TruthCursor::new(1, 1),
            event(1).idempotency_key,
            event_witness(&event(1)),
            "attempt:a",
        ))
        .expect("reference live intent");
    assert_eq!(
        ledger.try_recover().expect("busy recovery result"),
        reference
            .recover_intent(IntentRecoveryAuthority::WriterMayBeLive)
            .expect("reference busy recovery")
    );
    write_signal(&release, "release");
    assert!(writer_a.wait().expect("writer A").success());
    assert_eq!(ledger.head().expect("head").cursor, TruthCursor::new(1, 1));

    let ready = root.path().join("ready-death");
    let release = root.path().join("release-death");
    let result = root.path().join("result-death");
    let mut doomed = spawn_paused_child(root.path(), &ready, &release, &result, 2, "attempt:death");
    wait_for_file(&ready);
    doomed.kill().expect("kill paused writer");
    doomed.wait().expect("reap paused writer");
    assert_eq!(
        ledger.recover().expect("recover dead writer"),
        RecoveryResolution::RetiredAbsent
    );
    assert_eq!(
        ledger
            .append_event(&event(2), "attempt:after-death")
            .expect("append after death"),
        AppendResolution::Created(TruthCursor::new(1, 2))
    );
}

fn reference_recovery_after_crash(
    point: AppendCrashPoint,
    event: &ShoreEvent,
    attempt_token: &str,
) -> (RecoveryResolution, TruthCursor) {
    let mut reference = ReferenceCursorLedger::new("store:test", 1);
    let witness = event_witness(event);
    let intent = CursorIntent::new(
        TruthCursor::new(1, 1),
        event.idempotency_key.clone(),
        witness.clone(),
        attempt_token,
    );
    match point {
        AppendCrashPoint::BeforeIntentCommit => {}
        AppendCrashPoint::AfterIntentCommit => {
            reference
                .set_abandoned_intent(intent)
                .expect("reference intent");
        }
        AppendCrashPoint::AfterEventPublication | AppendCrashPoint::AfterReceiptBeforeHead => {
            reference
                .set_abandoned_intent(intent)
                .expect("reference intent");
            reference.set_carrier_for_test(
                event.idempotency_key.clone(),
                CarrierState::unambiguous(witness),
            );
        }
        AppendCrashPoint::AfterHeadBeforeIntentRetirement => {
            assert_eq!(
                reference
                    .append(&event.idempotency_key, &witness, attempt_token)
                    .expect("reference append"),
                AppendResolution::Created(TruthCursor::new(1, 1))
            );
            reference
                .set_abandoned_intent(intent)
                .expect("reference finalized intent");
        }
    }
    let recovery = reference
        .recover_abandoned_intent()
        .expect("reference recovery");
    (recovery, reference.head().cursor)
}

#[test]
fn abandoned_duplicate_intent_retires_against_its_earlier_receipt() {
    let root = tempfile::tempdir().expect("root");
    let ledger =
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("initialize");
    ledger
        .append_event(&event(1), "attempt:first")
        .expect("first append");
    let result = root.path().join("duplicate-crash");
    let mut child = spawn_child(
        "crash-append",
        root.path(),
        Some("after-intent-commit"),
        &result,
        1,
        "attempt:duplicate",
    );
    assert_eq!(child.wait().expect("duplicate child").code(), Some(86));

    let reopened = SqliteCursorLedger::open(root.path(), CursorLedgerIdentity::new("store:test"))
        .expect("reopen");
    assert_eq!(
        reopened.recover().expect("retire duplicate intent"),
        RecoveryResolution::Existing(TruthCursor::new(1, 1))
    );
    assert_eq!(
        reopened.head().expect("head").cursor,
        TruthCursor::new(1, 1)
    );
}

#[test]
fn orphan_receipt_advances_only_the_single_expected_head() {
    let root = tempfile::tempdir().expect("root");
    let ledger =
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("initialize");
    ledger
        .append_event(&event(1), "attempt:first")
        .expect("first append");
    let connection =
        rusqlite::Connection::open(root.path().join(".pointbreak-derived/cursor.sqlite3"))
            .expect("raw test connection");
    connection
        .execute(
            "UPDATE cursor_meta SET head_sequence = 0 WHERE singleton = 1",
            [],
        )
        .expect("inject recoverable old head");
    drop(connection);

    let reopened = SqliteCursorLedger::open(root.path(), CursorLedgerIdentity::new("store:test"))
        .expect("recoverable sidecar opens");
    assert!(matches!(
        reopened.head(),
        Err(CursorLedgerError::SchemaMismatch(_))
    ));
    assert_eq!(
        reopened.recover().expect("advance orphan receipt"),
        RecoveryResolution::AdvancedHead(TruthCursor::new(1, 1))
    );
    assert_eq!(
        reopened.head().expect("recovered head").cursor,
        TruthCursor::new(1, 1)
    );
}

#[test]
fn unreceipted_existing_and_recovery_witness_mismatch_quarantine() {
    let legacy_root = tempfile::tempdir().expect("legacy root");
    let legacy_ledger = SqliteCursorLedger::initialize_empty(
        legacy_root.path(),
        CursorLedgerIdentity::new("store:test"),
    )
    .expect("initialize");
    let legacy = event(1);
    EventStore::open(legacy_root.path())
        .record_event_once(&legacy)
        .expect("out-of-band carrier");
    assert!(matches!(
        legacy_ledger.append_event(&legacy, "attempt:legacy"),
        Err(CursorLedgerError::UnreceiptedCarrier(_))
    ));
    assert!(matches!(
        SqliteCursorLedger::open(legacy_root.path(), CursorLedgerIdentity::new("store:test")),
        Err(CursorLedgerError::Quarantined(_))
    ));

    let mismatch_root = tempfile::tempdir().expect("mismatch root");
    SqliteCursorLedger::initialize_empty(
        mismatch_root.path(),
        CursorLedgerIdentity::new("store:test"),
    )
    .expect("initialize");
    let mut child = spawn_child(
        "crash-append",
        mismatch_root.path(),
        Some("after-intent-commit"),
        &mismatch_root.path().join("mismatch-child"),
        1,
        "attempt:mismatch",
    );
    assert_eq!(child.wait().expect("mismatch child").code(), Some(86));
    let mut divergent = event(1);
    divergent.payload = serde_json::json!({"divergent": true});
    divergent.payload_hash =
        sha256_json_prefixed(&divergent.payload).expect("divergent payload hash");
    EventStore::open(mismatch_root.path())
        .record_event_once(&divergent)
        .expect("publish divergent carrier");
    let mismatch = SqliteCursorLedger::open(
        mismatch_root.path(),
        CursorLedgerIdentity::new("store:test"),
    )
    .expect("recoverable mismatch opens");
    assert!(matches!(
        mismatch.recover(),
        Err(CursorLedgerError::WitnessMismatch(_))
    ));
    assert!(matches!(
        mismatch.head(),
        Err(CursorLedgerError::Quarantined(_))
    ));
}

#[test]
fn competing_processes_produce_exact_unique_equal_and_conflict_counts() {
    let root = tempfile::tempdir().expect("root");
    let ledger =
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("initialize ledger");
    let first_result = root.path().join("race-first");
    let second_result = root.path().join("race-second");
    let mut first = spawn_child(
        "append",
        root.path(),
        None,
        &first_result,
        1,
        "attempt:race-a",
    );
    let mut second = spawn_child(
        "append",
        root.path(),
        None,
        &second_result,
        1,
        "attempt:race-b",
    );
    assert!(first.wait().expect("first racer").success());
    assert!(second.wait().expect("second racer").success());
    let mut outcomes = [
        std::fs::read_to_string(&first_result).expect("first result"),
        std::fs::read_to_string(&second_result).expect("second result"),
    ];
    outcomes.sort();
    assert_eq!(outcomes, ["created", "existing"]);

    let mut conflict = event(1);
    conflict.payload = serde_json::json!({"conflict": true});
    conflict.payload_hash = sha256_json_prefixed(&conflict.payload).expect("payload hash");
    assert_eq!(
        ledger
            .append_event(&conflict, "attempt:conflict")
            .expect("conflict"),
        AppendResolution::Conflict(TruthCursor::new(1, 1))
    );
    assert_eq!(ledger.head().expect("head").cursor, TruthCursor::new(1, 1));
}

#[test]
fn bootstrap_and_quarantine_publication_crashes_preserve_restartable_truth() {
    let staging_root = tempfile::tempdir().expect("staging root");
    for index in 0..3 {
        EventStore::open(staging_root.path())
            .record_event_once(&event(index))
            .expect("seed staging truth");
    }
    let mut staging_child = spawn_child(
        "crash-bootstrap",
        staging_root.path(),
        Some("during-staging"),
        &staging_root.path().join("staging-result"),
        0,
        "unused",
    );
    assert_eq!(
        staging_child.wait().expect("staging child").code(),
        Some(87)
    );
    assert!(matches!(
        SqliteCursorLedger::open(staging_root.path(), CursorLedgerIdentity::new("store:test")),
        Err(CursorLedgerError::IncompleteBootstrap)
    ));
    let staged = SqliteCursorLedger::bootstrap_from_truth(
        staging_root.path(),
        CursorLedgerIdentity::new("store:test"),
        1,
        |_| BootstrapControl::Continue,
    )
    .expect("restart staging bootstrap");
    assert_eq!(staged.head().expect("head").cursor, TruthCursor::new(1, 3));

    let quarantine_root = tempfile::tempdir().expect("quarantine root");
    SqliteCursorLedger::initialize_empty(
        quarantine_root.path(),
        CursorLedgerIdentity::new("store:test"),
    )
    .expect("initialize");
    assert!(
        SqliteCursorLedger::open(
            quarantine_root.path(),
            CursorLedgerIdentity::new("store:wrong")
        )
        .is_err()
    );
    let mut quarantine_child = spawn_child(
        "crash-bootstrap",
        quarantine_root.path(),
        Some("after-quarantine"),
        &quarantine_root.path().join("quarantine-result"),
        0,
        "unused",
    );
    assert_eq!(
        quarantine_child.wait().expect("quarantine child").code(),
        Some(88)
    );
    assert!(
        std::fs::read_dir(quarantine_root.path())
            .expect("root inventory")
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".pointbreak-derived.quarantine-"))
    );
    let rebuilt = SqliteCursorLedger::bootstrap_from_truth(
        quarantine_root.path(),
        CursorLedgerIdentity::new("store:test"),
        2,
        |_| BootstrapControl::Continue,
    )
    .expect("publish new epoch");
    assert_eq!(rebuilt.head().expect("head").cursor, TruthCursor::new(2, 0));
}

#[test]
fn carrier_tamper_and_cursor_ahead_fail_closed() {
    let root = tempfile::tempdir().expect("root");
    let ledger =
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("initialize");
    let first = event(1);
    ledger
        .append_event(&first, "attempt:1")
        .expect("append first");
    assert!(matches!(
        ledger.events_after(TruthCursor::new(1, 2), 1),
        Err(CursorLedgerError::CursorAhead { .. })
    ));
    let event_path =
        EventStore::open(root.path()).event_path_for_idempotency_key(&first.idempotency_key);
    std::fs::write(&event_path, b"tampered").expect("tamper disposable truth");
    assert!(matches!(
        ledger.events_after(TruthCursor::new(1, 0), 1),
        Err(CursorLedgerError::WitnessMismatch(_))
    ));
}

#[test]
fn cursor_chain_corruption_is_quarantined_without_repairing_truth() {
    let root = tempfile::tempdir().expect("root");
    let first = event(1);
    let ledger =
        SqliteCursorLedger::initialize_empty(root.path(), CursorLedgerIdentity::new("store:test"))
            .expect("initialize");
    ledger
        .append_event(&first, "attempt:1")
        .expect("append first");
    let connection =
        rusqlite::Connection::open(root.path().join(".pointbreak-derived/cursor.sqlite3"))
            .expect("raw test connection");
    connection
        .execute(
            "UPDATE cursor_meta SET head_sequence = 2 WHERE singleton = 1",
            [],
        )
        .expect("inject cursor-ahead metadata");
    drop(connection);

    assert!(matches!(
        SqliteCursorLedger::open(root.path(), CursorLedgerIdentity::new("store:test")),
        Err(CursorLedgerError::Quarantined(_))
    ));
    assert_eq!(
        EventStore::open(root.path())
            .list_events()
            .expect("truth remains readable"),
        vec![first]
    );
}

#[test]
fn structurally_corrupt_sidecar_is_rotated_out_of_the_canonical_path() {
    let root = tempfile::tempdir().expect("root");
    let sidecar = root.path().join(".pointbreak-derived");
    std::fs::create_dir_all(&sidecar).expect("sidecar directory");
    std::fs::write(sidecar.join("cursor.sqlite3"), b"not a sqlite database")
        .expect("corrupt database");

    assert!(matches!(
        SqliteCursorLedger::open(root.path(), CursorLedgerIdentity::new("store:test")),
        Err(CursorLedgerError::Quarantined(_))
    ));
    assert!(!sidecar.exists(), "canonical corrupt sidecar was rotated");
    assert!(
        std::fs::read_dir(root.path())
            .expect("root inventory")
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".pointbreak-derived.quarantine-"))
    );
    assert!(
        EventStore::open(root.path())
            .list_events()
            .expect("truth remains readable")
            .is_empty()
    );
}

#[test]
fn sqlite_cursor_subprocess_entrypoint() {
    let Some(mode) = std::env::var_os(CHILD_MODE) else {
        return;
    };
    let mode = mode.to_string_lossy();
    let root = PathBuf::from(std::env::var_os(CHILD_ROOT).expect("child root"));
    let result = PathBuf::from(std::env::var_os(CHILD_RESULT).expect("child result"));
    let index = std::env::var(CHILD_INDEX)
        .expect("child index")
        .parse::<usize>()
        .expect("numeric child index");
    let attempt = std::env::var(CHILD_ATTEMPT).expect("child attempt");
    match mode.as_ref() {
        "append" => {
            let ledger = SqliteCursorLedger::open(&root, CursorLedgerIdentity::new("store:test"))
                .expect("child open");
            let outcome = ledger
                .append_event(&event(index), &attempt)
                .expect("child append");
            write_signal(
                &result,
                match outcome {
                    AppendResolution::Created(_) => "created",
                    AppendResolution::Existing(_) => "existing",
                    AppendResolution::Conflict(_) => "conflict",
                },
            );
        }
        "crash-append" => {
            let point =
                parse_append_point(&std::env::var(CHILD_POINT).expect("child append crash point"));
            let ledger = SqliteCursorLedger::open(&root, CursorLedgerIdentity::new("store:test"))
                .expect("child open");
            let _ = ledger.append_event_with_hook(&event(index), &attempt, |observed| {
                if observed == point {
                    std::process::exit(86);
                }
            });
            panic!("append child did not reach {point:?}");
        }
        "pause-append" => {
            let ready = PathBuf::from(std::env::var_os(CHILD_READY).expect("child ready"));
            let release = PathBuf::from(std::env::var_os(CHILD_RELEASE).expect("child release"));
            let ledger = SqliteCursorLedger::open(&root, CursorLedgerIdentity::new("store:test"))
                .expect("child open");
            let outcome = ledger
                .append_event_with_hook(&event(index), &attempt, |point| {
                    if point == AppendCrashPoint::AfterIntentCommit {
                        write_signal(&ready, "ready");
                        wait_for_file(&release);
                    }
                })
                .expect("paused child append");
            write_signal(
                &result,
                match outcome {
                    AppendResolution::Created(_) => "created",
                    AppendResolution::Existing(_) => "existing",
                    AppendResolution::Conflict(_) => "conflict",
                },
            );
        }
        "crash-bootstrap" => {
            let point = std::env::var(CHILD_POINT).expect("bootstrap crash point");
            let _ = SqliteCursorLedger::bootstrap_from_truth_with_hook(
                &root,
                CursorLedgerIdentity::new("store:test"),
                if point == "after-quarantine" { 2 } else { 1 },
                |_| BootstrapControl::Continue,
                |observed| match (point.as_str(), observed) {
                    ("during-staging", BootstrapCrashPoint::DuringStaging) => {
                        std::process::exit(87)
                    }
                    ("after-quarantine", BootstrapCrashPoint::AfterQuarantineBeforeNewEpoch) => {
                        std::process::exit(88)
                    }
                    _ => {}
                },
            );
            panic!("bootstrap child did not reach {point}");
        }
        other => panic!("unknown cursor-ledger child mode {other}"),
    }
}

fn spawn_paused_child(
    root: &Path,
    ready: &Path,
    release: &Path,
    result: &Path,
    index: usize,
    attempt: &str,
) -> Child {
    let mut child = child_command("pause-append", root, result, index, attempt);
    child.env(CHILD_READY, ready).env(CHILD_RELEASE, release);
    child.spawn().expect("spawn paused child")
}

fn spawn_child(
    mode: &str,
    root: &Path,
    point: Option<&str>,
    result: &Path,
    index: usize,
    attempt: &str,
) -> Child {
    let mut child = child_command(mode, root, result, index, attempt);
    if let Some(point) = point {
        child.env(CHILD_POINT, point);
    }
    child.spawn().expect("spawn cursor-ledger child")
}

fn child_command(mode: &str, root: &Path, result: &Path, index: usize, attempt: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("current test executable"));
    command
        .arg("--exact")
        .arg(CHILD_TEST)
        .arg("--nocapture")
        .env(CHILD_MODE, mode)
        .env(CHILD_ROOT, root)
        .env(CHILD_RESULT, result)
        .env(CHILD_INDEX, index.to_string())
        .env(CHILD_ATTEMPT, attempt);
    command
}

fn point_code(point: AppendCrashPoint) -> &'static str {
    match point {
        AppendCrashPoint::BeforeIntentCommit => "before-intent-commit",
        AppendCrashPoint::AfterIntentCommit => "after-intent-commit",
        AppendCrashPoint::AfterEventPublication => "after-event-publication",
        AppendCrashPoint::AfterReceiptBeforeHead => "after-receipt-before-head",
        AppendCrashPoint::AfterHeadBeforeIntentRetirement => "after-head-before-intent-retirement",
    }
}

fn parse_append_point(value: &str) -> AppendCrashPoint {
    match value {
        "before-intent-commit" => AppendCrashPoint::BeforeIntentCommit,
        "after-intent-commit" => AppendCrashPoint::AfterIntentCommit,
        "after-event-publication" => AppendCrashPoint::AfterEventPublication,
        "after-receipt-before-head" => AppendCrashPoint::AfterReceiptBeforeHead,
        "after-head-before-intent-retirement" => AppendCrashPoint::AfterHeadBeforeIntentRetirement,
        other => panic!("unknown append crash point {other}"),
    }
}

fn wait_for_file(path: &Path) {
    for _ in 0..1_000 {
        if path.try_exists().expect("signal existence") {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {}", path.display());
}

fn write_signal(path: &Path, value: &str) {
    use std::io::Write;

    let mut file = std::fs::File::create(path).expect("create signal");
    file.write_all(value.as_bytes()).expect("write signal");
    file.sync_all().expect("sync signal");
}
