//! Storage-internal support-closure expansion for derived product reads.

use std::collections::BTreeSet;

use rusqlite::types::Value;

use super::cursor::TruthCursor;
use crate::session::event::ShoreEvent;

/// Find the authoritative carriers needed to interpret selected product rows.
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
    let mut targets = selected
        .iter()
        .map(|event| event.event_id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let mut content_hashes = crate::session::workflow::selected_support_content_hashes(selected)
        .map_err(|error| error.to_string())?;
    let mut support = BTreeSet::new();
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS pointbreak_product_support_lookup (
                 value TEXT PRIMARY KEY
             ) STRICT, WITHOUT ROWID;",
        )
        .map_err(|error| error.to_string())?;
    if !targets.is_empty() {
        replace_support_lookup_values(&transaction, targets.iter())?;
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
        content_hashes.extend(query_string_rows(&transaction, sql, &parameters)?);
    }
    if !content_hashes.is_empty() {
        replace_support_lookup_values(&transaction, content_hashes.iter())?;
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
        for event_id in query_string_rows(&transaction, sql, &parameters)? {
            targets.insert(event_id.clone());
            support.insert(event_id);
        }
    }
    if !targets.is_empty() {
        replace_support_lookup_values(&transaction, targets.iter())?;
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
        support.extend(query_string_rows(&transaction, sql, &parameters)?);
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(support.into_iter().collect())
}

fn replace_support_lookup_values<'a>(
    transaction: &rusqlite::Transaction<'_>,
    values: impl IntoIterator<Item = &'a String>,
) -> Result<(), String> {
    transaction
        .execute("DELETE FROM temp.pointbreak_product_support_lookup", [])
        .map_err(|error| error.to_string())?;
    let mut insert = transaction
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
