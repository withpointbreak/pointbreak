use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bench_support::longitudinal::contract::{
    LONGITUDINAL_CAPACITY_MATERIALIZATION_RECEIPT_SCHEMA_V1,
    LONGITUDINAL_MATERIALIZATION_RECEIPT_SCHEMA_V1,
    LONGITUDINAL_MATERIALIZATION_RESUME_RECEIPT_SCHEMA_V1,
    LONGITUDINAL_MATERIALIZER_EQUIVALENCE_RECEIPT_SCHEMA_V1, LONGITUDINAL_PUBLIC_SEED_HEX_V1,
    LongitudinalCapacityManifestV1, LongitudinalCapacityMaterializationReceiptV1,
    LongitudinalCapacityProfileV1, LongitudinalCapacitySubjectV1, LongitudinalEventFamilyCountV1,
    LongitudinalExecutionIdentityV1, LongitudinalExpectedSemanticReceiptV1,
    LongitudinalMaterializationReceiptV1, LongitudinalMaterializationResumeReceiptV1,
    LongitudinalMaterializationSubjectV1, LongitudinalMaterializerEquivalenceReceiptV1,
    LongitudinalMaterializerRootReceiptV1, LongitudinalRemovalUpgradePathV1,
    LongitudinalResumedMaterializationV1, LongitudinalStoreDataInventoryV1, LongitudinalTierV1,
    LongitudinalWorkloadManifestV1, longitudinal_capacity_contract_v1,
    longitudinal_runner_contract_v1,
};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
use crate::session::benchmark::{
    LongitudinalRecordShapeV1, LongitudinalRecordSpecV1, prepare_longitudinal_record_v1,
    upgrade_longitudinal_removals_v1, write_longitudinal_records_v1,
};
use crate::session::{
    carrier_target_full_scan_count, format_rfc3339_utc_millis,
    reset_carrier_target_full_scan_count, store_dir_for_repo,
};

pub const LONGITUDINAL_FIXED_EPOCH_V1: &str = "2026-01-01T00:00:00.000Z";
pub const LONGITUDINAL_FIXED_INGEST_RECEIVED_AT_V1: &str = "2026-02-01T00:00:00.000Z";
pub const LONGITUDINAL_FIXED_CLOCK_IDENTITY_V1: &str = "pointbreak.longitudinal.fixed-clock.v1";

const LONGITUDINAL_FIXED_EPOCH_MILLIS_V1: i64 = 1_767_225_600_000;
const SIX_HOURS_MILLIS: i64 = 6 * 60 * 60 * 1_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FixedLongitudinalClockV1;

impl FixedLongitudinalClockV1 {
    pub fn new() -> Self {
        Self
    }

    pub fn received_at(self) -> &'static str {
        LONGITUDINAL_FIXED_INGEST_RECEIVED_AT_V1
    }

    pub fn occurred_at(
        self,
        block: u64,
        ordinal: u64,
        block_event_count: u64,
    ) -> Result<String, LongitudinalMaterializeError> {
        let global_ordinal = block
            .checked_mul(block_event_count)
            .and_then(|base| base.checked_add(ordinal))
            .ok_or(LongitudinalMaterializeError::TimestampOverflow)?;
        let mut offset_seconds = i64::try_from(global_ordinal)
            .map_err(|_| LongitudinalMaterializeError::TimestampOverflow)?;
        if global_ordinal % 8 == 1 {
            offset_seconds -= 1;
        }
        let mut millis = LONGITUDINAL_FIXED_EPOCH_MILLIS_V1
            .checked_add(
                offset_seconds
                    .checked_mul(1_000)
                    .ok_or(LongitudinalMaterializeError::TimestampOverflow)?,
            )
            .ok_or(LongitudinalMaterializeError::TimestampOverflow)?;
        if global_ordinal % 16 == 15 {
            millis = millis
                .checked_sub(SIX_HOURS_MILLIS)
                .ok_or(LongitudinalMaterializeError::TimestampOverflow)?;
        }
        Ok(format_rfc3339_utc_millis(millis))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LongitudinalMaterializeError {
    #[error("longitudinal timestamp derivation overflowed")]
    TimestampOverflow,
    #[error("longitudinal materialization contract is unavailable")]
    UnsupportedContract,
    #[error("longitudinal materialization requires the frozen public seed")]
    NonFrozenSeed,
    #[error("longitudinal materialization requires the frozen clock")]
    NonFrozenClock,
    #[error("longitudinal materialization failed: {0}")]
    Store(String),
    #[error("longitudinal receipt validation failed: {0}")]
    Contract(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongitudinalMaterializeOptionsV1 {
    pub root: PathBuf,
    pub tier: LongitudinalTierV1,
    pub execution: LongitudinalExecutionIdentityV1,
    pub public_seed_hex: String,
    pub clock_identity: String,
}

impl LongitudinalMaterializeOptionsV1 {
    pub fn new(
        root: impl AsRef<Path>,
        tier: LongitudinalTierV1,
        execution: LongitudinalExecutionIdentityV1,
    ) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            tier,
            execution,
            public_seed_hex: LONGITUDINAL_PUBLIC_SEED_HEX_V1.to_owned(),
            clock_identity: LONGITUDINAL_FIXED_CLOCK_IDENTITY_V1.to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongitudinalCapacityMaterializeOptionsV1 {
    pub root: PathBuf,
    pub profile: LongitudinalCapacityProfileV1,
    pub execution: LongitudinalExecutionIdentityV1,
    pub public_seed_hex: String,
    pub clock_identity: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppliedLongitudinalRemovalUpgradeV1 {
    pub(crate) changed_paths: Vec<LongitudinalRemovalUpgradePathV1>,
    pub(crate) enrollment_relative_path: String,
    pub(crate) enrollment_sha256: String,
    pub(crate) enrollment_bytes: u64,
    pub(crate) resumed_materialization: LongitudinalMaterializationResumeReceiptV1,
}

impl LongitudinalCapacityMaterializeOptionsV1 {
    pub fn new(
        root: impl AsRef<Path>,
        profile: LongitudinalCapacityProfileV1,
        execution: LongitudinalExecutionIdentityV1,
    ) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            profile,
            execution,
            public_seed_hex: LONGITUDINAL_PUBLIC_SEED_HEX_V1.to_owned(),
            clock_identity: LONGITUDINAL_FIXED_CLOCK_IDENTITY_V1.to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCarrierScanDiagnosticV1 {
    pub schema: String,
    pub subject: LongitudinalMaterializationSubjectV1,
    pub event_count: u64,
    pub signature_carrier_count: u64,
    pub target_lookup_full_scans: u64,
    pub repeated_target_lookup_full_scans: u64,
    pub diagnostic_sha256: String,
}

impl LongitudinalCarrierScanDiagnosticV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalMaterializeError> {
        let mut preimage = self.clone();
        preimage.diagnostic_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn validate(&self) -> Result<(), LongitudinalMaterializeError> {
        if self.schema != "pointbreak.longitudinal-carrier-scan-diagnostic.v1"
            || self.signature_carrier_count == 0
            || self.target_lookup_full_scans != 1
            || self.repeated_target_lookup_full_scans != 0
            || self.diagnostic_sha256 != self.canonical_sha256()?
        {
            return Err(LongitudinalMaterializeError::Contract(
                "carrier scan diagnostic is invalid".to_owned(),
            ));
        }
        Ok(())
    }
}

pub fn materialize_longitudinal_workload_v1(
    options: LongitudinalMaterializeOptionsV1,
) -> Result<LongitudinalMaterializationReceiptV1, LongitudinalMaterializeError> {
    validate_frozen_inputs(&options.public_seed_hex, &options.clock_identity)?;
    let contract = longitudinal_runner_contract_v1();
    let requirement = contract
        .tiers
        .iter()
        .find(|requirement| requirement.tier == options.tier)
        .ok_or(LongitudinalMaterializeError::UnsupportedContract)?;
    let records = (0..requirement.block_count)
        .map(|block| {
            prepare_longitudinal_record_v1(LongitudinalRecordSpecV1::new(
                LongitudinalRecordShapeV1::Workload,
                block,
            ))
            .map_err(store_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let write = write_longitudinal_records_v1(&options.root, &records).map_err(store_error)?;

    if write.events_created != write.event_count
        || write.events_existing != 0
        || write.event_count != requirement.event_count
        || write.revision_count != requirement.revision_count
        || write.body_fact_count != requirement.body_fact_count
        || write.external_body_count != requirement.external_body_count
        || write.object_artifact_count != requirement.object_artifact_count
        || write.decoded_body_bytes != requirement.decoded_body_bytes
        || write.decoded_object_target_bytes != requirement.decoded_object_target_bytes
    {
        return Err(LongitudinalMaterializeError::Store(
            "strict workload counts drifted from the frozen tier".to_owned(),
        ));
    }

    let by_type = contract
        .event_families
        .iter()
        .map(|family| LongitudinalEventFamilyCountV1 {
            event_type: family.event_type.clone(),
            count: write
                .by_type
                .get(&family.event_type)
                .copied()
                .unwrap_or_default(),
        })
        .collect();
    let schedule = contract.operation_schedule.clone();
    let schedule_sha256 = canonical_sha256(&schedule)?;
    let expected_semantic_receipts = schedule
        .iter()
        .copied()
        .map(|operation| {
            Ok(LongitudinalExpectedSemanticReceiptV1 {
                operation,
                semantic_receipt_sha256: canonical_sha256(&(
                    operation,
                    &write.strict,
                    &write.ordered_events,
                ))?,
            })
        })
        .collect::<Result<Vec<_>, LongitudinalMaterializeError>>()?;
    let mut manifest = LongitudinalWorkloadManifestV1 {
        schema: contract.schema,
        protocol: contract.protocol,
        contract_sha256: contract.contract_sha256,
        execution: options.execution,
        public_seed_hex: options.public_seed_hex,
        tier: options.tier,
        event_count: write.event_count,
        revision_count: write.revision_count,
        by_type,
        ordered_events: write.ordered_events,
        event_carriers: write.event_carriers,
        content_inventory: write.content_inventory,
        removed_content_sha256: write.removed_content_sha256,
        capacity_selectors: write.capacity_selectors,
        expected_semantic_receipts,
        schedule,
        schedule_sha256,
        manifest_sha256: String::new(),
    };
    manifest.manifest_sha256 = manifest.canonical_sha256().map_err(contract_error)?;
    manifest.validate().map_err(contract_error)?;

    let mut receipt = LongitudinalMaterializationReceiptV1 {
        schema: LONGITUDINAL_MATERIALIZATION_RECEIPT_SCHEMA_V1.to_owned(),
        root_identity: root_identity(&options.root)?,
        manifest,
        strict: write.strict,
        materialization_sha256: String::new(),
    };
    receipt.materialization_sha256 = receipt.canonical_sha256().map_err(contract_error)?;
    receipt.validate().map_err(contract_error)?;
    Ok(receipt)
}

pub fn diagnose_longitudinal_workload_carrier_scans_v1(
    options: LongitudinalMaterializeOptionsV1,
) -> Result<LongitudinalCarrierScanDiagnosticV1, LongitudinalMaterializeError> {
    let subject = LongitudinalMaterializationSubjectV1::Workload(options.tier);
    reset_carrier_target_full_scan_count();
    let receipt = materialize_longitudinal_workload_v1(options)?;
    let signature_carrier_count = receipt
        .manifest
        .by_type
        .iter()
        .find(|entry| entry.event_type == "event_signature_recorded")
        .map(|entry| entry.count)
        .ok_or_else(|| {
            LongitudinalMaterializeError::Contract(
                "workload signature-carrier count is absent".to_owned(),
            )
        })?;
    carrier_scan_diagnostic_v1(
        subject,
        receipt.manifest.event_count,
        signature_carrier_count,
    )
}

pub fn resume_longitudinal_workload_v1(
    options: LongitudinalMaterializeOptionsV1,
) -> Result<LongitudinalMaterializationResumeReceiptV1, LongitudinalMaterializeError> {
    let pre_inventory = store_data_inventory_v1(&options.root)?;
    let root = options.root.clone();
    let execution = options.execution.clone();
    let subject = LongitudinalMaterializationSubjectV1::Workload(options.tier);
    let result = resume_longitudinal_workload_inner_v1(options)?;
    let post_inventory = store_data_inventory_v1(&root)?;
    let mut receipt = LongitudinalMaterializationResumeReceiptV1 {
        schema: LONGITUDINAL_MATERIALIZATION_RESUME_RECEIPT_SCHEMA_V1.to_owned(),
        subject,
        execution,
        root_identity: result.receipt.root_identity.clone(),
        pre_inventory,
        post_inventory,
        events_created: result.counts.events_created,
        events_existing: result.counts.events_existing,
        event_count: result.receipt.manifest.event_count,
        content_count: result.receipt.manifest.content_inventory.len() as u64,
        strict: result.receipt.strict.clone(),
        materialization_sha256: result.receipt.materialization_sha256.clone(),
        materialization: LongitudinalResumedMaterializationV1::Workload(result.receipt),
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = receipt.canonical_sha256().map_err(contract_error)?;
    receipt.validate().map_err(contract_error)?;
    Ok(receipt)
}

pub(crate) fn apply_longitudinal_removal_upgrade_v1(
    root: &Path,
    tier: LongitudinalTierV1,
    corrected_execution: LongitudinalExecutionIdentityV1,
) -> Result<AppliedLongitudinalRemovalUpgradeV1, LongitudinalMaterializeError> {
    let requirement = longitudinal_runner_contract_v1()
        .tiers
        .into_iter()
        .find(|requirement| requirement.tier == tier)
        .ok_or(LongitudinalMaterializeError::UnsupportedContract)?;
    let write =
        upgrade_longitudinal_removals_v1(root, requirement.block_count).map_err(store_error)?;
    let resumed_materialization = resume_longitudinal_workload_v1(
        LongitudinalMaterializeOptionsV1::new(root, tier, corrected_execution),
    )?;
    Ok(AppliedLongitudinalRemovalUpgradeV1 {
        changed_paths: write
            .rewrites
            .into_iter()
            .map(|rewrite| LongitudinalRemovalUpgradePathV1 {
                relative_path: rewrite.relative_path,
                event_id: rewrite.event_id,
                event_record_hash: rewrite.event_record_hash,
                payload_hash: rewrite.payload_hash,
                before_sha256: rewrite.before_sha256,
                before_bytes: rewrite.before_bytes,
                after_sha256: rewrite.after_sha256,
                after_bytes: rewrite.after_bytes,
            })
            .collect(),
        enrollment_relative_path: write.enrollment_relative_path,
        enrollment_sha256: write.enrollment_sha256,
        enrollment_bytes: write.enrollment_bytes,
        resumed_materialization,
    })
}

struct ResumedWorkloadMaterializationV1 {
    receipt: LongitudinalMaterializationReceiptV1,
    counts: MaterializationWriteCountsV1,
}

#[derive(Clone, Copy)]
struct MaterializationWriteCountsV1 {
    events_created: u64,
    events_existing: u64,
}

fn resume_longitudinal_workload_inner_v1(
    options: LongitudinalMaterializeOptionsV1,
) -> Result<ResumedWorkloadMaterializationV1, LongitudinalMaterializeError> {
    validate_frozen_inputs(&options.public_seed_hex, &options.clock_identity)?;
    let contract = longitudinal_runner_contract_v1();
    let requirement = contract
        .tiers
        .iter()
        .find(|requirement| requirement.tier == options.tier)
        .ok_or(LongitudinalMaterializeError::UnsupportedContract)?;
    let records = (0..requirement.block_count)
        .map(|block| {
            prepare_longitudinal_record_v1(LongitudinalRecordSpecV1::new(
                LongitudinalRecordShapeV1::Workload,
                block,
            ))
            .map_err(store_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let write = write_longitudinal_records_v1(&options.root, &records).map_err(store_error)?;
    let counts = MaterializationWriteCountsV1 {
        events_created: write.events_created,
        events_existing: write.events_existing,
    };

    if write
        .events_created
        .checked_add(write.events_existing)
        .is_none_or(|count| count != requirement.event_count)
        || write.event_count != requirement.event_count
        || write.revision_count != requirement.revision_count
        || write.body_fact_count != requirement.body_fact_count
        || write.external_body_count != requirement.external_body_count
        || write.object_artifact_count != requirement.object_artifact_count
        || write.decoded_body_bytes != requirement.decoded_body_bytes
        || write.decoded_object_target_bytes != requirement.decoded_object_target_bytes
    {
        return Err(LongitudinalMaterializeError::Store(
            "workload counts drifted from the frozen tier".to_owned(),
        ));
    }

    let by_type = contract
        .event_families
        .iter()
        .map(|family| LongitudinalEventFamilyCountV1 {
            event_type: family.event_type.clone(),
            count: write
                .by_type
                .get(&family.event_type)
                .copied()
                .unwrap_or_default(),
        })
        .collect();
    let schedule = contract.operation_schedule.clone();
    let schedule_sha256 = canonical_sha256(&schedule)?;
    let expected_semantic_receipts = schedule
        .iter()
        .copied()
        .map(|operation| {
            Ok(LongitudinalExpectedSemanticReceiptV1 {
                operation,
                semantic_receipt_sha256: canonical_sha256(&(
                    operation,
                    &write.strict,
                    &write.ordered_events,
                ))?,
            })
        })
        .collect::<Result<Vec<_>, LongitudinalMaterializeError>>()?;
    let mut manifest = LongitudinalWorkloadManifestV1 {
        schema: contract.schema,
        protocol: contract.protocol,
        contract_sha256: contract.contract_sha256,
        execution: options.execution,
        public_seed_hex: options.public_seed_hex,
        tier: options.tier,
        event_count: write.event_count,
        revision_count: write.revision_count,
        by_type,
        ordered_events: write.ordered_events,
        event_carriers: write.event_carriers,
        content_inventory: write.content_inventory,
        removed_content_sha256: write.removed_content_sha256,
        capacity_selectors: write.capacity_selectors,
        expected_semantic_receipts,
        schedule,
        schedule_sha256,
        manifest_sha256: String::new(),
    };
    manifest.manifest_sha256 = manifest.canonical_sha256().map_err(contract_error)?;
    manifest.validate().map_err(contract_error)?;

    let mut receipt = LongitudinalMaterializationReceiptV1 {
        schema: LONGITUDINAL_MATERIALIZATION_RECEIPT_SCHEMA_V1.to_owned(),
        root_identity: root_identity(&options.root)?,
        manifest,
        strict: write.strict,
        materialization_sha256: String::new(),
    };
    receipt.materialization_sha256 = receipt.canonical_sha256().map_err(contract_error)?;
    receipt.validate().map_err(contract_error)?;
    Ok(ResumedWorkloadMaterializationV1 { receipt, counts })
}

pub fn materialize_longitudinal_capacity_v1(
    options: LongitudinalCapacityMaterializeOptionsV1,
) -> Result<LongitudinalCapacityMaterializationReceiptV1, LongitudinalMaterializeError> {
    validate_frozen_inputs(&options.public_seed_hex, &options.clock_identity)?;
    let contract = longitudinal_capacity_contract_v1();
    let requirement = contract
        .profiles
        .iter()
        .find(|requirement| requirement.profile == options.profile)
        .ok_or(LongitudinalMaterializeError::UnsupportedContract)?;
    let (shape, block_count) = match options.profile {
        LongitudinalCapacityProfileV1::L100O10K => {
            (LongitudinalRecordShapeV1::CapacityL100O10K, 100)
        }
        LongitudinalCapacityProfileV1::C262 => (LongitudinalRecordShapeV1::CapacityV1, 1_024),
        LongitudinalCapacityProfileV1::C524 => (LongitudinalRecordShapeV1::CapacityV1, 2_048),
    };
    let records = (0..block_count)
        .map(|block| {
            prepare_longitudinal_record_v1(LongitudinalRecordSpecV1::new(shape, block))
                .map_err(store_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let write = write_longitudinal_records_v1(&options.root, &records).map_err(store_error)?;
    if write.events_created != write.event_count
        || write.events_existing != 0
        || write.event_count != requirement.event_count
        || write.revision_count != requirement.revision_count
        || write.object_artifact_count != requirement.object_artifact_count
        || write.task_attempt_count != requirement.task_attempt_count
        || write.body_fact_count != requirement.body_fact_count
        || write.external_body_count != requirement.external_body_count
        || write.decoded_body_bytes != requirement.decoded_body_bytes
        || write.decoded_object_target_bytes != requirement.decoded_object_target_bytes
    {
        return Err(LongitudinalMaterializeError::Store(
            "strict capacity counts drifted from the frozen profile".to_owned(),
        ));
    }

    let probe_schedule = contract.probes.clone();
    let schedule_sha256 = canonical_sha256(&probe_schedule)?;
    let mut manifest = LongitudinalCapacityManifestV1 {
        schema: contract.schema,
        contract_sha256: contract.contract_sha256,
        execution: options.execution,
        public_seed_hex: options.public_seed_hex,
        subject: LongitudinalCapacitySubjectV1::Companion(options.profile),
        event_count: write.event_count,
        revision_count: write.revision_count,
        object_artifact_count: write.object_artifact_count,
        task_attempt_count: write.task_attempt_count,
        body_fact_count: write.body_fact_count,
        external_body_count: write.external_body_count,
        decoded_body_bytes: write.decoded_body_bytes,
        decoded_object_target_bytes: write.decoded_object_target_bytes,
        ordered_events: write.ordered_events,
        event_carriers: write.event_carriers,
        content_inventory: write.content_inventory,
        removed_content_sha256: write.removed_content_sha256,
        selectors: write.capacity_selectors,
        probe_schedule,
        schedule_sha256,
        manifest_sha256: String::new(),
    };
    manifest.manifest_sha256 = manifest.canonical_sha256().map_err(contract_error)?;
    manifest.validate().map_err(contract_error)?;

    let mut receipt = LongitudinalCapacityMaterializationReceiptV1 {
        schema: LONGITUDINAL_CAPACITY_MATERIALIZATION_RECEIPT_SCHEMA_V1.to_owned(),
        root_identity: root_identity(&options.root)?,
        manifest,
        strict: write.strict,
        materialization_sha256: String::new(),
    };
    receipt.materialization_sha256 = receipt.canonical_sha256().map_err(contract_error)?;
    receipt.validate().map_err(contract_error)?;
    Ok(receipt)
}

pub fn diagnose_longitudinal_capacity_carrier_scans_v1(
    options: LongitudinalCapacityMaterializeOptionsV1,
) -> Result<LongitudinalCarrierScanDiagnosticV1, LongitudinalMaterializeError> {
    let profile = options.profile;
    let root = options.root.clone();
    let subject = LongitudinalMaterializationSubjectV1::Capacity(profile);
    reset_carrier_target_full_scan_count();
    let receipt = materialize_longitudinal_capacity_v1(options)?;
    let signature_carrier_count = crate::session::read_events(&root)
        .map_err(store_error)?
        .into_iter()
        .filter(|event| event.event_type.as_str() == "event_signature_recorded")
        .count() as u64;
    carrier_scan_diagnostic_v1(
        subject,
        receipt.manifest.event_count,
        signature_carrier_count,
    )
}

fn carrier_scan_diagnostic_v1(
    subject: LongitudinalMaterializationSubjectV1,
    event_count: u64,
    signature_carrier_count: u64,
) -> Result<LongitudinalCarrierScanDiagnosticV1, LongitudinalMaterializeError> {
    let target_lookup_full_scans = carrier_target_full_scan_count();
    let mut diagnostic = LongitudinalCarrierScanDiagnosticV1 {
        schema: "pointbreak.longitudinal-carrier-scan-diagnostic.v1".to_owned(),
        subject,
        event_count,
        signature_carrier_count,
        target_lookup_full_scans,
        repeated_target_lookup_full_scans: target_lookup_full_scans.saturating_sub(1),
        diagnostic_sha256: String::new(),
    };
    diagnostic.diagnostic_sha256 = diagnostic.canonical_sha256()?;
    diagnostic.validate()?;
    Ok(diagnostic)
}

pub fn resume_longitudinal_capacity_v1(
    options: LongitudinalCapacityMaterializeOptionsV1,
) -> Result<LongitudinalMaterializationResumeReceiptV1, LongitudinalMaterializeError> {
    let pre_inventory = store_data_inventory_v1(&options.root)?;
    let root = options.root.clone();
    let execution = options.execution.clone();
    let subject = LongitudinalMaterializationSubjectV1::Capacity(options.profile);
    let result = resume_longitudinal_capacity_inner_v1(options)?;
    let post_inventory = store_data_inventory_v1(&root)?;
    let mut receipt = LongitudinalMaterializationResumeReceiptV1 {
        schema: LONGITUDINAL_MATERIALIZATION_RESUME_RECEIPT_SCHEMA_V1.to_owned(),
        subject,
        execution,
        root_identity: result.receipt.root_identity.clone(),
        pre_inventory,
        post_inventory,
        events_created: result.counts.events_created,
        events_existing: result.counts.events_existing,
        event_count: result.receipt.manifest.event_count,
        content_count: result.receipt.manifest.content_inventory.len() as u64,
        strict: result.receipt.strict.clone(),
        materialization_sha256: result.receipt.materialization_sha256.clone(),
        materialization: LongitudinalResumedMaterializationV1::Capacity(result.receipt),
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = receipt.canonical_sha256().map_err(contract_error)?;
    receipt.validate().map_err(contract_error)?;
    Ok(receipt)
}

struct ResumedCapacityMaterializationV1 {
    receipt: LongitudinalCapacityMaterializationReceiptV1,
    counts: MaterializationWriteCountsV1,
}

fn resume_longitudinal_capacity_inner_v1(
    options: LongitudinalCapacityMaterializeOptionsV1,
) -> Result<ResumedCapacityMaterializationV1, LongitudinalMaterializeError> {
    validate_frozen_inputs(&options.public_seed_hex, &options.clock_identity)?;
    let contract = longitudinal_capacity_contract_v1();
    let requirement = contract
        .profiles
        .iter()
        .find(|requirement| requirement.profile == options.profile)
        .ok_or(LongitudinalMaterializeError::UnsupportedContract)?;
    let (shape, block_count) = match options.profile {
        LongitudinalCapacityProfileV1::L100O10K => {
            (LongitudinalRecordShapeV1::CapacityL100O10K, 100)
        }
        LongitudinalCapacityProfileV1::C262 => (LongitudinalRecordShapeV1::CapacityV1, 1_024),
        LongitudinalCapacityProfileV1::C524 => (LongitudinalRecordShapeV1::CapacityV1, 2_048),
    };
    let records = (0..block_count)
        .map(|block| {
            prepare_longitudinal_record_v1(LongitudinalRecordSpecV1::new(shape, block))
                .map_err(store_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let write = write_longitudinal_records_v1(&options.root, &records).map_err(store_error)?;
    let counts = MaterializationWriteCountsV1 {
        events_created: write.events_created,
        events_existing: write.events_existing,
    };
    if write
        .events_created
        .checked_add(write.events_existing)
        .is_none_or(|count| count != requirement.event_count)
        || write.event_count != requirement.event_count
        || write.revision_count != requirement.revision_count
        || write.object_artifact_count != requirement.object_artifact_count
        || write.task_attempt_count != requirement.task_attempt_count
        || write.body_fact_count != requirement.body_fact_count
        || write.external_body_count != requirement.external_body_count
        || write.decoded_body_bytes != requirement.decoded_body_bytes
        || write.decoded_object_target_bytes != requirement.decoded_object_target_bytes
    {
        return Err(LongitudinalMaterializeError::Store(
            "capacity counts drifted from the frozen profile".to_owned(),
        ));
    }

    let probe_schedule = contract.probes.clone();
    let schedule_sha256 = canonical_sha256(&probe_schedule)?;
    let mut manifest = LongitudinalCapacityManifestV1 {
        schema: contract.schema,
        contract_sha256: contract.contract_sha256,
        execution: options.execution,
        public_seed_hex: options.public_seed_hex,
        subject: LongitudinalCapacitySubjectV1::Companion(options.profile),
        event_count: write.event_count,
        revision_count: write.revision_count,
        object_artifact_count: write.object_artifact_count,
        task_attempt_count: write.task_attempt_count,
        body_fact_count: write.body_fact_count,
        external_body_count: write.external_body_count,
        decoded_body_bytes: write.decoded_body_bytes,
        decoded_object_target_bytes: write.decoded_object_target_bytes,
        ordered_events: write.ordered_events,
        event_carriers: write.event_carriers,
        content_inventory: write.content_inventory,
        removed_content_sha256: write.removed_content_sha256,
        selectors: write.capacity_selectors,
        probe_schedule,
        schedule_sha256,
        manifest_sha256: String::new(),
    };
    manifest.manifest_sha256 = manifest.canonical_sha256().map_err(contract_error)?;
    manifest.validate().map_err(contract_error)?;

    let mut receipt = LongitudinalCapacityMaterializationReceiptV1 {
        schema: LONGITUDINAL_CAPACITY_MATERIALIZATION_RECEIPT_SCHEMA_V1.to_owned(),
        root_identity: root_identity(&options.root)?,
        manifest,
        strict: write.strict,
        materialization_sha256: String::new(),
    };
    receipt.materialization_sha256 = receipt.canonical_sha256().map_err(contract_error)?;
    receipt.validate().map_err(contract_error)?;
    Ok(ResumedCapacityMaterializationV1 { receipt, counts })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoreDataFileV1 {
    relative_path: String,
    bytes: u64,
    sha256: String,
}

pub fn longitudinal_store_data_inventory_v1(
    root: &Path,
) -> Result<LongitudinalStoreDataInventoryV1, LongitudinalMaterializeError> {
    store_data_inventory_v1(root)
}

fn store_data_inventory_v1(
    root: &Path,
) -> Result<LongitudinalStoreDataInventoryV1, LongitudinalMaterializeError> {
    let store = store_dir_for_repo(root).map_err(store_error)?;
    let mut files = Vec::new();
    if store.exists() {
        collect_store_data_files_v1(&store, &store, &mut files)?;
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let byte_count = files.iter().try_fold(0_u64, |total, file| {
        total.checked_add(file.bytes).ok_or_else(|| {
            LongitudinalMaterializeError::Store("store byte count overflowed".into())
        })
    })?;
    let inventory_sha256 = canonical_sha256(&files)?;
    Ok(LongitudinalStoreDataInventoryV1 {
        file_count: files.len() as u64,
        byte_count,
        inventory_sha256,
    })
}

fn collect_store_data_files_v1(
    store: &Path,
    directory: &Path,
    files: &mut Vec<StoreDataFileV1>,
) -> Result<(), LongitudinalMaterializeError> {
    let entries = fs::read_dir(directory).map_err(|error| {
        LongitudinalMaterializeError::Store(format!(
            "cannot inventory store directory {}: {error}",
            directory.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            LongitudinalMaterializeError::Store(format!(
                "cannot read store directory entry in {}: {error}",
                directory.display()
            ))
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            LongitudinalMaterializeError::Store(format!(
                "cannot inspect store-data path {}: {error}",
                path.display()
            ))
        })?;
        if metadata.is_dir() {
            collect_store_data_files_v1(store, &path, files)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(LongitudinalMaterializeError::Store(format!(
                "store-data inventory rejects non-file path {}",
                path.display()
            )));
        }
        let relative_path = path
            .strip_prefix(store)
            .map_err(|_| {
                LongitudinalMaterializeError::Store(
                    "store-data inventory path escaped its root".to_owned(),
                )
            })?
            .to_str()
            .ok_or_else(|| {
                LongitudinalMaterializeError::Store(
                    "store-data inventory path is not UTF-8".to_owned(),
                )
            })?
            .replace('\\', "/");
        let bytes = fs::read(&path).map_err(|error| {
            LongitudinalMaterializeError::Store(format!(
                "cannot read store-data file {}: {error}",
                path.display()
            ))
        })?;
        files.push(StoreDataFileV1 {
            relative_path,
            bytes: bytes.len() as u64,
            sha256: sha256_bytes_hex(&bytes),
        });
    }
    Ok(())
}

pub fn verify_longitudinal_materializer_equivalence_v1(
    baseline_root: &Path,
    baseline: &LongitudinalMaterializationReceiptV1,
    successor_root: &Path,
    successor: &LongitudinalMaterializationReceiptV1,
    implementation_diff_sha256: String,
) -> Result<LongitudinalMaterializerEquivalenceReceiptV1, LongitudinalMaterializeError> {
    baseline.validate().map_err(contract_error)?;
    successor.validate().map_err(contract_error)?;
    let baseline = workload_equivalence_root_v1(baseline_root, baseline)?;
    let successor = workload_equivalence_root_v1(successor_root, successor)?;
    equivalence_receipt_v1(baseline, successor, implementation_diff_sha256)
}

pub fn verify_longitudinal_capacity_materializer_equivalence_v1(
    baseline_root: &Path,
    baseline: &LongitudinalCapacityMaterializationReceiptV1,
    successor_root: &Path,
    successor: &LongitudinalCapacityMaterializationReceiptV1,
    implementation_diff_sha256: String,
) -> Result<LongitudinalMaterializerEquivalenceReceiptV1, LongitudinalMaterializeError> {
    baseline.validate().map_err(contract_error)?;
    successor.validate().map_err(contract_error)?;
    let baseline = capacity_equivalence_root_v1(baseline_root, baseline)?;
    let successor = capacity_equivalence_root_v1(successor_root, successor)?;
    equivalence_receipt_v1(baseline, successor, implementation_diff_sha256)
}

fn workload_equivalence_root_v1(
    root: &Path,
    receipt: &LongitudinalMaterializationReceiptV1,
) -> Result<LongitudinalMaterializerRootReceiptV1, LongitudinalMaterializeError> {
    let actual_root_identity = root_identity(root)?;
    if receipt.root_identity != actual_root_identity {
        return Err(LongitudinalMaterializeError::Contract(
            "workload receipt does not describe the supplied root".to_owned(),
        ));
    }
    Ok(LongitudinalMaterializerRootReceiptV1 {
        subject: LongitudinalMaterializationSubjectV1::Workload(receipt.manifest.tier),
        execution: receipt.manifest.execution.clone(),
        root_identity: actual_root_identity,
        inventory: store_data_inventory_v1(root)?,
        event_count: receipt.manifest.event_count,
        content_count: receipt.manifest.content_inventory.len() as u64,
        strict: receipt.strict.clone(),
        materialization_sha256: receipt.materialization_sha256.clone(),
    })
}

fn capacity_equivalence_root_v1(
    root: &Path,
    receipt: &LongitudinalCapacityMaterializationReceiptV1,
) -> Result<LongitudinalMaterializerRootReceiptV1, LongitudinalMaterializeError> {
    let actual_root_identity = root_identity(root)?;
    if receipt.root_identity != actual_root_identity {
        return Err(LongitudinalMaterializeError::Contract(
            "capacity receipt does not describe the supplied root".to_owned(),
        ));
    }
    let LongitudinalCapacitySubjectV1::Companion(profile) = receipt.manifest.subject else {
        return Err(LongitudinalMaterializeError::UnsupportedContract);
    };
    Ok(LongitudinalMaterializerRootReceiptV1 {
        subject: LongitudinalMaterializationSubjectV1::Capacity(profile),
        execution: receipt.manifest.execution.clone(),
        root_identity: actual_root_identity,
        inventory: store_data_inventory_v1(root)?,
        event_count: receipt.manifest.event_count,
        content_count: receipt.manifest.content_inventory.len() as u64,
        strict: receipt.strict.clone(),
        materialization_sha256: receipt.materialization_sha256.clone(),
    })
}

fn equivalence_receipt_v1(
    baseline: LongitudinalMaterializerRootReceiptV1,
    successor: LongitudinalMaterializerRootReceiptV1,
    implementation_diff_sha256: String,
) -> Result<LongitudinalMaterializerEquivalenceReceiptV1, LongitudinalMaterializeError> {
    let equivalent = baseline.subject == successor.subject
        && baseline.event_count == successor.event_count
        && baseline.content_count == successor.content_count
        && baseline.strict == successor.strict
        && baseline.inventory == successor.inventory;
    let mut receipt = LongitudinalMaterializerEquivalenceReceiptV1 {
        schema: LONGITUDINAL_MATERIALIZER_EQUIVALENCE_RECEIPT_SCHEMA_V1.to_owned(),
        baseline,
        successor,
        implementation_diff_sha256,
        equivalent,
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = receipt.canonical_sha256().map_err(contract_error)?;
    receipt.validate().map_err(contract_error)?;
    Ok(receipt)
}

fn validate_frozen_inputs(
    public_seed_hex: &str,
    clock_identity: &str,
) -> Result<(), LongitudinalMaterializeError> {
    if public_seed_hex != LONGITUDINAL_PUBLIC_SEED_HEX_V1 {
        return Err(LongitudinalMaterializeError::NonFrozenSeed);
    }
    if clock_identity != LONGITUDINAL_FIXED_CLOCK_IDENTITY_V1 {
        return Err(LongitudinalMaterializeError::NonFrozenClock);
    }
    Ok(())
}

fn root_identity(root: &Path) -> Result<String, LongitudinalMaterializeError> {
    let root = std::fs::canonicalize(root).map_err(|error| {
        LongitudinalMaterializeError::Store(format!(
            "cannot resolve materialization root {}: {error}",
            root.display()
        ))
    })?;
    Ok(sha256_bytes_hex(root.as_os_str().as_encoded_bytes()))
}

fn canonical_sha256<T: serde::Serialize>(
    value: &T,
) -> Result<String, LongitudinalMaterializeError> {
    let value = serde_json::to_value(value)
        .map_err(|error| LongitudinalMaterializeError::Contract(error.to_string()))?;
    let bytes = canonical_json_bytes(&value)
        .map_err(|error| LongitudinalMaterializeError::Contract(error.to_string()))?;
    Ok(sha256_bytes_hex(&bytes))
}

fn store_error(error: crate::error::ShoreError) -> LongitudinalMaterializeError {
    LongitudinalMaterializeError::Store(error.to_string())
}

fn contract_error(
    error: crate::bench_support::longitudinal::LongitudinalContractError,
) -> LongitudinalMaterializeError {
    LongitudinalMaterializeError::Contract(error.to_string())
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct LongitudinalGenerationSummaryV1 {
    event_count: u64,
    revision_count: u64,
    task_attempt_count: u64,
    body_fact_count: u64,
    external_body_count: u64,
    object_artifact_count: u64,
    validation_log_count: u64,
    removed_content_count: u64,
    decoded_body_bytes: u64,
    decoded_object_target_bytes: u64,
    by_type: Vec<LongitudinalEventFamilyCountV1>,
}

#[cfg(test)]
fn generated_v1_block_summary_v1()
-> Result<LongitudinalGenerationSummaryV1, LongitudinalMaterializeError> {
    let contract = longitudinal_runner_contract_v1();
    let l1 = contract
        .tiers
        .iter()
        .find(|requirement| requirement.tier == LongitudinalTierV1::L1)
        .ok_or(LongitudinalMaterializeError::UnsupportedContract)?;
    Ok(LongitudinalGenerationSummaryV1 {
        event_count: contract.block_event_count,
        revision_count: l1.revision_count / l1.block_count,
        task_attempt_count: l1.task_attempt_count / l1.block_count,
        body_fact_count: l1.body_fact_count / l1.block_count,
        external_body_count: l1.external_body_count / l1.block_count,
        object_artifact_count: l1.object_artifact_count / l1.block_count,
        validation_log_count: l1.validation_log_count / l1.block_count,
        removed_content_count: l1.removed_content_count / l1.block_count,
        decoded_body_bytes: l1.decoded_body_bytes / l1.block_count,
        decoded_object_target_bytes: l1.decoded_object_target_bytes / l1.block_count,
        by_type: contract
            .event_families
            .into_iter()
            .map(|family| LongitudinalEventFamilyCountV1 {
                event_type: family.event_type,
                count: family.per_block,
            })
            .collect(),
    })
}

#[cfg(test)]
fn generated_v1_plan_summary_v1(
    tier: LongitudinalTierV1,
) -> Result<LongitudinalGenerationSummaryV1, LongitudinalMaterializeError> {
    let contract = longitudinal_runner_contract_v1();
    let requirement = contract
        .tiers
        .into_iter()
        .find(|requirement| requirement.tier == tier)
        .ok_or(LongitudinalMaterializeError::UnsupportedContract)?;
    Ok(LongitudinalGenerationSummaryV1 {
        event_count: requirement.event_count,
        revision_count: requirement.revision_count,
        task_attempt_count: requirement.task_attempt_count,
        body_fact_count: requirement.body_fact_count,
        external_body_count: requirement.external_body_count,
        object_artifact_count: requirement.object_artifact_count,
        validation_log_count: requirement.validation_log_count,
        removed_content_count: requirement.removed_content_count,
        decoded_body_bytes: requirement.decoded_body_bytes,
        decoded_object_target_bytes: requirement.decoded_object_target_bytes,
        by_type: Vec::new(),
    })
}

#[cfg(test)]
fn generated_capacity_plan_summary_v1(
    profile: LongitudinalCapacityProfileV1,
) -> Result<LongitudinalGenerationSummaryV1, LongitudinalMaterializeError> {
    let requirement = longitudinal_capacity_contract_v1()
        .profiles
        .into_iter()
        .find(|requirement| requirement.profile == profile)
        .ok_or(LongitudinalMaterializeError::UnsupportedContract)?;
    Ok(LongitudinalGenerationSummaryV1 {
        event_count: requirement.event_count,
        revision_count: requirement.revision_count,
        task_attempt_count: requirement.task_attempt_count,
        body_fact_count: requirement.body_fact_count,
        external_body_count: requirement.external_body_count,
        object_artifact_count: requirement.object_artifact_count,
        validation_log_count: requirement.validation_log_count,
        removed_content_count: requirement.removed_content_count,
        decoded_body_bytes: requirement.decoded_body_bytes,
        decoded_object_target_bytes: requirement.decoded_object_target_bytes,
        by_type: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::process::Command;

    use super::*;
    use crate::bench_support::longitudinal::{
        LongitudinalCapacityProfileV1, LongitudinalExecutionIdentityV1, LongitudinalTierV1,
        longitudinal_capacity_contract_v1, longitudinal_runner_contract_v1,
        verify_longitudinal_materialization_pair_v1,
    };

    #[test]
    fn longitudinal_materialize_fixed_clock_is_path_independent_and_exercises_ordering_edges() {
        let clock = FixedLongitudinalClockV1::new();

        assert_eq!(
            clock.received_at(),
            LONGITUDINAL_FIXED_INGEST_RECEIVED_AT_V1
        );
        assert_eq!(
            clock.occurred_at(0, 0, 256).unwrap(),
            LONGITUDINAL_FIXED_EPOCH_V1
        );
        assert_eq!(
            clock.occurred_at(0, 1, 256).unwrap(),
            clock.occurred_at(0, 0, 256).unwrap(),
            "the first adjacent pair in each eight-event cohort ties"
        );
        assert!(
            clock.occurred_at(0, 15, 256).unwrap() < clock.occurred_at(0, 14, 256).unwrap(),
            "one event in sixteen is backdated by six hours"
        );
        assert_eq!(
            clock.occurred_at(1, 0, 256).unwrap(),
            clock.occurred_at(0, 256, 256).unwrap(),
            "block and global ordinal derivations agree"
        );
    }

    #[test]
    fn longitudinal_materialize_v1_block_matches_the_frozen_family_and_content_mix() {
        let summary = generated_v1_block_summary_v1().unwrap();
        let by_type = summary
            .by_type
            .into_iter()
            .map(|entry| (entry.event_type, entry.count))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(summary.event_count, 256);
        assert_eq!(summary.revision_count, 12);
        assert_eq!(summary.task_attempt_count, 4);
        assert_eq!(summary.body_fact_count, 180);
        assert_eq!(summary.external_body_count, 90);
        assert_eq!(summary.object_artifact_count, 12);
        assert_eq!(summary.validation_log_count, 5);
        assert_eq!(summary.removed_content_count, 3);
        assert_eq!(summary.decoded_body_bytes, 1_474_560);
        assert_eq!(summary.decoded_object_target_bytes, 786_432);
        assert_eq!(by_type["work_object_proposed"], 16);
        assert_eq!(by_type["review_observation_recorded"], 64);
        assert_eq!(by_type["review_assessment_recorded"], 24);
        assert_eq!(by_type["input_request_opened"], 24);
        assert_eq!(by_type["input_request_responded"], 16);
        assert_eq!(by_type["validation_check_recorded"], 40);
        assert_eq!(by_type["event_signature_recorded"], 8);
        assert_eq!(by_type["artifact_removed"], 3);
    }

    #[test]
    fn longitudinal_materialize_profile_plans_match_every_frozen_scale() {
        for tier in LongitudinalTierV1::ALL {
            let plan = generated_v1_plan_summary_v1(tier).unwrap();
            let expected = longitudinal_runner_contract_v1()
                .tiers
                .into_iter()
                .find(|requirement| requirement.tier == tier)
                .unwrap();
            assert_eq!(plan.event_count, expected.event_count);
            assert_eq!(plan.revision_count, expected.revision_count);
            assert_eq!(plan.body_fact_count, expected.body_fact_count);
            assert_eq!(
                plan.decoded_object_target_bytes,
                expected.decoded_object_target_bytes
            );
        }

        for profile in LongitudinalCapacityProfileV1::ALL {
            let plan = generated_capacity_plan_summary_v1(profile).unwrap();
            let expected = longitudinal_capacity_contract_v1()
                .profiles
                .into_iter()
                .find(|requirement| requirement.profile == profile)
                .unwrap();
            assert_eq!(plan.event_count, expected.event_count);
            assert_eq!(plan.revision_count, expected.revision_count);
            assert_eq!(plan.object_artifact_count, expected.object_artifact_count);
            assert_eq!(plan.body_fact_count, expected.body_fact_count);
            assert_eq!(plan.external_body_count, expected.external_body_count);
            assert_eq!(plan.decoded_body_bytes, expected.decoded_body_bytes);
            assert_eq!(
                plan.decoded_object_target_bytes,
                expected.decoded_object_target_bytes
            );
        }
    }

    #[test]
    fn longitudinal_materialize_two_l1_roots_are_byte_and_semantic_identical() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        init_repo(left.path());
        init_repo(right.path());

        let left_receipt =
            materialize_longitudinal_workload_v1(LongitudinalMaterializeOptionsV1::new(
                left.path(),
                LongitudinalTierV1::L1,
                execution_identity(),
            ))
            .unwrap();
        reset_carrier_target_full_scan_count();
        let right_receipt =
            materialize_longitudinal_workload_v1(LongitudinalMaterializeOptionsV1::new(
                right.path(),
                LongitudinalTierV1::L1,
                execution_identity(),
            ))
            .unwrap();
        let scan_diagnostic = carrier_scan_diagnostic_v1(
            LongitudinalMaterializationSubjectV1::Workload(LongitudinalTierV1::L1),
            right_receipt.manifest.event_count,
            32,
        )
        .unwrap();
        assert_eq!(scan_diagnostic.target_lookup_full_scans, 1);
        assert_eq!(scan_diagnostic.repeated_target_lookup_full_scans, 0);

        verify_longitudinal_materialization_pair_v1(&left_receipt, &right_receipt).unwrap();
        assert_ne!(left_receipt.root_identity, right_receipt.root_identity);
        assert_eq!(left_receipt.manifest.event_count, 1_024);
        assert_eq!(left_receipt.manifest.revision_count, 48);
        assert_eq!(left_receipt.manifest.content_inventory.len(), 416);
        assert_eq!(left_receipt.manifest.removed_content_sha256.len(), 12);
        assert_eq!(left_receipt.strict, right_receipt.strict);

        let mut successor_receipt = right_receipt.clone();
        successor_receipt.manifest.execution.source_commit = "5".repeat(40);
        successor_receipt.manifest.manifest_sha256 =
            successor_receipt.manifest.canonical_sha256().unwrap();
        successor_receipt.materialization_sha256 = successor_receipt.canonical_sha256().unwrap();
        successor_receipt.validate().unwrap();
        let equivalence = verify_longitudinal_materializer_equivalence_v1(
            left.path(),
            &left_receipt,
            right.path(),
            &successor_receipt,
            sha256_bytes_hex(b"optimized implementation diff"),
        )
        .unwrap();
        assert!(equivalence.equivalent);
        assert_ne!(
            equivalence.baseline.execution.source_commit,
            equivalence.successor.execution.source_commit,
            "source identities are bound but excluded from store-data equality"
        );

        let complete_resume =
            resume_longitudinal_workload_v1(LongitudinalMaterializeOptionsV1::new(
                right.path(),
                LongitudinalTierV1::L1,
                execution_identity(),
            ))
            .unwrap();
        assert_eq!(complete_resume.events_created, 0);
        assert_eq!(complete_resume.events_existing, 1_024);
        assert_eq!(
            complete_resume.pre_inventory, complete_resume.post_inventory,
            "idempotent replay must preserve every store-data byte"
        );
        assert!(
            materialize_longitudinal_workload_v1(LongitudinalMaterializeOptionsV1::new(
                right.path(),
                LongitudinalTierV1::L1,
                execution_identity(),
            ))
            .is_err()
        );

        let store = store_dir_for_repo(right.path()).unwrap();
        let event_path = fs::read_dir(store.join("events"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::remove_file(&event_path).unwrap();
        let interrupted_resume =
            resume_longitudinal_workload_v1(LongitudinalMaterializeOptionsV1::new(
                right.path(),
                LongitudinalTierV1::L1,
                execution_identity(),
            ))
            .unwrap();
        assert_eq!(interrupted_resume.events_created, 1);
        assert_eq!(interrupted_resume.events_existing, 1_023);
        assert_eq!(
            interrupted_resume.post_inventory,
            complete_resume.post_inventory
        );

        let valid_event_bytes = fs::read(&event_path).unwrap();
        fs::write(&event_path, b"{}").unwrap();
        assert!(
            resume_longitudinal_workload_v1(LongitudinalMaterializeOptionsV1::new(
                right.path(),
                LongitudinalTierV1::L1,
                execution_identity(),
            ))
            .is_err(),
            "a corrupt existing event must fail closed"
        );
        fs::write(&event_path, valid_event_bytes).unwrap();

        for root in [left.path(), right.path()] {
            let events = crate::session::read_events(root).unwrap();
            assert_eq!(events.len(), 1_024);
            assert!(events.iter().all(|event| {
                event.ingest.as_ref().is_some_and(|ingest| {
                    ingest.via == crate::session::IngestVia::IngestEvents
                        && ingest.received_at == LONGITUDINAL_FIXED_INGEST_RECEIVED_AT_V1
                })
            }));
        }
    }

    #[test]
    fn longitudinal_materialize_enrolls_trusted_removals_for_absent_content() {
        use crate::bench_support::EventType;
        use crate::crypto::EventVerificationStatus;
        use crate::session::{
            BaseProjectionConfig, TrustSet, allowed_signers_path_for_repo, history_base_projection,
            read_events, verify_event_signature,
        };

        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path());
        let receipt = materialize_longitudinal_workload_v1(LongitudinalMaterializeOptionsV1::new(
            repo.path(),
            LongitudinalTierV1::L1,
            execution_identity(),
        ))
        .unwrap();

        let enrollment_path = allowed_signers_path_for_repo(repo.path()).unwrap();
        assert!(
            enrollment_path.is_file(),
            "fresh materialization must persist reader-visible trust enrollment"
        );
        let trust = TrustSet::from_allowed_signers_file(&enrollment_path).unwrap();
        let events = read_events(repo.path()).unwrap();
        let removals = events
            .iter()
            .filter(|event| event.event_type == EventType::ArtifactRemoved)
            .collect::<Vec<_>>();
        assert_eq!(
            removals.len(),
            receipt.manifest.removed_content_sha256.len()
        );
        for event in removals {
            assert_eq!(
                verify_event_signature(event, &trust).unwrap(),
                EventVerificationStatus::Valid,
                "every absent generated content target must have a reader-trusted removal"
            );
        }
        history_base_projection(
            repo.path(),
            &BaseProjectionConfig {
                trust_set: trust,
                ..BaseProjectionConfig::default()
            },
        )
        .expect("trusted generated removals keep fully hydrated history readable");
    }

    #[test]
    fn longitudinal_capacity_resume_rejects_non_public_inputs_before_generation() {
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path());
        let mut options = LongitudinalCapacityMaterializeOptionsV1::new(
            repo.path(),
            LongitudinalCapacityProfileV1::L100O10K,
            execution_identity(),
        );
        options.public_seed_hex = "00".repeat(32);

        assert!(matches!(
            resume_longitudinal_capacity_v1(options),
            Err(LongitudinalMaterializeError::NonFrozenSeed)
        ));
        assert_eq!(
            longitudinal_store_data_inventory_v1(repo.path())
                .unwrap()
                .file_count,
            0
        );
    }

    #[test]
    fn longitudinal_materialize_rejects_non_frozen_seed_and_clock_identity() {
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path());
        let mut options = LongitudinalMaterializeOptionsV1::new(
            repo.path(),
            LongitudinalTierV1::L1,
            execution_identity(),
        );
        options.public_seed_hex = "00".repeat(32);
        assert!(matches!(
            materialize_longitudinal_workload_v1(options),
            Err(LongitudinalMaterializeError::NonFrozenSeed)
        ));

        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path());
        let mut options = LongitudinalMaterializeOptionsV1::new(
            repo.path(),
            LongitudinalTierV1::L1,
            execution_identity(),
        );
        options.clock_identity = "system".to_owned();
        assert!(matches!(
            materialize_longitudinal_workload_v1(options),
            Err(LongitudinalMaterializeError::NonFrozenClock)
        ));
    }

    #[test]
    fn longitudinal_materialize_source_uses_no_direct_carrier_write_or_handwritten_event_json() {
        let builder = include_str!("builder.rs");
        let bridge = include_str!("../../session/benchmark.rs");

        for source in [builder, bridge] {
            for forbidden in [
                ["write_json_atomic", "(&event"].concat(),
                ["create_event", "_once("].concat(),
                ["serde_json::from_value::<", "ShoreEvent>"].concat(),
                ["serde_json::json!({", "\"schema\":\"shore.event\""].concat(),
            ] {
                assert!(!source.contains(&forbidden), "found {forbidden}");
            }
        }
    }

    fn execution_identity() -> LongitudinalExecutionIdentityV1 {
        LongitudinalExecutionIdentityV1 {
            source_commit: "1".repeat(40),
            source_tree: "2".repeat(40),
            cargo_lock_sha256: "3".repeat(64),
            runner_sha256: "4".repeat(64),
            build_profile: "test".to_owned(),
            operating_system: "macos".to_owned(),
            architecture: "aarch64".to_owned(),
            filesystem: "apfs".to_owned(),
            parent_commit: None,
        }
    }

    fn init_repo(root: &Path) {
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
    }
}
