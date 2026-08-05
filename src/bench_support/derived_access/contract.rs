use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

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
    const NATIVE: [Self; 3] = [Self::D0_128, Self::L1, Self::L7];
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
        evaluator_revision: QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V2.to_owned(),
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessPackageV1 {
    pub schema: String,
    pub evaluator_revision: String,
    pub proposed_profile_id: String,
    pub execution_identities: Vec<QualificationDerivedAccessExecutionIdentityV1>,
    pub root_bindings: Vec<QualificationDerivedAccessRootBindingV1>,
    pub d0_rows: Vec<QualificationDerivedAccessD0EvidenceV1>,
    pub operation_rows: Vec<QualificationDerivedAccessOperationEvidenceV1>,
    pub lifecycle_rows: Vec<QualificationDerivedAccessLifecycleEvidenceV1>,
    pub resources: Option<QualificationDerivedAccessResourceEvidenceV1>,
    pub allocation_rows: Vec<QualificationDerivedAccessAllocationEvidenceV1>,
    pub bootstrap_rows: Vec<QualificationDerivedAccessBootstrapEvidenceV1>,
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
        || package.evaluator_revision != QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V2
        || package.proposed_profile_id.trim().is_empty()
    {
        return Err("unsupported derived-access package".to_owned());
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
        evaluator_revision: QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V2.to_owned(),
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
    fn derived_access_evaluator_rejects_incomplete_or_ambiguous_rows() {
        let passing = complete_package();
        assert_eq!(
            evaluate_qualification_derived_access_v1(&passing)
                .expect("evaluation")
                .outcome,
            QualificationDerivedAccessTerminalOutcomeV1::SurvivesApfsFalsifier
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
