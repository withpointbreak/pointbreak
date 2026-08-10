//! SQLite locator implementation shared by the dormant product profile and qualification.
#![cfg_attr(not(test), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Params, StatementStatus, TransactionBehavior, params,
};

use super::{immutable_read_only_open_is_safe, sqlite_immutable_read_only_uri};
#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::record_chronological_sort_items;
use crate::canonical_hash::sha256_bytes_hex;
use crate::session::EventStore;
use crate::session::derived_access::QualificationLocalJournal;
use crate::session::derived_access::cursor::{CursorDelta, TruthCursor};
use crate::session::derived_access::layout::DerivedStorageLayout;
use crate::session::derived_access::locator::{
    ChronologicalWindowRequest, LocatorCheckpoint, LocatorModelError, LocatorRead, LocatorRow,
    LocatorWindow, WindowContinuation, WindowPosition,
};
use crate::session::event::ShoreEvent;

const DATABASE_FILE: &str = "cursor.sqlite3";
const CURSOR_PROFILE_ID: &str = "pointbreak.sqlite-derived-access-cursor.v1";
const LOCATOR_PROFILE_ID: &str = "pointbreak.sqlite-derived-access-locator.v1";
const LOCATOR_SCHEMA_VERSION: i64 = 3;
const APPLICATION_ID: i64 = 0x5042_4443;
const CURSOR_SCHEMA_VERSION: i64 = 4;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub(crate) struct SqliteLocator {
    store_root: PathBuf,
    database_path: PathBuf,
    access: LocatorAccess,
    published_immutable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocatorAccess {
    ReadWrite,
    PublishedReadOnly,
}

#[derive(Debug)]
pub(crate) struct HydratedLocatorRow {
    pub(crate) row: LocatorRow,
    pub(crate) event: ShoreEvent,
}

#[derive(Debug)]
pub(crate) struct HydratedLocatorWindow {
    pub(crate) window: LocatorWindow,
    pub(crate) events: Vec<ShoreEvent>,
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
    #[error("locator schema requires rebuild: {0}")]
    UpgradeRequired(String),
    #[error("locator delta does not follow its checkpoint: {0}")]
    Delta(String),
    #[error(transparent)]
    Model(#[from] LocatorModelError),
    #[error("locator carrier does not match persisted row at {0:?}")]
    CarrierMismatch(TruthCursor),
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
        let sidecar_root = DerivedStorageLayout::resolve(&store_root)
            .map_err(|error| SqliteLocatorError::Metadata(error.to_string()))?
            .root();
        Self::open_at(&store_root, &sidecar_root)
    }

    pub(crate) fn open_at(
        store_root: &Path,
        sidecar_root: &Path,
    ) -> Result<Self, SqliteLocatorError> {
        Self::open_at_with_access(store_root, sidecar_root, LocatorAccess::ReadWrite)
    }

    pub(crate) fn open_at_read_only(
        store_root: &Path,
        sidecar_root: &Path,
    ) -> Result<Self, SqliteLocatorError> {
        Self::open_at_with_access(store_root, sidecar_root, LocatorAccess::PublishedReadOnly)
    }

    fn open_at_with_access(
        store_root: &Path,
        sidecar_root: &Path,
        access: LocatorAccess,
    ) -> Result<Self, SqliteLocatorError> {
        let store_root = store_root
            .canonicalize()
            .map_err(|error| SqliteLocatorError::Metadata(error.to_string()))?;
        let database_path = sidecar_root.join(DATABASE_FILE);
        let published_immutable = access == LocatorAccess::PublishedReadOnly
            && immutable_read_only_open_is_safe(&database_path);
        let locator = Self {
            store_root: store_root.clone(),
            database_path,
            access,
            published_immutable,
        };
        if !locator.database_path.exists() {
            return Err(SqliteLocatorError::MissingSidecar(
                locator.database_path.clone(),
            ));
        }
        let connection = locator.connection()?;
        let cursor = validate_cursor_metadata(&connection)?;
        if access == LocatorAccess::ReadWrite {
            initialize_locator_schema(&connection, &cursor)?;
        }
        validate_locator_checkpoint(&connection, &cursor)?;
        Ok(locator)
    }

    pub(crate) fn checkpoint(&self) -> Result<LocatorCheckpoint, SqliteLocatorError> {
        let connection = self.validated_connection()?;
        read_locator_checkpoint(&connection)
    }

    pub(crate) fn apply_delta_with(
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
        Ok(match self.lookup_event_id_hydrated(event_id, observed)? {
            LocatorRead::Ready(row) => LocatorRead::Ready(row.map(|row| row.row)),
            LocatorRead::CatchUpRequired { applied, observed } => {
                LocatorRead::CatchUpRequired { applied, observed }
            }
        })
    }

    pub(crate) fn lookup_event_id_hydrated(
        &self,
        event_id: &str,
        observed: TruthCursor,
    ) -> Result<LocatorRead<Option<HydratedLocatorRow>>, SqliteLocatorError> {
        let event_ids = [event_id.to_owned()];
        Ok(
            match self.lookup_event_ids_hydrated(&event_ids, observed)? {
                LocatorRead::Ready(mut rows) => LocatorRead::Ready(
                    rows.pop()
                        .expect("one requested event id yields one optional row"),
                ),
                LocatorRead::CatchUpRequired { applied, observed } => {
                    LocatorRead::CatchUpRequired { applied, observed }
                }
            },
        )
    }

    /// Hydrate a caller-ordered event-id set at one frozen truth cursor.
    ///
    /// Opening and validating the SQLite connection is request-level work, not
    /// event-level work. Complete product collections can select tens of
    /// thousands of carriers; reopening the same immutable generation for each
    /// one turns an output-proportional read into connection-setup amplification.
    /// This batch holds one validated connection and one prepared point query,
    /// while every carrier still passes the same witness, decode, and typed-row
    /// validation as the scalar path.
    pub(crate) fn lookup_event_ids_hydrated(
        &self,
        event_ids: &[String],
        observed: TruthCursor,
    ) -> Result<LocatorRead<Vec<Option<HydratedLocatorRow>>>, SqliteLocatorError> {
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
        let sql = locator_select(
            "WHERE locator.event_hash = ?1
               AND locator.epoch = ?2
               AND locator.sequence <= ?3",
        );
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| sqlite_error("prepare semantic event batch", error))?;
        let journal = QualificationLocalJournal::new(&self.store_root);
        let epoch = to_i64(observed.epoch, "lookup epoch")?;
        let sequence = to_i64(observed.sequence, "lookup as_of")?;
        let mut hydrated = Vec::with_capacity(event_ids.len());
        for event_id in event_ids {
            let Some(event_hash) = decode_prefixed_digest(event_id, "evt:sha256:") else {
                hydrated.push(None);
                continue;
            };
            let stored = statement
                .query_row(
                    params![event_hash.as_slice(), epoch, sequence],
                    stored_locator_row_from_sql,
                )
                .optional()
                .map_err(|error| sqlite_error("lookup semantic event batch member", error))?;
            hydrated.push(
                stored
                    .map(|row| hydrate_locator_row(&journal, row))
                    .transpose()?,
            );
        }
        Ok(LocatorRead::Ready(hydrated))
    }

    pub(crate) fn chronological_window(
        &self,
        request: &ChronologicalWindowRequest,
        observed: TruthCursor,
    ) -> Result<LocatorRead<LocatorWindow>, SqliteLocatorError> {
        self.chronological_window_inner(request, observed)
            .map(|(selection, _)| match selection {
                LocatorRead::Ready(window) => LocatorRead::Ready(window.window),
                LocatorRead::CatchUpRequired { applied, observed } => {
                    LocatorRead::CatchUpRequired { applied, observed }
                }
            })
    }

    pub(crate) fn chronological_window_hydrated(
        &self,
        request: &ChronologicalWindowRequest,
        observed: TruthCursor,
    ) -> Result<LocatorRead<HydratedLocatorWindow>, SqliteLocatorError> {
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
            .map(|(selection, status)| {
                (
                    match selection {
                        LocatorRead::Ready(window) => LocatorRead::Ready(window.window),
                        LocatorRead::CatchUpRequired { applied, observed } => {
                            LocatorRead::CatchUpRequired { applied, observed }
                        }
                    },
                    status,
                )
            })
    }

    fn chronological_window_inner(
        &self,
        request: &ChronologicalWindowRequest,
        observed: TruthCursor,
    ) -> Result<(LocatorRead<HydratedLocatorWindow>, LocatorQueryStatus), SqliteLocatorError> {
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
                    &locator_select_indexed(
                        "WHERE locator.epoch = ?1 AND locator.sequence <= ?2
                         ORDER BY locator.normalized_occurred_at, locator.event_hash
                         LIMIT ?3",
                    ),
                    params![epoch, sequence, limit],
                )?,
                false,
            ),
            WindowPosition::Continue(WindowContinuation::After { anchor, .. }) => (
                query_locator_rows(
                    &connection,
                    &locator_select_indexed(
                        "WHERE locator.epoch = ?1 AND locator.sequence <= ?2
                           AND (locator.normalized_occurred_at, locator.event_hash) > (?3, ?4)
                         ORDER BY locator.normalized_occurred_at, locator.event_hash
                         LIMIT ?5",
                    ),
                    params![
                        epoch,
                        sequence,
                        anchor.normalized_occurred_at,
                        required_prefixed_digest(
                            &anchor.event_id,
                            "evt:sha256:",
                            "continuation event id",
                        )?
                        .as_slice(),
                        limit
                    ],
                )?,
                false,
            ),
            WindowPosition::Tail => (
                query_locator_rows(
                    &connection,
                    &locator_select_indexed(
                        "WHERE locator.epoch = ?1 AND locator.sequence <= ?2
                         ORDER BY locator.normalized_occurred_at DESC, locator.event_hash DESC
                         LIMIT ?3",
                    ),
                    params![epoch, sequence, limit],
                )?,
                true,
            ),
            WindowPosition::Continue(WindowContinuation::Before { anchor, .. }) => (
                query_locator_rows(
                    &connection,
                    &locator_select_indexed(
                        "WHERE locator.epoch = ?1 AND locator.sequence <= ?2
                           AND (locator.normalized_occurred_at, locator.event_hash) < (?3, ?4)
                         ORDER BY locator.normalized_occurred_at DESC, locator.event_hash DESC
                         LIMIT ?5",
                    ),
                    params![
                        epoch,
                        sequence,
                        anchor.normalized_occurred_at,
                        required_prefixed_digest(
                            &anchor.event_id,
                            "evt:sha256:",
                            "continuation event id",
                        )?
                        .as_slice(),
                        limit
                    ],
                )?,
                true,
            ),
        };
        let (mut stored_rows, status) = query;
        record_query_sort_work(status, stored_rows.len());
        let has_more = stored_rows.len() > request.limit();
        stored_rows.truncate(request.limit());
        if descending {
            stored_rows.reverse();
        }
        let journal = QualificationLocalJournal::new(&self.store_root);
        let hydrated_rows = stored_rows
            .into_iter()
            .map(|row| hydrate_locator_row(&journal, row))
            .collect::<Result<Vec<_>, _>>()?;
        let continuation = has_more.then(|| {
            let anchor = if descending {
                hydrated_rows.first()
            } else {
                hydrated_rows.last()
            }
            .expect("has_more implies at least one selected row")
            .row
            .display_key();
            if descending {
                WindowContinuation::Before { anchor, as_of }
            } else {
                WindowContinuation::After { anchor, as_of }
            }
        });
        let (rows, events): (Vec<_>, Vec<_>) = hydrated_rows
            .into_iter()
            .map(|row| (row.row, row.event))
            .unzip();
        Ok((
            LocatorRead::Ready(HydratedLocatorWindow {
                window: LocatorWindow {
                    as_of,
                    rows,
                    continuation,
                    has_more,
                },
                events,
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
        let connection = match self.access {
            LocatorAccess::ReadWrite => Connection::open_with_flags(
                &self.database_path,
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ),
            LocatorAccess::PublishedReadOnly if self.published_immutable => {
                Connection::open_with_flags(
                    sqlite_immutable_read_only_uri(&self.database_path),
                    OpenFlags::SQLITE_OPEN_READ_ONLY
                        | OpenFlags::SQLITE_OPEN_NO_MUTEX
                        | OpenFlags::SQLITE_OPEN_URI,
                )
            }
            LocatorAccess::PublishedReadOnly => Connection::open_with_flags(
                &self.database_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ),
        }
        .map_err(|error| sqlite_error("open locator sidecar", error))?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(|error| sqlite_error("set locator busy timeout", error))?;
        if self.access == LocatorAccess::ReadWrite {
            let mode = connection
                .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))
                .map_err(|error| sqlite_error("enable locator WAL", error))?;
            if !mode.eq_ignore_ascii_case("wal") {
                return Err(SqliteLocatorError::Metadata(format!(
                    "SQLite refused WAL mode and returned {mode}"
                )));
            }
            connection
                // Locator and semantic rows are an atomic, rebuildable projection.
                // WAL NORMAL may roll back the latest transaction after power loss;
                // the separately durable cursor head then forces bounded catch-up.
                .pragma_update(None, "synchronous", "NORMAL")
                .map_err(|error| sqlite_error("set locator synchronous", error))?;
            #[cfg(target_os = "macos")]
            connection
                .pragma_update(None, "fullfsync", true)
                .map_err(|error| sqlite_error("enable locator fullfsync", error))?;
        }
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(|error| sqlite_error("enable locator foreign keys", error))?;
        connection
            .pragma_update(None, "cell_size_check", true)
            .map_err(|error| sqlite_error("enable locator cell-size checks", error))?;
        Ok(connection)
    }

    pub(crate) fn validated_connection(&self) -> Result<Connection, SqliteLocatorError> {
        let connection = self.connection()?;
        let cursor = validate_cursor_metadata(&connection)?;
        validate_locator_checkpoint(&connection, &cursor)?;
        Ok(connection)
    }

    #[cfg(test)]
    pub(crate) fn connection_policy_for_test(&self) -> Result<(String, i64), SqliteLocatorError> {
        let connection = self.connection()?;
        let journal_mode = connection
            .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
            .map_err(|error| sqlite_error("read locator journal mode", error))?;
        let synchronous = connection
            .pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0))
            .map_err(|error| sqlite_error("read locator synchronous", error))?;
        Ok((journal_mode, synchronous))
    }

    pub(crate) fn store_root(&self) -> &Path {
        &self.store_root
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
                 schema_version INTEGER NOT NULL CHECK (schema_version = 3),
                 epoch INTEGER NOT NULL CHECK (epoch > 0),
                 applied_sequence INTEGER NOT NULL CHECK (applied_sequence >= 0),
                 observed_sequence INTEGER NOT NULL
                     CHECK (observed_sequence >= applied_sequence)
             ) STRICT;
             CREATE TABLE IF NOT EXISTS locator_event (
                 sequence INTEGER PRIMARY KEY CHECK (sequence > 0)
                     REFERENCES cursor_receipt(sequence),
                 epoch INTEGER NOT NULL CHECK (epoch > 0),
                 event_hash BLOB NOT NULL UNIQUE CHECK (length(event_hash) = 32),
                 normalized_occurred_at TEXT NOT NULL,
                 event_type_id INTEGER NOT NULL REFERENCES locator_event_type(id),
                 journal_id INTEGER NOT NULL REFERENCES locator_journal(id),
                 subject_id INTEGER REFERENCES locator_subject(id),
                 track_id INTEGER REFERENCES locator_track(id),
                 payload_hash BLOB NOT NULL CHECK (length(payload_hash) = 32)
             ) STRICT;
             CREATE TABLE IF NOT EXISTS locator_event_type (
                 id INTEGER PRIMARY KEY,
                 value TEXT NOT NULL UNIQUE
             ) STRICT;
             CREATE TABLE IF NOT EXISTS locator_journal (
                 id INTEGER PRIMARY KEY,
                 value TEXT NOT NULL UNIQUE
             ) STRICT;
             CREATE TABLE IF NOT EXISTS locator_subject (
                 id INTEGER PRIMARY KEY,
                 value TEXT NOT NULL UNIQUE
             ) STRICT;
             CREATE TABLE IF NOT EXISTS locator_track (
                 id INTEGER PRIMARY KEY,
                 value TEXT NOT NULL UNIQUE
             ) STRICT;
             CREATE INDEX IF NOT EXISTS locator_event_display
                 ON locator_event(epoch, normalized_occurred_at, event_hash, sequence);
             CREATE INDEX IF NOT EXISTS locator_event_cursor
                 ON locator_event(epoch, sequence);
             CREATE INDEX IF NOT EXISTS locator_event_target
                 ON locator_event(subject_id, event_type_id, track_id);
             CREATE VIEW IF NOT EXISTS locator_event_text AS
             SELECT locator.sequence, locator.epoch,
                    'evt:sha256:' || lower(hex(locator.event_hash)) AS event_id,
                    locator.normalized_occurred_at,
                    lower(hex(locator.event_hash)) AS replay_key,
                    event_type.value AS event_type,
                    journal.value AS journal_id,
                    subject.value AS subject_id,
                    track.value AS track_id,
                    'sha256:' || lower(hex(locator.payload_hash)) AS payload_hash
             FROM locator_event AS locator
             JOIN locator_event_type AS event_type ON event_type.id = locator.event_type_id
             JOIN locator_journal AS journal ON journal.id = locator.journal_id
             LEFT JOIN locator_subject AS subject ON subject.id = locator.subject_id
             LEFT JOIN locator_track AS track ON track.id = locator.track_id;",
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
        || to_u64(epoch, "locator epoch")? != cursor.epoch
    {
        return Err(SqliteLocatorError::Metadata(format!(
            "locator identity {store_id}/{profile_id}/{schema_version}/{epoch} \
             does not match cursor {}/{}/{}/{:?}",
            cursor.store_id, LOCATOR_PROFILE_ID, LOCATOR_SCHEMA_VERSION, cursor.epoch
        )));
    }
    if schema_version < LOCATOR_SCHEMA_VERSION {
        return Err(SqliteLocatorError::UpgradeRequired(format!(
            "expected schema {LOCATOR_SCHEMA_VERSION}, observed {schema_version}"
        )));
    }
    if schema_version != LOCATOR_SCHEMA_VERSION {
        return Err(SqliteLocatorError::Metadata(format!(
            "expected locator schema {LOCATOR_SCHEMA_VERSION}, observed {schema_version}"
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

pub(crate) fn read_locator_checkpoint(
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
    let event_hash = validated_locator_event_hash(row)?;
    let payload_hash =
        required_prefixed_digest(&row.payload_hash, "sha256:", "locator payload hash")?;
    let event_type_id = dimension_id(transaction, "locator_event_type", &row.event_type)?;
    let journal_id = dimension_id(transaction, "locator_journal", &row.journal_id)?;
    let subject_id = row
        .subject_id
        .as_deref()
        .map(|value| dimension_id(transaction, "locator_subject", value))
        .transpose()?;
    let track_id = row
        .track_id
        .as_deref()
        .map(|value| dimension_id(transaction, "locator_track", value))
        .transpose()?;
    transaction
        .execute(
            "INSERT INTO locator_event
             (sequence, epoch, event_hash, normalized_occurred_at,
              event_type_id, journal_id, subject_id, track_id, payload_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                to_i64(row.cursor.sequence, "locator sequence")?,
                to_i64(row.cursor.epoch, "locator epoch")?,
                event_hash.as_slice(),
                row.normalized_occurred_at,
                event_type_id,
                journal_id,
                subject_id,
                track_id,
                payload_hash.as_slice(),
            ],
        )
        .map_err(|error| sqlite_error("insert locator row", error))?;
    Ok(())
}

/// Return the one physical digest shared by the event-id and replay-key views.
///
/// The two values retain distinct domain meanings, so validate their equality
/// at the storage boundary instead of silently deriving one from untrusted row
/// input. Event construction and authoritative store validation enforce the
/// same SHA-256-of-idempotency-key invariant before rows reach this point.
fn validated_locator_event_hash(row: &LocatorRow) -> Result<[u8; 32], SqliteLocatorError> {
    let event_hash = required_prefixed_digest(&row.event_id, "evt:sha256:", "locator event id")?;
    let replay_hash = required_digest(&row.replay_key, "locator replay key")?;
    if replay_hash != event_hash {
        return Err(SqliteLocatorError::Delta(
            "locator replay key does not match the event id digest".to_owned(),
        ));
    }
    Ok(event_hash)
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
        .query_map(parameters, stored_locator_row_from_sql)
        .map_err(|error| sqlite_error("query locator range", error))?;
    let mut output = Vec::new();
    for row in rows {
        output.push(row.map_err(|error| sqlite_error("read locator range", error))?);
    }
    let status = LocatorQueryStatus {
        fullscan_steps: status_value(statement.get_status(StatementStatus::FullscanStep)),
        sort_operations: status_value(statement.get_status(StatementStatus::Sort)),
    };
    Ok((output, status))
}

fn stored_locator_row_from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocatorRow> {
    let epoch = row.get::<_, i64>(0)?;
    let sequence = row.get::<_, i64>(1)?;
    let receipt_epoch = row.get::<_, i64>(12)?;
    if epoch <= 0 || sequence <= 0 {
        return Err(rusqlite::Error::IntegralValueOutOfRange(0, epoch));
    }
    if receipt_epoch != epoch {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(LocatorRow {
        cursor: TruthCursor::new(epoch as u64, sequence as u64),
        logical_reread_key: row.get(2)?,
        event_id: prefixed_digest_from_sql(row, 3, "evt:sha256:")?,
        normalized_occurred_at: row.get(4)?,
        replay_key: digest_from_sql(row, 5)?,
        event_type: row.get(6)?,
        journal_id: row.get(7)?,
        subject_id: row.get(8)?,
        track_id: row.get(9)?,
        payload_hash: prefixed_digest_from_sql(row, 10, "sha256:")?,
        validation_witness: row.get(11)?,
    })
}

fn hydrate_locator_row(
    journal: &QualificationLocalJournal,
    mut stored: LocatorRow,
) -> Result<HydratedLocatorRow, SqliteLocatorError> {
    let bytes = journal
        .read_event_bytes_by_key_digest(&stored.logical_reread_key)
        .map_err(|error| SqliteLocatorError::Metadata(error.to_string()))?
        .ok_or_else(|| {
            SqliteLocatorError::Metadata(format!(
                "locator carrier is absent for key digest {}",
                stored.logical_reread_key
            ))
        })?;
    if sha256_bytes_hex(&bytes) != stored.validation_witness {
        return Err(SqliteLocatorError::CarrierMismatch(stored.cursor));
    }
    let event = EventStore::decode_qualification_entry(stored.logical_reread_key.clone(), bytes)
        .map_err(|error| SqliteLocatorError::Metadata(error.to_string()))?;
    stored.logical_reread_key = event.idempotency_key.clone();
    let observed =
        LocatorRow::from_event(stored.cursor, &event, stored.validation_witness.clone())?;
    if observed != stored {
        return Err(SqliteLocatorError::CarrierMismatch(stored.cursor));
    }
    Ok(HydratedLocatorRow { row: stored, event })
}

fn locator_select(suffix: &str) -> String {
    format!(
        "SELECT locator.epoch, locator.sequence, receipt.logical_reread_key_hash,
                locator.event_hash, locator.normalized_occurred_at,
                locator.event_hash, event_type.value, journal.value,
                subject.value, track.value, locator.payload_hash,
                receipt.validation_witness, receipt.epoch
         FROM locator_event AS locator
         JOIN cursor_receipt_text AS receipt ON receipt.sequence = locator.sequence
         JOIN locator_event_type AS event_type ON event_type.id = locator.event_type_id
         JOIN locator_journal AS journal ON journal.id = locator.journal_id
         LEFT JOIN locator_subject AS subject ON subject.id = locator.subject_id
         LEFT JOIN locator_track AS track ON track.id = locator.track_id
         {suffix}"
    )
}

fn locator_select_indexed(suffix: &str) -> String {
    locator_select(suffix).replacen(
        "FROM locator_event AS locator",
        "FROM locator_event AS locator INDEXED BY locator_event_display",
        1,
    )
}

fn dimension_id(
    transaction: &rusqlite::Transaction<'_>,
    table: &'static str,
    value: &str,
) -> Result<i64, SqliteLocatorError> {
    let insert = format!("INSERT INTO {table}(value) VALUES (?1) ON CONFLICT(value) DO NOTHING");
    transaction
        .execute(&insert, [value])
        .map_err(|error| sqlite_error("insert locator dimension", error))?;
    let select = format!("SELECT id FROM {table} WHERE value = ?1");
    transaction
        .query_row(&select, [value], |row| row.get(0))
        .map_err(|error| sqlite_error("read locator dimension", error))
}

fn required_prefixed_digest(
    value: &str,
    prefix: &str,
    label: &'static str,
) -> Result<[u8; 32], SqliteLocatorError> {
    let Some(value) = value.strip_prefix(prefix) else {
        return Err(SqliteLocatorError::Delta(format!(
            "{label} does not start with {prefix}"
        )));
    };
    required_digest(value, label)
}

fn decode_prefixed_digest(value: &str, prefix: &str) -> Option<[u8; 32]> {
    required_prefixed_digest(value, prefix, "digest").ok()
}

fn required_digest(value: &str, label: &'static str) -> Result<[u8; 32], SqliteLocatorError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(SqliteLocatorError::Delta(format!(
            "{label} is not a 64-character lowercase hexadecimal digest"
        )));
    }
    let mut digest = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(chunk).expect("ASCII hex slices are UTF-8");
        digest[index] =
            u8::from_str_radix(pair, 16).expect("validated hexadecimal pairs must decode");
    }
    Ok(digest)
}

fn digest_from_sql(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<String> {
    let bytes = row.get::<_, Vec<u8>>(index)?;
    if bytes.len() != 32 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn prefixed_digest_from_sql(
    row: &rusqlite::Row<'_>,
    index: usize,
    prefix: &str,
) -> rusqlite::Result<String> {
    digest_from_sql(row, index).map(|digest| format!("{prefix}{digest}"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::derived_access::cursor::TruthCursor;
    use crate::session::derived_access::locator::LocatorRow;

    #[test]
    fn locator_insert_rejects_divergent_event_and_replay_digests() {
        let row = LocatorRow {
            cursor: TruthCursor::new(1, 1),
            logical_reread_key: "logical:key".to_owned(),
            event_id: format!("evt:sha256:{}", "1".repeat(64)),
            normalized_occurred_at: "2026-08-01T20:00:00.000Z".to_owned(),
            replay_key: "2".repeat(64),
            event_type: "review_initialized".to_owned(),
            journal_id: "journal:test".to_owned(),
            subject_id: None,
            track_id: None,
            payload_hash: format!("sha256:{}", "3".repeat(64)),
            validation_witness: "4".repeat(64),
        };

        let error = validated_locator_event_hash(&row)
            .expect_err("divergent event and replay digests must fail before insertion");
        assert!(
            matches!(error, SqliteLocatorError::Delta(ref message) if message.contains("replay key")),
            "unexpected error: {error:?}"
        );
    }
}
