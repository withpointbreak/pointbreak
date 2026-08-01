#![cfg_attr(not(test), allow(dead_code))]

#[cfg(test)]
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rusqlite::{
    Connection, ErrorCode, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use sha2::{Digest, Sha256};

use super::writer_lock::{StoreWriterLock, WriterLockError};
use super::{DERIVED_QUARANTINE_PREFIX, DERIVED_SIDECAR_DIRECTORY};
use crate::canonical_hash::sha256_bytes_hex;
use crate::error::ShoreError;
use crate::session::derived_access::cursor::{
    AppendResolution, CursorDelta, CursorIntent, CursorReceipt, RecoveryResolution,
    TruthAuthoritySnapshot, TruthCursor, TruthHead,
};
use crate::session::derived_access::{QualificationJournalCursor, QualificationLocalJournal};
use crate::session::event::ShoreEvent;
use crate::session::store::backend::{
    JournalChangeStamp, JournalChangeVerdict, JournalCreatedTransitionVerdict,
};
use crate::session::{EventStore, EventWriteOutcome};

const DATABASE_FILE: &str = "cursor.sqlite3";
const PROFILE_ID: &str = "pointbreak.sqlite-derived-access-cursor.v1";
const SCHEMA_VERSION: i64 = 4;
const APPLICATION_ID: i64 = 0x5042_4443;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
static QUARANTINE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
thread_local! {
    static FULL_CHAIN_QUERY_COUNT: Cell<u64> = const { Cell::new(0) };
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CursorLedgerIdentity {
    store_id: String,
    profile_id: String,
}

impl CursorLedgerIdentity {
    pub(crate) fn new(store_id: impl Into<String>) -> Self {
        Self {
            store_id: store_id.into(),
            profile_id: PROFILE_ID.to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BootstrapControl {
    Continue,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BootstrapProgress {
    pub(crate) completed: usize,
    pub(crate) total: usize,
    pub(crate) bytes_processed: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CursorLedgerCheckpoint {
    pub(crate) busy: bool,
    pub(crate) log_frames: u64,
    pub(crate) checkpointed_frames: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CursorLedgerInventory {
    pub(crate) profile_id: String,
    pub(crate) schema_version: i64,
    pub(crate) epoch: u64,
    pub(crate) head_sequence: u64,
    pub(crate) receipt_count: u64,
    pub(crate) attempt_count: u64,
    pub(crate) active_intent: bool,
    pub(crate) database_bytes: u64,
    pub(crate) wal_bytes: u64,
    pub(crate) shared_memory_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppendCrashPoint {
    BeforeIntentCommit,
    AfterIntentCommit,
    AfterEventPublication,
    AfterReceiptBeforeHead,
    AfterHeadBeforeIntentRetirement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BootstrapCrashPoint {
    DuringStaging,
    AfterQuarantineBeforeNewEpoch,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CursorLedgerError {
    #[error("derived-access writer is busy")]
    WriterBusy,
    #[error("cursor-ledger identity field {field} is empty")]
    EmptyIdentity { field: &'static str },
    #[error("cursor-ledger attempt token was already used: {0}")]
    AttemptTokenUsed(String),
    #[error("cursor-ledger sidecar already exists")]
    AlreadyInitialized,
    #[error("cursor-ledger bootstrap was cancelled")]
    BootstrapCancelled,
    #[error("cursor-ledger bootstrap is incomplete")]
    IncompleteBootstrap,
    #[error("cursor-ledger metadata is quarantined: {0}")]
    Quarantined(String),
    #[error("cursor-ledger metadata identity mismatch: {0}")]
    IdentityMismatch(String),
    #[error("cursor-ledger schema mismatch: {0}")]
    SchemaMismatch(String),
    #[error("cursor-ledger schema requires rebuild: {0}")]
    UpgradeRequired(String),
    #[error("cursor-ledger could not bind the created truth carrier: {0}")]
    AuthorityTransition(String),
    #[error("cursor {cursor:?} is ahead of head {head:?}")]
    CursorAhead {
        cursor: TruthCursor,
        head: TruthCursor,
    },
    #[error("cursor epoch mismatch: expected {expected}, observed {observed}")]
    WrongEpoch { expected: u64, observed: u64 },
    #[error("cursor sequence gap: expected {expected}, observed {observed}")]
    SequenceGap { expected: u64, observed: u64 },
    #[error("delta limit must be greater than zero")]
    ZeroDeltaLimit,
    #[error("unreceipted pre-existing carrier requires quarantine: {0}")]
    UnreceiptedCarrier(String),
    #[error("authoritative carrier is absent: {0}")]
    CarrierAbsent(String),
    #[error("authoritative carrier witness mismatch: {0}")]
    WitnessMismatch(String),
    #[error("truth operation failed: {0}")]
    Truth(String),
    #[error("SQLite operation {operation} failed: {message}")]
    Sqlite {
        operation: &'static str,
        message: String,
    },
    #[error("cursor-ledger I/O failed at {path}: {message}")]
    Io { path: PathBuf, message: String },
}

impl From<WriterLockError> for CursorLedgerError {
    fn from(error: WriterLockError) -> Self {
        match error {
            WriterLockError::Busy => Self::WriterBusy,
            WriterLockError::Io { path, message } => Self::Io { path, message },
        }
    }
}

/// SQLite ledger for roots whose writes all enter through this candidate
/// protocol. Mixed-version and out-of-band writers remain an
/// activation blocker; ordinary freshness reads deliberately do not audit the
/// full loose journal.
#[derive(Clone, Debug)]
pub(crate) struct SqliteCursorLedger {
    store_root: PathBuf,
    sidecar_root: PathBuf,
    database_path: PathBuf,
    identity: CursorLedgerIdentity,
}

#[derive(Debug)]
struct Metadata {
    store_id: String,
    profile_id: String,
    schema_version: i64,
    epoch: u64,
    head_sequence: u64,
    authority_stamp: JournalChangeStamp,
    state: String,
    quarantine_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReceiptChainStats {
    count: u64,
    minimum: u64,
    maximum: u64,
}

#[derive(Clone, Debug)]
struct StoredCursorReceipt {
    cursor: TruthCursor,
    logical_reread_key_hash: String,
    validation_witness: String,
    attempt_token: String,
}

#[derive(Debug)]
pub(crate) struct HydratedCursorDelta {
    pub(crate) delta: CursorDelta,
    pub(crate) events: Vec<ShoreEvent>,
}

#[derive(Debug)]
pub(crate) struct BootstrapPopulationEntry {
    pub(crate) receipt: CursorReceipt,
    pub(crate) event: ShoreEvent,
    pub(crate) carrier_bytes: u64,
}

#[derive(Debug)]
pub(crate) struct BootstrapPopulation {
    pub(crate) ledger: SqliteCursorLedger,
    pub(crate) entries: Vec<BootstrapPopulationEntry>,
}

const BOOTSTRAP_TRANSACTION_BATCH: usize = 512;

impl SqliteCursorLedger {
    pub(crate) fn initialize_empty(
        store_root: &Path,
        identity: CursorLedgerIdentity,
    ) -> Result<Self, CursorLedgerError> {
        validate_identity(&identity)?;
        let store_root = canonical_store_root(store_root)?;
        let _writer_lock = StoreWriterLock::acquire(&store_root)?;
        if !EventStore::open(&store_root)
            .list_events()
            .map_err(|error| CursorLedgerError::Truth(error.to_string()))?
            .is_empty()
        {
            return Err(CursorLedgerError::SchemaMismatch(
                "empty initialization refused an existing loose journal".to_owned(),
            ));
        }
        let ledger = Self::for_root(store_root, identity);
        if ledger.sidecar_path().exists() {
            return Err(CursorLedgerError::AlreadyInitialized);
        }
        std::fs::create_dir_all(ledger.sidecar_path())
            .map_err(|error| io_error(ledger.sidecar_path(), error))?;
        let connection = open_connection(&ledger.database_path, true)?;
        let journal = QualificationLocalJournal::new(&ledger.store_root);
        journal
            .ensure_authority_directory()
            .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
        let authority_stamp = journal
            .change_stamp()
            .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
        initialize_schema(
            &connection,
            &ledger.identity,
            1,
            "complete",
            &authority_stamp,
        )?;
        validate_completed_metadata(&connection, &ledger.identity)?;
        Ok(ledger)
    }

    pub(crate) fn bootstrap_from_truth(
        store_root: &Path,
        identity: CursorLedgerIdentity,
        epoch: u64,
        progress: impl FnMut(BootstrapProgress) -> BootstrapControl,
    ) -> Result<Self, CursorLedgerError> {
        Self::bootstrap_from_truth_with_hook(store_root, identity, epoch, progress, |_| {})
    }

    pub(crate) fn bootstrap_from_truth_with_hook(
        store_root: &Path,
        identity: CursorLedgerIdentity,
        epoch: u64,
        progress: impl FnMut(BootstrapProgress) -> BootstrapControl,
        hook: impl FnMut(BootstrapCrashPoint),
    ) -> Result<Self, CursorLedgerError> {
        let store_root = canonical_store_root(store_root)?;
        let sidecar_root = store_root.join(DERIVED_SIDECAR_DIRECTORY);
        Self::bootstrap_from_truth_at_with_hook(
            &store_root,
            &sidecar_root,
            identity,
            epoch,
            progress,
            hook,
        )
    }

    pub(crate) fn bootstrap_from_truth_at_with_hook(
        store_root: &Path,
        sidecar_root: &Path,
        identity: CursorLedgerIdentity,
        epoch: u64,
        progress: impl FnMut(BootstrapProgress) -> BootstrapControl,
        hook: impl FnMut(BootstrapCrashPoint),
    ) -> Result<Self, CursorLedgerError> {
        Ok(Self::bootstrap_population_from_truth_at_with_hook(
            store_root,
            sidecar_root,
            identity,
            epoch,
            progress,
            hook,
        )?
        .ledger)
    }

    pub(crate) fn bootstrap_population_from_truth_at_with_hook(
        store_root: &Path,
        sidecar_root: &Path,
        identity: CursorLedgerIdentity,
        epoch: u64,
        mut progress: impl FnMut(BootstrapProgress) -> BootstrapControl,
        mut hook: impl FnMut(BootstrapCrashPoint),
    ) -> Result<BootstrapPopulation, CursorLedgerError> {
        validate_identity(&identity)?;
        if epoch == 0 {
            return Err(CursorLedgerError::SchemaMismatch(
                "cursor epoch must be greater than zero".to_owned(),
            ));
        }
        let store_root = canonical_store_root(store_root)?;
        let _writer_lock = StoreWriterLock::acquire(&store_root)?;
        let ledger = Self::for_paths(store_root, sidecar_root.to_path_buf(), identity);
        if ledger.prepare_sidecar_for_bootstrap()? {
            hook(BootstrapCrashPoint::AfterQuarantineBeforeNewEpoch);
        }

        QualificationLocalJournal::new(&ledger.store_root)
            .ensure_authority_directory()
            .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
        let journal = QualificationLocalJournal::new(&ledger.store_root);
        let authority_before = journal
            .change_stamp()
            .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
        let events = EventStore::open(&ledger.store_root)
            .list_events_with_witnesses()
            .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
        let authority_check = journal
            .changes_since(&authority_before)
            .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
        if authority_check.verdict != JournalChangeVerdict::Stable {
            return Err(CursorLedgerError::AuthorityTransition(format!(
                "authoritative truth changed during bootstrap population via {}",
                authority_check.mechanism
            )));
        }
        let authority_stamp = authority_check.after;
        std::fs::create_dir_all(ledger.sidecar_path())
            .map_err(|error| io_error(ledger.sidecar_path(), error))?;
        let connection = open_connection(&ledger.database_path, true)?;
        initialize_schema(
            &connection,
            &ledger.identity,
            epoch,
            "staging",
            &authority_stamp,
        )?;

        let total = events.len();
        if progress(BootstrapProgress {
            completed: 0,
            total,
            bytes_processed: 0,
        }) == BootstrapControl::Cancel
        {
            return Err(CursorLedgerError::BootstrapCancelled);
        }
        let mut connection = connection;
        let mut population = Vec::with_capacity(total);
        let mut events = events.into_iter();
        let mut completed = 0_usize;
        let mut bytes_processed = 0_u64;
        loop {
            let batch = events
                .by_ref()
                .take(BOOTSTRAP_TRANSACTION_BATCH)
                .collect::<Vec<_>>();
            if batch.is_empty() {
                break;
            }
            let batch_start = completed;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| sqlite_error("begin bootstrap batch", error))?;
            for (batch_offset, entry) in batch.into_iter().enumerate() {
                let offset = batch_start.checked_add(batch_offset).ok_or_else(|| {
                    CursorLedgerError::SchemaMismatch("bootstrap offset overflow".to_owned())
                })?;
                let sequence = u64::try_from(offset + 1).map_err(|_| {
                    CursorLedgerError::SchemaMismatch("bootstrap sequence overflow".to_owned())
                })?;
                let attempt_token =
                    format!("bootstrap:{epoch}:{sequence}:{}", entry.validation_witness);
                let receipt = CursorReceipt {
                    cursor: TruthCursor::new(epoch, sequence),
                    logical_reread_key: entry.event.idempotency_key.clone(),
                    validation_witness: entry.validation_witness,
                    attempt_token,
                };
                insert_attempt(&transaction, &receipt.attempt_token)?;
                insert_receipt(&transaction, &receipt)?;
                bytes_processed = bytes_processed.saturating_add(entry.carrier_bytes);
                population.push(BootstrapPopulationEntry {
                    receipt,
                    event: entry.event,
                    carrier_bytes: entry.carrier_bytes,
                });
                hook(BootstrapCrashPoint::DuringStaging);
                if progress(BootstrapProgress {
                    completed: offset + 1,
                    total,
                    bytes_processed,
                }) == BootstrapControl::Cancel
                {
                    return Err(CursorLedgerError::BootstrapCancelled);
                }
            }
            completed = population.len();
            transaction
                .execute(
                    "UPDATE cursor_meta
                     SET head_sequence = ?1
                     WHERE singleton = 1 AND bootstrap_state = 'staging'",
                    [usize_to_i64(completed, "bootstrap head")?],
                )
                .map_err(|error| sqlite_error("advance bootstrap head", error))?;
            transaction
                .commit()
                .map_err(|error| sqlite_error("commit bootstrap batch", error))?;
        }
        connection
            .execute(
                "UPDATE cursor_meta
                 SET bootstrap_state = 'complete'
                 WHERE singleton = 1 AND bootstrap_state = 'staging' AND head_sequence = ?1",
                [usize_to_i64(total, "bootstrap head")?],
            )
            .map_err(|error| sqlite_error("publish bootstrap completion", error))?;
        validate_completed_metadata(&connection, &ledger.identity)?;
        Ok(BootstrapPopulation {
            ledger,
            entries: population,
        })
    }

    pub(crate) fn open(
        store_root: &Path,
        identity: CursorLedgerIdentity,
    ) -> Result<Self, CursorLedgerError> {
        let store_root = canonical_store_root(store_root)?;
        let sidecar_root = store_root.join(DERIVED_SIDECAR_DIRECTORY);
        Self::open_at(&store_root, &sidecar_root, identity)
    }

    pub(crate) fn open_at(
        store_root: &Path,
        sidecar_root: &Path,
        identity: CursorLedgerIdentity,
    ) -> Result<Self, CursorLedgerError> {
        validate_identity(&identity)?;
        let store_root = canonical_store_root(store_root)?;
        let _writer_lock = StoreWriterLock::acquire(&store_root)?;
        let ledger = Self::for_paths(store_root, sidecar_root.to_path_buf(), identity);
        if !ledger.database_path.exists() {
            return Err(CursorLedgerError::IncompleteBootstrap);
        }
        let connection = match open_connection(&ledger.database_path, false) {
            Ok(connection) => connection,
            Err(error) => {
                let reason = error.to_string();
                ledger.rotate_sidecar()?;
                return Err(CursorLedgerError::Quarantined(reason));
            }
        };
        match validate_recoverable_metadata(&connection, &ledger.identity) {
            Ok(_) => Ok(ledger),
            Err(error @ CursorLedgerError::IncompleteBootstrap)
            | Err(error @ CursorLedgerError::UpgradeRequired(_)) => Err(error),
            Err(error) => {
                let reason = error.to_string();
                let _ = mark_quarantined(&connection, &reason);
                Err(CursorLedgerError::Quarantined(reason))
            }
        }
    }

    /// Open one immutable, already-published generation without taking the
    /// truth-publication lock or attempting recovery/quarantine mutations.
    pub(crate) fn open_immutable_at(
        store_root: &Path,
        sidecar_root: &Path,
        identity: CursorLedgerIdentity,
    ) -> Result<Self, CursorLedgerError> {
        validate_identity(&identity)?;
        let store_root = canonical_store_root(store_root)?;
        let ledger = Self::for_paths(store_root, sidecar_root.to_path_buf(), identity);
        if !ledger.database_path.exists() {
            return Err(CursorLedgerError::IncompleteBootstrap);
        }
        let connection = open_connection(&ledger.database_path, false)?;
        validate_completed_metadata(&connection, &ledger.identity)?;
        Ok(ledger)
    }

    pub(crate) fn head(&self) -> Result<TruthHead, CursorLedgerError> {
        let (_connection, metadata) = self.hot_read_connection()?;
        Ok(TruthHead {
            store_id: metadata.store_id,
            cursor: TruthCursor::new(metadata.epoch, metadata.head_sequence),
        })
    }

    /// Read the cursor head and its bound local authority cursor from the same
    /// SQLite snapshot. Callers must continue the stamp before treating the
    /// disposable generation as current.
    pub(crate) fn authority_snapshot(&self) -> Result<TruthAuthoritySnapshot, CursorLedgerError> {
        let (_connection, metadata) = self.hot_read_connection()?;
        Ok(TruthAuthoritySnapshot {
            head: TruthHead {
                store_id: metadata.store_id,
                cursor: TruthCursor::new(metadata.epoch, metadata.head_sequence),
            },
            change_stamp: metadata.authority_stamp,
        })
    }

    /// Replace only the authority cursor for an unchanged derived head. Rebuild
    /// uses this after an exact census and a continuous native no-change proof,
    /// while still holding the canonical truth writer lock.
    pub(crate) fn bind_authority_stamp_locked(
        &self,
        expected: TruthCursor,
        authority_stamp: &JournalChangeStamp,
        _writer_lock: &StoreWriterLock,
    ) -> Result<(), CursorLedgerError> {
        let connection = open_connection(&self.database_path, false)?;
        validate_completed_metadata(&connection, &self.identity)?;
        let updated = connection
            .execute(
                "UPDATE cursor_meta
                 SET authority_stamp_json = ?1
                 WHERE singleton = 1 AND epoch = ?2 AND head_sequence = ?3",
                params![
                    encode_authority_stamp(authority_stamp)?,
                    u64_to_i64(expected.epoch, "authority epoch")?,
                    u64_to_i64(expected.sequence, "authority head")?,
                ],
            )
            .map_err(|error| sqlite_error("bind authority stamp", error))?;
        if updated != 1 {
            return Err(CursorLedgerError::SchemaMismatch(
                "cursor head changed while binding authority stamp".to_owned(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn connection_policy_for_test(&self) -> Result<(String, i64), CursorLedgerError> {
        let connection = Connection::open_with_flags(
            &self.database_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| sqlite_error("open cursor ledger for policy test", error))?;
        connection
            .pragma_update(None, "synchronous", "NORMAL")
            .map_err(|error| sqlite_error("seed cursor synchronous policy test", error))?;
        configure_connection(&connection, false)?;
        let journal_mode = connection
            .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
            .map_err(|error| sqlite_error("read journal mode", error))?;
        let synchronous = connection
            .pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0))
            .map_err(|error| sqlite_error("read synchronous", error))?;
        Ok((journal_mode, synchronous))
    }

    #[cfg(test)]
    pub(crate) fn head_with_snapshot_hook(
        &self,
        hook: impl FnOnce(),
    ) -> Result<TruthHead, CursorLedgerError> {
        let (_connection, metadata) = self.hot_read_connection_with_hook(hook)?;
        Ok(TruthHead {
            store_id: metadata.store_id,
            cursor: TruthCursor::new(metadata.epoch, metadata.head_sequence),
        })
    }

    pub(crate) fn append_event(
        &self,
        event: &ShoreEvent,
        attempt_token: &str,
    ) -> Result<AppendResolution, CursorLedgerError> {
        self.append_event_with_lock(event, attempt_token, false, |_| {})
    }

    pub(crate) fn try_append_event(
        &self,
        event: &ShoreEvent,
        attempt_token: &str,
    ) -> Result<AppendResolution, CursorLedgerError> {
        self.append_event_with_lock(event, attempt_token, true, |_| {})
    }

    pub(crate) fn append_event_with_hook(
        &self,
        event: &ShoreEvent,
        attempt_token: &str,
        hook: impl FnMut(AppendCrashPoint),
    ) -> Result<AppendResolution, CursorLedgerError> {
        self.append_event_with_lock(event, attempt_token, false, hook)
    }

    fn append_event_with_lock(
        &self,
        event: &ShoreEvent,
        attempt_token: &str,
        try_only: bool,
        hook: impl FnMut(AppendCrashPoint),
    ) -> Result<AppendResolution, CursorLedgerError> {
        let journal = QualificationLocalJournal::new(&self.store_root);
        validate_nonempty("attempt_token", attempt_token)?;
        validate_nonempty("logical_reread_key", &event.idempotency_key)?;
        let writer_lock = if try_only {
            StoreWriterLock::try_acquire(&self.store_root)?
        } else {
            StoreWriterLock::acquire(&self.store_root)?
        };
        self.append_event_with_publisher_locked(event, attempt_token, &writer_lock, hook, || {
            journal.record_event_once(event)
        })
    }

    pub(crate) fn append_event_with_publisher_locked(
        &self,
        event: &ShoreEvent,
        attempt_token: &str,
        _writer_lock: &StoreWriterLock,
        hook: impl FnMut(AppendCrashPoint),
        publish: impl FnOnce() -> crate::error::Result<EventWriteOutcome>,
    ) -> Result<AppendResolution, CursorLedgerError> {
        self.append_event_locked(event, attempt_token, hook, publish)
    }

    fn append_event_locked(
        &self,
        event: &ShoreEvent,
        attempt_token: &str,
        mut hook: impl FnMut(AppendCrashPoint),
        publish: impl FnOnce() -> crate::error::Result<EventWriteOutcome>,
    ) -> Result<AppendResolution, CursorLedgerError> {
        validate_nonempty("attempt_token", attempt_token)?;
        validate_nonempty("logical_reread_key", &event.idempotency_key)?;
        let expected_bytes = serde_json::to_vec(event)
            .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
        let expected_witness = sha256_bytes_hex(&expected_bytes);
        let mut connection = open_connection(&self.database_path, false)?;
        validate_recoverable_metadata(&connection, &self.identity)?;
        recover_locked(&mut connection, &self.store_root, &self.identity)?;

        let metadata = read_metadata(&connection)?;
        let journal = QualificationLocalJournal::new(&self.store_root);
        let authority_observation = journal
            .begin_created_transition(&metadata.authority_stamp, &event.idempotency_key)
            .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
        let proposed_cursor = TruthCursor::new(
            metadata.epoch,
            metadata.head_sequence.checked_add(1).ok_or_else(|| {
                CursorLedgerError::SchemaMismatch("cursor sequence overflow".to_owned())
            })?,
        );
        let intent = CursorIntent::new(
            proposed_cursor,
            event.idempotency_key.clone(),
            expected_witness.clone(),
            attempt_token,
        );
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| sqlite_error("begin intent", error))?;
        insert_attempt(&transaction, attempt_token)?;
        insert_intent(&transaction, &intent)?;
        hook(AppendCrashPoint::BeforeIntentCommit);
        transaction
            .commit()
            .map_err(|error| sqlite_error("commit intent", error))?;
        hook(AppendCrashPoint::AfterIntentCommit);

        let existing_receipt = receipt_for_key(&connection, &journal, &event.idempotency_key)?;
        let publication = match publish() {
            Ok(outcome) => outcome,
            Err(ShoreError::Message(message))
                if existing_receipt.is_some()
                    && message
                        == format!(
                            "event conflict for idempotency key {}",
                            event.idempotency_key
                        ) =>
            {
                retire_intent(&connection)?;
                return Ok(AppendResolution::Conflict(
                    existing_receipt.expect("guarded existing receipt").cursor,
                ));
            }
            Err(error) => {
                retire_intent(&connection)?;
                return Err(CursorLedgerError::Truth(error.to_string()));
            }
        };
        hook(AppendCrashPoint::AfterEventPublication);

        if let Some(receipt) = existing_receipt {
            if matches!(publication, EventWriteOutcome::Created) {
                let reason = format!(
                    "receipt exists but authoritative carrier was recreated: {}",
                    event.idempotency_key
                );
                mark_quarantined(&connection, &reason)?;
                return Err(CursorLedgerError::Quarantined(reason));
            }
            retire_intent(&connection)?;
            return Ok(AppendResolution::Existing(receipt.cursor));
        }

        if !matches!(publication, EventWriteOutcome::Created) {
            let reason = format!(
                "unreceipted pre-existing carrier: {}",
                event.idempotency_key
            );
            mark_quarantined(&connection, &reason)?;
            return Err(CursorLedgerError::UnreceiptedCarrier(
                event.idempotency_key.clone(),
            ));
        }
        validate_named_carrier(&journal, &event.idempotency_key, &expected_witness)?;
        let transition = journal
            .finish_created_transition(authority_observation)
            .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
        if transition.verdict != JournalCreatedTransitionVerdict::Accepted {
            return Err(CursorLedgerError::AuthorityTransition(format!(
                "{:?}: {}",
                transition.verdict, transition.mechanism
            )));
        }
        let authority_stamp = transition.after;

        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| sqlite_error("begin receipt and head", error))?;
        insert_receipt(
            &transaction,
            &CursorReceipt {
                cursor: proposed_cursor,
                logical_reread_key: event.idempotency_key.clone(),
                validation_witness: expected_witness,
                attempt_token: attempt_token.to_owned(),
            },
        )?;
        hook(AppendCrashPoint::AfterReceiptBeforeHead);
        advance_head(&transaction, proposed_cursor, &authority_stamp)?;
        transaction
            .commit()
            .map_err(|error| sqlite_error("commit receipt and head", error))?;
        hook(AppendCrashPoint::AfterHeadBeforeIntentRetirement);
        retire_intent(&connection)?;
        Ok(AppendResolution::Created(proposed_cursor))
    }

    pub(crate) fn recover(&self) -> Result<RecoveryResolution, CursorLedgerError> {
        let _writer_lock = StoreWriterLock::acquire(&self.store_root)?;
        let mut connection = open_connection(&self.database_path, false)?;
        validate_recoverable_metadata(&connection, &self.identity)?;
        recover_locked(&mut connection, &self.store_root, &self.identity)
    }

    pub(crate) fn try_recover(&self) -> Result<RecoveryResolution, CursorLedgerError> {
        let _writer_lock = match StoreWriterLock::try_acquire(&self.store_root) {
            Ok(writer_lock) => writer_lock,
            Err(WriterLockError::Busy) => return Ok(RecoveryResolution::LiveWriterBusy),
            Err(error) => return Err(error.into()),
        };
        let mut connection = open_connection(&self.database_path, false)?;
        validate_recoverable_metadata(&connection, &self.identity)?;
        recover_locked(&mut connection, &self.store_root, &self.identity)
    }

    pub(crate) fn events_after(
        &self,
        after: TruthCursor,
        limit: usize,
    ) -> Result<CursorDelta, CursorLedgerError> {
        Ok(self.events_after_hydrated(after, limit)?.delta)
    }

    pub(crate) fn events_after_hydrated(
        &self,
        after: TruthCursor,
        limit: usize,
    ) -> Result<HydratedCursorDelta, CursorLedgerError> {
        if limit == 0 {
            return Err(CursorLedgerError::ZeroDeltaLimit);
        }
        let (connection, metadata) = self.hot_read_connection()?;
        if after.epoch != metadata.epoch {
            return Err(CursorLedgerError::WrongEpoch {
                expected: metadata.epoch,
                observed: after.epoch,
            });
        }
        let head = TruthCursor::new(metadata.epoch, metadata.head_sequence);
        if after.sequence > metadata.head_sequence {
            return Err(CursorLedgerError::CursorAhead {
                cursor: after,
                head,
            });
        }

        let mut statement = connection
            .prepare(
                "SELECT epoch, sequence, logical_reread_key_hash,
                        validation_witness, attempt_token
                 FROM cursor_receipt_text
                 WHERE sequence > ?1
                 ORDER BY sequence
                 LIMIT ?2",
            )
            .map_err(|error| sqlite_error("prepare bounded delta", error))?;
        let rows = statement
            .query_map(
                params![
                    u64_to_i64(after.sequence, "delta start")?,
                    usize_to_i64(limit, "delta limit")?
                ],
                stored_receipt_from_row,
            )
            .map_err(|error| sqlite_error("query bounded delta", error))?;
        let journal = QualificationLocalJournal::new(&self.store_root);
        let mut receipts = Vec::new();
        let mut events = Vec::new();
        for row in rows {
            let stored = row.map_err(|error| sqlite_error("read bounded delta", error))?;
            let (receipt, event) = hydrate_stored_receipt(&journal, &stored)?;
            receipts.push(receipt);
            events.push(event);
        }
        drop(statement);

        for (offset, receipt) in receipts.iter().enumerate() {
            let expected = after.sequence + u64::try_from(offset).unwrap_or(u64::MAX) + 1;
            if receipt.cursor.epoch != metadata.epoch || receipt.cursor.sequence != expected {
                return Err(CursorLedgerError::SequenceGap {
                    expected,
                    observed: receipt.cursor.sequence,
                });
            }
        }
        let observed = receipts
            .last()
            .map_or(after.sequence, |receipt| receipt.cursor.sequence);
        Ok(HydratedCursorDelta {
            delta: CursorDelta {
                after,
                observed_head: head,
                complete: observed == metadata.head_sequence,
                receipts,
            },
            events,
        })
    }

    pub(crate) fn integrity_check(&self) -> Result<(), CursorLedgerError> {
        let connection = self.validated_connection()?;
        let result = connection
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .map_err(|error| sqlite_error("run integrity check", error))?;
        if result != "ok" {
            return Err(CursorLedgerError::SchemaMismatch(format!(
                "SQLite integrity check returned {result}"
            )));
        }
        Ok(())
    }

    pub(crate) fn checkpoint(&self) -> Result<CursorLedgerCheckpoint, CursorLedgerError> {
        let connection = self.validated_connection()?;
        let (busy, log_frames, checkpointed_frames) = connection
            .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|error| sqlite_error("checkpoint cursor ledger", error))?;
        Ok(CursorLedgerCheckpoint {
            busy: busy != 0,
            log_frames: i64_to_u64(log_frames, "checkpoint log frames")?,
            checkpointed_frames: i64_to_u64(checkpointed_frames, "checkpointed frames")?,
        })
    }

    pub(crate) fn inventory(&self) -> Result<CursorLedgerInventory, CursorLedgerError> {
        let connection = self.validated_connection()?;
        let metadata = read_metadata(&connection)?;
        let stats = receipt_chain_stats(&connection)?;
        let attempt_count = connection
            .query_row("SELECT count(*) FROM cursor_attempt", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|error| sqlite_error("count cursor attempts", error))
            .and_then(|count| i64_to_u64(count, "attempt count"))?;
        Ok(CursorLedgerInventory {
            profile_id: metadata.profile_id,
            schema_version: metadata.schema_version,
            epoch: metadata.epoch,
            head_sequence: metadata.head_sequence,
            receipt_count: stats.count,
            attempt_count,
            active_intent: read_intent(&connection)?.is_some(),
            database_bytes: file_len(&self.database_path)?,
            wal_bytes: file_len(&sqlite_companion_path(&self.database_path, "-wal"))?,
            shared_memory_bytes: file_len(&sqlite_companion_path(&self.database_path, "-shm"))?,
        })
    }

    fn for_root(store_root: PathBuf, identity: CursorLedgerIdentity) -> Self {
        let sidecar_root = store_root.join(DERIVED_SIDECAR_DIRECTORY);
        Self::for_paths(store_root, sidecar_root, identity)
    }

    fn for_paths(
        store_root: PathBuf,
        sidecar_root: PathBuf,
        identity: CursorLedgerIdentity,
    ) -> Self {
        let database_path = sidecar_root.join(DATABASE_FILE);
        Self {
            store_root,
            sidecar_root,
            database_path,
            identity,
        }
    }

    fn sidecar_path(&self) -> &Path {
        &self.sidecar_root
    }

    fn validated_connection(&self) -> Result<Connection, CursorLedgerError> {
        let connection = open_connection(&self.database_path, false)?;
        validate_completed_metadata(&connection, &self.identity)?;
        Ok(connection)
    }

    fn hot_read_connection(&self) -> Result<(Connection, Metadata), CursorLedgerError> {
        self.hot_read_connection_with_hook(|| {})
    }

    fn hot_read_connection_with_hook(
        &self,
        hook: impl FnOnce(),
    ) -> Result<(Connection, Metadata), CursorLedgerError> {
        let connection = open_connection(&self.database_path, false)?;
        // Keep metadata, bounded-head validation, and any subsequent delta query
        // on one WAL snapshot. Without an explicit read transaction, a writer
        // can commit between the metadata and receipt SELECTs and make an atomic
        // receipt-plus-head publication look torn to the reader.
        connection
            .execute_batch("BEGIN DEFERRED")
            .map_err(|error| sqlite_error("begin hot-read snapshot", error))?;
        let metadata = validate_metadata_header(&connection, &self.identity)?;
        hook();
        validate_bounded_head(&connection, &metadata)?;
        Ok((connection, metadata))
    }

    fn prepare_sidecar_for_bootstrap(&self) -> Result<bool, CursorLedgerError> {
        let sidecar = self.sidecar_path();
        if !sidecar.exists() {
            return Ok(false);
        }
        if self.database_path.exists()
            && let Ok(connection) = open_connection(&self.database_path, false)
            && validate_completed_metadata(&connection, &self.identity).is_ok()
        {
            return Err(CursorLedgerError::AlreadyInitialized);
        }
        self.rotate_sidecar()?;
        Ok(true)
    }

    fn rotate_sidecar(&self) -> Result<PathBuf, CursorLedgerError> {
        let sidecar = self.sidecar_path();
        let quarantine = self.store_root.join(format!(
            "{DERIVED_QUARANTINE_PREFIX}{}-{}",
            std::process::id(),
            QUARANTINE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::rename(sidecar, &quarantine).map_err(|error| io_error(sidecar, error))?;
        Ok(quarantine)
    }
}

#[cfg(test)]
pub(crate) fn full_chain_query_count_for_test() -> u64 {
    FULL_CHAIN_QUERY_COUNT.with(Cell::get)
}

impl QualificationJournalCursor for SqliteCursorLedger {
    type Error = CursorLedgerError;

    fn qualification_truth_head(&self) -> Result<TruthHead, Self::Error> {
        self.head()
    }

    fn qualification_events_after(
        &self,
        after: TruthCursor,
        limit: usize,
    ) -> Result<CursorDelta, Self::Error> {
        self.events_after(after, limit)
    }
}

fn canonical_store_root(store_root: &Path) -> Result<PathBuf, CursorLedgerError> {
    std::fs::create_dir_all(store_root).map_err(|error| io_error(store_root, error))?;
    store_root
        .canonicalize()
        .map_err(|error| io_error(store_root, error))
}

fn validate_identity(identity: &CursorLedgerIdentity) -> Result<(), CursorLedgerError> {
    validate_nonempty("store_id", &identity.store_id)?;
    validate_nonempty("profile_id", &identity.profile_id)
}

fn validate_nonempty(field: &'static str, value: &str) -> Result<(), CursorLedgerError> {
    if value.is_empty() {
        return Err(CursorLedgerError::EmptyIdentity { field });
    }
    Ok(())
}

fn encode_authority_stamp(stamp: &JournalChangeStamp) -> Result<String, CursorLedgerError> {
    serde_json::to_string(stamp).map_err(|error| {
        CursorLedgerError::SchemaMismatch(format!("could not encode authority stamp: {error}"))
    })
}

fn decode_authority_stamp(value: &str) -> Result<JournalChangeStamp, CursorLedgerError> {
    serde_json::from_str(value).map_err(|error| {
        CursorLedgerError::SchemaMismatch(format!("could not decode authority stamp: {error}"))
    })
}

fn sha256_digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn decode_digest(value: &str, label: &'static str) -> Result<[u8; 32], CursorLedgerError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(CursorLedgerError::SchemaMismatch(format!(
            "{label} is not a 64-character lowercase hexadecimal digest"
        )));
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        digest[index] = u8::from_str_radix(
            std::str::from_utf8(pair).expect("ASCII hex slices are UTF-8"),
            16,
        )
        .expect("validated hexadecimal pairs must decode");
    }
    Ok(digest)
}

fn open_connection(path: &Path, create: bool) -> Result<Connection, CursorLedgerError> {
    if !create && !path.exists() {
        return Err(CursorLedgerError::IncompleteBootstrap);
    }
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let connection = Connection::open_with_flags(path, flags)
        .map_err(|error| sqlite_error("open cursor ledger", error))?;
    configure_connection(&connection, create)?;
    Ok(connection)
}

fn configure_connection(connection: &Connection, create: bool) -> Result<(), CursorLedgerError> {
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|error| sqlite_error("set busy timeout", error))?;
    if create {
        connection
            .pragma_update(None, "page_size", 4096_i64)
            .map_err(|error| sqlite_error("set page size", error))?;
        connection
            .pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(|error| sqlite_error("set application id", error))?;
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|error| sqlite_error("set user version", error))?;
    }
    let journal_mode = connection
        .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))
        .map_err(|error| sqlite_error("enable WAL", error))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(CursorLedgerError::SchemaMismatch(format!(
            "SQLite refused WAL mode and returned {journal_mode}"
        )));
    }
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|error| sqlite_error("set synchronous", error))?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|error| sqlite_error("enable foreign keys", error))?;
    connection
        .pragma_update(None, "cell_size_check", true)
        .map_err(|error| sqlite_error("enable cell-size checks", error))?;
    #[cfg(target_os = "macos")]
    connection
        .pragma_update(None, "fullfsync", true)
        .map_err(|error| sqlite_error("enable fullfsync", error))?;
    Ok(())
}

fn initialize_schema(
    connection: &Connection,
    identity: &CursorLedgerIdentity,
    epoch: u64,
    state: &str,
    authority_stamp: &JournalChangeStamp,
) -> Result<(), CursorLedgerError> {
    connection
        .execute_batch(
            "CREATE TABLE cursor_meta (
                 singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                 store_id TEXT NOT NULL,
                 profile_id TEXT NOT NULL,
                 schema_version INTEGER NOT NULL CHECK (schema_version = 4),
                 epoch INTEGER NOT NULL CHECK (epoch > 0),
                 head_sequence INTEGER NOT NULL CHECK (head_sequence >= 0),
                 authority_stamp_json TEXT NOT NULL
                     CHECK (length(authority_stamp_json) > 0),
                 bootstrap_state TEXT NOT NULL
                     CHECK (bootstrap_state IN ('staging', 'complete', 'quarantined')),
                 quarantine_reason TEXT
             ) STRICT;
             CREATE TABLE cursor_attempt (
                 attempt_hash BLOB PRIMARY KEY CHECK (length(attempt_hash) = 32)
             ) STRICT, WITHOUT ROWID;
             CREATE TABLE cursor_intent (
                 singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                 epoch INTEGER NOT NULL CHECK (epoch > 0),
                 sequence INTEGER NOT NULL CHECK (sequence > 0),
                 logical_reread_key TEXT NOT NULL CHECK (length(logical_reread_key) > 0),
                 validation_witness TEXT NOT NULL CHECK (length(validation_witness) = 64),
                 attempt_hash BLOB NOT NULL UNIQUE CHECK (length(attempt_hash) = 32)
                     REFERENCES cursor_attempt(attempt_hash),
                 attempt_token TEXT NOT NULL CHECK (length(attempt_token) > 0)
             ) STRICT;
             CREATE TABLE cursor_receipt (
                 sequence INTEGER PRIMARY KEY CHECK (sequence > 0),
                 epoch INTEGER NOT NULL CHECK (epoch > 0),
                 logical_reread_key_hash BLOB NOT NULL UNIQUE
                     CHECK (length(logical_reread_key_hash) = 32),
                 validation_witness_hash BLOB NOT NULL
                     CHECK (length(validation_witness_hash) = 32),
                 attempt_hash BLOB NOT NULL UNIQUE CHECK (length(attempt_hash) = 32)
                     REFERENCES cursor_attempt(attempt_hash),
                 attempt_token TEXT CHECK (length(attempt_token) > 0)
             ) STRICT;
             CREATE VIEW cursor_receipt_text AS
             SELECT sequence, epoch,
                    lower(hex(logical_reread_key_hash)) AS logical_reread_key_hash,
                    lower(hex(validation_witness_hash)) AS validation_witness,
                    coalesce(
                        attempt_token,
                        'bootstrap:' || epoch || ':' || sequence || ':'
                            || lower(hex(validation_witness_hash))
                    ) AS attempt_token
             FROM cursor_receipt;",
        )
        .map_err(|error| sqlite_error("create cursor schema", error))?;
    connection
        .execute(
            "INSERT INTO cursor_meta
             (singleton, store_id, profile_id, schema_version, epoch, head_sequence,
              authority_stamp_json, bootstrap_state, quarantine_reason)
             VALUES (1, ?1, ?2, ?3, ?4, 0, ?5, ?6, NULL)",
            params![
                identity.store_id,
                identity.profile_id,
                SCHEMA_VERSION,
                u64_to_i64(epoch, "epoch")?,
                encode_authority_stamp(authority_stamp)?,
                state,
            ],
        )
        .map_err(|error| sqlite_error("insert cursor metadata", error))?;
    Ok(())
}

fn read_metadata(connection: &Connection) -> Result<Metadata, CursorLedgerError> {
    connection
        .query_row(
            "SELECT store_id, profile_id, schema_version, epoch, head_sequence,
                    authority_stamp_json, bootstrap_state, quarantine_reason
             FROM cursor_meta WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .map_err(|error| sqlite_error("read cursor metadata", error))
        .and_then(
            |(
                store_id,
                profile_id,
                schema_version,
                epoch,
                head_sequence,
                authority_stamp_json,
                state,
                quarantine_reason,
            )| {
                Ok(Metadata {
                    store_id,
                    profile_id,
                    schema_version,
                    epoch: i64_to_u64(epoch, "epoch")?,
                    head_sequence: i64_to_u64(head_sequence, "head sequence")?,
                    authority_stamp: decode_authority_stamp(&authority_stamp_json)?,
                    state,
                    quarantine_reason,
                })
            },
        )
}

fn validate_completed_metadata(
    connection: &Connection,
    identity: &CursorLedgerIdentity,
) -> Result<Metadata, CursorLedgerError> {
    let metadata = validate_metadata_header(connection, identity)?;
    validate_receipt_chain(connection)?;
    Ok(metadata)
}

fn validate_recoverable_metadata(
    connection: &Connection,
    identity: &CursorLedgerIdentity,
) -> Result<Metadata, CursorLedgerError> {
    let metadata = validate_metadata_header(connection, identity)?;
    validate_recoverable_receipt_chain(connection, &metadata)?;
    Ok(metadata)
}

fn validate_metadata_header(
    connection: &Connection,
    identity: &CursorLedgerIdentity,
) -> Result<Metadata, CursorLedgerError> {
    let application_id = pragma_i64(connection, "application_id")?;
    let user_version = pragma_i64(connection, "user_version")?;
    if application_id == APPLICATION_ID && user_version < SCHEMA_VERSION {
        return Err(CursorLedgerError::UpgradeRequired(format!(
            "expected schema {SCHEMA_VERSION}, observed {user_version}"
        )));
    }
    if application_id != APPLICATION_ID || user_version != SCHEMA_VERSION {
        return Err(CursorLedgerError::SchemaMismatch(format!(
            "application_id={application_id}, user_version={user_version}"
        )));
    }
    let metadata = read_metadata(connection)?;
    if metadata.store_id != identity.store_id || metadata.profile_id != identity.profile_id {
        return Err(CursorLedgerError::IdentityMismatch(format!(
            "expected {}/{}, observed {}/{}",
            identity.store_id, identity.profile_id, metadata.store_id, metadata.profile_id
        )));
    }
    if metadata.schema_version < SCHEMA_VERSION {
        return Err(CursorLedgerError::UpgradeRequired(format!(
            "expected schema {SCHEMA_VERSION}, observed {}",
            metadata.schema_version
        )));
    }
    if metadata.schema_version != SCHEMA_VERSION {
        return Err(CursorLedgerError::SchemaMismatch(format!(
            "expected schema {SCHEMA_VERSION}, observed {}",
            metadata.schema_version
        )));
    }
    match metadata.state.as_str() {
        "complete" => {}
        "staging" => return Err(CursorLedgerError::IncompleteBootstrap),
        "quarantined" => {
            return Err(CursorLedgerError::Quarantined(
                metadata
                    .quarantine_reason
                    .clone()
                    .unwrap_or_else(|| "unspecified metadata failure".to_owned()),
            ));
        }
        other => {
            return Err(CursorLedgerError::SchemaMismatch(format!(
                "unsupported bootstrap state {other}"
            )));
        }
    }
    Ok(metadata)
}

fn validate_receipt_chain(connection: &Connection) -> Result<(), CursorLedgerError> {
    let metadata = read_metadata(connection)?;
    validate_receipt_epochs(connection, metadata.epoch)?;
    let stats = receipt_chain_stats(connection)?;
    if metadata.head_sequence != stats.count
        || (stats.count > 0 && (stats.minimum != 1 || stats.maximum != metadata.head_sequence))
    {
        return Err(CursorLedgerError::SchemaMismatch(format!(
            "receipt chain mismatch: head={}, count={}, min={}, max={}",
            metadata.head_sequence, stats.count, stats.minimum, stats.maximum
        )));
    }
    Ok(())
}

fn validate_bounded_head(
    connection: &Connection,
    metadata: &Metadata,
) -> Result<(), CursorLedgerError> {
    if metadata.head_sequence > 0 {
        let observed_epoch = connection
            .query_row(
                "SELECT epoch FROM cursor_receipt WHERE sequence = ?1",
                [u64_to_i64(metadata.head_sequence, "head receipt")?],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|error| sqlite_error("validate head receipt", error))?
            .ok_or_else(|| {
                CursorLedgerError::SchemaMismatch(format!(
                    "head {} has no receipt",
                    metadata.head_sequence
                ))
            })?;
        let observed_epoch = i64_to_u64(observed_epoch, "head receipt epoch")?;
        if observed_epoch != metadata.epoch {
            return Err(CursorLedgerError::WrongEpoch {
                expected: metadata.epoch,
                observed: observed_epoch,
            });
        }
    }

    let next_sequence = metadata
        .head_sequence
        .checked_add(1)
        .ok_or_else(|| CursorLedgerError::SchemaMismatch("cursor head overflow".to_owned()))?;
    let receipt_ahead = connection
        .query_row(
            "SELECT 1 FROM cursor_receipt WHERE sequence = ?1",
            [u64_to_i64(next_sequence, "next receipt")?],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| sqlite_error("validate next receipt", error))?
        .is_some();
    if receipt_ahead {
        return Err(CursorLedgerError::SchemaMismatch(format!(
            "receipt {next_sequence} is ahead of head {}",
            metadata.head_sequence
        )));
    }
    Ok(())
}

fn validate_recoverable_receipt_chain(
    connection: &Connection,
    metadata: &Metadata,
) -> Result<(), CursorLedgerError> {
    validate_receipt_epochs(connection, metadata.epoch)?;
    let stats = receipt_chain_stats(connection)?;
    let maximum_recoverable = metadata
        .head_sequence
        .checked_add(1)
        .ok_or_else(|| CursorLedgerError::SchemaMismatch("cursor head overflow".to_owned()))?;
    if stats.count < metadata.head_sequence
        || stats.count > maximum_recoverable
        || (stats.count > 0 && (stats.minimum != 1 || stats.maximum != stats.count))
    {
        return Err(CursorLedgerError::SchemaMismatch(format!(
            "unrecoverable receipt chain: head={}, count={}, min={}, max={}",
            metadata.head_sequence, stats.count, stats.minimum, stats.maximum
        )));
    }
    Ok(())
}

fn receipt_chain_stats(connection: &Connection) -> Result<ReceiptChainStats, CursorLedgerError> {
    note_full_chain_query();
    let (count, minimum, maximum) = connection
        .query_row(
            "SELECT count(*), coalesce(min(sequence), 0), coalesce(max(sequence), 0)
             FROM cursor_receipt",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .map_err(|error| sqlite_error("inspect receipt chain", error))?;
    Ok(ReceiptChainStats {
        count: i64_to_u64(count, "receipt count")?,
        minimum: i64_to_u64(minimum, "minimum receipt")?,
        maximum: i64_to_u64(maximum, "maximum receipt")?,
    })
}

fn validate_receipt_epochs(
    connection: &Connection,
    expected_epoch: u64,
) -> Result<(), CursorLedgerError> {
    note_full_chain_query();
    let wrong_epoch = connection
        .query_row(
            "SELECT epoch FROM cursor_receipt WHERE epoch != ?1 LIMIT 1",
            [u64_to_i64(expected_epoch, "expected receipt epoch")?],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| sqlite_error("validate receipt epochs", error))?;
    if let Some(observed) = wrong_epoch {
        return Err(CursorLedgerError::WrongEpoch {
            expected: expected_epoch,
            observed: i64_to_u64(observed, "observed receipt epoch")?,
        });
    }
    Ok(())
}

fn note_full_chain_query() {
    #[cfg(test)]
    FULL_CHAIN_QUERY_COUNT.with(|count| count.set(count.get() + 1));
}

fn capture_created_authority_stamp(
    store_root: &Path,
    before: &JournalChangeStamp,
    _expected_truth_count: u64,
) -> Result<JournalChangeStamp, CursorLedgerError> {
    let journal = QualificationLocalJournal::new(store_root);
    let unchanged = journal
        .changes_since(before)
        .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
    if unchanged.verdict == crate::session::store::backend::JournalChangeVerdict::Stable {
        return Ok(before.clone());
    }
    #[cfg(target_os = "linux")]
    let transition = {
        let after = journal
            .change_stamp()
            .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
        let observed = journal
            .head_marker()
            .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
        crate::session::store::backend::JournalCreatedTransition {
            after,
            verdict: if observed == _expected_truth_count {
                JournalCreatedTransitionVerdict::Accepted
            } else {
                JournalCreatedTransitionVerdict::Contended
            },
            mechanism: format!(
                "explicit recovery counted {observed} truth carriers; expected {_expected_truth_count}"
            ),
        }
    };
    #[cfg(not(target_os = "linux"))]
    let transition = journal
        .created_transition(before)
        .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
    if transition.verdict != JournalCreatedTransitionVerdict::Accepted {
        return Err(CursorLedgerError::AuthorityTransition(format!(
            "{:?}: {}",
            transition.verdict, transition.mechanism
        )));
    }
    Ok(transition.after)
}

fn recover_locked(
    connection: &mut Connection,
    store_root: &Path,
    identity: &CursorLedgerIdentity,
) -> Result<RecoveryResolution, CursorLedgerError> {
    let metadata = validate_recoverable_metadata(connection, identity)?;
    let Some(intent) = read_intent(connection)? else {
        return recover_head_only(connection, store_root, &metadata);
    };
    let journal = QualificationLocalJournal::new(store_root);
    if let Some(receipt) = receipt_for_key(connection, &journal, &intent.logical_reread_key)? {
        if receipt.cursor.sequence > metadata.head_sequence {
            if receipt.cursor.sequence != metadata.head_sequence + 1
                || receipt.cursor != intent.proposed_cursor
            {
                let reason = format!(
                    "receipt sequence {} cannot advance head {}",
                    receipt.cursor.sequence, metadata.head_sequence
                );
                mark_quarantined(connection, &reason)?;
                return Err(CursorLedgerError::Quarantined(reason));
            }
            let authority_stamp = capture_created_authority_stamp(
                store_root,
                &metadata.authority_stamp,
                receipt.cursor.sequence,
            )?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| sqlite_error("begin head recovery", error))?;
            advance_head(&transaction, receipt.cursor, &authority_stamp)?;
            transaction
                .execute("DELETE FROM cursor_intent WHERE singleton = 1", [])
                .map_err(|error| sqlite_error("retire recovered intent", error))?;
            transaction
                .commit()
                .map_err(|error| sqlite_error("commit head recovery", error))?;
            return Ok(RecoveryResolution::AdvancedHead(receipt.cursor));
        }
        retire_intent(connection)?;
        if receipt.cursor == intent.proposed_cursor
            && receipt.cursor.sequence == metadata.head_sequence
            && receipt.validation_witness == intent.validation_witness
            && receipt.attempt_token == intent.attempt_token
        {
            return Ok(RecoveryResolution::RetiredFinalized(receipt.cursor));
        }
        return Ok(classify_recovered_receipt(
            &receipt,
            &intent.validation_witness,
        ));
    }

    let Some(bytes) = journal
        .read_event_bytes(&intent.logical_reread_key)
        .map_err(|error| CursorLedgerError::Truth(error.to_string()))?
    else {
        retire_intent(connection)?;
        return Ok(RecoveryResolution::RetiredAbsent);
    };
    if sha256_bytes_hex(&bytes) != intent.validation_witness {
        let reason = format!(
            "unreceipted carrier witness mismatch: {}",
            intent.logical_reread_key
        );
        mark_quarantined(connection, &reason)?;
        return Err(CursorLedgerError::WitnessMismatch(
            intent.logical_reread_key,
        ));
    }
    if intent.proposed_cursor.epoch != metadata.epoch
        || intent.proposed_cursor.sequence != metadata.head_sequence + 1
    {
        let reason = format!(
            "intent cursor {:?} does not follow head {}",
            intent.proposed_cursor, metadata.head_sequence
        );
        mark_quarantined(connection, &reason)?;
        return Err(CursorLedgerError::Quarantined(reason));
    }

    let authority_stamp = capture_created_authority_stamp(
        store_root,
        &metadata.authority_stamp,
        intent.proposed_cursor.sequence,
    )?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| sqlite_error("begin intent recovery", error))?;
    let receipt = CursorReceipt {
        cursor: intent.proposed_cursor,
        logical_reread_key: intent.logical_reread_key,
        validation_witness: intent.validation_witness,
        attempt_token: intent.attempt_token,
    };
    insert_receipt(&transaction, &receipt)?;
    advance_head(&transaction, receipt.cursor, &authority_stamp)?;
    transaction
        .execute("DELETE FROM cursor_intent WHERE singleton = 1", [])
        .map_err(|error| sqlite_error("retire recovered intent", error))?;
    transaction
        .commit()
        .map_err(|error| sqlite_error("commit intent recovery", error))?;
    Ok(RecoveryResolution::Published(receipt.cursor))
}

fn recover_head_only(
    connection: &mut Connection,
    store_root: &Path,
    metadata: &Metadata,
) -> Result<RecoveryResolution, CursorLedgerError> {
    let next = metadata.head_sequence + 1;
    let journal = QualificationLocalJournal::new(store_root);
    let receipt = receipt_for_sequence(connection, &journal, next)?;
    let Some(receipt) = receipt else {
        if let Some(observed) = first_receipt_after(connection, next)? {
            let reason = format!("receipt sequence {observed} skips expected {next}");
            mark_quarantined(connection, &reason)?;
            return Err(CursorLedgerError::SequenceGap {
                expected: next,
                observed,
            });
        }
        return Ok(RecoveryResolution::NoIntent);
    };
    let authority_stamp = capture_created_authority_stamp(
        store_root,
        &metadata.authority_stamp,
        receipt.cursor.sequence,
    )?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| sqlite_error("begin orphan-head recovery", error))?;
    advance_head(&transaction, receipt.cursor, &authority_stamp)?;
    transaction
        .commit()
        .map_err(|error| sqlite_error("commit orphan-head recovery", error))?;
    Ok(RecoveryResolution::AdvancedHead(receipt.cursor))
}

fn insert_attempt(
    transaction: &Transaction<'_>,
    attempt_token: &str,
) -> Result<(), CursorLedgerError> {
    let attempt_hash = sha256_digest(attempt_token.as_bytes());
    match transaction.execute(
        "INSERT INTO cursor_attempt (attempt_hash) VALUES (?1)",
        [attempt_hash.as_slice()],
    ) {
        Ok(_) => Ok(()),
        Err(error) if error.sqlite_error_code() == Some(ErrorCode::ConstraintViolation) => Err(
            CursorLedgerError::AttemptTokenUsed(attempt_token.to_owned()),
        ),
        Err(error) => Err(sqlite_error("insert attempt token", error)),
    }
}

fn insert_intent(
    transaction: &Transaction<'_>,
    intent: &CursorIntent,
) -> Result<(), CursorLedgerError> {
    transaction
        .execute(
            "INSERT INTO cursor_intent
             (singleton, epoch, sequence, logical_reread_key, validation_witness,
              attempt_hash, attempt_token)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                u64_to_i64(intent.proposed_cursor.epoch, "intent epoch")?,
                u64_to_i64(intent.proposed_cursor.sequence, "intent sequence")?,
                intent.logical_reread_key,
                intent.validation_witness,
                sha256_digest(intent.attempt_token.as_bytes()).as_slice(),
                intent.attempt_token,
            ],
        )
        .map_err(|error| sqlite_error("insert cursor intent", error))?;
    Ok(())
}

fn insert_receipt(
    transaction: &Transaction<'_>,
    receipt: &CursorReceipt,
) -> Result<(), CursorLedgerError> {
    let witness = decode_digest(&receipt.validation_witness, "receipt validation witness")?;
    let bootstrap_token = format!(
        "bootstrap:{}:{}:{}",
        receipt.cursor.epoch, receipt.cursor.sequence, receipt.validation_witness
    );
    let retained_attempt_token =
        (receipt.attempt_token != bootstrap_token).then_some(receipt.attempt_token.as_str());
    transaction
        .execute(
            "INSERT INTO cursor_receipt
             (sequence, epoch, logical_reread_key_hash,
              validation_witness_hash, attempt_hash, attempt_token)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                u64_to_i64(receipt.cursor.sequence, "receipt sequence")?,
                u64_to_i64(receipt.cursor.epoch, "receipt epoch")?,
                sha256_digest(receipt.logical_reread_key.as_bytes()).as_slice(),
                witness.as_slice(),
                sha256_digest(receipt.attempt_token.as_bytes()).as_slice(),
                retained_attempt_token,
            ],
        )
        .map_err(|error| sqlite_error("insert cursor receipt", error))?;
    Ok(())
}

fn advance_head(
    transaction: &Transaction<'_>,
    cursor: TruthCursor,
    authority_stamp: &JournalChangeStamp,
) -> Result<(), CursorLedgerError> {
    let updated = transaction
        .execute(
            "UPDATE cursor_meta
             SET head_sequence = ?1, authority_stamp_json = ?2
             WHERE singleton = 1 AND epoch = ?3 AND head_sequence = ?4",
            params![
                u64_to_i64(cursor.sequence, "head sequence")?,
                encode_authority_stamp(authority_stamp)?,
                u64_to_i64(cursor.epoch, "head epoch")?,
                u64_to_i64(cursor.sequence - 1, "previous head")?,
            ],
        )
        .map_err(|error| sqlite_error("advance cursor head", error))?;
    if updated != 1 {
        return Err(CursorLedgerError::SequenceGap {
            expected: cursor.sequence,
            observed: read_metadata(transaction)?.head_sequence,
        });
    }
    Ok(())
}

fn retire_intent(connection: &Connection) -> Result<(), CursorLedgerError> {
    connection
        .execute("DELETE FROM cursor_intent WHERE singleton = 1", [])
        .map_err(|error| sqlite_error("retire cursor intent", error))?;
    Ok(())
}

fn read_intent(connection: &Connection) -> Result<Option<CursorIntent>, CursorLedgerError> {
    connection
        .query_row(
            "SELECT epoch, sequence, logical_reread_key, validation_witness, attempt_token
             FROM cursor_intent WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| sqlite_error("read cursor intent", error))
        .and_then(|row| {
            row.map(
                |(epoch, sequence, logical_reread_key, validation_witness, attempt_token)| {
                    Ok(CursorIntent::new(
                        TruthCursor::new(
                            i64_to_u64(epoch, "intent epoch")?,
                            i64_to_u64(sequence, "intent sequence")?,
                        ),
                        logical_reread_key,
                        validation_witness,
                        attempt_token,
                    ))
                },
            )
            .transpose()
        })
}

fn receipt_for_key(
    connection: &Connection,
    journal: &QualificationLocalJournal,
    logical_reread_key: &str,
) -> Result<Option<CursorReceipt>, CursorLedgerError> {
    let stored = connection
        .query_row(
            "SELECT epoch, sequence, lower(hex(logical_reread_key_hash)),
                    lower(hex(validation_witness_hash)),
                    coalesce(
                        attempt_token,
                        'bootstrap:' || epoch || ':' || sequence || ':'
                            || lower(hex(validation_witness_hash))
                    )
             FROM cursor_receipt WHERE logical_reread_key_hash = ?1",
            [sha256_digest(logical_reread_key.as_bytes()).as_slice()],
            stored_receipt_from_row,
        )
        .optional()
        .map_err(|error| sqlite_error("read receipt by key", error))?;
    let receipt = stored
        .as_ref()
        .map(|stored| hydrate_stored_receipt(journal, stored).map(|(receipt, _)| receipt))
        .transpose()?;
    if let Some(receipt) = &receipt
        && receipt.logical_reread_key != logical_reread_key
    {
        return Err(CursorLedgerError::SchemaMismatch(
            "cursor receipt key digest resolves to a different logical key".to_owned(),
        ));
    }
    Ok(receipt)
}

fn receipt_for_sequence(
    connection: &Connection,
    journal: &QualificationLocalJournal,
    sequence: u64,
) -> Result<Option<CursorReceipt>, CursorLedgerError> {
    let stored = connection
        .query_row(
            "SELECT epoch, sequence, logical_reread_key_hash,
                    validation_witness, attempt_token
             FROM cursor_receipt_text WHERE sequence = ?1",
            [u64_to_i64(sequence, "receipt sequence")?],
            stored_receipt_from_row,
        )
        .optional()
        .map_err(|error| sqlite_error("read receipt by sequence", error))?;
    stored
        .as_ref()
        .map(|stored| hydrate_stored_receipt(journal, stored).map(|(receipt, _)| receipt))
        .transpose()
}

fn first_receipt_after(
    connection: &Connection,
    sequence: u64,
) -> Result<Option<u64>, CursorLedgerError> {
    connection
        .query_row(
            "SELECT sequence FROM cursor_receipt WHERE sequence > ?1 ORDER BY sequence LIMIT 1",
            [u64_to_i64(sequence, "receipt lower bound")?],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| sqlite_error("read receipt gap", error))?
        .map(|value| i64_to_u64(value, "receipt gap"))
        .transpose()
}

fn stored_receipt_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredCursorReceipt> {
    let epoch = row.get::<_, i64>(0)?;
    let sequence = row.get::<_, i64>(1)?;
    if epoch <= 0 || sequence <= 0 {
        return Err(rusqlite::Error::IntegralValueOutOfRange(0, epoch));
    }
    Ok(StoredCursorReceipt {
        cursor: TruthCursor::new(epoch as u64, sequence as u64),
        logical_reread_key_hash: row.get(2)?,
        validation_witness: row.get(3)?,
        attempt_token: row.get(4)?,
    })
}

fn hydrate_stored_receipt(
    journal: &QualificationLocalJournal,
    stored: &StoredCursorReceipt,
) -> Result<(CursorReceipt, ShoreEvent), CursorLedgerError> {
    let bytes = journal
        .read_event_bytes_by_key_digest(&stored.logical_reread_key_hash)
        .map_err(|error| CursorLedgerError::Truth(error.to_string()))?
        .ok_or_else(|| CursorLedgerError::CarrierAbsent(stored.logical_reread_key_hash.clone()))?;
    if sha256_bytes_hex(&bytes) != stored.validation_witness {
        return Err(CursorLedgerError::WitnessMismatch(
            stored.logical_reread_key_hash.clone(),
        ));
    }
    let event =
        EventStore::decode_qualification_entry(stored.logical_reread_key_hash.clone(), bytes)
            .map_err(|error| CursorLedgerError::Truth(error.to_string()))?;
    Ok((
        CursorReceipt {
            cursor: stored.cursor,
            logical_reread_key: event.idempotency_key.clone(),
            validation_witness: stored.validation_witness.clone(),
            attempt_token: stored.attempt_token.clone(),
        },
        event,
    ))
}

fn classify_recovered_receipt(receipt: &CursorReceipt, witness: &str) -> RecoveryResolution {
    if receipt.validation_witness == witness {
        RecoveryResolution::Existing(receipt.cursor)
    } else {
        RecoveryResolution::Conflict(receipt.cursor)
    }
}

fn validate_named_carrier(
    journal: &QualificationLocalJournal,
    logical_reread_key: &str,
    witness: &str,
) -> Result<(), CursorLedgerError> {
    let bytes = journal
        .read_event_bytes(logical_reread_key)
        .map_err(|error| CursorLedgerError::Truth(error.to_string()))?
        .ok_or_else(|| CursorLedgerError::CarrierAbsent(logical_reread_key.to_owned()))?;
    if sha256_bytes_hex(&bytes) != witness {
        return Err(CursorLedgerError::WitnessMismatch(
            logical_reread_key.to_owned(),
        ));
    }
    Ok(())
}

fn mark_quarantined(connection: &Connection, reason: &str) -> Result<(), CursorLedgerError> {
    connection
        .execute(
            "UPDATE cursor_meta
             SET bootstrap_state = 'quarantined', quarantine_reason = ?1
             WHERE singleton = 1",
            [reason],
        )
        .map_err(|error| sqlite_error("quarantine cursor metadata", error))?;
    Ok(())
}

fn pragma_i64(connection: &Connection, name: &str) -> Result<i64, CursorLedgerError> {
    connection
        .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
        .map_err(|error| sqlite_error("read SQLite pragma", error))
}

fn u64_to_i64(value: u64, label: &'static str) -> Result<i64, CursorLedgerError> {
    i64::try_from(value).map_err(|_| {
        CursorLedgerError::SchemaMismatch(format!("{label} does not fit SQLite INTEGER"))
    })
}

fn usize_to_i64(value: usize, label: &'static str) -> Result<i64, CursorLedgerError> {
    i64::try_from(value).map_err(|_| {
        CursorLedgerError::SchemaMismatch(format!("{label} does not fit SQLite INTEGER"))
    })
}

fn i64_to_u64(value: i64, label: &'static str) -> Result<u64, CursorLedgerError> {
    u64::try_from(value)
        .map_err(|_| CursorLedgerError::SchemaMismatch(format!("{label} is negative")))
}

fn sqlite_error(operation: &'static str, error: rusqlite::Error) -> CursorLedgerError {
    CursorLedgerError::Sqlite {
        operation,
        message: error.to_string(),
    }
}

fn io_error(path: &Path, error: std::io::Error) -> CursorLedgerError {
    CursorLedgerError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

fn file_len(path: &Path) -> Result<u64, CursorLedgerError> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(io_error(path, error)),
    }
}

fn sqlite_companion_path(database_path: &Path, suffix: &str) -> PathBuf {
    let mut path = database_path.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}
