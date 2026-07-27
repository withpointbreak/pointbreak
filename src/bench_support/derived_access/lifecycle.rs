use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use serde::{Deserialize, Serialize};

use super::adapter::QualificationDerivedAccessAdapter;
use super::sqlite_cursor::{
    AppendCrashPoint, BootstrapControl, BootstrapCrashPoint, CursorLedgerIdentity,
    SqliteCursorLedger,
};
use super::{
    QualificationDerivedAccessLifecycleCriterionV1, QualificationDerivedAccessLifecycleEvidenceV1,
    QualificationDerivedAccessPlatformV1, QualificationDerivedAccessStatusV1,
    QualificationDerivedAccessTierV1,
};
use crate::bench_support::longitudinal::{
    LongitudinalStoreDataInventoryV1, longitudinal_store_data_inventory_v1,
};
use crate::canonical_hash::{sha256_bytes_hex, sha256_json_prefixed};
use crate::model::JournalId;
use crate::session::derived_access::cursor::{AppendResolution, TruthCursor};
use crate::session::derived_access::locator::LocatorRead;
use crate::session::derived_access::oracle::strict_bodyless_materialized_snapshot;
use crate::session::event::{EventTarget, EventType, ReviewInitializedPayload, ShoreEvent, Writer};
use crate::session::{EventStore, store_dir_for_repo};

pub const QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_REQUEST_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-lifecycle-request.v1";
pub const QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_VECTOR_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-lifecycle-vector.v1";
pub const QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_RUN_REQUEST_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-lifecycle-run-request.v1";
pub const QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_RUN_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-lifecycle-run.v1";
pub const QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_CHILD_MODE_V1: &str =
    "--derived-access-lifecycle-child";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedAccessAppendCrashPointV1 {
    BeforeIntentCommit,
    AfterIntentCommit,
    AfterEventPublication,
    AfterReceiptBeforeHead,
    AfterHeadBeforeIntentRetirement,
}

impl QualificationDerivedAccessAppendCrashPointV1 {
    fn physical(self) -> AppendCrashPoint {
        match self {
            Self::BeforeIntentCommit => AppendCrashPoint::BeforeIntentCommit,
            Self::AfterIntentCommit => AppendCrashPoint::AfterIntentCommit,
            Self::AfterEventPublication => AppendCrashPoint::AfterEventPublication,
            Self::AfterReceiptBeforeHead => AppendCrashPoint::AfterReceiptBeforeHead,
            Self::AfterHeadBeforeIntentRetirement => {
                AppendCrashPoint::AfterHeadBeforeIntentRetirement
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedAccessBootstrapCrashPointV1 {
    DuringStaging,
    AfterQuarantineBeforeNewEpoch,
}

impl QualificationDerivedAccessBootstrapCrashPointV1 {
    fn physical(self) -> BootstrapCrashPoint {
        match self {
            Self::DuringStaging => BootstrapCrashPoint::DuringStaging,
            Self::AfterQuarantineBeforeNewEpoch => {
                BootstrapCrashPoint::AfterQuarantineBeforeNewEpoch
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QualificationDerivedAccessLifecycleActionV1 {
    AppendCrash {
        point: QualificationDerivedAccessAppendCrashPointV1,
        exit_code: u8,
    },
    BootstrapCrash {
        point: QualificationDerivedAccessBootstrapCrashPointV1,
        exit_code: u8,
    },
    Append,
    OpenAndHold {
        ready_path: PathBuf,
        release_path: PathBuf,
    },
    Verify,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessLifecycleChildRequestV1 {
    pub schema: String,
    pub root: PathBuf,
    pub store_id: String,
    pub attempt_token: String,
    pub event_index: u64,
    pub result_path: PathBuf,
    pub action: QualificationDerivedAccessLifecycleActionV1,
}

impl QualificationDerivedAccessLifecycleChildRequestV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_REQUEST_SCHEMA_V1
            || self.store_id.trim().is_empty()
            || self.attempt_token.trim().is_empty()
            || self.root.as_os_str().is_empty()
            || self.result_path.as_os_str().is_empty()
            || self.result_path.starts_with(&self.root) && self.result_path == self.root
        {
            return Err("invalid derived-access lifecycle child request".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessLifecycleVectorV1 {
    pub schema: String,
    pub criterion: QualificationDerivedAccessLifecycleCriterionV1,
    pub requires_child_process: bool,
    pub expected_crash_exit_code: Option<u8>,
    pub description: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessLifecycleRunRequestV1 {
    pub schema: String,
    pub source_checkout: PathBuf,
    pub execution: super::QualificationDerivedAccessExecutionIdentityV1,
    pub root_authority_sha256: String,
    pub source_root: PathBuf,
    pub workspace_root: PathBuf,
    pub admitted_root_sha256: String,
    pub platform: QualificationDerivedAccessPlatformV1,
    pub tier: QualificationDerivedAccessTierV1,
}

impl QualificationDerivedAccessLifecycleRunRequestV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_RUN_REQUEST_SCHEMA_V1
            || self.source_root == self.workspace_root
            || self.execution.platform != self.platform
            || self.execution.root_provenance_sha256 != self.root_authority_sha256
            || !matches!(
                self.tier,
                QualificationDerivedAccessTierV1::D0_128
                    | QualificationDerivedAccessTierV1::L1
                    | QualificationDerivedAccessTierV1::L7
            )
            || !matches!(
                self.platform,
                QualificationDerivedAccessPlatformV1::MacosApfs
                    | QualificationDerivedAccessPlatformV1::WindowsNtfs
            )
            || self.admitted_root_sha256.len() != 64
        {
            return Err("invalid derived-access lifecycle run request".to_owned());
        }
        self.execution.validate()?;
        if self.root_authority_sha256.len() != 64
            || !self
                .root_authority_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("invalid derived-access lifecycle root authority".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessLifecycleVectorReceiptV1 {
    pub criterion: QualificationDerivedAccessLifecycleCriterionV1,
    pub status: QualificationDerivedAccessStatusV1,
    pub detail_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessLifecycleRunReceiptV1 {
    pub schema: String,
    pub execution: super::QualificationDerivedAccessExecutionIdentityV1,
    pub platform: QualificationDerivedAccessPlatformV1,
    pub tier: QualificationDerivedAccessTierV1,
    pub source_before: LongitudinalStoreDataInventoryV1,
    pub source_after: LongitudinalStoreDataInventoryV1,
    pub rows: Vec<QualificationDerivedAccessLifecycleEvidenceV1>,
    pub vectors: Vec<QualificationDerivedAccessLifecycleVectorReceiptV1>,
    pub source_unchanged: bool,
}

pub fn qualification_derived_access_lifecycle_vectors_v1()
-> Vec<QualificationDerivedAccessLifecycleVectorV1> {
    use QualificationDerivedAccessLifecycleCriterionV1 as Criterion;
    QualificationDerivedAccessLifecycleCriterionV1::ALL
        .into_iter()
        .map(|criterion| {
            let (requires_child_process, expected_crash_exit_code, description) = match criterion {
                Criterion::CrashBeforeIntentCommit => (
                    true,
                    Some(81),
                    "terminate before intent commit and preserve the old head",
                ),
                Criterion::CrashAfterIntentBeforeEvent => (
                    true,
                    Some(82),
                    "terminate after intent commit and retire the absent event",
                ),
                Criterion::CrashAfterEventBeforeReceipt => (
                    true,
                    Some(83),
                    "terminate after carrier publication and recover its exact receipt",
                ),
                Criterion::CrashAfterReceiptBeforeHead => (
                    true,
                    Some(84),
                    "terminate after receipt insertion and advance the verified head",
                ),
                Criterion::CrashAfterHeadBeforeIntentRetirement => (
                    true,
                    Some(85),
                    "terminate after head commit and retire the exact intent",
                ),
                Criterion::CrashDuringBootstrapStaging => (
                    true,
                    Some(86),
                    "terminate during staging and require restartable bootstrap",
                ),
                Criterion::CrashDuringQuarantineEpochPublication => (
                    true,
                    Some(87),
                    "terminate after quarantine and before publishing a new epoch",
                ),
                Criterion::ConcurrentWritersLongLivedReader
                | Criterion::UniqueEqualConflictCursorSequence
                | Criterion::ReaderHandleReleaseRetirement => (
                    true,
                    None,
                    "exercise native process and handle coordination",
                ),
                Criterion::DerivedTransactionInterruption => (
                    false,
                    None,
                    "interrupt a derived transaction before its checkpoint commits",
                ),
                Criterion::BackupWithoutDerivedThenRebuild => (
                    false,
                    None,
                    "restore authoritative carriers without the disposable sidecar and rebuild",
                ),
                Criterion::WrongRoot | Criterion::WrongSchema | Criterion::WrongProfile => (
                    false,
                    None,
                    "reject mismatched derived metadata without changing truth",
                ),
                Criterion::CorruptionQuarantineNewEpoch => (
                    false,
                    None,
                    "quarantine corrupt derived state and publish a rebuilt epoch",
                ),
                Criterion::OpenBootstrapReopenReplayEquality => (
                    false,
                    None,
                    "bootstrap, reopen, and compare the full replay receipt",
                ),
                Criterion::IndependentPackageVerification => (
                    false,
                    None,
                    "verify the completed package from an independent process",
                ),
            };
            QualificationDerivedAccessLifecycleVectorV1 {
                schema: QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_VECTOR_SCHEMA_V1.to_owned(),
                criterion,
                requires_child_process,
                expected_crash_exit_code,
                description: description.to_owned(),
            }
        })
        .collect()
}

pub fn run_qualification_derived_access_lifecycle_child_v1(
    request_path: &Path,
) -> Result<(), String> {
    let request: QualificationDerivedAccessLifecycleChildRequestV1 =
        serde_json::from_slice(&std::fs::read(request_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    request.validate()?;
    let identity = CursorLedgerIdentity::new(request.store_id.clone());
    match request.action {
        QualificationDerivedAccessLifecycleActionV1::AppendCrash { point, exit_code } => {
            let ledger = SqliteCursorLedger::open(&request.root, identity)
                .map_err(|error| error.to_string())?;
            let event = lifecycle_event(request.event_index)?;
            ledger
                .append_event_with_hook(&event, &request.attempt_token, |observed| {
                    if observed == point.physical() {
                        std::process::exit(i32::from(exit_code));
                    }
                })
                .map_err(|error| error.to_string())?;
            Err("append crash child returned without reaching its hook".to_owned())
        }
        QualificationDerivedAccessLifecycleActionV1::BootstrapCrash { point, exit_code } => {
            SqliteCursorLedger::bootstrap_from_truth_with_hook(
                &request.root,
                identity,
                2,
                |_| BootstrapControl::Continue,
                |observed| {
                    if observed == point.physical() {
                        std::process::exit(i32::from(exit_code));
                    }
                },
            )
            .map_err(|error| error.to_string())?;
            Err("bootstrap crash child returned without reaching its hook".to_owned())
        }
        QualificationDerivedAccessLifecycleActionV1::Append => {
            let ledger = SqliteCursorLedger::open(&request.root, identity)
                .map_err(|error| error.to_string())?;
            let outcome = ledger
                .append_event(
                    &lifecycle_event(request.event_index)?,
                    &request.attempt_token,
                )
                .map_err(|error| error.to_string())?;
            std::fs::write(&request.result_path, format!("{outcome:?}\n"))
                .map_err(|error| error.to_string())
        }
        QualificationDerivedAccessLifecycleActionV1::OpenAndHold {
            ready_path,
            release_path,
        } => {
            let ledger = SqliteCursorLedger::open(&request.root, identity)
                .map_err(|error| error.to_string())?;
            let _head = ledger.head().map_err(|error| error.to_string())?;
            std::fs::write(&ready_path, b"ready\n").map_err(|error| error.to_string())?;
            while !release_path.exists() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            drop(ledger);
            std::fs::write(&request.result_path, b"released\n").map_err(|error| error.to_string())
        }
        QualificationDerivedAccessLifecycleActionV1::Verify => {
            let ledger = SqliteCursorLedger::open(&request.root, identity)
                .map_err(|error| error.to_string())?;
            ledger
                .integrity_check()
                .map_err(|error| error.to_string())?;
            std::fs::write(&request.result_path, b"verified\n").map_err(|error| error.to_string())
        }
    }
}

pub fn run_qualification_derived_access_lifecycle_v1(
    request_path: &Path,
) -> Result<QualificationDerivedAccessLifecycleRunReceiptV1, String> {
    let request: QualificationDerivedAccessLifecycleRunRequestV1 =
        serde_json::from_slice(&std::fs::read(request_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    request.validate()?;
    super::evidence::validate_current_execution_identity_v1(
        &request.execution,
        &request.source_checkout,
        &request.workspace_root,
    )?;
    let source_before = longitudinal_store_data_inventory_v1(&request.source_root)
        .map_err(|error| error.to_string())?;
    if source_before.inventory_sha256 != request.admitted_root_sha256 {
        return Err("lifecycle source root does not match its admitted identity".to_owned());
    }
    if request.workspace_root.exists()
        && request
            .workspace_root
            .read_dir()
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
    {
        return Err("lifecycle workspace must be absent or empty".to_owned());
    }
    std::fs::create_dir_all(&request.workspace_root).map_err(|error| error.to_string())?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut rows = Vec::new();
    let mut receipts = Vec::new();
    for (ordinal, criterion) in QualificationDerivedAccessLifecycleCriterionV1::ALL
        .into_iter()
        .enumerate()
    {
        let vector_root = request
            .workspace_root
            .join(format!("{ordinal:02}-{criterion:?}"));
        copy_tree(&request.source_root, &vector_root)?;
        let result = run_lifecycle_vector(&executable, &vector_root, criterion);
        let (status, detail) = match result {
            Ok(detail) => (QualificationDerivedAccessStatusV1::Passed, detail),
            Err(detail) => (QualificationDerivedAccessStatusV1::Failed, detail),
        };
        rows.push(QualificationDerivedAccessLifecycleEvidenceV1 {
            tier: request.tier,
            platform: request.platform,
            criterion,
            status,
        });
        receipts.push(QualificationDerivedAccessLifecycleVectorReceiptV1 {
            criterion,
            status,
            detail_sha256: sha256_bytes_hex(detail.as_bytes()),
        });
        if status == QualificationDerivedAccessStatusV1::Failed {
            break;
        }
    }
    for criterion in QualificationDerivedAccessLifecycleCriterionV1::ALL {
        if !rows.iter().any(|row| row.criterion == criterion) {
            rows.push(QualificationDerivedAccessLifecycleEvidenceV1 {
                tier: request.tier,
                platform: request.platform,
                criterion,
                status: QualificationDerivedAccessStatusV1::Unknown,
            });
        }
    }
    rows.sort_by_key(|row| row.criterion);
    let source_after = longitudinal_store_data_inventory_v1(&request.source_root)
        .map_err(|error| error.to_string())?;
    let source_unchanged = source_before == source_after;
    if !source_unchanged {
        return Err("lifecycle run mutated its source root".to_owned());
    }
    Ok(QualificationDerivedAccessLifecycleRunReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_RUN_SCHEMA_V1.to_owned(),
        execution: request.execution,
        platform: request.platform,
        tier: request.tier,
        source_before,
        source_after,
        rows,
        vectors: receipts,
        source_unchanged,
    })
}

pub fn unknown_qualification_derived_access_lifecycle_rows_v1(
    platform: QualificationDerivedAccessPlatformV1,
    tier: QualificationDerivedAccessTierV1,
) -> Vec<QualificationDerivedAccessLifecycleEvidenceV1> {
    QualificationDerivedAccessLifecycleCriterionV1::ALL
        .into_iter()
        .map(|criterion| QualificationDerivedAccessLifecycleEvidenceV1 {
            tier,
            platform,
            criterion,
            status: QualificationDerivedAccessStatusV1::Unknown,
        })
        .collect()
}

fn lifecycle_event(index: u64) -> Result<ShoreEvent, String> {
    let journal_id = JournalId::new(format!("journal:derived-access-lifecycle:{index}"));
    ShoreEvent::new(
        EventType::ReviewInitialized,
        ReviewInitializedPayload::idempotency_key(&journal_id),
        EventTarget::for_journal(journal_id),
        Writer::shore_local(env!("CARGO_PKG_VERSION")),
        ReviewInitializedPayload {},
        "2026-07-27T00:00:00.000Z",
    )
    .map_err(|error| error.to_string())
}

fn run_lifecycle_vector(
    executable: &Path,
    repo_root: &Path,
    criterion: QualificationDerivedAccessLifecycleCriterionV1,
) -> Result<String, String> {
    use QualificationDerivedAccessLifecycleCriterionV1 as Criterion;
    match criterion {
        Criterion::OpenBootstrapReopenReplayEquality => open_replay_vector(repo_root),
        Criterion::ConcurrentWritersLongLivedReader => {
            concurrent_writer_reader_vector(executable, repo_root)
        }
        Criterion::UniqueEqualConflictCursorSequence => unique_equal_conflict_vector(repo_root),
        Criterion::CrashBeforeIntentCommit => crash_append_vector(
            executable,
            repo_root,
            QualificationDerivedAccessAppendCrashPointV1::BeforeIntentCommit,
            81,
            false,
        ),
        Criterion::CrashAfterIntentBeforeEvent => crash_append_vector(
            executable,
            repo_root,
            QualificationDerivedAccessAppendCrashPointV1::AfterIntentCommit,
            82,
            false,
        ),
        Criterion::CrashAfterEventBeforeReceipt => crash_append_vector(
            executable,
            repo_root,
            QualificationDerivedAccessAppendCrashPointV1::AfterEventPublication,
            83,
            true,
        ),
        Criterion::CrashAfterReceiptBeforeHead => crash_append_vector(
            executable,
            repo_root,
            QualificationDerivedAccessAppendCrashPointV1::AfterReceiptBeforeHead,
            84,
            true,
        ),
        Criterion::CrashAfterHeadBeforeIntentRetirement => crash_append_vector(
            executable,
            repo_root,
            QualificationDerivedAccessAppendCrashPointV1::AfterHeadBeforeIntentRetirement,
            85,
            true,
        ),
        Criterion::CrashDuringBootstrapStaging => {
            crash_bootstrap_vector(executable, repo_root, false)
        }
        Criterion::CrashDuringQuarantineEpochPublication => {
            crash_bootstrap_vector(executable, repo_root, true)
        }
        Criterion::DerivedTransactionInterruption => interrupted_transaction_vector(repo_root),
        Criterion::BackupWithoutDerivedThenRebuild => backup_rebuild_vector(repo_root),
        Criterion::WrongRoot => wrong_identity_vector(repo_root, "store:wrong-root"),
        Criterion::WrongSchema => corrupt_metadata_vector(repo_root, "user_version", "99"),
        Criterion::WrongProfile => {
            corrupt_metadata_vector(repo_root, "profile_id", "'wrong-profile'")
        }
        Criterion::CorruptionQuarantineNewEpoch => corruption_rebuild_vector(repo_root),
        Criterion::ReaderHandleReleaseRetirement => reader_retirement_vector(executable, repo_root),
        Criterion::IndependentPackageVerification => {
            independent_verification_vector(executable, repo_root)
        }
    }
}

fn open_replay_vector(repo_root: &Path) -> Result<String, String> {
    let (store, identity, ledger) = bootstrap_vector_root(repo_root)?;
    let expected_head = ledger.head().map_err(|error| error.to_string())?.cursor;
    drop(ledger);
    let adapter = QualificationDerivedAccessAdapter::open(&store, identity)
        .map_err(|error| error.to_string())?;
    let applied = adapter
        .catch_up_to_head(512)
        .map_err(|error| error.to_string())?;
    let events = EventStore::open(&store)
        .list_events()
        .map_err(|error| error.to_string())?;
    let strict =
        strict_bodyless_materialized_snapshot(&events).map_err(|error| error.to_string())?;
    let incremental = match adapter
        .semantic_materialized_audit_snapshot()
        .map_err(|error| error.to_string())?
    {
        LocatorRead::Ready(snapshot) => snapshot,
        LocatorRead::CatchUpRequired { .. } => return Err("reopen remained stale".to_owned()),
    };
    if applied != expected_head || incremental != strict {
        return Err("open/bootstrap/reopen replay equality failed".to_owned());
    }
    Ok(format!("head={expected_head:?};events={}", events.len()))
}

fn unique_equal_conflict_vector(repo_root: &Path) -> Result<String, String> {
    let (_store, _identity, ledger) = bootstrap_vector_root(repo_root)?;
    let before = ledger.head().map_err(|error| error.to_string())?.cursor;
    let event = lifecycle_event(10_001)?;
    let created = ledger
        .append_event(&event, "unique")
        .map_err(|error| error.to_string())?;
    let equal = ledger
        .append_event(&event, "equal")
        .map_err(|error| error.to_string())?;
    let mut conflict = event;
    conflict.payload = serde_json::json!({"conflict": true});
    conflict.payload_hash =
        sha256_json_prefixed(&conflict.payload).map_err(|error| error.to_string())?;
    let conflict = ledger
        .append_event(&conflict, "conflict")
        .map_err(|error| error.to_string())?;
    let after = ledger.head().map_err(|error| error.to_string())?.cursor;
    if !matches!(created, AppendResolution::Created(_))
        || !matches!(equal, AppendResolution::Existing(_))
        || !matches!(conflict, AppendResolution::Conflict(_))
        || after.sequence != before.sequence + 1
    {
        return Err("unique/equal/conflict cursor sequence drifted".to_owned());
    }
    Ok(format!("{created:?}/{equal:?}/{conflict:?}/{after:?}"))
}

fn concurrent_writer_reader_vector(executable: &Path, repo_root: &Path) -> Result<String, String> {
    let (store, _identity, ledger) = bootstrap_vector_root(repo_root)?;
    let before = ledger.head().map_err(|error| error.to_string())?.cursor;
    drop(ledger);
    let control = repo_root.join("lifecycle-control");
    std::fs::create_dir(&control).map_err(|error| error.to_string())?;
    let ready = control.join("reader-ready");
    let release = control.join("reader-release");
    let mut reader = spawn_lifecycle_child(
        executable,
        &control.join("reader-request.json"),
        QualificationDerivedAccessLifecycleChildRequestV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_REQUEST_SCHEMA_V1.to_owned(),
            root: store.clone(),
            store_id: "store:derived-access-lifecycle".to_owned(),
            attempt_token: "reader".to_owned(),
            event_index: 0,
            result_path: control.join("reader-result"),
            action: QualificationDerivedAccessLifecycleActionV1::OpenAndHold {
                ready_path: ready.clone(),
                release_path: release.clone(),
            },
        },
    )?;
    wait_for_path(&ready)?;
    let mut first = spawn_lifecycle_child(
        executable,
        &control.join("writer-a-request.json"),
        append_request(&store, &control.join("writer-a-result"), "writer-a", 10_002),
    )?;
    let mut second = spawn_lifecycle_child(
        executable,
        &control.join("writer-b-request.json"),
        append_request(&store, &control.join("writer-b-result"), "writer-b", 10_002),
    )?;
    require_success(first.wait().map_err(|error| error.to_string())?, "writer A")?;
    require_success(
        second.wait().map_err(|error| error.to_string())?,
        "writer B",
    )?;
    std::fs::write(&release, b"release\n").map_err(|error| error.to_string())?;
    require_success(reader.wait().map_err(|error| error.to_string())?, "reader")?;
    let mut outcomes = [
        std::fs::read_to_string(control.join("writer-a-result"))
            .map_err(|error| error.to_string())?,
        std::fs::read_to_string(control.join("writer-b-result"))
            .map_err(|error| error.to_string())?,
    ];
    outcomes.sort();
    let reopened = SqliteCursorLedger::open(
        &store,
        CursorLedgerIdentity::new("store:derived-access-lifecycle"),
    )
    .map_err(|error| error.to_string())?;
    let after = reopened.head().map_err(|error| error.to_string())?.cursor;
    if !outcomes[0].contains("Created")
        || !outcomes[1].contains("Existing")
        || after.sequence != before.sequence + 1
    {
        return Err("concurrent writers and reader produced unexpected outcomes".to_owned());
    }
    Ok(format!("{outcomes:?}/{after:?}"))
}

fn crash_append_vector(
    executable: &Path,
    repo_root: &Path,
    point: QualificationDerivedAccessAppendCrashPointV1,
    exit_code: u8,
    advances: bool,
) -> Result<String, String> {
    let (store, _identity, ledger) = bootstrap_vector_root(repo_root)?;
    let before = ledger.head().map_err(|error| error.to_string())?.cursor;
    drop(ledger);
    let request_path = repo_root.join("crash-request.json");
    let result_path = repo_root.join("crash-result");
    let mut child = spawn_lifecycle_child(
        executable,
        &request_path,
        QualificationDerivedAccessLifecycleChildRequestV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_REQUEST_SCHEMA_V1.to_owned(),
            root: store.clone(),
            store_id: "store:derived-access-lifecycle".to_owned(),
            attempt_token: format!("crash-{exit_code}"),
            event_index: 10_003 + u64::from(exit_code),
            result_path,
            action: QualificationDerivedAccessLifecycleActionV1::AppendCrash { point, exit_code },
        },
    )?;
    let status = child.wait().map_err(|error| error.to_string())?;
    if status.code() != Some(i32::from(exit_code)) {
        return Err(format!(
            "crash child returned {status} instead of {exit_code}"
        ));
    }
    let reopened = SqliteCursorLedger::open(
        &store,
        CursorLedgerIdentity::new("store:derived-access-lifecycle"),
    )
    .map_err(|error| error.to_string())?;
    reopened.recover().map_err(|error| error.to_string())?;
    let after = reopened.head().map_err(|error| error.to_string())?.cursor;
    if after.sequence != before.sequence + u64::from(advances) {
        return Err(format!(
            "crash recovery head drifted: {before:?} -> {after:?}"
        ));
    }
    Ok(format!("{point:?}/{before:?}/{after:?}"))
}

fn crash_bootstrap_vector(
    executable: &Path,
    repo_root: &Path,
    quarantine: bool,
) -> Result<String, String> {
    let store = store_dir_for_repo(repo_root).map_err(|error| error.to_string())?;
    if quarantine {
        let sidecar = store.join(".pointbreak-derived");
        std::fs::create_dir_all(&sidecar).map_err(|error| error.to_string())?;
        std::fs::write(sidecar.join("cursor.sqlite3"), b"corrupt")
            .map_err(|error| error.to_string())?;
    }
    let (point, exit_code) = if quarantine {
        (
            QualificationDerivedAccessBootstrapCrashPointV1::AfterQuarantineBeforeNewEpoch,
            87,
        )
    } else {
        (
            QualificationDerivedAccessBootstrapCrashPointV1::DuringStaging,
            86,
        )
    };
    let mut child = spawn_lifecycle_child(
        executable,
        &repo_root.join("bootstrap-crash-request.json"),
        QualificationDerivedAccessLifecycleChildRequestV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_REQUEST_SCHEMA_V1.to_owned(),
            root: store.clone(),
            store_id: "store:derived-access-lifecycle".to_owned(),
            attempt_token: "bootstrap".to_owned(),
            event_index: 0,
            result_path: repo_root.join("bootstrap-crash-result"),
            action: QualificationDerivedAccessLifecycleActionV1::BootstrapCrash {
                point,
                exit_code,
            },
        },
    )?;
    let status = child.wait().map_err(|error| error.to_string())?;
    if status.code() != Some(i32::from(exit_code)) {
        return Err(format!("bootstrap crash child returned {status}"));
    }
    let ledger = SqliteCursorLedger::bootstrap_from_truth(
        &store,
        CursorLedgerIdentity::new("store:derived-access-lifecycle"),
        2,
        |_| BootstrapControl::Continue,
    )
    .map_err(|error| error.to_string())?;
    let events = EventStore::open(&store)
        .list_events()
        .map_err(|error| error.to_string())?;
    if ledger
        .head()
        .map_err(|error| error.to_string())?
        .cursor
        .sequence
        != events.len() as u64
    {
        return Err("bootstrap restart did not reach truth head".to_owned());
    }
    Ok(format!("{point:?}/events={}", events.len()))
}

fn interrupted_transaction_vector(repo_root: &Path) -> Result<String, String> {
    let (store, identity, ledger) = bootstrap_vector_root(repo_root)?;
    let head = ledger.head().map_err(|error| error.to_string())?.cursor;
    drop(ledger);
    let adapter = QualificationDerivedAccessAdapter::open(&store, identity)
        .map_err(|error| error.to_string())?;
    if adapter.catch_up_with_interruption(512).is_ok()
        || adapter
            .locator_checkpoint()
            .map_err(|error| error.to_string())?
            != TruthCursor::new(head.epoch, 0)
    {
        return Err("interrupted derived transaction published partial state".to_owned());
    }
    let applied = adapter
        .catch_up_to_head(512)
        .map_err(|error| error.to_string())?;
    if applied != head {
        return Err("interrupted derived transaction did not retry cleanly".to_owned());
    }
    Ok(format!("{head:?}"))
}

fn backup_rebuild_vector(repo_root: &Path) -> Result<String, String> {
    let (store, _identity, ledger) = bootstrap_vector_root(repo_root)?;
    let expected = ledger.head().map_err(|error| error.to_string())?.cursor;
    drop(ledger);
    std::fs::remove_dir_all(store.join(".pointbreak-derived"))
        .map_err(|error| error.to_string())?;
    let rebuilt = SqliteCursorLedger::bootstrap_from_truth(
        &store,
        CursorLedgerIdentity::new("store:derived-access-lifecycle"),
        2,
        |_| BootstrapControl::Continue,
    )
    .map_err(|error| error.to_string())?;
    let observed = rebuilt.head().map_err(|error| error.to_string())?.cursor;
    if observed.sequence != expected.sequence || observed.epoch != 2 {
        return Err("backup without sidecar did not rebuild exact truth".to_owned());
    }
    Ok(format!("{expected:?}/{observed:?}"))
}

fn wrong_identity_vector(repo_root: &Path, identity: &str) -> Result<String, String> {
    let (store, _identity, ledger) = bootstrap_vector_root(repo_root)?;
    drop(ledger);
    if SqliteCursorLedger::open(&store, CursorLedgerIdentity::new(identity)).is_ok() {
        return Err("wrong root/store identity was accepted".to_owned());
    }
    Ok(identity.to_owned())
}

fn corrupt_metadata_vector(repo_root: &Path, column: &str, value: &str) -> Result<String, String> {
    if !matches!(column, "user_version" | "profile_id") {
        return Err("unsupported metadata corruption column".to_owned());
    }
    let (store, _identity, ledger) = bootstrap_vector_root(repo_root)?;
    drop(ledger);
    let database = store.join(".pointbreak-derived/cursor.sqlite3");
    let connection = rusqlite::Connection::open(&database).map_err(|error| error.to_string())?;
    let statement = if column == "user_version" {
        format!("PRAGMA user_version = {value}")
    } else {
        format!("UPDATE cursor_meta SET {column} = {value} WHERE singleton = 1")
    };
    connection
        .execute_batch(&statement)
        .map_err(|error| error.to_string())?;
    drop(connection);
    if SqliteCursorLedger::open(
        &store,
        CursorLedgerIdentity::new("store:derived-access-lifecycle"),
    )
    .is_ok()
    {
        return Err(format!("wrong {column} was accepted"));
    }
    Ok(format!("{column}={value}"))
}

fn corruption_rebuild_vector(repo_root: &Path) -> Result<String, String> {
    let (store, _identity, ledger) = bootstrap_vector_root(repo_root)?;
    let sequence = ledger
        .head()
        .map_err(|error| error.to_string())?
        .cursor
        .sequence;
    drop(ledger);
    let database = store.join(".pointbreak-derived/cursor.sqlite3");
    std::fs::write(&database, b"not a sqlite database").map_err(|error| error.to_string())?;
    if SqliteCursorLedger::open(
        &store,
        CursorLedgerIdentity::new("store:derived-access-lifecycle"),
    )
    .is_ok()
    {
        return Err("corrupt sidecar was accepted".to_owned());
    }
    let rebuilt = SqliteCursorLedger::bootstrap_from_truth(
        &store,
        CursorLedgerIdentity::new("store:derived-access-lifecycle"),
        2,
        |_| BootstrapControl::Continue,
    )
    .map_err(|error| error.to_string())?;
    if rebuilt.head().map_err(|error| error.to_string())?.cursor != TruthCursor::new(2, sequence) {
        return Err("corrupt sidecar rebuild did not preserve truth".to_owned());
    }
    Ok(format!("sequence={sequence}"))
}

fn reader_retirement_vector(executable: &Path, repo_root: &Path) -> Result<String, String> {
    let (store, _identity, ledger) = bootstrap_vector_root(repo_root)?;
    drop(ledger);
    let control = repo_root.join("reader-retirement");
    std::fs::create_dir(&control).map_err(|error| error.to_string())?;
    let ready = control.join("ready");
    let release = control.join("release");
    let mut reader = spawn_lifecycle_child(
        executable,
        &control.join("request.json"),
        QualificationDerivedAccessLifecycleChildRequestV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_REQUEST_SCHEMA_V1.to_owned(),
            root: store.clone(),
            store_id: "store:derived-access-lifecycle".to_owned(),
            attempt_token: "reader-retirement".to_owned(),
            event_index: 0,
            result_path: control.join("result"),
            action: QualificationDerivedAccessLifecycleActionV1::OpenAndHold {
                ready_path: ready.clone(),
                release_path: release.clone(),
            },
        },
    )?;
    wait_for_path(&ready)?;
    std::fs::write(&release, b"release\n").map_err(|error| error.to_string())?;
    require_success(reader.wait().map_err(|error| error.to_string())?, "reader")?;
    std::fs::remove_dir_all(store.join(".pointbreak-derived"))
        .map_err(|error| error.to_string())?;
    let rebuilt = SqliteCursorLedger::bootstrap_from_truth(
        &store,
        CursorLedgerIdentity::new("store:derived-access-lifecycle"),
        2,
        |_| BootstrapControl::Continue,
    )
    .map_err(|error| error.to_string())?;
    rebuilt
        .integrity_check()
        .map_err(|error| error.to_string())?;
    Ok("reader released and sidecar retired".to_owned())
}

fn independent_verification_vector(executable: &Path, repo_root: &Path) -> Result<String, String> {
    let (store, _identity, ledger) = bootstrap_vector_root(repo_root)?;
    drop(ledger);
    let request_path = repo_root.join("verify-request.json");
    let result_path = repo_root.join("verify-result");
    let mut child = spawn_lifecycle_child(
        executable,
        &request_path,
        QualificationDerivedAccessLifecycleChildRequestV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_REQUEST_SCHEMA_V1.to_owned(),
            root: store,
            store_id: "store:derived-access-lifecycle".to_owned(),
            attempt_token: "verify".to_owned(),
            event_index: 0,
            result_path: result_path.clone(),
            action: QualificationDerivedAccessLifecycleActionV1::Verify,
        },
    )?;
    require_success(
        child.wait().map_err(|error| error.to_string())?,
        "independent verifier",
    )?;
    if std::fs::read_to_string(result_path).map_err(|error| error.to_string())? != "verified\n" {
        return Err("independent verifier result drifted".to_owned());
    }
    Ok("independent child verification passed".to_owned())
}

fn bootstrap_vector_root(
    repo_root: &Path,
) -> Result<(PathBuf, CursorLedgerIdentity, SqliteCursorLedger), String> {
    let store = store_dir_for_repo(repo_root).map_err(|error| error.to_string())?;
    let identity = CursorLedgerIdentity::new("store:derived-access-lifecycle");
    let ledger = SqliteCursorLedger::bootstrap_from_truth(&store, identity.clone(), 1, |_| {
        BootstrapControl::Continue
    })
    .map_err(|error| error.to_string())?;
    Ok((store, identity, ledger))
}

fn append_request(
    store: &Path,
    result: &Path,
    attempt: &str,
    event_index: u64,
) -> QualificationDerivedAccessLifecycleChildRequestV1 {
    QualificationDerivedAccessLifecycleChildRequestV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_REQUEST_SCHEMA_V1.to_owned(),
        root: store.to_path_buf(),
        store_id: "store:derived-access-lifecycle".to_owned(),
        attempt_token: attempt.to_owned(),
        event_index,
        result_path: result.to_path_buf(),
        action: QualificationDerivedAccessLifecycleActionV1::Append,
    }
}

fn spawn_lifecycle_child(
    executable: &Path,
    request_path: &Path,
    request: QualificationDerivedAccessLifecycleChildRequestV1,
) -> Result<std::process::Child, String> {
    request.validate()?;
    let mut bytes = serde_json::to_vec_pretty(&request).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    std::fs::write(request_path, bytes).map_err(|error| error.to_string())?;
    Command::new(executable)
        .arg(QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_CHILD_MODE_V1)
        .arg(request_path)
        .spawn()
        .map_err(|error| error.to_string())
}

fn wait_for_path(path: &Path) -> Result<(), String> {
    for _ in 0..3_000 {
        if path.exists() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Err(format!("timed out waiting for {}", path.display()))
}

fn require_success(status: ExitStatus, label: &str) -> Result<(), String> {
    if status.success() {
        Ok(())
    } else {
        Err(format!("{label} failed with {status}"))
    }
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::create_dir(destination).map_err(|error| error.to_string())?;
    for entry in std::fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let target = destination.join(entry.file_name());
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), target).map_err(|error| error.to_string())?;
        } else {
            return Err("lifecycle source contains a non-file entry".to_owned());
        }
    }
    Ok(())
}
