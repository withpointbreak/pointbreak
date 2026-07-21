use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use heed3::types::Bytes;
use heed3::{Database, Env, EnvOpenOptions, Error as HeedError, MdbError};
use serde::{Deserialize, Serialize};

use super::{
    IndependentContentStoreV1, LogicalCapabilityEpochV1, QUALIFICATION_LOGICAL_KEY_MAX_BYTES_V1,
    QualificationCreateOutcome, QualificationEntry, QualificationGeneratedWorkloadV1,
    QualificationInventoryV1, QualificationJournal, QualificationProfile,
    QualificationProfileDescriptorV1, QualificationRecordKindV1,
    qualification_generated_manifest_v1, qualification_generator_spec_v1,
    qualification_operation_schedule_v1,
};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};

pub const QUALIFICATION_LMDB_PLAIN_PROFILE_ID_V1: &str = "qualification-lmdb-plain-v1";
pub const QUALIFICATION_LMDB_SMOKE_SCHEMA_V1: &str = "pointbreak.qualification-lmdb-smoke.v1";

const METADATA_SCHEMA_V1: &str = "pointbreak.qualification-lmdb-plain-metadata.v1";
const DATABASE_NAME_V1: &str = "journal-v1";
const JOURNAL_DIRECTORY_V1: &str = "journal";
const CONTENT_DIRECTORY_V1: &str = "content";
const RESIZE_LOCK_FILE_V1: &str = "pointbreak-lmdb-resize-v1.lock";
const METADATA_KEY_V1: &[u8] = b"\x00metadata-v1";
const HEAD_KEY_V1: &[u8] = b"\x00head-v1";
const ENTRY_KEY_PREFIX_V1: u8 = 1;
const ENTRY_MAGIC_V1: &[u8; 4] = b"PBLJ";
const ENTRY_VERSION_V1: u8 = 1;
const HEAD_MAGIC_V1: &[u8; 4] = b"PBHD";
const HEAD_VERSION_V1: u8 = 1;
const MIB: u64 = 1024 * 1024;

type JournalDatabase = Database<Bytes, Bytes>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LmdbMapPolicyV1 {
    pub initial_size_bytes: u64,
    pub growth_increment_bytes: u64,
    pub maximum_size_bytes: u64,
    pub resize_retry_limit: u32,
}

impl Default for LmdbMapPolicyV1 {
    fn default() -> Self {
        Self {
            initial_size_bytes: 16 * MIB,
            growth_increment_bytes: 64 * MIB,
            maximum_size_bytes: 256 * MIB,
            resize_retry_limit: 4,
        }
    }
}

impl LmdbMapPolicyV1 {
    fn validate(self) -> Result<(), String> {
        if self.initial_size_bytes == 0
            || self.growth_increment_bytes == 0
            || self.maximum_size_bytes < self.initial_size_bytes
        {
            return Err("plain LMDB map policy has invalid bounds".to_owned());
        }
        for (label, value) in [
            ("initial", self.initial_size_bytes),
            ("growth", self.growth_increment_bytes),
            ("maximum", self.maximum_size_bytes),
        ] {
            if value % 65_536 != 0 {
                return Err(format!(
                    "plain LMDB {label} map size must be a multiple of 65536 bytes"
                ));
            }
        }
        Ok(())
    }

    fn next_size(self, current: u64) -> Option<u64> {
        (current < self.maximum_size_bytes).then(|| {
            current
                .saturating_add(self.growth_increment_bytes)
                .min(self.maximum_size_bytes)
        })
    }

    fn admits_size(self, size: u64) -> bool {
        size >= self.initial_size_bytes && size <= self.maximum_size_bytes
    }

    #[cfg(test)]
    fn test_resize_policy() -> Self {
        Self {
            initial_size_bytes: MIB,
            growth_increment_bytes: MIB,
            maximum_size_bytes: 8 * MIB,
            resize_retry_limit: 7,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LmdbProfileMetadataV1 {
    schema: String,
    profile_id: String,
    map_policy: LmdbMapPolicyV1,
}

impl LmdbProfileMetadataV1 {
    fn expected(map_policy: LmdbMapPolicyV1) -> Self {
        Self {
            schema: METADATA_SCHEMA_V1.to_owned(),
            profile_id: QUALIFICATION_LMDB_PLAIN_PROFILE_ID_V1.to_owned(),
            map_policy,
        }
    }

    fn encode(&self) -> Result<Vec<u8>, String> {
        let value = serde_json::to_value(self)
            .map_err(|error| format!("plain LMDB metadata serialization failed: {error}"))?;
        canonical_json_bytes(&value)
            .map_err(|error| format!("plain LMDB metadata canonicalization failed: {error}"))
    }

    fn decode(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes)
            .map_err(|error| format!("plain LMDB metadata is invalid: {error}"))
    }

    fn validate(&self, expected_policy: LmdbMapPolicyV1) -> Result<(), String> {
        if self.schema != METADATA_SCHEMA_V1 {
            return Err(format!(
                "unsupported plain LMDB metadata schema {}",
                self.schema
            ));
        }
        if self.profile_id != QUALIFICATION_LMDB_PLAIN_PROFILE_ID_V1 {
            return Err(format!(
                "stale or incompatible plain LMDB profile identity {}",
                self.profile_id
            ));
        }
        if self.map_policy != expected_policy {
            return Err("plain LMDB profile uses an incompatible fixed map policy".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualificationLmdbSmokeV1 {
    pub schema: &'static str,
    pub mode: &'static str,
    pub profile_id: String,
    pub map_policy: LmdbMapPolicyV1,
    pub workload: QualificationGeneratedWorkloadV1,
    pub manifest_sha256: String,
    pub records: u64,
    pub head_marker: u64,
    pub receipts_exact: bool,
}

#[derive(Debug)]
pub struct LmdbQualificationJournal {
    root: PathBuf,
    environment: Env,
    database: JournalDatabase,
    map_policy: LmdbMapPolicyV1,
    transaction_gate: Mutex<()>,
}

#[derive(Debug)]
pub struct LmdbQualificationProfile {
    descriptor: QualificationProfileDescriptorV1,
    journal: LmdbQualificationJournal,
    content: IndependentContentStoreV1,
}

impl LmdbQualificationProfile {
    pub fn open(root: &Path) -> Result<Self, String> {
        Self::open_with_policy(root, LmdbMapPolicyV1::default())
    }

    pub fn open_with_policy(root: &Path, map_policy: LmdbMapPolicyV1) -> Result<Self, String> {
        map_policy.validate()?;
        fs::create_dir_all(root)
            .map_err(|error| format!("plain LMDB profile root creation failed: {error}"))?;
        let journal_root = root.join(JOURNAL_DIRECTORY_V1);
        fs::create_dir_all(&journal_root)
            .map_err(|error| format!("plain LMDB journal root creation failed: {error}"))?;
        let map_size = usize::try_from(map_policy.initial_size_bytes)
            .map_err(|_| "plain LMDB initial map size exceeds this platform".to_owned())?;
        let mut options = EnvOpenOptions::new();
        options.map_size(map_size).max_dbs(1);
        // SAFETY: the profile owns this stable directory for the environment's
        // lifetime. Reopens in other processes use the same fixed policy.
        let environment = unsafe { options.open(&journal_root) }
            .map_err(|error| format!("plain LMDB environment open failed: {error}"))?;
        let database = initialize_database(&environment, map_policy)?;
        let journal = LmdbQualificationJournal {
            root: root.to_path_buf(),
            environment,
            database,
            map_policy,
            transaction_gate: Mutex::new(()),
        };
        journal.validate_current_map_size()?;
        let content = IndependentContentStoreV1::open(&root.join(CONTENT_DIRECTORY_V1))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            descriptor: QualificationProfileDescriptorV1 {
                physical_profile_id: QUALIFICATION_LMDB_PLAIN_PROFILE_ID_V1.to_owned(),
                logical_capabilities: LogicalCapabilityEpochV1::foundation(),
            },
            journal,
            content,
        })
    }

    pub fn current_map_size_bytes(&self) -> u64 {
        self.journal.environment.info().map_size as u64
    }
}

fn initialize_database(
    environment: &Env,
    map_policy: LmdbMapPolicyV1,
) -> Result<JournalDatabase, String> {
    let mut refreshes = 0;
    loop {
        let mut transaction = match environment.write_txn() {
            Ok(transaction) => transaction,
            Err(error) if is_map_resized(&error) && refreshes < map_policy.resize_retry_limit => {
                refresh_environment_map(environment)?;
                refreshes += 1;
                continue;
            }
            Err(error) => return Err(format!("plain LMDB initialization failed: {error}")),
        };
        let database: JournalDatabase = environment
            .create_database(&mut transaction, Some(DATABASE_NAME_V1))
            .map_err(|error| format!("plain LMDB journal database open failed: {error}"))?;
        let expected = LmdbProfileMetadataV1::expected(map_policy);
        match database
            .get(&transaction, METADATA_KEY_V1)
            .map_err(|error| format!("plain LMDB metadata read failed: {error}"))?
        {
            Some(bytes) => LmdbProfileMetadataV1::decode(bytes)?.validate(map_policy)?,
            None => {
                if database.len(&transaction).map_err(|error| {
                    format!("plain LMDB initialization inspection failed: {error}")
                })? != 0
                {
                    return Err("plain LMDB journal contains entries without metadata".to_owned());
                }
                database
                    .put(&mut transaction, METADATA_KEY_V1, &expected.encode()?)
                    .map_err(|error| format!("plain LMDB metadata write failed: {error}"))?;
                database
                    .put(&mut transaction, HEAD_KEY_V1, &encode_head(0))
                    .map_err(|error| format!("plain LMDB head initialization failed: {error}"))?;
            }
        }
        let head = database
            .get(&transaction, HEAD_KEY_V1)
            .map_err(|error| format!("plain LMDB head read failed: {error}"))?
            .ok_or_else(|| "plain LMDB journal metadata omitted the head marker".to_owned())?;
        decode_head(head)?;
        transaction
            .commit()
            .map_err(|error| format!("plain LMDB initialization commit failed: {error}"))?;
        return Ok(database);
    }
}

impl LmdbQualificationJournal {
    fn gate(&self) -> Result<MutexGuard<'_, ()>, String> {
        self.transaction_gate
            .lock()
            .map_err(|_| "plain LMDB transaction gate is poisoned".to_owned())
    }

    fn validate_current_map_size(&self) -> Result<(), String> {
        let size = self.environment.info().map_size as u64;
        if !self.map_policy.admits_size(size) {
            return Err(format!(
                "plain LMDB environment map size {size} is outside the fixed policy"
            ));
        }
        Ok(())
    }

    fn refresh_map(&self) -> Result<(), String> {
        with_resize_lock(&self.root, || refresh_environment_map(&self.environment))?;
        self.validate_current_map_size()
    }

    fn grow_map(&self) -> Result<(), String> {
        with_resize_lock(&self.root, || {
            let current = self.environment.info().map_size as u64;
            if !self.map_policy.admits_size(current) {
                return Err(format!(
                    "plain LMDB environment map size {current} is outside the fixed policy"
                ));
            }
            let Some(next) = self.map_policy.next_size(current) else {
                return Err(format!(
                    "plain LMDB map full at fixed ceiling {current} bytes"
                ));
            };
            let next = usize::try_from(next)
                .map_err(|_| "plain LMDB map ceiling exceeds this platform".to_owned())?;
            // SAFETY: every operation in this process holds transaction_gate,
            // so no local transaction is active; the file lock serializes
            // cross-process resize decisions.
            unsafe { self.environment.resize(next) }
                .map_err(|error| format!("plain LMDB map resize failed: {error}"))
        })
    }

    fn read_transaction<T>(
        &self,
        mut operation: impl FnMut(&heed3::RoTxn<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        for refresh in 0..=self.map_policy.resize_retry_limit {
            match self.environment.read_txn() {
                Ok(transaction) => return operation(&transaction),
                Err(error)
                    if is_map_resized(&error) && refresh < self.map_policy.resize_retry_limit =>
                {
                    self.refresh_map()?;
                }
                Err(error) => return Err(format!("plain LMDB read transaction failed: {error}")),
            }
        }
        Err("plain LMDB read transaction exceeded the map refresh retry limit".to_owned())
    }

    fn list_in_transaction(
        &self,
        transaction: &heed3::RoTxn<'_>,
    ) -> Result<Vec<QualificationEntry>, String> {
        let mut entries = Vec::new();
        let iterator = self
            .database
            .iter(transaction)
            .map_err(|error| format!("plain LMDB replay cursor failed: {error}"))?;
        for result in iterator {
            let (key, value) =
                result.map_err(|error| format!("plain LMDB replay failed: {error}"))?;
            if key == METADATA_KEY_V1 || key == HEAD_KEY_V1 {
                continue;
            }
            let logical_key = decode_entry_key(key)?;
            entries.push(decode_entry(&logical_key, value)?);
        }
        Ok(entries)
    }

    fn head_in_transaction(&self, transaction: &heed3::RoTxn<'_>) -> Result<u64, String> {
        let bytes = self
            .database
            .get(transaction, HEAD_KEY_V1)
            .map_err(|error| format!("plain LMDB head read failed: {error}"))?
            .ok_or_else(|| "plain LMDB head marker is missing".to_owned())?;
        decode_head(bytes)
    }
}

impl QualificationJournal for LmdbQualificationJournal {
    fn create_once(
        &self,
        logical_key: &str,
        decoded_bytes: &[u8],
    ) -> Result<QualificationCreateOutcome, String> {
        validate_logical_key(logical_key)?;
        let key = encode_entry_key(logical_key);
        let envelope = encode_entry(decoded_bytes)?;
        let _gate = self.gate()?;
        let mut resizes = 0;
        let mut refreshes = 0;
        loop {
            let mut transaction = match self.environment.write_txn() {
                Ok(transaction) => transaction,
                Err(error)
                    if is_map_resized(&error) && refreshes < self.map_policy.resize_retry_limit =>
                {
                    self.refresh_map()?;
                    refreshes += 1;
                    continue;
                }
                Err(error) => return Err(format!("plain LMDB write transaction failed: {error}")),
            };
            let existing = match self.database.get(&transaction, &key) {
                Ok(existing) => existing,
                Err(error) => {
                    return Err(format!("plain LMDB existing-value read failed: {error}"));
                }
            };
            if let Some(existing) = existing {
                let existing = decode_entry(logical_key, existing)?;
                return if existing.decoded_bytes == decoded_bytes {
                    Ok(QualificationCreateOutcome::AlreadyExists)
                } else {
                    Err(format!(
                        "plain LMDB create conflict for logical key {logical_key}"
                    ))
                };
            }
            let head = self
                .database
                .get(&transaction, HEAD_KEY_V1)
                .map_err(|error| format!("plain LMDB head read failed: {error}"))?
                .ok_or_else(|| "plain LMDB head marker is missing".to_owned())?;
            let next_head = decode_head(head)?
                .checked_add(1)
                .ok_or_else(|| "plain LMDB head marker overflow".to_owned())?;
            let attempt = self
                .database
                .put(&mut transaction, &key, &envelope)
                .and_then(|()| {
                    self.database
                        .put(&mut transaction, HEAD_KEY_V1, &encode_head(next_head))
                })
                .and_then(|()| transaction.commit());
            match attempt {
                Ok(()) => return Ok(QualificationCreateOutcome::Created),
                Err(error) if is_map_full(&error) => {
                    if resizes >= self.map_policy.resize_retry_limit {
                        return Err(format!(
                            "plain LMDB map full after {resizes} bounded resize attempts"
                        ));
                    }
                    self.grow_map()?;
                    resizes += 1;
                }
                Err(error)
                    if is_map_resized(&error) && refreshes < self.map_policy.resize_retry_limit =>
                {
                    self.refresh_map()?;
                    refreshes += 1;
                }
                Err(error) if is_map_resized(&error) => {
                    return Err(format!(
                        "plain LMDB write exceeded the map refresh retry limit: {error}"
                    ));
                }
                Err(error) => return Err(format!("plain LMDB durable commit failed: {error}")),
            }
        }
    }

    fn read(&self, logical_key: &str) -> Result<Option<QualificationEntry>, String> {
        validate_logical_key(logical_key)?;
        let key = encode_entry_key(logical_key);
        let _gate = self.gate()?;
        self.read_transaction(|transaction| {
            self.database
                .get(transaction, &key)
                .map_err(|error| format!("plain LMDB keyed read failed: {error}"))?
                .map(|bytes| decode_entry(logical_key, bytes))
                .transpose()
        })
    }

    fn list(&self) -> Result<Vec<QualificationEntry>, String> {
        let _gate = self.gate()?;
        self.read_transaction(|transaction| self.list_in_transaction(transaction))
    }

    fn head_marker(&self) -> Result<u64, String> {
        let _gate = self.gate()?;
        self.read_transaction(|transaction| self.head_in_transaction(transaction))
    }

    fn integrity_check(&self) -> Result<(), String> {
        let _gate = self.gate()?;
        self.read_transaction(|transaction| {
            let entries = self.list_in_transaction(transaction)?;
            let head = self.head_in_transaction(transaction)?;
            if entries.len() as u64 != head {
                return Err(format!(
                    "plain LMDB head marker {head} does not match {} entries",
                    entries.len()
                ));
            }
            Ok(())
        })
    }
}

impl QualificationProfile for LmdbQualificationProfile {
    fn descriptor(&self) -> Result<QualificationProfileDescriptorV1, String> {
        Ok(self.descriptor.clone())
    }

    fn journal(&self) -> &dyn QualificationJournal {
        &self.journal
    }

    fn put_content_once(
        &self,
        content_key: &str,
        record_kind: QualificationRecordKindV1,
        decoded_bytes: &[u8],
    ) -> Result<QualificationCreateOutcome, String> {
        self.content
            .put_once(content_key, record_kind, decoded_bytes)
            .map_err(|error| error.to_string())
    }

    fn read_content(&self, content_key: &str) -> Result<Option<QualificationEntry>, String> {
        self.content
            .read(content_key)
            .map_err(|error| error.to_string())
    }

    fn remove_content(&self, content_key: &str) -> Result<bool, String> {
        self.content
            .remove(content_key)
            .map_err(|error| error.to_string())
    }

    fn backup_to(&self, _destination: &Path) -> Result<(), String> {
        Err("plain LMDB online copy is unavailable until the lifecycle proof".to_owned())
    }

    fn verify_restore(&self, _restored_root: &Path) -> Result<(), String> {
        Err("plain LMDB restore verification is unavailable until the lifecycle proof".to_owned())
    }

    fn inventory(&self) -> Result<QualificationInventoryV1, String> {
        Err("plain LMDB exhaustive inventory is unavailable until the lifecycle proof".to_owned())
    }
}

pub fn run_qualification_lmdb_smoke_v1(root: &Path) -> Result<QualificationLmdbSmokeV1, String> {
    let map_policy = LmdbMapPolicyV1::default();
    let profile = LmdbQualificationProfile::open_with_policy(root, map_policy)?;
    let spec = qualification_generator_spec_v1(QualificationGeneratedWorkloadV1::G0);
    let manifest = qualification_generated_manifest_v1(&spec).map_err(|error| error.to_string())?;
    for record in &manifest.records {
        if profile
            .journal()
            .create_once(&record.logical_key, &record.decoded_bytes)?
            != QualificationCreateOutcome::Created
        {
            return Err("plain LMDB smoke encountered a pre-existing generated record".to_owned());
        }
    }
    let listed = profile.journal().list()?;
    let receipts_exact = listed.len() == manifest.records.len()
        && listed.iter().zip(&manifest.records).all(|(entry, record)| {
            entry.logical_key == record.logical_key
                && entry.decoded_sha256 == record.decoded_sha256
                && entry.decoded_bytes == record.decoded_bytes
        });
    if !receipts_exact {
        return Err("plain LMDB smoke replay receipts are not exact".to_owned());
    }
    let schedule = qualification_operation_schedule_v1(&spec).map_err(|error| error.to_string())?;
    for scheduled in schedule.keyed_reads {
        let actual = profile.journal().read(&scheduled.logical_key)?;
        if matches!(
            scheduled.class,
            super::QualificationKeyedReadClassV1::Absent
        ) != actual.is_none()
        {
            return Err(format!(
                "plain LMDB smoke scheduled read {:?} returned the wrong presence",
                scheduled.class
            ));
        }
    }
    profile.journal().integrity_check()?;
    Ok(QualificationLmdbSmokeV1 {
        schema: QUALIFICATION_LMDB_SMOKE_SCHEMA_V1,
        mode: "non_timing_semantic_receipts",
        profile_id: QUALIFICATION_LMDB_PLAIN_PROFILE_ID_V1.to_owned(),
        map_policy,
        workload: QualificationGeneratedWorkloadV1::G0,
        manifest_sha256: manifest.manifest_sha256,
        records: listed.len() as u64,
        head_marker: profile.journal().head_marker()?,
        receipts_exact,
    })
}

fn validate_logical_key(logical_key: &str) -> Result<(), String> {
    if logical_key.is_empty() {
        return Err("plain LMDB logical key must not be empty".to_owned());
    }
    if logical_key.len() > QUALIFICATION_LOGICAL_KEY_MAX_BYTES_V1 {
        return Err(format!(
            "plain LMDB logical key exceeds the public {}-byte contract",
            QUALIFICATION_LOGICAL_KEY_MAX_BYTES_V1
        ));
    }
    Ok(())
}

fn encode_entry_key(logical_key: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(logical_key.len() + 1);
    key.push(ENTRY_KEY_PREFIX_V1);
    key.extend_from_slice(logical_key.as_bytes());
    key
}

fn decode_entry_key(key: &[u8]) -> Result<String, String> {
    let Some((&prefix, logical_key)) = key.split_first() else {
        return Err("plain LMDB journal contains an empty key".to_owned());
    };
    if prefix != ENTRY_KEY_PREFIX_V1 {
        return Err("plain LMDB journal contains an unknown internal key".to_owned());
    }
    let logical_key = std::str::from_utf8(logical_key)
        .map_err(|_| "plain LMDB journal key is not UTF-8".to_owned())?;
    validate_logical_key(logical_key)?;
    Ok(logical_key.to_owned())
}

fn encode_entry(decoded_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let decoded_len = u64::try_from(decoded_bytes.len())
        .map_err(|_| "plain LMDB entry length exceeds u64".to_owned())?;
    let hash = sha256_bytes_hex(decoded_bytes);
    let mut envelope = Vec::with_capacity(4 + 1 + 8 + 64 + decoded_bytes.len());
    envelope.extend_from_slice(ENTRY_MAGIC_V1);
    envelope.push(ENTRY_VERSION_V1);
    envelope.extend_from_slice(&decoded_len.to_be_bytes());
    envelope.extend_from_slice(hash.as_bytes());
    envelope.extend_from_slice(decoded_bytes);
    Ok(envelope)
}

fn decode_entry(logical_key: &str, envelope: &[u8]) -> Result<QualificationEntry, String> {
    const HEADER_LEN: usize = 4 + 1 + 8 + 64;
    if envelope.len() < HEADER_LEN
        || &envelope[..4] != ENTRY_MAGIC_V1
        || envelope[4] != ENTRY_VERSION_V1
    {
        return Err(format!(
            "plain LMDB value for {logical_key} has an invalid envelope"
        ));
    }
    let decoded_len = u64::from_be_bytes(
        envelope[5..13]
            .try_into()
            .expect("entry length slice has eight bytes"),
    );
    let decoded_bytes = &envelope[HEADER_LEN..];
    if decoded_len != decoded_bytes.len() as u64 {
        return Err(format!(
            "plain LMDB value for {logical_key} has a mismatched decoded length"
        ));
    }
    let stored_hash = std::str::from_utf8(&envelope[13..HEADER_LEN])
        .map_err(|_| format!("plain LMDB value for {logical_key} has a non-text hash"))?;
    let actual_hash = sha256_bytes_hex(decoded_bytes);
    if stored_hash != actual_hash {
        return Err(format!(
            "plain LMDB value for {logical_key} has decoded hash {actual_hash}, expected {stored_hash}"
        ));
    }
    Ok(QualificationEntry {
        logical_key: logical_key.to_owned(),
        decoded_sha256: actual_hash,
        decoded_bytes: decoded_bytes.to_vec(),
    })
}

fn encode_head(head: u64) -> [u8; 13] {
    let mut encoded = [0_u8; 13];
    encoded[..4].copy_from_slice(HEAD_MAGIC_V1);
    encoded[4] = HEAD_VERSION_V1;
    encoded[5..].copy_from_slice(&head.to_be_bytes());
    encoded
}

fn decode_head(encoded: &[u8]) -> Result<u64, String> {
    if encoded.len() != 13 || &encoded[..4] != HEAD_MAGIC_V1 || encoded[4] != HEAD_VERSION_V1 {
        return Err("plain LMDB head marker has an invalid envelope".to_owned());
    }
    Ok(u64::from_be_bytes(
        encoded[5..]
            .try_into()
            .expect("head marker slice has eight bytes"),
    ))
}

fn is_map_full(error: &HeedError) -> bool {
    matches!(error, HeedError::Mdb(MdbError::MapFull))
}

fn is_map_resized(error: &HeedError) -> bool {
    matches!(error, HeedError::Mdb(MdbError::MapResized))
}

fn refresh_environment_map(environment: &Env) -> Result<(), String> {
    // SAFETY: callers serialize all local transactions before adopting the
    // map size persisted by another process.
    unsafe { environment.resize(0) }
        .map_err(|error| format!("plain LMDB map refresh failed: {error}"))
}

fn with_resize_lock<T>(
    root: &Path,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let path = root.join(RESIZE_LOCK_FILE_V1);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| format!("plain LMDB resize lock open failed: {error}"))?;
    file.lock()
        .map_err(|error| format!("plain LMDB resize lock failed: {error}"))?;
    let result = operation();
    let unlock = file
        .unlock()
        .map_err(|error| format!("plain LMDB resize unlock failed: {error}"));
    match (result, unlock) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
    }
}

#[cfg(test)]
fn overwrite_raw_journal_value_for_test(
    root: &Path,
    logical_key: &str,
    value: &[u8],
) -> Result<(), String> {
    mutate_raw_database_for_test(root, |database, transaction| {
        database
            .put(transaction, &encode_entry_key(logical_key), value)
            .map_err(|error| error.to_string())
    })
}

#[cfg(test)]
fn overwrite_profile_id_for_test(root: &Path, profile_id: &str) -> Result<(), String> {
    mutate_raw_database_for_test(root, |database, transaction| {
        let bytes = database
            .get(transaction, METADATA_KEY_V1)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "test metadata missing".to_owned())?;
        let mut metadata = LmdbProfileMetadataV1::decode(bytes)?;
        metadata.profile_id = profile_id.to_owned();
        database
            .put(transaction, METADATA_KEY_V1, &metadata.encode()?)
            .map_err(|error| error.to_string())
    })
}

#[cfg(test)]
fn mutate_raw_database_for_test(
    root: &Path,
    mutation: impl FnOnce(&JournalDatabase, &mut heed3::RwTxn<'_>) -> Result<(), String>,
) -> Result<(), String> {
    let mut options = EnvOpenOptions::new();
    options
        .map_size(LmdbMapPolicyV1::default().initial_size_bytes as usize)
        .max_dbs(1);
    // SAFETY: tests call this only after dropping the profile handle.
    let environment = unsafe { options.open(root.join(JOURNAL_DIRECTORY_V1)) }
        .map_err(|error| error.to_string())?;
    let mut transaction = environment.write_txn().map_err(|error| error.to_string())?;
    let database: JournalDatabase = environment
        .create_database(&mut transaction, Some(DATABASE_NAME_V1))
        .map_err(|error| error.to_string())?;
    mutation(&database, &mut transaction)?;
    transaction.commit().map_err(|error| error.to_string())?;
    environment.prepare_for_closing().wait();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command};
    use std::time::Duration;

    use super::*;
    use crate::bench_support::foundation::{
        QualificationCreateOutcome, QualificationGeneratedWorkloadV1,
        QualificationKeyedReadClassV1, QualificationProcessBarrierParticipantV1,
        QualificationProcessBarrierV1, qualification_generated_manifest_v1,
        qualification_generator_spec_v1, qualification_operation_schedule_v1,
    };

    const CHILD_TEST: &str =
        "bench_support::foundation::lmdb::tests::lmdb_process_child_entrypoint";

    fn spawn_child(
        action: &str,
        root: &Path,
        key: &str,
        bytes: &[u8],
        result: &Path,
        barrier: Option<(&Path, &str)>,
    ) -> Child {
        let mut command = Command::new(std::env::current_exe().expect("current test executable"));
        command
            .args(["--exact", CHILD_TEST, "--nocapture"])
            .env("POINTBREAK_LMDB_CHILD_ACTION", action)
            .env("POINTBREAK_LMDB_CHILD_ROOT", root)
            .env("POINTBREAK_LMDB_CHILD_KEY", key)
            .env(
                "POINTBREAK_LMDB_CHILD_BYTES",
                String::from_utf8_lossy(bytes).as_ref(),
            )
            .env("POINTBREAK_LMDB_CHILD_RESULT", result);
        if let Some((barrier_root, participant)) = barrier {
            command
                .env("POINTBREAK_LMDB_CHILD_BARRIER", barrier_root)
                .env("POINTBREAK_LMDB_CHILD_PARTICIPANT", participant);
        }
        command.spawn().expect("spawn LMDB process child")
    }

    fn wait_success(mut child: Child) {
        let status = child.wait().expect("wait for LMDB process child");
        assert!(status.success(), "LMDB process child failed: {status}");
    }

    fn child_result(path: &Path) -> String {
        std::fs::read_to_string(path).expect("read child result")
    }

    #[test]
    fn lmdb_process_child_entrypoint() {
        let Some(action) = std::env::var_os("POINTBREAK_LMDB_CHILD_ACTION") else {
            return;
        };
        let root = PathBuf::from(std::env::var_os("POINTBREAK_LMDB_CHILD_ROOT").unwrap());
        let key = std::env::var("POINTBREAK_LMDB_CHILD_KEY").unwrap();
        let bytes = std::env::var("POINTBREAK_LMDB_CHILD_BYTES")
            .unwrap()
            .into_bytes();
        let result = PathBuf::from(std::env::var_os("POINTBREAK_LMDB_CHILD_RESULT").unwrap());
        let profile = if action.to_string_lossy().starts_with("refresh_") {
            LmdbQualificationProfile::open_with_policy(&root, LmdbMapPolicyV1::test_resize_policy())
        } else {
            LmdbQualificationProfile::open(&root)
        }
        .expect("open child LMDB profile");
        let participant = std::env::var_os("POINTBREAK_LMDB_CHILD_BARRIER").map(|barrier| {
            QualificationProcessBarrierParticipantV1::join(
                barrier,
                &std::env::var("POINTBREAK_LMDB_CHILD_PARTICIPANT").unwrap(),
            )
            .expect("join LMDB child barrier")
        });
        if let Some(participant) = &participant {
            participant
                .wait_for_release(Duration::from_secs(20))
                .expect("wait for LMDB child release");
        }
        let output = match action.to_string_lossy().as_ref() {
            "create" | "refresh_write" => profile
                .journal()
                .create_once(&key, &bytes)
                .map(|outcome| format!("{outcome:?}"))
                .unwrap_or_else(|error| format!("error:{error}")),
            "read" | "refresh_read" => profile
                .journal()
                .read(&key)
                .map(|entry| {
                    entry
                        .map(|entry| String::from_utf8(entry.decoded_bytes).unwrap())
                        .unwrap_or_else(|| "absent".to_owned())
                })
                .unwrap_or_else(|error| format!("error:{error}")),
            other => panic!("unknown child action {other}"),
        };
        std::fs::write(&result, output).expect("write child result");
        if let Some(participant) = participant {
            participant.complete().expect("complete LMDB child barrier");
        }
    }

    #[test]
    fn create_once_retry_and_commit_acknowledgement_survive_fresh_processes() {
        let root = tempfile::tempdir().expect("LMDB process root");
        let results = tempfile::tempdir().expect("LMDB process results");

        let created = results.path().join("created");
        wait_success(spawn_child(
            "create",
            root.path(),
            "journal/key",
            b"value",
            &created,
            None,
        ));
        assert_eq!(child_result(&created), "Created");

        let reopened = results.path().join("reopened");
        wait_success(spawn_child(
            "read",
            root.path(),
            "journal/key",
            b"",
            &reopened,
            None,
        ));
        assert_eq!(child_result(&reopened), "value");

        let exact_retry = results.path().join("exact-retry");
        wait_success(spawn_child(
            "create",
            root.path(),
            "journal/key",
            b"value",
            &exact_retry,
            None,
        ));
        assert_eq!(child_result(&exact_retry), "AlreadyExists");

        let divergent_retry = results.path().join("divergent-retry");
        wait_success(spawn_child(
            "create",
            root.path(),
            "journal/key",
            b"different",
            &divergent_retry,
            None,
        ));
        assert!(child_result(&divergent_retry).starts_with("error:"));

        let profile = LmdbQualificationProfile::open(root.path()).expect("reopen profile");
        assert_eq!(profile.journal().head_marker().unwrap(), 1);
    }

    #[test]
    fn synchronized_independent_writers_have_exactly_one_winner() {
        let root = tempfile::tempdir().expect("LMDB race root");
        let results = tempfile::tempdir().expect("LMDB race results");
        let barrier_root = results.path().join("barrier");
        std::fs::create_dir(&barrier_root).expect("create writer barrier root");
        let barrier =
            QualificationProcessBarrierV1::create(&barrier_root, &["writer-a", "writer-b"])
                .expect("create writer barrier");
        let first_result = results.path().join("writer-a");
        let second_result = results.path().join("writer-b");
        let first = spawn_child(
            "create",
            root.path(),
            "journal/race",
            b"same",
            &first_result,
            Some((&barrier_root, "writer-a")),
        );
        let second = spawn_child(
            "create",
            root.path(),
            "journal/race",
            b"same",
            &second_result,
            Some((&barrier_root, "writer-b")),
        );
        barrier
            .wait_until_ready(Duration::from_secs(20))
            .expect("both writers ready");
        barrier.release().expect("release writers");
        wait_success(first);
        wait_success(second);
        barrier
            .evidence()
            .expect("race evidence")
            .validate_overlap()
            .unwrap();

        let mut outcomes = [child_result(&first_result), child_result(&second_result)];
        outcomes.sort();
        assert_eq!(outcomes, ["AlreadyExists", "Created"]);
        let profile = LmdbQualificationProfile::open(root.path()).expect("reopen race profile");
        assert_eq!(profile.journal().head_marker().unwrap(), 1);
    }

    #[test]
    fn replay_reads_hashes_and_head_marker_are_exact_and_deterministic() {
        let root = tempfile::tempdir().expect("LMDB semantic root");
        let profile = LmdbQualificationProfile::open(root.path()).expect("open LMDB profile");
        let spec = qualification_generator_spec_v1(QualificationGeneratedWorkloadV1::G0);
        let manifest = qualification_generated_manifest_v1(&spec).expect("generate G0");
        for record in manifest.records.iter().rev() {
            assert_eq!(
                profile
                    .journal()
                    .create_once(&record.logical_key, &record.decoded_bytes)
                    .unwrap(),
                QualificationCreateOutcome::Created
            );
        }
        let listed = profile.journal().list().expect("list LMDB journal");
        assert_eq!(listed.len(), manifest.records.len());
        assert!(
            listed
                .windows(2)
                .all(|pair| pair[0].logical_key < pair[1].logical_key)
        );
        assert!(listed.iter().all(|entry| {
            crate::canonical_hash::sha256_bytes_hex(&entry.decoded_bytes) == entry.decoded_sha256
        }));
        assert_eq!(
            profile.journal().head_marker().unwrap(),
            manifest.records.len() as u64
        );

        let schedule = qualification_operation_schedule_v1(&spec).expect("G0 schedule");
        for read in schedule.keyed_reads {
            let entry = profile
                .journal()
                .read(&read.logical_key)
                .expect("scheduled read");
            match read.class {
                QualificationKeyedReadClassV1::Absent => assert!(entry.is_none()),
                _ => assert!(entry.is_some()),
            }
        }
        profile
            .journal()
            .integrity_check()
            .expect("integrity check");
    }

    #[test]
    fn invalid_envelope_and_stale_metadata_fail_without_partial_truth() {
        let root = tempfile::tempdir().expect("LMDB corruption root");
        let profile = LmdbQualificationProfile::open(root.path()).expect("open LMDB profile");
        profile
            .journal()
            .create_once("journal/a", b"valid")
            .unwrap();
        drop(profile);

        overwrite_raw_journal_value_for_test(root.path(), "journal/z", b"invalid-envelope")
            .expect("inject invalid envelope");
        let profile = LmdbQualificationProfile::open(root.path()).expect("reopen corrupt profile");
        assert!(profile.journal().list().is_err());
        drop(profile);

        overwrite_profile_id_for_test(root.path(), "qualification-lmdb-plain-stale")
            .expect("inject stale profile identity");
        assert!(LmdbQualificationProfile::open(root.path()).is_err());
    }

    #[test]
    fn map_full_aborts_without_acknowledging_a_partial_write() {
        let root = tempfile::tempdir().expect("LMDB map-full root");
        let policy = LmdbMapPolicyV1 {
            initial_size_bytes: 1_048_576,
            growth_increment_bytes: 1_048_576,
            maximum_size_bytes: 1_048_576,
            resize_retry_limit: 0,
        };
        let profile =
            LmdbQualificationProfile::open_with_policy(root.path(), policy).expect("small profile");
        let error = profile
            .journal()
            .create_once("journal/too-large", &vec![0x5a; 2 * 1_048_576])
            .unwrap_err();
        assert!(error.contains("map full"));
        drop(profile);

        let reopened = LmdbQualificationProfile::open_with_policy(root.path(), policy).unwrap();
        assert_eq!(reopened.journal().head_marker().unwrap(), 0);
        assert!(
            reopened
                .journal()
                .read("journal/too-large")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn bounded_resize_refreshes_already_open_reader_and_writer_processes() {
        let root = tempfile::tempdir().expect("LMDB resize root");
        let results = tempfile::tempdir().expect("LMDB resize results");
        let policy = LmdbMapPolicyV1::test_resize_policy();
        let profile = LmdbQualificationProfile::open_with_policy(root.path(), policy)
            .expect("resize profile");
        profile
            .journal()
            .create_once("journal/seed", b"seed")
            .unwrap();

        let barrier_root = results.path().join("barrier");
        std::fs::create_dir(&barrier_root).expect("create refresh barrier root");
        let barrier = QualificationProcessBarrierV1::create(&barrier_root, &["reader", "writer"])
            .expect("create refresh barrier");
        let reader_result = results.path().join("reader");
        let writer_result = results.path().join("writer");
        let reader = spawn_child(
            "refresh_read",
            root.path(),
            "journal/grown",
            b"",
            &reader_result,
            Some((&barrier_root, "reader")),
        );
        let writer = spawn_child(
            "refresh_write",
            root.path(),
            "journal/after-resize",
            b"writer",
            &writer_result,
            Some((&barrier_root, "writer")),
        );
        barrier.wait_until_ready(Duration::from_secs(20)).unwrap();
        profile
            .journal()
            .create_once("journal/grown", &vec![0x33; 2 * 1_048_576])
            .expect("grow map");
        assert!(profile.current_map_size_bytes() > policy.initial_size_bytes);
        assert!(profile.current_map_size_bytes() <= policy.maximum_size_bytes);
        barrier.release().unwrap();
        wait_success(reader);
        wait_success(writer);
        assert_eq!(child_result(&reader_result).len(), 2 * 1_048_576);
        assert_eq!(child_result(&writer_result), "Created");
        assert!(
            profile
                .journal()
                .read("journal/after-resize")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn profile_open_rejects_an_incompatible_fixed_map_policy() {
        let root = tempfile::tempdir().expect("LMDB policy root");
        let profile = LmdbQualificationProfile::open(root.path()).expect("default policy profile");
        drop(profile);
        let mut incompatible = LmdbMapPolicyV1::default();
        incompatible.maximum_size_bytes += incompatible.growth_increment_bytes;

        assert!(LmdbQualificationProfile::open_with_policy(root.path(), incompatible).is_err());
    }

    #[test]
    fn content_stays_independent_and_lifecycle_operations_fail_closed() {
        let root = tempfile::tempdir().expect("LMDB content root");
        let profile = LmdbQualificationProfile::open(root.path()).expect("LMDB profile");
        let key = "sha256:0000000000000000000000000000000000000000000000000000000000000001";

        assert_eq!(
            profile
                .put_content_once(
                    key,
                    crate::bench_support::foundation::QualificationRecordKindV1::ObjectArtifact,
                    b"object",
                )
                .unwrap(),
            QualificationCreateOutcome::Created
        );
        assert_eq!(
            profile.read_content(key).unwrap().unwrap().decoded_bytes,
            b"object"
        );
        assert_eq!(profile.journal().head_marker().unwrap(), 0);
        assert!(root.path().join("content").is_dir());
        assert!(profile.backup_to(&root.path().join("backup")).is_err());
        assert!(profile.verify_restore(root.path()).is_err());
        assert!(profile.inventory().is_err());
    }

    #[test]
    fn g0_smoke_is_non_timing_and_uses_the_plain_profile_identity() {
        let root = tempfile::tempdir().expect("LMDB smoke root");
        let report = run_qualification_lmdb_smoke_v1(root.path()).expect("LMDB G0 smoke");

        assert_eq!(report.schema, "pointbreak.qualification-lmdb-smoke.v1");
        assert_eq!(report.mode, "non_timing_semantic_receipts");
        assert_eq!(report.profile_id, QUALIFICATION_LMDB_PLAIN_PROFILE_ID_V1);
        assert_eq!(report.workload, QualificationGeneratedWorkloadV1::G0);
        assert_eq!(report.records, 128);
        assert_eq!(report.head_marker, 128);
        assert!(report.receipts_exact);
    }
}
