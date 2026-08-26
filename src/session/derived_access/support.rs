//! Storage-internal support-closure expansion for derived product reads.

use std::collections::BTreeSet;

use rusqlite::types::Value;

use super::cursor::TruthCursor;
use crate::session::event::ShoreEvent;

/// Find the authoritative carriers needed to interpret selected product rows.
///
/// Two reference paths deliberately coexist: this SELECTED support closure
/// computes references in Rust over the already-hydrated selected carriers,
/// while the STORE-WIDE removal-audit closure seeks the persisted
/// `product_history_content_reference` index because its carriers are unknown
/// before the seek. Both derive reference semantics from the shared per-event
/// extraction in `session::workflow::artifact_transfer`.
///
/// Support closure has three ordered phases:
///
/// 1. Payload extraction retains multi-value references such as validation
///    logs, while the indexed semantic fact for every selected carrier adds
///    canonical references nested inside typed payloads (notably a captured
///    revision's object artifact).
/// 2. Referenced content hashes select `artifact_removed` carriers. Those
///    carriers are also targets because a detached signature can attest to a
///    removal event.
/// 3. The original selection plus those removal carriers select detached
///    `event_signature_recorded` carriers.
///
/// Every phase may contain more values than SQLite can bind in one statement at
/// retained scale. A connection-local TEMP table carries each complete set into
/// a set-oriented join without changing the immutable generation. Complete the
/// removal phase before replacing that table with the signature targets;
/// otherwise signatures on removal carriers would be omitted. Each product
/// read owns its connection, so the TEMP table is isolated from other requests;
/// the transaction batches all populations, while `BTreeSet` preserves
/// deterministic, duplicate-free output.
pub(super) fn support_event_ids(
    connection: &rusqlite::Connection,
    selected: &[ShoreEvent],
    as_of: TruthCursor,
) -> Result<Vec<String>, String> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let plan = support_event_plan(&transaction, selected, as_of)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(plan.all_event_ids())
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct SupportEventPlan {
    pub(super) removal_event_ids: Vec<String>,
    pub(super) signature_event_ids: Vec<String>,
}

impl SupportEventPlan {
    pub(super) fn all_event_ids(&self) -> Vec<String> {
        self.removal_event_ids
            .iter()
            .chain(&self.signature_event_ids)
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

/// Expand removal/signature support inside a caller-owned exact read
/// snapshot. Timeline uses this after hydrating its selected and first-level
/// correlation/proposal carriers; legacy History wraps it in its own short
/// transaction through [`support_event_ids`].
pub(super) fn support_event_plan(
    connection: &rusqlite::Connection,
    selected: &[ShoreEvent],
    as_of: TruthCursor,
) -> Result<SupportEventPlan, String> {
    let mut targets = selected
        .iter()
        .map(|event| event.event_id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let mut content_hashes = crate::session::workflow::selected_support_content_hashes(selected)
        .map_err(|error| error.to_string())?;
    let mut removal_event_ids = BTreeSet::new();
    let mut signature_event_ids = BTreeSet::new();
    connection
        .execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS pointbreak_product_support_lookup (
                 value TEXT PRIMARY KEY
             ) STRICT, WITHOUT ROWID;",
        )
        .map_err(|error| error.to_string())?;
    if !targets.is_empty() {
        replace_support_lookup_values(connection, targets.iter())?;
        let sql = "SELECT DISTINCT event.content_hash
                   FROM semantic_event_fact_text AS event
                   JOIN locator_event_text AS locator ON locator.sequence = event.sequence
                   JOIN temp.pointbreak_product_support_lookup AS selected
                     ON selected.value = locator.event_id
                   WHERE event.content_hash IS NOT NULL
                     AND locator.epoch = ?
                     AND locator.sequence <= ?
                   ORDER BY event.content_hash";
        let parameters = [
            Value::from(to_sql_integer(as_of.epoch)?),
            Value::from(to_sql_integer(as_of.sequence)?),
        ];
        content_hashes.extend(query_string_rows(connection, sql, &parameters)?);
    }
    if !content_hashes.is_empty() {
        replace_support_lookup_values(connection, content_hashes.iter())?;
        let sql = "SELECT locator.event_id
                   FROM semantic_event_fact_text AS event
                   JOIN locator_event_text AS locator ON locator.sequence = event.sequence
                   JOIN temp.pointbreak_product_support_lookup AS selected
                     ON selected.value = event.content_hash
                   WHERE locator.event_type = 'artifact_removed'
                     AND locator.epoch = ?
                     AND locator.sequence <= ?
                   ORDER BY locator.event_id";
        let parameters = [
            Value::from(to_sql_integer(as_of.epoch)?),
            Value::from(to_sql_integer(as_of.sequence)?),
        ];
        for event_id in query_string_rows(connection, sql, &parameters)? {
            targets.insert(event_id.clone());
            removal_event_ids.insert(event_id);
        }
    }
    if !targets.is_empty() {
        replace_support_lookup_values(connection, targets.iter())?;
        let sql = "SELECT locator.event_id
                   FROM product_history_signature AS signature
                   JOIN locator_event_text AS locator ON locator.sequence = signature.sequence
                   JOIN temp.pointbreak_product_support_lookup AS selected
                     ON selected.value = signature.target_event_id
                   WHERE locator.epoch = ?
                     AND locator.sequence <= ?
                   ORDER BY locator.event_id";
        let parameters = [
            Value::from(to_sql_integer(as_of.epoch)?),
            Value::from(to_sql_integer(as_of.sequence)?),
        ];
        signature_event_ids.extend(query_string_rows(connection, sql, &parameters)?);
    }
    Ok(SupportEventPlan {
        removal_event_ids: removal_event_ids.into_iter().collect(),
        signature_event_ids: signature_event_ids.into_iter().collect(),
    })
}

fn replace_support_lookup_values<'a>(
    connection: &rusqlite::Connection,
    values: impl IntoIterator<Item = &'a String>,
) -> Result<(), String> {
    connection
        .execute("DELETE FROM temp.pointbreak_product_support_lookup", [])
        .map_err(|error| error.to_string())?;
    let mut insert = connection
        .prepare("INSERT INTO temp.pointbreak_product_support_lookup (value) VALUES (?1)")
        .map_err(|error| error.to_string())?;
    for value in values {
        insert.execute([value]).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn query_string_rows(
    connection: &rusqlite::Connection,
    sql: &str,
    parameters: &[Value],
) -> Result<Vec<String>, String> {
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn to_sql_integer(value: impl TryInto<i64>) -> Result<i64, String> {
    value
        .try_into()
        .map_err(|_| "history value does not fit SQLite INTEGER".to_owned())
}
