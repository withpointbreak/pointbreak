//! Bodyless SQLite semantic facts shared by the dormant product profile and qualification.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

use super::locator::{SqliteLocator, SqliteLocatorError, read_locator_checkpoint};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
use crate::model::{
    ActorId, ChangeId, EventId, ReviewTargetRef, RevisionId, RevisionRefV1, TrackId,
};
use crate::session::derived_access::QualificationLocalJournal;
use crate::session::derived_access::cursor::{CursorDelta, TruthCursor};
use crate::session::derived_access::locator::{LocatorRead, LocatorRow};
use crate::session::derived_access::semantic::change::{
    ReaderProjectionCheckpointV1, advance_reader_projection_checkpoint_v1,
};
use crate::session::derived_access::semantic::state::{
    MaterializedSemanticDuplicate, MaterializedSemanticState, SemanticStateSnapshot,
};
use crate::session::derived_access::semantic::{
    AssessmentFact, CommitAssociationFact, CommitWithdrawalFact, InputRequestFact,
    InputResponseFact, MaterializedAttentionSnapshot, RefAssociationFact, RefWithdrawalFact,
    RevisionFact, SemanticFact, SemanticFactKind, SemanticModelError, SemanticSnapshot,
    ValidationFact, decode_enum, decode_string_list, encode_enum, encode_string_list,
};
use crate::session::derived_access::sqlite::cursor::{
    insert_authority_event_identity, recompute_authority_cursor_from_identities,
};
use crate::session::event::{
    ChangeDeclaredPayload, ChangeLinkAssertedPayload, ChangeMembershipAssertedPayload,
    ChangeMembershipWithdrawnPayload, ChangeRevisionRelationAssertedPayload,
    ChangeRevisionRelationWithdrawnPayload, EventSignatureRecordedPayload, EventType,
    InputRequestRespondedPayload, ReviewFactPortedPayload, ReviewObservationRecordedPayload,
    RevisionRelationAttestedPayload, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload,
    decode_input_request_opened_payload,
};
use crate::session::projection::change::{
    ChangeDocumentProjectionFact, ChangeProjectionFact, project_change_documents_from_facts,
    project_changes_from_facts,
};
use crate::session::workflow::{referenced_content_hashes_for_event, tag_completion_key};
use crate::session::{EventStore, parse_event_instant};

const SEMANTIC_PROFILE_ID: &str = "pointbreak.sqlite-derived-access-semantic.v1";
const SEMANTIC_SCHEMA_VERSION: i64 = 8;
const PRODUCT_HISTORY_PROFILE_ID: &str = "pointbreak.sqlite-derived-access-history.v1";
const PRODUCT_HISTORY_SCHEMA_VERSION: i64 = 5;

#[derive(Clone, Debug)]
pub(crate) struct SqliteSemantic {
    locator: SqliteLocator,
}

#[derive(Debug)]
pub(crate) struct HydratedSemanticFact {
    pub(crate) fact: SemanticFact,
    pub(crate) event: ShoreEvent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SemanticInventory {
    pub(crate) profile_id: String,
    pub(crate) schema_version: u32,
    pub(crate) fact_count: u64,
    pub(crate) tables: Vec<String>,
    pub(crate) columns: Vec<String>,
    pub(crate) indexes: Vec<String>,
    pub(crate) proposal_carrier_count: u64,
    pub(crate) proposal_carrier_columns: Vec<String>,
    pub(crate) proposal_carrier_indexes: Vec<String>,
    pub(crate) product_history_profile_id: String,
    pub(crate) product_history_schema_version: u32,
    pub(crate) product_history_event_count: u64,
    pub(crate) product_history_tables: Vec<String>,
    pub(crate) product_history_columns: Vec<String>,
    pub(crate) product_history_indexes: Vec<String>,
    pub(crate) retained_body_object_bytes: u64,
}

/// One bodyless locator for an authoritative Revision proposal carrier.
///
/// Every field is either an exact identity, cursor/order locator, family, or
/// validation witness. Human-authored proposal summaries remain solely in the
/// loose authoritative carrier and are reopened only after selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProposalCarrierLocator {
    pub(crate) cursor: TruthCursor,
    pub(crate) logical_reread_key_hash: String,
    pub(crate) replay_key: String,
    pub(crate) event_id: EventId,
    pub(crate) event_type: String,
    pub(crate) payload_hash: String,
    pub(crate) validation_witness: String,
    pub(crate) revision: RevisionRefV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MaterializedChangeProjection {
    pub(crate) as_of: TruthCursor,
    pub(crate) projection: crate::session::ChangeProjection,
    pub(crate) document_projection: crate::session::ChangeDocumentProjectionV1,
}

/// One exact, connection-local fact read snapshot. The transaction owns the
/// exact locator checkpoint, materialized global diagnostics, exact-Revision
/// selection, and support planning without constructing a Change projection.
pub(crate) struct ExactRevisionFactReadSnapshot {
    pub(crate) connection: rusqlite::Connection,
    pub(crate) state: SemanticStateSnapshot,
}

impl ExactRevisionFactReadSnapshot {
    pub(crate) fn finish(self) -> Result<(), SqliteSemanticError> {
        self.connection
            .execute_batch("ROLLBACK")
            .map_err(|error| sqlite_error("close exact Revision fact read snapshot", error))
    }

    pub(crate) fn exact_revision_event_ids(
        &self,
        revision_id: &RevisionId,
        observed: TruthCursor,
    ) -> Result<Vec<String>, SqliteSemanticError> {
        let checkpoint = read_locator_checkpoint(&self.connection)?;
        if checkpoint.applied != observed {
            return Err(SqliteSemanticError::Metadata(
                "exact Revision fact selection differs from its pinned checkpoint".to_owned(),
            ));
        }
        let (sql, parameters) = exact_revision_event_ids_statement(revision_id, observed)?;
        let mut statement = self
            .connection
            .prepare(&sql)
            .map_err(|error| sqlite_error("prepare exact Revision fact selection", error))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(parameters), |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| sqlite_error("query exact Revision fact selection", error))?;
        let event_ids = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| sqlite_error("read exact Revision fact selection", error))?;
        #[cfg(any(test, feature = "longitudinal-counting"))]
        crate::bench_support::longitudinal::record_fact_sqlite_rows_selected(event_ids.len());
        Ok(event_ids)
    }

    #[cfg(test)]
    pub(crate) fn exact_revision_event_query_plan(
        &self,
        revision_id: &RevisionId,
        observed: TruthCursor,
    ) -> Result<Vec<String>, SqliteSemanticError> {
        let (sql, parameters) = exact_revision_event_ids_statement(revision_id, observed)?;
        let mut statement = self
            .connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .map_err(|error| sqlite_error("prepare exact Revision fact query plan", error))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(parameters), |row| {
                row.get::<_, String>(3)
            })
            .map_err(|error| sqlite_error("query exact Revision fact query plan", error))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| sqlite_error("read exact Revision fact query plan", error))
    }

    #[cfg(test)]
    pub(crate) fn exact_revision_event_vm_steps(
        &self,
        revision_id: &RevisionId,
        observed: TruthCursor,
    ) -> Result<u64, SqliteSemanticError> {
        let (sql, parameters) = exact_revision_event_ids_statement(revision_id, observed)?;
        let mut statement = self
            .connection
            .prepare(&sql)
            .map_err(|error| sqlite_error("prepare exact Revision fact work probe", error))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(parameters), |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| sqlite_error("query exact Revision fact work probe", error))?;
        for row in rows {
            row.map_err(|error| sqlite_error("read exact Revision fact work probe", error))?;
        }
        u64::try_from(statement.get_status(rusqlite::StatementStatus::VmStep)).map_err(|_| {
            SqliteSemanticError::Metadata(
                "exact Revision fact VM-step count does not fit u64".to_owned(),
            )
        })
    }

    /// Select the addressed Revision's complete fork-tolerant
    /// supersession-component semantic carriers on this snapshot's one open
    /// transaction, using the shared fenced component SQL (`CROSS JOIN` order
    /// and `INDEXED BY` planner fences preserved).
    pub(crate) fn revision_component_event_ids(
        &self,
        revision_id: &RevisionId,
        observed: TruthCursor,
    ) -> Result<Vec<String>, SqliteSemanticError> {
        let checkpoint = read_locator_checkpoint(&self.connection)?;
        if checkpoint.applied != observed {
            return Err(SqliteSemanticError::Metadata(
                "component detail selection differs from its pinned checkpoint".to_owned(),
            ));
        }
        let mut statement = self
            .connection
            .prepare(crate::session::derived_access::revisions::REVISION_COMPONENT_EVENT_IDS_SQL)
            .map_err(|error| sqlite_error("prepare component detail selection", error))?;
        let rows = statement
            .query_map(
                params![
                    to_i64(observed.epoch, "component detail epoch")?,
                    to_i64(observed.sequence, "component detail cursor")?,
                    revision_id.as_str(),
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| sqlite_error("query component detail selection", error))?;
        let event_ids = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| sqlite_error("read component detail selection", error))?;
        #[cfg(any(test, feature = "longitudinal-counting"))]
        crate::bench_support::longitudinal::record_fact_sqlite_rows_selected(event_ids.len());
        Ok(event_ids)
    }

    #[cfg(test)]
    pub(crate) fn revision_component_event_query_plan(
        &self,
        revision_id: &RevisionId,
        observed: TruthCursor,
    ) -> Result<Vec<String>, SqliteSemanticError> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "EXPLAIN QUERY PLAN {}",
                crate::session::derived_access::revisions::REVISION_COMPONENT_EVENT_IDS_SQL
            ))
            .map_err(|error| sqlite_error("prepare component detail query plan", error))?;
        let rows = statement
            .query_map(
                params![
                    to_i64(observed.epoch, "component detail epoch")?,
                    to_i64(observed.sequence, "component detail cursor")?,
                    revision_id.as_str(),
                ],
                |row| row.get::<_, String>(3),
            )
            .map_err(|error| sqlite_error("query component detail query plan", error))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| sqlite_error("read component detail query plan", error))
    }

    #[cfg(test)]
    pub(crate) fn revision_component_event_vm_steps(
        &self,
        revision_id: &RevisionId,
        observed: TruthCursor,
    ) -> Result<u64, SqliteSemanticError> {
        let mut statement = self
            .connection
            .prepare(crate::session::derived_access::revisions::REVISION_COMPONENT_EVENT_IDS_SQL)
            .map_err(|error| sqlite_error("prepare component detail work probe", error))?;
        let rows = statement
            .query_map(
                params![
                    to_i64(observed.epoch, "component detail epoch")?,
                    to_i64(observed.sequence, "component detail cursor")?,
                    revision_id.as_str(),
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| sqlite_error("query component detail work probe", error))?;
        for row in rows {
            row.map_err(|error| sqlite_error("read component detail work probe", error))?;
        }
        u64::try_from(statement.get_status(rusqlite::StatementStatus::VmStep)).map_err(|_| {
            SqliteSemanticError::Metadata(
                "component detail VM-step count does not fit u64".to_owned(),
            )
        })
    }

    /// Select the store-wide removal-audit closure: every `ArtifactRemoved`
    /// carrier at `observed`, the detached signatures targeting those
    /// carriers, and every carrier referencing a removed content hash —
    /// binding proposals and externalized note-body carriers alike. Removal
    /// enumeration is seeded from the `semantic_representative`
    /// removal-family primary-key range (one row per removed content hash);
    /// each removed hash then takes one `semantic_event_fact_content` index
    /// seek for its removal carriers — duplicate carriers included
    /// (`semantic_duplicate_projection` never holds removal-family rows, and
    /// its carrier list truncates, so the content seek is the only complete
    /// duplicate-carrier source) — plus one
    /// `product_history_content_reference_lookup` seek for every referencing
    /// carrier, so the legacy target-missing fold applied to the hydrated set
    /// reproduces store-wide truth exactly. The signature leg seeks the
    /// existing target index once per removal carrier. Total work is priced
    /// by administrative-event cardinality, never event-history cardinality.
    pub(crate) fn store_removal_audit_event_ids(
        &self,
        observed: TruthCursor,
    ) -> Result<Vec<String>, SqliteSemanticError> {
        Ok(self.store_removal_audit_event_ids_inner(observed)?.0)
    }

    /// Total VM steps across every statement the removal-audit closure runs.
    /// Used by the boundedness comparison: the total must grow with removal
    /// cardinality, never with unrelated event-history growth.
    #[cfg(test)]
    pub(crate) fn store_removal_audit_vm_steps(
        &self,
        observed: TruthCursor,
    ) -> Result<u64, SqliteSemanticError> {
        Ok(self.store_removal_audit_event_ids_inner(observed)?.1)
    }

    fn store_removal_audit_event_ids_inner(
        &self,
        observed: TruthCursor,
    ) -> Result<(Vec<String>, u64), SqliteSemanticError> {
        let checkpoint = read_locator_checkpoint(&self.connection)?;
        if checkpoint.applied != observed {
            return Err(SqliteSemanticError::Metadata(
                "removal-audit selection differs from its pinned checkpoint".to_owned(),
            ));
        }
        let epoch = to_i64(observed.epoch, "removal-audit epoch")?;
        let sequence = to_i64(observed.sequence, "removal-audit cursor")?;
        let mut vm_steps: u64 = 0;

        let mut removed_hashes = BTreeSet::new();
        {
            let mut representative = self
                .connection
                .prepare(
                    "SELECT coalesce(
                                representative.semantic_key_raw,
                                prefix.value || lower(hex(representative.semantic_key_digest))
                            ) AS semantic_key
                     FROM semantic_representative AS representative
                     LEFT JOIN semantic_identity_prefix AS prefix
                       ON prefix.id = representative.semantic_key_prefix_id
                     WHERE representative.family_id = 11
                       AND representative.sequence <= ?1",
                )
                .map_err(|error| sqlite_error("prepare removal-audit enumeration", error))?;
            let rows = representative
                .query_map(params![sequence], |row| row.get::<_, String>(0))
                .map_err(|error| sqlite_error("query removal-audit enumeration", error))?;
            for row in rows {
                removed_hashes.insert(
                    row.map_err(|error| sqlite_error("read removal-audit enumeration", error))?,
                );
            }
            vm_steps = vm_steps.saturating_add(
                u64::try_from(representative.get_status(rusqlite::StatementStatus::VmStep))
                    .unwrap_or(0),
            );
        }

        let mut removal_event_ids = BTreeSet::new();
        let mut audit_event_ids = BTreeSet::new();
        for removed_hash in &removed_hashes {
            let (predicate, parameters): (&str, Vec<rusqlite::types::Value>) =
                if let Some((prefix, digest)) = split_canonical_digest(removed_hash) {
                    (
                        "event.content_prefix_id = (
                             SELECT id FROM semantic_identity_prefix WHERE value = ?1
                         )
                         AND event.content_digest = ?2
                         AND event.content_raw IS NULL",
                        vec![
                            prefix.to_owned().into(),
                            digest.to_vec().into(),
                            epoch.into(),
                            sequence.into(),
                        ],
                    )
                } else {
                    (
                        "event.content_prefix_id IS NULL
                         AND event.content_digest IS NULL
                         AND event.content_raw = ?1",
                        vec![removed_hash.clone().into(), epoch.into(), sequence.into()],
                    )
                };
            let epoch_parameter = parameters.len() - 1;
            let sequence_parameter = parameters.len();
            // The CROSS JOIN is a deliberate planner fence: the content
            // index seek must run first, or SQLite starts from the bounded-
            // but-large locator range and probes it once per retained event.
            let sql = format!(
                "SELECT locator.event_id, locator.event_type
                 FROM semantic_event_fact AS event INDEXED BY semantic_event_fact_content
                 CROSS JOIN locator_event_text AS locator
                 WHERE {predicate}
                   AND locator.sequence = event.sequence
                   AND locator.epoch = ?{epoch_parameter}
                   AND event.sequence <= ?{sequence_parameter}"
            );
            let mut referencing = self
                .connection
                .prepare(&sql)
                .map_err(|error| sqlite_error("prepare removal-audit content seek", error))?;
            let rows = referencing
                .query_map(rusqlite::params_from_iter(parameters), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| sqlite_error("query removal-audit content seek", error))?;
            for row in rows {
                let (event_id, event_type) =
                    row.map_err(|error| sqlite_error("read removal-audit content seek", error))?;
                if event_type.as_str() == "artifact_removed" {
                    removal_event_ids.insert(event_id.clone());
                    audit_event_ids.insert(event_id);
                }
            }
            vm_steps = vm_steps.saturating_add(
                u64::try_from(referencing.get_status(rusqlite::StatementStatus::VmStep))
                    .unwrap_or(0),
            );
            let (reference_carriers, reference_steps) =
                self.removal_audit_reference_carriers(removed_hash, epoch, sequence)?;
            audit_event_ids.extend(reference_carriers);
            vm_steps = vm_steps.saturating_add(reference_steps);
        }

        {
            let mut signatures = self
                .connection
                .prepare(
                    // The CROSS JOIN fences the target-index seek ahead of
                    // the locator join, mirroring the content-seek fence.
                    "SELECT locator.event_id
                     FROM product_history_signature AS signature
                       INDEXED BY product_history_signature_target
                     CROSS JOIN locator_event_text AS locator
                     WHERE signature.target_event_id = ?1
                       AND locator.sequence = signature.sequence
                       AND locator.epoch = ?2
                       AND signature.sequence <= ?3",
                )
                .map_err(|error| sqlite_error("prepare removal-audit signatures", error))?;
            for removal_event_id in &removal_event_ids {
                let rows = signatures
                    .query_map(params![removal_event_id, epoch, sequence], |row| {
                        row.get::<_, String>(0)
                    })
                    .map_err(|error| sqlite_error("query removal-audit signatures", error))?;
                for row in rows {
                    audit_event_ids.insert(
                        row.map_err(|error| sqlite_error("read removal-audit signatures", error))?,
                    );
                }
            }
            vm_steps = vm_steps.saturating_add(
                u64::try_from(signatures.get_status(rusqlite::StatementStatus::VmStep))
                    .unwrap_or(0),
            );
        }

        let event_ids: Vec<String> = audit_event_ids.into_iter().collect();
        #[cfg(any(test, feature = "longitudinal-counting"))]
        crate::bench_support::longitudinal::record_fact_sqlite_rows_selected(event_ids.len());
        Ok((event_ids, vm_steps))
    }

    /// Every carrier referencing `removed_hash` through the persisted
    /// content-reference index, bounded by the pinned cursor. One indexed
    /// seek per removed hash, so the closure stays priced by removal
    /// cardinality.
    fn removal_audit_reference_carriers(
        &self,
        removed_hash: &str,
        epoch: i64,
        sequence: i64,
    ) -> Result<(Vec<String>, u64), SqliteSemanticError> {
        let (sql, parameters): (String, Vec<rusqlite::types::Value>) =
            if let Some((prefix, digest)) = split_canonical_digest(removed_hash) {
                (
                    removal_audit_reference_seek_sql(false),
                    vec![
                        prefix.to_owned().into(),
                        digest.to_vec().into(),
                        epoch.into(),
                        sequence.into(),
                    ],
                )
            } else {
                (
                    removal_audit_reference_seek_sql(true),
                    vec![
                        removed_hash.to_owned().into(),
                        epoch.into(),
                        sequence.into(),
                    ],
                )
            };
        let mut statement = self
            .connection
            .prepare(&sql)
            .map_err(|error| sqlite_error("prepare removal-audit reference seek", error))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(parameters), |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| sqlite_error("query removal-audit reference seek", error))?;
        let mut event_ids = Vec::new();
        for row in rows {
            event_ids.push(
                row.map_err(|error| sqlite_error("read removal-audit reference seek", error))?,
            );
        }
        let vm_steps =
            u64::try_from(statement.get_status(rusqlite::StatementStatus::VmStep)).unwrap_or(0);
        Ok((event_ids, vm_steps))
    }
}

/// One exact, connection-local per-Change seek read snapshot. The transaction
/// owns the exact locator checkpoint while the correlated-sequence selection
/// and the fenced fact batch run, so a governed append can never rewrite
/// correlation rows between the two statements. The constructor validates the
/// materialized state against the pinned checkpoint but does not retain it:
/// the seek reads fact rows only.
pub(crate) struct ChangeSeekReadSnapshot {
    pub(crate) connection: rusqlite::Connection,
}

impl ChangeSeekReadSnapshot {
    pub(crate) fn finish(self) -> Result<(), SqliteSemanticError> {
        self.connection
            .execute_batch("ROLLBACK")
            .map_err(|error| sqlite_error("close Change seek read snapshot", error))
    }

    /// Select the one Change's correlated fact rows: the correlated-sequence
    /// seek fills a connection-local TEMP set, then the fenced batch returns
    /// an ordered subset of the eager complete-Change scan's rows. Both
    /// statements run on this snapshot's one open transaction.
    pub(crate) fn change_document_facts_for_change(
        &self,
        change_id: &ChangeId,
        observed: TruthCursor,
    ) -> Result<Vec<ChangeDocumentProjectionFact>, SqliteSemanticError> {
        let checkpoint = read_locator_checkpoint(&self.connection)?;
        if checkpoint.applied != observed {
            return Err(SqliteSemanticError::Metadata(
                "Change seek selection differs from its pinned checkpoint".to_owned(),
            ));
        }
        let epoch = to_i64(observed.epoch, "Change seek epoch")?;
        let sequence = to_i64(observed.sequence, "Change seek cursor")?;

        self.connection
            .execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS pointbreak_change_seek_sequence (
                     sequence INTEGER NOT NULL PRIMARY KEY
                 ) STRICT, WITHOUT ROWID;
                 DELETE FROM temp.pointbreak_change_seek_sequence;",
            )
            .map_err(|error| sqlite_error("prepare Change seek sequence set", error))?;
        let correlated = {
            let mut statement = self
                .connection
                .prepare(CHANGE_CORRELATED_SEQUENCE_SEEK_SQL)
                .map_err(|error| sqlite_error("prepare Change seek correlation", error))?;
            let rows = statement
                .query_map(params![epoch, sequence, change_id.as_str()], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(|error| sqlite_error("query Change seek correlation", error))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|error| sqlite_error("read Change seek correlation", error))?
        };
        {
            let mut insert = self
                .connection
                .prepare(
                    "INSERT INTO temp.pointbreak_change_seek_sequence (sequence)
                     VALUES (?1)",
                )
                .map_err(|error| sqlite_error("prepare Change seek sequence member", error))?;
            for selected in &correlated {
                insert
                    .execute(params![selected])
                    .map_err(|error| sqlite_error("insert Change seek sequence member", error))?;
            }
        }

        let mut statement = self
            .connection
            .prepare(CHANGE_FACT_SEEK_BATCH_SQL)
            .map_err(|error| sqlite_error("prepare Change seek fact batch", error))?;
        let rows = statement
            .query_map(params![epoch, sequence], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(|error| sqlite_error("query Change seek fact batch", error))?;
        let mut facts = Vec::new();
        for row in rows {
            let (json, event_id, actor_id, track_id, locator_epoch, receipt_epoch) =
                row.map_err(|error| sqlite_error("read Change seek fact batch", error))?;
            if locator_epoch != epoch || receipt_epoch != locator_epoch {
                return Err(SqliteSemanticError::Metadata(format!(
                    "Change seek fact {event_id} receipt epoch {receipt_epoch} does not match \
                     locator epoch {locator_epoch} at expected epoch {}",
                    observed.epoch
                )));
            }
            facts.push(ChangeDocumentProjectionFact::new(
                serde_json::from_str::<ChangeProjectionFact>(&json)
                    .map_err(|error| SqliteSemanticError::Model(SemanticModelError::Json(error)))?,
                EventId::new(event_id),
                ActorId::new(actor_id),
                track_id.map(TrackId::new),
            ));
        }
        #[cfg(any(test, feature = "longitudinal-counting"))]
        crate::bench_support::longitudinal::record_change_seek_fact_rows_selected(facts.len());
        Ok(facts)
    }
}

/// The removal-audit reference seek statement, in the two-branch identity
/// form. The EXPLAIN-plan pin runs exactly this builder's output, so the
/// tested and production SQL cannot drift. The CROSS JOIN is the same
/// planner fence as the content seek: the reference-index seek must run
/// first, or SQLite starts from the bounded-but-large locator range.
pub(crate) fn removal_audit_reference_seek_sql(raw: bool) -> String {
    let (predicate, epoch_parameter, sequence_parameter) = if raw {
        (
            "reference.content_prefix_id IS NULL
               AND reference.content_digest IS NULL
               AND reference.content_raw = ?1",
            2,
            3,
        )
    } else {
        (
            "reference.content_prefix_id = (
                   SELECT id FROM semantic_identity_prefix WHERE value = ?1
               )
               AND reference.content_digest = ?2
               AND reference.content_raw IS NULL",
            3,
            4,
        )
    };
    format!(
        "SELECT locator.event_id
         FROM product_history_content_reference AS reference
           INDEXED BY product_history_content_reference_lookup
         CROSS JOIN locator_event_text AS locator
         WHERE {predicate}
           AND locator.sequence = reference.sequence
           AND locator.epoch = ?{epoch_parameter}
           AND reference.sequence <= ?{sequence_parameter}"
    )
}

fn exact_revision_event_ids_statement(
    revision_id: &RevisionId,
    observed: TruthCursor,
) -> Result<(String, Vec<rusqlite::types::Value>), SqliteSemanticError> {
    let epoch = to_i64(observed.epoch, "exact Revision fact epoch")?;
    let sequence = to_i64(observed.sequence, "exact Revision fact cursor")?;
    let (predicate, parameters): (&str, Vec<rusqlite::types::Value>) =
        if let Some((prefix, digest)) = split_canonical_digest(revision_id.as_str()) {
            (
                "physical.revision_prefix_id = (
                     SELECT id FROM semantic_identity_prefix WHERE value = ?1
                 )
                 AND physical.revision_digest = ?2
                 AND physical.revision_raw IS NULL",
                vec![
                    prefix.to_owned().into(),
                    digest.to_vec().into(),
                    epoch.into(),
                    sequence.into(),
                ],
            )
        } else {
            (
                "physical.revision_prefix_id IS NULL
                 AND physical.revision_digest IS NULL
                 AND physical.revision_raw = ?1",
                vec![
                    revision_id.as_str().to_owned().into(),
                    epoch.into(),
                    sequence.into(),
                ],
            )
        };
    let epoch_parameter = parameters.len() - 1;
    let sequence_parameter = parameters.len();
    Ok((
        exact_revision_event_ids_query(predicate, epoch_parameter, sequence_parameter),
        parameters,
    ))
}

fn exact_revision_event_ids_query(
    identity_predicate: &str,
    epoch_parameter: usize,
    sequence_parameter: usize,
) -> String {
    format!(
        "SELECT locator.event_id
         FROM semantic_event_fact AS physical
           INDEXED BY semantic_event_fact_revision
         CROSS JOIN locator_event_text AS locator
         WHERE {identity_predicate}
           AND locator.sequence = physical.sequence
           AND locator.epoch = ?{epoch_parameter}
           AND physical.sequence <= ?{sequence_parameter}
         ORDER BY locator.replay_key, locator.event_id"
    )
}

/// One exact, connection-local product-history read snapshot. The main
/// database transaction remains open while selection metadata and TEMP support
/// closure are read, preventing a K response from observing retroactive K+1
/// candidate/correlation rewrites.
pub(crate) struct ProductHistoryReadSnapshot {
    pub(crate) connection: rusqlite::Connection,
    pub(crate) state: SemanticStateSnapshot,
    pub(crate) changes: MaterializedChangeProjection,
}

impl ProductHistoryReadSnapshot {
    pub(crate) fn finish(self) -> Result<(), SqliteSemanticError> {
        self.connection
            .execute_batch("ROLLBACK")
            .map_err(|error| sqlite_error("close product history read snapshot", error))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProductHistoryFact {
    pub(crate) sequence: u64,
    pub(crate) tag_keys: Vec<String>,
    pub(crate) tag_values: Vec<String>,
    pub(crate) signature_target_event_id: Option<String>,
    pub(crate) revision: Option<ProductRevisionFact>,
    pub(crate) timeline: Option<ProductTimelineFact>,
    pub(crate) membership_claim: Option<ProductMembershipClaimFact>,
    pub(crate) membership_withdrawal_claim_id: Option<String>,
    pub(crate) relation_claim: Option<ProductRelationClaimFact>,
    pub(crate) relation_withdrawal_claim_id: Option<String>,
    pub(crate) content_references: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProductRevisionFact {
    pub(crate) revision_id: String,
    pub(crate) captured_at: String,
    pub(crate) captured_at_millis: i64,
    pub(crate) supersedes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProductTimelineFact {
    pub(crate) request_state: Option<&'static str>,
    pub(crate) revision_references: Vec<ProductRevisionReferenceFact>,
    pub(crate) direct_changes: Vec<ProductDirectChangeFact>,
}

impl ProductTimelineFact {
    fn new() -> Self {
        Self {
            request_state: None,
            revision_references: Vec::new(),
            direct_changes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProductRevisionReferenceFact {
    pub(crate) source_kind: &'static str,
    pub(crate) reference_role: &'static str,
    pub(crate) revision_id: String,
    pub(crate) object_artifact_content_hash: Option<String>,
    pub(crate) historical_change_eligible: bool,
}

impl ProductRevisionReferenceFact {
    fn candidate(
        source_kind: &'static str,
        revision_id: &RevisionId,
        historical_change_eligible: bool,
    ) -> Self {
        Self {
            source_kind,
            reference_role: "candidate",
            revision_id: revision_id.as_str().to_owned(),
            object_artifact_content_hash: None,
            historical_change_eligible,
        }
    }

    fn direct(
        source_kind: &'static str,
        reference: &RevisionRefV1,
        historical_change_eligible: bool,
    ) -> Self {
        Self {
            source_kind,
            reference_role: "direct",
            revision_id: reference.revision_id.as_str().to_owned(),
            object_artifact_content_hash: Some(reference.object_artifact_content_hash.clone()),
            historical_change_eligible,
        }
    }

    fn direct_parts(
        source_kind: &'static str,
        revision_id: &RevisionId,
        object_artifact_content_hash: &str,
        historical_change_eligible: bool,
    ) -> Self {
        let exact =
            RevisionRefV1::new(revision_id.clone(), object_artifact_content_hash.to_owned()).ok();
        Self {
            source_kind,
            reference_role: "direct",
            revision_id: revision_id.as_str().to_owned(),
            object_artifact_content_hash: exact
                .map(|reference| reference.object_artifact_content_hash),
            historical_change_eligible,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProductDirectChangeFact {
    pub(crate) change_id: String,
    pub(crate) source_kind: &'static str,
    pub(crate) source_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProductMembershipClaimFact {
    pub(crate) claim_id: String,
    pub(crate) change_id: String,
    pub(crate) revision_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProductRelationClaimFact {
    pub(crate) claim_id: String,
    pub(crate) change_id: String,
    pub(crate) successor: RevisionRefV1,
    pub(crate) predecessor: RevisionRefV1,
}

impl ProductHistoryFact {
    pub(crate) fn from_event(
        sequence: u64,
        event: &ShoreEvent,
    ) -> Result<Self, SemanticModelError> {
        let mut tag_keys = Vec::new();
        let mut tag_values = Vec::new();
        let mut signature_target_event_id = None;
        let mut revision = None;
        let mut timeline = None;
        let mut membership_claim = None;
        let mut membership_withdrawal_claim_id = None;
        let mut relation_claim = None;
        let mut relation_withdrawal_claim_id = None;
        match event.event_type {
            EventType::WorkObjectProposed => {
                let payload: WorkObjectProposedPayload =
                    serde_json::from_value(event.payload.clone())?;
                if let WorkObjectProposal::Revision {
                    revision: proposed,
                    object_artifact_content_hash,
                    supersedes,
                    ..
                } = payload.work_object
                {
                    let captured_at_millis =
                        parse_event_instant(&event.occurred_at).ok_or_else(|| {
                            SemanticModelError::InvalidEventInstant(event.occurred_at.clone())
                        })?;
                    revision = Some(ProductRevisionFact {
                        revision_id: proposed.id.as_str().to_owned(),
                        captured_at: event.occurred_at.clone(),
                        captured_at_millis,
                        supersedes: supersedes
                            .iter()
                            .map(|revision| revision.as_str().to_owned())
                            .collect(),
                    });
                    let mut event = ProductTimelineFact::new();
                    event
                        .revision_references
                        .push(ProductRevisionReferenceFact::direct_parts(
                            "proposal",
                            &proposed.id,
                            &object_artifact_content_hash,
                            true,
                        ));
                    event
                        .revision_references
                        .extend(supersedes.iter().map(|revision| {
                            ProductRevisionReferenceFact::candidate("supersedes", revision, true)
                        }));
                    timeline = Some(event);
                }
            }
            EventType::ReviewObservationRecorded => {
                let payload: ReviewObservationRecordedPayload =
                    serde_json::from_value(event.payload.clone())?;
                tag_keys.extend(
                    payload
                        .tags
                        .iter()
                        .filter_map(|tag| tag_completion_key(tag)),
                );
                tag_keys.sort();
                tag_keys.dedup();
                tag_values = payload
                    .tags
                    .into_iter()
                    .map(|tag| tag.to_lowercase())
                    .collect();
                tag_values.sort();
                tag_values.dedup();
                timeline = Some(timeline_for_review_target(&payload.target));
            }
            EventType::EventSignatureRecorded => {
                let payload: EventSignatureRecordedPayload =
                    serde_json::from_value(event.payload.clone())?;
                signature_target_event_id = Some(payload.target_event_id.as_str().to_owned());
            }
            EventType::ReviewAssessmentRecorded
            | EventType::RevisionRefAssociated
            | EventType::RevisionRefWithdrawn
            | EventType::RevisionCommitAssociated
            | EventType::RevisionCommitWithdrawn
            | EventType::ValidationCheckRecorded => {
                if let Some(revision_id) = event.subject_revision_id()? {
                    timeline = Some(timeline_for_revision_candidate(&revision_id, true));
                }
            }
            EventType::InputRequestOpened => {
                let payload = decode_input_request_opened_payload(event.payload.clone())?;
                if payload.task_target.is_none() {
                    let mut event = timeline_for_review_target(&payload.target);
                    event.request_state = Some("open");
                    timeline = Some(event);
                }
            }
            EventType::InputRequestResponded => {
                let payload: InputRequestRespondedPayload =
                    serde_json::from_value(event.payload.clone())?;
                if let Some(revision_id) = payload.revision_id {
                    let mut event = timeline_for_revision_candidate(&revision_id, true);
                    event.request_state = Some("answered");
                    timeline = Some(event);
                }
            }
            EventType::ChangeDeclared => {
                let payload: ChangeDeclaredPayload = serde_json::from_value(event.payload.clone())?;
                payload.validate()?;
                let mut product_event = ProductTimelineFact::new();
                if let crate::model::ChangeIdentityDescriptorV1::RootRevision {
                    revision_id, ..
                } = &payload.identity_descriptor
                {
                    product_event.revision_references.push(
                        ProductRevisionReferenceFact::candidate(
                            "declaration_root",
                            revision_id,
                            false,
                        ),
                    );
                }
                product_event.direct_changes.push(ProductDirectChangeFact {
                    change_id: payload.change_id.as_str().to_owned(),
                    source_kind: "declaration",
                    source_id: payload.declaration_claim_id.as_str().to_owned(),
                });
                timeline = Some(product_event);
            }
            EventType::ChangeMembershipAsserted => {
                let payload: ChangeMembershipAssertedPayload =
                    serde_json::from_value(event.payload.clone())?;
                payload.validate()?;
                let claim_id = payload.membership_claim_id.as_str().to_owned();
                let mut product_event = ProductTimelineFact::new();
                product_event
                    .revision_references
                    .push(ProductRevisionReferenceFact::candidate(
                        "membership_claim",
                        &payload.revision_id,
                        false,
                    ));
                product_event.direct_changes.push(ProductDirectChangeFact {
                    change_id: payload.change_id.as_str().to_owned(),
                    source_kind: "membership_claim",
                    source_id: claim_id.clone(),
                });
                membership_claim = Some(ProductMembershipClaimFact {
                    claim_id,
                    change_id: payload.change_id.as_str().to_owned(),
                    revision_id: payload.revision_id.as_str().to_owned(),
                });
                timeline = Some(product_event);
            }
            EventType::ChangeMembershipWithdrawn => {
                let payload: ChangeMembershipWithdrawnPayload =
                    serde_json::from_value(event.payload.clone())?;
                payload.validate()?;
                membership_withdrawal_claim_id =
                    Some(payload.membership_claim_id.as_str().to_owned());
                timeline = Some(ProductTimelineFact::new());
            }
            EventType::ChangeLinkAsserted => {
                let payload: ChangeLinkAssertedPayload =
                    serde_json::from_value(event.payload.clone())?;
                payload.validate()?;
                let source_id = payload.link_claim_id.as_str().to_owned();
                let mut product_event = ProductTimelineFact::new();
                for change_id in [&payload.left_change_id, &payload.right_change_id] {
                    product_event.direct_changes.push(ProductDirectChangeFact {
                        change_id: change_id.as_str().to_owned(),
                        source_kind: "link_claim",
                        source_id: source_id.clone(),
                    });
                }
                timeline = Some(product_event);
            }
            EventType::ChangeRevisionRelationAsserted => {
                let payload: ChangeRevisionRelationAssertedPayload =
                    serde_json::from_value(event.payload.clone())?;
                payload.validate()?;
                let claim_id = payload.relation_claim_id.as_str().to_owned();
                let mut product_event = ProductTimelineFact::new();
                product_event.revision_references.extend([
                    ProductRevisionReferenceFact::direct(
                        "relation_successor",
                        &payload.successor,
                        false,
                    ),
                    ProductRevisionReferenceFact::direct(
                        "relation_predecessor",
                        &payload.predecessor,
                        false,
                    ),
                ]);
                product_event.direct_changes.push(ProductDirectChangeFact {
                    change_id: payload.change_id.as_str().to_owned(),
                    source_kind: "relation_claim",
                    source_id: claim_id.clone(),
                });
                relation_claim = Some(ProductRelationClaimFact {
                    claim_id,
                    change_id: payload.change_id.as_str().to_owned(),
                    successor: payload.successor,
                    predecessor: payload.predecessor,
                });
                timeline = Some(product_event);
            }
            EventType::ChangeRevisionRelationWithdrawn => {
                let payload: ChangeRevisionRelationWithdrawnPayload =
                    serde_json::from_value(event.payload.clone())?;
                payload.validate()?;
                relation_withdrawal_claim_id = Some(payload.relation_claim_id.as_str().to_owned());
                timeline = Some(ProductTimelineFact::new());
            }
            EventType::RevisionRelationAttested => {
                let payload: RevisionRelationAttestedPayload =
                    serde_json::from_value(event.payload.clone())?;
                payload.validate()?;
                let mut product_event = ProductTimelineFact::new();
                product_event
                    .revision_references
                    .push(ProductRevisionReferenceFact::direct(
                        "attestation",
                        &payload.revision,
                        true,
                    ));
                timeline = Some(product_event);
            }
            EventType::ReviewFactPorted => {
                let payload: ReviewFactPortedPayload =
                    serde_json::from_value(event.payload.clone())?;
                let track_id = event
                    .target
                    .track_id
                    .as_ref()
                    .ok_or(SemanticModelError::MissingField("review fact port track"))?;
                payload.validate_attribution(&event.writer.actor_id, track_id)?;
                let mut product_event = ProductTimelineFact::new();
                product_event.revision_references.extend([
                    ProductRevisionReferenceFact::direct(
                        "fact_port_origin",
                        &payload.origin_revision,
                        true,
                    ),
                    ProductRevisionReferenceFact::direct(
                        "fact_port_target",
                        &payload.target_revision,
                        true,
                    ),
                ]);
                if let Some(change_id) = &payload.context_change_id {
                    product_event.direct_changes.push(ProductDirectChangeFact {
                        change_id: change_id.as_str().to_owned(),
                        source_kind: "fact_port_context",
                        source_id: payload.port_id.as_str().to_owned(),
                    });
                }
                timeline = Some(product_event);
            }
            EventType::ReviewInitialized | EventType::ReviewNoteImported => {
                timeline = Some(ProductTimelineFact::new());
            }
            EventType::TaskCheckpointCaptured
            | EventType::TaskObservationRecorded
            | EventType::ArtifactRemoved => {}
        }
        Ok(Self {
            sequence,
            tag_keys,
            tag_values,
            signature_target_event_id,
            revision,
            timeline,
            membership_claim,
            membership_withdrawal_claim_id,
            relation_claim,
            relation_withdrawal_claim_id,
            content_references: referenced_content_hashes_for_event(event)?,
        })
    }
}

fn timeline_for_review_target(target: &ReviewTargetRef) -> ProductTimelineFact {
    timeline_for_revision_candidate(review_target_revision(target), true)
}

fn timeline_for_revision_candidate(
    revision_id: &RevisionId,
    historical_change_eligible: bool,
) -> ProductTimelineFact {
    let mut event = ProductTimelineFact::new();
    event
        .revision_references
        .push(ProductRevisionReferenceFact::candidate(
            "review_target",
            revision_id,
            historical_change_eligible,
        ));
    event
}

fn review_target_revision(target: &ReviewTargetRef) -> &RevisionId {
    match target {
        ReviewTargetRef::Revision { revision_id }
        | ReviewTargetRef::File { revision_id, .. }
        | ReviewTargetRef::Range { revision_id, .. }
        | ReviewTargetRef::Observation { revision_id, .. }
        | ReviewTargetRef::InputRequest { revision_id, .. }
        | ReviewTargetRef::Assessment { revision_id, .. }
        | ReviewTargetRef::Event { revision_id, .. } => revision_id,
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SqliteSemanticError {
    #[error(transparent)]
    Locator(#[from] SqliteLocatorError),
    #[error(transparent)]
    Model(#[from] SemanticModelError),
    #[error("semantic metadata mismatch: {0}")]
    Metadata(String),
    #[error("derived product history requires rebuild: {0}")]
    ProductHistoryUpgradeRequired(String),
    #[error("semantic projection requires rebuild: {0}")]
    UpgradeRequired(String),
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

/// The batch step of the selected exact-Revision proposal locator read.
///
/// The `CROSS JOIN` order and the `INDEXED BY` clause are deliberate planner
/// fences: the read must advance from the selected TEMP set through the exact
/// proposal index, then point-read the locator and receipt rows by sequence.
/// With ordinary inner joins SQLite is free to begin from the
/// bounded-but-large locator range and, because `INDEXED BY` forbids the
/// proposal table's sequence key, rescan the whole proposal index once per
/// event row — `O(journal * proposals)` for a selected set of any size. Do
/// not rewrite these joins as ordinary inner joins without checking the
/// `EXPLAIN QUERY PLAN` regression on the bundled SQLite version.
pub(crate) const PROPOSAL_CARRIER_LOCATOR_BATCH_SQL: &str =
    "SELECT receipt.epoch, proposal.sequence,
            receipt.logical_reread_key_hash, locator.replay_key,
            locator.event_id, locator.event_type, locator.payload_hash,
            receipt.validation_witness, proposal.revision_id,
            proposal.object_artifact_content_hash
     FROM temp.pointbreak_proposal_exact_lookup AS selected
     CROSS JOIN semantic_revision_proposal_carrier AS proposal
          INDEXED BY semantic_revision_proposal_exact
     CROSS JOIN locator_event_text AS locator
     CROSS JOIN cursor_receipt_text AS receipt
     WHERE proposal.revision_id = selected.revision_id
       AND proposal.object_artifact_content_hash =
           selected.object_artifact_content_hash
       AND locator.sequence = proposal.sequence
       AND receipt.sequence = proposal.sequence
       AND locator.epoch = ?1
       AND proposal.sequence <= ?2
     ORDER BY proposal.sequence";

/// The correlated-sequence step of the per-Change seek read.
///
/// The `INDEXED BY` clause is a deliberate planner fence: the seek must probe
/// the Change-keyed correlation index, whose key leads with `change_id`. The
/// table's `WITHOUT ROWID` primary key leads with `sequence`, so without the
/// fence SQLite may satisfy the predicate with a full correlation scan whose
/// cost grows with total correlated history rather than the one selected
/// Change. Parameter order is shared with the batch step: `?1` epoch (unused
/// here — correlation rows are not epoch-tagged), `?2` sequence bound,
/// `?3` Change id.
pub(crate) const CHANGE_CORRELATED_SEQUENCE_SEEK_SQL: &str = "SELECT DISTINCT correlation.sequence
     FROM product_history_change_correlation AS correlation
          INDEXED BY product_history_change_correlation_change
     WHERE correlation.change_id = ?3
       AND correlation.sequence <= ?2";

/// The fenced batch step of the per-Change seek read.
///
/// The `CROSS JOIN` order fences the plan to advance from the selected TEMP
/// sequence set; every subsequent join is an `INTEGER PRIMARY KEY` point read
/// by sequence, so no `INDEXED BY` belongs on them — forcing a secondary
/// index onto an ordinary point-read join is exactly the shape that inverts a
/// plan into a per-row rescan. The select list and `ORDER BY` are
/// character-identical to the eager complete-Change scan so the returned rows
/// are an ordered subset of the eager scan's rows.
pub(crate) const CHANGE_FACT_SEEK_BATCH_SQL: &str =
    "SELECT change_fact.fact_json, locator.event_id,
            event.actor_id, locator.track_id,
            locator.epoch, receipt.epoch
     FROM temp.pointbreak_change_seek_sequence AS selected
     CROSS JOIN semantic_change_fact AS change_fact
     CROSS JOIN semantic_event_fact_text AS event
     CROSS JOIN locator_event_text AS locator
     CROSS JOIN cursor_receipt_text AS receipt
     WHERE change_fact.sequence = selected.sequence
       AND event.sequence = change_fact.sequence
       AND locator.sequence = change_fact.sequence
       AND receipt.sequence = change_fact.sequence
       AND locator.epoch = ?1
       AND change_fact.sequence <= ?2
     ORDER BY locator.replay_key, receipt.logical_reread_key_hash";

impl SqliteSemantic {
    pub(crate) fn open(locator: SqliteLocator) -> Result<Self, SqliteSemanticError> {
        Self::open_inner(locator, true)
    }

    /// Open an already-published semantic projection without creating or
    /// repairing schema state. Published generations are immutable as a
    /// lifecycle unit even though governed catch-up may append projection rows
    /// through the separate writer path.
    pub(crate) fn open_published(locator: SqliteLocator) -> Result<Self, SqliteSemanticError> {
        Self::open_inner(locator, false)
    }

    fn open_inner(
        locator: SqliteLocator,
        initialize_schema: bool,
    ) -> Result<Self, SqliteSemanticError> {
        let connection = locator.validated_connection()?;
        let locator_checkpoint = read_locator_checkpoint(&connection)?;
        let semantic_schema_exists = connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM sqlite_schema
                     WHERE type = 'table' AND name = 'semantic_meta'
                 )",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|error| sqlite_error("inspect semantic schema", error))?;
        if semantic_schema_exists {
            let schema_version = connection
                .query_row(
                    "SELECT schema_version FROM semantic_meta WHERE singleton = 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|error| sqlite_error("inspect semantic version", error))?;
            if schema_version < SEMANTIC_SCHEMA_VERSION {
                return Err(SqliteSemanticError::UpgradeRequired(format!(
                    "existing semantic schema {schema_version} predates version \
                     {SEMANTIC_SCHEMA_VERSION}"
                )));
            }
            if schema_version > SEMANTIC_SCHEMA_VERSION {
                return Err(SqliteSemanticError::Metadata(format!(
                    "existing semantic schema {schema_version} is newer than version \
                     {SEMANTIC_SCHEMA_VERSION}"
                )));
            }
        }
        let product_history_exists = connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM sqlite_schema
                     WHERE type = 'table' AND name = 'product_history_meta'
                 )",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|error| sqlite_error("inspect product history schema", error))?;
        if !product_history_exists && locator_checkpoint.applied.sequence != 0 {
            return Err(SqliteSemanticError::ProductHistoryUpgradeRequired(format!(
                "existing locator cursor {:?} predates the product history schema",
                locator_checkpoint.applied
            )));
        }
        if product_history_exists {
            let schema_version = connection
                .query_row(
                    "SELECT schema_version FROM product_history_meta WHERE singleton = 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|error| sqlite_error("inspect product history version", error))?;
            if schema_version < PRODUCT_HISTORY_SCHEMA_VERSION {
                return Err(SqliteSemanticError::ProductHistoryUpgradeRequired(format!(
                    "existing product history schema {schema_version} predates version \
                     {PRODUCT_HISTORY_SCHEMA_VERSION}"
                )));
            }
            if schema_version > PRODUCT_HISTORY_SCHEMA_VERSION {
                return Err(SqliteSemanticError::Metadata(format!(
                    "existing product history schema {schema_version} is newer than version \
                     {PRODUCT_HISTORY_SCHEMA_VERSION}"
                )));
            }
        }
        if !initialize_schema {
            if !semantic_schema_exists {
                return Err(SqliteSemanticError::UpgradeRequired(
                    "published semantic schema is absent".to_owned(),
                ));
            }
            if !product_history_exists {
                return Err(SqliteSemanticError::ProductHistoryUpgradeRequired(
                    "published product history schema is absent".to_owned(),
                ));
            }
            validate_meta(&connection, locator_checkpoint.applied)?;
            validate_product_history_meta(&connection, locator_checkpoint.applied)?;
            return Ok(Self { locator });
        }
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS semantic_meta (
                     singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                     profile_id TEXT NOT NULL,
                     schema_version INTEGER NOT NULL CHECK (schema_version = 8),
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
                 CREATE TABLE IF NOT EXISTS semantic_change_fact (
                     sequence INTEGER PRIMARY KEY REFERENCES semantic_event_fact(sequence),
                     fact_json TEXT NOT NULL
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS semantic_revision_proposal_carrier (
                     sequence INTEGER PRIMARY KEY
                         REFERENCES semantic_revision_fact(sequence),
                     revision_id TEXT NOT NULL,
                     object_artifact_content_hash TEXT NOT NULL
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS semantic_revision_proposal_exact
                     ON semantic_revision_proposal_carrier(
                         revision_id, object_artifact_content_hash, sequence
                     );
                 CREATE TABLE IF NOT EXISTS semantic_representative (
                     family_id INTEGER NOT NULL CHECK (family_id BETWEEN 1 AND 12),
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
                     PRIMARY KEY (family_id, semantic_key_hash)
                 ) STRICT, WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS semantic_representative_sequence
                     ON semantic_representative(sequence, family_id);
                 CREATE VIEW IF NOT EXISTS semantic_representative_text AS
                 SELECT representative.family_id,
                        CASE representative.family_id
                            WHEN 1 THEN 'revision'
                            WHEN 2 THEN 'observation'
                            WHEN 3 THEN 'assessment'
                            WHEN 4 THEN 'request'
                            WHEN 5 THEN 'response'
                            WHEN 6 THEN 'validation'
                            WHEN 7 THEN 'commit_association'
                            WHEN 8 THEN 'commit_withdrawal'
                            WHEN 9 THEN 'ref_association'
                            WHEN 10 THEN 'ref_withdrawal'
                            WHEN 11 THEN 'removal'
                            WHEN 12 THEN 'change_record'
                        END AS family,
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
                 JOIN semantic_actor AS actor ON actor.id = event.actor_id;
                 CREATE TABLE IF NOT EXISTS product_history_meta (
                     singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                     profile_id TEXT NOT NULL,
                     schema_version INTEGER NOT NULL CHECK (schema_version = 5),
                     epoch INTEGER NOT NULL CHECK (epoch > 0),
                     applied_sequence INTEGER NOT NULL CHECK (applied_sequence >= 0)
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS product_history_tag (
                     sequence INTEGER NOT NULL REFERENCES semantic_event_fact(sequence),
                     tag_key TEXT NOT NULL,
                     PRIMARY KEY (sequence, tag_key)
                 ) STRICT, WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS product_history_tag_key
                     ON product_history_tag(tag_key, sequence);
                 CREATE TABLE IF NOT EXISTS product_history_tag_value (
                     sequence INTEGER NOT NULL REFERENCES semantic_event_fact(sequence),
                     tag_value TEXT NOT NULL,
                     PRIMARY KEY (sequence, tag_value)
                 ) STRICT, WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS product_history_tag_value_lookup
                     ON product_history_tag_value(tag_value, sequence);
                 CREATE TABLE IF NOT EXISTS product_history_signature (
                     sequence INTEGER PRIMARY KEY REFERENCES semantic_event_fact(sequence),
                     target_event_id TEXT NOT NULL
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS product_history_signature_target
                     ON product_history_signature(target_event_id, sequence);
                 CREATE TABLE IF NOT EXISTS product_revision (
                     sequence INTEGER PRIMARY KEY REFERENCES semantic_event_fact(sequence),
                     revision_id TEXT NOT NULL,
                     captured_at TEXT NOT NULL,
                     captured_at_millis INTEGER NOT NULL
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS product_revision_identity
                     ON product_revision(revision_id, sequence);
                 CREATE INDEX IF NOT EXISTS product_revision_chronological
                     ON product_revision(captured_at_millis DESC, revision_id DESC, sequence);
                 CREATE TABLE IF NOT EXISTS product_revision_edge (
                     sequence INTEGER NOT NULL REFERENCES product_revision(sequence),
                     superseded_revision_id TEXT NOT NULL,
                     PRIMARY KEY (sequence, superseded_revision_id)
                 ) STRICT, WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS product_revision_edge_target
                     ON product_revision_edge(superseded_revision_id, sequence);
                 CREATE TABLE IF NOT EXISTS product_history_event (
                     sequence INTEGER PRIMARY KEY REFERENCES semantic_event_fact(sequence),
                     request_state TEXT CHECK (
                         request_state IS NULL OR request_state IN ('open', 'answered')
                     )
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS product_history_event_request_state
                     ON product_history_event(request_state, sequence)
                     WHERE request_state IS NOT NULL;
                 CREATE TABLE IF NOT EXISTS product_history_revision_reference (
                     sequence INTEGER NOT NULL REFERENCES product_history_event(sequence),
                     source_kind TEXT NOT NULL CHECK (source_kind IN (
                         'proposal',
                         'supersedes',
                         'review_target',
                         'declaration_root',
                         'membership_claim',
                         'relation_successor',
                         'relation_predecessor',
                         'attestation',
                         'fact_port_origin',
                         'fact_port_target'
                     )),
                     reference_role TEXT NOT NULL CHECK (
                         reference_role IN ('direct', 'candidate')
                     ),
                     resolution TEXT NOT NULL CHECK (resolution IN ('exact', 'unresolved')),
                     revision_id TEXT NOT NULL,
                     object_artifact_content_hash TEXT,
                     historical_change_eligible INTEGER NOT NULL CHECK (
                         historical_change_eligible IN (0, 1)
                     ),
                     CHECK (
                         (resolution = 'exact' AND object_artifact_content_hash IS NOT NULL)
                         OR
                         (resolution = 'unresolved' AND object_artifact_content_hash IS NULL)
                     ),
                     PRIMARY KEY (sequence, source_kind, revision_id)
                 ) STRICT, WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS product_history_revision_reference_exact
                     ON product_history_revision_reference(
                         revision_id, object_artifact_content_hash, sequence
                     ) WHERE resolution = 'exact';
                 CREATE INDEX IF NOT EXISTS product_history_revision_reference_unresolved
                     ON product_history_revision_reference(revision_id, sequence)
                     WHERE resolution = 'unresolved';
                 CREATE INDEX IF NOT EXISTS product_history_revision_reference_candidate
                     ON product_history_revision_reference(
                         revision_id, reference_role, sequence
                     ) WHERE reference_role = 'candidate';
                 CREATE INDEX IF NOT EXISTS product_history_revision_reference_historical
                     ON product_history_revision_reference(
                         revision_id, historical_change_eligible, sequence
                     ) WHERE historical_change_eligible = 1;
                 CREATE TABLE IF NOT EXISTS product_history_membership_claim (
                     sequence INTEGER PRIMARY KEY REFERENCES product_history_event(sequence),
                     claim_id TEXT NOT NULL,
                     change_id TEXT NOT NULL,
                     revision_id TEXT NOT NULL
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS product_history_membership_claim_identity
                     ON product_history_membership_claim(claim_id, sequence);
                 CREATE INDEX IF NOT EXISTS product_history_membership_claim_revision
                     ON product_history_membership_claim(revision_id, claim_id, sequence);
                 CREATE TABLE IF NOT EXISTS product_history_membership_withdrawal (
                     sequence INTEGER PRIMARY KEY REFERENCES product_history_event(sequence),
                     claim_id TEXT NOT NULL
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS product_history_membership_withdrawal_claim
                     ON product_history_membership_withdrawal(claim_id, sequence);
                 CREATE TABLE IF NOT EXISTS product_history_relation_claim (
                     sequence INTEGER PRIMARY KEY REFERENCES product_history_event(sequence),
                     claim_id TEXT NOT NULL,
                     change_id TEXT NOT NULL,
                     successor_revision_id TEXT NOT NULL,
                     successor_object_artifact_content_hash TEXT NOT NULL,
                     predecessor_revision_id TEXT NOT NULL,
                     predecessor_object_artifact_content_hash TEXT NOT NULL
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS product_history_relation_claim_identity
                     ON product_history_relation_claim(claim_id, sequence);
                 CREATE INDEX IF NOT EXISTS product_history_relation_claim_successor
                     ON product_history_relation_claim(
                         successor_revision_id, claim_id, sequence
                     );
                 CREATE INDEX IF NOT EXISTS product_history_relation_claim_predecessor
                     ON product_history_relation_claim(
                         predecessor_revision_id, claim_id, sequence
                     );
                 CREATE TABLE IF NOT EXISTS product_history_relation_withdrawal (
                     sequence INTEGER PRIMARY KEY REFERENCES product_history_event(sequence),
                     claim_id TEXT NOT NULL
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS product_history_relation_withdrawal_claim
                     ON product_history_relation_withdrawal(claim_id, sequence);
                 CREATE TABLE IF NOT EXISTS product_history_change_correlation (
                     sequence INTEGER NOT NULL REFERENCES product_history_event(sequence),
                     change_id TEXT NOT NULL,
                     correlation_role TEXT NOT NULL CHECK (
                         correlation_role IN ('direct', 'historical')
                     ),
                     source_kind TEXT NOT NULL CHECK (source_kind IN (
                         'declaration',
                         'membership_claim',
                         'relation_claim',
                         'link_claim',
                         'fact_port_context'
                     )),
                     source_id TEXT NOT NULL,
                     support_sequence INTEGER NOT NULL
                         REFERENCES semantic_event_fact(sequence),
                     PRIMARY KEY (
                         sequence, change_id, correlation_role,
                         source_kind, source_id, support_sequence
                     )
                 ) STRICT, WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS product_history_change_correlation_change
                     ON product_history_change_correlation(change_id, sequence);
                 CREATE INDEX IF NOT EXISTS product_history_change_correlation_support
                     ON product_history_change_correlation(
                         source_kind, source_id, support_sequence, sequence
                     );
                 CREATE TABLE IF NOT EXISTS product_history_content_reference (
                     sequence INTEGER NOT NULL REFERENCES semantic_event_fact(sequence),
                     content_prefix_id INTEGER REFERENCES semantic_identity_prefix(id),
                     content_digest BLOB CHECK (length(content_digest) = 32),
                     content_raw TEXT,
                     content_key_hash BLOB NOT NULL CHECK (length(content_key_hash) = 32),
                     CHECK (
                         (content_prefix_id IS NULL
                          AND content_digest IS NULL
                          AND content_raw IS NOT NULL)
                         OR (content_prefix_id IS NOT NULL
                             AND content_digest IS NOT NULL
                             AND content_raw IS NULL)
                     ),
                     PRIMARY KEY (sequence, content_key_hash)
                 ) STRICT, WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS product_history_content_reference_lookup
                     ON product_history_content_reference(
                         content_prefix_id, content_digest, content_raw, sequence
                     );
                 CREATE TABLE IF NOT EXISTS reader_projection_checkpoint (
                     singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                     checkpoint_json TEXT NOT NULL
                         CHECK (length(checkpoint_json) > 0)
                 ) STRICT;",
            )
            .map_err(|error| sqlite_error("create semantic schema", error))?;
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
        let product_inserted = connection
            .execute(
                "INSERT INTO product_history_meta
                 (singleton, profile_id, schema_version, epoch, applied_sequence)
                 VALUES (1, ?1, ?2, ?3, 0)
                 ON CONFLICT(singleton) DO NOTHING",
                params![
                    PRODUCT_HISTORY_PROFILE_ID,
                    PRODUCT_HISTORY_SCHEMA_VERSION,
                    to_i64(locator_checkpoint.applied.epoch, "product history epoch")?,
                ],
            )
            .map_err(|error| sqlite_error("initialize product history metadata", error))?;
        debug_assert!(product_inserted == 0 || locator_checkpoint.applied.sequence == 0);
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
        validate_product_history_meta(&connection, locator_checkpoint.applied)?;
        Ok(Self { locator })
    }

    /// Seed the publication-anchored live checkpoint while a generation is
    /// still staging. Published opens never call this method.
    pub(crate) fn seed_reader_projection_checkpoint(
        &self,
        checkpoint: &ReaderProjectionCheckpointV1,
    ) -> Result<(), SqliteSemanticError> {
        let checkpoint_json = canonical_checkpoint_json(checkpoint)?;
        let connection = self.locator.validated_connection()?;
        connection
            .execute(
                "INSERT INTO reader_projection_checkpoint (singleton, checkpoint_json)
                 VALUES (1, ?1)",
                [checkpoint_json],
            )
            .map_err(|error| sqlite_error("seed reader projection checkpoint", error))?;
        Ok(())
    }

    pub(crate) fn apply_delta(
        &self,
        delta: &CursorDelta,
        locator_rows: &[LocatorRow],
        semantic_facts: &[SemanticFact],
        product_history_facts: &[ProductHistoryFact],
    ) -> Result<TruthCursor, SqliteSemanticError> {
        self.apply_delta_inner(
            delta,
            locator_rows,
            semantic_facts,
            product_history_facts,
            false,
        )
    }

    pub(crate) fn apply_delta_with_failure(
        &self,
        delta: &CursorDelta,
        locator_rows: &[LocatorRow],
        semantic_facts: &[SemanticFact],
        product_history_facts: &[ProductHistoryFact],
    ) -> Result<TruthCursor, SqliteSemanticError> {
        self.apply_delta_inner(
            delta,
            locator_rows,
            semantic_facts,
            product_history_facts,
            true,
        )
    }

    fn apply_delta_inner(
        &self,
        delta: &CursorDelta,
        locator_rows: &[LocatorRow],
        semantic_facts: &[SemanticFact],
        product_history_facts: &[ProductHistoryFact],
        inject_failure: bool,
    ) -> Result<TruthCursor, SqliteSemanticError> {
        if semantic_facts.len() != delta.receipts.len()
            || semantic_facts.len() != locator_rows.len()
            || semantic_facts.len() != product_history_facts.len()
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
                insert_product_history_facts(transaction, product_history_facts)?;
                if let Some(checkpoint) = read_reader_projection_checkpoint(transaction)
                    .map_err(|error| SqliteLocatorError::Delta(error.to_string()))?
                {
                    for (receipt, fact) in delta.receipts.iter().zip(semantic_facts) {
                        insert_authority_event_identity(
                            transaction,
                            &receipt.logical_reread_key,
                            &receipt.validation_witness,
                            &fact.event_id,
                            &fact.payload_hash,
                        )
                        .map_err(|error| SqliteLocatorError::Delta(error.to_string()))?;
                    }
                    let authority_cursor = recompute_authority_cursor_from_identities(
                        transaction,
                        &checkpoint.authority_cursor.capability_set_hash,
                    )
                    .map_err(|error| SqliteLocatorError::Delta(error.to_string()))?;
                    let advanced = advance_reader_projection_checkpoint_v1(
                        &checkpoint,
                        authority_cursor,
                        applied,
                    )
                    .map_err(|error| SqliteLocatorError::Delta(error.to_string()))?;
                    let previous_json = canonical_checkpoint_json(&checkpoint)
                        .map_err(|error| SqliteLocatorError::Delta(error.to_string()))?;
                    let advanced_json = canonical_checkpoint_json(&advanced)
                        .map_err(|error| SqliteLocatorError::Delta(error.to_string()))?;
                    let updated = transaction
                        .execute(
                            "UPDATE reader_projection_checkpoint
                             SET checkpoint_json = ?1
                             WHERE singleton = 1 AND checkpoint_json = ?2",
                            params![advanced_json, previous_json],
                        )
                        .map_err(|error| {
                            locator_sqlite_error("advance reader projection checkpoint", error)
                        })?;
                    if updated != 1 {
                        return Err(SqliteLocatorError::Delta(
                            "reader projection checkpoint changed concurrently".to_owned(),
                        ));
                    }
                }
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
                let product_updated = transaction
                    .execute(
                        "UPDATE product_history_meta
                         SET applied_sequence = ?1
                         WHERE singleton = 1 AND epoch = ?2 AND applied_sequence = ?3",
                        params![
                            to_i64_locator(applied.sequence, "product history applied")?,
                            to_i64_locator(applied.epoch, "product history epoch")?,
                            to_i64_locator(
                                delta.after.sequence,
                                "product history previous applied"
                            )?,
                        ],
                    )
                    .map_err(|error| {
                        locator_sqlite_error("advance product history metadata", error)
                    })?;
                if product_updated != 1 {
                    return Err(SqliteLocatorError::Delta(
                        "product history checkpoint changed concurrently".to_owned(),
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
            MaterializedFactFamilies::AllExceptObservations,
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

    pub(crate) fn materialized_attention_snapshot(
        &self,
        observed: TruthCursor,
    ) -> Result<LocatorRead<MaterializedAttentionSnapshot>, SqliteSemanticError> {
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
        let facts = query_materialized_compact_facts(
            &connection,
            observed.epoch,
            observed.sequence,
            None,
            MaterializedFactFamilies::Attention,
        )?;
        let supersession =
            crate::session::derived_access::semantic::thread::supersession_from_facts(&facts)?;
        let attention = crate::session::derived_access::semantic::attention::AttentionSemanticSnapshot::from_facts_with_supersession(
            &facts,
            &supersession,
        )?;
        Ok(LocatorRead::Ready(MaterializedAttentionSnapshot {
            as_of: observed,
            state,
            supersession,
            attention,
        }))
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
            MaterializedFactFamilies::AllExceptObservations,
        )?);
        let changes =
            query_materialized_change_projection(&connection, observed.epoch, observed.sequence)?;
        Ok(LocatorRead::Ready(
            SemanticSnapshot::from_materialized_with_changes(observed, state, &facts, changes)?,
        ))
    }

    /// Reconstruct the complete bodyless Change semantic and provenance pair
    /// from every persisted Change fact at one checkpoint.
    ///
    /// This deliberately bypasses `semantic_representative`: duplicate claim
    /// carriers are document provenance even when effective semantic state
    /// deduplicates them.
    pub(crate) fn materialized_change_projection(
        &self,
        observed: TruthCursor,
    ) -> Result<LocatorRead<MaterializedChangeProjection>, SqliteSemanticError> {
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
        let (projection, document_projection) =
            query_materialized_change_projections(&connection, observed.epoch, observed.sequence)?;
        Ok(LocatorRead::Ready(MaterializedChangeProjection {
            as_of: observed,
            projection,
            document_projection,
        }))
    }

    /// Test-only eager complete-Change scan probe: the ordered-subset seek
    /// regression compares the seek's output against exactly this production
    /// row source.
    #[cfg(test)]
    pub(crate) fn materialized_change_document_facts(
        &self,
        observed: TruthCursor,
    ) -> Result<LocatorRead<Vec<ChangeDocumentProjectionFact>>, SqliteSemanticError> {
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
        Ok(LocatorRead::Ready(
            query_materialized_change_document_facts(
                &connection,
                observed.epoch,
                observed.sequence,
            )?,
        ))
    }

    /// Select every proposal carrier for one exact Revision without opening
    /// authoritative bytes. The caller must hydrate and validate each returned
    /// identity before presenting proposal prose.
    pub(crate) fn proposal_carrier_locators(
        &self,
        exact: &RevisionRefV1,
        observed: TruthCursor,
    ) -> Result<LocatorRead<Vec<ProposalCarrierLocator>>, SqliteSemanticError> {
        let selected = BTreeSet::from([exact.clone()]);
        match self.proposal_carrier_locators_for_exact_revisions(&selected, observed)? {
            LocatorRead::Ready(mut grouped) => Ok(LocatorRead::Ready(
                grouped.remove(exact).unwrap_or_default(),
            )),
            LocatorRead::CatchUpRequired { applied, observed } => {
                Ok(LocatorRead::CatchUpRequired { applied, observed })
            }
        }
    }

    /// Select every proposal carrier for a complete selected exact-Revision
    /// set without opening authoritative bytes.
    ///
    /// A connection-local two-column TEMP relation keeps the query portable
    /// beyond SQLite's bind-variable limits. The returned map retains one
    /// entry for every selected exact Revision, including explicit empty
    /// groups, and every duplicate carrier remains independently ordered by
    /// its truth sequence for subsequent authoritative hydration.
    ///
    /// [`PROPOSAL_CARRIER_LOCATOR_BATCH_SQL`] carries the planner fence for
    /// the batch step; see its comment before touching the join shape.
    pub(crate) fn proposal_carrier_locators_for_exact_revisions(
        &self,
        selected: &BTreeSet<RevisionRefV1>,
        observed: TruthCursor,
    ) -> Result<
        LocatorRead<BTreeMap<RevisionRefV1, Vec<ProposalCarrierLocator>>>,
        SqliteSemanticError,
    > {
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
        let mut grouped = selected
            .iter()
            .cloned()
            .map(|exact| (exact, Vec::new()))
            .collect::<BTreeMap<_, _>>();
        if selected.is_empty() {
            return Ok(LocatorRead::Ready(grouped));
        }

        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| sqlite_error("begin proposal carrier locator batch", error))?;
        transaction
            .execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS pointbreak_proposal_exact_lookup (
                     revision_id TEXT NOT NULL,
                     object_artifact_content_hash TEXT NOT NULL,
                     PRIMARY KEY (revision_id, object_artifact_content_hash)
                 ) STRICT, WITHOUT ROWID;
                 DELETE FROM temp.pointbreak_proposal_exact_lookup;",
            )
            .map_err(|error| sqlite_error("prepare proposal exact lookup", error))?;
        {
            let mut insert = transaction
                .prepare(
                    "INSERT INTO temp.pointbreak_proposal_exact_lookup (
                         revision_id, object_artifact_content_hash
                     ) VALUES (?1, ?2)",
                )
                .map_err(|error| sqlite_error("prepare proposal exact lookup member", error))?;
            for exact in selected {
                insert
                    .execute(params![
                        exact.revision_id.as_str(),
                        exact.object_artifact_content_hash
                    ])
                    .map_err(|error| sqlite_error("insert proposal exact lookup member", error))?;
            }
        }
        {
            let mut statement = transaction
                .prepare(PROPOSAL_CARRIER_LOCATOR_BATCH_SQL)
                .map_err(|error| sqlite_error("prepare proposal carrier locator batch", error))?;
            let rows = statement
                .query_map(
                    params![
                        to_i64(observed.epoch, "proposal carrier epoch")?,
                        to_i64(observed.sequence, "proposal carrier sequence")?,
                    ],
                    proposal_carrier_locator_from_sql,
                )
                .map_err(|error| sqlite_error("query proposal carrier locator batch", error))?;
            for row in rows {
                let locator = row
                    .map_err(|error| sqlite_error("read proposal carrier locator batch", error))?;
                if locator.cursor.epoch != observed.epoch {
                    return Err(SqliteSemanticError::Metadata(format!(
                        "proposal carrier {} receipt epoch {} does not match locator epoch {}",
                        locator.event_id.as_str(),
                        locator.cursor.epoch,
                        observed.epoch
                    )));
                }
                if locator.event_type != EventType::WorkObjectProposed.as_str() {
                    return Err(SqliteSemanticError::Metadata(format!(
                        "proposal carrier {} does not match its indexed exact Revision",
                        locator.event_id.as_str()
                    )));
                }
                let exact_group = grouped.get_mut(&locator.revision).ok_or_else(|| {
                    SqliteSemanticError::Metadata(format!(
                        "proposal carrier {} does not match its selected exact Revision",
                        locator.event_id.as_str()
                    ))
                })?;
                exact_group.push(locator);
            }
        }
        transaction
            .commit()
            .map_err(|error| sqlite_error("commit proposal carrier locator batch", error))?;
        Ok(LocatorRead::Ready(grouped))
    }

    pub(crate) fn facts_for_revision_hydrated(
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

    pub(crate) fn product_history_connection(
        &self,
        observed: TruthCursor,
    ) -> Result<LocatorRead<(rusqlite::Connection, SemanticStateSnapshot)>, SqliteSemanticError>
    {
        let connection = self.locator.validated_connection()?;
        let checkpoint = read_locator_checkpoint(&connection)?;
        validate_meta(&connection, checkpoint.applied)?;
        validate_product_history_meta(&connection, checkpoint.applied)?;
        if checkpoint.applied.epoch != observed.epoch
            || checkpoint.applied.sequence < observed.sequence
        {
            return Ok(LocatorRead::CatchUpRequired {
                applied: checkpoint.applied,
                observed,
            });
        }
        let state = query_materialized_state(&connection)?;
        Ok(LocatorRead::Ready((connection, state)))
    }

    pub(crate) fn exact_revision_fact_read_snapshot(
        &self,
        observed: TruthCursor,
    ) -> Result<LocatorRead<ExactRevisionFactReadSnapshot>, SqliteSemanticError> {
        let connection = self.locator.validated_connection()?;
        connection
            .execute_batch("BEGIN DEFERRED TRANSACTION")
            .map_err(|error| sqlite_error("begin exact Revision fact read snapshot", error))?;
        let checkpoint = read_locator_checkpoint(&connection)?;
        validate_meta(&connection, checkpoint.applied)?;
        validate_product_history_meta(&connection, checkpoint.applied)?;
        if checkpoint.applied != observed {
            connection.execute_batch("ROLLBACK").map_err(|error| {
                sqlite_error("close moved exact Revision fact read snapshot", error)
            })?;
            return Ok(LocatorRead::CatchUpRequired {
                applied: checkpoint.applied,
                observed,
            });
        }
        let state = query_materialized_state(&connection)?;
        if u64::try_from(state.event_count).ok() != Some(observed.sequence) {
            connection.execute_batch("ROLLBACK").map_err(|error| {
                sqlite_error(
                    "close inconsistent exact Revision fact read snapshot",
                    error,
                )
            })?;
            return Err(SqliteSemanticError::Metadata(
                "exact Revision fact state differs from its pinned checkpoint".to_owned(),
            ));
        }
        Ok(LocatorRead::Ready(ExactRevisionFactReadSnapshot {
            connection,
            state,
        }))
    }

    pub(crate) fn change_seek_read_snapshot(
        &self,
        observed: TruthCursor,
    ) -> Result<LocatorRead<ChangeSeekReadSnapshot>, SqliteSemanticError> {
        let connection = self.locator.validated_connection()?;
        connection
            .execute_batch("BEGIN DEFERRED TRANSACTION")
            .map_err(|error| sqlite_error("begin Change seek read snapshot", error))?;
        let checkpoint = read_locator_checkpoint(&connection)?;
        validate_meta(&connection, checkpoint.applied)?;
        validate_product_history_meta(&connection, checkpoint.applied)?;
        if checkpoint.applied != observed {
            connection
                .execute_batch("ROLLBACK")
                .map_err(|error| sqlite_error("close moved Change seek read snapshot", error))?;
            return Ok(LocatorRead::CatchUpRequired {
                applied: checkpoint.applied,
                observed,
            });
        }
        let state = query_materialized_state(&connection)?;
        if u64::try_from(state.event_count).ok() != Some(observed.sequence) {
            connection.execute_batch("ROLLBACK").map_err(|error| {
                sqlite_error("close inconsistent Change seek read snapshot", error)
            })?;
            return Err(SqliteSemanticError::Metadata(
                "Change seek state differs from its pinned checkpoint".to_owned(),
            ));
        }
        Ok(LocatorRead::Ready(ChangeSeekReadSnapshot { connection }))
    }

    /// Open the exact Timeline snapshot selected by an already-pinned reader
    /// checkpoint. Unlike the legacy History connection, coverage beyond the
    /// requested cursor is not accepted because v4 correlation and candidate
    /// rows can be rewritten by a later governed append.
    pub(crate) fn product_history_read_snapshot(
        &self,
        observed: TruthCursor,
    ) -> Result<LocatorRead<ProductHistoryReadSnapshot>, SqliteSemanticError> {
        let connection = self.locator.validated_connection()?;
        connection
            .execute_batch("BEGIN DEFERRED TRANSACTION")
            .map_err(|error| sqlite_error("begin product history read snapshot", error))?;
        let checkpoint = read_locator_checkpoint(&connection)?;
        validate_meta(&connection, checkpoint.applied)?;
        validate_product_history_meta(&connection, checkpoint.applied)?;
        if checkpoint.applied != observed {
            connection
                .execute_batch("ROLLBACK")
                .map_err(|error| sqlite_error("close moved product history snapshot", error))?;
            return Ok(LocatorRead::CatchUpRequired {
                applied: checkpoint.applied,
                observed,
            });
        }
        let state = query_materialized_state(&connection)?;
        let (projection, document_projection) =
            query_materialized_change_projections(&connection, observed.epoch, observed.sequence)?;
        Ok(LocatorRead::Ready(ProductHistoryReadSnapshot {
            connection,
            state,
            changes: MaterializedChangeProjection {
                as_of: observed,
                projection,
                document_projection,
            },
        }))
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
        let proposal_carrier_count = connection
            .query_row(
                "SELECT count(*) FROM semantic_revision_proposal_carrier",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| sqlite_error("count proposal carriers", error))?;
        let (product_history_profile_id, product_history_schema_version) = connection
            .query_row(
                "SELECT profile_id, schema_version
                 FROM product_history_meta WHERE singleton = 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(|error| sqlite_error("read product history inventory identity", error))?;
        let product_history_event_count = connection
            .query_row("SELECT count(*) FROM product_history_event", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|error| sqlite_error("count product history events", error))?;
        let product_history_tables = query_names(
            &connection,
            "SELECT name FROM sqlite_schema
             WHERE type = 'table'
               AND (name LIKE 'product_history_%'
                    OR name IN ('product_revision', 'product_revision_edge'))
             ORDER BY name",
            0,
        )?;
        let product_history_columns = query_table_columns(&connection, &product_history_tables)?;
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
            proposal_carrier_count: u64::try_from(proposal_carrier_count).map_err(|_| {
                SqliteSemanticError::Metadata("negative proposal carrier count".to_owned())
            })?,
            proposal_carrier_columns: query_names(
                &connection,
                "PRAGMA table_info(semantic_revision_proposal_carrier)",
                1,
            )?,
            proposal_carrier_indexes: query_names(
                &connection,
                "PRAGMA index_list(semantic_revision_proposal_carrier)",
                1,
            )?,
            product_history_profile_id,
            product_history_schema_version: u32::try_from(product_history_schema_version).map_err(
                |_| {
                    SqliteSemanticError::Metadata(
                        "negative product history schema version".to_owned(),
                    )
                },
            )?,
            product_history_event_count: u64::try_from(product_history_event_count).map_err(
                |_| {
                    SqliteSemanticError::Metadata("negative product history event count".to_owned())
                },
            )?,
            product_history_tables,
            product_history_columns,
            product_history_indexes: query_names(
                &connection,
                "SELECT name FROM sqlite_schema
                 WHERE type = 'index' AND sql IS NOT NULL
                   AND (name LIKE 'product_history_%'
                        OR tbl_name IN ('product_revision', 'product_revision_edge'))
                 ORDER BY name",
                0,
            )?,
            retained_body_object_bytes,
        })
    }
}

fn query_table_columns(
    connection: &rusqlite::Connection,
    tables: &[String],
) -> Result<Vec<String>, SqliteSemanticError> {
    let mut columns = Vec::new();
    for table in tables {
        let pragma = format!("PRAGMA table_info({})", quote_identifier(table));
        let mut statement = connection
            .prepare(&pragma)
            .map_err(|error| sqlite_error("prepare product history columns", error))?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|error| sqlite_error("query product history columns", error))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| sqlite_error("read product history columns", error))?;
        columns.extend(names.into_iter().map(|name| format!("{table}.{name}")));
    }
    columns.sort();
    Ok(columns)
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

fn insert_product_history_facts(
    transaction: &Transaction<'_>,
    facts: &[ProductHistoryFact],
) -> Result<(), SqliteLocatorError> {
    let mut candidate_revision_ids = BTreeSet::new();
    let mut new_historical_references = BTreeSet::new();
    let mut affected_membership_claims = BTreeSet::new();
    let mut affected_relation_claims = BTreeSet::new();
    for fact in facts {
        let sequence = to_i64_locator(fact.sequence, "product history sequence")?;
        for tag_key in &fact.tag_keys {
            transaction
                .execute(
                    "INSERT INTO product_history_tag (sequence, tag_key) VALUES (?1, ?2)",
                    params![sequence, tag_key],
                )
                .map_err(|error| locator_sqlite_error("insert product history tag", error))?;
        }
        for tag_value in &fact.tag_values {
            transaction
                .execute(
                    "INSERT INTO product_history_tag_value (sequence, tag_value)
                     VALUES (?1, ?2)",
                    params![sequence, tag_value],
                )
                .map_err(|error| locator_sqlite_error("insert product history tag value", error))?;
        }
        if let Some(target_event_id) = &fact.signature_target_event_id {
            transaction
                .execute(
                    "INSERT INTO product_history_signature (sequence, target_event_id)
                     VALUES (?1, ?2)",
                    params![sequence, target_event_id],
                )
                .map_err(|error| locator_sqlite_error("insert product history signature", error))?;
        }
        insert_content_reference_facts(transaction, sequence, &fact.content_references)?;
        if let Some(revision) = &fact.revision {
            candidate_revision_ids.insert(revision.revision_id.clone());
            transaction
                .execute(
                    "INSERT INTO product_revision
                         (sequence, revision_id, captured_at, captured_at_millis)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        sequence,
                        revision.revision_id,
                        revision.captured_at,
                        revision.captured_at_millis
                    ],
                )
                .map_err(|error| locator_sqlite_error("insert product revision", error))?;
            for superseded_revision_id in &revision.supersedes {
                transaction
                    .execute(
                        "INSERT INTO product_revision_edge
                         (sequence, superseded_revision_id) VALUES (?1, ?2)",
                        params![sequence, superseded_revision_id],
                    )
                    .map_err(|error| locator_sqlite_error("insert product revision edge", error))?;
            }
        }
        if let Some(timeline) = &fact.timeline {
            transaction
                .execute(
                    "INSERT INTO product_history_event (sequence, request_state)
                     VALUES (?1, ?2)",
                    params![sequence, timeline.request_state],
                )
                .map_err(|error| locator_sqlite_error("insert product history event", error))?;
            for reference in &timeline.revision_references {
                let resolution = if reference.object_artifact_content_hash.is_some() {
                    "exact"
                } else {
                    "unresolved"
                };
                transaction
                    .execute(
                        "INSERT INTO product_history_revision_reference
                         (sequence, source_kind, reference_role, resolution, revision_id,
                          object_artifact_content_hash, historical_change_eligible)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            sequence,
                            reference.source_kind,
                            reference.reference_role,
                            resolution,
                            reference.revision_id,
                            reference.object_artifact_content_hash,
                            i64::from(reference.historical_change_eligible),
                        ],
                    )
                    .map_err(|error| {
                        locator_sqlite_error("insert product history Revision reference", error)
                    })?;
                if reference.reference_role == "candidate" {
                    candidate_revision_ids.insert(reference.revision_id.clone());
                }
                if reference.historical_change_eligible {
                    new_historical_references.insert((sequence, reference.revision_id.clone()));
                }
            }
            for direct in &timeline.direct_changes {
                insert_direct_change_correlation(transaction, sequence, direct)?;
            }
        }
        if let Some(claim) = &fact.membership_claim {
            transaction
                .execute(
                    "INSERT INTO product_history_membership_claim
                     (sequence, claim_id, change_id, revision_id)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![sequence, claim.claim_id, claim.change_id, claim.revision_id],
                )
                .map_err(|error| {
                    locator_sqlite_error("insert product history membership claim", error)
                })?;
            affected_membership_claims.insert(claim.claim_id.clone());
        }
        if let Some(claim_id) = &fact.membership_withdrawal_claim_id {
            transaction
                .execute(
                    "INSERT INTO product_history_membership_withdrawal (sequence, claim_id)
                     VALUES (?1, ?2)",
                    params![sequence, claim_id],
                )
                .map_err(|error| {
                    locator_sqlite_error("insert product history membership withdrawal", error)
                })?;
            affected_membership_claims.insert(claim_id.clone());
        }
        if let Some(claim) = &fact.relation_claim {
            transaction
                .execute(
                    "INSERT INTO product_history_relation_claim
                     (sequence, claim_id, change_id,
                      successor_revision_id, successor_object_artifact_content_hash,
                      predecessor_revision_id, predecessor_object_artifact_content_hash)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        sequence,
                        claim.claim_id,
                        claim.change_id,
                        claim.successor.revision_id.as_str(),
                        claim.successor.object_artifact_content_hash,
                        claim.predecessor.revision_id.as_str(),
                        claim.predecessor.object_artifact_content_hash,
                    ],
                )
                .map_err(|error| {
                    locator_sqlite_error("insert product history relation claim", error)
                })?;
            affected_relation_claims.insert(claim.claim_id.clone());
        }
        if let Some(claim_id) = &fact.relation_withdrawal_claim_id {
            transaction
                .execute(
                    "INSERT INTO product_history_relation_withdrawal (sequence, claim_id)
                     VALUES (?1, ?2)",
                    params![sequence, claim_id],
                )
                .map_err(|error| {
                    locator_sqlite_error("insert product history relation withdrawal", error)
                })?;
            affected_relation_claims.insert(claim_id.clone());
        }
    }

    for claim_id in &affected_membership_claims {
        if let Some(revision_id) = refresh_membership_withdrawals(transaction, claim_id)? {
            candidate_revision_ids.insert(revision_id);
        }
        refresh_membership_history(transaction, claim_id)?;
    }
    for claim_id in &affected_relation_claims {
        refresh_relation_withdrawals(transaction, claim_id)?;
        refresh_relation_history(transaction, claim_id)?;
    }
    for revision_id in candidate_revision_ids {
        refresh_candidate_revision_resolution(transaction, &revision_id)?;
    }
    for (sequence, revision_id) in new_historical_references {
        insert_historical_correlations_for_reference(transaction, sequence, &revision_id)?;
    }
    Ok(())
}

fn insert_direct_change_correlation(
    transaction: &Transaction<'_>,
    sequence: i64,
    direct: &ProductDirectChangeFact,
) -> Result<(), SqliteLocatorError> {
    transaction
        .execute(
            "INSERT INTO product_history_change_correlation
             (sequence, change_id, correlation_role, source_kind, source_id, support_sequence)
             VALUES (?1, ?2, 'direct', ?3, ?4, ?1)",
            params![
                sequence,
                direct.change_id,
                direct.source_kind,
                direct.source_id
            ],
        )
        .map_err(|error| locator_sqlite_error("insert direct Change correlation", error))?;
    Ok(())
}

fn refresh_candidate_revision_resolution(
    transaction: &Transaction<'_>,
    revision_id: &str,
) -> Result<(), SqliteLocatorError> {
    let hashes = {
        let mut statement = transaction
            .prepare(
                "SELECT object_artifact_content_hash
                 FROM semantic_revision_proposal_carrier
                 WHERE revision_id = ?1
                 ORDER BY object_artifact_content_hash, sequence",
            )
            .map_err(|error| locator_sqlite_error("prepare candidate Revision bindings", error))?;
        statement
            .query_map([revision_id], |row| row.get::<_, String>(0))
            .map_err(|error| locator_sqlite_error("query candidate Revision bindings", error))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| locator_sqlite_error("read candidate Revision bindings", error))?
    };
    let revision = RevisionId::new(revision_id);
    let mut exact_hashes = BTreeSet::new();
    let mut all_valid = !hashes.is_empty();
    for hash in hashes {
        match RevisionRefV1::new(revision.clone(), hash) {
            Ok(reference) => {
                exact_hashes.insert(reference.object_artifact_content_hash);
            }
            Err(_) => all_valid = false,
        }
    }
    let exact = (all_valid && exact_hashes.len() == 1)
        .then(|| exact_hashes.into_iter().next())
        .flatten();
    transaction
        .execute(
            "UPDATE product_history_revision_reference
             SET resolution = CASE WHEN ?2 IS NULL THEN 'unresolved' ELSE 'exact' END,
                 object_artifact_content_hash = ?2
             WHERE revision_id = ?1 AND reference_role = 'candidate'",
            params![revision_id, exact],
        )
        .map_err(|error| locator_sqlite_error("refresh candidate Revision resolution", error))?;
    Ok(())
}

fn refresh_membership_withdrawals(
    transaction: &Transaction<'_>,
    claim_id: &str,
) -> Result<Option<String>, SqliteLocatorError> {
    transaction
        .execute(
            "DELETE FROM product_history_change_correlation
             WHERE correlation_role = 'direct'
               AND source_kind = 'membership_claim'
               AND source_id = ?1
               AND sequence IN (
                   SELECT sequence FROM product_history_membership_withdrawal
                   WHERE claim_id = ?1
               )",
            [claim_id],
        )
        .map_err(|error| locator_sqlite_error("clear membership withdrawal correlation", error))?;
    transaction
        .execute(
            "DELETE FROM product_history_revision_reference
             WHERE source_kind = 'membership_claim'
               AND sequence IN (
                   SELECT sequence FROM product_history_membership_withdrawal
                   WHERE claim_id = ?1
               )",
            [claim_id],
        )
        .map_err(|error| locator_sqlite_error("clear membership withdrawal reference", error))?;
    let canonical = transaction
        .query_row(
            "SELECT change_id, revision_id
             FROM product_history_membership_claim
             WHERE claim_id = ?1
             ORDER BY sequence
             LIMIT 1",
            [claim_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| locator_sqlite_error("read membership claim", error))?;
    let Some((change_id, revision_id)) = canonical else {
        return Ok(None);
    };
    transaction
        .execute(
            "INSERT INTO product_history_revision_reference
             (sequence, source_kind, reference_role, resolution, revision_id,
              object_artifact_content_hash, historical_change_eligible)
             SELECT sequence, 'membership_claim', 'candidate', 'unresolved', ?2, NULL, 0
             FROM product_history_membership_withdrawal
             WHERE claim_id = ?1",
            params![claim_id, revision_id],
        )
        .map_err(|error| locator_sqlite_error("refresh membership withdrawal reference", error))?;
    transaction
        .execute(
            "INSERT INTO product_history_change_correlation
             (sequence, change_id, correlation_role, source_kind, source_id, support_sequence)
             SELECT withdrawal.sequence, ?2, 'direct', 'membership_claim', ?1,
                    support.sequence
             FROM product_history_membership_withdrawal AS withdrawal
             JOIN product_history_membership_claim AS support
               ON support.claim_id = withdrawal.claim_id
             WHERE withdrawal.claim_id = ?1",
            params![claim_id, change_id],
        )
        .map_err(|error| {
            locator_sqlite_error("refresh membership withdrawal correlation", error)
        })?;
    Ok(Some(revision_id))
}

fn refresh_relation_withdrawals(
    transaction: &Transaction<'_>,
    claim_id: &str,
) -> Result<(), SqliteLocatorError> {
    transaction
        .execute(
            "DELETE FROM product_history_change_correlation
             WHERE correlation_role = 'direct'
               AND source_kind = 'relation_claim'
               AND source_id = ?1
               AND sequence IN (
                   SELECT sequence FROM product_history_relation_withdrawal
                   WHERE claim_id = ?1
               )",
            [claim_id],
        )
        .map_err(|error| locator_sqlite_error("clear relation withdrawal correlation", error))?;
    transaction
        .execute(
            "DELETE FROM product_history_revision_reference
             WHERE source_kind IN ('relation_successor', 'relation_predecessor')
               AND sequence IN (
                   SELECT sequence FROM product_history_relation_withdrawal
                   WHERE claim_id = ?1
               )",
            [claim_id],
        )
        .map_err(|error| locator_sqlite_error("clear relation withdrawal references", error))?;
    let canonical = transaction
        .query_row(
            "SELECT change_id,
                    successor_revision_id, successor_object_artifact_content_hash,
                    predecessor_revision_id, predecessor_object_artifact_content_hash
             FROM product_history_relation_claim
             WHERE claim_id = ?1
             ORDER BY sequence
             LIMIT 1",
            [claim_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| locator_sqlite_error("read relation claim", error))?;
    let Some((change_id, successor_id, successor_hash, predecessor_id, predecessor_hash)) =
        canonical
    else {
        return Ok(());
    };
    for (source_kind, revision_id, object_hash) in [
        ("relation_successor", successor_id, successor_hash),
        ("relation_predecessor", predecessor_id, predecessor_hash),
    ] {
        transaction
            .execute(
                "INSERT INTO product_history_revision_reference
                 (sequence, source_kind, reference_role, resolution, revision_id,
                  object_artifact_content_hash, historical_change_eligible)
                 SELECT sequence, ?2, 'direct', 'exact', ?3, ?4, 0
                 FROM product_history_relation_withdrawal
                 WHERE claim_id = ?1",
                params![claim_id, source_kind, revision_id, object_hash],
            )
            .map_err(|error| {
                locator_sqlite_error("refresh relation withdrawal reference", error)
            })?;
    }
    transaction
        .execute(
            "INSERT INTO product_history_change_correlation
             (sequence, change_id, correlation_role, source_kind, source_id, support_sequence)
             SELECT withdrawal.sequence, ?2, 'direct', 'relation_claim', ?1,
                    support.sequence
             FROM product_history_relation_withdrawal AS withdrawal
             JOIN product_history_relation_claim AS support
               ON support.claim_id = withdrawal.claim_id
             WHERE withdrawal.claim_id = ?1",
            params![claim_id, change_id],
        )
        .map_err(|error| locator_sqlite_error("refresh relation withdrawal correlation", error))?;
    Ok(())
}

fn refresh_membership_history(
    transaction: &Transaction<'_>,
    claim_id: &str,
) -> Result<(), SqliteLocatorError> {
    transaction
        .execute(
            "DELETE FROM product_history_change_correlation
             WHERE correlation_role = 'historical'
               AND source_kind = 'membership_claim'
               AND source_id = ?1",
            [claim_id],
        )
        .map_err(|error| locator_sqlite_error("clear membership history", error))?;
    let canonical = transaction
        .query_row(
            "SELECT change_id, revision_id
             FROM product_history_membership_claim
             WHERE claim_id = ?1
             ORDER BY sequence
             LIMIT 1",
            [claim_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| locator_sqlite_error("read membership history claim", error))?;
    let Some((change_id, revision_id)) = canonical else {
        return Ok(());
    };
    transaction
        .execute(
            "INSERT OR IGNORE INTO product_history_change_correlation
             (sequence, change_id, correlation_role, source_kind, source_id, support_sequence)
             SELECT reference.sequence, ?2, 'historical', 'membership_claim', ?1,
                    support.sequence
             FROM product_history_revision_reference AS reference
             JOIN product_history_membership_claim AS support
               ON support.claim_id = ?1
             WHERE reference.revision_id = ?3
               AND reference.historical_change_eligible = 1",
            params![claim_id, change_id, revision_id],
        )
        .map_err(|error| locator_sqlite_error("refresh membership history", error))?;
    Ok(())
}

fn refresh_relation_history(
    transaction: &Transaction<'_>,
    claim_id: &str,
) -> Result<(), SqliteLocatorError> {
    transaction
        .execute(
            "DELETE FROM product_history_change_correlation
             WHERE correlation_role = 'historical'
               AND source_kind = 'relation_claim'
               AND source_id = ?1",
            [claim_id],
        )
        .map_err(|error| locator_sqlite_error("clear relation history", error))?;
    let canonical = transaction
        .query_row(
            "SELECT change_id, successor_revision_id, predecessor_revision_id
             FROM product_history_relation_claim
             WHERE claim_id = ?1
             ORDER BY sequence
             LIMIT 1",
            [claim_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| locator_sqlite_error("read relation history claim", error))?;
    let Some((change_id, successor_id, predecessor_id)) = canonical else {
        return Ok(());
    };
    transaction
        .execute(
            "INSERT OR IGNORE INTO product_history_change_correlation
             (sequence, change_id, correlation_role, source_kind, source_id, support_sequence)
             SELECT reference.sequence, ?2, 'historical', 'relation_claim', ?1,
                    support.sequence
             FROM product_history_revision_reference AS reference
             JOIN product_history_relation_claim AS support
               ON support.claim_id = ?1
             WHERE reference.revision_id IN (?3, ?4)
               AND reference.historical_change_eligible = 1",
            params![claim_id, change_id, successor_id, predecessor_id],
        )
        .map_err(|error| locator_sqlite_error("refresh relation history", error))?;
    Ok(())
}

fn insert_historical_correlations_for_reference(
    transaction: &Transaction<'_>,
    sequence: i64,
    revision_id: &str,
) -> Result<(), SqliteLocatorError> {
    transaction
        .execute(
            "INSERT OR IGNORE INTO product_history_change_correlation
             (sequence, change_id, correlation_role, source_kind, source_id, support_sequence)
             SELECT ?1, canonical.change_id, 'historical', 'membership_claim',
                    canonical.claim_id, support.sequence
             FROM product_history_membership_claim AS canonical
             JOIN product_history_membership_claim AS support
               ON support.claim_id = canonical.claim_id
             WHERE canonical.revision_id = ?2
               AND canonical.sequence = (
                   SELECT min(first.sequence)
                   FROM product_history_membership_claim AS first
                   WHERE first.claim_id = canonical.claim_id
               )",
            params![sequence, revision_id],
        )
        .map_err(|error| locator_sqlite_error("insert membership history for reference", error))?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO product_history_change_correlation
             (sequence, change_id, correlation_role, source_kind, source_id, support_sequence)
             SELECT ?1, canonical.change_id, 'historical', 'relation_claim',
                    canonical.claim_id, support.sequence
             FROM product_history_relation_claim AS canonical
             JOIN product_history_relation_claim AS support
               ON support.claim_id = canonical.claim_id
             WHERE (?2 = canonical.successor_revision_id
                    OR ?2 = canonical.predecessor_revision_id)
               AND canonical.sequence = (
                   SELECT min(first.sequence)
                   FROM product_history_relation_claim AS first
                   WHERE first.claim_id = canonical.claim_id
               )",
            params![sequence, revision_id],
        )
        .map_err(|error| locator_sqlite_error("insert relation history for reference", error))?;
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
    for (index, pair) in hex.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        digest[index] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some((prefix, digest))
}

fn semantic_key_digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

/// One row per content hash the event references, in the shared
/// prefix/digest-or-raw identity encoding. `content_key_hash` is a fixed-width
/// surrogate for the primary key only — SQLite forbids NULL columns in a
/// WITHOUT ROWID primary key, and the encoded triple is nullable per branch.
fn insert_content_reference_facts(
    transaction: &Transaction<'_>,
    sequence: i64,
    content_references: &[String],
) -> Result<(), SqliteLocatorError> {
    for content_hash in content_references {
        let content = encode_identity(transaction, Some(content_hash))?;
        transaction
            .execute(
                "INSERT INTO product_history_content_reference
                     (sequence, content_prefix_id, content_digest, content_raw,
                      content_key_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    sequence,
                    content.prefix_id,
                    content.digest.as_deref(),
                    content.raw,
                    semantic_key_digest(content_hash).as_slice(),
                ],
            )
            .map_err(|error| locator_sqlite_error("insert content reference", error))?;
    }
    Ok(())
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
    if let Some(change) = &fact.change {
        let fact_json = serde_json::to_string(change)
            .map_err(|error| SqliteLocatorError::Delta(error.to_string()))?;
        transaction
            .execute(
                "INSERT INTO semantic_change_fact (sequence, fact_json) VALUES (?1, ?2)",
                params![sequence, fact_json],
            )
            .map_err(|error| locator_sqlite_error("insert Change semantic fact", error))?;
    }
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
    if let Some(ChangeProjectionFact::Revision {
        revision_id,
        object_artifact_content_hash,
    }) = &fact.change
    {
        transaction
            .execute(
                "INSERT INTO semantic_revision_proposal_carrier
                 (sequence, revision_id, object_artifact_content_hash)
                 VALUES (?1, ?2, ?3)",
                params![sequence, revision_id.as_str(), object_artifact_content_hash],
            )
            .map_err(|error| locator_sqlite_error("insert proposal carrier", error))?;
    }
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
             WHERE representative.family_id = ?1 AND representative.semantic_key_hash = ?2",
            params![
                family_code(family)?,
                semantic_key_digest(semantic_key).as_slice()
            ],
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
             (family_id, semantic_key_prefix_id, semantic_key_digest, semantic_key_raw,
              semantic_key_hash, sequence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(family_id, semantic_key_hash) DO UPDATE SET
                 semantic_key_prefix_id = excluded.semantic_key_prefix_id,
                 semantic_key_digest = excluded.semantic_key_digest,
                 semantic_key_raw = excluded.semantic_key_raw,
                 sequence = excluded.sequence",
            params![
                family_code(family)?,
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
        SemanticFactKind::Other => fact
            .change
            .as_ref()
            .map(|_| ("change_record", fact.event_id.as_str())),
    }
}

fn duplicate_family(family: &str) -> bool {
    matches!(
        family,
        "observation" | "assessment" | "request" | "response" | "validation"
    )
}

fn family_code(family: &str) -> Result<i64, SqliteLocatorError> {
    match family {
        "revision" => Ok(1),
        "observation" => Ok(2),
        "assessment" => Ok(3),
        "request" => Ok(4),
        "response" => Ok(5),
        "validation" => Ok(6),
        "commit_association" => Ok(7),
        "commit_withdrawal" => Ok(8),
        "ref_association" => Ok(9),
        "ref_withdrawal" => Ok(10),
        "removal" => Ok(11),
        "change_record" => Ok(12),
        _ => Err(SqliteLocatorError::Delta(format!(
            "unsupported semantic representative family {family}"
        ))),
    }
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
                     WHERE representative.family_id = ?1
                       AND representative.semantic_key_hash = ?2",
                    params![
                        family_code(family)?,
                        semantic_key_digest(semantic_key).as_slice()
                    ],
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
             WHERE representative.family_id = 4
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
               ON representative.family_id = 5
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

#[derive(Clone, Copy)]
enum MaterializedFactFamilies {
    AllExceptObservations,
    Attention,
}

impl MaterializedFactFamilies {
    const fn predicate(self) -> &'static str {
        match self {
            Self::AllExceptObservations => "representative.family_id != 2",
            Self::Attention => "representative.family_id IN (1, 3, 4, 5, 6)",
        }
    }
}

fn query_materialized_facts(
    connection: &rusqlite::Connection,
    journal: &QualificationLocalJournal,
    epoch: u64,
    sequence: u64,
    engagement_id: Option<&str>,
    families: MaterializedFactFamilies,
) -> Result<Vec<HydratedSemanticFact>, SqliteSemanticError> {
    query_materialized_compact_facts(connection, epoch, sequence, engagement_id, families)?
        .into_iter()
        .map(|fact| hydrate_semantic_fact(journal, fact))
        .collect()
}

fn query_materialized_compact_facts(
    connection: &rusqlite::Connection,
    epoch: u64,
    sequence: u64,
    engagement_id: Option<&str>,
    families: MaterializedFactFamilies,
) -> Result<Vec<SemanticFact>, SqliteSemanticError> {
    let sql = format!(
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
                change_fact.fact_json,
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
         LEFT JOIN semantic_change_fact AS change_fact
           ON change_fact.sequence = event.sequence
         WHERE {}
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
                     ON selected_representative.family_id = 1
                    AND selected_representative.sequence = selected_event.sequence
                   WHERE selected_revision.engagement_id = ?3
                     AND selected_locator.epoch = ?1
                     AND selected_event.sequence <= ?2
               )
               OR representative.family_id = 12
               OR (
                   representative.family_id = 11
                   AND event.content_hash IN (
                       SELECT selected_event.content_hash
                       FROM semantic_revision_fact AS selected_revision
                       JOIN semantic_event_fact_text AS selected_event
                         ON selected_event.sequence = selected_revision.sequence
                       JOIN locator_event AS selected_locator
                         ON selected_locator.sequence = selected_event.sequence
                       JOIN semantic_representative AS selected_representative
                         ON selected_representative.family_id = 1
                        AND selected_representative.sequence = selected_event.sequence
                       WHERE selected_revision.engagement_id = ?3
                         AND selected_locator.epoch = ?1
                         AND selected_event.sequence <= ?2
                   )
               )
           )
         ORDER BY locator.replay_key, receipt.logical_reread_key_hash",
        families.predicate()
    );
    let mut statement = connection
        .prepare(&sql)
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
        fact.change = row
            .get::<_, Option<String>>(41)
            .map_err(|error| sqlite_error("read materialized Change fact", error))?
            .map(|value| serde_json::from_str::<ChangeProjectionFact>(&value))
            .transpose()
            .map_err(|error| SqliteSemanticError::Model(SemanticModelError::Json(error)))?;
        let receipt_epoch = row
            .get::<_, i64>(42)
            .map_err(|error| sqlite_error("read materialized receipt epoch", error))?;
        if receipt_epoch != to_i64(fact.cursor.epoch, "materialized receipt epoch")? {
            return Err(SqliteSemanticError::Metadata(format!(
                "materialized fact does not match cursor receipt at {:?}",
                fact.cursor
            )));
        }
        facts.push(fact);
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
        fact.change = query_change_fact(connection, fact.cursor.sequence)?;
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
        change: None,
    })
}

fn query_change_fact(
    connection: &rusqlite::Connection,
    sequence: u64,
) -> Result<Option<ChangeProjectionFact>, SqliteSemanticError> {
    let value = connection
        .query_row(
            "SELECT fact_json FROM semantic_change_fact WHERE sequence = ?1",
            [to_i64(sequence, "Change semantic fact sequence")?],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| sqlite_error("query Change semantic fact", error))?;
    value
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|error| SqliteSemanticError::Model(SemanticModelError::Json(error)))
}

fn query_materialized_change_projection(
    connection: &rusqlite::Connection,
    epoch: u64,
    sequence: u64,
) -> Result<crate::session::ChangeProjection, SqliteSemanticError> {
    let facts = query_materialized_change_document_facts(connection, epoch, sequence)?;
    project_changes_from_facts(
        &facts
            .into_iter()
            .map(|fact| fact.change)
            .collect::<Vec<_>>(),
    )
    .map_err(|error| SqliteSemanticError::Model(SemanticModelError::Product(error)))
}

fn query_materialized_change_projections(
    connection: &rusqlite::Connection,
    epoch: u64,
    sequence: u64,
) -> Result<
    (
        crate::session::ChangeProjection,
        crate::session::ChangeDocumentProjectionV1,
    ),
    SqliteSemanticError,
> {
    let facts = query_materialized_change_document_facts(connection, epoch, sequence)?;
    let projection_facts = facts
        .iter()
        .map(|fact| fact.change.clone())
        .collect::<Vec<_>>();
    let projection = project_changes_from_facts(&projection_facts)
        .map_err(|error| SqliteSemanticError::Model(SemanticModelError::Product(error)))?;
    let document_projection = project_change_documents_from_facts(&facts)
        .map_err(|error| SqliteSemanticError::Model(SemanticModelError::Product(error)))?;
    Ok((projection, document_projection))
}

fn query_materialized_change_document_facts(
    connection: &rusqlite::Connection,
    epoch: u64,
    sequence: u64,
) -> Result<Vec<ChangeDocumentProjectionFact>, SqliteSemanticError> {
    let mut statement = connection
        .prepare(
            "SELECT change_fact.fact_json, locator.event_id,
                    event.actor_id, locator.track_id,
                    locator.epoch, receipt.epoch
             FROM semantic_change_fact AS change_fact
             JOIN semantic_event_fact_text AS event
               ON event.sequence = change_fact.sequence
             JOIN locator_event_text AS locator ON locator.sequence = change_fact.sequence
             JOIN cursor_receipt_text AS receipt ON receipt.sequence = change_fact.sequence
             WHERE locator.epoch = ?1 AND change_fact.sequence <= ?2
             ORDER BY locator.replay_key, receipt.logical_reread_key_hash",
        )
        .map_err(|error| sqlite_error("prepare materialized Change projections", error))?;
    let rows = statement
        .query_map(
            params![
                to_i64(epoch, "materialized Change epoch")?,
                to_i64(sequence, "materialized Change sequence")?,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .map_err(|error| sqlite_error("query materialized Change projections", error))?;
    let mut facts = Vec::new();
    for row in rows {
        let (json, event_id, actor_id, track_id, locator_epoch, receipt_epoch) =
            row.map_err(|error| sqlite_error("read materialized Change fact", error))?;
        let expected_epoch = to_i64(epoch, "materialized Change epoch")?;
        if locator_epoch != expected_epoch || receipt_epoch != locator_epoch {
            return Err(SqliteSemanticError::Metadata(format!(
                "materialized Change fact {event_id} receipt epoch {receipt_epoch} does not match locator epoch {locator_epoch} at expected epoch {epoch}"
            )));
        }
        facts.push(ChangeDocumentProjectionFact::new(
            serde_json::from_str::<ChangeProjectionFact>(&json)
                .map_err(|error| SqliteSemanticError::Model(SemanticModelError::Json(error)))?,
            EventId::new(event_id),
            ActorId::new(actor_id),
            track_id.map(TrackId::new),
        ));
    }
    Ok(facts)
}

fn proposal_carrier_locator_from_sql(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ProposalCarrierLocator> {
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
    let revision_id = RevisionId::new(row.get::<_, String>(8)?);
    let object_artifact_content_hash = row.get::<_, String>(9)?;
    let revision =
        RevisionRefV1::new(revision_id, object_artifact_content_hash).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                9,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    Ok(ProposalCarrierLocator {
        cursor: TruthCursor::new(epoch, sequence),
        logical_reread_key_hash: row.get(2)?,
        replay_key: row.get(3)?,
        event_id: EventId::new(row.get::<_, String>(4)?),
        event_type: row.get(5)?,
        payload_hash: row.get(6)?,
        validation_witness: row.get(7)?,
        revision,
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

fn validate_product_history_meta(
    connection: &rusqlite::Connection,
    expected: TruthCursor,
) -> Result<(), SqliteSemanticError> {
    let (profile, version, epoch, applied) = connection
        .query_row(
            "SELECT profile_id, schema_version, epoch, applied_sequence
             FROM product_history_meta WHERE singleton = 1",
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
        .map_err(|error| sqlite_error("validate product history metadata", error))?;
    if profile != PRODUCT_HISTORY_PROFILE_ID
        || version != PRODUCT_HISTORY_SCHEMA_VERSION
        || epoch != to_i64(expected.epoch, "expected product history epoch")?
        || applied != to_i64(expected.sequence, "expected product history applied")?
    {
        return Err(SqliteSemanticError::Metadata(format!(
            "product history identity/checkpoint {profile}/{version}/{epoch}/{applied} \
             does not match {PRODUCT_HISTORY_PROFILE_ID}/{PRODUCT_HISTORY_SCHEMA_VERSION}/{expected:?}"
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

fn read_reader_projection_checkpoint(
    connection: &rusqlite::Connection,
) -> Result<Option<ReaderProjectionCheckpointV1>, SqliteSemanticError> {
    let exists = connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_schema
                 WHERE type = 'table' AND name = 'reader_projection_checkpoint'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| sqlite_error("inspect reader projection checkpoint schema", error))?;
    if !exists {
        return Ok(None);
    }
    let checkpoint_json = connection
        .query_row(
            "SELECT checkpoint_json
             FROM reader_projection_checkpoint
             WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| sqlite_error("read reader projection checkpoint", error))?;
    checkpoint_json
        .map(|checkpoint_json| {
            serde_json::from_str(&checkpoint_json)
                .map_err(|error| SqliteSemanticError::Metadata(error.to_string()))
        })
        .transpose()
}

fn canonical_checkpoint_json(
    checkpoint: &ReaderProjectionCheckpointV1,
) -> Result<String, SqliteSemanticError> {
    let value = serde_json::to_value(checkpoint)
        .map_err(|error| SqliteSemanticError::Metadata(error.to_string()))?;
    String::from_utf8(
        canonical_json_bytes(&value)
            .map_err(|error| SqliteSemanticError::Metadata(error.to_string()))?,
    )
    .map_err(|error| SqliteSemanticError::Metadata(error.to_string()))
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
