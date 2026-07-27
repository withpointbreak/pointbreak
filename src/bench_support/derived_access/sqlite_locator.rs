//! Qualification-only SQLite locator implementation.
#![cfg_attr(not(test), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Params, StatementStatus, TransactionBehavior, params,
};

#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::record_chronological_sort_items;
use crate::session::derived_access::cursor::{CursorDelta, TruthCursor};
use crate::session::derived_access::locator::{
    ChronologicalWindowRequest, LocatorCheckpoint, LocatorModelError, LocatorRead, LocatorRow,
    LocatorWindow, WindowContinuation, WindowPosition,
};

const SIDECAR_DIRECTORY: &str = ".pointbreak-derived";
const DATABASE_FILE: &str = "cursor.sqlite3";
const CURSOR_PROFILE_ID: &str = "pointbreak.sqlite-derived-access-cursor.v1";
const LOCATOR_PROFILE_ID: &str = "pointbreak.sqlite-derived-access-locator.v1";
const LOCATOR_SCHEMA_VERSION: i64 = 1;
const APPLICATION_ID: i64 = 0x5042_4443;
const CURSOR_SCHEMA_VERSION: i64 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub(crate) struct SqliteLocator {
    database_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocatorInventory {
    pub(crate) profile_id: String,
    pub(crate) schema_version: u32,
    pub(crate) row_count: u64,
    pub(crate) columns: Vec<String>,
    pub(crate) indexes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct LocatorQueryStatus {
    pub(crate) fullscan_steps: u64,
    pub(crate) sort_operations: u64,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SqliteLocatorError {
    #[error("locator sidecar is absent at {0}")]
    MissingSidecar(PathBuf),
    #[error("locator SQLite failure during {operation}: {message}")]
    Sqlite {
        operation: &'static str,
        message: String,
    },
    #[error("locator metadata mismatch: {0}")]
    Metadata(String),
    #[error("locator delta does not follow its checkpoint: {0}")]
    Delta(String),
    #[error(transparent)]
    Model(#[from] LocatorModelError),
}

#[derive(Clone, Debug)]
struct CursorMetadata {
    store_id: String,
    profile_id: String,
    epoch: u64,
    head_sequence: u64,
    state: String,
}

impl SqliteLocator {
    pub(crate) fn open(store_root: &Path) -> Result<Self, SqliteLocatorError> {
        let store_root = store_root
            .canonicalize()
            .map_err(|error| SqliteLocatorError::Metadata(error.to_string()))?;
        let locator = Self {
            database_path: store_root.join(SIDECAR_DIRECTORY).join(DATABASE_FILE),
        };
        if !locator.database_path.exists() {
            return Err(SqliteLocatorError::MissingSidecar(
                locator.database_path.clone(),
            ));
        }
        let connection = locator.connection()?;
        let cursor = validate_cursor_metadata(&connection)?;
        initialize_locator_schema(&connection, &cursor)?;
        validate_locator_checkpoint(&connection, &cursor)?;
        Ok(locator)
    }

    pub(crate) fn checkpoint(&self) -> Result<LocatorCheckpoint, SqliteLocatorError> {
        let connection = self.validated_connection()?;
        read_locator_checkpoint(&connection)
    }

    pub(super) fn apply_delta_with(
        &self,
        delta: &CursorDelta,
        rows: &[LocatorRow],
        apply_semantic: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<(), SqliteLocatorError>,
    ) -> Result<LocatorCheckpoint, SqliteLocatorError> {
        if rows.len() != delta.receipts.len() {
            return Err(SqliteLocatorError::Delta(format!(
                "{} locator rows for {} cursor receipts",
                rows.len(),
                delta.receipts.len()
            )));
        }
        let mut connection = self.validated_connection()?;
        let checkpoint = read_locator_checkpoint(&connection)?;
        if checkpoint.applied != delta.after {
            return Err(SqliteLocatorError::Delta(format!(
                "delta starts at {:?}, checkpoint is {:?}",
                delta.after, checkpoint.applied
            )));
        }
        if delta.observed_head.epoch != checkpoint.applied.epoch {
            return Err(SqliteLocatorError::Delta(format!(
                "observed epoch {} does not match checkpoint epoch {}",
                delta.observed_head.epoch, checkpoint.applied.epoch
            )));
        }

        for (receipt, row) in delta.receipts.iter().zip(rows) {
            if row.cursor != receipt.cursor
                || row.logical_reread_key != receipt.logical_reread_key
                || row.validation_witness != receipt.validation_witness
            {
                return Err(SqliteLocatorError::Delta(format!(
                    "locator row does not match cursor receipt at {:?}",
                    receipt.cursor
                )));
            }
        }
        let applied = delta
            .receipts
            .last()
            .map_or(delta.after, |receipt| receipt.cursor);
        if delta.complete && applied != delta.observed_head {
            return Err(SqliteLocatorError::Delta(format!(
                "complete delta ended at {applied:?}, observed {:?}",
                delta.observed_head
            )));
        }

        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| sqlite_error("begin locator delta", error))?;
        for row in rows {
            insert_locator_row(&transaction, row)?;
        }
        apply_semantic(&transaction)?;
        let updated = transaction
            .execute(
                "UPDATE locator_checkpoint
                 SET applied_sequence = ?1, observed_sequence = ?2
                 WHERE singleton = 1 AND epoch = ?3 AND applied_sequence = ?4",
                params![
                    to_i64(applied.sequence, "applied sequence")?,
                    to_i64(delta.observed_head.sequence, "observed sequence")?,
                    to_i64(applied.epoch, "checkpoint epoch")?,
                    to_i64(checkpoint.applied.sequence, "previous applied sequence")?,
                ],
            )
            .map_err(|error| sqlite_error("advance locator checkpoint", error))?;
        if updated != 1 {
            return Err(SqliteLocatorError::Delta(
                "locator checkpoint changed concurrently".to_owned(),
            ));
        }
        transaction
            .commit()
            .map_err(|error| sqlite_error("commit locator delta", error))?;
        Ok(LocatorCheckpoint {
            applied,
            observed: delta.observed_head,
        })
    }

    pub(crate) fn lookup_event_id(
        &self,
        event_id: &str,
        observed: TruthCursor,
    ) -> Result<LocatorRead<Option<LocatorRow>>, SqliteLocatorError> {
        let connection = self.validated_connection()?;
        let checkpoint = read_locator_checkpoint(&connection)?;
        if checkpoint.applied.epoch != observed.epoch
            || checkpoint.applied.sequence < observed.sequence
        {
            return Ok(LocatorRead::CatchUpRequired {
                applied: checkpoint.applied,
                observed,
            });
        }
        let row = connection
            .query_row(
                "SELECT epoch, sequence, logical_reread_key, event_id,
                        normalized_occurred_at, replay_key, event_type, journal_id,
                        subject_id, track_id, payload_hash, validation_witness
                 FROM locator_event
                 WHERE event_id = ?1 AND epoch = ?2 AND sequence <= ?3",
                params![
                    event_id,
                    to_i64(observed.epoch, "lookup epoch")?,
                    to_i64(observed.sequence, "lookup as_of")?,
                ],
                locator_row_from_sql,
            )
            .optional()
            .map_err(|error| sqlite_error("lookup semantic event id", error))?;
        if let Some(row) = &row {
            validate_locator_receipt(&connection, row)?;
        }
        Ok(LocatorRead::Ready(row))
    }

    pub(crate) fn chronological_window(
        &self,
        request: &ChronologicalWindowRequest,
        observed: TruthCursor,
    ) -> Result<LocatorRead<LocatorWindow>, SqliteLocatorError> {
        self.chronological_window_inner(request, observed)
            .map(|(window, _)| window)
    }

    #[cfg(test)]
    pub(crate) fn chronological_window_with_status(
        &self,
        request: &ChronologicalWindowRequest,
        observed: TruthCursor,
    ) -> Result<(LocatorRead<LocatorWindow>, LocatorQueryStatus), SqliteLocatorError> {
        self.chronological_window_inner(request, observed)
    }

    fn chronological_window_inner(
        &self,
        request: &ChronologicalWindowRequest,
        observed: TruthCursor,
    ) -> Result<(LocatorRead<LocatorWindow>, LocatorQueryStatus), SqliteLocatorError> {
        if request.limit() == 0 {
            return Err(LocatorModelError::ZeroWindowLimit.into());
        }
        let connection = self.validated_connection()?;
        let checkpoint = read_locator_checkpoint(&connection)?;
        let as_of = match request.requested_as_of() {
            Some(requested) => {
                if requested.epoch != observed.epoch {
                    return Err(LocatorModelError::AsOfEpochMismatch {
                        requested,
                        observed,
                    }
                    .into());
                }
                if requested.sequence > observed.sequence {
                    return Err(LocatorModelError::AsOfAhead {
                        requested,
                        observed,
                    }
                    .into());
                }
                if requested.sequence > checkpoint.applied.sequence
                    || requested.epoch != checkpoint.applied.epoch
                {
                    return Ok((
                        LocatorRead::CatchUpRequired {
                            applied: checkpoint.applied,
                            observed,
                        },
                        LocatorQueryStatus::default(),
                    ));
                }
                requested
            }
            None if checkpoint.applied.epoch == observed.epoch
                && checkpoint.applied.sequence >= observed.sequence =>
            {
                observed
            }
            None => {
                return Ok((
                    LocatorRead::CatchUpRequired {
                        applied: checkpoint.applied,
                        observed,
                    },
                    LocatorQueryStatus::default(),
                ));
            }
        };

        let limit = request.limit().saturating_add(1);
        let limit = i64::try_from(limit)
            .map_err(|_| SqliteLocatorError::Delta("window limit overflow".to_owned()))?;
        let epoch = to_i64(as_of.epoch, "window epoch")?;
        let sequence = to_i64(as_of.sequence, "window as_of")?;
        let (query, descending) = match request.position() {
            WindowPosition::Head => (
                query_locator_rows(
                    &connection,
                    "SELECT epoch, sequence, logical_reread_key, event_id,
                            normalized_occurred_at, replay_key, event_type, journal_id,
                            subject_id, track_id, payload_hash, validation_witness
                     FROM locator_event INDEXED BY locator_event_display
                     WHERE epoch = ?1 AND sequence <= ?2
                     ORDER BY normalized_occurred_at, event_id
                     LIMIT ?3",
                    params![epoch, sequence, limit],
                )?,
                false,
            ),
            WindowPosition::Continue(WindowContinuation::After { anchor, .. }) => (
                query_locator_rows(
                    &connection,
                    "SELECT epoch, sequence, logical_reread_key, event_id,
                            normalized_occurred_at, replay_key, event_type, journal_id,
                            subject_id, track_id, payload_hash, validation_witness
                     FROM locator_event INDEXED BY locator_event_display
                     WHERE epoch = ?1 AND sequence <= ?2
                       AND (normalized_occurred_at, event_id) > (?3, ?4)
                     ORDER BY normalized_occurred_at, event_id
                     LIMIT ?5",
                    params![
                        epoch,
                        sequence,
                        anchor.normalized_occurred_at,
                        anchor.event_id,
                        limit
                    ],
                )?,
                false,
            ),
            WindowPosition::Tail => (
                query_locator_rows(
                    &connection,
                    "SELECT epoch, sequence, logical_reread_key, event_id,
                            normalized_occurred_at, replay_key, event_type, journal_id,
                            subject_id, track_id, payload_hash, validation_witness
                     FROM locator_event INDEXED BY locator_event_display
                     WHERE epoch = ?1 AND sequence <= ?2
                     ORDER BY normalized_occurred_at DESC, event_id DESC
                     LIMIT ?3",
                    params![epoch, sequence, limit],
                )?,
                true,
            ),
            WindowPosition::Continue(WindowContinuation::Before { anchor, .. }) => (
                query_locator_rows(
                    &connection,
                    "SELECT epoch, sequence, logical_reread_key, event_id,
                            normalized_occurred_at, replay_key, event_type, journal_id,
                            subject_id, track_id, payload_hash, validation_witness
                     FROM locator_event INDEXED BY locator_event_display
                     WHERE epoch = ?1 AND sequence <= ?2
                       AND (normalized_occurred_at, event_id) < (?3, ?4)
                     ORDER BY normalized_occurred_at DESC, event_id DESC
                     LIMIT ?5",
                    params![
                        epoch,
                        sequence,
                        anchor.normalized_occurred_at,
                        anchor.event_id,
                        limit
                    ],
                )?,
                true,
            ),
        };
        let (mut rows, status) = query;
        record_query_sort_work(status, rows.len());
        let has_more = rows.len() > request.limit();
        rows.truncate(request.limit());
        if descending {
            rows.reverse();
        }
        let continuation = has_more.then(|| {
            let anchor = if descending {
                rows.first()
            } else {
                rows.last()
            }
            .expect("has_more implies at least one selected row")
            .display_key();
            if descending {
                WindowContinuation::Before { anchor, as_of }
            } else {
                WindowContinuation::After { anchor, as_of }
            }
        });
        Ok((
            LocatorRead::Ready(LocatorWindow {
                as_of,
                rows,
                continuation,
                has_more,
            }),
            status,
        ))
    }

    pub(crate) fn inventory(&self) -> Result<LocatorInventory, SqliteLocatorError> {
        let connection = self.validated_connection()?;
        let (profile_id, schema_version) = connection
            .query_row(
                "SELECT profile_id, schema_version
                 FROM locator_checkpoint WHERE singleton = 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(|error| sqlite_error("read locator inventory identity", error))?;
        let row_count = connection
            .query_row("SELECT count(*) FROM locator_event", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|error| sqlite_error("count locator rows", error))?;
        let columns = query_names(&connection, "PRAGMA table_info(locator_event)", 1)?;
        let indexes = query_names(&connection, "PRAGMA index_list(locator_event)", 1)?;
        Ok(LocatorInventory {
            profile_id,
            schema_version: u32::try_from(schema_version).map_err(|_| {
                SqliteLocatorError::Metadata("negative locator schema version".to_owned())
            })?,
            row_count: to_u64(row_count, "locator row count")?,
            columns,
            indexes,
        })
    }

    fn connection(&self) -> Result<Connection, SqliteLocatorError> {
        let connection = Connection::open_with_flags(
            &self.database_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| sqlite_error("open locator sidecar", error))?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(|error| sqlite_error("set locator busy timeout", error))?;
        let mode = connection
            .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))
            .map_err(|error| sqlite_error("enable locator WAL", error))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(SqliteLocatorError::Metadata(format!(
                "SQLite refused WAL mode and returned {mode}"
            )));
        }
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(|error| sqlite_error("set locator synchronous", error))?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(|error| sqlite_error("enable locator foreign keys", error))?;
        connection
            .pragma_update(None, "cell_size_check", true)
            .map_err(|error| sqlite_error("enable locator cell-size checks", error))?;
        #[cfg(target_os = "macos")]
        connection
            .pragma_update(None, "fullfsync", true)
            .map_err(|error| sqlite_error("enable locator fullfsync", error))?;
        Ok(connection)
    }

    pub(super) fn validated_connection(&self) -> Result<Connection, SqliteLocatorError> {
        let connection = self.connection()?;
        let cursor = validate_cursor_metadata(&connection)?;
        validate_locator_checkpoint(&connection, &cursor)?;
        Ok(connection)
    }
}

fn initialize_locator_schema(
    connection: &Connection,
    cursor: &CursorMetadata,
) -> Result<(), SqliteLocatorError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS locator_checkpoint (
                 singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                 store_id TEXT NOT NULL,
                 profile_id TEXT NOT NULL,
                 schema_version INTEGER NOT NULL CHECK (schema_version = 1),
                 epoch INTEGER NOT NULL CHECK (epoch > 0),
                 applied_sequence INTEGER NOT NULL CHECK (applied_sequence >= 0),
                 observed_sequence INTEGER NOT NULL
                     CHECK (observed_sequence >= applied_sequence)
             ) STRICT;
             CREATE TABLE IF NOT EXISTS locator_event (
                 sequence INTEGER PRIMARY KEY CHECK (sequence > 0)
                     REFERENCES cursor_receipt(sequence),
                 epoch INTEGER NOT NULL CHECK (epoch > 0),
                 logical_reread_key TEXT NOT NULL UNIQUE,
                 event_id TEXT NOT NULL UNIQUE,
                 normalized_occurred_at TEXT NOT NULL,
                 replay_key TEXT NOT NULL UNIQUE CHECK (length(replay_key) = 64),
                 event_type TEXT NOT NULL,
                 journal_id TEXT NOT NULL,
                 subject_id TEXT,
                 track_id TEXT,
                 payload_hash TEXT NOT NULL,
                 validation_witness TEXT NOT NULL CHECK (length(validation_witness) = 64)
             ) STRICT;
             CREATE INDEX IF NOT EXISTS locator_event_display
                 ON locator_event(epoch, normalized_occurred_at, event_id, sequence);
             CREATE INDEX IF NOT EXISTS locator_event_cursor
                 ON locator_event(epoch, sequence);
             CREATE INDEX IF NOT EXISTS locator_event_target
                 ON locator_event(subject_id, event_type, track_id);",
        )
        .map_err(|error| sqlite_error("create locator schema", error))?;
    connection
        .execute(
            "INSERT INTO locator_checkpoint
             (singleton, store_id, profile_id, schema_version, epoch,
              applied_sequence, observed_sequence)
             VALUES (1, ?1, ?2, ?3, ?4, 0, ?5)
             ON CONFLICT(singleton) DO NOTHING",
            params![
                cursor.store_id,
                LOCATOR_PROFILE_ID,
                LOCATOR_SCHEMA_VERSION,
                to_i64(cursor.epoch, "locator epoch")?,
                to_i64(cursor.head_sequence, "initial observed sequence")?,
            ],
        )
        .map_err(|error| sqlite_error("initialize locator checkpoint", error))?;
    Ok(())
}

fn validate_cursor_metadata(connection: &Connection) -> Result<CursorMetadata, SqliteLocatorError> {
    let application_id = connection
        .pragma_query_value(None, "application_id", |row| row.get::<_, i64>(0))
        .map_err(|error| sqlite_error("read application id", error))?;
    let user_version = connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(|error| sqlite_error("read user version", error))?;
    if application_id != APPLICATION_ID || user_version != CURSOR_SCHEMA_VERSION {
        return Err(SqliteLocatorError::Metadata(format!(
            "application_id={application_id}, user_version={user_version}"
        )));
    }
    let (store_id, profile_id, epoch, head_sequence, state) = connection
        .query_row(
            "SELECT store_id, profile_id, epoch, head_sequence, bootstrap_state
             FROM cursor_meta WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .map_err(|error| sqlite_error("read cursor identity for locator", error))?;
    let metadata = CursorMetadata {
        store_id,
        profile_id,
        epoch: to_u64(epoch, "cursor epoch")?,
        head_sequence: to_u64(head_sequence, "cursor head")?,
        state,
    };
    if metadata.profile_id != CURSOR_PROFILE_ID || metadata.state != "complete" {
        return Err(SqliteLocatorError::Metadata(format!(
            "cursor profile/state is {}/{}",
            metadata.profile_id, metadata.state
        )));
    }
    Ok(metadata)
}

fn validate_locator_checkpoint(
    connection: &Connection,
    cursor: &CursorMetadata,
) -> Result<(), SqliteLocatorError> {
    let (store_id, profile_id, schema_version, epoch, applied, observed) = connection
        .query_row(
            "SELECT store_id, profile_id, schema_version, epoch,
                    applied_sequence, observed_sequence
             FROM locator_checkpoint WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .map_err(|error| sqlite_error("validate locator checkpoint", error))?;
    if store_id != cursor.store_id
        || profile_id != LOCATOR_PROFILE_ID
        || schema_version != LOCATOR_SCHEMA_VERSION
        || to_u64(epoch, "locator epoch")? != cursor.epoch
    {
        return Err(SqliteLocatorError::Metadata(format!(
            "locator identity {store_id}/{profile_id}/{schema_version}/{epoch} \
             does not match cursor {}/{}/{}/{:?}",
            cursor.store_id, LOCATOR_PROFILE_ID, LOCATOR_SCHEMA_VERSION, cursor.epoch
        )));
    }
    let applied = to_u64(applied, "locator applied")?;
    let observed = to_u64(observed, "locator observed")?;
    if applied > observed || applied > cursor.head_sequence || observed > cursor.head_sequence {
        return Err(SqliteLocatorError::Metadata(format!(
            "locator applied/observed {applied}/{observed} is invalid for cursor head {}",
            cursor.head_sequence
        )));
    }
    Ok(())
}

pub(super) fn read_locator_checkpoint(
    connection: &Connection,
) -> Result<LocatorCheckpoint, SqliteLocatorError> {
    connection
        .query_row(
            "SELECT epoch, applied_sequence, observed_sequence
             FROM locator_checkpoint WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .map_err(|error| sqlite_error("read locator checkpoint", error))
        .and_then(|(epoch, applied, observed)| {
            let epoch = to_u64(epoch, "checkpoint epoch")?;
            Ok(LocatorCheckpoint {
                applied: TruthCursor::new(epoch, to_u64(applied, "checkpoint applied")?),
                observed: TruthCursor::new(epoch, to_u64(observed, "checkpoint observed")?),
            })
        })
}

fn insert_locator_row(
    transaction: &rusqlite::Transaction<'_>,
    row: &LocatorRow,
) -> Result<(), SqliteLocatorError> {
    transaction
        .execute(
            "INSERT INTO locator_event
             (sequence, epoch, logical_reread_key, event_id, normalized_occurred_at,
              replay_key, event_type, journal_id, subject_id, track_id, payload_hash,
              validation_witness)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                to_i64(row.cursor.sequence, "locator sequence")?,
                to_i64(row.cursor.epoch, "locator epoch")?,
                row.logical_reread_key,
                row.event_id,
                row.normalized_occurred_at,
                row.replay_key,
                row.event_type,
                row.journal_id,
                row.subject_id,
                row.track_id,
                row.payload_hash,
                row.validation_witness,
            ],
        )
        .map_err(|error| sqlite_error("insert locator row", error))?;
    Ok(())
}

fn query_locator_rows(
    connection: &Connection,
    sql: &str,
    parameters: impl Params,
) -> Result<(Vec<LocatorRow>, LocatorQueryStatus), SqliteLocatorError> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| sqlite_error("prepare locator range", error))?;
    let rows = statement
        .query_map(parameters, locator_row_from_sql)
        .map_err(|error| sqlite_error("query locator range", error))?;
    let mut output = Vec::new();
    for row in rows {
        output.push(row.map_err(|error| sqlite_error("read locator range", error))?);
    }
    let status = LocatorQueryStatus {
        fullscan_steps: status_value(statement.get_status(StatementStatus::FullscanStep)),
        sort_operations: status_value(statement.get_status(StatementStatus::Sort)),
    };
    for row in &output {
        validate_locator_receipt(connection, row)?;
    }
    Ok((output, status))
}

fn locator_row_from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocatorRow> {
    let epoch = row.get::<_, i64>(0)?;
    let sequence = row.get::<_, i64>(1)?;
    if epoch <= 0 || sequence <= 0 {
        return Err(rusqlite::Error::IntegralValueOutOfRange(0, epoch));
    }
    Ok(LocatorRow {
        cursor: TruthCursor::new(epoch as u64, sequence as u64),
        logical_reread_key: row.get(2)?,
        event_id: row.get(3)?,
        normalized_occurred_at: row.get(4)?,
        replay_key: row.get(5)?,
        event_type: row.get(6)?,
        journal_id: row.get(7)?,
        subject_id: row.get(8)?,
        track_id: row.get(9)?,
        payload_hash: row.get(10)?,
        validation_witness: row.get(11)?,
    })
}

fn validate_locator_receipt(
    connection: &Connection,
    locator: &LocatorRow,
) -> Result<(), SqliteLocatorError> {
    let receipt = connection
        .query_row(
            "SELECT epoch, logical_reread_key, validation_witness
             FROM cursor_receipt WHERE sequence = ?1",
            [to_i64(locator.cursor.sequence, "locator receipt sequence")?],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(|error| sqlite_error("validate selected locator receipt", error))?;
    if receipt.0 != to_i64(locator.cursor.epoch, "locator receipt epoch")?
        || receipt.1 != locator.logical_reread_key
        || receipt.2 != locator.validation_witness
    {
        return Err(SqliteLocatorError::Metadata(format!(
            "locator row does not match cursor receipt at {:?}",
            locator.cursor
        )));
    }
    Ok(())
}

fn status_value(value: i32) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn record_query_sort_work(status: LocatorQueryStatus, selected_rows: usize) {
    #[cfg(any(test, feature = "longitudinal-counting"))]
    if status.sort_operations > 0 {
        record_chronological_sort_items(selected_rows.max(1));
    }
    #[cfg(not(any(test, feature = "longitudinal-counting")))]
    let _ = (status, selected_rows);
}

fn query_names(
    connection: &Connection,
    sql: &str,
    column: usize,
) -> Result<Vec<String>, SqliteLocatorError> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| sqlite_error("prepare locator names", error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(column))
        .map_err(|error| sqlite_error("query locator names", error))?;
    let mut names = Vec::new();
    for row in rows {
        names.push(row.map_err(|error| sqlite_error("read locator name", error))?);
    }
    names.sort();
    Ok(names)
}

fn sqlite_error(operation: &'static str, error: rusqlite::Error) -> SqliteLocatorError {
    SqliteLocatorError::Sqlite {
        operation,
        message: error.to_string(),
    }
}

fn to_i64(value: u64, label: &'static str) -> Result<i64, SqliteLocatorError> {
    i64::try_from(value)
        .map_err(|_| SqliteLocatorError::Metadata(format!("{label} does not fit SQLite INTEGER")))
}

fn to_u64(value: i64, label: &'static str) -> Result<u64, SqliteLocatorError> {
    u64::try_from(value).map_err(|_| SqliteLocatorError::Metadata(format!("{label} is negative")))
}
