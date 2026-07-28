//! Qualification-only bodyless SQLite semantic facts.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

use super::sqlite_locator::{SqliteLocator, SqliteLocatorError, read_locator_checkpoint};
use crate::canonical_hash::sha256_bytes_hex;
use crate::session::EventStore;
use crate::session::derived_access::QualificationLocalJournal;
use crate::session::derived_access::cursor::{CursorDelta, TruthCursor};
use crate::session::derived_access::locator::{LocatorRead, LocatorRow};
use crate::session::derived_access::semantic::state::{
    MaterializedSemanticDuplicate, MaterializedSemanticState, SemanticStateSnapshot,
};
use crate::session::derived_access::semantic::{
    AssessmentFact, CommitAssociationFact, CommitWithdrawalFact, InputRequestFact,
    InputResponseFact, RefAssociationFact, RefWithdrawalFact, RevisionFact, SemanticFact,
    SemanticFactKind, SemanticModelError, SemanticSnapshot, ValidationFact, decode_enum,
    decode_string_list, encode_enum, encode_string_list,
};
use crate::session::event::ShoreEvent;

const SEMANTIC_PROFILE_ID: &str = "pointbreak.sqlite-derived-access-semantic.v1";
const SEMANTIC_SCHEMA_VERSION: i64 = 3;

#[derive(Clone, Debug)]
pub(crate) struct SqliteSemantic {
    locator: SqliteLocator,
}

#[derive(Debug)]
pub(super) struct HydratedSemanticFact {
    pub(super) fact: SemanticFact,
    pub(super) event: ShoreEvent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SemanticInventory {
    pub(crate) profile_id: String,
    pub(crate) schema_version: u32,
    pub(crate) fact_count: u64,
    pub(crate) tables: Vec<String>,
    pub(crate) columns: Vec<String>,
    pub(crate) indexes: Vec<String>,
    pub(crate) retained_body_object_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SqliteSemanticError {
    #[error(transparent)]
    Locator(#[from] SqliteLocatorError),
    #[error(transparent)]
    Model(#[from] SemanticModelError),
    #[error("semantic metadata mismatch: {0}")]
    Metadata(String),
    #[error("semantic delta does not follow its checkpoint: {0}")]
    Delta(String),
    #[error("semantic SQLite failure during {operation}: {message}")]
    Sqlite {
        operation: &'static str,
        message: String,
    },
    #[error("semantic carrier does not match persisted fact at {0:?}")]
    CarrierMismatch(TruthCursor),
}

impl SqliteSemantic {
    pub(crate) fn open(locator: SqliteLocator) -> Result<Self, SqliteSemanticError> {
        let connection = locator.validated_connection()?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS semantic_meta (
                     singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                     profile_id TEXT NOT NULL,
                     schema_version INTEGER NOT NULL CHECK (schema_version = 3),
                     epoch INTEGER NOT NULL CHECK (epoch > 0),
                     applied_sequence INTEGER NOT NULL CHECK (applied_sequence >= 0)
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS semantic_identity_prefix (
                     id INTEGER PRIMARY KEY,
                     value TEXT NOT NULL UNIQUE
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS semantic_actor (
                     id INTEGER PRIMARY KEY,
                     value TEXT NOT NULL UNIQUE
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS semantic_event_fact (
                     sequence INTEGER PRIMARY KEY CHECK (sequence > 0)
                         REFERENCES locator_event(sequence),
                     revision_prefix_id INTEGER REFERENCES semantic_identity_prefix(id),
                     revision_digest BLOB CHECK (length(revision_digest) = 32),
                     revision_raw TEXT,
                     semantic_prefix_id INTEGER REFERENCES semantic_identity_prefix(id),
                     semantic_digest BLOB CHECK (length(semantic_digest) = 32),
                     semantic_raw TEXT,
                     content_prefix_id INTEGER REFERENCES semantic_identity_prefix(id),
                     content_digest BLOB CHECK (length(content_digest) = 32),
                     content_raw TEXT,
                     occurred_at TEXT NOT NULL,
                     assertion_mode INTEGER NOT NULL CHECK (assertion_mode IN (0, 1)),
                     actor_id INTEGER NOT NULL REFERENCES semantic_actor(id),
                     CHECK (
                         (revision_prefix_id IS NULL AND revision_digest IS NULL)
                         OR (revision_prefix_id IS NOT NULL AND revision_digest IS NOT NULL)
                     ),
                     CHECK (
                         (semantic_prefix_id IS NULL AND semantic_digest IS NULL)
                         OR (semantic_prefix_id IS NOT NULL AND semantic_digest IS NOT NULL)
                     ),
                     CHECK (
                         (content_prefix_id IS NULL AND content_digest IS NULL)
                         OR (content_prefix_id IS NOT NULL AND content_digest IS NOT NULL)
                     )
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS semantic_revision_fact (
                     sequence INTEGER PRIMARY KEY REFERENCES semantic_event_fact(sequence),
                     object_id TEXT NOT NULL,
                     engagement_id TEXT NOT NULL,
                     supersedes_json TEXT NOT NULL,
                     base_commit_oid TEXT,
                     capture_commit_oid TEXT,
                     capture_tree_oid TEXT
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS semantic_revision_engagement
                     ON semantic_revision_fact(engagement_id, sequence);
                 CREATE TABLE IF NOT EXISTS semantic_assessment_fact (
                     sequence INTEGER PRIMARY KEY REFERENCES semantic_event_fact(sequence),
                     assessment TEXT NOT NULL,
                     replaces_json TEXT NOT NULL,
                     related_observations_json TEXT NOT NULL,
                     related_requests_json TEXT NOT NULL,
                     revision_scoped INTEGER NOT NULL CHECK (revision_scoped IN (0, 1))
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS semantic_request_fact (
                     sequence INTEGER PRIMARY KEY REFERENCES semantic_event_fact(sequence),
                     reason_code TEXT NOT NULL,
                     title TEXT NOT NULL
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS semantic_response_fact (
                     sequence INTEGER PRIMARY KEY REFERENCES semantic_event_fact(sequence),
                     request_id TEXT NOT NULL
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS semantic_response_request
                     ON semantic_response_fact(request_id);
                 CREATE TABLE IF NOT EXISTS semantic_validation_fact (
                     sequence INTEGER PRIMARY KEY REFERENCES semantic_event_fact(sequence),
                     check_name TEXT NOT NULL,
                     status TEXT NOT NULL,
                     exit_code INTEGER,
                     completed_at TEXT,
                     log_hashes_json TEXT NOT NULL
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS semantic_commit_association_fact (
                     sequence INTEGER PRIMARY KEY REFERENCES semantic_event_fact(sequence),
                     commit_oid TEXT NOT NULL,
                     tree_oid TEXT NOT NULL
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS semantic_commit_withdrawal_fact (
                     sequence INTEGER PRIMARY KEY REFERENCES semantic_event_fact(sequence),
                     association_id TEXT NOT NULL
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS semantic_ref_association_fact (
                     sequence INTEGER PRIMARY KEY REFERENCES semantic_event_fact(sequence),
                     ref_name TEXT NOT NULL,
                     head_oid TEXT NOT NULL
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS semantic_ref_withdrawal_fact (
                     sequence INTEGER PRIMARY KEY REFERENCES semantic_event_fact(sequence),
                     association_id TEXT NOT NULL
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS semantic_representative (
                     family TEXT NOT NULL,
                     semantic_key_prefix_id INTEGER
                         REFERENCES semantic_identity_prefix(id),
                     semantic_key_digest BLOB CHECK (length(semantic_key_digest) = 32),
                     semantic_key_raw TEXT,
                     semantic_key_hash BLOB NOT NULL CHECK (length(semantic_key_hash) = 32),
                     sequence INTEGER NOT NULL REFERENCES semantic_event_fact(sequence),
                     CHECK (
                         (
                             semantic_key_prefix_id IS NULL
                             AND semantic_key_digest IS NULL
                             AND semantic_key_raw IS NOT NULL
                         )
                         OR (
                             semantic_key_prefix_id IS NOT NULL
                             AND semantic_key_digest IS NOT NULL
                             AND semantic_key_raw IS NULL
                         )
                     ),
                     PRIMARY KEY (family, semantic_key_hash)
                 ) STRICT, WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS semantic_representative_sequence
                     ON semantic_representative(sequence, family);
                 CREATE VIEW IF NOT EXISTS semantic_representative_text AS
                 SELECT representative.family,
                        coalesce(
                            representative.semantic_key_raw,
                            prefix.value || lower(hex(representative.semantic_key_digest))
                        ) AS semantic_key,
                        representative.semantic_key_hash,
                        representative.sequence
                 FROM semantic_representative AS representative
                 LEFT JOIN semantic_identity_prefix AS prefix
                   ON prefix.id = representative.semantic_key_prefix_id;
                 CREATE TABLE IF NOT EXISTS semantic_duplicate_projection (
                     family TEXT NOT NULL,
                     semantic_key TEXT NOT NULL,
                     event_count INTEGER NOT NULL CHECK (event_count >= 1),
                     event_ids_json TEXT NOT NULL,
                     PRIMARY KEY (family, semantic_key)
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS semantic_state_projection (
                     singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                     journal_id TEXT NOT NULL,
                     current_revision_id TEXT,
                     current_object_id TEXT,
                     revision_count INTEGER NOT NULL CHECK (revision_count >= 0),
                     event_count INTEGER NOT NULL CHECK (event_count >= 0),
                     observation_count INTEGER NOT NULL CHECK (observation_count >= 0),
                     assessment_count INTEGER NOT NULL CHECK (assessment_count >= 0),
                     validation_check_count INTEGER NOT NULL
                         CHECK (validation_check_count >= 0),
                     input_request_count INTEGER NOT NULL CHECK (input_request_count >= 0),
                     open_input_request_count INTEGER NOT NULL
                         CHECK (open_input_request_count >= 0),
                     open_operative_input_request_count INTEGER NOT NULL
                         CHECK (open_operative_input_request_count >= 0)
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS semantic_event_fact_revision
                     ON semantic_event_fact(
                         revision_prefix_id, revision_digest, revision_raw, sequence
                     );
                 CREATE INDEX IF NOT EXISTS semantic_event_fact_content
                     ON semantic_event_fact(
                         content_prefix_id, content_digest, content_raw, sequence
                     );
                 CREATE VIEW IF NOT EXISTS semantic_event_fact_text AS
                 SELECT event.sequence,
                        coalesce(
                            event.revision_raw,
                            revision_prefix.value || lower(hex(event.revision_digest))
                        ) AS revision_id,
                        coalesce(
                            event.semantic_raw,
                            semantic_prefix.value || lower(hex(event.semantic_digest))
                        ) AS semantic_id,
                        coalesce(
                            event.content_raw,
                            content_prefix.value || lower(hex(event.content_digest))
                        ) AS content_hash,
                        event.occurred_at,
                        CASE event.assertion_mode
                            WHEN 0 THEN 'advisory'
                            WHEN 1 THEN 'operative'
                        END AS assertion_mode,
                        actor.value AS actor_id,
                        event.revision_prefix_id,
                        event.revision_digest,
                        event.semantic_prefix_id,
                        event.semantic_digest,
                        event.content_prefix_id,
                        event.content_digest
                 FROM semantic_event_fact AS event
                 LEFT JOIN semantic_identity_prefix AS revision_prefix
                   ON revision_prefix.id = event.revision_prefix_id
                 LEFT JOIN semantic_identity_prefix AS semantic_prefix
                   ON semantic_prefix.id = event.semantic_prefix_id
                 LEFT JOIN semantic_identity_prefix AS content_prefix
                   ON content_prefix.id = event.content_prefix_id
                 JOIN semantic_actor AS actor ON actor.id = event.actor_id;",
            )
            .map_err(|error| sqlite_error("create semantic schema", error))?;
        let locator_checkpoint = read_locator_checkpoint(&connection)?;
        let inserted = connection
            .execute(
                "INSERT INTO semantic_meta
                 (singleton, profile_id, schema_version, epoch, applied_sequence)
                 VALUES (1, ?1, ?2, ?3, 0)
                 ON CONFLICT(singleton) DO NOTHING",
                params![
                    SEMANTIC_PROFILE_ID,
                    SEMANTIC_SCHEMA_VERSION,
                    to_i64(locator_checkpoint.applied.epoch, "semantic epoch")?,
                ],
            )
            .map_err(|error| sqlite_error("initialize semantic metadata", error))?;
        if inserted == 1 && locator_checkpoint.applied.sequence != 0 {
            return Err(SqliteSemanticError::Metadata(format!(
                "semantic profile requires deliberate rebuild for existing locator cursor {:?}",
                locator_checkpoint.applied
            )));
        }
        connection
            .execute(
                "INSERT INTO semantic_state_projection
                 (singleton, journal_id, current_revision_id, current_object_id,
                  revision_count, event_count, observation_count, assessment_count,
                  validation_check_count, input_request_count, open_input_request_count,
                  open_operative_input_request_count)
                 VALUES (1, 'journal:default', NULL, NULL, 0, 0, 0, 0, 0, 0, 0, 0)
                 ON CONFLICT(singleton) DO NOTHING",
                [],
            )
            .map_err(|error| sqlite_error("initialize semantic state projection", error))?;
        validate_meta(&connection, locator_checkpoint.applied)?;
        Ok(Self { locator })
    }

    pub(crate) fn apply_delta(
        &self,
        delta: &CursorDelta,
        locator_rows: &[LocatorRow],
        semantic_facts: &[SemanticFact],
    ) -> Result<TruthCursor, SqliteSemanticError> {
        self.apply_delta_inner(delta, locator_rows, semantic_facts, false)
    }

    pub(crate) fn apply_delta_with_failure(
        &self,
        delta: &CursorDelta,
        locator_rows: &[LocatorRow],
        semantic_facts: &[SemanticFact],
    ) -> Result<TruthCursor, SqliteSemanticError> {
        self.apply_delta_inner(delta, locator_rows, semantic_facts, true)
    }

    fn apply_delta_inner(
        &self,
        delta: &CursorDelta,
        locator_rows: &[LocatorRow],
        semantic_facts: &[SemanticFact],
        inject_failure: bool,
    ) -> Result<TruthCursor, SqliteSemanticError> {
        if semantic_facts.len() != delta.receipts.len()
            || semantic_facts.len() != locator_rows.len()
        {
            return Err(SqliteSemanticError::Delta(format!(
                "{} semantic facts and {} locator rows for {} cursor receipts",
                semantic_facts.len(),
                locator_rows.len(),
                delta.receipts.len()
            )));
        }
        for ((receipt, locator), fact) in
            delta.receipts.iter().zip(locator_rows).zip(semantic_facts)
        {
            if fact.cursor != receipt.cursor
                || fact.logical_reread_key != receipt.logical_reread_key
                || fact.validation_witness != receipt.validation_witness
                || fact.event_id != locator.event_id
            {
                return Err(SqliteSemanticError::Delta(format!(
                    "semantic fact does not match receipt/locator at {:?}",
                    receipt.cursor
                )));
            }
        }
        let applied = delta
            .receipts
            .last()
            .map_or(delta.after, |receipt| receipt.cursor);
        let result = self
            .locator
            .apply_delta_with(delta, locator_rows, |transaction| {
                insert_facts(transaction, semantic_facts)?;
                if inject_failure {
                    return Err(SqliteLocatorError::Delta(
                        "injected semantic transaction failure".to_owned(),
                    ));
                }
                let updated = transaction
                    .execute(
                        "UPDATE semantic_meta
                         SET applied_sequence = ?1
                         WHERE singleton = 1 AND epoch = ?2 AND applied_sequence = ?3",
                        params![
                            to_i64_locator(applied.sequence, "semantic applied")?,
                            to_i64_locator(applied.epoch, "semantic epoch")?,
                            to_i64_locator(delta.after.sequence, "semantic previous applied")?,
                        ],
                    )
                    .map_err(|error| locator_sqlite_error("advance semantic metadata", error))?;
                if updated != 1 {
                    return Err(SqliteLocatorError::Delta(
                        "semantic checkpoint changed concurrently".to_owned(),
                    ));
                }
                Ok(())
            });
        result.map_err(SqliteSemanticError::from)?;
        Ok(applied)
    }

    pub(crate) fn audit_snapshot(
        &self,
        observed: TruthCursor,
    ) -> Result<LocatorRead<SemanticSnapshot>, SqliteSemanticError> {
        let connection = self.locator.validated_connection()?;
        let checkpoint = read_locator_checkpoint(&connection)?;
        validate_meta(&connection, checkpoint.applied)?;
        if checkpoint.applied.epoch != observed.epoch
            || checkpoint.applied.sequence < observed.sequence
        {
            return Ok(LocatorRead::CatchUpRequired {
                applied: checkpoint.applied,
                observed,
            });
        }
        let journal = QualificationLocalJournal::new(self.locator.store_root());
        let facts = query_facts(
            &connection,
            &journal,
            "SELECT locator.epoch, event.sequence, receipt.logical_reread_key_hash,
                    locator.replay_key, locator.event_id, locator.event_type,
                    locator.journal_id, event.revision_id, event.semantic_id,
                    event.content_hash, locator.payload_hash,
                    event.occurred_at, event.assertion_mode,
                    locator.track_id, event.actor_id, receipt.validation_witness,
                    receipt.epoch
             FROM semantic_event_fact_text AS event
             JOIN locator_event_text AS locator ON locator.sequence = event.sequence
             JOIN cursor_receipt_text AS receipt ON receipt.sequence = event.sequence
             WHERE locator.epoch = ?1 AND event.sequence <= ?2
             ORDER BY locator.replay_key, receipt.logical_reread_key_hash",
            params![
                to_i64(observed.epoch, "snapshot epoch")?,
                to_i64(observed.sequence, "snapshot cursor")?,
            ],
        )?;
        let facts = hydrated_facts_only(facts);
        Ok(LocatorRead::Ready(SemanticSnapshot::audit_from_facts(
            observed, &facts,
        )?))
    }

    pub(crate) fn materialized_audit_snapshot(
        &self,
        observed: TruthCursor,
    ) -> Result<LocatorRead<SemanticSnapshot>, SqliteSemanticError> {
        let connection = self.locator.validated_connection()?;
        let checkpoint = read_locator_checkpoint(&connection)?;
        validate_meta(&connection, checkpoint.applied)?;
        if checkpoint.applied.epoch != observed.epoch
            || checkpoint.applied.sequence < observed.sequence
        {
            return Ok(LocatorRead::CatchUpRequired {
                applied: checkpoint.applied,
                observed,
            });
        }
        let state = query_materialized_state(&connection)?;
        let journal = QualificationLocalJournal::new(self.locator.store_root());
        let facts = hydrated_facts_only(query_materialized_facts(
            &connection,
            &journal,
            observed.epoch,
            observed.sequence,
            None,
        )?);
        #[cfg(any(test, feature = "longitudinal-counting"))]
        {
            crate::bench_support::longitudinal::record_projection_rebuild();
            crate::bench_support::longitudinal::record_event_folds(facts.len());
        }
        Ok(LocatorRead::Ready(SemanticSnapshot::from_materialized(
            observed, state, &facts,
        )?))
    }

    pub(crate) fn materialized_engagement_snapshot(
        &self,
        engagement_id: &str,
        observed: TruthCursor,
    ) -> Result<LocatorRead<SemanticSnapshot>, SqliteSemanticError> {
        let connection = self.locator.validated_connection()?;
        let checkpoint = read_locator_checkpoint(&connection)?;
        validate_meta(&connection, checkpoint.applied)?;
        if checkpoint.applied.epoch != observed.epoch
            || checkpoint.applied.sequence < observed.sequence
        {
            return Ok(LocatorRead::CatchUpRequired {
                applied: checkpoint.applied,
                observed,
            });
        }
        let state = query_materialized_state(&connection)?;
        let journal = QualificationLocalJournal::new(self.locator.store_root());
        let facts = hydrated_facts_only(query_materialized_facts(
            &connection,
            &journal,
            observed.epoch,
            observed.sequence,
            Some(engagement_id),
        )?);
        Ok(LocatorRead::Ready(SemanticSnapshot::from_materialized(
            observed, state, &facts,
        )?))
    }

    pub(super) fn facts_for_revision_hydrated(
        &self,
        revision_id: &str,
        observed: TruthCursor,
    ) -> Result<LocatorRead<Vec<HydratedSemanticFact>>, SqliteSemanticError> {
        let connection = self.locator.validated_connection()?;
        let checkpoint = read_locator_checkpoint(&connection)?;
        validate_meta(&connection, checkpoint.applied)?;
        if checkpoint.applied.epoch != observed.epoch
            || checkpoint.applied.sequence < observed.sequence
        {
            return Ok(LocatorRead::CatchUpRequired {
                applied: checkpoint.applied,
                observed,
            });
        }
        let epoch = to_i64(observed.epoch, "detail epoch")?;
        let sequence = to_i64(observed.sequence, "detail cursor")?;
        let journal = QualificationLocalJournal::new(self.locator.store_root());
        let facts = if let Some((prefix, digest)) = split_canonical_digest(revision_id) {
            query_facts(
                &connection,
                &journal,
                &selected_semantic_facts(
                    "physical.revision_prefix_id = (
                         SELECT id FROM semantic_identity_prefix WHERE value = ?1
                     )
                     AND physical.revision_digest = ?2",
                    "semantic_event_fact_revision",
                    3,
                    4,
                ),
                params![prefix, digest.as_slice(), epoch, sequence],
            )?
        } else {
            query_facts(
                &connection,
                &journal,
                &selected_semantic_facts(
                    "physical.revision_prefix_id IS NULL
                     AND physical.revision_digest IS NULL
                     AND physical.revision_raw = ?1",
                    "semantic_event_fact_revision",
                    2,
                    3,
                ),
                params![revision_id, epoch, sequence],
            )?
        };
        Ok(LocatorRead::Ready(facts))
    }

    pub(crate) fn content_is_removed(
        &self,
        content_hash: &str,
        observed: TruthCursor,
    ) -> Result<bool, SqliteSemanticError> {
        let connection = self.locator.validated_connection()?;
        let epoch = to_i64(observed.epoch, "removal epoch")?;
        let sequence = to_i64(observed.sequence, "removal cursor")?;
        let (query, parameters): (String, Vec<rusqlite::types::Value>) =
            if let Some((prefix, digest)) = split_canonical_digest(content_hash) {
                (
                    selected_content_query(
                        "event.content_prefix_id = (
                             SELECT id FROM semantic_identity_prefix WHERE value = ?1
                         )
                         AND event.content_digest = ?2",
                        3,
                        4,
                    ),
                    vec![
                        prefix.to_owned().into(),
                        digest.to_vec().into(),
                        epoch.into(),
                        sequence.into(),
                    ],
                )
            } else {
                (
                    selected_content_query(
                        "event.content_prefix_id IS NULL
                         AND event.content_digest IS NULL
                         AND event.content_raw = ?1",
                        2,
                        3,
                    ),
                    vec![
                        content_hash.to_owned().into(),
                        epoch.into(),
                        sequence.into(),
                    ],
                )
            };
        let count = connection
            .query_row(&query, rusqlite::params_from_iter(parameters), |_| Ok(()))
            .optional()
            .map_err(|error| sqlite_error("query removal fact", error))?;
        Ok(count.is_some())
    }

    pub(crate) fn inventory(&self) -> Result<SemanticInventory, SqliteSemanticError> {
        let connection = self.locator.validated_connection()?;
        let (profile_id, schema_version) = connection
            .query_row(
                "SELECT profile_id, schema_version FROM semantic_meta WHERE singleton = 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(|error| sqlite_error("read semantic inventory identity", error))?;
        let fact_count = connection
            .query_row("SELECT count(*) FROM semantic_event_fact", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|error| sqlite_error("count semantic facts", error))?;
        let retained_body_object_bytes = retained_body_object_bytes(&connection)?;
        Ok(SemanticInventory {
            profile_id,
            schema_version: u32::try_from(schema_version)
                .map_err(|_| SqliteSemanticError::Metadata("negative schema version".to_owned()))?,
            fact_count: u64::try_from(fact_count)
                .map_err(|_| SqliteSemanticError::Metadata("negative fact count".to_owned()))?,
            tables: query_names(
                &connection,
                "SELECT name FROM sqlite_schema
                 WHERE type = 'table' AND name LIKE 'semantic_%'
                 ORDER BY name",
                0,
            )?,
            columns: query_names(&connection, "PRAGMA table_info(semantic_event_fact)", 1)?,
            indexes: query_names(&connection, "PRAGMA index_list(semantic_event_fact)", 1)?,
            retained_body_object_bytes,
        })
    }
}

fn retained_body_object_bytes(
    connection: &rusqlite::Connection,
) -> Result<u64, SqliteSemanticError> {
    let tables = query_names(
        connection,
        "SELECT name FROM sqlite_schema
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
        0,
    )?;
    let mut total = 0_u64;
    for table in tables {
        let pragma = format!("PRAGMA table_info({})", quote_identifier(&table));
        let mut statement = connection
            .prepare(&pragma)
            .map_err(|error| sqlite_error("inspect derived-access columns", error))?;
        let columns = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })
            .map_err(|error| sqlite_error("inspect derived-access columns", error))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| sqlite_error("inspect derived-access columns", error))?;
        for (column, declared_type) in columns {
            if !is_retained_body_object_column(&column, &declared_type) {
                continue;
            }
            let query = format!(
                "SELECT coalesce(sum(length({})), 0) FROM {}",
                quote_identifier(&column),
                quote_identifier(&table)
            );
            let bytes = connection
                .query_row(&query, [], |row| row.get::<_, i64>(0))
                .map_err(|error| sqlite_error("measure retained body/object bytes", error))?;
            total = total.saturating_add(u64::try_from(bytes).map_err(|_| {
                SqliteSemanticError::Metadata("negative retained body/object bytes".to_owned())
            })?);
        }
    }
    Ok(total)
}

fn is_retained_body_object_column(name: &str, declared_type: &str) -> bool {
    let name = name.to_ascii_lowercase();
    if declared_type.eq_ignore_ascii_case("BLOB")
        && !name.ends_with("_digest")
        && !name.ends_with("_hash")
    {
        return true;
    }
    matches!(name.as_str(), "body" | "object" | "payload" | "content")
        || ["body", "object", "payload", "content"]
            .iter()
            .any(|subject| {
                ["bytes", "json", "text", "content"]
                    .iter()
                    .any(|representation| name == format!("{subject}_{representation}"))
            })
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn insert_facts(
    transaction: &Transaction<'_>,
    facts: &[SemanticFact],
) -> Result<(), SqliteLocatorError> {
    for fact in facts {
        let revision = encode_identity(transaction, fact.revision_id.as_deref())?;
        let semantic = encode_identity(transaction, fact.semantic_id.as_deref())?;
        let content = encode_identity(transaction, fact.content_hash.as_deref())?;
        let actor_id = semantic_dimension_id(transaction, "semantic_actor", &fact.actor_id)?;
        let assertion_mode = match fact.assertion_mode {
            crate::session::event::AssertionMode::Advisory => 0_i64,
            crate::session::event::AssertionMode::Operative => 1_i64,
        };
        transaction
            .execute(
                "INSERT INTO semantic_event_fact
                 (sequence,
                  revision_prefix_id, revision_digest, revision_raw,
                  semantic_prefix_id, semantic_digest, semantic_raw,
                  content_prefix_id, content_digest, content_raw,
                  occurred_at,
                  assertion_mode, actor_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    to_i64_locator(fact.cursor.sequence, "semantic sequence")?,
                    revision.prefix_id,
                    revision.digest.as_deref(),
                    revision.raw,
                    semantic.prefix_id,
                    semantic.digest.as_deref(),
                    semantic.raw,
                    content.prefix_id,
                    content.digest.as_deref(),
                    content.raw,
                    fact.occurred_at,
                    assertion_mode,
                    actor_id,
                ],
            )
            .map_err(|error| locator_sqlite_error("insert semantic fact", error))?;
        insert_family_fact(transaction, fact)?;
        update_materialized_projection(transaction, fact)?;
    }
    Ok(())
}

struct EncodedIdentity {
    prefix_id: Option<i64>,
    digest: Option<Vec<u8>>,
    raw: Option<String>,
}

fn encode_identity(
    transaction: &Transaction<'_>,
    value: Option<&str>,
) -> Result<EncodedIdentity, SqliteLocatorError> {
    let Some(value) = value else {
        return Ok(EncodedIdentity {
            prefix_id: None,
            digest: None,
            raw: None,
        });
    };
    if let Some((prefix, digest)) = split_canonical_digest(value) {
        return Ok(EncodedIdentity {
            prefix_id: Some(semantic_dimension_id(
                transaction,
                "semantic_identity_prefix",
                prefix,
            )?),
            digest: Some(digest.to_vec()),
            raw: None,
        });
    }
    Ok(EncodedIdentity {
        prefix_id: None,
        digest: None,
        raw: Some(value.to_owned()),
    })
}

fn split_canonical_digest(value: &str) -> Option<(&str, [u8; 32])> {
    let split = value.len().checked_sub(64)?;
    let (prefix, hex) = value.split_at(split);
    if prefix.is_empty()
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
        digest[index] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some((prefix, digest))
}

fn semantic_key_digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

fn semantic_dimension_id(
    transaction: &Transaction<'_>,
    table: &'static str,
    value: &str,
) -> Result<i64, SqliteLocatorError> {
    let insert = format!("INSERT INTO {table}(value) VALUES (?1) ON CONFLICT(value) DO NOTHING");
    transaction
        .execute(&insert, [value])
        .map_err(|error| locator_sqlite_error("insert semantic dimension", error))?;
    let select = format!("SELECT id FROM {table} WHERE value = ?1");
    transaction
        .query_row(&select, [value], |row| row.get(0))
        .map_err(|error| locator_sqlite_error("read semantic dimension", error))
}

fn insert_family_fact(
    transaction: &Transaction<'_>,
    fact: &SemanticFact,
) -> Result<(), SqliteLocatorError> {
    let sequence = to_i64_locator(fact.cursor.sequence, "semantic family sequence")?;
    match &fact.kind {
        SemanticFactKind::Revision(revision) => transaction.execute(
            "INSERT INTO semantic_revision_fact
                 (sequence, object_id, engagement_id, supersedes_json, base_commit_oid,
                  capture_commit_oid, capture_tree_oid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                sequence,
                revision.object_id,
                revision.engagement_id,
                list_text(&revision.supersedes)?,
                revision.base_commit_oid,
                revision.capture_commit_oid,
                revision.capture_tree_oid,
            ],
        ),
        SemanticFactKind::Assessment(assessment) => transaction.execute(
            "INSERT INTO semantic_assessment_fact
             (sequence, assessment, replaces_json, related_observations_json,
              related_requests_json, revision_scoped)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                sequence,
                enum_text(assessment.assessment)?,
                list_text(&assessment.replaces)?,
                list_text(&assessment.related_observations)?,
                list_text(&assessment.related_requests)?,
                i64::from(assessment.revision_scoped),
            ],
        ),
        SemanticFactKind::InputRequestOpened(request) => transaction.execute(
            "INSERT INTO semantic_request_fact (sequence, reason_code, title)
             VALUES (?1, ?2, ?3)",
            params![sequence, enum_text(request.reason_code)?, request.title],
        ),
        SemanticFactKind::InputRequestResponded(response) => transaction.execute(
            "INSERT INTO semantic_response_fact (sequence, request_id) VALUES (?1, ?2)",
            params![sequence, response.request_id],
        ),
        SemanticFactKind::Validation(validation) => transaction.execute(
            "INSERT INTO semantic_validation_fact
             (sequence, check_name, status, exit_code, completed_at, log_hashes_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                sequence,
                validation.check_name,
                enum_text(validation.status)?,
                validation.exit_code,
                validation.completed_at,
                list_text(&validation.log_artifact_content_hashes)?,
            ],
        ),
        SemanticFactKind::CommitAssociated(association) => transaction.execute(
            "INSERT INTO semantic_commit_association_fact
             (sequence, commit_oid, tree_oid) VALUES (?1, ?2, ?3)",
            params![sequence, association.commit_oid, association.tree_oid],
        ),
        SemanticFactKind::CommitWithdrawn(withdrawal) => transaction.execute(
            "INSERT INTO semantic_commit_withdrawal_fact (sequence, association_id)
             VALUES (?1, ?2)",
            params![sequence, withdrawal.association_id],
        ),
        SemanticFactKind::RefAssociated(association) => transaction.execute(
            "INSERT INTO semantic_ref_association_fact
             (sequence, ref_name, head_oid) VALUES (?1, ?2, ?3)",
            params![sequence, association.ref_name, association.head_oid],
        ),
        SemanticFactKind::RefWithdrawn(withdrawal) => transaction.execute(
            "INSERT INTO semantic_ref_withdrawal_fact (sequence, association_id)
             VALUES (?1, ?2)",
            params![sequence, withdrawal.association_id],
        ),
        SemanticFactKind::Observation
        | SemanticFactKind::ArtifactRemoved
        | SemanticFactKind::Other => return Ok(()),
    }
    .map_err(|error| locator_sqlite_error("insert semantic family fact", error))?;
    Ok(())
}

fn update_materialized_projection(
    transaction: &Transaction<'_>,
    fact: &SemanticFact,
) -> Result<(), SqliteLocatorError> {
    if fact.event_type == "review_initialized" {
        let journal_id = transaction
            .query_row(
                "SELECT locator.journal_id
                 FROM semantic_event_fact_text AS event
                 JOIN locator_event_text AS locator ON locator.sequence = event.sequence
                 WHERE locator.event_type = 'review_initialized'
                   AND event.semantic_id IS NULL
                 ORDER BY locator.replay_key DESC, locator.event_id DESC
                 LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| locator_sqlite_error("select materialized journal", error))?;
        transaction
            .execute(
                "UPDATE semantic_state_projection
                 SET event_count = event_count + 1, journal_id = ?1
                 WHERE singleton = 1",
                [journal_id],
            )
            .map_err(|error| locator_sqlite_error("advance materialized state", error))?;
    } else {
        transaction
            .execute(
                "UPDATE semantic_state_projection
                 SET event_count = event_count + 1,
                     journal_id = CASE
                         WHEN event_count = 0 THEN ?1
                         ELSE journal_id
                     END
                 WHERE singleton = 1",
                [&fact.journal_id],
            )
            .map_err(|error| locator_sqlite_error("advance materialized state", error))?;
    }

    let Some((family, semantic_key)) = materialized_identity(fact) else {
        return Ok(());
    };
    if duplicate_family(family) {
        update_materialized_duplicate(transaction, family, semantic_key, &fact.event_id)?;
    }

    let previous = transaction
        .query_row(
            "SELECT representative.sequence, locator.event_id, representative.semantic_key
             FROM semantic_representative_text AS representative
             JOIN locator_event_text AS locator ON locator.sequence = representative.sequence
             WHERE representative.family = ?1 AND representative.semantic_key_hash = ?2",
            params![family, semantic_key_digest(semantic_key).as_slice()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| locator_sqlite_error("read semantic representative", error))?;
    if let Some((_, _, observed_key)) = &previous
        && observed_key != semantic_key
    {
        return Err(SqliteLocatorError::Delta(
            "semantic representative digest resolves to a different key".to_owned(),
        ));
    }
    let replace = previous
        .as_ref()
        .is_none_or(|(_, event_id, _)| fact.event_id < *event_id);
    if !replace {
        return Ok(());
    }

    let mut affected_requests = BTreeSet::new();
    if family == "request" {
        affected_requests.insert(semantic_key.to_owned());
    } else if family == "response" {
        if let Some((sequence, _, _)) = &previous
            && let Some(request_id) = response_request_id(transaction, *sequence)?
        {
            affected_requests.insert(request_id);
        }
        let SemanticFactKind::InputRequestResponded(response) = &fact.kind else {
            return Err(SqliteLocatorError::Delta(
                "response representative has the wrong semantic kind".to_owned(),
            ));
        };
        affected_requests.insert(response.request_id.clone());
    }
    let before_request_states = affected_requests
        .iter()
        .map(|request_id| {
            Ok((
                request_id.clone(),
                request_projection_state(transaction, request_id)?,
            ))
        })
        .collect::<Result<Vec<_>, SqliteLocatorError>>()?;

    let inserted = previous.is_none();
    let encoded_key = encode_identity(transaction, Some(semantic_key))?;
    transaction
        .execute(
            "INSERT INTO semantic_representative
             (family, semantic_key_prefix_id, semantic_key_digest, semantic_key_raw,
              semantic_key_hash, sequence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(family, semantic_key_hash) DO UPDATE SET
                 semantic_key_prefix_id = excluded.semantic_key_prefix_id,
                 semantic_key_digest = excluded.semantic_key_digest,
                 semantic_key_raw = excluded.semantic_key_raw,
                 sequence = excluded.sequence",
            params![
                family,
                encoded_key.prefix_id,
                encoded_key.digest.as_deref(),
                encoded_key.raw,
                semantic_key_digest(semantic_key).as_slice(),
                to_i64_locator(fact.cursor.sequence, "representative sequence")?,
            ],
        )
        .map_err(|error| locator_sqlite_error("upsert semantic representative", error))?;

    if inserted {
        increment_materialized_family_count(transaction, family, fact)?;
    } else if family == "revision" {
        let SemanticFactKind::Revision(revision) = &fact.kind else {
            return Err(SqliteLocatorError::Delta(
                "revision representative has the wrong semantic kind".to_owned(),
            ));
        };
        transaction
            .execute(
                "UPDATE semantic_state_projection
                 SET current_object_id = CASE
                     WHEN revision_count = 1 AND current_revision_id = ?1 THEN ?2
                     ELSE current_object_id
                 END
                 WHERE singleton = 1",
                params![semantic_key, revision.object_id],
            )
            .map_err(|error| {
                locator_sqlite_error("replace current revision materialization", error)
            })?;
    }

    for (request_id, before) in before_request_states {
        let after = request_projection_state(transaction, &request_id)?;
        adjust_request_state_counts(transaction, before, after)?;
    }
    Ok(())
}

fn materialized_identity(fact: &SemanticFact) -> Option<(&'static str, &str)> {
    match &fact.kind {
        SemanticFactKind::Revision(_) => fact.revision_id.as_deref().map(|key| ("revision", key)),
        SemanticFactKind::Observation => {
            fact.semantic_id.as_deref().map(|key| ("observation", key))
        }
        SemanticFactKind::Assessment(_) => {
            fact.semantic_id.as_deref().map(|key| ("assessment", key))
        }
        SemanticFactKind::InputRequestOpened(_) => {
            fact.semantic_id.as_deref().map(|key| ("request", key))
        }
        SemanticFactKind::InputRequestResponded(_) => {
            fact.semantic_id.as_deref().map(|key| ("response", key))
        }
        SemanticFactKind::Validation(_) => {
            fact.semantic_id.as_deref().map(|key| ("validation", key))
        }
        SemanticFactKind::CommitAssociated(_) => fact
            .semantic_id
            .as_deref()
            .map(|key| ("commit_association", key)),
        SemanticFactKind::CommitWithdrawn(_) => fact
            .semantic_id
            .as_deref()
            .map(|key| ("commit_withdrawal", key)),
        SemanticFactKind::RefAssociated(_) => fact
            .semantic_id
            .as_deref()
            .map(|key| ("ref_association", key)),
        SemanticFactKind::RefWithdrawn(_) => fact
            .semantic_id
            .as_deref()
            .map(|key| ("ref_withdrawal", key)),
        SemanticFactKind::ArtifactRemoved => {
            fact.content_hash.as_deref().map(|key| ("removal", key))
        }
        SemanticFactKind::Other => None,
    }
}

fn duplicate_family(family: &str) -> bool {
    matches!(
        family,
        "observation" | "assessment" | "request" | "response" | "validation"
    )
}

fn update_materialized_duplicate(
    transaction: &Transaction<'_>,
    family: &str,
    semantic_key: &str,
    event_id: &str,
) -> Result<(), SqliteLocatorError> {
    let current = transaction
        .query_row(
            "SELECT event_count, event_ids_json
             FROM semantic_duplicate_projection
             WHERE family = ?1 AND semantic_key = ?2",
            params![family, semantic_key],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| locator_sqlite_error("read semantic duplicate row", error))?;
    let (event_count, mut event_ids) = match current {
        Some((count, ids)) => (
            count + 1,
            decode_string_list(&ids)
                .map_err(|error| SqliteLocatorError::Delta(error.to_string()))?,
        ),
        None => {
            let representative = transaction
                .query_row(
                    "SELECT locator.event_id, representative.semantic_key
                     FROM semantic_representative_text AS representative
                     JOIN locator_event_text AS locator
                       ON locator.sequence = representative.sequence
                     WHERE representative.family = ?1
                       AND representative.semantic_key_hash = ?2",
                    params![family, semantic_key_digest(semantic_key).as_slice()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()
                .map_err(|error| {
                    locator_sqlite_error("read first semantic duplicate representative", error)
                })?;
            let Some((representative, observed_key)) = representative else {
                return Ok(());
            };
            if observed_key != semantic_key {
                return Err(SqliteLocatorError::Delta(
                    "semantic duplicate digest resolves to a different key".to_owned(),
                ));
            }
            (2, vec![representative])
        }
    };
    event_ids.push(event_id.to_owned());
    event_ids.sort();
    event_ids.dedup();
    event_ids.truncate(5);
    transaction
        .execute(
            "INSERT INTO semantic_duplicate_projection
             (family, semantic_key, event_count, event_ids_json)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(family, semantic_key) DO UPDATE SET
                 event_count = excluded.event_count,
                 event_ids_json = excluded.event_ids_json",
            params![family, semantic_key, event_count, list_text(&event_ids)?],
        )
        .map_err(|error| locator_sqlite_error("upsert semantic duplicate row", error))?;
    Ok(())
}

fn increment_materialized_family_count(
    transaction: &Transaction<'_>,
    family: &str,
    fact: &SemanticFact,
) -> Result<(), SqliteLocatorError> {
    match family {
        "revision" => {
            let SemanticFactKind::Revision(revision) = &fact.kind else {
                return Err(SqliteLocatorError::Delta(
                    "revision representative has the wrong semantic kind".to_owned(),
                ));
            };
            transaction
                .execute(
                    "UPDATE semantic_state_projection
                     SET revision_count = revision_count + 1,
                         current_revision_id = CASE
                             WHEN revision_count = 0 THEN ?1
                             ELSE NULL
                         END,
                         current_object_id = CASE
                             WHEN revision_count = 0 THEN ?2
                             ELSE NULL
                         END
                     WHERE singleton = 1",
                    params![fact.revision_id, revision.object_id],
                )
                .map_err(|error| {
                    locator_sqlite_error("increment revision projection count", error)
                })?;
        }
        "observation" => increment_state_column(transaction, "observation_count")?,
        "assessment" => increment_state_column(transaction, "assessment_count")?,
        "validation" => increment_state_column(transaction, "validation_check_count")?,
        "request" => increment_state_column(transaction, "input_request_count")?,
        _ => {}
    }
    Ok(())
}

fn increment_state_column(
    transaction: &Transaction<'_>,
    column: &'static str,
) -> Result<(), SqliteLocatorError> {
    let sql = match column {
        "observation_count" => {
            "UPDATE semantic_state_projection
             SET observation_count = observation_count + 1 WHERE singleton = 1"
        }
        "assessment_count" => {
            "UPDATE semantic_state_projection
             SET assessment_count = assessment_count + 1 WHERE singleton = 1"
        }
        "validation_check_count" => {
            "UPDATE semantic_state_projection
             SET validation_check_count = validation_check_count + 1 WHERE singleton = 1"
        }
        "input_request_count" => {
            "UPDATE semantic_state_projection
             SET input_request_count = input_request_count + 1 WHERE singleton = 1"
        }
        _ => {
            return Err(SqliteLocatorError::Delta(
                "unsupported materialized state counter".to_owned(),
            ));
        }
    };
    transaction
        .execute(sql, [])
        .map_err(|error| locator_sqlite_error("increment materialized state counter", error))?;
    Ok(())
}

fn response_request_id(
    transaction: &Transaction<'_>,
    sequence: i64,
) -> Result<Option<String>, SqliteLocatorError> {
    transaction
        .query_row(
            "SELECT request_id FROM semantic_response_fact WHERE sequence = ?1",
            [sequence],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| locator_sqlite_error("read response representative target", error))
}

fn request_projection_state(
    transaction: &Transaction<'_>,
    request_id: &str,
) -> Result<(bool, bool), SqliteLocatorError> {
    let mode = transaction
        .query_row(
            "SELECT event.assertion_mode, representative.semantic_key
             FROM semantic_representative_text AS representative
             JOIN semantic_event_fact_text AS event
               ON event.sequence = representative.sequence
             WHERE representative.family = 'request'
               AND representative.semantic_key_hash = ?1",
            [semantic_key_digest(request_id).as_slice()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| locator_sqlite_error("read request projection state", error))?;
    let Some((mode, observed_key)) = mode else {
        return Ok((false, false));
    };
    if observed_key != request_id {
        return Err(SqliteLocatorError::Delta(
            "request representative digest resolves to a different key".to_owned(),
        ));
    }
    let responded = transaction
        .query_row(
            "SELECT 1
             FROM semantic_response_fact AS response
             JOIN semantic_representative AS representative
               ON representative.family = 'response'
              AND representative.sequence = response.sequence
             WHERE response.request_id = ?1
             LIMIT 1",
            [request_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| locator_sqlite_error("read response projection state", error))?
        .is_some();
    let open = !responded;
    Ok((open, open && mode == "operative"))
}

fn adjust_request_state_counts(
    transaction: &Transaction<'_>,
    before: (bool, bool),
    after: (bool, bool),
) -> Result<(), SqliteLocatorError> {
    let open_delta = i64::from(after.0) - i64::from(before.0);
    let operative_delta = i64::from(after.1) - i64::from(before.1);
    transaction
        .execute(
            "UPDATE semantic_state_projection
             SET open_input_request_count = open_input_request_count + ?1,
                 open_operative_input_request_count =
                     open_operative_input_request_count + ?2
             WHERE singleton = 1",
            params![open_delta, operative_delta],
        )
        .map_err(|error| locator_sqlite_error("adjust open request counts", error))?;
    Ok(())
}

fn query_materialized_state(
    connection: &rusqlite::Connection,
) -> Result<SemanticStateSnapshot, SqliteSemanticError> {
    let row = connection
        .query_row(
            "SELECT journal_id, current_revision_id, current_object_id,
                    revision_count, event_count, observation_count, assessment_count,
                    validation_check_count, input_request_count, open_input_request_count,
                    open_operative_input_request_count
             FROM semantic_state_projection WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                ))
            },
        )
        .map_err(|error| sqlite_error("read materialized semantic state", error))?;
    let state = MaterializedSemanticState {
        journal_id: row.0,
        current_revision_id: row.1,
        current_object_id: row.2,
        revision_count: to_usize(row.3, "revision count")?,
        event_count: to_usize(row.4, "event count")?,
        observation_count: to_usize(row.5, "observation count")?,
        assessment_count: to_usize(row.6, "assessment count")?,
        validation_check_count: to_usize(row.7, "validation count")?,
        input_request_count: to_usize(row.8, "input request count")?,
        open_input_request_count: to_usize(row.9, "open input request count")?,
        open_operative_input_request_count: to_usize(row.10, "open operative input request count")?,
    };
    let mut statement = connection
        .prepare(
            "SELECT family, semantic_key, event_count, event_ids_json
             FROM semantic_duplicate_projection
             WHERE event_count >= 2
             ORDER BY family, semantic_key",
        )
        .map_err(|error| sqlite_error("prepare materialized semantic duplicates", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| sqlite_error("query materialized semantic duplicates", error))?;
    let mut duplicates = Vec::new();
    for row in rows {
        let (family, semantic_id, event_count, event_ids) =
            row.map_err(|error| sqlite_error("read materialized semantic duplicate", error))?;
        duplicates.push(MaterializedSemanticDuplicate {
            family,
            semantic_id,
            event_ids: decode_string_list(&event_ids)?,
            event_count: to_usize(event_count, "semantic duplicate count")?,
        });
    }
    Ok(SemanticStateSnapshot::from_materialized(state, &duplicates))
}

fn query_materialized_facts(
    connection: &rusqlite::Connection,
    journal: &QualificationLocalJournal,
    epoch: u64,
    sequence: u64,
    engagement_id: Option<&str>,
) -> Result<Vec<HydratedSemanticFact>, SqliteSemanticError> {
    let mut statement = connection
        .prepare(
            "SELECT locator.epoch, event.sequence, receipt.logical_reread_key_hash,
                    locator.replay_key, locator.event_id, locator.event_type,
                    locator.journal_id, event.revision_id, event.semantic_id,
                    event.content_hash, locator.payload_hash,
                    event.occurred_at, event.assertion_mode,
                    locator.track_id, event.actor_id, receipt.validation_witness,
                    revision.object_id, revision.engagement_id, revision.supersedes_json,
                    revision.base_commit_oid, revision.capture_commit_oid,
                    revision.capture_tree_oid,
                    assessment.assessment, assessment.replaces_json,
                    assessment.related_observations_json,
                    assessment.related_requests_json, assessment.revision_scoped,
                    request.reason_code, request.title,
                    response.request_id,
                    validation.check_name, validation.status, validation.exit_code,
                    validation.completed_at, validation.log_hashes_json,
                    commit_association.commit_oid, commit_association.tree_oid,
                    commit_withdrawal.association_id,
                    ref_association.ref_name, ref_association.head_oid,
                    ref_withdrawal.association_id,
                    receipt.epoch
             FROM semantic_representative AS representative
             JOIN semantic_event_fact_text AS event ON event.sequence = representative.sequence
             JOIN locator_event_text AS locator ON locator.sequence = event.sequence
             JOIN cursor_receipt_text AS receipt ON receipt.sequence = event.sequence
             LEFT JOIN semantic_revision_fact AS revision
               ON revision.sequence = event.sequence
             LEFT JOIN semantic_assessment_fact AS assessment
               ON assessment.sequence = event.sequence
             LEFT JOIN semantic_request_fact AS request
               ON request.sequence = event.sequence
             LEFT JOIN semantic_response_fact AS response
               ON response.sequence = event.sequence
             LEFT JOIN semantic_validation_fact AS validation
               ON validation.sequence = event.sequence
             LEFT JOIN semantic_commit_association_fact AS commit_association
               ON commit_association.sequence = event.sequence
             LEFT JOIN semantic_commit_withdrawal_fact AS commit_withdrawal
               ON commit_withdrawal.sequence = event.sequence
             LEFT JOIN semantic_ref_association_fact AS ref_association
               ON ref_association.sequence = event.sequence
             LEFT JOIN semantic_ref_withdrawal_fact AS ref_withdrawal
               ON ref_withdrawal.sequence = event.sequence
             WHERE representative.family != 'observation'
               AND locator.epoch = ?1 AND event.sequence <= ?2
               AND (
                   ?3 IS NULL
                   OR event.revision_id IN (
                       SELECT selected_event.revision_id
                       FROM semantic_revision_fact AS selected_revision
                       JOIN semantic_event_fact_text AS selected_event
                         ON selected_event.sequence = selected_revision.sequence
                       JOIN locator_event AS selected_locator
                         ON selected_locator.sequence = selected_event.sequence
                       JOIN semantic_representative AS selected_representative
                         ON selected_representative.family = 'revision'
                        AND selected_representative.sequence = selected_event.sequence
                       WHERE selected_revision.engagement_id = ?3
                         AND selected_locator.epoch = ?1
                         AND selected_event.sequence <= ?2
                   )
                   OR (
                       representative.family = 'removal'
                       AND event.content_hash IN (
                           SELECT selected_event.content_hash
                           FROM semantic_revision_fact AS selected_revision
                           JOIN semantic_event_fact_text AS selected_event
                             ON selected_event.sequence = selected_revision.sequence
                           JOIN locator_event AS selected_locator
                             ON selected_locator.sequence = selected_event.sequence
                           JOIN semantic_representative AS selected_representative
                             ON selected_representative.family = 'revision'
                            AND selected_representative.sequence = selected_event.sequence
                           WHERE selected_revision.engagement_id = ?3
                             AND selected_locator.epoch = ?1
                             AND selected_event.sequence <= ?2
                       )
                   )
               )
             ORDER BY locator.replay_key, receipt.logical_reread_key_hash",
        )
        .map_err(|error| sqlite_error("prepare materialized semantic facts", error))?;
    let mut rows = statement
        .query(params![
            to_i64(epoch, "materialized semantic epoch")?,
            to_i64(sequence, "materialized semantic cursor")?,
            engagement_id,
        ])
        .map_err(|error| sqlite_error("query materialized semantic facts", error))?;
    let mut facts = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| sqlite_error("advance materialized semantic facts", error))?
    {
        let mut fact = semantic_fact_from_sql(row)
            .map_err(|error| sqlite_error("read materialized semantic fact", error))?;
        fact.kind = materialized_kind_from_sql(&fact, row)?;
        let receipt_epoch = row
            .get::<_, i64>(41)
            .map_err(|error| sqlite_error("read materialized receipt epoch", error))?;
        if receipt_epoch != to_i64(fact.cursor.epoch, "materialized receipt epoch")? {
            return Err(SqliteSemanticError::Metadata(format!(
                "materialized fact does not match cursor receipt at {:?}",
                fact.cursor
            )));
        }
        facts.push(hydrate_semantic_fact(journal, fact)?);
    }
    Ok(facts)
}

fn materialized_kind_from_sql(
    fact: &SemanticFact,
    row: &rusqlite::Row<'_>,
) -> Result<SemanticFactKind, SqliteSemanticError> {
    match fact.event_type.as_str() {
        "work_object_proposed" => Ok(SemanticFactKind::Revision(RevisionFact {
            object_id: materialized_text(row, 16, "revision object id")?,
            engagement_id: materialized_text(row, 17, "revision engagement id")?,
            supersedes: decode_string_list(&materialized_text(row, 18, "revision supersedes")?)?,
            base_commit_oid: materialized_optional_text(row, 19, "revision base commit")?,
            capture_commit_oid: materialized_optional_text(row, 20, "revision capture commit")?,
            capture_tree_oid: materialized_optional_text(row, 21, "revision capture tree")?,
        })),
        "review_assessment_recorded" => Ok(SemanticFactKind::Assessment(AssessmentFact {
            assessment: decode_enum(&materialized_text(row, 22, "assessment")?)?,
            replaces: decode_string_list(&materialized_text(row, 23, "assessment replacements")?)?,
            related_observations: decode_string_list(&materialized_text(
                row,
                24,
                "assessment observations",
            )?)?,
            related_requests: decode_string_list(&materialized_text(
                row,
                25,
                "assessment requests",
            )?)?,
            revision_scoped: row
                .get::<_, i64>(26)
                .map_err(|error| sqlite_error("read assessment scope", error))?
                != 0,
        })),
        "input_request_opened" => Ok(SemanticFactKind::InputRequestOpened(InputRequestFact {
            reason_code: decode_enum(&materialized_text(row, 27, "request reason code")?)?,
            title: materialized_text(row, 28, "request title")?,
        })),
        "input_request_responded" => {
            Ok(SemanticFactKind::InputRequestResponded(InputResponseFact {
                request_id: materialized_text(row, 29, "response request id")?,
            }))
        }
        "validation_check_recorded" => Ok(SemanticFactKind::Validation(ValidationFact {
            check_name: materialized_text(row, 30, "validation name")?,
            status: decode_enum(&materialized_text(row, 31, "validation status")?)?,
            exit_code: row
                .get(32)
                .map_err(|error| sqlite_error("read validation exit code", error))?,
            completed_at: materialized_optional_text(row, 33, "validation completed at")?,
            log_artifact_content_hashes: decode_string_list(&materialized_text(
                row,
                34,
                "validation log hashes",
            )?)?,
        })),
        "revision_commit_associated" => {
            Ok(SemanticFactKind::CommitAssociated(CommitAssociationFact {
                commit_oid: materialized_text(row, 35, "commit association oid")?,
                tree_oid: materialized_text(row, 36, "commit association tree")?,
            }))
        }
        "revision_commit_withdrawn" => {
            Ok(SemanticFactKind::CommitWithdrawn(CommitWithdrawalFact {
                association_id: materialized_text(row, 37, "commit withdrawal target")?,
            }))
        }
        "revision_ref_associated" => Ok(SemanticFactKind::RefAssociated(RefAssociationFact {
            ref_name: materialized_text(row, 38, "ref association name")?,
            head_oid: materialized_text(row, 39, "ref association head")?,
        })),
        "revision_ref_withdrawn" => Ok(SemanticFactKind::RefWithdrawn(RefWithdrawalFact {
            association_id: materialized_text(row, 40, "ref withdrawal target")?,
        })),
        "artifact_removed" => Ok(SemanticFactKind::ArtifactRemoved),
        _ => Ok(SemanticFactKind::Other),
    }
}

fn materialized_text(
    row: &rusqlite::Row<'_>,
    column: usize,
    label: &'static str,
) -> Result<String, SqliteSemanticError> {
    row.get::<_, Option<String>>(column)
        .map_err(|error| sqlite_error("read materialized family text", error))?
        .ok_or_else(|| SqliteSemanticError::Metadata(format!("missing {label}")))
}

fn materialized_optional_text(
    row: &rusqlite::Row<'_>,
    column: usize,
    label: &'static str,
) -> Result<Option<String>, SqliteSemanticError> {
    row.get(column)
        .map_err(|error| SqliteSemanticError::Metadata(format!("invalid {label}: {error}")))
}

fn selected_semantic_facts(
    identity_predicate: &str,
    index: &str,
    epoch_parameter: usize,
    sequence_parameter: usize,
) -> String {
    format!(
        "SELECT locator.epoch, event.sequence, receipt.logical_reread_key_hash,
                locator.replay_key, locator.event_id, locator.event_type,
                locator.journal_id, event.revision_id, event.semantic_id,
                event.content_hash, locator.payload_hash,
                event.occurred_at, event.assertion_mode,
                locator.track_id, event.actor_id, receipt.validation_witness,
                receipt.epoch
         FROM semantic_event_fact AS physical INDEXED BY {index}
         JOIN semantic_event_fact_text AS event ON event.sequence = physical.sequence
         JOIN locator_event_text AS locator ON locator.sequence = event.sequence
         JOIN cursor_receipt_text AS receipt ON receipt.sequence = event.sequence
         WHERE {identity_predicate}
           AND locator.epoch = ?{epoch_parameter}
           AND event.sequence <= ?{sequence_parameter}
         ORDER BY locator.replay_key, receipt.logical_reread_key_hash"
    )
}

fn selected_content_query(
    identity_predicate: &str,
    epoch_parameter: usize,
    sequence_parameter: usize,
) -> String {
    format!(
        "SELECT 1
         FROM semantic_event_fact AS event INDEXED BY semantic_event_fact_content
         JOIN locator_event_text AS locator ON locator.sequence = event.sequence
         WHERE {identity_predicate}
           AND locator.event_type = 'artifact_removed'
           AND locator.epoch = ?{epoch_parameter}
           AND event.sequence <= ?{sequence_parameter}
         LIMIT 1"
    )
}

fn query_facts(
    connection: &rusqlite::Connection,
    journal: &QualificationLocalJournal,
    sql: &str,
    parameters: impl rusqlite::Params,
) -> Result<Vec<HydratedSemanticFact>, SqliteSemanticError> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| sqlite_error("prepare semantic facts", error))?;
    let rows = statement
        .query_map(parameters, semantic_fact_from_joined_sql)
        .map_err(|error| sqlite_error("query semantic facts", error))?;
    let mut facts = Vec::new();
    for fact in rows {
        let mut fact = fact.map_err(|error| sqlite_error("read semantic fact", error))?;
        fact.kind = query_family_fact(connection, &fact)?;
        facts.push(hydrate_semantic_fact(journal, fact)?);
    }
    Ok(facts)
}

fn hydrate_semantic_fact(
    journal: &QualificationLocalJournal,
    mut stored: SemanticFact,
) -> Result<HydratedSemanticFact, SqliteSemanticError> {
    let bytes = journal
        .read_event_bytes_by_key_digest(&stored.logical_reread_key)
        .map_err(|error| SqliteSemanticError::Metadata(error.to_string()))?
        .ok_or_else(|| {
            SqliteSemanticError::Metadata(format!(
                "semantic carrier is absent for key digest {}",
                stored.logical_reread_key
            ))
        })?;
    if sha256_bytes_hex(&bytes) != stored.validation_witness {
        return Err(SqliteSemanticError::CarrierMismatch(stored.cursor));
    }
    let event = EventStore::decode_qualification_entry(stored.logical_reread_key.clone(), bytes)
        .map_err(|error| SqliteSemanticError::Metadata(error.to_string()))?;
    stored.logical_reread_key = event.idempotency_key.clone();
    let observed =
        SemanticFact::from_event(stored.cursor, &event, stored.validation_witness.clone())?;
    if observed != stored {
        return Err(SqliteSemanticError::CarrierMismatch(stored.cursor));
    }
    Ok(HydratedSemanticFact {
        fact: stored,
        event,
    })
}

fn hydrated_facts_only(facts: Vec<HydratedSemanticFact>) -> Vec<SemanticFact> {
    facts.into_iter().map(|fact| fact.fact).collect()
}

fn semantic_fact_from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<SemanticFact> {
    let epoch = row.get::<_, i64>(0)?;
    let sequence = row.get::<_, i64>(1)?;
    let epoch = u64::try_from(epoch).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })?;
    let sequence = u64::try_from(sequence).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })?;
    let assertion_mode = decode_enum::<crate::session::event::AssertionMode>(
        &row.get::<_, String>(12)?,
    )
    .map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(12, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(SemanticFact {
        cursor: TruthCursor::new(epoch, sequence),
        logical_reread_key: row.get(2)?,
        replay_key: row.get(3)?,
        event_id: row.get(4)?,
        event_type: row.get(5)?,
        journal_id: row.get(6)?,
        revision_id: row.get(7)?,
        semantic_id: row.get(8)?,
        content_hash: row.get(9)?,
        payload_hash: row.get(10)?,
        occurred_at: row.get(11)?,
        assertion_mode,
        track_id: row.get(13)?,
        actor_id: row.get(14)?,
        validation_witness: row.get(15)?,
        kind: SemanticFactKind::Other,
    })
}

fn semantic_fact_from_joined_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<SemanticFact> {
    let fact = semantic_fact_from_sql(row)?;
    let receipt_epoch = row.get::<_, i64>(16)?;
    let receipt_epoch = u64::try_from(receipt_epoch).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            16,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })?;
    if receipt_epoch != fact.cursor.epoch {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(fact)
}

fn query_family_fact(
    connection: &rusqlite::Connection,
    fact: &SemanticFact,
) -> Result<SemanticFactKind, SqliteSemanticError> {
    let sequence = to_i64(fact.cursor.sequence, "semantic family query sequence")?;
    match fact.event_type.as_str() {
        "work_object_proposed" => connection
            .query_row(
                "SELECT object_id, engagement_id, supersedes_json, base_commit_oid,
                        capture_commit_oid, capture_tree_oid
                 FROM semantic_revision_fact WHERE sequence = ?1",
                [sequence],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| sqlite_error("query revision fact", error))?
            .map(|row| {
                Ok(SemanticFactKind::Revision(RevisionFact {
                    object_id: row.0,
                    engagement_id: row.1,
                    supersedes: decode_string_list(&row.2)?,
                    base_commit_oid: row.3,
                    capture_commit_oid: row.4,
                    capture_tree_oid: row.5,
                }))
            })
            .transpose()
            .map(|kind| kind.unwrap_or(SemanticFactKind::Other)),
        "review_observation_recorded" => Ok(SemanticFactKind::Observation),
        "review_assessment_recorded" => {
            let row = connection
                .query_row(
                    "SELECT assessment, replaces_json, related_observations_json,
                            related_requests_json, revision_scoped
                     FROM semantic_assessment_fact WHERE sequence = ?1",
                    [sequence],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    },
                )
                .map_err(|error| sqlite_error("query assessment fact", error))?;
            Ok(SemanticFactKind::Assessment(AssessmentFact {
                assessment: decode_enum(&row.0)?,
                replaces: decode_string_list(&row.1)?,
                related_observations: decode_string_list(&row.2)?,
                related_requests: decode_string_list(&row.3)?,
                revision_scoped: row.4 != 0,
            }))
        }
        "input_request_opened" => {
            let row = connection
                .query_row(
                    "SELECT reason_code, title FROM semantic_request_fact WHERE sequence = ?1",
                    [sequence],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .map_err(|error| sqlite_error("query request fact", error))?;
            Ok(SemanticFactKind::InputRequestOpened(InputRequestFact {
                reason_code: decode_enum(&row.0)?,
                title: row.1,
            }))
        }
        "input_request_responded" => {
            Ok(SemanticFactKind::InputRequestResponded(InputResponseFact {
                request_id: connection
                    .query_row(
                        "SELECT request_id FROM semantic_response_fact WHERE sequence = ?1",
                        [sequence],
                        |row| row.get(0),
                    )
                    .map_err(|error| sqlite_error("query response fact", error))?,
            }))
        }
        "validation_check_recorded" => {
            let row = connection
                .query_row(
                    "SELECT check_name, status, exit_code, completed_at, log_hashes_json
                     FROM semantic_validation_fact WHERE sequence = ?1",
                    [sequence],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    },
                )
                .map_err(|error| sqlite_error("query validation fact", error))?;
            Ok(SemanticFactKind::Validation(ValidationFact {
                check_name: row.0,
                status: decode_enum(&row.1)?,
                exit_code: row.2,
                completed_at: row.3,
                log_artifact_content_hashes: decode_string_list(&row.4)?,
            }))
        }
        "revision_commit_associated" => query_pair(
            connection,
            "SELECT commit_oid, tree_oid FROM semantic_commit_association_fact WHERE sequence = ?1",
            sequence,
            "query commit association fact",
        )
        .map(|pair| {
            pair.map_or(SemanticFactKind::Other, |(commit_oid, tree_oid)| {
                SemanticFactKind::CommitAssociated(CommitAssociationFact {
                    commit_oid,
                    tree_oid,
                })
            })
        }),
        "revision_commit_withdrawn" => {
            Ok(SemanticFactKind::CommitWithdrawn(CommitWithdrawalFact {
                association_id: query_single(
                    connection,
                    "SELECT association_id FROM semantic_commit_withdrawal_fact WHERE sequence = ?1",
                    sequence,
                    "query commit withdrawal fact",
                )?,
            }))
        }
        "revision_ref_associated" => query_pair(
            connection,
            "SELECT ref_name, head_oid FROM semantic_ref_association_fact WHERE sequence = ?1",
            sequence,
            "query ref association fact",
        )
        .map(|pair| {
            pair.map_or(SemanticFactKind::Other, |(ref_name, head_oid)| {
                SemanticFactKind::RefAssociated(RefAssociationFact { ref_name, head_oid })
            })
        }),
        "revision_ref_withdrawn" => Ok(SemanticFactKind::RefWithdrawn(RefWithdrawalFact {
            association_id: query_single(
                connection,
                "SELECT association_id FROM semantic_ref_withdrawal_fact WHERE sequence = ?1",
                sequence,
                "query ref withdrawal fact",
            )?,
        })),
        "artifact_removed" => Ok(SemanticFactKind::ArtifactRemoved),
        _ => Ok(SemanticFactKind::Other),
    }
}

fn query_pair(
    connection: &rusqlite::Connection,
    sql: &str,
    sequence: i64,
    operation: &'static str,
) -> Result<Option<(String, String)>, SqliteSemanticError> {
    connection
        .query_row(sql, [sequence], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()
        .map_err(|error| sqlite_error(operation, error))
}

fn query_single(
    connection: &rusqlite::Connection,
    sql: &str,
    sequence: i64,
    operation: &'static str,
) -> Result<String, SqliteSemanticError> {
    connection
        .query_row(sql, [sequence], |row| row.get(0))
        .map_err(|error| sqlite_error(operation, error))
}

fn enum_text<T: serde::Serialize>(value: T) -> Result<String, SqliteLocatorError> {
    encode_enum(value).map_err(|error| SqliteLocatorError::Delta(error.to_string()))
}

fn list_text(values: &[String]) -> Result<String, SqliteLocatorError> {
    encode_string_list(values).map_err(|error| SqliteLocatorError::Delta(error.to_string()))
}

fn validate_meta(
    connection: &rusqlite::Connection,
    expected: TruthCursor,
) -> Result<(), SqliteSemanticError> {
    let (profile, version, epoch, applied) = connection
        .query_row(
            "SELECT profile_id, schema_version, epoch, applied_sequence
             FROM semantic_meta WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .map_err(|error| sqlite_error("validate semantic metadata", error))?;
    if profile != SEMANTIC_PROFILE_ID
        || version != SEMANTIC_SCHEMA_VERSION
        || epoch != to_i64(expected.epoch, "expected semantic epoch")?
        || applied != to_i64(expected.sequence, "expected semantic applied")?
    {
        return Err(SqliteSemanticError::Metadata(format!(
            "semantic identity/checkpoint {profile}/{version}/{epoch}/{applied} \
             does not match {SEMANTIC_PROFILE_ID}/{SEMANTIC_SCHEMA_VERSION}/{expected:?}"
        )));
    }
    Ok(())
}

fn query_names(
    connection: &rusqlite::Connection,
    sql: &str,
    column: usize,
) -> Result<Vec<String>, SqliteSemanticError> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| sqlite_error("prepare semantic names", error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(column))
        .map_err(|error| sqlite_error("query semantic names", error))?;
    let mut names = Vec::new();
    for row in rows {
        names.push(row.map_err(|error| sqlite_error("read semantic name", error))?);
    }
    names.sort();
    Ok(names)
}

fn sqlite_error(operation: &'static str, error: rusqlite::Error) -> SqliteSemanticError {
    SqliteSemanticError::Sqlite {
        operation,
        message: error.to_string(),
    }
}

fn locator_sqlite_error(operation: &'static str, error: rusqlite::Error) -> SqliteLocatorError {
    SqliteLocatorError::Sqlite {
        operation,
        message: error.to_string(),
    }
}

fn to_i64(value: u64, label: &'static str) -> Result<i64, SqliteSemanticError> {
    i64::try_from(value)
        .map_err(|_| SqliteSemanticError::Metadata(format!("{label} does not fit SQLite INTEGER")))
}

fn to_i64_locator(value: u64, label: &'static str) -> Result<i64, SqliteLocatorError> {
    i64::try_from(value)
        .map_err(|_| SqliteLocatorError::Metadata(format!("{label} does not fit SQLite INTEGER")))
}

fn to_usize(value: i64, label: &'static str) -> Result<usize, SqliteSemanticError> {
    usize::try_from(value)
        .map_err(|_| SqliteSemanticError::Metadata(format!("{label} is negative or too large")))
}
