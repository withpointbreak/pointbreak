use std::path::{Path, PathBuf};

use rusqlite::Connection;
use sha2::{Digest as _, Sha256};

pub(super) fn hide_target_fact(root: &Path, generation_id: &str, event_id: &str) {
    with_generation_database(root, generation_id, |connection| {
        assert_eq!(
            connection
                .execute(
                    "UPDATE semantic_event_fact
                     SET revision_prefix_id = NULL,
                         revision_digest = NULL,
                         revision_raw = NULL
                     WHERE sequence = (
                         SELECT sequence FROM locator_event_text WHERE event_id = ?1
                     )",
                    [event_id],
                )
                .expect("hide target fact from the exact component"),
            1,
        );
    });
}

pub(super) fn mismatch_origin_artifact(
    root: &Path,
    generation_id: &str,
    port_event_id: &str,
    origin_hash: &str,
    wrong_hash: &str,
) {
    with_generation_database(root, generation_id, |connection| {
        let (fact_json, actor_id, track_id): (String, String, String) = connection
            .query_row(
                "SELECT change_fact.fact_json, event.actor_id, locator.track_id
                 FROM semantic_change_fact AS change_fact
                 JOIN semantic_event_fact_text AS event
                   ON event.sequence = change_fact.sequence
                 JOIN locator_event_text AS locator
                   ON locator.sequence = change_fact.sequence
                 WHERE locator.event_id = ?1",
                [port_event_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read materialized fact-port carrier");
        assert_eq!(
            connection
                .execute(
                    "UPDATE semantic_change_fact
                     SET fact_json = replace(fact_json, ?1, ?2)
                     WHERE json_extract(fact_json, '$.kind') = 'revision'
                       AND instr(fact_json, ?1) > 0",
                    rusqlite::params![origin_hash, wrong_hash],
                )
                .expect("inject mismatched materialized origin hash"),
            1,
        );

        let mut fact: serde_json::Value =
            serde_json::from_str(&fact_json).expect("decode materialized fact port");
        let port = fact
            .get_mut("port")
            .and_then(serde_json::Value::as_object_mut)
            .expect("fact-port payload is an object");
        port.get_mut("originRevision")
            .and_then(serde_json::Value::as_object_mut)
            .expect("origin Revision is an object")
            .insert(
                "objectArtifactContentHash".to_owned(),
                serde_json::Value::String(wrong_hash.to_owned()),
            );
        port.remove("portId");
        let port_id = format!(
            "fact-port:{}",
            canonical_json_sha256(&serde_json::json!({
                "payload": serde_json::Value::Object(port.clone()),
                "actorId": actor_id,
                "trackId": track_id,
            }))
        );
        port.insert("portId".to_owned(), serde_json::Value::String(port_id));
        assert_eq!(
            connection
                .execute(
                    "UPDATE semantic_change_fact
                     SET fact_json = ?1
                     WHERE sequence = (
                         SELECT sequence FROM locator_event_text WHERE event_id = ?2
                     )",
                    rusqlite::params![fact.to_string(), port_event_id],
                )
                .expect("bind fact port to mismatched materialized origin"),
            1,
        );
    });
}

fn with_generation_database(root: &Path, generation_id: &str, mutate: impl FnOnce(&Connection)) {
    let database = find_generation_database(root, generation_id)
        .expect("locate rebuilt exact-read fixture database");
    let connection = Connection::open(database).expect("open exact-read fixture database");
    mutate(&connection);
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .expect("checkpoint exact-read fixture mutation");
}

fn find_generation_database(root: &Path, generation_id: &str) -> Option<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(directory).ok()?;
        for entry in entries {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.file_name().and_then(|name| name.to_str()) == Some("cursor.sqlite3")
                && path
                    .parent()
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    == Some(generation_id)
            {
                return Some(path);
            }
        }
    }
    None
}

fn canonical_json_sha256(value: &serde_json::Value) -> String {
    fn canonical(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.iter().map(canonical).collect())
            }
            serde_json::Value::Object(fields) => {
                let mut keys = fields.keys().collect::<Vec<_>>();
                keys.sort_unstable();
                let mut canonical_fields = serde_json::Map::new();
                for key in keys {
                    canonical_fields.insert(
                        key.clone(),
                        canonical(fields.get(key).expect("canonical key remains present")),
                    );
                }
                serde_json::Value::Object(canonical_fields)
            }
            _ => value.clone(),
        }
    }

    let bytes = serde_json::to_vec(&canonical(value)).expect("serialize canonical fixture JSON");
    format!("sha256:{:x}", Sha256::digest(bytes))
}
