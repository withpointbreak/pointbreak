use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::storage_witness::{
    QualificationDerivedStorageForbiddenProbeHashesV1,
    QualificationDerivedStorageForbiddenProbeKindV1, QualificationDerivedStorageWitnessV1,
};
use crate::bench_support::longitudinal::{
    LongitudinalCounterReceiptV1, LongitudinalCountersV1,
    LongitudinalTimelineCarrierMismatchKindV1, LongitudinalTimelinePostPinBarrierReceiptV1,
    LongitudinalTimelinePostPinBoundaryV1,
};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};

pub const QUALIFICATION_DERIVED_ACCESS_CONTRACT_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-contract.v1";
pub const QUALIFICATION_DERIVED_ACCESS_CONTRACT_PUBLICATION_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-contract-publication.v1";
pub const QUALIFICATION_DERIVED_ACCESS_CONTRACT_FIXTURE_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-contract-fixture.v1";
pub const QUALIFICATION_DERIVED_ACCESS_EVIDENCE_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-evidence.v1";
pub const QUALIFICATION_DERIVED_ACCESS_EVALUATION_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-evaluation.v1";
pub const QUALIFICATION_DERIVED_ACCESS_PACKAGE_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-package.v1";
pub const QUALIFICATION_DERIVED_ACCESS_CONTRACT_MODE_V1: &str = "--derived-access-contract";
pub const QUALIFICATION_DERIVED_ACCESS_CONTRACT_SHA256_V1: &str =
    "c29fd0b862cfd3594c02b88f159477adb9b8666b8dfeebd868e766f8cf025ab8";
pub const QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V2: &str =
    "pointbreak.qualification-derived-access-evaluator.v2";
pub const QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3: &str =
    "pointbreak.qualification-derived-access-evaluator.v3";
pub const QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4: &str =
    "pointbreak.qualification-derived-access-evaluator.v4";
pub const QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V3_PROCEDURE_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-evaluator-v3-procedure.v1";
pub const QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V3_PROCEDURE_SHA256_V1: &str =
    "7ed026636813cdfa3abdcc06bac30268f9968d66bcdd1ad9cd174b36bdd9bae1";
pub const QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V4_PROCEDURE_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-evaluator-v4-procedure.v1";
pub const QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V4_PROCEDURE_SHA256_V1: &str =
    "a1caa365f3c4fdcad11b63605d8b2755989752f55692594bcd6c810a9c0bd22c";

const QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V3_STEPS_V1: [&str; 6] = [
    "change-read-parity-and-bounds-v1",
    "exact-product-and-harness-identity-v1",
    "complete-typed-error-documents-v1",
    "reader-v3-authority-lifecycle-concurrency-v1",
    "immutable-schema-and-byte-inventory-v1",
    "completion-last-independent-package-verification-v1",
];

const QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V4_STEPS_V1: [&str; 7] = [
    "change-read-parity-and-bounds-v1",
    "exact-product-and-harness-identity-v1",
    "complete-typed-error-documents-v1",
    "reader-v3-authority-lifecycle-concurrency-v1",
    "immutable-schema-and-byte-inventory-v1",
    "completion-last-independent-package-verification-v1",
    "timeline-route-parity-independent-errors-request-bounds-concurrent-trust-validated-stamps-canonical-byte-clone-seeded-disjoint-reference-fault-roots-post-pin-exact-carrier-barrier-and-asymmetric-one-bit-signature-recovery-v1",
];

pub const QUALIFICATION_TIMELINE_INVALID_SIGNATURE_MUTATION_RECIPE_SHA256_V1: &str =
    "27a29d47470c013d8df0ce7531f20670e8acbee9374d324a8285ec50e1a01a32";

pub fn qualification_derived_access_evaluator_v3_procedure_sha256() -> String {
    let procedure = serde_json::json!({
        "schema": QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V3_PROCEDURE_SCHEMA_V1,
        "steps": QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V3_STEPS_V1,
    });
    let bytes = canonical_json_bytes(&procedure)
        .expect("the derived-access evaluator-v3 procedure is canonical");
    let digest = sha256_bytes_hex(&bytes);
    assert_eq!(
        digest, QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V3_PROCEDURE_SHA256_V1,
        "compiled derived-access evaluator-v3 procedure drifted"
    );
    digest
}

pub fn qualification_derived_access_evaluator_v4_procedure_sha256() -> String {
    let procedure = serde_json::json!({
        "schema": QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V4_PROCEDURE_SCHEMA_V1,
        "steps": QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V4_STEPS_V1,
    });
    let bytes = canonical_json_bytes(&procedure)
        .expect("the derived-access evaluator-v4 procedure is canonical");
    let digest = sha256_bytes_hex(&bytes);
    assert_eq!(
        digest, QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V4_PROCEDURE_SHA256_V1,
        "compiled derived-access evaluator-v4 procedure drifted"
    );
    digest
}

const DERIVATION_COMMIT_V1: &str = "a0d1519e5dc86d385114abfbd8e806b1456f0474";
const DERIVATION_TREE_V1: &str = "6445e9e5af5062924ecee29647e8288c8865060d";
const DERIVATION_CARGO_LOCK_SHA256_V1: &str =
    "cc41c7f2ac96667f0da126bdc77ac70234a85238e3f4da1795adfa1bd56f86a3";
const LONGITUDINAL_SYNTHESIS_SHA256_V1: &str =
    "f543a41f63ea6f29fcf8ab71f3ec45894965a6c9a7119f3ef168308ea9a213b0";

const MIB: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedAccessStatusV1 {
    Passed,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum QualificationDerivedAccessTierV1 {
    #[serde(rename = "D0-128")]
    D0_128,
    #[serde(rename = "L1")]
    L1,
    #[serde(rename = "L7")]
    L7,
    #[serde(rename = "L100")]
    L100,
    #[serde(rename = "C262")]
    C262,
}

impl QualificationDerivedAccessTierV1 {
    pub const ALL: [Self; 5] = [Self::D0_128, Self::L1, Self::L7, Self::L100, Self::C262];
    pub(crate) const NATIVE: [Self; 3] = [Self::D0_128, Self::L1, Self::L7];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedAccessPlatformV1 {
    MacosApfs,
    WindowsNtfs,
    LinuxCompileCi,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QualificationDerivedAccessOperationV1 {
    SemanticId,
    FreshNoChange,
    #[serde(rename = "NEWCOUNT_ZERO")]
    NewCountZero,
    WindowHead,
    WindowMiddle,
    WindowTail,
    RevisionDetailActive,
    RevisionDetailRemoved,
    AppendOne,
    PostOne,
    Restart,
}

impl QualificationDerivedAccessOperationV1 {
    pub const ALL: [Self; 11] = [
        Self::SemanticId,
        Self::FreshNoChange,
        Self::NewCountZero,
        Self::WindowHead,
        Self::WindowMiddle,
        Self::WindowTail,
        Self::RevisionDetailActive,
        Self::RevisionDetailRemoved,
        Self::AppendOne,
        Self::PostOne,
        Self::Restart,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::SemanticId => "SEMANTIC_ID",
            Self::FreshNoChange => "FRESH_NO_CHANGE",
            Self::NewCountZero => "NEWCOUNT_ZERO",
            Self::WindowHead => "WINDOW_HEAD",
            Self::WindowMiddle => "WINDOW_MIDDLE",
            Self::WindowTail => "WINDOW_TAIL",
            Self::RevisionDetailActive => "REVISION_DETAIL_ACTIVE",
            Self::RevisionDetailRemoved => "REVISION_DETAIL_REMOVED",
            Self::AppendOne => "APPEND_ONE",
            Self::PostOne => "POST_ONE",
            Self::Restart => "RESTART",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedAccessProcessScopeV1 {
    InspectorServiceChild,
    Driver,
    QualificationHarness,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedAccessComplexityV1 {
    BoundedSelectedWork,
    HistoryOrCardinalityProportional,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedAccessLifecycleCriterionV1 {
    OpenBootstrapReopenReplayEquality,
    ConcurrentWritersLongLivedReader,
    UniqueEqualConflictCursorSequence,
    CrashBeforeIntentCommit,
    CrashAfterIntentBeforeEvent,
    CrashAfterEventBeforeReceipt,
    CrashAfterReceiptBeforeHead,
    CrashAfterHeadBeforeIntentRetirement,
    CrashDuringBootstrapStaging,
    CrashDuringQuarantineEpochPublication,
    DerivedTransactionInterruption,
    BackupWithoutDerivedThenRebuild,
    WrongRoot,
    WrongSchema,
    WrongProfile,
    CorruptionQuarantineNewEpoch,
    ReaderHandleReleaseRetirement,
    IndependentPackageVerification,
}

impl QualificationDerivedAccessLifecycleCriterionV1 {
    pub const ALL: [Self; 18] = [
        Self::OpenBootstrapReopenReplayEquality,
        Self::ConcurrentWritersLongLivedReader,
        Self::UniqueEqualConflictCursorSequence,
        Self::CrashBeforeIntentCommit,
        Self::CrashAfterIntentBeforeEvent,
        Self::CrashAfterEventBeforeReceipt,
        Self::CrashAfterReceiptBeforeHead,
        Self::CrashAfterHeadBeforeIntentRetirement,
        Self::CrashDuringBootstrapStaging,
        Self::CrashDuringQuarantineEpochPublication,
        Self::DerivedTransactionInterruption,
        Self::BackupWithoutDerivedThenRebuild,
        Self::WrongRoot,
        Self::WrongSchema,
        Self::WrongProfile,
        Self::CorruptionQuarantineNewEpoch,
        Self::ReaderHandleReleaseRetirement,
        Self::IndependentPackageVerification,
    ];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedAccessTransitionV1 {
    UniqueCreate,
    EqualDuplicate,
    ConflictingDuplicate,
    Fork,
    CanonicalEarlierInsertion,
    BackdatedInsertion,
    TrustOverlay,
    RemovalOverlay,
}

impl QualificationDerivedAccessTransitionV1 {
    pub const ALL: [Self; 8] = [
        Self::UniqueCreate,
        Self::EqualDuplicate,
        Self::ConflictingDuplicate,
        Self::Fork,
        Self::CanonicalEarlierInsertion,
        Self::BackdatedInsertion,
        Self::TrustOverlay,
        Self::RemovalOverlay,
    ];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedAccessTerminalOutcomeV1 {
    Reject,
    SurvivesApfsFalsifier,
    InsufficientEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessDerivationV1 {
    pub pointbreak_commit: String,
    pub pointbreak_tree: String,
    pub cargo_lock_sha256: String,
    pub longitudinal_synthesis_sha256: String,
    pub candidate_measurements_used: bool,
    pub private_corpus_used: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessEventFamilyV1 {
    pub event_type: String,
    pub count: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessD0V1 {
    pub tier: QualificationDerivedAccessTierV1,
    pub stored_events: u16,
    pub revisions: u16,
    pub revision_work_object_proposals: u16,
    pub task_work_object_proposals: u16,
    pub independently_referenced_objects: u16,
    pub schedule_sha256: String,
    pub event_families: Vec<QualificationDerivedAccessEventFamilyV1>,
    pub transitions: Vec<QualificationDerivedAccessTransitionV1>,
    pub operations: Vec<QualificationDerivedAccessOperationV1>,
    pub lifecycle_criteria: Vec<QualificationDerivedAccessLifecycleCriterionV1>,
    pub independent_roots: u8,
    pub byte_identical_roots_required: bool,
    pub timing_threshold_authorized: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessScaleProfilesV1 {
    pub l100_event_count: u64,
    pub c262_event_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessSamplingV1 {
    pub release_roots: u8,
    pub untimed_requests_per_warm_operation: u8,
    pub excluded_warmups_per_warm_operation: u8,
    pub retained_samples_per_warm_operation_per_root: u8,
    pub append_post_pairs_per_root: u8,
    pub restart_samples_per_root: u8,
    pub counting_samples_per_operation_and_tier: u8,
    pub p95_statistic: String,
    pub outlier_removal_allowed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessCounterCeilingsV1 {
    pub directory_entries_walked: u64,
    pub carrier_opens: u64,
    pub event_decodes: u64,
    pub event_validations: u64,
    pub event_folds: u64,
    pub chronological_sort_items: u64,
    pub body_artifact_reads: Option<u64>,
    pub object_artifact_reads: Option<u64>,
    pub projection_rebuilds: u64,
    pub state_rebuilds: u64,
    pub unselected_body_artifact_reads: u64,
    pub unselected_object_artifact_reads: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessOperationRequirementV1 {
    pub operation: QualificationDerivedAccessOperationV1,
    pub process_scope: QualificationDerivedAccessProcessScopeV1,
    pub semantic_receipt: String,
    pub l100_wall_p95_ceiling_ms: u64,
    pub l100_process_cpu_p95_ceiling_ms: u64,
    pub fixed_output: bool,
    pub max_l100_to_c262_selected_work_ratio_milli: u16,
    pub counters: QualificationDerivedAccessCounterCeilingsV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessMemoryV1 {
    pub l100_steady_rss_bytes: u64,
    pub l100_peak_rss_bytes: u64,
    pub l7_to_l100_steady_slope_bytes_per_event: u64,
    pub retained_body_object_bytes_outside_active_window: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessAllocationV1 {
    pub steady_fixed_floor_bytes: u64,
    pub steady_bytes_per_event: u64,
    pub high_water_ratio_milli: u16,
    pub append_write_amplification_ratio_milli: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessBootstrapV1 {
    pub l100_ceiling_seconds: u32,
    pub c262_ceiling_seconds: u32,
    pub progress_required: bool,
    pub experiment_cost_guard_not_product_startup_target: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessPlatformRequirementV1 {
    pub platform: QualificationDerivedAccessPlatformV1,
    pub required_tiers: Vec<QualificationDerivedAccessTierV1>,
    pub native_execution_required: bool,
    pub compile_ci_only: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessContractV1 {
    pub schema: String,
    pub evidence_schema: String,
    pub evaluation_schema: String,
    pub package_schema: String,
    pub derivation: QualificationDerivedAccessDerivationV1,
    pub contract_id: String,
    pub proposed_profile_identifier_field: String,
    pub authoritative_truth: String,
    pub derived_state_authority: String,
    pub d0: QualificationDerivedAccessD0V1,
    pub scale_profiles: QualificationDerivedAccessScaleProfilesV1,
    pub sampling: QualificationDerivedAccessSamplingV1,
    pub operations: Vec<QualificationDerivedAccessOperationRequirementV1>,
    pub memory: QualificationDerivedAccessMemoryV1,
    pub allocation: QualificationDerivedAccessAllocationV1,
    pub bootstrap: QualificationDerivedAccessBootstrapV1,
    pub platforms: Vec<QualificationDerivedAccessPlatformRequirementV1>,
    pub terminal_outcomes: Vec<QualificationDerivedAccessTerminalOutcomeV1>,
    pub complexity_precedes_latency: bool,
    pub gates_compensate_for_each_other: bool,
    pub public_inputs_only: bool,
    pub observed_candidate_result_present: bool,
    pub production_activation_authorized: bool,
    pub migration_authorized: bool,
    pub search_or_body_persistence_authorized: bool,
}

impl QualificationDerivedAccessContractV1 {
    fn frozen() -> Self {
        Self {
            schema: QUALIFICATION_DERIVED_ACCESS_CONTRACT_SCHEMA_V1.to_owned(),
            evidence_schema: QUALIFICATION_DERIVED_ACCESS_EVIDENCE_SCHEMA_V1.to_owned(),
            evaluation_schema: QUALIFICATION_DERIVED_ACCESS_EVALUATION_SCHEMA_V1.to_owned(),
            package_schema: QUALIFICATION_DERIVED_ACCESS_PACKAGE_SCHEMA_V1.to_owned(),
            derivation: QualificationDerivedAccessDerivationV1 {
                pointbreak_commit: DERIVATION_COMMIT_V1.to_owned(),
                pointbreak_tree: DERIVATION_TREE_V1.to_owned(),
                cargo_lock_sha256: DERIVATION_CARGO_LOCK_SHA256_V1.to_owned(),
                longitudinal_synthesis_sha256: LONGITUDINAL_SYNTHESIS_SHA256_V1.to_owned(),
                candidate_measurements_used: false,
                private_corpus_used: false,
            },
            contract_id: "incremental-derived-access-falsifier-v1".to_owned(),
            proposed_profile_identifier_field: "proposedProfileId".to_owned(),
            authoritative_truth: "unchanged loose journal and content carriers".to_owned(),
            derived_state_authority: "private, bodyless, disposable, rebuildable, and never truth"
                .to_owned(),
            d0: d0_contract(),
            scale_profiles: QualificationDerivedAccessScaleProfilesV1 {
                l100_event_count: 102_400,
                c262_event_count: 262_144,
            },
            sampling: QualificationDerivedAccessSamplingV1 {
                release_roots: 2,
                untimed_requests_per_warm_operation: 1,
                excluded_warmups_per_warm_operation: 3,
                retained_samples_per_warm_operation_per_root: 30,
                append_post_pairs_per_root: 30,
                restart_samples_per_root: 10,
                counting_samples_per_operation_and_tier: 1,
                p95_statistic: "nearest_rank_ceil_0.95_n".to_owned(),
                outlier_removal_allowed: false,
            },
            operations: QualificationDerivedAccessOperationV1::ALL
                .into_iter()
                .map(operation_requirement)
                .collect(),
            memory: QualificationDerivedAccessMemoryV1 {
                l100_steady_rss_bytes: 96 * MIB,
                l100_peak_rss_bytes: 128 * MIB,
                l7_to_l100_steady_slope_bytes_per_event: 512,
                retained_body_object_bytes_outside_active_window: 0,
            },
            allocation: QualificationDerivedAccessAllocationV1 {
                steady_fixed_floor_bytes: 64 * MIB,
                steady_bytes_per_event: 1_024,
                high_water_ratio_milli: 1_500,
                append_write_amplification_ratio_milli: 8_000,
            },
            bootstrap: QualificationDerivedAccessBootstrapV1 {
                l100_ceiling_seconds: 60 * 60,
                c262_ceiling_seconds: 180 * 60,
                progress_required: true,
                experiment_cost_guard_not_product_startup_target: true,
            },
            platforms: vec![
                QualificationDerivedAccessPlatformRequirementV1 {
                    platform: QualificationDerivedAccessPlatformV1::MacosApfs,
                    required_tiers: QualificationDerivedAccessTierV1::ALL.to_vec(),
                    native_execution_required: true,
                    compile_ci_only: false,
                },
                QualificationDerivedAccessPlatformRequirementV1 {
                    platform: QualificationDerivedAccessPlatformV1::WindowsNtfs,
                    required_tiers: QualificationDerivedAccessTierV1::NATIVE.to_vec(),
                    native_execution_required: true,
                    compile_ci_only: false,
                },
                QualificationDerivedAccessPlatformRequirementV1 {
                    platform: QualificationDerivedAccessPlatformV1::LinuxCompileCi,
                    required_tiers: Vec::new(),
                    native_execution_required: false,
                    compile_ci_only: true,
                },
            ],
            terminal_outcomes: vec![
                QualificationDerivedAccessTerminalOutcomeV1::Reject,
                QualificationDerivedAccessTerminalOutcomeV1::SurvivesApfsFalsifier,
                QualificationDerivedAccessTerminalOutcomeV1::InsufficientEvidence,
            ],
            complexity_precedes_latency: true,
            gates_compensate_for_each_other: false,
            public_inputs_only: true,
            observed_candidate_result_present: false,
            production_activation_authorized: false,
            migration_authorized: false,
            search_or_body_persistence_authorized: false,
        }
    }

    pub fn canonical_sha256(&self) -> Result<String, String> {
        canonical_sha256(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self != &Self::frozen() {
            return Err("unsupported derived-access contract".to_owned());
        }
        let actual = self.canonical_sha256()?;
        if actual != QUALIFICATION_DERIVED_ACCESS_CONTRACT_SHA256_V1 {
            return Err(format!(
                "derived-access contract hash is not frozen: expected {}, actual {actual}",
                QUALIFICATION_DERIVED_ACCESS_CONTRACT_SHA256_V1
            ));
        }
        Ok(())
    }

    pub fn decision_table_markdown(&self) -> String {
        let operation_limits = self
            .operations
            .iter()
            .map(|row| {
                format!(
                    "`{}` `{}/{} ms`",
                    row.operation.as_str(),
                    row.l100_wall_p95_ceiling_ms,
                    row.l100_process_cpu_p95_ceiling_ms
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        let selected_work_ratio = self
            .operations
            .first()
            .map(|row| row.max_l100_to_c262_selected_work_ratio_milli)
            .expect("the frozen contract has operation requirements");
        assert!(
            self.operations
                .iter()
                .all(|row| row.max_l100_to_c262_selected_work_ratio_milli == selected_work_ratio),
            "the grouped complexity row requires one shared selected-work ratio"
        );
        [
            "| Decision | Frozen requirement |".to_owned(),
            "| --- | --- |".to_owned(),
            format!("| Contract | `{}` |", self.contract_id),
            format!(
                "| Evaluator | `{QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V2}`; this revision identifies the decision procedure independently of the frozen parameter digest |"
            ),
            "| Authority | loose journal/content carriers remain truth; derived state is private, bodyless, disposable, and rebuildable |".to_owned(),
            format!(
                "| Correctness tier | `D0-128`: {} events, {} revisions, {} independent objects, {} byte-identical roots, frozen transition/operation/lifecycle coverage, no timing threshold; the runner later binds one public seed and ordered-schedule receipt |",
                self.d0.stored_events,
                self.d0.revisions,
                self.d0.independently_referenced_objects,
                self.d0.independent_roots
            ),
            format!(
                "| Operations | {} |",
                self.operations
                    .iter()
                    .map(|row| format!("`{}`", row.operation.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            format!(
                "| Samples | {} release roots; {} untimed request and {} excluded warmups; {} retained warm/append-post samples per root; {} restart samples per root; no outlier removal |",
                self.sampling.release_roots,
                self.sampling.untimed_requests_per_warm_operation,
                self.sampling.excluded_warmups_per_warm_operation,
                self.sampling.retained_samples_per_warm_operation_per_root,
                self.sampling.restart_samples_per_root
            ),
            format!(
                "| Complexity | classify before latency; fixed-output work is bounded selected work; L100-to-C262 work/retention ratio at most `{}` unless the receipt proves that all excess work is selected-output growth |",
                format_ratio_milli(selected_work_ratio)
            ),
            format!("| L100 latency / CPU | {operation_limits} |"),
            format!(
                "| Memory | store-attributable L100 steady/peak RSS at most `{}/{} MiB`; L7-to-L100 steady slope at most `{} bytes/event`; zero retained body/object bytes outside the active window |",
                self.memory.l100_steady_rss_bytes / MIB,
                self.memory.l100_peak_rss_bytes / MIB,
                self.memory.l7_to_l100_steady_slope_bytes_per_event
            ),
            format!(
                "| Allocation | steady derived bytes at most `max({} MiB, {} × event count)`; high-water at most `{}×`; append write amplification at most `{}×` |",
                self.allocation.steady_fixed_floor_bytes / MIB,
                self.allocation.steady_bytes_per_event,
                format_ratio_milli(self.allocation.high_water_ratio_milli),
                format_ratio_milli(self.allocation.append_write_amplification_ratio_milli)
            ),
            format!(
                "| Bootstrap | L100 at most {} minutes; C262 at most {} minutes; progress required; experiment-cost guards only |",
                self.bootstrap.l100_ceiling_seconds / 60,
                self.bootstrap.c262_ceiling_seconds / 60
            ),
            "| Native gates | macOS/APFS and Windows/NTFS independently pass D0-128/L1/L7 before APFS L100/C262; Linux is compile/CI only |".to_owned(),
            "| Non-compensation | semantics, provenance, native, lifecycle, complexity, latency/CPU, memory, allocation, write amplification, and bootstrap gate independently |".to_owned(),
            "| Outcomes | `reject`, `survives_apfs_falsifier`, or `insufficient_evidence`; survival authorizes no production activation or migration |".to_owned(),
            "| Inputs | qualification evidence and measurement use only public generated inputs; derivation hash commitments are provenance, not workload inputs |".to_owned(),
            "| Excluded | observed candidate result, search/body persistence, private corpus, candidate measurements, production selection, activation, migration, and release promises |".to_owned(),
        ]
        .join("\n")
    }
}

fn format_ratio_milli(value: u16) -> String {
    let whole = value / 1_000;
    let fractional = value % 1_000;
    if fractional == 0 {
        whole.to_string()
    } else if fractional.is_multiple_of(100) {
        format!("{whole}.{}", fractional / 100)
    } else if fractional.is_multiple_of(10) {
        format!("{whole}.{:02}", fractional / 10)
    } else {
        format!("{whole}.{fractional:03}")
    }
}

pub fn qualification_derived_access_contract_v1() -> QualificationDerivedAccessContractV1 {
    QualificationDerivedAccessContractV1::frozen()
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessContractPublicationV1 {
    pub schema: String,
    pub mode: String,
    pub evaluator_revision: String,
    pub evaluator_procedure_sha256: String,
    pub contract: QualificationDerivedAccessContractV1,
    pub contract_sha256: String,
    pub decision_table_markdown: String,
}

pub fn qualification_derived_access_contract_v1_publication()
-> QualificationDerivedAccessContractPublicationV1 {
    let contract = qualification_derived_access_contract_v1();
    QualificationDerivedAccessContractPublicationV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_CONTRACT_PUBLICATION_SCHEMA_V1.to_owned(),
        mode: "non_timing_contract_publication".to_owned(),
        evaluator_revision: QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3.to_owned(),
        evaluator_procedure_sha256: qualification_derived_access_evaluator_v3_procedure_sha256(),
        contract_sha256: contract
            .canonical_sha256()
            .expect("the frozen derived-access contract is canonical"),
        decision_table_markdown: contract.decision_table_markdown(),
        contract,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessContractFixtureV1 {
    pub schema: String,
    pub contract_schema: String,
    pub contract_sha256: String,
    pub evaluator_revision: String,
    pub d0_stored_events: u16,
    pub d0_revisions: u16,
    pub d0_independently_referenced_objects: u16,
    pub d0_schedule_sha256: String,
    pub operation_ids: Vec<QualificationDerivedAccessOperationV1>,
    pub terminal_outcomes: Vec<QualificationDerivedAccessTerminalOutcomeV1>,
    pub observed_candidate_result_present: bool,
}

pub fn qualification_derived_access_contract_fixture_v1()
-> QualificationDerivedAccessContractFixtureV1 {
    let contract = qualification_derived_access_contract_v1();
    QualificationDerivedAccessContractFixtureV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_CONTRACT_FIXTURE_SCHEMA_V1.to_owned(),
        contract_schema: contract.schema,
        contract_sha256: QUALIFICATION_DERIVED_ACCESS_CONTRACT_SHA256_V1.to_owned(),
        evaluator_revision: QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V2.to_owned(),
        d0_stored_events: contract.d0.stored_events,
        d0_revisions: contract.d0.revisions,
        d0_independently_referenced_objects: contract.d0.independently_referenced_objects,
        d0_schedule_sha256: contract.d0.schedule_sha256,
        operation_ids: contract
            .operations
            .into_iter()
            .map(|row| row.operation)
            .collect(),
        terminal_outcomes: contract.terminal_outcomes,
        observed_candidate_result_present: contract.observed_candidate_result_present,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessExecutionIdentityV1 {
    pub platform: QualificationDerivedAccessPlatformV1,
    pub source_commit: String,
    pub source_tree: String,
    pub cargo_lock_sha256: String,
    pub binary_sha256: String,
    pub contract_schema: String,
    pub contract_sha256: String,
    pub root_provenance_sha256: String,
    pub command_sha256: String,
    pub operating_system: String,
    pub architecture: String,
    pub filesystem: String,
    /// SHA-256 of the explicit logical campaign-host label. This is not a
    /// hardware identifier or a hash of an ambient network hostname.
    pub host_identity_sha256: String,
    pub source_dirty: bool,
    pub private_corpus_configured: bool,
}

impl QualificationDerivedAccessExecutionIdentityV1 {
    pub fn validate(&self) -> Result<(), String> {
        validate_hex(&self.source_commit, 40, "source commit")?;
        validate_hex(&self.source_tree, 40, "source tree")?;
        for (value, label) in [
            (&self.cargo_lock_sha256, "Cargo.lock SHA-256"),
            (&self.binary_sha256, "binary SHA-256"),
            (&self.contract_sha256, "contract SHA-256"),
            (&self.root_provenance_sha256, "root provenance SHA-256"),
            (&self.command_sha256, "command SHA-256"),
            (&self.host_identity_sha256, "host identity SHA-256"),
        ] {
            validate_hex(value, 64, label)?;
        }
        if self.contract_schema != QUALIFICATION_DERIVED_ACCESS_CONTRACT_SCHEMA_V1
            || self.contract_sha256 != QUALIFICATION_DERIVED_ACCESS_CONTRACT_SHA256_V1
            || self.source_dirty
            || self.private_corpus_configured
            || self.architecture.is_empty()
        {
            return Err("derived-access execution identity is not admissible".to_owned());
        }
        let platform_matches = match self.platform {
            QualificationDerivedAccessPlatformV1::MacosApfs => {
                self.operating_system == "macos" && self.filesystem == "apfs"
            }
            QualificationDerivedAccessPlatformV1::WindowsNtfs => {
                self.operating_system == "windows" && self.filesystem == "ntfs"
            }
            QualificationDerivedAccessPlatformV1::LinuxCompileCi => false,
        };
        if !platform_matches {
            return Err("derived-access platform identity is inconsistent".to_owned());
        }
        Ok(())
    }

    pub fn canonical_sha256(&self) -> Result<String, String> {
        canonical_sha256(self)
    }
}

/// Exact product binary and source identity, kept separate from the
/// instrumented qualification-harness execution identity.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessProductIdentityV1 {
    pub platform: QualificationDerivedAccessPlatformV1,
    pub source_commit: String,
    pub source_tree: String,
    pub cargo_lock_sha256: String,
    pub binary_sha256: String,
    pub version_sha256: String,
    pub build_profile: String,
    pub enabled_features: Vec<String>,
    pub build_command_sha256: String,
    pub operating_system: String,
    pub architecture: String,
    pub source_dirty: bool,
}

impl QualificationDerivedAccessProductIdentityV1 {
    pub fn validate(&self) -> Result<(), String> {
        validate_hex(&self.source_commit, 40, "product source commit")?;
        validate_hex(&self.source_tree, 40, "product source tree")?;
        for (value, label) in [
            (&self.cargo_lock_sha256, "product Cargo.lock SHA-256"),
            (&self.binary_sha256, "product binary SHA-256"),
            (&self.version_sha256, "product version SHA-256"),
            (&self.build_command_sha256, "product build command SHA-256"),
        ] {
            validate_hex(value, 64, label)?;
        }
        if self.source_dirty
            || self.build_profile.trim().is_empty()
            || self.build_profile.trim() != self.build_profile
            || self.operating_system.trim().is_empty()
            || self.architecture.trim().is_empty()
            || self
                .enabled_features
                .iter()
                .any(|feature| feature.trim().is_empty() || feature.trim() != feature)
            || !self
                .enabled_features
                .windows(2)
                .all(|features| features[0] < features[1])
        {
            return Err("derived-access product identity is not admissible".to_owned());
        }
        let platform_matches = match self.platform {
            QualificationDerivedAccessPlatformV1::MacosApfs => self.operating_system == "macos",
            QualificationDerivedAccessPlatformV1::WindowsNtfs => self.operating_system == "windows",
            QualificationDerivedAccessPlatformV1::LinuxCompileCi => false,
        };
        if !platform_matches {
            return Err("derived-access product platform identity is inconsistent".to_owned());
        }
        Ok(())
    }

    pub fn canonical_sha256(&self) -> Result<String, String> {
        canonical_sha256(self)
    }

    pub fn is_exact_source_for(
        &self,
        execution: &QualificationDerivedAccessExecutionIdentityV1,
    ) -> bool {
        self.platform == execution.platform
            && self.source_commit == execution.source_commit
            && self.source_tree == execution.source_tree
            && self.cargo_lock_sha256 == execution.cargo_lock_sha256
            && self.operating_system == execution.operating_system
            && self.architecture == execution.architecture
            && !self.source_dirty
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessCountersV1 {
    pub directory_entries_walked: u64,
    pub carrier_opens: u64,
    pub carrier_bytes_read: u64,
    pub event_decodes: u64,
    pub event_validations: u64,
    pub event_folds: u64,
    pub chronological_sort_items: u64,
    pub body_artifact_reads: u64,
    pub body_bytes_read: u64,
    pub object_artifact_reads: u64,
    pub object_bytes_read: u64,
    pub unselected_body_artifact_reads: u64,
    pub unselected_object_artifact_reads: u64,
    pub projection_rebuilds: u64,
    pub state_rebuilds: u64,
    pub response_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessOperationEvidenceV1 {
    pub tier: QualificationDerivedAccessTierV1,
    pub platform: QualificationDerivedAccessPlatformV1,
    pub operation: QualificationDerivedAccessOperationV1,
    pub status: QualificationDerivedAccessStatusV1,
    pub process_scope: QualificationDerivedAccessProcessScopeV1,
    pub semantic_receipt_matches: bool,
    pub complexity: QualificationDerivedAccessComplexityV1,
    pub retained_samples: u16,
    pub wall_p95_ms: Option<u64>,
    pub process_cpu_p95_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_output_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unselected_work_count: Option<u64>,
    pub selected_work_count: u64,
    pub retained_cardinality: u64,
    pub l100_to_c262_selected_work_ratio_milli: Option<u16>,
    pub counters: QualificationDerivedAccessCountersV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessLifecycleEvidenceV1 {
    pub tier: QualificationDerivedAccessTierV1,
    pub platform: QualificationDerivedAccessPlatformV1,
    pub criterion: QualificationDerivedAccessLifecycleCriterionV1,
    pub status: QualificationDerivedAccessStatusV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessD0EvidenceV1 {
    pub platform: QualificationDerivedAccessPlatformV1,
    pub stored_events: u16,
    pub revisions: u16,
    pub independently_referenced_objects: u16,
    pub schedule_sha256: String,
    pub ordered_schedule_sha256: String,
    pub root_a_sha256: String,
    pub root_b_sha256: String,
    pub byte_identical: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessResourceEvidenceV1 {
    pub l100_steady_rss_bytes: u64,
    pub l100_peak_rss_bytes: u64,
    pub l7_to_l100_steady_slope_bytes_per_event: u64,
    pub retained_body_object_bytes_outside_active_window: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessAllocationEvidenceV1 {
    pub tier: QualificationDerivedAccessTierV1,
    pub event_count: u64,
    pub steady_derived_bytes: u64,
    pub high_water_derived_bytes: u64,
    pub append_write_amplification_ratio_milli: u16,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessRootBindingV1 {
    pub platform: QualificationDerivedAccessPlatformV1,
    pub tier: QualificationDerivedAccessTierV1,
    pub role: String,
    pub command_sha256: String,
    pub admitted_root_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessBootstrapEvidenceV1 {
    pub tier: QualificationDerivedAccessTierV1,
    pub status: QualificationDerivedAccessStatusV1,
    pub elapsed_seconds: u32,
    pub progress_reported: bool,
    pub high_water_derived_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedChangeReadCaseV1 {
    Profile,
    ChangesBare,
    ChangesBounded,
    AttentionBare,
    AttentionBounded,
    BodylessFilterSuite,
    SummaryQuery,
    SummaryFilterSuite,
    PageTokenSuite,
    ConcurrentReaders,
    FreshProcessSuite,
    WarmReuseSuite,
    StalePageToken,
    PostAppendSuite,
    PostAppendFreshProcessSuite,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedChangeEvidencePurposeV1 {
    PreCutFalsifier,
    ExactSourceQualification,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedChangeTypedDocumentV1 {
    pub schema: String,
    pub version: u32,
    pub code: String,
    pub retryable: Option<bool>,
    pub canonical_sha256: String,
}

impl QualificationDerivedChangeTypedDocumentV1 {
    fn validate(&self) -> Result<(), String> {
        if self.schema.trim().is_empty()
            || self.schema.trim() != self.schema
            || self.version == 0
            || self.code.trim().is_empty()
            || self.code.trim() != self.code
        {
            return Err("typed Change failure document is incomplete".to_owned());
        }
        validate_hex(&self.canonical_sha256, 64, "typed Change failure document")
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedChangeControlCaseV1 {
    L0NoGeneration,
    M1PreactivationInvalidation,
    L2CurrentProfile,
    AuthorityFailureAxes,
    IncompatibleReader,
    AbsentV3,
    StaleV3,
    CorruptV3,
    CheckpointAuthorityMismatch,
    CheckpointStampMismatch,
    CheckpointAnchorMismatch,
    InterruptedPublication,
    InterruptedCatchUpResume,
    CatchUpMaintenanceAttribution,
    CurrentReadNoMaintenance,
    MovingCheckpoint,
    MovingPublication,
    GenerationLeaseOverlap,
    NPlusOnePublication,
    GenerationReclamation,
    DirectReadyCallGraphRefusal,
    AutomaticErrorCallGraphRefusal,
    CapabilityClassifierAuthorityOnly,
    ExplicitOffIsolation,
    ExplicitOffStrictReader,
    ConcurrentWritersAndReaders,
    BusyWriterNonblocking,
}

impl QualificationDerivedChangeControlCaseV1 {
    pub const ALL: [Self; 27] = [
        Self::L0NoGeneration,
        Self::M1PreactivationInvalidation,
        Self::L2CurrentProfile,
        Self::AuthorityFailureAxes,
        Self::IncompatibleReader,
        Self::AbsentV3,
        Self::StaleV3,
        Self::CorruptV3,
        Self::CheckpointAuthorityMismatch,
        Self::CheckpointStampMismatch,
        Self::CheckpointAnchorMismatch,
        Self::InterruptedPublication,
        Self::InterruptedCatchUpResume,
        Self::CatchUpMaintenanceAttribution,
        Self::CurrentReadNoMaintenance,
        Self::MovingCheckpoint,
        Self::MovingPublication,
        Self::GenerationLeaseOverlap,
        Self::NPlusOnePublication,
        Self::GenerationReclamation,
        Self::DirectReadyCallGraphRefusal,
        Self::AutomaticErrorCallGraphRefusal,
        Self::CapabilityClassifierAuthorityOnly,
        Self::ExplicitOffIsolation,
        Self::ExplicitOffStrictReader,
        Self::ConcurrentWritersAndReaders,
        Self::BusyWriterNonblocking,
    ];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedChangeControlBinaryKindV1 {
    Library,
    Cli,
}

impl QualificationDerivedChangeControlBinaryKindV1 {
    pub const ALL: [Self; 2] = [Self::Library, Self::Cli];
}

pub(crate) fn qualification_derived_change_control_test_v1(
    case: QualificationDerivedChangeControlCaseV1,
) -> (QualificationDerivedChangeControlBinaryKindV1, &'static str) {
    use QualificationDerivedChangeControlBinaryKindV1 as Binary;
    use QualificationDerivedChangeControlCaseV1 as Case;
    match case {
        Case::L0NoGeneration => (
            Binary::Library,
            "session::derived_access::changes::tests::l0_control_path_survives_a_preactivation_generation_without_a_live_checkpoint",
        ),
        Case::M1PreactivationInvalidation => (
            Binary::Library,
            "session::derived_access::changes::tests::m1_control_path_invalidates_a_preactivation_current_without_semantic_fallback",
        ),
        Case::L2CurrentProfile => (
            Binary::Library,
            "session::derived_access::changes::tests::receipt_backed_profile_matches_strict_oracle_at_the_live_checkpoint",
        ),
        Case::AuthorityFailureAxes => (
            Binary::Library,
            "session::derived_access::changes::tests::derived_change_outcomes_keep_failure_axes_distinct",
        ),
        Case::IncompatibleReader | Case::AbsentV3 => (
            Binary::Library,
            "session::derived_access::changes::tests::missing_and_incompatible_v3_profiles_are_typed_without_strict_fallback",
        ),
        Case::StaleV3 | Case::CorruptV3 | Case::CheckpointAnchorMismatch => (
            Binary::Library,
            "session::derived_access::lifecycle::tests::change_receipt_failures_are_classified_without_strict_fallback",
        ),
        Case::CheckpointAuthorityMismatch | Case::CheckpointStampMismatch => (
            Binary::Library,
            "session::derived_access::changes::tests::strict_stamp_binder_never_binds_a_mismatched_or_moving_checkpoint",
        ),
        Case::MovingCheckpoint => (
            Binary::Library,
            "session::derived_access::changes::tests::derived_change_reads_retry_when_the_checkpoint_moves_mid_read",
        ),
        Case::InterruptedPublication => (
            Binary::Library,
            "session::derived_access::lifecycle::tests::every_publication_boundary_is_process_interruption_safe",
        ),
        Case::InterruptedCatchUpResume => (
            Binary::Library,
            "session::derived_access::writer::tests::interrupted_change_catch_up_rolls_back_identity_and_checkpoint_then_resumes",
        ),
        Case::CatchUpMaintenanceAttribution => (
            Binary::Library,
            "session::derived_access::writer::tests::governed_change_catch_up_counts_bodyless_authority_maintenance_separately",
        ),
        Case::CurrentReadNoMaintenance => (
            Binary::Library,
            "session::derived_access::writer::tests::current_change_read_does_not_run_authority_cursor_maintenance",
        ),
        Case::MovingPublication => (
            Binary::Library,
            "session::derived_access::changes::tests::derived_change_reads_retry_when_publication_moves_mid_read",
        ),
        Case::GenerationLeaseOverlap => (
            Binary::Library,
            "session::derived_access::lifecycle::tests::replacement_keeps_an_open_reader_on_the_prior_generation",
        ),
        Case::NPlusOnePublication => (
            Binary::Library,
            "session::derived_access::lifecycle::tests::reader_retries_when_publication_changes_before_lease_acquisition",
        ),
        Case::GenerationReclamation => (
            Binary::Library,
            "session::derived_access::lifecycle::tests::repeated_rebuilds_collect_reclaimed_generation_leases",
        ),
        Case::DirectReadyCallGraphRefusal => (
            Binary::Cli,
            "cli::inspect::server::tests::change_query_validation_precedes_derived_generation_access",
        ),
        Case::AutomaticErrorCallGraphRefusal => (
            Binary::Cli,
            "cli::inspect::server::tests::l0_v2_routes_survive_the_automatic_preactivation_generation",
        ),
        Case::CapabilityClassifierAuthorityOnly => (
            Binary::Library,
            "session::derived_access::changes::tests::m1_control_path_invalidates_a_preactivation_current_without_semantic_fallback",
        ),
        Case::ExplicitOffIsolation => (
            Binary::Library,
            "session::derived_access::service_tests::explicit_off_stays_off_through_the_change_recovery_adapter",
        ),
        Case::ExplicitOffStrictReader => (
            Binary::Cli,
            "cli::inspect::server::tests::routes_split_derived_collections_and_timeline_from_explicit_off_and_exact_reads",
        ),
        Case::ConcurrentWritersAndReaders => (
            Binary::Library,
            "session::derived_access::writer::tests::concurrent_product_writers_preserve_authoritative_events",
        ),
        Case::BusyWriterNonblocking => (
            Binary::Library,
            "session::derived_access::lifecycle::tests::stable_authority_successor_does_not_wait_for_a_busy_writer",
        ),
    }
}

pub(crate) fn qualification_derived_change_control_attestation_test_v1(
    kind: QualificationDerivedChangeControlBinaryKindV1,
) -> &'static str {
    match kind {
        QualificationDerivedChangeControlBinaryKindV1::Library => {
            "bench_support::derived_access::contract::tests::qualification_library_control_binary_attests_clean_source"
        }
        QualificationDerivedChangeControlBinaryKindV1::Cli => {
            "cli::inspect::server::tests::qualification_cli_control_binary_attests_clean_source"
        }
    }
}

pub(crate) fn qualification_derived_change_control_command_sha256_v1(test_name: &str) -> String {
    let arguments = ["--exact", test_name, "--nocapture", "--test-threads=1"];
    sha256_bytes_hex(
        &canonical_json_bytes(&serde_json::json!({ "arguments": arguments }))
            .expect("the exact control command is canonical"),
    )
}

pub(crate) fn qualification_derived_change_control_build_command_sha256_v1(
    kind: QualificationDerivedChangeControlBinaryKindV1,
) -> String {
    let arguments = match kind {
        QualificationDerivedChangeControlBinaryKindV1::Library => vec![
            "+stable",
            "test",
            "--locked",
            "--features",
            "longitudinal-counting",
            "--lib",
            "--no-run",
        ],
        QualificationDerivedChangeControlBinaryKindV1::Cli => vec![
            "+stable",
            "test",
            "--locked",
            "--features",
            "longitudinal-counting",
            "--bin",
            "pointbreak",
            "--no-run",
        ],
    };
    sha256_bytes_hex(
        &canonical_json_bytes(&serde_json::json!({
            "program": "cargo",
            "arguments": arguments,
        }))
        .expect("the exact control build command is canonical"),
    )
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedChangeControlBinaryIdentityV1 {
    pub platform: QualificationDerivedAccessPlatformV1,
    pub kind: QualificationDerivedChangeControlBinaryKindV1,
    pub source_commit: String,
    pub source_tree: String,
    pub cargo_lock_sha256: String,
    pub binary_sha256: String,
    pub build_command_sha256: String,
    pub operating_system: String,
    pub architecture: String,
    pub source_dirty: bool,
    pub attestation_test: String,
    pub attestation_command_sha256: String,
    pub attestation_stdout_sha256: String,
    pub attestation_stderr_sha256: String,
}

impl QualificationDerivedChangeControlBinaryIdentityV1 {
    pub fn validate(&self) -> Result<(), String> {
        validate_hex(&self.source_commit, 40, "control source commit")?;
        validate_hex(&self.source_tree, 40, "control source tree")?;
        for (value, label) in [
            (&self.cargo_lock_sha256, "control Cargo.lock SHA-256"),
            (&self.binary_sha256, "control binary SHA-256"),
            (&self.build_command_sha256, "control build command SHA-256"),
            (
                &self.attestation_command_sha256,
                "control attestation command SHA-256",
            ),
            (
                &self.attestation_stdout_sha256,
                "control attestation stdout SHA-256",
            ),
            (
                &self.attestation_stderr_sha256,
                "control attestation stderr SHA-256",
            ),
        ] {
            validate_hex(value, 64, label)?;
        }
        if self.source_dirty
            || self.operating_system.trim().is_empty()
            || self.architecture.trim().is_empty()
            || self.attestation_test
                != qualification_derived_change_control_attestation_test_v1(self.kind)
            || self.attestation_command_sha256
                != qualification_derived_change_control_command_sha256_v1(&self.attestation_test)
            || self.build_command_sha256
                != qualification_derived_change_control_build_command_sha256_v1(self.kind)
        {
            return Err("derived Change control binary identity is not admissible".to_owned());
        }
        Ok(())
    }

    pub fn is_exact_source_for(
        &self,
        execution: &QualificationDerivedAccessExecutionIdentityV1,
    ) -> bool {
        self.platform == execution.platform
            && self.source_commit == execution.source_commit
            && self.source_tree == execution.source_tree
            && self.cargo_lock_sha256 == execution.cargo_lock_sha256
            && self.operating_system == execution.operating_system
            && self.architecture == execution.architecture
            && !self.source_dirty
    }

    pub fn canonical_sha256(&self) -> Result<String, String> {
        canonical_sha256(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedChangeControlEvidenceV1 {
    pub platform: QualificationDerivedAccessPlatformV1,
    pub case: QualificationDerivedChangeControlCaseV1,
    pub binary_kind: QualificationDerivedChangeControlBinaryKindV1,
    pub test_name: String,
    pub status: QualificationDerivedAccessStatusV1,
    pub execution_identity_sha256: String,
    pub product_identity_sha256: String,
    pub test_binary_identity_sha256: String,
    pub test_binary_sha256: String,
    pub command_sha256: String,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
    pub exit_code: i32,
    pub tests_run: u16,
    pub tests_passed: u16,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedChangeStoragePhaseV1 {
    InitialPublication,
    PostAppendCheckpoint,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedChangeStorageEvidenceV1 {
    pub platform: QualificationDerivedAccessPlatformV1,
    pub fixture: QualificationDerivedChangeFixtureV1,
    pub phase: QualificationDerivedChangeStoragePhaseV1,
    pub fixture_inventory_sha256: String,
    pub fixture_witness_sha256: String,
    pub product_identity_sha256: String,
    pub execution_identity_sha256: String,
    pub witness: QualificationDerivedStorageWitnessV1,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationDerivedChangeFixtureV1 {
    TopologyV1,
    DuplicateEqualV1,
    DuplicateConflictV1,
    RemovalV1,
    MissingCarrierV1,
    MutatedCarrierV1,
    WrongFamilyCarrierV1,
    IncompleteV1,
    CycleConflictedV1,
}

impl QualificationDerivedChangeReadCaseV1 {
    pub const ALL: [Self; 15] = [
        Self::Profile,
        Self::ChangesBare,
        Self::ChangesBounded,
        Self::AttentionBare,
        Self::AttentionBounded,
        Self::BodylessFilterSuite,
        Self::SummaryQuery,
        Self::SummaryFilterSuite,
        Self::PageTokenSuite,
        Self::ConcurrentReaders,
        Self::FreshProcessSuite,
        Self::WarmReuseSuite,
        Self::StalePageToken,
        Self::PostAppendSuite,
        Self::PostAppendFreshProcessSuite,
    ];
}

impl QualificationDerivedChangeFixtureV1 {
    const COMPLETE_READY_CASES: [QualificationDerivedChangeReadCaseV1; 8] = [
        QualificationDerivedChangeReadCaseV1::Profile,
        QualificationDerivedChangeReadCaseV1::ChangesBare,
        QualificationDerivedChangeReadCaseV1::ChangesBounded,
        QualificationDerivedChangeReadCaseV1::AttentionBare,
        QualificationDerivedChangeReadCaseV1::AttentionBounded,
        QualificationDerivedChangeReadCaseV1::BodylessFilterSuite,
        QualificationDerivedChangeReadCaseV1::SummaryQuery,
        QualificationDerivedChangeReadCaseV1::SummaryFilterSuite,
    ];
    const TYPED_FAILURE_CASES: [QualificationDerivedChangeReadCaseV1; 6] = [
        QualificationDerivedChangeReadCaseV1::Profile,
        QualificationDerivedChangeReadCaseV1::ChangesBare,
        QualificationDerivedChangeReadCaseV1::ChangesBounded,
        QualificationDerivedChangeReadCaseV1::AttentionBare,
        QualificationDerivedChangeReadCaseV1::AttentionBounded,
        QualificationDerivedChangeReadCaseV1::SummaryQuery,
    ];
    pub const ALL: [Self; 9] = [
        Self::TopologyV1,
        Self::DuplicateEqualV1,
        Self::DuplicateConflictV1,
        Self::RemovalV1,
        Self::MissingCarrierV1,
        Self::MutatedCarrierV1,
        Self::WrongFamilyCarrierV1,
        Self::IncompleteV1,
        Self::CycleConflictedV1,
    ];

    pub fn required_cases(self) -> &'static [QualificationDerivedChangeReadCaseV1] {
        use QualificationDerivedChangeReadCaseV1 as Case;
        match self {
            Self::TopologyV1 => &Case::ALL,
            Self::DuplicateConflictV1
            | Self::MissingCarrierV1
            | Self::MutatedCarrierV1
            | Self::WrongFamilyCarrierV1 => &Self::TYPED_FAILURE_CASES,
            Self::DuplicateEqualV1
            | Self::RemovalV1
            | Self::IncompleteV1
            | Self::CycleConflictedV1 => &Self::COMPLETE_READY_CASES,
        }
    }
}

pub fn qualification_derived_change_storage_probe_hashes_v1(
    fixture: QualificationDerivedChangeFixtureV1,
) -> QualificationDerivedStorageForbiddenProbeHashesV1 {
    let (proposal_summary_sha256, prose_sha256) = match fixture {
        QualificationDerivedChangeFixtureV1::TopologyV1 => (
            "21f749c5f166ae819a99a8ff0e303297a43685fd14cc7f1b86a90751989b167c",
            "da79cc8c9b04f41616275f4a6bd027acf6d0358f3605dac74ccadfeea92945a4",
        ),
        _ => (
            "c28dcb78bb4ccee57a2c6af8c1496b9fc8a14dd4860404907cc8607077ef4fc7",
            "50598e3fd911558ba8a903c07689d5128156d63db94dbcce8deda237e8bc73aa",
        ),
    };
    QualificationDerivedStorageForbiddenProbeHashesV1 {
        proposal_summary_sha256: proposal_summary_sha256.to_owned(),
        prose_sha256: prose_sha256.to_owned(),
        payload_document_sha256: "20dfd0d4e1ce81bfb753001a61c0394914d4711e84f90fb745a659dba1ff11bf"
            .to_owned(),
    }
}

fn required_change_read_rows_v1() -> impl Iterator<
    Item = (
        QualificationDerivedChangeFixtureV1,
        QualificationDerivedChangeReadCaseV1,
    ),
> {
    QualificationDerivedChangeFixtureV1::ALL
        .into_iter()
        .flat_map(|fixture| {
            fixture
                .required_cases()
                .iter()
                .copied()
                .map(move |case| (fixture, case))
        })
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedChangeReadOracleV1 {
    StrictParity,
    ReadyProfileParity,
    TypedFailure,
}

pub(crate) fn qualification_derived_change_expected_outcome_v1(
    platform: QualificationDerivedAccessPlatformV1,
    fixture: QualificationDerivedChangeFixtureV1,
    case: QualificationDerivedChangeReadCaseV1,
) -> (
    QualificationDerivedChangeReadOracleV1,
    u16,
    Option<&'static str>,
) {
    if case == QualificationDerivedChangeReadCaseV1::StalePageToken {
        return (
            QualificationDerivedChangeReadOracleV1::TypedFailure,
            409,
            Some("stale_projection"),
        );
    }
    let profile_remains_ready = case == QualificationDerivedChangeReadCaseV1::Profile
        && matches!(
            fixture,
            QualificationDerivedChangeFixtureV1::DuplicateConflictV1
        );
    let apfs_existing_carrier_profile_remains_ready = platform
        == QualificationDerivedAccessPlatformV1::MacosApfs
        && case == QualificationDerivedChangeReadCaseV1::Profile
        && matches!(
            fixture,
            QualificationDerivedChangeFixtureV1::MutatedCarrierV1
                | QualificationDerivedChangeFixtureV1::WrongFamilyCarrierV1
        );
    if apfs_existing_carrier_profile_remains_ready {
        return (
            QualificationDerivedChangeReadOracleV1::ReadyProfileParity,
            200,
            None,
        );
    }
    if profile_remains_ready {
        return (
            QualificationDerivedChangeReadOracleV1::StrictParity,
            200,
            None,
        );
    }
    match fixture {
        QualificationDerivedChangeFixtureV1::DuplicateConflictV1 => (
            QualificationDerivedChangeReadOracleV1::TypedFailure,
            503,
            Some("projection_invalid"),
        ),
        QualificationDerivedChangeFixtureV1::MutatedCarrierV1
        | QualificationDerivedChangeFixtureV1::WrongFamilyCarrierV1
            if platform == QualificationDerivedAccessPlatformV1::MacosApfs =>
        {
            (
                QualificationDerivedChangeReadOracleV1::TypedFailure,
                503,
                Some("projection_invalid"),
            )
        }
        QualificationDerivedChangeFixtureV1::MutatedCarrierV1
        | QualificationDerivedChangeFixtureV1::WrongFamilyCarrierV1 => (
            QualificationDerivedChangeReadOracleV1::TypedFailure,
            503,
            Some("projection_rebuild_required"),
        ),
        QualificationDerivedChangeFixtureV1::MissingCarrierV1 => (
            QualificationDerivedChangeReadOracleV1::TypedFailure,
            503,
            Some("projection_rebuild_required"),
        ),
        _ => (
            QualificationDerivedChangeReadOracleV1::StrictParity,
            200,
            None,
        ),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedChangeReadEvidenceV1 {
    pub platform: QualificationDerivedAccessPlatformV1,
    pub fixture: QualificationDerivedChangeFixtureV1,
    pub fixture_inventory_sha256: String,
    pub fixture_witness_sha256: String,
    pub case: QualificationDerivedChangeReadCaseV1,
    pub semantic_process_scope: QualificationDerivedAccessProcessScopeV1,
    pub counter_process_scope: QualificationDerivedAccessProcessScopeV1,
    pub product_identity_sha256: String,
    pub counter_execution_identity_sha256: String,
    pub status: QualificationDerivedAccessStatusV1,
    pub oracle: QualificationDerivedChangeReadOracleV1,
    pub strict_semantic_sha256: Option<String>,
    pub derived_semantic_sha256: String,
    pub wire_contract_matches: bool,
    pub expected_http_status: u16,
    pub observed_http_status: u16,
    pub expected_code: Option<String>,
    pub observed_code: Option<String>,
    pub expected_typed_document: Option<QualificationDerivedChangeTypedDocumentV1>,
    pub observed_typed_document: Option<QualificationDerivedChangeTypedDocumentV1>,
    pub counters: LongitudinalCountersV1,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedTimelineReadCaseV1 {
    StructuredQuerySuite,
    ExhaustiveQuerySuite,
    PageTokenSuite,
    TrustSuite,
    ProcessLifecycleSuite,
    PostAppendSuite,
}

impl QualificationDerivedTimelineReadCaseV1 {
    pub const ALL: [Self; 6] = [
        Self::StructuredQuerySuite,
        Self::ExhaustiveQuerySuite,
        Self::PageTokenSuite,
        Self::TrustSuite,
        Self::ProcessLifecycleSuite,
        Self::PostAppendSuite,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StructuredQuerySuite => "structured_query_suite",
            Self::ExhaustiveQuerySuite => "exhaustive_query_suite",
            Self::PageTokenSuite => "page_token_suite",
            Self::TrustSuite => "trust_suite",
            Self::ProcessLifecycleSuite => "process_lifecycle_suite",
            Self::PostAppendSuite => "post_append_suite",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedTimelineReadOracleV1 {
    StrictParity,
    TypedFailure,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedTimelineAuthorityEvidenceV1 {
    pub request_schedule_sha256: String,
    pub generation_identity_before_sha256: String,
    pub generation_identity_after_sha256: String,
    pub checkpoint_identity_before_sha256: String,
    pub checkpoint_identity_after_sha256: String,
    pub timeline_projection_stamp_before_sha256: String,
    pub timeline_projection_stamp_after_sha256: String,
    pub trust_identity_before_sha256: String,
    pub trust_identity_after_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_token_set_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub authoritative_event_family_counts: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub strict_event_family_counts: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub derived_event_family_counts: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub excluded_timeline_case_counts:
        BTreeMap<String, QualificationDerivedTimelineExclusionCountsV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedTimelineExclusionCountsV1 {
    pub source_count: u64,
    pub strict_output_count: u64,
    pub derived_output_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedTimelineReadEvidenceV1 {
    pub platform: QualificationDerivedAccessPlatformV1,
    pub fixture: QualificationDerivedChangeFixtureV1,
    pub fixture_inventory_sha256: String,
    pub fixture_witness_sha256: String,
    pub case: QualificationDerivedTimelineReadCaseV1,
    pub semantic_process_scope: QualificationDerivedAccessProcessScopeV1,
    pub counter_process_scope: QualificationDerivedAccessProcessScopeV1,
    pub product_identity_sha256: String,
    pub counter_execution_identity_sha256: String,
    pub status: QualificationDerivedAccessStatusV1,
    pub oracle: QualificationDerivedTimelineReadOracleV1,
    pub strict_semantic_sha256: Option<String>,
    pub derived_semantic_sha256: String,
    pub wire_contract_matches: bool,
    pub expected_typed_documents: Vec<QualificationDerivedTimelineTypedExpectationV1>,
    pub observed_typed_documents: Vec<QualificationDerivedTimelineTypedObservationV1>,
    pub authority: QualificationDerivedTimelineAuthorityEvidenceV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust_transition: Option<QualificationDerivedTimelineTrustTransitionV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrent_trust_transition: Option<QualificationDerivedTimelineConcurrentTrustEvidenceV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_signature_failure:
        Option<QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1>,
    pub counter_receipts: Vec<LongitudinalCounterReceiptV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedTimelineTypedExpectationV1 {
    pub operation: String,
    pub http_status: u16,
    pub schema: String,
    pub version: u32,
    pub code: String,
    pub retryable: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedTimelineTypedObservationV1 {
    pub operation: String,
    pub http_status: u16,
    pub document: QualificationDerivedChangeTypedDocumentV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedTimelineTrustTransitionV1 {
    pub unsigned_event_id: String,
    pub signed_event_id: String,
    pub signer_identity: String,
    pub status_before_by_event: BTreeMap<String, String>,
    pub status_after_by_event: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedTimelineConcurrentTrustEvidenceV1 {
    pub signed_event_id: String,
    pub signer_identity: String,
    pub trust_identity_before_sha256: String,
    pub trust_identity_during_sha256: String,
    pub trust_identity_restored_sha256: String,
    pub status_before: String,
    pub status_during: String,
    pub status_restored: String,
    pub observed_status_by_operation: BTreeMap<String, String>,
}

pub const QUALIFICATION_DERIVED_TIMELINE_FAULT_SEED_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-timeline-fault-seed-receipt.v1";

/// Proof that the Timeline fault authority was byte-cloned from one validated
/// canonical public-matrix materialization into a canonical-path-disjoint
/// root before any governed derived state beyond idle zero-byte writer locks
/// and before any staged lifecycle trust existed. The claim is isolation: the
/// clone shares the reference materialization by construction and makes no
/// independent fixture-reproducibility claim.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedTimelineFaultSeedReceiptV1 {
    pub schema: String,
    pub reference_root_path_sha256: String,
    pub fault_root_path_sha256: String,
    pub reference_witness_path_sha256: String,
    pub fault_witness_path_sha256: String,
    pub witness_sha256: String,
    pub tree_manifest_sha256: String,
    pub authoritative_inventory_sha256: String,
    pub inclusive_inventory_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_trust_sha256: Option<String>,
    pub cloned_file_count: u64,
    pub cloned_byte_count: u64,
    pub receipt_sha256: String,
}

impl QualificationDerivedTimelineFaultSeedReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.receipt_sha256 = String::new();
        canonical_sha256(&preimage)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_TIMELINE_FAULT_SEED_RECEIPT_SCHEMA_V1 {
            return Err("unknown Timeline fault-seed receipt schema".to_owned());
        }
        for (value, label) in [
            (
                &self.reference_root_path_sha256,
                "fault-seed reference root path",
            ),
            (&self.fault_root_path_sha256, "fault-seed fault root path"),
            (
                &self.reference_witness_path_sha256,
                "fault-seed reference witness path",
            ),
            (
                &self.fault_witness_path_sha256,
                "fault-seed fault witness path",
            ),
            (&self.witness_sha256, "fault-seed witness"),
            (&self.tree_manifest_sha256, "fault-seed tree manifest"),
            (
                &self.authoritative_inventory_sha256,
                "fault-seed authoritative inventory",
            ),
            (
                &self.inclusive_inventory_sha256,
                "fault-seed inclusive inventory",
            ),
            (&self.receipt_sha256, "fault-seed receipt"),
        ] {
            validate_hex(value, 64, label)?;
        }
        if let Some(initial_trust) = &self.initial_trust_sha256 {
            validate_hex(initial_trust, 64, "fault-seed initial trust")?;
        }
        if self.reference_root_path_sha256 == self.fault_root_path_sha256
            || self.reference_witness_path_sha256 == self.fault_witness_path_sha256
        {
            return Err("Timeline fault-seed roots are not canonical-path-disjoint".to_owned());
        }
        if self.cloned_file_count == 0 || self.cloned_byte_count == 0 {
            return Err("Timeline fault-seed clone is empty".to_owned());
        }
        if self.receipt_sha256 != self.canonical_sha256()? {
            return Err("Timeline fault-seed receipt hash drifted".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1 {
    pub fault_seed_receipt: QualificationDerivedTimelineFaultSeedReceiptV1,
    pub reference_root_identity_sha256: String,
    pub fault_execution: QualificationDerivedAccessExecutionIdentityV1,
    pub reference_fixture_witness_sha256: String,
    pub fault_fixture_witness_sha256: String,
    pub carrier_event_id: String,
    pub clean_event_record_hash: String,
    pub mutated_event_record_hash: String,
    pub reference_inventory_sha256: String,
    pub reference_recovery_inventory_sha256: String,
    pub fault_clean_inventory_sha256: String,
    pub fault_derivative_inventory_sha256: String,
    pub fault_restored_inventory_sha256: String,
    pub clean_carrier_sha256: String,
    pub mutated_carrier_sha256: String,
    pub mutation_recipe_sha256: String,
    pub clean_signature_status: String,
    pub mutated_signature_status: String,
    pub strict_observed_signature_status: String,
    pub observed_typed_document: QualificationDerivedChangeTypedDocumentV1,
    pub clean_semantic_sha256: String,
    pub strict_clean_semantic_sha256: String,
    pub strict_semantic_sha256: String,
    pub derived_semantic_sha256: String,
    pub strict_recovery_semantic_sha256: String,
    pub derived_recovery_semantic_sha256: String,
    pub recovery_signature_status: String,
    pub reference_trust_identity_staged_sha256: String,
    pub reference_trust_identity_restored_sha256: String,
    pub fault_trust_identity_staged_sha256: String,
    pub fault_trust_identity_restored_sha256: String,
    pub phase_process_identity_sha256: [String; 6],
    pub phase_http_status: [u16; 6],
    pub phase_source_change_projection_stamp: [String; 5],
    pub phase_timeline_projection_stamp: [String; 5],
    pub phase_authority_cursor_sha256: [String; 5],
    pub counter_receipt: LongitudinalCounterReceiptV1,
    pub barrier_receipt: LongitudinalTimelinePostPinBarrierReceiptV1,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedTimelineForbiddenProbeKindV1 {
    TimelineProse,
    TimelinePayload,
    TimelineResponseDocument,
    TimelineTrustResult,
    TimelineContinuationToken,
}

impl QualificationDerivedTimelineForbiddenProbeKindV1 {
    pub const ALL: [Self; 5] = [
        Self::TimelineProse,
        Self::TimelinePayload,
        Self::TimelineResponseDocument,
        Self::TimelineTrustResult,
        Self::TimelineContinuationToken,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedTimelineForbiddenProbeEvidenceV1 {
    pub kind: QualificationDerivedTimelineForbiddenProbeKindV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentinel_sha256: Option<String>,
    pub sqlite_match_count: u64,
    pub file_match_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedTimelineStorageEvidenceV1 {
    pub platform: QualificationDerivedAccessPlatformV1,
    pub fixture: QualificationDerivedChangeFixtureV1,
    pub phase: QualificationDerivedChangeStoragePhaseV1,
    pub fixture_inventory_sha256: String,
    pub fixture_witness_sha256: String,
    pub product_identity_sha256: String,
    pub execution_identity_sha256: String,
    pub forbidden_probes: Vec<QualificationDerivedTimelineForbiddenProbeEvidenceV1>,
}

pub(crate) const QUALIFICATION_TIMELINE_SOURCE_EVENT_FAMILIES_V1: [&str; 24] = [
    "review_initialized",
    "work_object_proposed",
    "review_observation_recorded",
    "review_assessment_recorded",
    "input_request_opened",
    "input_request_responded",
    "review_note_imported",
    "revision_ref_associated",
    "revision_ref_withdrawn",
    "revision_commit_associated",
    "revision_commit_withdrawn",
    "validation_check_recorded",
    "task_checkpoint_captured",
    "task_observation_recorded",
    "event_signature_recorded",
    "artifact_removed",
    "change_declared",
    "change_membership_asserted",
    "change_membership_withdrawn",
    "change_link_asserted",
    "change_revision_relation_asserted",
    "change_revision_relation_withdrawn",
    "revision_relation_attested",
    "review_fact_ported",
];

pub(crate) const QUALIFICATION_TIMELINE_ADMITTED_EVENT_FAMILIES_V1: [&str; 20] = [
    "review_initialized",
    "work_object_proposed",
    "review_observation_recorded",
    "review_assessment_recorded",
    "input_request_opened",
    "input_request_responded",
    "review_note_imported",
    "revision_ref_associated",
    "revision_ref_withdrawn",
    "revision_commit_associated",
    "revision_commit_withdrawn",
    "validation_check_recorded",
    "change_declared",
    "change_membership_asserted",
    "change_membership_withdrawn",
    "change_link_asserted",
    "change_revision_relation_asserted",
    "change_revision_relation_withdrawn",
    "revision_relation_attested",
    "review_fact_ported",
];

pub(crate) const QUALIFICATION_TIMELINE_EXCLUDED_CASES_V1: [&str; 7] = [
    "work_object_proposed_task_attempt",
    "input_request_opened_task",
    "input_request_responded_task",
    "task_checkpoint_captured",
    "task_observation_recorded",
    "event_signature_recorded",
    "artifact_removed",
];

const TIMELINE_NON_TOPOLOGY_CASES_V1: [QualificationDerivedTimelineReadCaseV1; 1] =
    [QualificationDerivedTimelineReadCaseV1::StructuredQuerySuite];

pub(crate) fn required_timeline_cases_v1(
    fixture: QualificationDerivedChangeFixtureV1,
) -> &'static [QualificationDerivedTimelineReadCaseV1] {
    if fixture == QualificationDerivedChangeFixtureV1::TopologyV1 {
        &QualificationDerivedTimelineReadCaseV1::ALL
    } else {
        &TIMELINE_NON_TOPOLOGY_CASES_V1
    }
}

const TIMELINE_STRUCTURED_TOPOLOGY_SCHEDULE_V1: [&str; 10] = [
    "timeline_all_asc",
    "timeline_all_desc",
    "timeline_type_filter",
    "timeline_track_filter",
    "timeline_change_filter",
    "timeline_exact_revision_filter",
    "timeline_facets_count_at",
    "timeline_revision_correlations",
    "timeline_withdrawal_equal_time_ordering",
    "timeline_invalid_query",
];
const TIMELINE_STRUCTURED_FAULT_SCHEDULE_V1: [&str; 1] = ["timeline_fault_outcome"];
const TIMELINE_EXHAUSTIVE_SCHEDULE_V1: [&str; 2] = [
    "timeline_exhaustive_body_search",
    "timeline_exhaustive_facets_count_window",
];
const TIMELINE_PAGE_TOKEN_SCHEDULE_V1: [&str; 5] = [
    "timeline_next",
    "timeline_previous",
    "timeline_token_query_mismatch",
    "timeline_token_direction_limit_mismatch",
    "timeline_at_token_exclusive",
];
const TIMELINE_TRUST_SCHEDULE_V1: [&str; 3] = [
    "timeline_trust_before",
    "timeline_trust_after",
    "timeline_trust_stale_token",
];
const TIMELINE_PROCESS_LIFECYCLE_SCHEDULE_V1: [&str; 5] = [
    "timeline_cold",
    "timeline_restart",
    "timeline_warm",
    "timeline_concurrent_asc",
    "timeline_concurrent_desc",
];
const TIMELINE_POST_APPEND_SCHEDULE_V1: [&str; 4] = [
    "timeline_k",
    "timeline_k_plus_one",
    "timeline_k_stale_token",
    "timeline_k_plus_one_fresh_process",
];

pub(crate) fn timeline_request_schedule_v1(
    fixture: QualificationDerivedChangeFixtureV1,
    case: QualificationDerivedTimelineReadCaseV1,
) -> &'static [&'static str] {
    if fixture != QualificationDerivedChangeFixtureV1::TopologyV1 {
        return &TIMELINE_STRUCTURED_FAULT_SCHEDULE_V1;
    }
    match case {
        QualificationDerivedTimelineReadCaseV1::StructuredQuerySuite => {
            &TIMELINE_STRUCTURED_TOPOLOGY_SCHEDULE_V1
        }
        QualificationDerivedTimelineReadCaseV1::ExhaustiveQuerySuite => {
            &TIMELINE_EXHAUSTIVE_SCHEDULE_V1
        }
        QualificationDerivedTimelineReadCaseV1::PageTokenSuite => &TIMELINE_PAGE_TOKEN_SCHEDULE_V1,
        QualificationDerivedTimelineReadCaseV1::TrustSuite => &TIMELINE_TRUST_SCHEDULE_V1,
        QualificationDerivedTimelineReadCaseV1::ProcessLifecycleSuite => {
            &TIMELINE_PROCESS_LIFECYCLE_SCHEDULE_V1
        }
        QualificationDerivedTimelineReadCaseV1::PostAppendSuite => {
            &TIMELINE_POST_APPEND_SCHEDULE_V1
        }
    }
}

pub(crate) fn expected_timeline_typed_documents_v1(
    platform: QualificationDerivedAccessPlatformV1,
    fixture: QualificationDerivedChangeFixtureV1,
    case: QualificationDerivedTimelineReadCaseV1,
) -> Vec<QualificationDerivedTimelineTypedExpectationV1> {
    timeline_request_schedule_v1(fixture, case)
        .iter()
        .filter_map(|operation| {
            let (http_status, schema, code, retryable) = match *operation {
                "timeline_invalid_query"
                | "timeline_token_query_mismatch"
                | "timeline_token_direction_limit_mismatch"
                | "timeline_at_token_exclusive" => (
                    400,
                    "pointbreak.inspect-event-history-error",
                    "invalid_query",
                    Some(false),
                ),
                "timeline_trust_stale_token" | "timeline_k_stale_token" => (
                    409,
                    "pointbreak.inspect-event-history-error",
                    "stale_projection",
                    Some(false),
                ),
                "timeline_fault_outcome" => {
                    let (oracle, status, code) = qualification_derived_change_expected_outcome_v1(
                        platform,
                        fixture,
                        QualificationDerivedChangeReadCaseV1::ChangesBare,
                    );
                    if oracle != QualificationDerivedChangeReadOracleV1::TypedFailure {
                        return None;
                    }
                    (
                        status,
                        "pointbreak.inspect-change-projection-error",
                        code.expect("typed Timeline fixture must have an error code"),
                        Some(false),
                    )
                }
                _ => return None,
            };
            Some(QualificationDerivedTimelineTypedExpectationV1 {
                operation: (*operation).to_owned(),
                http_status,
                schema: schema.to_owned(),
                version: 1,
                code: code.to_owned(),
                retryable,
            })
        })
        .collect()
}

pub(crate) fn timeline_request_schedule_sha256_v1(
    fixture: QualificationDerivedChangeFixtureV1,
    case: QualificationDerivedTimelineReadCaseV1,
) -> String {
    canonical_sha256(&timeline_request_schedule_v1(fixture, case))
        .expect("the Timeline request schedule is canonical")
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn timeline_invalid_signature_run_identity_v1(
    reference_root_identity_sha256: &str,
    fault_root_identity_sha256: &str,
    product_identity_sha256: &str,
    carrier_event_id: &str,
    carrier_key_digest: &str,
    clean_carrier_sha256: &str,
    mutated_carrier_sha256: &str,
    mutation_recipe_sha256: &str,
    barrier_identity_sha256: &str,
    mutated_derived_process_identity_sha256: &str,
) -> Result<String, String> {
    canonical_json_bytes(&serde_json::json!({
        "referenceRoot": reference_root_identity_sha256,
        "faultRoot": fault_root_identity_sha256,
        "product": product_identity_sha256,
        "fixture": QualificationDerivedChangeFixtureV1::TopologyV1,
        "case": QualificationDerivedTimelineReadCaseV1::TrustSuite,
        "operation": "timeline_invalid_signature_fault",
        "carrierEventId": carrier_event_id,
        "carrierKeyDigest": carrier_key_digest,
        "cleanCarrier": clean_carrier_sha256,
        "mutatedCarrier": mutated_carrier_sha256,
        "mutationRecipe": mutation_recipe_sha256,
        "barrier": barrier_identity_sha256,
        "derivedChild": mutated_derived_process_identity_sha256,
    }))
    .map(|bytes| sha256_bytes_hex(&bytes))
    .map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessPackageV1 {
    pub schema: String,
    pub evaluator_revision: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub evaluator_procedure_sha256: String,
    pub proposed_profile_id: String,
    pub execution_identities: Vec<QualificationDerivedAccessExecutionIdentityV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub product_identities: Vec<QualificationDerivedAccessProductIdentityV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub change_control_binary_identities: Vec<QualificationDerivedChangeControlBinaryIdentityV1>,
    pub root_bindings: Vec<QualificationDerivedAccessRootBindingV1>,
    pub d0_rows: Vec<QualificationDerivedAccessD0EvidenceV1>,
    pub operation_rows: Vec<QualificationDerivedAccessOperationEvidenceV1>,
    pub lifecycle_rows: Vec<QualificationDerivedAccessLifecycleEvidenceV1>,
    pub resources: Option<QualificationDerivedAccessResourceEvidenceV1>,
    pub allocation_rows: Vec<QualificationDerivedAccessAllocationEvidenceV1>,
    pub bootstrap_rows: Vec<QualificationDerivedAccessBootstrapEvidenceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub change_read_rows: Vec<QualificationDerivedChangeReadEvidenceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub change_control_rows: Vec<QualificationDerivedChangeControlEvidenceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub change_storage_rows: Vec<QualificationDerivedChangeStorageEvidenceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timeline_read_rows: Vec<QualificationDerivedTimelineReadEvidenceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timeline_storage_rows: Vec<QualificationDerivedTimelineStorageEvidenceV1>,
    pub complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessEvaluationV1 {
    pub schema: String,
    pub contract_sha256: String,
    pub evaluator_revision: String,
    pub proposed_profile_id: String,
    pub outcome: QualificationDerivedAccessTerminalOutcomeV1,
    pub failed_criteria: Vec<String>,
    pub missing_or_unknown_criteria: Vec<String>,
}

pub fn evaluate_qualification_derived_access_v1(
    package: &QualificationDerivedAccessPackageV1,
) -> Result<QualificationDerivedAccessEvaluationV1, String> {
    if package.schema != QUALIFICATION_DERIVED_ACCESS_PACKAGE_SCHEMA_V1
        || !matches!(
            package.evaluator_revision.as_str(),
            QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V2
                | QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3
                | QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4
        )
        || package.proposed_profile_id.trim().is_empty()
    {
        return Err("unsupported derived-access package".to_owned());
    }
    if package.evaluator_revision == QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V2
        && (!package.change_read_rows.is_empty()
            || !package.change_control_rows.is_empty()
            || !package.change_storage_rows.is_empty()
            || !package.timeline_read_rows.is_empty()
            || !package.timeline_storage_rows.is_empty()
            || !package.product_identities.is_empty()
            || !package.change_control_binary_identities.is_empty()
            || !package.evaluator_procedure_sha256.is_empty())
    {
        return Err("evaluator v2 cannot carry successor evidence".to_owned());
    }
    if package.evaluator_revision == QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3
        && package.evaluator_procedure_sha256
            != qualification_derived_access_evaluator_v3_procedure_sha256()
    {
        return Err("evaluator v3 procedure binding drifted".to_owned());
    }
    if package.evaluator_revision == QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3
        && (!package.timeline_read_rows.is_empty() || !package.timeline_storage_rows.is_empty())
    {
        return Err("evaluator v3 cannot carry Timeline successor evidence".to_owned());
    }
    if package.evaluator_revision == QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4
        && package.evaluator_procedure_sha256
            != qualification_derived_access_evaluator_v4_procedure_sha256()
    {
        return Err("evaluator v4 procedure binding drifted".to_owned());
    }
    let missing_platforms = validate_execution_identities(package)?;
    reject_duplicate_rows(package)?;

    let contract = qualification_derived_access_contract_v1();
    let mut failed = Vec::new();
    let mut missing = missing_platforms
        .into_iter()
        .map(|platform| format!("execution identity on {platform:?}"))
        .collect::<Vec<_>>();

    evaluate_d0(package, &contract, &mut failed, &mut missing)?;
    evaluate_operations(package, &contract, &mut failed, &mut missing)?;
    evaluate_lifecycle(package, &mut failed, &mut missing);
    evaluate_resources(package, &contract, &mut failed, &mut missing);
    evaluate_allocation(package, &contract, &mut failed, &mut missing);
    evaluate_bootstrap(package, &contract, &mut failed, &mut missing);
    if matches!(
        package.evaluator_revision.as_str(),
        QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3
            | QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4
    ) {
        evaluate_change_reads(package, &mut failed, &mut missing);
        evaluate_change_controls(package, &mut failed, &mut missing);
        evaluate_change_storage(package, &mut failed, &mut missing);
    }
    if package.evaluator_revision == QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4 {
        evaluate_timeline_reads_v1(package, &mut failed, &mut missing);
        evaluate_timeline_storage_v1(package, &mut failed, &mut missing);
    }
    if !package.complete {
        missing.push("completion-last package marker".to_owned());
    }

    let outcome = if !failed.is_empty() {
        QualificationDerivedAccessTerminalOutcomeV1::Reject
    } else if !missing.is_empty() {
        QualificationDerivedAccessTerminalOutcomeV1::InsufficientEvidence
    } else {
        QualificationDerivedAccessTerminalOutcomeV1::SurvivesApfsFalsifier
    };
    Ok(QualificationDerivedAccessEvaluationV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_EVALUATION_SCHEMA_V1.to_owned(),
        contract_sha256: QUALIFICATION_DERIVED_ACCESS_CONTRACT_SHA256_V1.to_owned(),
        evaluator_revision: package.evaluator_revision.clone(),
        proposed_profile_id: package.proposed_profile_id.clone(),
        outcome,
        failed_criteria: failed,
        missing_or_unknown_criteria: missing,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QualificationDerivedAccessD0SchedulePreimageV1<'a> {
    event_families: &'a [QualificationDerivedAccessEventFamilyV1],
    transitions: &'a [QualificationDerivedAccessTransitionV1],
    operations: &'a [QualificationDerivedAccessOperationV1],
    lifecycle_criteria: &'a [QualificationDerivedAccessLifecycleCriterionV1],
}

fn d0_contract() -> QualificationDerivedAccessD0V1 {
    let family_counts = [
        ("review_initialized", 1),
        ("work_object_proposed", 20),
        ("review_observation_recorded", 20),
        ("review_assessment_recorded", 12),
        ("input_request_opened", 12),
        ("input_request_responded", 8),
        ("revision_ref_associated", 6),
        ("revision_ref_withdrawn", 2),
        ("revision_commit_associated", 8),
        ("revision_commit_withdrawn", 2),
        ("validation_check_recorded", 20),
        ("task_checkpoint_captured", 6),
        ("task_observation_recorded", 6),
        ("event_signature_recorded", 4),
        ("artifact_removed", 1),
        ("review_note_imported", 0),
    ];
    debug_assert_eq!(
        family_counts.iter().map(|(_, count)| count).sum::<u16>(),
        128
    );
    let event_families = family_counts
        .into_iter()
        .map(
            |(event_type, count)| QualificationDerivedAccessEventFamilyV1 {
                event_type: event_type.to_owned(),
                count,
            },
        )
        .collect::<Vec<_>>();
    let transitions = QualificationDerivedAccessTransitionV1::ALL.to_vec();
    let operations = QualificationDerivedAccessOperationV1::ALL.to_vec();
    let lifecycle_criteria = QualificationDerivedAccessLifecycleCriterionV1::ALL.to_vec();
    let schedule_sha256 = canonical_sha256(&QualificationDerivedAccessD0SchedulePreimageV1 {
        event_families: &event_families,
        transitions: &transitions,
        operations: &operations,
        lifecycle_criteria: &lifecycle_criteria,
    })
    .expect("the D0-128 schedule is canonical");
    QualificationDerivedAccessD0V1 {
        tier: QualificationDerivedAccessTierV1::D0_128,
        stored_events: 128,
        revisions: 16,
        revision_work_object_proposals: 16,
        task_work_object_proposals: 4,
        independently_referenced_objects: 16,
        schedule_sha256,
        event_families,
        transitions,
        operations,
        lifecycle_criteria,
        independent_roots: 2,
        byte_identical_roots_required: true,
        timing_threshold_authorized: false,
    }
}

fn operation_requirement(
    operation: QualificationDerivedAccessOperationV1,
) -> QualificationDerivedAccessOperationRequirementV1 {
    use QualificationDerivedAccessOperationV1 as Operation;
    let (wall, cpu, counters, receipt) = match operation {
        Operation::SemanticId => (
            150,
            100,
            counters(0, 1, 1, 0, 0, Some(0), Some(0)),
            "exact semantic id resolves one validated authoritative carrier",
        ),
        Operation::FreshNoChange => (
            50,
            25,
            counters(0, 0, 0, 0, 0, Some(0), Some(0)),
            "unchanged truth head and derived checkpoint report fresh",
        ),
        Operation::NewCountZero => (
            50,
            25,
            counters(0, 0, 0, 0, 0, Some(0), Some(0)),
            "unchanged cursor reports exactly zero new events",
        ),
        Operation::WindowHead | Operation::WindowMiddle | Operation::WindowTail => (
            150,
            100,
            counters(0, 512, 256, 2_048, 512, Some(100), Some(1)),
            "exact 100-row chronological window equals authoritative replay",
        ),
        Operation::RevisionDetailActive | Operation::RevisionDetailRemoved => (
            250,
            175,
            counters(0, 512, 256, 2_048, 0, None, None),
            "selected revision detail and removal state equal authoritative replay",
        ),
        Operation::AppendOne => (
            250,
            200,
            counters(16, 512, 256, 2_048, 0, None, None),
            "one durable create is acknowledged once and advances exact affected state",
        ),
        Operation::PostOne => (
            500,
            400,
            counters(0, 512, 256, 2_048, 512, Some(100), Some(1)),
            "post-append views equal authoritative replay at the acknowledged head",
        ),
        Operation::Restart => (
            3_000,
            2_500,
            counters(0, 512, 256, 2_048, 512, Some(100), Some(1)),
            "fresh process reaches the exact retained checkpoint and head",
        ),
    };
    QualificationDerivedAccessOperationRequirementV1 {
        operation,
        process_scope: if operation == Operation::AppendOne {
            QualificationDerivedAccessProcessScopeV1::Driver
        } else {
            QualificationDerivedAccessProcessScopeV1::InspectorServiceChild
        },
        semantic_receipt: receipt.to_owned(),
        l100_wall_p95_ceiling_ms: wall,
        l100_process_cpu_p95_ceiling_ms: cpu,
        fixed_output: true,
        max_l100_to_c262_selected_work_ratio_milli: 1_250,
        counters,
    }
}

fn counters(
    directory_entries_walked: u64,
    carrier_opens: u64,
    event_decodes: u64,
    event_folds: u64,
    chronological_sort_items: u64,
    body_artifact_reads: Option<u64>,
    object_artifact_reads: Option<u64>,
) -> QualificationDerivedAccessCounterCeilingsV1 {
    QualificationDerivedAccessCounterCeilingsV1 {
        directory_entries_walked,
        carrier_opens,
        event_decodes,
        event_validations: event_decodes,
        event_folds,
        chronological_sort_items,
        body_artifact_reads,
        object_artifact_reads,
        projection_rebuilds: 0,
        state_rebuilds: 0,
        unselected_body_artifact_reads: 0,
        unselected_object_artifact_reads: 0,
    }
}

fn validate_execution_identities(
    package: &QualificationDerivedAccessPackageV1,
) -> Result<Vec<QualificationDerivedAccessPlatformV1>, String> {
    for identity in &package.execution_identities {
        identity.validate()?;
    }
    for identity in &package.product_identities {
        identity.validate()?;
    }
    for identity in &package.change_control_binary_identities {
        identity.validate()?;
    }
    let platforms = package
        .execution_identities
        .iter()
        .map(|identity| identity.platform)
        .collect::<BTreeSet<_>>();
    let required = BTreeSet::from([
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ]);
    let duplicated = package
        .execution_identities
        .iter()
        .enumerate()
        .any(|(index, identity)| package.execution_identities[..index].contains(identity));
    if !platforms.is_subset(&required) || duplicated {
        return Err("derived-access execution identities are duplicated or unsupported".to_owned());
    }
    let product_platforms = package
        .product_identities
        .iter()
        .map(|identity| identity.platform)
        .collect::<BTreeSet<_>>();
    if matches!(
        package.evaluator_revision.as_str(),
        QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3
            | QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4
    ) && (product_platforms.len() != package.product_identities.len()
        || !product_platforms.is_subset(&required))
    {
        return Err("derived-access product identities are duplicated or unsupported".to_owned());
    }
    let control_binaries = package
        .change_control_binary_identities
        .iter()
        .map(|identity| (identity.platform, identity.kind))
        .collect::<BTreeSet<_>>();
    let expected_control_binaries = [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ]
    .into_iter()
    .flat_map(|platform| {
        QualificationDerivedChangeControlBinaryKindV1::ALL
            .into_iter()
            .map(move |kind| (platform, kind))
    })
    .collect::<BTreeSet<_>>();
    if control_binaries.len() != package.change_control_binary_identities.len()
        || !control_binaries.is_subset(&expected_control_binaries)
    {
        return Err(
            "derived-access control binary identities are duplicated or unsupported".to_owned(),
        );
    }
    let root_bindings = package.root_bindings.iter().collect::<BTreeSet<_>>();
    if root_bindings.len() != package.root_bindings.len() {
        return Err("derived-access root bindings are duplicated".to_owned());
    }
    for binding in &package.root_bindings {
        if binding.role.trim().is_empty()
            || validate_hex(&binding.command_sha256, 64, "root-binding command SHA-256").is_err()
            || validate_hex(
                &binding.admitted_root_sha256,
                64,
                "root-binding admitted-root SHA-256",
            )
            .is_err()
            || !package.execution_identities.iter().any(|identity| {
                identity.platform == binding.platform
                    && identity.command_sha256 == binding.command_sha256
            })
        {
            return Err("derived-access root binding lacks execution authority".to_owned());
        }
    }
    if let Some(first) = package.execution_identities.first() {
        if package.execution_identities.iter().any(|identity| {
            identity.source_commit != first.source_commit
                || identity.source_tree != first.source_tree
                || identity.cargo_lock_sha256 != first.cargo_lock_sha256
                || identity.contract_schema != first.contract_schema
                || identity.contract_sha256 != first.contract_sha256
                || identity.root_provenance_sha256 != first.root_provenance_sha256
        }) {
            return Err("derived-access execution identities mix source authority".to_owned());
        }
        for platform in &platforms {
            let identities = package
                .execution_identities
                .iter()
                .filter(|identity| identity.platform == *platform)
                .collect::<Vec<_>>();
            if let Some(platform_first) = identities.first()
                && identities.iter().any(|identity| {
                    identity.binary_sha256 != platform_first.binary_sha256
                        || identity.host_identity_sha256 != platform_first.host_identity_sha256
                        || identity.operating_system != platform_first.operating_system
                        || identity.architecture != platform_first.architecture
                        || identity.filesystem != platform_first.filesystem
                })
            {
                return Err(
                    "derived-access execution identities mix native platform authority".to_owned(),
                );
            }
        }
        let apfs_host_identity = package
            .execution_identities
            .iter()
            .find(|identity| identity.platform == QualificationDerivedAccessPlatformV1::MacosApfs)
            .map(|identity| &identity.host_identity_sha256);
        let ntfs_host_identity = package
            .execution_identities
            .iter()
            .find(|identity| identity.platform == QualificationDerivedAccessPlatformV1::WindowsNtfs)
            .map(|identity| &identity.host_identity_sha256);
        if let (Some(apfs_host_identity), Some(ntfs_host_identity)) =
            (apfs_host_identity, ntfs_host_identity)
            && apfs_host_identity == ntfs_host_identity
        {
            return Err(
                "derived-access execution identities reuse one campaign-host authority".to_owned(),
            );
        }
        for product in &package.product_identities {
            let Some(execution) = package
                .execution_identities
                .iter()
                .find(|execution| execution.platform == product.platform)
            else {
                return Err("derived-access product identity lacks execution authority".to_owned());
            };
            if !product.is_exact_source_for(execution) {
                return Err(
                    "derived-access product identity differs from harness source".to_owned(),
                );
            }
        }
        for control in &package.change_control_binary_identities {
            let Some(execution) = package
                .execution_identities
                .iter()
                .find(|execution| execution.platform == control.platform)
            else {
                return Err("control binary identity lacks execution authority".to_owned());
            };
            if !control.is_exact_source_for(execution) {
                return Err("control binary differs from harness source".to_owned());
            }
        }
    }
    Ok(required.difference(&platforms).copied().collect())
}

fn reject_duplicate_rows(package: &QualificationDerivedAccessPackageV1) -> Result<(), String> {
    let d0 = package
        .d0_rows
        .iter()
        .map(|row| row.platform)
        .collect::<BTreeSet<_>>();
    let expected_d0 = BTreeSet::from([
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ]);
    if d0.len() != package.d0_rows.len() || !d0.is_subset(&expected_d0) {
        return Err("duplicate or unsupported D0 row".to_owned());
    }
    let operations = package
        .operation_rows
        .iter()
        .map(|row| (row.platform, row.tier, row.operation))
        .collect::<BTreeSet<_>>();
    let mut expected_operations = BTreeSet::new();
    for (platform, tiers) in [
        (
            QualificationDerivedAccessPlatformV1::MacosApfs,
            QualificationDerivedAccessTierV1::ALL.as_slice(),
        ),
        (
            QualificationDerivedAccessPlatformV1::WindowsNtfs,
            QualificationDerivedAccessTierV1::NATIVE.as_slice(),
        ),
    ] {
        for &tier in tiers {
            for operation in QualificationDerivedAccessOperationV1::ALL {
                expected_operations.insert((platform, tier, operation));
            }
        }
    }
    if operations.len() != package.operation_rows.len()
        || !operations.is_subset(&expected_operations)
    {
        return Err("duplicate or unsupported operation row".to_owned());
    }
    let lifecycle = package
        .lifecycle_rows
        .iter()
        .map(|row| (row.platform, row.tier, row.criterion))
        .collect::<BTreeSet<_>>();
    let mut expected_lifecycle = BTreeSet::new();
    for platform in [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ] {
        for tier in QualificationDerivedAccessTierV1::NATIVE {
            for criterion in QualificationDerivedAccessLifecycleCriterionV1::ALL {
                expected_lifecycle.insert((platform, tier, criterion));
            }
        }
    }
    if lifecycle.len() != package.lifecycle_rows.len() || !lifecycle.is_subset(&expected_lifecycle)
    {
        return Err("duplicate or unsupported lifecycle row".to_owned());
    }
    let allocations = package
        .allocation_rows
        .iter()
        .map(|row| row.tier)
        .collect::<BTreeSet<_>>();
    let expected_allocations = BTreeSet::from([
        QualificationDerivedAccessTierV1::L100,
        QualificationDerivedAccessTierV1::C262,
    ]);
    if allocations.len() != package.allocation_rows.len()
        || !allocations.is_subset(&expected_allocations)
    {
        return Err("duplicate or unsupported allocation row".to_owned());
    }
    let bootstrap = package
        .bootstrap_rows
        .iter()
        .map(|row| row.tier)
        .collect::<BTreeSet<_>>();
    let expected_bootstrap = BTreeSet::from([
        QualificationDerivedAccessTierV1::L100,
        QualificationDerivedAccessTierV1::C262,
    ]);
    if bootstrap.len() != package.bootstrap_rows.len() || !bootstrap.is_subset(&expected_bootstrap)
    {
        return Err("duplicate or unsupported bootstrap row".to_owned());
    }
    let change_reads = package
        .change_read_rows
        .iter()
        .map(|row| (row.platform, row.fixture, row.case))
        .collect::<BTreeSet<_>>();
    let expected_change_reads = [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ]
    .into_iter()
    .flat_map(|platform| {
        required_change_read_rows_v1().map(move |(fixture, case)| (platform, fixture, case))
    })
    .collect::<BTreeSet<_>>();
    if change_reads.len() != package.change_read_rows.len()
        || !change_reads.is_subset(&expected_change_reads)
    {
        return Err("duplicate or unsupported Change read row".to_owned());
    }
    let change_controls = package
        .change_control_rows
        .iter()
        .map(|row| (row.platform, row.case))
        .collect::<BTreeSet<_>>();
    let expected_change_controls = [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ]
    .into_iter()
    .flat_map(|platform| {
        QualificationDerivedChangeControlCaseV1::ALL
            .into_iter()
            .map(move |case| (platform, case))
    })
    .collect::<BTreeSet<_>>();
    if change_controls.len() != package.change_control_rows.len()
        || !change_controls.is_subset(&expected_change_controls)
    {
        return Err("duplicate or unsupported Change control row".to_owned());
    }
    let change_storage = package
        .change_storage_rows
        .iter()
        .map(|row| (row.platform, row.fixture, row.phase))
        .collect::<BTreeSet<_>>();
    let expected_change_storage = [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ]
    .into_iter()
    .flat_map(|platform| {
        QualificationDerivedChangeFixtureV1::ALL
            .into_iter()
            .map(move |fixture| {
                (
                    platform,
                    fixture,
                    QualificationDerivedChangeStoragePhaseV1::InitialPublication,
                )
            })
            .chain(std::iter::once((
                platform,
                QualificationDerivedChangeFixtureV1::TopologyV1,
                QualificationDerivedChangeStoragePhaseV1::PostAppendCheckpoint,
            )))
    })
    .collect::<BTreeSet<_>>();
    if change_storage.len() != package.change_storage_rows.len()
        || !change_storage.is_subset(&expected_change_storage)
    {
        return Err("duplicate or unsupported Change storage row".to_owned());
    }
    let timeline_reads = package
        .timeline_read_rows
        .iter()
        .map(|row| (row.platform, row.fixture, row.case))
        .collect::<BTreeSet<_>>();
    let expected_timeline_reads = [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ]
    .into_iter()
    .flat_map(|platform| {
        QualificationDerivedChangeFixtureV1::ALL
            .into_iter()
            .flat_map(move |fixture| {
                required_timeline_cases_v1(fixture)
                    .iter()
                    .copied()
                    .map(move |case| (platform, fixture, case))
            })
    })
    .collect::<BTreeSet<_>>();
    if timeline_reads.len() != package.timeline_read_rows.len()
        || !timeline_reads.is_subset(&expected_timeline_reads)
    {
        return Err("duplicate or unsupported Timeline read row".to_owned());
    }
    let timeline_storage = package
        .timeline_storage_rows
        .iter()
        .map(|row| (row.platform, row.fixture, row.phase))
        .collect::<BTreeSet<_>>();
    if timeline_storage.len() != package.timeline_storage_rows.len()
        || !timeline_storage.is_subset(&expected_change_storage)
    {
        return Err("duplicate or unsupported Timeline storage row".to_owned());
    }
    Ok(())
}

fn evaluate_d0(
    package: &QualificationDerivedAccessPackageV1,
    contract: &QualificationDerivedAccessContractV1,
    failed: &mut Vec<String>,
    missing: &mut Vec<String>,
) -> Result<(), String> {
    let mut ordered_schedule_sha256 = None;
    for platform in [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ] {
        let Some(row) = package.d0_rows.iter().find(|row| row.platform == platform) else {
            missing.push(format!("D0-128 root identity on {platform:?}"));
            continue;
        };
        validate_hex(&row.root_a_sha256, 64, "D0 root A SHA-256")?;
        validate_hex(&row.root_b_sha256, 64, "D0 root B SHA-256")?;
        validate_hex(&row.schedule_sha256, 64, "D0 schedule SHA-256")?;
        validate_hex(
            &row.ordered_schedule_sha256,
            64,
            "D0 ordered schedule SHA-256",
        )?;
        if ordered_schedule_sha256
            .as_ref()
            .is_some_and(|expected| expected != &row.ordered_schedule_sha256)
        {
            failed.push("D0-128 ordered schedule identity".to_owned());
        } else {
            ordered_schedule_sha256 = Some(row.ordered_schedule_sha256.clone());
        }
        if row.stored_events != contract.d0.stored_events
            || row.revisions != contract.d0.revisions
            || row.independently_referenced_objects != contract.d0.independently_referenced_objects
            || row.schedule_sha256 != contract.d0.schedule_sha256
            || !row.byte_identical
            || row.root_a_sha256 != row.root_b_sha256
        {
            failed.push(format!("D0-128 root identity on {platform:?}"));
        }
    }
    Ok(())
}

fn evaluate_operations(
    package: &QualificationDerivedAccessPackageV1,
    contract: &QualificationDerivedAccessContractV1,
    failed: &mut Vec<String>,
    missing: &mut Vec<String>,
) -> Result<(), String> {
    let expected_subjects = [
        (
            QualificationDerivedAccessPlatformV1::MacosApfs,
            QualificationDerivedAccessTierV1::D0_128,
        ),
        (
            QualificationDerivedAccessPlatformV1::WindowsNtfs,
            QualificationDerivedAccessTierV1::D0_128,
        ),
        (
            QualificationDerivedAccessPlatformV1::MacosApfs,
            QualificationDerivedAccessTierV1::L1,
        ),
        (
            QualificationDerivedAccessPlatformV1::WindowsNtfs,
            QualificationDerivedAccessTierV1::L1,
        ),
        (
            QualificationDerivedAccessPlatformV1::MacosApfs,
            QualificationDerivedAccessTierV1::L7,
        ),
        (
            QualificationDerivedAccessPlatformV1::WindowsNtfs,
            QualificationDerivedAccessTierV1::L7,
        ),
        (
            QualificationDerivedAccessPlatformV1::MacosApfs,
            QualificationDerivedAccessTierV1::L100,
        ),
        (
            QualificationDerivedAccessPlatformV1::MacosApfs,
            QualificationDerivedAccessTierV1::C262,
        ),
    ];
    for (platform, tier) in expected_subjects {
        for requirement in &contract.operations {
            let Some(row) = package.operation_rows.iter().find(|row| {
                row.platform == platform
                    && row.tier == tier
                    && row.operation == requirement.operation
            }) else {
                missing.push(format!(
                    "{platform:?}/{tier:?}/{}",
                    requirement.operation.as_str()
                ));
                continue;
            };
            if row.status == QualificationDerivedAccessStatusV1::Unknown {
                missing.push(format!(
                    "{platform:?}/{tier:?}/{}",
                    requirement.operation.as_str()
                ));
                continue;
            }
            if row.status == QualificationDerivedAccessStatusV1::Failed
                || !row.semantic_receipt_matches
                || row.process_scope != requirement.process_scope
                || row.complexity != QualificationDerivedAccessComplexityV1::BoundedSelectedWork
                || row.selected_output_count == Some(0)
                || row
                    .selected_output_count
                    .is_some_and(|selected_output| selected_output > row.selected_work_count)
                || row
                    .unselected_work_count
                    .is_some_and(|unselected_work| unselected_work > row.selected_work_count)
                || row.selected_work_count < minimum_observed_work(&row.counters)
                || row.retained_cardinality
                    != expected_retained_cardinality(tier, requirement.operation, contract)
                || counters_exceed(&row.counters, &requirement.counters)
            {
                failed.push(format!(
                    "{platform:?}/{tier:?}/{}",
                    requirement.operation.as_str()
                ));
                continue;
            }
            let Some(_selected_output_count) = row.selected_output_count else {
                missing.push(format!(
                    "{platform:?}/{tier:?}/{} selected-output count",
                    requirement.operation.as_str()
                ));
                continue;
            };
            let Some(unselected_work_count) = row.unselected_work_count else {
                missing.push(format!(
                    "{platform:?}/{tier:?}/{} unselected-work count",
                    requirement.operation.as_str()
                ));
                continue;
            };
            let required_samples = match tier {
                QualificationDerivedAccessTierV1::D0_128
                | QualificationDerivedAccessTierV1::L1
                | QualificationDerivedAccessTierV1::L7 => {
                    u16::from(contract.sampling.counting_samples_per_operation_and_tier)
                }
                QualificationDerivedAccessTierV1::L100 | QualificationDerivedAccessTierV1::C262
                    if requirement.operation == QualificationDerivedAccessOperationV1::Restart =>
                {
                    u16::from(contract.sampling.release_roots)
                        * u16::from(contract.sampling.restart_samples_per_root)
                }
                QualificationDerivedAccessTierV1::L100 | QualificationDerivedAccessTierV1::C262 => {
                    u16::from(contract.sampling.release_roots)
                        * u16::from(
                            contract
                                .sampling
                                .retained_samples_per_warm_operation_per_root,
                        )
                }
            };
            if row.retained_samples != required_samples {
                failed.push(format!(
                    "{platform:?}/{tier:?}/{} sample count",
                    requirement.operation.as_str()
                ));
            }
            if tier == QualificationDerivedAccessTierV1::L100 {
                match (row.wall_p95_ms, row.process_cpu_p95_ms) {
                    (Some(wall), Some(cpu))
                        if wall <= requirement.l100_wall_p95_ceiling_ms
                            && cpu <= requirement.l100_process_cpu_p95_ceiling_ms => {}
                    (Some(_), Some(_)) => failed.push(format!(
                        "{platform:?}/{tier:?}/{} latency or CPU",
                        requirement.operation.as_str()
                    )),
                    _ => missing.push(format!(
                        "{platform:?}/{tier:?}/{} latency or CPU",
                        requirement.operation.as_str()
                    )),
                }
            }
            if tier == QualificationDerivedAccessTierV1::C262 {
                let l100 = package
                    .operation_rows
                    .iter()
                    .find(|candidate| {
                        candidate.platform == platform
                            && candidate.tier == QualificationDerivedAccessTierV1::L100
                            && candidate.operation == requirement.operation
                    })
                    .ok_or_else(|| {
                        format!(
                            "C262 {} has no L100 selected-work authority",
                            requirement.operation.as_str()
                        )
                    })?;
                let derived_ratio =
                    selected_work_ratio_milli(row.selected_work_count, l100.selected_work_count);
                let Some(l100_unselected_work_count) = l100.unselected_work_count else {
                    missing.push(format!(
                        "{platform:?}/L100/{} unselected-work count",
                        requirement.operation.as_str()
                    ));
                    continue;
                };
                match row.l100_to_c262_selected_work_ratio_milli {
                    None => missing.push(format!(
                        "{platform:?}/{tier:?}/{} selected-work ratio",
                        requirement.operation.as_str()
                    )),
                    Some(recorded) if recorded != derived_ratio => {
                        return Err(format!(
                            "C262 {} selected-work ratio is not derived from L100",
                            requirement.operation.as_str()
                        ));
                    }
                    Some(_)
                        if selected_work_growth_exceeds_bound(
                            l100.selected_work_count,
                            l100_unselected_work_count,
                            row.selected_work_count,
                            unselected_work_count,
                            requirement.max_l100_to_c262_selected_work_ratio_milli,
                        ) =>
                    {
                        failed.push(format!(
                            "{platform:?}/{tier:?}/{} selected-work ratio",
                            requirement.operation.as_str()
                        ));
                    }
                    Some(_) => {}
                }
            }
        }
    }
    Ok(())
}

fn expected_retained_cardinality(
    tier: QualificationDerivedAccessTierV1,
    operation: QualificationDerivedAccessOperationV1,
    contract: &QualificationDerivedAccessContractV1,
) -> u64 {
    let baseline = match tier {
        QualificationDerivedAccessTierV1::D0_128 => u64::from(contract.d0.stored_events),
        QualificationDerivedAccessTierV1::L1 => 1_024,
        QualificationDerivedAccessTierV1::L7 => 7_168,
        QualificationDerivedAccessTierV1::L100 => contract.scale_profiles.l100_event_count,
        QualificationDerivedAccessTierV1::C262 => contract.scale_profiles.c262_event_count,
    };
    if matches!(
        tier,
        QualificationDerivedAccessTierV1::L100 | QualificationDerivedAccessTierV1::C262
    ) && matches!(
        operation,
        QualificationDerivedAccessOperationV1::AppendOne
            | QualificationDerivedAccessOperationV1::PostOne
            | QualificationDerivedAccessOperationV1::Restart
    ) {
        baseline + u64::from(contract.sampling.append_post_pairs_per_root)
    } else {
        baseline
    }
}

fn minimum_observed_work(counters: &QualificationDerivedAccessCountersV1) -> u64 {
    [
        counters.directory_entries_walked,
        counters.carrier_opens,
        counters.event_decodes,
        counters.event_validations,
        counters.event_folds,
        counters.chronological_sort_items,
        counters.projection_rebuilds,
        counters.state_rebuilds,
    ]
    .into_iter()
    .max()
    .unwrap_or_default()
}

fn selected_work_ratio_milli(selected_work: u64, baseline_work: u64) -> u16 {
    selected_work
        .saturating_mul(1_000)
        .checked_div(baseline_work.max(1))
        .unwrap_or(u64::MAX)
        .min(u64::from(u16::MAX)) as u16
}

fn selected_work_growth_exceeds_bound(
    l100_work: u64,
    l100_unselected_work: u64,
    c262_work: u64,
    c262_unselected_work: u64,
    maximum_ratio_milli: u16,
) -> bool {
    if selected_work_ratio_milli(c262_work, l100_work) <= maximum_ratio_milli {
        return false;
    }
    c262_unselected_work > l100_unselected_work
}

fn counters_exceed(
    observed: &QualificationDerivedAccessCountersV1,
    limits: &QualificationDerivedAccessCounterCeilingsV1,
) -> bool {
    observed.directory_entries_walked > limits.directory_entries_walked
        || observed.carrier_opens > limits.carrier_opens
        || observed.event_decodes > limits.event_decodes
        || observed.event_validations > limits.event_validations
        || observed.event_folds > limits.event_folds
        || observed.chronological_sort_items > limits.chronological_sort_items
        || limits
            .body_artifact_reads
            .is_some_and(|limit| observed.body_artifact_reads > limit)
        || limits
            .object_artifact_reads
            .is_some_and(|limit| observed.object_artifact_reads > limit)
        || observed.projection_rebuilds > limits.projection_rebuilds
        || observed.state_rebuilds > limits.state_rebuilds
        || observed.unselected_body_artifact_reads > limits.unselected_body_artifact_reads
        || observed.unselected_object_artifact_reads > limits.unselected_object_artifact_reads
}

fn evaluate_lifecycle(
    package: &QualificationDerivedAccessPackageV1,
    failed: &mut Vec<String>,
    missing: &mut Vec<String>,
) {
    for platform in [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ] {
        for tier in QualificationDerivedAccessTierV1::NATIVE {
            for criterion in QualificationDerivedAccessLifecycleCriterionV1::ALL {
                let Some(row) = package.lifecycle_rows.iter().find(|row| {
                    row.platform == platform && row.tier == tier && row.criterion == criterion
                }) else {
                    missing.push(format!("{platform:?}/{tier:?}/{criterion:?}"));
                    continue;
                };
                match row.status {
                    QualificationDerivedAccessStatusV1::Passed => {}
                    QualificationDerivedAccessStatusV1::Failed => {
                        failed.push(format!("{platform:?}/{tier:?}/{criterion:?}"));
                    }
                    QualificationDerivedAccessStatusV1::Unknown => {
                        missing.push(format!("{platform:?}/{tier:?}/{criterion:?}"));
                    }
                }
            }
        }
    }
}

fn evaluate_resources(
    package: &QualificationDerivedAccessPackageV1,
    contract: &QualificationDerivedAccessContractV1,
    failed: &mut Vec<String>,
    missing: &mut Vec<String>,
) {
    let Some(resources) = &package.resources else {
        missing.push("resource inventory".to_owned());
        return;
    };
    if resources.l100_steady_rss_bytes > contract.memory.l100_steady_rss_bytes
        || resources.l100_peak_rss_bytes > contract.memory.l100_peak_rss_bytes
        || resources.l7_to_l100_steady_slope_bytes_per_event
            > contract.memory.l7_to_l100_steady_slope_bytes_per_event
        || resources.retained_body_object_bytes_outside_active_window
            != contract
                .memory
                .retained_body_object_bytes_outside_active_window
    {
        failed.push("resource inventory".to_owned());
    }
}

fn evaluate_allocation(
    package: &QualificationDerivedAccessPackageV1,
    contract: &QualificationDerivedAccessContractV1,
    failed: &mut Vec<String>,
    missing: &mut Vec<String>,
) {
    for (tier, expected_event_count) in [
        (
            QualificationDerivedAccessTierV1::L100,
            contract.scale_profiles.l100_event_count,
        ),
        (
            QualificationDerivedAccessTierV1::C262,
            contract.scale_profiles.c262_event_count,
        ),
    ] {
        let Some(row) = package.allocation_rows.iter().find(|row| row.tier == tier) else {
            missing.push(format!("{tier:?} allocation inventory"));
            continue;
        };
        let steady_ceiling = contract
            .allocation
            .steady_fixed_floor_bytes
            .max(contract.allocation.steady_bytes_per_event * expected_event_count);
        let bootstrap_high_water = package
            .bootstrap_rows
            .iter()
            .find(|bootstrap| bootstrap.tier == tier)
            .map(|bootstrap| bootstrap.high_water_derived_bytes)
            .unwrap_or_default();
        if row.event_count != expected_event_count
            || row.steady_derived_bytes == 0
            || row.steady_derived_bytes > steady_ceiling
            || row
                .high_water_derived_bytes
                .max(bootstrap_high_water)
                .saturating_mul(1_000)
                > row
                    .steady_derived_bytes
                    .saturating_mul(u64::from(contract.allocation.high_water_ratio_milli))
            || row.append_write_amplification_ratio_milli
                > contract.allocation.append_write_amplification_ratio_milli
        {
            failed.push(format!("{tier:?} allocation inventory"));
        }
    }
}

fn evaluate_bootstrap(
    package: &QualificationDerivedAccessPackageV1,
    contract: &QualificationDerivedAccessContractV1,
    failed: &mut Vec<String>,
    missing: &mut Vec<String>,
) {
    for (tier, ceiling) in [
        (
            QualificationDerivedAccessTierV1::L100,
            contract.bootstrap.l100_ceiling_seconds,
        ),
        (
            QualificationDerivedAccessTierV1::C262,
            contract.bootstrap.c262_ceiling_seconds,
        ),
    ] {
        let Some(row) = package.bootstrap_rows.iter().find(|row| row.tier == tier) else {
            missing.push(format!("{tier:?} bootstrap"));
            continue;
        };
        match row.status {
            QualificationDerivedAccessStatusV1::Unknown => {
                missing.push(format!("{tier:?} bootstrap"));
            }
            QualificationDerivedAccessStatusV1::Failed => {
                failed.push(format!("{tier:?} bootstrap"));
            }
            QualificationDerivedAccessStatusV1::Passed
                if row.elapsed_seconds > ceiling || !row.progress_reported =>
            {
                failed.push(format!("{tier:?} bootstrap"));
            }
            QualificationDerivedAccessStatusV1::Passed => {}
        }
    }
}

fn evaluate_change_reads(
    package: &QualificationDerivedAccessPackageV1,
    failed: &mut Vec<String>,
    missing: &mut Vec<String>,
) {
    for platform in [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ] {
        if !package
            .product_identities
            .iter()
            .any(|identity| identity.platform == platform)
        {
            missing.push(format!("exact product identity on {platform:?}"));
        }
    }
    if package.change_read_rows.is_empty() {
        missing.push("Change read matrix".to_owned());
        return;
    }
    for fixture in QualificationDerivedChangeFixtureV1::ALL {
        let authorities = package
            .change_read_rows
            .iter()
            .filter(|row| row.fixture == fixture)
            .map(|row| {
                (
                    row.fixture_inventory_sha256.as_str(),
                    row.fixture_witness_sha256.as_str(),
                )
            })
            .collect::<BTreeSet<_>>();
        if authorities.len() > 1 {
            failed.push(format!("{fixture:?} cross-platform fixture authority"));
        }
    }
    for platform in [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ] {
        for (fixture, case) in required_change_read_rows_v1() {
            let criterion = format!("{platform:?}/{fixture:?}/{case:?} Change read");
            let Some(row) = package
                .change_read_rows
                .iter()
                .find(|row| row.platform == platform && row.fixture == fixture && row.case == case)
            else {
                missing.push(criterion);
                continue;
            };
            let product_identity_sha256 = package
                .product_identities
                .iter()
                .find(|identity| identity.platform == platform)
                .and_then(|identity| identity.canonical_sha256().ok());
            let execution_identity_sha256 = package
                .execution_identities
                .iter()
                .find(|identity| identity.platform == platform)
                .and_then(|identity| identity.canonical_sha256().ok());
            if product_identity_sha256.as_deref() != Some(row.product_identity_sha256.as_str())
                || execution_identity_sha256.as_deref()
                    != Some(row.counter_execution_identity_sha256.as_str())
            {
                failed.push(format!(
                    "{platform:?}/{fixture:?}/{case:?} source authority"
                ));
            }
            match row.status {
                QualificationDerivedAccessStatusV1::Unknown => missing.push(criterion),
                QualificationDerivedAccessStatusV1::Failed => failed.push(criterion),
                QualificationDerivedAccessStatusV1::Passed if change_read_row_failed(row) => {
                    failed.push(criterion);
                }
                QualificationDerivedAccessStatusV1::Passed => {}
            }
        }
        for fixture in QualificationDerivedChangeFixtureV1::ALL {
            for (index, case) in fixture.required_cases().iter().copied().enumerate() {
                let Some(row) = package.change_read_rows.iter().find(|row| {
                    row.platform == platform && row.fixture == fixture && row.case == case
                }) else {
                    continue;
                };
                if matches!(
                    case,
                    QualificationDerivedChangeReadCaseV1::StalePageToken
                        | QualificationDerivedChangeReadCaseV1::PostAppendSuite
                ) {
                    continue;
                }
                let expected_capability_opens = if index == 0
                    || matches!(
                        case,
                        QualificationDerivedChangeReadCaseV1::FreshProcessSuite
                            | QualificationDerivedChangeReadCaseV1::PostAppendFreshProcessSuite
                    ) {
                    2
                } else {
                    0
                };
                if row.counters.change_capability_carriers_opened != expected_capability_opens {
                    failed.push(format!(
                        "{platform:?}/{fixture:?}/{case:?} Change capability cache"
                    ));
                }
            }
        }
    }
}

fn evaluate_change_controls(
    package: &QualificationDerivedAccessPackageV1,
    failed: &mut Vec<String>,
    missing: &mut Vec<String>,
) {
    for platform in [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ] {
        let product = package
            .product_identities
            .iter()
            .find(|identity| identity.platform == platform);
        let product_identity_sha256 = product.and_then(|identity| identity.canonical_sha256().ok());
        let execution_identity_sha256 = package
            .execution_identities
            .iter()
            .find(|identity| identity.platform == platform)
            .and_then(|identity| identity.canonical_sha256().ok());
        for kind in QualificationDerivedChangeControlBinaryKindV1::ALL {
            if !package
                .change_control_binary_identities
                .iter()
                .any(|identity| identity.platform == platform && identity.kind == kind)
            {
                missing.push(format!("{platform:?}/{kind:?} Change control binary"));
            }
        }
        for case in QualificationDerivedChangeControlCaseV1::ALL {
            let criterion = format!("{platform:?}/{case:?} Change control");
            let Some(row) = package
                .change_control_rows
                .iter()
                .find(|row| row.platform == platform && row.case == case)
            else {
                missing.push(criterion);
                continue;
            };
            let digests_valid = [
                (&row.test_binary_sha256, "Change control test binary"),
                (&row.command_sha256, "Change control command"),
                (&row.stdout_sha256, "Change control stdout"),
                (&row.stderr_sha256, "Change control stderr"),
            ]
            .into_iter()
            .all(|(value, label)| validate_hex(value, 64, label).is_ok());
            let (expected_kind, expected_test_name) =
                qualification_derived_change_control_test_v1(case);
            let binary_identity = package
                .change_control_binary_identities
                .iter()
                .find(|identity| identity.platform == platform && identity.kind == expected_kind);
            let binary_identity_sha256 =
                binary_identity.and_then(|identity| identity.canonical_sha256().ok());
            match row.status {
                QualificationDerivedAccessStatusV1::Unknown => missing.push(criterion),
                QualificationDerivedAccessStatusV1::Failed => failed.push(criterion),
                QualificationDerivedAccessStatusV1::Passed
                    if row.exit_code != 0
                        || !digests_valid
                        || row.binary_kind != expected_kind
                        || row.test_name != expected_test_name
                        || row.command_sha256
                            != qualification_derived_change_control_command_sha256_v1(
                                expected_test_name,
                            )
                        || row.tests_run != 1
                        || row.tests_passed != 1
                        || binary_identity.is_none()
                        || binary_identity_sha256.as_deref()
                            != Some(row.test_binary_identity_sha256.as_str())
                        || binary_identity.is_some_and(|identity| {
                            identity.binary_sha256 != row.test_binary_sha256
                        })
                        || product_identity_sha256.as_deref()
                            != Some(row.product_identity_sha256.as_str())
                        || execution_identity_sha256.as_deref()
                            != Some(row.execution_identity_sha256.as_str()) =>
                {
                    failed.push(criterion);
                }
                QualificationDerivedAccessStatusV1::Passed => {}
            }
        }
    }
}

fn evaluate_change_storage(
    package: &QualificationDerivedAccessPackageV1,
    failed: &mut Vec<String>,
    missing: &mut Vec<String>,
) {
    for platform in [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ] {
        let product_identity_sha256 = package
            .product_identities
            .iter()
            .find(|identity| identity.platform == platform)
            .and_then(|identity| identity.canonical_sha256().ok());
        let execution_identity_sha256 = package
            .execution_identities
            .iter()
            .find(|identity| identity.platform == platform)
            .and_then(|identity| identity.canonical_sha256().ok());
        for fixture in QualificationDerivedChangeFixtureV1::ALL {
            let phases: &[QualificationDerivedChangeStoragePhaseV1] =
                if fixture == QualificationDerivedChangeFixtureV1::TopologyV1 {
                    &[
                        QualificationDerivedChangeStoragePhaseV1::InitialPublication,
                        QualificationDerivedChangeStoragePhaseV1::PostAppendCheckpoint,
                    ]
                } else {
                    &[QualificationDerivedChangeStoragePhaseV1::InitialPublication]
                };
            for &phase in phases {
                let criterion = format!("{platform:?}/{fixture:?}/{phase:?} Change storage");
                let Some(row) = package.change_storage_rows.iter().find(|row| {
                    row.platform == platform && row.fixture == fixture && row.phase == phase
                }) else {
                    missing.push(criterion);
                    continue;
                };
                let expected_fixture_probes =
                    qualification_derived_change_storage_probe_hashes_v1(fixture);
                let observed_fixture_probes = row
                    .witness
                    .forbidden_probes
                    .iter()
                    .filter_map(|probe| match probe.kind {
                        QualificationDerivedStorageForbiddenProbeKindV1::ProposalSummary
                        | QualificationDerivedStorageForbiddenProbeKindV1::Prose
                        | QualificationDerivedStorageForbiddenProbeKindV1::PayloadDocument => {
                            Some((probe.kind, probe.sentinel_sha256.clone()))
                        }
                        QualificationDerivedStorageForbiddenProbeKindV1::FixturePrivatePath
                        | QualificationDerivedStorageForbiddenProbeKindV1::StoreRootPath => None,
                    })
                    .collect::<BTreeSet<_>>();
                let required_fixture_probes = [
                    (
                        QualificationDerivedStorageForbiddenProbeKindV1::ProposalSummary,
                        expected_fixture_probes.proposal_summary_sha256,
                    ),
                    (
                        QualificationDerivedStorageForbiddenProbeKindV1::Prose,
                        expected_fixture_probes.prose_sha256,
                    ),
                    (
                        QualificationDerivedStorageForbiddenProbeKindV1::PayloadDocument,
                        expected_fixture_probes.payload_document_sha256,
                    ),
                ]
                .into_iter()
                .collect::<BTreeSet<_>>();
                let forbidden_search_schema =
                    row.witness.sqlite_catalog.entries.iter().any(|entry| {
                        forbidden_bodyless_storage_name_v1(&entry.name)
                            || entry
                                .columns
                                .iter()
                                .any(|column| forbidden_bodyless_storage_name_v1(&column.name))
                            || entry.indexes.iter().any(|index| {
                                forbidden_bodyless_storage_name_v1(&index.name)
                                    || index.columns.iter().any(|column| {
                                        column
                                            .name
                                            .as_deref()
                                            .is_some_and(forbidden_bodyless_storage_name_v1)
                                    })
                            })
                    });
                if row.witness.validate().is_err()
                    || observed_fixture_probes != required_fixture_probes
                    || forbidden_search_schema
                    || product_identity_sha256.as_deref()
                        != Some(row.product_identity_sha256.as_str())
                    || execution_identity_sha256.as_deref()
                        != Some(row.execution_identity_sha256.as_str())
                {
                    failed.push(criterion);
                }
            }
            let initial = package.change_storage_rows.iter().find(|row| {
                row.platform == platform
                    && row.fixture == fixture
                    && row.phase == QualificationDerivedChangeStoragePhaseV1::InitialPublication
            });
            let counterpart_platform = match platform {
                QualificationDerivedAccessPlatformV1::MacosApfs => {
                    QualificationDerivedAccessPlatformV1::WindowsNtfs
                }
                QualificationDerivedAccessPlatformV1::WindowsNtfs => {
                    QualificationDerivedAccessPlatformV1::MacosApfs
                }
                QualificationDerivedAccessPlatformV1::LinuxCompileCi => unreachable!(),
            };
            let counterpart = package.change_storage_rows.iter().find(|row| {
                row.platform == counterpart_platform
                    && row.fixture == fixture
                    && row.phase == QualificationDerivedChangeStoragePhaseV1::InitialPublication
            });
            if let (Some(initial), Some(counterpart)) = (initial, counterpart) {
                let fixture_probes = |witness: &QualificationDerivedStorageWitnessV1| {
                    witness
                        .forbidden_probes
                        .iter()
                        .filter(|probe| {
                            matches!(
                                probe.kind,
                                QualificationDerivedStorageForbiddenProbeKindV1::ProposalSummary
                                    | QualificationDerivedStorageForbiddenProbeKindV1::Prose
                                    | QualificationDerivedStorageForbiddenProbeKindV1::PayloadDocument
                            )
                        })
                        .map(|probe| (probe.kind, probe.sentinel_sha256.clone()))
                        .collect::<BTreeSet<_>>()
                };
                if initial.witness.sqlite_catalog != counterpart.witness.sqlite_catalog
                    || initial.fixture_inventory_sha256 != counterpart.fixture_inventory_sha256
                    || initial.fixture_witness_sha256 != counterpart.fixture_witness_sha256
                    || fixture_probes(&initial.witness) != fixture_probes(&counterpart.witness)
                {
                    failed.push(format!("{fixture:?} cross-platform storage authority"));
                }
            }
        }
        let initial = package.change_storage_rows.iter().find(|row| {
            row.platform == platform
                && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                && row.phase == QualificationDerivedChangeStoragePhaseV1::InitialPublication
        });
        let post_append = package.change_storage_rows.iter().find(|row| {
            row.platform == platform
                && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                && row.phase == QualificationDerivedChangeStoragePhaseV1::PostAppendCheckpoint
        });
        if let (Some(initial), Some(post_append)) = (initial, post_append)
            && (initial.witness.publication.generation_id_sha256
                != post_append.witness.publication.generation_id_sha256
                || initial.witness.publication.descriptor_sha256
                    != post_append.witness.publication.descriptor_sha256
                || initial.witness.sqlite_catalog != post_append.witness.sqlite_catalog
                || initial
                    .witness
                    .live_checkpoint
                    .as_ref()
                    .zip(post_append.witness.live_checkpoint.as_ref())
                    .is_none_or(|(before, after)| {
                        before.checkpoint_sha256 == after.checkpoint_sha256
                            || before.reader_receipt_sha256 != after.reader_receipt_sha256
                    }))
        {
            failed.push(format!("{platform:?} same-generation checkpoint advance"));
        }
    }
}

fn evaluate_timeline_reads_v1(
    package: &QualificationDerivedAccessPackageV1,
    failed: &mut Vec<String>,
    missing: &mut Vec<String>,
) {
    if package.timeline_read_rows.is_empty() {
        missing.push("Timeline read matrix".to_owned());
        return;
    }

    for fixture in QualificationDerivedChangeFixtureV1::ALL {
        let authorities = package
            .timeline_read_rows
            .iter()
            .filter(|row| row.fixture == fixture)
            .map(|row| {
                (
                    row.fixture_inventory_sha256.as_str(),
                    row.fixture_witness_sha256.as_str(),
                )
            })
            .collect::<BTreeSet<_>>();
        if authorities.len() > 1 {
            failed.push(format!(
                "{fixture:?} cross-platform Timeline fixture authority"
            ));
        }
    }

    let mut run_identities = BTreeSet::new();
    for platform in [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ] {
        let product = package
            .product_identities
            .iter()
            .find(|identity| identity.platform == platform);
        let product_identity_sha256 = product.and_then(|identity| identity.canonical_sha256().ok());
        let execution = package
            .execution_identities
            .iter()
            .find(|identity| identity.platform == platform);
        let execution_identity_sha256 =
            execution.and_then(|identity| identity.canonical_sha256().ok());

        for fixture in QualificationDerivedChangeFixtureV1::ALL {
            for &case in required_timeline_cases_v1(fixture) {
                let criterion = format!("{platform:?}/{fixture:?}/{case:?} Timeline read");
                let Some(row) = package.timeline_read_rows.iter().find(|row| {
                    row.platform == platform && row.fixture == fixture && row.case == case
                }) else {
                    missing.push(criterion);
                    continue;
                };

                let source_authority_matches = package.change_read_rows.iter().any(|change| {
                    change.platform == platform
                        && change.fixture == fixture
                        && change.case == QualificationDerivedChangeReadCaseV1::ChangesBare
                        && change.fixture_inventory_sha256 == row.fixture_inventory_sha256
                        && change.fixture_witness_sha256 == row.fixture_witness_sha256
                });
                if product_identity_sha256.as_deref() != Some(row.product_identity_sha256.as_str())
                    || execution_identity_sha256.as_deref()
                        != Some(row.counter_execution_identity_sha256.as_str())
                    || product.is_none_or(|identity| {
                        !identity
                            .enabled_features
                            .iter()
                            .any(|feature| feature == "longitudinal-counting")
                    })
                    || !source_authority_matches
                {
                    failed.push(format!("{criterion} source authority"));
                }

                match row.status {
                    QualificationDerivedAccessStatusV1::Unknown => {
                        missing.push(criterion);
                        continue;
                    }
                    QualificationDerivedAccessStatusV1::Failed => {
                        failed.push(criterion);
                        continue;
                    }
                    QualificationDerivedAccessStatusV1::Passed => {}
                }

                let schedule = timeline_request_schedule_v1(fixture, case);
                let schedule_sha256 = timeline_request_schedule_sha256_v1(fixture, case);
                let receipts_valid = row.counter_receipts.len() == schedule.len()
                    && row
                        .counter_receipts
                        .iter()
                        .zip(schedule)
                        .all(|(receipt, operation)| {
                            let expected_success = !timeline_operation_is_typed_failure_v1(
                                operation,
                            ) && !(operation == &"timeline_fault_outcome"
                                && row.oracle
                                    == QualificationDerivedTimelineReadOracleV1::TypedFailure);
                            receipt.validate().is_ok()
                                && run_identities.insert(receipt.run_identity.clone())
                                && receipt.operation == *operation
                                && receipt.phase == case.as_str()
                                && receipt.success == expected_success
                                && execution.is_some_and(|identity| {
                                    receipt.root_identity == identity.root_provenance_sha256
                                })
                                && execution_identity_sha256.as_deref()
                                    == Some(receipt.base_execution_identity_sha256.as_str())
                                && product_identity_sha256.as_deref()
                                    == Some(receipt.derivative_execution_identity_sha256.as_str())
                                && receipt.manifest_sha256 == row.fixture_inventory_sha256
                                && receipt.schedule_sha256 == schedule_sha256
                                && timeline_counter_bounds_hold_v1(
                                    case,
                                    operation,
                                    &receipt.counters,
                                )
                        });
                let receipt_semantics = row
                    .counter_receipts
                    .iter()
                    .map(|receipt| receipt.semantic_result_sha256.clone())
                    .collect::<Vec<_>>();
                let semantic_receipts_match = canonical_sha256(&receipt_semantics)
                    .is_ok_and(|sha256| sha256 == row.derived_semantic_sha256);
                let expected_oracle =
                    qualification_derived_timeline_expected_oracle_v1(platform, fixture);
                let semantic_parity = match row.oracle {
                    QualificationDerivedTimelineReadOracleV1::StrictParity => {
                        row.strict_semantic_sha256.as_deref().is_some_and(|strict| {
                            validate_hex(strict, 64, "strict Timeline semantic receipt").is_ok()
                                && strict == row.derived_semantic_sha256
                        })
                    }
                    QualificationDerivedTimelineReadOracleV1::TypedFailure => {
                        row.strict_semantic_sha256.is_none()
                    }
                };
                let independently_expected =
                    expected_timeline_typed_documents_v1(platform, fixture, case);
                let typed_documents_match = row.expected_typed_documents == independently_expected
                    && row.observed_typed_documents.len() == independently_expected.len()
                    && row
                        .observed_typed_documents
                        .iter()
                        .zip(&independently_expected)
                        .all(|(observed, expected)| {
                            observed.operation == expected.operation
                                && observed.http_status == expected.http_status
                                && observed.document.validate().is_ok()
                                && observed.document.schema == expected.schema
                                && observed.document.version == expected.version
                                && observed.document.code == expected.code
                                && observed.document.retryable == expected.retryable
                        });
                if row.oracle != expected_oracle
                    || !row.wire_contract_matches
                    || !receipts_valid
                    || !semantic_receipts_match
                    || !semantic_parity
                    || !typed_documents_match
                    || !timeline_authority_valid_v1(row)
                    || !timeline_trust_transition_valid_v1(row)
                    || !timeline_concurrent_trust_valid_v1(row)
                    || !timeline_invalid_signature_failure_valid_v1(row, execution)
                {
                    failed.push(criterion);
                }
            }
        }
    }
}

fn qualification_derived_timeline_expected_oracle_v1(
    platform: QualificationDerivedAccessPlatformV1,
    fixture: QualificationDerivedChangeFixtureV1,
) -> QualificationDerivedTimelineReadOracleV1 {
    let (oracle, _, _) = qualification_derived_change_expected_outcome_v1(
        platform,
        fixture,
        QualificationDerivedChangeReadCaseV1::ChangesBare,
    );
    if oracle == QualificationDerivedChangeReadOracleV1::TypedFailure {
        QualificationDerivedTimelineReadOracleV1::TypedFailure
    } else {
        QualificationDerivedTimelineReadOracleV1::StrictParity
    }
}

fn timeline_operation_is_typed_failure_v1(operation: &str) -> bool {
    matches!(
        operation,
        "timeline_invalid_query"
            | "timeline_token_query_mismatch"
            | "timeline_token_direction_limit_mismatch"
            | "timeline_at_token_exclusive"
            | "timeline_trust_stale_token"
            | "timeline_k_stale_token"
    )
}

fn timeline_authority_valid_v1(row: &QualificationDerivedTimelineReadEvidenceV1) -> bool {
    let authority = &row.authority;
    let hashes_valid = [
        &authority.request_schedule_sha256,
        &authority.generation_identity_before_sha256,
        &authority.generation_identity_after_sha256,
        &authority.checkpoint_identity_before_sha256,
        &authority.checkpoint_identity_after_sha256,
        &authority.timeline_projection_stamp_before_sha256,
        &authority.timeline_projection_stamp_after_sha256,
        &authority.trust_identity_before_sha256,
        &authority.trust_identity_after_sha256,
    ]
    .into_iter()
    .all(|value| validate_hex(value, 64, "Timeline authority witness").is_ok())
        && authority
            .continuation_token_set_sha256
            .as_ref()
            .is_none_or(|value| validate_hex(value, 64, "Timeline continuation-token set").is_ok());
    let expects_token_set = matches!(
        row.case,
        QualificationDerivedTimelineReadCaseV1::PageTokenSuite
            | QualificationDerivedTimelineReadCaseV1::TrustSuite
            | QualificationDerivedTimelineReadCaseV1::PostAppendSuite
    );
    let checkpoint_changes = row.case == QualificationDerivedTimelineReadCaseV1::PostAppendSuite;
    let stamp_changes = matches!(
        row.case,
        QualificationDerivedTimelineReadCaseV1::TrustSuite
            | QualificationDerivedTimelineReadCaseV1::PostAppendSuite
    );
    let trust_changes = row.case == QualificationDerivedTimelineReadCaseV1::TrustSuite;
    let expects_family_counts = row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
        && row.case == QualificationDerivedTimelineReadCaseV1::StructuredQuerySuite;

    hashes_valid
        && authority.request_schedule_sha256
            == timeline_request_schedule_sha256_v1(row.fixture, row.case)
        && authority.generation_identity_before_sha256 == authority.generation_identity_after_sha256
        && (authority.checkpoint_identity_before_sha256
            != authority.checkpoint_identity_after_sha256)
            == checkpoint_changes
        && (authority.timeline_projection_stamp_before_sha256
            != authority.timeline_projection_stamp_after_sha256)
            == stamp_changes
        && (authority.trust_identity_before_sha256 != authority.trust_identity_after_sha256)
            == trust_changes
        && authority.continuation_token_set_sha256.is_some() == expects_token_set
        && if expects_family_counts {
            positive_counts_match_exact_keys_v1(
                &authority.authoritative_event_family_counts,
                &QUALIFICATION_TIMELINE_SOURCE_EVENT_FAMILIES_V1,
            ) && positive_counts_match_exact_keys_v1(
                &authority.strict_event_family_counts,
                &QUALIFICATION_TIMELINE_ADMITTED_EVENT_FAMILIES_V1,
            ) && authority.strict_event_family_counts == authority.derived_event_family_counts
                && authority.excluded_timeline_case_counts.len()
                    == QUALIFICATION_TIMELINE_EXCLUDED_CASES_V1.len()
                && QUALIFICATION_TIMELINE_EXCLUDED_CASES_V1.iter().all(|case| {
                    authority
                        .excluded_timeline_case_counts
                        .get(*case)
                        .is_some_and(|counts| {
                            counts.source_count > 0
                                && counts.strict_output_count == 0
                                && counts.derived_output_count == 0
                        })
                })
        } else {
            authority.authoritative_event_family_counts.is_empty()
                && authority.strict_event_family_counts.is_empty()
                && authority.derived_event_family_counts.is_empty()
                && authority.excluded_timeline_case_counts.is_empty()
        }
}

fn positive_counts_match_exact_keys_v1<const N: usize>(
    counts: &BTreeMap<String, u64>,
    expected: &[&str; N],
) -> bool {
    counts.len() == expected.len()
        && expected
            .iter()
            .all(|event_type| counts.get(*event_type).is_some_and(|count| *count > 0))
}

fn timeline_trust_transition_valid_v1(row: &QualificationDerivedTimelineReadEvidenceV1) -> bool {
    let expected = row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
        && row.case == QualificationDerivedTimelineReadCaseV1::TrustSuite;
    let Some(transition) = &row.trust_transition else {
        return !expected;
    };
    if !expected
        || transition.unsigned_event_id.trim().is_empty()
        || transition.signed_event_id.trim().is_empty()
        || transition.unsigned_event_id == transition.signed_event_id
        || transition.signer_identity.trim().is_empty()
        || transition.signer_identity.trim() != transition.signer_identity
    {
        return false;
    }
    let expected_before = BTreeMap::from([
        (
            transition.signed_event_id.clone(),
            "untrusted_key".to_owned(),
        ),
        (transition.unsigned_event_id.clone(), "unsigned".to_owned()),
    ]);
    let expected_after = BTreeMap::from([
        (transition.signed_event_id.clone(), "valid".to_owned()),
        (transition.unsigned_event_id.clone(), "unsigned".to_owned()),
    ]);
    transition.status_before_by_event == expected_before
        && transition.status_after_by_event == expected_after
}

fn timeline_concurrent_trust_valid_v1(row: &QualificationDerivedTimelineReadEvidenceV1) -> bool {
    let expected = row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
        && row.case == QualificationDerivedTimelineReadCaseV1::ProcessLifecycleSuite;
    let Some(transition) = &row.concurrent_trust_transition else {
        return !expected;
    };
    let expected_operations = BTreeSet::from([
        "timeline_concurrent_asc".to_owned(),
        "timeline_concurrent_desc".to_owned(),
    ]);
    expected
        && !transition.signed_event_id.trim().is_empty()
        && !transition.signer_identity.trim().is_empty()
        && transition.signer_identity.trim() == transition.signer_identity
        && [
            &transition.trust_identity_before_sha256,
            &transition.trust_identity_during_sha256,
            &transition.trust_identity_restored_sha256,
        ]
        .into_iter()
        .all(|identity| validate_hex(identity, 64, "concurrent Timeline trust identity").is_ok())
        && transition.trust_identity_before_sha256 == transition.trust_identity_restored_sha256
        && transition.trust_identity_before_sha256 != transition.trust_identity_during_sha256
        && transition.status_before == "untrusted_key"
        && transition.status_during == "valid"
        && transition.status_restored == "untrusted_key"
        && transition
            .observed_status_by_operation
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            == expected_operations
        && transition
            .observed_status_by_operation
            .values()
            .all(|status| matches!(status.as_str(), "untrusted_key" | "valid"))
}

pub(super) fn timeline_invalid_signature_failure_valid_v1(
    row: &QualificationDerivedTimelineReadEvidenceV1,
    execution: Option<&QualificationDerivedAccessExecutionIdentityV1>,
) -> bool {
    timeline_invalid_signature_failure_check_v1(row, execution).is_ok()
}

/// Per-condition form of the invalid-signature failure contract. The boolean
/// wrapper above accepts exactly the rows this check accepts, so evaluator
/// behavior is unchanged; the producer's receipt assembly and the disposable
/// lifecycle test surface the first failing NAMED condition instead of one
/// combined predicate. Conditions are pure, so check order affects only which
/// name is reported, never acceptance.
pub(super) fn timeline_invalid_signature_failure_check_v1(
    row: &QualificationDerivedTimelineReadEvidenceV1,
    execution: Option<&QualificationDerivedAccessExecutionIdentityV1>,
) -> Result<(), String> {
    let case = row.case;
    let expected = row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
        && row.case == QualificationDerivedTimelineReadCaseV1::TrustSuite;
    let Some(failure) = &row.invalid_signature_failure else {
        if expected {
            return Err(format!(
                "{case:?} Timeline invalid-signature failure evidence is absent"
            ));
        }
        return Ok(());
    };
    if !expected {
        return Err(format!(
            "{case:?} Timeline row carries an unexpected invalid-signature failure"
        ));
    }
    let Some(reference_execution) = execution else {
        return Err(format!(
            "{case:?} Timeline invalid-signature check requires the reference execution identity"
        ));
    };
    if row.status != QualificationDerivedAccessStatusV1::Passed {
        return Err(format!(
            "{case:?} Timeline invalid-signature row status is not passed"
        ));
    }
    if row.oracle != QualificationDerivedTimelineReadOracleV1::StrictParity {
        return Err(format!(
            "{case:?} Timeline invalid-signature row oracle is not strict parity"
        ));
    }
    if failure.carrier_event_id.trim().is_empty() {
        return Err(format!(
            "{case:?} Timeline invalid-signature carrier event id is empty"
        ));
    }
    let counter = &failure.counter_receipt;
    for (value, field) in [
        (
            &failure.reference_root_identity_sha256,
            "reference root identity",
        ),
        (
            &failure.reference_fixture_witness_sha256,
            "reference fixture witness",
        ),
        (
            &failure.fault_fixture_witness_sha256,
            "fault fixture witness",
        ),
        (&failure.reference_inventory_sha256, "reference inventory"),
        (
            &failure.reference_recovery_inventory_sha256,
            "reference recovery inventory",
        ),
        (
            &failure.fault_clean_inventory_sha256,
            "fault clean inventory",
        ),
        (
            &failure.fault_derivative_inventory_sha256,
            "fault derivative inventory",
        ),
        (
            &failure.fault_restored_inventory_sha256,
            "fault restored inventory",
        ),
        (&failure.clean_carrier_sha256, "clean carrier"),
        (&failure.mutated_carrier_sha256, "mutated carrier"),
        (
            &failure.observed_typed_document.canonical_sha256,
            "observed typed document",
        ),
        (&failure.clean_semantic_sha256, "clean semantic"),
        (
            &failure.strict_clean_semantic_sha256,
            "strict clean semantic",
        ),
        (&failure.strict_semantic_sha256, "strict semantic"),
        (&failure.derived_semantic_sha256, "derived semantic"),
        (
            &failure.strict_recovery_semantic_sha256,
            "strict recovery semantic",
        ),
        (
            &failure.derived_recovery_semantic_sha256,
            "derived recovery semantic",
        ),
        (
            &failure.reference_trust_identity_staged_sha256,
            "reference trust identity staged",
        ),
        (
            &failure.reference_trust_identity_restored_sha256,
            "reference trust identity restored",
        ),
        (
            &failure.fault_trust_identity_staged_sha256,
            "fault trust identity staged",
        ),
        (
            &failure.fault_trust_identity_restored_sha256,
            "fault trust identity restored",
        ),
    ] {
        if validate_hex(value, 64, field).is_err() {
            return Err(format!(
                "{case:?} Timeline invalid-signature {field} digest is not bare 64-hex: {value:?}"
            ));
        }
    }
    for (holds, condition) in [
        (
            failure.reference_inventory_sha256 == failure.reference_recovery_inventory_sha256,
            "reference inventory differs from its recovery inventory",
        ),
        (
            failure.reference_inventory_sha256 == failure.fault_clean_inventory_sha256,
            "fault clean inventory differs from the reference inventory",
        ),
        (
            failure.reference_inventory_sha256 == failure.fault_restored_inventory_sha256,
            "fault restored inventory differs from the reference inventory",
        ),
        (
            failure.reference_inventory_sha256 != failure.fault_derivative_inventory_sha256,
            "fault derivative inventory did not diverge from the reference inventory",
        ),
        (
            failure.reference_inventory_sha256 == row.fixture_inventory_sha256,
            "reference inventory differs from the row fixture inventory",
        ),
        (
            failure.reference_fixture_witness_sha256 == row.fixture_witness_sha256,
            "reference fixture witness differs from the row fixture witness",
        ),
        (
            failure.fault_fixture_witness_sha256 == failure.reference_fixture_witness_sha256,
            "fault fixture witness differs from the reference fixture witness",
        ),
    ] {
        if !holds {
            return Err(format!("{case:?} Timeline invalid-signature {condition}"));
        }
    }
    reference_execution.validate().map_err(|error| {
        format!("{case:?} Timeline invalid-signature reference execution is inadmissible: {error}")
    })?;
    failure.fault_execution.validate().map_err(|error| {
        format!("{case:?} Timeline invalid-signature fault execution is inadmissible: {error}")
    })?;
    if failure.reference_root_identity_sha256 != reference_execution.root_provenance_sha256 {
        return Err(format!(
            "{case:?} Timeline invalid-signature reference root identity differs from the \
             reference execution root provenance"
        ));
    }
    if failure.reference_root_identity_sha256 == failure.fault_execution.root_provenance_sha256 {
        return Err(format!(
            "{case:?} Timeline invalid-signature fault root provenance duplicates the reference \
             root identity"
        ));
    }
    let mut expected_fault_execution = reference_execution.clone();
    expected_fault_execution.root_provenance_sha256 =
        failure.fault_execution.root_provenance_sha256.clone();
    if failure.fault_execution != expected_fault_execution {
        return Err(format!(
            "{case:?} Timeline invalid-signature fault execution drifted from its reference in: {}",
            super::evidence::execution_identity_mismatches(
                &expected_fault_execution,
                &failure.fault_execution,
            )
            .join(", ")
        ));
    }
    for (holds, condition) in [
        (
            failure.reference_trust_identity_staged_sha256
                == failure.fault_trust_identity_staged_sha256,
            "staged trust identity differs between the reference and fault roots",
        ),
        (
            failure.reference_trust_identity_restored_sha256
                == failure.fault_trust_identity_restored_sha256,
            "restored trust identity differs between the reference and fault roots",
        ),
        (
            failure.reference_trust_identity_staged_sha256
                == row.authority.trust_identity_after_sha256,
            "staged trust identity differs from the row authority trust-after identity",
        ),
        (
            failure.reference_trust_identity_restored_sha256
                == row.authority.trust_identity_before_sha256,
            "restored trust identity differs from the row authority trust-before identity",
        ),
        (
            failure.reference_trust_identity_staged_sha256
                != failure.reference_trust_identity_restored_sha256,
            "staged trust identity did not diverge from the restored trust identity",
        ),
    ] {
        if !holds {
            return Err(format!("{case:?} Timeline invalid-signature {condition}"));
        }
    }
    failure
        .observed_typed_document
        .validate()
        .map_err(|error| {
            format!(
                "{case:?} Timeline invalid-signature observed typed document is invalid: {error}"
            )
        })?;
    for (holds, condition) in [
        (
            failure.observed_typed_document.schema == "pointbreak.inspect-change-projection-error",
            "observed typed document schema drifted",
        ),
        (
            failure.observed_typed_document.version == 1,
            "observed typed document version drifted",
        ),
        (
            failure.observed_typed_document.code == "projection_invalid",
            "observed typed document code drifted",
        ),
        (
            failure.observed_typed_document.retryable == Some(false),
            "observed typed document retryable flag drifted",
        ),
        (
            failure.clean_semantic_sha256 == failure.strict_clean_semantic_sha256,
            "clean derived and strict semantics diverged",
        ),
        (
            failure.clean_semantic_sha256 == failure.strict_recovery_semantic_sha256,
            "strict recovery semantics diverged from the clean semantics",
        ),
        (
            failure.clean_semantic_sha256 == failure.derived_recovery_semantic_sha256,
            "derived recovery semantics diverged from the clean semantics",
        ),
        (
            failure.clean_semantic_sha256 != failure.derived_semantic_sha256,
            "mutated derived semantics did not diverge from the clean semantics",
        ),
        (
            failure.clean_semantic_sha256 != failure.strict_semantic_sha256,
            "mutated strict semantics did not diverge from the clean semantics",
        ),
        (
            failure.strict_semantic_sha256 != failure.derived_semantic_sha256,
            "mutated strict and derived semantics did not diverge",
        ),
    ] {
        if !holds {
            return Err(format!("{case:?} Timeline invalid-signature {condition}"));
        }
    }
    let counters = &counter.counters;
    let barrier = &failure.barrier_receipt;
    counter.validate().map_err(|error| {
        format!("{case:?} Timeline invalid-signature counter receipt is invalid: {error}")
    })?;
    let expected_run_identity = timeline_invalid_signature_run_identity_v1(
        &failure.reference_root_identity_sha256,
        &failure.fault_execution.root_provenance_sha256,
        &row.product_identity_sha256,
        &failure.carrier_event_id,
        &barrier.expected_carrier_key_digest,
        &failure.clean_carrier_sha256,
        &failure.mutated_carrier_sha256,
        &failure.mutation_recipe_sha256,
        &barrier.barrier_identity_sha256,
        &failure.phase_process_identity_sha256[2],
    )
    .map_err(|error| {
        format!("{case:?} Timeline invalid-signature run identity is not derivable: {error}")
    })?;
    if counter.run_identity != expected_run_identity {
        return Err(format!(
            "{case:?} Timeline invalid-signature counter run identity drifted: expected \
             {expected_run_identity:?}, observed {:?}",
            counter.run_identity
        ));
    }
    barrier.validate().map_err(|error| {
        format!("{case:?} Timeline invalid-signature barrier receipt is invalid: {error}")
    })?;
    for (holds, condition) in [
        (
            barrier.run_identity == counter.run_identity,
            "barrier run identity differs from the counter run identity",
        ),
        (
            barrier.boundary == LongitudinalTimelinePostPinBoundaryV1::CarrierLocatorsSelected,
            "barrier boundary is not carrier_locators_selected",
        ),
        (
            barrier.carrier_opens_before == 0,
            "barrier observed carrier opens before the pin",
        ),
        (
            barrier.selected_carriers_before > 0,
            "barrier observed no selected carriers before the pin",
        ),
        (
            barrier.expected_carrier_key_digest == barrier.observed_mismatch_key_digest,
            "barrier mismatch key differs from the expected carrier key",
        ),
        (
            barrier.mismatch_kind == LongitudinalTimelineCarrierMismatchKindV1::ValidationWitness,
            "barrier mismatch kind is not validation_witness",
        ),
        (
            barrier.clean_carrier_sha256 == failure.clean_carrier_sha256,
            "barrier clean carrier differs from the failure clean carrier",
        ),
        (
            barrier.mutated_carrier_sha256 == failure.mutated_carrier_sha256,
            "barrier mutated carrier differs from the failure mutated carrier",
        ),
        (
            barrier.mutation_recipe_sha256 == failure.mutation_recipe_sha256,
            "barrier mutation recipe differs from the failure mutation recipe",
        ),
        (
            barrier.derivative_inventory_sha256 == failure.fault_derivative_inventory_sha256,
            "barrier derivative inventory differs from the fault derivative inventory",
        ),
        (
            !counter.success,
            "counter receipt reports success for the fault request",
        ),
        (
            counter.operation == "timeline_invalid_signature_fault",
            "counter operation drifted",
        ),
        (
            counter.phase == QualificationDerivedTimelineReadCaseV1::TrustSuite.as_str(),
            "counter phase drifted",
        ),
        (
            counter.root_identity == failure.fault_execution.root_provenance_sha256,
            "counter root identity differs from the fault root provenance",
        ),
        (
            counter.derivative_execution_identity_sha256 == row.product_identity_sha256,
            "counter derivative execution identity differs from the product identity",
        ),
        (
            counter.manifest_sha256 == barrier.barrier_identity_sha256,
            "counter manifest differs from the barrier identity",
        ),
        (
            counter.schedule_sha256
                == timeline_request_schedule_sha256_v1(
                    QualificationDerivedChangeFixtureV1::TopologyV1,
                    QualificationDerivedTimelineReadCaseV1::TrustSuite,
                ),
            "counter schedule digest drifted",
        ),
        (
            counter.semantic_result_sha256 == failure.observed_typed_document.canonical_sha256,
            "counter semantic result differs from the observed typed document",
        ),
        (
            counter.semantic_result_sha256 == failure.derived_semantic_sha256,
            "counter semantic result differs from the derived semantic digest",
        ),
    ] {
        if !holds {
            return Err(format!("{case:?} Timeline invalid-signature {condition}"));
        }
    }
    let fault_execution_identity_sha256 = failure.fault_execution.canonical_sha256().map_err(
        |error| {
            format!(
                "{case:?} Timeline invalid-signature fault execution identity is not canonical: \
                 {error}"
            )
        },
    )?;
    if counter.base_execution_identity_sha256 != fault_execution_identity_sha256 {
        return Err(format!(
            "{case:?} Timeline invalid-signature counter base execution identity differs from \
             the fault execution identity"
        ));
    }
    for (holds, condition) in [
        (
            counters.directory_entries_walked == 0,
            "counters walked directory entries",
        ),
        (
            counters.authority_identity_rows_scanned == 0,
            "counters scanned authority identity rows",
        ),
        (
            counters.change_candidates == 0,
            "counters observed Change candidates",
        ),
        (
            counters.change_candidate_current_revisions == 0,
            "counters observed Change candidate current revisions",
        ),
        (
            counters.change_capability_carriers_opened == 0,
            "counters opened Change capability carriers",
        ),
        (
            counters.change_proposal_carriers_opened == 0,
            "counters opened Change proposal carriers",
        ),
        (
            counters.change_proposal_carriers_validated == 0,
            "counters validated Change proposal carriers",
        ),
        (
            counters.change_support_carriers_opened == 0,
            "counters opened Change support carriers",
        ),
        (
            counters.change_matches == 0,
            "counters observed Change matches",
        ),
        (
            counters.change_rows_emitted == 0,
            "counters emitted Change rows",
        ),
        (
            counters.authoritative_fallbacks == 0,
            "counters observed authoritative fallbacks",
        ),
        (
            counters.full_history_fallbacks == 0,
            "counters observed full-history fallbacks",
        ),
        (counters.event_folds == 0, "counters observed event folds"),
        (
            counters.projection_rebuilds == 0,
            "counters observed projection rebuilds",
        ),
        (
            counters.state_rebuilds == 0,
            "counters observed state rebuilds",
        ),
        (
            counters.body_artifact_reads == 0,
            "counters observed body artifact reads",
        ),
        (
            counters.body_bytes_read == 0,
            "counters observed body bytes read",
        ),
        (
            counters.object_artifact_reads == 0,
            "counters observed object artifact reads",
        ),
        (
            counters.object_bytes_read == 0,
            "counters observed object bytes read",
        ),
        (
            counters.chronological_sort_items == 0,
            "counters observed chronological sort items",
        ),
        (
            counters.carrier_opens > 0,
            "counters observed no carrier opens",
        ),
        (
            counters.carrier_bytes_read > 0,
            "counters observed no carrier bytes read",
        ),
    ] {
        if !holds {
            return Err(format!("{case:?} Timeline invalid-signature {condition}"));
        }
    }
    // Primary hydration covers the selected, revision-candidate, and
    // correlation-support carriers in one batch that short-circuits at the
    // first witness mismatch, so the fault request opens the clean carriers
    // that sort before the mutated one plus the mutated carrier itself. The
    // exact abort point is witnessed by the validation count: every opened
    // carrier before the mutated one validated, and the mutated one failed
    // before validation.
    let primary_hydration_carriers = counters
        .timeline_selected_carriers
        .saturating_add(counters.timeline_revision_candidate_carriers)
        .saturating_add(counters.timeline_correlation_support_carriers);
    if counters.carrier_opens > primary_hydration_carriers {
        return Err(format!(
            "{case:?} Timeline invalid-signature counters carrier opens ({}) exceed the primary \
             hydration set ({} selected + {} candidates + {} correlation)",
            counters.carrier_opens,
            counters.timeline_selected_carriers,
            counters.timeline_revision_candidate_carriers,
            counters.timeline_correlation_support_carriers
        ));
    }
    if counters.event_validations != counters.carrier_opens.saturating_sub(1) {
        return Err(format!(
            "{case:?} Timeline invalid-signature counters event validations ({}) do not witness \
             an abort at the mutated carrier (carrier opens {})",
            counters.event_validations, counters.carrier_opens
        ));
    }
    if counters.timeline_sqlite_window_rows != counters.timeline_selected_carriers {
        return Err(format!(
            "{case:?} Timeline invalid-signature counters sqlite window rows ({}) differ from \
             the selected carriers ({})",
            counters.timeline_sqlite_window_rows, counters.timeline_selected_carriers
        ));
    }
    for (holds, condition) in [
        // The window selection records candidate and facet rows before the
        // post-pin abort, so neither is pinned to zero here; the zero pins
        // below cover only the stages the abort genuinely never reaches.
        (
            counters.timeline_selected_carriers > 0,
            "counters observed no selected Timeline carriers",
        ),
        (
            counters.timeline_revision_candidate_carriers
                <= counters.timeline_selected_carriers.saturating_mul(2),
            "counters revision candidate carriers exceed twice the selected carriers",
        ),
        (
            counters.timeline_removal_support_carriers == 0,
            "counters observed removal support carriers",
        ),
        (
            counters.timeline_signature_support_carriers == 0,
            "counters observed signature support carriers",
        ),
        (
            counters.timeline_correlation_support_carriers
                <= counters.timeline_selected_carriers.saturating_mul(2),
            "counters correlation support carriers exceed twice the selected carriers",
        ),
        (
            counters.timeline_trust_support_carriers == 0,
            "counters observed trust support carriers",
        ),
        (
            counters.timeline_exhaustive_candidates == 0,
            "counters observed exhaustive candidates",
        ),
        (
            counters.timeline_entries_emitted == 0,
            "counters emitted Timeline entries",
        ),
        (
            counters.response_bytes > 0,
            "counters observed no response bytes",
        ),
    ] {
        if !holds {
            return Err(format!("{case:?} Timeline invalid-signature {condition}"));
        }
    }
    // Phase order is reference-clean derived/strict, fault-mutated derived/strict,
    // reference-recovery derived/strict. Success-authority order omits the
    // fault-mutated derived lane, whose typed failure carries no stamps.
    for (index, value) in failure.phase_process_identity_sha256.iter().enumerate() {
        if validate_hex(value, 64, "Timeline phase process").is_err() {
            return Err(format!(
                "{case:?} Timeline invalid-signature phase {index} process identity is not bare \
                 64-hex: {value:?}"
            ));
        }
    }
    if failure
        .phase_process_identity_sha256
        .iter()
        .collect::<BTreeSet<_>>()
        .len()
        != 6
    {
        return Err(format!(
            "{case:?} Timeline invalid-signature phase process identities are not six distinct \
             processes"
        ));
    }
    if failure.phase_http_status != [200, 200, 503, 200, 200, 200] {
        return Err(format!(
            "{case:?} Timeline invalid-signature phase HTTP statuses drifted: observed {:?}",
            failure.phase_http_status
        ));
    }
    for (stamps, label) in [
        (
            &failure.phase_source_change_projection_stamp,
            "source Change projection stamp",
        ),
        (
            &failure.phase_timeline_projection_stamp,
            "Timeline projection stamp",
        ),
    ] {
        for (index, stamp) in stamps.iter().enumerate() {
            if !validate_prefixed_sha256_v1(stamp, "Timeline phase stamp") {
                return Err(format!(
                    "{case:?} Timeline invalid-signature phase {index} {label} is not a prefixed \
                     sha256: {stamp:?}"
                ));
            }
        }
        if stamps[0] != stamps[3] || stamps[1] != stamps[4] {
            return Err(format!(
                "{case:?} Timeline invalid-signature recovery {label} lanes drifted from their \
                 clean lanes"
            ));
        }
    }
    for (index, cursor) in failure.phase_authority_cursor_sha256.iter().enumerate() {
        if validate_hex(cursor, 64, "Timeline phase cursor").is_err() {
            return Err(format!(
                "{case:?} Timeline invalid-signature phase {index} authority cursor is not bare \
                 64-hex: {cursor:?}"
            ));
        }
    }
    // Reference-root lanes share one clean cursor; the mutated fault-root
    // strict lane (index 2) must witness the one-bit carrier mutation in
    // its live raw journal-record set and therefore differ.
    for reference_lane in [1, 3, 4] {
        if failure.phase_authority_cursor_sha256[reference_lane]
            != failure.phase_authority_cursor_sha256[0]
        {
            return Err(format!(
                "{case:?} Timeline invalid-signature reference lane {reference_lane} authority \
                 cursor drifted from the clean cursor"
            ));
        }
    }
    if failure.phase_authority_cursor_sha256[2] == failure.phase_authority_cursor_sha256[0] {
        return Err(format!(
            "{case:?} Timeline invalid-signature mutated strict lane did not witness the carrier \
             mutation in its authority cursor"
        ));
    }
    if sha256_bytes_hex(failure.phase_timeline_projection_stamp[0].as_bytes())
        != row.authority.timeline_projection_stamp_after_sha256
    {
        return Err(format!(
            "{case:?} Timeline invalid-signature clean Timeline projection stamp digest differs \
             from the row authority stamp-after digest"
        ));
    }
    let seed = &failure.fault_seed_receipt;
    seed.validate().map_err(|error| {
        format!("{case:?} Timeline invalid-signature fault-seed receipt is invalid: {error}")
    })?;
    for (holds, condition) in [
        (
            seed.authoritative_inventory_sha256 == failure.reference_inventory_sha256,
            "fault-seed authoritative inventory differs from the reference inventory",
        ),
        (
            seed.witness_sha256 == failure.reference_fixture_witness_sha256,
            "fault-seed witness differs from the reference fixture witness",
        ),
        (
            failure.clean_carrier_sha256 != failure.mutated_carrier_sha256,
            "mutated carrier did not diverge from the clean carrier",
        ),
        (
            validate_prefixed_sha256_v1(
                &failure.clean_event_record_hash,
                "clean signature event record",
            ),
            "clean event record hash is not a prefixed sha256",
        ),
        (
            failure.clean_event_record_hash == failure.mutated_event_record_hash,
            "event record identity drifted across the one-bit carrier mutation",
        ),
        (
            failure.mutation_recipe_sha256
                == QUALIFICATION_TIMELINE_INVALID_SIGNATURE_MUTATION_RECIPE_SHA256_V1,
            "mutation recipe digest drifted from the frozen recipe",
        ),
        (
            failure.clean_signature_status == "valid",
            "clean signature status is not valid",
        ),
        (
            failure.mutated_signature_status == "invalid",
            "mutated signature status is not invalid",
        ),
        (
            failure.strict_observed_signature_status == "invalid",
            "strict observed signature status is not invalid",
        ),
        (
            failure.recovery_signature_status == "valid",
            "recovery signature status is not valid",
        ),
        (
            row.authority.checkpoint_identity_before_sha256
                == row.authority.checkpoint_identity_after_sha256,
            "row authority checkpoint identity drifted across the suite",
        ),
        (
            row.authority.trust_identity_before_sha256 != row.authority.trust_identity_after_sha256,
            "row authority trust identity did not transition across the suite",
        ),
    ] {
        if !holds {
            return Err(format!("{case:?} Timeline invalid-signature {condition}"));
        }
    }
    Ok(())
}

fn validate_prefixed_sha256_v1(value: &str, label: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| validate_hex(digest, 64, label).is_ok())
}

fn timeline_counter_bounds_hold_v1(
    case: QualificationDerivedTimelineReadCaseV1,
    operation: &str,
    counters: &LongitudinalCountersV1,
) -> bool {
    let classified_carrier_opens = counters
        .timeline_selected_carriers
        .saturating_add(counters.timeline_revision_candidate_carriers)
        .saturating_add(counters.timeline_removal_support_carriers)
        .saturating_add(counters.timeline_signature_support_carriers)
        .saturating_add(counters.timeline_correlation_support_carriers);
    let common_bounds = counters.directory_entries_walked == 0
        && counters.authoritative_fallbacks == 0
        && counters.full_history_fallbacks == 0
        && counters.event_folds == 0
        && counters.projection_rebuilds == 0
        && counters.state_rebuilds == 0
        && counters.timeline_sqlite_window_rows <= counters.timeline_sqlite_candidates
        && counters.timeline_selected_carriers == counters.timeline_sqlite_window_rows
        && counters.carrier_opens == classified_carrier_opens
        && counters.timeline_revision_candidate_carriers
            <= counters.timeline_selected_carriers.saturating_mul(2)
        && counters.timeline_removal_support_carriers
            <= counters.timeline_selected_carriers.saturating_mul(2)
        && counters.timeline_signature_support_carriers
            <= counters.timeline_selected_carriers.saturating_mul(2)
        && counters.timeline_correlation_support_carriers
            <= counters.timeline_selected_carriers.saturating_mul(2)
        && counters.timeline_trust_support_carriers == counters.timeline_selected_carriers
        && counters.timeline_entries_emitted <= counters.timeline_trust_support_carriers
        && counters.event_validations == counters.carrier_opens;
    let pre_store_failure = matches!(
        operation,
        "timeline_invalid_query"
            | "timeline_token_query_mismatch"
            | "timeline_token_direction_limit_mismatch"
            | "timeline_at_token_exclusive"
    );
    if pre_store_failure {
        return common_bounds
            && counters.timeline_sqlite_candidates == 0
            && counters.timeline_sqlite_window_rows == 0
            && counters.timeline_sqlite_facet_rows == 0
            && counters.timeline_selected_carriers == 0
            && counters.timeline_exhaustive_candidates == 0
            && counters.timeline_entries_emitted == 0
            && counters.carrier_opens == 0
            && counters.carrier_bytes_read == 0
            && counters.body_artifact_reads == 0
            && counters.object_artifact_reads == 0;
    }
    let exhaustive_bounds = if case == QualificationDerivedTimelineReadCaseV1::ExhaustiveQuerySuite
    {
        counters.timeline_sqlite_facet_rows == 0
            && counters.timeline_exhaustive_candidates > 0
            && counters.timeline_exhaustive_candidates == counters.timeline_sqlite_window_rows
            && counters.timeline_exhaustive_candidates <= counters.timeline_sqlite_candidates
            && counters.timeline_selected_carriers == counters.timeline_exhaustive_candidates
            && counters.carrier_opens >= counters.timeline_exhaustive_candidates
            && counters.carrier_bytes_read > 0
            && counters.body_artifact_reads <= counters.timeline_exhaustive_candidates
            && counters.object_artifact_reads <= counters.timeline_exhaustive_candidates
    } else {
        counters.timeline_exhaustive_candidates == 0
            && counters.body_artifact_reads == 0
            && counters.object_artifact_reads == 0
            && timeline_request_limit_v1(operation)
                .is_none_or(|limit| counters.timeline_sqlite_window_rows <= limit)
    };
    let work_present = timeline_operation_is_typed_failure_v1(operation)
        || operation == "timeline_fault_outcome"
        || counters.timeline_sqlite_candidates > 0
            && counters.timeline_selected_carriers > 0
            && counters.timeline_entries_emitted > 0;
    common_bounds && exhaustive_bounds && work_present
}

fn timeline_request_limit_v1(operation: &str) -> Option<u64> {
    match operation {
        "timeline_all_asc"
        | "timeline_all_desc"
        | "timeline_type_filter"
        | "timeline_track_filter"
        | "timeline_change_filter"
        | "timeline_exact_revision_filter"
        | "timeline_facets_count_at"
        | "timeline_revision_correlations"
        | "timeline_withdrawal_equal_time_ordering"
        | "timeline_fault_outcome" => Some(100),
        "timeline_next"
        | "timeline_previous"
        | "timeline_trust_before"
        | "timeline_trust_after"
        | "timeline_trust_stale_token"
        | "timeline_cold"
        | "timeline_restart"
        | "timeline_warm"
        | "timeline_k"
        | "timeline_k_plus_one"
        | "timeline_k_stale_token"
        | "timeline_k_plus_one_fresh_process" => Some(2),
        "timeline_concurrent_asc" | "timeline_concurrent_desc" => Some(1),
        _ => None,
    }
}

fn evaluate_timeline_storage_v1(
    package: &QualificationDerivedAccessPackageV1,
    failed: &mut Vec<String>,
    missing: &mut Vec<String>,
) {
    if package.timeline_storage_rows.is_empty() {
        missing.push("Timeline storage matrix".to_owned());
        return;
    }
    for platform in [
        QualificationDerivedAccessPlatformV1::MacosApfs,
        QualificationDerivedAccessPlatformV1::WindowsNtfs,
    ] {
        let product_identity_sha256 = package
            .product_identities
            .iter()
            .find(|identity| identity.platform == platform)
            .and_then(|identity| identity.canonical_sha256().ok());
        let execution_identity_sha256 = package
            .execution_identities
            .iter()
            .find(|identity| identity.platform == platform)
            .and_then(|identity| identity.canonical_sha256().ok());
        for fixture in QualificationDerivedChangeFixtureV1::ALL {
            let phases: &[QualificationDerivedChangeStoragePhaseV1] =
                if fixture == QualificationDerivedChangeFixtureV1::TopologyV1 {
                    &[
                        QualificationDerivedChangeStoragePhaseV1::InitialPublication,
                        QualificationDerivedChangeStoragePhaseV1::PostAppendCheckpoint,
                    ]
                } else {
                    &[QualificationDerivedChangeStoragePhaseV1::InitialPublication]
                };
            for &phase in phases {
                let criterion = format!("{platform:?}/{fixture:?}/{phase:?} Timeline storage");
                let Some(row) = package.timeline_storage_rows.iter().find(|row| {
                    row.platform == platform && row.fixture == fixture && row.phase == phase
                }) else {
                    missing.push(criterion);
                    continue;
                };
                let change_row = package.change_storage_rows.iter().find(|change| {
                    change.platform == platform
                        && change.fixture == fixture
                        && change.phase == phase
                });
                let probe_kinds = row
                    .forbidden_probes
                    .iter()
                    .map(|probe| probe.kind)
                    .collect::<BTreeSet<_>>();
                let token_sentinel_expected = qualification_derived_change_expected_outcome_v1(
                    platform,
                    fixture,
                    QualificationDerivedChangeReadCaseV1::ChangesBare,
                )
                .0
                    != QualificationDerivedChangeReadOracleV1::TypedFailure;
                let probes_valid = row.forbidden_probes.len()
                    == QualificationDerivedTimelineForbiddenProbeKindV1::ALL.len()
                    && probe_kinds
                        == QualificationDerivedTimelineForbiddenProbeKindV1::ALL
                            .into_iter()
                            .collect()
                    && row.forbidden_probes.iter().all(|probe| {
                        let sentinel_valid = match (&probe.kind, &probe.sentinel_sha256) {
                            (
                                QualificationDerivedTimelineForbiddenProbeKindV1::TimelineContinuationToken,
                                None,
                            ) => !token_sentinel_expected,
                            (
                                QualificationDerivedTimelineForbiddenProbeKindV1::TimelineContinuationToken,
                                Some(sentinel),
                            ) => {
                                token_sentinel_expected
                                    && validate_hex(sentinel, 64, "Timeline storage probe").is_ok()
                            }
                            (_, Some(sentinel)) => {
                                validate_hex(sentinel, 64, "Timeline storage probe").is_ok()
                            }
                            (_, None) => false,
                        };
                        sentinel_valid
                            && probe.sqlite_match_count == 0
                            && probe.file_match_count == 0
                    });
                let schema_valid = change_row.is_some_and(|change| {
                    !change.witness.sqlite_catalog.entries.iter().any(|entry| {
                        forbidden_timeline_storage_name_v1(&entry.name)
                            || entry
                                .columns
                                .iter()
                                .any(|column| forbidden_timeline_storage_name_v1(&column.name))
                            || entry.indexes.iter().any(|index| {
                                forbidden_timeline_storage_name_v1(&index.name)
                                    || index.columns.iter().any(|column| {
                                        column
                                            .name
                                            .as_deref()
                                            .is_some_and(forbidden_timeline_storage_name_v1)
                                    })
                            })
                    }) && change.fixture_inventory_sha256 == row.fixture_inventory_sha256
                        && change.fixture_witness_sha256 == row.fixture_witness_sha256
                });
                if product_identity_sha256.as_deref() != Some(row.product_identity_sha256.as_str())
                    || execution_identity_sha256.as_deref()
                        != Some(row.execution_identity_sha256.as_str())
                    || !probes_valid
                    || !schema_valid
                {
                    failed.push(criterion);
                }
            }
        }
    }
}

fn forbidden_timeline_storage_name_v1(name: &str) -> bool {
    if forbidden_bodyless_storage_name_v1(name) {
        return true;
    }
    let name = name.to_ascii_lowercase();
    let tokens = name
        .split(|character: char| !character.is_ascii_alphanumeric())
        .collect::<BTreeSet<_>>();
    ["timeline", "trust", "token", "continuation"]
        .into_iter()
        .any(|subject| tokens.contains(subject))
        || tokens.contains("response")
            && ["body", "cache", "document", "payload"]
                .into_iter()
                .any(|material| tokens.contains(material))
}

fn forbidden_bodyless_storage_name_v1(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    if [
        "fts",
        "search",
        "summary",
        "snippet",
        "prose",
        "private_path",
        "owner_path",
    ]
    .into_iter()
    .any(|forbidden| name.contains(forbidden))
    {
        return true;
    }
    let tokens = name
        .split(|character: char| !character.is_ascii_alphanumeric())
        .collect::<BTreeSet<_>>();
    let names_body_material = ["body", "payload", "document", "documents"]
        .into_iter()
        .any(|subject| tokens.contains(subject));
    let names_metadata_only = [
        "hash",
        "digest",
        "sha256",
        "id",
        "type",
        "size",
        "count",
        "encoding",
        "version",
        "availability",
        "removed",
        "ordinal",
    ]
    .into_iter()
    .any(|metadata| tokens.contains(metadata));
    names_body_material && !names_metadata_only
}

fn change_read_row_failed(row: &QualificationDerivedChangeReadEvidenceV1) -> bool {
    const TOPOLOGY_CANDIDATES_PER_UNFILTERED_READ: u64 = 14;
    const TOPOLOGY_ROWS_PER_BOUNDED_READ: u64 = 2;
    const TOPOLOGY_PROPOSAL_OPENS_PER_BOUNDED_READ_MAX: u64 = 4;
    const TOPOLOGY_SUPPORT_OPENS_PER_BOUNDED_READ_MAX: u64 = 8;

    let counters = &row.counters;
    let (expected_oracle, expected_http_status, expected_code) =
        qualification_derived_change_expected_outcome_v1(row.platform, row.fixture, row.case);
    let classified_opens = counters
        .change_capability_carriers_opened
        .saturating_add(counters.change_proposal_carriers_opened)
        .saturating_add(counters.change_support_carriers_opened);
    let oracle_failed = match row.oracle {
        QualificationDerivedChangeReadOracleV1::StrictParity
        | QualificationDerivedChangeReadOracleV1::ReadyProfileParity => {
            row.expected_typed_document.is_some()
                || row.observed_typed_document.is_some()
                || row.strict_semantic_sha256.as_deref().is_none_or(|strict| {
                    validate_hex(strict, 64, "strict Change semantic receipt").is_err()
                        || strict != row.derived_semantic_sha256
                })
        }
        QualificationDerivedChangeReadOracleV1::TypedFailure => {
            let expected_document_shape = row.expected_code.as_deref().map(|code| {
                if code == "stale_projection" {
                    ("pointbreak.inspect-change-page-error", 1, None)
                } else {
                    ("pointbreak.inspect-change-projection-error", 1, Some(false))
                }
            });
            row.strict_semantic_sha256.is_some()
                || row.expected_http_status < 400
                || row
                    .expected_code
                    .as_deref()
                    .is_none_or(|code| code.trim().is_empty() || code.trim() != code)
                || row
                    .expected_typed_document
                    .as_ref()
                    .is_none_or(|document| document.validate().is_err())
                || row
                    .observed_typed_document
                    .as_ref()
                    .is_none_or(|document| document.validate().is_err())
                || row.expected_typed_document != row.observed_typed_document
                || row.expected_typed_document.as_ref().is_none_or(|document| {
                    expected_document_shape.is_none_or(|(schema, version, retryable)| {
                        document.schema != schema
                            || document.version != version
                            || document.code != row.expected_code.as_deref().unwrap_or_default()
                            || document.retryable != retryable
                    })
                })
        }
    };
    let case_failed = match row.case {
        QualificationDerivedChangeReadCaseV1::Profile => {
            counters.change_candidates != 0
                || counters.change_candidate_current_revisions != 0
                || counters.change_proposal_carriers_opened != 0
                || counters.change_proposal_carriers_validated != 0
                || counters.change_support_carriers_opened != 0
                || counters.change_matches != 0
                || counters.change_rows_emitted != 0
        }
        QualificationDerivedChangeReadCaseV1::ChangesBounded
        | QualificationDerivedChangeReadCaseV1::AttentionBounded => {
            counters.change_rows_emitted > 2
        }
        QualificationDerivedChangeReadCaseV1::SummaryQuery
        | QualificationDerivedChangeReadCaseV1::SummaryFilterSuite => {
            counters.change_proposal_carriers_opened < counters.change_candidate_current_revisions
        }
        _ => false,
    };
    let direct_work_expected = row.case != QualificationDerivedChangeReadCaseV1::Profile
        && (row.oracle != QualificationDerivedChangeReadOracleV1::TypedFailure
            || row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1);
    let direct_work_failed = direct_work_expected
        && (counters.change_candidates == 0
            || counters.change_candidate_current_revisions
                > counters.change_candidates.saturating_mul(2)
            || counters.change_proposal_carriers_opened
                > counters
                    .change_candidate_current_revisions
                    .saturating_mul(2)
            || counters.change_support_carriers_opened
                > counters.change_proposal_carriers_opened.saturating_mul(4)
            || !matches!(
                row.case,
                QualificationDerivedChangeReadCaseV1::SummaryQuery
                    | QualificationDerivedChangeReadCaseV1::SummaryFilterSuite
            ) && counters.change_matches != 0);
    let topology_work_failed = row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
        && row.case != QualificationDerivedChangeReadCaseV1::Profile
        && (counters.change_candidate_current_revisions == 0
            || counters.change_proposal_carriers_opened == 0
            || counters.change_rows_emitted == 0
            || matches!(
                row.case,
                QualificationDerivedChangeReadCaseV1::SummaryQuery
                    | QualificationDerivedChangeReadCaseV1::SummaryFilterSuite
            ) && counters.change_matches == 0);
    let bounded_topology_page_count = match row.case {
        QualificationDerivedChangeReadCaseV1::ChangesBounded
        | QualificationDerivedChangeReadCaseV1::AttentionBounded
        | QualificationDerivedChangeReadCaseV1::StalePageToken => Some(1_u64),
        QualificationDerivedChangeReadCaseV1::PageTokenSuite
        | QualificationDerivedChangeReadCaseV1::FreshProcessSuite
        | QualificationDerivedChangeReadCaseV1::PostAppendSuite
        | QualificationDerivedChangeReadCaseV1::PostAppendFreshProcessSuite => Some(2),
        QualificationDerivedChangeReadCaseV1::ConcurrentReaders
        | QualificationDerivedChangeReadCaseV1::WarmReuseSuite => Some(4),
        _ => None,
    };
    let bounded_topology_failed = row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
        && bounded_topology_page_count.is_some_and(|page_count| {
            let expected_candidates =
                page_count.saturating_mul(TOPOLOGY_CANDIDATES_PER_UNFILTERED_READ);
            counters.change_candidates != expected_candidates
                || counters.change_candidate_current_revisions != expected_candidates
                || counters.change_rows_emitted
                    != page_count.saturating_mul(TOPOLOGY_ROWS_PER_BOUNDED_READ)
                || counters.change_proposal_carriers_opened
                    > page_count.saturating_mul(TOPOLOGY_PROPOSAL_OPENS_PER_BOUNDED_READ_MAX)
                || counters.change_support_carriers_opened
                    > page_count.saturating_mul(TOPOLOGY_SUPPORT_OPENS_PER_BOUNDED_READ_MAX)
        });
    let fixture_failed = match (row.fixture, row.case) {
        (
            QualificationDerivedChangeFixtureV1::DuplicateEqualV1,
            QualificationDerivedChangeReadCaseV1::ChangesBare
            | QualificationDerivedChangeReadCaseV1::ChangesBounded
            | QualificationDerivedChangeReadCaseV1::AttentionBare
            | QualificationDerivedChangeReadCaseV1::AttentionBounded
            | QualificationDerivedChangeReadCaseV1::SummaryQuery,
        ) => {
            counters.change_proposal_carriers_opened != 2
                || counters.change_proposal_carriers_validated != 2
        }
        (
            QualificationDerivedChangeFixtureV1::RemovalV1,
            QualificationDerivedChangeReadCaseV1::ChangesBare
            | QualificationDerivedChangeReadCaseV1::ChangesBounded
            | QualificationDerivedChangeReadCaseV1::AttentionBare
            | QualificationDerivedChangeReadCaseV1::AttentionBounded
            | QualificationDerivedChangeReadCaseV1::SummaryQuery,
        ) => {
            counters.change_proposal_carriers_opened != 1
                || counters.change_proposal_carriers_validated != 1
                || counters.change_support_carriers_opened != 2
        }
        _ => false,
    };
    oracle_failed
        || row.oracle != expected_oracle
        || row.expected_http_status != expected_http_status
        || row.expected_code.as_deref() != expected_code
        || case_failed
        || direct_work_failed
        || topology_work_failed
        || bounded_topology_failed
        || fixture_failed
        || validate_hex(
            &row.fixture_inventory_sha256,
            64,
            "Change fixture inventory",
        )
        .is_err()
        || validate_hex(&row.fixture_witness_sha256, 64, "Change fixture witness").is_err()
        || row.expected_http_status < 200
        || row.expected_http_status > 599
        || row.observed_http_status != row.expected_http_status
        || row.observed_code != row.expected_code
        || validate_hex(
            &row.derived_semantic_sha256,
            64,
            "derived Change semantic receipt",
        )
        .is_err()
        || row.semantic_process_scope
            != QualificationDerivedAccessProcessScopeV1::InspectorServiceChild
        || row.counter_process_scope
            != QualificationDerivedAccessProcessScopeV1::QualificationHarness
        || !row.wire_contract_matches
        || counters.directory_entries_walked != 0
        || counters.carrier_opens != classified_opens
        || !matches!(counters.change_capability_carriers_opened, 0 | 2)
        || counters.change_proposal_carriers_validated > counters.change_proposal_carriers_opened
        || row.oracle != QualificationDerivedChangeReadOracleV1::TypedFailure
            && counters.change_proposal_carriers_opened
                != counters.change_proposal_carriers_validated
        || counters.change_matches > counters.change_candidates
        || counters.change_rows_emitted > counters.change_candidates
        || counters.authoritative_fallbacks != 0
        || counters.full_history_fallbacks != 0
        || counters.event_folds != 0
        || counters.body_artifact_reads != 0
        || counters.object_artifact_reads != 0
        || counters.projection_rebuilds != 0
        || counters.state_rebuilds != 0
}

fn validate_hex(value: &str, width: usize, label: &str) -> Result<(), String> {
    if value.len() != width
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "{label} must be {width} lowercase hexadecimal digits"
        ));
    }
    Ok(())
}

fn canonical_sha256<T: Serialize>(value: &T) -> Result<String, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    canonical_json_bytes(&value)
        .map(|bytes| sha256_bytes_hex(&bytes))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_off_strict_reader_control_names_one_existing_exact_test() {
        let (kind, test_name) = qualification_derived_change_control_test_v1(
            QualificationDerivedChangeControlCaseV1::ExplicitOffStrictReader,
        );
        assert_eq!(kind, QualificationDerivedChangeControlBinaryKindV1::Cli);
        assert_eq!(
            test_name,
            "cli::inspect::server::tests::routes_split_derived_collections_and_timeline_from_explicit_off_and_exact_reads"
        );
        assert!(include_str!("../../cli/inspect/server.rs").contains(
            "fn routes_split_derived_collections_and_timeline_from_explicit_off_and_exact_reads("
        ));
    }

    #[test]
    fn qualification_library_control_binary_attests_clean_source() {
        if let Ok(expected_commit) =
            std::env::var("POINTBREAK_QUALIFICATION_EXPECTED_CONTROL_COMMIT")
        {
            assert_eq!(env!("POINTBREAK_BUILD_SOURCE"), "git");
            assert_eq!(env!("POINTBREAK_BUILD_COMMIT"), expected_commit);
            assert_eq!(env!("POINTBREAK_BUILD_DIRTY"), "false");
            let build_configuration = format!(
                "debug={},gix={},bench={},longitudinal-counting={},lmdb-proof={},gix-parity={}",
                cfg!(debug_assertions),
                cfg!(feature = "gix"),
                cfg!(feature = "bench"),
                cfg!(feature = "longitudinal-counting"),
                cfg!(feature = "lmdb-proof"),
                cfg!(feature = "gix-parity"),
            );
            assert_eq!(
                build_configuration,
                "debug=true,gix=true,bench=true,longitudinal-counting=true,lmdb-proof=false,gix-parity=false"
            );
        }
        println!(
            "pointbreak-control-source={} commit={} dirty={} longitudinal-counting={}",
            env!("POINTBREAK_BUILD_SOURCE"),
            env!("POINTBREAK_BUILD_COMMIT"),
            env!("POINTBREAK_BUILD_DIRTY"),
            cfg!(feature = "longitudinal-counting"),
        );
    }

    fn zeros() -> QualificationDerivedAccessCountersV1 {
        QualificationDerivedAccessCountersV1 {
            directory_entries_walked: 0,
            carrier_opens: 0,
            carrier_bytes_read: 0,
            event_decodes: 0,
            event_validations: 0,
            event_folds: 0,
            chronological_sort_items: 0,
            body_artifact_reads: 0,
            body_bytes_read: 0,
            object_artifact_reads: 0,
            object_bytes_read: 0,
            unselected_body_artifact_reads: 0,
            unselected_object_artifact_reads: 0,
            projection_rebuilds: 0,
            state_rebuilds: 0,
            response_bytes: 1,
        }
    }

    fn digest(label: &str) -> String {
        sha256_bytes_hex(label.as_bytes())
    }

    fn product_identity(
        execution: &QualificationDerivedAccessExecutionIdentityV1,
    ) -> QualificationDerivedAccessProductIdentityV1 {
        QualificationDerivedAccessProductIdentityV1 {
            platform: execution.platform,
            source_commit: execution.source_commit.clone(),
            source_tree: execution.source_tree.clone(),
            cargo_lock_sha256: execution.cargo_lock_sha256.clone(),
            binary_sha256: digest(&format!("{:?} product binary", execution.platform)),
            version_sha256: digest(&format!("{:?} product version", execution.platform)),
            build_profile: "release".to_owned(),
            enabled_features: vec!["default".to_owned(), "longitudinal-counting".to_owned()],
            build_command_sha256: digest(&format!("{:?} product build", execution.platform)),
            operating_system: execution.operating_system.clone(),
            architecture: execution.architecture.clone(),
            source_dirty: false,
        }
    }

    fn control_binary_identity(
        execution: &QualificationDerivedAccessExecutionIdentityV1,
        kind: QualificationDerivedChangeControlBinaryKindV1,
    ) -> QualificationDerivedChangeControlBinaryIdentityV1 {
        let attestation_test =
            qualification_derived_change_control_attestation_test_v1(kind).to_owned();
        QualificationDerivedChangeControlBinaryIdentityV1 {
            platform: execution.platform,
            kind,
            source_commit: execution.source_commit.clone(),
            source_tree: execution.source_tree.clone(),
            cargo_lock_sha256: execution.cargo_lock_sha256.clone(),
            binary_sha256: digest(&format!("{:?}/{kind:?} control binary", execution.platform)),
            build_command_sha256: qualification_derived_change_control_build_command_sha256_v1(
                kind,
            ),
            operating_system: execution.operating_system.clone(),
            architecture: execution.architecture.clone(),
            source_dirty: false,
            attestation_command_sha256: qualification_derived_change_control_command_sha256_v1(
                &attestation_test,
            ),
            attestation_stdout_sha256: digest(&format!(
                "{:?}/{kind:?} attestation stdout",
                execution.platform
            )),
            attestation_stderr_sha256: digest(&format!(
                "{:?}/{kind:?} attestation stderr",
                execution.platform
            )),
            attestation_test,
        }
    }

    fn storage_witness(
        checkpoint: &str,
        fixture: QualificationDerivedChangeFixtureV1,
    ) -> QualificationDerivedStorageWitnessV1 {
        let hashes = qualification_derived_change_storage_probe_hashes_v1(fixture);
        let mut witness = QualificationDerivedStorageWitnessV1::test_fixture(checkpoint);
        for probe in &mut witness.forbidden_probes {
            probe.sentinel_sha256 = match probe.kind {
                QualificationDerivedStorageForbiddenProbeKindV1::ProposalSummary => {
                    hashes.proposal_summary_sha256.clone()
                }
                QualificationDerivedStorageForbiddenProbeKindV1::Prose => {
                    hashes.prose_sha256.clone()
                }
                QualificationDerivedStorageForbiddenProbeKindV1::PayloadDocument => {
                    hashes.payload_document_sha256.clone()
                }
                QualificationDerivedStorageForbiddenProbeKindV1::FixturePrivatePath
                | QualificationDerivedStorageForbiddenProbeKindV1::StoreRootPath => {
                    probe.sentinel_sha256.clone()
                }
            };
        }
        witness.refresh_sha256().expect("storage witness hash");
        witness
    }

    fn timeline_typed_document(
        code: &str,
        label: &str,
    ) -> QualificationDerivedChangeTypedDocumentV1 {
        QualificationDerivedChangeTypedDocumentV1 {
            schema: if matches!(
                code,
                "invalid_query" | "stale_projection" | "moving_journal"
            ) {
                "pointbreak.inspect-event-history-error"
            } else {
                "pointbreak.inspect-change-projection-error"
            }
            .to_owned(),
            version: 1,
            code: code.to_owned(),
            retryable: Some(code == "moving_journal"),
            canonical_sha256: digest(label),
        }
    }

    fn timeline_test_counters(
        case: QualificationDerivedTimelineReadCaseV1,
        operation: &str,
    ) -> LongitudinalCountersV1 {
        let mut counters = LongitudinalCountersV1 {
            response_bytes: 1,
            ..LongitudinalCountersV1::default()
        };
        if timeline_operation_is_typed_failure_v1(operation)
            || operation == "timeline_fault_outcome"
        {
            return counters;
        }
        let selected = if case == QualificationDerivedTimelineReadCaseV1::ExhaustiveQuerySuite {
            3
        } else {
            1
        };
        counters.timeline_sqlite_candidates = 3;
        counters.timeline_sqlite_window_rows = selected;
        counters.timeline_sqlite_facet_rows =
            u64::from(case == QualificationDerivedTimelineReadCaseV1::StructuredQuerySuite) * 3;
        counters.timeline_selected_carriers = selected;
        counters.timeline_trust_support_carriers = selected;
        counters.timeline_exhaustive_candidates =
            u64::from(case == QualificationDerivedTimelineReadCaseV1::ExhaustiveQuerySuite)
                * selected;
        counters.timeline_entries_emitted = 1;
        counters.carrier_opens = selected;
        counters.carrier_bytes_read = selected;
        counters.event_decodes = selected;
        counters.event_validations = selected;
        counters
    }

    fn timeline_test_receipt(
        execution: &QualificationDerivedAccessExecutionIdentityV1,
        product: &QualificationDerivedAccessProductIdentityV1,
        fixture: QualificationDerivedChangeFixtureV1,
        case: QualificationDerivedTimelineReadCaseV1,
        operation: &str,
        ordinal: usize,
        oracle: QualificationDerivedTimelineReadOracleV1,
    ) -> LongitudinalCounterReceiptV1 {
        let mut receipt = LongitudinalCounterReceiptV1 {
            schema: crate::bench_support::longitudinal::LONGITUDINAL_COUNTER_RECEIPT_SCHEMA_V1
                .to_owned(),
            run_identity: digest(&format!(
                "{:?}/{fixture:?}/{case:?}/{operation}/{ordinal}",
                execution.platform
            )),
            root_identity: execution.root_provenance_sha256.clone(),
            operation: operation.to_owned(),
            phase: case.as_str().to_owned(),
            base_execution_identity_sha256: execution
                .canonical_sha256()
                .expect("execution identity"),
            derivative_execution_identity_sha256: product
                .canonical_sha256()
                .expect("product identity"),
            manifest_sha256: digest("fixture inventory"),
            schedule_sha256: timeline_request_schedule_sha256_v1(fixture, case),
            success: !timeline_operation_is_typed_failure_v1(operation)
                && !(operation == "timeline_fault_outcome"
                    && oracle == QualificationDerivedTimelineReadOracleV1::TypedFailure),
            semantic_result_sha256: digest(&format!(
                "{:?}/{fixture:?}/{case:?}/{operation}/semantic",
                execution.platform
            )),
            counters: timeline_test_counters(case, operation),
            capacity_ownership: None,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt.canonical_sha256().expect("counter receipt hash");
        receipt
    }

    #[allow(clippy::too_many_arguments)]
    fn timeline_invalid_signature_test_receipts(
        reference_execution: &QualificationDerivedAccessExecutionIdentityV1,
        fault_execution: &QualificationDerivedAccessExecutionIdentityV1,
        product: &QualificationDerivedAccessProductIdentityV1,
        derivative_inventory_sha256: String,
        carrier_event_id: &str,
        carrier_key_digest: &str,
        clean_carrier_sha256: &str,
        mutated_carrier_sha256: &str,
        mutation_recipe_sha256: &str,
        semantic_result_sha256: &str,
        mutated_derived_process_identity_sha256: &str,
    ) -> (
        LongitudinalCounterReceiptV1,
        LongitudinalTimelinePostPinBarrierReceiptV1,
    ) {
        let product_identity_sha256 = product
            .canonical_sha256()
            .expect("invalid-signature product identity");
        let barrier_identity_sha256 = digest(&format!(
            "{}/{carrier_event_id}/post-pin-barrier",
            fault_execution.root_provenance_sha256
        ));
        let run_identity = timeline_invalid_signature_run_identity_v1(
            &reference_execution.root_provenance_sha256,
            &fault_execution.root_provenance_sha256,
            &product_identity_sha256,
            carrier_event_id,
            carrier_key_digest,
            clean_carrier_sha256,
            mutated_carrier_sha256,
            mutation_recipe_sha256,
            &barrier_identity_sha256,
            mutated_derived_process_identity_sha256,
        )
        .expect("invalid-signature run identity");
        let mut receipt = LongitudinalCounterReceiptV1 {
            schema: crate::bench_support::longitudinal::LONGITUDINAL_COUNTER_RECEIPT_SCHEMA_V1
                .to_owned(),
            run_identity: run_identity.clone(),
            root_identity: fault_execution.root_provenance_sha256.clone(),
            operation: "timeline_invalid_signature_fault".to_owned(),
            phase: QualificationDerivedTimelineReadCaseV1::TrustSuite
                .as_str()
                .to_owned(),
            base_execution_identity_sha256: fault_execution
                .canonical_sha256()
                .expect("invalid-signature execution identity"),
            derivative_execution_identity_sha256: product_identity_sha256,
            manifest_sha256: barrier_identity_sha256.clone(),
            schedule_sha256: timeline_request_schedule_sha256_v1(
                QualificationDerivedChangeFixtureV1::TopologyV1,
                QualificationDerivedTimelineReadCaseV1::TrustSuite,
            ),
            success: false,
            semantic_result_sha256: semantic_result_sha256.to_owned(),
            counters: LongitudinalCountersV1 {
                // The single open is the mutated carrier itself, which aborts
                // at its validation witness before decode or validation, so
                // the abort witness is validations == opens - 1 == 0.
                carrier_opens: 1,
                carrier_bytes_read: 1,
                timeline_sqlite_candidates: 1,
                timeline_sqlite_window_rows: 1,
                timeline_selected_carriers: 1,
                response_bytes: 1,
                ..LongitudinalCountersV1::default()
            },
            capacity_ownership: None,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt
            .canonical_sha256()
            .expect("invalid-signature counter receipt hash");
        let mut barrier_receipt = LongitudinalTimelinePostPinBarrierReceiptV1 {
            schema: crate::bench_support::longitudinal::LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_RECEIPT_SCHEMA_V1
                .to_owned(),
            run_identity,
            barrier_identity_sha256,
            boundary: LongitudinalTimelinePostPinBoundaryV1::CarrierLocatorsSelected,
            carrier_opens_before: 0,
            selected_carriers_before: 1,
            expected_carrier_key_digest: carrier_key_digest.to_owned(),
            observed_mismatch_key_digest: carrier_key_digest.to_owned(),
            mismatch_kind: LongitudinalTimelineCarrierMismatchKindV1::ValidationWitness,
            clean_carrier_sha256: clean_carrier_sha256.to_owned(),
            mutated_carrier_sha256: mutated_carrier_sha256.to_owned(),
            mutation_recipe_sha256: mutation_recipe_sha256.to_owned(),
            derivative_inventory_sha256,
            ready_receipt_sha256: digest("invalid-signature barrier ready receipt"),
            release_receipt_sha256: digest("invalid-signature barrier release receipt"),
            receipt_sha256: String::new(),
        };
        barrier_receipt.receipt_sha256 = barrier_receipt
            .canonical_sha256()
            .expect("invalid-signature barrier receipt hash");
        (receipt, barrier_receipt)
    }

    fn add_complete_timeline_evidence(package: &mut QualificationDerivedAccessPackageV1) {
        package.evaluator_revision = QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4.to_owned();
        package.evaluator_procedure_sha256 =
            qualification_derived_access_evaluator_v4_procedure_sha256();
        let products = package.product_identities.clone();
        let executions = package.execution_identities.clone();
        package.timeline_read_rows = [
            QualificationDerivedAccessPlatformV1::MacosApfs,
            QualificationDerivedAccessPlatformV1::WindowsNtfs,
        ]
        .into_iter()
        .flat_map(|platform| {
            let product = products
                .iter()
                .find(|identity| identity.platform == platform)
                .expect("Timeline product identity")
                .clone();
            let execution = executions
                .iter()
                .find(|identity| identity.platform == platform)
                .expect("Timeline execution identity")
                .clone();
            QualificationDerivedChangeFixtureV1::ALL
                .into_iter()
                .flat_map(move |fixture| {
                    let product = product.clone();
                    let execution = execution.clone();
                    required_timeline_cases_v1(fixture)
                        .iter()
                        .copied()
                        .map(move |case| {
                            let oracle = qualification_derived_timeline_expected_oracle_v1(
                                platform, fixture,
                            );
                            let receipts = timeline_request_schedule_v1(fixture, case)
                                .iter()
                                .enumerate()
                                .map(|(ordinal, operation)| {
                                    timeline_test_receipt(
                                        &execution,
                                        &product,
                                        fixture,
                                        case,
                                        operation,
                                        ordinal,
                                        oracle,
                                    )
                                })
                                .collect::<Vec<_>>();
                            let semantic_receipts = receipts
                                .iter()
                                .map(|receipt| receipt.semantic_result_sha256.clone())
                                .collect::<Vec<_>>();
                            let derived_semantic_sha256 = canonical_sha256(&semantic_receipts)
                                .expect("Timeline semantic receipt");
                            let expected_typed_documents =
                                expected_timeline_typed_documents_v1(platform, fixture, case);
                            let observed_typed_documents = expected_typed_documents
                                .iter()
                                .enumerate()
                                .map(|(index, expected)| {
                                    QualificationDerivedTimelineTypedObservationV1 {
                                        operation: expected.operation.clone(),
                                        http_status: expected.http_status,
                                        document: QualificationDerivedChangeTypedDocumentV1 {
                                            schema: expected.schema.clone(),
                                            version: expected.version,
                                            code: expected.code.clone(),
                                            retryable: expected.retryable,
                                            canonical_sha256: digest(&format!(
                                                "{platform:?}/{fixture:?}/{case:?}/{}/{index}",
                                                expected.code
                                            )),
                                        },
                                    }
                                })
                                .collect::<Vec<_>>();
                            let checkpoint_before = digest(&format!(
                                "{platform:?}/{fixture:?}/{case:?}/checkpoint"
                            ));
                            let stamp_before =
                                digest(&format!("{platform:?}/{fixture:?}/{case:?}/stamp"));
                            let trust_before =
                                digest(&format!("{platform:?}/{fixture:?}/{case:?}/trust"));
                            let trust_after = digest(&format!(
                                "{platform:?}/{fixture:?}/{case:?}/trust-after"
                            ));
                            let carries_family_counts = fixture
                                == QualificationDerivedChangeFixtureV1::TopologyV1
                                && case
                                    == QualificationDerivedTimelineReadCaseV1::StructuredQuerySuite;
                            let authoritative_event_family_counts: BTreeMap<String, u64> =
                                if carries_family_counts {
                                    QUALIFICATION_TIMELINE_SOURCE_EVENT_FAMILIES_V1
                                        .into_iter()
                                        .map(|event_type| (event_type.to_owned(), 1))
                                        .collect()
                                } else {
                                    BTreeMap::new()
                                };
                            let strict_event_family_counts: BTreeMap<String, u64> =
                                if carries_family_counts {
                                    QUALIFICATION_TIMELINE_ADMITTED_EVENT_FAMILIES_V1
                                        .into_iter()
                                        .map(|event_type| (event_type.to_owned(), 1))
                                        .collect()
                                } else {
                                    BTreeMap::new()
                                };
                            let excluded_timeline_case_counts: BTreeMap<
                                String,
                                QualificationDerivedTimelineExclusionCountsV1,
                            > = if carries_family_counts {
                                    QUALIFICATION_TIMELINE_EXCLUDED_CASES_V1
                                        .into_iter()
                                        .map(|excluded| {
                                            (
                                                excluded.to_owned(),
                                                QualificationDerivedTimelineExclusionCountsV1 {
                                                    source_count: 1,
                                                    strict_output_count: 0,
                                                    derived_output_count: 0,
                                                },
                                            )
                                        })
                                        .collect()
                                } else {
                                    BTreeMap::new()
                                };
                            let trust_transition = (fixture
                                == QualificationDerivedChangeFixtureV1::TopologyV1
                                && case == QualificationDerivedTimelineReadCaseV1::TrustSuite)
                                .then(|| QualificationDerivedTimelineTrustTransitionV1 {
                                    unsigned_event_id: "event:unsigned".to_owned(),
                                    signed_event_id: "event:signed".to_owned(),
                                    signer_identity: "did:key:z6MkTimelineTest".to_owned(),
                                    status_before_by_event: BTreeMap::from([
                                        (
                                            "event:signed".to_owned(),
                                            "untrusted_key".to_owned(),
                                        ),
                                        ("event:unsigned".to_owned(), "unsigned".to_owned()),
                                    ]),
                                    status_after_by_event: BTreeMap::from([
                                        ("event:signed".to_owned(), "valid".to_owned()),
                                        ("event:unsigned".to_owned(), "unsigned".to_owned()),
                                    ]),
                                });
                            let concurrent_trust_transition = (fixture
                                == QualificationDerivedChangeFixtureV1::TopologyV1
                                && case
                                    == QualificationDerivedTimelineReadCaseV1::ProcessLifecycleSuite)
                                .then(|| {
                                    let before = digest(&format!(
                                        "{platform:?}/TopologyV1/ProcessLifecycleSuite/concurrent-trust-before"
                                    ));
                                    QualificationDerivedTimelineConcurrentTrustEvidenceV1 {
                                        signed_event_id: "event:signed".to_owned(),
                                        signer_identity: "did:key:z6MkTimelineTest".to_owned(),
                                        trust_identity_before_sha256: before.clone(),
                                        trust_identity_during_sha256: digest(&format!(
                                            "{platform:?}/TopologyV1/ProcessLifecycleSuite/concurrent-trust-during"
                                        )),
                                        trust_identity_restored_sha256: before,
                                        status_before: "untrusted_key".to_owned(),
                                        status_during: "valid".to_owned(),
                                        status_restored: "untrusted_key".to_owned(),
                                        observed_status_by_operation: BTreeMap::from([
                                            (
                                                "timeline_concurrent_asc".to_owned(),
                                                "untrusted_key".to_owned(),
                                            ),
                                            (
                                                "timeline_concurrent_desc".to_owned(),
                                                "valid".to_owned(),
                                            ),
                                        ]),
                                    }
                                });
                            let invalid_signature_failure = (fixture
                                == QualificationDerivedChangeFixtureV1::TopologyV1
                                && case == QualificationDerivedTimelineReadCaseV1::TrustSuite)
                                .then(|| {
                                    let reference_inventory_sha256 = digest("fixture inventory");
                                    let fault_derivative_inventory_sha256 = digest(&format!(
                                        "{platform:?}/TopologyV1/invalid-signature/derivative"
                                    ));
                                    let carrier_event_id = "event:invalid-inline-signature";
                                    let carrier_key_digest = digest(&format!(
                                        "{platform:?}/TopologyV1/invalid-signature/carrier-key"
                                    ));
                                    let clean_carrier_sha256 =
                                        digest("clean inline signature carrier");
                                    let mutated_carrier_sha256 =
                                        digest("mutated inline signature carrier");
                                    let mutation_recipe_sha256 =
                                        QUALIFICATION_TIMELINE_INVALID_SIGNATURE_MUTATION_RECIPE_SHA256_V1
                                            .to_owned();
                                    let clean_cursor = digest(&format!(
                                        "{platform:?}/TopologyV1/TrustSuite/invalid-signature/cursor"
                                    ));
                                    let clean_semantic = digest(&format!(
                                        "{platform:?}/TopologyV1/TrustSuite/invalid-signature/clean"
                                    ));
                                    let event_record_hash = format!(
                                        "sha256:{}",
                                        digest("invalid-signature event record")
                                    );
                                    let phase_process_identity_sha256 = std::array::from_fn(|index| {
                                        digest(&format!("{platform:?}/invalid-signature/child-{index}"))
                                    });
                                    let derived_failure_semantic = digest(&format!(
                                        "{:?}/TopologyV1/TrustSuite/invalid-signature/semantic",
                                        execution.platform
                                    ));
                                    let mut fault_execution = execution.clone();
                                    fault_execution.root_provenance_sha256 = digest(&format!(
                                        "{platform:?}/TopologyV1/invalid-signature/fault-root"
                                    ));
                                    let (counter_receipt, barrier_receipt) =
                                        timeline_invalid_signature_test_receipts(
                                        &execution,
                                        &fault_execution,
                                        &product,
                                        fault_derivative_inventory_sha256.clone(),
                                        carrier_event_id,
                                        &carrier_key_digest,
                                        &clean_carrier_sha256,
                                        &mutated_carrier_sha256,
                                        &mutation_recipe_sha256,
                                        &derived_failure_semantic,
                                        &phase_process_identity_sha256[2],
                                    );
                                    let phase_stamp = |kind: &str| {
                                        let reference_derived = format!(
                                            "sha256:{}",
                                            digest(&format!("{platform:?}/reference-derived/{kind}"))
                                        );
                                        let reference_strict = format!(
                                            "sha256:{}",
                                            digest(&format!("{platform:?}/reference-strict/{kind}"))
                                        );
                                        let fault_strict = format!(
                                            "sha256:{}",
                                            digest(&format!("{platform:?}/fault-strict/{kind}"))
                                        );
                                        [
                                            reference_derived.clone(),
                                            reference_strict.clone(),
                                            fault_strict,
                                            reference_derived,
                                            reference_strict,
                                        ]
                                    };
                                    let fault_seed_receipt = {
                                        let mut receipt =
                                            QualificationDerivedTimelineFaultSeedReceiptV1 {
                                                schema:
                                                    QUALIFICATION_DERIVED_TIMELINE_FAULT_SEED_RECEIPT_SCHEMA_V1
                                                        .to_owned(),
                                                reference_root_path_sha256: digest(
                                                    "fault-seed reference root path",
                                                ),
                                                fault_root_path_sha256: digest(
                                                    "fault-seed fault root path",
                                                ),
                                                reference_witness_path_sha256: digest(
                                                    "fault-seed reference witness path",
                                                ),
                                                fault_witness_path_sha256: digest(
                                                    "fault-seed fault witness path",
                                                ),
                                                witness_sha256: digest("fixture witness"),
                                                tree_manifest_sha256: digest(
                                                    "fault-seed tree manifest",
                                                ),
                                                authoritative_inventory_sha256:
                                                    reference_inventory_sha256.clone(),
                                                inclusive_inventory_sha256:
                                                    reference_inventory_sha256.clone(),
                                                initial_trust_sha256: Some(trust_before.clone()),
                                                cloned_file_count: 42,
                                                cloned_byte_count: 4_096,
                                                receipt_sha256: String::new(),
                                            };
                                        receipt.receipt_sha256 = receipt
                                            .canonical_sha256()
                                            .expect("fault-seed receipt hash");
                                        receipt
                                    };
                                    QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1 {
                                        fault_seed_receipt,
                                        reference_root_identity_sha256: execution
                                            .root_provenance_sha256
                                            .clone(),
                                        fault_execution,
                                        reference_fixture_witness_sha256:
                                            digest("fixture witness"),
                                        fault_fixture_witness_sha256: digest("fixture witness"),
                                        carrier_event_id: carrier_event_id.to_owned(),
                                        clean_event_record_hash: event_record_hash.clone(),
                                        mutated_event_record_hash: event_record_hash,
                                        reference_inventory_sha256:
                                            reference_inventory_sha256.clone(),
                                        reference_recovery_inventory_sha256:
                                            reference_inventory_sha256.clone(),
                                        fault_clean_inventory_sha256:
                                            reference_inventory_sha256.clone(),
                                        fault_derivative_inventory_sha256,
                                        fault_restored_inventory_sha256:
                                            reference_inventory_sha256,
                                        clean_carrier_sha256,
                                        mutated_carrier_sha256,
                                        mutation_recipe_sha256,
                                        clean_signature_status: "valid".to_owned(),
                                        mutated_signature_status: "invalid".to_owned(),
                                        strict_observed_signature_status: "invalid".to_owned(),
                                        observed_typed_document: timeline_typed_document(
                                            "projection_invalid",
                                            &format!(
                                                "{platform:?}/TopologyV1/TrustSuite/invalid-signature/semantic"
                                            ),
                                        ),
                                        clean_semantic_sha256: clean_semantic.clone(),
                                        strict_clean_semantic_sha256: clean_semantic.clone(),
                                        strict_semantic_sha256: digest(&format!(
                                            "{platform:?}/TopologyV1/TrustSuite/invalid-signature/strict-mutated"
                                        )),
                                        derived_semantic_sha256: derived_failure_semantic,
                                        strict_recovery_semantic_sha256: clean_semantic.clone(),
                                        derived_recovery_semantic_sha256: clean_semantic,
                                        recovery_signature_status: "valid".to_owned(),
                                        reference_trust_identity_staged_sha256:
                                            trust_after.clone(),
                                        reference_trust_identity_restored_sha256:
                                            trust_before.clone(),
                                        fault_trust_identity_staged_sha256: trust_after.clone(),
                                        fault_trust_identity_restored_sha256:
                                            trust_before.clone(),
                                        phase_process_identity_sha256,
                                        phase_http_status: [200, 200, 503, 200, 200, 200],
                                        phase_source_change_projection_stamp: phase_stamp("source"),
                                        phase_timeline_projection_stamp: phase_stamp("timeline"),
                                        phase_authority_cursor_sha256: std::array::from_fn(
                                            |index| {
                                                if index == 2 {
                                                    digest(&format!(
                                                        "{platform:?}/TopologyV1/TrustSuite/invalid-signature/mutated-cursor"
                                                    ))
                                                } else {
                                                    clean_cursor.clone()
                                                }
                                            },
                                        ),
                                        counter_receipt,
                                        barrier_receipt,
                                    }
                                });
                            QualificationDerivedTimelineReadEvidenceV1 {
                                platform,
                                fixture,
                                fixture_inventory_sha256: digest("fixture inventory"),
                                fixture_witness_sha256: digest("fixture witness"),
                                case,
                                semantic_process_scope:
                                    QualificationDerivedAccessProcessScopeV1::InspectorServiceChild,
                                counter_process_scope:
                                    QualificationDerivedAccessProcessScopeV1::InspectorServiceChild,
                                product_identity_sha256: product
                                    .canonical_sha256()
                                    .expect("Timeline product identity"),
                                counter_execution_identity_sha256: execution
                                    .canonical_sha256()
                                    .expect("Timeline execution identity"),
                                status: QualificationDerivedAccessStatusV1::Passed,
                                oracle,
                                strict_semantic_sha256: (oracle
                                    == QualificationDerivedTimelineReadOracleV1::StrictParity)
                                    .then(|| derived_semantic_sha256.clone()),
                                derived_semantic_sha256,
                                wire_contract_matches: true,
                                expected_typed_documents,
                                observed_typed_documents,
                                authority: QualificationDerivedTimelineAuthorityEvidenceV1 {
                                    request_schedule_sha256:
                                        timeline_request_schedule_sha256_v1(fixture, case),
                                    generation_identity_before_sha256: digest(&format!(
                                        "{platform:?}/{fixture:?}/generation"
                                    )),
                                    generation_identity_after_sha256: digest(&format!(
                                        "{platform:?}/{fixture:?}/generation"
                                    )),
                                    checkpoint_identity_before_sha256: checkpoint_before.clone(),
                                    checkpoint_identity_after_sha256: if case
                                        == QualificationDerivedTimelineReadCaseV1::PostAppendSuite
                                    {
                                        digest(&format!(
                                            "{platform:?}/{fixture:?}/{case:?}/checkpoint-after"
                                        ))
                                    } else {
                                        checkpoint_before
                                    },
                                    timeline_projection_stamp_before_sha256: stamp_before.clone(),
                                    timeline_projection_stamp_after_sha256: if case
                                        == QualificationDerivedTimelineReadCaseV1::TrustSuite
                                    {
                                        invalid_signature_failure
                                            .as_ref()
                                            .map(|failure| {
                                                sha256_bytes_hex(
                                                    failure.phase_timeline_projection_stamp[0]
                                                        .as_bytes(),
                                                )
                                            })
                                            .unwrap_or_else(|| {
                                                digest(&format!(
                                                    "{platform:?}/{fixture:?}/{case:?}/stamp-after"
                                                ))
                                            })
                                    } else if case
                                        == QualificationDerivedTimelineReadCaseV1::PostAppendSuite
                                    {
                                        digest(&format!(
                                            "{platform:?}/{fixture:?}/{case:?}/stamp-after"
                                        ))
                                    } else {
                                        stamp_before
                                    },
                                    trust_identity_before_sha256: trust_before.clone(),
                                    trust_identity_after_sha256: if case
                                        == QualificationDerivedTimelineReadCaseV1::TrustSuite
                                    {
                                        trust_after
                                    } else {
                                        trust_before
                                    },
                                    continuation_token_set_sha256: matches!(
                                        case,
                                        QualificationDerivedTimelineReadCaseV1::PageTokenSuite
                                            | QualificationDerivedTimelineReadCaseV1::TrustSuite
                                            | QualificationDerivedTimelineReadCaseV1::PostAppendSuite
                                    )
                                    .then(|| {
                                        digest(&format!(
                                            "{platform:?}/{fixture:?}/{case:?}/tokens"
                                        ))
                                    }),
                                    authoritative_event_family_counts,
                                    strict_event_family_counts: strict_event_family_counts.clone(),
                                    derived_event_family_counts: strict_event_family_counts,
                                    excluded_timeline_case_counts,
                                },
                                trust_transition,
                                concurrent_trust_transition,
                                invalid_signature_failure,
                                counter_receipts: receipts,
                            }
                        })
                })
        })
        .collect();

        package.timeline_storage_rows = package
            .change_storage_rows
            .iter()
            .map(|change| QualificationDerivedTimelineStorageEvidenceV1 {
                platform: change.platform,
                fixture: change.fixture,
                phase: change.phase,
                fixture_inventory_sha256: change.fixture_inventory_sha256.clone(),
                fixture_witness_sha256: change.fixture_witness_sha256.clone(),
                product_identity_sha256: change.product_identity_sha256.clone(),
                execution_identity_sha256: change.execution_identity_sha256.clone(),
                forbidden_probes: QualificationDerivedTimelineForbiddenProbeKindV1::ALL
                    .into_iter()
                    .map(
                        |kind| QualificationDerivedTimelineForbiddenProbeEvidenceV1 {
                            kind,
                            sentinel_sha256: if kind
                                == QualificationDerivedTimelineForbiddenProbeKindV1::TimelineContinuationToken
                                && qualification_derived_change_expected_outcome_v1(
                                    change.platform,
                                    change.fixture,
                                    QualificationDerivedChangeReadCaseV1::ChangesBare,
                                )
                                .0 == QualificationDerivedChangeReadOracleV1::TypedFailure
                            {
                                None
                            } else {
                                Some(digest(&format!("{:?}/{kind:?}", change.fixture)))
                            },
                            sqlite_match_count: 0,
                            file_match_count: 0,
                        },
                    )
                    .collect(),
            })
            .collect();
    }

    fn complete_package() -> QualificationDerivedAccessPackageV1 {
        let contract = qualification_derived_access_contract_v1();
        let mut operation_rows = Vec::new();
        for (platform, tiers) in [
            (
                QualificationDerivedAccessPlatformV1::MacosApfs,
                QualificationDerivedAccessTierV1::ALL.as_slice(),
            ),
            (
                QualificationDerivedAccessPlatformV1::WindowsNtfs,
                QualificationDerivedAccessTierV1::NATIVE.as_slice(),
            ),
        ] {
            for &tier in tiers {
                for requirement in &contract.operations {
                    operation_rows.push(QualificationDerivedAccessOperationEvidenceV1 {
                        tier,
                        platform,
                        operation: requirement.operation,
                        status: QualificationDerivedAccessStatusV1::Passed,
                        process_scope: requirement.process_scope,
                        semantic_receipt_matches: true,
                        complexity: QualificationDerivedAccessComplexityV1::BoundedSelectedWork,
                        retained_samples: match tier {
                            QualificationDerivedAccessTierV1::D0_128
                            | QualificationDerivedAccessTierV1::L1
                            | QualificationDerivedAccessTierV1::L7 => 1,
                            _ if requirement.operation
                                == QualificationDerivedAccessOperationV1::Restart =>
                            {
                                20
                            }
                            _ => 60,
                        },
                        wall_p95_ms: (tier == QualificationDerivedAccessTierV1::L100)
                            .then_some(requirement.l100_wall_p95_ceiling_ms),
                        process_cpu_p95_ms: (tier == QualificationDerivedAccessTierV1::L100)
                            .then_some(requirement.l100_process_cpu_p95_ceiling_ms),
                        selected_output_count: Some(
                            if tier == QualificationDerivedAccessTierV1::C262 {
                                1_250
                            } else {
                                1_000
                            },
                        ),
                        unselected_work_count: Some(0),
                        selected_work_count: if tier == QualificationDerivedAccessTierV1::C262 {
                            1_250
                        } else {
                            1_000
                        },
                        retained_cardinality: match tier {
                            QualificationDerivedAccessTierV1::D0_128 => 128,
                            QualificationDerivedAccessTierV1::L1 => 1_024,
                            QualificationDerivedAccessTierV1::L7 => 7_168,
                            QualificationDerivedAccessTierV1::L100
                                if matches!(
                                    requirement.operation,
                                    QualificationDerivedAccessOperationV1::AppendOne
                                        | QualificationDerivedAccessOperationV1::PostOne
                                        | QualificationDerivedAccessOperationV1::Restart
                                ) =>
                            {
                                102_430
                            }
                            QualificationDerivedAccessTierV1::C262
                                if matches!(
                                    requirement.operation,
                                    QualificationDerivedAccessOperationV1::AppendOne
                                        | QualificationDerivedAccessOperationV1::PostOne
                                        | QualificationDerivedAccessOperationV1::Restart
                                ) =>
                            {
                                262_174
                            }
                            QualificationDerivedAccessTierV1::L100 => 102_400,
                            QualificationDerivedAccessTierV1::C262 => 262_144,
                        },
                        l100_to_c262_selected_work_ratio_milli: (tier
                            == QualificationDerivedAccessTierV1::C262)
                            .then_some(1_250),
                        counters: zeros(),
                    });
                }
            }
        }
        let lifecycle_rows = [
            QualificationDerivedAccessPlatformV1::MacosApfs,
            QualificationDerivedAccessPlatformV1::WindowsNtfs,
        ]
        .into_iter()
        .flat_map(|platform| {
            QualificationDerivedAccessTierV1::NATIVE
                .into_iter()
                .flat_map(move |tier| {
                    QualificationDerivedAccessLifecycleCriterionV1::ALL
                        .into_iter()
                        .map(
                            move |criterion| QualificationDerivedAccessLifecycleEvidenceV1 {
                                tier,
                                platform,
                                criterion,
                                status: QualificationDerivedAccessStatusV1::Passed,
                            },
                        )
                })
        })
        .collect();
        QualificationDerivedAccessPackageV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_PACKAGE_SCHEMA_V1.to_owned(),
            evaluator_revision: QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V2.to_owned(),
            evaluator_procedure_sha256: String::new(),
            proposed_profile_id: "example-physical-profile-v1".to_owned(),
            execution_identities: vec![
                QualificationDerivedAccessExecutionIdentityV1 {
                    platform: QualificationDerivedAccessPlatformV1::MacosApfs,
                    source_commit: "1".repeat(40),
                    source_tree: "2".repeat(40),
                    cargo_lock_sha256: "3".repeat(64),
                    binary_sha256: "4".repeat(64),
                    contract_schema: QUALIFICATION_DERIVED_ACCESS_CONTRACT_SCHEMA_V1.to_owned(),
                    contract_sha256: QUALIFICATION_DERIVED_ACCESS_CONTRACT_SHA256_V1.to_owned(),
                    root_provenance_sha256: "5".repeat(64),
                    command_sha256: "6".repeat(64),
                    operating_system: "macos".to_owned(),
                    architecture: "aarch64".to_owned(),
                    filesystem: "apfs".to_owned(),
                    host_identity_sha256: "a".repeat(64),
                    source_dirty: false,
                    private_corpus_configured: false,
                },
                QualificationDerivedAccessExecutionIdentityV1 {
                    platform: QualificationDerivedAccessPlatformV1::WindowsNtfs,
                    source_commit: "1".repeat(40),
                    source_tree: "2".repeat(40),
                    cargo_lock_sha256: "3".repeat(64),
                    binary_sha256: "7".repeat(64),
                    contract_schema: QUALIFICATION_DERIVED_ACCESS_CONTRACT_SCHEMA_V1.to_owned(),
                    contract_sha256: QUALIFICATION_DERIVED_ACCESS_CONTRACT_SHA256_V1.to_owned(),
                    root_provenance_sha256: "5".repeat(64),
                    command_sha256: "9".repeat(64),
                    operating_system: "windows".to_owned(),
                    architecture: "x86_64".to_owned(),
                    filesystem: "ntfs".to_owned(),
                    host_identity_sha256: "b".repeat(64),
                    source_dirty: false,
                    private_corpus_configured: false,
                },
            ],
            product_identities: Vec::new(),
            change_control_binary_identities: Vec::new(),
            root_bindings: Vec::new(),
            d0_rows: [
                QualificationDerivedAccessPlatformV1::MacosApfs,
                QualificationDerivedAccessPlatformV1::WindowsNtfs,
            ]
            .into_iter()
            .map(|platform| QualificationDerivedAccessD0EvidenceV1 {
                platform,
                stored_events: 128,
                revisions: 16,
                independently_referenced_objects: 16,
                schedule_sha256: contract.d0.schedule_sha256.clone(),
                ordered_schedule_sha256: digest("D0 ordered schedule"),
                root_a_sha256: digest("D0 root"),
                root_b_sha256: digest("D0 root"),
                byte_identical: true,
            })
            .collect(),
            operation_rows,
            lifecycle_rows,
            resources: Some(QualificationDerivedAccessResourceEvidenceV1 {
                l100_steady_rss_bytes: 96 * MIB,
                l100_peak_rss_bytes: 128 * MIB,
                l7_to_l100_steady_slope_bytes_per_event: 512,
                retained_body_object_bytes_outside_active_window: 0,
            }),
            allocation_rows: vec![
                QualificationDerivedAccessAllocationEvidenceV1 {
                    tier: QualificationDerivedAccessTierV1::L100,
                    event_count: 102_400,
                    steady_derived_bytes: 64 * MIB,
                    high_water_derived_bytes: 96 * MIB,
                    append_write_amplification_ratio_milli: 8_000,
                },
                QualificationDerivedAccessAllocationEvidenceV1 {
                    tier: QualificationDerivedAccessTierV1::C262,
                    event_count: 262_144,
                    steady_derived_bytes: 128 * MIB,
                    high_water_derived_bytes: 192 * MIB,
                    append_write_amplification_ratio_milli: 8_000,
                },
            ],
            bootstrap_rows: vec![
                QualificationDerivedAccessBootstrapEvidenceV1 {
                    tier: QualificationDerivedAccessTierV1::L100,
                    status: QualificationDerivedAccessStatusV1::Passed,
                    elapsed_seconds: 3_600,
                    progress_reported: true,
                    high_water_derived_bytes: 1,
                },
                QualificationDerivedAccessBootstrapEvidenceV1 {
                    tier: QualificationDerivedAccessTierV1::C262,
                    status: QualificationDerivedAccessStatusV1::Passed,
                    elapsed_seconds: 10_800,
                    progress_reported: true,
                    high_water_derived_bytes: 1,
                },
            ],
            change_read_rows: Vec::new(),
            change_control_rows: Vec::new(),
            change_storage_rows: Vec::new(),
            timeline_read_rows: Vec::new(),
            timeline_storage_rows: Vec::new(),
            complete: true,
        }
    }

    #[test]
    fn derived_access_contract_freezes_candidate_independent_authority() {
        let contract = qualification_derived_access_contract_v1();
        assert_eq!(contract.d0.stored_events, 128);
        assert_eq!(
            contract
                .d0
                .event_families
                .iter()
                .map(|row| u32::from(row.count))
                .sum::<u32>(),
            128
        );
        assert_eq!(
            contract
                .operations
                .iter()
                .map(|row| row.operation)
                .collect::<Vec<_>>(),
            QualificationDerivedAccessOperationV1::ALL
        );
        assert!(!contract.observed_candidate_result_present);
        contract.validate().expect("frozen contract");
    }

    #[test]
    fn derived_access_d0_event_families_match_the_live_event_vocabulary() {
        use crate::session::event::EventType;

        let contract = qualification_derived_access_contract_v1();
        let mut contract_event_types = contract
            .d0
            .event_families
            .iter()
            .map(|row| row.event_type.as_str())
            .collect::<Vec<_>>();
        contract_event_types.sort_unstable();

        let mut live_event_types = EventType::ALL
            .iter()
            // D0 is the frozen pre-activation workload. The writer-dark
            // Change/Revision cohort is separately qualified before a capable
            // derived schema may consume it; it must not silently rewrite this
            // historical baseline's contract identity.
            .filter(|event_type| {
                !matches!(
                    event_type,
                    EventType::ChangeDeclared
                        | EventType::ChangeMembershipAsserted
                        | EventType::ChangeMembershipWithdrawn
                        | EventType::ChangeLinkAsserted
                        | EventType::ChangeRevisionRelationAsserted
                        | EventType::ChangeRevisionRelationWithdrawn
                        | EventType::RevisionRelationAttested
                        | EventType::ReviewFactPorted
                )
            })
            .map(|event_type| event_type.as_str())
            .collect::<Vec<_>>();
        live_event_types.sort_unstable();

        assert_eq!(
            contract_event_types, live_event_types,
            "a changed L0 EventType vocabulary requires a new D0 schedule and contract identity"
        );
    }

    #[test]
    fn evaluator_v3_procedure_remains_frozen_after_timeline_v4() {
        assert_eq!(
            qualification_derived_access_evaluator_v3_procedure_sha256(),
            QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V3_PROCEDURE_SHA256_V1
        );
        assert_eq!(
            qualification_derived_access_evaluator_v4_procedure_sha256(),
            QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V4_PROCEDURE_SHA256_V1
        );
        assert_eq!(
            &QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V4_STEPS_V1[..6],
            QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V3_STEPS_V1
        );
        assert_ne!(
            QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V3_PROCEDURE_SHA256_V1,
            QUALIFICATION_DERIVED_ACCESS_EVALUATOR_V4_PROCEDURE_SHA256_V1
        );
    }

    #[test]
    fn timeline_source_and_admitted_family_inventories_track_live_vocabulary() {
        use crate::session::event::EventType;

        let mut live_source = EventType::ALL
            .into_iter()
            .map(|event_type| event_type.as_str())
            .collect::<Vec<_>>();
        let mut frozen_source = QUALIFICATION_TIMELINE_SOURCE_EVENT_FAMILIES_V1.to_vec();
        live_source.sort_unstable();
        frozen_source.sort_unstable();
        assert_eq!(live_source, frozen_source);

        let mut live_admitted = EventType::ALL
            .into_iter()
            .filter(|event_type| {
                !matches!(
                    event_type,
                    EventType::TaskCheckpointCaptured
                        | EventType::TaskObservationRecorded
                        | EventType::EventSignatureRecorded
                        | EventType::ArtifactRemoved
                )
            })
            .map(|event_type| event_type.as_str())
            .collect::<Vec<_>>();
        let mut frozen_admitted = QUALIFICATION_TIMELINE_ADMITTED_EVENT_FAMILIES_V1.to_vec();
        live_admitted.sort_unstable();
        frozen_admitted.sort_unstable();
        assert_eq!(live_admitted, frozen_admitted);
        assert_eq!(QUALIFICATION_TIMELINE_EXCLUDED_CASES_V1.len(), 7);
    }

    #[test]
    fn derived_access_decision_table_reads_thresholds_from_the_contract() {
        let mut contract = qualification_derived_access_contract_v1();
        contract.d0.stored_events = 129;
        contract.sampling.release_roots = 3;
        contract.operations[0].l100_wall_p95_ceiling_ms = 151;
        contract.memory.l100_steady_rss_bytes = 97 * MIB;
        contract.allocation.high_water_ratio_milli = 1_750;
        contract.bootstrap.l100_ceiling_seconds = 61 * 60;

        let table = contract.decision_table_markdown();
        for expected in [
            "`D0-128`: 129 events",
            "3 release roots",
            "`SEMANTIC_ID` `151/100 ms`",
            "steady/peak RSS at most `97/128 MiB`",
            "high-water at most `1.75×`",
            "L100 at most 61 minutes",
        ] {
            assert!(
                table.contains(expected),
                "decision table did not reflect changed contract field: {expected}"
            );
        }
    }

    #[test]
    fn bodyless_schema_names_allow_hash_metadata_and_refuse_body_material() {
        for allowed in [
            "payload_hash",
            "event_payload_sha256",
            "body_content_type",
            "content_digest",
        ] {
            assert!(!forbidden_bodyless_storage_name_v1(allowed), "{allowed}");
        }
        for forbidden in [
            "payload_documents",
            "proposal_summary",
            "body_text",
            "private_path_cache",
            "semantic_search",
        ] {
            assert!(forbidden_bodyless_storage_name_v1(forbidden), "{forbidden}");
        }
        for forbidden in [
            "timeline_response_cache",
            "continuation_token",
            "runtime_trust_result",
        ] {
            assert!(forbidden_timeline_storage_name_v1(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn derived_access_evaluator_rejects_incomplete_or_ambiguous_rows() {
        let passing = complete_package();
        assert_eq!(
            evaluate_qualification_derived_access_v1(&passing)
                .expect("evaluation")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::SurvivesApfsFalsifier
        );

        let mut missing_change_reads = passing.clone();
        missing_change_reads.evaluator_revision =
            QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3.to_owned();
        missing_change_reads.evaluator_procedure_sha256 =
            qualification_derived_access_evaluator_v3_procedure_sha256();
        let evaluation = evaluate_qualification_derived_access_v1(&missing_change_reads)
            .expect("the successor evaluator classifies missing Change-read evidence");
        assert_eq!(
            evaluation.outcome,
            QualificationDerivedAccessTerminalOutcomeV1::InsufficientEvidence
        );
        assert!(
            evaluation
                .missing_or_unknown_criteria
                .iter()
                .any(|criterion| criterion == "Change read matrix")
        );

        let mut complete_change_reads = passing.clone();
        complete_change_reads.evaluator_revision =
            QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3.to_owned();
        complete_change_reads.evaluator_procedure_sha256 =
            qualification_derived_access_evaluator_v3_procedure_sha256();
        complete_change_reads.product_identities = complete_change_reads
            .execution_identities
            .iter()
            .map(product_identity)
            .collect();
        complete_change_reads.change_control_binary_identities = complete_change_reads
            .execution_identities
            .iter()
            .flat_map(|execution| {
                QualificationDerivedChangeControlBinaryKindV1::ALL
                    .into_iter()
                    .map(|kind| control_binary_identity(execution, kind))
            })
            .collect();
        let product_identities = complete_change_reads.product_identities.clone();
        let execution_identities = complete_change_reads.execution_identities.clone();
        let control_binary_identities = complete_change_reads
            .change_control_binary_identities
            .clone();
        complete_change_reads.change_read_rows = [
            QualificationDerivedAccessPlatformV1::MacosApfs,
            QualificationDerivedAccessPlatformV1::WindowsNtfs,
        ]
        .into_iter()
        .flat_map(|platform| {
            let product_identities = product_identities.clone();
            let execution_identities = execution_identities.clone();
            required_change_read_rows_v1().map(move |(fixture, case)| {
                let semantic = digest(&format!("{platform:?}/{fixture:?}/{case:?}"));
                let (oracle, http_status, typed_code) =
                    qualification_derived_change_expected_outcome_v1(platform, fixture, case);
                let typed_failure = oracle == QualificationDerivedChangeReadOracleV1::TypedFailure;
                let typed_document =
                    typed_code.map(|code| QualificationDerivedChangeTypedDocumentV1 {
                        schema: if code == "stale_projection" {
                            "pointbreak.inspect-change-page-error"
                        } else {
                            "pointbreak.inspect-change-projection-error"
                        }
                        .to_owned(),
                        version: 1,
                        code: code.to_owned(),
                        retryable: (code != "stale_projection").then_some(false),
                        canonical_sha256: digest(&format!(
                            "{platform:?}/{fixture:?}/{case:?}/typed"
                        )),
                    });
                let product_identity_sha256 = product_identities
                    .iter()
                    .find(|identity| identity.platform == platform)
                    .expect("product identity")
                    .canonical_sha256()
                    .expect("product identity hash");
                let counter_execution_identity_sha256 = execution_identities
                    .iter()
                    .find(|identity| identity.platform == platform)
                    .expect("execution identity")
                    .canonical_sha256()
                    .expect("execution identity hash");
                let mut counters = LongitudinalCountersV1::default();
                if fixture.required_cases().first() == Some(&case)
                    || matches!(
                        case,
                        QualificationDerivedChangeReadCaseV1::FreshProcessSuite
                            | QualificationDerivedChangeReadCaseV1::PostAppendFreshProcessSuite
                    )
                {
                    counters.change_capability_carriers_opened = 2;
                }
                let direct_work_expected = case != QualificationDerivedChangeReadCaseV1::Profile
                    && (!typed_failure
                        || fixture == QualificationDerivedChangeFixtureV1::TopologyV1);
                if direct_work_expected {
                    let topology = fixture == QualificationDerivedChangeFixtureV1::TopologyV1;
                    counters.change_candidates = if topology { 14 } else { 1 };
                    counters.change_candidate_current_revisions = if topology {
                        14
                    } else if matches!(
                        fixture,
                        QualificationDerivedChangeFixtureV1::IncompleteV1
                            | QualificationDerivedChangeFixtureV1::CycleConflictedV1
                    ) {
                        0
                    } else {
                        1
                    };
                    if topology {
                        if matches!(
                            case,
                            QualificationDerivedChangeReadCaseV1::ConcurrentReaders
                                | QualificationDerivedChangeReadCaseV1::WarmReuseSuite
                        ) {
                            counters.change_candidates = 56;
                            counters.change_candidate_current_revisions = 56;
                        } else if matches!(
                            case,
                            QualificationDerivedChangeReadCaseV1::PageTokenSuite
                                | QualificationDerivedChangeReadCaseV1::FreshProcessSuite
                                | QualificationDerivedChangeReadCaseV1::PostAppendSuite
                                | QualificationDerivedChangeReadCaseV1::PostAppendFreshProcessSuite
                        ) {
                            counters.change_candidates = 28;
                            counters.change_candidate_current_revisions = 28;
                        }
                        let (proposal_opens, rows_emitted) = match case {
                            QualificationDerivedChangeReadCaseV1::ChangesBounded
                            | QualificationDerivedChangeReadCaseV1::AttentionBounded
                            | QualificationDerivedChangeReadCaseV1::StalePageToken => (2, 2),
                            QualificationDerivedChangeReadCaseV1::PageTokenSuite
                            | QualificationDerivedChangeReadCaseV1::FreshProcessSuite
                            | QualificationDerivedChangeReadCaseV1::PostAppendSuite
                            | QualificationDerivedChangeReadCaseV1::PostAppendFreshProcessSuite => {
                                (4, 4)
                            }
                            QualificationDerivedChangeReadCaseV1::ConcurrentReaders
                            | QualificationDerivedChangeReadCaseV1::WarmReuseSuite => (8, 8),
                            QualificationDerivedChangeReadCaseV1::SummaryQuery
                            | QualificationDerivedChangeReadCaseV1::SummaryFilterSuite => {
                                counters.change_matches = 1;
                                (14, 1)
                            }
                            _ => (14, 8),
                        };
                        counters.change_proposal_carriers_opened = proposal_opens;
                        counters.change_proposal_carriers_validated = proposal_opens;
                        counters.change_rows_emitted = rows_emitted;
                    } else {
                        counters.change_proposal_carriers_opened =
                            counters.change_candidate_current_revisions;
                        counters.change_proposal_carriers_validated =
                            counters.change_proposal_carriers_opened;
                        counters.change_rows_emitted = if matches!(
                            case,
                            QualificationDerivedChangeReadCaseV1::SummaryQuery
                                | QualificationDerivedChangeReadCaseV1::SummaryFilterSuite
                        ) {
                            0
                        } else {
                            1
                        };
                    }
                }
                let single_fixture_read = matches!(
                    case,
                    QualificationDerivedChangeReadCaseV1::ChangesBare
                        | QualificationDerivedChangeReadCaseV1::ChangesBounded
                        | QualificationDerivedChangeReadCaseV1::AttentionBare
                        | QualificationDerivedChangeReadCaseV1::AttentionBounded
                        | QualificationDerivedChangeReadCaseV1::SummaryQuery
                );
                match (fixture, single_fixture_read) {
                    (QualificationDerivedChangeFixtureV1::DuplicateEqualV1, true) => {
                        counters.change_proposal_carriers_opened = 2;
                        counters.change_proposal_carriers_validated = 2;
                        counters.change_rows_emitted = 1;
                    }
                    (QualificationDerivedChangeFixtureV1::RemovalV1, true) => {
                        counters.change_proposal_carriers_opened = 1;
                        counters.change_proposal_carriers_validated = 1;
                        counters.change_support_carriers_opened = 2;
                        counters.change_rows_emitted = 1;
                    }
                    _ => {}
                }
                counters.carrier_opens = counters
                    .change_capability_carriers_opened
                    .saturating_add(counters.change_proposal_carriers_opened)
                    .saturating_add(counters.change_support_carriers_opened);
                QualificationDerivedChangeReadEvidenceV1 {
                    platform,
                    fixture,
                    fixture_inventory_sha256: digest("fixture inventory"),
                    fixture_witness_sha256: digest("fixture witness"),
                    case,
                    semantic_process_scope:
                        QualificationDerivedAccessProcessScopeV1::InspectorServiceChild,
                    counter_process_scope: QualificationDerivedAccessProcessScopeV1::Driver,
                    product_identity_sha256,
                    counter_execution_identity_sha256,
                    status: QualificationDerivedAccessStatusV1::Passed,
                    oracle,
                    strict_semantic_sha256: (!typed_failure).then(|| semantic.clone()),
                    derived_semantic_sha256: semantic,
                    wire_contract_matches: true,
                    expected_http_status: http_status,
                    observed_http_status: http_status,
                    expected_code: typed_code.map(str::to_owned),
                    observed_code: typed_code.map(str::to_owned),
                    expected_typed_document: typed_document.clone(),
                    observed_typed_document: typed_document,
                    counters,
                }
            })
        })
        .collect();
        for row in &mut complete_change_reads.change_read_rows {
            row.counter_process_scope =
                QualificationDerivedAccessProcessScopeV1::QualificationHarness;
        }
        complete_change_reads.change_control_rows = [
            QualificationDerivedAccessPlatformV1::MacosApfs,
            QualificationDerivedAccessPlatformV1::WindowsNtfs,
        ]
        .into_iter()
        .flat_map(|platform| {
            let product_identity_sha256 = product_identities
                .iter()
                .find(|identity| identity.platform == platform)
                .expect("product identity")
                .canonical_sha256()
                .expect("product identity hash");
            let execution_identity_sha256 = execution_identities
                .iter()
                .find(|identity| identity.platform == platform)
                .expect("execution identity")
                .canonical_sha256()
                .expect("execution identity hash");
            let control_binary_identities = control_binary_identities.clone();
            QualificationDerivedChangeControlCaseV1::ALL
                .into_iter()
                .map(move |case| {
                    let (binary_kind, test_name) =
                        qualification_derived_change_control_test_v1(case);
                    let binary_identity = control_binary_identities
                        .iter()
                        .find(|identity| {
                            identity.platform == platform && identity.kind == binary_kind
                        })
                        .expect("control binary identity");
                    QualificationDerivedChangeControlEvidenceV1 {
                        platform,
                        case,
                        binary_kind,
                        test_name: test_name.to_owned(),
                        status: QualificationDerivedAccessStatusV1::Passed,
                        execution_identity_sha256: execution_identity_sha256.clone(),
                        product_identity_sha256: product_identity_sha256.clone(),
                        test_binary_identity_sha256: binary_identity
                            .canonical_sha256()
                            .expect("control binary identity hash"),
                        test_binary_sha256: binary_identity.binary_sha256.clone(),
                        command_sha256: qualification_derived_change_control_command_sha256_v1(
                            test_name,
                        ),
                        stdout_sha256: digest(&format!("{platform:?}/{case:?} stdout")),
                        stderr_sha256: digest(&format!("{platform:?}/{case:?} stderr")),
                        exit_code: 0,
                        tests_run: 1,
                        tests_passed: 1,
                    }
                })
        })
        .collect();
        complete_change_reads.change_storage_rows = [
            QualificationDerivedAccessPlatformV1::MacosApfs,
            QualificationDerivedAccessPlatformV1::WindowsNtfs,
        ]
        .into_iter()
        .flat_map(|platform| {
            let product_identity_sha256 = product_identities
                .iter()
                .find(|identity| identity.platform == platform)
                .expect("product identity")
                .canonical_sha256()
                .expect("product identity hash");
            let execution_identity_sha256 = execution_identities
                .iter()
                .find(|identity| identity.platform == platform)
                .expect("execution identity")
                .canonical_sha256()
                .expect("execution identity hash");
            let initial_product_identity_sha256 = product_identity_sha256.clone();
            let initial_execution_identity_sha256 = execution_identity_sha256.clone();
            QualificationDerivedChangeFixtureV1::ALL
                .into_iter()
                .map(move |fixture| QualificationDerivedChangeStorageEvidenceV1 {
                    platform,
                    fixture,
                    phase: QualificationDerivedChangeStoragePhaseV1::InitialPublication,
                    fixture_inventory_sha256: digest("fixture inventory"),
                    fixture_witness_sha256: digest("fixture witness"),
                    product_identity_sha256: initial_product_identity_sha256.clone(),
                    execution_identity_sha256: initial_execution_identity_sha256.clone(),
                    witness: storage_witness("initial checkpoint", fixture),
                })
                .chain(std::iter::once(
                    QualificationDerivedChangeStorageEvidenceV1 {
                        platform,
                        fixture: QualificationDerivedChangeFixtureV1::TopologyV1,
                        phase: QualificationDerivedChangeStoragePhaseV1::PostAppendCheckpoint,
                        fixture_inventory_sha256: digest("fixture inventory"),
                        fixture_witness_sha256: digest("fixture witness"),
                        product_identity_sha256,
                        execution_identity_sha256,
                        witness: storage_witness(
                            "post-append checkpoint",
                            QualificationDerivedChangeFixtureV1::TopologyV1,
                        ),
                    },
                ))
        })
        .collect();
        let complete_evaluation = evaluate_qualification_derived_access_v1(&complete_change_reads)
            .expect("complete successor evaluation");
        assert_eq!(
            complete_evaluation.outcome,
            QualificationDerivedAccessTerminalOutcomeV1::SurvivesApfsFalsifier,
            "{complete_evaluation:?}"
        );

        let mut missing_timeline = complete_change_reads.clone();
        missing_timeline.evaluator_revision =
            QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4.to_owned();
        missing_timeline.evaluator_procedure_sha256 =
            qualification_derived_access_evaluator_v4_procedure_sha256();
        let missing_timeline_evaluation =
            evaluate_qualification_derived_access_v1(&missing_timeline)
                .expect("v4 classifies missing Timeline evidence");
        assert_eq!(
            missing_timeline_evaluation.outcome,
            QualificationDerivedAccessTerminalOutcomeV1::InsufficientEvidence
        );
        assert!(
            missing_timeline_evaluation
                .missing_or_unknown_criteria
                .iter()
                .any(|criterion| criterion == "Timeline read matrix")
        );

        let mut complete_timeline = complete_change_reads.clone();
        add_complete_timeline_evidence(&mut complete_timeline);
        let complete_timeline_evaluation =
            evaluate_qualification_derived_access_v1(&complete_timeline)
                .expect("complete Timeline successor evaluation");
        assert_eq!(
            complete_timeline_evaluation.outcome,
            QualificationDerivedAccessTerminalOutcomeV1::SurvivesApfsFalsifier,
            "{complete_timeline_evaluation:?}"
        );

        let mut v3_with_timeline = complete_timeline.clone();
        v3_with_timeline.evaluator_revision =
            QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3.to_owned();
        v3_with_timeline.evaluator_procedure_sha256 =
            qualification_derived_access_evaluator_v3_procedure_sha256();
        assert!(evaluate_qualification_derived_access_v1(&v3_with_timeline).is_err());

        let mut duplicate_timeline = complete_timeline.clone();
        duplicate_timeline
            .timeline_read_rows
            .push(duplicate_timeline.timeline_read_rows[0].clone());
        assert!(evaluate_qualification_derived_access_v1(&duplicate_timeline).is_err());

        let mut fallback_timeline = complete_timeline.clone();
        let fallback_receipt = fallback_timeline
            .timeline_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedTimelineReadCaseV1::StructuredQuerySuite
            })
            .expect("structured Timeline row")
            .counter_receipts
            .first_mut()
            .expect("structured Timeline receipt");
        fallback_receipt.counters.authoritative_fallbacks = 1;
        fallback_receipt.receipt_sha256 = fallback_receipt
            .canonical_sha256()
            .expect("refreshed fallback receipt");
        assert_eq!(
            evaluate_qualification_derived_access_v1(&fallback_timeline)
                .expect("fallback Timeline evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut structured_as_exhaustive = complete_timeline.clone();
        let structured_receipt = structured_as_exhaustive
            .timeline_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedTimelineReadCaseV1::StructuredQuerySuite
            })
            .expect("structured Timeline row")
            .counter_receipts
            .first_mut()
            .expect("structured Timeline receipt");
        structured_receipt.counters.timeline_exhaustive_candidates = 1;
        structured_receipt.receipt_sha256 = structured_receipt
            .canonical_sha256()
            .expect("refreshed structured receipt");
        assert_eq!(
            evaluate_qualification_derived_access_v1(&structured_as_exhaustive)
                .expect("structured/exhaustive drift evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut over_limit_concurrent = complete_timeline.clone();
        let concurrent_receipt = over_limit_concurrent
            .timeline_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedTimelineReadCaseV1::ProcessLifecycleSuite
            })
            .expect("lifecycle Timeline row")
            .counter_receipts
            .iter_mut()
            .find(|receipt| receipt.operation == "timeline_concurrent_asc")
            .expect("concurrent Timeline receipt");
        concurrent_receipt.counters.timeline_sqlite_window_rows = 2;
        concurrent_receipt.counters.timeline_selected_carriers = 2;
        concurrent_receipt.counters.timeline_trust_support_carriers = 2;
        concurrent_receipt.counters.carrier_opens = 2;
        concurrent_receipt.counters.carrier_bytes_read = 2;
        concurrent_receipt.counters.event_decodes = 2;
        concurrent_receipt.counters.event_validations = 2;
        concurrent_receipt.receipt_sha256 = concurrent_receipt
            .canonical_sha256()
            .expect("refreshed concurrent receipt");
        assert_eq!(
            evaluate_qualification_derived_access_v1(&over_limit_concurrent)
                .expect("over-limit concurrent Timeline evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut zero_exhaustive_hydration = complete_timeline.clone();
        let exhaustive_receipt = zero_exhaustive_hydration
            .timeline_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedTimelineReadCaseV1::ExhaustiveQuerySuite
            })
            .expect("exhaustive Timeline row")
            .counter_receipts
            .first_mut()
            .expect("exhaustive Timeline receipt");
        exhaustive_receipt.counters.carrier_bytes_read = 0;
        exhaustive_receipt.receipt_sha256 = exhaustive_receipt
            .canonical_sha256()
            .expect("refreshed exhaustive receipt");
        assert_eq!(
            evaluate_qualification_derived_access_v1(&zero_exhaustive_hydration)
                .expect("zero-hydration exhaustive Timeline evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut copied_typed_error = complete_timeline.clone();
        copied_typed_error
            .timeline_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedTimelineReadCaseV1::PageTokenSuite
            })
            .expect("page-token Timeline row")
            .observed_typed_documents
            .first_mut()
            .expect("typed Timeline observation")
            .document
            .code = "stale_projection".to_owned();
        assert_eq!(
            evaluate_qualification_derived_access_v1(&copied_typed_error)
                .expect("copied typed Timeline error evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut mixed_authority = complete_timeline.clone();
        let structured_authority = &mut mixed_authority
            .timeline_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedTimelineReadCaseV1::StructuredQuerySuite
            })
            .expect("structured Timeline row")
            .authority;
        structured_authority.checkpoint_identity_after_sha256 =
            digest("mixed structured checkpoint");
        assert_eq!(
            evaluate_qualification_derived_access_v1(&mixed_authority)
                .expect("mixed Timeline authority evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut persisted_timeline = complete_timeline.clone();
        persisted_timeline.timeline_storage_rows[0].forbidden_probes[0].sqlite_match_count = 1;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&persisted_timeline)
                .expect("persisted Timeline evidence evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut request_local_storage_sentinels = complete_timeline.clone();
        request_local_storage_sentinels
            .timeline_storage_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::WindowsNtfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.phase == QualificationDerivedChangeStoragePhaseV1::InitialPublication
            })
            .expect("Windows Timeline storage row")
            .forbidden_probes
            .iter_mut()
            .find(|probe| {
                probe.kind
                    == QualificationDerivedTimelineForbiddenProbeKindV1::TimelineContinuationToken
            })
            .expect("request-local continuation-token probe")
            .sentinel_sha256 = Some(digest("distinct request-local Windows Timeline token"));
        assert_eq!(
            evaluate_qualification_derived_access_v1(&request_local_storage_sentinels)
                .expect("request-local Timeline storage sentinels evaluate")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::SurvivesApfsFalsifier
        );

        let mut unavailable_ready_token = complete_timeline.clone();
        unavailable_ready_token
            .timeline_storage_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.phase == QualificationDerivedChangeStoragePhaseV1::InitialPublication
            })
            .expect("ready Timeline storage row")
            .forbidden_probes
            .iter_mut()
            .find(|probe| {
                probe.kind
                    == QualificationDerivedTimelineForbiddenProbeKindV1::TimelineContinuationToken
            })
            .expect("ready continuation-token probe")
            .sentinel_sha256 = None;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&unavailable_ready_token)
                .expect("unavailable ready Timeline token evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut fabricated_fault_token = complete_timeline.clone();
        fabricated_fault_token
            .timeline_storage_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::DuplicateConflictV1
                    && row.phase == QualificationDerivedChangeStoragePhaseV1::InitialPublication
            })
            .expect("fault Timeline storage row")
            .forbidden_probes
            .iter_mut()
            .find(|probe| {
                probe.kind
                    == QualificationDerivedTimelineForbiddenProbeKindV1::TimelineContinuationToken
            })
            .expect("fault continuation-token probe")
            .sentinel_sha256 = Some(digest("fabricated fault token"));
        assert_eq!(
            evaluate_qualification_derived_access_v1(&fabricated_fault_token)
                .expect("fabricated fault Timeline token evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut missing_source_family = complete_timeline.clone();
        missing_source_family
            .timeline_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedTimelineReadCaseV1::StructuredQuerySuite
            })
            .expect("structured Timeline row")
            .authority
            .authoritative_event_family_counts
            .remove("review_note_imported");
        assert_eq!(
            evaluate_qualification_derived_access_v1(&missing_source_family)
                .expect("source-family omission evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut invalid_trust_transition = complete_timeline.clone();
        invalid_trust_transition
            .timeline_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedTimelineReadCaseV1::TrustSuite
            })
            .expect("trust Timeline row")
            .trust_transition
            .as_mut()
            .expect("trust transition")
            .status_after_by_event
            .insert("event:signed".to_owned(), "untrusted_key".to_owned());
        assert_eq!(
            evaluate_qualification_derived_access_v1(&invalid_trust_transition)
                .expect("trust transition drift evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut incomplete_concurrent_transition = complete_timeline.clone();
        incomplete_concurrent_transition
            .timeline_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedTimelineReadCaseV1::ProcessLifecycleSuite
            })
            .expect("concurrent Timeline row")
            .concurrent_trust_transition
            .as_mut()
            .expect("concurrent trust transition")
            .observed_status_by_operation
            .remove("timeline_concurrent_desc");
        assert_eq!(
            evaluate_qualification_derived_access_v1(&incomplete_concurrent_transition)
                .expect("incomplete concurrent transition evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let signature_row = complete_timeline
            .timeline_read_rows
            .iter()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedTimelineReadCaseV1::TrustSuite
            })
            .expect("invalid-signature Timeline row");
        let execution = complete_timeline
            .execution_identities
            .iter()
            .find(|identity| identity.platform == signature_row.platform)
            .expect("invalid-signature execution");
        for falsify in [
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure
                    .counter_receipt
                    .counters
                    .timeline_trust_support_carriers = 1;
                failure.counter_receipt.receipt_sha256 = failure
                    .counter_receipt
                    .canonical_sha256()
                    .expect("trust-support receipt");
            })
                as fn(&mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1),
            |failure| failure.strict_recovery_semantic_sha256 = digest("one-sided recovery"),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.phase_http_status[3] = 503;
                failure.strict_semantic_sha256 = failure.derived_semantic_sha256.clone();
            }),
            |failure| {
                failure.phase_process_identity_sha256[2] = digest("substituted mutated child");
            },
            |failure| failure.phase_timeline_projection_stamp[2] = "invalid".to_owned(),
            |failure| {
                failure.phase_timeline_projection_stamp[3] =
                    format!("sha256:{}", digest("one-sided reference derived stamp"));
            },
            |failure| {
                let derived = format!("sha256:{}", digest("substituted derived stamp"));
                let strict = format!("sha256:{}", digest("substituted strict stamp"));
                failure.phase_timeline_projection_stamp = [
                    derived.clone(),
                    strict.clone(),
                    strict.clone(),
                    derived,
                    strict,
                ];
            },
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.counter_receipt.counters.body_bytes_read = 1;
                failure.counter_receipt.receipt_sha256 = failure
                    .counter_receipt
                    .canonical_sha256()
                    .expect("body-byte receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.counter_receipt.counters.object_bytes_read = 1;
                failure.counter_receipt.receipt_sha256 = failure
                    .counter_receipt
                    .canonical_sha256()
                    .expect("object-byte receipt");
            }),
            |failure| failure.phase_authority_cursor_sha256[4] = digest("mixed cursor"),
            |failure| {
                failure.phase_authority_cursor_sha256[2] =
                    failure.phase_authority_cursor_sha256[0].clone();
            },
            |failure| failure.reference_trust_identity_restored_sha256 = digest("wrong trust"),
            |failure| failure.mutated_event_record_hash = format!("sha256:{}", digest("record")),
            |failure| {
                failure.fault_execution.root_provenance_sha256 =
                    failure.reference_root_identity_sha256.clone();
            },
            |failure| failure.fault_execution.command_sha256 = digest("fault command drift"),
            |failure| failure.fault_fixture_witness_sha256 = digest("wrong fault witness"),
            |failure| {
                failure.reference_recovery_inventory_sha256 =
                    digest("wrong reference recovery inventory")
            },
            |failure| failure.fault_clean_inventory_sha256 = digest("wrong fault clean inventory"),
            |failure| {
                failure.fault_restored_inventory_sha256 = digest("wrong fault restored inventory")
            },
            |failure| {
                failure.fault_derivative_inventory_sha256 =
                    failure.fault_clean_inventory_sha256.clone()
            },
            |failure| {
                failure.fault_trust_identity_staged_sha256 = digest("wrong fault staged trust")
            },
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.barrier_receipt.carrier_opens_before = 1;
                failure.barrier_receipt.receipt_sha256 = failure
                    .barrier_receipt
                    .canonical_sha256()
                    .expect("carrier-open barrier receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.barrier_receipt.selected_carriers_before = 0;
                failure.barrier_receipt.receipt_sha256 = failure
                    .barrier_receipt
                    .canonical_sha256()
                    .expect("empty-selection barrier receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.barrier_receipt.observed_mismatch_key_digest =
                    digest("different mismatch key");
                failure.barrier_receipt.receipt_sha256 = failure
                    .barrier_receipt
                    .canonical_sha256()
                    .expect("mismatched-key barrier receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.barrier_receipt.clean_carrier_sha256 = digest("different clean carrier");
                failure.barrier_receipt.receipt_sha256 = failure
                    .barrier_receipt
                    .canonical_sha256()
                    .expect("different-carrier barrier receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.counter_receipt.root_identity =
                    failure.reference_root_identity_sha256.clone();
                failure.counter_receipt.receipt_sha256 = failure
                    .counter_receipt
                    .canonical_sha256()
                    .expect("reference-root counter receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.counter_receipt.base_execution_identity_sha256 =
                    digest("wrong fault execution identity");
                failure.counter_receipt.receipt_sha256 = failure
                    .counter_receipt
                    .canonical_sha256()
                    .expect("wrong-execution counter receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.counter_receipt.manifest_sha256 =
                    failure.fault_derivative_inventory_sha256.clone();
                failure.counter_receipt.receipt_sha256 = failure
                    .counter_receipt
                    .canonical_sha256()
                    .expect("derivative-manifest counter receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.counter_receipt.semantic_result_sha256 = digest("wrong response semantic");
                failure.counter_receipt.receipt_sha256 = failure
                    .counter_receipt
                    .canonical_sha256()
                    .expect("wrong-semantic counter receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.barrier_receipt.run_identity = digest("wrong barrier run identity");
                failure.barrier_receipt.receipt_sha256 = failure
                    .barrier_receipt
                    .canonical_sha256()
                    .expect("wrong-run barrier receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.barrier_receipt.derivative_inventory_sha256 =
                    digest("wrong barrier derivative inventory");
                failure.barrier_receipt.receipt_sha256 = failure
                    .barrier_receipt
                    .canonical_sha256()
                    .expect("wrong-inventory barrier receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.fault_seed_receipt.authoritative_inventory_sha256 =
                    digest("wrong fault-seed inventory");
                failure.fault_seed_receipt.inclusive_inventory_sha256 =
                    digest("wrong fault-seed inventory");
                failure.fault_seed_receipt.receipt_sha256 = failure
                    .fault_seed_receipt
                    .canonical_sha256()
                    .expect("wrong-inventory fault-seed receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.fault_seed_receipt.witness_sha256 = digest("wrong fault-seed witness");
                failure.fault_seed_receipt.receipt_sha256 = failure
                    .fault_seed_receipt
                    .canonical_sha256()
                    .expect("wrong-witness fault-seed receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.fault_seed_receipt.fault_root_path_sha256 = failure
                    .fault_seed_receipt
                    .reference_root_path_sha256
                    .clone();
                failure.fault_seed_receipt.receipt_sha256 = failure
                    .fault_seed_receipt
                    .canonical_sha256()
                    .expect("overlapping-path fault-seed receipt");
            }),
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.fault_seed_receipt.schema =
                    "pointbreak.wrong-fault-seed-schema.v1".to_owned();
                failure.fault_seed_receipt.receipt_sha256 = failure
                    .fault_seed_receipt
                    .canonical_sha256()
                    .expect("wrong-schema fault-seed receipt");
            }),
            // The tree manifest is bound to the live roots at runtime by
            // validate_timeline_fault_seed_bindings_v1, not statically by this
            // evaluator; the un-rehashed mutation below falsifies only the
            // receipt's canonical self-hash.
            (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                failure.fault_seed_receipt.tree_manifest_sha256 =
                    digest("stale fault-seed manifest");
            }),
        ] {
            let mut row = signature_row.clone();
            falsify(row.invalid_signature_failure.as_mut().expect("witness"));
            assert!(!timeline_invalid_signature_failure_valid_v1(
                &row,
                Some(execution)
            ));
        }

        // The repaired fault counter contract names its failing condition:
        // carrier opens are bounded by the primary hydration set, the
        // validation count witnesses the abort at the mutated carrier, and
        // the selection-time classifications keep the normal-path shape
        // bounds instead of blind zero pins.
        for (falsify, expected_condition) in [
            (
                (|failure: &mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1| {
                    failure.counter_receipt.counters.carrier_opens = 2;
                    failure.counter_receipt.counters.carrier_bytes_read = 2;
                    failure.counter_receipt.counters.event_validations = 1;
                })
                    as fn(&mut QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1),
                "exceed the primary hydration set",
            ),
            (
                |failure| {
                    failure.counter_receipt.counters.event_validations = 1;
                },
                "do not witness an abort at the mutated carrier",
            ),
            (
                |failure| {
                    failure
                        .counter_receipt
                        .counters
                        .timeline_revision_candidate_carriers = 3;
                },
                "revision candidate carriers exceed twice the selected carriers",
            ),
            (
                |failure| {
                    failure
                        .counter_receipt
                        .counters
                        .timeline_correlation_support_carriers = 3;
                },
                "correlation support carriers exceed twice the selected carriers",
            ),
        ] {
            let mut row = signature_row.clone();
            let failure = row.invalid_signature_failure.as_mut().expect("witness");
            falsify(failure);
            failure.counter_receipt.receipt_sha256 = failure
                .counter_receipt
                .canonical_sha256()
                .expect("falsified counter receipt rehashes");
            let error = timeline_invalid_signature_failure_check_v1(&row, Some(execution))
                .expect_err("falsified counter contract must fail");
            assert!(
                error.contains(expected_condition),
                "expected {expected_condition:?} in {error:?}"
            );
        }

        // The repaired bound must ADMIT the observed real fault shape the old
        // blind pins rejected: the primary hydration batch (selected=1,
        // candidates=1, correlation=2) aborts at the mutated carrier after two
        // opens, having validated exactly the one clean carrier before it.
        {
            let mut row = signature_row.clone();
            let failure = row.invalid_signature_failure.as_mut().expect("witness");
            let counters = &mut failure.counter_receipt.counters;
            counters.carrier_opens = 2;
            counters.carrier_bytes_read = 2;
            counters.event_decodes = 1;
            counters.event_validations = 1;
            counters.timeline_revision_candidate_carriers = 1;
            counters.timeline_correlation_support_carriers = 2;
            counters.timeline_sqlite_candidates = 108;
            counters.timeline_sqlite_facet_rows = 108;
            failure.counter_receipt.receipt_sha256 = failure
                .counter_receipt
                .canonical_sha256()
                .expect("observed-shape counter receipt rehashes");
            timeline_invalid_signature_failure_check_v1(&row, Some(execution))
                .expect("the observed real fault counter shape is admitted");
        }

        let mut dead_bounded_instrumentation = complete_change_reads.clone();
        let dead_bounded = dead_bounded_instrumentation
            .change_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedChangeReadCaseV1::ChangesBounded
            })
            .expect("bounded topology Change row");
        dead_bounded.counters = LongitudinalCountersV1::default();
        assert_eq!(
            evaluate_qualification_derived_access_v1(&dead_bounded_instrumentation)
                .expect("dead bounded instrumentation evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut candidate_wide_bounded_hydration = complete_change_reads.clone();
        let candidate_wide = candidate_wide_bounded_hydration
            .change_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedChangeReadCaseV1::ChangesBounded
            })
            .expect("bounded topology Change row");
        candidate_wide.counters.change_candidates = 14;
        candidate_wide.counters.change_candidate_current_revisions = 14;
        candidate_wide.counters.change_proposal_carriers_opened = 14;
        candidate_wide.counters.change_proposal_carriers_validated = 14;
        candidate_wide.counters.change_rows_emitted = 2;
        candidate_wide.counters.carrier_opens = 14;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&candidate_wide_bounded_hydration)
                .expect("candidate-wide bounded hydration evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut support_wide_bounded_hydration = complete_change_reads.clone();
        let support_wide = support_wide_bounded_hydration
            .change_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedChangeReadCaseV1::ChangesBounded
            })
            .expect("bounded topology Change row");
        support_wide.counters.change_support_carriers_opened = 9;
        support_wide.counters.carrier_opens = support_wide
            .counters
            .change_proposal_carriers_opened
            .saturating_add(9);
        assert_eq!(
            evaluate_qualification_derived_access_v1(&support_wide_bounded_hydration)
                .expect("support-wide bounded hydration evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut zero_control_tests = complete_change_reads.clone();
        zero_control_tests.change_control_rows[0].tests_run = 0;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&zero_control_tests)
                .expect("zero-test control evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut drifted_control_source = complete_change_reads.clone();
        drifted_control_source.change_control_binary_identities[0].source_commit =
            if drifted_control_source.change_control_binary_identities[0].source_commit
                == "a".repeat(40)
            {
                "b".repeat(40)
            } else {
                "a".repeat(40)
            };
        assert!(evaluate_qualification_derived_access_v1(&drifted_control_source).is_err());

        let mut arbitrary_storage_probe = complete_change_reads.clone();
        let witness = &mut arbitrary_storage_probe.change_storage_rows[0].witness;
        witness
            .forbidden_probes
            .iter_mut()
            .find(|probe| {
                probe.kind == QualificationDerivedStorageForbiddenProbeKindV1::ProposalSummary
            })
            .expect("summary probe")
            .sentinel_sha256 = digest("arbitrary absent summary");
        witness.refresh_sha256().expect("changed storage witness");
        assert_eq!(
            evaluate_qualification_derived_access_v1(&arbitrary_storage_probe)
                .expect("arbitrary storage probe evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut forbidden_storage_table = complete_change_reads.clone();
        let witness = &mut forbidden_storage_table.change_storage_rows[0].witness;
        witness.sqlite_catalog.entries.push(
            crate::bench_support::derived_access::QualificationDerivedStorageCatalogEntryV1 {
                schema: "main".to_owned(),
                name: "payload_documents".to_owned(),
                kind: "table".to_owned(),
                declared_column_count: 0,
                strict: true,
                without_rowid: false,
                columns: Vec::new(),
                indexes: Vec::new(),
            },
        );
        witness.sqlite_catalog.catalog_sha256 =
            canonical_sha256(&witness.sqlite_catalog.entries).expect("catalog hash");
        witness.refresh_sha256().expect("forbidden-table witness");
        assert_eq!(
            evaluate_qualification_derived_access_v1(&forbidden_storage_table)
                .expect("forbidden storage table evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut downgraded_change_reads = complete_change_reads.clone();
        downgraded_change_reads.evaluator_revision =
            QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V2.to_owned();
        assert!(
            evaluate_qualification_derived_access_v1(&downgraded_change_reads).is_err(),
            "successor Change rows must not be silently ignored by evaluator v2"
        );

        let mut mismatched_change_read = complete_change_reads.clone();
        mismatched_change_read.change_read_rows[0].derived_semantic_sha256 = digest("mismatch");
        assert_eq!(
            evaluate_qualification_derived_access_v1(&mismatched_change_read)
                .expect("mismatched Change read evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut mismatched_typed_document = complete_change_reads.clone();
        mismatched_typed_document
            .change_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedChangeReadCaseV1::StalePageToken
            })
            .and_then(|row| row.observed_typed_document.as_mut())
            .expect("typed Change document")
            .canonical_sha256 = digest("drifted typed document");
        assert_eq!(
            evaluate_qualification_derived_access_v1(&mismatched_typed_document)
                .expect("mismatched typed Change document evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut warm_capability_reopen = complete_change_reads.clone();
        let warm = warm_capability_reopen
            .change_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
                    && row.case == QualificationDerivedChangeReadCaseV1::ChangesBare
            })
            .expect("warm Change row");
        warm.counters.change_capability_carriers_opened = 2;
        warm.counters.carrier_opens = 2;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&warm_capability_reopen)
                .expect("warm capability reopen evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut incomplete_removal_support = complete_change_reads.clone();
        let removal = incomplete_removal_support
            .change_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::RemovalV1
                    && row.case == QualificationDerivedChangeReadCaseV1::ChangesBounded
            })
            .expect("removal Change row");
        removal.counters.change_support_carriers_opened = 1;
        removal.counters.carrier_opens -= 1;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&incomplete_removal_support)
                .expect("incomplete removal support evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut mixed_fixture_authority = complete_change_reads.clone();
        mixed_fixture_authority
            .change_read_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::WindowsNtfs
                    && row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
            })
            .expect("Windows topology row")
            .fixture_witness_sha256 = digest("different topology witness");
        assert_eq!(
            evaluate_qualification_derived_access_v1(&mixed_fixture_authority)
                .expect("mixed fixture authority evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut unclassified_change_open = complete_change_reads;
        unclassified_change_open.change_read_rows[0]
            .counters
            .carrier_opens = 1;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&unclassified_change_open)
                .expect("unclassified Change carrier open evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut missing = passing.clone();
        missing.operation_rows.pop();
        assert_eq!(
            evaluate_qualification_derived_access_v1(&missing)
                .expect("evaluation")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::InsufficientEvidence
        );

        let mut unknown = passing.clone();
        unknown.operation_rows[0].status = QualificationDerivedAccessStatusV1::Unknown;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&unknown)
                .expect("evaluation")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::InsufficientEvidence
        );

        let mut duplicate = passing.clone();
        duplicate
            .operation_rows
            .push(duplicate.operation_rows[0].clone());
        assert!(evaluate_qualification_derived_access_v1(&duplicate).is_err());

        let mut wrong_provenance = passing;
        wrong_provenance.execution_identities[0].source_dirty = true;
        assert!(evaluate_qualification_derived_access_v1(&wrong_provenance).is_err());

        let mut wrong_evaluator = complete_package();
        wrong_evaluator.evaluator_revision = "pointbreak.other-evaluator.v1".to_owned();
        assert!(evaluate_qualification_derived_access_v1(&wrong_evaluator).is_err());

        let mut mixed_source = complete_package();
        mixed_source.execution_identities[1].source_tree = "a".repeat(40);
        assert!(evaluate_qualification_derived_access_v1(&mixed_source).is_err());

        let mut reused_host_authority = complete_package();
        reused_host_authority.execution_identities[1].host_identity_sha256 = reused_host_authority
            .execution_identities[0]
            .host_identity_sha256
            .clone();
        assert_eq!(
            validate_execution_identities(&reused_host_authority).unwrap_err(),
            "derived-access execution identities reuse one campaign-host authority"
        );

        let mut duplicate_identity = complete_package();
        duplicate_identity
            .execution_identities
            .push(duplicate_identity.execution_identities[0].clone());
        assert!(evaluate_qualification_derived_access_v1(&duplicate_identity).is_err());

        let mut distinct_command = complete_package();
        let mut second_macos_command = distinct_command.execution_identities[0].clone();
        second_macos_command.command_sha256 = "c".repeat(64);
        distinct_command
            .execution_identities
            .push(second_macos_command);
        assert!(evaluate_qualification_derived_access_v1(&distinct_command).is_ok());

        let mut mixed_native_binary = complete_package();
        let mut second_macos_binary = mixed_native_binary.execution_identities[0].clone();
        second_macos_binary.command_sha256 = "c".repeat(64);
        second_macos_binary.binary_sha256 = "d".repeat(64);
        mixed_native_binary
            .execution_identities
            .push(second_macos_binary);
        assert!(evaluate_qualification_derived_access_v1(&mixed_native_binary).is_err());

        let mut missing_timing = complete_package();
        let l100 = missing_timing
            .operation_rows
            .iter_mut()
            .find(|row| row.tier == QualificationDerivedAccessTierV1::L100)
            .expect("L100 row");
        l100.wall_p95_ms = None;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&missing_timing)
                .expect("evaluation")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::InsufficientEvidence
        );

        let mut missing_ratio = complete_package();
        let c262 = missing_ratio
            .operation_rows
            .iter_mut()
            .find(|row| row.tier == QualificationDerivedAccessTierV1::C262)
            .expect("C262 row");
        c262.l100_to_c262_selected_work_ratio_milli = None;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&missing_ratio)
                .expect("evaluation")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::InsufficientEvidence
        );

        let mut forged_ratio = complete_package();
        let c262 = forged_ratio
            .operation_rows
            .iter_mut()
            .find(|row| row.tier == QualificationDerivedAccessTierV1::C262)
            .expect("C262 row");
        c262.l100_to_c262_selected_work_ratio_milli = Some(1_000);
        assert!(evaluate_qualification_derived_access_v1(&forged_ratio).is_err());

        let mut hidden_bootstrap_high_water = complete_package();
        hidden_bootstrap_high_water.bootstrap_rows[0].high_water_derived_bytes = 97 * MIB;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&hidden_bootstrap_high_water)
                .expect("evaluation")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut wrong_d0_schedule = complete_package();
        wrong_d0_schedule.d0_rows[0].schedule_sha256 = "a".repeat(64);
        assert_eq!(
            evaluate_qualification_derived_access_v1(&wrong_d0_schedule)
                .expect("evaluation")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut missing_allocation = complete_package();
        missing_allocation.allocation_rows.pop();
        assert_eq!(
            evaluate_qualification_derived_access_v1(&missing_allocation)
                .expect("evaluation")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::InsufficientEvidence
        );

        let mut governed_early_stop = complete_package();
        governed_early_stop.execution_identities.retain(|identity| {
            identity.platform == QualificationDerivedAccessPlatformV1::MacosApfs
        });
        governed_early_stop
            .d0_rows
            .retain(|row| row.platform == QualificationDerivedAccessPlatformV1::MacosApfs);
        governed_early_stop
            .operation_rows
            .retain(|row| row.platform == QualificationDerivedAccessPlatformV1::MacosApfs);
        governed_early_stop
            .lifecycle_rows
            .retain(|row| row.platform == QualificationDerivedAccessPlatformV1::MacosApfs);
        governed_early_stop.complete = false;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&governed_early_stop)
                .expect("governed early-stop evaluation")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::InsufficientEvidence
        );
    }

    #[test]
    fn selected_output_growth_is_not_unselected_scale_work() {
        assert!(!selected_work_growth_exceeds_bound(10, 0, 15, 0, 1_250));
        assert!(!selected_work_growth_exceeds_bound(11, 0, 21, 0, 1_250));
        assert!(selected_work_growth_exceeds_bound(10, 0, 16, 1, 1_250));
        assert!(selected_work_growth_exceeds_bound(10, 0, 15, 1, 1_250));

        let mut explained_growth = complete_package();
        for (operation, l100_count, c262_count) in [
            (
                QualificationDerivedAccessOperationV1::RevisionDetailActive,
                10,
                15,
            ),
            (
                QualificationDerivedAccessOperationV1::RevisionDetailRemoved,
                11,
                21,
            ),
        ] {
            let l100 = explained_growth
                .operation_rows
                .iter_mut()
                .find(|row| {
                    row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                        && row.tier == QualificationDerivedAccessTierV1::L100
                        && row.operation == operation
                })
                .expect("L100 operation");
            l100.selected_output_count = Some(l100_count);
            l100.unselected_work_count = Some(0);
            l100.selected_work_count = l100_count;

            let c262 = explained_growth
                .operation_rows
                .iter_mut()
                .find(|row| {
                    row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                        && row.tier == QualificationDerivedAccessTierV1::C262
                        && row.operation == operation
                })
                .expect("C262 operation");
            c262.selected_output_count = Some(c262_count);
            c262.unselected_work_count = Some(0);
            c262.selected_work_count = c262_count;
            c262.l100_to_c262_selected_work_ratio_milli =
                Some(selected_work_ratio_milli(c262_count, l100_count));
        }
        assert_eq!(
            evaluate_qualification_derived_access_v1(&explained_growth)
                .expect("explained selected-output growth evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::SurvivesApfsFalsifier
        );

        let c262 = explained_growth
            .operation_rows
            .iter_mut()
            .find(|row| {
                row.platform == QualificationDerivedAccessPlatformV1::MacosApfs
                    && row.tier == QualificationDerivedAccessTierV1::C262
                    && row.operation == QualificationDerivedAccessOperationV1::RevisionDetailActive
            })
            .expect("C262 operation");
        c262.selected_work_count += 1;
        c262.unselected_work_count = Some(1);
        c262.l100_to_c262_selected_work_ratio_milli =
            Some(selected_work_ratio_milli(c262.selected_work_count, 10));
        assert_eq!(
            evaluate_qualification_derived_access_v1(&explained_growth)
                .expect("unselected growth evaluates")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );
    }

    #[test]
    fn derived_access_evaluator_rejects_body_bearing_or_whole_history_work() {
        let mut body_bearing = complete_package();
        body_bearing
            .resources
            .as_mut()
            .expect("resources")
            .retained_body_object_bytes_outside_active_window = 1;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&body_bearing)
                .expect("evaluation")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut whole_history = complete_package();
        whole_history.operation_rows[0].complexity =
            QualificationDerivedAccessComplexityV1::HistoryOrCardinalityProportional;
        assert_eq!(
            evaluate_qualification_derived_access_v1(&whole_history)
                .expect("evaluation")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::Reject
        );

        let mut incomplete_lifecycle = complete_package();
        incomplete_lifecycle.lifecycle_rows.pop();
        assert_eq!(
            evaluate_qualification_derived_access_v1(&incomplete_lifecycle)
                .expect("evaluation")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::InsufficientEvidence
        );
    }

    #[test]
    fn derived_access_contract_publication_fixture_and_docs_are_byte_stable() {
        let publication = qualification_derived_access_contract_v1_publication();
        let json = format!(
            "{}\n",
            serde_json::to_string_pretty(&qualification_derived_access_contract_fixture_v1())
                .expect("contract fixture JSON")
        );
        let fixture = include_str!("../../../tests/fixtures/derived-access/contract-v1.json")
            .replace("\r\n", "\n");
        assert_eq!(json, fixture);

        let docs = include_str!("../../../docs/benchmarking.md");
        let table = docs
            .split_once("<!-- derived-access-contract-v1:start -->\n")
            .expect("derived-access docs start marker")
            .1
            .split_once("\n<!-- derived-access-contract-v1:end -->")
            .expect("derived-access docs end marker")
            .0;
        assert_eq!(table, publication.decision_table_markdown);

        let serialized = serde_json::to_string(&publication).expect("publication JSON");
        let derivation =
            serde_json::to_value(&publication.contract.derivation).expect("derivation JSON");
        let derivation_fields = derivation
            .as_object()
            .expect("derivation object")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            derivation_fields,
            BTreeSet::from([
                "candidateMeasurementsUsed",
                "cargoLockSha256",
                "longitudinalSynthesisSha256",
                "pointbreakCommit",
                "pointbreakTree",
                "privateCorpusUsed",
            ])
        );
        for forbidden in [
            "\"proposedProfileId\":",
            "\"operationRows\":",
            "\"lifecycleRows\":",
            "\"resources\":",
            "\"allocationRows\":",
            "\"bootstrapRows\":",
            "\"outcome\":",
        ] {
            assert!(!serialized.contains(forbidden), "published {forbidden}");
        }
    }
}
