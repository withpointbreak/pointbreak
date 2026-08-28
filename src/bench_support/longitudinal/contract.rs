use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};

pub const LONGITUDINAL_WORKLOAD_SCHEMA_V1: &str = "pointbreak.longitudinal-workload.v1";
pub const LONGITUDINAL_PROTOCOL_SCHEMA_V1: &str = "pointbreak.longitudinal-q3.v1";
pub const LONGITUDINAL_PUBLIC_SEED_HEX_V1: &str =
    "f4da49601a212010bae444e6ca2de6c6bf28b5ec1b0a05bf42154a533ca513ff";
pub const LONGITUDINAL_CAPACITY_SCHEMA_V1: &str = "pointbreak.longitudinal-capacity-sentinel.v1";
pub const LONGITUDINAL_CAPACITY_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.longitudinal-capacity-receipt.v1";
pub const LONGITUDINAL_CAPACITY_MEMORY_SCHEMA_V1: &str =
    "pointbreak.longitudinal-capacity-memory-receipt.v1";

pub const LONGITUDINAL_MATERIALIZATION_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.longitudinal-materialization-receipt.v1";
pub const LONGITUDINAL_OPERATION_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.longitudinal-operation-receipt.v1";
pub const LONGITUDINAL_COUNTER_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.longitudinal-counter-receipt.v1";
#[cfg(any(test, feature = "longitudinal-counting"))]
pub const INTERACTION_PERFORMANCE_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.interaction-performance-receipt.v1";
pub const LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.longitudinal-timeline-post-pin-barrier-receipt.v1";
pub const LONGITUDINAL_EVIDENCE_PACKAGE_SCHEMA_V1: &str =
    "pointbreak.longitudinal-evidence-package.v1";
pub const LONGITUDINAL_CAPACITY_MATERIALIZATION_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.longitudinal-capacity-materialization-receipt.v1";
pub const LONGITUDINAL_MATERIALIZATION_RESUME_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.longitudinal-materialization-resume-receipt.v1";
pub const LONGITUDINAL_MATERIALIZER_EQUIVALENCE_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.longitudinal-materializer-equivalence-receipt.v1";
pub const LONGITUDINAL_CARRY_FORWARD_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.longitudinal-carry-forward-receipt.v1";
pub const LONGITUDINAL_CARRY_FORWARD_AUTHORITY_PACKAGE_SCHEMA_V1: &str =
    "pointbreak.longitudinal-carry-forward-authority-package.v1";
pub const LONGITUDINAL_REMOVAL_UPGRADE_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.longitudinal-removal-upgrade-receipt.v1";
pub const LONGITUDINAL_REMOVAL_UPGRADE_AUTHORITY_PACKAGE_SCHEMA_V1: &str =
    "pointbreak.longitudinal-removal-upgrade-authority-package.v1";
pub const LONGITUDINAL_PACKAGE_VERIFICATION_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.longitudinal-package-verification-receipt.v1";
pub const LONGITUDINAL_CONTROLLER_FAILURE_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.longitudinal-controller-failure-receipt.v1";
pub const LONGITUDINAL_CARRY_FORWARD_REQUEST_SCHEMA_V1: &str =
    "pointbreak.longitudinal-carry-forward-request.v1";
pub const LONGITUDINAL_CAPACITY_PACKAGE_SCHEMA_V1: &str =
    "pointbreak.longitudinal-capacity-package.v1";
pub const LONGITUDINAL_CONTRACT_PUBLICATION_SCHEMA_V1: &str =
    "pointbreak.longitudinal-contract-publication.v1";
pub const LONGITUDINAL_HELP_SCHEMA_V1: &str = "pointbreak.longitudinal-help.v1";
pub const LONGITUDINAL_CONTRACT_MODE_V1: &str = "--longitudinal-contract";
pub const LONGITUDINAL_HELP_MODE_V1: &str = "--longitudinal-help";
pub const LONGITUDINAL_SMOKE_MODE_V1: &str = "--longitudinal-smoke";
pub const LONGITUDINAL_VERIFY_PACKAGE_MODE_V1: &str = "--longitudinal-verify-package";
pub const LONGITUDINAL_CARRY_FORWARD_MODE_V1: &str = "--longitudinal-carry-forward";
pub const LONGITUDINAL_CARRY_FORWARD_SMOKE_MODE_V1: &str = "--longitudinal-carry-forward-smoke";
pub const LONGITUDINAL_VERIFY_PACKAGE_RECEIPT_MODE_V1: &str =
    "--longitudinal-verify-package-receipt";
pub const LONGITUDINAL_VERIFY_CARRY_FORWARD_MODE_V1: &str = "--longitudinal-verify-carry-forward";
pub const LONGITUDINAL_RUNNER_CONTRACT_SHA256_V1: &str =
    "a2182c741530317f31532c62d4b0152b228e3d85aa177f812fdca433c265ed85";
pub const LONGITUDINAL_CAPACITY_CONTRACT_SHA256_V1: &str =
    "b888f40f415cc8a9d75b23c6ab135a9be33247ffff68f67c78287292c10a532d";
pub const LONGITUDINAL_CONTRACT_PUBLICATION_SHA256_V1: &str =
    "acb0a65963d7eff873c1cee4890cfa3e9e73ea900eab44121eb14c73c92c36e3";

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LongitudinalTierV1 {
    L1,
    L7,
    L25,
    L100,
}

impl LongitudinalTierV1 {
    pub const ALL: [Self; 4] = [Self::L1, Self::L7, Self::L25, Self::L100];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum LongitudinalCapacityProfileV1 {
    #[serde(rename = "L100-O10K")]
    L100O10K,
    #[serde(rename = "C262")]
    C262,
    #[serde(rename = "C524")]
    C524,
}

impl LongitudinalCapacityProfileV1 {
    pub const ALL: [Self; 3] = [Self::L100O10K, Self::C262, Self::C524];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(tag = "kind", content = "profile")]
pub enum LongitudinalCapacitySubjectV1 {
    #[serde(rename = "V1L100")]
    V1L100,
    #[serde(rename = "companion")]
    Companion(LongitudinalCapacityProfileV1),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LongitudinalCapacityProbeV1 {
    CarrierKey,
    SemanticId,
    ChronologicalHead,
    ChronologicalMiddle,
    ChronologicalTail,
    ObjectDetail,
    AppendDelta,
}

impl LongitudinalCapacityProbeV1 {
    pub const ALL: [Self; 7] = [
        Self::CarrierKey,
        Self::SemanticId,
        Self::ChronologicalHead,
        Self::ChronologicalMiddle,
        Self::ChronologicalTail,
        Self::ObjectDetail,
        Self::AppendDelta,
    ];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalCapacityClassificationV1 {
    Bounded,
    HistoryOrCardinalityProportional,
    UnsupportedNoCurrentSurface,
    ValidFailure,
    NotRunByGate,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LongitudinalLaneV1 {
    ReleaseUninstrumented,
    DebugUninstrumented,
    AttributionCounts,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LongitudinalOperationV1 {
    ColdHead,
    WarmHead,
    WinAdjacent,
    WinDeep,
    WinTail,
    AtDeep,
    SearchStructured,
    SearchBody,
    SearchMiss,
    FilterFacet,
    Revisions,
    Threads,
    AttentionAll,
    AttentionOne,
    DetailActive,
    SnapshotActive,
    DetailRemoved,
    FreshNoChange,
    NewCountZero,
    AppendOne,
    PostOne,
    AppendBurst,
    PostBurst,
    Restart,
    AuditExact,
    ExportExact,
    MigrateFresh,
    RebuildFull,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalOperationOutcomeV1 {
    Measured,
    UnavailableNoCurrentSurface,
    ValidFailure,
}

impl LongitudinalOperationV1 {
    pub const ALL: [Self; 28] = [
        Self::ColdHead,
        Self::WarmHead,
        Self::WinAdjacent,
        Self::WinDeep,
        Self::WinTail,
        Self::AtDeep,
        Self::SearchStructured,
        Self::SearchBody,
        Self::SearchMiss,
        Self::FilterFacet,
        Self::Revisions,
        Self::Threads,
        Self::AttentionAll,
        Self::AttentionOne,
        Self::DetailActive,
        Self::SnapshotActive,
        Self::DetailRemoved,
        Self::FreshNoChange,
        Self::NewCountZero,
        Self::AppendOne,
        Self::PostOne,
        Self::AppendBurst,
        Self::PostBurst,
        Self::Restart,
        Self::AuditExact,
        Self::ExportExact,
        Self::MigrateFresh,
        Self::RebuildFull,
    ];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalInteractionClassV1 {
    ColdHead,
    WarmWindow,
    SearchFilterFacet,
    FullRevisionThreadAttention,
    RevisionDetail,
    SnapshotDetail,
    FreshnessNewCount,
    AppendOne,
    PostOne,
    AppendBurst,
    PostBurst,
    Restart,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalMemoryCheckpointV1 {
    Empty,
    WarmHead,
    Search,
    DetailPeak,
    PostDetail,
    PostAppend,
    PostBurst,
}

impl LongitudinalMemoryCheckpointV1 {
    pub const ALL: [Self; 7] = [
        Self::Empty,
        Self::WarmHead,
        Self::Search,
        Self::DetailPeak,
        Self::PostDetail,
        Self::PostAppend,
        Self::PostBurst,
    ];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalCapacityMemoryCheckpointV1 {
    ClosedRootReopenIdle,
    PostCarrierKey,
    PostSemanticId,
    PostChronologicalHead,
    PostChronologicalMiddle,
    PostChronologicalTail,
    PostObjectDetailL100O10K,
    PostContentionQuiescence,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalInterruptionOperationV1 {
    ExportImport,
    MigrateFresh,
    RebuildFull,
}

impl LongitudinalInterruptionOperationV1 {
    pub const ALL: [Self; 3] = [Self::ExportImport, Self::MigrateFresh, Self::RebuildFull];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalInterruptionPointV1 {
    TenPercent,
    FiftyPercent,
    NinetyPercent,
}

impl LongitudinalInterruptionPointV1 {
    pub const ALL: [Self; 3] = [Self::TenPercent, Self::FiftyPercent, Self::NinetyPercent];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalPackagePurposeV1 {
    TerminalEvidence,
    NonTimingSmoke,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalVerifiedPackageKindV1 {
    Workload,
    Capacity,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCarryForwardSlotV1 {
    pub tier: LongitudinalTierV1,
    pub lane: LongitudinalLaneV1,
    pub independent_run: u8,
}

impl LongitudinalCarryForwardSlotV1 {
    pub fn validate(self) -> Result<(), LongitudinalContractError> {
        match (self.lane, self.independent_run) {
            (LongitudinalLaneV1::ReleaseUninstrumented, 1 | 2)
            | (LongitudinalLaneV1::DebugUninstrumented, 1) => Ok(()),
            _ => Err(LongitudinalContractError::ContractDrift {
                field: "carry-forward root slot",
            }),
        }
    }
}

fn carry_forward_lane_profile_v1(
    lane: LongitudinalLaneV1,
) -> Result<&'static str, LongitudinalContractError> {
    match lane {
        LongitudinalLaneV1::ReleaseUninstrumented => Ok("release-uninstrumented"),
        LongitudinalLaneV1::DebugUninstrumented => Ok("debug-uninstrumented"),
        LongitudinalLaneV1::AttributionCounts => Err(LongitudinalContractError::ContractDrift {
            field: "carry-forward source lane",
        }),
    }
}

fn execution_without_lane_identity_v1(
    execution: &LongitudinalExecutionIdentityV1,
) -> LongitudinalExecutionIdentityV1 {
    let mut common = execution.clone();
    common.runner_sha256.clear();
    common.build_profile.clear();
    common
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LongitudinalFailureOperationSelectorV1 {
    Workload(LongitudinalOperationV1),
    Capacity(LongitudinalCapacityProbeV1),
    InspectorRevisionListPreflight,
    PackageVerification(LongitudinalVerifiedPackageKindV1),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalHttpBodyClassificationV1 {
    Empty,
    JsonSuccess,
    JsonError,
    Html,
    Text,
    Binary,
    RedactedSensitive,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(tag = "kind", content = "code", rename_all = "snake_case")]
pub enum LongitudinalInspectorExitV1 {
    NotStarted,
    Exited(i32),
    Signaled,
    SpawnFailed,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalStderrClassificationV1 {
    NotCaptured,
    Empty,
    KnownDiagnostic,
    Panic,
    IoFailure,
    OtherSanitized,
    RedactedSensitive,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalCounterFieldV1 {
    DirectoryEntriesWalked,
    CarrierOpens,
    CarrierBytesRead,
    EventDecodes,
    EventValidations,
    EventFolds,
    ChronologicalSortItems,
    BodyArtifactReads,
    BodyBytesRead,
    ObjectArtifactReads,
    ObjectBytesRead,
    ProjectionRebuilds,
    StateRebuilds,
    ResponseBytes,
}

impl LongitudinalCounterFieldV1 {
    pub const ALL: [Self; 14] = [
        Self::DirectoryEntriesWalked,
        Self::CarrierOpens,
        Self::CarrierBytesRead,
        Self::EventDecodes,
        Self::EventValidations,
        Self::EventFolds,
        Self::ChronologicalSortItems,
        Self::BodyArtifactReads,
        Self::BodyBytesRead,
        Self::ObjectArtifactReads,
        Self::ObjectBytesRead,
        Self::ProjectionRebuilds,
        Self::StateRebuilds,
        Self::ResponseBytes,
    ];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalCapacityOwnershipFieldV1 {
    RetainedDecodedEvents,
    RetainedHydratedHistoryEntries,
    RetainedHydratedBodyBytes,
    RetainedSearchRecordStrings,
    RetainedSearchRecordFieldBytes,
    RetainedSerializedResponseCacheBytes,
    RetainedSnapshotHighlightEntries,
    RetainedSnapshotHighlightBytes,
}

impl LongitudinalCapacityOwnershipFieldV1 {
    pub const ALL: [Self; 8] = [
        Self::RetainedDecodedEvents,
        Self::RetainedHydratedHistoryEntries,
        Self::RetainedHydratedBodyBytes,
        Self::RetainedSearchRecordStrings,
        Self::RetainedSearchRecordFieldBytes,
        Self::RetainedSerializedResponseCacheBytes,
        Self::RetainedSnapshotHighlightEntries,
        Self::RetainedSnapshotHighlightBytes,
    ];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalContentKindV1 {
    ExternalBody,
    ObjectArtifact,
    ValidationLog,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalTierRequirementV1 {
    pub tier: LongitudinalTierV1,
    pub event_count: u64,
    pub block_count: u64,
    pub journal_count: u64,
    pub revision_count: u64,
    pub task_attempt_count: u64,
    pub body_fact_count: u64,
    pub external_body_count: u64,
    pub object_artifact_count: u64,
    pub validation_log_count: u64,
    pub removed_content_count: u64,
    pub present_content_count: u64,
    pub decoded_body_bytes: u64,
    pub decoded_object_target_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalEventFamilyRequirementV1 {
    pub event_type: String,
    pub per_block: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalEventFamilyCountV1 {
    pub event_type: String,
    pub count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalRunMatrixEntryV1 {
    pub lane: LongitudinalLaneV1,
    pub roots_per_tier: u8,
    pub records_timing: bool,
    pub records_cpu: bool,
    pub records_rss_heap: bool,
    pub performance_comparison_admissible: bool,
    pub counters_admissible: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalSamplePlanV1 {
    pub process_cold_per_release_root: u16,
    pub untimed_warm_requests: u16,
    pub excluded_warmups: u16,
    pub warm_samples_per_release_root: u16,
    pub append_one_samples_per_release_root: u16,
    pub append_burst_samples_per_release_root: u16,
    pub restart_samples_per_release_root: u16,
    pub maintenance_samples_per_release_root: u16,
    pub debug_cold_samples: u16,
    pub debug_warm_samples: u16,
    pub debug_write_restart_maintenance_samples: u16,
    pub counting_operations: Vec<LongitudinalOperationV1>,
    pub nearest_rank_percentile: u8,
    pub retain_all_samples: bool,
    pub outlier_removal_allowed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalInteractionEnvelopeV1 {
    pub class: LongitudinalInteractionClassV1,
    pub l1_wall_millis: u64,
    pub l7_wall_millis: u64,
    pub l25_wall_millis: u64,
    pub l100_wall_millis: u64,
    pub l100_cpu_millis: u64,
    pub complexity_intent: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalMemoryEnvelopeV1 {
    pub l100_steady_rss_mib: u64,
    pub l100_peak_rss_mib: u64,
    pub l100_retained_heap_mib: u64,
    pub l7_to_l100_steady_rss_bytes_per_event: u64,
    pub l7_to_l100_retained_heap_bytes_per_event: u64,
    pub selected_allocation_release_basis_points: u16,
    pub checkpoints: Vec<LongitudinalMemoryCheckpointV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalWriteEnvelopeV1 {
    pub steady_amplification_basis_points: u32,
    pub high_water_amplification_basis_points: u32,
    pub append_one_fixed_overhead_bytes: u64,
    pub append_burst_fixed_overhead_bytes: u64,
    pub old_authoritative_rewrite_allowed: bool,
    pub history_proportional_derived_rewrite_allowed: bool,
    pub full_state_rebuild_allowed: bool,
    pub burst_checkpoint_publications: u8,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalMaintenanceEnvelopeV1 {
    pub rebuild_wall_seconds: u64,
    pub rebuild_cpu_seconds: u64,
    pub rebuild_high_water_basis_points: u16,
    pub audit_wall_seconds: u64,
    pub export_wall_seconds: u64,
    pub import_wall_seconds: u64,
    pub migration_wall_seconds: u64,
    pub repair_wall_seconds: u64,
    pub free_space_source_basis_points: u16,
    pub free_space_fixed_bytes: u64,
    pub destination_estimate_required: bool,
    pub temporary_estimate_required: bool,
    pub take_maximum_of_source_and_destination_formulas: bool,
    pub interruption_percentages: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalCounterEnvelopeClassV1 {
    NoChangeFreshness,
    FixedHistorySearchFilterWindow,
    RevisionsThreadsAttention,
    RevisionDetail,
    AppendOneMaintenance,
    AppendBurstMaintenance,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCounterEnvelopeV1 {
    pub class: LongitudinalCounterEnvelopeClassV1,
    pub directory_entries_walked_max: u64,
    pub carrier_opens_max: u64,
    pub event_decodes_max: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_artifact_reads_max: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_artifact_reads_max: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_folds_max: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chronological_sort_items_max: Option<u64>,
    pub full_projection_rebuilds_max: u8,
    pub state_rebuilds_max: u8,
    pub selected_content_only: bool,
    pub output_sensitive_fold_sort: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalRunnerContractV1 {
    pub schema: String,
    pub protocol: String,
    pub public_seed_hex: String,
    pub block_event_count: u64,
    pub history_page_size: u64,
    pub body_size_pattern: Vec<u64>,
    pub object_size_multiplier: u64,
    pub append_batch_size: u64,
    pub tiers: Vec<LongitudinalTierRequirementV1>,
    pub event_families: Vec<LongitudinalEventFamilyRequirementV1>,
    pub operation_schedule: Vec<LongitudinalOperationV1>,
    pub run_matrix: Vec<LongitudinalRunMatrixEntryV1>,
    pub samples: LongitudinalSamplePlanV1,
    pub interaction_envelopes: Vec<LongitudinalInteractionEnvelopeV1>,
    pub memory_envelope: LongitudinalMemoryEnvelopeV1,
    pub counter_fields: Vec<LongitudinalCounterFieldV1>,
    pub counter_envelopes: Vec<LongitudinalCounterEnvelopeV1>,
    pub fixed_output_l100_to_l25_max_ratio_basis_points: u16,
    pub write_envelope: LongitudinalWriteEnvelopeV1,
    pub maintenance_envelope: LongitudinalMaintenanceEnvelopeV1,
    pub public_inputs_only: bool,
    pub lane_pooling_allowed: bool,
    pub contract_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCapacityProfileRequirementV1 {
    pub profile: LongitudinalCapacityProfileV1,
    pub required: bool,
    pub event_count: u64,
    pub revision_count: u64,
    pub object_artifact_count: u64,
    pub task_attempt_count: u64,
    pub body_fact_count: u64,
    pub external_body_count: u64,
    pub validation_log_count: u64,
    pub removed_content_count: u64,
    pub present_content_count: u64,
    pub decoded_body_bytes: u64,
    pub decoded_object_target_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCapacitySamplePlanV1 {
    pub process_cold_samples: u16,
    pub excluded_warmups: u16,
    pub warm_samples: u16,
    pub report_median: bool,
    pub report_maximum: bool,
    pub report_p95: bool,
    pub latency_threshold: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalContentionContractV1 {
    pub profile: LongitudinalCapacityProfileV1,
    pub writer_processes: u8,
    pub reader_processes: u8,
    pub unique_attempts_per_writer: u8,
    pub shared_attempts_per_writer: u8,
    pub expected_created: u8,
    pub expected_existing: u8,
    pub reader_cycles: u8,
    pub wall_guard_seconds: u64,
    pub rerun_allowed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalC524GateContractV1 {
    pub event_growth_basis_points: u32,
    pub maximum_pre_admission_ratio_basis_points: u32,
    pub maximum_post_admission_ratio_basis_points: u32,
    pub free_space_high_water_basis_points: u32,
    pub free_space_fixed_bytes: u64,
    pub identity_required: bool,
    pub base_rows_required: bool,
    pub counting_rows_required: bool,
    pub memory_receipts_required: bool,
    pub package_verification_required: bool,
    pub host_pressure_must_be_clear: bool,
    pub manual_override_allowed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalNonCompensationContractV1 {
    pub maximum_ratio_basis_points: u16,
    pub direct_whole_history_is_proportional: bool,
    pub wall_time_can_rescue: bool,
    pub unsupported_can_pass: bool,
    pub allowed_exhaustive_operations: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCapacityContractV1 {
    pub schema: String,
    pub receipt_schema: String,
    pub memory_schema: String,
    pub public_seed_hex: String,
    pub derivation_domain: String,
    pub profiles: Vec<LongitudinalCapacityProfileRequirementV1>,
    pub subjects: Vec<LongitudinalCapacitySubjectV1>,
    pub probes: Vec<LongitudinalCapacityProbeV1>,
    pub samples: LongitudinalCapacitySamplePlanV1,
    pub memory_checkpoints: Vec<LongitudinalCapacityMemoryCheckpointV1>,
    pub ownership_fields: Vec<LongitudinalCapacityOwnershipFieldV1>,
    pub contention: LongitudinalContentionContractV1,
    pub c524_gate: LongitudinalC524GateContractV1,
    pub non_compensation: LongitudinalNonCompensationContractV1,
    pub v1_pooling_allowed: bool,
    pub contract_sha256: String,
}

impl LongitudinalRunnerContractV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.contract_sha256, |contract_sha256| {
            let mut preimage = self.clone();
            preimage.contract_sha256 = contract_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_WORKLOAD_SCHEMA_V1
            || self.protocol != LONGITUDINAL_PROTOCOL_SCHEMA_V1
            || self.public_seed_hex != LONGITUDINAL_PUBLIC_SEED_HEX_V1
        {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        require_exact_sequence(
            self.tiers.iter().map(|entry| entry.tier),
            LongitudinalTierV1::ALL,
            "workload tiers",
        )?;
        require_exact_sequence(
            self.operation_schedule.iter().copied(),
            LongitudinalOperationV1::ALL,
            "operation schedule",
        )?;
        require_exact_sequence(
            self.counter_fields.iter().copied(),
            LongitudinalCounterFieldV1::ALL,
            "counter fields",
        )?;
        validate_bound_hash(
            &self.contract_sha256,
            LONGITUDINAL_RUNNER_CONTRACT_SHA256_V1,
            "longitudinal runner contract",
        )?;
        if self.canonical_sha256()? != LONGITUDINAL_RUNNER_CONTRACT_SHA256_V1 {
            return Err(LongitudinalContractError::ContractDrift {
                field: "longitudinal runner contract",
            });
        }
        Ok(())
    }
}

impl LongitudinalCapacityContractV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.contract_sha256, |contract_sha256| {
            let mut preimage = self.clone();
            preimage.contract_sha256 = contract_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_CAPACITY_SCHEMA_V1
            || self.receipt_schema != LONGITUDINAL_CAPACITY_RECEIPT_SCHEMA_V1
            || self.memory_schema != LONGITUDINAL_CAPACITY_MEMORY_SCHEMA_V1
            || self.public_seed_hex != LONGITUDINAL_PUBLIC_SEED_HEX_V1
            || self.derivation_domain != LONGITUDINAL_CAPACITY_SCHEMA_V1
        {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        require_exact_sequence(
            self.profiles.iter().map(|entry| entry.profile),
            LongitudinalCapacityProfileV1::ALL,
            "capacity profiles",
        )?;
        require_exact_sequence(
            self.probes.iter().copied(),
            LongitudinalCapacityProbeV1::ALL,
            "capacity probes",
        )?;
        require_exact_sequence(
            self.ownership_fields.iter().copied(),
            LongitudinalCapacityOwnershipFieldV1::ALL,
            "capacity ownership fields",
        )?;
        validate_bound_hash(
            &self.contract_sha256,
            LONGITUDINAL_CAPACITY_CONTRACT_SHA256_V1,
            "longitudinal capacity contract",
        )?;
        if self.canonical_sha256()? != LONGITUDINAL_CAPACITY_CONTRACT_SHA256_V1 {
            return Err(LongitudinalContractError::ContractDrift {
                field: "longitudinal capacity contract",
            });
        }
        Ok(())
    }
}

pub fn longitudinal_runner_contract_v1() -> LongitudinalRunnerContractV1 {
    let body_sizes = vec![512, 1_024, 2_048, 4_096, 8_192, 12_288, 16_384, 20_992];
    let mut contract = LongitudinalRunnerContractV1 {
        schema: LONGITUDINAL_WORKLOAD_SCHEMA_V1.to_owned(),
        protocol: LONGITUDINAL_PROTOCOL_SCHEMA_V1.to_owned(),
        public_seed_hex: LONGITUDINAL_PUBLIC_SEED_HEX_V1.to_owned(),
        block_event_count: 256,
        history_page_size: 100,
        body_size_pattern: body_sizes,
        object_size_multiplier: 8,
        append_batch_size: 30,
        tiers: vec![
            tier_requirement(LongitudinalTierV1::L1, 4),
            tier_requirement(LongitudinalTierV1::L7, 28),
            tier_requirement(LongitudinalTierV1::L25, 100),
            tier_requirement(LongitudinalTierV1::L100, 400),
        ],
        event_families: vec![
            event_family("review_initialized", 1),
            event_family("work_object_proposed", 16),
            event_family("review_observation_recorded", 64),
            event_family("review_assessment_recorded", 24),
            event_family("input_request_opened", 24),
            event_family("input_request_responded", 16),
            event_family("revision_ref_associated", 12),
            event_family("revision_ref_withdrawn", 4),
            event_family("revision_commit_associated", 16),
            event_family("revision_commit_withdrawn", 4),
            event_family("validation_check_recorded", 40),
            event_family("task_checkpoint_captured", 12),
            event_family("task_observation_recorded", 12),
            event_family("event_signature_recorded", 8),
            event_family("artifact_removed", 3),
            event_family("review_note_imported", 0),
        ],
        operation_schedule: LongitudinalOperationV1::ALL.to_vec(),
        run_matrix: vec![
            LongitudinalRunMatrixEntryV1 {
                lane: LongitudinalLaneV1::ReleaseUninstrumented,
                roots_per_tier: 2,
                records_timing: true,
                records_cpu: true,
                records_rss_heap: true,
                performance_comparison_admissible: true,
                counters_admissible: false,
            },
            LongitudinalRunMatrixEntryV1 {
                lane: LongitudinalLaneV1::DebugUninstrumented,
                roots_per_tier: 1,
                records_timing: true,
                records_cpu: true,
                records_rss_heap: true,
                performance_comparison_admissible: false,
                counters_admissible: false,
            },
            LongitudinalRunMatrixEntryV1 {
                lane: LongitudinalLaneV1::AttributionCounts,
                roots_per_tier: 1,
                records_timing: false,
                records_cpu: false,
                records_rss_heap: false,
                performance_comparison_admissible: false,
                counters_admissible: true,
            },
        ],
        samples: LongitudinalSamplePlanV1 {
            process_cold_per_release_root: 10,
            untimed_warm_requests: 1,
            excluded_warmups: 3,
            warm_samples_per_release_root: 30,
            append_one_samples_per_release_root: 30,
            append_burst_samples_per_release_root: 10,
            restart_samples_per_release_root: 10,
            maintenance_samples_per_release_root: 3,
            debug_cold_samples: 1,
            debug_warm_samples: 3,
            debug_write_restart_maintenance_samples: 1,
            counting_operations: vec![
                LongitudinalOperationV1::ColdHead,
                LongitudinalOperationV1::WarmHead,
                LongitudinalOperationV1::WinDeep,
                LongitudinalOperationV1::SearchBody,
                LongitudinalOperationV1::Revisions,
                LongitudinalOperationV1::Threads,
                LongitudinalOperationV1::AttentionAll,
                LongitudinalOperationV1::DetailActive,
                LongitudinalOperationV1::FreshNoChange,
                LongitudinalOperationV1::AppendOne,
                LongitudinalOperationV1::AppendBurst,
                LongitudinalOperationV1::Restart,
                LongitudinalOperationV1::AuditExact,
                LongitudinalOperationV1::ExportExact,
                LongitudinalOperationV1::MigrateFresh,
                LongitudinalOperationV1::RebuildFull,
            ],
            nearest_rank_percentile: 95,
            retain_all_samples: true,
            outlier_removal_allowed: false,
        },
        interaction_envelopes: interaction_envelopes(),
        memory_envelope: LongitudinalMemoryEnvelopeV1 {
            l100_steady_rss_mib: 96,
            l100_peak_rss_mib: 128,
            l100_retained_heap_mib: 64,
            l7_to_l100_steady_rss_bytes_per_event: 512,
            l7_to_l100_retained_heap_bytes_per_event: 384,
            selected_allocation_release_basis_points: 9_500,
            checkpoints: vec![
                LongitudinalMemoryCheckpointV1::Empty,
                LongitudinalMemoryCheckpointV1::WarmHead,
                LongitudinalMemoryCheckpointV1::Search,
                LongitudinalMemoryCheckpointV1::DetailPeak,
                LongitudinalMemoryCheckpointV1::PostDetail,
                LongitudinalMemoryCheckpointV1::PostAppend,
                LongitudinalMemoryCheckpointV1::PostBurst,
            ],
        },
        counter_fields: LongitudinalCounterFieldV1::ALL.to_vec(),
        counter_envelopes: counter_envelopes(),
        fixed_output_l100_to_l25_max_ratio_basis_points: 20_000,
        write_envelope: LongitudinalWriteEnvelopeV1 {
            steady_amplification_basis_points: 80_000,
            high_water_amplification_basis_points: 120_000,
            append_one_fixed_overhead_bytes: 256 * 1024,
            append_burst_fixed_overhead_bytes: MIB,
            old_authoritative_rewrite_allowed: false,
            history_proportional_derived_rewrite_allowed: false,
            full_state_rebuild_allowed: false,
            burst_checkpoint_publications: 1,
        },
        maintenance_envelope: LongitudinalMaintenanceEnvelopeV1 {
            rebuild_wall_seconds: 60,
            rebuild_cpu_seconds: 45,
            rebuild_high_water_basis_points: 15_000,
            audit_wall_seconds: 60,
            export_wall_seconds: 120,
            import_wall_seconds: 180,
            migration_wall_seconds: 15 * 60,
            repair_wall_seconds: 15 * 60,
            free_space_source_basis_points: 12_000,
            free_space_fixed_bytes: 256 * MIB,
            destination_estimate_required: true,
            temporary_estimate_required: true,
            take_maximum_of_source_and_destination_formulas: true,
            interruption_percentages: vec![10, 50, 90],
        },
        public_inputs_only: true,
        lane_pooling_allowed: false,
        contract_sha256: String::new(),
    };
    contract.contract_sha256 = contract
        .canonical_sha256()
        .expect("the frozen longitudinal runner contract is canonical");
    contract
}

pub fn longitudinal_capacity_contract_v1() -> LongitudinalCapacityContractV1 {
    let mut contract = LongitudinalCapacityContractV1 {
        schema: LONGITUDINAL_CAPACITY_SCHEMA_V1.to_owned(),
        receipt_schema: LONGITUDINAL_CAPACITY_RECEIPT_SCHEMA_V1.to_owned(),
        memory_schema: LONGITUDINAL_CAPACITY_MEMORY_SCHEMA_V1.to_owned(),
        public_seed_hex: LONGITUDINAL_PUBLIC_SEED_HEX_V1.to_owned(),
        derivation_domain: LONGITUDINAL_CAPACITY_SCHEMA_V1.to_owned(),
        profiles: vec![
            LongitudinalCapacityProfileRequirementV1 {
                profile: LongitudinalCapacityProfileV1::L100O10K,
                required: true,
                event_count: 102_400,
                revision_count: 10_000,
                object_artifact_count: 10_000,
                task_attempt_count: 400,
                body_fact_count: 65_800,
                external_body_count: 32_900,
                validation_log_count: 1_000,
                removed_content_count: 300,
                present_content_count: 43_600,
                decoded_body_bytes: 539_033_600,
                decoded_object_target_bytes: 655_360_000,
            },
            LongitudinalCapacityProfileRequirementV1 {
                profile: LongitudinalCapacityProfileV1::C262,
                required: true,
                event_count: 262_144,
                revision_count: 12_288,
                object_artifact_count: 12_288,
                task_attempt_count: 4_096,
                body_fact_count: 184_320,
                external_body_count: 92_160,
                validation_log_count: 5_120,
                removed_content_count: 3_072,
                present_content_count: 106_496,
                decoded_body_bytes: 1_509_949_440,
                decoded_object_target_bytes: 805_306_368,
            },
            LongitudinalCapacityProfileRequirementV1 {
                profile: LongitudinalCapacityProfileV1::C524,
                required: false,
                event_count: 524_288,
                revision_count: 24_576,
                object_artifact_count: 24_576,
                task_attempt_count: 8_192,
                body_fact_count: 368_640,
                external_body_count: 184_320,
                validation_log_count: 10_240,
                removed_content_count: 6_144,
                present_content_count: 212_992,
                decoded_body_bytes: 3_019_898_880,
                decoded_object_target_bytes: 1_610_612_736,
            },
        ],
        subjects: vec![
            LongitudinalCapacitySubjectV1::V1L100,
            LongitudinalCapacitySubjectV1::Companion(LongitudinalCapacityProfileV1::L100O10K),
            LongitudinalCapacitySubjectV1::Companion(LongitudinalCapacityProfileV1::C262),
            LongitudinalCapacitySubjectV1::Companion(LongitudinalCapacityProfileV1::C524),
        ],
        probes: LongitudinalCapacityProbeV1::ALL.to_vec(),
        samples: LongitudinalCapacitySamplePlanV1 {
            process_cold_samples: 1,
            excluded_warmups: 1,
            warm_samples: 5,
            report_median: true,
            report_maximum: true,
            report_p95: false,
            latency_threshold: None,
        },
        memory_checkpoints: vec![
            LongitudinalCapacityMemoryCheckpointV1::ClosedRootReopenIdle,
            LongitudinalCapacityMemoryCheckpointV1::PostCarrierKey,
            LongitudinalCapacityMemoryCheckpointV1::PostSemanticId,
            LongitudinalCapacityMemoryCheckpointV1::PostChronologicalHead,
            LongitudinalCapacityMemoryCheckpointV1::PostChronologicalMiddle,
            LongitudinalCapacityMemoryCheckpointV1::PostChronologicalTail,
            LongitudinalCapacityMemoryCheckpointV1::PostObjectDetailL100O10K,
            LongitudinalCapacityMemoryCheckpointV1::PostContentionQuiescence,
        ],
        ownership_fields: LongitudinalCapacityOwnershipFieldV1::ALL.to_vec(),
        contention: LongitudinalContentionContractV1 {
            profile: LongitudinalCapacityProfileV1::L100O10K,
            writer_processes: 2,
            reader_processes: 1,
            unique_attempts_per_writer: 4,
            shared_attempts_per_writer: 2,
            expected_created: 10,
            expected_existing: 2,
            reader_cycles: 20,
            wall_guard_seconds: 15 * 60,
            rerun_allowed: false,
        },
        c524_gate: LongitudinalC524GateContractV1 {
            event_growth_basis_points: 25_600,
            maximum_pre_admission_ratio_basis_points: 32_000,
            maximum_post_admission_ratio_basis_points: 25_000,
            free_space_high_water_basis_points: 25_000,
            free_space_fixed_bytes: 16 * GIB,
            identity_required: true,
            base_rows_required: true,
            counting_rows_required: true,
            memory_receipts_required: true,
            package_verification_required: true,
            host_pressure_must_be_clear: true,
            manual_override_allowed: false,
        },
        non_compensation: LongitudinalNonCompensationContractV1 {
            maximum_ratio_basis_points: 12_500,
            direct_whole_history_is_proportional: true,
            wall_time_can_rescue: false,
            unsupported_can_pass: false,
            allowed_exhaustive_operations: [
                "integrity_audit",
                "logical_export_import_verification",
                "migration",
                "copy_out_repair",
                "backup_restore_verification",
                "removal_compaction_reconciliation",
                "deliberate_rebuild",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        },
        v1_pooling_allowed: false,
        contract_sha256: String::new(),
    };
    contract.contract_sha256 = contract
        .canonical_sha256()
        .expect("the frozen longitudinal capacity contract is canonical");
    contract
}

fn tier_requirement(tier: LongitudinalTierV1, blocks: u64) -> LongitudinalTierRequirementV1 {
    LongitudinalTierRequirementV1 {
        tier,
        event_count: blocks * 256,
        block_count: blocks,
        journal_count: blocks,
        revision_count: blocks * 12,
        task_attempt_count: blocks * 4,
        body_fact_count: blocks * 180,
        external_body_count: blocks * 90,
        object_artifact_count: blocks * 12,
        validation_log_count: blocks * 5,
        removed_content_count: blocks * 3,
        present_content_count: blocks * 104,
        decoded_body_bytes: blocks * 1_474_560,
        decoded_object_target_bytes: blocks * 786_432,
    }
}

fn event_family(event_type: &str, per_block: u64) -> LongitudinalEventFamilyRequirementV1 {
    LongitudinalEventFamilyRequirementV1 {
        event_type: event_type.to_owned(),
        per_block,
    }
}

fn interaction_envelopes() -> Vec<LongitudinalInteractionEnvelopeV1> {
    use LongitudinalInteractionClassV1 as Class;
    [
        (
            Class::ColdHead,
            500,
            750,
            1_000,
            2_000,
            1_500,
            "product target; exact fresh-process validation remains visible",
        ),
        (
            Class::WarmWindow,
            75,
            100,
            125,
            150,
            100,
            "window plus bounded delta, never all events",
        ),
        (
            Class::SearchFilterFacet,
            100,
            150,
            200,
            250,
            200,
            "query result plus derived lookup, never body-wide scan",
        ),
        (
            Class::FullRevisionThreadAttention,
            200,
            350,
            750,
            1_500,
            1_000,
            "response rows or edges plus bounded delta",
        ),
        (
            Class::RevisionDetail,
            100,
            125,
            175,
            250,
            175,
            "selected revision facts plus selected content",
        ),
        (
            Class::SnapshotDetail,
            150,
            200,
            350,
            500,
            350,
            "selected snapshot bytes and rows",
        ),
        (
            Class::FreshnessNewCount,
            25,
            30,
            40,
            50,
            25,
            "constant marker or query delta; no full journal",
        ),
        (
            Class::AppendOne,
            100,
            125,
            175,
            250,
            200,
            "new fact plus bounded target state; no full replay",
        ),
        (
            Class::PostOne,
            200,
            250,
            350,
            500,
            400,
            "bounded incremental maintenance",
        ),
        (
            Class::AppendBurst,
            750,
            1_000,
            1_500,
            3_000,
            2_000,
            "thirty events plus bounded affected state",
        ),
        (
            Class::PostBurst,
            350,
            500,
            750,
            1_000,
            750,
            "one coalesced incremental refresh",
        ),
        (
            Class::Restart,
            750,
            1_000,
            1_500,
            3_000,
            2_500,
            "exact recovery receipt remains mandatory",
        ),
    ]
    .into_iter()
    .map(
        |(class, l1, l7, l25, l100, cpu, intent)| LongitudinalInteractionEnvelopeV1 {
            class,
            l1_wall_millis: l1,
            l7_wall_millis: l7,
            l25_wall_millis: l25,
            l100_wall_millis: l100,
            l100_cpu_millis: cpu,
            complexity_intent: intent.to_owned(),
        },
    )
    .collect()
}

fn counter_envelopes() -> Vec<LongitudinalCounterEnvelopeV1> {
    use LongitudinalCounterEnvelopeClassV1 as Class;
    vec![
        LongitudinalCounterEnvelopeV1 {
            class: Class::NoChangeFreshness,
            directory_entries_walked_max: 4,
            carrier_opens_max: 0,
            event_decodes_max: 0,
            body_artifact_reads_max: Some(0),
            object_artifact_reads_max: Some(0),
            event_folds_max: Some(0),
            chronological_sort_items_max: Some(0),
            full_projection_rebuilds_max: 0,
            state_rebuilds_max: 0,
            selected_content_only: true,
            output_sensitive_fold_sort: false,
        },
        LongitudinalCounterEnvelopeV1 {
            class: Class::FixedHistorySearchFilterWindow,
            directory_entries_walked_max: 8,
            carrier_opens_max: 512,
            event_decodes_max: 256,
            body_artifact_reads_max: Some(100),
            object_artifact_reads_max: Some(1),
            event_folds_max: Some(2_048),
            chronological_sort_items_max: Some(512),
            full_projection_rebuilds_max: 0,
            state_rebuilds_max: 0,
            selected_content_only: true,
            output_sensitive_fold_sort: false,
        },
        LongitudinalCounterEnvelopeV1 {
            class: Class::RevisionsThreadsAttention,
            directory_entries_walked_max: 8,
            carrier_opens_max: 2_048,
            event_decodes_max: 1_024,
            body_artifact_reads_max: Some(0),
            object_artifact_reads_max: Some(0),
            event_folds_max: None,
            chronological_sort_items_max: None,
            full_projection_rebuilds_max: 0,
            state_rebuilds_max: 0,
            selected_content_only: true,
            output_sensitive_fold_sort: true,
        },
        LongitudinalCounterEnvelopeV1 {
            class: Class::RevisionDetail,
            directory_entries_walked_max: 8,
            carrier_opens_max: 512,
            event_decodes_max: 256,
            body_artifact_reads_max: None,
            object_artifact_reads_max: None,
            event_folds_max: Some(2_048),
            chronological_sort_items_max: None,
            full_projection_rebuilds_max: 0,
            state_rebuilds_max: 0,
            selected_content_only: true,
            output_sensitive_fold_sort: false,
        },
        LongitudinalCounterEnvelopeV1 {
            class: Class::AppendOneMaintenance,
            directory_entries_walked_max: 16,
            carrier_opens_max: 512,
            event_decodes_max: 256,
            body_artifact_reads_max: None,
            object_artifact_reads_max: None,
            event_folds_max: Some(2_048),
            chronological_sort_items_max: None,
            full_projection_rebuilds_max: 0,
            state_rebuilds_max: 0,
            selected_content_only: true,
            output_sensitive_fold_sort: false,
        },
        LongitudinalCounterEnvelopeV1 {
            class: Class::AppendBurstMaintenance,
            directory_entries_walked_max: 32,
            carrier_opens_max: 2_048,
            event_decodes_max: 1_024,
            body_artifact_reads_max: None,
            object_artifact_reads_max: None,
            event_folds_max: Some(8_192),
            chronological_sort_items_max: None,
            full_projection_rebuilds_max: 0,
            state_rebuilds_max: 0,
            selected_content_only: true,
            output_sensitive_fold_sort: false,
        },
    ]
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalExecutionIdentityV1 {
    pub source_commit: String,
    pub source_tree: String,
    pub cargo_lock_sha256: String,
    pub runner_sha256: String,
    pub build_profile: String,
    pub operating_system: String,
    pub architecture: String,
    pub filesystem: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_commit: Option<String>,
}

impl LongitudinalExecutionIdentityV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256(self)
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        validate_hex(&self.source_commit, 40, "source commit")?;
        validate_hex(&self.source_tree, 40, "source tree")?;
        validate_hex(&self.cargo_lock_sha256, 64, "Cargo.lock SHA-256")?;
        validate_hex(&self.runner_sha256, 64, "runner SHA-256")?;
        if let Some(parent) = &self.parent_commit {
            validate_hex(parent, 40, "parent commit")?;
        }
        require_nonempty(&self.build_profile, "build profile")?;
        require_nonempty(&self.operating_system, "operating system")?;
        require_nonempty(&self.architecture, "architecture")?;
        require_nonempty(&self.filesystem, "filesystem")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalEventIdentityV1 {
    pub event_id: String,
    pub canonical_decoded_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalInventoryEntryV1 {
    pub logical_key: String,
    pub kind: LongitudinalContentKindV1,
    pub raw_sha256: String,
    pub decoded_sha256: String,
    pub raw_bytes: u64,
    pub decoded_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalEventCarrierV1 {
    pub event_id: String,
    pub logical_key_sha256: String,
    pub raw_sha256: String,
    pub raw_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalExpectedSemanticReceiptV1 {
    pub operation: LongitudinalOperationV1,
    pub semantic_receipt_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalWorkloadManifestV1 {
    pub schema: String,
    pub protocol: String,
    pub contract_sha256: String,
    pub execution: LongitudinalExecutionIdentityV1,
    pub public_seed_hex: String,
    pub tier: LongitudinalTierV1,
    pub event_count: u64,
    pub revision_count: u64,
    pub by_type: Vec<LongitudinalEventFamilyCountV1>,
    pub ordered_events: Vec<LongitudinalEventIdentityV1>,
    pub event_carriers: Vec<LongitudinalEventCarrierV1>,
    pub content_inventory: Vec<LongitudinalInventoryEntryV1>,
    pub removed_content_sha256: Vec<String>,
    pub capacity_selectors: LongitudinalCapacitySelectorsV1,
    pub expected_semantic_receipts: Vec<LongitudinalExpectedSemanticReceiptV1>,
    pub schedule: Vec<LongitudinalOperationV1>,
    pub schedule_sha256: String,
    pub manifest_sha256: String,
}

impl LongitudinalWorkloadManifestV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.manifest_sha256, |manifest_sha256| {
            let mut preimage = self.clone();
            preimage.manifest_sha256 = manifest_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        let contract = longitudinal_runner_contract_v1();
        if self.schema != contract.schema
            || self.protocol != contract.protocol
            || self.contract_sha256 != contract.contract_sha256
            || self.public_seed_hex != contract.public_seed_hex
        {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        self.execution.validate()?;
        let tier = contract
            .tiers
            .iter()
            .find(|candidate| candidate.tier == self.tier)
            .ok_or(LongitudinalContractError::UnsupportedTier)?;
        require_count(self.event_count, tier.event_count, "manifest event count")?;
        require_count(
            self.revision_count,
            tier.revision_count,
            "manifest revision count",
        )?;
        require_len(
            self.ordered_events.len(),
            tier.event_count,
            "ordered event identities",
        )?;
        require_len(
            self.event_carriers.len(),
            tier.event_count,
            "event carrier inventory",
        )?;
        require_len(
            self.content_inventory.len(),
            tier.present_content_count,
            "content inventory",
        )?;
        require_len(
            self.removed_content_sha256.len(),
            tier.removed_content_count,
            "removed-content set",
        )?;
        require_exact_sequence(
            self.schedule.iter().copied(),
            LongitudinalOperationV1::ALL,
            "manifest schedule",
        )?;
        require_exact_sequence(
            self.expected_semantic_receipts
                .iter()
                .map(|receipt| receipt.operation),
            LongitudinalOperationV1::ALL,
            "expected semantic receipts",
        )?;
        if self.by_type != scaled_event_families(&contract.event_families, tier.block_count) {
            return Err(LongitudinalContractError::ContractDrift {
                field: "event-family counts",
            });
        }
        require_unique(
            self.ordered_events
                .iter()
                .map(|event| event.event_id.as_str()),
            "ordered event ids",
        )?;
        require_unique(
            self.event_carriers
                .iter()
                .map(|carrier| carrier.event_id.as_str()),
            "event carrier ids",
        )?;
        require_unique(
            self.content_inventory
                .iter()
                .map(|entry| entry.logical_key.as_str()),
            "content logical keys",
        )?;
        require_unique(
            self.removed_content_sha256.iter().map(String::as_str),
            "removed-content hashes",
        )?;
        for event in &self.ordered_events {
            validate_hex(
                &event.canonical_decoded_sha256,
                64,
                "canonical event SHA-256",
            )?;
        }
        for carrier in &self.event_carriers {
            validate_hex(&carrier.logical_key_sha256, 64, "event key SHA-256")?;
            validate_hex(&carrier.raw_sha256, 64, "event carrier SHA-256")?;
        }
        for content in &self.content_inventory {
            validate_hex(&content.raw_sha256, 64, "content raw SHA-256")?;
            validate_hex(&content.decoded_sha256, 64, "content decoded SHA-256")?;
        }
        for hash in &self.removed_content_sha256 {
            validate_hex(hash, 64, "removed-content SHA-256")?;
        }
        self.capacity_selectors.validate(
            self.event_count,
            &self.event_carriers,
            &self.content_inventory,
        )?;
        for receipt in &self.expected_semantic_receipts {
            validate_hex(
                &receipt.semantic_receipt_sha256,
                64,
                "semantic receipt SHA-256",
            )?;
        }
        validate_bound_hash(
            &self.schedule_sha256,
            &canonical_sha256(&self.schedule)?,
            "operation schedule",
        )?;
        validate_bound_hash(
            &self.manifest_sha256,
            &self.canonical_sha256()?,
            "workload manifest",
        )
    }
}

pub fn longitudinal_workload_manifest_carry_invariant_sha256_v1(
    manifest: &LongitudinalWorkloadManifestV1,
) -> Result<String, LongitudinalContractError> {
    manifest.validate()?;
    let mut value = serde_json::to_value(manifest).map_err(|error| {
        LongitudinalContractError::CanonicalJson {
            message: error.to_string(),
        }
    })?;
    let object = value
        .as_object_mut()
        .ok_or(LongitudinalContractError::ContractDrift {
            field: "carry-forward manifest representation",
        })?;
    if object.remove("execution").is_none() || object.remove("manifestSha256").is_none() {
        return Err(LongitudinalContractError::ContractDrift {
            field: "carry-forward manifest representation",
        });
    }
    let bytes =
        canonical_json_bytes(&value).map_err(|error| LongitudinalContractError::CanonicalJson {
            message: error.to_string(),
        })?;
    Ok(sha256_bytes_hex(&bytes))
}

pub fn longitudinal_workload_manifest_upgrade_invariant_sha256_v1(
    manifest: &LongitudinalWorkloadManifestV1,
    strict: &LongitudinalStrictSemanticReceiptV1,
    changed_event_ids: &[String],
) -> Result<String, LongitudinalContractError> {
    manifest.validate()?;
    validate_strict_receipt(strict)?;
    for receipt in &manifest.expected_semantic_receipts {
        validate_bound_hash(
            &receipt.semantic_receipt_sha256,
            &canonical_sha256(&(receipt.operation, strict, &manifest.ordered_events))?,
            "expected semantic receipt",
        )?;
    }
    require_unique(
        changed_event_ids.iter().map(String::as_str),
        "removal-upgrade invariant event ids",
    )?;
    let changed_event_ids = changed_event_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut value = serde_json::to_value(manifest).map_err(|error| {
        LongitudinalContractError::CanonicalJson {
            message: error.to_string(),
        }
    })?;
    let object = value
        .as_object_mut()
        .ok_or(LongitudinalContractError::ContractDrift {
            field: "removal-upgrade manifest representation",
        })?;
    if object.remove("execution").is_none() || object.remove("manifestSha256").is_none() {
        return Err(LongitudinalContractError::ContractDrift {
            field: "removal-upgrade manifest representation",
        });
    }
    let expected_semantic_receipts = object
        .get_mut("expectedSemanticReceipts")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or(LongitudinalContractError::ContractDrift {
            field: "removal-upgrade expected semantic receipts",
        })?;
    for receipt in expected_semantic_receipts {
        let receipt = receipt
            .as_object_mut()
            .ok_or(LongitudinalContractError::ContractDrift {
                field: "removal-upgrade expected semantic receipt",
            })?;
        if receipt.remove("semanticReceiptSha256").is_none() {
            return Err(LongitudinalContractError::ContractDrift {
                field: "removal-upgrade expected semantic receipt",
            });
        }
    }
    let ordered_events = object
        .get_mut("orderedEvents")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or(LongitudinalContractError::ContractDrift {
            field: "removal-upgrade ordered events",
        })?;
    for event in ordered_events {
        let event = event
            .as_object_mut()
            .ok_or(LongitudinalContractError::ContractDrift {
                field: "removal-upgrade ordered event identity",
            })?;
        let event_id = event
            .get("eventId")
            .and_then(serde_json::Value::as_str)
            .ok_or(LongitudinalContractError::ContractDrift {
                field: "removal-upgrade ordered event identity",
            })?;
        if changed_event_ids.contains(event_id) && event.remove("canonicalDecodedSha256").is_none()
        {
            return Err(LongitudinalContractError::ContractDrift {
                field: "removal-upgrade ordered event identity",
            });
        }
    }
    let carriers = object
        .get_mut("eventCarriers")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or(LongitudinalContractError::ContractDrift {
            field: "removal-upgrade event carriers",
        })?;
    for carrier in carriers {
        let carrier = carrier
            .as_object_mut()
            .ok_or(LongitudinalContractError::ContractDrift {
                field: "removal-upgrade event carrier",
            })?;
        let event_id = carrier
            .get("eventId")
            .and_then(serde_json::Value::as_str)
            .ok_or(LongitudinalContractError::ContractDrift {
                field: "removal-upgrade event carrier",
            })?;
        if changed_event_ids.contains(event_id)
            && (carrier.remove("rawSha256").is_none() || carrier.remove("rawBytes").is_none())
        {
            return Err(LongitudinalContractError::ContractDrift {
                field: "removal-upgrade event carrier bytes",
            });
        }
    }
    if changed_event_ids.len()
        != manifest
            .ordered_events
            .iter()
            .filter(|event| changed_event_ids.contains(event.event_id.as_str()))
            .count()
    {
        return Err(LongitudinalContractError::ContractDrift {
            field: "removal-upgrade invariant event ids",
        });
    }
    let bytes =
        canonical_json_bytes(&value).map_err(|error| LongitudinalContractError::CanonicalJson {
            message: error.to_string(),
        })?;
    Ok(sha256_bytes_hex(&bytes))
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalStrictSemanticReceiptV1 {
    pub event_set_sha256: String,
    pub ordered_journal_sha256: String,
    pub state_sha256: String,
    pub projection_sha256: String,
    pub content_inventory_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LongitudinalMaterializationSubjectV1 {
    Workload(LongitudinalTierV1),
    Capacity(LongitudinalCapacityProfileV1),
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "receipt", rename_all = "snake_case")]
pub enum LongitudinalResumedMaterializationV1 {
    Workload(LongitudinalMaterializationReceiptV1),
    Capacity(LongitudinalCapacityMaterializationReceiptV1),
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalStoreDataInventoryV1 {
    pub file_count: u64,
    pub byte_count: u64,
    pub inventory_sha256: String,
}

impl LongitudinalStoreDataInventoryV1 {
    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        validate_hex(&self.inventory_sha256, 64, "store-data inventory SHA-256")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalMaterializationResumeReceiptV1 {
    pub schema: String,
    pub subject: LongitudinalMaterializationSubjectV1,
    pub execution: LongitudinalExecutionIdentityV1,
    pub root_identity: String,
    pub pre_inventory: LongitudinalStoreDataInventoryV1,
    pub post_inventory: LongitudinalStoreDataInventoryV1,
    pub events_created: u64,
    pub events_existing: u64,
    pub event_count: u64,
    pub content_count: u64,
    pub strict: LongitudinalStrictSemanticReceiptV1,
    pub materialization_sha256: String,
    pub materialization: LongitudinalResumedMaterializationV1,
    pub receipt_sha256: String,
}

impl LongitudinalMaterializationResumeReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_MATERIALIZATION_RESUME_RECEIPT_SCHEMA_V1 {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        self.execution.validate()?;
        validate_hex(&self.root_identity, 64, "resume root identity")?;
        self.pre_inventory.validate()?;
        self.post_inventory.validate()?;
        validate_strict_receipt(&self.strict)?;
        validate_hex(
            &self.materialization_sha256,
            64,
            "resumed materialization SHA-256",
        )?;
        let (event_count, content_count) = materialization_subject_counts(self.subject)?;
        require_count(self.event_count, event_count, "resume event count")?;
        require_count(self.content_count, content_count, "resume content count")?;
        require_count(
            self.events_created
                .checked_add(self.events_existing)
                .ok_or(LongitudinalContractError::ContractDrift {
                    field: "resume write counts",
                })?,
            event_count,
            "resume write counts",
        )?;
        match (&self.subject, &self.materialization) {
            (
                LongitudinalMaterializationSubjectV1::Workload(tier),
                LongitudinalResumedMaterializationV1::Workload(materialization),
            ) => {
                materialization.validate()?;
                if materialization.manifest.tier != *tier
                    || materialization.manifest.execution != self.execution
                    || materialization.root_identity != self.root_identity
                    || materialization.manifest.event_count != self.event_count
                    || materialization.manifest.content_inventory.len() as u64 != self.content_count
                    || materialization.strict != self.strict
                    || materialization.materialization_sha256 != self.materialization_sha256
                {
                    return Err(LongitudinalContractError::PairMismatch);
                }
            }
            (
                LongitudinalMaterializationSubjectV1::Capacity(profile),
                LongitudinalResumedMaterializationV1::Capacity(materialization),
            ) => {
                materialization.validate()?;
                if materialization.manifest.subject
                    != LongitudinalCapacitySubjectV1::Companion(*profile)
                    || materialization.manifest.execution != self.execution
                    || materialization.root_identity != self.root_identity
                    || materialization.manifest.event_count != self.event_count
                    || materialization.manifest.content_inventory.len() as u64 != self.content_count
                    || materialization.strict != self.strict
                    || materialization.materialization_sha256 != self.materialization_sha256
                {
                    return Err(LongitudinalContractError::PairMismatch);
                }
            }
            _ => return Err(LongitudinalContractError::PairMismatch),
        }
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "materialization resume receipt",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalMaterializerRootReceiptV1 {
    pub subject: LongitudinalMaterializationSubjectV1,
    pub execution: LongitudinalExecutionIdentityV1,
    pub root_identity: String,
    pub inventory: LongitudinalStoreDataInventoryV1,
    pub event_count: u64,
    pub content_count: u64,
    pub strict: LongitudinalStrictSemanticReceiptV1,
    pub materialization_sha256: String,
}

impl LongitudinalMaterializerRootReceiptV1 {
    fn validate(&self) -> Result<(), LongitudinalContractError> {
        self.execution.validate()?;
        validate_hex(&self.root_identity, 64, "equivalence root identity")?;
        self.inventory.validate()?;
        validate_strict_receipt(&self.strict)?;
        validate_hex(
            &self.materialization_sha256,
            64,
            "equivalence materialization SHA-256",
        )?;
        let (event_count, content_count) = materialization_subject_counts(self.subject)?;
        require_count(self.event_count, event_count, "equivalence event count")?;
        require_count(
            self.content_count,
            content_count,
            "equivalence content count",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalMaterializerEquivalenceReceiptV1 {
    pub schema: String,
    pub baseline: LongitudinalMaterializerRootReceiptV1,
    pub successor: LongitudinalMaterializerRootReceiptV1,
    pub implementation_diff_sha256: String,
    pub equivalent: bool,
    pub receipt_sha256: String,
}

impl LongitudinalMaterializerEquivalenceReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_MATERIALIZER_EQUIVALENCE_RECEIPT_SCHEMA_V1 {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        self.baseline.validate()?;
        self.successor.validate()?;
        validate_hex(
            &self.implementation_diff_sha256,
            64,
            "implementation diff SHA-256",
        )?;
        if !self.equivalent
            || self.baseline.root_identity == self.successor.root_identity
            || self.baseline.subject != self.successor.subject
            || self.baseline.event_count != self.successor.event_count
            || self.baseline.content_count != self.successor.content_count
            || self.baseline.strict != self.successor.strict
            || self.baseline.inventory != self.successor.inventory
        {
            return Err(LongitudinalContractError::PairMismatch);
        }
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "materializer equivalence receipt",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCarryForwardReceiptV1 {
    pub schema: String,
    pub slot: LongitudinalCarryForwardSlotV1,
    pub contract_sha256: String,
    pub schedule_sha256: String,
    pub source_execution: LongitudinalExecutionIdentityV1,
    pub corrected_execution: LongitudinalExecutionIdentityV1,
    pub source_root_identity: String,
    pub clone_root_identity: String,
    pub source_pre_inventory: LongitudinalStoreDataInventoryV1,
    pub source_post_inventory: LongitudinalStoreDataInventoryV1,
    pub clone_pre_inventory: LongitudinalStoreDataInventoryV1,
    pub clone_post_inventory: LongitudinalStoreDataInventoryV1,
    pub source_strict: LongitudinalStrictSemanticReceiptV1,
    pub clone_strict: LongitudinalStrictSemanticReceiptV1,
    pub event_count: u64,
    pub content_count: u64,
    pub source_manifest_sha256: String,
    pub carried_manifest_sha256: String,
    pub source_manifest_invariant_sha256: String,
    pub carried_manifest_invariant_sha256: String,
    pub source_materialization_sha256: String,
    pub carried_materialization_sha256: String,
    pub materializer_equivalence: LongitudinalMaterializerEquivalenceReceiptV1,
    pub final_authority_diff_sha256: String,
    pub receipt_sha256: String,
}

impl LongitudinalCarryForwardReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_CARRY_FORWARD_RECEIPT_SCHEMA_V1
            || self.contract_sha256 != LONGITUDINAL_RUNNER_CONTRACT_SHA256_V1
        {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        self.slot.validate()?;
        self.source_execution.validate()?;
        self.corrected_execution.validate()?;
        if self.source_execution == self.corrected_execution
            || self.source_execution.parent_commit.is_some()
            || self.corrected_execution.parent_commit.is_some()
            || self.source_execution.build_profile != carry_forward_lane_profile_v1(self.slot.lane)?
        {
            return Err(LongitudinalContractError::MixedRevision);
        }
        validate_hex(
            &self.source_root_identity,
            64,
            "carry-forward source root identity",
        )?;
        validate_hex(
            &self.clone_root_identity,
            64,
            "carry-forward clone root identity",
        )?;
        if self.source_root_identity == self.clone_root_identity {
            return Err(LongitudinalContractError::RootIdentityNotDistinct);
        }
        for inventory in [
            &self.source_pre_inventory,
            &self.source_post_inventory,
            &self.clone_pre_inventory,
            &self.clone_post_inventory,
        ] {
            inventory.validate()?;
        }
        if self.source_pre_inventory != self.source_post_inventory
            || self.source_pre_inventory != self.clone_pre_inventory
            || self.source_pre_inventory != self.clone_post_inventory
        {
            return Err(LongitudinalContractError::PairMismatch);
        }
        validate_strict_receipt(&self.source_strict)?;
        validate_strict_receipt(&self.clone_strict)?;
        if self.source_strict != self.clone_strict {
            return Err(LongitudinalContractError::PairMismatch);
        }
        let requirement = longitudinal_runner_contract_v1()
            .tiers
            .into_iter()
            .find(|requirement| requirement.tier == self.slot.tier)
            .ok_or(LongitudinalContractError::UnsupportedTier)?;
        require_count(
            self.event_count,
            requirement.event_count,
            "carry-forward event count",
        )?;
        require_count(
            self.content_count,
            requirement.present_content_count,
            "carry-forward content count",
        )?;
        validate_bound_hash(
            &self.schedule_sha256,
            &canonical_sha256(&LongitudinalOperationV1::ALL)?,
            "carry-forward schedule",
        )?;
        for (hash, field) in [
            (
                &self.source_manifest_sha256,
                "carry-forward source manifest SHA-256",
            ),
            (
                &self.carried_manifest_sha256,
                "carry-forward carried manifest SHA-256",
            ),
            (
                &self.source_manifest_invariant_sha256,
                "carry-forward source manifest invariant SHA-256",
            ),
            (
                &self.carried_manifest_invariant_sha256,
                "carry-forward carried manifest invariant SHA-256",
            ),
            (
                &self.source_materialization_sha256,
                "carry-forward source materialization SHA-256",
            ),
            (
                &self.carried_materialization_sha256,
                "carry-forward carried materialization SHA-256",
            ),
            (
                &self.final_authority_diff_sha256,
                "carry-forward final authority diff SHA-256",
            ),
        ] {
            validate_hex(hash, 64, field)?;
        }
        if self.source_manifest_invariant_sha256 != self.carried_manifest_invariant_sha256 {
            return Err(LongitudinalContractError::ContractDrift {
                field: "carry-forward manifest invariant",
            });
        }
        self.materializer_equivalence.validate()?;
        let baseline = &self.materializer_equivalence.baseline;
        let successor = &self.materializer_equivalence.successor;
        if baseline.subject != LongitudinalMaterializationSubjectV1::Workload(self.slot.tier)
            || successor.execution.parent_commit.is_some()
            || baseline.inventory != self.source_pre_inventory
            || baseline.strict != self.source_strict
            || baseline.event_count != self.event_count
            || baseline.content_count != self.content_count
            || successor.subject != LongitudinalMaterializationSubjectV1::Workload(self.slot.tier)
            || successor.inventory != self.clone_post_inventory
            || successor.strict != self.clone_strict
            || successor.event_count != self.event_count
            || successor.content_count != self.content_count
            || self.materializer_equivalence.implementation_diff_sha256
                != self.final_authority_diff_sha256
        {
            return Err(LongitudinalContractError::PairMismatch);
        }
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "carry-forward receipt",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalRemovalUpgradePathV1 {
    pub relative_path: String,
    pub event_id: String,
    pub event_record_hash: String,
    pub payload_hash: String,
    pub before_sha256: String,
    pub before_bytes: u64,
    pub after_sha256: String,
    pub after_bytes: u64,
}

impl LongitudinalRemovalUpgradePathV1 {
    fn validate(&self) -> Result<(), LongitudinalContractError> {
        validate_relative_path(&self.relative_path)?;
        if !self.relative_path.starts_with(".pointbreak/data/events/")
            || !self.relative_path.ends_with(".json")
            || self.before_sha256 == self.after_sha256
            || self.before_bytes == self.after_bytes
        {
            return Err(LongitudinalContractError::ContractDrift {
                field: "removal-upgrade changed path",
            });
        }
        validate_prefixed_hash(&self.event_id, "evt:sha256:", "removal-upgrade event id")?;
        validate_prefixed_hash(
            &self.event_record_hash,
            "sha256:",
            "removal-upgrade event record hash",
        )?;
        validate_prefixed_hash(
            &self.payload_hash,
            "sha256:",
            "removal-upgrade payload hash",
        )?;
        validate_hex(&self.before_sha256, 64, "removal-upgrade before SHA-256")?;
        validate_hex(&self.after_sha256, 64, "removal-upgrade after SHA-256")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalRemovalUpgradeReceiptV1 {
    pub schema: String,
    pub slot: LongitudinalCarryForwardSlotV1,
    pub contract_sha256: String,
    pub schedule_sha256: String,
    pub source_execution: LongitudinalExecutionIdentityV1,
    pub corrected_execution: LongitudinalExecutionIdentityV1,
    pub source_root_identity: String,
    pub successor_root_identity: String,
    pub source_pre_inventory: LongitudinalStoreDataInventoryV1,
    pub source_post_inventory: LongitudinalStoreDataInventoryV1,
    pub successor_pre_inventory: LongitudinalStoreDataInventoryV1,
    pub successor_post_inventory: LongitudinalStoreDataInventoryV1,
    pub source_strict: LongitudinalStrictSemanticReceiptV1,
    pub successor_strict: LongitudinalStrictSemanticReceiptV1,
    pub event_count: u64,
    pub content_count: u64,
    pub removed_content_count: u64,
    pub changed_paths: Vec<LongitudinalRemovalUpgradePathV1>,
    pub enrollment_relative_path: String,
    pub enrollment_sha256: String,
    pub enrollment_bytes: u64,
    pub source_manifest_sha256: String,
    pub corrected_manifest_sha256: String,
    pub source_manifest_invariant_sha256: String,
    pub corrected_manifest_invariant_sha256: String,
    pub source_materialization_sha256: String,
    pub corrected_materialization_sha256: String,
    pub resume_receipt_sha256: String,
    pub resume_events_created: u64,
    pub resume_events_existing: u64,
    pub final_authority_diff_sha256: String,
    pub receipt_sha256: String,
}

impl LongitudinalRemovalUpgradeReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_REMOVAL_UPGRADE_RECEIPT_SCHEMA_V1
            || self.contract_sha256 != LONGITUDINAL_RUNNER_CONTRACT_SHA256_V1
        {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        self.slot.validate()?;
        self.source_execution.validate()?;
        self.corrected_execution.validate()?;
        if self.source_execution == self.corrected_execution
            || self.source_execution.parent_commit.is_some()
            || self.corrected_execution.parent_commit.is_some()
            || self.source_execution.build_profile != carry_forward_lane_profile_v1(self.slot.lane)?
        {
            return Err(LongitudinalContractError::MixedRevision);
        }
        validate_hex(
            &self.source_root_identity,
            64,
            "removal-upgrade source root identity",
        )?;
        validate_hex(
            &self.successor_root_identity,
            64,
            "removal-upgrade successor root identity",
        )?;
        if self.source_root_identity == self.successor_root_identity {
            return Err(LongitudinalContractError::RootIdentityNotDistinct);
        }
        for inventory in [
            &self.source_pre_inventory,
            &self.source_post_inventory,
            &self.successor_pre_inventory,
            &self.successor_post_inventory,
        ] {
            inventory.validate()?;
        }
        if self.source_pre_inventory != self.source_post_inventory
            || self.source_pre_inventory != self.successor_pre_inventory
            || self.successor_pre_inventory == self.successor_post_inventory
        {
            return Err(LongitudinalContractError::PairMismatch);
        }
        validate_strict_receipt(&self.source_strict)?;
        validate_strict_receipt(&self.successor_strict)?;
        if self.source_strict != self.successor_strict {
            return Err(LongitudinalContractError::PairMismatch);
        }
        let requirement = longitudinal_runner_contract_v1()
            .tiers
            .into_iter()
            .find(|requirement| requirement.tier == self.slot.tier)
            .ok_or(LongitudinalContractError::UnsupportedTier)?;
        require_count(
            self.event_count,
            requirement.event_count,
            "removal-upgrade event count",
        )?;
        require_count(
            self.content_count,
            requirement.present_content_count,
            "removal-upgrade content count",
        )?;
        require_count(
            self.removed_content_count,
            requirement.removed_content_count,
            "removal-upgrade removed-content count",
        )?;
        require_len(
            self.changed_paths.len(),
            requirement.removed_content_count,
            "removal-upgrade changed paths",
        )?;
        require_unique(
            self.changed_paths
                .iter()
                .map(|path| path.relative_path.as_str()),
            "removal-upgrade changed paths",
        )?;
        require_unique(
            self.changed_paths.iter().map(|path| path.event_id.as_str()),
            "removal-upgrade event ids",
        )?;
        if !self
            .changed_paths
            .windows(2)
            .all(|pair| pair[0].relative_path < pair[1].relative_path)
        {
            return Err(LongitudinalContractError::ContractDrift {
                field: "removal-upgrade changed-path order",
            });
        }
        for path in &self.changed_paths {
            path.validate()?;
        }
        if self.enrollment_relative_path != ".pointbreak/allowed-signers.json"
            || self.enrollment_bytes == 0
        {
            return Err(LongitudinalContractError::ContractDrift {
                field: "removal-upgrade signer enrollment",
            });
        }
        validate_hex(
            &self.enrollment_sha256,
            64,
            "removal-upgrade enrollment SHA-256",
        )?;
        validate_bound_hash(
            &self.schedule_sha256,
            &canonical_sha256(&LongitudinalOperationV1::ALL)?,
            "removal-upgrade schedule",
        )?;
        for (hash, field) in [
            (
                &self.source_manifest_sha256,
                "removal-upgrade source manifest SHA-256",
            ),
            (
                &self.corrected_manifest_sha256,
                "removal-upgrade corrected manifest SHA-256",
            ),
            (
                &self.source_manifest_invariant_sha256,
                "removal-upgrade source manifest invariant SHA-256",
            ),
            (
                &self.corrected_manifest_invariant_sha256,
                "removal-upgrade corrected manifest invariant SHA-256",
            ),
            (
                &self.source_materialization_sha256,
                "removal-upgrade source materialization SHA-256",
            ),
            (
                &self.corrected_materialization_sha256,
                "removal-upgrade corrected materialization SHA-256",
            ),
            (
                &self.resume_receipt_sha256,
                "removal-upgrade resume receipt SHA-256",
            ),
            (
                &self.final_authority_diff_sha256,
                "removal-upgrade final authority diff SHA-256",
            ),
        ] {
            validate_hex(hash, 64, field)?;
        }
        if self.source_manifest_invariant_sha256 != self.corrected_manifest_invariant_sha256 {
            return Err(LongitudinalContractError::ContractDrift {
                field: "removal-upgrade manifest invariant",
            });
        }
        if self.resume_events_created != 0 || self.resume_events_existing != self.event_count {
            return Err(LongitudinalContractError::PairMismatch);
        }
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "removal-upgrade receipt",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalPackageVerificationReceiptV1 {
    pub schema: String,
    pub package_kind: LongitudinalVerifiedPackageKindV1,
    pub package_sha256: String,
    pub contract_sha256: String,
    pub execution_identity_sha256: String,
    pub raw_inventory_sha256: String,
    pub verified: bool,
    pub receipt_sha256: String,
}

impl LongitudinalPackageVerificationReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_PACKAGE_VERIFICATION_RECEIPT_SCHEMA_V1 || !self.verified {
            return Err(LongitudinalContractError::IncompleteEvidence);
        }
        let expected_contract = match self.package_kind {
            LongitudinalVerifiedPackageKindV1::Workload => LONGITUDINAL_RUNNER_CONTRACT_SHA256_V1,
            LongitudinalVerifiedPackageKindV1::Capacity => LONGITUDINAL_CAPACITY_CONTRACT_SHA256_V1,
        };
        if self.contract_sha256 != expected_contract {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        for (hash, field) in [
            (&self.package_sha256, "verified package SHA-256"),
            (
                &self.execution_identity_sha256,
                "verified package execution identity SHA-256",
            ),
            (
                &self.raw_inventory_sha256,
                "verified package raw inventory SHA-256",
            ),
        ] {
            validate_hex(hash, 64, field)?;
        }
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "package verification receipt",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCarryForwardAuthorityCompletionV1 {
    pub final_workload_package_sha256: String,
    pub verification_receipt: LongitudinalPackageVerificationReceiptV1,
    pub package_verified: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCarryForwardAuthorityPackageV1 {
    pub schema: String,
    pub contract_sha256: String,
    pub corrected_execution: LongitudinalExecutionIdentityV1,
    pub carry_receipts: Vec<LongitudinalCarryForwardReceiptV1>,
    pub authority_set_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<LongitudinalCarryForwardAuthorityCompletionV1>,
    pub package_sha256: String,
}

impl LongitudinalCarryForwardAuthorityPackageV1 {
    pub fn canonical_authority_set_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256(&(
            &self.contract_sha256,
            &self.corrected_execution,
            &self.carry_receipts,
        ))
    }

    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.package_sha256, |package_sha256| {
            let mut preimage = self.clone();
            preimage.package_sha256 = package_sha256;
            preimage
        })
    }

    pub fn validate_roots(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_CARRY_FORWARD_AUTHORITY_PACKAGE_SCHEMA_V1
            || self.contract_sha256 != LONGITUDINAL_RUNNER_CONTRACT_SHA256_V1
        {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        self.corrected_execution.validate()?;
        if self.corrected_execution.parent_commit.is_some() {
            return Err(LongitudinalContractError::MixedRevision);
        }
        let expected = LongitudinalTierV1::ALL
            .into_iter()
            .flat_map(|tier| {
                [
                    LongitudinalCarryForwardSlotV1 {
                        tier,
                        lane: LongitudinalLaneV1::ReleaseUninstrumented,
                        independent_run: 1,
                    },
                    LongitudinalCarryForwardSlotV1 {
                        tier,
                        lane: LongitudinalLaneV1::ReleaseUninstrumented,
                        independent_run: 2,
                    },
                    LongitudinalCarryForwardSlotV1 {
                        tier,
                        lane: LongitudinalLaneV1::DebugUninstrumented,
                        independent_run: 1,
                    },
                ]
            })
            .collect::<Vec<_>>();
        let mut actual = Vec::new();
        let mut unique_slots = BTreeSet::new();
        let mut source_roots = BTreeSet::new();
        let mut clone_roots = BTreeSet::new();
        let mut common_source_execution = None;
        let mut source_execution_by_lane = BTreeMap::new();
        let mut equivalence_by_tier = BTreeMap::new();
        let mut equivalence_baseline_execution = None;
        let mut equivalence_successor_execution = None;
        let mut final_authority_diff_sha256 = None;
        for receipt in &self.carry_receipts {
            receipt.validate()?;
            if receipt.corrected_execution != self.corrected_execution
                || !unique_slots.insert(receipt.slot)
                || !source_roots.insert(receipt.source_root_identity.as_str())
                || !clone_roots.insert(receipt.clone_root_identity.as_str())
            {
                return Err(LongitudinalContractError::PairMismatch);
            }
            let source_common = execution_without_lane_identity_v1(&receipt.source_execution);
            if common_source_execution.get_or_insert_with(|| source_common.clone())
                != &source_common
                || source_execution_by_lane
                    .entry(receipt.slot.lane)
                    .or_insert_with(|| receipt.source_execution.clone())
                    != &receipt.source_execution
                || equivalence_by_tier
                    .entry(receipt.slot.tier)
                    .or_insert_with(|| receipt.materializer_equivalence.clone())
                    != &receipt.materializer_equivalence
                || equivalence_baseline_execution.get_or_insert_with(|| {
                    receipt.materializer_equivalence.baseline.execution.clone()
                }) != &receipt.materializer_equivalence.baseline.execution
                || equivalence_successor_execution.get_or_insert_with(|| {
                    receipt.materializer_equivalence.successor.execution.clone()
                }) != &receipt.materializer_equivalence.successor.execution
                || final_authority_diff_sha256
                    .get_or_insert_with(|| receipt.final_authority_diff_sha256.clone())
                    != &receipt.final_authority_diff_sha256
            {
                return Err(LongitudinalContractError::PairMismatch);
            }
            actual.push(receipt.slot);
        }
        if actual != expected || !source_roots.is_disjoint(&clone_roots) {
            return Err(LongitudinalContractError::IncompleteEvidence);
        }
        validate_bound_hash(
            &self.authority_set_sha256,
            &self.canonical_authority_set_sha256()?,
            "carry-forward authority set",
        )
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        self.validate_roots()?;
        let completion = self
            .completion
            .as_ref()
            .ok_or(LongitudinalContractError::IncompleteEvidence)?;
        completion.verification_receipt.validate()?;
        if !completion.package_verified
            || completion.verification_receipt.package_kind
                != LongitudinalVerifiedPackageKindV1::Workload
            || completion.final_workload_package_sha256
                != completion.verification_receipt.package_sha256
            || completion.verification_receipt.execution_identity_sha256
                != self.corrected_execution.canonical_sha256()?
        {
            return Err(LongitudinalContractError::IncompleteEvidence);
        }
        validate_bound_hash(
            &self.package_sha256,
            &self.canonical_sha256()?,
            "carry-forward authority package",
        )
    }

    pub fn validate_incomplete(&self) -> Result<(), LongitudinalContractError> {
        self.validate_roots()?;
        if self.completion.is_some() {
            return Err(LongitudinalContractError::IncompleteEvidence);
        }
        validate_bound_hash(
            &self.package_sha256,
            &self.canonical_sha256()?,
            "incomplete carry-forward authority package",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalRemovalUpgradeAuthorityCompletionV1 {
    pub final_workload_package_sha256: String,
    pub verification_receipt: LongitudinalPackageVerificationReceiptV1,
    pub package_verified: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalRemovalUpgradeAuthorityPackageV1 {
    pub schema: String,
    pub contract_sha256: String,
    pub corrected_execution: LongitudinalExecutionIdentityV1,
    pub upgrade_receipts: Vec<LongitudinalRemovalUpgradeReceiptV1>,
    pub materializer_equivalences: Vec<LongitudinalMaterializerEquivalenceReceiptV1>,
    pub authority_set_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<LongitudinalRemovalUpgradeAuthorityCompletionV1>,
    pub package_sha256: String,
}

impl LongitudinalRemovalUpgradeAuthorityPackageV1 {
    pub fn canonical_authority_set_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256(&(
            &self.contract_sha256,
            &self.corrected_execution,
            &self.upgrade_receipts,
            &self.materializer_equivalences,
        ))
    }

    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.package_sha256, |package_sha256| {
            let mut preimage = self.clone();
            preimage.package_sha256 = package_sha256;
            preimage
        })
    }

    pub fn validate_roots(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_REMOVAL_UPGRADE_AUTHORITY_PACKAGE_SCHEMA_V1
            || self.contract_sha256 != LONGITUDINAL_RUNNER_CONTRACT_SHA256_V1
        {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        self.corrected_execution.validate()?;
        if self.corrected_execution.parent_commit.is_some() {
            return Err(LongitudinalContractError::MixedRevision);
        }
        let expected_slots = LongitudinalTierV1::ALL
            .into_iter()
            .flat_map(|tier| {
                [
                    LongitudinalCarryForwardSlotV1 {
                        tier,
                        lane: LongitudinalLaneV1::ReleaseUninstrumented,
                        independent_run: 1,
                    },
                    LongitudinalCarryForwardSlotV1 {
                        tier,
                        lane: LongitudinalLaneV1::ReleaseUninstrumented,
                        independent_run: 2,
                    },
                    LongitudinalCarryForwardSlotV1 {
                        tier,
                        lane: LongitudinalLaneV1::DebugUninstrumented,
                        independent_run: 1,
                    },
                ]
            })
            .collect::<Vec<_>>();
        if self
            .upgrade_receipts
            .iter()
            .map(|receipt| receipt.slot)
            .ne(expected_slots.iter().copied())
        {
            return Err(LongitudinalContractError::IncompleteEvidence);
        }
        let mut source_roots = BTreeSet::new();
        let mut successor_roots = BTreeSet::new();
        let mut source_execution_by_lane = BTreeMap::new();
        let mut common_source_execution = None;
        let mut final_authority_diff_sha256 = None;
        for receipt in &self.upgrade_receipts {
            receipt.validate()?;
            if receipt.corrected_execution != self.corrected_execution
                || !source_roots.insert(receipt.source_root_identity.as_str())
                || !successor_roots.insert(receipt.successor_root_identity.as_str())
            {
                return Err(LongitudinalContractError::PairMismatch);
            }
            let source_common = execution_without_lane_identity_v1(&receipt.source_execution);
            if common_source_execution.get_or_insert_with(|| source_common.clone())
                != &source_common
                || source_execution_by_lane
                    .entry(receipt.slot.lane)
                    .or_insert_with(|| receipt.source_execution.clone())
                    != &receipt.source_execution
                || final_authority_diff_sha256
                    .get_or_insert_with(|| receipt.final_authority_diff_sha256.clone())
                    != &receipt.final_authority_diff_sha256
            {
                return Err(LongitudinalContractError::PairMismatch);
            }
        }
        if !source_roots.is_disjoint(&successor_roots)
            || self.materializer_equivalences.len() != LongitudinalTierV1::ALL.len()
        {
            return Err(LongitudinalContractError::IncompleteEvidence);
        }
        for (tier, equivalence) in LongitudinalTierV1::ALL
            .into_iter()
            .zip(&self.materializer_equivalences)
        {
            equivalence.validate()?;
            let release_one = self
                .upgrade_receipts
                .iter()
                .find(|receipt| {
                    receipt.slot
                        == LongitudinalCarryForwardSlotV1 {
                            tier,
                            lane: LongitudinalLaneV1::ReleaseUninstrumented,
                            independent_run: 1,
                        }
                })
                .ok_or(LongitudinalContractError::IncompleteEvidence)?;
            if equivalence.baseline.subject != LongitudinalMaterializationSubjectV1::Workload(tier)
                || equivalence.baseline.execution != self.corrected_execution
                || equivalence.baseline.root_identity != release_one.successor_root_identity
                || equivalence.baseline.inventory != release_one.successor_post_inventory
                || equivalence.baseline.strict != release_one.successor_strict
                || equivalence.baseline.materialization_sha256
                    != release_one.corrected_materialization_sha256
                || equivalence.successor.subject
                    != LongitudinalMaterializationSubjectV1::Workload(tier)
                || equivalence.successor.execution != self.corrected_execution
                || successor_roots.contains(equivalence.successor.root_identity.as_str())
                || Some(&equivalence.implementation_diff_sha256)
                    != final_authority_diff_sha256.as_ref()
            {
                return Err(LongitudinalContractError::PairMismatch);
            }
        }
        validate_bound_hash(
            &self.authority_set_sha256,
            &self.canonical_authority_set_sha256()?,
            "removal-upgrade authority set",
        )
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        self.validate_roots()?;
        let completion = self
            .completion
            .as_ref()
            .ok_or(LongitudinalContractError::IncompleteEvidence)?;
        completion.verification_receipt.validate()?;
        if !completion.package_verified
            || completion.verification_receipt.package_kind
                != LongitudinalVerifiedPackageKindV1::Workload
            || completion.final_workload_package_sha256
                != completion.verification_receipt.package_sha256
            || completion.verification_receipt.execution_identity_sha256
                != self.corrected_execution.canonical_sha256()?
        {
            return Err(LongitudinalContractError::IncompleteEvidence);
        }
        validate_bound_hash(
            &self.package_sha256,
            &self.canonical_sha256()?,
            "removal-upgrade authority package",
        )
    }

    pub fn validate_incomplete(&self) -> Result<(), LongitudinalContractError> {
        self.validate_roots()?;
        if self.completion.is_some() {
            return Err(LongitudinalContractError::IncompleteEvidence);
        }
        validate_bound_hash(
            &self.package_sha256,
            &self.canonical_sha256()?,
            "incomplete removal-upgrade authority package",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalHttpFailureV1 {
    pub status: u16,
    pub body_classification: LongitudinalHttpBodyClassificationV1,
    pub body_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_sha256: Option<String>,
}

impl LongitudinalHttpFailureV1 {
    fn validate(&self) -> Result<(), LongitudinalContractError> {
        if !(100..=599).contains(&self.status) {
            return Err(LongitudinalContractError::ContractDrift {
                field: "controller failure HTTP status",
            });
        }
        match (self.body_bytes, &self.body_sha256) {
            (0, None)
                if self.body_classification == LongitudinalHttpBodyClassificationV1::Empty =>
            {
                Ok(())
            }
            (bytes, Some(hash))
                if bytes > 0
                    && self.body_classification != LongitudinalHttpBodyClassificationV1::Empty =>
            {
                validate_hex(hash, 64, "controller failure HTTP body SHA-256")
            }
            _ => Err(LongitudinalContractError::ContractDrift {
                field: "controller failure HTTP body",
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalStderrFailureV1 {
    pub classification: LongitudinalStderrClassificationV1,
    pub bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl LongitudinalStderrFailureV1 {
    fn validate(&self) -> Result<(), LongitudinalContractError> {
        match (self.classification, self.bytes, &self.sha256) {
            (LongitudinalStderrClassificationV1::NotCaptured, 0, None)
            | (LongitudinalStderrClassificationV1::Empty, 0, None) => Ok(()),
            (classification, bytes, Some(hash))
                if bytes > 0
                    && !matches!(
                        classification,
                        LongitudinalStderrClassificationV1::NotCaptured
                            | LongitudinalStderrClassificationV1::Empty
                    ) =>
            {
                validate_hex(hash, 64, "controller failure stderr SHA-256")
            }
            _ => Err(LongitudinalContractError::ContractDrift {
                field: "controller failure stderr",
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalControllerFailureReceiptV1 {
    pub schema: String,
    pub operation_selector: LongitudinalFailureOperationSelectorV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<LongitudinalHttpFailureV1>,
    pub inspector_exit: LongitudinalInspectorExitV1,
    pub stderr: LongitudinalStderrFailureV1,
    pub immutable: bool,
    pub receipt_sha256: String,
}

impl LongitudinalControllerFailureReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_CONTROLLER_FAILURE_RECEIPT_SCHEMA_V1 {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        if !self.immutable {
            return Err(LongitudinalContractError::MutableFailure);
        }
        if let Some(http) = &self.http {
            http.validate()?;
        }
        self.stderr.validate()?;
        if self.http.is_none()
            && self.inspector_exit == LongitudinalInspectorExitV1::NotStarted
            && self.stderr.classification == LongitudinalStderrClassificationV1::NotCaptured
        {
            return Err(LongitudinalContractError::IncompleteEvidence);
        }
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "controller failure receipt",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalMaterializationReceiptV1 {
    pub schema: String,
    pub root_identity: String,
    pub manifest: LongitudinalWorkloadManifestV1,
    pub strict: LongitudinalStrictSemanticReceiptV1,
    pub materialization_sha256: String,
}

impl LongitudinalMaterializationReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.materialization_sha256, |materialization_sha256| {
            let mut preimage = self.clone();
            preimage.materialization_sha256 = materialization_sha256;
            preimage
        })
    }

    fn pair_identity_sha256(&self) -> Result<String, LongitudinalContractError> {
        let mut preimage = self.clone();
        preimage.root_identity.clear();
        preimage.materialization_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_MATERIALIZATION_RECEIPT_SCHEMA_V1 {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        validate_hex(&self.root_identity, 64, "root identity")?;
        self.manifest.validate()?;
        validate_strict_receipt(&self.strict)?;
        validate_bound_hash(
            &self.materialization_sha256,
            &self.canonical_sha256()?,
            "materialization receipt",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalSemanticFactsV1 {
    pub ordered_ids: Vec<String>,
    pub fact_ids: Vec<String>,
    pub diagnostics: Vec<String>,
    pub event_count: u64,
    pub event_set_sha256: String,
    pub content_state_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalOperationMetricsV1 {
    pub wall_nanos: u64,
    pub process_user_cpu_nanos: u64,
    pub process_system_cpu_nanos: u64,
    pub baseline_rss_bytes: u64,
    pub peak_rss_bytes: u64,
    pub steady_rss_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalOperationReceiptV1 {
    pub schema: String,
    pub run_identity: String,
    pub root_identity: String,
    pub tier: LongitudinalTierV1,
    pub manifest_sha256: String,
    pub schedule_sha256: String,
    pub execution_identity_sha256: String,
    pub lane: LongitudinalLaneV1,
    pub operation: LongitudinalOperationV1,
    pub outcome: LongitudinalOperationOutcomeV1,
    pub sample_ordinal: u32,
    pub pre_event_count: u64,
    pub post_event_count: u64,
    pub pre_event_set_sha256: String,
    pub post_event_set_sha256: String,
    pub response_bytes: u64,
    pub response_canonical_sha256: String,
    pub semantic_facts: LongitudinalSemanticFactsV1,
    pub semantic_receipt_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<LongitudinalOperationMetricsV1>,
    pub receipt_sha256: String,
}

impl LongitudinalOperationReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_OPERATION_RECEIPT_SCHEMA_V1
            || self.lane == LongitudinalLaneV1::AttributionCounts
        {
            return Err(LongitudinalContractError::MixedLane);
        }
        for (hash, name) in [
            (&self.run_identity, "run identity"),
            (&self.root_identity, "root identity"),
            (&self.manifest_sha256, "manifest SHA-256"),
            (&self.schedule_sha256, "schedule SHA-256"),
            (
                &self.execution_identity_sha256,
                "execution identity SHA-256",
            ),
            (&self.pre_event_set_sha256, "pre-event-set SHA-256"),
            (&self.post_event_set_sha256, "post-event-set SHA-256"),
            (&self.response_canonical_sha256, "response SHA-256"),
            (&self.semantic_receipt_sha256, "semantic receipt SHA-256"),
        ] {
            validate_hex(hash, 64, name)?;
        }
        if self.semantic_facts.event_count != self.post_event_count
            || self.semantic_facts.event_set_sha256 != self.post_event_set_sha256
        {
            return Err(LongitudinalContractError::ContractDrift {
                field: "operation semantic state",
            });
        }
        match self.outcome {
            LongitudinalOperationOutcomeV1::Measured => {
                let metrics =
                    self.metrics
                        .as_ref()
                        .ok_or(LongitudinalContractError::ContractDrift {
                            field: "measured operation metrics",
                        })?;
                if metrics.wall_nanos == 0
                    || metrics.peak_rss_bytes < metrics.baseline_rss_bytes
                    || metrics.peak_rss_bytes < metrics.steady_rss_bytes
                {
                    return Err(LongitudinalContractError::ContractDrift {
                        field: "measured operation metrics",
                    });
                }
            }
            LongitudinalOperationOutcomeV1::UnavailableNoCurrentSurface => {
                if self.metrics.is_some()
                    || self.sample_ordinal != 0
                    || self.pre_event_count != self.post_event_count
                    || self.pre_event_set_sha256 != self.post_event_set_sha256
                    || self.semantic_facts.diagnostics.as_slice()
                        != ["unsupported_no_current_surface"]
                {
                    return Err(LongitudinalContractError::ContractDrift {
                        field: "unavailable operation receipt",
                    });
                }
            }
            LongitudinalOperationOutcomeV1::ValidFailure => {
                if self.metrics.is_some() || self.semantic_facts.diagnostics.is_empty() {
                    return Err(LongitudinalContractError::ContractDrift {
                        field: "failed operation receipt",
                    });
                }
            }
        }
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "operation receipt",
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCountersV1 {
    pub directory_entries_walked: u64,
    pub carrier_opens: u64,
    pub carrier_bytes_read: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub authority_identity_rows_scanned: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub strict_journal_inspections: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub fact_sqlite_rows_selected: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub change_semantic_constructions: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub change_projection_constructions: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub change_candidates: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub change_candidate_current_revisions: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub change_capability_carriers_opened: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub change_proposal_carriers_opened: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub change_proposal_carriers_validated: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub change_support_carriers_opened: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub change_matches: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub change_rows_emitted: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub change_seek_fact_rows_selected: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub timeline_sqlite_candidates: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub timeline_sqlite_window_rows: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub timeline_sqlite_facet_rows: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub timeline_selected_carriers: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub timeline_revision_candidate_carriers: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub timeline_removal_support_carriers: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub timeline_signature_support_carriers: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub timeline_correlation_support_carriers: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub timeline_trust_support_carriers: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub timeline_exhaustive_candidates: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub timeline_entries_emitted: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub authoritative_fallbacks: u64,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub full_history_fallbacks: u64,
    pub event_decodes: u64,
    pub event_validations: u64,
    pub event_folds: u64,
    pub chronological_sort_items: u64,
    pub body_artifact_reads: u64,
    pub body_bytes_read: u64,
    pub object_artifact_reads: u64,
    pub object_bytes_read: u64,
    pub projection_rebuilds: u64,
    pub state_rebuilds: u64,
    pub response_bytes: u64,
}

fn u64_is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCapacityOwnershipV1 {
    pub retained_decoded_events: u64,
    pub retained_hydrated_history_entries: u64,
    pub retained_hydrated_body_bytes: u64,
    pub retained_search_record_strings: u64,
    pub retained_search_record_field_bytes: u64,
    pub retained_serialized_response_cache_bytes: u64,
    pub retained_snapshot_highlight_entries: u64,
    pub retained_snapshot_highlight_bytes: u64,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InteractionExecutionIdentityV1 {
    pub source_commit: String,
    pub source_tree: String,
    pub cargo_lock_sha256: String,
    pub binary_path: String,
    pub binary_sha256: String,
    pub build_profile: String,
    pub rustc_version: String,
    pub features: Vec<String>,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
impl InteractionExecutionIdentityV1 {
    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        validate_hex(&self.source_commit, 40, "interaction source commit")?;
        validate_hex(&self.source_tree, 40, "interaction source tree")?;
        validate_hex(
            &self.cargo_lock_sha256,
            64,
            "interaction Cargo.lock SHA-256",
        )?;
        validate_hex(&self.binary_sha256, 64, "interaction binary SHA-256")?;
        let binary_path = std::path::Path::new(&self.binary_path);
        if !binary_path.is_absolute() || binary_path.file_name().is_none() {
            return Err(LongitudinalContractError::ContractDrift {
                field: "interaction binary path",
            });
        }
        require_nonempty(&self.build_profile, "interaction build profile")?;
        require_nonempty(&self.rustc_version, "interaction rustc version")?;
        if self.features.is_empty() {
            return Err(LongitudinalContractError::EmptyField {
                field: "interaction feature set",
            });
        }
        for feature in &self.features {
            require_nonempty(feature, "interaction feature")?;
        }
        require_unique(
            self.features.iter().map(String::as_str),
            "interaction feature",
        )?;
        if !self.features.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(LongitudinalContractError::ContractDrift {
                field: "interaction feature order",
            });
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionRouteV1 {
    AssessmentCurrentResult,
    AssessmentCurrentSummary,
    InputRequestOpenAllTracks,
    ObservationReviewerList,
    ValidationReviewerList,
    VersionJson,
    AttentionCurrentOrFallback,
    RevisionShowDetail,
    ChangeProfileRead,
    ChangeListRead,
    ChangeAttentionRead,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
impl InteractionRouteV1 {
    pub const FACT_READS: [Self; 5] = [
        Self::AssessmentCurrentResult,
        Self::AssessmentCurrentSummary,
        Self::InputRequestOpenAllTracks,
        Self::ObservationReviewerList,
        Self::ValidationReviewerList,
    ];

    pub fn is_fact_read(self) -> bool {
        Self::FACT_READS.contains(&self)
    }
}

/// The two owner-approved `revision show` performance cells: the derived
/// active-current target and the explicit-off compatibility characterization.
/// Active-unavailable revision show is characterization-only prose with no
/// catalog cell and no measurement.
#[cfg(any(test, feature = "longitudinal-counting"))]
pub const INTERACTION_REVISION_SHOW_CELLS_V1: [(
    &str,
    InteractionRouteV1,
    InteractionSetupExpectationV1,
); 2] = [
    (
        "revision_show_current_exact",
        InteractionRouteV1::RevisionShowDetail,
        InteractionSetupExpectationV1::FactActiveCurrent,
    ),
    (
        "revision_show_explicit_off",
        InteractionRouteV1::RevisionShowDetail,
        InteractionSetupExpectationV1::FactExplicitOff,
    ),
];

/// The four owner-approved change-read cells: three derived active-current
/// targets plus the change list explicit-off compatibility characterization.
/// No change read has an unavailable or terminal catalog cell.
#[cfg(any(test, feature = "longitudinal-counting"))]
pub const INTERACTION_CHANGE_READ_CELLS_V1: [(
    &str,
    InteractionRouteV1,
    InteractionSetupExpectationV1,
); 4] = [
    (
        "change_profile_current",
        InteractionRouteV1::ChangeProfileRead,
        InteractionSetupExpectationV1::FactActiveCurrent,
    ),
    (
        "change_list_current",
        InteractionRouteV1::ChangeListRead,
        InteractionSetupExpectationV1::FactActiveCurrent,
    ),
    (
        "change_attention_current",
        InteractionRouteV1::ChangeAttentionRead,
        InteractionSetupExpectationV1::FactActiveCurrent,
    ),
    (
        "change_list_explicit_off",
        InteractionRouteV1::ChangeListRead,
        InteractionSetupExpectationV1::FactExplicitOff,
    ),
];

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionSetupExpectationV1 {
    NotApplicable,
    AuthoritativeReplay,
    FactActiveCurrent,
    FactExplicitOff,
    FactActiveUnavailable,
    FactPostSelectionFailure,
    AttentionDerivedCurrent,
    AttentionColdInactive,
    AttentionActiveUnavailable,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionObservedRouteStateV1 {
    NotApplicable,
    AuthoritativeReplay,
    DerivedCurrent,
    UnlabeledFallbackToAuthoritative,
    LabeledFallbackToAuthoritative,
    DerivedSelectionFailedClosed,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractionPerformanceRoleV1 {
    ProvisionalTarget {
        sample_count: u8,
        strict_upper_bound_millis: u64,
    },
    CompatibilityCharacterization,
    TerminalDiagnostic,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractionFallbackPresentationV1 {
    None,
    Unlabeled,
    LabeledHintBeforeError,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InteractionRouteStateContractV1 {
    pub observed: InteractionObservedRouteStateV1,
    pub success: bool,
    pub performance_role: InteractionPerformanceRoleV1,
    pub historical_evidence_unchanged: bool,
    pub fallback_presentation: InteractionFallbackPresentationV1,
    pub strict_authoritative_snapshots: u8,
    pub background_maintenance_children: u8,
    pub authoritative_fallbacks: u8,
    pub full_history_fallbacks: u8,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
pub fn interaction_route_state_contract_v1(
    route: InteractionRouteV1,
    setup: InteractionSetupExpectationV1,
) -> Option<InteractionRouteStateContractV1> {
    use InteractionFallbackPresentationV1 as Fallback;
    use InteractionObservedRouteStateV1 as Observed;
    use InteractionPerformanceRoleV1 as Role;
    use InteractionSetupExpectationV1 as Setup;

    const TARGET: Role = Role::ProvisionalTarget {
        sample_count: 5,
        strict_upper_bound_millis: 2_000,
    };

    const BASE: InteractionRouteStateContractV1 = InteractionRouteStateContractV1 {
        observed: Observed::NotApplicable,
        success: true,
        performance_role: Role::CompatibilityCharacterization,
        historical_evidence_unchanged: true,
        fallback_presentation: Fallback::None,
        strict_authoritative_snapshots: 0,
        background_maintenance_children: 0,
        authoritative_fallbacks: 0,
        full_history_fallbacks: 0,
    };
    const FACT_CURRENT: InteractionRouteStateContractV1 = InteractionRouteStateContractV1 {
        observed: Observed::DerivedCurrent,
        performance_role: TARGET,
        ..BASE
    };
    const AUTHORITATIVE: InteractionRouteStateContractV1 = InteractionRouteStateContractV1 {
        observed: Observed::AuthoritativeReplay,
        strict_authoritative_snapshots: 1,
        ..BASE
    };
    const FACT_UNAVAILABLE: InteractionRouteStateContractV1 = InteractionRouteStateContractV1 {
        observed: Observed::UnlabeledFallbackToAuthoritative,
        fallback_presentation: Fallback::Unlabeled,
        strict_authoritative_snapshots: 1,
        background_maintenance_children: 1,
        authoritative_fallbacks: 1,
        full_history_fallbacks: 1,
        ..BASE
    };
    const FACT_TERMINAL: InteractionRouteStateContractV1 = InteractionRouteStateContractV1 {
        observed: Observed::DerivedSelectionFailedClosed,
        success: false,
        performance_role: Role::TerminalDiagnostic,
        background_maintenance_children: 1,
        ..BASE
    };
    const ATTENTION_CURRENT: InteractionRouteStateContractV1 = InteractionRouteStateContractV1 {
        observed: Observed::DerivedCurrent,
        performance_role: TARGET,
        ..BASE
    };
    const ATTENTION_UNAVAILABLE: InteractionRouteStateContractV1 =
        InteractionRouteStateContractV1 {
            observed: Observed::LabeledFallbackToAuthoritative,
            fallback_presentation: Fallback::LabeledHintBeforeError,
            ..FACT_UNAVAILABLE
        };

    if route.is_fact_read() {
        return match setup {
            Setup::FactActiveCurrent => Some(FACT_CURRENT),
            Setup::FactExplicitOff | Setup::AuthoritativeReplay => Some(AUTHORITATIVE),
            Setup::FactActiveUnavailable => Some(FACT_UNAVAILABLE),
            Setup::FactPostSelectionFailure => Some(FACT_TERMINAL),
            _ => None,
        };
    }

    if route == InteractionRouteV1::RevisionShowDetail {
        // The two owner-approved revision-show cells. Active-unavailable is
        // characterization-only prose: it carries no catalog cell and no
        // measurement, so every other setup stays out of the catalog. The
        // explicit-off arm keeps the lenient complete read — never the strict
        // change-reader snapshot — so it expects zero strict authoritative
        // snapshots, unlike the fact routes.
        return match setup {
            Setup::FactActiveCurrent => Some(FACT_CURRENT),
            Setup::FactExplicitOff => Some(InteractionRouteStateContractV1 {
                observed: Observed::AuthoritativeReplay,
                ..BASE
            }),
            _ => None,
        };
    }

    if matches!(
        route,
        InteractionRouteV1::ChangeProfileRead
            | InteractionRouteV1::ChangeListRead
            | InteractionRouteV1::ChangeAttentionRead
    ) {
        // The owner-approved change-read cells: three active-current targets
        // plus the change list explicit-off characterization control. The
        // change family records no strict authoritative snapshot on either
        // lane, and profile/attention have no explicit-off cell; every other
        // setup stays out of the catalog.
        return match setup {
            Setup::FactActiveCurrent => Some(FACT_CURRENT),
            Setup::FactExplicitOff if route == InteractionRouteV1::ChangeListRead => {
                Some(InteractionRouteStateContractV1 {
                    observed: Observed::AuthoritativeReplay,
                    ..BASE
                })
            }
            _ => None,
        };
    }

    match (route, setup) {
        (InteractionRouteV1::VersionJson, Setup::NotApplicable) => Some(BASE),
        (InteractionRouteV1::AttentionCurrentOrFallback, Setup::AttentionDerivedCurrent) => {
            Some(ATTENTION_CURRENT)
        }
        (InteractionRouteV1::AttentionCurrentOrFallback, Setup::AttentionColdInactive) => {
            Some(AUTHORITATIVE)
        }
        (InteractionRouteV1::AttentionCurrentOrFallback, Setup::AttentionActiveUnavailable) => {
            Some(ATTENTION_UNAVAILABLE)
        }
        _ => None,
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionActorV1 {
    RequestReader,
    ProductWriter,
    BackgroundMaintenance,
    BackgroundRebuild,
    ExplicitRecovery,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "status", deny_unknown_fields)]
pub enum InteractionScopeCoverageV1 {
    Complete,
    Incomplete { reason: String },
}

#[cfg(any(test, feature = "longitudinal-counting"))]
impl InteractionScopeCoverageV1 {
    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if let Self::Incomplete { reason } = self {
            require_nonempty(reason, "interaction incomplete reason")?;
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InteractionPerformanceExpectedContextV1 {
    pub execution: InteractionExecutionIdentityV1,
    pub route: InteractionRouteV1,
    pub arguments: Vec<String>,
    pub setup_expectation: InteractionSetupExpectationV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixture_identity_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_actor: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub expected_child_actors: BTreeMap<InteractionActorV1, u16>,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
impl InteractionPerformanceExpectedContextV1 {
    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        self.execution.validate()?;
        if self.arguments.is_empty() {
            return Err(LongitudinalContractError::EmptyField {
                field: "interaction route arguments",
            });
        }
        for argument in &self.arguments {
            require_nonempty(argument, "interaction route argument")?;
        }
        for count in self.expected_child_actors.values() {
            if *count == 0 {
                return Err(LongitudinalContractError::CountMismatch {
                    field: "interaction expected child actor count",
                    expected: 1,
                    actual: 0,
                });
            }
        }

        let requires_fixture = self.route != InteractionRouteV1::VersionJson;
        match (&self.fixture_identity_sha256, requires_fixture) {
            (Some(identity), true) => validate_hex(identity, 64, "interaction fixture identity")?,
            (None, false) => {}
            _ => {
                return Err(LongitudinalContractError::ContractDrift {
                    field: "interaction route fixture applicability",
                });
            }
        }

        // The change-read routes take no Revision selector (their argv is
        // repo-plus-format only), so their expected context carries none.
        let requires_revision = !matches!(
            self.route,
            InteractionRouteV1::VersionJson
                | InteractionRouteV1::ChangeProfileRead
                | InteractionRouteV1::ChangeListRead
                | InteractionRouteV1::ChangeAttentionRead
        );
        match (&self.revision, requires_revision) {
            (Some(revision), true) => {
                validate_prefixed_hash(revision, "rev:sha256:", "interaction Revision")?
            }
            (None, false) => {}
            _ => {
                return Err(LongitudinalContractError::ContractDrift {
                    field: "interaction route Revision applicability",
                });
            }
        }

        let requires_track = matches!(
            self.route,
            InteractionRouteV1::AssessmentCurrentResult
                | InteractionRouteV1::AssessmentCurrentSummary
                | InteractionRouteV1::ObservationReviewerList
                | InteractionRouteV1::ValidationReviewerList
        );
        match (&self.track, requires_track) {
            (Some(track), true) => require_nonempty(track, "interaction route track")?,
            (None, false) => {}
            _ => {
                return Err(LongitudinalContractError::ContractDrift {
                    field: "interaction route track applicability",
                });
            }
        }

        let requires_domain_actor = self.route != InteractionRouteV1::VersionJson;
        match (&self.domain_actor, requires_domain_actor) {
            (Some(actor), true) => require_nonempty(actor, "interaction domain actor")?,
            (None, false) => {}
            _ => {
                return Err(LongitudinalContractError::ContractDrift {
                    field: "interaction domain actor applicability",
                });
            }
        }

        if interaction_route_state_contract_v1(self.route, self.setup_expectation).is_none() {
            return Err(LongitudinalContractError::ContractDrift {
                field: "interaction expected setup",
            });
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InteractionObservedFactsV1 {
    pub route: InteractionRouteV1,
    pub route_state: InteractionObservedRouteStateV1,
    pub execution_actor: InteractionActorV1,
    pub success: bool,
    pub exit_code: i32,
    pub semantic_result_sha256: String,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
impl InteractionObservedFactsV1 {
    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        validate_hex(
            &self.semantic_result_sha256,
            64,
            "interaction semantic result SHA-256",
        )?;
        if self.success != (self.exit_code == 0) {
            return Err(LongitudinalContractError::ContractDrift {
                field: "interaction success and exit code",
            });
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InteractionChildScopeFactV1 {
    pub ordinal: u16,
    pub actor: InteractionActorV1,
    pub coverage: InteractionScopeCoverageV1,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
impl InteractionChildScopeFactV1 {
    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        self.coverage.validate()
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionLockKindV1 {
    Authority,
    Derived,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionLockModeV1 {
    Blocking,
    Try,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionLockOutcomeV1 {
    Acquired,
    Busy,
    Deferred,
    Failed,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionLockAcquisitionV1 {
    Physical,
    Reentrant,
    NotAcquired,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InteractionLockFactV1 {
    pub ordinal: u16,
    pub actor: InteractionActorV1,
    pub kind: InteractionLockKindV1,
    pub mode: InteractionLockModeV1,
    pub outcome: InteractionLockOutcomeV1,
    pub acquisition: InteractionLockAcquisitionV1,
    pub wait_nanos: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_nanos: Option<u64>,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
impl InteractionLockFactV1 {
    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        let valid = match (self.outcome, self.acquisition) {
            (InteractionLockOutcomeV1::Acquired, InteractionLockAcquisitionV1::Physical) => {
                self.hold_nanos.is_some()
            }
            (InteractionLockOutcomeV1::Acquired, InteractionLockAcquisitionV1::Reentrant) => {
                self.kind == InteractionLockKindV1::Authority
                    && self.wait_nanos == 0
                    && self.hold_nanos.is_none()
            }
            (
                InteractionLockOutcomeV1::Busy
                | InteractionLockOutcomeV1::Deferred
                | InteractionLockOutcomeV1::Failed,
                InteractionLockAcquisitionV1::NotAcquired,
            ) => self.hold_nanos.is_none(),
            _ => false,
        };
        if !valid {
            return Err(LongitudinalContractError::ContractDrift {
                field: "interaction lock acquisition",
            });
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InteractionPerformanceReceiptV1 {
    pub schema: String,
    pub expected: InteractionPerformanceExpectedContextV1,
    pub observed: InteractionObservedFactsV1,
    pub scope_coverage: InteractionScopeCoverageV1,
    pub counters: LongitudinalCountersV1,
    pub capacity_ownership: LongitudinalCapacityOwnershipV1,
    pub phases: Vec<super::counters::LongitudinalDerivedAccessPhaseSampleV1>,
    pub children: Vec<InteractionChildScopeFactV1>,
    pub locks: Vec<InteractionLockFactV1>,
    pub receipt_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCounterReceiptV1 {
    pub schema: String,
    pub run_identity: String,
    pub root_identity: String,
    pub operation: String,
    pub phase: String,
    pub base_execution_identity_sha256: String,
    pub derivative_execution_identity_sha256: String,
    pub manifest_sha256: String,
    pub schedule_sha256: String,
    pub success: bool,
    pub semantic_result_sha256: String,
    pub counters: LongitudinalCountersV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_ownership: Option<LongitudinalCapacityOwnershipV1>,
    pub receipt_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalTimelinePostPinBoundaryV1 {
    CarrierLocatorsSelected,
}

impl LongitudinalTimelinePostPinBoundaryV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CarrierLocatorsSelected => "carrier_locators_selected",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalTimelineCarrierMismatchKindV1 {
    ValidationWitness,
}

impl LongitudinalTimelineCarrierMismatchKindV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ValidationWitness => "validation_witness",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalTimelinePostPinBarrierReceiptV1 {
    pub schema: String,
    pub run_identity: String,
    pub barrier_identity_sha256: String,
    pub boundary: LongitudinalTimelinePostPinBoundaryV1,
    pub carrier_opens_before: u64,
    pub selected_carriers_before: u64,
    pub expected_carrier_key_digest: String,
    pub observed_mismatch_key_digest: String,
    pub mismatch_kind: LongitudinalTimelineCarrierMismatchKindV1,
    pub clean_carrier_sha256: String,
    pub mutated_carrier_sha256: String,
    pub mutation_recipe_sha256: String,
    pub derivative_inventory_sha256: String,
    pub ready_receipt_sha256: String,
    pub release_receipt_sha256: String,
    pub receipt_sha256: String,
}

impl LongitudinalTimelinePostPinBarrierReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_RECEIPT_SCHEMA_V1 {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        for (hash, field) in [
            (&self.run_identity, "run identity"),
            (&self.barrier_identity_sha256, "barrier identity"),
            (
                &self.expected_carrier_key_digest,
                "expected carrier key digest",
            ),
            (
                &self.observed_mismatch_key_digest,
                "observed mismatch key digest",
            ),
            (&self.clean_carrier_sha256, "clean carrier SHA-256"),
            (&self.mutated_carrier_sha256, "mutated carrier SHA-256"),
            (&self.mutation_recipe_sha256, "mutation recipe SHA-256"),
            (
                &self.derivative_inventory_sha256,
                "derivative inventory SHA-256",
            ),
            (&self.ready_receipt_sha256, "ready receipt SHA-256"),
            (&self.release_receipt_sha256, "release receipt SHA-256"),
            (&self.receipt_sha256, "barrier receipt SHA-256"),
        ] {
            validate_hex(hash, 64, field)?;
        }
        if self.carrier_opens_before != 0 {
            return Err(LongitudinalContractError::CountMismatch {
                field: "Timeline post-pin carrier opens before",
                expected: 0,
                actual: self.carrier_opens_before,
            });
        }
        if self.selected_carriers_before == 0
            || self.expected_carrier_key_digest != self.observed_mismatch_key_digest
        {
            return Err(LongitudinalContractError::PairMismatch);
        }
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "Timeline post-pin barrier receipt",
        )
    }
}

impl LongitudinalCounterReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_COUNTER_RECEIPT_SCHEMA_V1 {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        for (hash, name) in [
            (&self.run_identity, "run identity"),
            (&self.root_identity, "root identity"),
            (
                &self.base_execution_identity_sha256,
                "base execution identity",
            ),
            (
                &self.derivative_execution_identity_sha256,
                "derivative execution identity",
            ),
            (&self.manifest_sha256, "manifest SHA-256"),
            (&self.schedule_sha256, "schedule SHA-256"),
            (&self.semantic_result_sha256, "semantic result SHA-256"),
        ] {
            validate_hex(hash, 64, name)?;
        }
        require_nonempty(&self.operation, "counter operation")?;
        require_nonempty(&self.phase, "counter phase")?;
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "counter receipt",
        )
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
impl InteractionPerformanceReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != INTERACTION_PERFORMANCE_RECEIPT_SCHEMA_V1 {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        self.expected.validate()?;
        self.observed.validate()?;
        self.scope_coverage.validate()?;
        if self.expected.route != self.observed.route {
            return Err(LongitudinalContractError::PairMismatch);
        }
        if self.observed.execution_actor != InteractionActorV1::RequestReader {
            return Err(LongitudinalContractError::ContractDrift {
                field: "interaction root execution actor",
            });
        }
        validate_interaction_route_state_v1(
            self.expected.route,
            self.expected.setup_expectation,
            self.observed.route_state,
            self.observed.success,
        )?;
        let route_state_contract = interaction_route_state_contract_v1(
            self.expected.route,
            self.expected.setup_expectation,
        )
        .ok_or(LongitudinalContractError::ContractDrift {
            field: "interaction expected setup",
        })?;
        let expected_strict_snapshots =
            u64::from(route_state_contract.strict_authoritative_snapshots);
        if self.counters.strict_journal_inspections != expected_strict_snapshots {
            return Err(LongitudinalContractError::CountMismatch {
                field: "interaction strict authoritative snapshots",
                expected: expected_strict_snapshots,
                actual: self.counters.strict_journal_inspections,
            });
        }

        for (index, phase) in self.phases.iter().enumerate() {
            let expected_ordinal =
                u16::try_from(index).map_err(|_| LongitudinalContractError::ContractDrift {
                    field: "interaction phase ordinal",
                })?;
            if phase.ordinal != expected_ordinal
                || phase.ownership != phase.phase.ownership()
                || phase.actor.is_none()
                || phase
                    .parent_ordinal
                    .is_some_and(|parent| parent >= phase.ordinal)
            {
                return Err(LongitudinalContractError::ContractDrift {
                    field: "interaction phase facts",
                });
            }
        }

        let mut actual_child_actors = BTreeMap::new();
        for (index, child) in self.children.iter().enumerate() {
            child.validate()?;
            let expected_ordinal =
                u16::try_from(index).map_err(|_| LongitudinalContractError::ContractDrift {
                    field: "interaction child ordinal",
                })?;
            if child.ordinal != expected_ordinal {
                return Err(LongitudinalContractError::ContractDrift {
                    field: "interaction child ordinal",
                });
            }
            let count = actual_child_actors.entry(child.actor).or_insert(0_u16);
            *count = count
                .checked_add(1)
                .ok_or(LongitudinalContractError::ContractDrift {
                    field: "interaction child actor count",
                })?;
        }
        if actual_child_actors != self.expected.expected_child_actors {
            return Err(LongitudinalContractError::ContractDrift {
                field: "interaction expected child actors",
            });
        }
        if self.scope_coverage != interaction_scope_coverage_v1(&self.children)? {
            return Err(LongitudinalContractError::ContractDrift {
                field: "interaction scope coverage",
            });
        }

        for (index, lock) in self.locks.iter().enumerate() {
            lock.validate()?;
            let expected_ordinal =
                u16::try_from(index).map_err(|_| LongitudinalContractError::ContractDrift {
                    field: "interaction lock ordinal",
                })?;
            if lock.ordinal != expected_ordinal {
                return Err(LongitudinalContractError::ContractDrift {
                    field: "interaction lock ordinal",
                });
            }
        }

        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "interaction performance receipt",
        )
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
pub(crate) fn interaction_scope_coverage_v1(
    children: &[InteractionChildScopeFactV1],
) -> Result<InteractionScopeCoverageV1, LongitudinalContractError> {
    let mut incomplete_reasons = Vec::new();
    for child in children {
        child.validate()?;
        if let InteractionScopeCoverageV1::Incomplete { reason } = &child.coverage {
            incomplete_reasons.push(reason.as_str());
        }
    }
    if incomplete_reasons.is_empty() {
        Ok(InteractionScopeCoverageV1::Complete)
    } else {
        Ok(InteractionScopeCoverageV1::Incomplete {
            reason: incomplete_reasons.join("; "),
        })
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
fn validate_interaction_route_state_v1(
    route: InteractionRouteV1,
    setup: InteractionSetupExpectationV1,
    observed: InteractionObservedRouteStateV1,
    success: bool,
) -> Result<(), LongitudinalContractError> {
    let valid = interaction_route_state_contract_v1(route, setup).is_some_and(|contract| {
        contract.observed == observed
                // A normal setup's expected success is the gate outcome, not
                // permission to suppress a truthful product-failure receipt.
                // A terminal setup must still refuse a claimed success.
                && (contract.success || !success)
    });
    if !valid {
        return Err(LongitudinalContractError::PairMismatch);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCapacityManifestV1 {
    pub schema: String,
    pub contract_sha256: String,
    pub execution: LongitudinalExecutionIdentityV1,
    pub public_seed_hex: String,
    pub subject: LongitudinalCapacitySubjectV1,
    pub event_count: u64,
    pub revision_count: u64,
    pub object_artifact_count: u64,
    pub task_attempt_count: u64,
    pub body_fact_count: u64,
    pub external_body_count: u64,
    pub decoded_body_bytes: u64,
    pub decoded_object_target_bytes: u64,
    pub ordered_events: Vec<LongitudinalEventIdentityV1>,
    pub event_carriers: Vec<LongitudinalEventCarrierV1>,
    pub content_inventory: Vec<LongitudinalInventoryEntryV1>,
    pub removed_content_sha256: Vec<String>,
    pub selectors: LongitudinalCapacitySelectorsV1,
    pub probe_schedule: Vec<LongitudinalCapacityProbeV1>,
    pub schedule_sha256: String,
    pub manifest_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCapacitySelectorsV1 {
    pub carrier_hit_key: String,
    pub carrier_hit_event_id: String,
    pub carrier_miss_key: String,
    pub semantic_revision_id: String,
    pub semantic_object_id: String,
    pub semantic_missing_revision_id: String,
    pub semantic_missing_object_id: String,
    pub object_detail_object_id: String,
    pub object_detail_content_hash: String,
    pub chronological_window_size: u16,
    pub chronological_head_start: u64,
    pub chronological_middle_start: u64,
    pub chronological_tail_start: u64,
    pub selectors_sha256: String,
}

impl LongitudinalCapacitySelectorsV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.selectors_sha256, |selectors_sha256| {
            let mut preimage = self.clone();
            preimage.selectors_sha256 = selectors_sha256;
            preimage
        })
    }

    pub fn validate(
        &self,
        event_count: u64,
        event_carriers: &[LongitudinalEventCarrierV1],
        content_inventory: &[LongitudinalInventoryEntryV1],
    ) -> Result<(), LongitudinalContractError> {
        for (value, field) in [
            (&self.carrier_hit_key, "carrier hit key"),
            (&self.carrier_hit_event_id, "carrier hit event id"),
            (&self.carrier_miss_key, "carrier miss key"),
            (&self.semantic_revision_id, "semantic revision id"),
            (&self.semantic_object_id, "semantic object id"),
            (
                &self.semantic_missing_revision_id,
                "semantic missing revision id",
            ),
            (
                &self.semantic_missing_object_id,
                "semantic missing object id",
            ),
            (&self.object_detail_object_id, "object detail object id"),
        ] {
            require_nonempty(value, field)?;
        }
        let carrier = event_carriers
            .iter()
            .find(|carrier| carrier.event_id == self.carrier_hit_event_id)
            .ok_or(LongitudinalContractError::ContractDrift {
                field: "carrier hit selector",
            })?;
        validate_bound_hash(
            &carrier.logical_key_sha256,
            &sha256_bytes_hex(self.carrier_hit_key.as_bytes()),
            "carrier hit selector",
        )?;
        if event_carriers.iter().any(|candidate| {
            candidate.logical_key_sha256 == sha256_bytes_hex(self.carrier_miss_key.as_bytes())
        }) {
            return Err(LongitudinalContractError::ContractDrift {
                field: "carrier miss selector",
            });
        }
        let content_hash = self
            .object_detail_content_hash
            .strip_prefix("sha256:")
            .unwrap_or(&self.object_detail_content_hash);
        validate_hex(content_hash, 64, "object detail content hash")?;
        if !content_inventory
            .iter()
            .any(|content| content.logical_key.contains(content_hash))
        {
            return Err(LongitudinalContractError::ContractDrift {
                field: "object detail selector",
            });
        }
        if self.chronological_window_size != 100
            || self.chronological_head_start != 0
            || self.chronological_middle_start
                != event_count.saturating_sub(u64::from(self.chronological_window_size)) / 2
            || self.chronological_tail_start
                != event_count.saturating_sub(u64::from(self.chronological_window_size))
        {
            return Err(LongitudinalContractError::ContractDrift {
                field: "chronological selectors",
            });
        }
        validate_bound_hash(
            &self.selectors_sha256,
            &self.canonical_sha256()?,
            "capacity selectors",
        )
    }
}

impl LongitudinalCapacityManifestV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.manifest_sha256, |manifest_sha256| {
            let mut preimage = self.clone();
            preimage.manifest_sha256 = manifest_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        let contract = longitudinal_capacity_contract_v1();
        if self.schema != contract.schema
            || self.contract_sha256 != contract.contract_sha256
            || self.public_seed_hex != contract.public_seed_hex
        {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        self.execution.validate()?;
        let requirement = match self.subject {
            LongitudinalCapacitySubjectV1::V1L100 => {
                let tier = longitudinal_runner_contract_v1()
                    .tiers
                    .into_iter()
                    .find(|candidate| candidate.tier == LongitudinalTierV1::L100)
                    .ok_or(LongitudinalContractError::UnsupportedTier)?;
                LongitudinalCapacityManifestRequirementV1 {
                    event_count: tier.event_count,
                    revision_count: tier.revision_count,
                    object_artifact_count: tier.object_artifact_count,
                    task_attempt_count: tier.task_attempt_count,
                    body_fact_count: tier.body_fact_count,
                    external_body_count: tier.external_body_count,
                    present_content_count: tier.present_content_count,
                    removed_content_count: tier.removed_content_count,
                    decoded_body_bytes: tier.decoded_body_bytes,
                    decoded_object_target_bytes: tier.decoded_object_target_bytes,
                }
            }
            LongitudinalCapacitySubjectV1::Companion(profile) => {
                let profile = contract
                    .profiles
                    .iter()
                    .find(|candidate| candidate.profile == profile)
                    .ok_or(LongitudinalContractError::UnsupportedProfile)?;
                LongitudinalCapacityManifestRequirementV1 {
                    event_count: profile.event_count,
                    revision_count: profile.revision_count,
                    object_artifact_count: profile.object_artifact_count,
                    task_attempt_count: profile.task_attempt_count,
                    body_fact_count: profile.body_fact_count,
                    external_body_count: profile.external_body_count,
                    present_content_count: profile.present_content_count,
                    removed_content_count: profile.removed_content_count,
                    decoded_body_bytes: profile.decoded_body_bytes,
                    decoded_object_target_bytes: profile.decoded_object_target_bytes,
                }
            }
        };
        require_count(
            self.event_count,
            requirement.event_count,
            "capacity event count",
        )?;
        require_count(
            self.revision_count,
            requirement.revision_count,
            "capacity revision count",
        )?;
        require_count(
            self.object_artifact_count,
            requirement.object_artifact_count,
            "capacity object count",
        )?;
        require_count(
            self.task_attempt_count,
            requirement.task_attempt_count,
            "capacity task-attempt count",
        )?;
        require_count(
            self.body_fact_count,
            requirement.body_fact_count,
            "capacity body-fact count",
        )?;
        require_count(
            self.external_body_count,
            requirement.external_body_count,
            "capacity external-body count",
        )?;
        require_count(
            self.decoded_body_bytes,
            requirement.decoded_body_bytes,
            "capacity decoded body bytes",
        )?;
        require_count(
            self.decoded_object_target_bytes,
            requirement.decoded_object_target_bytes,
            "capacity decoded object bytes",
        )?;
        require_len(
            self.content_inventory.len(),
            requirement.present_content_count,
            "capacity content inventory",
        )?;
        require_len(
            self.removed_content_sha256.len(),
            requirement.removed_content_count,
            "capacity removed-content set",
        )?;
        require_len(
            self.ordered_events.len(),
            requirement.event_count,
            "capacity ordered events",
        )?;
        require_len(
            self.event_carriers.len(),
            requirement.event_count,
            "capacity event carriers",
        )?;
        require_exact_sequence(
            self.probe_schedule.iter().copied(),
            LongitudinalCapacityProbeV1::ALL,
            "capacity probe schedule",
        )?;
        require_unique(
            self.ordered_events
                .iter()
                .map(|event| event.event_id.as_str()),
            "capacity ordered event ids",
        )?;
        require_unique(
            self.event_carriers
                .iter()
                .map(|event| event.event_id.as_str()),
            "capacity event carrier ids",
        )?;
        require_unique(
            self.content_inventory
                .iter()
                .map(|content| content.logical_key.as_str()),
            "capacity content logical keys",
        )?;
        for event in &self.ordered_events {
            validate_hex(
                &event.canonical_decoded_sha256,
                64,
                "capacity event SHA-256",
            )?;
        }
        for carrier in &self.event_carriers {
            validate_hex(&carrier.logical_key_sha256, 64, "capacity key SHA-256")?;
            validate_hex(&carrier.raw_sha256, 64, "capacity carrier SHA-256")?;
        }
        for content in &self.content_inventory {
            validate_hex(&content.raw_sha256, 64, "capacity content raw SHA-256")?;
            validate_hex(
                &content.decoded_sha256,
                64,
                "capacity content decoded SHA-256",
            )?;
        }
        for hash in &self.removed_content_sha256 {
            validate_hex(hash, 64, "capacity removed-content SHA-256")?;
        }
        self.selectors.validate(
            self.event_count,
            &self.event_carriers,
            &self.content_inventory,
        )?;
        validate_bound_hash(
            &self.schedule_sha256,
            &canonical_sha256(&self.probe_schedule)?,
            "capacity schedule",
        )?;
        validate_bound_hash(
            &self.manifest_sha256,
            &self.canonical_sha256()?,
            "capacity manifest",
        )
    }
}

struct LongitudinalCapacityManifestRequirementV1 {
    event_count: u64,
    revision_count: u64,
    object_artifact_count: u64,
    task_attempt_count: u64,
    body_fact_count: u64,
    external_body_count: u64,
    present_content_count: u64,
    removed_content_count: u64,
    decoded_body_bytes: u64,
    decoded_object_target_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCapacityMaterializationReceiptV1 {
    pub schema: String,
    pub root_identity: String,
    pub manifest: LongitudinalCapacityManifestV1,
    pub strict: LongitudinalStrictSemanticReceiptV1,
    pub materialization_sha256: String,
}

impl LongitudinalCapacityMaterializationReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.materialization_sha256, |materialization_sha256| {
            let mut preimage = self.clone();
            preimage.materialization_sha256 = materialization_sha256;
            preimage
        })
    }

    fn pair_identity_sha256(&self) -> Result<String, LongitudinalContractError> {
        let mut preimage = self.clone();
        preimage.root_identity.clear();
        preimage.materialization_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_CAPACITY_MATERIALIZATION_RECEIPT_SCHEMA_V1 {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        validate_hex(&self.root_identity, 64, "capacity root identity")?;
        self.manifest.validate()?;
        validate_strict_receipt(&self.strict)?;
        validate_bound_hash(
            &self.materialization_sha256,
            &self.canonical_sha256()?,
            "capacity materialization receipt",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalGenerationResourceReceiptV1 {
    pub subject: LongitudinalCapacitySubjectV1,
    pub wall_nanos: u64,
    pub process_cpu_nanos: u64,
    pub encoded_bytes: u64,
    pub allocated_bytes: u64,
    pub high_water_bytes: u64,
    pub receipt_sha256: String,
}

impl LongitudinalGenerationResourceReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCapacityReceiptV1 {
    pub schema: String,
    pub subject: LongitudinalCapacitySubjectV1,
    pub probe: LongitudinalCapacityProbeV1,
    pub execution_identity_sha256: String,
    pub manifest_sha256: String,
    pub sample_ordinal: u16,
    pub cold: bool,
    pub output_count: u64,
    pub selected_bytes: u64,
    pub semantic_receipt_sha256: String,
    pub classification: LongitudinalCapacityClassificationV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<LongitudinalOperationMetricsV1>,
    pub receipt_sha256: String,
}

impl LongitudinalCapacityReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_CAPACITY_RECEIPT_SCHEMA_V1 {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        validate_hex(
            &self.execution_identity_sha256,
            64,
            "capacity execution identity",
        )?;
        validate_hex(&self.manifest_sha256, 64, "capacity manifest SHA-256")?;
        validate_hex(
            &self.semantic_receipt_sha256,
            64,
            "capacity semantic receipt",
        )?;
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "capacity receipt",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCapacityMemoryReceiptV1 {
    pub schema: String,
    pub subject: LongitudinalCapacitySubjectV1,
    pub checkpoint: LongitudinalCapacityMemoryCheckpointV1,
    pub execution_identity_sha256: String,
    pub absolute_rss_bytes: u64,
    pub adjusted_rss_bytes: u64,
    pub vmmap_sha256: String,
    pub ownership: LongitudinalCapacityOwnershipV1,
    pub unknown_ownership: Vec<String>,
    pub receipt_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalMemoryReceiptV1 {
    pub schema: String,
    pub tier: LongitudinalTierV1,
    pub checkpoint: LongitudinalMemoryCheckpointV1,
    pub execution_identity_sha256: String,
    pub absolute_rss_bytes: u64,
    pub vmmap_sha256: String,
    pub unknown_ownership: Vec<String>,
    pub receipt_sha256: String,
}

impl LongitudinalMemoryReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != "pointbreak.longitudinal-memory-receipt.v1" {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        validate_hex(
            &self.execution_identity_sha256,
            64,
            "memory execution identity",
        )?;
        validate_hex(&self.vmmap_sha256, 64, "vmmap SHA-256")?;
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "memory receipt",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalInterruptionReceiptV1 {
    pub operation: LongitudinalInterruptionOperationV1,
    pub point: LongitudinalInterruptionPointV1,
    pub available: bool,
    pub source_unchanged: bool,
    pub destination_disposition: String,
    pub diagnostic: String,
    pub receipt_sha256: String,
}

impl LongitudinalInterruptionReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        require_nonempty(
            &self.destination_disposition,
            "interruption destination disposition",
        )?;
        require_nonempty(&self.diagnostic, "interruption diagnostic")?;
        if !self.available
            && (self.diagnostic != "unsupported_no_current_surface"
                || self.destination_disposition != "not_created")
        {
            return Err(LongitudinalContractError::ContractDrift {
                field: "unavailable interruption receipt",
            });
        }
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "interruption receipt",
        )
    }
}

impl LongitudinalCapacityMemoryReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_CAPACITY_MEMORY_SCHEMA_V1 {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        validate_hex(
            &self.execution_identity_sha256,
            64,
            "capacity memory execution identity",
        )?;
        validate_hex(&self.vmmap_sha256, 64, "vmmap SHA-256")?;
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "capacity memory receipt",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalContentionReceiptV1 {
    pub schema: String,
    pub manifest_sha256: String,
    pub writer_attempts: u8,
    pub created: u8,
    pub existing: u8,
    pub conflicts: u8,
    pub reader_cycles: u8,
    pub final_event_count: u64,
    pub strict_replay_sha256: String,
    pub timed_out: bool,
    pub semantic_failure: bool,
    pub receipt_sha256: String,
}

impl LongitudinalContentionReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != "pointbreak.longitudinal-contention-receipt.v1" {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        let contract = longitudinal_capacity_contract_v1().contention;
        if self.writer_attempts
            != contract.writer_processes
                * (contract.unique_attempts_per_writer + contract.shared_attempts_per_writer)
            || self.created != contract.expected_created
            || self.existing != contract.expected_existing
            || self.conflicts != 0
            || self.reader_cycles != contract.reader_cycles
            || self.timed_out
            || self.semantic_failure
        {
            return Err(LongitudinalContractError::ContractDrift {
                field: "contention schedule",
            });
        }
        validate_hex(&self.manifest_sha256, 64, "contention manifest SHA-256")?;
        validate_hex(&self.strict_replay_sha256, 64, "contention replay SHA-256")?;
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "contention receipt",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalC524GateInputsV1 {
    pub v1_l100_generation: LongitudinalGenerationResourceReceiptV1,
    pub c262_generation: LongitudinalGenerationResourceReceiptV1,
    pub c262_identity_complete: bool,
    pub c262_base_complete: bool,
    pub c262_counting_complete: bool,
    pub c262_memory_complete: bool,
    pub c262_package_complete: bool,
    pub host_pressure_clear: bool,
    pub free_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalC524GateReceiptV1 {
    pub schema: String,
    pub inputs_sha256: String,
    pub admitted: bool,
    pub reasons: Vec<String>,
    pub receipt_sha256: String,
}

impl LongitudinalC524GateReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.receipt_sha256, |receipt_sha256| {
            let mut preimage = self.clone();
            preimage.receipt_sha256 = receipt_sha256;
            preimage
        })
    }

    pub fn validate_against(
        &self,
        inputs: &LongitudinalC524GateInputsV1,
    ) -> Result<(), LongitudinalContractError> {
        if self.schema != "pointbreak.longitudinal-c524-gate-receipt.v1" {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        let expected_inputs = canonical_sha256(inputs)?;
        validate_bound_hash(&self.inputs_sha256, &expected_inputs, "C524 gate inputs")?;
        let expected = evaluate_c524_gate(inputs)?;
        if self.admitted != expected.0 || self.reasons != expected.1 {
            return Err(LongitudinalContractError::ContractDrift {
                field: "C524 gate decision",
            });
        }
        validate_bound_hash(
            &self.receipt_sha256,
            &self.canonical_sha256()?,
            "C524 gate receipt",
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalComplexityCountsV1 {
    pub carrier_opens: u64,
    pub event_decodes: u64,
    pub event_validations: u64,
    pub event_folds: u64,
    pub chronological_sort_items: u64,
    pub rebuilds: u64,
    pub retained_records: u64,
    pub retained_bytes: u64,
}

pub fn classify_longitudinal_capacity_work_v1(
    baseline: &LongitudinalComplexityCountsV1,
    candidate: &LongitudinalComplexityCountsV1,
    output_grew: bool,
    direct_whole_history_work_or_retention: bool,
    surface_supported: bool,
) -> LongitudinalCapacityClassificationV1 {
    if !surface_supported {
        return LongitudinalCapacityClassificationV1::UnsupportedNoCurrentSurface;
    }
    if direct_whole_history_work_or_retention {
        return LongitudinalCapacityClassificationV1::HistoryOrCardinalityProportional;
    }
    if !output_grew
        && baseline
            .as_array()
            .into_iter()
            .zip(candidate.as_array())
            .any(|(baseline, candidate)| exceeds_ratio(baseline, candidate, 12_500))
    {
        return LongitudinalCapacityClassificationV1::HistoryOrCardinalityProportional;
    }
    LongitudinalCapacityClassificationV1::Bounded
}

impl LongitudinalComplexityCountsV1 {
    fn as_array(&self) -> [u64; 8] {
        [
            self.carrier_opens,
            self.event_decodes,
            self.event_validations,
            self.event_folds,
            self.chronological_sort_items,
            self.rebuilds,
            self.retained_records,
            self.retained_bytes,
        ]
    }
}

pub(super) fn evaluate_c524_gate(
    inputs: &LongitudinalC524GateInputsV1,
) -> Result<(bool, Vec<String>), LongitudinalContractError> {
    let gate = longitudinal_capacity_contract_v1().c524_gate;
    validate_generation_resource(&inputs.v1_l100_generation)?;
    validate_generation_resource(&inputs.c262_generation)?;
    if inputs.v1_l100_generation.subject != LongitudinalCapacitySubjectV1::V1L100
        || inputs.c262_generation.subject
            != LongitudinalCapacitySubjectV1::Companion(LongitudinalCapacityProfileV1::C262)
    {
        return Err(LongitudinalContractError::ContractDrift {
            field: "C524 generation subjects",
        });
    }
    let mut reasons = Vec::new();
    for (complete, reason) in [
        (inputs.c262_identity_complete, "c262_identity_incomplete"),
        (inputs.c262_base_complete, "c262_base_incomplete"),
        (inputs.c262_counting_complete, "c262_counting_incomplete"),
        (inputs.c262_memory_complete, "c262_memory_incomplete"),
        (inputs.c262_package_complete, "c262_package_incomplete"),
        (inputs.host_pressure_clear, "host_pressure_not_clear"),
    ] {
        if !complete {
            reasons.push(reason.to_owned());
        }
    }
    for (baseline, candidate, reason) in [
        (
            inputs.v1_l100_generation.wall_nanos,
            inputs.c262_generation.wall_nanos,
            "generation_wall_ratio_exceeded",
        ),
        (
            inputs.v1_l100_generation.process_cpu_nanos,
            inputs.c262_generation.process_cpu_nanos,
            "generation_cpu_ratio_exceeded",
        ),
        (
            inputs.v1_l100_generation.encoded_bytes,
            inputs.c262_generation.encoded_bytes,
            "encoded_byte_ratio_exceeded",
        ),
        (
            inputs.v1_l100_generation.allocated_bytes,
            inputs.c262_generation.allocated_bytes,
            "allocated_byte_ratio_exceeded",
        ),
        (
            inputs.v1_l100_generation.high_water_bytes,
            inputs.c262_generation.high_water_bytes,
            "high_water_ratio_exceeded",
        ),
    ] {
        if exceeds_ratio(
            baseline,
            candidate,
            gate.maximum_pre_admission_ratio_basis_points,
        ) {
            reasons.push(reason.to_owned());
        }
    }
    let scaled_high_water = u128::from(inputs.c262_generation.high_water_bytes)
        .checked_mul(u128::from(gate.free_space_high_water_basis_points))
        .ok_or(LongitudinalContractError::ContractDrift {
            field: "C524 free-space arithmetic",
        })?
        .div_ceil(10_000);
    let required_free = scaled_high_water
        .checked_add(u128::from(gate.free_space_fixed_bytes))
        .ok_or(LongitudinalContractError::ContractDrift {
            field: "C524 free-space arithmetic",
        })?;
    if u128::from(inputs.free_bytes) < required_free {
        reasons.push("insufficient_free_space".to_owned());
    }
    Ok((reasons.is_empty(), reasons))
}

fn validate_generation_resource(
    receipt: &LongitudinalGenerationResourceReceiptV1,
) -> Result<(), LongitudinalContractError> {
    validate_bound_hash(
        &receipt.receipt_sha256,
        &receipt.canonical_sha256()?,
        "generation resource receipt",
    )
}

fn exceeds_ratio(baseline: u64, candidate: u64, maximum_basis_points: u32) -> bool {
    if baseline == 0 {
        return candidate > 0;
    }
    u128::from(candidate) * 10_000 > u128::from(baseline) * u128::from(maximum_basis_points)
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalRawFileV1 {
    pub relative_path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalFailureReceiptV1 {
    pub reason_code: String,
    pub detail_sha256: String,
    pub immutable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalEvidencePackageV1 {
    pub schema: String,
    pub purpose: LongitudinalPackagePurposeV1,
    pub contract_sha256: String,
    pub base_execution: LongitudinalExecutionIdentityV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derivative_execution: Option<LongitudinalExecutionIdentityV1>,
    pub materializations: Vec<LongitudinalMaterializationReceiptV1>,
    pub operations: Vec<LongitudinalOperationReceiptV1>,
    pub memory_receipts: Vec<LongitudinalMemoryReceiptV1>,
    pub interruption_receipts: Vec<LongitudinalInterruptionReceiptV1>,
    pub counters: Vec<LongitudinalCounterReceiptV1>,
    pub raw_inventory: Vec<LongitudinalRawFileV1>,
    pub failures: Vec<LongitudinalFailureReceiptV1>,
    pub compensation_applied: bool,
    pub package_sha256: String,
}

impl LongitudinalEvidencePackageV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.package_sha256, |package_sha256| {
            let mut preimage = self.clone();
            preimage.package_sha256 = package_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        let contract = longitudinal_runner_contract_v1();
        if self.schema != LONGITUDINAL_EVIDENCE_PACKAGE_SCHEMA_V1
            || self.contract_sha256 != contract.contract_sha256
        {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        self.base_execution.validate()?;
        if self.base_execution.parent_commit.is_some() {
            return Err(LongitudinalContractError::MixedRevision);
        }
        let base_hash = self.base_execution.canonical_sha256()?;
        let derivative_hash = match &self.derivative_execution {
            Some(derivative) => {
                derivative.validate()?;
                if derivative.parent_commit.as_deref()
                    != Some(self.base_execution.source_commit.as_str())
                {
                    return Err(LongitudinalContractError::MixedRevision);
                }
                Some(derivative.canonical_sha256()?)
            }
            None => None,
        };
        if self.raw_inventory.is_empty() {
            return Err(LongitudinalContractError::MissingRawInventory);
        }
        if self.compensation_applied {
            return Err(LongitudinalContractError::CompensationForbidden);
        }
        require_unique(
            self.raw_inventory
                .iter()
                .map(|file| file.relative_path.as_str()),
            "raw inventory paths",
        )?;
        for file in &self.raw_inventory {
            validate_relative_path(&file.relative_path)?;
            validate_hex(&file.sha256, 64, "raw file SHA-256")?;
        }
        for receipt in &self.materializations {
            receipt.validate()?;
            if receipt.manifest.execution != self.base_execution {
                return Err(LongitudinalContractError::MixedRevision);
            }
        }
        let mut sample_keys = BTreeSet::new();
        for receipt in &self.operations {
            receipt.validate()?;
            if receipt.execution_identity_sha256 != base_hash {
                return Err(LongitudinalContractError::MixedRevision);
            }
            if !sample_keys.insert((
                receipt.run_identity.clone(),
                receipt.operation,
                receipt.sample_ordinal,
            )) {
                return Err(LongitudinalContractError::DuplicateRow {
                    field: "operation samples",
                });
            }
        }
        let mut memory_keys = BTreeSet::new();
        for receipt in &self.memory_receipts {
            receipt.validate()?;
            if receipt.execution_identity_sha256 != base_hash
                || !memory_keys.insert((receipt.tier, receipt.checkpoint))
            {
                return Err(LongitudinalContractError::MixedRevision);
            }
        }
        let mut interruption_keys = BTreeSet::new();
        for receipt in &self.interruption_receipts {
            receipt.validate()?;
            if !interruption_keys.insert((receipt.operation, receipt.point)) {
                return Err(LongitudinalContractError::DuplicateRow {
                    field: "interruption receipts",
                });
            }
        }
        let mut counter_keys = BTreeSet::new();
        for receipt in &self.counters {
            receipt.validate()?;
            if receipt.base_execution_identity_sha256 != base_hash
                || derivative_hash.as_deref()
                    != Some(receipt.derivative_execution_identity_sha256.as_str())
            {
                return Err(LongitudinalContractError::MixedRevision);
            }
            if !counter_keys.insert((
                receipt.run_identity.clone(),
                receipt.operation.clone(),
                receipt.phase.clone(),
            )) {
                return Err(LongitudinalContractError::DuplicateRow {
                    field: "counter receipts",
                });
            }
        }
        for failure in &self.failures {
            if !failure.immutable {
                return Err(LongitudinalContractError::MutableFailure);
            }
            require_nonempty(&failure.reason_code, "failure reason")?;
            validate_hex(&failure.detail_sha256, 64, "failure detail SHA-256")?;
        }
        if self.failures.len() > 1 {
            return Err(LongitudinalContractError::IncompleteEvidence);
        }
        match self.purpose {
            LongitudinalPackagePurposeV1::NonTimingSmoke => {
                if !self.failures.is_empty() {
                    return Err(LongitudinalContractError::IncompleteEvidence);
                }
            }
            LongitudinalPackagePurposeV1::TerminalEvidence => {
                if self.failures.is_empty() {
                    validate_complete_workload_package(self)?;
                }
            }
        }
        validate_bound_hash(
            &self.package_sha256,
            &self.canonical_sha256()?,
            "longitudinal evidence package",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCapacityPackageV1 {
    pub schema: String,
    pub purpose: LongitudinalPackagePurposeV1,
    pub contract_sha256: String,
    pub linked_v1_package_sha256: String,
    pub base_execution: LongitudinalExecutionIdentityV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derivative_execution: Option<LongitudinalExecutionIdentityV1>,
    pub materializations: Vec<LongitudinalCapacityMaterializationReceiptV1>,
    pub generation_resources: Vec<LongitudinalGenerationResourceReceiptV1>,
    pub probe_receipts: Vec<LongitudinalCapacityReceiptV1>,
    pub counter_receipts: Vec<LongitudinalCounterReceiptV1>,
    pub memory_receipts: Vec<LongitudinalCapacityMemoryReceiptV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contention: Option<LongitudinalContentionReceiptV1>,
    pub c524_gate_inputs: LongitudinalC524GateInputsV1,
    pub c524_gate: LongitudinalC524GateReceiptV1,
    pub raw_inventory: Vec<LongitudinalRawFileV1>,
    pub failures: Vec<LongitudinalFailureReceiptV1>,
    pub compensation_applied: bool,
    pub package_sha256: String,
}

impl LongitudinalCapacityPackageV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.package_sha256, |package_sha256| {
            let mut preimage = self.clone();
            preimage.package_sha256 = package_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        let contract = longitudinal_capacity_contract_v1();
        if self.schema != LONGITUDINAL_CAPACITY_PACKAGE_SCHEMA_V1
            || self.contract_sha256 != contract.contract_sha256
        {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        validate_hex(
            &self.linked_v1_package_sha256,
            64,
            "linked v1 package SHA-256",
        )?;
        self.base_execution.validate()?;
        if self.base_execution.parent_commit.is_some() {
            return Err(LongitudinalContractError::MixedRevision);
        }
        if self.raw_inventory.is_empty() {
            return Err(LongitudinalContractError::MissingRawInventory);
        }
        if self.compensation_applied {
            return Err(LongitudinalContractError::CompensationForbidden);
        }
        let base_hash = self.base_execution.canonical_sha256()?;
        let derivative_hash = match &self.derivative_execution {
            Some(derivative) => {
                derivative.validate()?;
                if derivative.parent_commit.as_deref()
                    != Some(self.base_execution.source_commit.as_str())
                {
                    return Err(LongitudinalContractError::MixedRevision);
                }
                Some(derivative.canonical_sha256()?)
            }
            None => None,
        };
        require_unique(
            self.raw_inventory
                .iter()
                .map(|file| file.relative_path.as_str()),
            "capacity raw inventory paths",
        )?;
        for file in &self.raw_inventory {
            validate_relative_path(&file.relative_path)?;
            validate_hex(&file.sha256, 64, "capacity raw file SHA-256")?;
        }
        for receipt in &self.materializations {
            receipt.validate()?;
            if receipt.manifest.execution != self.base_execution {
                return Err(LongitudinalContractError::MixedRevision);
            }
        }
        let mut generation_subjects = BTreeSet::new();
        for receipt in &self.generation_resources {
            validate_generation_resource(receipt)?;
            if !generation_subjects.insert(receipt.subject) {
                return Err(LongitudinalContractError::DuplicateRow {
                    field: "generation resource subjects",
                });
            }
        }
        let mut probe_keys = BTreeSet::new();
        for receipt in &self.probe_receipts {
            receipt.validate()?;
            if receipt.execution_identity_sha256 != base_hash {
                return Err(LongitudinalContractError::MixedRevision);
            }
            if !probe_keys.insert((receipt.subject, receipt.probe, receipt.sample_ordinal)) {
                return Err(LongitudinalContractError::DuplicateRow {
                    field: "capacity probe samples",
                });
            }
        }
        for receipt in &self.counter_receipts {
            receipt.validate()?;
            if receipt.base_execution_identity_sha256 != base_hash
                || derivative_hash.as_deref()
                    != Some(receipt.derivative_execution_identity_sha256.as_str())
            {
                return Err(LongitudinalContractError::MixedRevision);
            }
        }
        let mut memory_keys = BTreeSet::new();
        for receipt in &self.memory_receipts {
            receipt.validate()?;
            if receipt.execution_identity_sha256 != base_hash {
                return Err(LongitudinalContractError::MixedRevision);
            }
            if !memory_keys.insert((receipt.subject, receipt.checkpoint)) {
                return Err(LongitudinalContractError::DuplicateRow {
                    field: "capacity memory receipts",
                });
            }
        }
        if let Some(contention) = &self.contention {
            contention.validate()?;
        }
        if !self
            .generation_resources
            .contains(&self.c524_gate_inputs.v1_l100_generation)
            || !self
                .generation_resources
                .contains(&self.c524_gate_inputs.c262_generation)
            || self.c524_gate_inputs.c262_identity_complete
                != has_capacity_materialization_pair(
                    &self.materializations,
                    LongitudinalCapacitySubjectV1::Companion(LongitudinalCapacityProfileV1::C262),
                )
        {
            return Err(LongitudinalContractError::ContractDrift {
                field: "C524 package gate inputs",
            });
        }
        self.c524_gate.validate_against(&self.c524_gate_inputs)?;
        for failure in &self.failures {
            if !failure.immutable {
                return Err(LongitudinalContractError::MutableFailure);
            }
            require_nonempty(&failure.reason_code, "capacity failure reason")?;
            validate_hex(
                &failure.detail_sha256,
                64,
                "capacity failure detail SHA-256",
            )?;
        }
        if self.failures.len() > 1 {
            return Err(LongitudinalContractError::IncompleteEvidence);
        }
        match self.purpose {
            LongitudinalPackagePurposeV1::NonTimingSmoke => {
                if !self.failures.is_empty() {
                    return Err(LongitudinalContractError::IncompleteEvidence);
                }
            }
            LongitudinalPackagePurposeV1::TerminalEvidence => {
                if self.failures.is_empty() {
                    validate_complete_capacity_package(self)?;
                }
            }
        }
        validate_bound_hash(
            &self.package_sha256,
            &self.canonical_sha256()?,
            "longitudinal capacity package",
        )
    }
}

fn validate_complete_workload_package(
    package: &LongitudinalEvidencePackageV1,
) -> Result<(), LongitudinalContractError> {
    let [materialization] = package.materializations.as_slice() else {
        return Err(LongitudinalContractError::IncompleteEvidence);
    };
    if package.operations.is_empty()
        || package.operations.iter().any(|receipt| {
            receipt.root_identity != materialization.root_identity
                || receipt.tier != materialization.manifest.tier
                || receipt.manifest_sha256 != materialization.manifest.manifest_sha256
                || receipt.schedule_sha256 != materialization.manifest.schedule_sha256
                || receipt.outcome == LongitudinalOperationOutcomeV1::ValidFailure
        })
    {
        return Err(LongitudinalContractError::IncompleteEvidence);
    }
    let lane = package.operations[0].lane;
    if package
        .operations
        .iter()
        .any(|receipt| receipt.lane != lane)
    {
        return Err(LongitudinalContractError::MixedLane);
    }
    let samples = longitudinal_runner_contract_v1().samples;
    let mut by_operation = BTreeSet::new();
    for operation in LongitudinalOperationV1::ALL {
        let rows = package
            .operations
            .iter()
            .filter(|receipt| receipt.operation == operation)
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return Err(LongitudinalContractError::IncompleteEvidence);
        }
        let governed_unavailable = rows.len() == 1
            && rows[0].sample_ordinal == 0
            && rows[0].outcome == LongitudinalOperationOutcomeV1::UnavailableNoCurrentSurface
            && matches!(
                operation,
                LongitudinalOperationV1::AuditExact
                    | LongitudinalOperationV1::ExportExact
                    | LongitudinalOperationV1::MigrateFresh
                    | LongitudinalOperationV1::RebuildFull
            );
        let expected = expected_workload_samples(lane, operation, &samples)?;
        let exact_measured = rows.len() == usize::from(expected)
            && rows.iter().enumerate().all(|(ordinal, receipt)| {
                receipt.sample_ordinal == ordinal as u32
                    && receipt.outcome == LongitudinalOperationOutcomeV1::Measured
            });
        if !governed_unavailable && !exact_measured {
            return Err(LongitudinalContractError::IncompleteEvidence);
        }
        by_operation.insert(operation);
    }
    if by_operation.len() != LongitudinalOperationV1::ALL.len() {
        return Err(LongitudinalContractError::IncompleteEvidence);
    }
    validate_workload_operation_order(&package.operations)?;

    match lane {
        LongitudinalLaneV1::ReleaseUninstrumented => {
            let checkpoints = package
                .memory_receipts
                .iter()
                .map(|receipt| receipt.checkpoint)
                .collect::<BTreeSet<_>>();
            if checkpoints != LongitudinalMemoryCheckpointV1::ALL.into_iter().collect()
                || package
                    .memory_receipts
                    .iter()
                    .any(|receipt| receipt.tier != materialization.manifest.tier)
            {
                return Err(LongitudinalContractError::IncompleteEvidence);
            }
            let interruption_keys = package
                .interruption_receipts
                .iter()
                .map(|receipt| (receipt.operation, receipt.point))
                .collect::<BTreeSet<_>>();
            let expected = LongitudinalInterruptionOperationV1::ALL
                .into_iter()
                .flat_map(|operation| {
                    LongitudinalInterruptionPointV1::ALL
                        .into_iter()
                        .map(move |point| (operation, point))
                })
                .collect::<BTreeSet<_>>();
            if interruption_keys != expected {
                return Err(LongitudinalContractError::IncompleteEvidence);
            }
        }
        LongitudinalLaneV1::DebugUninstrumented => {
            if !package.memory_receipts.is_empty() || !package.interruption_receipts.is_empty() {
                return Err(LongitudinalContractError::IncompleteEvidence);
            }
        }
        LongitudinalLaneV1::AttributionCounts => {
            return Err(LongitudinalContractError::MixedLane);
        }
    }
    Ok(())
}

fn expected_workload_samples(
    lane: LongitudinalLaneV1,
    operation: LongitudinalOperationV1,
    samples: &LongitudinalSamplePlanV1,
) -> Result<u16, LongitudinalContractError> {
    use LongitudinalOperationV1 as Operation;
    match lane {
        LongitudinalLaneV1::ReleaseUninstrumented => Ok(match operation {
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
            Operation::AppendOne | Operation::PostOne => {
                samples.append_one_samples_per_release_root
            }
            Operation::AppendBurst | Operation::PostBurst => {
                samples.append_burst_samples_per_release_root
            }
            Operation::Restart => samples.restart_samples_per_release_root,
            Operation::AuditExact
            | Operation::ExportExact
            | Operation::MigrateFresh
            | Operation::RebuildFull => samples.maintenance_samples_per_release_root,
        }),
        LongitudinalLaneV1::DebugUninstrumented => Ok(match operation {
            Operation::ColdHead => samples.debug_cold_samples,
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
            | Operation::NewCountZero => samples.debug_warm_samples,
            _ => samples.debug_write_restart_maintenance_samples,
        }),
        LongitudinalLaneV1::AttributionCounts => Err(LongitudinalContractError::MixedLane),
    }
}

fn validate_workload_operation_order(
    operations: &[LongitudinalOperationReceiptV1],
) -> Result<(), LongitudinalContractError> {
    let mut expected = Vec::new();
    append_operation_keys(&mut expected, operations, LongitudinalOperationV1::ColdHead);
    for operation in [
        LongitudinalOperationV1::WarmHead,
        LongitudinalOperationV1::WinAdjacent,
        LongitudinalOperationV1::WinDeep,
        LongitudinalOperationV1::WinTail,
        LongitudinalOperationV1::AtDeep,
        LongitudinalOperationV1::SearchStructured,
        LongitudinalOperationV1::SearchBody,
        LongitudinalOperationV1::SearchMiss,
        LongitudinalOperationV1::FilterFacet,
        LongitudinalOperationV1::Revisions,
        LongitudinalOperationV1::Threads,
        LongitudinalOperationV1::AttentionAll,
        LongitudinalOperationV1::AttentionOne,
        LongitudinalOperationV1::DetailActive,
        LongitudinalOperationV1::SnapshotActive,
        LongitudinalOperationV1::DetailRemoved,
        LongitudinalOperationV1::FreshNoChange,
        LongitudinalOperationV1::NewCountZero,
    ] {
        append_operation_keys(&mut expected, operations, operation);
    }
    let append_count = operations
        .iter()
        .filter(|receipt| receipt.operation == LongitudinalOperationV1::AppendOne)
        .count();
    for ordinal in 0..append_count as u32 {
        expected.push((LongitudinalOperationV1::AppendOne, ordinal));
        expected.push((LongitudinalOperationV1::PostOne, ordinal));
    }
    let burst_count = operations
        .iter()
        .filter(|receipt| receipt.operation == LongitudinalOperationV1::AppendBurst)
        .count();
    for ordinal in 0..burst_count as u32 {
        expected.push((LongitudinalOperationV1::AppendBurst, ordinal));
        expected.push((LongitudinalOperationV1::PostBurst, ordinal));
    }
    append_operation_keys(&mut expected, operations, LongitudinalOperationV1::Restart);
    for operation in [
        LongitudinalOperationV1::AuditExact,
        LongitudinalOperationV1::ExportExact,
        LongitudinalOperationV1::MigrateFresh,
        LongitudinalOperationV1::RebuildFull,
    ] {
        append_operation_keys(&mut expected, operations, operation);
    }
    if operations
        .iter()
        .map(|receipt| (receipt.operation, receipt.sample_ordinal))
        .ne(expected)
    {
        return Err(LongitudinalContractError::IncompleteEvidence);
    }
    Ok(())
}

fn append_operation_keys(
    output: &mut Vec<(LongitudinalOperationV1, u32)>,
    operations: &[LongitudinalOperationReceiptV1],
    operation: LongitudinalOperationV1,
) {
    output.extend(
        operations
            .iter()
            .filter(|receipt| receipt.operation == operation)
            .map(|receipt| (operation, receipt.sample_ordinal)),
    );
}

fn validate_complete_capacity_package(
    package: &LongitudinalCapacityPackageV1,
) -> Result<(), LongitudinalContractError> {
    if package.materializations.is_empty()
        || package.generation_resources.is_empty()
        || package.probe_receipts.is_empty()
        || package.memory_receipts.is_empty()
    {
        return Err(LongitudinalContractError::IncompleteEvidence);
    }
    let subjects = package
        .probe_receipts
        .iter()
        .map(|receipt| receipt.subject)
        .collect::<BTreeSet<_>>();
    for subject in subjects {
        for probe in LongitudinalCapacityProbeV1::ALL {
            let mut rows = package
                .probe_receipts
                .iter()
                .filter(|receipt| receipt.subject == subject && receipt.probe == probe)
                .collect::<Vec<_>>();
            rows.sort_by_key(|receipt| receipt.sample_ordinal);
            let valid = if probe == LongitudinalCapacityProbeV1::AppendDelta {
                rows.len() == 1
                    && rows[0].sample_ordinal == 0
                    && rows[0].metrics.is_none()
                    && rows[0].classification
                        == LongitudinalCapacityClassificationV1::UnsupportedNoCurrentSurface
            } else {
                rows.len() == 6
                    && rows.iter().enumerate().all(|(ordinal, receipt)| {
                        receipt.sample_ordinal == ordinal as u16
                            && receipt.cold == (ordinal == 0)
                            && receipt.metrics.is_some()
                            && !matches!(
                                receipt.classification,
                                LongitudinalCapacityClassificationV1::UnsupportedNoCurrentSurface
                                    | LongitudinalCapacityClassificationV1::ValidFailure
                                    | LongitudinalCapacityClassificationV1::NotRunByGate
                            )
                    })
            };
            if !valid {
                return Err(LongitudinalContractError::IncompleteEvidence);
            }
        }
        for checkpoint in [
            LongitudinalCapacityMemoryCheckpointV1::ClosedRootReopenIdle,
            LongitudinalCapacityMemoryCheckpointV1::PostCarrierKey,
            LongitudinalCapacityMemoryCheckpointV1::PostSemanticId,
            LongitudinalCapacityMemoryCheckpointV1::PostChronologicalHead,
            LongitudinalCapacityMemoryCheckpointV1::PostChronologicalMiddle,
            LongitudinalCapacityMemoryCheckpointV1::PostChronologicalTail,
        ] {
            if !package
                .memory_receipts
                .iter()
                .any(|receipt| receipt.subject == subject && receipt.checkpoint == checkpoint)
            {
                return Err(LongitudinalContractError::IncompleteEvidence);
            }
        }
        if subject
            == LongitudinalCapacitySubjectV1::Companion(LongitudinalCapacityProfileV1::L100O10K)
        {
            for checkpoint in [
                LongitudinalCapacityMemoryCheckpointV1::PostObjectDetailL100O10K,
                LongitudinalCapacityMemoryCheckpointV1::PostContentionQuiescence,
            ] {
                if !package
                    .memory_receipts
                    .iter()
                    .any(|receipt| receipt.subject == subject && receipt.checkpoint == checkpoint)
                {
                    return Err(LongitudinalContractError::IncompleteEvidence);
                }
            }
            package
                .contention
                .as_ref()
                .ok_or(LongitudinalContractError::IncompleteEvidence)?
                .validate()?;
        }
    }
    Ok(())
}

fn has_capacity_materialization_pair(
    receipts: &[LongitudinalCapacityMaterializationReceiptV1],
    subject: LongitudinalCapacitySubjectV1,
) -> bool {
    let mut matching = receipts
        .iter()
        .filter(|receipt| receipt.manifest.subject == subject);
    let (Some(left), Some(right)) = (matching.next(), matching.next()) else {
        return false;
    };
    matching.next().is_none() && verify_longitudinal_capacity_pair_v1(left, right).is_ok()
}

pub fn verify_longitudinal_materialization_pair_v1(
    left: &LongitudinalMaterializationReceiptV1,
    right: &LongitudinalMaterializationReceiptV1,
) -> Result<(), LongitudinalContractError> {
    left.validate()?;
    right.validate()?;
    if left.root_identity == right.root_identity {
        return Err(LongitudinalContractError::RootIdentityNotDistinct);
    }
    if left.pair_identity_sha256()? != right.pair_identity_sha256()? {
        return Err(LongitudinalContractError::PairMismatch);
    }
    Ok(())
}

pub fn verify_longitudinal_capacity_pair_v1(
    left: &LongitudinalCapacityMaterializationReceiptV1,
    right: &LongitudinalCapacityMaterializationReceiptV1,
) -> Result<(), LongitudinalContractError> {
    left.validate()?;
    right.validate()?;
    if left.root_identity == right.root_identity {
        return Err(LongitudinalContractError::RootIdentityNotDistinct);
    }
    if left.pair_identity_sha256()? != right.pair_identity_sha256()? {
        return Err(LongitudinalContractError::PairMismatch);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalContractPublicationV1 {
    pub schema: String,
    pub mode: String,
    pub workload: LongitudinalRunnerContractV1,
    pub capacity: LongitudinalCapacityContractV1,
    pub publication_sha256: String,
}

impl LongitudinalContractPublicationV1 {
    pub fn canonical_sha256(&self) -> Result<String, LongitudinalContractError> {
        canonical_sha256_without(&self.publication_sha256, |publication_sha256| {
            let mut preimage = self.clone();
            preimage.publication_sha256 = publication_sha256;
            preimage
        })
    }

    pub fn validate(&self) -> Result<(), LongitudinalContractError> {
        if self.schema != LONGITUDINAL_CONTRACT_PUBLICATION_SCHEMA_V1
            || self.mode != "non_timing_contract_publication"
        {
            return Err(LongitudinalContractError::UnsupportedContract);
        }
        self.workload.validate()?;
        self.capacity.validate()?;
        validate_bound_hash(
            &self.publication_sha256,
            LONGITUDINAL_CONTRACT_PUBLICATION_SHA256_V1,
            "longitudinal contract publication",
        )?;
        if self.canonical_sha256()? != LONGITUDINAL_CONTRACT_PUBLICATION_SHA256_V1 {
            return Err(LongitudinalContractError::ContractDrift {
                field: "longitudinal contract publication",
            });
        }
        Ok(())
    }
}

pub fn longitudinal_contract_publication_v1() -> LongitudinalContractPublicationV1 {
    let mut publication = LongitudinalContractPublicationV1 {
        schema: LONGITUDINAL_CONTRACT_PUBLICATION_SCHEMA_V1.to_owned(),
        mode: "non_timing_contract_publication".to_owned(),
        workload: longitudinal_runner_contract_v1(),
        capacity: longitudinal_capacity_contract_v1(),
        publication_sha256: String::new(),
    };
    publication.publication_sha256 = publication
        .canonical_sha256()
        .expect("the frozen longitudinal publication is canonical");
    publication
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalHelpModeV1 {
    pub flag: String,
    pub purpose: String,
    pub constructs_roots: bool,
    pub runs_timing: bool,
    pub reads_private_inputs: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalHelpV1 {
    pub schema: String,
    pub modes: Vec<LongitudinalHelpModeV1>,
    pub native_evidence_guidance: String,
    pub later_materialization_available: bool,
    pub later_evidence_available: bool,
    pub private_inputs_allowed: bool,
}

pub fn longitudinal_help_v1() -> LongitudinalHelpV1 {
    LongitudinalHelpV1 {
        schema: LONGITUDINAL_HELP_SCHEMA_V1.to_owned(),
        modes: vec![
            LongitudinalHelpModeV1 {
                flag: LONGITUDINAL_CONTRACT_MODE_V1.to_owned(),
                purpose: "print the frozen workload and capacity contracts".to_owned(),
                constructs_roots: false,
                runs_timing: false,
                reads_private_inputs: false,
            },
            LongitudinalHelpModeV1 {
                flag: LONGITUDINAL_HELP_MODE_V1.to_owned(),
                purpose: "describe the available non-timing longitudinal modes".to_owned(),
                constructs_roots: false,
                runs_timing: false,
                reads_private_inputs: false,
            },
            LongitudinalHelpModeV1 {
                flag: LONGITUDINAL_SMOKE_MODE_V1.to_owned(),
                purpose: "exercise disposable public construction, pair, preflight, and package mechanics"
                    .to_owned(),
                constructs_roots: true,
                runs_timing: false,
                reads_private_inputs: false,
            },
            LongitudinalHelpModeV1 {
                flag: LONGITUDINAL_VERIFY_PACKAGE_MODE_V1.to_owned(),
                purpose: "read and recursively verify one completed raw-evidence package"
                    .to_owned(),
                constructs_roots: false,
                runs_timing: false,
                reads_private_inputs: false,
            },
            LongitudinalHelpModeV1 {
                flag: LONGITUDINAL_CARRY_FORWARD_MODE_V1.to_owned(),
                purpose: "strictly verify an immutable source and isolated clone, then write completion-last carry authority"
                    .to_owned(),
                constructs_roots: false,
                runs_timing: false,
                reads_private_inputs: false,
            },
            LongitudinalHelpModeV1 {
                flag: LONGITUDINAL_CARRY_FORWARD_SMOKE_MODE_V1.to_owned(),
                purpose: "exercise disposable carry-forward, failure, and package-verifier mechanics"
                    .to_owned(),
                constructs_roots: true,
                runs_timing: false,
                reads_private_inputs: false,
            },
            LongitudinalHelpModeV1 {
                flag: LONGITUDINAL_VERIFY_PACKAGE_RECEIPT_MODE_V1.to_owned(),
                purpose: "derive a typed verification receipt from one completed raw-evidence package"
                    .to_owned(),
                constructs_roots: false,
                runs_timing: false,
                reads_private_inputs: false,
            },
            LongitudinalHelpModeV1 {
                flag: LONGITUDINAL_VERIFY_CARRY_FORWARD_MODE_V1.to_owned(),
                purpose: "verify final carry-forward authority against its exact workload package"
                    .to_owned(),
                constructs_roots: false,
                runs_timing: false,
                reads_private_inputs: false,
            },
        ],
        native_evidence_guidance:
            "use the separately frozen explicit operator command ledger; native collection is not a public bench mode"
                .to_owned(),
        later_materialization_available: true,
        later_evidence_available: true,
        private_inputs_allowed: false,
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum LongitudinalContractError {
    #[error("unsupported longitudinal contract identity")]
    UnsupportedContract,
    #[error("unsupported longitudinal tier")]
    UnsupportedTier,
    #[error("unsupported longitudinal capacity profile")]
    UnsupportedProfile,
    #[error("{field} is empty")]
    EmptyField { field: &'static str },
    #[error("{field} must be {length} lowercase hexadecimal characters")]
    InvalidHex { field: &'static str, length: usize },
    #[error("{field} hash mismatch")]
    HashMismatch { field: &'static str },
    #[error("{field} count mismatch: expected {expected}, got {actual}")]
    CountMismatch {
        field: &'static str,
        expected: u64,
        actual: u64,
    },
    #[error("duplicate {field}")]
    DuplicateRow { field: &'static str },
    #[error("{field} drifted from the frozen contract")]
    ContractDrift { field: &'static str },
    #[error("materialization roots must be distinct")]
    RootIdentityNotDistinct,
    #[error("materialization pair identity differs")]
    PairMismatch,
    #[error("uninstrumented and attribution lanes were mixed")]
    MixedLane,
    #[error("evidence contains mixed source revisions")]
    MixedRevision,
    #[error("raw evidence inventory is absent")]
    MissingRawInventory,
    #[error("performance or complexity compensation is forbidden")]
    CompensationForbidden,
    #[error("a retained failure was marked mutable")]
    MutableFailure,
    #[error("evidence package contains neither one causal failure nor a complete required matrix")]
    IncompleteEvidence,
    #[error("raw inventory path must be relative and normalized")]
    InvalidRelativePath,
    #[error("canonical JSON failed: {message}")]
    CanonicalJson { message: String },
}

impl LongitudinalContractError {
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::UnsupportedContract => "unsupported_contract",
            Self::UnsupportedTier => "unsupported_tier",
            Self::UnsupportedProfile => "unsupported_profile",
            Self::EmptyField { .. } => "empty_field",
            Self::InvalidHex { .. } => "invalid_hex",
            Self::HashMismatch { .. } => "hash_mismatch",
            Self::CountMismatch { .. } => "count_mismatch",
            Self::DuplicateRow { .. } => "duplicate_row",
            Self::ContractDrift { .. } => "contract_drift",
            Self::RootIdentityNotDistinct => "root_identity_not_distinct",
            Self::PairMismatch => "pair_mismatch",
            Self::MixedLane => "mixed_lane",
            Self::MixedRevision => "mixed_revision",
            Self::MissingRawInventory => "missing_raw_inventory",
            Self::CompensationForbidden => "compensation_forbidden",
            Self::MutableFailure => "mutable_failure",
            Self::IncompleteEvidence => "incomplete_evidence",
            Self::InvalidRelativePath => "invalid_relative_path",
            Self::CanonicalJson { .. } => "canonical_json",
        }
    }
}

fn canonical_sha256<T: Serialize>(value: &T) -> Result<String, LongitudinalContractError> {
    let value =
        serde_json::to_value(value).map_err(|error| LongitudinalContractError::CanonicalJson {
            message: error.to_string(),
        })?;
    let bytes =
        canonical_json_bytes(&value).map_err(|error| LongitudinalContractError::CanonicalJson {
            message: error.to_string(),
        })?;
    Ok(sha256_bytes_hex(&bytes))
}

fn canonical_sha256_without<T: Serialize>(
    _current: &str,
    preimage: impl FnOnce(String) -> T,
) -> Result<String, LongitudinalContractError> {
    canonical_sha256(&preimage(String::new()))
}

fn validate_bound_hash(
    provided: &str,
    expected: &str,
    field: &'static str,
) -> Result<(), LongitudinalContractError> {
    validate_hex(provided, 64, field)?;
    if provided != expected {
        return Err(LongitudinalContractError::HashMismatch { field });
    }
    Ok(())
}

fn validate_hex(
    value: &str,
    length: usize,
    field: &'static str,
) -> Result<(), LongitudinalContractError> {
    if value.len() != length
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(LongitudinalContractError::InvalidHex { field, length });
    }
    Ok(())
}

fn validate_prefixed_hash(
    value: &str,
    prefix: &'static str,
    field: &'static str,
) -> Result<(), LongitudinalContractError> {
    let digest = value
        .strip_prefix(prefix)
        .ok_or(LongitudinalContractError::InvalidHex { field, length: 64 })?;
    validate_hex(digest, 64, field)
}

fn require_nonempty(value: &str, field: &'static str) -> Result<(), LongitudinalContractError> {
    if value.trim().is_empty() {
        return Err(LongitudinalContractError::EmptyField { field });
    }
    Ok(())
}

fn require_count(
    actual: u64,
    expected: u64,
    field: &'static str,
) -> Result<(), LongitudinalContractError> {
    if actual != expected {
        return Err(LongitudinalContractError::CountMismatch {
            field,
            expected,
            actual,
        });
    }
    Ok(())
}

fn require_len(
    actual: usize,
    expected: u64,
    field: &'static str,
) -> Result<(), LongitudinalContractError> {
    require_count(
        u64::try_from(actual).map_err(|_| LongitudinalContractError::CountMismatch {
            field,
            expected,
            actual: u64::MAX,
        })?,
        expected,
        field,
    )
}

fn require_unique<'a>(
    values: impl IntoIterator<Item = &'a str>,
    field: &'static str,
) -> Result<(), LongitudinalContractError> {
    let mut seen = BTreeSet::new();
    if values.into_iter().any(|value| !seen.insert(value)) {
        return Err(LongitudinalContractError::DuplicateRow { field });
    }
    Ok(())
}

fn require_exact_sequence<T, const N: usize>(
    actual: impl IntoIterator<Item = T>,
    expected: [T; N],
    field: &'static str,
) -> Result<(), LongitudinalContractError>
where
    T: Eq,
{
    if !actual.into_iter().eq(expected) {
        return Err(LongitudinalContractError::ContractDrift { field });
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<(), LongitudinalContractError> {
    let path = std::path::Path::new(path);
    if path.is_absolute()
        || path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(LongitudinalContractError::InvalidRelativePath);
    }
    Ok(())
}

fn scaled_event_families(
    families: &[LongitudinalEventFamilyRequirementV1],
    blocks: u64,
) -> Vec<LongitudinalEventFamilyCountV1> {
    families
        .iter()
        .map(|family| LongitudinalEventFamilyCountV1 {
            event_type: family.event_type.clone(),
            count: family.per_block * blocks,
        })
        .collect()
}

fn materialization_subject_counts(
    subject: LongitudinalMaterializationSubjectV1,
) -> Result<(u64, u64), LongitudinalContractError> {
    match subject {
        LongitudinalMaterializationSubjectV1::Workload(tier) => {
            let requirement = longitudinal_runner_contract_v1()
                .tiers
                .into_iter()
                .find(|candidate| candidate.tier == tier)
                .ok_or(LongitudinalContractError::UnsupportedTier)?;
            Ok((requirement.event_count, requirement.present_content_count))
        }
        LongitudinalMaterializationSubjectV1::Capacity(profile) => {
            let requirement = longitudinal_capacity_contract_v1()
                .profiles
                .into_iter()
                .find(|candidate| candidate.profile == profile)
                .ok_or(LongitudinalContractError::UnsupportedProfile)?;
            Ok((requirement.event_count, requirement.present_content_count))
        }
    }
}

fn validate_strict_receipt(
    receipt: &LongitudinalStrictSemanticReceiptV1,
) -> Result<(), LongitudinalContractError> {
    for (hash, field) in [
        (&receipt.event_set_sha256, "event-set SHA-256"),
        (&receipt.ordered_journal_sha256, "ordered-journal SHA-256"),
        (&receipt.state_sha256, "state SHA-256"),
        (&receipt.projection_sha256, "projection SHA-256"),
        (
            &receipt.content_inventory_sha256,
            "content-inventory SHA-256",
        ),
    ] {
        validate_hex(hash, 64, field)?;
    }
    Ok(())
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    fn digest(label: &str) -> String {
        sha256_bytes_hex(label.as_bytes())
    }

    fn execution() -> LongitudinalExecutionIdentityV1 {
        LongitudinalExecutionIdentityV1 {
            source_commit: "1".repeat(40),
            source_tree: "2".repeat(40),
            cargo_lock_sha256: "3".repeat(64),
            runner_sha256: "4".repeat(64),
            build_profile: "release".to_owned(),
            operating_system: "macos".to_owned(),
            architecture: "aarch64".to_owned(),
            filesystem: "apfs".to_owned(),
            parent_commit: None,
        }
    }

    fn interaction_execution() -> InteractionExecutionIdentityV1 {
        InteractionExecutionIdentityV1 {
            source_commit: "a".repeat(40),
            source_tree: "b".repeat(40),
            cargo_lock_sha256: digest("interaction-cargo-lock"),
            binary_path: std::env::current_exe()
                .expect("current interaction contract test binary")
                .display()
                .to_string(),
            binary_sha256: digest("interaction-binary"),
            build_profile: "debug".to_owned(),
            rustc_version: "rustc 1.89.0".to_owned(),
            features: vec!["gix".to_owned(), "longitudinal-counting".to_owned()],
        }
    }

    fn interaction_context(
        route: InteractionRouteV1,
        setup_expectation: InteractionSetupExpectationV1,
    ) -> InteractionPerformanceExpectedContextV1 {
        let is_version = route == InteractionRouteV1::VersionJson;
        let requires_track = matches!(
            route,
            InteractionRouteV1::AssessmentCurrentResult
                | InteractionRouteV1::AssessmentCurrentSummary
                | InteractionRouteV1::ObservationReviewerList
                | InteractionRouteV1::ValidationReviewerList
        );
        InteractionPerformanceExpectedContextV1 {
            execution: interaction_execution(),
            route,
            arguments: if is_version {
                vec![
                    "version".to_owned(),
                    "--format".to_owned(),
                    "json".to_owned(),
                ]
            } else {
                vec!["fixture-route".to_owned()]
            },
            setup_expectation,
            fixture_identity_sha256: (!is_version).then(|| digest("fixture")),
            revision: (!is_version).then(|| format!("rev:sha256:{}", digest("revision"))),
            track: requires_track.then(|| "agent:reviewer".to_owned()),
            domain_actor: (!is_version).then(|| "actor:agent:claude-code".to_owned()),
            expected_child_actors: BTreeMap::new(),
        }
    }

    #[test]
    fn interaction_contract_round_trips_strict_expected_and_fact_types() {
        let context = interaction_context(
            InteractionRouteV1::AssessmentCurrentSummary,
            InteractionSetupExpectationV1::AuthoritativeReplay,
        );
        context.validate().expect("valid expected context");
        let encoded = serde_json::to_value(&context).expect("expected context JSON");
        assert_eq!(
            serde_json::from_value::<InteractionPerformanceExpectedContextV1>(encoded.clone())
                .expect("strict expected context"),
            context
        );

        let mut unknown = encoded;
        unknown
            .as_object_mut()
            .expect("expected context object")
            .insert(
                "observedState".to_owned(),
                serde_json::json!("derived_current"),
            );
        assert!(
            serde_json::from_value::<InteractionPerformanceExpectedContextV1>(unknown).is_err()
        );

        for actor in [
            InteractionActorV1::RequestReader,
            InteractionActorV1::ProductWriter,
            InteractionActorV1::BackgroundMaintenance,
            InteractionActorV1::BackgroundRebuild,
            InteractionActorV1::ExplicitRecovery,
        ] {
            let json = serde_json::to_string(&actor).expect("actor JSON");
            assert_eq!(
                serde_json::from_str::<InteractionActorV1>(&json).expect("actor round trip"),
                actor
            );
        }

        for coverage in [
            InteractionScopeCoverageV1::Complete,
            InteractionScopeCoverageV1::Incomplete {
                reason: "child propagation unavailable".to_owned(),
            },
        ] {
            coverage.validate().expect("valid coverage");
            let json = serde_json::to_string(&coverage).expect("coverage JSON");
            assert_eq!(
                serde_json::from_str::<InteractionScopeCoverageV1>(&json)
                    .expect("coverage round trip"),
                coverage
            );
        }

        let facts = InteractionObservedFactsV1 {
            route: InteractionRouteV1::AssessmentCurrentSummary,
            route_state: InteractionObservedRouteStateV1::AuthoritativeReplay,
            execution_actor: InteractionActorV1::RequestReader,
            success: true,
            exit_code: 0,
            semantic_result_sha256: digest("stdout"),
        };
        facts.validate().expect("valid observed facts");
        let json = serde_json::to_string(&facts).expect("observed facts JSON");
        assert_eq!(
            serde_json::from_str::<InteractionObservedFactsV1>(&json)
                .expect("observed facts round trip"),
            facts
        );

        let child = InteractionChildScopeFactV1 {
            ordinal: 0,
            actor: InteractionActorV1::BackgroundMaintenance,
            coverage: InteractionScopeCoverageV1::Complete,
        };
        child.validate().expect("valid child fact");
        let lock = InteractionLockFactV1 {
            ordinal: 0,
            actor: InteractionActorV1::ProductWriter,
            kind: InteractionLockKindV1::Authority,
            mode: InteractionLockModeV1::Blocking,
            outcome: InteractionLockOutcomeV1::Acquired,
            acquisition: InteractionLockAcquisitionV1::Physical,
            wait_nanos: 7,
            hold_nanos: Some(11),
        };
        lock.validate().expect("valid lock fact");
    }

    #[test]
    fn interaction_contract_freezes_fact_and_attention_state_matrices() {
        use crate::bench_support::longitudinal::{
            INTERACTION_FACT_CURRENT_FORBIDDEN_PHASES_V1,
            INTERACTION_FACT_CURRENT_REQUIRED_PHASES_V1, LongitudinalDerivedAccessPhaseV1 as Phase,
        };

        let target = InteractionPerformanceRoleV1::ProvisionalTarget {
            sample_count: 5,
            strict_upper_bound_millis: 2_000,
        };
        macro_rules! assert_state {
            ($route:expr, $setup:expr, $expected:expr) => {{
                let contract = interaction_route_state_contract_v1($route, $setup).unwrap();
                assert_eq!(
                    (
                        contract.observed,
                        contract.performance_role,
                        contract.success,
                        contract.strict_authoritative_snapshots,
                        contract.background_maintenance_children,
                        contract.authoritative_fallbacks,
                        contract.full_history_fallbacks,
                        contract.fallback_presentation,
                    ),
                    $expected
                );
                assert!(contract.historical_evidence_unchanged);
                contract
            }};
        }

        for route in InteractionRouteV1::FACT_READS {
            assert_state!(
                route,
                InteractionSetupExpectationV1::FactActiveCurrent,
                (
                    InteractionObservedRouteStateV1::DerivedCurrent,
                    target,
                    true,
                    0,
                    0,
                    0,
                    0,
                    InteractionFallbackPresentationV1::None,
                )
            );
            for (setup, expected) in [
                (
                    InteractionSetupExpectationV1::FactExplicitOff,
                    (
                        InteractionObservedRouteStateV1::AuthoritativeReplay,
                        InteractionPerformanceRoleV1::CompatibilityCharacterization,
                        true,
                        1,
                        0,
                        0,
                        0,
                        InteractionFallbackPresentationV1::None,
                    ),
                ),
                (
                    InteractionSetupExpectationV1::FactActiveUnavailable,
                    (
                        InteractionObservedRouteStateV1::UnlabeledFallbackToAuthoritative,
                        InteractionPerformanceRoleV1::CompatibilityCharacterization,
                        true,
                        1,
                        1,
                        1,
                        1,
                        InteractionFallbackPresentationV1::Unlabeled,
                    ),
                ),
                (
                    InteractionSetupExpectationV1::FactPostSelectionFailure,
                    (
                        InteractionObservedRouteStateV1::DerivedSelectionFailedClosed,
                        InteractionPerformanceRoleV1::TerminalDiagnostic,
                        false,
                        0,
                        1,
                        0,
                        0,
                        InteractionFallbackPresentationV1::None,
                    ),
                ),
            ] {
                assert_state!(route, setup, expected);
            }
        }

        assert_eq!(
            INTERACTION_FACT_CURRENT_REQUIRED_PHASES_V1,
            [
                Phase::FactSqliteSelection,
                Phase::FactSelectedCarrierHydrationValidation,
                Phase::FactSupportCarrierHydrationValidation,
                Phase::FactWorkflowProjection,
            ]
        );
        assert_eq!(
            INTERACTION_FACT_CURRENT_FORBIDDEN_PHASES_V1,
            [
                Phase::WorkflowChangeReaderReplayH3,
                Phase::WorkflowChangeStoreReopenInspection,
                Phase::CacheAndFallback,
            ]
        );

        for (setup, expected) in [
            (
                InteractionSetupExpectationV1::AttentionDerivedCurrent,
                (
                    InteractionObservedRouteStateV1::DerivedCurrent,
                    target,
                    true,
                    0,
                    0,
                    0,
                    0,
                    InteractionFallbackPresentationV1::None,
                ),
            ),
            (
                InteractionSetupExpectationV1::AttentionColdInactive,
                (
                    InteractionObservedRouteStateV1::AuthoritativeReplay,
                    InteractionPerformanceRoleV1::CompatibilityCharacterization,
                    true,
                    1,
                    0,
                    0,
                    0,
                    InteractionFallbackPresentationV1::None,
                ),
            ),
            (
                InteractionSetupExpectationV1::AttentionActiveUnavailable,
                (
                    InteractionObservedRouteStateV1::LabeledFallbackToAuthoritative,
                    InteractionPerformanceRoleV1::CompatibilityCharacterization,
                    true,
                    1,
                    1,
                    1,
                    1,
                    InteractionFallbackPresentationV1::LabeledHintBeforeError,
                ),
            ),
        ] {
            assert_state!(
                InteractionRouteV1::AttentionCurrentOrFallback,
                setup,
                expected
            );
        }
    }

    #[test]
    fn interaction_expected_context_rejects_identity_and_route_drift() {
        let mut context = interaction_context(
            InteractionRouteV1::AssessmentCurrentResult,
            InteractionSetupExpectationV1::AuthoritativeReplay,
        );
        context.execution.binary_path = "target/debug/pointbreak".to_owned();
        assert!(context.validate().is_err());

        let mut context = interaction_context(
            InteractionRouteV1::AssessmentCurrentResult,
            InteractionSetupExpectationV1::AuthoritativeReplay,
        );
        context.execution.binary_sha256 = "not-a-hash".to_owned();
        assert!(context.validate().is_err());

        let mut context = interaction_context(
            InteractionRouteV1::AssessmentCurrentResult,
            InteractionSetupExpectationV1::AuthoritativeReplay,
        );
        context.execution.features.clear();
        assert!(context.validate().is_err());

        let mut context = interaction_context(
            InteractionRouteV1::AssessmentCurrentResult,
            InteractionSetupExpectationV1::AuthoritativeReplay,
        );
        context.arguments.clear();
        assert!(context.validate().is_err());

        let mut context = interaction_context(
            InteractionRouteV1::AssessmentCurrentResult,
            InteractionSetupExpectationV1::AuthoritativeReplay,
        );
        context.track = None;
        assert!(context.validate().is_err());

        let mut version = interaction_context(
            InteractionRouteV1::VersionJson,
            InteractionSetupExpectationV1::NotApplicable,
        );
        version.fixture_identity_sha256 = Some(digest("forbidden-store-fixture"));
        assert!(version.validate().is_err());
    }

    #[test]
    fn interaction_lock_contract_rejects_impossible_acquisitions() {
        let mut fact = InteractionLockFactV1 {
            ordinal: 0,
            actor: InteractionActorV1::RequestReader,
            kind: InteractionLockKindV1::Authority,
            mode: InteractionLockModeV1::Blocking,
            outcome: InteractionLockOutcomeV1::Acquired,
            acquisition: InteractionLockAcquisitionV1::Reentrant,
            wait_nanos: 0,
            hold_nanos: None,
        };
        fact.validate().expect("authority reentry is valid");

        fact.kind = InteractionLockKindV1::Derived;
        assert!(fact.validate().is_err());
        fact.kind = InteractionLockKindV1::Authority;
        fact.wait_nanos = 1;
        assert!(fact.validate().is_err());

        fact.outcome = InteractionLockOutcomeV1::Busy;
        fact.acquisition = InteractionLockAcquisitionV1::NotAcquired;
        fact.wait_nanos = 3;
        fact.validate().expect("busy is not acquired");
        fact.hold_nanos = Some(5);
        assert!(fact.validate().is_err());
    }

    fn selectors(
        event_count: u64,
        carrier_hit_event_id: &str,
        carrier_hit_key: &str,
        object_content_hash: &str,
    ) -> LongitudinalCapacitySelectorsV1 {
        let mut selectors = LongitudinalCapacitySelectorsV1 {
            carrier_hit_key: carrier_hit_key.to_owned(),
            carrier_hit_event_id: carrier_hit_event_id.to_owned(),
            carrier_miss_key: "longitudinal:missing:test".to_owned(),
            semantic_revision_id: format!("rev:sha256:{}", digest("revision")),
            semantic_object_id: format!("obj:sha256:{}", digest("object")),
            semantic_missing_revision_id: format!("rev:sha256:{}", digest("missing-revision")),
            semantic_missing_object_id: format!("obj:sha256:{}", digest("missing-object")),
            object_detail_object_id: format!("obj:sha256:{}", digest("object")),
            object_detail_content_hash: format!("sha256:{object_content_hash}"),
            chronological_window_size: 100,
            chronological_head_start: 0,
            chronological_middle_start: event_count.saturating_sub(100) / 2,
            chronological_tail_start: event_count.saturating_sub(100),
            selectors_sha256: String::new(),
        };
        selectors.selectors_sha256 = selectors.canonical_sha256().expect("selector hash");
        selectors
    }

    fn materialization(root_label: &str) -> LongitudinalMaterializationReceiptV1 {
        materialization_for_tier(root_label, LongitudinalTierV1::L1)
    }

    fn materialization_for_tier(
        root_label: &str,
        selected_tier: LongitudinalTierV1,
    ) -> LongitudinalMaterializationReceiptV1 {
        let contract = longitudinal_runner_contract_v1();
        let tier = contract
            .tiers
            .iter()
            .find(|tier| tier.tier == selected_tier)
            .expect("tier contract");
        let ordered_events = (0..tier.event_count)
            .map(|ordinal| LongitudinalEventIdentityV1 {
                event_id: format!("evt:{ordinal:04}"),
                canonical_decoded_sha256: digest(&format!("event-decoded-{ordinal}")),
            })
            .collect::<Vec<_>>();
        let event_carriers = ordered_events
            .iter()
            .enumerate()
            .map(|(ordinal, event)| LongitudinalEventCarrierV1 {
                event_id: event.event_id.clone(),
                logical_key_sha256: digest(&format!("event-key-{ordinal}")),
                raw_sha256: digest(&format!("event-raw-{ordinal}")),
                raw_bytes: 128,
            })
            .collect::<Vec<_>>();
        let object_content_hash = digest("object-content");
        let mut content_inventory = (0..tier.present_content_count)
            .map(|ordinal| LongitudinalInventoryEntryV1 {
                logical_key: format!("content:{ordinal:04}"),
                kind: match ordinal % 3 {
                    0 => LongitudinalContentKindV1::ExternalBody,
                    1 => LongitudinalContentKindV1::ObjectArtifact,
                    _ => LongitudinalContentKindV1::ValidationLog,
                },
                raw_sha256: digest(&format!("content-raw-{ordinal}")),
                decoded_sha256: digest(&format!("content-decoded-{ordinal}")),
                raw_bytes: 256,
                decoded_bytes: 512,
            })
            .collect::<Vec<_>>();
        content_inventory[0].kind = LongitudinalContentKindV1::ObjectArtifact;
        content_inventory[0].logical_key = format!("artifacts/objects/{object_content_hash}.json");
        let schedule = LongitudinalOperationV1::ALL.to_vec();
        let strict = LongitudinalStrictSemanticReceiptV1 {
            event_set_sha256: digest("event-set"),
            ordered_journal_sha256: digest("ordered-journal"),
            state_sha256: digest("state"),
            projection_sha256: digest("projection"),
            content_inventory_sha256: digest("content-inventory"),
        };
        let expected_semantic_receipts = LongitudinalOperationV1::ALL
            .into_iter()
            .map(|operation| LongitudinalExpectedSemanticReceiptV1 {
                operation,
                semantic_receipt_sha256: canonical_sha256(&(operation, &strict, &ordered_events))
                    .expect("semantic receipt hash"),
            })
            .collect();
        let mut manifest = LongitudinalWorkloadManifestV1 {
            schema: contract.schema.clone(),
            protocol: contract.protocol.clone(),
            contract_sha256: contract.contract_sha256.clone(),
            execution: execution(),
            public_seed_hex: contract.public_seed_hex.clone(),
            tier: tier.tier,
            event_count: tier.event_count,
            revision_count: tier.revision_count,
            by_type: scaled_event_families(&contract.event_families, tier.block_count),
            ordered_events,
            event_carriers,
            content_inventory,
            removed_content_sha256: (0..tier.removed_content_count)
                .map(|ordinal| digest(&format!("removed-{ordinal}")))
                .collect(),
            capacity_selectors: selectors(
                tier.event_count,
                "evt:0000",
                "event-key-0",
                &object_content_hash,
            ),
            expected_semantic_receipts,
            schedule_sha256: canonical_sha256(&schedule).expect("schedule hash"),
            schedule,
            manifest_sha256: String::new(),
        };
        manifest.manifest_sha256 = manifest.canonical_sha256().expect("manifest hash");
        let mut receipt = LongitudinalMaterializationReceiptV1 {
            schema: LONGITUDINAL_MATERIALIZATION_RECEIPT_SCHEMA_V1.to_owned(),
            root_identity: digest(root_label),
            manifest,
            strict,
            materialization_sha256: String::new(),
        };
        receipt.materialization_sha256 = receipt.canonical_sha256().expect("materialization hash");
        receipt
    }

    fn rebind(receipt: &mut LongitudinalMaterializationReceiptV1) {
        receipt.manifest.manifest_sha256 =
            receipt.manifest.canonical_sha256().expect("manifest hash");
        receipt.materialization_sha256 = receipt.canonical_sha256().expect("materialization hash");
    }

    fn store_inventory(label: &str) -> LongitudinalStoreDataInventoryV1 {
        LongitudinalStoreDataInventoryV1 {
            file_count: 1,
            byte_count: 1,
            inventory_sha256: digest(label),
        }
    }

    fn resume_receipt() -> LongitudinalMaterializationResumeReceiptV1 {
        let materialization = materialization("resume-root");
        let mut receipt = LongitudinalMaterializationResumeReceiptV1 {
            schema: LONGITUDINAL_MATERIALIZATION_RESUME_RECEIPT_SCHEMA_V1.to_owned(),
            subject: LongitudinalMaterializationSubjectV1::Workload(LongitudinalTierV1::L1),
            execution: materialization.manifest.execution.clone(),
            root_identity: materialization.root_identity.clone(),
            pre_inventory: store_inventory("pre-inventory"),
            post_inventory: store_inventory("post-inventory"),
            events_created: 0,
            events_existing: materialization.manifest.event_count,
            event_count: materialization.manifest.event_count,
            content_count: materialization.manifest.content_inventory.len() as u64,
            strict: materialization.strict.clone(),
            materialization_sha256: materialization.materialization_sha256.clone(),
            materialization: LongitudinalResumedMaterializationV1::Workload(materialization),
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("resume receipt hash");
        receipt
    }

    fn equivalence_receipt() -> LongitudinalMaterializerEquivalenceReceiptV1 {
        let materialization = materialization("equivalence-source");
        let inventory = store_inventory("equivalent-inventory");
        let baseline = LongitudinalMaterializerRootReceiptV1 {
            subject: LongitudinalMaterializationSubjectV1::Workload(LongitudinalTierV1::L1),
            execution: materialization.manifest.execution.clone(),
            root_identity: digest("equivalence-baseline"),
            inventory: inventory.clone(),
            event_count: materialization.manifest.event_count,
            content_count: materialization.manifest.content_inventory.len() as u64,
            strict: materialization.strict.clone(),
            materialization_sha256: materialization.materialization_sha256.clone(),
        };
        let mut successor = baseline.clone();
        successor.root_identity = digest("equivalence-successor");
        successor.execution.source_commit = "5".repeat(40);
        let mut receipt = LongitudinalMaterializerEquivalenceReceiptV1 {
            schema: LONGITUDINAL_MATERIALIZER_EQUIVALENCE_RECEIPT_SCHEMA_V1.to_owned(),
            baseline,
            successor,
            implementation_diff_sha256: digest("implementation-diff"),
            equivalent: true,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt
            .canonical_sha256()
            .expect("equivalence receipt hash");
        receipt
    }

    fn carry_receipt(
        slot: LongitudinalCarryForwardSlotV1,
        ordinal: usize,
    ) -> LongitudinalCarryForwardReceiptV1 {
        let requirement = longitudinal_runner_contract_v1()
            .tiers
            .into_iter()
            .find(|requirement| requirement.tier == slot.tier)
            .expect("tier requirement");
        let inventory = store_inventory("equivalent-inventory");
        let strict = materialization("carry-strict").strict;
        let mut source_execution = execution();
        source_execution.build_profile = carry_forward_lane_profile_v1(slot.lane)
            .expect("supported carry lane")
            .to_owned();
        source_execution.runner_sha256 = match slot.lane {
            LongitudinalLaneV1::ReleaseUninstrumented => digest("source release runner"),
            LongitudinalLaneV1::DebugUninstrumented => digest("source debug runner"),
            LongitudinalLaneV1::AttributionCounts => unreachable!("unsupported carry lane"),
        };
        let mut equivalence_baseline_execution = execution();
        equivalence_baseline_execution.build_profile = "release-uninstrumented".to_owned();
        equivalence_baseline_execution.runner_sha256 = digest("equivalence baseline runner");
        let mut optimized_execution = equivalence_baseline_execution.clone();
        optimized_execution.source_commit = "5".repeat(40);
        optimized_execution.source_tree = "6".repeat(40);
        optimized_execution.runner_sha256 = digest("equivalence successor runner");
        let mut corrected_execution = optimized_execution.clone();
        corrected_execution.source_commit = "7".repeat(40);
        corrected_execution.source_tree = "8".repeat(40);
        corrected_execution.runner_sha256 = digest("corrected runner");
        let mut equivalence = LongitudinalMaterializerEquivalenceReceiptV1 {
            schema: LONGITUDINAL_MATERIALIZER_EQUIVALENCE_RECEIPT_SCHEMA_V1.to_owned(),
            baseline: LongitudinalMaterializerRootReceiptV1 {
                subject: LongitudinalMaterializationSubjectV1::Workload(slot.tier),
                execution: equivalence_baseline_execution,
                root_identity: digest(&format!("equivalence-baseline-{:?}", slot.tier)),
                inventory: inventory.clone(),
                event_count: requirement.event_count,
                content_count: requirement.present_content_count,
                strict: strict.clone(),
                materialization_sha256: digest(&format!(
                    "baseline-materialization-{:?}",
                    slot.tier
                )),
            },
            successor: LongitudinalMaterializerRootReceiptV1 {
                subject: LongitudinalMaterializationSubjectV1::Workload(slot.tier),
                execution: optimized_execution,
                root_identity: digest(&format!("equivalence-successor-{:?}", slot.tier)),
                inventory: inventory.clone(),
                event_count: requirement.event_count,
                content_count: requirement.present_content_count,
                strict: strict.clone(),
                materialization_sha256: digest(&format!(
                    "successor-materialization-{:?}",
                    slot.tier
                )),
            },
            implementation_diff_sha256: digest("implementation-diff"),
            equivalent: true,
            receipt_sha256: String::new(),
        };
        equivalence.receipt_sha256 = equivalence
            .canonical_sha256()
            .expect("equivalence receipt hash");
        let mut receipt = LongitudinalCarryForwardReceiptV1 {
            schema: LONGITUDINAL_CARRY_FORWARD_RECEIPT_SCHEMA_V1.to_owned(),
            slot,
            contract_sha256: LONGITUDINAL_RUNNER_CONTRACT_SHA256_V1.to_owned(),
            schedule_sha256: canonical_sha256(&LongitudinalOperationV1::ALL)
                .expect("schedule hash"),
            source_execution,
            corrected_execution,
            source_root_identity: digest(&format!("source-root-{ordinal}")),
            clone_root_identity: digest(&format!("clone-root-{ordinal}")),
            source_pre_inventory: inventory.clone(),
            source_post_inventory: inventory.clone(),
            clone_pre_inventory: inventory.clone(),
            clone_post_inventory: inventory,
            source_strict: strict.clone(),
            clone_strict: strict,
            event_count: requirement.event_count,
            content_count: requirement.present_content_count,
            source_manifest_sha256: digest(&format!("source-manifest-{ordinal}")),
            carried_manifest_sha256: digest(&format!("carried-manifest-{ordinal}")),
            source_manifest_invariant_sha256: digest("manifest-invariant"),
            carried_manifest_invariant_sha256: digest("manifest-invariant"),
            source_materialization_sha256: digest(&format!("source-materialization-{ordinal}")),
            carried_materialization_sha256: digest(&format!("carried-materialization-{ordinal}")),
            materializer_equivalence: equivalence,
            final_authority_diff_sha256: digest("implementation-diff"),
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("carry receipt hash");
        receipt
    }

    fn package_verification_receipt(
        execution: &LongitudinalExecutionIdentityV1,
    ) -> LongitudinalPackageVerificationReceiptV1 {
        let mut receipt = LongitudinalPackageVerificationReceiptV1 {
            schema: LONGITUDINAL_PACKAGE_VERIFICATION_RECEIPT_SCHEMA_V1.to_owned(),
            package_kind: LongitudinalVerifiedPackageKindV1::Workload,
            package_sha256: digest("workload package"),
            contract_sha256: LONGITUDINAL_RUNNER_CONTRACT_SHA256_V1.to_owned(),
            execution_identity_sha256: execution
                .canonical_sha256()
                .expect("execution identity hash"),
            raw_inventory_sha256: digest("raw inventory"),
            verified: true,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt
            .canonical_sha256()
            .expect("verification receipt hash");
        receipt
    }

    fn carry_authority_package() -> LongitudinalCarryForwardAuthorityPackageV1 {
        let carry_receipts = LongitudinalTierV1::ALL
            .into_iter()
            .flat_map(|tier| {
                [
                    LongitudinalCarryForwardSlotV1 {
                        tier,
                        lane: LongitudinalLaneV1::ReleaseUninstrumented,
                        independent_run: 1,
                    },
                    LongitudinalCarryForwardSlotV1 {
                        tier,
                        lane: LongitudinalLaneV1::ReleaseUninstrumented,
                        independent_run: 2,
                    },
                    LongitudinalCarryForwardSlotV1 {
                        tier,
                        lane: LongitudinalLaneV1::DebugUninstrumented,
                        independent_run: 1,
                    },
                ]
            })
            .enumerate()
            .map(|(ordinal, slot)| carry_receipt(slot, ordinal))
            .collect::<Vec<_>>();
        let corrected_execution = carry_receipts[0].corrected_execution.clone();
        let verification_receipt = package_verification_receipt(&corrected_execution);
        let mut package = LongitudinalCarryForwardAuthorityPackageV1 {
            schema: LONGITUDINAL_CARRY_FORWARD_AUTHORITY_PACKAGE_SCHEMA_V1.to_owned(),
            contract_sha256: LONGITUDINAL_RUNNER_CONTRACT_SHA256_V1.to_owned(),
            corrected_execution,
            carry_receipts,
            authority_set_sha256: String::new(),
            completion: Some(LongitudinalCarryForwardAuthorityCompletionV1 {
                final_workload_package_sha256: verification_receipt.package_sha256.clone(),
                verification_receipt,
                package_verified: true,
            }),
            package_sha256: String::new(),
        };
        package.authority_set_sha256 = package
            .canonical_authority_set_sha256()
            .expect("authority set hash");
        package.package_sha256 = package.canonical_sha256().expect("authority package hash");
        package
    }

    fn rehash_carry_authority_package(package: &mut LongitudinalCarryForwardAuthorityPackageV1) {
        package.authority_set_sha256 = package
            .canonical_authority_set_sha256()
            .expect("authority set hash");
        package.package_sha256 = package.canonical_sha256().expect("authority package hash");
    }

    fn removal_upgrade_receipt(
        slot: LongitudinalCarryForwardSlotV1,
        ordinal: usize,
        corrected_execution: &LongitudinalExecutionIdentityV1,
    ) -> LongitudinalRemovalUpgradeReceiptV1 {
        let requirement = longitudinal_runner_contract_v1()
            .tiers
            .into_iter()
            .find(|requirement| requirement.tier == slot.tier)
            .expect("tier requirement");
        let mut source_execution = execution();
        source_execution.build_profile = carry_forward_lane_profile_v1(slot.lane)
            .expect("upgrade source lane")
            .to_owned();
        source_execution.runner_sha256 = match slot.lane {
            LongitudinalLaneV1::ReleaseUninstrumented => digest("upgrade source release runner"),
            LongitudinalLaneV1::DebugUninstrumented => digest("upgrade source debug runner"),
            LongitudinalLaneV1::AttributionCounts => unreachable!("unsupported upgrade lane"),
        };
        let strict = materialization("upgrade-strict").strict;
        let legacy_inventory = store_inventory(&format!("upgrade-legacy-inventory-{ordinal}"));
        let corrected_inventory =
            store_inventory(&format!("upgrade-corrected-inventory-{ordinal}"));
        let mut changed_paths = (0..requirement.removed_content_count)
            .map(|change| LongitudinalRemovalUpgradePathV1 {
                relative_path: format!(
                    ".pointbreak/data/events/{}.json",
                    digest(&format!("upgrade-path-{ordinal}-{change}"))
                ),
                event_id: format!(
                    "evt:sha256:{}",
                    digest(&format!("upgrade-event-{ordinal}-{change}"))
                ),
                event_record_hash: format!(
                    "sha256:{}",
                    digest(&format!("upgrade-record-{ordinal}-{change}"))
                ),
                payload_hash: format!(
                    "sha256:{}",
                    digest(&format!("upgrade-payload-{ordinal}-{change}"))
                ),
                before_sha256: digest(&format!("upgrade-before-{ordinal}-{change}")),
                before_bytes: 100,
                after_sha256: digest(&format!("upgrade-after-{ordinal}-{change}")),
                after_bytes: 200,
            })
            .collect::<Vec<_>>();
        changed_paths.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let mut receipt = LongitudinalRemovalUpgradeReceiptV1 {
            schema: LONGITUDINAL_REMOVAL_UPGRADE_RECEIPT_SCHEMA_V1.to_owned(),
            slot,
            contract_sha256: LONGITUDINAL_RUNNER_CONTRACT_SHA256_V1.to_owned(),
            schedule_sha256: canonical_sha256(&LongitudinalOperationV1::ALL)
                .expect("upgrade schedule hash"),
            source_execution,
            corrected_execution: corrected_execution.clone(),
            source_root_identity: digest(&format!("upgrade-source-root-{ordinal}")),
            successor_root_identity: digest(&format!("upgrade-successor-root-{ordinal}")),
            source_pre_inventory: legacy_inventory.clone(),
            source_post_inventory: legacy_inventory.clone(),
            successor_pre_inventory: legacy_inventory,
            successor_post_inventory: corrected_inventory,
            source_strict: strict.clone(),
            successor_strict: strict,
            event_count: requirement.event_count,
            content_count: requirement.present_content_count,
            removed_content_count: requirement.removed_content_count,
            changed_paths,
            enrollment_relative_path: ".pointbreak/allowed-signers.json".to_owned(),
            enrollment_sha256: digest("upgrade enrollment"),
            enrollment_bytes: 1_024,
            source_manifest_sha256: digest(&format!("upgrade-source-manifest-{ordinal}")),
            corrected_manifest_sha256: digest(&format!("upgrade-corrected-manifest-{ordinal}")),
            source_manifest_invariant_sha256: digest(&format!("upgrade-invariant-{ordinal}")),
            corrected_manifest_invariant_sha256: digest(&format!("upgrade-invariant-{ordinal}")),
            source_materialization_sha256: digest(&format!(
                "upgrade-source-materialization-{ordinal}"
            )),
            corrected_materialization_sha256: digest(&format!(
                "upgrade-corrected-materialization-{ordinal}"
            )),
            resume_receipt_sha256: digest(&format!("upgrade-resume-{ordinal}")),
            resume_events_created: 0,
            resume_events_existing: requirement.event_count,
            final_authority_diff_sha256: digest("upgrade implementation diff"),
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("upgrade receipt hash");
        receipt
    }

    fn removal_upgrade_authority_package() -> LongitudinalRemovalUpgradeAuthorityPackageV1 {
        let mut corrected_execution = execution();
        corrected_execution.source_commit = "7".repeat(40);
        corrected_execution.source_tree = "8".repeat(40);
        corrected_execution.runner_sha256 = digest("upgrade corrected runner");
        corrected_execution.build_profile = "corrected-uninstrumented".to_owned();
        let upgrade_receipts = LongitudinalTierV1::ALL
            .into_iter()
            .flat_map(|tier| {
                [
                    LongitudinalCarryForwardSlotV1 {
                        tier,
                        lane: LongitudinalLaneV1::ReleaseUninstrumented,
                        independent_run: 1,
                    },
                    LongitudinalCarryForwardSlotV1 {
                        tier,
                        lane: LongitudinalLaneV1::ReleaseUninstrumented,
                        independent_run: 2,
                    },
                    LongitudinalCarryForwardSlotV1 {
                        tier,
                        lane: LongitudinalLaneV1::DebugUninstrumented,
                        independent_run: 1,
                    },
                ]
            })
            .enumerate()
            .map(|(ordinal, slot)| removal_upgrade_receipt(slot, ordinal, &corrected_execution))
            .collect::<Vec<_>>();
        let materializer_equivalences = LongitudinalTierV1::ALL
            .into_iter()
            .map(|tier| {
                let release_one = upgrade_receipts
                    .iter()
                    .find(|receipt| {
                        receipt.slot.tier == tier
                            && receipt.slot.lane == LongitudinalLaneV1::ReleaseUninstrumented
                            && receipt.slot.independent_run == 1
                    })
                    .expect("release-one upgrade");
                let baseline = LongitudinalMaterializerRootReceiptV1 {
                    subject: LongitudinalMaterializationSubjectV1::Workload(tier),
                    execution: corrected_execution.clone(),
                    root_identity: release_one.successor_root_identity.clone(),
                    inventory: release_one.successor_post_inventory.clone(),
                    event_count: release_one.event_count,
                    content_count: release_one.content_count,
                    strict: release_one.successor_strict.clone(),
                    materialization_sha256: release_one.corrected_materialization_sha256.clone(),
                };
                let mut successor = baseline.clone();
                successor.root_identity = digest(&format!("upgrade-fresh-root-{tier:?}"));
                let mut equivalence = LongitudinalMaterializerEquivalenceReceiptV1 {
                    schema: LONGITUDINAL_MATERIALIZER_EQUIVALENCE_RECEIPT_SCHEMA_V1.to_owned(),
                    baseline,
                    successor,
                    implementation_diff_sha256: digest("upgrade implementation diff"),
                    equivalent: true,
                    receipt_sha256: String::new(),
                };
                equivalence.receipt_sha256 = equivalence
                    .canonical_sha256()
                    .expect("upgrade equivalence hash");
                equivalence
            })
            .collect::<Vec<_>>();
        let verification_receipt = package_verification_receipt(&corrected_execution);
        let mut package = LongitudinalRemovalUpgradeAuthorityPackageV1 {
            schema: LONGITUDINAL_REMOVAL_UPGRADE_AUTHORITY_PACKAGE_SCHEMA_V1.to_owned(),
            contract_sha256: LONGITUDINAL_RUNNER_CONTRACT_SHA256_V1.to_owned(),
            corrected_execution,
            upgrade_receipts,
            materializer_equivalences,
            authority_set_sha256: String::new(),
            completion: Some(LongitudinalRemovalUpgradeAuthorityCompletionV1 {
                final_workload_package_sha256: verification_receipt.package_sha256.clone(),
                verification_receipt,
                package_verified: true,
            }),
            package_sha256: String::new(),
        };
        rehash_removal_upgrade_authority_package(&mut package);
        package
    }

    fn rehash_removal_upgrade_authority_package(
        package: &mut LongitudinalRemovalUpgradeAuthorityPackageV1,
    ) {
        package.authority_set_sha256 = package
            .canonical_authority_set_sha256()
            .expect("upgrade authority set hash");
        package.package_sha256 = package
            .canonical_sha256()
            .expect("upgrade authority package hash");
    }

    fn generation(
        subject: LongitudinalCapacitySubjectV1,
        scalar: u64,
    ) -> LongitudinalGenerationResourceReceiptV1 {
        let mut receipt = LongitudinalGenerationResourceReceiptV1 {
            subject,
            wall_nanos: scalar,
            process_cpu_nanos: scalar,
            encoded_bytes: scalar,
            allocated_bytes: scalar,
            high_water_bytes: scalar,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("generation hash");
        receipt
    }

    fn capacity_manifest_header() -> LongitudinalCapacityManifestV1 {
        let contract = longitudinal_capacity_contract_v1();
        let profile = contract
            .profiles
            .iter()
            .find(|profile| profile.profile == LongitudinalCapacityProfileV1::C262)
            .expect("C262 contract");
        let probe_schedule = LongitudinalCapacityProbeV1::ALL.to_vec();
        LongitudinalCapacityManifestV1 {
            schema: contract.schema,
            contract_sha256: contract.contract_sha256,
            execution: execution(),
            public_seed_hex: contract.public_seed_hex,
            subject: LongitudinalCapacitySubjectV1::Companion(LongitudinalCapacityProfileV1::C262),
            event_count: profile.event_count,
            revision_count: profile.revision_count,
            object_artifact_count: profile.object_artifact_count,
            task_attempt_count: profile.task_attempt_count,
            body_fact_count: profile.body_fact_count,
            external_body_count: profile.external_body_count,
            decoded_body_bytes: profile.decoded_body_bytes,
            decoded_object_target_bytes: profile.decoded_object_target_bytes,
            ordered_events: Vec::new(),
            event_carriers: Vec::new(),
            content_inventory: Vec::new(),
            removed_content_sha256: Vec::new(),
            selectors: selectors(
                profile.event_count,
                "evt:0000",
                "event-key-0",
                &digest("object-content"),
            ),
            schedule_sha256: canonical_sha256(&probe_schedule).expect("probe schedule hash"),
            probe_schedule,
            manifest_sha256: String::new(),
        }
    }

    fn operation_receipt(
        lane: LongitudinalLaneV1,
        execution_identity_sha256: String,
        sample_ordinal: u32,
    ) -> LongitudinalOperationReceiptV1 {
        let mut receipt = LongitudinalOperationReceiptV1 {
            schema: LONGITUDINAL_OPERATION_RECEIPT_SCHEMA_V1.to_owned(),
            run_identity: digest("operation-run"),
            root_identity: digest("operation-root"),
            tier: LongitudinalTierV1::L1,
            manifest_sha256: digest("operation-manifest"),
            schedule_sha256: digest("operation-schedule"),
            execution_identity_sha256,
            lane,
            operation: LongitudinalOperationV1::WarmHead,
            outcome: LongitudinalOperationOutcomeV1::Measured,
            sample_ordinal,
            pre_event_count: 1_024,
            post_event_count: 1_024,
            pre_event_set_sha256: digest("pre-set"),
            post_event_set_sha256: digest("post-set"),
            response_bytes: 1,
            response_canonical_sha256: digest("response"),
            semantic_facts: LongitudinalSemanticFactsV1 {
                ordered_ids: vec!["evt:0000".to_owned()],
                fact_ids: Vec::new(),
                diagnostics: Vec::new(),
                event_count: 1_024,
                event_set_sha256: digest("post-set"),
                content_state_sha256: digest("semantic-content-state"),
            },
            semantic_receipt_sha256: digest("operation-semantic"),
            metrics: Some(LongitudinalOperationMetricsV1 {
                wall_nanos: 1,
                process_user_cpu_nanos: 1,
                process_system_cpu_nanos: 1,
                baseline_rss_bytes: 1,
                peak_rss_bytes: 1,
                steady_rss_bytes: 1,
            }),
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("operation hash");
        receipt
    }

    fn empty_package() -> LongitudinalEvidencePackageV1 {
        let contract = longitudinal_runner_contract_v1();
        let mut package = LongitudinalEvidencePackageV1 {
            schema: LONGITUDINAL_EVIDENCE_PACKAGE_SCHEMA_V1.to_owned(),
            purpose: LongitudinalPackagePurposeV1::NonTimingSmoke,
            contract_sha256: contract.contract_sha256,
            base_execution: execution(),
            derivative_execution: None,
            materializations: Vec::new(),
            operations: Vec::new(),
            memory_receipts: Vec::new(),
            interruption_receipts: Vec::new(),
            counters: Vec::new(),
            raw_inventory: vec![LongitudinalRawFileV1 {
                relative_path: "raw/sample.json".to_owned(),
                sha256: digest("sample"),
                bytes: 1,
            }],
            failures: Vec::new(),
            compensation_applied: false,
            package_sha256: String::new(),
        };
        package.package_sha256 = package.canonical_sha256().expect("package hash");
        package
    }

    #[test]
    fn longitudinal_contract_freezes_workload_matrix_samples_and_envelopes() {
        let contract = longitudinal_runner_contract_v1();
        contract.validate().expect("frozen contract validates");

        assert_eq!(contract.block_event_count, 256);
        assert_eq!(contract.history_page_size, 100);
        assert_eq!(contract.append_batch_size, 30);
        assert_eq!(contract.operation_schedule, LongitudinalOperationV1::ALL);
        assert_eq!(contract.samples.process_cold_per_release_root, 10);
        assert_eq!(contract.samples.excluded_warmups, 3);
        assert_eq!(contract.samples.warm_samples_per_release_root, 30);
        assert_eq!(contract.samples.append_one_samples_per_release_root, 30);
        assert_eq!(contract.samples.append_burst_samples_per_release_root, 10);
        assert_eq!(contract.samples.maintenance_samples_per_release_root, 3);
        assert_eq!(contract.samples.nearest_rank_percentile, 95);
        assert!(contract.samples.retain_all_samples);
        assert!(!contract.samples.outlier_removal_allowed);
        assert_eq!(contract.run_matrix[0].roots_per_tier, 2);
        assert!(contract.run_matrix[1].records_timing);
        assert!(!contract.run_matrix[1].performance_comparison_admissible);
        assert!(contract.run_matrix[2].counters_admissible);
        assert!(!contract.run_matrix[2].records_timing);
        assert_eq!(contract.interaction_envelopes.len(), 12);
        assert_eq!(contract.counter_envelopes.len(), 6);
        assert_eq!(contract.memory_envelope.checkpoints.len(), 7);
    }

    #[test]
    fn longitudinal_capacity_manifest_binds_frozen_counts_and_content_cardinality() {
        let mut manifest = capacity_manifest_header();
        manifest.body_fact_count -= 1;
        assert_eq!(
            manifest.validate(),
            Err(LongitudinalContractError::CountMismatch {
                field: "capacity body-fact count",
                expected: 184_320,
                actual: 184_319,
            })
        );

        manifest = capacity_manifest_header();
        manifest.decoded_body_bytes -= 1;
        assert_eq!(
            manifest.validate(),
            Err(LongitudinalContractError::CountMismatch {
                field: "capacity decoded body bytes",
                expected: 1_509_949_440,
                actual: 1_509_949_439,
            })
        );

        manifest = capacity_manifest_header();
        assert_eq!(
            manifest.validate(),
            Err(LongitudinalContractError::CountMismatch {
                field: "capacity content inventory",
                expected: 106_496,
                actual: 0,
            })
        );
    }

    #[test]
    fn longitudinal_counters_have_exact_stable_fields_and_no_metric_slots() {
        let value = serde_json::to_value(LongitudinalCountersV1::default()).expect("counter JSON");
        assert_eq!(
            value
                .as_object()
                .expect("counter object")
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec![
                "bodyArtifactReads",
                "bodyBytesRead",
                "carrierBytesRead",
                "carrierOpens",
                "chronologicalSortItems",
                "directoryEntriesWalked",
                "eventDecodes",
                "eventFolds",
                "eventValidations",
                "objectArtifactReads",
                "objectBytesRead",
                "projectionRebuilds",
                "responseBytes",
                "stateRebuilds",
            ]
        );
        assert_eq!(
            serde_json::from_value::<LongitudinalCountersV1>(value.clone())
                .expect("legacy zero-valued counter JSON"),
            LongitudinalCountersV1::default()
        );

        let nonzero = LongitudinalCountersV1 {
            authoritative_fallbacks: 1,
            full_history_fallbacks: 2,
            change_candidates: 3,
            change_candidate_current_revisions: 4,
            change_capability_carriers_opened: 5,
            change_proposal_carriers_opened: 6,
            change_proposal_carriers_validated: 7,
            change_support_carriers_opened: 8,
            change_matches: 9,
            change_rows_emitted: 10,
            timeline_sqlite_candidates: 11,
            timeline_sqlite_window_rows: 12,
            timeline_sqlite_facet_rows: 13,
            timeline_selected_carriers: 14,
            timeline_revision_candidate_carriers: 21,
            timeline_removal_support_carriers: 15,
            timeline_signature_support_carriers: 16,
            timeline_correlation_support_carriers: 17,
            timeline_trust_support_carriers: 18,
            timeline_exhaustive_candidates: 19,
            timeline_entries_emitted: 20,
            ..LongitudinalCountersV1::default()
        };
        let nonzero = serde_json::to_value(nonzero).expect("nonzero fallback counter JSON");
        assert_eq!(nonzero["authoritativeFallbacks"], 1);
        assert_eq!(nonzero["fullHistoryFallbacks"], 2);
        assert_eq!(nonzero["changeCandidates"], 3);
        assert_eq!(nonzero["changeCandidateCurrentRevisions"], 4);
        assert_eq!(nonzero["changeCapabilityCarriersOpened"], 5);
        assert_eq!(nonzero["changeProposalCarriersOpened"], 6);
        assert_eq!(nonzero["changeProposalCarriersValidated"], 7);
        assert_eq!(nonzero["changeSupportCarriersOpened"], 8);
        assert_eq!(nonzero["changeMatches"], 9);
        assert_eq!(nonzero["changeRowsEmitted"], 10);
        assert_eq!(nonzero["timelineSqliteCandidates"], 11);
        assert_eq!(nonzero["timelineSqliteWindowRows"], 12);
        assert_eq!(nonzero["timelineSqliteFacetRows"], 13);
        assert_eq!(nonzero["timelineSelectedCarriers"], 14);
        assert_eq!(nonzero["timelineRevisionCandidateCarriers"], 21);
        assert_eq!(nonzero["timelineRemovalSupportCarriers"], 15);
        assert_eq!(nonzero["timelineSignatureSupportCarriers"], 16);
        assert_eq!(nonzero["timelineCorrelationSupportCarriers"], 17);
        assert_eq!(nonzero["timelineTrustSupportCarriers"], 18);
        assert_eq!(nonzero["timelineExhaustiveCandidates"], 19);
        assert_eq!(nonzero["timelineEntriesEmitted"], 20);

        let mut receipt = LongitudinalCounterReceiptV1 {
            schema: LONGITUDINAL_COUNTER_RECEIPT_SCHEMA_V1.to_owned(),
            run_identity: digest("run"),
            root_identity: digest("root"),
            operation: "WARM_HEAD".to_owned(),
            phase: "warm".to_owned(),
            base_execution_identity_sha256: digest("base"),
            derivative_execution_identity_sha256: digest("derivative"),
            manifest_sha256: digest("manifest"),
            schedule_sha256: digest("schedule"),
            success: true,
            semantic_result_sha256: digest("semantic"),
            counters: LongitudinalCountersV1::default(),
            capacity_ownership: None,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("counter hash");
        receipt.validate().expect("counter receipt validates");
        let mut value = serde_json::to_value(receipt).expect("receipt JSON");
        value
            .as_object_mut()
            .expect("receipt object")
            .insert("wallNanos".to_owned(), 1_u64.into());
        assert!(serde_json::from_value::<LongitudinalCounterReceiptV1>(value).is_err());
    }

    #[test]
    fn longitudinal_operation_outcomes_cannot_fabricate_measurements() {
        let execution_hash = execution()
            .canonical_sha256()
            .expect("execution identity hash");
        let mut unavailable =
            operation_receipt(LongitudinalLaneV1::ReleaseUninstrumented, execution_hash, 0);
        unavailable.outcome = LongitudinalOperationOutcomeV1::UnavailableNoCurrentSurface;
        unavailable.metrics = None;
        unavailable.pre_event_set_sha256 = unavailable.post_event_set_sha256.clone();
        unavailable.semantic_facts.diagnostics = vec!["unsupported_no_current_surface".to_owned()];
        unavailable.receipt_sha256 = unavailable
            .canonical_sha256()
            .expect("unavailable receipt hash");
        unavailable
            .validate()
            .expect("an unavailable row carries no metrics");

        unavailable.metrics = Some(LongitudinalOperationMetricsV1 {
            wall_nanos: 1,
            process_user_cpu_nanos: 0,
            process_system_cpu_nanos: 0,
            baseline_rss_bytes: 0,
            peak_rss_bytes: 0,
            steady_rss_bytes: 0,
        });
        unavailable.receipt_sha256 = unavailable
            .canonical_sha256()
            .expect("invalid receipt hash");
        assert!(unavailable.validate().is_err());
    }

    #[test]
    fn longitudinal_canonical_hashes_ignore_only_their_own_slot() {
        let contract = longitudinal_runner_contract_v1();
        let canonical = contract.canonical_sha256().expect("contract hash");
        let mut own_hash_changed = contract.clone();
        own_hash_changed.contract_sha256 = "f".repeat(64);
        assert_eq!(
            own_hash_changed.canonical_sha256().expect("contract hash"),
            canonical
        );

        let mut governed_field_changed = contract;
        governed_field_changed.history_page_size += 1;
        assert_ne!(
            governed_field_changed
                .canonical_sha256()
                .expect("contract hash"),
            canonical
        );

        let capacity = longitudinal_capacity_contract_v1();
        let capacity_hash = capacity.canonical_sha256().expect("capacity hash");
        let mut capacity_changed = capacity;
        capacity_changed.contention.reader_cycles += 1;
        assert_ne!(
            capacity_changed.canonical_sha256().expect("capacity hash"),
            capacity_hash
        );
    }

    #[test]
    fn longitudinal_pair_verifier_accepts_distinct_identical_roots() {
        let left = materialization("root-left");
        let mut right = left.clone();
        right.root_identity = digest("root-right");
        right.materialization_sha256 = right.canonical_sha256().expect("right hash");

        verify_longitudinal_materialization_pair_v1(&left, &right)
            .expect("the two roots have identical canonical materialization");
    }

    #[test]
    fn longitudinal_pair_verifier_rejects_carrier_order_and_receipt_drift() {
        let left = materialization("root-left");

        let mut carrier_drift = left.clone();
        carrier_drift.root_identity = digest("root-carrier-drift");
        carrier_drift.manifest.event_carriers[0].raw_sha256 = digest("one-byte-drift");
        rebind(&mut carrier_drift);
        assert_eq!(
            verify_longitudinal_materialization_pair_v1(&left, &carrier_drift),
            Err(LongitudinalContractError::PairMismatch)
        );

        let mut order_drift = left.clone();
        order_drift.root_identity = digest("root-order-drift");
        order_drift.manifest.ordered_events.swap(0, 1);
        rebind(&mut order_drift);
        assert_eq!(
            verify_longitudinal_materialization_pair_v1(&left, &order_drift),
            Err(LongitudinalContractError::PairMismatch)
        );

        let mut receipt_drift = left.clone();
        receipt_drift.root_identity = digest("root-receipt-drift");
        receipt_drift.strict.state_sha256 = digest("state-drift");
        rebind(&mut receipt_drift);
        assert_eq!(
            verify_longitudinal_materialization_pair_v1(&left, &receipt_drift),
            Err(LongitudinalContractError::PairMismatch)
        );

        let mut content_drift = left.clone();
        content_drift.root_identity = digest("root-content-drift");
        content_drift.manifest.content_inventory[0].raw_sha256 = digest("content-byte-drift");
        rebind(&mut content_drift);
        assert_eq!(
            verify_longitudinal_materialization_pair_v1(&left, &content_drift),
            Err(LongitudinalContractError::PairMismatch)
        );

        let mut source_drift = left.clone();
        source_drift.root_identity = digest("root-source-drift");
        source_drift.manifest.execution.runner_sha256 = digest("other-runner");
        rebind(&mut source_drift);
        assert_eq!(
            verify_longitudinal_materialization_pair_v1(&left, &source_drift),
            Err(LongitudinalContractError::PairMismatch)
        );
    }

    #[test]
    fn longitudinal_resume_receipt_rejects_write_count_and_embedded_receipt_drift() {
        let receipt = resume_receipt();
        receipt.validate().expect("resume receipt validates");

        let mut count_drift = receipt.clone();
        count_drift.events_existing -= 1;
        count_drift.receipt_sha256 = count_drift
            .canonical_sha256()
            .expect("count-drift receipt hash");
        assert!(matches!(
            count_drift.validate(),
            Err(LongitudinalContractError::CountMismatch { .. })
        ));

        let mut materialization_drift = receipt;
        materialization_drift.materialization_sha256 = digest("wrong-materialization");
        materialization_drift.receipt_sha256 = materialization_drift
            .canonical_sha256()
            .expect("materialization-drift receipt hash");
        assert_eq!(
            materialization_drift.validate(),
            Err(LongitudinalContractError::PairMismatch)
        );
    }

    #[test]
    fn longitudinal_equivalence_receipt_rejects_inventory_and_subject_drift() {
        let receipt = equivalence_receipt();
        receipt
            .validate()
            .expect("materializer equivalence receipt validates");

        let mut inventory_drift = receipt.clone();
        inventory_drift.successor.inventory.byte_count += 1;
        inventory_drift.receipt_sha256 = inventory_drift
            .canonical_sha256()
            .expect("inventory-drift receipt hash");
        assert_eq!(
            inventory_drift.validate(),
            Err(LongitudinalContractError::PairMismatch)
        );

        let mut subject_drift = receipt;
        subject_drift.successor.subject =
            LongitudinalMaterializationSubjectV1::Capacity(LongitudinalCapacityProfileV1::L100O10K);
        subject_drift.receipt_sha256 = subject_drift
            .canonical_sha256()
            .expect("subject-drift receipt hash");
        assert!(subject_drift.validate().is_err());
    }

    #[test]
    fn longitudinal_manifest_rejects_missing_duplicate_and_schedule_rows() {
        let receipt = materialization("root");

        let mut missing = receipt.manifest.clone();
        missing.ordered_events.pop();
        missing.manifest_sha256 = missing.canonical_sha256().expect("missing hash");
        assert!(matches!(
            missing.validate(),
            Err(LongitudinalContractError::CountMismatch { .. })
        ));

        let mut duplicate = receipt.manifest.clone();
        duplicate.ordered_events[1].event_id = duplicate.ordered_events[0].event_id.clone();
        duplicate.manifest_sha256 = duplicate.canonical_sha256().expect("duplicate hash");
        assert_eq!(
            duplicate.validate(),
            Err(LongitudinalContractError::DuplicateRow {
                field: "ordered event ids"
            })
        );

        let mut schedule = receipt.manifest.clone();
        schedule.schedule.pop();
        schedule.schedule_sha256 = canonical_sha256(&schedule.schedule).expect("schedule hash");
        schedule.manifest_sha256 = schedule.canonical_sha256().expect("manifest hash");
        assert_eq!(
            schedule.validate(),
            Err(LongitudinalContractError::ContractDrift {
                field: "manifest schedule"
            })
        );
    }

    #[test]
    fn longitudinal_non_compensation_classifies_shape_before_wall_time() {
        let baseline = LongitudinalComplexityCountsV1 {
            carrier_opens: 100,
            ..LongitudinalComplexityCountsV1::default()
        };
        let at_limit = LongitudinalComplexityCountsV1 {
            carrier_opens: 125,
            ..LongitudinalComplexityCountsV1::default()
        };
        let above_limit = LongitudinalComplexityCountsV1 {
            carrier_opens: 126,
            ..LongitudinalComplexityCountsV1::default()
        };
        assert_eq!(
            classify_longitudinal_capacity_work_v1(&baseline, &at_limit, false, false, true),
            LongitudinalCapacityClassificationV1::Bounded
        );
        assert_eq!(
            classify_longitudinal_capacity_work_v1(&baseline, &above_limit, false, false, true),
            LongitudinalCapacityClassificationV1::HistoryOrCardinalityProportional
        );
        assert_eq!(
            classify_longitudinal_capacity_work_v1(&baseline, &baseline, false, true, true),
            LongitudinalCapacityClassificationV1::HistoryOrCardinalityProportional
        );
        assert_eq!(
            classify_longitudinal_capacity_work_v1(&baseline, &baseline, false, false, false),
            LongitudinalCapacityClassificationV1::UnsupportedNoCurrentSurface
        );
    }

    #[test]
    fn longitudinal_c524_gate_is_typed_and_has_no_manual_override() {
        let baseline = generation(LongitudinalCapacitySubjectV1::V1L100, 100);
        let c262 = generation(
            LongitudinalCapacitySubjectV1::Companion(LongitudinalCapacityProfileV1::C262),
            256,
        );
        let inputs = LongitudinalC524GateInputsV1 {
            v1_l100_generation: baseline,
            c262_generation: c262,
            c262_identity_complete: true,
            c262_base_complete: true,
            c262_counting_complete: true,
            c262_memory_complete: true,
            c262_package_complete: true,
            host_pressure_clear: true,
            free_bytes: 17 * GIB,
        };
        let (admitted, reasons) = evaluate_c524_gate(&inputs).expect("gate evaluation");
        assert!(admitted);
        assert!(reasons.is_empty());

        let mut receipt = LongitudinalC524GateReceiptV1 {
            schema: "pointbreak.longitudinal-c524-gate-receipt.v1".to_owned(),
            inputs_sha256: canonical_sha256(&inputs).expect("input hash"),
            admitted,
            reasons,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("gate hash");
        receipt
            .validate_against(&inputs)
            .expect("gate receipt is bound to its inputs");

        let mut pressure = inputs;
        pressure.host_pressure_clear = false;
        assert_eq!(
            evaluate_c524_gate(&pressure).expect("pressure gate").1,
            vec!["host_pressure_not_clear"]
        );
        assert!(
            !longitudinal_capacity_contract_v1()
                .c524_gate
                .manual_override_allowed
        );
    }

    #[test]
    fn longitudinal_package_rejects_missing_inventory_compensation_and_mutable_failure() {
        let mut package = empty_package();
        package.raw_inventory.clear();
        package.package_sha256 = package.canonical_sha256().expect("package hash");
        assert_eq!(
            package.validate(),
            Err(LongitudinalContractError::MissingRawInventory)
        );

        package = empty_package();
        package.compensation_applied = true;
        package.package_sha256 = package.canonical_sha256().expect("package hash");
        assert_eq!(
            package.validate(),
            Err(LongitudinalContractError::CompensationForbidden)
        );

        package.compensation_applied = false;
        package.failures.push(LongitudinalFailureReceiptV1 {
            reason_code: "valid_failure".to_owned(),
            detail_sha256: digest("failure"),
            immutable: false,
        });
        package.package_sha256 = package.canonical_sha256().expect("package hash");
        assert_eq!(
            package.validate(),
            Err(LongitudinalContractError::MutableFailure)
        );
    }

    #[test]
    fn longitudinal_package_rejects_mixed_revisions_duplicate_samples_and_lane_pooling() {
        let mut package = empty_package();
        let base_hash = package
            .base_execution
            .canonical_sha256()
            .expect("base identity hash");
        let sample = operation_receipt(LongitudinalLaneV1::ReleaseUninstrumented, base_hash, 0);
        package.operations = vec![sample.clone(), sample];
        package.package_sha256 = package.canonical_sha256().expect("package hash");
        assert_eq!(
            package.validate(),
            Err(LongitudinalContractError::DuplicateRow {
                field: "operation samples"
            })
        );

        let mut mixed = empty_package();
        mixed.operations.push(operation_receipt(
            LongitudinalLaneV1::ReleaseUninstrumented,
            digest("other-execution"),
            0,
        ));
        mixed.package_sha256 = mixed.canonical_sha256().expect("package hash");
        assert_eq!(
            mixed.validate(),
            Err(LongitudinalContractError::MixedRevision)
        );

        let attribution_timing = operation_receipt(
            LongitudinalLaneV1::AttributionCounts,
            digest("execution"),
            0,
        );
        assert_eq!(
            attribution_timing.validate(),
            Err(LongitudinalContractError::MixedLane)
        );

        let mut serialized = serde_json::to_value(empty_package()).expect("package JSON");
        serialized
            .as_object_mut()
            .expect("package object")
            .insert("capacityProbeReceipts".to_owned(), serde_json::json!([]));
        assert!(serde_json::from_value::<LongitudinalEvidencePackageV1>(serialized).is_err());
    }

    #[test]
    fn longitudinal_manifests_cannot_serialize_root_paths() {
        let mut serialized =
            serde_json::to_value(materialization("root").manifest).expect("manifest JSON");
        serialized
            .as_object_mut()
            .expect("manifest object")
            .insert("rootPath".to_owned(), "/tmp/root".into());
        assert!(serde_json::from_value::<LongitudinalWorkloadManifestV1>(serialized).is_err());
    }

    #[test]
    fn longitudinal_contract_fixtures_and_publication_are_byte_stable() {
        let workload = longitudinal_runner_contract_v1();
        let capacity = longitudinal_capacity_contract_v1();
        let workload_fixture = include_str!(
            "../../../tests/fixtures/store-foundation/longitudinal-runner-contract-v1.json"
        );
        let capacity_fixture = include_str!(
            "../../../tests/fixtures/store-foundation/longitudinal-capacity-sentinel-v1.json"
        );
        let expected_workload = format!(
            "{}\n",
            serde_json::to_string_pretty(&workload).expect("workload fixture JSON")
        );
        let expected_capacity = format!(
            "{}\n",
            serde_json::to_string_pretty(&capacity).expect("capacity fixture JSON")
        );

        assert_eq!(normalize_lf(workload_fixture), expected_workload);
        assert_eq!(normalize_lf(capacity_fixture), expected_capacity);
        assert_eq!(
            normalize_lf(&expected_workload.replace('\n', "\r\n")),
            expected_workload
        );
        assert_eq!(
            normalize_lf(&expected_capacity.replace('\n', "\r\n")),
            expected_capacity
        );

        let attributes = include_str!("../../../.gitattributes");
        assert!(attributes.contains(
            "tests/fixtures/store-foundation/longitudinal-runner-contract-v1.json text eol=lf"
        ));
        assert!(attributes.contains(
            "tests/fixtures/store-foundation/longitudinal-capacity-sentinel-v1.json text eol=lf"
        ));

        let publication = longitudinal_contract_publication_v1();
        publication.validate().expect("publication validates");
        let bytes = serde_json::to_string(&publication).expect("publication JSON");
        assert!(!bytes.contains('\n'));
        assert!(!bytes.contains('\r'));
        assert_eq!(
            publication.publication_sha256,
            LONGITUDINAL_CONTRACT_PUBLICATION_SHA256_V1
        );
    }

    #[test]
    fn carry_forward_contract_surface_is_versioned() {
        assert_eq!(
            LONGITUDINAL_CARRY_FORWARD_RECEIPT_SCHEMA_V1,
            "pointbreak.longitudinal-carry-forward-receipt.v1"
        );
        assert_eq!(
            LONGITUDINAL_CARRY_FORWARD_AUTHORITY_PACKAGE_SCHEMA_V1,
            "pointbreak.longitudinal-carry-forward-authority-package.v1"
        );
        assert_eq!(
            LONGITUDINAL_REMOVAL_UPGRADE_RECEIPT_SCHEMA_V1,
            "pointbreak.longitudinal-removal-upgrade-receipt.v1"
        );
        assert_eq!(
            LONGITUDINAL_REMOVAL_UPGRADE_AUTHORITY_PACKAGE_SCHEMA_V1,
            "pointbreak.longitudinal-removal-upgrade-authority-package.v1"
        );
        assert_eq!(
            LONGITUDINAL_PACKAGE_VERIFICATION_RECEIPT_SCHEMA_V1,
            "pointbreak.longitudinal-package-verification-receipt.v1"
        );
        assert_eq!(
            LONGITUDINAL_CONTROLLER_FAILURE_RECEIPT_SCHEMA_V1,
            "pointbreak.longitudinal-controller-failure-receipt.v1"
        );
        assert_eq!(
            LONGITUDINAL_CARRY_FORWARD_REQUEST_SCHEMA_V1,
            "pointbreak.longitudinal-carry-forward-request.v1"
        );
    }

    #[test]
    fn carry_forward_receipt_fails_closed_for_every_authority_binding() {
        let slot = LongitudinalCarryForwardSlotV1 {
            tier: LongitudinalTierV1::L1,
            lane: LongitudinalLaneV1::ReleaseUninstrumented,
            independent_run: 1,
        };
        let receipt = carry_receipt(slot, 0);
        receipt.validate().expect("carry receipt validates");

        let mut tampered = receipt.clone();
        tampered.carried_manifest_invariant_sha256 = digest("drifted invariant");
        tampered.receipt_sha256 = tampered.canonical_sha256().expect("tampered receipt hash");
        assert!(tampered.validate().is_err());

        tampered = receipt.clone();
        tampered.clone_post_inventory.inventory_sha256 = digest("changed store file");
        tampered.receipt_sha256 = tampered.canonical_sha256().expect("tampered receipt hash");
        assert!(tampered.validate().is_err());

        tampered = receipt.clone();
        tampered.source_strict.state_sha256 = digest("changed semantics");
        tampered.receipt_sha256 = tampered.canonical_sha256().expect("tampered receipt hash");
        assert!(tampered.validate().is_err());

        tampered = receipt.clone();
        tampered.schedule_sha256 = digest("changed schedule");
        tampered.receipt_sha256 = tampered.canonical_sha256().expect("tampered receipt hash");
        assert!(tampered.validate().is_err());

        tampered = receipt.clone();
        tampered.contract_sha256 = digest("changed workload contract");
        tampered.receipt_sha256 = tampered.canonical_sha256().expect("tampered receipt hash");
        assert!(tampered.validate().is_err());

        tampered = receipt.clone();
        tampered.materializer_equivalence.equivalent = false;
        tampered.materializer_equivalence.receipt_sha256 = tampered
            .materializer_equivalence
            .canonical_sha256()
            .expect("tampered equivalence hash");
        tampered.receipt_sha256 = tampered.canonical_sha256().expect("tampered receipt hash");
        assert!(tampered.validate().is_err());

        tampered = receipt;
        tampered.clone_root_identity = tampered.source_root_identity.clone();
        tampered.receipt_sha256 = tampered.canonical_sha256().expect("tampered receipt hash");
        assert!(tampered.validate().is_err());
    }

    #[test]
    fn carry_forward_authority_requires_all_slots_and_a_matching_verified_workload_package() {
        let package = carry_authority_package();
        package.validate().expect("complete authority validates");

        let mut incomplete = package.clone();
        incomplete.completion = None;
        incomplete.package_sha256 = incomplete.canonical_sha256().expect("package hash");
        incomplete
            .validate_incomplete()
            .expect("root authority validates before workload collection");
        assert_eq!(
            incomplete.validate(),
            Err(LongitudinalContractError::IncompleteEvidence)
        );

        let mut missing = package.clone();
        missing.carry_receipts.pop();
        missing.authority_set_sha256 = missing
            .canonical_authority_set_sha256()
            .expect("authority set hash");
        missing.package_sha256 = missing.canonical_sha256().expect("package hash");
        assert_eq!(
            missing.validate(),
            Err(LongitudinalContractError::IncompleteEvidence)
        );

        let mut reordered = package.clone();
        reordered.carry_receipts.swap(0, 1);
        reordered.authority_set_sha256 = reordered
            .canonical_authority_set_sha256()
            .expect("authority set hash");
        reordered.package_sha256 = reordered.canonical_sha256().expect("package hash");
        assert_eq!(
            reordered.validate(),
            Err(LongitudinalContractError::IncompleteEvidence)
        );

        let mut mismatch = package.clone();
        mismatch
            .completion
            .as_mut()
            .expect("completion")
            .final_workload_package_sha256 = digest("other package");
        mismatch.package_sha256 = mismatch.canonical_sha256().expect("package hash");
        assert_eq!(
            mismatch.validate(),
            Err(LongitudinalContractError::IncompleteEvidence)
        );

        let mut unverified = package;
        let completion = unverified.completion.as_mut().expect("completion");
        completion.package_verified = false;
        unverified.package_sha256 = unverified.canonical_sha256().expect("package hash");
        assert_eq!(
            unverified.validate(),
            Err(LongitudinalContractError::IncompleteEvidence)
        );
    }

    #[test]
    fn carry_forward_authority_rejects_cross_slot_execution_and_proof_drift() {
        let package = carry_authority_package();

        let mut final_diff_drift = package.clone();
        let receipt = &mut final_diff_drift.carry_receipts[1];
        receipt.final_authority_diff_sha256 = digest("different final authority diff");
        receipt.materializer_equivalence.implementation_diff_sha256 =
            receipt.final_authority_diff_sha256.clone();
        receipt.materializer_equivalence.receipt_sha256 = receipt
            .materializer_equivalence
            .canonical_sha256()
            .expect("equivalence receipt hash");
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("carry receipt hash");
        rehash_carry_authority_package(&mut final_diff_drift);
        assert_eq!(
            final_diff_drift.validate(),
            Err(LongitudinalContractError::PairMismatch)
        );

        let mut equivalence_drift = package.clone();
        let receipt = &mut equivalence_drift.carry_receipts[1];
        receipt.materializer_equivalence.baseline.root_identity = digest("different baseline root");
        receipt.materializer_equivalence.receipt_sha256 = receipt
            .materializer_equivalence
            .canonical_sha256()
            .expect("equivalence receipt hash");
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("carry receipt hash");
        rehash_carry_authority_package(&mut equivalence_drift);
        assert_eq!(
            equivalence_drift.validate(),
            Err(LongitudinalContractError::PairMismatch)
        );

        let mut same_lane_drift = package.clone();
        let receipt = &mut same_lane_drift.carry_receipts[1];
        receipt.source_execution.runner_sha256 = digest("different release runner");
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("carry receipt hash");
        rehash_carry_authority_package(&mut same_lane_drift);
        assert_eq!(
            same_lane_drift.validate(),
            Err(LongitudinalContractError::PairMismatch)
        );

        let mut wrong_lane_profile = package;
        let receipt = &mut wrong_lane_profile.carry_receipts[2];
        receipt.source_execution.build_profile = "release-uninstrumented".to_owned();
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("carry receipt hash");
        rehash_carry_authority_package(&mut wrong_lane_profile);
        assert_eq!(
            wrong_lane_profile.validate(),
            Err(LongitudinalContractError::MixedRevision)
        );
    }

    #[test]
    fn carry_forward_manifest_invariant_excludes_only_execution_and_its_hash() {
        let source = materialization("invariant-source").manifest;
        let mut carried = source.clone();
        carried.execution.source_commit = "9".repeat(40);
        carried.execution.source_tree = "8".repeat(40);
        carried.manifest_sha256 = carried.canonical_sha256().expect("carried manifest hash");
        assert_ne!(source.manifest_sha256, carried.manifest_sha256);
        assert_eq!(
            longitudinal_workload_manifest_carry_invariant_sha256_v1(&source).unwrap(),
            longitudinal_workload_manifest_carry_invariant_sha256_v1(&carried).unwrap()
        );

        let mut semantic_drift = source.clone();
        semantic_drift.expected_semantic_receipts[0].semantic_receipt_sha256 =
            digest("different semantic receipt");
        semantic_drift.manifest_sha256 = semantic_drift
            .canonical_sha256()
            .expect("semantic-drift manifest hash");
        semantic_drift
            .validate()
            .expect("self-consistent semantic drift remains structurally valid");
        assert_ne!(
            longitudinal_workload_manifest_carry_invariant_sha256_v1(&source).unwrap(),
            longitudinal_workload_manifest_carry_invariant_sha256_v1(&semantic_drift).unwrap(),
            "the carry invariant excludes execution only"
        );
    }

    #[test]
    fn removal_upgrade_receipt_fails_closed_for_upgrade_drift() {
        let package = removal_upgrade_authority_package();
        let receipt = package.upgrade_receipts[0].clone();
        receipt.validate().expect("upgrade receipt validates");

        let mut tampered = receipt.clone();
        tampered.changed_paths.pop();
        tampered.receipt_sha256 = tampered.canonical_sha256().expect("upgrade receipt hash");
        assert!(tampered.validate().is_err());

        tampered = receipt.clone();
        tampered.enrollment_relative_path = ".pointbreak/other.json".to_owned();
        tampered.receipt_sha256 = tampered.canonical_sha256().expect("upgrade receipt hash");
        assert!(tampered.validate().is_err());

        tampered = receipt.clone();
        tampered.successor_pre_inventory = tampered.successor_post_inventory.clone();
        tampered.receipt_sha256 = tampered.canonical_sha256().expect("upgrade receipt hash");
        assert!(tampered.validate().is_err());

        tampered = receipt.clone();
        tampered.successor_strict.state_sha256 = digest("changed upgrade semantics");
        tampered.receipt_sha256 = tampered.canonical_sha256().expect("upgrade receipt hash");
        assert!(tampered.validate().is_err());

        tampered = receipt.clone();
        tampered.resume_events_created = 1;
        tampered.resume_events_existing -= 1;
        tampered.receipt_sha256 = tampered.canonical_sha256().expect("upgrade receipt hash");
        assert!(tampered.validate().is_err());

        tampered = receipt;
        tampered.corrected_manifest_invariant_sha256 = digest("changed upgrade invariant");
        tampered.receipt_sha256 = tampered.canonical_sha256().expect("upgrade receipt hash");
        assert!(tampered.validate().is_err());
    }

    #[test]
    fn removal_upgrade_authority_requires_ordered_slots_equivalence_and_q2_binding() {
        let package = removal_upgrade_authority_package();
        package
            .validate()
            .expect("complete upgrade authority validates");

        let mut incomplete = package.clone();
        incomplete.completion = None;
        rehash_removal_upgrade_authority_package(&mut incomplete);
        incomplete
            .validate_incomplete()
            .expect("upgrade root authority validates before Q2");
        assert_eq!(
            incomplete.validate(),
            Err(LongitudinalContractError::IncompleteEvidence)
        );

        let mut missing = package.clone();
        missing.upgrade_receipts.pop();
        rehash_removal_upgrade_authority_package(&mut missing);
        assert!(missing.validate().is_err());

        let mut duplicate = package.clone();
        duplicate.upgrade_receipts[1] = duplicate.upgrade_receipts[0].clone();
        rehash_removal_upgrade_authority_package(&mut duplicate);
        assert!(duplicate.validate().is_err());

        let mut reordered = package.clone();
        reordered.upgrade_receipts.swap(0, 1);
        rehash_removal_upgrade_authority_package(&mut reordered);
        assert!(reordered.validate().is_err());

        let mut mixed = package.clone();
        mixed.corrected_execution.runner_sha256 = digest("other corrected runner");
        let verification = package_verification_receipt(&mixed.corrected_execution);
        mixed.completion = Some(LongitudinalRemovalUpgradeAuthorityCompletionV1 {
            final_workload_package_sha256: verification.package_sha256.clone(),
            verification_receipt: verification,
            package_verified: true,
        });
        rehash_removal_upgrade_authority_package(&mut mixed);
        assert_eq!(
            mixed.validate(),
            Err(LongitudinalContractError::PairMismatch)
        );

        let mut mismatched_equivalence = package.clone();
        let equivalence = &mut mismatched_equivalence.materializer_equivalences[0];
        equivalence.baseline.root_identity = digest("wrong upgraded root");
        equivalence.receipt_sha256 = equivalence
            .canonical_sha256()
            .expect("equivalence receipt hash");
        rehash_removal_upgrade_authority_package(&mut mismatched_equivalence);
        assert_eq!(
            mismatched_equivalence.validate(),
            Err(LongitudinalContractError::PairMismatch)
        );

        let mut wrong_q2 = package;
        wrong_q2
            .completion
            .as_mut()
            .expect("upgrade completion")
            .final_workload_package_sha256 = digest("wrong Q2 package");
        rehash_removal_upgrade_authority_package(&mut wrong_q2);
        assert_eq!(
            wrong_q2.validate(),
            Err(LongitudinalContractError::IncompleteEvidence)
        );
    }

    #[test]
    fn removal_upgrade_manifest_invariant_excludes_only_changed_carrier_and_execution_bytes() {
        let source_materialization = materialization("upgrade-invariant-source");
        let strict = source_materialization.strict;
        let source = source_materialization.manifest;
        let changed_event_ids = vec![source.ordered_events[0].event_id.clone()];
        let mut corrected = source.clone();
        corrected.execution.source_commit = "9".repeat(40);
        corrected.ordered_events[0].canonical_decoded_sha256 = digest("signed decoded event");
        corrected.event_carriers[0].raw_sha256 = digest("signed raw event");
        corrected.event_carriers[0].raw_bytes += 128;
        for receipt in &mut corrected.expected_semantic_receipts {
            receipt.semantic_receipt_sha256 =
                canonical_sha256(&(receipt.operation, &strict, &corrected.ordered_events))
                    .expect("corrected semantic receipt hash");
        }
        corrected.manifest_sha256 = corrected
            .canonical_sha256()
            .expect("corrected manifest hash");
        assert_eq!(
            longitudinal_workload_manifest_upgrade_invariant_sha256_v1(
                &source,
                &strict,
                &changed_event_ids
            )
            .unwrap(),
            longitudinal_workload_manifest_upgrade_invariant_sha256_v1(
                &corrected,
                &strict,
                &changed_event_ids
            )
            .unwrap()
        );

        let mut semantic_drift = source.clone();
        semantic_drift.expected_semantic_receipts[0].semantic_receipt_sha256 =
            digest("different upgrade semantic receipt");
        semantic_drift.manifest_sha256 = semantic_drift
            .canonical_sha256()
            .expect("semantic-drift manifest hash");
        assert!(matches!(
            longitudinal_workload_manifest_upgrade_invariant_sha256_v1(
                &semantic_drift,
                &strict,
                &changed_event_ids
            ),
            Err(LongitudinalContractError::HashMismatch {
                field: "expected semantic receipt"
            })
        ));

        let mut non_removal_drift = source.clone();
        non_removal_drift.event_carriers[1].raw_sha256 = digest("changed non-removal carrier");
        non_removal_drift.manifest_sha256 = non_removal_drift
            .canonical_sha256()
            .expect("non-removal-drift manifest hash");
        assert_ne!(
            longitudinal_workload_manifest_upgrade_invariant_sha256_v1(
                &source,
                &strict,
                &changed_event_ids
            )
            .unwrap(),
            longitudinal_workload_manifest_upgrade_invariant_sha256_v1(
                &non_removal_drift,
                &strict,
                &changed_event_ids
            )
            .unwrap(),
            "non-removal carrier bytes remain inside the upgrade invariant"
        );
    }

    #[test]
    fn controller_failure_receipt_is_typed_immutable_and_denies_unsanitized_fields() {
        let mut receipt = LongitudinalControllerFailureReceiptV1 {
            schema: LONGITUDINAL_CONTROLLER_FAILURE_RECEIPT_SCHEMA_V1.to_owned(),
            operation_selector:
                LongitudinalFailureOperationSelectorV1::InspectorRevisionListPreflight,
            http: Some(LongitudinalHttpFailureV1 {
                status: 500,
                body_classification: LongitudinalHttpBodyClassificationV1::JsonError,
                body_bytes: 4,
                body_sha256: Some(digest("body")),
            }),
            inspector_exit: LongitudinalInspectorExitV1::Exited(1),
            stderr: LongitudinalStderrFailureV1 {
                classification: LongitudinalStderrClassificationV1::KnownDiagnostic,
                bytes: 6,
                sha256: Some(digest("stderr")),
            },
            immutable: true,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("failure receipt hash");
        receipt.validate().expect("failure receipt validates");

        let mut serialized = serde_json::to_value(&receipt).expect("failure JSON");
        serialized
            .as_object_mut()
            .expect("failure object")
            .insert("responseBody".to_owned(), "secret-token".into());
        assert!(
            serde_json::from_value::<LongitudinalControllerFailureReceiptV1>(serialized).is_err()
        );

        receipt.immutable = false;
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("failure receipt hash");
        assert_eq!(
            receipt.validate(),
            Err(LongitudinalContractError::MutableFailure)
        );
    }

    fn normalize_lf(value: &str) -> String {
        value.replace("\r\n", "\n")
    }
}
