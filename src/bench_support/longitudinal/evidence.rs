use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    LONGITUDINAL_CAPACITY_MATERIALIZATION_RECEIPT_SCHEMA_V1,
    LONGITUDINAL_CARRY_FORWARD_RECEIPT_SCHEMA_V1, LONGITUDINAL_CARRY_FORWARD_REQUEST_SCHEMA_V1,
    LONGITUDINAL_CONTROLLER_FAILURE_RECEIPT_SCHEMA_V1,
    LONGITUDINAL_MATERIALIZATION_RECEIPT_SCHEMA_V1, LongitudinalC524GateInputsV1,
    LongitudinalC524GateReceiptV1, LongitudinalCapacityManifestV1,
    LongitudinalCapacityMaterializationReceiptV1, LongitudinalCapacityPackageV1,
    LongitudinalCapacityProbeV1, LongitudinalCapacityProfileV1, LongitudinalCapacityReceiptV1,
    LongitudinalCapacitySubjectV1, LongitudinalCarryForwardAuthorityCompletionV1,
    LongitudinalCarryForwardAuthorityPackageV1, LongitudinalCarryForwardReceiptV1,
    LongitudinalCarryForwardSlotV1, LongitudinalControllerFailureReceiptV1,
    LongitudinalEvidencePackageV1, LongitudinalExecutionIdentityV1,
    LongitudinalFailureOperationSelectorV1, LongitudinalHttpBodyClassificationV1,
    LongitudinalHttpFailureV1, LongitudinalInspectorExitV1, LongitudinalLaneV1,
    LongitudinalMaterializationReceiptV1, LongitudinalMaterializerEquivalenceReceiptV1,
    LongitudinalOperationReceiptV1, LongitudinalOperationV1,
    LongitudinalPackageVerificationReceiptV1, LongitudinalRemovalUpgradeAuthorityCompletionV1,
    LongitudinalRemovalUpgradeAuthorityPackageV1, LongitudinalRemovalUpgradeReceiptV1,
    LongitudinalStderrClassificationV1, LongitudinalStderrFailureV1, LongitudinalTierV1,
    LongitudinalVerifiedPackageKindV1, LongitudinalWorkloadManifestV1,
    apply_longitudinal_removal_upgrade_v1, longitudinal_capacity_contract_v1,
    longitudinal_runner_contract_v1, longitudinal_store_data_inventory_v1,
    longitudinal_workload_manifest_carry_invariant_sha256_v1,
    longitudinal_workload_manifest_upgrade_invariant_sha256_v1,
    materialize_longitudinal_capacity_v1, materialize_longitudinal_workload_v1,
    verify_longitudinal_materialization_pair_v1, verify_longitudinal_materializer_equivalence_v1,
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
    SessionState, StoreMode, allowed_signers_path_for_repo, event_log_head_marker,
    read_bound_object_artifact, read_events, set_store_mode_for_repo, store_dir_for_repo,
    store_id_index,
};

pub const LONGITUDINAL_EVIDENCE_PACKAGE_FILE_V1: &str = "longitudinal-evidence-package.json";
pub const LONGITUDINAL_CAPACITY_PACKAGE_FILE_V1: &str = "longitudinal-capacity-package.json";
pub const LONGITUDINAL_CARRIED_MATERIALIZATION_FILE_V1: &str = "carried-materialization.json";
pub const LONGITUDINAL_CARRY_FORWARD_RECEIPT_FILE_V1: &str = "carry-forward-receipt.json";
pub const LONGITUDINAL_CARRY_FORWARD_ROOT_AUTHORITY_FILE_V1: &str =
    "carry-forward-root-authority.json";
pub const LONGITUDINAL_CARRY_FORWARD_AUTHORITY_PACKAGE_FILE_V1: &str =
    "carry-forward-authority-package.json";
pub const LONGITUDINAL_REMOVAL_UPGRADE_RECEIPT_FILE_V1: &str = "removal-upgrade-receipt.json";
pub const LONGITUDINAL_CORRECTED_MATERIALIZATION_FILE_V1: &str = "corrected-materialization.json";
pub const LONGITUDINAL_REMOVAL_UPGRADE_ROOT_AUTHORITY_FILE_V1: &str =
    "removal-upgrade-root-authority.json";
pub const LONGITUDINAL_REMOVAL_UPGRADE_AUTHORITY_PACKAGE_FILE_V1: &str =
    "removal-upgrade-authority-package.json";
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongitudinalCarryForwardOptionsV1 {
    pub source_root: PathBuf,
    pub clone_root: PathBuf,
    pub source_materialization: LongitudinalMaterializationReceiptV1,
    pub corrected_execution: LongitudinalExecutionIdentityV1,
    pub slot: LongitudinalCarryForwardSlotV1,
    pub materializer_equivalence: LongitudinalMaterializerEquivalenceReceiptV1,
    pub final_authority_diff_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongitudinalRemovalUpgradeOptionsV1 {
    pub source_root: PathBuf,
    pub successor_root: PathBuf,
    pub source_materialization: LongitudinalMaterializationReceiptV1,
    pub corrected_execution: LongitudinalExecutionIdentityV1,
    pub slot: LongitudinalCarryForwardSlotV1,
    pub final_authority_diff_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongitudinalRemovalUpgradeArtifactsV1 {
    pub corrected_materialization: LongitudinalMaterializationReceiptV1,
    pub receipt: LongitudinalRemovalUpgradeReceiptV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCarryForwardRequestV1 {
    pub schema: String,
    pub source_root: PathBuf,
    pub clone_root: PathBuf,
    pub authority_root: PathBuf,
    pub source_materialization: LongitudinalMaterializationReceiptV1,
    pub corrected_execution: LongitudinalExecutionIdentityV1,
    pub slot: LongitudinalCarryForwardSlotV1,
    pub materializer_equivalence: LongitudinalMaterializerEquivalenceReceiptV1,
    pub final_authority_diff_sha256: String,
}

impl LongitudinalCarryForwardRequestV1 {
    pub fn execute(
        &self,
    ) -> Result<LongitudinalCarryForwardArtifactsV1, LongitudinalEvidenceError> {
        if self.schema != LONGITUDINAL_CARRY_FORWARD_REQUEST_SCHEMA_V1 {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }
        write_longitudinal_carry_forward_v1(
            &LongitudinalCarryForwardOptionsV1 {
                source_root: self.source_root.clone(),
                clone_root: self.clone_root.clone(),
                source_materialization: self.source_materialization.clone(),
                corrected_execution: self.corrected_execution.clone(),
                slot: self.slot,
                materializer_equivalence: self.materializer_equivalence.clone(),
                final_authority_diff_sha256: self.final_authority_diff_sha256.clone(),
            },
            &self.authority_root,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCarryForwardArtifactsV1 {
    pub carried_materialization: LongitudinalMaterializationReceiptV1,
    pub receipt: LongitudinalCarryForwardReceiptV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCarryForwardSmokeReceiptV1 {
    pub schema: String,
    pub timing_admissible: bool,
    pub terminal_evidence_admissible: bool,
    pub carry_forward_verified: bool,
    pub failure_receipt_verified: bool,
    pub package_verifier_receipt_verified: bool,
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

pub fn carry_forward_longitudinal_root_v1(
    options: &LongitudinalCarryForwardOptionsV1,
) -> Result<LongitudinalCarryForwardArtifactsV1, LongitudinalEvidenceError> {
    require_unprotected_environment()?;
    options
        .source_materialization
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    options
        .corrected_execution
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    options
        .slot
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    if options.slot.tier != options.source_materialization.manifest.tier
        || options.corrected_execution.parent_commit.is_some()
        || options.corrected_execution == options.source_materialization.manifest.execution
    {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let source_root =
        fs::canonicalize(&options.source_root).map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let clone_root =
        fs::canonicalize(&options.clone_root).map_err(|_| LongitudinalEvidenceError::Preflight)?;
    if source_root == clone_root
        || source_root.starts_with(&clone_root)
        || clone_root.starts_with(&source_root)
    {
        return Err(LongitudinalEvidenceError::UnsafeRoot);
    }

    let source_pre_inventory = longitudinal_store_data_inventory_v1(&source_root)
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let clone_pre_inventory = longitudinal_store_data_inventory_v1(&clone_root)
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let source_preflight =
        preflight_longitudinal_root_v1(&source_root, &options.source_materialization.manifest)?;
    if source_preflight != options.source_materialization
        || source_pre_inventory != clone_pre_inventory
    {
        return Err(LongitudinalEvidenceError::Preflight);
    }

    let mut carried_manifest = options.source_materialization.manifest.clone();
    carried_manifest.execution = options.corrected_execution.clone();
    carried_manifest.manifest_sha256 = carried_manifest
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    let carried_materialization = preflight_longitudinal_root_v1(&clone_root, &carried_manifest)?;

    let source_post_inventory = longitudinal_store_data_inventory_v1(&source_root)
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let clone_post_inventory = longitudinal_store_data_inventory_v1(&clone_root)
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    if source_pre_inventory != source_post_inventory || source_pre_inventory != clone_post_inventory
    {
        return Err(LongitudinalEvidenceError::Preflight);
    }

    options
        .materializer_equivalence
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;

    let mut receipt = LongitudinalCarryForwardReceiptV1 {
        schema: LONGITUDINAL_CARRY_FORWARD_RECEIPT_SCHEMA_V1.to_owned(),
        slot: options.slot,
        contract_sha256: options
            .source_materialization
            .manifest
            .contract_sha256
            .clone(),
        schedule_sha256: options
            .source_materialization
            .manifest
            .schedule_sha256
            .clone(),
        source_execution: options.source_materialization.manifest.execution.clone(),
        corrected_execution: options.corrected_execution.clone(),
        source_root_identity: options.source_materialization.root_identity.clone(),
        clone_root_identity: carried_materialization.root_identity.clone(),
        source_pre_inventory,
        source_post_inventory,
        clone_pre_inventory,
        clone_post_inventory,
        source_strict: options.source_materialization.strict.clone(),
        clone_strict: carried_materialization.strict.clone(),
        event_count: carried_materialization.manifest.event_count,
        content_count: carried_materialization.manifest.content_inventory.len() as u64,
        source_manifest_sha256: options
            .source_materialization
            .manifest
            .manifest_sha256
            .clone(),
        carried_manifest_sha256: carried_materialization.manifest.manifest_sha256.clone(),
        source_manifest_invariant_sha256: longitudinal_workload_manifest_carry_invariant_sha256_v1(
            &options.source_materialization.manifest,
        )
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?,
        carried_manifest_invariant_sha256:
            longitudinal_workload_manifest_carry_invariant_sha256_v1(
                &carried_materialization.manifest,
            )
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?,
        source_materialization_sha256: options
            .source_materialization
            .materialization_sha256
            .clone(),
        carried_materialization_sha256: carried_materialization.materialization_sha256.clone(),
        materializer_equivalence: options.materializer_equivalence.clone(),
        final_authority_diff_sha256: options.final_authority_diff_sha256.clone(),
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = receipt
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    receipt
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    Ok(LongitudinalCarryForwardArtifactsV1 {
        carried_materialization,
        receipt,
    })
}

pub fn verify_longitudinal_carry_forward_v1(
    options: &LongitudinalCarryForwardOptionsV1,
    artifacts: &LongitudinalCarryForwardArtifactsV1,
) -> Result<(), LongitudinalEvidenceError> {
    let verified = carry_forward_longitudinal_root_v1(options)?;
    if verified != *artifacts {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    Ok(())
}

pub fn write_longitudinal_carry_forward_v1(
    options: &LongitudinalCarryForwardOptionsV1,
    authority_root: &Path,
) -> Result<LongitudinalCarryForwardArtifactsV1, LongitudinalEvidenceError> {
    validate_fresh_local_root(authority_root, &options.source_root)?;
    let parent = authority_root
        .parent()
        .ok_or(LongitudinalEvidenceError::UnsafeRoot)?;
    let parent = fs::canonicalize(parent).map_err(|_| LongitudinalEvidenceError::UnsafeRoot)?;
    let source =
        fs::canonicalize(&options.source_root).map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let clone =
        fs::canonicalize(&options.clone_root).map_err(|_| LongitudinalEvidenceError::Preflight)?;
    if parent.starts_with(&source) || parent.starts_with(&clone) {
        return Err(LongitudinalEvidenceError::UnsafeRoot);
    }
    let artifacts = carry_forward_longitudinal_root_v1(options)?;
    fs::create_dir(authority_root).map_err(io_error)?;
    write_json_create_new(
        &authority_root.join(LONGITUDINAL_CARRIED_MATERIALIZATION_FILE_V1),
        &artifacts.carried_materialization,
    )
    .and_then(|()| {
        write_json_create_new(
            &authority_root.join(LONGITUDINAL_CARRY_FORWARD_RECEIPT_FILE_V1),
            &artifacts.receipt,
        )
    })?;
    Ok(artifacts)
}

pub fn upgrade_longitudinal_root_removals_v1(
    options: &LongitudinalRemovalUpgradeOptionsV1,
) -> Result<LongitudinalRemovalUpgradeArtifactsV1, LongitudinalEvidenceError> {
    require_unprotected_environment()?;
    options
        .source_materialization
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    options
        .corrected_execution
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    options
        .slot
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    if options.slot.tier != options.source_materialization.manifest.tier
        || options.corrected_execution.parent_commit.is_some()
        || options.corrected_execution == options.source_materialization.manifest.execution
    {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let source_root =
        fs::canonicalize(&options.source_root).map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let successor_root = fs::canonicalize(&options.successor_root)
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    if source_root == successor_root
        || source_root.starts_with(&successor_root)
        || successor_root.starts_with(&source_root)
    {
        return Err(LongitudinalEvidenceError::UnsafeRoot);
    }
    let source_enrollment = allowed_signers_path_for_repo(&source_root)
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let successor_enrollment = allowed_signers_path_for_repo(&successor_root)
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    if source_enrollment.exists() || successor_enrollment.exists() {
        return Err(LongitudinalEvidenceError::Preflight);
    }

    let source_pre_inventory = longitudinal_store_data_inventory_v1(&source_root)
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let successor_pre_inventory = longitudinal_store_data_inventory_v1(&successor_root)
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let source_preflight =
        preflight_longitudinal_root_v1(&source_root, &options.source_materialization.manifest)?;
    let successor_preflight =
        preflight_longitudinal_root_v1(&successor_root, &options.source_materialization.manifest)?;
    verify_longitudinal_materialization_pair_v1(&source_preflight, &successor_preflight)
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    if source_preflight != options.source_materialization
        || source_pre_inventory != successor_pre_inventory
    {
        return Err(LongitudinalEvidenceError::Preflight);
    }

    let applied = apply_longitudinal_removal_upgrade_v1(
        &successor_root,
        options.slot.tier,
        options.corrected_execution.clone(),
    )
    .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let corrected_materialization = match &applied.resumed_materialization.materialization {
        super::LongitudinalResumedMaterializationV1::Workload(materialization) => {
            materialization.clone()
        }
        super::LongitudinalResumedMaterializationV1::Capacity(_) => {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }
    };
    let source_post_inventory = longitudinal_store_data_inventory_v1(&source_root)
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    let successor_post_inventory = longitudinal_store_data_inventory_v1(&successor_root)
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    if source_pre_inventory != source_post_inventory
        || successor_post_inventory != applied.resumed_materialization.post_inventory
    {
        return Err(LongitudinalEvidenceError::Preflight);
    }
    let changed_event_ids = applied
        .changed_paths
        .iter()
        .map(|path| path.event_id.clone())
        .collect::<Vec<_>>();

    let mut receipt = LongitudinalRemovalUpgradeReceiptV1 {
        schema: super::LONGITUDINAL_REMOVAL_UPGRADE_RECEIPT_SCHEMA_V1.to_owned(),
        slot: options.slot,
        contract_sha256: options
            .source_materialization
            .manifest
            .contract_sha256
            .clone(),
        schedule_sha256: options
            .source_materialization
            .manifest
            .schedule_sha256
            .clone(),
        source_execution: options.source_materialization.manifest.execution.clone(),
        corrected_execution: options.corrected_execution.clone(),
        source_root_identity: options.source_materialization.root_identity.clone(),
        successor_root_identity: corrected_materialization.root_identity.clone(),
        source_pre_inventory,
        source_post_inventory,
        successor_pre_inventory,
        successor_post_inventory,
        source_strict: options.source_materialization.strict.clone(),
        successor_strict: corrected_materialization.strict.clone(),
        event_count: corrected_materialization.manifest.event_count,
        content_count: corrected_materialization.manifest.content_inventory.len() as u64,
        removed_content_count: corrected_materialization
            .manifest
            .removed_content_sha256
            .len() as u64,
        changed_paths: applied.changed_paths,
        enrollment_relative_path: applied.enrollment_relative_path,
        enrollment_sha256: applied.enrollment_sha256,
        enrollment_bytes: applied.enrollment_bytes,
        source_manifest_sha256: options
            .source_materialization
            .manifest
            .manifest_sha256
            .clone(),
        corrected_manifest_sha256: corrected_materialization.manifest.manifest_sha256.clone(),
        source_manifest_invariant_sha256:
            longitudinal_workload_manifest_upgrade_invariant_sha256_v1(
                &options.source_materialization.manifest,
                &options.source_materialization.strict,
                &changed_event_ids,
            )
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?,
        corrected_manifest_invariant_sha256:
            longitudinal_workload_manifest_upgrade_invariant_sha256_v1(
                &corrected_materialization.manifest,
                &corrected_materialization.strict,
                &changed_event_ids,
            )
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?,
        source_materialization_sha256: options
            .source_materialization
            .materialization_sha256
            .clone(),
        corrected_materialization_sha256: corrected_materialization.materialization_sha256.clone(),
        resume_receipt_sha256: applied.resumed_materialization.receipt_sha256.clone(),
        resume_events_created: applied.resumed_materialization.events_created,
        resume_events_existing: applied.resumed_materialization.events_existing,
        final_authority_diff_sha256: options.final_authority_diff_sha256.clone(),
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = receipt
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    receipt
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    Ok(LongitudinalRemovalUpgradeArtifactsV1 {
        corrected_materialization,
        receipt,
    })
}

pub fn write_longitudinal_removal_upgrade_v1(
    options: &LongitudinalRemovalUpgradeOptionsV1,
    authority_root: &Path,
) -> Result<LongitudinalRemovalUpgradeArtifactsV1, LongitudinalEvidenceError> {
    validate_fresh_local_root(authority_root, &options.source_root)?;
    let parent = authority_root
        .parent()
        .ok_or(LongitudinalEvidenceError::UnsafeRoot)?;
    let parent = fs::canonicalize(parent).map_err(|_| LongitudinalEvidenceError::UnsafeRoot)?;
    let successor = fs::canonicalize(&options.successor_root)
        .map_err(|_| LongitudinalEvidenceError::Preflight)?;
    if parent.starts_with(&successor) {
        return Err(LongitudinalEvidenceError::UnsafeRoot);
    }
    let artifacts = upgrade_longitudinal_root_removals_v1(options)?;
    fs::create_dir(authority_root).map_err(io_error)?;
    write_json_create_new(
        &authority_root.join(LONGITUDINAL_CORRECTED_MATERIALIZATION_FILE_V1),
        &artifacts.corrected_materialization,
    )?;
    write_json_create_new(
        &authority_root.join(LONGITUDINAL_REMOVAL_UPGRADE_RECEIPT_FILE_V1),
        &artifacts.receipt,
    )?;
    Ok(artifacts)
}

pub fn longitudinal_controller_failure_receipt_v1(
    operation_selector: LongitudinalFailureOperationSelectorV1,
    http: Option<LongitudinalHttpFailureV1>,
    inspector_exit: LongitudinalInspectorExitV1,
    stderr: LongitudinalStderrFailureV1,
) -> Result<LongitudinalControllerFailureReceiptV1, LongitudinalEvidenceError> {
    let mut receipt = LongitudinalControllerFailureReceiptV1 {
        schema: LONGITUDINAL_CONTROLLER_FAILURE_RECEIPT_SCHEMA_V1.to_owned(),
        operation_selector,
        http,
        inspector_exit,
        stderr,
        immutable: true,
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

pub fn verify_longitudinal_package_receipt_v1(
    raw_root: &Path,
) -> Result<LongitudinalPackageVerificationReceiptV1, LongitudinalEvidenceError> {
    let workload = raw_root
        .join(LONGITUDINAL_EVIDENCE_PACKAGE_FILE_V1)
        .is_file();
    let capacity = raw_root
        .join(LONGITUDINAL_CAPACITY_PACKAGE_FILE_V1)
        .is_file();
    let (package_kind, package_sha256, contract_sha256, execution_identity_sha256, raw_inventory) =
        match (workload, capacity) {
            (true, false) => {
                let package = verify_longitudinal_evidence_package_v1(raw_root)?;
                (
                    LongitudinalVerifiedPackageKindV1::Workload,
                    package.package_sha256,
                    package.contract_sha256,
                    package
                        .base_execution
                        .canonical_sha256()
                        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?,
                    package.raw_inventory,
                )
            }
            (false, true) => {
                let package = verify_longitudinal_capacity_package_v1(raw_root)?;
                (
                    LongitudinalVerifiedPackageKindV1::Capacity,
                    package.package_sha256,
                    package.contract_sha256,
                    package
                        .base_execution
                        .canonical_sha256()
                        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?,
                    package.raw_inventory,
                )
            }
            _ => return Err(LongitudinalEvidenceError::InvalidReceipt),
        };
    let mut receipt = LongitudinalPackageVerificationReceiptV1 {
        schema: super::LONGITUDINAL_PACKAGE_VERIFICATION_RECEIPT_SCHEMA_V1.to_owned(),
        package_kind,
        package_sha256,
        contract_sha256,
        execution_identity_sha256,
        raw_inventory_sha256: canonical_sha256(&raw_inventory)?,
        verified: true,
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

pub fn verify_longitudinal_carry_forward_authority_package_v1(
    authority_package_path: &Path,
    workload_package_root: &Path,
) -> Result<LongitudinalCarryForwardAuthorityPackageV1, LongitudinalEvidenceError> {
    let package: LongitudinalCarryForwardAuthorityPackageV1 = read_json(authority_package_path)?;
    let verification_receipt = verify_longitudinal_package_receipt_v1(workload_package_root)?;
    let completion = package
        .completion
        .as_ref()
        .ok_or(LongitudinalEvidenceError::InvalidReceipt)?;
    if completion.verification_receipt != verification_receipt
        || !completion.package_verified
        || completion.final_workload_package_sha256 != verification_receipt.package_sha256
    {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    package
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    Ok(package)
}

pub fn assemble_longitudinal_carry_forward_root_authority_v1(
    corrected_execution: LongitudinalExecutionIdentityV1,
    carry_receipts: Vec<LongitudinalCarryForwardReceiptV1>,
) -> Result<LongitudinalCarryForwardAuthorityPackageV1, LongitudinalEvidenceError> {
    let mut package = LongitudinalCarryForwardAuthorityPackageV1 {
        schema: super::LONGITUDINAL_CARRY_FORWARD_AUTHORITY_PACKAGE_SCHEMA_V1.to_owned(),
        contract_sha256: longitudinal_runner_contract_v1().contract_sha256,
        corrected_execution,
        carry_receipts,
        authority_set_sha256: String::new(),
        completion: None,
        package_sha256: String::new(),
    };
    package.authority_set_sha256 = package
        .canonical_authority_set_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    package.package_sha256 = package
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    package
        .validate_incomplete()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    Ok(package)
}

pub fn complete_longitudinal_carry_forward_authority_package_v1(
    root_authority: &LongitudinalCarryForwardAuthorityPackageV1,
    workload_package_root: &Path,
) -> Result<LongitudinalCarryForwardAuthorityPackageV1, LongitudinalEvidenceError> {
    root_authority
        .validate_incomplete()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    let verification_receipt = verify_longitudinal_package_receipt_v1(workload_package_root)?;
    if verification_receipt.package_kind != LongitudinalVerifiedPackageKindV1::Workload {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let mut package = root_authority.clone();
    package.completion = Some(LongitudinalCarryForwardAuthorityCompletionV1 {
        final_workload_package_sha256: verification_receipt.package_sha256.clone(),
        verification_receipt,
        package_verified: true,
    });
    package.package_sha256 = package
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    package
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    Ok(package)
}

pub fn write_longitudinal_carry_forward_authority_package_v1(
    root_authority: &LongitudinalCarryForwardAuthorityPackageV1,
    workload_package_root: &Path,
    output_path: &Path,
) -> Result<LongitudinalCarryForwardAuthorityPackageV1, LongitudinalEvidenceError> {
    validate_fresh_local_root(output_path, workload_package_root)?;
    let package = complete_longitudinal_carry_forward_authority_package_v1(
        root_authority,
        workload_package_root,
    )?;
    write_json_create_new(output_path, &package)?;
    Ok(package)
}

pub fn verify_longitudinal_removal_upgrade_authority_package_v1(
    authority_package_path: &Path,
    workload_package_root: &Path,
) -> Result<LongitudinalRemovalUpgradeAuthorityPackageV1, LongitudinalEvidenceError> {
    let package: LongitudinalRemovalUpgradeAuthorityPackageV1 = read_json(authority_package_path)?;
    let verification_receipt = verify_longitudinal_package_receipt_v1(workload_package_root)?;
    let completion = package
        .completion
        .as_ref()
        .ok_or(LongitudinalEvidenceError::InvalidReceipt)?;
    if completion.verification_receipt != verification_receipt
        || !completion.package_verified
        || completion.final_workload_package_sha256 != verification_receipt.package_sha256
    {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    package
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    Ok(package)
}

pub fn assemble_longitudinal_removal_upgrade_root_authority_v1(
    corrected_execution: LongitudinalExecutionIdentityV1,
    upgrade_artifacts: Vec<LongitudinalRemovalUpgradeArtifactsV1>,
    materializer_equivalences: Vec<LongitudinalMaterializerEquivalenceReceiptV1>,
) -> Result<LongitudinalRemovalUpgradeAuthorityPackageV1, LongitudinalEvidenceError> {
    let upgrade_receipts = upgrade_artifacts
        .into_iter()
        .map(|artifacts| {
            artifacts
                .corrected_materialization
                .validate()
                .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
            artifacts
                .receipt
                .validate()
                .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
            if artifacts.corrected_materialization.manifest.execution
                != artifacts.receipt.corrected_execution
                || artifacts.corrected_materialization.root_identity
                    != artifacts.receipt.successor_root_identity
                || artifacts.corrected_materialization.strict != artifacts.receipt.successor_strict
                || artifacts.corrected_materialization.materialization_sha256
                    != artifacts.receipt.corrected_materialization_sha256
            {
                return Err(LongitudinalEvidenceError::InvalidReceipt);
            }
            Ok(artifacts.receipt)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut package = LongitudinalRemovalUpgradeAuthorityPackageV1 {
        schema: super::LONGITUDINAL_REMOVAL_UPGRADE_AUTHORITY_PACKAGE_SCHEMA_V1.to_owned(),
        contract_sha256: longitudinal_runner_contract_v1().contract_sha256,
        corrected_execution,
        upgrade_receipts,
        materializer_equivalences,
        authority_set_sha256: String::new(),
        completion: None,
        package_sha256: String::new(),
    };
    package.authority_set_sha256 = package
        .canonical_authority_set_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    package.package_sha256 = package
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    package
        .validate_incomplete()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    Ok(package)
}

pub fn complete_longitudinal_removal_upgrade_authority_package_v1(
    root_authority: &LongitudinalRemovalUpgradeAuthorityPackageV1,
    workload_package_root: &Path,
) -> Result<LongitudinalRemovalUpgradeAuthorityPackageV1, LongitudinalEvidenceError> {
    root_authority
        .validate_incomplete()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    let verification_receipt = verify_longitudinal_package_receipt_v1(workload_package_root)?;
    if verification_receipt.package_kind != LongitudinalVerifiedPackageKindV1::Workload {
        return Err(LongitudinalEvidenceError::InvalidReceipt);
    }
    let mut package = root_authority.clone();
    package.completion = Some(LongitudinalRemovalUpgradeAuthorityCompletionV1 {
        final_workload_package_sha256: verification_receipt.package_sha256.clone(),
        verification_receipt,
        package_verified: true,
    });
    package.package_sha256 = package
        .canonical_sha256()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    package
        .validate()
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
    Ok(package)
}

pub fn write_longitudinal_removal_upgrade_authority_package_v1(
    root_authority: &LongitudinalRemovalUpgradeAuthorityPackageV1,
    workload_package_root: &Path,
    output_path: &Path,
) -> Result<LongitudinalRemovalUpgradeAuthorityPackageV1, LongitudinalEvidenceError> {
    validate_fresh_local_root(output_path, workload_package_root)?;
    let package = complete_longitudinal_removal_upgrade_authority_package_v1(
        root_authority,
        workload_package_root,
    )?;
    write_json_create_new(output_path, &package)?;
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

pub fn longitudinal_carry_forward_non_timing_smoke_v1()
-> Result<LongitudinalCarryForwardSmokeReceiptV1, LongitudinalEvidenceError> {
    require_unprotected_environment()?;
    let smoke_root = std::env::temp_dir().join(format!(
        "pointbreak-longitudinal-carry-forward-smoke-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io_error)?
            .as_nanos()
    ));
    fs::create_dir(&smoke_root).map_err(io_error)?;
    let result = (|| {
        let source_root = smoke_root.join("source");
        let optimized_root = smoke_root.join("optimized");
        let clone_root = smoke_root.join("clone");
        initialize_evidence_root(&source_root)?;

        let mut source_execution = smoke_execution_identity();
        source_execution.build_profile = "release-uninstrumented".to_owned();
        let mut optimized_execution = source_execution.clone();
        optimized_execution.source_commit = "4".repeat(40);
        optimized_execution.source_tree = "5".repeat(40);
        optimized_execution.runner_sha256 = "6".repeat(64);
        let source_materialization =
            materialize_longitudinal_workload_v1(super::LongitudinalMaterializeOptionsV1::new(
                &source_root,
                LongitudinalTierV1::L1,
                source_execution,
            ))
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
        let source_inventory = longitudinal_store_data_inventory_v1(&source_root)
            .map_err(|_| LongitudinalEvidenceError::Preflight)?;
        copy_directory_tree(&source_root, &optimized_root)?;
        let mut optimized_manifest = source_materialization.manifest.clone();
        optimized_manifest.execution = optimized_execution;
        optimized_manifest.manifest_sha256 = optimized_manifest
            .canonical_sha256()
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
        let optimized_materialization =
            preflight_longitudinal_root_v1(&optimized_root, &optimized_manifest)?;
        let final_authority_diff_sha256 = sha256_bytes_hex(b"public carry-forward smoke diff");
        let materializer_equivalence = verify_longitudinal_materializer_equivalence_v1(
            &source_root,
            &source_materialization,
            &optimized_root,
            &optimized_materialization,
            final_authority_diff_sha256.clone(),
        )
        .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;

        copy_directory_tree(&source_root, &clone_root)?;
        let mut corrected_execution = source_materialization.manifest.execution.clone();
        corrected_execution.source_commit = "7".repeat(40);
        corrected_execution.source_tree = "8".repeat(40);
        corrected_execution.runner_sha256 = "9".repeat(64);
        let options = LongitudinalCarryForwardOptionsV1 {
            source_root: source_root.clone(),
            clone_root: clone_root.clone(),
            source_materialization,
            corrected_execution,
            slot: LongitudinalCarryForwardSlotV1 {
                tier: LongitudinalTierV1::L1,
                lane: LongitudinalLaneV1::ReleaseUninstrumented,
                independent_run: 1,
            },
            materializer_equivalence,
            final_authority_diff_sha256,
        };
        let authority_root = smoke_root.join("authority");
        let request = LongitudinalCarryForwardRequestV1 {
            schema: LONGITUDINAL_CARRY_FORWARD_REQUEST_SCHEMA_V1.to_owned(),
            source_root: options.source_root.clone(),
            clone_root: options.clone_root.clone(),
            authority_root: authority_root.clone(),
            source_materialization: options.source_materialization.clone(),
            corrected_execution: options.corrected_execution.clone(),
            slot: options.slot,
            materializer_equivalence: options.materializer_equivalence.clone(),
            final_authority_diff_sha256: options.final_authority_diff_sha256.clone(),
        };
        let mut unsanitized_request = serde_json::to_value(&request)
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
        unsanitized_request
            .as_object_mut()
            .ok_or(LongitudinalEvidenceError::InvalidReceipt)?
            .insert("pointbreakHome".to_owned(), "/private/store".into());
        if serde_json::from_value::<LongitudinalCarryForwardRequestV1>(unsanitized_request).is_ok()
        {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }
        let artifacts = request.execute()?;
        verify_longitudinal_carry_forward_v1(&options, &artifacts)?;
        let persisted = LongitudinalCarryForwardArtifactsV1 {
            carried_materialization: read_json(
                &authority_root.join(LONGITUDINAL_CARRIED_MATERIALIZATION_FILE_V1),
            )?,
            receipt: read_json(&authority_root.join(LONGITUDINAL_CARRY_FORWARD_RECEIPT_FILE_V1))?,
        };
        if persisted != artifacts {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }
        let clone_store = store_dir_for_repo(&clone_root)
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;
        let carrier = &artifacts.carried_materialization.manifest.event_carriers[0];
        fs::write(
            clone_store
                .join("events")
                .join(format!("{}.json", carrier.logical_key_sha256)),
            b"{}",
        )
        .map_err(io_error)?;
        if !matches!(
            verify_longitudinal_carry_forward_v1(&options, &artifacts),
            Err(LongitudinalEvidenceError::Preflight)
        ) || longitudinal_store_data_inventory_v1(&source_root)
            .map_err(|_| LongitudinalEvidenceError::Preflight)?
            != source_inventory
        {
            return Err(LongitudinalEvidenceError::InvalidReceipt);
        }

        let failure = longitudinal_controller_failure_receipt_v1(
            LongitudinalFailureOperationSelectorV1::InspectorRevisionListPreflight,
            Some(LongitudinalHttpFailureV1 {
                status: 500,
                body_classification: LongitudinalHttpBodyClassificationV1::JsonError,
                body_bytes: 21,
                body_sha256: Some(sha256_bytes_hex(b"sanitized error body")),
            }),
            LongitudinalInspectorExitV1::Exited(1),
            LongitudinalStderrFailureV1 {
                classification: LongitudinalStderrClassificationV1::KnownDiagnostic,
                bytes: 20,
                sha256: Some(sha256_bytes_hex(b"sanitized diagnostic")),
            },
        )?;
        failure
            .validate()
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;

        let package_root = smoke_root.join("package");
        fs::create_dir(&package_root).map_err(io_error)?;
        write_smoke_workload_package(&package_root, artifacts.carried_materialization.clone())?;
        let package_verifier = verify_longitudinal_package_receipt_v1(&package_root)?;
        package_verifier
            .validate()
            .map_err(|_| LongitudinalEvidenceError::InvalidReceipt)?;

        Ok(LongitudinalCarryForwardSmokeReceiptV1 {
            schema: "pointbreak.longitudinal-carry-forward-non-timing-smoke.v1".to_owned(),
            timing_admissible: false,
            terminal_evidence_admissible: false,
            carry_forward_verified: true,
            failure_receipt_verified: true,
            package_verifier_receipt_verified: true,
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

fn write_smoke_workload_package(
    package_root: &Path,
    materialization: LongitudinalMaterializationReceiptV1,
) -> Result<(), LongitudinalEvidenceError> {
    let raw = b"public carry-forward package smoke fixture\n";
    fs::write(package_root.join("raw.json"), raw).map_err(io_error)?;
    let raw_inventory = vec![super::LongitudinalRawFileV1 {
        relative_path: "raw.json".to_owned(),
        sha256: sha256_bytes_hex(raw),
        bytes: raw.len() as u64,
    }];
    let mut package = LongitudinalEvidencePackageV1 {
        schema: super::LONGITUDINAL_EVIDENCE_PACKAGE_SCHEMA_V1.to_owned(),
        purpose: super::LongitudinalPackagePurposeV1::NonTimingSmoke,
        contract_sha256: longitudinal_runner_contract_v1().contract_sha256,
        base_execution: materialization.manifest.execution.clone(),
        derivative_execution: None,
        materializations: vec![materialization],
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
    write_json_create_new(
        &package_root.join(LONGITUDINAL_EVIDENCE_PACKAGE_FILE_V1),
        &package,
    )
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
    require_unprotected_environment()?;
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

fn require_unprotected_environment() -> Result<(), LongitudinalEvidenceError> {
    if PROTECTED_ENVIRONMENT_VARIABLES
        .iter()
        .any(|variable| std::env::var_os(variable).is_some())
    {
        return Err(LongitudinalEvidenceError::ProtectedEnvironment);
    }
    Ok(())
}

fn write_json_create_new(
    path: &Path,
    value: &impl Serialize,
) -> Result<(), LongitudinalEvidenceError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io_error)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(io_error)?;
    file.write_all(&bytes).map_err(io_error)?;
    file.write_all(b"\n").map_err(io_error)?;
    file.sync_all().map_err(io_error)
}

fn copy_directory_tree(source: &Path, destination: &Path) -> Result<(), LongitudinalEvidenceError> {
    if destination.exists() {
        return Err(LongitudinalEvidenceError::UnsafeRoot);
    }
    fs::create_dir(destination).map_err(io_error)?;
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((source_directory, destination_directory)) = pending.pop() {
        for entry in fs::read_dir(&source_directory).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let source_path = entry.path();
            let destination_path = destination_directory.join(entry.file_name());
            let metadata = fs::symlink_metadata(&source_path).map_err(io_error)?;
            if metadata.is_dir() {
                fs::create_dir(&destination_path).map_err(io_error)?;
                pending.push((source_path, destination_path));
            } else if metadata.is_file() {
                fs::copy(&source_path, &destination_path).map_err(io_error)?;
            } else {
                return Err(LongitudinalEvidenceError::UnsafeRoot);
            }
        }
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
    use crate::bench_support::EventType;
    use crate::session::{BaseProjectionConfig, TrustSet, history_base_projection};

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
    fn longitudinal_removal_upgrade_matches_a_fresh_corrected_l1_root() {
        let disposable = tempfile::tempdir().unwrap();
        let source = disposable.path().join("source");
        let successor = disposable.path().join("successor");
        let fresh = disposable.path().join("fresh");
        initialize_evidence_root(&source).unwrap();
        let source_execution = removal_upgrade_source_execution();
        let corrected_execution = removal_upgrade_corrected_execution();
        let corrected_source = materialize_longitudinal_workload_v1(
            super::super::LongitudinalMaterializeOptionsV1::new(
                &source,
                LongitudinalTierV1::L1,
                source_execution.clone(),
            ),
        )
        .unwrap();
        let legacy_source = downgrade_longitudinal_l1_to_legacy(&source, corrected_source);
        let legacy_inventory = longitudinal_store_data_inventory_v1(&source).unwrap();
        assert!(
            super::super::resume_longitudinal_workload_v1(
                super::super::LongitudinalMaterializeOptionsV1::new(
                    &source,
                    LongitudinalTierV1::L1,
                    corrected_execution.clone(),
                ),
            )
            .is_err(),
            "legacy roots must use the explicit clone upgrade"
        );
        assert_eq!(
            longitudinal_store_data_inventory_v1(&source).unwrap(),
            legacy_inventory
        );
        assert!(!allowed_signers_path_for_repo(&source).unwrap().exists());
        copy_directory_tree(&source, &successor).unwrap();

        let options = LongitudinalRemovalUpgradeOptionsV1 {
            source_root: source.clone(),
            successor_root: successor.clone(),
            source_materialization: legacy_source,
            corrected_execution: corrected_execution.clone(),
            slot: LongitudinalCarryForwardSlotV1 {
                tier: LongitudinalTierV1::L1,
                lane: LongitudinalLaneV1::ReleaseUninstrumented,
                independent_run: 1,
            },
            final_authority_diff_sha256: sha256_bytes_hex(b"removal-integrity-diff"),
        };
        let upgraded = upgrade_longitudinal_root_removals_v1(&options).unwrap();
        assert_eq!(upgraded.receipt.changed_paths.len(), 12);
        assert_eq!(
            upgraded.receipt.resume_events_created, 0,
            "the corrected materializer must resume the upgraded root without replay"
        );
        assert_eq!(upgraded.receipt.resume_events_existing, 1_024);

        initialize_evidence_root(&fresh).unwrap();
        let fresh_materialization = materialize_longitudinal_workload_v1(
            super::super::LongitudinalMaterializeOptionsV1::new(
                &fresh,
                LongitudinalTierV1::L1,
                corrected_execution,
            ),
        )
        .unwrap();
        let equivalence = verify_longitudinal_materializer_equivalence_v1(
            &successor,
            &upgraded.corrected_materialization,
            &fresh,
            &fresh_materialization,
            options.final_authority_diff_sha256,
        )
        .unwrap();
        assert!(equivalence.equivalent);

        for root in [&successor, &fresh] {
            let trust =
                TrustSet::from_allowed_signers_file(allowed_signers_path_for_repo(root).unwrap())
                    .unwrap();
            history_base_projection(
                root,
                &BaseProjectionConfig {
                    trust_set: trust,
                    ..BaseProjectionConfig::default()
                },
            )
            .expect("trusted generated removals keep full history readable");
        }
    }

    #[test]
    fn longitudinal_removal_upgrade_rejects_legacy_root_drift_before_writing() {
        let disposable = tempfile::tempdir().unwrap();
        let source = disposable.path().join("source");
        initialize_evidence_root(&source).unwrap();
        let corrected_source = materialize_longitudinal_workload_v1(
            super::super::LongitudinalMaterializeOptionsV1::new(
                &source,
                LongitudinalTierV1::L1,
                removal_upgrade_source_execution(),
            ),
        )
        .unwrap();
        let legacy_source = downgrade_longitudinal_l1_to_legacy(&source, corrected_source);

        assert_rejected_upgrade_clone(
            &source,
            &legacy_source,
            disposable.path(),
            "conflicting-enrollment",
            |root| {
                let enrollment = allowed_signers_path_for_repo(root).unwrap();
                fs::write(enrollment, b"{\"allowedSigners\":{}}\n").unwrap();
            },
        );
        assert_rejected_upgrade_clone(
            &source,
            &legacy_source,
            disposable.path(),
            "wrong-store-mode",
            |root| set_store_mode_for_repo(root, StoreMode::Shared).unwrap(),
        );
        assert_rejected_upgrade_clone(
            &source,
            &legacy_source,
            disposable.path(),
            "missing-removal",
            |root| {
                let event = read_events(root)
                    .unwrap()
                    .into_iter()
                    .find(|event| event.event_type == EventType::ArtifactRemoved)
                    .unwrap();
                fs::remove_file(event_file(root, &event.idempotency_key)).unwrap();
            },
        );
        assert_rejected_upgrade_clone(
            &source,
            &legacy_source,
            disposable.path(),
            "unexpected-signature",
            |root| {
                let events = read_events(root).unwrap();
                let signed = events.iter().find(|event| event.signer.is_some()).unwrap();
                let mut removal = events
                    .iter()
                    .find(|event| event.event_type == EventType::ArtifactRemoved)
                    .unwrap()
                    .clone();
                removal.signer = signed.signer.clone();
                removal.signature = signed.signature.clone();
                fs::write(
                    event_file(root, &removal.idempotency_key),
                    serde_json::to_vec(&removal).unwrap(),
                )
                .unwrap();
            },
        );
        assert_rejected_upgrade_clone(
            &source,
            &legacy_source,
            disposable.path(),
            "unexpected-ingest",
            |root| {
                let mut event = read_events(root)
                    .unwrap()
                    .into_iter()
                    .find(|event| event.event_type == EventType::ArtifactRemoved)
                    .unwrap();
                event.ingest.as_mut().unwrap().received_at = "2026-02-01T00:00:00.001Z".to_owned();
                fs::write(
                    event_file(root, &event.idempotency_key),
                    serde_json::to_vec(&event).unwrap(),
                )
                .unwrap();
            },
        );
        assert_rejected_upgrade_clone(
            &source,
            &legacy_source,
            disposable.path(),
            "unexpected-path",
            |root| {
                let event = read_events(root)
                    .unwrap()
                    .into_iter()
                    .find(|event| event.event_type == EventType::ArtifactRemoved)
                    .unwrap();
                let path = event_file(root, &event.idempotency_key);
                fs::rename(&path, path.with_extension("moved")).unwrap();
            },
        );
        assert_rejected_upgrade_clone(
            &source,
            &legacy_source,
            disposable.path(),
            "present-content",
            |root| {
                let path = store_dir_for_repo(root)
                    .unwrap()
                    .join("artifacts/notes/unexpected.json");
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, b"unexpected").unwrap();
            },
        );
        assert_rejected_upgrade_clone(
            &source,
            &legacy_source,
            disposable.path(),
            "non-removal-drift",
            |root| {
                let event = read_events(root)
                    .unwrap()
                    .into_iter()
                    .find(|event| event.event_type != EventType::ArtifactRemoved)
                    .unwrap();
                let path = event_file(root, &event.idempotency_key);
                let mut bytes = fs::read(&path).unwrap();
                bytes.push(b' ');
                fs::write(path, bytes).unwrap();
            },
        );

        let successor = disposable.path().join("source-mutation");
        copy_directory_tree(&source, &successor).unwrap();
        let event = read_events(&source)
            .unwrap()
            .into_iter()
            .find(|event| event.event_type != EventType::ArtifactRemoved)
            .unwrap();
        let source_event = event_file(&source, &event.idempotency_key);
        let original = fs::read(&source_event).unwrap();
        let mut drifted = original.clone();
        drifted.push(b' ');
        fs::write(&source_event, drifted).unwrap();
        let successor_before = test_root_inventory(&successor);
        let result = upgrade_longitudinal_root_removals_v1(&LongitudinalRemovalUpgradeOptionsV1 {
            source_root: source.clone(),
            successor_root: successor.clone(),
            source_materialization: legacy_source.clone(),
            corrected_execution: removal_upgrade_corrected_execution(),
            slot: LongitudinalCarryForwardSlotV1 {
                tier: LongitudinalTierV1::L1,
                lane: LongitudinalLaneV1::ReleaseUninstrumented,
                independent_run: 1,
            },
            final_authority_diff_sha256: sha256_bytes_hex(b"removal-integrity-diff"),
        });
        assert!(result.is_err(), "source mutation must fail closed");
        assert_eq!(test_root_inventory(&successor), successor_before);
        fs::write(source_event, original).unwrap();
        assert_eq!(
            preflight_longitudinal_root_v1(&source, &legacy_source.manifest).unwrap(),
            legacy_source
        );
    }

    fn assert_rejected_upgrade_clone(
        source: &Path,
        source_materialization: &LongitudinalMaterializationReceiptV1,
        parent: &Path,
        label: &str,
        mutate: impl FnOnce(&Path),
    ) {
        let successor = parent.join(label);
        copy_directory_tree(source, &successor).unwrap();
        mutate(&successor);
        let pre_inventory = test_root_inventory(&successor);
        let enrollment = allowed_signers_path_for_repo(&successor).unwrap();
        let pre_enrollment = fs::read(&enrollment).ok();
        let result = upgrade_longitudinal_root_removals_v1(&LongitudinalRemovalUpgradeOptionsV1 {
            source_root: source.to_path_buf(),
            successor_root: successor.clone(),
            source_materialization: source_materialization.clone(),
            corrected_execution: removal_upgrade_corrected_execution(),
            slot: LongitudinalCarryForwardSlotV1 {
                tier: LongitudinalTierV1::L1,
                lane: LongitudinalLaneV1::ReleaseUninstrumented,
                independent_run: 1,
            },
            final_authority_diff_sha256: sha256_bytes_hex(b"removal-integrity-diff"),
        });
        assert!(result.is_err(), "{label} must fail closed");
        assert_eq!(
            test_root_inventory(&successor),
            pre_inventory,
            "{label} must not write after failed preflight"
        );
        assert_eq!(
            fs::read(enrollment).ok(),
            pre_enrollment,
            "{label} must not alter signer enrollment"
        );
        fs::remove_dir_all(successor).unwrap();
    }

    fn test_root_inventory(root: &Path) -> String {
        let files = list_relative_files(root, root).unwrap();
        let inventory = files
            .into_iter()
            .map(|relative_path| {
                let bytes = fs::read(root.join(&relative_path)).unwrap();
                (relative_path, bytes.len() as u64, sha256_bytes_hex(&bytes))
            })
            .collect::<Vec<_>>();
        canonical_sha256(&inventory).unwrap()
    }

    fn event_file(root: &Path, idempotency_key: &str) -> PathBuf {
        store_dir_for_repo(root)
            .unwrap()
            .join("events")
            .join(format!(
                "{}.json",
                sha256_bytes_hex(idempotency_key.as_bytes())
            ))
    }

    fn downgrade_longitudinal_l1_to_legacy(
        root: &Path,
        mut materialization: LongitudinalMaterializationReceiptV1,
    ) -> LongitudinalMaterializationReceiptV1 {
        fs::remove_file(allowed_signers_path_for_repo(root).unwrap()).unwrap();
        let store = store_dir_for_repo(root).unwrap();
        let events = read_events(root).unwrap();
        for mut event in events
            .into_iter()
            .filter(|event| event.event_type == EventType::ArtifactRemoved)
        {
            event.signer = None;
            event.signature = None;
            let carrier = materialization
                .manifest
                .event_carriers
                .iter_mut()
                .find(|carrier| carrier.event_id == event.event_id.as_str())
                .unwrap();
            let raw = serde_json::to_vec(&event).unwrap();
            fs::write(
                store
                    .join("events")
                    .join(format!("{}.json", carrier.logical_key_sha256)),
                &raw,
            )
            .unwrap();
            carrier.raw_sha256 = sha256_bytes_hex(&raw);
            carrier.raw_bytes = raw.len() as u64;
            let identity = materialization
                .manifest
                .ordered_events
                .iter_mut()
                .find(|identity| identity.event_id == event.event_id.as_str())
                .unwrap();
            identity.canonical_decoded_sha256 = sha256_bytes_hex(
                &canonical_json_bytes(&serde_json::to_value(&event).unwrap()).unwrap(),
            );
        }
        materialization.strict = strict_preflight(
            root,
            &materialization.manifest.ordered_events,
            &materialization.manifest.event_carriers,
            &materialization.manifest.content_inventory,
        )
        .unwrap();
        for receipt in &mut materialization.manifest.expected_semantic_receipts {
            receipt.semantic_receipt_sha256 = longitudinal_canonical_sha256_v1(&(
                receipt.operation,
                &materialization.strict,
                &materialization.manifest.ordered_events,
            ))
            .unwrap();
        }
        materialization.manifest.manifest_sha256 =
            materialization.manifest.canonical_sha256().unwrap();
        materialization.materialization_sha256 = materialization.canonical_sha256().unwrap();
        materialization.validate().unwrap();
        materialization
    }

    fn removal_upgrade_source_execution() -> LongitudinalExecutionIdentityV1 {
        let mut execution = smoke_execution_identity();
        execution.source_commit = "4".repeat(40);
        execution.source_tree = "5".repeat(40);
        execution.runner_sha256 = "6".repeat(64);
        execution.build_profile = "release-uninstrumented".to_owned();
        execution
    }

    fn removal_upgrade_corrected_execution() -> LongitudinalExecutionIdentityV1 {
        let mut execution = smoke_execution_identity();
        execution.source_commit = "7".repeat(40);
        execution.source_tree = "8".repeat(40);
        execution.runner_sha256 = "9".repeat(64);
        execution.build_profile = "corrected-uninstrumented".to_owned();
        execution
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

    #[test]
    fn longitudinal_carry_forward_public_smoke_proves_clone_and_verifier_mechanics() {
        let receipt = longitudinal_carry_forward_non_timing_smoke_v1().unwrap();
        assert_eq!(
            receipt.schema,
            "pointbreak.longitudinal-carry-forward-non-timing-smoke.v1"
        );
        assert!(!receipt.timing_admissible);
        assert!(!receipt.terminal_evidence_admissible);
        assert!(receipt.carry_forward_verified);
        assert!(receipt.failure_receipt_verified);
        assert!(receipt.package_verifier_receipt_verified);
    }
}
