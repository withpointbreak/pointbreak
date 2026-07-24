use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    LONGITUDINAL_CAPACITY_MATERIALIZATION_RECEIPT_SCHEMA_V1,
    LONGITUDINAL_MATERIALIZATION_RECEIPT_SCHEMA_V1, LongitudinalC524GateInputsV1,
    LongitudinalC524GateReceiptV1, LongitudinalCapacityManifestV1,
    LongitudinalCapacityMaterializationReceiptV1, LongitudinalCapacityPackageV1,
    LongitudinalCapacityProbeV1, LongitudinalCapacityProfileV1, LongitudinalCapacityReceiptV1,
    LongitudinalCapacitySubjectV1, LongitudinalEvidencePackageV1, LongitudinalExecutionIdentityV1,
    LongitudinalLaneV1, LongitudinalMaterializationReceiptV1, LongitudinalOperationReceiptV1,
    LongitudinalOperationV1, LongitudinalTierV1, LongitudinalWorkloadManifestV1,
    longitudinal_capacity_contract_v1, longitudinal_runner_contract_v1,
    materialize_longitudinal_capacity_v1, materialize_longitudinal_workload_v1,
};
use crate::bench_support::foundation::{
    QualificationFilesystemDispositionV1, classify_qualification_filesystem,
    qualification_filesystem_name,
};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
use crate::model::ObjectId;
use crate::session::benchmark::{
    append_longitudinal_contention_writer_v1, append_longitudinal_event_slice_v1,
    read_longitudinal_carrier_by_key_v1, stage_longitudinal_append_records_v1,
};
use crate::session::{
    SessionState, StoreMode, event_log_head_marker, read_bound_object_artifact, read_events,
    set_store_mode_for_repo, store_dir_for_repo, store_id_index,
};

pub const LONGITUDINAL_EVIDENCE_PACKAGE_FILE_V1: &str = "longitudinal-evidence-package.json";
pub const LONGITUDINAL_CAPACITY_PACKAGE_FILE_V1: &str = "longitudinal-capacity-package.json";
const LONGITUDINAL_PACKAGE_VERIFICATION_RECEIPT_FILE_V1: &str = "package-receipt.json";

const PROTECTED_ENVIRONMENT_VARIABLES: [&str; 4] = [
    "POINTBREAK_QUALIFICATION_CORPUS",
    "POINTBREAK_BENCH_FIXTURE",
    "POINTBREAK_BENCH_REPO",
    "POINTBREAK_HOME",
];

#[derive(Debug, Error)]
pub enum LongitudinalEvidenceError {
    #[error("longitudinal evidence root must be a new absolute local path")]
    UnsafeRoot,
    #[error("longitudinal evidence root is protected or synchronized")]
    ProtectedRoot,
    #[error("longitudinal evidence requires protected input variables to be unset")]
    ProtectedEnvironment,
    #[error("longitudinal evidence source identity does not match the clean checkout")]
    SourceIdentity,
    #[error("longitudinal evidence runner identity does not match the supplied binary")]
    RunnerIdentity,
    #[error("longitudinal evidence input failed strict preflight")]
    Preflight,
    #[error("longitudinal evidence lane or schedule is unavailable")]
    UnavailableLane,
    #[error("longitudinal evidence receipt or package is invalid")]
    InvalidReceipt,
    #[error("longitudinal C524 evidence was not admitted by the typed gate")]
    C524NotAdmitted,
    #[error("longitudinal evidence I/O failed: {0}")]
    Io(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongitudinalEvidenceMaterializeOptionsV1 {
    pub root: PathBuf,
    pub source_root: PathBuf,
    pub runner: PathBuf,
    pub tier: LongitudinalTierV1,
    pub execution: LongitudinalExecutionIdentityV1,
}

impl LongitudinalEvidenceMaterializeOptionsV1 {
    pub fn new(
        root: impl AsRef<Path>,
        source_root: impl AsRef<Path>,
        runner: impl AsRef<Path>,
        tier: LongitudinalTierV1,
        execution: LongitudinalExecutionIdentityV1,
    ) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            source_root: source_root.as_ref().to_path_buf(),
            runner: runner.as_ref().to_path_buf(),
            tier,
            execution,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongitudinalCapacityEvidenceMaterializeOptionsV1 {
    pub root: PathBuf,
    pub source_root: PathBuf,
    pub runner: PathBuf,
    pub subject: LongitudinalCapacitySubjectV1,
    pub execution: LongitudinalExecutionIdentityV1,
    pub c524_gate: Option<LongitudinalC524GateReceiptV1>,
    pub c524_gate_inputs: Option<LongitudinalC524GateInputsV1>,
}

impl LongitudinalCapacityEvidenceMaterializeOptionsV1 {
    pub fn new(
        root: impl AsRef<Path>,
        source_root: impl AsRef<Path>,
        runner: impl AsRef<Path>,
        subject: LongitudinalCapacitySubjectV1,
        execution: LongitudinalExecutionIdentityV1,
    ) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            source_root: source_root.as_ref().to_path_buf(),
            runner: runner.as_ref().to_path_buf(),
            subject,
            execution,
            c524_gate: None,
            c524_gate_inputs: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongitudinalRunOptionsV1 {
    pub root: PathBuf,
    pub manifest: LongitudinalWorkloadManifestV1,
    pub lane: LongitudinalLaneV1,
    pub operations: Vec<LongitudinalOperationReceiptV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalLaneReceiptV1 {
    pub lane: LongitudinalLaneV1,
    pub root_identity: String,
    pub manifest_sha256: String,
    pub operations: Vec<LongitudinalOperationReceiptV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongitudinalCapacityRunOptionsV1 {
    pub manifest: LongitudinalCapacityManifestV1,
    pub lane: LongitudinalLaneV1,
    pub probe_output: LongitudinalCapacityProbeOutputV1,
    pub receipt: LongitudinalCapacityReceiptV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCapacityProbeOutputV1 {
    pub probe: LongitudinalCapacityProbeV1,
    pub output_count: u64,
    pub selected_bytes: u64,
    pub ordered_ids: Vec<String>,
    pub fact_ids: Vec<String>,
    pub diagnostics: Vec<String>,
    pub semantic_receipt_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalContentionWriterReceiptV1 {
    pub writer_index: u8,
    pub manifest_sha256: String,
    pub attempt_event_ids: Vec<String>,
    pub outcomes: Vec<String>,
    pub created: u8,
    pub existing: u8,
    pub final_event_count: u64,
    pub event_set_sha256: String,
    pub receipt_sha256: String,
}

impl LongitudinalContentionWriterReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalEvidenceError> {
        let mut preimage = self.clone();
        preimage.receipt_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn validate(&self) -> Result<(), LongitudinalEvidenceError> {
        if self.writer_index > 1
            || self.attempt_event_ids.len() != 6
            || self.outcomes.len() != 6
            || u8::try_from(self.outcomes.len()).ok() != self.created.checked_add(self.existing)
            || self
                .outcomes
                .iter()
                .any(|outcome| outcome != "created" && outcome != "existing")
            || self.receipt_sha256 != self.canonical_sha256()?
        {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalContentionReaderCycleReceiptV1 {
    pub cycle_ordinal: u8,
    pub carrier_semantic_sha256: String,
    pub semantic_id_sha256: String,
    pub chronological_window_sha256: String,
    pub observed_event_count: u64,
    pub observed_event_set_sha256: String,
    pub receipt_sha256: String,
}

impl LongitudinalContentionReaderCycleReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalEvidenceError> {
        let mut preimage = self.clone();
        preimage.receipt_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn validate(&self) -> Result<(), LongitudinalEvidenceError> {
        for hash in [
            &self.carrier_semantic_sha256,
            &self.semantic_id_sha256,
            &self.chronological_window_sha256,
            &self.observed_event_set_sha256,
        ] {
            if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(LongitudinalEvidenceError::InvalidReceipt);
            }
        }
        if self.receipt_sha256 != self.canonical_sha256()? {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }
        Ok(())
    }
}

impl LongitudinalCapacityProbeOutputV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalEvidenceError> {
        let mut preimage = self.clone();
        preimage.semantic_receipt_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn validate(&self) -> Result<(), LongitudinalEvidenceError> {
        if self.semantic_receipt_sha256 != self.canonical_sha256()?
            || (self.probe == LongitudinalCapacityProbeV1::AppendDelta
                && (self.output_count != 0
                    || self.selected_bytes != 0
                    || self.diagnostics.as_slice() != ["unsupported_no_current_surface"]))
        {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalSmokeReceiptV1 {
    pub schema: String,
    pub timing_admissible: bool,
    pub terminal_evidence_admissible: bool,
    pub pair_verified: bool,
    pub preflight_verified: bool,
    pub package_mechanics_verified: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalAppendReceiptV1 {
    pub schema: String,
    pub tier: LongitudinalTierV1,
    pub start_ordinal: u64,
    pub appended_events: u64,
    pub pre_event_count: u64,
    pub post_event_count: u64,
    pub event_set_sha256: String,
    pub receipt_sha256: String,
}

impl LongitudinalAppendReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalEvidenceError> {
        let mut preimage = self.clone();
        preimage.receipt_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn validate(&self) -> Result<(), LongitudinalEvidenceError> {
        if self.schema != "pointbreak.longitudinal-append-receipt.v1"
            || self.post_event_count
                != self
                    .pre_event_count
                    .checked_add(self.appended_events)
                    .ok_or(LongitudinalEvidenceError::InvalidReceipt)?
            || self.receipt_sha256 != self.canonical_sha256()?
        {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }
        Ok(())
    }
}

pub fn longitudinal_execution_identity_v1(
    source_root: &Path,
    runner: &Path,
    root_parent: &Path,
    build_profile: impl Into<String>,
    parent_commit: Option<String>,
) -> Result<LongitudinalExecutionIdentityV1, LongitudinalEvidenceError> {
    let source_root =
        fs::canonicalize(source_root).map_err(|_| LongitudinalEvidenceError::SourceIdentity)?;
    let runner = fs::canonicalize(runner).map_err(|_| LongitudinalEvidenceError::RunnerIdentity)?;
    let root_parent =
        fs::canonicalize(root_parent).map_err(|_| LongitudinalEvidenceError::UnsafeRoot)?;
    if !runner.is_file() || !git_output(&source_root, &["status", "--porcelain=v1"])?.is_empty() {
        return Err(LongitudinalEvidenceError::SourceIdentity);
    }
    let identity = LongitudinalExecutionIdentityV1 {
        source_commit: git_output(&source_root, &["rev-parse", "HEAD"])?,
        source_tree: git_output(&source_root, &["rev-parse", "HEAD^{tree}"])?,
        cargo_lock_sha256: sha256_file(&source_root.join("Cargo.lock"))?,
        runner_sha256: sha256_file(&runner)?,
        build_profile: build_profile.into(),
        operating_system: std::env::consts::OS.to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
        filesystem: qualification_filesystem_name(&root_parent),
        parent_commit,
    };
    identity
        .validate()
        .map_err(|_| LongitudinalEvidenceError::SourceIdentity)?;
    Ok(identity)
}

pub fn longitudinal_canonical_sha256_v1(
    value: &impl Serialize,
) -> Result<String, LongitudinalEvidenceError> {
    canonical_sha256(value)
}

pub fn longitudinal_root_event_set_v1(
    root: &Path,
) -> Result<(u64, String), LongitudinalEvidenceError> {
    let events = read_events(root).map_err(|_| LongitudinalEvidenceError::Preflight)?;
    Ok((events.len() as u64, event_set_sha256(&events)?))
}

pub fn materialize_longitudinal_evidence_root_v1(
    options: LongitudinalEvidenceMaterializeOptionsV1,
) -> Result<LongitudinalMaterializationReceiptV1, LongitudinalEvidenceError> {
    validate_materialization_boundary(
        &options.root,
        &options.source_root,
        &options.runner,
        &options.execution,
    )?;
    initialize_evidence_root(&options.root)?;
    materialize_longitudinal_workload_v1(super::LongitudinalMaterializeOptionsV1::new(
        &options.root,
        options.tier,
        options.execution,
    ))
    .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)
}

pub fn prepare_longitudinal_append_records_v1(
    root: &Path,
    tier: LongitudinalTierV1,
) -> Result<(), LongitudinalEvidenceError> {
    let requirement = longitudinal_runner_contract_v1()
        .tiers
        .into_iter()
        .find(|requirement| requirement.tier == tier)
        .ok_or(LongitudinalEvidenceError::InvalidReceipt)?;
    if event_log_head_marker(root).map_err(|_| LongitudinalEvidenceError::Preflight)?
        != requirement.event_count
    {
        return Err(LongitudinalEvidenceError::Preflight);
    }
    stage_longitudinal_append_records_v1(root, requirement.block_count, 2)
        .map_err(|error| LongitudinalEvidenceError::Io(error.to_string()))
}

pub fn append_longitudinal_events_v1(
    root: &Path,
    tier: LongitudinalTierV1,
    start_ordinal: u64,
    count: u64,
) -> Result<LongitudinalAppendReceiptV1, LongitudinalEvidenceError> {
    if count == 0 || start_ordinal.checked_add(count).is_none_or(|end| end > 512) {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let requirement = longitudinal_runner_contract_v1()
        .tiers
        .into_iter()
        .find(|requirement| requirement.tier == tier)
        .ok_or(LongitudinalEvidenceError::InvalidReceipt)?;
    let pre_event_count =
        event_log_head_marker(root).map_err(|_| LongitudinalEvidenceError::Preflight)?;
    if pre_event_count
        != requirement
            .event_count
            .checked_add(start_ordinal)
            .ok_or(LongitudinalEvidenceError::InvalidReceipt)?
    {
        return Err(LongitudinalEvidenceError::Preflight);
    }
    let write =
        append_longitudinal_event_slice_v1(root, requirement.block_count, start_ordinal, count)
            .map_err(|error| LongitudinalEvidenceError::Io(error.to_string()))?;
    if write.events_created != count
        || write.events_existing != 0
        || write.final_event_count != pre_event_count + count
    {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let mut receipt = LongitudinalAppendReceiptV1 {
        schema: "pointbreak.longitudinal-append-receipt.v1".to_owned(),
        tier,
        start_ordinal,
        appended_events: count,
        pre_event_count,
        post_event_count: write.final_event_count,
        event_set_sha256: write.event_set_sha256,
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = receipt.canonical_sha256()?;
    receipt.validate()?;
    Ok(receipt)
}

pub fn materialize_longitudinal_capacity_evidence_root_v1(
    options: LongitudinalCapacityEvidenceMaterializeOptionsV1,
) -> Result<LongitudinalCapacityMaterializationReceiptV1, LongitudinalEvidenceError> {
    validate_materialization_boundary(
        &options.root,
        &options.source_root,
        &options.runner,
        &options.execution,
    )?;
    if options.subject
        == LongitudinalCapacitySubjectV1::Companion(LongitudinalCapacityProfileV1::C524)
    {
        let (Some(inputs), Some(gate)) = (&options.c524_gate_inputs, &options.c524_gate) else {
            return Err(LongitudinalEvidenceError::C524NotAdmitted);
        };
        gate.validate_against(inputs)
            .map_err(|_| LongitudinalEvidenceError::C524NotAdmitted)?;
        if !gate.admitted {
            return Err(LongitudinalEvidenceError::C524NotAdmitted);
        }
    } else if options.c524_gate.is_some() || options.c524_gate_inputs.is_some() {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }

    initialize_evidence_root(&options.root)?;
    match options.subject {
        LongitudinalCapacitySubjectV1::V1L100 => {
            let receipt =
                materialize_longitudinal_workload_v1(super::LongitudinalMaterializeOptionsV1::new(
                    &options.root,
                    LongitudinalTierV1::L100,
                    options.execution,
                ))
                .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
            capacity_receipt_from_v1_l100(receipt)
        }
        LongitudinalCapacitySubjectV1::Companion(profile) => materialize_longitudinal_capacity_v1(
            super::LongitudinalCapacityMaterializeOptionsV1::new(
                &options.root,
                profile,
                options.execution,
            ),
        )
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt),
    }
}

pub fn preflight_longitudinal_root_v1(
    root: &Path,
    manifest: &LongitudinalWorkloadManifestV1,
) -> Result<LongitudinalMaterializationReceiptV1, LongitudinalEvidenceError> {
    manifest
        .validate()
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let strict = strict_preflight(
        root,
        &manifest.ordered_events,
        &manifest.event_carriers,
        &manifest.content_inventory,
    )?;
    let mut receipt = LongitudinalMaterializationReceiptV1 {
        schema: LONGITUDINAL_MATERIALIZATION_RECEIPT_SCHEMA_V1.to_owned(),
        root_identity: root_identity(root)?,
        manifest: manifest.clone(),
        strict,
        materialization_sha256: String::new(),
    };
    receipt.materialization_sha256 = receipt
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    receipt
        .validate()
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    Ok(receipt)
}

pub fn preflight_longitudinal_capacity_root_v1(
    root: &Path,
    manifest: &LongitudinalCapacityManifestV1,
) -> Result<LongitudinalCapacityMaterializationReceiptV1, LongitudinalEvidenceError> {
    manifest
        .validate()
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let strict = strict_preflight(
        root,
        &manifest.ordered_events,
        &manifest.event_carriers,
        &manifest.content_inventory,
    )?;
    let mut receipt = LongitudinalCapacityMaterializationReceiptV1 {
        schema: LONGITUDINAL_CAPACITY_MATERIALIZATION_RECEIPT_SCHEMA_V1.to_owned(),
        root_identity: root_identity(root)?,
        manifest: manifest.clone(),
        strict,
        materialization_sha256: String::new(),
    };
    receipt.materialization_sha256 = receipt
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    receipt
        .validate()
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    Ok(receipt)
}

pub fn run_longitudinal_lane_v1(
    options: LongitudinalRunOptionsV1,
) -> Result<LongitudinalLaneReceiptV1, LongitudinalEvidenceError> {
    if options.lane == LongitudinalLaneV1::AttributionCounts {
        return Err(LongitudinalEvidenceError::UnavailableLane);
    }
    options
        .manifest
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    let root_identity = root_identity(&options.root)?;
    let execution_identity = options
        .manifest
        .execution
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    if options.operations.is_empty() {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let mut sample_keys = BTreeSet::new();
    let expected_semantics = options
        .manifest
        .expected_semantic_receipts
        .iter()
        .map(|receipt| (receipt.operation, receipt.semantic_receipt_sha256.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut samples_by_operation = BTreeMap::<
        LongitudinalOperationV1,
        Vec<(u32, super::LongitudinalOperationOutcomeV1)>,
    >::new();
    for receipt in &options.operations {
        receipt
            .validate()
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
        if receipt.lane != options.lane
            || receipt.root_identity != root_identity
            || receipt.tier != options.manifest.tier
            || receipt.manifest_sha256 != options.manifest.manifest_sha256
            || receipt.schedule_sha256 != options.manifest.schedule_sha256
            || receipt.execution_identity_sha256 != execution_identity
            || receipt.semantic_receipt_sha256
                != expected_semantics
                    .get(&receipt.operation)
                    .copied()
                    .ok_or(LongitudinalEvidenceError::InvalidReceipt)?
            || !sample_keys.insert((receipt.operation, receipt.sample_ordinal))
        {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }
        samples_by_operation
            .entry(receipt.operation)
            .or_default()
            .push((receipt.sample_ordinal, receipt.outcome));
    }
    let sample_plan = longitudinal_runner_contract_v1().samples;
    for operation in LongitudinalOperationV1::ALL {
        let expected = expected_operation_samples(options.lane, operation, &sample_plan)?;
        let mut actual = samples_by_operation.remove(&operation).unwrap_or_default();
        actual.sort_unstable();
        let measured = (0..u32::from(expected))
            .map(|sample| (sample, super::LongitudinalOperationOutcomeV1::Measured))
            .collect::<Vec<_>>();
        let governed_omission = matches!(
            actual.as_slice(),
            [(
                0,
                super::LongitudinalOperationOutcomeV1::UnavailableNoCurrentSurface
                    | super::LongitudinalOperationOutcomeV1::ValidFailure
            )]
        );
        if actual != measured && !governed_omission {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }
    }
    Ok(LongitudinalLaneReceiptV1 {
        lane: options.lane,
        root_identity,
        manifest_sha256: options.manifest.manifest_sha256,
        operations: options.operations,
    })
}

fn expected_operation_samples(
    lane: LongitudinalLaneV1,
    operation: LongitudinalOperationV1,
    samples: &super::LongitudinalSamplePlanV1,
) -> Result<u16, LongitudinalEvidenceError> {
    use LongitudinalOperationV1 as Operation;
    let release = match operation {
        Operation::ColdHead => samples.process_cold_per_release_root,
        Operation::WarmHead
        | Operation::WinAdjacent
        | Operation::WinDeep
        | Operation::WinTail
        | Operation::AtDeep
        | Operation::SearchStructured
        | Operation::SearchBody
        | Operation::SearchMiss
        | Operation::FilterFacet
        | Operation::Revisions
        | Operation::Threads
        | Operation::AttentionAll
        | Operation::AttentionOne
        | Operation::DetailActive
        | Operation::SnapshotActive
        | Operation::DetailRemoved
        | Operation::FreshNoChange
        | Operation::NewCountZero => samples.warm_samples_per_release_root,
        Operation::AppendOne | Operation::PostOne => samples.append_one_samples_per_release_root,
        Operation::AppendBurst | Operation::PostBurst => {
            samples.append_burst_samples_per_release_root
        }
        Operation::Restart => samples.restart_samples_per_release_root,
        Operation::AuditExact
        | Operation::ExportExact
        | Operation::MigrateFresh
        | Operation::RebuildFull => samples.maintenance_samples_per_release_root,
    };
    match lane {
        LongitudinalLaneV1::ReleaseUninstrumented => Ok(release),
        LongitudinalLaneV1::DebugUninstrumented => match operation {
            Operation::ColdHead => Ok(samples.debug_cold_samples),
            Operation::WarmHead
            | Operation::WinAdjacent
            | Operation::WinDeep
            | Operation::WinTail
            | Operation::AtDeep
            | Operation::SearchStructured
            | Operation::SearchBody
            | Operation::SearchMiss
            | Operation::FilterFacet
            | Operation::Revisions
            | Operation::Threads
            | Operation::AttentionAll
            | Operation::AttentionOne
            | Operation::DetailActive
            | Operation::SnapshotActive
            | Operation::DetailRemoved
            | Operation::FreshNoChange
            | Operation::NewCountZero => Ok(samples.debug_warm_samples),
            _ => Ok(samples.debug_write_restart_maintenance_samples),
        },
        LongitudinalLaneV1::AttributionCounts => Err(LongitudinalEvidenceError::UnavailableLane),
    }
}

pub fn run_longitudinal_capacity_probe_v1(
    options: LongitudinalCapacityRunOptionsV1,
) -> Result<LongitudinalCapacityReceiptV1, LongitudinalEvidenceError> {
    if options.lane == LongitudinalLaneV1::AttributionCounts {
        return Err(LongitudinalEvidenceError::UnavailableLane);
    }
    options
        .manifest
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    options
        .receipt
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    options.probe_output.validate()?;
    let identity = options
        .manifest
        .execution
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    if options.receipt.subject != options.manifest.subject
        || options.receipt.manifest_sha256 != options.manifest.manifest_sha256
        || options.receipt.execution_identity_sha256 != identity
        || options.receipt.probe != options.probe_output.probe
        || options.receipt.output_count != options.probe_output.output_count
        || options.receipt.selected_bytes != options.probe_output.selected_bytes
        || options.receipt.semantic_receipt_sha256 != options.probe_output.semantic_receipt_sha256
        || !options
            .manifest
            .probe_schedule
            .contains(&options.receipt.probe)
    {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    if options.receipt.classification
        == super::LongitudinalCapacityClassificationV1::UnsupportedNoCurrentSurface
    {
        if options.receipt.metrics.is_some()
            || options.probe_output.diagnostics.as_slice() != ["unsupported_no_current_surface"]
        {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }
    } else if options.receipt.metrics.is_none() {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    Ok(options.receipt)
}

pub fn execute_longitudinal_capacity_probe_v1(
    root: &Path,
    manifest: &LongitudinalCapacityManifestV1,
    probe: LongitudinalCapacityProbeV1,
) -> Result<LongitudinalCapacityProbeOutputV1, LongitudinalEvidenceError> {
    manifest
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    execute_longitudinal_capacity_probe_with_selectors_v1(root, &manifest.selectors, probe)
}

pub fn validate_longitudinal_object_detail_response_v1(
    root: &Path,
    manifest: &LongitudinalCapacityManifestV1,
    response: &[u8],
) -> Result<LongitudinalCapacityProbeOutputV1, LongitudinalEvidenceError> {
    manifest
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    let selectors = &manifest.selectors;
    let artifact = read_bound_object_artifact(
        root,
        &ObjectId::new(selectors.object_detail_object_id.clone()),
        &selectors.object_detail_content_hash,
    )
    .map_err(|error| LongitudinalEvidenceError::Io(error.to_string()))?;
    if artifact.content_hash != selectors.object_detail_content_hash {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let value: serde_json::Value =
        serde_json::from_slice(response).map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    if value.get("schema").and_then(serde_json::Value::as_str) != Some("pointbreak.review-snapshot")
        || value.get("version").and_then(serde_json::Value::as_u64) != Some(1)
        || value.get("contentHash").and_then(serde_json::Value::as_str)
            != Some(selectors.object_detail_content_hash.as_str())
    {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let files = value
        .pointer("/snapshot/files")
        .and_then(serde_json::Value::as_array)
        .ok_or(LongitudinalEvidenceError::InvalidReceipt)?;
    let hunk_count = files
        .iter()
        .filter_map(|file| file.get("hunks").and_then(serde_json::Value::as_array))
        .map(Vec::len)
        .sum::<usize>();
    let row_count = files
        .iter()
        .filter_map(|file| file.get("hunks").and_then(serde_json::Value::as_array))
        .flatten()
        .filter_map(|hunk| hunk.get("rows").and_then(serde_json::Value::as_array))
        .map(Vec::len)
        .sum::<usize>();
    let expected_hunk_count = artifact
        .snapshot
        .files
        .iter()
        .map(|file| file.hunks.len())
        .sum::<usize>();
    let expected_row_count = artifact
        .snapshot
        .files
        .iter()
        .flat_map(|file| &file.hunks)
        .map(|hunk| hunk.rows.len())
        .sum::<usize>();
    if files.len() != artifact.snapshot.files.len()
        || hunk_count != expected_hunk_count
        || row_count != expected_row_count
    {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let response_sha256 = canonical_sha256(&value)?;
    let mut output = LongitudinalCapacityProbeOutputV1 {
        probe: LongitudinalCapacityProbeV1::ObjectDetail,
        output_count: 1,
        selected_bytes: response.len() as u64,
        ordered_ids: Vec::new(),
        fact_ids: vec![
            selectors.object_detail_object_id.clone(),
            selectors.object_detail_content_hash.clone(),
        ],
        diagnostics: vec![
            "inspector_snapshot_detail_route".to_owned(),
            format!("response_sha256:{response_sha256}"),
            format!("files:{};hunks:{hunk_count};rows:{row_count}", files.len()),
        ],
        semantic_receipt_sha256: String::new(),
    };
    output.semantic_receipt_sha256 = output.canonical_sha256()?;
    output.validate()?;
    Ok(output)
}

fn execute_longitudinal_capacity_probe_with_selectors_v1(
    root: &Path,
    selectors: &super::LongitudinalCapacitySelectorsV1,
    probe: LongitudinalCapacityProbeV1,
) -> Result<LongitudinalCapacityProbeOutputV1, LongitudinalEvidenceError> {
    let (output_count, selected_bytes, ordered_ids, fact_ids, diagnostics) = match probe {
        LongitudinalCapacityProbeV1::CarrierKey => {
            let hit = read_longitudinal_carrier_by_key_v1(root, &selectors.carrier_hit_key)
                .map_err(|error| LongitudinalEvidenceError::Io(error.to_string()))?
                .ok_or(LongitudinalEvidenceError::InvalidReceipt)?;
            if hit.event_id.as_str() != selectors.carrier_hit_event_id
                || read_longitudinal_carrier_by_key_v1(root, &selectors.carrier_miss_key)
                    .map_err(|error| LongitudinalEvidenceError::Io(error.to_string()))?
                    .is_some()
            {
                return Err(LongitudinalEvidenceError::InvalidReceipt);
            }
            let bytes = serde_json::to_vec(&hit).map_err(io_error)?;
            (
                1,
                bytes.len() as u64,
                vec![hit.event_id.as_str().to_owned()],
                Vec::new(),
                Vec::new(),
            )
        }
        LongitudinalCapacityProbeV1::SemanticId => {
            let index = store_id_index(root)
                .map_err(|error| LongitudinalEvidenceError::Io(error.to_string()))?;
            if !index.revisions.contains(&selectors.semantic_revision_id)
                || !index.objects.contains(&selectors.semantic_object_id)
                || index
                    .revisions
                    .contains(&selectors.semantic_missing_revision_id)
                || index
                    .objects
                    .contains(&selectors.semantic_missing_object_id)
            {
                return Err(LongitudinalEvidenceError::InvalidReceipt);
            }
            (
                2,
                (selectors.semantic_revision_id.len() + selectors.semantic_object_id.len()) as u64,
                Vec::new(),
                vec![
                    selectors.semantic_revision_id.clone(),
                    selectors.semantic_object_id.clone(),
                ],
                vec!["whole_history_semantic_index".to_owned()],
            )
        }
        LongitudinalCapacityProbeV1::ChronologicalHead
        | LongitudinalCapacityProbeV1::ChronologicalMiddle
        | LongitudinalCapacityProbeV1::ChronologicalTail => {
            let mut events = read_events(root)
                .map_err(|error| LongitudinalEvidenceError::Io(error.to_string()))?;
            events.sort_by(|left, right| {
                right
                    .occurred_at
                    .cmp(&left.occurred_at)
                    .then_with(|| right.event_id.cmp(&left.event_id))
            });
            let start = match probe {
                LongitudinalCapacityProbeV1::ChronologicalHead => {
                    selectors.chronological_head_start
                }
                LongitudinalCapacityProbeV1::ChronologicalMiddle => {
                    selectors.chronological_middle_start
                }
                LongitudinalCapacityProbeV1::ChronologicalTail => {
                    selectors.chronological_tail_start
                }
                _ => unreachable!(),
            } as usize;
            let end = start
                .checked_add(usize::from(selectors.chronological_window_size))
                .ok_or(LongitudinalEvidenceError::InvalidReceipt)?;
            let window = events
                .get(start..end)
                .ok_or(LongitudinalEvidenceError::InvalidReceipt)?;
            let bytes = serde_json::to_vec(window).map_err(io_error)?;
            (
                window.len() as u64,
                bytes.len() as u64,
                window
                    .iter()
                    .map(|event| event.event_id.as_str().to_owned())
                    .collect(),
                Vec::new(),
                vec!["whole_history_chronological_sort".to_owned()],
            )
        }
        LongitudinalCapacityProbeV1::ObjectDetail => {
            let object_id = ObjectId::new(selectors.object_detail_object_id.clone());
            let artifact =
                read_bound_object_artifact(root, &object_id, &selectors.object_detail_content_hash)
                    .map_err(|error| LongitudinalEvidenceError::Io(error.to_string()))?;
            if artifact.snapshot.object_id != object_id
                || artifact.content_hash != selectors.object_detail_content_hash
            {
                return Err(LongitudinalEvidenceError::InvalidReceipt);
            }
            let bytes = serde_json::to_vec(&artifact).map_err(io_error)?;
            (
                1,
                bytes.len() as u64,
                Vec::new(),
                vec![
                    selectors.object_detail_object_id.clone(),
                    selectors.object_detail_content_hash.clone(),
                ],
                vec!["production_bound_object_read".to_owned()],
            )
        }
        LongitudinalCapacityProbeV1::AppendDelta => (
            0,
            0,
            Vec::new(),
            Vec::new(),
            vec!["unsupported_no_current_surface".to_owned()],
        ),
    };
    let mut output = LongitudinalCapacityProbeOutputV1 {
        probe,
        output_count,
        selected_bytes,
        ordered_ids,
        fact_ids,
        diagnostics,
        semantic_receipt_sha256: String::new(),
    };
    output.semantic_receipt_sha256 = output.canonical_sha256()?;
    output.validate()?;
    Ok(output)
}

pub fn run_longitudinal_contention_writer_v1(
    root: &Path,
    manifest: &LongitudinalCapacityManifestV1,
    writer_index: u8,
) -> Result<LongitudinalContentionWriterReceiptV1, LongitudinalEvidenceError> {
    manifest
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    if manifest.subject
        != LongitudinalCapacitySubjectV1::Companion(LongitudinalCapacityProfileV1::L100O10K)
    {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let write = append_longitudinal_contention_writer_v1(root, writer_index)
        .map_err(|error| LongitudinalEvidenceError::Io(error.to_string()))?;
    let mut receipt = LongitudinalContentionWriterReceiptV1 {
        writer_index,
        manifest_sha256: manifest.manifest_sha256.clone(),
        attempt_event_ids: write.attempt_event_ids,
        outcomes: write.outcomes.into_iter().map(str::to_owned).collect(),
        created: u8::try_from(write.events_created)
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?,
        existing: u8::try_from(write.events_existing)
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?,
        final_event_count: write.final_event_count,
        event_set_sha256: write.event_set_sha256,
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = receipt.canonical_sha256()?;
    receipt.validate()?;
    Ok(receipt)
}

pub fn finalize_longitudinal_contention_v1(
    root: &Path,
    manifest: &LongitudinalCapacityManifestV1,
    mut writers: Vec<LongitudinalContentionWriterReceiptV1>,
    mut readers: Vec<LongitudinalContentionReaderCycleReceiptV1>,
    elapsed_nanos: u64,
) -> Result<super::LongitudinalContentionReceiptV1, LongitudinalEvidenceError> {
    manifest
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    writers.sort_by_key(|receipt| receipt.writer_index);
    readers.sort_by_key(|receipt| receipt.cycle_ordinal);
    if writers.len() != 2
        || readers.len() != 20
        || writers.iter().any(|receipt| {
            receipt.validate().is_err() || receipt.manifest_sha256 != manifest.manifest_sha256
        })
        || readers.iter().enumerate().any(|(ordinal, receipt)| {
            receipt.validate().is_err()
                || usize::from(receipt.cycle_ordinal) != ordinal
                || receipt.observed_event_count < manifest.event_count
                || receipt.observed_event_count > manifest.event_count + 10
        })
    {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let created = writers.iter().map(|receipt| receipt.created).sum::<u8>();
    let existing = writers.iter().map(|receipt| receipt.existing).sum::<u8>();
    let attempts = writers
        .iter()
        .flat_map(|receipt| receipt.attempt_event_ids.iter())
        .collect::<Vec<_>>();
    if attempts.len() != 12 || attempts.iter().collect::<BTreeSet<_>>().len() != 10 {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let events =
        read_events(root).map_err(|error| LongitudinalEvidenceError::Io(error.to_string()))?;
    if events.len() as u64 != manifest.event_count + 10 {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let state = SessionState::from_events(&events)
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    let strict_replay_sha256 =
        canonical_sha256(&(event_set_sha256(&events)?, state, attempts, readers))?;
    let guard_nanos = longitudinal_capacity_contract_v1()
        .contention
        .wall_guard_seconds
        .saturating_mul(1_000_000_000);
    let mut receipt = super::LongitudinalContentionReceiptV1 {
        schema: "pointbreak.longitudinal-contention-receipt.v1".to_owned(),
        manifest_sha256: manifest.manifest_sha256.clone(),
        writer_attempts: 12,
        created,
        existing,
        conflicts: 0,
        reader_cycles: 20,
        final_event_count: events.len() as u64,
        strict_replay_sha256,
        timed_out: elapsed_nanos > guard_nanos,
        semantic_failure: false,
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = receipt
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    receipt
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    Ok(receipt)
}

pub fn evaluate_longitudinal_c524_gate_v1(
    inputs: &LongitudinalC524GateInputsV1,
) -> Result<LongitudinalC524GateReceiptV1, LongitudinalEvidenceError> {
    let (admitted, reasons) = super::contract::evaluate_c524_gate(inputs)
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    let mut receipt = LongitudinalC524GateReceiptV1 {
        schema: "pointbreak.longitudinal-c524-gate-receipt.v1".to_owned(),
        inputs_sha256: canonical_sha256(inputs)?,
        admitted,
        reasons,
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = receipt
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    receipt
        .validate_against(inputs)
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    Ok(receipt)
}

pub fn verify_longitudinal_evidence_package_v1(
    raw_root: &Path,
) -> Result<LongitudinalEvidencePackageV1, LongitudinalEvidenceError> {
    let package: LongitudinalEvidencePackageV1 =
        read_json(&raw_root.join(LONGITUDINAL_EVIDENCE_PACKAGE_FILE_V1))?;
    package
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    verify_optional_package_receipt(raw_root, &package)?;
    verify_raw_inventory(
        raw_root,
        &package.raw_inventory,
        LONGITUDINAL_EVIDENCE_PACKAGE_FILE_V1,
    )?;
    Ok(package)
}

pub fn verify_longitudinal_capacity_package_v1(
    raw_root: &Path,
) -> Result<LongitudinalCapacityPackageV1, LongitudinalEvidenceError> {
    let package: LongitudinalCapacityPackageV1 =
        read_json(&raw_root.join(LONGITUDINAL_CAPACITY_PACKAGE_FILE_V1))?;
    package
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    verify_optional_package_receipt(raw_root, &package)?;
    verify_raw_inventory(
        raw_root,
        &package.raw_inventory,
        LONGITUDINAL_CAPACITY_PACKAGE_FILE_V1,
    )?;
    Ok(package)
}

pub fn longitudinal_non_timing_smoke_v1()
-> Result<LongitudinalSmokeReceiptV1, LongitudinalEvidenceError> {
    let contract = longitudinal_runner_contract_v1();
    contract
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    longitudinal_capacity_contract_v1()
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;

    let smoke_root = std::env::temp_dir().join(format!(
        "pointbreak-longitudinal-smoke-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io_error)?
            .as_nanos()
    ));
    fs::create_dir(&smoke_root).map_err(io_error)?;
    let result = (|| {
        let left = smoke_root.join("left");
        let right = smoke_root.join("right");
        initialize_evidence_root(&left)?;
        initialize_evidence_root(&right)?;
        let execution = smoke_execution_identity();
        let left_receipt =
            materialize_longitudinal_workload_v1(super::LongitudinalMaterializeOptionsV1::new(
                &left,
                LongitudinalTierV1::L1,
                execution.clone(),
            ))
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
        let right_receipt = materialize_longitudinal_workload_v1(
            super::LongitudinalMaterializeOptionsV1::new(&right, LongitudinalTierV1::L1, execution),
        )
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
        super::verify_longitudinal_materialization_pair_v1(&left_receipt, &right_receipt)
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
        if preflight_longitudinal_root_v1(&left, &left_receipt.manifest)? != left_receipt
            || preflight_longitudinal_root_v1(&right, &right_receipt.manifest)? != right_receipt
        {
            return Err(LongitudinalEvidenceError::Preflight);
        }

        let package_root = smoke_root.join("package");
        fs::create_dir(&package_root).map_err(io_error)?;
        let raw = b"public longitudinal package smoke fixture\n";
        fs::write(package_root.join("raw.json"), raw).map_err(io_error)?;
        let raw_inventory = vec![super::LongitudinalRawFileV1 {
            relative_path: "raw.json".to_owned(),
            sha256: sha256_bytes_hex(raw),
            bytes: raw.len() as u64,
        }];
        verify_raw_inventory(
            &package_root,
            &raw_inventory,
            LONGITUDINAL_EVIDENCE_PACKAGE_FILE_V1,
        )?;
        let mut package = LongitudinalEvidencePackageV1 {
            schema: super::LONGITUDINAL_EVIDENCE_PACKAGE_SCHEMA_V1.to_owned(),
            purpose: super::LongitudinalPackagePurposeV1::NonTimingSmoke,
            contract_sha256: contract.contract_sha256,
            base_execution: left_receipt.manifest.execution.clone(),
            derivative_execution: None,
            materializations: vec![left_receipt],
            operations: Vec::new(),
            memory_receipts: Vec::new(),
            interruption_receipts: Vec::new(),
            counters: Vec::new(),
            raw_inventory,
            failures: Vec::new(),
            compensation_applied: false,
            package_sha256: String::new(),
        };
        package.package_sha256 = package
            .canonical_sha256()
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
        fs::write(
            package_root.join(LONGITUDINAL_EVIDENCE_PACKAGE_FILE_V1),
            serde_json::to_vec_pretty(&package).map_err(io_error)?,
        )
        .map_err(io_error)?;
        if verify_longitudinal_evidence_package_v1(&package_root)? != package {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }
        fs::write(
            package_root.join(LONGITUDINAL_PACKAGE_VERIFICATION_RECEIPT_FILE_V1),
            serde_json::to_vec_pretty(&package).map_err(io_error)?,
        )
        .map_err(io_error)?;
        if verify_longitudinal_evidence_package_v1(&package_root)? != package {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }

        Ok(LongitudinalSmokeReceiptV1 {
            schema: "pointbreak.longitudinal-non-timing-smoke.v1".to_owned(),
            timing_admissible: false,
            terminal_evidence_admissible: false,
            pair_verified: true,
            preflight_verified: true,
            package_mechanics_verified: true,
        })
    })();
    let cleanup = fs::remove_dir_all(&smoke_root);
    if result.is_ok() {
        cleanup.map_err(io_error)?;
    }
    result
}

fn smoke_execution_identity() -> LongitudinalExecutionIdentityV1 {
    LongitudinalExecutionIdentityV1 {
        source_commit: "0".repeat(40),
        source_tree: "1".repeat(40),
        cargo_lock_sha256: "2".repeat(64),
        runner_sha256: "3".repeat(64),
        build_profile: "non-timing-smoke".to_owned(),
        operating_system: std::env::consts::OS.to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
        filesystem: "disposable".to_owned(),
        parent_commit: None,
    }
}

fn validate_materialization_boundary(
    root: &Path,
    source_root: &Path,
    runner: &Path,
    execution: &LongitudinalExecutionIdentityV1,
) -> Result<(), LongitudinalEvidenceError> {
    execution
        .validate()
        .map_err(|_| LongitudinalEvidenceError::SourceIdentity)?;
    validate_fresh_local_root(root, source_root)?;
    if PROTECTED_ENVIRONMENT_VARIABLES
        .iter()
        .any(|name| std::env::var_os(name).is_some())
    {
        return Err(LongitudinalEvidenceError::ProtectedEnvironment);
    }
    let source_root =
        fs::canonicalize(source_root).map_err(|_| LongitudinalEvidenceError::SourceIdentity)?;
    let runner = fs::canonicalize(runner).map_err(|_| LongitudinalEvidenceError::RunnerIdentity)?;
    if !runner.is_file()
        || sha256_file(&runner)? != execution.runner_sha256
        || git_output(&source_root, &["rev-parse", "HEAD"])? != execution.source_commit
        || git_output(&source_root, &["rev-parse", "HEAD^{tree}"])? != execution.source_tree
        || !git_output(&source_root, &["status", "--porcelain=v1"])?.is_empty()
        || sha256_file(&source_root.join("Cargo.lock"))? != execution.cargo_lock_sha256
        || execution.operating_system != std::env::consts::OS
        || execution.architecture != std::env::consts::ARCH
    {
        return Err(LongitudinalEvidenceError::SourceIdentity);
    }
    let parent = root.parent().ok_or(LongitudinalEvidenceError::UnsafeRoot)?;
    let parent = fs::canonicalize(parent).map_err(|_| LongitudinalEvidenceError::UnsafeRoot)?;
    let filesystem = qualification_filesystem_name(&parent);
    if execution.filesystem != filesystem {
        return Err(LongitudinalEvidenceError::SourceIdentity);
    }
    Ok(())
}

fn validate_fresh_local_root(
    root: &Path,
    source_root: &Path,
) -> Result<(), LongitudinalEvidenceError> {
    if !root.is_absolute() || root.exists() {
        return Err(LongitudinalEvidenceError::UnsafeRoot);
    }
    if root
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(LongitudinalEvidenceError::UnsafeRoot);
    }
    let parent = root.parent().ok_or(LongitudinalEvidenceError::UnsafeRoot)?;
    let parent = fs::canonicalize(parent).map_err(|_| LongitudinalEvidenceError::UnsafeRoot)?;
    let source_root =
        fs::canonicalize(source_root).map_err(|_| LongitudinalEvidenceError::SourceIdentity)?;
    let candidate = parent.join(
        root.file_name()
            .ok_or(LongitudinalEvidenceError::UnsafeRoot)?,
    );
    if candidate.starts_with(&source_root)
        || root
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .any(|component| {
                matches!(
                    component.to_ascii_lowercase().as_str(),
                    ".pointbreak"
                        | ".gumbo"
                        | ".git"
                        | "dropbox"
                        | "onedrive"
                        | "icloud drive"
                        | "google drive"
                        | "syncthing"
                )
            })
    {
        return Err(LongitudinalEvidenceError::ProtectedRoot);
    }
    let filesystem = qualification_filesystem_name(&parent);
    if filesystem == "unavailable"
        || classify_qualification_filesystem(&filesystem)
            != QualificationFilesystemDispositionV1::LocalProofEligible
    {
        return Err(LongitudinalEvidenceError::UnsafeRoot);
    }
    Ok(())
}

fn initialize_evidence_root(root: &Path) -> Result<(), LongitudinalEvidenceError> {
    fs::create_dir(root).map_err(io_error)?;
    let output = Command::new("git")
        .args(["init", "--quiet"])
        .arg(root)
        .output()
        .map_err(io_error)?;
    if !output.status.success() {
        return Err(LongitudinalEvidenceError::Io(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    set_store_mode_for_repo(root, StoreMode::Ephemeral).map_err(|error| {
        LongitudinalEvidenceError::Io(format!("set disposable store mode: {error}"))
    })
}

fn strict_preflight(
    root: &Path,
    ordered_events: &[super::LongitudinalEventIdentityV1],
    event_carriers: &[super::LongitudinalEventCarrierV1],
    content_inventory: &[super::LongitudinalInventoryEntryV1],
) -> Result<super::LongitudinalStrictSemanticReceiptV1, LongitudinalEvidenceError> {
    let root = fs::canonicalize(root).map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let store_dir = store_dir_for_repo(&root).map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let events = read_events(&root).map_err(|_| LongitudinalEvidenceError::Preflight)?;
    if events.len() != ordered_events.len() || event_carriers.len() != ordered_events.len() {
        return Err(LongitudinalEvidenceError::Preflight);
    }

    let by_id = events
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect::<BTreeMap<_, _>>();
    for (expected, carrier) in ordered_events.iter().zip(event_carriers) {
        if expected.event_id != carrier.event_id {
            return Err(LongitudinalEvidenceError::Preflight);
        }
        let event = by_id
            .get(expected.event_id.as_str())
            .ok_or(LongitudinalEvidenceError::Preflight)?;
        let decoded = canonical_json_bytes(
            &serde_json::to_value(event).map_err(|_| LongitudinalEvidenceError::Preflight)?,
        )
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
        let raw = fs::read(
            store_dir
                .join("events")
                .join(format!("{}.json", carrier.logical_key_sha256)),
        )
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
        if sha256_bytes_hex(&decoded) != expected.canonical_decoded_sha256
            || raw.len() as u64 != carrier.raw_bytes
            || sha256_bytes_hex(&raw) != carrier.raw_sha256
        {
            return Err(LongitudinalEvidenceError::Preflight);
        }
    }

    let expected_content = content_inventory
        .iter()
        .map(|entry| entry.logical_key.as_str())
        .collect::<BTreeSet<_>>();
    let actual_content = list_relative_files(&store_dir.join("artifacts"), &store_dir)?;
    if actual_content
        != expected_content
            .iter()
            .map(|path| (*path).to_owned())
            .collect::<BTreeSet<_>>()
    {
        return Err(LongitudinalEvidenceError::Preflight);
    }
    for entry in content_inventory {
        let raw = fs::read(store_dir.join(&entry.logical_key))
            .map_err(|_| LongitudinalEvidenceError::Preflight)?;
        if raw.len() as u64 != entry.raw_bytes || sha256_bytes_hex(&raw) != entry.raw_sha256 {
            return Err(LongitudinalEvidenceError::Preflight);
        }
    }

    let stored_state: SessionState = read_json(&store_dir.join("state.json"))?;
    let rebuilt_state =
        SessionState::from_events(&events).map_err(|_| LongitudinalEvidenceError::Preflight)?;
    if stored_state != rebuilt_state {
        return Err(LongitudinalEvidenceError::Preflight);
    }
    let event_set_sha256 = event_set_sha256(&events)?;
    let ordered_journal_sha256 = canonical_sha256(
        &events
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<Vec<_>>(),
    )?;
    let state_sha256 = canonical_sha256(&stored_state)?;
    let projection_sha256 = canonical_sha256(&serde_json::json!({
        "journalId": &stored_state.journal_id,
        "currentRevisionId": &stored_state.current_revision_id,
        "currentObjectId": &stored_state.current_object_id,
        "revisionCount": stored_state.revision_count,
        "eventCount": stored_state.event_count,
        "observationCount": stored_state.observation_count,
        "assessmentCount": stored_state.assessment_count,
        "validationCheckCount": stored_state.validation_check_count,
        "inputRequestCount": stored_state.input_request_count,
        "openInputRequestCount": stored_state.open_input_request_count,
        "openOperativeInputRequestCount": stored_state.open_operative_input_request_count,
        "diagnostics": &stored_state.diagnostics,
    }))?;
    let content_inventory_sha256 = canonical_sha256(&content_inventory)?;
    Ok(super::LongitudinalStrictSemanticReceiptV1 {
        event_set_sha256,
        ordered_journal_sha256,
        state_sha256,
        projection_sha256,
        content_inventory_sha256,
    })
}

fn capacity_receipt_from_v1_l100(
    receipt: LongitudinalMaterializationReceiptV1,
) -> Result<LongitudinalCapacityMaterializationReceiptV1, LongitudinalEvidenceError> {
    let v1 = longitudinal_runner_contract_v1()
        .tiers
        .into_iter()
        .find(|requirement| requirement.tier == LongitudinalTierV1::L100)
        .ok_or(LongitudinalEvidenceError::InvalidReceipt)?;
    let probes = longitudinal_capacity_contract_v1().probes;
    let mut manifest = LongitudinalCapacityManifestV1 {
        schema: super::LONGITUDINAL_CAPACITY_SCHEMA_V1.to_owned(),
        contract_sha256: super::LONGITUDINAL_CAPACITY_CONTRACT_SHA256_V1.to_owned(),
        execution: receipt.manifest.execution,
        public_seed_hex: receipt.manifest.public_seed_hex,
        subject: LongitudinalCapacitySubjectV1::V1L100,
        event_count: receipt.manifest.event_count,
        revision_count: receipt.manifest.revision_count,
        object_artifact_count: v1.object_artifact_count,
        task_attempt_count: v1.task_attempt_count,
        body_fact_count: v1.body_fact_count,
        external_body_count: v1.external_body_count,
        decoded_body_bytes: v1.decoded_body_bytes,
        decoded_object_target_bytes: v1.decoded_object_target_bytes,
        ordered_events: receipt.manifest.ordered_events,
        event_carriers: receipt.manifest.event_carriers,
        content_inventory: receipt.manifest.content_inventory,
        removed_content_sha256: receipt.manifest.removed_content_sha256,
        selectors: receipt.manifest.capacity_selectors,
        schedule_sha256: canonical_sha256(&probes)?,
        probe_schedule: probes,
        manifest_sha256: String::new(),
    };
    manifest.manifest_sha256 = manifest
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    let mut capacity = LongitudinalCapacityMaterializationReceiptV1 {
        schema: LONGITUDINAL_CAPACITY_MATERIALIZATION_RECEIPT_SCHEMA_V1.to_owned(),
        root_identity: receipt.root_identity,
        manifest,
        strict: receipt.strict,
        materialization_sha256: String::new(),
    };
    capacity.materialization_sha256 = capacity
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    capacity
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    Ok(capacity)
}

fn verify_raw_inventory(
    root: &Path,
    inventory: &[super::LongitudinalRawFileV1],
    package_name: &str,
) -> Result<(), LongitudinalEvidenceError> {
    let root = fs::canonicalize(root).map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    let expected = inventory
        .iter()
        .map(|entry| entry.relative_path.clone())
        .collect::<BTreeSet<_>>();
    let mut actual = list_relative_files(&root, &root)?;
    actual.remove(package_name);
    actual.remove(LONGITUDINAL_PACKAGE_VERIFICATION_RECEIPT_FILE_V1);
    if actual != expected {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    for entry in inventory {
        let path = root.join(&entry.relative_path);
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
        if !metadata.file_type().is_file()
            || metadata.len() != entry.bytes
            || sha256_file(&path)? != entry.sha256
        {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }
    }
    Ok(())
}

fn verify_optional_package_receipt<T>(
    root: &Path,
    package: &T,
) -> Result<(), LongitudinalEvidenceError>
where
    T: for<'de> Deserialize<'de> + PartialEq,
{
    let receipt = root.join(LONGITUDINAL_PACKAGE_VERIFICATION_RECEIPT_FILE_V1);
    if receipt.exists() && read_json::<T>(&receipt)? != *package {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    Ok(())
}

fn list_relative_files(
    root: &Path,
    relative_to: &Path,
) -> Result<BTreeSet<String>, LongitudinalEvidenceError> {
    let mut result = BTreeSet::new();
    if !root.exists() {
        return Ok(result);
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(io_error)?;
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(relative_to)
                    .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?
                    .to_str()
                    .ok_or(LongitudinalEvidenceError::InvalidReceipt)?
                    .replace('\\', "/");
                result.insert(relative);
            } else {
                return Err(LongitudinalEvidenceError::InvalidReceipt);
            }
        }
    }
    Ok(result)
}

fn event_set_sha256(
    events: &[crate::session::event::ShoreEvent],
) -> Result<String, LongitudinalEvidenceError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Entry<'a> {
        event_id: &'a str,
        payload_hash: &'a str,
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Material<'a> {
        schema: &'static str,
        events: Vec<Entry<'a>>,
    }
    let mut entries = events
        .iter()
        .map(|event| Entry {
            event_id: event.event_id.as_str(),
            payload_hash: &event.payload_hash,
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| (entry.event_id, entry.payload_hash));
    canonical_sha256(&Material {
        schema: "shore.event-set.v1",
        events: entries,
    })
}

fn root_identity(root: &Path) -> Result<String, LongitudinalEvidenceError> {
    let root = fs::canonicalize(root).map_err(|_| LongitudinalEvidenceError::Preflight)?;
    Ok(sha256_bytes_hex(root.as_os_str().as_encoded_bytes()))
}

fn canonical_sha256(value: &impl Serialize) -> Result<String, LongitudinalEvidenceError> {
    let value =
        serde_json::to_value(value).map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    let bytes =
        canonical_json_bytes(&value).map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    Ok(sha256_bytes_hex(&bytes))
}

fn sha256_file(path: &Path) -> Result<String, LongitudinalEvidenceError> {
    fs::read(path)
        .map(|bytes| sha256_bytes_hex(&bytes))
        .map_err(io_error)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, LongitudinalEvidenceError> {
    let bytes = fs::read(path).map_err(io_error)?;
    serde_json::from_slice(&bytes).map_err(|_| LongitudinalEvidenceError::InvalidReceipt)
}

fn git_output(root: &Path, arguments: &[&str]) -> Result<String, LongitudinalEvidenceError> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .map_err(io_error)?;
    if !output.status.success() {
        return Err(LongitudinalEvidenceError::SourceIdentity);
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|_| LongitudinalEvidenceError::SourceIdentity)
}

fn io_error(error: impl std::fmt::Display) -> LongitudinalEvidenceError {
    LongitudinalEvidenceError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::super::{
        LONGITUDINAL_EVIDENCE_PACKAGE_SCHEMA_V1, LongitudinalCapacitySelectorsV1,
        LongitudinalContractError, LongitudinalPackagePurposeV1, LongitudinalRawFileV1,
    };
    use super::*;

    #[test]
    fn longitudinal_evidence_rejects_existing_and_relative_roots() {
        let source = crate::bench_support::manifest_dir();
        assert_eq!(
            validate_fresh_local_root(&source, &source)
                .unwrap_err()
                .to_string(),
            LongitudinalEvidenceError::UnsafeRoot.to_string()
        );
        assert_eq!(
            validate_fresh_local_root(Path::new("relative"), &source)
                .unwrap_err()
                .to_string(),
            LongitudinalEvidenceError::UnsafeRoot.to_string()
        );
    }

    #[test]
    fn longitudinal_evidence_rejects_attribution_until_the_derivative_exists() {
        let contract = longitudinal_runner_contract_v1();
        let manifest = LongitudinalWorkloadManifestV1 {
            schema: String::new(),
            protocol: String::new(),
            contract_sha256: String::new(),
            execution: LongitudinalExecutionIdentityV1 {
                source_commit: String::new(),
                source_tree: String::new(),
                cargo_lock_sha256: String::new(),
                runner_sha256: String::new(),
                build_profile: String::new(),
                operating_system: String::new(),
                architecture: String::new(),
                filesystem: String::new(),
                parent_commit: None,
            },
            public_seed_hex: String::new(),
            tier: LongitudinalTierV1::L1,
            event_count: 0,
            revision_count: 0,
            by_type: Vec::new(),
            ordered_events: Vec::new(),
            event_carriers: Vec::new(),
            content_inventory: Vec::new(),
            removed_content_sha256: Vec::new(),
            capacity_selectors: LongitudinalCapacitySelectorsV1 {
                carrier_hit_key: String::new(),
                carrier_hit_event_id: String::new(),
                carrier_miss_key: String::new(),
                semantic_revision_id: String::new(),
                semantic_object_id: String::new(),
                semantic_missing_revision_id: String::new(),
                semantic_missing_object_id: String::new(),
                object_detail_object_id: String::new(),
                object_detail_content_hash: String::new(),
                chronological_window_size: 0,
                chronological_head_start: 0,
                chronological_middle_start: 0,
                chronological_tail_start: 0,
                selectors_sha256: String::new(),
            },
            expected_semantic_receipts: Vec::new(),
            schedule: contract.operation_schedule,
            schedule_sha256: String::new(),
            manifest_sha256: String::new(),
        };
        assert!(matches!(
            run_longitudinal_lane_v1(LongitudinalRunOptionsV1 {
                root: PathBuf::from("/tmp/does-not-matter"),
                manifest,
                lane: LongitudinalLaneV1::AttributionCounts,
                operations: Vec::new(),
            }),
            Err(LongitudinalEvidenceError::UnavailableLane)
        ));
    }

    #[test]
    fn longitudinal_evidence_smoke_is_never_terminal_evidence() {
        longitudinal_runner_contract_v1().validate().unwrap();
        let fields = LongitudinalSmokeReceiptV1 {
            schema: "pointbreak.longitudinal-non-timing-smoke.v1".to_owned(),
            timing_admissible: false,
            terminal_evidence_admissible: false,
            pair_verified: true,
            preflight_verified: true,
            package_mechanics_verified: true,
        };
        assert!(!fields.timing_admissible);
        assert!(!fields.terminal_evidence_admissible);
        assert!(fields.pair_verified);
    }

    #[test]
    fn longitudinal_evidence_preflight_detects_extra_and_tampered_carriers() {
        let disposable = tempfile::tempdir().unwrap();
        let root = disposable.path().join("root");
        initialize_evidence_root(&root).unwrap();
        let materialized = materialize_longitudinal_workload_v1(
            super::super::LongitudinalMaterializeOptionsV1::new(
                &root,
                LongitudinalTierV1::L1,
                smoke_execution_identity(),
            ),
        )
        .unwrap();
        assert_eq!(
            preflight_longitudinal_root_v1(&root, &materialized.manifest).unwrap(),
            materialized
        );

        let store = store_dir_for_repo(&root).unwrap();
        let extra = store.join("artifacts/objects/uninventoried");
        fs::write(&extra, b"extra").unwrap();
        assert!(matches!(
            preflight_longitudinal_root_v1(&root, &materialized.manifest),
            Err(LongitudinalEvidenceError::Preflight)
        ));
        fs::remove_file(extra).unwrap();

        let carrier = &materialized.manifest.event_carriers[0];
        fs::write(
            store
                .join("events")
                .join(format!("{}.json", carrier.logical_key_sha256)),
            b"{}",
        )
        .unwrap();
        assert!(matches!(
            preflight_longitudinal_root_v1(&root, &materialized.manifest),
            Err(LongitudinalEvidenceError::Preflight)
        ));
    }

    #[test]
    fn longitudinal_evidence_append_schedule_is_sequential_and_exact() {
        let disposable = tempfile::tempdir().unwrap();
        let root = disposable.path().join("root");
        initialize_evidence_root(&root).unwrap();
        materialize_longitudinal_workload_v1(super::super::LongitudinalMaterializeOptionsV1::new(
            &root,
            LongitudinalTierV1::L1,
            smoke_execution_identity(),
        ))
        .unwrap();
        prepare_longitudinal_append_records_v1(&root, LongitudinalTierV1::L1).unwrap();
        let one = append_longitudinal_events_v1(&root, LongitudinalTierV1::L1, 0, 1).unwrap();
        assert_eq!(one.appended_events, 1);
        assert_eq!(one.pre_event_count, 1_024);
        assert_eq!(one.post_event_count, 1_025);
        let burst = append_longitudinal_events_v1(&root, LongitudinalTierV1::L1, 1, 30).unwrap();
        assert_eq!(burst.appended_events, 30);
        assert_eq!(burst.pre_event_count, 1_025);
        assert_eq!(burst.post_event_count, 1_055);
        assert!(matches!(
            append_longitudinal_events_v1(&root, LongitudinalTierV1::L1, 0, 1),
            Err(LongitudinalEvidenceError::Preflight)
        ));
    }

    #[test]
    fn longitudinal_evidence_capacity_probes_execute_against_production_surfaces() {
        let disposable = tempfile::tempdir().unwrap();
        let root = disposable.path().join("root");
        initialize_evidence_root(&root).unwrap();
        let materialized = materialize_longitudinal_workload_v1(
            super::super::LongitudinalMaterializeOptionsV1::new(
                &root,
                LongitudinalTierV1::L1,
                smoke_execution_identity(),
            ),
        )
        .unwrap();

        let outputs = LongitudinalCapacityProbeV1::ALL
            .into_iter()
            .map(|probe| {
                execute_longitudinal_capacity_probe_with_selectors_v1(
                    &root,
                    &materialized.manifest.capacity_selectors,
                    probe,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();

        assert_eq!(outputs.len(), LongitudinalCapacityProbeV1::ALL.len());
        assert!(outputs.iter().all(|output| output.validate().is_ok()));
        assert_eq!(outputs[0].output_count, 1);
        assert!(
            outputs[1]
                .diagnostics
                .contains(&"whole_history_semantic_index".to_owned())
        );
        assert!(outputs[2..=4].iter().all(|output| {
            output.output_count == 100
                && output
                    .diagnostics
                    .contains(&"whole_history_chronological_sort".to_owned())
        }));
        assert_eq!(outputs[6].diagnostics, ["unsupported_no_current_surface"]);
    }

    #[test]
    fn longitudinal_evidence_terminal_package_rejects_partial_rows() {
        let disposable = tempfile::tempdir().unwrap();
        let root = disposable.path().join("root");
        initialize_evidence_root(&root).unwrap();
        let materialization = materialize_longitudinal_workload_v1(
            super::super::LongitudinalMaterializeOptionsV1::new(
                &root,
                LongitudinalTierV1::L1,
                smoke_execution_identity(),
            ),
        )
        .unwrap();
        let mut package = LongitudinalEvidencePackageV1 {
            schema: LONGITUDINAL_EVIDENCE_PACKAGE_SCHEMA_V1.to_owned(),
            purpose: LongitudinalPackagePurposeV1::TerminalEvidence,
            contract_sha256: longitudinal_runner_contract_v1().contract_sha256,
            base_execution: materialization.manifest.execution.clone(),
            derivative_execution: None,
            materializations: vec![materialization],
            operations: Vec::new(),
            memory_receipts: Vec::new(),
            interruption_receipts: Vec::new(),
            counters: Vec::new(),
            raw_inventory: vec![LongitudinalRawFileV1 {
                relative_path: "raw.json".to_owned(),
                sha256: "a".repeat(64),
                bytes: 1,
            }],
            failures: Vec::new(),
            compensation_applied: false,
            package_sha256: String::new(),
        };
        package.package_sha256 = package.canonical_sha256().unwrap();

        assert!(matches!(
            package.validate(),
            Err(LongitudinalContractError::IncompleteEvidence)
        ));
        package.purpose = LongitudinalPackagePurposeV1::NonTimingSmoke;
        package.package_sha256 = package.canonical_sha256().unwrap();
        package.validate().unwrap();
    }
}
