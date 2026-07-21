use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use heed3::types::Bytes;
use heed3::{
    CompactionOption, Database, Env, EnvFlags, EnvOpenOptions, Error as HeedError, MdbError,
};
use serde::{Deserialize, Serialize};

use super::{
    IndependentContentStoreV1, LogicalCapabilityEpochV1, QUALIFICATION_LOGICAL_KEY_MAX_BYTES_V1,
    QualificationCreateOutcome, QualificationEntry, QualificationGeneratedWorkloadV1,
    QualificationInventoryV1, QualificationJournal, QualificationPerformanceInventoryStateV1,
    QualificationPerformanceInventoryV2, QualificationProcessOverlapEvidenceV1,
    QualificationProfile, QualificationProfileDescriptorV1, QualificationRecordKindV1,
    publish_completed_backup, qualification_generated_manifest_v1, qualification_generator_spec_v1,
    qualification_operation_schedule_v1, verify_completed_backup,
};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};

pub const QUALIFICATION_LMDB_PLAIN_PROFILE_ID_V1: &str = "qualification-lmdb-plain-v1";
pub const QUALIFICATION_LMDB_SMOKE_SCHEMA_V1: &str = "pointbreak.qualification-lmdb-smoke.v1";
pub const QUALIFICATION_LMDB_LIFECYCLE_SMOKE_SCHEMA_V1: &str =
    "pointbreak.qualification-lmdb-lifecycle-smoke.v1";
pub const QUALIFICATION_LMDB_LIFECYCLE_SMOKE_MODE_V1: &str = "--lmdb-lifecycle-smoke";
pub const QUALIFICATION_LMDB_LIFECYCLE_REPORT_MODE_V1: &str = "non_timing_lifecycle_receipts";
pub const LIFECYCLE_READER_RETENTION_BOUND_BYTES_V1: u64 = 16 * MIB;
pub const LIFECYCLE_POST_RELEASE_REUSE_BOUND_BYTES_V1: u64 = 2 * MIB;

const METADATA_SCHEMA_V1: &str = "pointbreak.qualification-lmdb-plain-metadata.v1";
const DATABASE_NAME_V1: &str = "journal-v1";
const JOURNAL_DIRECTORY_V1: &str = "journal";
const CONTENT_DIRECTORY_V1: &str = "content";
const RESIZE_LOCK_FILE_V1: &str = "pointbreak-lmdb-resize-v1.lock";
const LMDB_BACKUP_RECEIPT_SCHEMA_V1: &str = "pointbreak.qualification-lmdb-backup-receipt.v1";
const LMDB_BACKUP_DATABASE_FILE_V1: &str = "journal/data.mdb";
const LMDB_BACKUP_RECEIPT_FILE_V1: &str = "pointbreak-lmdb-receipt-v1.json";
const METADATA_KEY_V1: &[u8] = b"\x00metadata-v1";
const HEAD_KEY_V1: &[u8] = b"\x00head-v1";
const ENTRY_KEY_PREFIX_V1: u8 = 1;
const ENTRY_MAGIC_V1: &[u8; 4] = b"PBLJ";
const ENTRY_VERSION_V1: u8 = 1;
const HEAD_MAGIC_V1: &[u8; 4] = b"PBHD";
const HEAD_VERSION_V1: u8 = 1;
const MIB: u64 = 1024 * 1024;
static REPAIR_STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
    environment: Env<heed3::WithoutTls>,
    database: JournalDatabase,
    map_policy: LmdbMapPolicyV1,
    transaction_gate: Mutex<()>,
    active_pinned_readers: Arc<AtomicUsize>,
}

#[derive(Debug)]
pub struct LmdbQualificationProfile {
    descriptor: QualificationProfileDescriptorV1,
    journal: LmdbQualificationJournal,
    content: IndependentContentStoreV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LmdbExactReceiptV1 {
    pub profile_id: String,
    pub map_policy: LmdbMapPolicyV1,
    pub head_marker: u64,
    pub journal_records: u64,
    pub journal_logical_bytes: u64,
    pub journal_receipt_sha256: String,
    pub content_records: u64,
    pub content_logical_bytes: u64,
    pub content_receipt_sha256: String,
}

pub struct LmdbPinnedReaderV1 {
    transaction: Option<heed3::RoTxn<'static, heed3::WithoutTls>>,
    database: JournalDatabase,
    map_policy: LmdbMapPolicyV1,
    content: IndependentContentStoreV1,
    active_pinned_readers: Arc<AtomicUsize>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LmdbCarrierClassV1 {
    Database,
    Lock,
    ResizeLock,
    IndependentContent,
    Copy,
    Temporary,
    Obsolete,
    Pinned,
    Repair,
    Sidecar,
}

impl LmdbCarrierClassV1 {
    pub const ALL: [Self; 10] = [
        Self::Database,
        Self::Lock,
        Self::ResizeLock,
        Self::IndependentContent,
        Self::Copy,
        Self::Temporary,
        Self::Obsolete,
        Self::Pinned,
        Self::Repair,
        Self::Sidecar,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::Lock => "lock",
            Self::ResizeLock => "resize_lock",
            Self::IndependentContent => "independent_content",
            Self::Copy => "copy",
            Self::Temporary => "temporary",
            Self::Obsolete => "obsolete",
            Self::Pinned => "pinned",
            Self::Repair => "repair",
            Self::Sidecar => "sidecar",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationLmdbCarrierClassInventoryV1 {
    pub class: LmdbCarrierClassV1,
    pub carrier_count: u64,
    pub carrier_set_sha256: String,
    pub encoded_bytes: u64,
    pub allocated_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationLmdbSanitizedInventoryV1 {
    pub carrier_classes: Vec<LmdbCarrierClassV1>,
    pub class_inventories: Vec<QualificationLmdbCarrierClassInventoryV1>,
    pub inventory: QualificationPerformanceInventoryV2,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationLmdbReaderLifecycleV1 {
    pub pinned_receipt: LmdbExactReceiptV1,
    pub latest_receipt: LmdbExactReceiptV1,
    pub process_overlap: QualificationProcessOverlapEvidenceV1,
    pub stale_readers_cleared: u64,
    pub live_reader_preserved: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationLmdbRetentionLifecycleV1 {
    pub steady_allocated_bytes: u64,
    pub retained_allocated_bytes: u64,
    pub reused_allocated_bytes: u64,
    pub retention_bound_bytes: u64,
    pub post_release_reuse_bound_bytes: u64,
    pub within_predeclared_bounds: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationLmdbCopyLifecycleV1 {
    pub copied_receipt: LmdbExactReceiptV1,
    pub source_before_receipt: LmdbExactReceiptV1,
    pub source_after_receipt: LmdbExactReceiptV1,
    pub exact_coherent_prefix: bool,
    pub process_overlap: QualificationProcessOverlapEvidenceV1,
    pub completion_marker_last: bool,
    pub interrupted_backup_rejected: bool,
    pub interrupted_retry_rejected: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationLmdbRestoreRepairLifecycleV1 {
    pub restored_receipt: LmdbExactReceiptV1,
    pub repaired_receipt: LmdbExactReceiptV1,
    pub backup_preserved: bool,
    pub source_preserved: bool,
    pub restore_inventory_identity_exact: bool,
    pub repair_inventory_identity_exact: bool,
    pub corrupt_truth_rejected: bool,
    pub incomplete_destination_rejected: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationLmdbInventorySnapshotV1 {
    pub state: QualificationPerformanceInventoryStateV1,
    pub inventory: QualificationPerformanceInventoryV2,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationLmdbWindowsLifecycleV1 {
    pub required: bool,
    pub replacement_blocked_while_open: bool,
    pub replacement_succeeded_after_close: bool,
    pub reopened_exact: bool,
    pub interrupted_copy_cleaned: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationLmdbLifecycleSmokeV1 {
    pub schema: &'static str,
    pub mode: &'static str,
    pub profile_id: String,
    pub map_policy: LmdbMapPolicyV1,
    pub workload: QualificationGeneratedWorkloadV1,
    pub workload_manifest_sha256: String,
    pub reader: QualificationLmdbReaderLifecycleV1,
    pub retention: QualificationLmdbRetentionLifecycleV1,
    pub copy: QualificationLmdbCopyLifecycleV1,
    pub restore_repair: QualificationLmdbRestoreRepairLifecycleV1,
    pub inventory: QualificationLmdbSanitizedInventoryV1,
    pub inventory_snapshots: Vec<QualificationLmdbInventorySnapshotV1>,
    pub native_allocation_excludes_virtual_map: bool,
    pub windows: QualificationLmdbWindowsLifecycleV1,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LmdbLifecycleChildRequestV1 {
    source_root: PathBuf,
    destination: Option<PathBuf>,
    barrier_root: Option<PathBuf>,
    participant: String,
    result_path: PathBuf,
    operation: LmdbLifecycleChildOperationV1,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum LmdbLifecycleChildOperationV1 {
    PinnedReader,
    HoldPinnedReader,
    CreateCohort { records: Vec<LmdbLifecycleRecordV1> },
    OnlineCopy,
    InterruptedCopy,
    Restore,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LmdbLifecycleRecordV1 {
    logical_key: String,
    decoded_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LmdbBackupReceiptV1 {
    schema: String,
    exact: LmdbExactReceiptV1,
}

#[derive(Clone, Debug)]
struct LmdbCarrierV1 {
    class: LmdbCarrierClassV1,
    relative_path: String,
    encoded_sha256: String,
    encoded_bytes: u64,
    allocated_bytes: u64,
}

struct LmdbLifecycleInventoryRoots<'a> {
    source: &'a Path,
    backup: &'a Path,
    interrupted: &'a Path,
    restored: &'a Path,
    repair_backup: &'a Path,
    repair_restored: &'a Path,
    retention: &'a Path,
    corrupt: &'a Path,
}

struct RepairStagingDirectory {
    path: PathBuf,
}

impl RepairStagingDirectory {
    fn create(parent: &Path) -> Result<Self, String> {
        for _ in 0..64 {
            let sequence = REPAIR_STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                ".pointbreak-lmdb-repair-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "plain LMDB repair staging creation failed: {error}"
                    ));
                }
            }
        }
        Err("plain LMDB repair could not allocate a fresh staging directory".to_owned())
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for RepairStagingDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
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
        let mut options = EnvOpenOptions::new().read_txn_without_tls();
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
            active_pinned_readers: Arc::new(AtomicUsize::new(0)),
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

    pub fn pin_reader(&self) -> Result<LmdbPinnedReaderV1, String> {
        let _gate = self.journal.gate()?;
        let transaction = self
            .journal
            .environment
            .clone()
            .static_read_txn()
            .map_err(|error| format!("plain LMDB pinned read transaction failed: {error}"))?;
        self.journal
            .active_pinned_readers
            .fetch_add(1, Ordering::SeqCst);
        Ok(LmdbPinnedReaderV1 {
            transaction: Some(transaction),
            database: self.journal.database,
            map_policy: self.journal.map_policy,
            content: self.content.clone(),
            active_pinned_readers: Arc::clone(&self.journal.active_pinned_readers),
        })
    }

    pub fn clear_stale_readers(&self) -> Result<usize, String> {
        self.journal
            .environment
            .clear_stale_readers()
            .map_err(|error| format!("plain LMDB stale reader cleanup failed: {error}"))
    }

    pub fn exact_receipt(&self) -> Result<LmdbExactReceiptV1, String> {
        let _gate = self.journal.gate()?;
        self.journal.read_transaction(|transaction| {
            exact_receipt_from_transaction(
                self.journal.database,
                transaction,
                self.journal.map_policy,
                &self.content,
            )
        })
    }

    pub fn sanitized_inventory(&self) -> Result<QualificationLmdbSanitizedInventoryV1, String> {
        let carriers = collect_active_lmdb_carriers(&self.journal.root)?;
        sanitized_inventory_from_carriers(&carriers, self.inventory()?)
    }

    pub fn repair_to(&self, destination: &Path) -> Result<(), String> {
        if destination
            .try_exists()
            .map_err(|error| format!("plain LMDB repair destination check failed: {error}"))?
        {
            return Err("plain LMDB repair destination already exists".to_owned());
        }
        let source_receipt = self.exact_receipt()?;
        let entries = self.journal.list()?;
        let parent = destination.parent().ok_or_else(|| {
            "plain LMDB repair destination must have a parent directory".to_owned()
        })?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("plain LMDB repair parent creation failed: {error}"))?;
        let staging = RepairStagingDirectory::create(parent)?;
        let repaired = LmdbQualificationProfile::open(staging.path())?;
        for entry in entries {
            if repaired
                .journal()
                .create_once(&entry.logical_key, &entry.decoded_bytes)?
                != QualificationCreateOutcome::Created
            {
                return Err("plain LMDB repair replay encountered existing truth".to_owned());
            }
        }
        copy_directory_contents(self.content.root(), repaired.content.root())?;
        if repaired.exact_receipt()? != source_receipt {
            return Err("plain LMDB repaired truth receipt does not match the source".to_owned());
        }
        repaired.backup_to(destination)?;
        verify_lmdb_backup_receipt(destination, &self.descriptor, Some(&source_receipt))?;
        Ok(())
    }

    fn backup_to_with_hook(
        &self,
        destination: &Path,
        after_database_copy: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let mut after_database_copy = Some(after_database_copy);
        publish_completed_backup(destination, &self.descriptor, |backup_root| {
            let journal_root = backup_root.join(JOURNAL_DIRECTORY_V1);
            fs::create_dir_all(&journal_root)
                .map_err(|error| format!("plain LMDB backup journal creation failed: {error}"))?;
            let database_path = backup_root.join(LMDB_BACKUP_DATABASE_FILE_V1);
            let database_file = self
                .journal
                .environment
                .copy_to_path(&database_path, CompactionOption::Disabled)
                .map_err(|error| format!("plain LMDB online copy failed: {error}"))?;
            database_file
                .sync_all()
                .map_err(|error| format!("plain LMDB online copy sync failed: {error}"))?;
            after_database_copy
                .take()
                .expect("database-copy hook is called once")()?;
            copy_directory_contents(self.content.root(), &backup_root.join(CONTENT_DIRECTORY_V1))?;
            let exact = exact_receipt_from_candidate(backup_root, self.journal.map_policy)?;
            write_canonical_new(
                &backup_root.join(LMDB_BACKUP_RECEIPT_FILE_V1),
                &LmdbBackupReceiptV1 {
                    schema: LMDB_BACKUP_RECEIPT_SCHEMA_V1.to_owned(),
                    exact,
                },
            )
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    fn backup_to_after_copy_barrier(
        &self,
        destination: &Path,
        barrier_root: &Path,
        participant: &str,
    ) -> Result<(), String> {
        self.backup_to_with_hook(destination, || {
            let participant =
                super::QualificationProcessBarrierParticipantV1::join(barrier_root, participant)
                    .map_err(|error| error.to_string())?;
            participant
                .wait_for_release(std::time::Duration::from_secs(20))
                .map_err(|error| error.to_string())?;
            participant.complete().map_err(|error| error.to_string())
        })
    }
}

impl LmdbPinnedReaderV1 {
    fn transaction(&self) -> Result<&heed3::RoTxn<'static, heed3::WithoutTls>, String> {
        self.transaction
            .as_ref()
            .ok_or_else(|| "plain LMDB pinned reader is closed".to_owned())
    }

    pub fn head_marker(&self) -> Result<u64, String> {
        head_from_transaction(self.database, self.transaction()?)
    }

    pub fn exact_receipt(&self) -> Result<LmdbExactReceiptV1, String> {
        exact_receipt_from_transaction(
            self.database,
            self.transaction()?,
            self.map_policy,
            &self.content,
        )
    }
}

impl Drop for LmdbPinnedReaderV1 {
    fn drop(&mut self) {
        self.transaction.take();
        self.active_pinned_readers.fetch_sub(1, Ordering::SeqCst);
    }
}

fn initialize_database(
    environment: &Env<heed3::WithoutTls>,
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
        self.ensure_no_pinned_readers("refresh")?;
        with_resize_lock(&self.root, || refresh_environment_map(&self.environment))?;
        self.validate_current_map_size()
    }

    fn grow_map(&self) -> Result<(), String> {
        self.ensure_no_pinned_readers("resize")?;
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

    fn ensure_no_pinned_readers(&self, operation: &str) -> Result<(), String> {
        if self.active_pinned_readers.load(Ordering::SeqCst) != 0 {
            return Err(format!(
                "plain LMDB map {operation} is blocked by a live pinned reader"
            ));
        }
        Ok(())
    }

    fn read_transaction<T>(
        &self,
        mut operation: impl FnMut(&heed3::RoTxn<'_, heed3::WithoutTls>) -> Result<T, String>,
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
        transaction: &heed3::RoTxn<'_, heed3::WithoutTls>,
    ) -> Result<Vec<QualificationEntry>, String> {
        list_from_transaction(self.database, transaction)
    }

    fn head_in_transaction(
        &self,
        transaction: &heed3::RoTxn<'_, heed3::WithoutTls>,
    ) -> Result<u64, String> {
        head_from_transaction(self.database, transaction)
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

fn list_from_transaction(
    database: JournalDatabase,
    transaction: &heed3::RoTxn<'_, heed3::WithoutTls>,
) -> Result<Vec<QualificationEntry>, String> {
    let mut entries = Vec::new();
    let iterator = database
        .iter(transaction)
        .map_err(|error| format!("plain LMDB replay cursor failed: {error}"))?;
    for result in iterator {
        let (key, value) = result.map_err(|error| format!("plain LMDB replay failed: {error}"))?;
        if key == METADATA_KEY_V1 || key == HEAD_KEY_V1 {
            continue;
        }
        let logical_key = decode_entry_key(key)?;
        entries.push(decode_entry(&logical_key, value)?);
    }
    Ok(entries)
}

fn head_from_transaction(
    database: JournalDatabase,
    transaction: &heed3::RoTxn<'_, heed3::WithoutTls>,
) -> Result<u64, String> {
    let bytes = database
        .get(transaction, HEAD_KEY_V1)
        .map_err(|error| format!("plain LMDB head read failed: {error}"))?
        .ok_or_else(|| "plain LMDB head marker is missing".to_owned())?;
    decode_head(bytes)
}

fn exact_receipt_from_transaction(
    database: JournalDatabase,
    transaction: &heed3::RoTxn<'_, heed3::WithoutTls>,
    map_policy: LmdbMapPolicyV1,
    content: &IndependentContentStoreV1,
) -> Result<LmdbExactReceiptV1, String> {
    let metadata = database
        .get(transaction, METADATA_KEY_V1)
        .map_err(|error| format!("plain LMDB metadata read failed: {error}"))?
        .ok_or_else(|| "plain LMDB profile metadata is missing".to_owned())?;
    LmdbProfileMetadataV1::decode(metadata)?.validate(map_policy)?;
    let entries = list_from_transaction(database, transaction)?;
    let head_marker = head_from_transaction(database, transaction)?;
    if entries.len() as u64 != head_marker {
        return Err(format!(
            "plain LMDB head marker {head_marker} does not match {} entries",
            entries.len()
        ));
    }
    let content_entries = content.list().map_err(|error| error.to_string())?;
    Ok(LmdbExactReceiptV1 {
        profile_id: QUALIFICATION_LMDB_PLAIN_PROFILE_ID_V1.to_owned(),
        map_policy,
        head_marker,
        journal_records: entries.len() as u64,
        journal_logical_bytes: logical_bytes(&entries)?,
        journal_receipt_sha256: entry_set_sha256(&entries)?,
        content_records: content_entries.len() as u64,
        content_logical_bytes: logical_bytes(&content_entries)?,
        content_receipt_sha256: entry_set_sha256(&content_entries)?,
    })
}

fn logical_bytes(entries: &[QualificationEntry]) -> Result<u64, String> {
    entries.iter().try_fold(0_u64, |total, entry| {
        total
            .checked_add(entry.decoded_bytes.len() as u64)
            .ok_or_else(|| "plain LMDB receipt byte count overflow".to_owned())
    })
}

fn entry_set_sha256(entries: &[QualificationEntry]) -> Result<String, String> {
    let values = entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "logicalKey": entry.logical_key,
                "decodedSha256": entry.decoded_sha256,
                "decodedBytes": entry.decoded_bytes.len(),
            })
        })
        .collect::<Vec<_>>();
    let bytes = canonical_json_bytes(&serde_json::Value::Array(values))
        .map_err(|error| format!("plain LMDB receipt canonicalization failed: {error}"))?;
    Ok(sha256_bytes_hex(&bytes))
}

fn exact_receipt_from_candidate(
    root: &Path,
    map_policy: LmdbMapPolicyV1,
) -> Result<LmdbExactReceiptV1, String> {
    let mut options = EnvOpenOptions::new().read_txn_without_tls();
    options.max_dbs(1);
    // SAFETY: completed or in-progress candidate carriers are immutable while
    // this read-only, lock-free inspection is active.
    unsafe { options.flags(EnvFlags::READ_ONLY | EnvFlags::NO_LOCK) };
    // SAFETY: the candidate journal directory remains stable for this bounded
    // read and is not modified through this environment handle.
    let environment = unsafe { options.open(root.join(JOURNAL_DIRECTORY_V1)) }
        .map_err(|error| format!("plain LMDB backup candidate open failed: {error}"))?;
    let transaction = environment
        .read_txn()
        .map_err(|error| format!("plain LMDB backup candidate read failed: {error}"))?;
    let database: JournalDatabase = environment
        .open_database(&transaction, Some(DATABASE_NAME_V1))
        .map_err(|error| format!("plain LMDB backup database open failed: {error}"))?
        .ok_or_else(|| "plain LMDB backup omitted the journal database".to_owned())?;
    let content = IndependentContentStoreV1::open(&root.join(CONTENT_DIRECTORY_V1))
        .map_err(|error| error.to_string())?;
    exact_receipt_from_transaction(database, &transaction, map_policy, &content)
}

fn verify_lmdb_backup_receipt(
    backup_root: &Path,
    descriptor: &QualificationProfileDescriptorV1,
    expected: Option<&LmdbExactReceiptV1>,
) -> Result<LmdbExactReceiptV1, String> {
    verify_completed_backup(backup_root, descriptor).map_err(|error| error.to_string())?;
    let receipt_path = backup_root.join(LMDB_BACKUP_RECEIPT_FILE_V1);
    let receipt: LmdbBackupReceiptV1 = serde_json::from_slice(
        &fs::read(&receipt_path)
            .map_err(|error| format!("plain LMDB backup receipt read failed: {error}"))?,
    )
    .map_err(|error| format!("plain LMDB backup receipt is invalid: {error}"))?;
    if receipt.schema != LMDB_BACKUP_RECEIPT_SCHEMA_V1 {
        return Err(format!(
            "unsupported plain LMDB backup receipt schema {}",
            receipt.schema
        ));
    }
    receipt.exact.map_policy.validate()?;
    let actual = exact_receipt_from_candidate(backup_root, receipt.exact.map_policy)?;
    if actual != receipt.exact {
        return Err("plain LMDB backup receipt does not match its candidate carriers".to_owned());
    }
    if expected.is_some_and(|expected| expected != &actual) {
        return Err("plain LMDB backup receipt does not match the expected truth".to_owned());
    }
    Ok(actual)
}

pub fn restore_completed_lmdb_backup_v1(
    backup_root: &Path,
    destination: &Path,
) -> Result<LmdbExactReceiptV1, String> {
    let descriptor = QualificationProfileDescriptorV1 {
        physical_profile_id: QUALIFICATION_LMDB_PLAIN_PROFILE_ID_V1.to_owned(),
        logical_capabilities: LogicalCapabilityEpochV1::foundation(),
    };
    let expected = verify_lmdb_backup_receipt(backup_root, &descriptor, None)?;
    if destination
        .try_exists()
        .map_err(|error| format!("plain LMDB restore destination check failed: {error}"))?
    {
        return Err("plain LMDB restore destination already exists".to_owned());
    }
    fs::create_dir_all(destination.join(JOURNAL_DIRECTORY_V1))
        .map_err(|error| format!("plain LMDB restore journal creation failed: {error}"))?;
    copy_file_synced(
        &backup_root.join(LMDB_BACKUP_DATABASE_FILE_V1),
        &destination.join(LMDB_BACKUP_DATABASE_FILE_V1),
    )?;
    copy_directory_contents(
        &backup_root.join(CONTENT_DIRECTORY_V1),
        &destination.join(CONTENT_DIRECTORY_V1),
    )?;
    copy_file_synced(
        &backup_root.join(LMDB_BACKUP_RECEIPT_FILE_V1),
        &destination.join(LMDB_BACKUP_RECEIPT_FILE_V1),
    )?;
    let actual = exact_receipt_from_candidate(destination, expected.map_policy)?;
    if actual != expected {
        return Err("plain LMDB restored truth does not match the completed backup".to_owned());
    }
    Ok(actual)
}

fn write_canonical_new(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let value = serde_json::to_value(value)
        .map_err(|error| format!("plain LMDB sidecar serialization failed: {error}"))?;
    let bytes = canonical_json_bytes(&value)
        .map_err(|error| format!("plain LMDB sidecar canonicalization failed: {error}"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("plain LMDB sidecar creation failed: {error}"))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("plain LMDB sidecar write failed: {error}"))
}

fn copy_directory_contents(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("plain LMDB copy directory creation failed: {error}"))?;
    let mut entries = fs::read_dir(source)
        .map_err(|error| format!("plain LMDB copy directory read failed: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("plain LMDB copy directory entry failed: {error}"))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("plain LMDB copy carrier inspection failed: {error}"))?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory_contents(&entry.path(), &target)?;
        } else if file_type.is_file() {
            copy_file_synced(&entry.path(), &target)?;
        } else {
            return Err("plain LMDB copy rejected a non-file carrier".to_owned());
        }
    }
    Ok(())
}

fn copy_file_synced(source: &Path, destination: &Path) -> Result<(), String> {
    if destination
        .try_exists()
        .map_err(|error| format!("plain LMDB copy destination check failed: {error}"))?
    {
        return Err(format!(
            "plain LMDB copy destination already exists: {}",
            destination.display()
        ));
    }
    fs::copy(source, destination)
        .map_err(|error| format!("plain LMDB carrier copy failed: {error}"))?;
    OpenOptions::new()
        .write(true)
        .open(destination)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("plain LMDB copied carrier sync failed: {error}"))
}

fn collect_active_lmdb_carriers(root: &Path) -> Result<Vec<LmdbCarrierV1>, String> {
    let mut carriers = Vec::new();
    collect_carriers_recursive(root, root, &mut carriers, &classify_active_carrier)?;
    carriers.sort_by(|left, right| {
        left.relative_path
            .as_bytes()
            .cmp(right.relative_path.as_bytes())
    });
    Ok(carriers)
}

fn collect_carriers_recursive(
    root: &Path,
    directory: &Path,
    carriers: &mut Vec<LmdbCarrierV1>,
    classify: &impl Fn(&str) -> Result<LmdbCarrierClassV1, String>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| format!("plain LMDB inventory directory read failed: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("plain LMDB inventory directory entry failed: {error}"))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("plain LMDB inventory carrier inspection failed: {error}"))?;
        if file_type.is_dir() {
            collect_carriers_recursive(root, &path, carriers, classify)?;
            continue;
        }
        if !file_type.is_file() {
            return Err(format!(
                "plain LMDB inventory rejected non-file carrier {}",
                path.display()
            ));
        }
        let relative_path = path
            .strip_prefix(root)
            .map_err(|_| "plain LMDB inventory carrier escaped its root".to_owned())?
            .to_string_lossy()
            .replace('\\', "/");
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("plain LMDB inventory metadata read failed: {error}"))?;
        carriers.push(LmdbCarrierV1 {
            class: classify(&relative_path)?,
            relative_path,
            encoded_sha256: sha256_bytes_hex(
                &fs::read(&path).map_err(|error| {
                    format!("plain LMDB inventory carrier read failed: {error}")
                })?,
            ),
            encoded_bytes: metadata.len(),
            allocated_bytes: super::fault::native_file_allocation(&path, &metadata)?,
        });
    }
    Ok(())
}

fn classify_active_carrier(relative_path: &str) -> Result<LmdbCarrierClassV1, String> {
    match relative_path {
        "journal/data.mdb" => Ok(LmdbCarrierClassV1::Database),
        "journal/lock.mdb" => Ok(LmdbCarrierClassV1::Lock),
        RESIZE_LOCK_FILE_V1 => Ok(LmdbCarrierClassV1::ResizeLock),
        LMDB_BACKUP_RECEIPT_FILE_V1 => Ok(LmdbCarrierClassV1::Sidecar),
        path if path.starts_with("content/") => Ok(LmdbCarrierClassV1::IndependentContent),
        other => Err(format!(
            "plain LMDB inventory found an unclassified carrier {other}"
        )),
    }
}

fn inventory_from_carriers(
    carriers: &[LmdbCarrierV1],
    logical_bytes: u64,
) -> Result<QualificationInventoryV1, String> {
    if carriers.is_empty() {
        return Err("plain LMDB inventory is empty".to_owned());
    }
    let mut encoded_bytes = 0_u64;
    let mut allocated_bytes = 0_u64;
    let mut names = Vec::with_capacity(carriers.len());
    for carrier in carriers {
        encoded_bytes = encoded_bytes
            .checked_add(carrier.encoded_bytes)
            .ok_or_else(|| "plain LMDB inventory encoded byte count overflow".to_owned())?;
        allocated_bytes = allocated_bytes
            .checked_add(carrier.allocated_bytes)
            .ok_or_else(|| "plain LMDB inventory allocation byte count overflow".to_owned())?;
        names.push(format!(
            "{}:{}",
            carrier.class.as_str(),
            carrier.relative_path
        ));
    }
    names.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    Ok(QualificationInventoryV1 {
        carriers: names,
        logical_bytes,
        encoded_bytes,
        allocated_bytes,
        high_water_bytes: allocated_bytes,
    })
}

fn sanitized_inventory_from_carriers(
    carriers: &[LmdbCarrierV1],
    inventory: QualificationInventoryV1,
) -> Result<QualificationLmdbSanitizedInventoryV1, String> {
    let mut by_class = BTreeMap::<LmdbCarrierClassV1, Vec<&LmdbCarrierV1>>::new();
    for carrier in carriers {
        by_class.entry(carrier.class).or_default().push(carrier);
    }
    let carrier_classes = LmdbCarrierClassV1::ALL.to_vec();
    let class_inventories = LmdbCarrierClassV1::ALL
        .into_iter()
        .map(|class| {
            let carriers = by_class.remove(&class).unwrap_or_default();
            let encoded_bytes = carriers.iter().try_fold(0_u64, |total, carrier| {
                total
                    .checked_add(carrier.encoded_bytes)
                    .ok_or_else(|| "plain LMDB class inventory byte count overflow".to_owned())
            })?;
            let allocated_bytes = carriers.iter().try_fold(0_u64, |total, carrier| {
                total
                    .checked_add(carrier.allocated_bytes)
                    .ok_or_else(|| "plain LMDB class inventory allocation overflow".to_owned())
            })?;
            let identities = carriers
                .iter()
                .map(|carrier| {
                    serde_json::json!({
                        "relativePath": carrier.relative_path,
                        "encodedSha256": carrier.encoded_sha256,
                        "encodedBytes": carrier.encoded_bytes,
                    })
                })
                .collect::<Vec<_>>();
            let identity = canonical_json_bytes(
                &serde_json::to_value(identities).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            Ok(QualificationLmdbCarrierClassInventoryV1 {
                class,
                carrier_count: carriers.len() as u64,
                carrier_set_sha256: sha256_bytes_hex(&identity),
                encoded_bytes,
                allocated_bytes,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(QualificationLmdbSanitizedInventoryV1 {
        carrier_classes,
        class_inventories,
        inventory: QualificationPerformanceInventoryV2::from_inventory(&inventory)?,
    })
}

pub fn run_qualification_lmdb_lifecycle_child_v1(request_path: &Path) -> Result<(), String> {
    let request: LmdbLifecycleChildRequestV1 = serde_json::from_slice(
        &fs::read(request_path)
            .map_err(|error| format!("plain LMDB lifecycle child request read failed: {error}"))?,
    )
    .map_err(|error| format!("plain LMDB lifecycle child request is invalid: {error}"))?;
    match request.operation.clone() {
        LmdbLifecycleChildOperationV1::Restore => {
            let destination = request
                .destination
                .as_deref()
                .ok_or_else(|| "plain LMDB restore child omitted its destination".to_owned())?;
            let receipt = restore_completed_lmdb_backup_v1(&request.source_root, destination)?;
            let restored = LmdbQualificationProfile::open(destination)?;
            if restored.exact_receipt()? != receipt {
                return Err("plain LMDB fresh-process restore receipt drifted on open".to_owned());
            }
            write_canonical_new(&request.result_path, &receipt)
        }
        LmdbLifecycleChildOperationV1::PinnedReader
        | LmdbLifecycleChildOperationV1::HoldPinnedReader => {
            let profile = LmdbQualificationProfile::open(&request.source_root)?;
            let pinned = profile.pin_reader()?;
            let participant = join_lifecycle_participant(&request)?;
            participant.wait_for_release(Duration::from_secs(300))?;
            let receipt = pinned.exact_receipt()?;
            write_canonical_new(&request.result_path, &receipt)?;
            participant.complete()
        }
        LmdbLifecycleChildOperationV1::InterruptedCopy => {
            let profile = LmdbQualificationProfile::open(&request.source_root)?;
            profile.backup_to_after_copy_barrier(
                request
                    .destination
                    .as_deref()
                    .ok_or_else(|| "plain LMDB copy child omitted its destination".to_owned())?,
                request
                    .barrier_root
                    .as_deref()
                    .ok_or_else(|| "plain LMDB copy child omitted its barrier".to_owned())?,
                &request.participant,
            )
        }
        operation => {
            let profile = LmdbQualificationProfile::open(&request.source_root)?;
            let participant = join_lifecycle_participant(&request)?;
            participant.wait_for_release(Duration::from_secs(300))?;
            let receipt = match operation {
                LmdbLifecycleChildOperationV1::CreateCohort { records } => {
                    for record in records {
                        if profile
                            .journal()
                            .create_once(&record.logical_key, &record.decoded_bytes)?
                            != QualificationCreateOutcome::Created
                        {
                            return Err(format!(
                                "plain LMDB lifecycle writer found existing key {}",
                                record.logical_key
                            ));
                        }
                    }
                    profile.exact_receipt()?
                }
                LmdbLifecycleChildOperationV1::OnlineCopy => {
                    let destination = request.destination.as_deref().ok_or_else(|| {
                        "plain LMDB copy child omitted its destination".to_owned()
                    })?;
                    profile.backup_to(destination)?;
                    verify_lmdb_backup_receipt(destination, &profile.descriptor, None)?
                }
                _ => unreachable!("early lifecycle child operations returned above"),
            };
            write_canonical_new(&request.result_path, &receipt)?;
            participant.complete()
        }
    }
}

fn join_lifecycle_participant(
    request: &LmdbLifecycleChildRequestV1,
) -> Result<super::QualificationProcessBarrierParticipantV1, String> {
    super::QualificationProcessBarrierParticipantV1::join(
        request
            .barrier_root
            .as_deref()
            .ok_or_else(|| "plain LMDB lifecycle child omitted its barrier".to_owned())?,
        &request.participant,
    )
}

fn spawn_lmdb_lifecycle_child(
    executable: &Path,
    requests_root: &Path,
    label: &str,
    request: &LmdbLifecycleChildRequestV1,
) -> Result<Child, String> {
    fs::create_dir_all(requests_root)
        .map_err(|error| format!("plain LMDB lifecycle request root creation failed: {error}"))?;
    let request_path = requests_root.join(format!("{label}.json"));
    write_canonical_new(&request_path, request)?;
    Command::new(executable)
        .arg("--lmdb-lifecycle-child")
        .arg(request_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to spawn plain LMDB lifecycle child {label}: {error}"))
}

fn read_lmdb_lifecycle_receipt(path: &Path) -> Result<LmdbExactReceiptV1, String> {
    serde_json::from_slice(
        &fs::read(path)
            .map_err(|error| format!("plain LMDB lifecycle child result read failed: {error}"))?,
    )
    .map_err(|error| format!("plain LMDB lifecycle child result is invalid: {error}"))
}

fn terminate_lmdb_lifecycle_child(child: &mut Child) -> Result<(), String> {
    child
        .kill()
        .map_err(|error| format!("plain LMDB lifecycle child termination failed: {error}"))?;
    let status = child
        .wait()
        .map_err(|error| format!("plain LMDB lifecycle child wait failed: {error}"))?;
    if status.success() {
        return Err("plain LMDB lifecycle child exited successfully before termination".to_owned());
    }
    Ok(())
}

pub fn run_qualification_lmdb_lifecycle_smoke_v1(
    executable: &Path,
    root: &Path,
) -> Result<QualificationLmdbLifecycleSmokeV1, String> {
    fs::create_dir_all(root)
        .map_err(|error| format!("plain LMDB lifecycle root creation failed: {error}"))?;
    let requests_root = root.join("orchestration").join("requests");
    let results_root = root.join("orchestration").join("results");
    fs::create_dir_all(&results_root)
        .map_err(|error| format!("plain LMDB lifecycle result root creation failed: {error}"))?;
    let source_root = root.join("source");
    let profile = LmdbQualificationProfile::open(&source_root)?;
    profile.put_content_once(
        "sha256:3000000000000000000000000000000000000000000000000000000000000000",
        QualificationRecordKindV1::ObjectArtifact,
        b"plain-lmdb-lifecycle-content",
    )?;
    let spec = qualification_generator_spec_v1(QualificationGeneratedWorkloadV1::G0);
    let manifest = qualification_generated_manifest_v1(&spec).map_err(|error| error.to_string())?;
    let split = manifest.records.len() / 2;
    for record in &manifest.records[..split] {
        if profile
            .journal()
            .create_once(&record.logical_key, &record.decoded_bytes)?
            != QualificationCreateOutcome::Created
        {
            return Err("plain LMDB lifecycle seed cohort was not created once".to_owned());
        }
    }

    let reader_barrier_root = root.join("orchestration").join("reader-barrier");
    fs::create_dir_all(&reader_barrier_root)
        .map_err(|error| format!("plain LMDB reader barrier root creation failed: {error}"))?;
    let reader_barrier = super::QualificationProcessBarrierV1::create(
        &reader_barrier_root,
        &["pinned-reader", "later-writer"],
    )?;
    let pinned_result = results_root.join("pinned-reader.json");
    let writer_result = results_root.join("later-writer.json");
    let mut pinned_child = spawn_lmdb_lifecycle_child(
        executable,
        &requests_root,
        "pinned-reader",
        &LmdbLifecycleChildRequestV1 {
            source_root: source_root.clone(),
            destination: None,
            barrier_root: Some(reader_barrier_root.clone()),
            participant: "pinned-reader".to_owned(),
            result_path: pinned_result.clone(),
            operation: LmdbLifecycleChildOperationV1::PinnedReader,
        },
    )?;
    let mut writer_child = spawn_lmdb_lifecycle_child(
        executable,
        &requests_root,
        "later-writer",
        &LmdbLifecycleChildRequestV1 {
            source_root: source_root.clone(),
            destination: None,
            barrier_root: Some(reader_barrier_root.clone()),
            participant: "later-writer".to_owned(),
            result_path: writer_result,
            operation: LmdbLifecycleChildOperationV1::CreateCohort {
                records: manifest.records[split..]
                    .iter()
                    .map(|record| LmdbLifecycleRecordV1 {
                        logical_key: record.logical_key.clone(),
                        decoded_bytes: record.decoded_bytes.clone(),
                    })
                    .collect(),
            },
        },
    )?;
    reader_barrier.wait_until_ready(Duration::from_secs(30))?;
    reader_barrier.release()?;
    super::fault::wait_child(&mut pinned_child, Duration::from_secs(60))?;
    super::fault::wait_child(&mut writer_child, Duration::from_secs(60))?;
    let reader_overlap = reader_barrier.evidence()?;
    let pinned_receipt = read_lmdb_lifecycle_receipt(&pinned_result)?;
    let latest_receipt = profile.exact_receipt()?;
    if pinned_receipt.head_marker != split as u64
        || latest_receipt.head_marker != manifest.records.len() as u64
        || pinned_receipt == latest_receipt
    {
        return Err(
            "plain LMDB pinned-reader lifecycle receipts are not stable and current".to_owned(),
        );
    }

    let retention_root = root.join("retention");
    let retention_profile = LmdbQualificationProfile::open(&retention_root)?;
    populate_lifecycle_range(&retention_profile, "seed", 0..32, 512)?;
    let steady_allocated_bytes = retention_profile.inventory()?.allocated_bytes;
    let retained_reader = retention_profile.pin_reader()?;
    populate_lifecycle_range(&retention_profile, "retained", 0..128, 4096)?;
    let retained_allocated_bytes = retention_profile.inventory()?.allocated_bytes;
    drop(retained_reader);
    populate_lifecycle_range(&retention_profile, "reuse", 0..128, 512)?;
    let reused_allocated_bytes = retention_profile.inventory()?.allocated_bytes;
    let within_predeclared_bounds = reused_allocated_bytes
        <= LIFECYCLE_READER_RETENTION_BOUND_BYTES_V1
        && reused_allocated_bytes.saturating_sub(retained_allocated_bytes)
            <= LIFECYCLE_POST_RELEASE_REUSE_BOUND_BYTES_V1;
    if !within_predeclared_bounds {
        return Err(format!(
            "plain LMDB reader retention exceeded its predeclared native-allocation bounds: steady={steady_allocated_bytes}, retained={retained_allocated_bytes}, reused={reused_allocated_bytes}"
        ));
    }

    let live_reader = profile.pin_reader()?;
    let live_receipt = live_reader.exact_receipt()?;
    let stale_barrier_root = root.join("orchestration").join("stale-reader-barrier");
    fs::create_dir_all(&stale_barrier_root)
        .map_err(|error| format!("plain LMDB stale-reader barrier creation failed: {error}"))?;
    let stale_barrier =
        super::QualificationProcessBarrierV1::create(&stale_barrier_root, &["stale-reader"])?;
    let mut stale_child = spawn_lmdb_lifecycle_child(
        executable,
        &requests_root,
        "stale-reader",
        &LmdbLifecycleChildRequestV1 {
            source_root: source_root.clone(),
            destination: None,
            barrier_root: Some(stale_barrier_root),
            participant: "stale-reader".to_owned(),
            result_path: results_root.join("stale-reader.json"),
            operation: LmdbLifecycleChildOperationV1::HoldPinnedReader,
        },
    )?;
    stale_barrier.wait_until_ready(Duration::from_secs(30))?;
    terminate_lmdb_lifecycle_child(&mut stale_child)?;
    let stale_readers_cleared = profile.clear_stale_readers()? as u64;
    drop(stale_child);
    let live_reader_preserved = live_reader.exact_receipt()? == live_receipt;
    drop(live_reader);
    if stale_readers_cleared == 0 || !live_reader_preserved || profile.clear_stale_readers()? != 0 {
        return Err("plain LMDB stale-reader cleanup did not preserve the live reader".to_owned());
    }

    let copy_before = profile.exact_receipt()?;
    let copy_barrier_root = root.join("orchestration").join("copy-barrier");
    fs::create_dir_all(&copy_barrier_root)
        .map_err(|error| format!("plain LMDB copy barrier root creation failed: {error}"))?;
    let copy_barrier = super::QualificationProcessBarrierV1::create(
        &copy_barrier_root,
        &["online-copy", "copy-writer"],
    )?;
    let backup_root = root.join("backup");
    let copy_result = results_root.join("online-copy.json");
    let mut copy_child = spawn_lmdb_lifecycle_child(
        executable,
        &requests_root,
        "online-copy",
        &LmdbLifecycleChildRequestV1 {
            source_root: source_root.clone(),
            destination: Some(backup_root.clone()),
            barrier_root: Some(copy_barrier_root.clone()),
            participant: "online-copy".to_owned(),
            result_path: copy_result.clone(),
            operation: LmdbLifecycleChildOperationV1::OnlineCopy,
        },
    )?;
    let mut copy_writer_child = spawn_lmdb_lifecycle_child(
        executable,
        &requests_root,
        "copy-writer",
        &LmdbLifecycleChildRequestV1 {
            source_root: source_root.clone(),
            destination: None,
            barrier_root: Some(copy_barrier_root),
            participant: "copy-writer".to_owned(),
            result_path: results_root.join("copy-writer.json"),
            operation: LmdbLifecycleChildOperationV1::CreateCohort {
                records: vec![LmdbLifecycleRecordV1 {
                    logical_key: "journal/lifecycle-copy-writer".to_owned(),
                    decoded_bytes: b"later-writer-cohort".to_vec(),
                }],
            },
        },
    )?;
    copy_barrier.wait_until_ready(Duration::from_secs(30))?;
    copy_barrier.release()?;
    super::fault::wait_child(&mut copy_child, Duration::from_secs(60))?;
    super::fault::wait_child(&mut copy_writer_child, Duration::from_secs(60))?;
    let copy_overlap = copy_barrier.evidence()?;
    let copied_receipt = read_lmdb_lifecycle_receipt(&copy_result)?;
    let copy_after = profile.exact_receipt()?;
    let exact_coherent_prefix = copied_receipt == copy_before || copied_receipt == copy_after;
    if !exact_coherent_prefix {
        return Err("plain LMDB online copy is not an exact coherent cohort prefix".to_owned());
    }
    let completed_manifest = verify_completed_backup(&backup_root, &profile.descriptor)
        .map_err(|error| error.to_string())?;
    let completion_marker_last = backup_root.join(super::BACKUP_COMPLETION_FILE_V1).is_file()
        && backup_root.join(super::BACKUP_MANIFEST_FILE_V1).is_file()
        && !completed_manifest.carriers.iter().any(|carrier| {
            carrier.relative_path == super::BACKUP_COMPLETION_FILE_V1
                || carrier.relative_path == super::BACKUP_MANIFEST_FILE_V1
        });
    if !completion_marker_last {
        return Err("plain LMDB completion marker publication order is invalid".to_owned());
    }

    let interrupted_root = root.join("interrupted");
    let interrupted_barrier_root = root.join("orchestration").join("interrupted-barrier");
    fs::create_dir_all(&interrupted_barrier_root).map_err(|error| {
        format!("plain LMDB interrupted-copy barrier root creation failed: {error}")
    })?;
    let interrupted_barrier =
        super::QualificationProcessBarrierV1::create(&interrupted_barrier_root, &["copy"])?;
    let mut interrupted_child = spawn_lmdb_lifecycle_child(
        executable,
        &requests_root,
        "interrupted-copy",
        &LmdbLifecycleChildRequestV1 {
            source_root: source_root.clone(),
            destination: Some(interrupted_root.clone()),
            barrier_root: Some(interrupted_barrier_root),
            participant: "copy".to_owned(),
            result_path: results_root.join("interrupted-copy.json"),
            operation: LmdbLifecycleChildOperationV1::InterruptedCopy,
        },
    )?;
    interrupted_barrier.wait_until_ready(Duration::from_secs(30))?;
    terminate_lmdb_lifecycle_child(&mut interrupted_child)?;
    let interrupted_backup_rejected =
        verify_completed_backup(&interrupted_root, &profile.descriptor).is_err();
    let interrupted_retry_rejected = profile.backup_to(&interrupted_root).is_err();
    if !interrupted_backup_rejected || !interrupted_retry_rejected {
        return Err("plain LMDB interrupted backup was reinterpreted as complete".to_owned());
    }

    let backup_before_restore = tree_carrier_receipt(&backup_root, false)?;
    let restored_root = root.join("restored");
    let restored_result = results_root.join("restored.json");
    let mut restore_child = spawn_lmdb_lifecycle_child(
        executable,
        &requests_root,
        "fresh-restore",
        &LmdbLifecycleChildRequestV1 {
            source_root: backup_root.clone(),
            destination: Some(restored_root.clone()),
            barrier_root: None,
            participant: "restore".to_owned(),
            result_path: restored_result.clone(),
            operation: LmdbLifecycleChildOperationV1::Restore,
        },
    )?;
    super::fault::wait_child(&mut restore_child, Duration::from_secs(60))?;
    let restored_receipt = read_lmdb_lifecycle_receipt(&restored_result)?;
    let backup_preserved = tree_carrier_receipt(&backup_root, false)? == backup_before_restore;
    let restore_inventory_identity_exact =
        truth_carrier_identity(&backup_root)? == truth_carrier_identity(&restored_root)?;
    if restored_receipt != copied_receipt || !backup_preserved || !restore_inventory_identity_exact
    {
        return Err("plain LMDB fresh-process restore was not exact and read-only".to_owned());
    }

    with_resize_lock(&source_root, || Ok(()))?;
    let source_before_repair = tree_carrier_receipt(&source_root, true)?;
    let repair_backup_root = root.join("repair-backup");
    profile.repair_to(&repair_backup_root)?;
    let source_preserved = tree_carrier_receipt(&source_root, true)? == source_before_repair;
    let repair_root = root.join("repair-restored");
    let repaired_receipt = restore_completed_lmdb_backup_v1(&repair_backup_root, &repair_root)?;
    let repair_inventory_identity_exact =
        truth_carrier_identity(&repair_backup_root)? == truth_carrier_identity(&repair_root)?;
    if repaired_receipt != copy_after || !source_preserved || !repair_inventory_identity_exact {
        return Err("plain LMDB fresh-copy repair did not preserve exact source truth".to_owned());
    }
    let corrupt_root = root.join("corrupt-source");
    fs::create_dir_all(corrupt_root.join(JOURNAL_DIRECTORY_V1))
        .map_err(|error| format!("plain LMDB corrupt fixture creation failed: {error}"))?;
    copy_file_synced(
        &backup_root.join(LMDB_BACKUP_DATABASE_FILE_V1),
        &corrupt_root.join(JOURNAL_DIRECTORY_V1).join("data.mdb"),
    )?;
    OpenOptions::new()
        .write(true)
        .open(corrupt_root.join(JOURNAL_DIRECTORY_V1).join("data.mdb"))
        .and_then(|file| file.set_len(1024))
        .map_err(|error| format!("plain LMDB corrupt fixture truncation failed: {error}"))?;
    let corrupt_truth_rejected = LmdbQualificationProfile::open(&corrupt_root).is_err();
    let incomplete_repair_root = root.join("incomplete-repair");
    fs::create_dir(&incomplete_repair_root)
        .map_err(|error| format!("plain LMDB incomplete repair fixture failed: {error}"))?;
    let incomplete_destination_rejected = profile.repair_to(&incomplete_repair_root).is_err();
    if !corrupt_truth_rejected || !incomplete_destination_rejected {
        return Err(
            "plain LMDB repair admitted corrupt truth or an incomplete destination".to_owned(),
        );
    }

    let source_inventory = profile.inventory()?;
    let steady_inventory = QualificationPerformanceInventoryV2::from_inventory(&source_inventory)?;
    let native_allocation_excludes_virtual_map =
        source_inventory.allocated_bytes < profile.current_map_size_bytes();
    if !native_allocation_excludes_virtual_map {
        return Err(
            "plain LMDB inventory counted virtual map reservation as allocation".to_owned(),
        );
    }
    let inventory = collect_lifecycle_inventory(
        LmdbLifecycleInventoryRoots {
            source: &source_root,
            backup: &backup_root,
            interrupted: &interrupted_root,
            restored: &restored_root,
            repair_backup: &repair_backup_root,
            repair_restored: &repair_root,
            retention: &retention_root,
            corrupt: &corrupt_root,
        },
        source_inventory.logical_bytes,
    )?;
    if inventory.carrier_classes != LmdbCarrierClassV1::ALL {
        return Err("plain LMDB lifecycle inventory omitted an owned carrier class".to_owned());
    }

    drop(retention_profile);
    let interrupted_copy_cleaned = fs::remove_dir_all(&interrupted_root).is_ok()
        && !interrupted_root.try_exists().map_err(|error| {
            format!("plain LMDB interrupted-copy cleanup check failed: {error}")
        })?;
    if !interrupted_copy_cleaned {
        return Err("plain LMDB interrupted-copy carriers could not be cleaned".to_owned());
    }
    drop(profile);
    let reopened_profile = LmdbQualificationProfile::open(&source_root)?;
    let reopened_inventory =
        QualificationPerformanceInventoryV2::from_inventory(&reopened_profile.inventory()?)?;
    drop(reopened_profile);
    let windows = exercise_windows_lmdb_lifecycle(&backup_root, root, interrupted_copy_cleaned)?;
    let inventory_snapshots = vec![
        QualificationLmdbInventorySnapshotV1 {
            state: QualificationPerformanceInventoryStateV1::Steady,
            inventory: steady_inventory,
        },
        QualificationLmdbInventorySnapshotV1 {
            state: QualificationPerformanceInventoryStateV1::Reopened,
            inventory: reopened_inventory,
        },
        QualificationLmdbInventorySnapshotV1 {
            state: QualificationPerformanceInventoryStateV1::HighWater,
            inventory: inventory.inventory.clone(),
        },
    ];

    Ok(QualificationLmdbLifecycleSmokeV1 {
        schema: QUALIFICATION_LMDB_LIFECYCLE_SMOKE_SCHEMA_V1,
        mode: QUALIFICATION_LMDB_LIFECYCLE_REPORT_MODE_V1,
        profile_id: QUALIFICATION_LMDB_PLAIN_PROFILE_ID_V1.to_owned(),
        map_policy: LmdbMapPolicyV1::default(),
        workload: QualificationGeneratedWorkloadV1::G0,
        workload_manifest_sha256: manifest.manifest_sha256,
        reader: QualificationLmdbReaderLifecycleV1 {
            pinned_receipt,
            latest_receipt,
            process_overlap: reader_overlap,
            stale_readers_cleared,
            live_reader_preserved,
        },
        retention: QualificationLmdbRetentionLifecycleV1 {
            steady_allocated_bytes,
            retained_allocated_bytes,
            reused_allocated_bytes,
            retention_bound_bytes: LIFECYCLE_READER_RETENTION_BOUND_BYTES_V1,
            post_release_reuse_bound_bytes: LIFECYCLE_POST_RELEASE_REUSE_BOUND_BYTES_V1,
            within_predeclared_bounds,
        },
        copy: QualificationLmdbCopyLifecycleV1 {
            copied_receipt,
            source_before_receipt: copy_before,
            source_after_receipt: copy_after,
            exact_coherent_prefix,
            process_overlap: copy_overlap,
            completion_marker_last,
            interrupted_backup_rejected,
            interrupted_retry_rejected,
        },
        restore_repair: QualificationLmdbRestoreRepairLifecycleV1 {
            restored_receipt,
            repaired_receipt,
            backup_preserved,
            source_preserved,
            restore_inventory_identity_exact,
            repair_inventory_identity_exact,
            corrupt_truth_rejected,
            incomplete_destination_rejected,
        },
        inventory,
        inventory_snapshots,
        native_allocation_excludes_virtual_map,
        windows,
    })
}

fn populate_lifecycle_range(
    profile: &LmdbQualificationProfile,
    prefix: &str,
    range: std::ops::Range<u64>,
    value_bytes: usize,
) -> Result<(), String> {
    for index in range {
        let outcome = profile.journal().create_once(
            &format!("journal/{prefix}-{index:04}"),
            &vec![(index % 251) as u8; value_bytes],
        )?;
        if outcome != QualificationCreateOutcome::Created {
            return Err(format!(
                "plain LMDB lifecycle record {prefix}-{index:04} was not created once"
            ));
        }
    }
    Ok(())
}

fn tree_carrier_receipt(root: &Path, exclude_lock: bool) -> Result<String, String> {
    fn visit(
        root: &Path,
        directory: &Path,
        exclude_lock: bool,
        carriers: &mut Vec<serde_json::Value>,
    ) -> Result<(), String> {
        let mut entries = fs::read_dir(directory)
            .map_err(|error| format!("plain LMDB receipt directory read failed: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("plain LMDB receipt directory entry failed: {error}"))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let file_type = entry.file_type().map_err(|error| {
                format!("plain LMDB receipt carrier inspection failed: {error}")
            })?;
            if file_type.is_dir() {
                visit(root, &path, exclude_lock, carriers)?;
                continue;
            }
            if !file_type.is_file() {
                return Err("plain LMDB receipt rejected a non-file carrier".to_owned());
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "plain LMDB receipt carrier escaped its root".to_owned())?
                .to_string_lossy()
                .replace('\\', "/");
            if exclude_lock && relative == "journal/lock.mdb" {
                continue;
            }
            carriers.push(serde_json::json!({
                "relativePath": relative,
                "encodedSha256": sha256_bytes_hex(
                    &fs::read(&path)
                        .map_err(|error| format!("plain LMDB receipt carrier read failed: {error}"))?
                ),
            }));
        }
        Ok(())
    }

    let mut carriers = Vec::new();
    visit(root, root, exclude_lock, &mut carriers)?;
    let canonical =
        canonical_json_bytes(&serde_json::to_value(carriers).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    Ok(sha256_bytes_hex(&canonical))
}

fn truth_carrier_identity(root: &Path) -> Result<String, String> {
    fn add_file(
        root: &Path,
        path: &Path,
        carriers: &mut Vec<serde_json::Value>,
    ) -> Result<(), String> {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "plain LMDB truth carrier escaped its root".to_owned())?
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = fs::read(path)
            .map_err(|error| format!("plain LMDB truth carrier read failed: {error}"))?;
        carriers.push(serde_json::json!({
            "relativePath": relative,
            "encodedBytes": bytes.len(),
            "encodedSha256": sha256_bytes_hex(&bytes),
        }));
        Ok(())
    }

    fn visit_content(
        root: &Path,
        directory: &Path,
        carriers: &mut Vec<serde_json::Value>,
    ) -> Result<(), String> {
        let mut entries = fs::read_dir(directory)
            .map_err(|error| format!("plain LMDB truth content read failed: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("plain LMDB truth content entry failed: {error}"))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file_type = entry
                .file_type()
                .map_err(|error| format!("plain LMDB truth carrier inspection failed: {error}"))?;
            if file_type.is_dir() {
                visit_content(root, &entry.path(), carriers)?;
            } else if file_type.is_file() {
                add_file(root, &entry.path(), carriers)?;
            } else {
                return Err("plain LMDB truth identity rejected a non-file carrier".to_owned());
            }
        }
        Ok(())
    }

    let mut carriers = Vec::new();
    add_file(
        root,
        &root.join(LMDB_BACKUP_DATABASE_FILE_V1),
        &mut carriers,
    )?;
    visit_content(root, &root.join(CONTENT_DIRECTORY_V1), &mut carriers)?;
    carriers.sort_by(|left, right| {
        left["relativePath"]
            .as_str()
            .cmp(&right["relativePath"].as_str())
    });
    let canonical = canonical_json_bytes(&serde_json::Value::Array(carriers))
        .map_err(|error| error.to_string())?;
    Ok(sha256_bytes_hex(&canonical))
}

fn collect_lifecycle_inventory(
    roots: LmdbLifecycleInventoryRoots<'_>,
    logical_bytes: u64,
) -> Result<QualificationLmdbSanitizedInventoryV1, String> {
    let mut carriers = collect_active_lmdb_carriers(roots.source)?;
    prefix_carrier_paths(&mut carriers, "source");
    append_role_carriers(&mut carriers, "backup", roots.backup, |path| {
        if matches!(
            path,
            LMDB_BACKUP_RECEIPT_FILE_V1
                | super::BACKUP_MANIFEST_FILE_V1
                | super::BACKUP_COMPLETION_FILE_V1
        ) {
            Ok(LmdbCarrierClassV1::Sidecar)
        } else {
            Ok(LmdbCarrierClassV1::Copy)
        }
    })?;
    append_role_carriers(&mut carriers, "interrupted", roots.interrupted, |_| {
        Ok(LmdbCarrierClassV1::Temporary)
    })?;
    append_role_carriers(&mut carriers, "restored", roots.restored, |_| {
        Ok(LmdbCarrierClassV1::Copy)
    })?;
    append_role_carriers(&mut carriers, "repair-backup", roots.repair_backup, |_| {
        Ok(LmdbCarrierClassV1::Repair)
    })?;
    append_role_carriers(
        &mut carriers,
        "repair-restored",
        roots.repair_restored,
        |_| Ok(LmdbCarrierClassV1::Repair),
    )?;
    append_role_carriers(&mut carriers, "retention", roots.retention, |_| {
        Ok(LmdbCarrierClassV1::Obsolete)
    })?;
    append_role_carriers(&mut carriers, "corrupt", roots.corrupt, |_| {
        Ok(LmdbCarrierClassV1::Obsolete)
    })?;
    carriers.sort_by(|left, right| {
        left.relative_path
            .as_bytes()
            .cmp(right.relative_path.as_bytes())
    });
    let aggregate = inventory_from_carriers(&carriers, logical_bytes)?;
    sanitized_inventory_from_carriers(&carriers, aggregate)
}

fn append_role_carriers(
    carriers: &mut Vec<LmdbCarrierV1>,
    role: &str,
    root: &Path,
    classify: impl Fn(&str) -> Result<LmdbCarrierClassV1, String>,
) -> Result<(), String> {
    let mut role_carriers = Vec::new();
    collect_carriers_recursive(root, root, &mut role_carriers, &classify)?;
    prefix_carrier_paths(&mut role_carriers, role);
    carriers.extend(role_carriers);
    Ok(())
}

fn prefix_carrier_paths(carriers: &mut [LmdbCarrierV1], role: &str) {
    for carrier in carriers {
        carrier.relative_path = format!("{role}/{}", carrier.relative_path);
    }
}

#[cfg(not(windows))]
fn exercise_windows_lmdb_lifecycle(
    _backup_root: &Path,
    _workspace_root: &Path,
    interrupted_copy_cleaned: bool,
) -> Result<QualificationLmdbWindowsLifecycleV1, String> {
    Ok(QualificationLmdbWindowsLifecycleV1 {
        required: false,
        replacement_blocked_while_open: false,
        replacement_succeeded_after_close: false,
        reopened_exact: false,
        interrupted_copy_cleaned,
    })
}

#[cfg(windows)]
fn exercise_windows_lmdb_lifecycle(
    backup_root: &Path,
    workspace_root: &Path,
    interrupted_copy_cleaned: bool,
) -> Result<QualificationLmdbWindowsLifecycleV1, String> {
    let open_root = workspace_root.join("windows-open-handle");
    let expected = restore_completed_lmdb_backup_v1(backup_root, &open_root)?;
    let profile = LmdbQualificationProfile::open(&open_root)?;
    let journal_root = open_root.join(JOURNAL_DIRECTORY_V1);
    let database_path = journal_root.join("data.mdb");
    let replacement_path = journal_root.join("replacement.mdb");
    copy_file_synced(
        &backup_root.join(LMDB_BACKUP_DATABASE_FILE_V1),
        &replacement_path,
    )?;
    let replacement_blocked_while_open = fs::remove_file(&database_path).is_err();
    let closing = heed3::env_closing_event(&journal_root)
        .ok_or_else(|| "plain LMDB Windows closing event is unavailable".to_owned())?;
    drop(profile);
    closing.wait();
    if database_path.exists() {
        fs::remove_file(&database_path).map_err(|error| {
            format!("plain LMDB Windows closed carrier removal failed: {error}")
        })?;
    }
    let replacement_succeeded_after_close = fs::rename(&replacement_path, &database_path).is_ok();
    let reopened_exact = replacement_succeeded_after_close
        && LmdbQualificationProfile::open(&open_root)
            .and_then(|profile| profile.exact_receipt())
            .is_ok_and(|receipt| receipt == expected);
    if !replacement_blocked_while_open
        || !replacement_succeeded_after_close
        || !reopened_exact
        || !interrupted_copy_cleaned
    {
        return Err("plain LMDB Windows handle lifecycle proof failed".to_owned());
    }
    Ok(QualificationLmdbWindowsLifecycleV1 {
        required: true,
        replacement_blocked_while_open,
        replacement_succeeded_after_close,
        reopened_exact,
        interrupted_copy_cleaned,
    })
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

    fn backup_to(&self, destination: &Path) -> Result<(), String> {
        self.backup_to_with_hook(destination, || Ok(()))
    }

    fn verify_restore(&self, restored_root: &Path) -> Result<(), String> {
        verify_lmdb_backup_receipt(restored_root, &self.descriptor, None).map(|_| ())
    }

    fn inventory(&self) -> Result<QualificationInventoryV1, String> {
        let carriers = collect_active_lmdb_carriers(&self.journal.root)?;
        let logical_bytes = self
            .journal
            .list()?
            .into_iter()
            .chain(self.content.list().map_err(|error| error.to_string())?)
            .try_fold(0_u64, |total, entry| {
                total
                    .checked_add(entry.decoded_bytes.len() as u64)
                    .ok_or_else(|| "plain LMDB inventory logical byte count overflow".to_owned())
            })?;
        inventory_from_carriers(&carriers, logical_bytes)
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

fn refresh_environment_map(environment: &Env<heed3::WithoutTls>) -> Result<(), String> {
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
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command};
    use std::time::Duration;

    use super::*;
    use crate::bench_support::foundation::{
        BACKUP_COMPLETION_FILE_V1, BACKUP_MANIFEST_FILE_V1, QualificationCreateOutcome,
        QualificationGeneratedWorkloadV1, QualificationKeyedReadClassV1,
        QualificationProcessBarrierParticipantV1, QualificationProcessBarrierV1,
        qualification_generated_manifest_v1, qualification_generator_spec_v1,
        qualification_operation_schedule_v1,
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

    fn populate_records(
        profile: &LmdbQualificationProfile,
        prefix: &str,
        range: std::ops::Range<u64>,
        value_bytes: usize,
    ) {
        for index in range {
            assert_eq!(
                profile
                    .journal()
                    .create_once(
                        &format!("journal/{prefix}-{index:04}"),
                        &vec![(index % 251) as u8; value_bytes],
                    )
                    .expect("create lifecycle record"),
                QualificationCreateOutcome::Created
            );
        }
    }

    fn tree_receipt(root: &Path) -> String {
        fn visit(root: &Path, directory: &Path, carriers: &mut Vec<(String, String)>) {
            let mut entries = fs::read_dir(directory)
                .expect("read receipt directory")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect receipt directory");
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                if entry.file_type().expect("receipt file type").is_dir() {
                    visit(root, &path, carriers);
                } else {
                    let relative = path
                        .strip_prefix(root)
                        .expect("receipt relative path")
                        .to_string_lossy()
                        .replace('\\', "/");
                    carriers.push((
                        relative,
                        sha256_bytes_hex(&fs::read(path).expect("read receipt carrier")),
                    ));
                }
            }
        }

        let mut carriers = Vec::new();
        visit(root, root, &mut carriers);
        sha256_bytes_hex(&canonical_json_bytes(&serde_json::to_value(carriers).unwrap()).unwrap())
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
        if action == "restore" {
            let destination = PathBuf::from(&key);
            restore_completed_lmdb_backup_v1(&root, &destination).expect("restore LMDB backup");
            let restored = LmdbQualificationProfile::open(&destination)
                .expect("open restored LMDB profile in child");
            fs::write(
                result,
                serde_json::to_vec(&restored.exact_receipt().expect("restored receipt")).unwrap(),
            )
            .expect("write restored receipt");
            return;
        }
        let profile = if action.to_string_lossy().starts_with("refresh_") {
            LmdbQualificationProfile::open_with_policy(&root, LmdbMapPolicyV1::test_resize_policy())
        } else {
            LmdbQualificationProfile::open(&root)
        }
        .expect("open child LMDB profile");
        if action == "pin_wait" {
            let pinned = profile.pin_reader().expect("pin LMDB reader");
            let participant = QualificationProcessBarrierParticipantV1::join(
                std::env::var_os("POINTBREAK_LMDB_CHILD_BARRIER").unwrap(),
                &std::env::var("POINTBREAK_LMDB_CHILD_PARTICIPANT").unwrap(),
            )
            .expect("join pinned-reader barrier");
            participant
                .wait_for_release(Duration::from_secs(20))
                .expect("wait for pinned-reader release");
            fs::write(
                result,
                serde_json::to_vec(&pinned.exact_receipt().expect("pinned receipt")).unwrap(),
            )
            .expect("write pinned receipt");
            participant
                .complete()
                .expect("complete pinned-reader barrier");
            return;
        }
        if action == "interrupt_backup" {
            profile
                .backup_to_after_copy_barrier(
                    &result,
                    Path::new(&std::env::var_os("POINTBREAK_LMDB_CHILD_BARRIER").unwrap()),
                    &std::env::var("POINTBREAK_LMDB_CHILD_PARTICIPANT").unwrap(),
                )
                .expect("interrupted backup should be killed at barrier");
            panic!("interrupted backup unexpectedly passed its copy barrier");
        }
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
        if action == "backup" {
            profile.backup_to(&result).expect("online LMDB backup");
            if let Some(participant) = participant {
                participant.complete().expect("complete LMDB child barrier");
            }
            return;
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
    fn content_stays_independent_across_completed_backup_and_restore() {
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
        let backup = root.path().join("backup");
        profile
            .backup_to(&backup)
            .expect("backup independent content");
        profile
            .verify_restore(&backup)
            .expect("verify completed backup");
        let restored_root = root.path().join("restored");
        restore_completed_lmdb_backup_v1(&backup, &restored_root)
            .expect("restore independent content");
        let restored =
            LmdbQualificationProfile::open(&restored_root).expect("open restored profile");
        assert_eq!(
            restored.read_content(key).unwrap().unwrap().decoded_bytes,
            b"object"
        );
        assert_eq!(restored.journal().head_marker().unwrap(), 0);
    }

    #[test]
    fn pinned_reader_keeps_a_stable_snapshot_while_later_writers_commit() {
        let root = tempfile::tempdir().expect("LMDB pinned-reader root");
        let profile = LmdbQualificationProfile::open(root.path()).expect("open LMDB profile");
        populate_records(&profile, "before-pin", 0..8, 128);
        let expected = profile.exact_receipt().expect("pre-pin receipt");
        let pinned = profile.pin_reader().expect("pin reader snapshot");

        populate_records(&profile, "after-pin", 0..8, 128);

        assert_eq!(pinned.exact_receipt().unwrap(), expected);
        assert_eq!(pinned.head_marker().unwrap(), 8);
        assert_eq!(profile.journal().head_marker().unwrap(), 16);
        assert_ne!(profile.exact_receipt().unwrap(), expected);
    }

    #[test]
    fn reader_retention_has_a_predeclared_bound_and_reuses_pages_after_release() {
        let root = tempfile::tempdir().expect("LMDB retention root");
        let profile = LmdbQualificationProfile::open(root.path()).expect("open LMDB profile");
        populate_records(&profile, "seed", 0..32, 512);
        let steady = profile.inventory().unwrap().allocated_bytes;
        let pinned = profile.pin_reader().expect("pin retention reader");
        populate_records(&profile, "retained", 0..128, 4096);
        let retained = profile.inventory().unwrap().allocated_bytes;
        assert!(retained >= steady);
        drop(pinned);
        populate_records(&profile, "reuse", 0..128, 512);
        let reused = profile.inventory().unwrap().allocated_bytes;

        assert!(
            reused <= LIFECYCLE_READER_RETENTION_BOUND_BYTES_V1,
            "native allocation {reused} exceeded the predeclared {}-byte bound",
            LIFECYCLE_READER_RETENTION_BOUND_BYTES_V1
        );
        assert!(
            reused.saturating_sub(retained) <= LIFECYCLE_POST_RELEASE_REUSE_BOUND_BYTES_V1,
            "ordinary post-release commits grew by {} bytes",
            reused.saturating_sub(retained)
        );
    }

    #[test]
    fn stale_reader_cleanup_clears_dead_slots_without_evicting_a_live_reader() {
        let root = tempfile::tempdir().expect("LMDB stale-reader root");
        let results = tempfile::tempdir().expect("LMDB stale-reader results");
        let profile = LmdbQualificationProfile::open(root.path()).expect("open LMDB profile");
        populate_records(&profile, "stable", 0..4, 64);
        let live = profile.pin_reader().expect("pin live reader");
        let expected = live.exact_receipt().unwrap();

        let barrier_root = results.path().join("barrier");
        fs::create_dir(&barrier_root).expect("create stale-reader barrier root");
        let barrier = QualificationProcessBarrierV1::create(&barrier_root, &["stale-reader"])
            .expect("create stale-reader barrier");
        let mut stale = spawn_child(
            "pin_wait",
            root.path(),
            "unused",
            b"",
            &results.path().join("stale-result"),
            Some((&barrier_root, "stale-reader")),
        );
        barrier
            .wait_until_ready(Duration::from_secs(20))
            .expect("stale reader pinned");
        stale.kill().expect("terminate stale reader process");
        assert!(!stale.wait().expect("wait for stale reader").success());

        assert!(profile.clear_stale_readers().expect("clear stale readers") >= 1);
        drop(stale);
        assert_eq!(live.exact_receipt().unwrap(), expected);
        assert_eq!(profile.clear_stale_readers().unwrap(), 0);
    }

    #[test]
    fn online_copy_overlapping_a_writer_restores_one_exact_coherent_prefix() {
        let root = tempfile::tempdir().expect("LMDB online-copy root");
        let results = tempfile::tempdir().expect("LMDB online-copy results");
        let profile = LmdbQualificationProfile::open(root.path()).expect("open LMDB profile");
        populate_records(&profile, "prefix", 0..32, 1024);
        let before = profile.exact_receipt().unwrap();

        let barrier_root = results.path().join("barrier");
        fs::create_dir(&barrier_root).expect("create copy barrier root");
        let barrier = QualificationProcessBarrierV1::create(&barrier_root, &["copy", "writer"])
            .expect("create copy barrier");
        let backup = results.path().join("completed-backup");
        let writer_result = results.path().join("writer-result");
        let copy = spawn_child(
            "backup",
            root.path(),
            "unused",
            b"",
            &backup,
            Some((&barrier_root, "copy")),
        );
        let writer = spawn_child(
            "create",
            root.path(),
            "journal/later-writer",
            b"later",
            &writer_result,
            Some((&barrier_root, "writer")),
        );
        barrier.wait_until_ready(Duration::from_secs(20)).unwrap();
        barrier.release().unwrap();
        wait_success(copy);
        wait_success(writer);
        barrier.evidence().unwrap().validate_overlap().unwrap();
        let after = profile.exact_receipt().unwrap();

        let restored_root = results.path().join("restored-prefix");
        restore_completed_lmdb_backup_v1(&backup, &restored_root).unwrap();
        let restored = LmdbQualificationProfile::open(&restored_root).unwrap();
        let restored_receipt = restored.exact_receipt().unwrap();
        assert!(restored_receipt == before || restored_receipt == after);
        restored.journal().integrity_check().unwrap();
    }

    #[test]
    fn backup_publishes_candidate_and_content_before_the_completion_marker() {
        let root = tempfile::tempdir().expect("LMDB publication root");
        let profile = LmdbQualificationProfile::open(root.path()).expect("open LMDB profile");
        populate_records(&profile, "backup", 0..4, 64);
        profile
            .put_content_once(
                "sha256:1000000000000000000000000000000000000000000000000000000000000000",
                QualificationRecordKindV1::NoteBody,
                b"independent",
            )
            .unwrap();
        let backup = root.path().join("completed");
        profile
            .backup_to(&backup)
            .expect("publish completed backup");
        let manifest = verify_completed_backup(&backup, &profile.descriptor().unwrap()).unwrap();

        assert!(backup.join(BACKUP_COMPLETION_FILE_V1).is_file());
        assert!(backup.join(BACKUP_MANIFEST_FILE_V1).is_file());
        assert!(
            manifest
                .carriers
                .iter()
                .any(|carrier| carrier.relative_path == LMDB_BACKUP_DATABASE_FILE_V1)
        );
        assert!(
            manifest
                .carriers
                .iter()
                .any(|carrier| carrier.relative_path.starts_with("content/"))
        );
        assert!(
            manifest
                .carriers
                .iter()
                .any(|carrier| carrier.relative_path == LMDB_BACKUP_RECEIPT_FILE_V1)
        );
    }

    #[test]
    fn interrupted_backup_is_incomplete_and_retry_does_not_reinterpret_it() {
        let root = tempfile::tempdir().expect("LMDB interrupted-copy root");
        let results = tempfile::tempdir().expect("LMDB interrupted-copy results");
        let profile = LmdbQualificationProfile::open(root.path()).expect("open LMDB profile");
        populate_records(&profile, "interrupt", 0..32, 2048);
        let barrier_root = results.path().join("barrier");
        fs::create_dir(&barrier_root).expect("create interruption barrier root");
        let barrier = QualificationProcessBarrierV1::create(&barrier_root, &["copy"])
            .expect("create interruption barrier");
        let destination = results.path().join("interrupted");
        let mut child = spawn_child(
            "interrupt_backup",
            root.path(),
            "unused",
            b"",
            &destination,
            Some((&barrier_root, "copy")),
        );
        barrier.wait_until_ready(Duration::from_secs(20)).unwrap();
        child.kill().expect("terminate interrupted copy");
        assert!(!child.wait().expect("wait interrupted copy").success());

        assert!(!destination.join(BACKUP_COMPLETION_FILE_V1).exists());
        assert!(verify_completed_backup(&destination, &profile.descriptor().unwrap()).is_err());
        assert!(profile.backup_to(&destination).is_err());
    }

    #[test]
    fn exact_restore_runs_in_a_fresh_process_without_mutating_the_backup() {
        let root = tempfile::tempdir().expect("LMDB fresh-restore root");
        let results = tempfile::tempdir().expect("LMDB fresh-restore results");
        let profile = LmdbQualificationProfile::open(root.path()).expect("open LMDB profile");
        populate_records(&profile, "restore", 0..12, 256);
        let expected = profile.exact_receipt().unwrap();
        let backup = results.path().join("backup");
        profile.backup_to(&backup).unwrap();
        let backup_before = tree_receipt(&backup);
        let restored_root = results.path().join("restored");
        let receipt_path = results.path().join("restored-receipt.json");

        wait_success(spawn_child(
            "restore",
            &backup,
            restored_root.to_string_lossy().as_ref(),
            b"",
            &receipt_path,
            None,
        ));
        let actual: LmdbExactReceiptV1 =
            serde_json::from_slice(&fs::read(receipt_path).unwrap()).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            truth_carrier_identity(&backup).unwrap(),
            truth_carrier_identity(&restored_root).unwrap()
        );
        assert_eq!(tree_receipt(&backup), backup_before);
    }

    #[test]
    fn fresh_copy_repair_preserves_source_and_rejects_corrupt_or_incomplete_truth() {
        let root = tempfile::tempdir().expect("LMDB repair root");
        let results = tempfile::tempdir().expect("LMDB repair results");
        let profile = LmdbQualificationProfile::open(root.path()).expect("open LMDB profile");
        populate_records(&profile, "repair", 0..8, 128);
        let expected = profile.exact_receipt().unwrap();
        let source_before = tree_receipt(root.path());
        let repaired_backup = results.path().join("repaired-backup");

        profile
            .repair_to(&repaired_backup)
            .expect("fresh-copy repair");
        assert_eq!(tree_receipt(root.path()), source_before);
        let repaired_root = results.path().join("repaired-root");
        restore_completed_lmdb_backup_v1(&repaired_backup, &repaired_root).unwrap();
        assert_eq!(
            LmdbQualificationProfile::open(&repaired_root)
                .unwrap()
                .exact_receipt()
                .unwrap(),
            expected
        );
        assert_eq!(
            truth_carrier_identity(&repaired_backup).unwrap(),
            truth_carrier_identity(&repaired_root).unwrap()
        );

        drop(profile);
        overwrite_raw_journal_value_for_test(root.path(), "journal/corrupt", b"bad").unwrap();
        let corrupt = LmdbQualificationProfile::open(root.path()).unwrap();
        assert!(
            corrupt
                .repair_to(&results.path().join("corrupt-output"))
                .is_err()
        );
        let incomplete = results.path().join("incomplete-output");
        fs::create_dir(&incomplete).unwrap();
        fs::write(incomplete.join("partial"), b"partial").unwrap();
        assert!(corrupt.repair_to(&incomplete).is_err());
    }

    #[test]
    fn inventory_classifies_all_owned_carriers_and_excludes_virtual_map_reservation() {
        let root = tempfile::tempdir().expect("LMDB inventory root");
        let profile = LmdbQualificationProfile::open(root.path()).expect("open LMDB profile");
        populate_records(&profile, "inventory", 0..4, 64);
        profile
            .journal
            .refresh_map()
            .expect("materialize resize lock");
        profile
            .put_content_once(
                "sha256:2000000000000000000000000000000000000000000000000000000000000000",
                QualificationRecordKindV1::ObjectArtifact,
                b"content",
            )
            .unwrap();
        let inventory = profile.inventory().expect("native LMDB inventory");
        let sanitized = profile.sanitized_inventory().expect("sanitized inventory");

        assert_eq!(LmdbCarrierClassV1::ALL.len(), 10);
        assert!(
            sanitized
                .carrier_classes
                .contains(&LmdbCarrierClassV1::Database)
        );
        assert!(
            sanitized
                .carrier_classes
                .contains(&LmdbCarrierClassV1::Lock)
        );
        assert!(
            sanitized
                .carrier_classes
                .contains(&LmdbCarrierClassV1::ResizeLock)
        );
        assert!(
            sanitized
                .carrier_classes
                .contains(&LmdbCarrierClassV1::IndependentContent)
        );
        assert!(inventory.allocated_bytes < profile.current_map_size_bytes());
        assert_eq!(inventory.high_water_bytes, inventory.allocated_bytes);
        assert_eq!(
            sanitized
                .class_inventories
                .iter()
                .find(|class| class.class == LmdbCarrierClassV1::Pinned)
                .expect("pinned class remains explicit")
                .carrier_count,
            0
        );
        let json = serde_json::to_string(&sanitized).unwrap();
        assert!(!json.contains(root.path().to_string_lossy().as_ref()));
        assert!(!json.contains("data.mdb"));
        assert!(!json.contains("lock.mdb"));
    }

    #[test]
    fn lifecycle_schema_mode_and_carrier_names_are_frozen() {
        assert_eq!(
            QUALIFICATION_LMDB_LIFECYCLE_SMOKE_SCHEMA_V1,
            "pointbreak.qualification-lmdb-lifecycle-smoke.v1"
        );
        assert_eq!(
            QUALIFICATION_LMDB_LIFECYCLE_SMOKE_MODE_V1,
            "--lmdb-lifecycle-smoke"
        );
        assert_eq!(
            QUALIFICATION_LMDB_LIFECYCLE_REPORT_MODE_V1,
            "non_timing_lifecycle_receipts"
        );
        assert_eq!(
            serde_json::to_value(LmdbCarrierClassV1::ALL).unwrap(),
            serde_json::json!([
                "database",
                "lock",
                "resize_lock",
                "independent_content",
                "copy",
                "temporary",
                "obsolete",
                "pinned",
                "repair",
                "sidecar"
            ])
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_open_handles_block_replacement_then_allow_reopen_and_cleanup() {
        let root = tempfile::tempdir().expect("LMDB Windows lifecycle root");
        let source = LmdbQualificationProfile::open(&root.path().join("source"))
            .expect("open LMDB source profile");
        populate_records(&source, "windows", 0..4, 64);
        let backup = root.path().join("backup");
        source.backup_to(&backup).expect("create online backup");
        let open_root = root.path().join("open");
        let expected = restore_completed_lmdb_backup_v1(&backup, &open_root).unwrap();
        let profile = LmdbQualificationProfile::open(&open_root).expect("open restored profile");
        let database = open_root.join(JOURNAL_DIRECTORY_V1).join("data.mdb");
        let replacement = open_root.join(JOURNAL_DIRECTORY_V1).join("replacement.mdb");
        copy_file_synced(&backup.join(LMDB_BACKUP_DATABASE_FILE_V1), &replacement)
            .expect("copy offline replacement carrier");
        assert!(fs::remove_file(&database).is_err());
        let closing = heed3::env_closing_event(open_root.join(JOURNAL_DIRECTORY_V1)).unwrap();
        drop(profile);
        closing.wait();
        fs::remove_file(&database).expect("remove after handles close");
        fs::rename(&replacement, &database).expect("install replacement after handles close");
        let reopened = LmdbQualificationProfile::open(&open_root).expect("reopen after replace");
        assert_eq!(reopened.exact_receipt().unwrap(), expected);
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
