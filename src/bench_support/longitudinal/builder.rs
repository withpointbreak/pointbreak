use std::path::{Path, PathBuf};

use crate::bench_support::longitudinal::contract::{
    LONGITUDINAL_CAPACITY_MATERIALIZATION_RECEIPT_SCHEMA_V1,
    LONGITUDINAL_MATERIALIZATION_RECEIPT_SCHEMA_V1, LONGITUDINAL_PUBLIC_SEED_HEX_V1,
    LongitudinalCapacityManifestV1, LongitudinalCapacityMaterializationReceiptV1,
    LongitudinalCapacityProfileV1, LongitudinalCapacitySubjectV1, LongitudinalEventFamilyCountV1,
    LongitudinalExecutionIdentityV1, LongitudinalExpectedSemanticReceiptV1,
    LongitudinalMaterializationReceiptV1, LongitudinalTierV1, LongitudinalWorkloadManifestV1,
    longitudinal_capacity_contract_v1, longitudinal_runner_contract_v1,
};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
use crate::session::benchmark::{
    LongitudinalRecordShapeV1, LongitudinalRecordSpecV1, prepare_longitudinal_record_v1,
    write_longitudinal_records_v1,
};
use crate::session::format_rfc3339_utc_millis;

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
        let right_receipt =
            materialize_longitudinal_workload_v1(LongitudinalMaterializeOptionsV1::new(
                right.path(),
                LongitudinalTierV1::L1,
                execution_identity(),
            ))
            .unwrap();

        verify_longitudinal_materialization_pair_v1(&left_receipt, &right_receipt).unwrap();
        assert_ne!(left_receipt.root_identity, right_receipt.root_identity);
        assert_eq!(left_receipt.manifest.event_count, 1_024);
        assert_eq!(left_receipt.manifest.revision_count, 48);
        assert_eq!(left_receipt.manifest.content_inventory.len(), 416);
        assert_eq!(left_receipt.manifest.removed_content_sha256.len(), 12);
        assert_eq!(left_receipt.strict, right_receipt.strict);

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
