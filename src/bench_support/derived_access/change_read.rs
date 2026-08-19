use std::path::{Path, PathBuf};
#[cfg(feature = "longitudinal-counting")]
use std::process::{Child, Command, Stdio};
#[cfg(feature = "longitudinal-counting")]
use std::sync::{Arc, Barrier, Mutex, PoisonError};
#[cfg(feature = "longitudinal-counting")]
use std::thread;
#[cfg(feature = "longitudinal-counting")]
use std::time::{Duration, Instant};
#[cfg(feature = "longitudinal-counting")]
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead as _, BufReader, Read as _, Write as _},
    net::{Shutdown, TcpStream},
};

#[cfg(feature = "longitudinal-counting")]
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE_NO_PAD},
};
use serde::{Deserialize, Serialize};
#[cfg(feature = "longitudinal-counting")]
use serde_json::{Value, json};
#[cfg(feature = "longitudinal-counting")]
use sha2::{Digest as _, Sha256};

#[cfg(feature = "longitudinal-counting")]
use super::DerivedStorageLayout;
#[cfg(feature = "longitudinal-counting")]
use super::QUALIFICATION_DERIVED_TIMELINE_FAULT_SEED_RECEIPT_SCHEMA_V1;
use super::evidence::QualificationDerivedChangeReadReceiptV2;
#[cfg(feature = "longitudinal-counting")]
use super::evidence::validate_current_execution_identity_v1;
#[cfg(feature = "longitudinal-counting")]
use super::evidence::{
    QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V2, QualificationDerivedChangeReadReceiptV1,
};
#[cfg(feature = "longitudinal-counting")]
use super::{
    QUALIFICATION_DERIVED_ACCESS_CONTRACT_SCHEMA_V1,
    QUALIFICATION_DERIVED_ACCESS_CONTRACT_SHA256_V1,
    QUALIFICATION_DERIVED_CHANGE_ACTIVATION_FIXTURE_V1,
    QUALIFICATION_DERIVED_CHANGE_COMPLETION_FIXTURE_V1,
    QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V1,
    QUALIFICATION_TIMELINE_ADMITTED_EVENT_FAMILIES_V1, QUALIFICATION_TIMELINE_EXCLUDED_CASES_V1,
    QUALIFICATION_TIMELINE_INVALID_SIGNATURE_MUTATION_RECIPE_SHA256_V1,
    QUALIFICATION_TIMELINE_SOURCE_EVENT_FAMILIES_V1, QualificationDerivedAccessPlatformV1,
    QualificationDerivedAccessProcessScopeV1, QualificationDerivedAccessStatusV1,
    QualificationDerivedChangeControlBinaryIdentityV1, QualificationDerivedChangeControlEvidenceV1,
    QualificationDerivedChangeFixtureKindV1, QualificationDerivedChangeFixtureRequestV1,
    QualificationDerivedChangeFixtureWitnessV1, QualificationDerivedChangeReadEvidenceV1,
    QualificationDerivedChangeReadOracleV1, QualificationDerivedChangeStorageEvidenceV1,
    QualificationDerivedChangeStoragePhaseV1, QualificationDerivedChangeTypedDocumentV1,
    QualificationDerivedStorageForbiddenProbeHashesV1,
    QualificationDerivedTimelineAuthorityEvidenceV1,
    QualificationDerivedTimelineConcurrentTrustEvidenceV1,
    QualificationDerivedTimelineExclusionCountsV1,
    QualificationDerivedTimelineForbiddenProbeEvidenceV1,
    QualificationDerivedTimelineForbiddenProbeKindV1,
    QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1,
    QualificationDerivedTimelineReadCaseV1, QualificationDerivedTimelineReadOracleV1,
    QualificationDerivedTimelineStorageEvidenceV1, QualificationDerivedTimelineTrustTransitionV1,
    QualificationDerivedTimelineTypedObservationV1,
    capture_qualification_derived_storage_witness_v1, expected_timeline_typed_documents_v1,
    materialize_qualification_derived_change_fixture_v1,
    qualification_derived_change_control_attestation_test_v1,
    qualification_derived_change_control_command_sha256_v1,
    qualification_derived_change_expected_outcome_v1,
    qualification_derived_change_storage_probe_hashes_v1, required_timeline_cases_v1,
    scan_qualification_derived_storage_v1, timeline_invalid_signature_run_identity_v1,
    timeline_request_schedule_sha256_v1, timeline_request_schedule_v1,
};
use super::{
    QualificationDerivedAccessExecutionIdentityV1, QualificationDerivedAccessProductIdentityV1,
    QualificationDerivedChangeControlBinaryKindV1, QualificationDerivedChangeControlCaseV1,
    QualificationDerivedChangeEvidencePurposeV1, QualificationDerivedChangeFixtureV1,
    QualificationDerivedChangeReadCaseV1, QualificationDerivedStorageForbiddenProbeInputV1,
    QualificationDerivedTimelineFaultSeedReceiptV1, QualificationDerivedTimelineReadEvidenceV1,
    qualification_derived_change_control_build_command_sha256_v1,
    qualification_derived_change_control_test_v1,
};
#[cfg(feature = "longitudinal-counting")]
use crate::bench_support::longitudinal::{
    LONGITUDINAL_COUNTER_RECEIPT_HEADER_V1, LONGITUDINAL_COUNTING_REQUEST_HEADER_V1,
    LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_RECEIPT_HEADER_V1,
    LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_REQUEST_SCHEMA_V1,
    LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_ROOT_ENV_V1,
    LONGITUDINAL_TIMELINE_POST_PIN_RELEASE_SCHEMA_V1, LongitudinalCounterReceiptContextV1,
    LongitudinalCounterReceiptV1, LongitudinalCountersV1, LongitudinalCountingScopeV1,
    LongitudinalTimelinePostPinBarrierReceiptV1, LongitudinalTimelinePostPinBarrierRequestV1,
    LongitudinalTimelinePostPinReadyV1, LongitudinalTimelinePostPinReleaseV1,
    longitudinal_authoritative_store_data_inventory_v1, longitudinal_store_data_inventory_v1,
    longitudinal_timeline_post_pin_ready_path_v1, longitudinal_timeline_post_pin_release_path_v1,
    read_longitudinal_timeline_barrier_document_v1,
    write_longitudinal_timeline_barrier_document_v1,
};
#[cfg(feature = "longitudinal-counting")]
use crate::canonical_hash::canonical_json_bytes;
use crate::canonical_hash::sha256_bytes_hex;
#[cfg(feature = "longitudinal-counting")]
use crate::crypto::{EventSignatureBytes, EventVerificationStatus};
#[cfg(feature = "longitudinal-counting")]
use crate::session::derived_access::lifecycle::DerivedAccessLifecycle;
#[cfg(feature = "longitudinal-counting")]
use crate::session::derived_access::product_contract::{
    DerivedAccessAvailability, DerivedAccessProfile,
};
#[cfg(feature = "longitudinal-counting")]
use crate::session::derived_access::writer::DerivedWriteCoordinator;
#[cfg(feature = "longitudinal-counting")]
use crate::session::event::{
    EventTarget, EventType, InputRequestOpenedPayload, InputRequestRespondedPayload,
    ReviewInitializedPayload, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload, Writer,
};
use crate::session::store_dir_for_repo;
#[cfg(feature = "longitudinal-counting")]
use crate::session::{
    ChangeLifecycleV1, ChangeTopologyV1, DerivedAttentionPageV1, DerivedChangeAccess,
    DerivedChangeAttentionFilterV1, DerivedChangeAvailabilityFilterV1, DerivedChangeOutcomeV1,
    DerivedChangePageContinuationV1, DerivedChangePageRequestV1, DerivedChangePageSelectionV1,
    DerivedChangePageV1, DerivedProjectionFailureCodeV1, EventStore, EventWriteOutcome, TrustSet,
    allowed_signers_path_for_repo, opaque_path_identity, read_events_for_display,
    verify_event_signature,
};
#[cfg(feature = "longitudinal-counting")]
use crate::storage::{Durability, LocalStorage};

pub const QUALIFICATION_DERIVED_CHANGE_READ_REQUEST_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-change-read-request.v1";
pub const QUALIFICATION_DERIVED_CHANGE_READ_REQUEST_SCHEMA_V2: &str =
    "pointbreak.qualification-derived-change-read-request.v2";
pub const QUALIFICATION_DERIVED_CHANGE_READ_MODE_V1: &str = "--derived-change-read-evidence";
pub const DERIVED_CHANGE_READ_DIAGNOSTIC_MODE_V1: &str = "--derived-change-read-diagnostic";
pub const DERIVED_CHANGE_READ_DIAGNOSTIC_REQUEST_SCHEMA_V1: &str =
    "pointbreak.derived-change-read-diagnostic-request.v1";

/// A disposable, non-evidence wrapper around one public Change-read fixture.
/// Its output is intentionally schema-less so it cannot be mistaken for a
/// receipt or any other terminal evidence input.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DerivedChangeReadDiagnosticRunRequestV1 {
    pub schema: String,
    pub read_request: QualificationDerivedChangeReadRunRequestV1,
    pub workspace_root: PathBuf,
}

impl DerivedChangeReadDiagnosticRunRequestV1 {
    #[cfg(feature = "longitudinal-counting")]
    fn validate(&self) -> Result<(), String> {
        self.read_request.validate()?;
        if self.schema != DERIVED_CHANGE_READ_DIAGNOSTIC_REQUEST_SCHEMA_V1
            || !self.workspace_root.is_absolute()
            || self.read_request.purpose
                != QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification
            || self.workspace_root == self.read_request.repository
            || self
                .workspace_root
                .starts_with(&self.read_request.repository)
            || self
                .read_request
                .repository
                .starts_with(&self.workspace_root)
            || self
                .workspace_root
                .starts_with(&self.read_request.source_checkout)
            || self
                .read_request
                .source_checkout
                .starts_with(&self.workspace_root)
            || self
                .workspace_root
                .starts_with(&self.read_request.product_source_checkout)
            || self
                .read_request
                .product_source_checkout
                .starts_with(&self.workspace_root)
        {
            return Err("invalid derived Change diagnostic request".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedChangeReadDiagnosticStatusV1 {
    Passed,
    Failed,
    Skipped,
}

/// Test-only failure points for the disposable Timeline lifecycle diagnostic.
/// They are per-call values rather than ambient process state, so parallel
/// tests cannot change qualification behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DerivedTimelineInvalidSignatureDiagnosticFaultV1 {
    None,
    AfterTrustStageIdentityRead,
    TrustStageReportsUnchangedIdentity,
    AfterCarrierWrite,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedChangeReadDiagnosticPreflightKindV1 {
    Source,
    Fixture,
    LibraryControl,
    CliControl,
    TemplatePostflight,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedChangeReadDiagnosticPreflightV1 {
    pub kind: DerivedChangeReadDiagnosticPreflightKindV1,
    pub status: DerivedChangeReadDiagnosticStatusV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_detail: Option<String>,
}

impl DerivedChangeReadDiagnosticPreflightV1 {
    pub fn passed(kind: DerivedChangeReadDiagnosticPreflightKindV1) -> Self {
        Self {
            kind,
            status: DerivedChangeReadDiagnosticStatusV1::Passed,
            failure_detail: None,
        }
    }

    pub fn failed(
        kind: DerivedChangeReadDiagnosticPreflightKindV1,
        failure_detail: String,
    ) -> Self {
        Self {
            kind,
            status: DerivedChangeReadDiagnosticStatusV1::Failed,
            failure_detail: Some(failure_detail),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedChangeReadDiagnosticRowV1 {
    pub case: QualificationDerivedChangeReadCaseV1,
    pub status: DerivedChangeReadDiagnosticStatusV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_witness: Option<DerivedChangeReadDiagnosticFailureWitnessV1>,
}

/// Bounded, diagnostic-only oracle witness. It intentionally carries hashes and
/// typed envelope fields, never response documents, messages, or filesystem paths.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(
    tag = "oracle",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum DerivedChangeReadDiagnosticFailureWitnessV1 {
    StrictParity {
        derived: DerivedChangeReadDiagnosticSemanticWitnessV1,
        strict: DerivedChangeReadDiagnosticSemanticWitnessV1,
        expected_http_status: u16,
        expected_code: Option<String>,
    },
    TypedFailure {
        observed: DerivedChangeReadDiagnosticTypedWitnessV1,
        expected: DerivedChangeReadDiagnosticTypedWitnessV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DerivedChangeReadDiagnosticSemanticWitnessV1 {
    pub http_status: u16,
    pub code: Option<String>,
    pub normalized_document_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DerivedChangeReadDiagnosticTypedWitnessV1 {
    pub schema: Option<String>,
    pub version: Option<u64>,
    pub code: Option<String>,
    pub retryable: Option<bool>,
    pub key_set: Vec<String>,
    pub canonical_sha256: String,
}

struct DiagnosticReadFailure {
    detail: String,
    witness: Option<Box<DerivedChangeReadDiagnosticFailureWitnessV1>>,
}

impl From<String> for DiagnosticReadFailure {
    fn from(detail: String) -> Self {
        Self {
            detail,
            witness: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedChangeReadDiagnosticControlV1 {
    pub case: QualificationDerivedChangeControlCaseV1,
    pub status: DerivedChangeReadDiagnosticStatusV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_detail: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedChangeReadDiagnosticStorageCaseV1 {
    Initial,
    PostAppend,
}

impl DerivedChangeReadDiagnosticStorageCaseV1 {
    pub const ALL: [Self; 2] = [Self::Initial, Self::PostAppend];
    pub const INITIAL_ONLY: [Self; 1] = [Self::Initial];

    pub fn required_for(fixture: QualificationDerivedChangeFixtureV1) -> &'static [Self] {
        if fixture == QualificationDerivedChangeFixtureV1::TopologyV1 {
            &Self::ALL
        } else {
            &Self::INITIAL_ONLY
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedChangeReadDiagnosticStorageV1 {
    pub case: DerivedChangeReadDiagnosticStorageCaseV1,
    pub status: DerivedChangeReadDiagnosticStatusV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_detail: Option<String>,
}

/// Schema-less transport for the derived-Change diagnostic wrapper. Never use
/// this type to construct a terminal evidence document.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedChangeReadDiagnosticCollectionV1 {
    pub mode: String,
    pub source_unchanged: bool,
    pub preflight: Vec<DerivedChangeReadDiagnosticPreflightV1>,
    pub rows: Vec<DerivedChangeReadDiagnosticRowV1>,
    pub controls: Vec<DerivedChangeReadDiagnosticControlV1>,
    pub storage: Vec<DerivedChangeReadDiagnosticStorageV1>,
}

pub fn collect_derived_change_read_diagnostic_rows_v1<F>(
    run: F,
) -> Vec<DerivedChangeReadDiagnosticRowV1>
where
    F: FnMut(QualificationDerivedChangeReadCaseV1) -> Result<(), String>,
{
    collect_derived_change_read_diagnostic_rows_for_cases_v1(
        &QualificationDerivedChangeReadCaseV1::ALL,
        run,
    )
}

fn collect_derived_change_read_diagnostic_rows_for_cases_v1<F, E>(
    cases: &[QualificationDerivedChangeReadCaseV1],
    mut run: F,
) -> Vec<DerivedChangeReadDiagnosticRowV1>
where
    F: FnMut(QualificationDerivedChangeReadCaseV1) -> Result<(), E>,
    E: Into<DiagnosticReadFailure>,
{
    cases
        .iter()
        .copied()
        .map(|case| match run(case) {
            Ok(()) => DerivedChangeReadDiagnosticRowV1 {
                case,
                status: DerivedChangeReadDiagnosticStatusV1::Passed,
                failure_detail: None,
                failure_witness: None,
            },
            Err(failure) => {
                let failure = failure.into();
                DerivedChangeReadDiagnosticRowV1 {
                    case,
                    status: DerivedChangeReadDiagnosticStatusV1::Failed,
                    failure_detail: Some(failure.detail),
                    failure_witness: failure.witness.map(|witness| *witness),
                }
            }
        })
        .collect()
}

pub fn collect_derived_change_read_diagnostic_rows_after_preflight_v1<F>(
    preflight: &DerivedChangeReadDiagnosticPreflightV1,
    run: F,
) -> Vec<DerivedChangeReadDiagnosticRowV1>
where
    F: FnMut(QualificationDerivedChangeReadCaseV1) -> Result<(), String>,
{
    collect_derived_change_read_diagnostic_rows_after_preflight_for_cases_v1(
        &QualificationDerivedChangeReadCaseV1::ALL,
        preflight,
        run,
    )
}

fn collect_derived_change_read_diagnostic_rows_after_preflight_for_cases_v1<F, E>(
    cases: &[QualificationDerivedChangeReadCaseV1],
    preflight: &DerivedChangeReadDiagnosticPreflightV1,
    run: F,
) -> Vec<DerivedChangeReadDiagnosticRowV1>
where
    F: FnMut(QualificationDerivedChangeReadCaseV1) -> Result<(), E>,
    E: Into<DiagnosticReadFailure>,
{
    if preflight.status == DerivedChangeReadDiagnosticStatusV1::Passed {
        collect_derived_change_read_diagnostic_rows_for_cases_v1(cases, run)
    } else {
        cases
            .iter()
            .copied()
            .map(|case| DerivedChangeReadDiagnosticRowV1 {
                case,
                status: DerivedChangeReadDiagnosticStatusV1::Skipped,
                failure_detail: preflight.failure_detail.clone(),
                failure_witness: None,
            })
            .collect()
    }
}

pub fn collect_derived_change_read_diagnostic_controls_v1<F>(
    preflight: [DerivedChangeReadDiagnosticPreflightV1; 2],
    mut run: F,
) -> Vec<DerivedChangeReadDiagnosticControlV1>
where
    F: FnMut(QualificationDerivedChangeControlCaseV1) -> Result<(), String>,
{
    let mut outcomes: std::collections::BTreeMap<
        (QualificationDerivedChangeControlBinaryKindV1, &'static str),
        Result<(), String>,
    > = std::collections::BTreeMap::new();
    QualificationDerivedChangeControlCaseV1::ALL
        .into_iter()
        .map(|case| {
            let control = qualification_derived_change_control_test_v1(case);
            let kind = control.0;
            let prerequisite = preflight.iter().find(|candidate| {
                candidate.kind
                    == match kind {
                        QualificationDerivedChangeControlBinaryKindV1::Library => {
                            DerivedChangeReadDiagnosticPreflightKindV1::LibraryControl
                        }
                        QualificationDerivedChangeControlBinaryKindV1::Cli => {
                            DerivedChangeReadDiagnosticPreflightKindV1::CliControl
                        }
                    }
            });
            match prerequisite {
                Some(candidate)
                    if candidate.status == DerivedChangeReadDiagnosticStatusV1::Passed =>
                {
                    let outcome = if let Some(outcome) = outcomes.get(&control) {
                        outcome.clone()
                    } else {
                        let outcome = run(case);
                        outcomes.insert(control, outcome.clone());
                        outcome
                    };
                    match outcome {
                        Ok(()) => DerivedChangeReadDiagnosticControlV1 {
                            case,
                            status: DerivedChangeReadDiagnosticStatusV1::Passed,
                            failure_detail: None,
                        },
                        Err(failure_detail) => DerivedChangeReadDiagnosticControlV1 {
                            case,
                            status: DerivedChangeReadDiagnosticStatusV1::Failed,
                            failure_detail: Some(failure_detail),
                        },
                    }
                }
                Some(candidate) => DerivedChangeReadDiagnosticControlV1 {
                    case,
                    status: DerivedChangeReadDiagnosticStatusV1::Skipped,
                    failure_detail: candidate.failure_detail.clone(),
                },
                None => DerivedChangeReadDiagnosticControlV1 {
                    case,
                    status: DerivedChangeReadDiagnosticStatusV1::Skipped,
                    failure_detail: Some("control diagnostic preflight is missing".to_owned()),
                },
            }
        })
        .collect()
}

pub fn collect_derived_change_read_diagnostic_storage_v1<F>(
    run: F,
) -> Vec<DerivedChangeReadDiagnosticStorageV1>
where
    F: FnMut(DerivedChangeReadDiagnosticStorageCaseV1) -> Result<(), String>,
{
    collect_derived_change_read_diagnostic_storage_for_cases_v1(
        &DerivedChangeReadDiagnosticStorageCaseV1::ALL,
        run,
    )
}

fn collect_derived_change_read_diagnostic_storage_for_cases_v1<F>(
    cases: &[DerivedChangeReadDiagnosticStorageCaseV1],
    mut run: F,
) -> Vec<DerivedChangeReadDiagnosticStorageV1>
where
    F: FnMut(DerivedChangeReadDiagnosticStorageCaseV1) -> Result<(), String>,
{
    cases
        .iter()
        .copied()
        .map(|case| match run(case) {
            Ok(()) => DerivedChangeReadDiagnosticStorageV1 {
                case,
                status: DerivedChangeReadDiagnosticStatusV1::Passed,
                failure_detail: None,
            },
            Err(failure_detail) => DerivedChangeReadDiagnosticStorageV1 {
                case,
                status: DerivedChangeReadDiagnosticStatusV1::Failed,
                failure_detail: Some(failure_detail),
            },
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedChangeReadRunRequestV1 {
    pub schema: String,
    pub purpose: QualificationDerivedChangeEvidencePurposeV1,
    pub source_checkout: PathBuf,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub product_source_checkout: PathBuf,
    pub product: QualificationDerivedAccessProductIdentityV1,
    pub fixture: QualificationDerivedChangeFixtureV1,
    pub fixture_witness: PathBuf,
    pub fixture_witness_sha256: String,
    pub repository: PathBuf,
    pub pointbreak_home: PathBuf,
    pub product_binary: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_test_binary: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_test_binary_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_test_build_command_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_cli_test_binary: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_cli_test_binary_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_cli_test_build_command_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_forbidden_probes: Option<QualificationDerivedStorageForbiddenProbeInputV1>,
    pub summary_query: String,
}

/// Byte-clone-seeded authority used only by the Timeline fault falsifier. It
/// deliberately carries a complete execution identity: the evidence validator
/// must not infer a second root from the primary request. Its fault-seed
/// receipt proves the root was cloned from one validated canonical
/// materialization before any governed derived state beyond idle zero-byte
/// writer locks and before any staged lifecycle trust existed.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedTimelineFaultRootV1 {
    pub repository: PathBuf,
    pub pointbreak_home: PathBuf,
    pub fixture_witness: PathBuf,
    pub fixture_witness_sha256: String,
    pub barrier_root: PathBuf,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub fault_seed_receipt: QualificationDerivedTimelineFaultSeedReceiptV1,
}

/// V2 adds a disjoint fault authority without widening the frozen V1 request
/// domain used by evaluator V3 inputs.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedChangeReadRunRequestV2 {
    pub schema: String,
    pub base: QualificationDerivedChangeReadRunRequestV1,
    pub timeline_fault_root: QualificationDerivedTimelineFaultRootV1,
}

impl QualificationDerivedChangeReadRunRequestV2 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_CHANGE_READ_REQUEST_SCHEMA_V2 {
            return Err("invalid derived Change read v2 request".to_owned());
        }
        self.base.validate()?;
        validate_timeline_fault_authority_layout(&self.base, &self.timeline_fault_root)
    }
}

fn validate_timeline_fault_authority_layout(
    reference: &QualificationDerivedChangeReadRunRequestV1,
    fault: &QualificationDerivedTimelineFaultRootV1,
) -> Result<(), String> {
    fault.execution.validate()?;
    validate_digest(
        &fault.fixture_witness_sha256,
        "Timeline fault fixture witness",
    )?;
    let reference_repository =
        canonical_authority_path(&reference.repository, "reference repository")?;
    let reference_home =
        canonical_authority_path(&reference.pointbreak_home, "reference Pointbreak home")?;
    let reference_witness =
        canonical_authority_path(&reference.fixture_witness, "reference fixture witness")?;
    let fault_repository = canonical_authority_path(&fault.repository, "fault repository")?;
    let fault_home = canonical_authority_path(&fault.pointbreak_home, "fault Pointbreak home")?;
    let fault_witness = canonical_authority_path(&fault.fixture_witness, "fault fixture witness")?;
    let barrier_root = canonical_authority_path(&fault.barrier_root, "Timeline barrier root")?;
    let reference_authorities = [
        reference_repository.as_path(),
        reference_home.as_path(),
        reference_witness.as_path(),
    ];
    let fault_authorities = [
        fault_repository.as_path(),
        fault_home.as_path(),
        fault_witness.as_path(),
    ];
    let cross_root_overlap = reference_authorities.iter().any(|reference_path| {
        fault_authorities
            .iter()
            .any(|fault_path| paths_overlap(reference_path, fault_path))
    });
    let barrier_overlap = reference_authorities
        .iter()
        .chain(fault_authorities.iter())
        .any(|authority_path| paths_overlap(&barrier_root, authority_path));
    if reference.purpose != QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification
        || reference.fixture != QualificationDerivedChangeFixtureV1::TopologyV1
        || !fault.repository.is_absolute()
        || !fault.pointbreak_home.is_absolute()
        || !fault.fixture_witness.is_absolute()
        || !fault.barrier_root.is_absolute()
        || !reference_home.starts_with(&reference_repository)
        || !fault_home.starts_with(&fault_repository)
        || cross_root_overlap
        || barrier_overlap
    {
        return Err("Timeline fault authority is not disjoint".to_owned());
    }
    let mut expected_fault_execution = reference.execution.clone();
    expected_fault_execution.root_provenance_sha256 =
        fault.execution.root_provenance_sha256.clone();
    if fault.execution != expected_fault_execution
        || fault.execution.root_provenance_sha256 == reference.execution.root_provenance_sha256
    {
        return Err("Timeline fault execution identity drifted from its reference".to_owned());
    }
    let barrier_entries = std::fs::read_dir(&fault.barrier_root)
        .map_err(|error| format!("read Timeline barrier root: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read Timeline barrier entry: {error}"))?;
    if !barrier_entries.is_empty() {
        return Err("Timeline barrier root must start empty".to_owned());
    }
    let seed = &fault.fault_seed_receipt;
    seed.validate()?;
    if seed.reference_root_path_sha256
        != timeline_fault_seed_path_digest(&reference_repository, "reference repository")?
        || seed.fault_root_path_sha256
            != timeline_fault_seed_path_digest(&fault_repository, "fault repository")?
        || seed.reference_witness_path_sha256
            != timeline_fault_seed_path_digest(&reference_witness, "reference fixture witness")?
        || seed.fault_witness_path_sha256
            != timeline_fault_seed_path_digest(&fault_witness, "fault fixture witness")?
    {
        return Err("Timeline fault-seed receipt is bound to different roots".to_owned());
    }
    if reference.fixture_witness_sha256 != seed.witness_sha256
        || fault.fixture_witness_sha256 != seed.witness_sha256
    {
        return Err("Timeline fault-seed witness identity drifted".to_owned());
    }
    // The carrier under test lives in the resolved store, so isolation is a
    // property of the store directories, not only the repository paths: a
    // family binding cloned with the repository would otherwise resolve both
    // roots to one shared store and make every cross-root inventory check
    // vacuous.
    let reference_store = canonical_authority_path(
        &store_dir_for_repo(&reference_repository).map_err(|error| error.to_string())?,
        "reference store",
    )?;
    let fault_store = canonical_authority_path(
        &store_dir_for_repo(&fault_repository).map_err(|error| error.to_string())?,
        "fault store",
    )?;
    if !reference_store.starts_with(&reference_repository)
        || !fault_store.starts_with(&fault_repository)
        || paths_overlap(&reference_store, &fault_store)
    {
        return Err(
            "Timeline fault store authority is not repository-contained and disjoint".to_owned(),
        );
    }
    Ok(())
}

fn timeline_fault_seed_path_digest(path: &Path, label: &str) -> Result<String, String> {
    path.to_str()
        .map(|text| sha256_bytes_hex(text.as_bytes()))
        .ok_or_else(|| format!("{label} path is not UTF-8"))
}

fn canonical_authority_path(path: &Path, label: &str) -> Result<PathBuf, String> {
    std::fs::canonicalize(path).map_err(|error| format!("canonicalize {label}: {error}"))
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

impl QualificationDerivedChangeReadRunRequestV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_CHANGE_READ_REQUEST_SCHEMA_V1
            || !self.source_checkout.is_absolute()
            || !self.product_source_checkout.is_absolute()
            || !self.repository.is_absolute()
            || !self.pointbreak_home.is_absolute()
            || !self.product_binary.is_absolute()
            || !self.fixture_witness.is_absolute()
            || self.source_checkout == self.repository
            || self.repository.starts_with(&self.source_checkout)
            || self.source_checkout.starts_with(&self.repository)
            || self.product_source_checkout == self.repository
            || self.repository.starts_with(&self.product_source_checkout)
            || self.product_source_checkout.starts_with(&self.repository)
            || !self.pointbreak_home.starts_with(&self.repository)
            || self.summary_query.trim().is_empty()
            || self.summary_query.trim() != self.summary_query
            || self.summary_query.len() > 256
        {
            return Err("invalid derived Change read request".to_owned());
        }
        self.execution.validate()?;
        self.product.validate()?;
        validate_digest(&self.fixture_witness_sha256, "Change fixture witness")?;
        if self.product.platform != self.execution.platform
            || self.product.operating_system != std::env::consts::OS
            || self.product.architecture != std::env::consts::ARCH
            || self.product_binary == self.control_test_binary.clone().unwrap_or_default()
            || self.product_binary == self.control_cli_test_binary.clone().unwrap_or_default()
            || self.control_test_binary.is_some()
                && self.control_test_binary == self.control_cli_test_binary
            || self.control_test_binary_sha256.as_deref()
                == Some(self.product.binary_sha256.as_str())
            || self.control_cli_test_binary_sha256.as_deref()
                == Some(self.product.binary_sha256.as_str())
            || self.control_test_binary_sha256.is_some()
                && self.control_test_binary_sha256 == self.control_cli_test_binary_sha256
        {
            return Err("derived Change product request mixes binary authority".to_owned());
        }
        let exact_source = self.product.is_exact_source_for(&self.execution);
        match self.purpose {
            QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification
                if !exact_source =>
            {
                return Err(
                    "exact Change qualification product differs from its harness".to_owned(),
                );
            }
            QualificationDerivedChangeEvidencePurposeV1::PreCutFalsifier if exact_source => {
                return Err("pre-cut Change falsifier uses the successor source".to_owned());
            }
            _ => {}
        }
        let controls_required = self.purpose
            == QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification
            && self.fixture == QualificationDerivedChangeFixtureV1::TopologyV1;
        match (
            controls_required,
            self.control_test_binary.as_ref(),
            self.control_test_binary_sha256.as_deref(),
            self.control_test_build_command_sha256.as_deref(),
            self.control_cli_test_binary.as_ref(),
            self.control_cli_test_binary_sha256.as_deref(),
            self.control_cli_test_build_command_sha256.as_deref(),
        ) {
            (
                true,
                Some(library),
                Some(library_sha256),
                Some(library_build_sha256),
                Some(cli),
                Some(cli_sha256),
                Some(cli_build_sha256),
            ) if library.is_absolute() && cli.is_absolute() => {
                validate_digest(library_sha256, "Change library control test binary")?;
                validate_digest(library_build_sha256, "Change library control build command")?;
                validate_digest(cli_sha256, "Change CLI control test binary")?;
                validate_digest(cli_build_sha256, "Change CLI control build command")?;
                if library_build_sha256
                    != qualification_derived_change_control_build_command_sha256_v1(
                        QualificationDerivedChangeControlBinaryKindV1::Library,
                    )
                    || cli_build_sha256
                        != qualification_derived_change_control_build_command_sha256_v1(
                            QualificationDerivedChangeControlBinaryKindV1::Cli,
                        )
                {
                    return Err("derived Change control build command drifted".to_owned());
                }
            }
            (false, None, None, None, None, None, None) => {}
            _ => return Err("derived Change control binary request is inconsistent".to_owned()),
        }
        match (self.purpose, self.storage_forbidden_probes.as_ref()) {
            (
                QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification,
                Some(probes),
            ) => probes.validate()?,
            (QualificationDerivedChangeEvidencePurposeV1::PreCutFalsifier, None) => {}
            _ => return Err("derived Change storage probes are inconsistent".to_owned()),
        }
        if self.execution.root_provenance_sha256.trim().is_empty() {
            return Err("derived Change read request omitted fixture authority".to_owned());
        }
        Ok(())
    }
}

fn validate_digest(value: &str, label: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{label} must be 64 lowercase hexadecimal digits"));
    }
    Ok(())
}

pub fn run_qualification_derived_change_read_v1(
    request_path: &Path,
) -> Result<QualificationDerivedChangeReadReceiptV2, String> {
    #[cfg(feature = "longitudinal-counting")]
    {
        instrumented::run_qualification_derived_change_read_v1(request_path)
    }
    #[cfg(not(feature = "longitudinal-counting"))]
    {
        let _ = request_path;
        Err("derived Change read evidence requires --features longitudinal-counting".to_owned())
    }
}

/// Run isolated derived-Change diagnostics without constructing qualification
/// evidence. The schema-less result is transport for the diagnostic wrapper.
pub fn run_derived_change_read_diagnostic_v1(
    request_path: &Path,
) -> Result<DerivedChangeReadDiagnosticCollectionV1, String> {
    #[cfg(feature = "longitudinal-counting")]
    {
        instrumented::run_derived_change_read_diagnostic_v1(request_path)
    }
    #[cfg(not(feature = "longitudinal-counting"))]
    {
        let _ = request_path;
        Err("derived Change diagnostic requires --features longitudinal-counting".to_owned())
    }
}

/// Exercise the real Timeline TrustSuite lifecycle in a disposable repository.
/// This returns one in-memory row and never writes a receipt or package.
#[allow(clippy::too_many_arguments)]
pub fn run_timeline_invalid_signature_lifecycle_diagnostic_v1(
    reference_repository: &Path,
    reference_pointbreak_home: &Path,
    reference_fixture_witness: &Path,
    fault_repository: &Path,
    fault_pointbreak_home: &Path,
    fault_fixture_witness: &Path,
    barrier_root: &Path,
    product_binary: &Path,
    fault_seed_receipt: &QualificationDerivedTimelineFaultSeedReceiptV1,
    fault: DerivedTimelineInvalidSignatureDiagnosticFaultV1,
) -> Result<QualificationDerivedTimelineReadEvidenceV1, String> {
    #[cfg(feature = "longitudinal-counting")]
    {
        instrumented::run_timeline_invalid_signature_lifecycle_diagnostic_v1(
            reference_repository,
            reference_pointbreak_home,
            reference_fixture_witness,
            fault_repository,
            fault_pointbreak_home,
            fault_fixture_witness,
            barrier_root,
            product_binary,
            fault_seed_receipt,
            fault,
        )
    }
    #[cfg(not(feature = "longitudinal-counting"))]
    {
        let _ = (
            reference_repository,
            reference_pointbreak_home,
            reference_fixture_witness,
            fault_repository,
            fault_pointbreak_home,
            fault_fixture_witness,
            barrier_root,
            product_binary,
            fault_seed_receipt,
            fault,
        );
        Err("Timeline lifecycle diagnostic requires --features longitudinal-counting".to_owned())
    }
}

/// One canonical reference materialization and the absent destination paths
/// that will hold its byte-cloned fault authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualificationDerivedTimelineFaultSeedPlanV1 {
    pub reference_repository: PathBuf,
    pub reference_fixture_witness: PathBuf,
    pub fault_repository: PathBuf,
    pub fault_fixture_witness: PathBuf,
}

/// Byte-clone one validated canonical public-matrix materialization into an
/// absent, canonical-path-disjoint fault root before any governed derived
/// state beyond idle zero-byte writer locks and before any staged lifecycle
/// trust exists, then return the bound fault-seed receipt. The fault root
/// shares the reference materialization by construction; this proves
/// isolation, not independent fixture reproducibility.
pub fn seed_qualification_derived_timeline_fault_root_v1(
    plan: &QualificationDerivedTimelineFaultSeedPlanV1,
) -> Result<QualificationDerivedTimelineFaultSeedReceiptV1, String> {
    seed_qualification_derived_timeline_fault_root_with_injection_v1(
        plan,
        QualificationDerivedTimelineFaultSeedInjectionV1::None,
    )
}

/// Injected fault points for the clone-protocol falsifiers. Every variant
/// other than `None` forces a failure so tests can prove the cleanup guard
/// leaves each destination absent; production seeding always passes `None`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualificationDerivedTimelineFaultSeedInjectionV1 {
    None,
    AfterRepositoryFinalize,
}

pub fn seed_qualification_derived_timeline_fault_root_with_injection_v1(
    plan: &QualificationDerivedTimelineFaultSeedPlanV1,
    injection: QualificationDerivedTimelineFaultSeedInjectionV1,
) -> Result<QualificationDerivedTimelineFaultSeedReceiptV1, String> {
    #[cfg(feature = "longitudinal-counting")]
    {
        instrumented::seed_qualification_derived_timeline_fault_root_v1(plan, injection)
    }
    #[cfg(not(feature = "longitudinal-counting"))]
    {
        let _ = (plan, injection);
        Err("Timeline fault-seed clone requires --features longitudinal-counting".to_owned())
    }
}

#[cfg(feature = "longitudinal-counting")]
mod instrumented {
    use super::*;

    fn read_qualification_change_read_request(
        path: &Path,
    ) -> Result<
        (
            QualificationDerivedChangeReadRunRequestV1,
            Option<QualificationDerivedTimelineFaultRootV1>,
        ),
        String,
    > {
        let document: Value = read_json(path)?;
        match document.get("schema").and_then(Value::as_str) {
            Some(QUALIFICATION_DERIVED_CHANGE_READ_REQUEST_SCHEMA_V1) => {
                let request =
                    serde_json::from_value(document).map_err(|error| error.to_string())?;
                Ok((request, None))
            }
            Some(QUALIFICATION_DERIVED_CHANGE_READ_REQUEST_SCHEMA_V2) => {
                let request: QualificationDerivedChangeReadRunRequestV2 =
                    serde_json::from_value(document).map_err(|error| error.to_string())?;
                request.validate()?;
                Ok((request.base, Some(request.timeline_fault_root)))
            }
            _ => Err("unknown derived Change read request schema".to_owned()),
        }
    }

    fn request_for_timeline_fault_root(
        reference: &QualificationDerivedChangeReadRunRequestV1,
        fault: &QualificationDerivedTimelineFaultRootV1,
    ) -> Result<QualificationDerivedChangeReadRunRequestV1, String> {
        let mut request = reference.clone();
        request.execution = fault.execution.clone();
        request.repository = fault.repository.clone();
        request.pointbreak_home = fault.pointbreak_home.clone();
        request.fixture_witness = fault.fixture_witness.clone();
        request.fixture_witness_sha256 = fault.fixture_witness_sha256.clone();
        if let Some(probes) = request.storage_forbidden_probes.as_mut() {
            probes.private_path = request
                .repository
                .to_str()
                .ok_or_else(|| "Timeline fault repository path is not UTF-8".to_owned())?
                .to_owned();
        }
        Ok(request)
    }

    struct TimelineFaultSeedTreeManifestV1 {
        manifest_sha256: String,
        file_count: u64,
        byte_count: u64,
    }

    fn timeline_fault_seed_tree_manifest_v1(
        root: &Path,
    ) -> Result<TimelineFaultSeedTreeManifestV1, String> {
        let mut files = Vec::new();
        let mut directories = Vec::new();
        collect_timeline_fault_seed_files_v1(root, root, &mut files, &mut directories)?;
        files.sort_by(|left, right| left.0.cmp(&right.0));
        let byte_count = files.iter().try_fold(0_u64, |total, file| {
            total
                .checked_add(file.1)
                .ok_or_else(|| "Timeline fault-seed byte count overflowed".to_owned())
        })?;
        let listing = files
            .iter()
            .map(|(relative_path, bytes, sha256)| {
                json!({
                    "relativePath": relative_path,
                    "bytes": bytes,
                    "sha256": sha256,
                })
            })
            .collect::<Vec<_>>();
        directories.sort();
        let manifest_bytes = canonical_json_bytes(&json!({
            "directories": directories,
            "files": Value::Array(listing),
        }))
        .map_err(|error| format!("Timeline fault-seed manifest is not canonical: {error}"))?;
        Ok(TimelineFaultSeedTreeManifestV1 {
            manifest_sha256: sha256_bytes_hex(&manifest_bytes),
            file_count: files.len() as u64,
            byte_count,
        })
    }

    fn timeline_fault_seed_relative_path(root: &Path, path: &Path) -> Result<String, String> {
        path.strip_prefix(root)
            .map_err(|error| error.to_string())?
            .components()
            .map(|component| {
                component
                    .as_os_str()
                    .to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "Timeline fault-seed path is not UTF-8".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|components| components.join("/"))
    }

    fn collect_timeline_fault_seed_files_v1(
        root: &Path,
        directory: &Path,
        files: &mut Vec<(String, u64, String)>,
        directories: &mut Vec<String>,
    ) -> Result<(), String> {
        let entries = std::fs::read_dir(directory)
            .map_err(|error| format!("read fault-seed directory {}: {error}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                format!("read fault-seed entry in {}: {error}", directory.display())
            })?;
        for entry in entries {
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|error| format!("inspect fault-seed path {}: {error}", path.display()))?;
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "Timeline fault-seed source contains a symlink: {}",
                    path.display()
                ));
            }
            if metadata.is_dir() {
                directories.push(timeline_fault_seed_relative_path(root, &path)?);
                collect_timeline_fault_seed_files_v1(root, &path, files, directories)?;
            } else if metadata.is_file() {
                let relative_path = timeline_fault_seed_relative_path(root, &path)?;
                let bytes = std::fs::read(&path)
                    .map_err(|error| format!("read fault-seed file {}: {error}", path.display()))?;
                files.push((relative_path, bytes.len() as u64, sha256_bytes_hex(&bytes)));
            } else {
                return Err(format!(
                    "Timeline fault-seed source contains a special file: {}",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    fn copy_timeline_fault_seed_tree_v1(source: &Path, destination: &Path) -> Result<(), String> {
        std::fs::create_dir(destination).map_err(|error| {
            format!("create fault-seed copy {}: {error}", destination.display())
        })?;
        let entries = std::fs::read_dir(source)
            .map_err(|error| format!("read fault-seed directory {}: {error}", source.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read fault-seed entry in {}: {error}", source.display()))?;
        for entry in entries {
            let path = entry.path();
            let target = destination.join(entry.file_name());
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|error| format!("inspect fault-seed path {}: {error}", path.display()))?;
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "Timeline fault-seed source contains a symlink: {}",
                    path.display()
                ));
            }
            if metadata.is_dir() {
                copy_timeline_fault_seed_tree_v1(&path, &target)?;
            } else if metadata.is_file() {
                std::fs::copy(&path, &target)
                    .map_err(|error| format!("copy fault-seed file {}: {error}", path.display()))?;
            } else {
                return Err(format!(
                    "Timeline fault-seed source contains a special file: {}",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    fn optional_file_bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("read {}: {error}", path.display())),
        }
    }

    /// Reject any governed derived state beyond the idle zero-byte writer
    /// locks that ordinary CLI writes leave behind: no derived namespace,
    /// rebuild lock, generation lease, quarantine, retired root, or non-empty
    /// lock may exist when the fault authority is seeded.
    fn validate_timeline_fault_seed_derived_surface_v1(repo: &Path) -> Result<(), String> {
        let store = store_dir_for_repo(repo).map_err(|error| error.to_string())?;
        if !store.exists() {
            return Ok(());
        }
        let entries = std::fs::read_dir(&store)
            .map_err(|error| format!("read fault-seed store {}: {error}", store.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read fault-seed store entry: {error}"))?;
        for entry in entries {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return Err("Timeline fault-seed store entry name is not UTF-8".to_owned());
            };
            let metadata = std::fs::symlink_metadata(entry.path())
                .map_err(|error| format!("inspect fault-seed store entry {name}: {error}"))?;
            if !metadata.is_file() && !metadata.is_dir() {
                return Err(format!(
                    "Timeline fault-seed store entry {name} is neither a regular file nor a \
                     directory"
                ));
            }
            if !DerivedStorageLayout::is_governed_store_entry(
                name,
                metadata.is_dir(),
                metadata.is_file(),
            ) {
                continue;
            }
            if !(DerivedStorageLayout::is_derived_writer_lock_entry(name, metadata.is_file())
                && metadata.len() == 0)
            {
                return Err(
                    "Timeline fault-seed source already carries governed derived state".to_owned(),
                );
            }
        }
        Ok(())
    }

    /// Re-verify every claim the fault-seed receipt carries against the live
    /// roots: both store inventories in both scopes, both full tree manifests
    /// and their counts, and the unstaged initial trust state. Both the
    /// diagnostic lifecycle and the shipped V2 producer apply this binding so
    /// no receipt field is attested at seed time only.
    fn validate_timeline_fault_seed_bindings_v1(
        reference_repository: &Path,
        fault_repository: &Path,
        seed: &QualificationDerivedTimelineFaultSeedReceiptV1,
    ) -> Result<(), String> {
        for repository in [reference_repository, fault_repository] {
            let authoritative = longitudinal_authoritative_store_data_inventory_v1(repository)
                .map_err(|error| error.to_string())?;
            let inclusive = longitudinal_store_data_inventory_v1(repository)
                .map_err(|error| error.to_string())?;
            if authoritative.inventory_sha256 != seed.authoritative_inventory_sha256
                || inclusive.inventory_sha256 != seed.inclusive_inventory_sha256
            {
                return Err(
                    "Timeline lifecycle roots drifted from their validated fault-seed clone"
                        .to_owned(),
                );
            }
            let manifest = timeline_fault_seed_tree_manifest_v1(repository)?;
            if manifest.manifest_sha256 != seed.tree_manifest_sha256
                || manifest.file_count != seed.cloned_file_count
                || manifest.byte_count != seed.cloned_byte_count
            {
                return Err(
                    "Timeline lifecycle root trees drifted from their fault-seed manifest"
                        .to_owned(),
                );
            }
            let trust_path =
                allowed_signers_path_for_repo(repository).map_err(|error| error.to_string())?;
            let trust_sha256 = optional_file_bytes(&trust_path)?
                .as_deref()
                .map(sha256_bytes_hex);
            if trust_sha256 != seed.initial_trust_sha256 {
                return Err(
                    "Timeline lifecycle trust drifted from its fault-seed initial state".to_owned(),
                );
            }
        }
        Ok(())
    }

    pub(super) fn seed_qualification_derived_timeline_fault_root_v1(
        plan: &QualificationDerivedTimelineFaultSeedPlanV1,
        injection: QualificationDerivedTimelineFaultSeedInjectionV1,
    ) -> Result<QualificationDerivedTimelineFaultSeedReceiptV1, String> {
        if !plan.reference_repository.is_absolute()
            || !plan.reference_fixture_witness.is_absolute()
            || !plan.fault_repository.is_absolute()
            || !plan.fault_fixture_witness.is_absolute()
        {
            return Err("Timeline fault-seed plan requires absolute paths".to_owned());
        }
        let reference_repository = std::fs::canonicalize(&plan.reference_repository)
            .map_err(|error| format!("canonicalize fault-seed reference repository: {error}"))?;
        let reference_witness = std::fs::canonicalize(&plan.reference_fixture_witness)
            .map_err(|error| format!("canonicalize fault-seed reference witness: {error}"))?;
        if !reference_repository.is_dir() || !reference_witness.is_file() {
            return Err("Timeline fault-seed reference authority is malformed".to_owned());
        }
        for destination in [&plan.fault_repository, &plan.fault_fixture_witness] {
            if std::fs::symlink_metadata(destination).is_ok() {
                return Err(format!(
                    "Timeline fault-seed destination must be absent: {}",
                    destination.display()
                ));
            }
        }
        let canonical_destination = |destination: &Path, label: &str| -> Result<PathBuf, String> {
            let parent = destination
                .parent()
                .ok_or_else(|| format!("{label} has no parent directory"))?;
            let name = destination
                .file_name()
                .ok_or_else(|| format!("{label} has no file name"))?;
            let parent = std::fs::canonicalize(parent)
                .map_err(|error| format!("canonicalize {label} parent: {error}"))?;
            Ok(parent.join(name))
        };
        let fault_repository =
            canonical_destination(&plan.fault_repository, "fault-seed fault repository")?;
        let fault_witness =
            canonical_destination(&plan.fault_fixture_witness, "fault-seed fault witness")?;
        let sources = [reference_repository.as_path(), reference_witness.as_path()];
        let destinations = [fault_repository.as_path(), fault_witness.as_path()];
        let overlap = destinations.iter().any(|destination| {
            sources
                .iter()
                .any(|source| paths_overlap(destination, source))
        }) || paths_overlap(&fault_repository, &fault_witness)
            || paths_overlap(&reference_repository, &reference_witness);
        if overlap {
            return Err("Timeline fault-seed authority paths overlap".to_owned());
        }

        let reference_store = std::fs::canonicalize(
            store_dir_for_repo(&reference_repository).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("canonicalize fault-seed reference store: {error}"))?;
        if !reference_store.starts_with(&reference_repository) {
            return Err(
                "Timeline fault-seed reference store resolves outside its repository".to_owned(),
            );
        }
        let authoritative_before =
            longitudinal_authoritative_store_data_inventory_v1(&reference_repository)
                .map_err(|error| error.to_string())?;
        let inclusive_before = longitudinal_store_data_inventory_v1(&reference_repository)
            .map_err(|error| error.to_string())?;
        validate_timeline_fault_seed_derived_surface_v1(&reference_repository)?;
        let initial_trust_path = allowed_signers_path_for_repo(&reference_repository)
            .map_err(|error| error.to_string())?;
        let initial_trust_bytes = optional_file_bytes(&initial_trust_path)?;
        let manifest_before = timeline_fault_seed_tree_manifest_v1(&reference_repository)?;

        let staging_name = |name: &std::ffi::OsStr, label: &str| -> Result<String, String> {
            name.to_str()
                .map(|text| format!(".{text}.pointbreak-fault-seed"))
                .ok_or_else(|| format!("{label} name is not UTF-8"))
        };
        let fault_repository_name = fault_repository
            .file_name()
            .ok_or_else(|| "fault-seed fault repository has no name".to_owned())?;
        let fault_witness_name = fault_witness
            .file_name()
            .ok_or_else(|| "fault-seed fault witness has no name".to_owned())?;
        let staging_repository =
            fault_repository
                .parent()
                .expect("canonical parent")
                .join(staging_name(
                    fault_repository_name,
                    "fault-seed fault repository",
                )?);
        let staging_witness = fault_witness
            .parent()
            .expect("canonical parent")
            .join(staging_name(
                fault_witness_name,
                "fault-seed fault witness",
            )?);
        for staging in [&staging_repository, &staging_witness] {
            if std::fs::symlink_metadata(staging).is_ok() {
                return Err(format!(
                    "Timeline fault-seed staging path must be absent: {}",
                    staging.display()
                ));
            }
        }

        let seeded = (|| -> Result<QualificationDerivedTimelineFaultSeedReceiptV1, String> {
            copy_timeline_fault_seed_tree_v1(&reference_repository, &staging_repository)?;
            let staged_manifest = timeline_fault_seed_tree_manifest_v1(&staging_repository)?;
            if staged_manifest.manifest_sha256 != manifest_before.manifest_sha256 {
                return Err("Timeline fault-seed copy did not validate byte-exact".to_owned());
            }
            let witness_bytes = std::fs::read(&reference_witness)
                .map_err(|error| format!("read fault-seed reference witness: {error}"))?;
            let witness_sha256 = sha256_bytes_hex(&witness_bytes);
            std::fs::write(&staging_witness, &witness_bytes)
                .map_err(|error| format!("stage fault-seed witness: {error}"))?;
            std::fs::rename(&staging_repository, &fault_repository)
                .map_err(|error| format!("finalize fault-seed repository: {error}"))?;
            if injection
                == QualificationDerivedTimelineFaultSeedInjectionV1::AfterRepositoryFinalize
            {
                return Err("injected fault-seed failure after repository finalization".to_owned());
            }
            std::fs::rename(&staging_witness, &fault_witness)
                .map_err(|error| format!("finalize fault-seed witness: {error}"))?;

            let fault_store = std::fs::canonicalize(
                store_dir_for_repo(&fault_repository).map_err(|error| error.to_string())?,
            )
            .map_err(|error| format!("canonicalize fault-seed fault store: {error}"))?;
            if !fault_store.starts_with(&fault_repository)
                || paths_overlap(&fault_store, &reference_store)
            {
                return Err(
                    "Timeline fault-seed fault store is not repository-contained and disjoint"
                        .to_owned(),
                );
            }
            let manifest_after_source =
                timeline_fault_seed_tree_manifest_v1(&reference_repository)?;
            let fault_manifest = timeline_fault_seed_tree_manifest_v1(&fault_repository)?;
            if manifest_after_source.manifest_sha256 != manifest_before.manifest_sha256 {
                return Err("Timeline fault-seed source drifted during the clone".to_owned());
            }
            if fault_manifest.manifest_sha256 != manifest_before.manifest_sha256 {
                return Err("Timeline fault-seed clone did not finalize byte-exact".to_owned());
            }
            let fault_authoritative =
                longitudinal_authoritative_store_data_inventory_v1(&fault_repository)
                    .map_err(|error| error.to_string())?;
            let fault_inclusive = longitudinal_store_data_inventory_v1(&fault_repository)
                .map_err(|error| error.to_string())?;
            if fault_authoritative.inventory_sha256 != authoritative_before.inventory_sha256
                || fault_inclusive.inventory_sha256 != inclusive_before.inventory_sha256
            {
                return Err("Timeline fault-seed store inventory drifted".to_owned());
            }
            validate_timeline_fault_seed_derived_surface_v1(&fault_repository)?;
            let fault_trust_path = allowed_signers_path_for_repo(&fault_repository)
                .map_err(|error| error.to_string())?;
            if optional_file_bytes(&fault_trust_path)? != initial_trust_bytes {
                return Err("Timeline fault-seed initial trust drifted".to_owned());
            }
            let restored_witness_sha256 = sha256_bytes_hex(
                &std::fs::read(&fault_witness)
                    .map_err(|error| format!("read fault-seed fault witness: {error}"))?,
            );
            if restored_witness_sha256 != witness_sha256 {
                return Err("Timeline fault-seed witness copy drifted".to_owned());
            }

            let mut receipt = QualificationDerivedTimelineFaultSeedReceiptV1 {
                schema: QUALIFICATION_DERIVED_TIMELINE_FAULT_SEED_RECEIPT_SCHEMA_V1.to_owned(),
                reference_root_path_sha256: timeline_fault_seed_path_digest(
                    &reference_repository,
                    "reference repository",
                )?,
                fault_root_path_sha256: timeline_fault_seed_path_digest(
                    &fault_repository,
                    "fault repository",
                )?,
                reference_witness_path_sha256: timeline_fault_seed_path_digest(
                    &reference_witness,
                    "reference fixture witness",
                )?,
                fault_witness_path_sha256: timeline_fault_seed_path_digest(
                    &fault_witness,
                    "fault fixture witness",
                )?,
                witness_sha256,
                tree_manifest_sha256: manifest_before.manifest_sha256.clone(),
                authoritative_inventory_sha256: authoritative_before.inventory_sha256.clone(),
                inclusive_inventory_sha256: inclusive_before.inventory_sha256.clone(),
                initial_trust_sha256: initial_trust_bytes.as_deref().map(sha256_bytes_hex),
                cloned_file_count: manifest_before.file_count,
                cloned_byte_count: manifest_before.byte_count,
                receipt_sha256: String::new(),
            };
            receipt.receipt_sha256 = receipt.canonical_sha256()?;
            receipt.validate()?;
            Ok(receipt)
        })();
        match seeded {
            Ok(receipt) => Ok(receipt),
            Err(error) => {
                let mut cleanup_failures = Vec::new();
                for created_directory in [&staging_repository, &fault_repository] {
                    if let Err(cleanup_error) = std::fs::remove_dir_all(created_directory)
                        && cleanup_error.kind() != std::io::ErrorKind::NotFound
                    {
                        cleanup_failures
                            .push(format!("{}: {cleanup_error}", created_directory.display()));
                    }
                }
                for created_file in [&staging_witness, &fault_witness] {
                    if let Err(cleanup_error) = std::fs::remove_file(created_file)
                        && cleanup_error.kind() != std::io::ErrorKind::NotFound
                    {
                        cleanup_failures
                            .push(format!("{}: {cleanup_error}", created_file.display()));
                    }
                }
                if cleanup_failures.is_empty() {
                    Err(error)
                } else {
                    Err(format!(
                        "{error}; fault-seed cleanup also failed: {}",
                        cleanup_failures.join(", ")
                    ))
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_timeline_invalid_signature_lifecycle_diagnostic_v1(
        reference_repository: &Path,
        reference_pointbreak_home: &Path,
        reference_fixture_witness: &Path,
        fault_repository: &Path,
        fault_pointbreak_home: &Path,
        fault_fixture_witness: &Path,
        barrier_root: &Path,
        product_binary: &Path,
        fault_seed_receipt: &QualificationDerivedTimelineFaultSeedReceiptV1,
        fault: DerivedTimelineInvalidSignatureDiagnosticFaultV1,
    ) -> Result<QualificationDerivedTimelineReadEvidenceV1, String> {
        if !reference_repository.is_absolute()
            || !reference_pointbreak_home.is_absolute()
            || !reference_fixture_witness.is_absolute()
            || !fault_repository.is_absolute()
            || !fault_pointbreak_home.is_absolute()
            || !fault_fixture_witness.is_absolute()
            || !barrier_root.is_absolute()
            || !product_binary.is_absolute()
        {
            return Err("Timeline lifecycle diagnostic requires absolute paths".to_owned());
        }
        let platform = if cfg!(target_os = "macos") {
            QualificationDerivedAccessPlatformV1::MacosApfs
        } else if cfg!(target_os = "windows") {
            QualificationDerivedAccessPlatformV1::WindowsNtfs
        } else {
            return Err("Timeline lifecycle diagnostic requires APFS or NTFS".to_owned());
        };
        let source_checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let binary_sha256 = sha256_file(product_binary)?;
        let cargo_lock_sha256 = sha256_file(&source_checkout.join("Cargo.lock"))?;
        let source_commit = "0".repeat(40);
        let source_tree = "1".repeat(40);
        let digest = |label: &str| sha256_bytes_hex(label.as_bytes());
        let execution = QualificationDerivedAccessExecutionIdentityV1 {
            platform,
            source_commit: source_commit.clone(),
            source_tree: source_tree.clone(),
            cargo_lock_sha256: cargo_lock_sha256.clone(),
            binary_sha256: digest("timeline lifecycle diagnostic harness"),
            contract_schema: QUALIFICATION_DERIVED_ACCESS_CONTRACT_SCHEMA_V1.to_owned(),
            contract_sha256: QUALIFICATION_DERIVED_ACCESS_CONTRACT_SHA256_V1.to_owned(),
            root_provenance_sha256: digest(&format!(
                "timeline lifecycle diagnostic root:{}",
                reference_repository.display()
            )),
            command_sha256: digest("timeline lifecycle diagnostic command"),
            operating_system: std::env::consts::OS.to_owned(),
            architecture: std::env::consts::ARCH.to_owned(),
            filesystem: if cfg!(target_os = "macos") {
                "apfs".to_owned()
            } else {
                "ntfs".to_owned()
            },
            host_identity_sha256: digest("timeline lifecycle diagnostic host"),
            source_dirty: false,
            private_corpus_configured: false,
        };
        let product = QualificationDerivedAccessProductIdentityV1 {
            platform,
            source_commit,
            source_tree,
            cargo_lock_sha256,
            binary_sha256,
            version_sha256: digest("timeline lifecycle diagnostic version"),
            build_profile: "integration-test".to_owned(),
            enabled_features: vec!["longitudinal-counting".to_owned()],
            build_command_sha256: digest("timeline lifecycle diagnostic build"),
            operating_system: std::env::consts::OS.to_owned(),
            architecture: std::env::consts::ARCH.to_owned(),
            source_dirty: false,
        };
        let probes = QualificationDerivedStorageForbiddenProbeInputV1::new(
            "timeline-lifecycle-diagnostic-proposal",
            "timeline-lifecycle-diagnostic-prose",
            "timeline-lifecycle-diagnostic-payload",
            "timeline-lifecycle-diagnostic-private-path",
        )?;
        let request = QualificationDerivedChangeReadRunRequestV1 {
            schema: QUALIFICATION_DERIVED_CHANGE_READ_REQUEST_SCHEMA_V1.to_owned(),
            purpose: QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification,
            source_checkout: source_checkout.clone(),
            execution,
            product_source_checkout: source_checkout,
            product,
            fixture: QualificationDerivedChangeFixtureV1::TopologyV1,
            fixture_witness: reference_fixture_witness.to_owned(),
            fixture_witness_sha256: sha256_file(reference_fixture_witness)?,
            repository: reference_repository.to_owned(),
            pointbreak_home: reference_pointbreak_home.to_owned(),
            product_binary: product_binary.to_owned(),
            control_test_binary: None,
            control_test_binary_sha256: None,
            control_test_build_command_sha256: None,
            control_cli_test_binary: None,
            control_cli_test_binary_sha256: None,
            control_cli_test_build_command_sha256: None,
            storage_forbidden_probes: Some(probes.clone()),
            summary_query: "Decision".to_owned(),
        };
        let mut fault_execution = request.execution.clone();
        fault_execution.root_provenance_sha256 = digest(&format!(
            "timeline lifecycle diagnostic fault root:{}",
            fault_repository.display()
        ));
        let fault_root = QualificationDerivedTimelineFaultRootV1 {
            repository: fault_repository.to_owned(),
            pointbreak_home: fault_pointbreak_home.to_owned(),
            fixture_witness: fault_fixture_witness.to_owned(),
            fixture_witness_sha256: sha256_file(fault_fixture_witness)?,
            barrier_root: barrier_root.to_owned(),
            execution: fault_execution,
            fault_seed_receipt: fault_seed_receipt.clone(),
        };
        validate_timeline_fault_authority_layout(&request, &fault_root)?;
        validate_timeline_fault_seed_bindings_v1(
            reference_repository,
            fault_repository,
            fault_seed_receipt,
        )?;
        let fixture_inventory =
            longitudinal_authoritative_store_data_inventory_v1(reference_repository)
                .map_err(|error| error.to_string())?;
        let derived = InspectorChild::spawn(&request, "sqlite-wal-bodyless-v1")?;
        derived.ensure_ready()?;
        let strict = InspectorChild::spawn(&request, "off")?;
        let product_identity_sha256 = request.product.canonical_sha256()?;
        let execution_identity_sha256 = request.execution.canonical_sha256()?;
        let store_root =
            store_dir_for_repo(reference_repository).map_err(|error| error.to_string())?;
        let storage = QualificationDerivedChangeStorageEvidenceV1 {
            platform,
            fixture: QualificationDerivedChangeFixtureV1::TopologyV1,
            phase: QualificationDerivedChangeStoragePhaseV1::InitialPublication,
            fixture_inventory_sha256: fixture_inventory.inventory_sha256.clone(),
            fixture_witness_sha256: request.fixture_witness_sha256.clone(),
            product_identity_sha256: product_identity_sha256.clone(),
            execution_identity_sha256: execution_identity_sha256.clone(),
            witness: capture_qualification_derived_storage_witness_v1(&store_root, &probes)
                .map_err(|error| {
                    format!("lifecycle initial-publication storage witness: {error}")
                })?,
        };
        run_timeline_case_with_fault(
            &request,
            QualificationDerivedTimelineReadCaseV1::TrustSuite,
            &derived.endpoint,
            Some(&strict.endpoint),
            &product_identity_sha256,
            &execution_identity_sha256,
            &fixture_inventory.inventory_sha256,
            Some(&storage),
            Some(&fault_root),
            fault,
        )
    }

    pub(super) fn run_qualification_derived_change_read_v1(
        request_path: &Path,
    ) -> Result<QualificationDerivedChangeReadReceiptV2, String> {
        let (request, timeline_fault_root) = read_qualification_change_read_request(request_path)?;
        request.validate()?;
        validate_current_execution_identity_v1(
            &request.execution,
            &request.source_checkout,
            &request.repository,
        )?;
        validate_product_identity(&request)?;
        validate_environment(&request)?;
        validate_public_fixture_shape(&request)?;

        let fixture_before =
            longitudinal_authoritative_store_data_inventory_v1(&request.repository)
                .map_err(|error| error.to_string())?;
        validate_fixture_witness(&request, &fixture_before.inventory_sha256)?;
        let fault_request = timeline_fault_root
            .as_ref()
            .map(|fault| request_for_timeline_fault_root(&request, fault))
            .transpose()?;
        if request.purpose == QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification
            && request.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
            && fault_request.is_none()
        {
            return Err(
                "Timeline qualification omitted its independent fault authority".to_owned(),
            );
        }
        if let Some(fault_request) = &fault_request {
            fault_request.validate()?;
            validate_current_execution_identity_v1(
                &fault_request.execution,
                &fault_request.source_checkout,
                &fault_request.repository,
            )?;
            validate_public_fixture_shape(fault_request)?;
            let fault_seed_receipt = &timeline_fault_root
                .as_ref()
                .expect("fault request implies a fault root")
                .fault_seed_receipt;
            let fault_inventory =
                longitudinal_authoritative_store_data_inventory_v1(&fault_request.repository)
                    .map_err(|error| error.to_string())?;
            if fault_inventory.inventory_sha256 != fixture_before.inventory_sha256 {
                return Err(
                    "Timeline fault authority does not match the reference inventory".to_owned(),
                );
            }
            validate_timeline_fault_seed_bindings_v1(
                &request.repository,
                &fault_request.repository,
                fault_seed_receipt,
            )?;
            validate_fixture_witness(fault_request, &fault_inventory.inventory_sha256)?;
        }

        let product_binary_sha256 = sha256_file(&request.product_binary)?;
        if product_binary_sha256 != request.product.binary_sha256 {
            return Err("derived Change product binary drifted".to_owned());
        }
        let product_version_sha256 = validate_product_version(&request)?;
        if product_version_sha256 != request.product.version_sha256 {
            return Err("derived Change product version drifted".to_owned());
        }
        let fixture_builder = if request.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
        {
            request
                .source_checkout
                .join("scripts/materialize-inspector-decision-matrix.sh")
        } else {
            request
                .source_checkout
                .join("src/bench_support/derived_access/materializer.rs")
        };
        let fixture_builder_sha256 = sha256_file(&fixture_builder)?;
        let activation_fixture_sha256 = sha256_file(
            &request
                .source_checkout
                .join("tests/support/assets/change-ready-store")
                .join(QUALIFICATION_DERIVED_CHANGE_ACTIVATION_FIXTURE_V1),
        )?;
        let completion_fixture_sha256 = sha256_file(
            &request
                .source_checkout
                .join("tests/support/assets/change-ready-store")
                .join(QUALIFICATION_DERIVED_CHANGE_COMPLETION_FIXTURE_V1),
        )?;

        let has_strict_cases = request
            .fixture
            .required_cases()
            .iter()
            .copied()
            .any(|case| {
                qualification_derived_change_expected_outcome_v1(
                    request.execution.platform,
                    request.fixture,
                    case,
                )
                .0 != QualificationDerivedChangeReadOracleV1::TypedFailure
            });
        let fixture_semantics_are_ready = qualification_derived_change_expected_outcome_v1(
            request.execution.platform,
            request.fixture,
            QualificationDerivedChangeReadCaseV1::ChangesBounded,
        )
        .0
            != QualificationDerivedChangeReadOracleV1::TypedFailure;
        let derived = InspectorChild::spawn(&request, "sqlite-wal-bodyless-v1")?;
        if has_strict_cases {
            derived.ensure_ready()?;
        }
        let authoritative = if has_strict_cases {
            Some(InspectorChild::spawn(&request, "off")?)
        } else {
            None
        };
        if fixture_semantics_are_ready {
            validate_fixture_semantics(&request, &derived.endpoint)?;
            validate_fixture_semantics(
                &request,
                &authoritative
                    .as_ref()
                    .ok_or_else(|| "strict fixture omitted its authoritative child".to_owned())?
                    .endpoint,
            )?;
        }
        let access = (request.purpose
            == QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification)
            .then(|| DerivedChangeAccess::resolve_for_inspector(&request.repository))
            .transpose()
            .map_err(|error| error.to_string())?;
        if access.as_ref().is_some_and(|access| !access.is_active()) {
            return Err(
                "derived Change qualification adapter resolved explicit-off state".to_owned(),
            );
        }

        let product_identity_sha256 = request.product.canonical_sha256()?;
        let execution_identity_sha256 = request.execution.canonical_sha256()?;
        let store_root =
            store_dir_for_repo(&request.repository).map_err(|error| error.to_string())?;
        let mut storage_rows = if let Some(probes) = request.storage_forbidden_probes.as_ref() {
            vec![QualificationDerivedChangeStorageEvidenceV1 {
                platform: request.execution.platform,
                fixture: request.fixture,
                phase: QualificationDerivedChangeStoragePhaseV1::InitialPublication,
                fixture_inventory_sha256: fixture_before.inventory_sha256.clone(),
                fixture_witness_sha256: request.fixture_witness_sha256.clone(),
                product_identity_sha256: product_identity_sha256.clone(),
                execution_identity_sha256: execution_identity_sha256.clone(),
                witness: capture_qualification_derived_storage_witness_v1(&store_root, probes)
                    .map_err(|error| format!("initial-publication storage witness: {error}"))?,
            }]
        } else {
            Vec::new()
        };

        let initial_storage = storage_rows.first().cloned();
        let mut timeline_storage_rows = Vec::new();
        let mut timeline_rows = Vec::new();
        let mut pending_timeline_post_append = None;
        for &case in required_timeline_cases_v1(request.fixture) {
            if case == QualificationDerivedTimelineReadCaseV1::PostAppendSuite {
                pending_timeline_post_append = Some(begin_timeline_post_append(
                    &request,
                    &derived.endpoint,
                    authoritative.as_ref().map(|child| &child.endpoint),
                    &product_identity_sha256,
                    &execution_identity_sha256,
                    &fixture_before.inventory_sha256,
                    initial_storage.as_ref(),
                )?);
            } else {
                timeline_rows.push(run_timeline_case(
                    &request,
                    timeline_fault_root.as_ref(),
                    case,
                    &derived.endpoint,
                    authoritative.as_ref().map(|child| &child.endpoint),
                    &product_identity_sha256,
                    &execution_identity_sha256,
                    &fixture_before.inventory_sha256,
                    initial_storage.as_ref(),
                )?);
            }
        }
        if initial_storage.is_some() {
            timeline_storage_rows.push(capture_timeline_storage_row(
                &request,
                &derived.endpoint,
                QualificationDerivedChangeStoragePhaseV1::InitialPublication,
                &product_identity_sha256,
                &execution_identity_sha256,
                &fixture_before.inventory_sha256,
            )?);
        }

        let mut rows = Vec::new();
        let mut post_append_generation_sha256 = None;
        for &case in request.fixture.required_cases() {
            let expected =
                expected_fixture_outcome(request.execution.platform, request.fixture, case);
            let authoritative_endpoint =
                if expected.oracle != QualificationDerivedChangeReadOracleV1::TypedFailure {
                    authoritative.as_ref().map(|child| &child.endpoint)
                } else {
                    None
                };
            let semantic = if case == QualificationDerivedChangeReadCaseV1::PostAppendSuite {
                let (semantic, generation_sha256) = post_append_semantic_pair(
                    &request,
                    &derived.endpoint,
                    authoritative_endpoint.ok_or_else(|| {
                        "post-append Change suite omitted its strict child".to_owned()
                    })?,
                )?;
                post_append_generation_sha256 = Some(generation_sha256);
                semantic
            } else if matches!(
                case,
                QualificationDerivedChangeReadCaseV1::FreshProcessSuite
                    | QualificationDerivedChangeReadCaseV1::PostAppendFreshProcessSuite
            ) {
                fresh_process_semantic_pair(&request, case, &expected)?
            } else {
                semantic_pair(
                    &request,
                    case,
                    &request.summary_query,
                    &derived.endpoint,
                    authoritative_endpoint,
                    &expected,
                )?
            };
            let measured = if let Some(access) = access.as_ref() {
                measure_case(
                    access,
                    &request.repository,
                    case,
                    &request.summary_query,
                    &expected,
                )?
            } else {
                MeasuredCase {
                    counters: LongitudinalCountersV1::default(),
                    expected_typed_document: semantic.typed_document.clone(),
                }
            };
            let passed = semantic.wire_contract_matches
                && measured.expected_typed_document == semantic.typed_document;
            rows.push(QualificationDerivedChangeReadEvidenceV1 {
                platform: request.execution.platform,
                fixture: request.fixture,
                fixture_inventory_sha256: fixture_before.inventory_sha256.clone(),
                fixture_witness_sha256: request.fixture_witness_sha256.clone(),
                case,
                semantic_process_scope:
                    QualificationDerivedAccessProcessScopeV1::InspectorServiceChild,
                counter_process_scope:
                    QualificationDerivedAccessProcessScopeV1::QualificationHarness,
                product_identity_sha256: product_identity_sha256.clone(),
                counter_execution_identity_sha256: execution_identity_sha256.clone(),
                status: if passed {
                    QualificationDerivedAccessStatusV1::Passed
                } else {
                    QualificationDerivedAccessStatusV1::Failed
                },
                oracle: expected.oracle,
                strict_semantic_sha256: semantic.strict_sha256,
                derived_semantic_sha256: semantic.derived_sha256,
                wire_contract_matches: semantic.wire_contract_matches,
                expected_http_status: expected.http_status,
                observed_http_status: semantic.observed_http_status,
                expected_code: expected.code.map(str::to_owned),
                observed_code: semantic.observed_code,
                expected_typed_document: measured.expected_typed_document,
                observed_typed_document: semantic.typed_document,
                counters: measured.counters,
            });
        }

        if let Some(pending) = pending_timeline_post_append {
            timeline_rows.push(finish_timeline_post_append(
                &request,
                &derived.endpoint,
                authoritative.as_ref().map(|child| &child.endpoint),
                &product_identity_sha256,
                &execution_identity_sha256,
                &fixture_before.inventory_sha256,
                initial_storage.as_ref(),
                pending,
            )?);
        }

        if request.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
            && let Some(probes) = request.storage_forbidden_probes.as_ref()
        {
            storage_rows.push(QualificationDerivedChangeStorageEvidenceV1 {
                platform: request.execution.platform,
                fixture: request.fixture,
                phase: QualificationDerivedChangeStoragePhaseV1::PostAppendCheckpoint,
                fixture_inventory_sha256: fixture_before.inventory_sha256.clone(),
                fixture_witness_sha256: request.fixture_witness_sha256.clone(),
                product_identity_sha256: product_identity_sha256.clone(),
                execution_identity_sha256: execution_identity_sha256.clone(),
                witness: capture_qualification_derived_storage_witness_v1(&store_root, probes)
                    .map_err(|error| format!("post-append-checkpoint storage witness: {error}"))?,
            });
            timeline_storage_rows.push(capture_timeline_storage_row(
                &request,
                &derived.endpoint,
                QualificationDerivedChangeStoragePhaseV1::PostAppendCheckpoint,
                &product_identity_sha256,
                &execution_identity_sha256,
                &fixture_before.inventory_sha256,
            )?);
        }

        let (control_binary_identities, control_rows) = if request.fixture
            == QualificationDerivedChangeFixtureV1::TopologyV1
            && request.purpose
                == QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification
        {
            run_control_matrix(
                &request,
                &product_identity_sha256,
                &execution_identity_sha256,
            )?
        } else {
            (Vec::new(), Vec::new())
        };

        drop(authoritative);
        drop(derived);
        let fixture_after = longitudinal_authoritative_store_data_inventory_v1(&request.repository)
            .map_err(|error| error.to_string())?;
        if request.fixture == QualificationDerivedChangeFixtureV1::TopologyV1 {
            if fixture_after == fixture_before || post_append_generation_sha256.is_none() {
                return Err("derived Change post-append evidence did not advance truth".to_owned());
            }
        } else if fixture_after != fixture_before || post_append_generation_sha256.is_some() {
            return Err("derived Change evidence mutated authoritative fixture bytes".to_owned());
        }

        let pre_cut_deficiencies =
            if request.purpose == QualificationDerivedChangeEvidencePurposeV1::PreCutFalsifier {
                rows.iter()
                    .filter(|row| row.status == QualificationDerivedAccessStatusV1::Failed)
                    .map(|row| row.case)
                    .collect()
            } else {
                Vec::new()
            };

        let mut receipt = QualificationDerivedChangeReadReceiptV1 {
            schema: QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V1.to_owned(),
            purpose: request.purpose,
            execution: request.execution,
            product: request.product,
            fixture: request.fixture,
            fixture_builder_sha256,
            activation_fixture_sha256,
            completion_fixture_sha256,
            fixture_inventory_sha256: fixture_before.inventory_sha256,
            fixture_after_inventory_sha256: fixture_after.inventory_sha256,
            fixture_witness_sha256: request.fixture_witness_sha256,
            post_append_generation_sha256,
            rows,
            pre_cut_deficiencies,
            control_binary_identities,
            control_rows,
            storage_rows,
            complete: true,
            receipt_sha256: String::new(),
        };
        receipt.refresh_sha256()?;
        receipt.validate()?;
        let mut successor = QualificationDerivedChangeReadReceiptV2 {
            schema: QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V2.to_owned(),
            base: receipt,
            timeline_read_rows: timeline_rows,
            timeline_storage_rows,
            complete: true,
            receipt_sha256: String::new(),
        };
        successor.refresh_sha256()?;
        successor.validate()?;
        Ok(successor)
    }

    pub(super) fn run_derived_change_read_diagnostic_v1(
        request_path: &Path,
    ) -> Result<DerivedChangeReadDiagnosticCollectionV1, String> {
        let diagnostic: DerivedChangeReadDiagnosticRunRequestV1 = read_json(request_path)?;
        diagnostic.validate()?;

        let request = &diagnostic.read_request;
        let source = diagnostic_preflight(
            DerivedChangeReadDiagnosticPreflightKindV1::Source,
            diagnostic_source_preflight(request),
        );
        if source.status != DerivedChangeReadDiagnosticStatusV1::Passed {
            let detail = source
                .failure_detail
                .clone()
                .unwrap_or_else(|| "source diagnostic preflight failed".to_owned());
            let mut preflight = vec![
                source,
                diagnostic_skipped_preflight(
                    DerivedChangeReadDiagnosticPreflightKindV1::Fixture,
                    &detail,
                ),
            ];
            if request.fixture == QualificationDerivedChangeFixtureV1::TopologyV1 {
                preflight.extend([
                    diagnostic_skipped_preflight(
                        DerivedChangeReadDiagnosticPreflightKindV1::LibraryControl,
                        &detail,
                    ),
                    diagnostic_skipped_preflight(
                        DerivedChangeReadDiagnosticPreflightKindV1::CliControl,
                        &detail,
                    ),
                ]);
            }
            preflight.push(diagnostic_skipped_preflight(
                DerivedChangeReadDiagnosticPreflightKindV1::TemplatePostflight,
                &detail,
            ));
            return Ok(DerivedChangeReadDiagnosticCollectionV1 {
                mode: DERIVED_CHANGE_READ_DIAGNOSTIC_MODE_V1.to_owned(),
                source_unchanged: true,
                preflight,
                rows: diagnostic_skipped_rows(request.fixture, &detail),
                controls: diagnostic_skipped_controls(request.fixture, &detail),
                storage: diagnostic_skipped_storage(request.fixture, &detail),
            });
        }

        let template_before =
            longitudinal_authoritative_store_data_inventory_v1(&request.repository)
                .map_err(|error| error.to_string());
        let fixture = diagnostic_preflight(
            DerivedChangeReadDiagnosticPreflightKindV1::Fixture,
            match &template_before {
                Ok(inventory) => diagnostic_fixture_preflight(
                    request,
                    &diagnostic.workspace_root,
                    &inventory.inventory_sha256,
                ),
                Err(detail) => Err(detail.clone()),
            },
        );

        let library =
            (request.fixture == QualificationDerivedChangeFixtureV1::TopologyV1).then(|| {
                diagnostic_control_preflight(
                    request,
                    QualificationDerivedChangeControlBinaryKindV1::Library,
                )
            });
        let cli = (request.fixture == QualificationDerivedChangeFixtureV1::TopologyV1).then(|| {
            diagnostic_control_preflight(
                request,
                QualificationDerivedChangeControlBinaryKindV1::Cli,
            )
        });
        let rows = collect_derived_change_read_diagnostic_rows_after_preflight_for_cases_v1(
            request.fixture.required_cases(),
            &fixture,
            |case| run_diagnostic_read_case(request, &diagnostic.workspace_root, case),
        );
        let controls = match (library.as_ref(), cli.as_ref()) {
            (Some(library), Some(cli)) => collect_derived_change_read_diagnostic_controls_v1(
                [library.clone(), cli.clone()],
                |case| run_diagnostic_control_case(request, case),
            ),
            (None, None) => Vec::new(),
            _ => {
                return Err(
                    "derived Change diagnostic control preflight is inconsistent".to_owned(),
                );
            }
        };
        let storage = if fixture.status == DerivedChangeReadDiagnosticStatusV1::Passed {
            collect_derived_change_read_diagnostic_storage_for_cases_v1(
                DerivedChangeReadDiagnosticStorageCaseV1::required_for(request.fixture),
                |case| run_diagnostic_storage_case(request, &diagnostic.workspace_root, case),
            )
        } else {
            diagnostic_skipped_storage(
                request.fixture,
                fixture
                    .failure_detail
                    .as_deref()
                    .unwrap_or("fixture diagnostic preflight failed"),
            )
        };

        let (source_unchanged, postflight) = match template_before {
            Ok(template_before) => diagnostic_template_postflight(
                longitudinal_authoritative_store_data_inventory_v1(&request.repository)
                    .map(|template_after| template_before == template_after)
                    .map_err(|error| error.to_string()),
            ),
            Err(detail) => diagnostic_template_postflight(Err(format!(
                "initial immutable fixture inventory failed: {detail}"
            ))),
        };
        Ok(DerivedChangeReadDiagnosticCollectionV1 {
            mode: DERIVED_CHANGE_READ_DIAGNOSTIC_MODE_V1.to_owned(),
            source_unchanged,
            preflight: [Some(source), Some(fixture), library, cli, Some(postflight)]
                .into_iter()
                .flatten()
                .collect(),
            rows,
            controls,
            storage,
        })
    }

    fn diagnostic_preflight(
        kind: DerivedChangeReadDiagnosticPreflightKindV1,
        result: Result<(), String>,
    ) -> DerivedChangeReadDiagnosticPreflightV1 {
        match result {
            Ok(()) => DerivedChangeReadDiagnosticPreflightV1::passed(kind),
            Err(detail) => DerivedChangeReadDiagnosticPreflightV1::failed(kind, detail),
        }
    }

    pub(super) fn diagnostic_template_postflight(
        result: Result<bool, String>,
    ) -> (bool, DerivedChangeReadDiagnosticPreflightV1) {
        match result {
            Ok(true) => (
                true,
                DerivedChangeReadDiagnosticPreflightV1::passed(
                    DerivedChangeReadDiagnosticPreflightKindV1::TemplatePostflight,
                ),
            ),
            Ok(false) => (
                false,
                DerivedChangeReadDiagnosticPreflightV1::failed(
                    DerivedChangeReadDiagnosticPreflightKindV1::TemplatePostflight,
                    "derived Change diagnostic mutated its immutable fixture template".to_owned(),
                ),
            ),
            Err(detail) => (
                false,
                DerivedChangeReadDiagnosticPreflightV1::failed(
                    DerivedChangeReadDiagnosticPreflightKindV1::TemplatePostflight,
                    detail,
                ),
            ),
        }
    }

    fn diagnostic_skipped_preflight(
        kind: DerivedChangeReadDiagnosticPreflightKindV1,
        detail: &str,
    ) -> DerivedChangeReadDiagnosticPreflightV1 {
        DerivedChangeReadDiagnosticPreflightV1 {
            kind,
            status: DerivedChangeReadDiagnosticStatusV1::Skipped,
            failure_detail: Some(detail.to_owned()),
        }
    }

    fn diagnostic_skipped_rows(
        fixture: QualificationDerivedChangeFixtureV1,
        detail: &str,
    ) -> Vec<DerivedChangeReadDiagnosticRowV1> {
        fixture
            .required_cases()
            .iter()
            .copied()
            .map(|case| DerivedChangeReadDiagnosticRowV1 {
                case,
                status: DerivedChangeReadDiagnosticStatusV1::Skipped,
                failure_detail: Some(detail.to_owned()),
                failure_witness: None,
            })
            .collect()
    }

    fn diagnostic_skipped_controls(
        fixture: QualificationDerivedChangeFixtureV1,
        detail: &str,
    ) -> Vec<DerivedChangeReadDiagnosticControlV1> {
        if fixture != QualificationDerivedChangeFixtureV1::TopologyV1 {
            return Vec::new();
        }
        QualificationDerivedChangeControlCaseV1::ALL
            .into_iter()
            .map(|case| DerivedChangeReadDiagnosticControlV1 {
                case,
                status: DerivedChangeReadDiagnosticStatusV1::Skipped,
                failure_detail: Some(detail.to_owned()),
            })
            .collect()
    }

    fn diagnostic_skipped_storage(
        fixture: QualificationDerivedChangeFixtureV1,
        detail: &str,
    ) -> Vec<DerivedChangeReadDiagnosticStorageV1> {
        DerivedChangeReadDiagnosticStorageCaseV1::required_for(fixture)
            .iter()
            .copied()
            .map(|case| DerivedChangeReadDiagnosticStorageV1 {
                case,
                status: DerivedChangeReadDiagnosticStatusV1::Skipped,
                failure_detail: Some(detail.to_owned()),
            })
            .collect()
    }

    fn require_empty_diagnostic_workspace(workspace: &Path) -> Result<(), String> {
        if workspace.exists()
            && workspace
                .read_dir()
                .map_err(|error| error.to_string())?
                .next()
                .is_some()
        {
            return Err("derived Change diagnostic workspace must be absent or empty".to_owned());
        }
        Ok(())
    }

    fn diagnostic_source_preflight(
        request: &QualificationDerivedChangeReadRunRequestV1,
    ) -> Result<(), String> {
        for variable in [
            "POINTBREAK_HOME",
            "POINTBREAK_STORE",
            "POINTBREAK_QUALIFICATION_CORPUS",
            "POINTBREAK_CHANGE_READY_FIXTURE_DIR",
        ] {
            if std::env::var_os(variable).is_some() {
                return Err(format!(
                    "derived Change diagnostic forbids ambient {variable}"
                ));
            }
        }
        validate_current_execution_identity_v1(
            &request.execution,
            &request.source_checkout,
            &request.repository,
        )?;
        validate_product_identity(request)?;
        if sha256_file(&request.product_binary)? != request.product.binary_sha256 {
            return Err("derived Change product binary drifted".to_owned());
        }
        Ok(())
    }

    fn diagnostic_fixture_preflight(
        request: &QualificationDerivedChangeReadRunRequestV1,
        workspace: &Path,
        inventory_sha256: &str,
    ) -> Result<(), String> {
        require_empty_diagnostic_workspace(workspace)?;
        validate_public_fixture_shape(request)?;
        validate_fixture_witness(request, inventory_sha256)
    }

    fn diagnostic_control_preflight(
        request: &QualificationDerivedChangeReadRunRequestV1,
        kind: QualificationDerivedChangeControlBinaryKindV1,
    ) -> DerivedChangeReadDiagnosticPreflightV1 {
        let result = (|| {
            let (binary, expected_sha256, build_command_sha256) = match kind {
                QualificationDerivedChangeControlBinaryKindV1::Library => (
                    request
                        .control_test_binary
                        .as_deref()
                        .ok_or_else(|| "Change library control test binary is absent".to_owned())?,
                    request
                        .control_test_binary_sha256
                        .as_deref()
                        .ok_or_else(|| {
                            "Change library control test binary hash is absent".to_owned()
                        })?,
                    request
                        .control_test_build_command_sha256
                        .as_deref()
                        .ok_or_else(|| {
                            "Change library control build command is absent".to_owned()
                        })?,
                ),
                QualificationDerivedChangeControlBinaryKindV1::Cli => (
                    request
                        .control_cli_test_binary
                        .as_deref()
                        .ok_or_else(|| "Change CLI control test binary is absent".to_owned())?,
                    request
                        .control_cli_test_binary_sha256
                        .as_deref()
                        .ok_or_else(|| {
                            "Change CLI control test binary hash is absent".to_owned()
                        })?,
                    request
                        .control_cli_test_build_command_sha256
                        .as_deref()
                        .ok_or_else(|| "Change CLI control build command is absent".to_owned())?,
                ),
            };
            let actual_sha256 = sha256_file(binary)?;
            if actual_sha256 != expected_sha256 {
                return Err("Change control test binary drifted".to_owned());
            }
            attest_control_binary(request, kind, binary, &actual_sha256, build_command_sha256)
                .map(|_| ())
        })();
        diagnostic_preflight(
            match kind {
                QualificationDerivedChangeControlBinaryKindV1::Library => {
                    DerivedChangeReadDiagnosticPreflightKindV1::LibraryControl
                }
                QualificationDerivedChangeControlBinaryKindV1::Cli => {
                    DerivedChangeReadDiagnosticPreflightKindV1::CliControl
                }
            },
            result,
        )
    }

    fn run_diagnostic_control_case(
        request: &QualificationDerivedChangeReadRunRequestV1,
        case: QualificationDerivedChangeControlCaseV1,
    ) -> Result<(), String> {
        let (kind, test_name) = qualification_derived_change_control_test_v1(case);
        let binary = match kind {
            QualificationDerivedChangeControlBinaryKindV1::Library => request
                .control_test_binary
                .as_deref()
                .ok_or_else(|| "Change library control test binary is absent".to_owned())?,
            QualificationDerivedChangeControlBinaryKindV1::Cli => request
                .control_cli_test_binary
                .as_deref()
                .ok_or_else(|| "Change CLI control test binary is absent".to_owned())?,
        };
        let output = run_exact_control_test(binary, test_name, None)
            .map_err(|error| format!("run exact Change control {test_name}: {error}"))?;
        if !output.status.success() || !exact_libtest_passed(&output.stdout, test_name)? {
            return Err(format!("exact Change control {test_name} failed"));
        }
        Ok(())
    }

    pub(super) fn requires_pre_mutation_measurement(
        case: QualificationDerivedChangeReadCaseV1,
    ) -> bool {
        case == QualificationDerivedChangeReadCaseV1::StalePageToken
    }

    pub(super) fn requires_semantic_fixture_preflight(
        case: QualificationDerivedChangeReadCaseV1,
    ) -> bool {
        case != QualificationDerivedChangeReadCaseV1::Profile
    }

    fn measure_diagnostic_read_case(
        request: &QualificationDerivedChangeReadRunRequestV1,
        case: QualificationDerivedChangeReadCaseV1,
        expected: &ExpectedFixtureOutcome,
    ) -> Result<MeasuredCase, String> {
        let access = DerivedChangeAccess::resolve_for_inspector(&request.repository)
            .map_err(|error| error.to_string())?;
        if !access.is_active() {
            return Err("derived Change diagnostic adapter resolved explicit-off state".to_owned());
        }
        measure_case(
            &access,
            &request.repository,
            case,
            &request.summary_query,
            expected,
        )
    }

    fn run_diagnostic_read_case(
        template: &QualificationDerivedChangeReadRunRequestV1,
        workspace: &Path,
        case: QualificationDerivedChangeReadCaseV1,
    ) -> Result<(), DiagnosticReadFailure> {
        let root = diagnostic_case_root(workspace, "rows", case as u8, &format!("{case:?}"));
        let request = diagnostic_clone_request(template, &root)?;
        validate_diagnostic_clone(&request)?;
        let expected = expected_fixture_outcome(request.execution.platform, request.fixture, case);
        let mut prepared_derived = None;
        let mut pre_mutation_measurement = None;
        if requires_pre_mutation_measurement(case) {
            let derived = InspectorChild::spawn(&request, "sqlite-wal-bodyless-v1")?;
            derived.ensure_ready()?;
            pre_mutation_measurement =
                Some(measure_diagnostic_read_case(&request, case, &expected)?);
            prepared_derived = Some(derived);
        }

        let semantic = match case {
            QualificationDerivedChangeReadCaseV1::PostAppendFreshProcessSuite => {
                establish_diagnostic_post_append(&request)?;
                fresh_process_semantic_pair(&request, case, &expected)?
            }
            QualificationDerivedChangeReadCaseV1::FreshProcessSuite => {
                fresh_process_semantic_pair(&request, case, &expected)?
            }
            QualificationDerivedChangeReadCaseV1::PostAppendSuite => {
                let derived = InspectorChild::spawn(&request, "sqlite-wal-bodyless-v1")?;
                derived.ensure_ready()?;
                let authoritative = InspectorChild::spawn(&request, "off")?;
                validate_fixture_semantics(&request, &derived.endpoint)?;
                validate_fixture_semantics(&request, &authoritative.endpoint)?;
                post_append_semantic_pair(&request, &derived.endpoint, &authoritative.endpoint)?.0
            }
            _ => {
                let derived = match prepared_derived.take() {
                    Some(derived) => derived,
                    None => InspectorChild::spawn(&request, "sqlite-wal-bodyless-v1")?,
                };
                if expected.oracle != QualificationDerivedChangeReadOracleV1::TypedFailure {
                    derived.ensure_ready()?;
                }
                let authoritative = (expected.oracle
                    != QualificationDerivedChangeReadOracleV1::TypedFailure)
                    .then(|| InspectorChild::spawn(&request, "off"))
                    .transpose()?;
                if expected.oracle != QualificationDerivedChangeReadOracleV1::TypedFailure
                    && requires_semantic_fixture_preflight(case)
                {
                    validate_fixture_semantics(&request, &derived.endpoint)?;
                    validate_fixture_semantics(
                        &request,
                        &authoritative
                            .as_ref()
                            .ok_or_else(|| {
                                "strict diagnostic fixture omitted its authoritative child"
                                    .to_owned()
                            })?
                            .endpoint,
                    )?;
                }
                match semantic_pair_observed(
                    &request,
                    case,
                    &request.summary_query,
                    &derived.endpoint,
                    authoritative.as_ref().map(|child| &child.endpoint),
                    &expected,
                ) {
                    Ok(semantic) => semantic,
                    Err(failure)
                        if expected.oracle
                            == QualificationDerivedChangeReadOracleV1::TypedFailure =>
                    {
                        let measured = match pre_mutation_measurement.take() {
                            Some(measured) => measured,
                            None => measure_diagnostic_read_case(&request, case, &expected)?,
                        };
                        let expected_document =
                            measured.expected_typed_document.as_ref().ok_or_else(|| {
                                "typed diagnostic witness omitted its expected document".to_owned()
                            })?;
                        return Err(DiagnosticReadFailure {
                            detail: failure.detail,
                            witness: Some(Box::new(
                                DerivedChangeReadDiagnosticFailureWitnessV1::TypedFailure {
                                    observed: *failure.typed_witness.ok_or_else(|| {
                                        "typed diagnostic failure omitted its observed witness"
                                            .to_owned()
                                    })?,
                                    expected: diagnostic_expected_typed_witness(expected_document),
                                },
                            )),
                        });
                    }
                    Err(failure) => return Err(failure.detail.into()),
                }
            }
        };
        let measured = match pre_mutation_measurement {
            Some(measured) => measured,
            None => measure_diagnostic_read_case(&request, case, &expected)?,
        };
        if !semantic.wire_contract_matches
            || measured.expected_typed_document != semantic.typed_document
        {
            let witness = match expected.oracle {
                QualificationDerivedChangeReadOracleV1::StrictParity
                | QualificationDerivedChangeReadOracleV1::ReadyProfileParity => {
                    strict_diagnostic_witness(&semantic, &expected)?
                }
                QualificationDerivedChangeReadOracleV1::TypedFailure => {
                    let observed = semantic.typed_document.as_ref().ok_or_else(|| {
                        "typed diagnostic witness omitted its observed document".to_owned()
                    })?;
                    let expected_document =
                        measured.expected_typed_document.as_ref().ok_or_else(|| {
                            "typed diagnostic witness omitted its expected document".to_owned()
                        })?;
                    DerivedChangeReadDiagnosticFailureWitnessV1::TypedFailure {
                        observed: diagnostic_expected_typed_witness(observed),
                        expected: diagnostic_expected_typed_witness(expected_document),
                    }
                }
            };
            return Err(DiagnosticReadFailure {
                detail: format!(
                    "derived Change diagnostic case {case:?} did not satisfy its oracle"
                ),
                witness: Some(Box::new(witness)),
            });
        }
        Ok(())
    }

    fn establish_diagnostic_post_append(
        request: &QualificationDerivedChangeReadRunRequestV1,
    ) -> Result<(), String> {
        let derived = InspectorChild::spawn(request, "sqlite-wal-bodyless-v1")?;
        derived.ensure_ready()?;
        let authoritative = InspectorChild::spawn(request, "off")?;
        validate_fixture_semantics(request, &derived.endpoint)?;
        validate_fixture_semantics(request, &authoritative.endpoint)?;
        post_append_semantic_pair(request, &derived.endpoint, &authoritative.endpoint).map(|_| ())
    }

    fn run_diagnostic_storage_case(
        template: &QualificationDerivedChangeReadRunRequestV1,
        workspace: &Path,
        case: DerivedChangeReadDiagnosticStorageCaseV1,
    ) -> Result<(), String> {
        let root = diagnostic_case_root(workspace, "storage", case as u8, &format!("{case:?}"));
        let request = diagnostic_clone_request(template, &root)?;
        validate_diagnostic_clone(&request)?;
        let probes = request
            .storage_forbidden_probes
            .as_ref()
            .ok_or_else(|| "derived Change diagnostic storage probes are absent".to_owned())?;
        match case {
            DerivedChangeReadDiagnosticStorageCaseV1::Initial => {
                let derived = InspectorChild::spawn(&request, "sqlite-wal-bodyless-v1")?;
                derived.ensure_ready()?;
            }
            DerivedChangeReadDiagnosticStorageCaseV1::PostAppend => {
                establish_diagnostic_post_append(&request)?;
            }
        }
        let store_root =
            store_dir_for_repo(&request.repository).map_err(|error| error.to_string())?;
        capture_qualification_derived_storage_witness_v1(&store_root, probes).map(|_| ())
    }

    fn diagnostic_case_root(workspace: &Path, family: &str, ordinal: u8, label: &str) -> PathBuf {
        workspace.join(family).join(format!("{ordinal:02}-{label}"))
    }

    fn diagnostic_clone_request(
        template: &QualificationDerivedChangeReadRunRequestV1,
        root: &Path,
    ) -> Result<QualificationDerivedChangeReadRunRequestV1, String> {
        let mut request = template.clone();
        if template.fixture == QualificationDerivedChangeFixtureV1::TopologyV1 {
            copy_public_fixture_tree(&template.repository, root)?;
        } else {
            let witness = materialize_diagnostic_fixture_at_root(
                &template.source_checkout,
                root,
                template.fixture,
            )?;
            let witness_path = root.with_extension("fixture-witness.json");
            let witness_bytes = serde_json::to_vec(&witness).map_err(|error| error.to_string())?;
            std::fs::write(&witness_path, &witness_bytes).map_err(|error| error.to_string())?;
            request.fixture_witness = witness_path;
            request.fixture_witness_sha256 = sha256_bytes_hex(&witness_bytes);
        }
        request.repository = root.to_path_buf();
        request.pointbreak_home = root.join(".git/pointbreak-home");
        if let Some(probes) = request.storage_forbidden_probes.as_mut() {
            probes.private_path = root
                .to_str()
                .ok_or_else(|| "diagnostic fixture root is not UTF-8".to_owned())?
                .to_owned();
        }
        request.validate()?;
        Ok(request)
    }

    pub(super) fn materialize_diagnostic_fixture_at_root(
        source_checkout: &Path,
        root: &Path,
        fixture: QualificationDerivedChangeFixtureV1,
    ) -> Result<QualificationDerivedChangeFixtureWitnessV1, String> {
        let kind = match fixture {
            QualificationDerivedChangeFixtureV1::TopologyV1 => {
                return Err("topology diagnostic fixture uses its public materializer".to_owned());
            }
            QualificationDerivedChangeFixtureV1::DuplicateEqualV1 => {
                QualificationDerivedChangeFixtureKindV1::DuplicateEqual
            }
            QualificationDerivedChangeFixtureV1::DuplicateConflictV1 => {
                QualificationDerivedChangeFixtureKindV1::DuplicateConflicting
            }
            QualificationDerivedChangeFixtureV1::RemovalV1 => {
                QualificationDerivedChangeFixtureKindV1::OperativeRemoval
            }
            QualificationDerivedChangeFixtureV1::MissingCarrierV1 => {
                QualificationDerivedChangeFixtureKindV1::MissingSelectedCarrier
            }
            QualificationDerivedChangeFixtureV1::MutatedCarrierV1 => {
                QualificationDerivedChangeFixtureKindV1::MutatedSelectedCarrier
            }
            QualificationDerivedChangeFixtureV1::WrongFamilyCarrierV1 => {
                QualificationDerivedChangeFixtureKindV1::WrongFamilySelectedCarrier
            }
            QualificationDerivedChangeFixtureV1::IncompleteV1 => {
                QualificationDerivedChangeFixtureKindV1::IncompleteChange
            }
            QualificationDerivedChangeFixtureV1::CycleConflictedV1 => {
                QualificationDerivedChangeFixtureKindV1::CycleConflictedChange
            }
        };
        materialize_qualification_derived_change_fixture_v1(
            QualificationDerivedChangeFixtureRequestV1::new(root, kind)
                .with_source_checkout(source_checkout),
        )
    }

    fn validate_diagnostic_clone(
        request: &QualificationDerivedChangeReadRunRequestV1,
    ) -> Result<(), String> {
        validate_public_fixture_shape(request)?;
        let inventory = longitudinal_authoritative_store_data_inventory_v1(&request.repository)
            .map_err(|error| error.to_string())?;
        validate_fixture_witness(request, &inventory.inventory_sha256)?;
        if sha256_file(&request.product_binary)? != request.product.binary_sha256 {
            return Err("derived Change product binary drifted".to_owned());
        }
        if validate_product_version(request)? != request.product.version_sha256 {
            return Err("derived Change product version drifted".to_owned());
        }
        Ok(())
    }

    pub(super) fn copy_public_fixture_tree(
        source: &Path,
        destination: &Path,
    ) -> Result<(), String> {
        if destination.exists() {
            return Err("derived Change diagnostic clone destination already exists".to_owned());
        }
        let source = std::fs::canonicalize(source).map_err(|error| error.to_string())?;
        let destination_parent = destination.parent().ok_or_else(|| {
            "derived Change diagnostic clone destination has no parent".to_owned()
        })?;
        std::fs::create_dir_all(destination_parent).map_err(|error| error.to_string())?;
        copy_public_fixture_tree_inner(&source, destination)
    }

    fn copy_public_fixture_tree_inner(source: &Path, destination: &Path) -> Result<(), String> {
        let metadata = std::fs::symlink_metadata(source).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("derived Change diagnostic fixture must not contain symlinks".to_owned());
        }
        if metadata.is_dir() {
            std::fs::create_dir(destination).map_err(|error| error.to_string())?;
            for entry in std::fs::read_dir(source).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                copy_public_fixture_tree_inner(
                    &entry.path(),
                    &destination.join(entry.file_name()),
                )?;
            }
            return Ok(());
        }
        if metadata.is_file() {
            std::fs::copy(source, destination).map_err(|error| error.to_string())?;
            return Ok(());
        }
        Err("derived Change diagnostic fixture contains an unsupported file type".to_owned())
    }

    #[derive(Clone, Copy)]
    struct ExpectedFixtureOutcome {
        oracle: QualificationDerivedChangeReadOracleV1,
        http_status: u16,
        code: Option<&'static str>,
    }

    fn expected_fixture_outcome(
        platform: QualificationDerivedAccessPlatformV1,
        fixture: QualificationDerivedChangeFixtureV1,
        case: QualificationDerivedChangeReadCaseV1,
    ) -> ExpectedFixtureOutcome {
        let (oracle, http_status, code) =
            qualification_derived_change_expected_outcome_v1(platform, fixture, case);
        ExpectedFixtureOutcome {
            oracle,
            http_status,
            code,
        }
    }

    pub(super) fn validate_fixture_authoritative_inventory(
        witness: &Value,
        fixture_inventory_sha256: &str,
    ) -> Result<(), String> {
        let witnessed_inventory = witness
            .get("authoritativeInventorySha256")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                "derived Change fixture witness omitted authoritative inventory".to_owned()
            })?;
        if witnessed_inventory.len() != 64
            || !witnessed_inventory
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || witnessed_inventory != fixture_inventory_sha256
        {
            return Err("derived Change fixture witness authority drifted".to_owned());
        }
        Ok(())
    }

    fn validate_fixture_witness(
        request: &QualificationDerivedChangeReadRunRequestV1,
        fixture_inventory_sha256: &str,
    ) -> Result<(), String> {
        if sha256_file(&request.fixture_witness)? != request.fixture_witness_sha256 {
            return Err("derived Change fixture witness drifted".to_owned());
        }
        let witness: Value = read_json(&request.fixture_witness)?;
        let expected_fixture = serde_json::to_value(request.fixture)
            .map_err(|error| error.to_string())?
            .as_str()
            .ok_or_else(|| "derived Change fixture identifier is not a string".to_owned())?
            .to_owned();
        if witness.get("schema").and_then(Value::as_str)
            != Some("pointbreak.qualification-derived-change-fixture-witness.v1")
            || witness.get("fixtureId").and_then(Value::as_str) != Some(expected_fixture.as_str())
        {
            return Err("derived Change fixture witness is incompatible".to_owned());
        }
        validate_fixture_authoritative_inventory(&witness, fixture_inventory_sha256)?;
        let probe_hashes: QualificationDerivedStorageForbiddenProbeHashesV1 =
            serde_json::from_value(
                witness
                    .get("storageForbiddenProbeHashes")
                    .cloned()
                    .ok_or_else(|| {
                        "derived Change fixture witness omitted storage probe authority".to_owned()
                    })?,
            )
            .map_err(|error| error.to_string())?;
        probe_hashes.validate()?;
        if probe_hashes != qualification_derived_change_storage_probe_hashes_v1(request.fixture) {
            return Err("derived Change fixture witness has unknown storage probes".to_owned());
        }
        if let Some(probes) = request.storage_forbidden_probes.as_ref()
            && (probes.canonical_hashes() != probe_hashes
                || probes.private_path
                    != request
                        .repository
                        .to_str()
                        .ok_or_else(|| "fixture repository path is not UTF-8".to_owned())?)
        {
            return Err("derived Change storage probes do not match fixture authority".to_owned());
        }
        if request.fixture != QualificationDerivedChangeFixtureV1::TopologyV1 {
            let witness: QualificationDerivedChangeFixtureWitnessV1 =
                serde_json::from_value(witness).map_err(|error| error.to_string())?;
            witness.validate()?;
        }
        Ok(())
    }

    fn validate_fixture_semantics(
        request: &QualificationDerivedChangeReadRunRequestV1,
        endpoint: &InspectorEndpoint,
    ) -> Result<(), String> {
        let (status, document) = endpoint.json("/api/v2/changes")?;
        if status != 200 {
            return Err(format!(
                "derived Change fixture semantic preflight returned {status}"
            ));
        }
        let changes = document
            .get("changes")
            .and_then(Value::as_array)
            .ok_or_else(|| "derived Change fixture omitted its Changes array".to_owned())?;
        let witness: Value = read_json(&request.fixture_witness)?;
        if request.fixture == QualificationDerivedChangeFixtureV1::TopologyV1 {
            return validate_topology_fixture_semantics(&witness, changes);
        }
        let witness: QualificationDerivedChangeFixtureWitnessV1 =
            serde_json::from_value(witness).map_err(|error| error.to_string())?;
        let change = changes
            .iter()
            .find(|change| {
                change
                    .get("changeId")
                    .and_then(Value::as_str)
                    .is_some_and(|change_id| {
                        sha256_bytes_hex(change_id.as_bytes()) == witness.topology.change_id_sha256
                    })
            })
            .ok_or_else(|| "derived Change fixture omitted its witnessed Change".to_owned())?;
        if change.get("topology")
            != Some(
                &serde_json::to_value(witness.topology.expected_topology)
                    .map_err(|error| error.to_string())?,
            )
            || change.get("lifecycle")
                != Some(
                    &serde_json::to_value(witness.topology.expected_lifecycle)
                        .map_err(|error| error.to_string())?,
                )
        {
            return Err("derived Change fixture topology or lifecycle drifted".to_owned());
        }
        let observed_current = change
            .get("currentRevisionRefs")
            .and_then(Value::as_array)
            .ok_or_else(|| "derived Change fixture omitted current exact Revisions".to_owned())?
            .iter()
            .map(|exact| {
                canonical_json_bytes(exact)
                    .map(|bytes| sha256_bytes_hex(&bytes))
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        let expected_current = witness
            .topology
            .current_revision_ref_sha256
            .into_iter()
            .collect::<BTreeSet<_>>();
        if observed_current != expected_current {
            return Err("derived Change fixture current exact Revisions drifted".to_owned());
        }
        Ok(())
    }

    pub(super) fn validate_topology_fixture_semantics(
        witness: &Value,
        changes: &[Value],
    ) -> Result<(), String> {
        let topology = witness
            .get("topology")
            .and_then(Value::as_object)
            .ok_or_else(|| "topology fixture witness omitted its matrix".to_owned())?;
        for (name, expected_topology) in [
            ("initial", "initial"),
            ("replacement", "replacement"),
            ("parallel_current", "parallel_current"),
            ("replacement_divergent", "replacement_divergent"),
            ("consolidation", "consolidation"),
        ] {
            let expected = topology
                .get(name)
                .ok_or_else(|| format!("topology fixture omitted {name}"))?;
            let change_id = expected
                .get("change")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("topology fixture {name} omitted its Change"))?;
            let change = changes
                .iter()
                .find(|change| change.get("changeId").and_then(Value::as_str) == Some(change_id))
                .ok_or_else(|| format!("topology fixture did not emit {name}"))?;
            if change.get("topology").and_then(Value::as_str) != Some(expected_topology) {
                return Err(format!("topology fixture {name} classification drifted"));
            }
            let expected_current = topology_witness_current_refs(expected)?;
            let observed_current =
                change
                    .get("currentRevisionRefs")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        format!("topology fixture {name} omitted current exact Revisions")
                    })?
                    .iter()
                    .map(|exact| {
                        Ok((
                    exact
                        .get("revisionId")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            format!("topology fixture {name} emitted a Revision without identity")
                        })?
                        .to_owned(),
                    exact
                        .get("objectArtifactContentHash")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            format!("topology fixture {name} emitted a Revision without artifact")
                        })?
                        .to_owned(),
                ))
                    })
                    .collect::<Result<BTreeSet<_>, String>>()?;
            if observed_current != expected_current {
                return Err(format!(
                    "topology fixture {name} current exact Revisions drifted"
                ));
            }
        }
        Ok(())
    }

    fn topology_witness_current_refs(
        expected: &Value,
    ) -> Result<BTreeSet<(String, String)>, String> {
        let current = expected
            .get("current")
            .ok_or_else(|| "topology fixture omitted current exact Revisions".to_owned())?;
        let values = current
            .as_array()
            .cloned()
            .unwrap_or_else(|| vec![current.clone()]);
        values
            .into_iter()
            .map(|exact| {
                Ok((
                    exact
                        .get("revision")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            "topology fixture omitted current Revision identity".to_owned()
                        })?
                        .to_owned(),
                    exact
                        .get("artifact")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            "topology fixture omitted current artifact identity".to_owned()
                        })?
                        .to_owned(),
                ))
            })
            .collect()
    }

    fn validate_product_identity(
        request: &QualificationDerivedChangeReadRunRequestV1,
    ) -> Result<(), String> {
        if !request.product_source_checkout.is_dir()
            || !git_text(
                &request.product_source_checkout,
                &["status", "--porcelain=v1"],
            )?
            .is_empty()
            || git_text(&request.product_source_checkout, &["rev-parse", "HEAD"])?
                != request.product.source_commit
            || git_text(
                &request.product_source_checkout,
                &["rev-parse", "HEAD^{tree}"],
            )? != request.product.source_tree
            || sha256_file(&request.product_source_checkout.join("Cargo.lock"))?
                != request.product.cargo_lock_sha256
        {
            return Err("derived Change product source identity drifted".to_owned());
        }
        Ok(())
    }

    fn git_text(checkout: &Path, arguments: &[&str]) -> Result<String, String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(checkout)
            .args(arguments)
            .output()
            .map_err(|error| format!("inspect Change product source: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "inspect Change product source: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        String::from_utf8(output.stdout)
            .map(|value| value.trim().to_owned())
            .map_err(|error| error.to_string())
    }

    fn validate_product_version(
        request: &QualificationDerivedChangeReadRunRequestV1,
    ) -> Result<String, String> {
        let output = Command::new(&request.product_binary)
            .args(["version", "--format", "json"])
            .env("POINTBREAK_HOME", &request.pointbreak_home)
            .env_remove("POINTBREAK_QUALIFICATION_CORPUS")
            .output()
            .map_err(|error| format!("read Change product version: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "Change product version command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("Change product version is invalid JSON: {error}"))?;
        if value.get("schema").and_then(Value::as_str) != Some("pointbreak.version")
            || value.get("version").and_then(Value::as_u64) != Some(1)
            || value.pointer("/build/source").and_then(Value::as_str) != Some("git")
            || value.pointer("/build/commit").and_then(Value::as_str)
                != Some(request.product.source_commit.as_str())
            || value.pointer("/build/dirty").and_then(Value::as_bool) != Some(false)
        {
            return Err("Change product binary does not attest the exact clean source".to_owned());
        }
        canonical_json_bytes(&value)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())
    }

    fn run_control_matrix(
        request: &QualificationDerivedChangeReadRunRequestV1,
        product_identity_sha256: &str,
        execution_identity_sha256: &str,
    ) -> Result<
        (
            Vec<QualificationDerivedChangeControlBinaryIdentityV1>,
            Vec<QualificationDerivedChangeControlEvidenceV1>,
        ),
        String,
    > {
        let library_binary = request
            .control_test_binary
            .as_ref()
            .ok_or_else(|| "Change library control test binary is absent".to_owned())?;
        let expected_library_sha256 = request
            .control_test_binary_sha256
            .as_deref()
            .ok_or_else(|| "Change library control test binary hash is absent".to_owned())?;
        let library_build_command_sha256 = request
            .control_test_build_command_sha256
            .as_deref()
            .ok_or_else(|| "Change library control build command is absent".to_owned())?;
        let cli_binary = request
            .control_cli_test_binary
            .as_ref()
            .ok_or_else(|| "Change CLI control test binary is absent".to_owned())?;
        let expected_cli_sha256 = request
            .control_cli_test_binary_sha256
            .as_deref()
            .ok_or_else(|| "Change CLI control test binary hash is absent".to_owned())?;
        let cli_build_command_sha256 = request
            .control_cli_test_build_command_sha256
            .as_deref()
            .ok_or_else(|| "Change CLI control build command is absent".to_owned())?;
        let library_sha256 = sha256_file(library_binary)?;
        let cli_sha256 = sha256_file(cli_binary)?;
        if library_sha256 != expected_library_sha256 || cli_sha256 != expected_cli_sha256 {
            return Err("Change control test binary drifted".to_owned());
        }
        let library_identity = attest_control_binary(
            request,
            QualificationDerivedChangeControlBinaryKindV1::Library,
            library_binary,
            &library_sha256,
            library_build_command_sha256,
        )?;
        let cli_identity = attest_control_binary(
            request,
            QualificationDerivedChangeControlBinaryKindV1::Cli,
            cli_binary,
            &cli_sha256,
            cli_build_command_sha256,
        )?;
        let library_identity_sha256 = library_identity.canonical_sha256()?;
        let cli_identity_sha256 = cli_identity.canonical_sha256()?;
        let mut rows = Vec::new();
        for case in QualificationDerivedChangeControlCaseV1::ALL {
            let (binary_kind, test_name) = qualification_derived_change_control_test_v1(case);
            let (test_binary, test_binary_sha256, test_binary_identity_sha256) = match binary_kind {
                QualificationDerivedChangeControlBinaryKindV1::Library => {
                    (library_binary, &library_sha256, &library_identity_sha256)
                }
                QualificationDerivedChangeControlBinaryKindV1::Cli => {
                    (cli_binary, &cli_sha256, &cli_identity_sha256)
                }
            };
            let output = run_exact_control_test(test_binary, test_name, None)
                .map_err(|error| format!("run Change control {case:?}: {error}"))?;
            let exact_test_passed = exact_libtest_passed(&output.stdout, test_name)?;
            rows.push(QualificationDerivedChangeControlEvidenceV1 {
                platform: request.execution.platform,
                case,
                binary_kind,
                test_name: test_name.to_owned(),
                status: if output.status.success() && exact_test_passed {
                    QualificationDerivedAccessStatusV1::Passed
                } else {
                    QualificationDerivedAccessStatusV1::Failed
                },
                execution_identity_sha256: execution_identity_sha256.to_owned(),
                product_identity_sha256: product_identity_sha256.to_owned(),
                test_binary_identity_sha256: test_binary_identity_sha256.clone(),
                test_binary_sha256: test_binary_sha256.clone(),
                command_sha256: qualification_derived_change_control_command_sha256_v1(test_name),
                stdout_sha256: sha256_bytes_hex(&output.stdout),
                stderr_sha256: sha256_bytes_hex(&output.stderr),
                exit_code: output.status.code().unwrap_or(-1),
                tests_run: u16::from(exact_test_passed),
                tests_passed: u16::from(exact_test_passed && output.status.success()),
            });
        }
        Ok((vec![library_identity, cli_identity], rows))
    }

    fn attest_control_binary(
        request: &QualificationDerivedChangeReadRunRequestV1,
        kind: QualificationDerivedChangeControlBinaryKindV1,
        binary: &Path,
        binary_sha256: &str,
        build_command_sha256: &str,
    ) -> Result<QualificationDerivedChangeControlBinaryIdentityV1, String> {
        let attestation_test = qualification_derived_change_control_attestation_test_v1(kind);
        let output = run_exact_control_test(
            binary,
            attestation_test,
            Some(request.execution.source_commit.as_str()),
        )?;
        if !output.status.success() || !exact_libtest_passed(&output.stdout, attestation_test)? {
            return Err(format!(
                "{kind:?} Change control binary did not attest its exact clean source"
            ));
        }
        let identity = QualificationDerivedChangeControlBinaryIdentityV1 {
            platform: request.execution.platform,
            kind,
            source_commit: request.execution.source_commit.clone(),
            source_tree: request.execution.source_tree.clone(),
            cargo_lock_sha256: request.execution.cargo_lock_sha256.clone(),
            binary_sha256: binary_sha256.to_owned(),
            build_command_sha256: build_command_sha256.to_owned(),
            operating_system: request.execution.operating_system.clone(),
            architecture: request.execution.architecture.clone(),
            source_dirty: false,
            attestation_test: attestation_test.to_owned(),
            attestation_command_sha256: qualification_derived_change_control_command_sha256_v1(
                attestation_test,
            ),
            attestation_stdout_sha256: sha256_bytes_hex(&output.stdout),
            attestation_stderr_sha256: sha256_bytes_hex(&output.stderr),
        };
        identity.validate()?;
        Ok(identity)
    }

    fn run_exact_control_test(
        binary: &Path,
        test_name: &str,
        expected_source_commit: Option<&str>,
    ) -> Result<std::process::Output, String> {
        let mut command = Command::new(binary);
        command
            .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
            .env_remove("POINTBREAK_QUALIFICATION_CORPUS");
        if let Some(commit) = expected_source_commit {
            command.env("POINTBREAK_QUALIFICATION_EXPECTED_CONTROL_COMMIT", commit);
        } else {
            command.env_remove("POINTBREAK_QUALIFICATION_EXPECTED_CONTROL_COMMIT");
        }
        command.output().map_err(|error| error.to_string())
    }

    fn exact_libtest_passed(stdout: &[u8], test_name: &str) -> Result<bool, String> {
        let stdout = std::str::from_utf8(stdout)
            .map_err(|error| format!("Change control output was not UTF-8: {error}"))?;
        Ok(stdout.contains("running 1 test")
            && stdout.contains(&format!("test {test_name} ... "))
            && stdout.contains("1 passed; 0 failed"))
    }

    #[derive(Clone)]
    struct InspectorEndpoint {
        address: String,
        token: String,
    }

    struct InspectorResponse {
        status: u16,
        headers: BTreeMap<String, String>,
        body: String,
    }

    impl InspectorEndpoint {
        fn request(&self, method: &str, target: &str) -> Result<(u16, String), String> {
            let response = self.request_with_headers(method, target, &[])?;
            Ok((response.status, response.body))
        }

        fn request_with_headers(
            &self,
            method: &str,
            target: &str,
            headers: &[(&str, &str)],
        ) -> Result<InspectorResponse, String> {
            let mut last_error = String::new();
            for attempt in 0..12 {
                match self.try_request(method, target, headers) {
                    Ok(response) => return Ok(response),
                    Err(error) => {
                        last_error = error;
                        thread::sleep(Duration::from_millis(20 * (attempt + 1)));
                    }
                }
            }
            Err(format!(
                "Inspector {method} {target} failed after retries: {last_error}"
            ))
        }

        /// One physical HTTP attempt for evidence operations whose execution
        /// count is itself part of the oracle.
        fn request_once_with_headers(
            &self,
            method: &str,
            target: &str,
            headers: &[(&str, &str)],
        ) -> Result<InspectorResponse, String> {
            self.try_request(method, target, headers)
        }

        fn try_request(
            &self,
            method: &str,
            target: &str,
            headers: &[(&str, &str)],
        ) -> Result<InspectorResponse, String> {
            let mut stream =
                TcpStream::connect(&self.address).map_err(|error| error.to_string())?;
            let mut request = format!(
                "{method} {target} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\n",
                self.address, self.token
            );
            for (name, value) in headers {
                request.push_str(name);
                request.push_str(": ");
                request.push_str(value);
                request.push_str("\r\n");
            }
            request.push_str("Connection: close\r\nContent-Length: 0\r\n\r\n");
            stream
                .write_all(request.as_bytes())
                .map_err(|error| error.to_string())?;
            let _ = stream.shutdown(Shutdown::Write);
            let mut response = Vec::new();
            stream
                .read_to_end(&mut response)
                .map_err(|error| error.to_string())?;
            let text = String::from_utf8(response).map_err(|error| error.to_string())?;
            let (head, body) = text
                .split_once("\r\n\r\n")
                .ok_or_else(|| "Inspector response omitted its header delimiter".to_owned())?;
            let status = head
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|value| value.parse::<u16>().ok())
                .ok_or_else(|| "Inspector response omitted its status code".to_owned())?;
            let headers = head
                .lines()
                .skip(1)
                .filter_map(|line| line.split_once(':'))
                .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
                .collect();
            Ok(InspectorResponse {
                status,
                headers,
                body: body.to_owned(),
            })
        }

        fn json(&self, target: &str) -> Result<(u16, Value), String> {
            let (status, body) = self.request("GET", target)?;
            let value = serde_json::from_str(&body).map_err(|error| {
                format!("Inspector {target} returned invalid JSON: {error}; body: {body}")
            })?;
            Ok((status, value))
        }
    }

    struct InspectorChild {
        child: Child,
        endpoint: InspectorEndpoint,
        stderr: Arc<Mutex<String>>,
        process_identity_sha256: String,
    }

    impl InspectorChild {
        fn spawn(
            request: &QualificationDerivedChangeReadRunRequestV1,
            profile: &str,
        ) -> Result<Self, String> {
            Self::spawn_with_timeline_barrier_root(request, profile, None)
        }

        fn spawn_with_timeline_barrier_root(
            request: &QualificationDerivedChangeReadRunRequestV1,
            profile: &str,
            barrier_root: Option<&Path>,
        ) -> Result<Self, String> {
            let mut command = Command::new(&request.product_binary);
            command
                .args([
                    "inspect",
                    "--repo",
                    request
                        .repository
                        .to_str()
                        .ok_or_else(|| "fixture repository path is not UTF-8".to_owned())?,
                    "--host",
                    "127.0.0.1",
                    "--port",
                    "0",
                    "--api-only",
                    "--format",
                    "json",
                ])
                .env("POINTBREAK_HOME", &request.pointbreak_home)
                .env("POINTBREAK_DERIVED_ACCESS", profile)
                .env_remove(LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_ROOT_ENV_V1)
                .env_remove("POINTBREAK_LOG")
                .env_remove("RUST_LOG")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if let Some(root) = barrier_root {
                command.env(LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_ROOT_ENV_V1, root);
            }
            let mut child = command
                .spawn()
                .map_err(|error| format!("spawn Inspector child: {error}"))?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| "Inspector child omitted stdout".to_owned())?;
            let mut reader = BufReader::new(stdout);
            let mut startup_line = String::new();
            reader
                .read_line(&mut startup_line)
                .map_err(|error| error.to_string())?;
            let startup: Value = serde_json::from_str(startup_line.trim())
                .map_err(|error| format!("invalid Inspector startup JSON: {error}"))?;
            let process_identity_sha256 = sha256_bytes_hex(
                &canonical_json_bytes(&json!([
                    child.id(),
                    profile,
                    request.product.binary_sha256,
                    request.product.source_commit,
                    &startup,
                ]))
                .map_err(|error| error.to_string())?,
            );
            let host = startup
                .get("host")
                .and_then(Value::as_str)
                .ok_or_else(|| "Inspector startup omitted host".to_owned())?;
            let port = startup
                .get("port")
                .and_then(Value::as_u64)
                .ok_or_else(|| "Inspector startup omitted port".to_owned())?;
            let token = startup
                .get("token")
                .and_then(Value::as_str)
                .ok_or_else(|| "Inspector startup omitted token".to_owned())?
                .to_owned();
            thread::spawn(move || {
                let mut sink = Vec::new();
                let _ = reader.read_to_end(&mut sink);
            });

            let stderr = Arc::new(Mutex::new(String::new()));
            let mut child_stderr = child
                .stderr
                .take()
                .ok_or_else(|| "Inspector child omitted stderr".to_owned())?;
            let stderr_sink = Arc::clone(&stderr);
            thread::spawn(move || {
                let mut buffer = String::new();
                let _ = child_stderr.read_to_string(&mut buffer);
                *stderr_sink.lock().unwrap_or_else(PoisonError::into_inner) = buffer;
            });

            Ok(Self {
                child,
                endpoint: InspectorEndpoint {
                    address: format!("{host}:{port}"),
                    token,
                },
                stderr,
                process_identity_sha256,
            })
        }

        fn ensure_ready(&self) -> Result<(), String> {
            if self.profile_is_ready()? {
                return Ok(());
            }
            let (status, body) = self.endpoint.request("POST", "/api/derived-access/retry")?;
            if status != 200 {
                return Err(format!(
                    "Inspector rebuild request returned {status}: {body}"
                ));
            }
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if self.profile_is_ready()? {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    let stderr = self
                        .stderr
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .clone();
                    return Err(format!(
                        "derived Change Inspector did not become ready; stderr: {stderr}"
                    ));
                }
                thread::sleep(Duration::from_millis(20));
            }
        }

        fn profile_is_ready(&self) -> Result<bool, String> {
            let (status, value) = self.endpoint.json("/api/v2/profile")?;
            Ok(status == 200 && value.get("availability").and_then(Value::as_str) == Some("ready"))
        }
    }

    impl Drop for InspectorChild {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    struct SemanticPair {
        strict_sha256: Option<String>,
        strict_http_status: Option<u16>,
        strict_code: Option<String>,
        derived_sha256: String,
        wire_contract_matches: bool,
        observed_http_status: u16,
        observed_code: Option<String>,
        typed_document: Option<QualificationDerivedChangeTypedDocumentV1>,
    }

    struct SemanticPairFailure {
        detail: String,
        typed_witness: Option<Box<DerivedChangeReadDiagnosticTypedWitnessV1>>,
    }

    #[derive(Clone)]
    struct TimelineAuthorityPoint {
        generation: String,
        checkpoint: String,
        stamp: String,
        trust: String,
    }

    struct TimelineOperationEvidence {
        receipt: LongitudinalCounterReceiptV1,
        strict_matches: bool,
        typed_document: Option<QualificationDerivedTimelineTypedObservationV1>,
        raw_tokens: Vec<String>,
        document: Value,
    }

    struct TimelineConcurrentTrustResult {
        operations: Vec<TimelineOperationEvidence>,
        transition: QualificationDerivedTimelineConcurrentTrustEvidenceV1,
    }

    #[derive(Clone)]
    struct TimelineSuccessAuthority {
        source_change_projection_stamp: String,
        timeline_projection_stamp: String,
        authority_cursor_sha256: String,
        authority_cursor: Value,
    }

    struct TimelineSignaturePhaseAuthority {
        process_identity_sha256: [String; 2],
        http_status: [u16; 2],
        authority: [Option<TimelineSuccessAuthority>; 2],
    }

    impl TimelineSignaturePhaseAuthority {
        fn success(&self, lane: usize) -> Result<&TimelineSuccessAuthority, String> {
            self.authority[lane]
                .as_ref()
                .ok_or_else(|| "successful Timeline phase omitted authority".to_owned())
        }
    }

    struct PendingTimelinePostAppend {
        before: TimelineAuthorityPoint,
        first: TimelineOperationEvidence,
        stale_token: String,
        strict_stale_token: Option<String>,
    }

    struct OptionalFileRestore {
        path: PathBuf,
        bytes: Option<Vec<u8>>,
        armed: bool,
    }

    struct TimelineTrustRestoreState {
        restore: OptionalFileRestore,
        expected_identity_sha256: String,
    }

    impl OptionalFileRestore {
        fn restore(mut self) -> Result<(), String> {
            restore_optional_file(&self.path, self.bytes.as_deref())?;
            if read_optional_file(&self.path)? != self.bytes {
                return Err(format!(
                    "restored file bytes drifted at {}",
                    self.path.display()
                ));
            }
            self.armed = false;
            Ok(())
        }
    }

    impl Drop for OptionalFileRestore {
        fn drop(&mut self) {
            if self.armed {
                let _ = restore_optional_file(&self.path, self.bytes.as_deref());
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_timeline_case(
        request: &QualificationDerivedChangeReadRunRequestV1,
        timeline_fault_root: Option<&QualificationDerivedTimelineFaultRootV1>,
        case: QualificationDerivedTimelineReadCaseV1,
        derived: &InspectorEndpoint,
        authoritative: Option<&InspectorEndpoint>,
        product_identity_sha256: &str,
        execution_identity_sha256: &str,
        fixture_inventory_sha256: &str,
        storage: Option<&QualificationDerivedChangeStorageEvidenceV1>,
    ) -> Result<QualificationDerivedTimelineReadEvidenceV1, String> {
        run_timeline_case_with_fault(
            request,
            case,
            derived,
            authoritative,
            product_identity_sha256,
            execution_identity_sha256,
            fixture_inventory_sha256,
            storage,
            timeline_fault_root,
            DerivedTimelineInvalidSignatureDiagnosticFaultV1::None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn run_timeline_case_with_fault(
        request: &QualificationDerivedChangeReadRunRequestV1,
        case: QualificationDerivedTimelineReadCaseV1,
        derived: &InspectorEndpoint,
        authoritative: Option<&InspectorEndpoint>,
        product_identity_sha256: &str,
        execution_identity_sha256: &str,
        fixture_inventory_sha256: &str,
        storage: Option<&QualificationDerivedChangeStorageEvidenceV1>,
        timeline_fault_root: Option<&QualificationDerivedTimelineFaultRootV1>,
        fault: DerivedTimelineInvalidSignatureDiagnosticFaultV1,
    ) -> Result<QualificationDerivedTimelineReadEvidenceV1, String> {
        let mut trust_restore = None;
        let mut result = run_timeline_case_inner(
            request,
            case,
            derived,
            authoritative,
            product_identity_sha256,
            execution_identity_sha256,
            fixture_inventory_sha256,
            storage,
            &mut trust_restore,
            timeline_fault_root,
            fault,
        );
        let Some(trust_restore) = trust_restore else {
            return result;
        };
        let restored_identity = trust_restore.restore.restore().and_then(|()| {
            let actual = timeline_trust_identity_sha256(request)?;
            if actual != trust_restore.expected_identity_sha256 {
                return Err("Timeline trust restoration identity drifted".to_owned());
            }
            Ok(actual)
        });
        match (&mut result, restored_identity) {
            (Ok(row), Ok(restored_identity)) => {
                if let Some(failure) = &mut row.invalid_signature_failure {
                    failure.reference_trust_identity_restored_sha256 = restored_identity;
                }
                result
            }
            (Err(_), Ok(_)) => result,
            (Ok(_), Err(restore)) => Err(format!(
                "explicit Timeline trust restoration failed: {restore}"
            )),
            (Err(primary), Err(restore)) => Err(format!(
                "{primary}; explicit Timeline trust restoration also failed: {restore}"
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_timeline_case_inner(
        request: &QualificationDerivedChangeReadRunRequestV1,
        case: QualificationDerivedTimelineReadCaseV1,
        derived: &InspectorEndpoint,
        authoritative: Option<&InspectorEndpoint>,
        product_identity_sha256: &str,
        execution_identity_sha256: &str,
        fixture_inventory_sha256: &str,
        storage: Option<&QualificationDerivedChangeStorageEvidenceV1>,
        trust_restore: &mut Option<TimelineTrustRestoreState>,
        timeline_fault_root: Option<&QualificationDerivedTimelineFaultRootV1>,
        fault: DerivedTimelineInvalidSignatureDiagnosticFaultV1,
    ) -> Result<QualificationDerivedTimelineReadEvidenceV1, String> {
        let before = timeline_authority_point(request, derived, storage)?;
        let schedule = timeline_request_schedule_v1(request.fixture, case);
        let mut operations = Vec::with_capacity(schedule.len());
        let mut trust_transition = None;
        let mut trust_witness = None;
        let mut derived_trust_token = None;
        let mut strict_trust_token = None;
        let mut invalid_signature_failure = None;
        let mut concurrent_trust_transition = None;

        let mut index = 0;
        while index < schedule.len() {
            let operation = schedule[index];
            if operation == "timeline_concurrent_asc" {
                if schedule.get(index + 1) != Some(&"timeline_concurrent_desc") {
                    return Err("Timeline concurrent schedule lost its paired request".to_owned());
                }
                let concurrent = run_timeline_concurrent_trust(
                    request,
                    case,
                    derived,
                    authoritative.ok_or_else(|| {
                        "Timeline concurrent trust probe omitted its strict child".to_owned()
                    })?,
                    index,
                    product_identity_sha256,
                    execution_identity_sha256,
                    fixture_inventory_sha256,
                )?;
                operations.extend(concurrent.operations);
                concurrent_trust_transition = Some(concurrent.transition);
                index += 2;
                continue;
            }
            if operation == "timeline_trust_after" {
                let identity_before = timeline_trust_identity_sha256(request)?;
                let (backup, transition) = stage_timeline_trust(request, derived)?;
                *trust_restore = Some(TimelineTrustRestoreState {
                    restore: backup,
                    expected_identity_sha256: identity_before.clone(),
                });
                let identity_staged = match fault {
                    DerivedTimelineInvalidSignatureDiagnosticFaultV1::AfterTrustStageIdentityRead => {
                        return Err("injected post-stage identity read failure".to_owned());
                    }
                    DerivedTimelineInvalidSignatureDiagnosticFaultV1::TrustStageReportsUnchangedIdentity => {
                        identity_before.clone()
                    }
                    _ => timeline_trust_identity_sha256(request)?,
                };
                if identity_before == identity_staged {
                    return Err("Timeline trust stage did not change authority identity".to_owned());
                }
                trust_witness = Some(transition);
            }
            let fresh_process = case
                == QualificationDerivedTimelineReadCaseV1::ProcessLifecycleSuite
                && matches!(operation, "timeline_cold" | "timeline_restart");
            let fresh_derived = fresh_process
                .then(|| InspectorChild::spawn(request, "sqlite-wal-bodyless-v1"))
                .transpose()?;
            if let Some(child) = &fresh_derived {
                child.ensure_ready()?;
            }
            let fresh_strict = (fresh_process && authoritative.is_some())
                .then(|| InspectorChild::spawn(request, "off"))
                .transpose()?;
            let derived_endpoint = fresh_derived
                .as_ref()
                .map_or(derived, |child| &child.endpoint);
            let authoritative_endpoint = fresh_strict
                .as_ref()
                .map(|child| &child.endpoint)
                .or(authoritative);
            let derived_target = timeline_operation_target(
                operation,
                derived_endpoint,
                derived_trust_token.as_deref(),
            )?;
            let strict_target = authoritative_endpoint
                .map(|endpoint| {
                    timeline_operation_target(operation, endpoint, strict_trust_token.as_deref())
                })
                .transpose()?;
            let evidence = run_timeline_operation(
                request,
                case,
                operation,
                index,
                derived_endpoint,
                authoritative_endpoint,
                &derived_target,
                strict_target.as_deref(),
                product_identity_sha256,
                execution_identity_sha256,
                fixture_inventory_sha256,
            )?;
            if operation == "timeline_trust_before" {
                derived_trust_token = evidence
                    .document
                    .get("next")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                strict_trust_token = authoritative
                    .map(|endpoint| endpoint.json("/api/v2/history?limit=2&order=asc"))
                    .transpose()?
                    .and_then(|(_, document)| {
                        document
                            .get("next")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    });
            }
            operations.push(evidence);
            index += 1;
        }
        if request.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
            && case == QualificationDerivedTimelineReadCaseV1::TrustSuite
        {
            invalid_signature_failure = Some(capture_timeline_invalid_signature_failure(
                request,
                product_identity_sha256,
                execution_identity_sha256,
                fixture_inventory_sha256,
                trust_witness.as_ref().ok_or_else(|| {
                    "Timeline invalid-signature derivative omitted its trust witness".to_owned()
                })?,
                &before,
                timeline_fault_root.ok_or_else(|| {
                    "Timeline TrustSuite omitted its independent fault authority".to_owned()
                })?,
                fault,
            )?);
        }
        let after = timeline_authority_point(request, derived, storage)?;
        if trust_restore.is_some() {
            trust_transition = trust_witness;
        }
        finish_timeline_row(
            request,
            case,
            before,
            after,
            operations,
            product_identity_sha256,
            execution_identity_sha256,
            fixture_inventory_sha256,
            trust_transition,
            concurrent_trust_transition,
            invalid_signature_failure,
        )
    }

    fn begin_timeline_post_append(
        request: &QualificationDerivedChangeReadRunRequestV1,
        derived: &InspectorEndpoint,
        authoritative: Option<&InspectorEndpoint>,
        product_identity_sha256: &str,
        execution_identity_sha256: &str,
        fixture_inventory_sha256: &str,
        storage: Option<&QualificationDerivedChangeStorageEvidenceV1>,
    ) -> Result<PendingTimelinePostAppend, String> {
        let case = QualificationDerivedTimelineReadCaseV1::PostAppendSuite;
        let before = timeline_authority_point(request, derived, storage)?;
        let derived_target = "/api/v2/history?limit=2&order=asc";
        let strict_target = derived_target;
        let first = run_timeline_operation(
            request,
            case,
            "timeline_k",
            0,
            derived,
            authoritative,
            derived_target,
            authoritative.map(|_| strict_target),
            product_identity_sha256,
            execution_identity_sha256,
            fixture_inventory_sha256,
        )?;
        let stale_token = first
            .document
            .get("next")
            .and_then(Value::as_str)
            .ok_or_else(|| "Timeline K omitted a continuation token".to_owned())?
            .to_owned();
        let strict_stale_token = authoritative
            .map(|endpoint| endpoint.json("/api/v2/history?limit=2&order=asc"))
            .transpose()?
            .and_then(|(_, document)| {
                document
                    .get("next")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        Ok(PendingTimelinePostAppend {
            before,
            first,
            stale_token,
            strict_stale_token,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_timeline_post_append(
        request: &QualificationDerivedChangeReadRunRequestV1,
        derived: &InspectorEndpoint,
        authoritative: Option<&InspectorEndpoint>,
        product_identity_sha256: &str,
        execution_identity_sha256: &str,
        fixture_inventory_sha256: &str,
        storage: Option<&QualificationDerivedChangeStorageEvidenceV1>,
        pending: PendingTimelinePostAppend,
    ) -> Result<QualificationDerivedTimelineReadEvidenceV1, String> {
        let case = QualificationDerivedTimelineReadCaseV1::PostAppendSuite;
        let mut operations = vec![pending.first];
        for (index, operation) in timeline_request_schedule_v1(request.fixture, case)
            .iter()
            .enumerate()
            .skip(1)
        {
            let derived_target = if *operation == "timeline_k_stale_token" {
                format!(
                    "/api/v2/history?limit=2&order=asc&after={}",
                    percent_encode(&pending.stale_token)
                )
            } else {
                "/api/v2/history?limit=2&order=asc".to_owned()
            };
            let strict_target = if authoritative.is_some() {
                if *operation == "timeline_k_stale_token" {
                    pending.strict_stale_token.as_ref().map(|token| {
                        format!(
                            "/api/v2/history?limit=2&order=asc&after={}",
                            percent_encode(token)
                        )
                    })
                } else {
                    Some("/api/v2/history?limit=2&order=asc".to_owned())
                }
            } else {
                None
            };
            let fresh_process = *operation == "timeline_k_plus_one_fresh_process";
            let fresh_derived = fresh_process
                .then(|| InspectorChild::spawn(request, "sqlite-wal-bodyless-v1"))
                .transpose()?;
            if let Some(child) = &fresh_derived {
                child.ensure_ready()?;
            }
            let fresh_strict = (fresh_process && authoritative.is_some())
                .then(|| InspectorChild::spawn(request, "off"))
                .transpose()?;
            let derived_endpoint = fresh_derived
                .as_ref()
                .map_or(derived, |child| &child.endpoint);
            let authoritative_endpoint = fresh_strict
                .as_ref()
                .map(|child| &child.endpoint)
                .or(authoritative);
            operations.push(run_timeline_operation(
                request,
                case,
                operation,
                index,
                derived_endpoint,
                authoritative_endpoint,
                &derived_target,
                strict_target.as_deref(),
                product_identity_sha256,
                execution_identity_sha256,
                fixture_inventory_sha256,
            )?);
        }
        let post_storage = if let Some(probes) = request.storage_forbidden_probes.as_ref() {
            let store_root =
                store_dir_for_repo(&request.repository).map_err(|error| error.to_string())?;
            let mut fresh = storage
                .cloned()
                .ok_or_else(|| "post-append Timeline omitted storage authority".to_owned())?;
            fresh.witness =
                capture_qualification_derived_storage_witness_v1(&store_root, probes)
                    .map_err(|error| format!("timeline post-append storage witness: {error}"))?;
            Some(fresh)
        } else {
            None
        };
        let after = timeline_authority_point(request, derived, post_storage.as_ref())?;
        finish_timeline_row(
            request,
            case,
            pending.before,
            after,
            operations,
            product_identity_sha256,
            execution_identity_sha256,
            fixture_inventory_sha256,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn run_timeline_operation(
        request: &QualificationDerivedChangeReadRunRequestV1,
        case: QualificationDerivedTimelineReadCaseV1,
        operation: &str,
        index: usize,
        derived: &InspectorEndpoint,
        authoritative: Option<&InspectorEndpoint>,
        derived_target: &str,
        strict_target: Option<&str>,
        product_identity_sha256: &str,
        execution_identity_sha256: &str,
        fixture_inventory_sha256: &str,
    ) -> Result<TimelineOperationEvidence, String> {
        let (derived_status, derived_document) = derived.json(derived_target)?;
        let normalized = normalize_timeline_semantic_checked(derived_document.clone())?;
        let semantic_result_sha256 = canonical_json_bytes(&normalized)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())?;
        let schedule_sha256 = timeline_request_schedule_sha256_v1(request.fixture, case);
        let run_identity = sha256_bytes_hex(
            &canonical_json_bytes(&json!({
                "root": request.execution.root_provenance_sha256,
                "product": product_identity_sha256,
                "fixture": request.fixture,
                "case": case,
                "operation": operation,
                "index": index,
                "semantic": semantic_result_sha256,
            }))
            .map_err(|error| error.to_string())?,
        );
        let counting = json!({
            "runIdentity": run_identity,
            "context": LongitudinalCounterReceiptContextV1 {
                root_identity: request.execution.root_provenance_sha256.clone(),
                operation: operation.to_owned(),
                phase: case.as_str().to_owned(),
                base_execution_identity_sha256: execution_identity_sha256.to_owned(),
                derivative_execution_identity_sha256: product_identity_sha256.to_owned(),
                manifest_sha256: fixture_inventory_sha256.to_owned(),
                schedule_sha256,
                success: !timeline_operation_is_expected_failure(operation)
                    || (operation == "timeline_fault_outcome"
                        && qualification_derived_change_expected_outcome_v1(
                            request.execution.platform,
                            request.fixture,
                            QualificationDerivedChangeReadCaseV1::ChangesBare,
                        )
                        .0
                            != QualificationDerivedChangeReadOracleV1::TypedFailure),
                semantic_result_sha256: semantic_result_sha256.clone(),
                include_capacity_ownership: false,
            }
        });
        let encoded = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&counting).map_err(|error| error.to_string())?);
        let counted = derived.request_with_headers(
            "GET",
            derived_target,
            &[("X-Pointbreak-Longitudinal-Counting", &encoded)],
        )?;
        let counted_document: Value = serde_json::from_str(&counted.body)
            .map_err(|error| format!("counted Timeline response was invalid: {error}"))?;
        if counted.status != derived_status
            || normalize_timeline_semantic_checked(counted_document)? != normalized
        {
            return Err(format!("counted Timeline operation {operation} drifted"));
        }
        let encoded_receipt = counted
            .headers
            .get("x-pointbreak-longitudinal-receipt")
            .ok_or_else(|| format!("Timeline operation {operation} omitted counter receipt"))?;
        let receipt: LongitudinalCounterReceiptV1 = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(encoded_receipt)
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        receipt.validate().map_err(|error| error.to_string())?;

        let expected_typed =
            expected_timeline_typed_documents_v1(request.execution.platform, request.fixture, case)
                .into_iter()
                .find(|expected| expected.operation == operation);
        let observed_typed = (derived_status != 200)
            .then(|| timeline_typed_document(&derived_document))
            .transpose()?;
        let typed_contract_matches = match (&expected_typed, &observed_typed) {
            (Some(expected), Some(observed)) => {
                derived_status == expected.http_status
                    && observed.schema == expected.schema
                    && observed.version == expected.version
                    && observed.code == expected.code
                    && observed.retryable == expected.retryable
            }
            (None, None) => derived_status == 200,
            _ => false,
        };
        let strict_matches = match (authoritative, strict_target) {
            (Some(authoritative), Some(target)) => {
                let (status, value) = authoritative.json(target)?;
                status == derived_status
                    && normalize_timeline_semantic_checked(value)? == normalized
            }
            (None, None) => true,
            _ if timeline_operation_is_expected_failure(operation) => true,
            _ => false,
        } && typed_contract_matches
            && timeline_operation_semantics_match(operation, derived_status, &derived_document);
        let typed_document =
            observed_typed.map(|document| QualificationDerivedTimelineTypedObservationV1 {
                operation: operation.to_owned(),
                http_status: derived_status,
                document,
            });
        let raw_tokens = ["previous", "next"]
            .into_iter()
            .filter_map(|name| derived_document.get(name).and_then(Value::as_str))
            .map(|token| sha256_bytes_hex(token.as_bytes()))
            .collect();
        Ok(TimelineOperationEvidence {
            receipt,
            strict_matches,
            typed_document,
            raw_tokens,
            document: derived_document,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn run_timeline_concurrent_trust(
        request: &QualificationDerivedChangeReadRunRequestV1,
        case: QualificationDerivedTimelineReadCaseV1,
        derived: &InspectorEndpoint,
        authoritative: &InspectorEndpoint,
        first_index: usize,
        product_identity_sha256: &str,
        execution_identity_sha256: &str,
        fixture_inventory_sha256: &str,
    ) -> Result<TimelineConcurrentTrustResult, String> {
        if case != QualificationDerivedTimelineReadCaseV1::ProcessLifecycleSuite {
            return Err("Timeline concurrent trust probe escaped its lifecycle suite".to_owned());
        }
        let witness: Value = read_json(&request.fixture_witness)?;
        let signed_event_id = witness
            .pointer("/timeline/trust/signedEvent")
            .and_then(Value::as_str)
            .ok_or_else(|| "Timeline concurrent trust witness omitted signedEvent".to_owned())?
            .to_owned();
        let target_for = |order: &str| {
            format!(
                "/api/v2/history?limit=1&order={order}&at={}",
                percent_encode(&signed_event_id)
            )
        };
        let targets = [target_for("asc"), target_for("desc")];
        let operations = ["timeline_concurrent_asc", "timeline_concurrent_desc"];
        let trust_path = allowed_signers_path_for_repo(&request.repository)
            .map_err(|error| error.to_string())?;
        let trust_backup = std::fs::read(&trust_path).ok();
        let trust_identity_before_sha256 = timeline_trust_identity_sha256(request)?;

        let mut before_documents = Vec::with_capacity(2);
        for target in &targets {
            let (derived_status, derived_document) = derived.json(target)?;
            let (strict_status, strict_document) = authoritative.json(target)?;
            if derived_status != 200
                || strict_status != 200
                || normalize_timeline_semantic_checked(derived_document.clone())?
                    != normalize_timeline_semantic_checked(strict_document)?
            {
                return Err("Timeline concurrent trust pre-state lost strict parity".to_owned());
            }
            require_timeline_event_status(
                &derived_document,
                &signed_event_id,
                EventVerificationStatus::UntrustedKey,
            )?;
            before_documents.push(derived_document);
        }
        let signed_entry = before_documents[0].pointer("/entries/0").ok_or_else(|| {
            "Timeline concurrent trust response omitted its signed entry".to_owned()
        })?;
        let signer_identity = signed_entry
            .get("signer")
            .and_then(Value::as_str)
            .ok_or_else(|| "Timeline concurrent trust response omitted signer".to_owned())?
            .to_owned();
        let actor = signed_entry
            .pointer("/writer/actorId")
            .and_then(Value::as_str)
            .ok_or_else(|| "Timeline concurrent trust response omitted writer actor".to_owned())?;
        let trust_bytes = timeline_allowed_signers_bytes(actor, &signer_identity)?;

        let preflight_restore = OptionalFileRestore {
            path: trust_path.clone(),
            bytes: trust_backup.clone(),
            armed: true,
        };
        write_qualification_file_atomically(&trust_path, &trust_bytes)?;
        let trust_identity_during_sha256 = timeline_trust_identity_sha256(request)?;
        let mut during_documents = Vec::with_capacity(2);
        for target in &targets {
            let (derived_status, derived_document) = derived.json(target)?;
            let (strict_status, strict_document) = authoritative.json(target)?;
            if derived_status != 200
                || strict_status != 200
                || normalize_timeline_semantic_checked(derived_document.clone())?
                    != normalize_timeline_semantic_checked(strict_document)?
            {
                return Err("Timeline concurrent trust post-state lost strict parity".to_owned());
            }
            require_timeline_event_status(
                &derived_document,
                &signed_event_id,
                EventVerificationStatus::Valid,
            )?;
            during_documents.push(derived_document);
        }
        preflight_restore.restore()?;
        if timeline_trust_identity_sha256(request)? != trust_identity_before_sha256 {
            return Err("Timeline concurrent trust preflight did not restore authority".to_owned());
        }

        let allowed_semantics = before_documents
            .iter()
            .zip(&during_documents)
            .map(|(before, during)| {
                let hashes = [before, during]
                    .into_iter()
                    .map(|document| {
                        canonical_json_bytes(&normalize_timeline_semantic_checked(
                            document.clone(),
                        )?)
                        .map(|bytes| sha256_bytes_hex(&bytes))
                        .map_err(|error| error.to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                canonical_json_bytes(
                    &serde_json::to_value(hashes).map_err(|error| error.to_string())?,
                )
                .map(|bytes| sha256_bytes_hex(&bytes))
                .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;

        let race_restore = OptionalFileRestore {
            path: trust_path.clone(),
            bytes: trust_backup,
            armed: true,
        };
        let barrier = Arc::new(Barrier::new(3));
        let mut raced = thread::scope(|scope| {
            let left_barrier = Arc::clone(&barrier);
            let right_barrier = Arc::clone(&barrier);
            let left_target = &targets[0];
            let right_target = &targets[1];
            let left_semantics = &allowed_semantics[0];
            let right_semantics = &allowed_semantics[1];
            let left = scope.spawn(move || {
                left_barrier.wait();
                run_counted_timeline_concurrent_request(
                    request,
                    case,
                    operations[0],
                    first_index,
                    derived,
                    left_target,
                    left_semantics,
                    product_identity_sha256,
                    execution_identity_sha256,
                    fixture_inventory_sha256,
                )
            });
            let right = scope.spawn(move || {
                right_barrier.wait();
                run_counted_timeline_concurrent_request(
                    request,
                    case,
                    operations[1],
                    first_index + 1,
                    derived,
                    right_target,
                    right_semantics,
                    product_identity_sha256,
                    execution_identity_sha256,
                    fixture_inventory_sha256,
                )
            });
            barrier.wait();
            write_qualification_file_atomically(&trust_path, &trust_bytes)?;
            let left = left
                .join()
                .map_err(|_| "Timeline concurrent asc reader panicked".to_owned())??;
            let right = right
                .join()
                .map_err(|_| "Timeline concurrent desc reader panicked".to_owned())??;
            Ok::<_, String>(vec![left, right])
        })?;
        if timeline_trust_identity_sha256(request)? != trust_identity_during_sha256 {
            return Err(
                "Timeline concurrent trust mutation did not become authoritative".to_owned(),
            );
        }

        let mut observed_status_by_operation = BTreeMap::new();
        for (index, evidence) in raced.iter_mut().enumerate() {
            let normalized = normalize_timeline_semantic_checked(evidence.document.clone())?;
            let before = normalize_timeline_semantic_checked(before_documents[index].clone())?;
            let during = normalize_timeline_semantic_checked(during_documents[index].clone())?;
            if evidence.receipt.semantic_result_sha256 != allowed_semantics[index]
                || evidence
                    .document
                    .get("entries")
                    .and_then(Value::as_array)
                    .is_none()
                || normalized != before && normalized != during
            {
                return Err("Timeline concurrent response mixed trust snapshots".to_owned());
            }
            let status = evidence
                .document
                .pointer("/entries/0/verificationStatus")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    "Timeline concurrent response omitted verification status".to_owned()
                })?;
            if !matches!(status, "untrusted_key" | "valid") {
                return Err(
                    "Timeline concurrent response returned an impossible trust state".to_owned(),
                );
            }
            observed_status_by_operation.insert(operations[index].to_owned(), status.to_owned());
            evidence.strict_matches = true;
        }

        race_restore.restore()?;
        let trust_identity_restored_sha256 = timeline_trust_identity_sha256(request)?;
        let (restored_status, restored_document) = derived.json(&targets[0])?;
        let (strict_restored_status, strict_restored_document) = authoritative.json(&targets[0])?;
        if restored_status != 200
            || strict_restored_status != 200
            || normalize_timeline_semantic_checked(restored_document.clone())?
                != normalize_timeline_semantic_checked(strict_restored_document)?
        {
            return Err("Timeline concurrent trust recovery lost strict parity".to_owned());
        }
        require_timeline_event_status(
            &restored_document,
            &signed_event_id,
            EventVerificationStatus::UntrustedKey,
        )?;

        Ok(TimelineConcurrentTrustResult {
            operations: raced,
            transition: QualificationDerivedTimelineConcurrentTrustEvidenceV1 {
                signed_event_id,
                signer_identity,
                trust_identity_before_sha256,
                trust_identity_during_sha256,
                trust_identity_restored_sha256,
                status_before: EventVerificationStatus::UntrustedKey.as_str().to_owned(),
                status_during: EventVerificationStatus::Valid.as_str().to_owned(),
                status_restored: EventVerificationStatus::UntrustedKey.as_str().to_owned(),
                observed_status_by_operation,
            },
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn run_counted_timeline_concurrent_request(
        request: &QualificationDerivedChangeReadRunRequestV1,
        case: QualificationDerivedTimelineReadCaseV1,
        operation: &str,
        index: usize,
        derived: &InspectorEndpoint,
        target: &str,
        semantic_result_sha256: &str,
        product_identity_sha256: &str,
        execution_identity_sha256: &str,
        fixture_inventory_sha256: &str,
    ) -> Result<TimelineOperationEvidence, String> {
        let run_identity = sha256_bytes_hex(
            &canonical_json_bytes(&json!({
                "root": request.execution.root_provenance_sha256,
                "product": product_identity_sha256,
                "fixture": request.fixture,
                "case": case,
                "operation": operation,
                "index": index,
                "semantic": semantic_result_sha256,
            }))
            .map_err(|error| error.to_string())?,
        );
        let counting = json!({
            "runIdentity": run_identity,
            "context": LongitudinalCounterReceiptContextV1 {
                root_identity: request.execution.root_provenance_sha256.clone(),
                operation: operation.to_owned(),
                phase: case.as_str().to_owned(),
                base_execution_identity_sha256: execution_identity_sha256.to_owned(),
                derivative_execution_identity_sha256: product_identity_sha256.to_owned(),
                manifest_sha256: fixture_inventory_sha256.to_owned(),
                schedule_sha256: timeline_request_schedule_sha256_v1(request.fixture, case),
                success: true,
                semantic_result_sha256: semantic_result_sha256.to_owned(),
                include_capacity_ownership: false,
            }
        });
        let encoded = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&counting).map_err(|error| error.to_string())?);
        let response = derived.request_with_headers(
            "GET",
            target,
            &[("X-Pointbreak-Longitudinal-Counting", &encoded)],
        )?;
        let document: Value = serde_json::from_str(&response.body)
            .map_err(|error| format!("concurrent Timeline response was invalid: {error}"))?;
        if response.status != 200 {
            return Err(format!(
                "concurrent Timeline operation {operation} returned {}",
                response.status
            ));
        }
        let encoded_receipt = response
            .headers
            .get("x-pointbreak-longitudinal-receipt")
            .ok_or_else(|| format!("Timeline operation {operation} omitted counter receipt"))?;
        let receipt: LongitudinalCounterReceiptV1 = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(encoded_receipt)
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        receipt.validate().map_err(|error| error.to_string())?;
        let raw_tokens = ["previous", "next"]
            .into_iter()
            .filter_map(|name| document.get(name).and_then(Value::as_str))
            .map(|token| sha256_bytes_hex(token.as_bytes()))
            .collect();
        Ok(TimelineOperationEvidence {
            receipt,
            strict_matches: false,
            typed_document: None,
            raw_tokens,
            document,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn capture_timeline_invalid_signature_failure(
        request: &QualificationDerivedChangeReadRunRequestV1,
        product_identity_sha256: &str,
        execution_identity_sha256: &str,
        fixture_inventory_sha256: &str,
        trust_transition: &QualificationDerivedTimelineTrustTransitionV1,
        authority_before: &TimelineAuthorityPoint,
        timeline_fault_root: &QualificationDerivedTimelineFaultRootV1,
        fault: DerivedTimelineInvalidSignatureDiagnosticFaultV1,
    ) -> Result<QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1, String> {
        let reference_inventory =
            longitudinal_authoritative_store_data_inventory_v1(&request.repository)
                .map_err(|error| error.to_string())?;
        let fault_inventory =
            longitudinal_authoritative_store_data_inventory_v1(&timeline_fault_root.repository)
                .map_err(|error| error.to_string())?;
        if reference_inventory.inventory_sha256 != fixture_inventory_sha256
            || fault_inventory.inventory_sha256 != fixture_inventory_sha256
        {
            return Err(
                "Timeline invalid-signature roots started from unequal authority".to_owned(),
            );
        }
        let mut carrier_restore = None;
        let mut fault_trust_restore = None;
        let mut result = capture_timeline_invalid_signature_failure_inner(
            request,
            product_identity_sha256,
            execution_identity_sha256,
            fixture_inventory_sha256,
            trust_transition,
            authority_before,
            timeline_fault_root,
            &mut carrier_restore,
            &mut fault_trust_restore,
            fault,
        );
        let restoration = (|| {
            if let Some(restore) = carrier_restore {
                restore.restore()?;
            }
            let restored_inventory =
                longitudinal_authoritative_store_data_inventory_v1(&timeline_fault_root.repository)
                    .map_err(|error| error.to_string())?;
            if restored_inventory.inventory_sha256 != fixture_inventory_sha256 {
                return Err("Timeline signature fault-root carrier restoration drifted".to_owned());
            }
            let fault_request = request_for_timeline_fault_root(request, timeline_fault_root)?;
            let restored_events = read_events_for_display(&fault_request.repository)
                .map_err(|error| error.to_string())?
                .0;
            let restored_event = restored_events
                .iter()
                .find(|event| event.event_id.as_str() == trust_transition.signed_event_id)
                .ok_or_else(|| "restored Timeline fault root omitted its carrier".to_owned())?;
            let staged_trust = TrustSet::from_allowed_signers_file(
                &allowed_signers_path_for_repo(&fault_request.repository)
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            let recovery_signature_status = verify_event_signature(restored_event, &staged_trust)
                .map_err(|error| error.to_string())?;
            if recovery_signature_status != EventVerificationStatus::Valid {
                return Err(
                    "restored Timeline fault carrier did not recover valid trust".to_owned(),
                );
            }
            if let Some(restore) = fault_trust_restore {
                restore.restore()?;
            }
            let restored_trust = timeline_trust_identity_sha256(&fault_request)?;
            if let Ok(evidence) = &mut result {
                evidence.fault_restored_inventory_sha256 = restored_inventory.inventory_sha256;
                evidence.fault_trust_identity_restored_sha256 = restored_trust;
                evidence.recovery_signature_status = recovery_signature_status.as_str().to_owned();
            }
            Ok::<(), String>(())
        })();
        match (result, restoration) {
            (result, Ok(())) => result,
            (Ok(_), Err(restore)) => Err(format!(
                "explicit Timeline fault-authority restoration failed: {restore}"
            )),
            (Err(primary), Err(restore)) => Err(format!(
                "{primary}; explicit Timeline fault-authority restoration also failed: {restore}"
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn capture_timeline_invalid_signature_failure_inner(
        request: &QualificationDerivedChangeReadRunRequestV1,
        product_identity_sha256: &str,
        _execution_identity_sha256: &str,
        fixture_inventory_sha256: &str,
        trust_transition: &QualificationDerivedTimelineTrustTransitionV1,
        authority_before: &TimelineAuthorityPoint,
        timeline_fault_root: &QualificationDerivedTimelineFaultRootV1,
        carrier_restore: &mut Option<OptionalFileRestore>,
        fault_trust_restore: &mut Option<OptionalFileRestore>,
        fault: DerivedTimelineInvalidSignatureDiagnosticFaultV1,
    ) -> Result<QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1, String> {
        let fault_request = request_for_timeline_fault_root(request, timeline_fault_root)?;
        let reference_trust_identity_staged_sha256 = timeline_trust_identity_sha256(request)?;
        if reference_trust_identity_staged_sha256 == authority_before.trust {
            return Err("Timeline invalid-signature reference trust was not staged".to_owned());
        }

        // Prepare the second publication from its own authoritative root. No
        // generation or sidecar bytes are copied from the reference root.
        let fault_preparation = InspectorChild::spawn(&fault_request, "sqlite-wal-bodyless-v1")?;
        fault_preparation.ensure_ready()?;
        drop(fault_preparation);

        let reference_trust_path = allowed_signers_path_for_repo(&request.repository)
            .map_err(|error| error.to_string())?;
        let staged_trust_bytes = std::fs::read(&reference_trust_path)
            .map_err(|error| format!("read staged reference trust: {error}"))?;
        let fault_trust_path = allowed_signers_path_for_repo(&fault_request.repository)
            .map_err(|error| error.to_string())?;
        *fault_trust_restore = Some(OptionalFileRestore {
            path: fault_trust_path.clone(),
            bytes: read_optional_file(&fault_trust_path)?,
            armed: true,
        });
        write_qualification_file_atomically(&fault_trust_path, &staged_trust_bytes)?;
        let fault_trust_identity_staged_sha256 = timeline_trust_identity_sha256(&fault_request)?;
        if fault_trust_identity_staged_sha256 != reference_trust_identity_staged_sha256 {
            return Err("Timeline fault-root trust authority differs from reference".to_owned());
        }

        let carrier_event_id = trust_transition.signed_event_id.clone();
        let (events, _) = read_events_for_display(&fault_request.repository)
            .map_err(|error| error.to_string())?;
        let selected = events
            .iter()
            .find(|event| event.event_id.as_str() == carrier_event_id)
            .ok_or_else(|| {
                "Timeline invalid-signature derivative omitted its signed carrier".to_owned()
            })?;
        let store_root =
            store_dir_for_repo(&fault_request.repository).map_err(|error| error.to_string())?;
        let store = EventStore::open(&store_root);
        let carrier_path = store.event_path_for_idempotency_key(&selected.idempotency_key);
        let clean_bytes = std::fs::read(&carrier_path).map_err(|error| error.to_string())?;
        let clean_event: ShoreEvent =
            serde_json::from_slice(&clean_bytes).map_err(|error| error.to_string())?;
        if clean_event.event_id.as_str() != carrier_event_id {
            return Err("Timeline invalid-signature carrier identity drifted".to_owned());
        }

        let trust_path = allowed_signers_path_for_repo(&fault_request.repository)
            .map_err(|error| error.to_string())?;
        let trust =
            TrustSet::from_allowed_signers_file(&trust_path).map_err(|error| error.to_string())?;
        let clean_signature_status =
            verify_event_signature(&clean_event, &trust).map_err(|error| error.to_string())?;
        if clean_signature_status != EventVerificationStatus::Valid {
            return Err("Timeline signature derivative did not start valid".to_owned());
        }

        let target = format!(
            "/api/v2/history?limit=1&order=asc&at={}",
            percent_encode(&carrier_event_id)
        );
        let (clean_phase, clean_semantic_sha256, strict_clean_semantic_sha256, clean_cursor_sha256) =
            timeline_signature_success_phase(request, &target, &carrier_event_id, "clean")?;

        let clean_signature = clean_event
            .signature
            .as_ref()
            .ok_or_else(|| "Timeline signature derivative selected an unsigned event".to_owned())?
            .sig
            .as_str();
        let clean_signature_bytes = BASE64_STANDARD
            .decode(clean_signature.as_bytes())
            .map_err(|error| error.to_string())?;
        if clean_signature_bytes.len() != 64 {
            return Err("Timeline signature derivative was not Ed25519-sized".to_owned());
        }
        let mut mutated_signature_bytes = clean_signature_bytes.clone();
        mutated_signature_bytes[0] ^= 1;
        let invalid_signature = EventSignatureBytes::from_bytes(&mutated_signature_bytes);
        let mutated_bytes = replace_unique_bytes(
            &clean_bytes,
            clean_signature.as_bytes(),
            invalid_signature.as_str().as_bytes(),
        )?;
        let mutated_event: ShoreEvent =
            serde_json::from_slice(&mutated_bytes).map_err(|error| error.to_string())?;
        let mut expected_mutated_event = clean_event.clone();
        expected_mutated_event
            .signature
            .as_mut()
            .expect("the signed carrier was checked above")
            .sig = invalid_signature;
        let clean_event_record_hash = clean_event
            .event_record_hash()
            .map_err(|error| error.to_string())?;
        let mutated_event_record_hash = mutated_event
            .event_record_hash()
            .map_err(|error| error.to_string())?;
        if mutated_event != expected_mutated_event
            || clean_signature_bytes
                .iter()
                .zip(&mutated_signature_bytes)
                .map(|(clean, mutated)| (clean ^ mutated).count_ones())
                .sum::<u32>()
                != 1
            || clean_event_record_hash != mutated_event_record_hash
        {
            return Err(
                "Timeline invalid-signature recipe changed record identity or non-signature bytes"
                    .to_owned(),
            );
        }
        let mutated_signature_status =
            verify_event_signature(&mutated_event, &trust).map_err(|error| error.to_string())?;
        if mutated_signature_status != EventVerificationStatus::Invalid {
            return Err("Timeline signature derivative did not become invalid".to_owned());
        }
        let mutation_recipe_sha256 = canonical_json_bytes(&json!({
            "schema": "pointbreak.timeline-invalid-inline-signature-mutation-recipe.v1",
            "target": "topology-valid-inline-signature-carrier",
            "mutation": "flip-one-bit-in-inline-signature",
            "byteIndex": 0,
            "bitMask": 1,
        }))
        .map(|bytes| sha256_bytes_hex(&bytes))
        .map_err(|error| error.to_string())?;
        if mutation_recipe_sha256
            != QUALIFICATION_TIMELINE_INVALID_SIGNATURE_MUTATION_RECIPE_SHA256_V1
        {
            return Err("Timeline invalid-signature mutation recipe identity drifted".to_owned());
        }

        let clean_carrier_sha256 = sha256_bytes_hex(&clean_bytes);
        let mutated_carrier_sha256 = sha256_bytes_hex(&mutated_bytes);
        let expected_carrier_key_digest = sha256_bytes_hex(selected.idempotency_key.as_bytes());
        let barrier_identity_sha256 = canonical_json_bytes(&json!({
            "schema": "pointbreak.timeline-invalid-signature-post-pin-barrier-identity.v1",
            "referenceRoot": request.execution.root_provenance_sha256,
            "faultRoot": fault_request.execution.root_provenance_sha256,
            "product": product_identity_sha256,
            "carrierEventId": carrier_event_id,
            "carrierKeyDigest": expected_carrier_key_digest,
            "cleanCarrier": clean_carrier_sha256,
            "mutatedCarrier": mutated_carrier_sha256,
            "mutationRecipe": mutation_recipe_sha256,
        }))
        .map(|bytes| sha256_bytes_hex(&bytes))
        .map_err(|error| error.to_string())?;
        let derived = InspectorChild::spawn_with_timeline_barrier_root(
            &fault_request,
            "sqlite-wal-bodyless-v1",
            Some(&timeline_fault_root.barrier_root),
        )?;
        // Consume the fresh child's cold-start journal freshness reads with
        // one uncounted read that never selects or opens the fault carrier,
        // so the counted request's post-pin barrier can honestly require
        // zero carrier opens before its pin.
        let (warm_status, warm_body) = derived.endpoint.request("GET", "/api/v2/profile")?;
        if warm_status != 200 {
            return Err(format!(
                "Timeline fault child cold-start read failed {warm_status}: {warm_body}"
            ));
        }
        let run_identity = timeline_invalid_signature_run_identity_v1(
            &request.execution.root_provenance_sha256,
            &fault_request.execution.root_provenance_sha256,
            product_identity_sha256,
            &carrier_event_id,
            &expected_carrier_key_digest,
            &clean_carrier_sha256,
            &mutated_carrier_sha256,
            &mutation_recipe_sha256,
            &barrier_identity_sha256,
            &derived.process_identity_sha256,
        )?;
        let barrier_request = LongitudinalTimelinePostPinBarrierRequestV1 {
            schema: LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_REQUEST_SCHEMA_V1.to_owned(),
            barrier_identity_sha256: barrier_identity_sha256.clone(),
            expected_carrier_key_digest,
            clean_carrier_sha256: clean_carrier_sha256.clone(),
            mutated_carrier_sha256: mutated_carrier_sha256.clone(),
            mutation_recipe_sha256: mutation_recipe_sha256.clone(),
        };
        barrier_request.validate()?;
        let counting = json!({
            "runIdentity": run_identity,
            "context": LongitudinalCounterReceiptContextV1 {
                root_identity: fault_request.execution.root_provenance_sha256.clone(),
                operation: "timeline_invalid_signature_fault".to_owned(),
                phase: QualificationDerivedTimelineReadCaseV1::TrustSuite.as_str().to_owned(),
                base_execution_identity_sha256: fault_request.execution.canonical_sha256()?,
                derivative_execution_identity_sha256: product_identity_sha256.to_owned(),
                manifest_sha256: barrier_identity_sha256.clone(),
                schedule_sha256: timeline_request_schedule_sha256_v1(
                    QualificationDerivedChangeFixtureV1::TopologyV1,
                    QualificationDerivedTimelineReadCaseV1::TrustSuite,
                ),
                success: false,
                semantic_result_sha256: sha256_bytes_hex(
                    b"timeline-post-pin-response-semantic-bound-by-server"
                ),
                include_capacity_ownership: false,
            },
            "timelinePostPinBarrier": barrier_request,
        });
        let encoded = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&counting).map_err(|error| error.to_string())?);
        let counted_endpoint = derived.endpoint.clone();
        let counted_target = target.clone();
        let counted_request = thread::spawn(move || {
            counted_endpoint.request_once_with_headers(
                "GET",
                &counted_target,
                &[(LONGITUDINAL_COUNTING_REQUEST_HEADER_V1, &encoded)],
            )
        });
        let ready_path = longitudinal_timeline_post_pin_ready_path_v1(
            &timeline_fault_root.barrier_root,
            &barrier_identity_sha256,
        );
        let release_path = longitudinal_timeline_post_pin_release_path_v1(
            &timeline_fault_root.barrier_root,
            &barrier_identity_sha256,
        );
        let barrier_cleanup =
            TimelinePostPinArtifactCleanup::new(ready_path.clone(), release_path.clone());
        let ready = match wait_for_timeline_post_pin_ready(&ready_path) {
            Ok(ready) => ready,
            Err(error) => {
                // Non-evidence diagnostic: report what the counted request
                // actually did instead of pinning, so a missed barrier is
                // attributable without another invocation. A short grace
                // window distinguishes a still-running request from one the
                // child already answered or refused.
                let grace_deadline = Instant::now() + Duration::from_secs(12);
                while !counted_request.is_finished() && Instant::now() < grace_deadline {
                    thread::sleep(Duration::from_millis(50));
                }
                let completed_before_kill = counted_request.is_finished();
                let child_stderr: String = derived
                    .stderr
                    .lock()
                    .map(|captured| captured.chars().take(600).collect())
                    .unwrap_or_default();
                drop(derived);
                let counted_outcome = match counted_request.join() {
                    Ok(Ok(response)) => {
                        let body: String = response.body.chars().take(400).collect();
                        format!("counted request completed HTTP {}: {body}", response.status)
                    }
                    Ok(Err(request_error)) => {
                        format!("counted request failed: {request_error}")
                    }
                    Err(_) => "counted request thread panicked".to_owned(),
                };
                let _ = barrier_cleanup.finish();
                return Err(format!(
                    "{error}; completed_before_kill={completed_before_kill}; \
                     {counted_outcome}; child stderr: {child_stderr}"
                ));
            }
        };
        if ready.run_identity != run_identity
            || ready.barrier_identity_sha256 != barrier_identity_sha256
            || ready.clean_carrier_sha256 != clean_carrier_sha256
            || ready.mutated_carrier_sha256 != mutated_carrier_sha256
            || ready.mutation_recipe_sha256 != mutation_recipe_sha256
        {
            abort_timeline_post_pin_request(
                &release_path,
                &ready,
                fixture_inventory_sha256,
                "ready document identity mismatch",
                counted_request,
            )?;
            barrier_cleanup.finish()?;
            return Err("Timeline post-pin ready document drifted".to_owned());
        }
        *carrier_restore = Some(OptionalFileRestore {
            path: carrier_path.clone(),
            bytes: Some(clean_bytes.clone()),
            armed: true,
        });
        let mutation_result = (|| {
            write_qualification_file_atomically(&carrier_path, &mutated_bytes)?;
            let reread = std::fs::read(&carrier_path).map_err(|error| error.to_string())?;
            if reread != mutated_bytes || sha256_bytes_hex(&reread) != mutated_carrier_sha256 {
                return Err("Timeline post-pin mutation did not persist exact bytes".to_owned());
            }
            let derivative_inventory =
                longitudinal_authoritative_store_data_inventory_v1(&fault_request.repository)
                    .map_err(|error| error.to_string())?;
            if derivative_inventory.inventory_sha256 == fixture_inventory_sha256 {
                return Err(
                    "Timeline signature derivative did not change source inventory".to_owned(),
                );
            }
            Ok::<_, String>(derivative_inventory)
        })();
        let derivative_inventory = match mutation_result {
            Ok(inventory) => inventory,
            Err(error) => {
                abort_timeline_post_pin_request(
                    &release_path,
                    &ready,
                    fixture_inventory_sha256,
                    &error,
                    counted_request,
                )?;
                barrier_cleanup.finish()?;
                return Err(error);
            }
        };
        if fault == DerivedTimelineInvalidSignatureDiagnosticFaultV1::AfterCarrierWrite {
            abort_timeline_post_pin_request(
                &release_path,
                &ready,
                &derivative_inventory.inventory_sha256,
                "injected post-carrier-write failure",
                counted_request,
            )?;
            barrier_cleanup.finish()?;
            return Err("injected post-carrier-write failure".to_owned());
        }
        let release = LongitudinalTimelinePostPinReleaseV1 {
            schema: LONGITUDINAL_TIMELINE_POST_PIN_RELEASE_SCHEMA_V1.to_owned(),
            run_identity: run_identity.clone(),
            barrier_identity_sha256: barrier_identity_sha256.clone(),
            ready_receipt_sha256: ready.canonical_sha256()?,
            clean_carrier_sha256: clean_carrier_sha256.clone(),
            mutated_carrier_sha256: mutated_carrier_sha256.clone(),
            mutation_recipe_sha256: mutation_recipe_sha256.clone(),
            derivative_inventory_sha256: derivative_inventory.inventory_sha256.clone(),
            abort_reason_sha256: None,
        };
        if let Err(error) = release.validate().and_then(|()| {
            write_longitudinal_timeline_barrier_document_v1(&release_path, &release)
                .map_err(|error| error.to_string())
        }) {
            drop(derived);
            let _ = counted_request.join();
            return Err(format!("publish Timeline post-pin release: {error}"));
        }
        let counted = counted_request
            .join()
            .map_err(|_| "Timeline post-pin request thread panicked".to_owned())??;
        barrier_cleanup.finish()?;
        let derived_document: Value = serde_json::from_str(&counted.body)
            .map_err(|error| format!("counted invalid-signature response was invalid: {error}"))?;
        let derived_status = counted.status;
        let derived_semantic_sha256 = canonical_json_bytes(&derived_document)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())?;

        // Spawn strict after mutation so it cannot inherit a clean carrier.
        let strict = InspectorChild::spawn(&fault_request, "off")?;
        let (strict_status, strict_document) = strict.endpoint.json(&target)?;
        let (_, strict_semantic_sha256) = normalized_timeline_sha256(&strict_document)?;
        if derived_status != 503 || strict_status != 200 {
            return Err(format!(
                "Timeline invalid-signature asymmetric oracle drifted: \
                 derivedStatus={derived_status} strictStatus={strict_status} \
                 derivedSemantic={derived_semantic_sha256} strictSemantic={strict_semantic_sha256}"
            ));
        }
        require_timeline_event_status(
            &strict_document,
            &carrier_event_id,
            EventVerificationStatus::Invalid,
        )?;
        let mutation_phase = timeline_signature_phase_authority(
            &derived,
            derived_status,
            &derived_document,
            &strict,
            strict_status,
            &strict_document,
        )?;
        let observed_typed_document = timeline_typed_document(&derived_document)?;
        let failure_keys = derived_document
            .as_object()
            .map(|document| document.keys().map(String::as_str).collect::<BTreeSet<_>>())
            .unwrap_or_default();
        if observed_typed_document.schema != "pointbreak.inspect-change-projection-error"
            || observed_typed_document.version != 1
            || observed_typed_document.code != "projection_invalid"
            || observed_typed_document.retryable != Some(false)
            || derived_document
                .get("message")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            || failure_keys != BTreeSet::from(["code", "message", "retryable", "schema", "version"])
        {
            return Err("Timeline invalid-signature typed failure document drifted".to_owned());
        }

        let counter_receipt: LongitudinalCounterReceiptV1 = decode_timeline_receipt_header(
            &counted,
            LONGITUDINAL_COUNTER_RECEIPT_HEADER_V1,
            "counter",
        )?;
        let barrier_receipt: LongitudinalTimelinePostPinBarrierReceiptV1 =
            decode_timeline_receipt_header(
                &counted,
                LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_RECEIPT_HEADER_V1,
                "post-pin barrier",
            )?;
        counter_receipt
            .validate()
            .map_err(|error| error.to_string())?;
        barrier_receipt
            .validate()
            .map_err(|error| error.to_string())?;
        let counters = &counter_receipt.counters;
        if counter_receipt.success
            || counters.carrier_opens == 0
            || counters.timeline_selected_carriers == 0
            || counters.authoritative_fallbacks != 0
            || counters.full_history_fallbacks != 0
            || counters.event_folds != 0
            || counters.projection_rebuilds != 0
            || counters.state_rebuilds != 0
            || counters.body_artifact_reads != 0
            || counters.body_bytes_read != 0
            || counters.object_artifact_reads != 0
            || counters.object_bytes_read != 0
            || counters.timeline_trust_support_carriers != 0
            || counters.timeline_entries_emitted != 0
        {
            return Err("Timeline invalid-signature counter bounds drifted".to_owned());
        }
        drop(derived);
        drop(strict);

        let (restored_phase, derived_recovery_semantic_sha256, strict_recovery_semantic_sha256, _) =
            timeline_signature_success_phase(request, &target, &carrier_event_id, "recovery")?;
        let reference_recovery_inventory =
            longitudinal_authoritative_store_data_inventory_v1(&request.repository)
                .map_err(|error| error.to_string())?;
        if derived_recovery_semantic_sha256 != clean_semantic_sha256
            || strict_recovery_semantic_sha256 != strict_clean_semantic_sha256
            || reference_recovery_inventory.inventory_sha256 != fixture_inventory_sha256
        {
            return Err("Timeline signature recovery semantic drifted".to_owned());
        }

        let phases = [&clean_phase, &mutation_phase, &restored_phase];
        let success_authority = [
            clean_phase.success(0)?,
            clean_phase.success(1)?,
            mutation_phase.success(1)?,
            restored_phase.success(0)?,
            restored_phase.success(1)?,
        ];
        if phases
            .iter()
            .flat_map(|phase| phase.process_identity_sha256.iter())
            .collect::<BTreeSet<_>>()
            .len()
            != 6
        {
            return Err("Timeline invalid-signature phases reused a service child".to_owned());
        }
        // Reference-root lanes (clean and recovery) must share the exact clean
        // cursor. The mutated fault-root strict lane is index 2: its live
        // cursor must keep every event-record identity coordinate while its
        // raw journal-record set hash must witness the one-bit carrier
        // mutation, because signature bytes never participate in
        // event-record identity.
        if let Some((index, authority)) = success_authority
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != 2)
            .find(|(_, authority)| authority.authority_cursor_sha256 != clean_cursor_sha256)
        {
            return Err(format!(
                "Timeline invalid-signature phase authority drifted: reference lane {index} \
                 cursor {} differs from the clean cursor {clean_cursor_sha256}",
                authority.authority_cursor_sha256
            ));
        }
        let clean_cursor_document = &success_authority[0].authority_cursor;
        let mutated_cursor_document = &success_authority[2].authority_cursor;
        let cursor_field = |cursor: &Value, name: &str| cursor.get(name).cloned();
        let identity_coordinates_match = [
            "journalRecordCount",
            "eventCount",
            "eventSetHash",
            "capabilitySetHash",
        ]
        .into_iter()
        .all(|name| {
            cursor_field(mutated_cursor_document, name).is_some()
                && cursor_field(mutated_cursor_document, name)
                    == cursor_field(clean_cursor_document, name)
        });
        let raw_record_set_witnesses_mutation =
            cursor_field(mutated_cursor_document, "journalRecordSetHash").is_some()
                && cursor_field(mutated_cursor_document, "journalRecordSetHash")
                    != cursor_field(clean_cursor_document, "journalRecordSetHash");
        if !identity_coordinates_match || !raw_record_set_witnesses_mutation {
            return Err(format!(
                "Timeline invalid-signature mutated strict cursor did not witness the one-bit \
                 carrier mutation with unchanged event-record identity: mutated {}; clean {}",
                mutated_cursor_document, clean_cursor_document,
            ));
        }
        if success_authority[0].source_change_projection_stamp
            != success_authority[3].source_change_projection_stamp
            || success_authority[1].source_change_projection_stamp
                != success_authority[4].source_change_projection_stamp
            || success_authority[0].timeline_projection_stamp
                != success_authority[3].timeline_projection_stamp
            || success_authority[1].timeline_projection_stamp
                != success_authority[4].timeline_projection_stamp
        {
            return Err(
                "Timeline invalid-signature recovery stamps drifted from their clean lanes"
                    .to_owned(),
            );
        }
        if authority_before.trust == timeline_trust_identity_sha256(request)? {
            return Err(
                "Timeline invalid-signature reference trust did not stay staged".to_owned(),
            );
        }
        let phase_process_identity_sha256 = std::array::from_fn(|index| {
            phases[index / 2].process_identity_sha256[index % 2].clone()
        });
        let phase_http_status =
            std::array::from_fn(|index| phases[index / 2].http_status[index % 2]);

        Ok(
            QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1 {
                fault_seed_receipt: timeline_fault_root.fault_seed_receipt.clone(),
                reference_root_identity_sha256: request.execution.root_provenance_sha256.clone(),
                fault_execution: fault_request.execution.clone(),
                reference_fixture_witness_sha256: request.fixture_witness_sha256.clone(),
                fault_fixture_witness_sha256: fault_request.fixture_witness_sha256.clone(),
                carrier_event_id,
                clean_event_record_hash,
                mutated_event_record_hash,
                reference_inventory_sha256: fixture_inventory_sha256.to_owned(),
                reference_recovery_inventory_sha256: reference_recovery_inventory.inventory_sha256,
                fault_clean_inventory_sha256: fixture_inventory_sha256.to_owned(),
                fault_derivative_inventory_sha256: derivative_inventory.inventory_sha256,
                fault_restored_inventory_sha256: String::new(),
                clean_carrier_sha256,
                mutated_carrier_sha256,
                mutation_recipe_sha256,
                clean_signature_status: clean_signature_status.as_str().to_owned(),
                mutated_signature_status: mutated_signature_status.as_str().to_owned(),
                strict_observed_signature_status: EventVerificationStatus::Invalid
                    .as_str()
                    .to_owned(),
                observed_typed_document,
                clean_semantic_sha256,
                strict_clean_semantic_sha256,
                strict_semantic_sha256,
                derived_semantic_sha256,
                strict_recovery_semantic_sha256,
                derived_recovery_semantic_sha256,
                recovery_signature_status: String::new(),
                reference_trust_identity_staged_sha256,
                reference_trust_identity_restored_sha256: String::new(),
                fault_trust_identity_staged_sha256,
                fault_trust_identity_restored_sha256: String::new(),
                phase_process_identity_sha256,
                phase_http_status,
                phase_source_change_projection_stamp: success_authority
                    .map(|authority| authority.source_change_projection_stamp.clone()),
                phase_timeline_projection_stamp: success_authority
                    .map(|authority| authority.timeline_projection_stamp.clone()),
                phase_authority_cursor_sha256: success_authority
                    .map(|authority| authority.authority_cursor_sha256.clone()),
                counter_receipt,
                barrier_receipt,
            },
        )
    }

    fn wait_for_timeline_post_pin_ready(
        path: &Path,
    ) -> Result<LongitudinalTimelinePostPinReadyV1, String> {
        let deadline = Instant::now() + Duration::from_secs(9);
        loop {
            match read_longitudinal_timeline_barrier_document_v1(path) {
                Ok(ready) => {
                    let ready: LongitudinalTimelinePostPinReadyV1 = ready;
                    ready.validate()?;
                    return Ok(ready);
                }
                Err(error) if error.starts_with("not_found:") && Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) if error.starts_with("not_found:") => {
                    return Err("Timeline post-pin ready document timed out".to_owned());
                }
                Err(error) => {
                    return Err(format!("read Timeline post-pin ready document: {error}"));
                }
            }
        }
    }

    fn abort_timeline_post_pin_request(
        release_path: &Path,
        ready: &LongitudinalTimelinePostPinReadyV1,
        derivative_inventory_sha256: &str,
        reason: &str,
        request: thread::JoinHandle<Result<InspectorResponse, String>>,
    ) -> Result<(), String> {
        let release = LongitudinalTimelinePostPinReleaseV1 {
            schema: LONGITUDINAL_TIMELINE_POST_PIN_RELEASE_SCHEMA_V1.to_owned(),
            run_identity: ready.run_identity.clone(),
            barrier_identity_sha256: ready.barrier_identity_sha256.clone(),
            ready_receipt_sha256: ready.canonical_sha256()?,
            clean_carrier_sha256: ready.clean_carrier_sha256.clone(),
            mutated_carrier_sha256: ready.mutated_carrier_sha256.clone(),
            mutation_recipe_sha256: ready.mutation_recipe_sha256.clone(),
            derivative_inventory_sha256: derivative_inventory_sha256.to_owned(),
            abort_reason_sha256: Some(sha256_bytes_hex(reason.as_bytes())),
        };
        release.validate()?;
        write_longitudinal_timeline_barrier_document_v1(release_path, &release)
            .map_err(|error| error.to_string())?;
        let _ = request
            .join()
            .map_err(|_| "Timeline post-pin abort request thread panicked".to_owned())?;
        Ok(())
    }

    fn cleanup_timeline_post_pin_files(ready: &Path, release: &Path) -> Result<(), String> {
        for path in [ready, release] {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "remove Timeline post-pin artifact {}: {error}",
                        path.display()
                    ));
                }
            }
        }
        Ok(())
    }

    struct TimelinePostPinArtifactCleanup {
        ready: PathBuf,
        release: PathBuf,
        armed: bool,
    }

    impl TimelinePostPinArtifactCleanup {
        fn new(ready: PathBuf, release: PathBuf) -> Self {
            Self {
                ready,
                release,
                armed: true,
            }
        }

        fn finish(mut self) -> Result<(), String> {
            cleanup_timeline_post_pin_files(&self.ready, &self.release)?;
            self.armed = false;
            Ok(())
        }
    }

    impl Drop for TimelinePostPinArtifactCleanup {
        fn drop(&mut self) {
            if self.armed {
                let _ = cleanup_timeline_post_pin_files(&self.ready, &self.release);
            }
        }
    }

    fn decode_timeline_receipt_header<T: for<'de> Deserialize<'de>>(
        response: &InspectorResponse,
        header: &str,
        label: &str,
    ) -> Result<T, String> {
        let encoded = response
            .headers
            .get(&header.to_ascii_lowercase())
            .ok_or_else(|| format!("Timeline response omitted its {label} receipt"))?;
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|error| format!("decode Timeline {label} receipt: {error}"))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| format!("parse Timeline {label} receipt: {error}"))
    }

    pub(super) fn replace_unique_bytes(
        source: &[u8],
        old: &[u8],
        replacement: &[u8],
    ) -> Result<Vec<u8>, String> {
        if old.is_empty() || old.len() != replacement.len() {
            return Err("Timeline signature replacement length drifted".to_owned());
        }
        let matches = source
            .windows(old.len())
            .enumerate()
            .filter_map(|(index, candidate)| (candidate == old).then_some(index))
            .collect::<Vec<_>>();
        let [index] = matches.as_slice() else {
            return Err("Timeline signature bytes were not uniquely bound".to_owned());
        };
        let mut mutated = source.to_vec();
        mutated[*index..*index + old.len()].copy_from_slice(replacement);
        Ok(mutated)
    }

    fn require_timeline_event_status(
        document: &Value,
        event_id: &str,
        expected: EventVerificationStatus,
    ) -> Result<(), String> {
        let entries = document
            .get("entries")
            .and_then(Value::as_array)
            .ok_or_else(|| "Timeline signature response omitted entries".to_owned())?;
        if entries.len() != 1
            || entries[0].get("eventId").and_then(Value::as_str) != Some(event_id)
            || entries[0].get("verificationStatus").and_then(Value::as_str)
                != Some(expected.as_str())
        {
            return Err("Timeline signature response status drifted".to_owned());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_timeline_row(
        request: &QualificationDerivedChangeReadRunRequestV1,
        case: QualificationDerivedTimelineReadCaseV1,
        before: TimelineAuthorityPoint,
        after: TimelineAuthorityPoint,
        operations: Vec<TimelineOperationEvidence>,
        product_identity_sha256: &str,
        execution_identity_sha256: &str,
        fixture_inventory_sha256: &str,
        trust_transition: Option<QualificationDerivedTimelineTrustTransitionV1>,
        concurrent_trust_transition: Option<QualificationDerivedTimelineConcurrentTrustEvidenceV1>,
        invalid_signature_failure: Option<
            QualificationDerivedTimelineInvalidSignatureFailureEvidenceV1,
        >,
    ) -> Result<QualificationDerivedTimelineReadEvidenceV1, String> {
        let counter_receipts = operations
            .iter()
            .map(|operation| operation.receipt.clone())
            .collect::<Vec<_>>();
        let semantic_hashes = counter_receipts
            .iter()
            .map(|receipt| receipt.semantic_result_sha256.clone())
            .collect::<Vec<_>>();
        let derived_semantic_sha256 = canonical_json_bytes(
            &serde_json::to_value(&semantic_hashes).map_err(|error| error.to_string())?,
        )
        .map(|bytes| sha256_bytes_hex(&bytes))
        .map_err(|error| error.to_string())?;
        let mut wire_contract_matches = operations.iter().all(|operation| operation.strict_matches);
        let observed_typed_documents = operations
            .iter()
            .filter_map(|operation| operation.typed_document.clone())
            .collect::<Vec<_>>();
        let raw_tokens = operations
            .iter()
            .flat_map(|operation| operation.raw_tokens.iter().cloned())
            .collect::<Vec<_>>();
        let continuation_token_set_sha256 = matches!(
            case,
            QualificationDerivedTimelineReadCaseV1::PageTokenSuite
                | QualificationDerivedTimelineReadCaseV1::TrustSuite
                | QualificationDerivedTimelineReadCaseV1::PostAppendSuite
        )
        .then(|| {
            canonical_json_bytes(
                &serde_json::to_value(raw_tokens).map_err(|error| error.to_string())?,
            )
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())
        })
        .transpose()?;
        let (
            authoritative_event_family_counts,
            strict_event_family_counts,
            derived_event_family_counts,
            excluded_timeline_case_counts,
        ) = if request.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
            && case == QualificationDerivedTimelineReadCaseV1::StructuredQuerySuite
        {
            timeline_family_authority(request)?
        } else {
            (
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
            )
        };
        wire_contract_matches &= excluded_timeline_case_counts.values().all(|counts| {
            counts.source_count > 0
                && counts.strict_output_count == 0
                && counts.derived_output_count == 0
        });
        if case == QualificationDerivedTimelineReadCaseV1::TrustSuite {
            wire_contract_matches &= trust_transition.as_ref().is_some_and(|transition| {
                transition.status_before_by_event
                    == BTreeMap::from([
                        (
                            transition.signed_event_id.clone(),
                            "untrusted_key".to_owned(),
                        ),
                        (transition.unsigned_event_id.clone(), "unsigned".to_owned()),
                    ])
                    && transition.status_after_by_event
                        == BTreeMap::from([
                            (transition.signed_event_id.clone(), "valid".to_owned()),
                            (transition.unsigned_event_id.clone(), "unsigned".to_owned()),
                        ])
            });
        }
        let oracle = if qualification_derived_change_expected_outcome_v1(
            request.execution.platform,
            request.fixture,
            QualificationDerivedChangeReadCaseV1::ChangesBare,
        )
        .0 == QualificationDerivedChangeReadOracleV1::TypedFailure
        {
            QualificationDerivedTimelineReadOracleV1::TypedFailure
        } else {
            QualificationDerivedTimelineReadOracleV1::StrictParity
        };
        Ok(QualificationDerivedTimelineReadEvidenceV1 {
            platform: request.execution.platform,
            fixture: request.fixture,
            fixture_inventory_sha256: fixture_inventory_sha256.to_owned(),
            fixture_witness_sha256: request.fixture_witness_sha256.clone(),
            case,
            semantic_process_scope: QualificationDerivedAccessProcessScopeV1::InspectorServiceChild,
            counter_process_scope: QualificationDerivedAccessProcessScopeV1::InspectorServiceChild,
            product_identity_sha256: product_identity_sha256.to_owned(),
            counter_execution_identity_sha256: execution_identity_sha256.to_owned(),
            status: if wire_contract_matches {
                QualificationDerivedAccessStatusV1::Passed
            } else {
                QualificationDerivedAccessStatusV1::Failed
            },
            oracle,
            strict_semantic_sha256: (oracle
                == QualificationDerivedTimelineReadOracleV1::StrictParity)
                .then(|| derived_semantic_sha256.clone()),
            derived_semantic_sha256,
            wire_contract_matches,
            expected_typed_documents: expected_timeline_typed_documents_v1(
                request.execution.platform,
                request.fixture,
                case,
            ),
            observed_typed_documents,
            authority: QualificationDerivedTimelineAuthorityEvidenceV1 {
                request_schedule_sha256: timeline_request_schedule_sha256_v1(request.fixture, case),
                generation_identity_before_sha256: before.generation,
                generation_identity_after_sha256: after.generation,
                checkpoint_identity_before_sha256: before.checkpoint,
                checkpoint_identity_after_sha256: after.checkpoint,
                timeline_projection_stamp_before_sha256: before.stamp,
                timeline_projection_stamp_after_sha256: after.stamp,
                trust_identity_before_sha256: before.trust,
                trust_identity_after_sha256: after.trust,
                continuation_token_set_sha256,
                authoritative_event_family_counts,
                strict_event_family_counts,
                derived_event_family_counts,
                excluded_timeline_case_counts,
            },
            trust_transition,
            concurrent_trust_transition,
            invalid_signature_failure,
            counter_receipts,
        })
    }

    fn timeline_operation_target(
        operation: &str,
        endpoint: &InspectorEndpoint,
        stale_token: Option<&str>,
    ) -> Result<String, String> {
        let base = "/api/v2/history?limit=2&order=asc";
        let first = || endpoint.json(base).map(|(_, value)| value);
        let token = |name: &str| -> Result<String, String> {
            first()?
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("Timeline operation {operation} omitted {name}"))
        };
        Ok(match operation {
            "timeline_all_asc" => "/api/v2/history?limit=100&order=asc".to_owned(),
            "timeline_all_desc" => "/api/v2/history?limit=100&order=desc".to_owned(),
            "timeline_type_filter" => {
                "/api/v2/history?limit=100&order=asc&type=validation_check_recorded".to_owned()
            }
            "timeline_track_filter" => {
                let value = endpoint.json("/api/v2/history?limit=100&order=asc")?.1;
                let track = value.get("entries").and_then(Value::as_array)
                    .and_then(|entries| entries.iter().find_map(|entry| entry.get("trackId").and_then(Value::as_str)))
                    .ok_or_else(|| "Timeline fixture omitted a track filter witness".to_owned())?;
                format!("/api/v2/history?limit=100&order=asc&track={}", percent_encode(track))
            }
            "timeline_change_filter" => {
                let value = endpoint.json("/api/v2/history?limit=100&order=asc")?.1;
                let change = value.get("entries").and_then(Value::as_array)
                    .and_then(|entries| entries.iter().find_map(|entry| entry.get("changeIds").and_then(Value::as_array).and_then(|ids| ids.first()).and_then(Value::as_str)))
                    .ok_or_else(|| "Timeline fixture omitted a Change filter witness".to_owned())?;
                format!("/api/v2/history?limit=100&order=asc&change={}", percent_encode(change))
            }
            "timeline_exact_revision_filter" => timeline_exact_revision_target(endpoint)?,
            "timeline_facets_count_at" => {
                let value = first()?;
                let event = value.pointer("/entries/0/eventId").and_then(Value::as_str)
                    .ok_or_else(|| "Timeline fixture omitted an event locator".to_owned())?;
                format!("/api/v2/history?limit=2&order=asc&at={}", percent_encode(event))
            }
            "timeline_revision_correlations" => timeline_exact_revision_target(endpoint)?,
            "timeline_withdrawal_equal_time_ordering" => "/api/v2/history?limit=100&order=asc&type=revision_ref_withdrawn%2Cchange_revision_relation_withdrawn".to_owned(),
            "timeline_invalid_query" => "/api/v2/history?limit=2&at=evt%3Ainvalid&after=invalid".to_owned(),
            "timeline_fault_outcome" => "/api/v2/history".to_owned(),
            "timeline_exhaustive_body_search" => "/api/v2/history?limit=100&order=asc&q=the%20matrix%20keeps%20evidence%20classes%20distinct".to_owned(),
            "timeline_exhaustive_facets_count_window" => "/api/v2/history?limit=100&order=asc&q=matrix".to_owned(),
            "timeline_next" => format!("{base}&after={}", percent_encode(&token("next")?)),
            "timeline_previous" => {
                let page_two = endpoint.json(&format!("{base}&after={}", percent_encode(&token("next")?)))?.1;
                let previous = page_two.get("previous").and_then(Value::as_str)
                    .ok_or_else(|| "Timeline second page omitted previous".to_owned())?;
                format!("{base}&after={}", percent_encode(previous))
            }
            "timeline_token_query_mismatch" => format!("{base}&q=changed&after={}", percent_encode(&token("next")?)),
            "timeline_token_direction_limit_mismatch" => format!("/api/v2/history?limit=3&order=asc&after={}", percent_encode(&token("next")?)),
            "timeline_at_token_exclusive" => format!("{base}&at=evt%3Ainvalid&after={}", percent_encode(&token("next")?)),
            "timeline_trust_stale_token" | "timeline_k_stale_token" => {
                let selected = match stale_token {
                    Some(token) => token.to_owned(),
                    None => token("next")?,
                };
                format!("{base}&after={}", percent_encode(&selected))
            }
            _ => base.to_owned(),
        })
    }

    fn timeline_exact_revision_target(endpoint: &InspectorEndpoint) -> Result<String, String> {
        let value = endpoint.json("/api/v2/history?limit=100&order=asc")?.1;
        timeline_exact_revision_target_from_document(&value)
    }

    pub(super) fn timeline_exact_revision_target_from_document(
        value: &Value,
    ) -> Result<String, String> {
        let reference = value
            .get("entries")
            .and_then(Value::as_array)
            .and_then(|entries| {
                entries.iter().find_map(|entry| {
                    entry
                        .get("revisionRefs")
                        .and_then(Value::as_array)
                        .and_then(|refs| refs.first())
                })
            })
            .ok_or_else(|| {
                "Timeline fixture omitted an exact Revision filter witness".to_owned()
            })?;
        let revision = reference
            .get("revisionId")
            .and_then(Value::as_str)
            .ok_or_else(|| "Timeline Revision witness omitted revisionId".to_owned())?;
        let artifact = reference
            .get("objectArtifactContentHash")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                "Timeline Revision witness omitted objectArtifactContentHash".to_owned()
            })?;
        Ok(format!(
            "/api/v2/history?limit=100&order=asc&revision={}&artifactHash={}",
            percent_encode(revision),
            percent_encode(artifact)
        ))
    }

    fn timeline_operation_semantics_match(operation: &str, status: u16, document: &Value) -> bool {
        if timeline_operation_is_expected_failure(operation) {
            return if operation == "timeline_fault_outcome" {
                status == 200
                    || status != 200
                        && matches!(
                            document.get("schema").and_then(Value::as_str),
                            Some("pointbreak.inspect-change-projection-error")
                                | Some("pointbreak.inspect-event-history-error")
                        )
            } else {
                status != 200
                    && document.get("schema").and_then(Value::as_str)
                        == Some("pointbreak.inspect-event-history-error")
            };
        }
        let Some(entries) = document.get("entries").and_then(Value::as_array) else {
            return false;
        };
        if status != 200 || entries.is_empty() {
            return false;
        }
        let ordered = |descending: bool| {
            let keys = entries
                .iter()
                .filter_map(|entry| {
                    Some((
                        entry.get("occurredAt")?.as_str()?,
                        entry.get("eventId")?.as_str()?,
                    ))
                })
                .collect::<Vec<_>>();
            keys.len() == entries.len()
                && keys.windows(2).all(|pair| {
                    if descending {
                        pair[0] >= pair[1]
                    } else {
                        pair[0] <= pair[1]
                    }
                })
        };
        match operation {
            "timeline_all_asc" => ordered(false),
            "timeline_all_desc" => ordered(true),
            "timeline_type_filter" => entries.iter().all(|entry| {
                entry.get("eventType").and_then(Value::as_str) == Some("validation_check_recorded")
            }),
            "timeline_track_filter" => entries
                .iter()
                .all(|entry| entry.get("trackId").and_then(Value::as_str).is_some()),
            "timeline_change_filter" => entries.iter().all(|entry| {
                entry
                    .get("changeIds")
                    .and_then(Value::as_array)
                    .is_some_and(|ids| !ids.is_empty())
            }),
            "timeline_exact_revision_filter" => entries.iter().all(|entry| {
                entry
                    .get("revisionRefs")
                    .and_then(Value::as_array)
                    .is_some_and(|refs| !refs.is_empty())
            }),
            "timeline_facets_count_at" => {
                document.get("facets").and_then(Value::as_object).is_some()
                    && document.get("matchCount").and_then(Value::as_u64).is_some()
                    && document.get("matchIndex").and_then(Value::as_u64).is_some()
            }
            "timeline_revision_correlations" => entries.iter().all(|entry| {
                entry
                    .get("revisionRefs")
                    .and_then(Value::as_array)
                    .is_some_and(|refs| !refs.is_empty())
                    && entry
                        .get("changeIds")
                        .and_then(Value::as_array)
                        .is_some_and(|ids| !ids.is_empty())
            }),
            "timeline_withdrawal_equal_time_ordering" => {
                ordered(false)
                    && entries.iter().all(|entry| {
                        matches!(
                            entry.get("eventType").and_then(Value::as_str),
                            Some("revision_ref_withdrawn")
                                | Some("change_revision_relation_withdrawn")
                        )
                    })
            }
            _ => true,
        }
    }

    pub(super) fn normalize_timeline_semantic_checked(mut value: Value) -> Result<Value, String> {
        let Some(object) = value.as_object_mut() else {
            return Ok(value);
        };
        let valid_hash = |value: &str| {
            value.strip_prefix("sha256:").is_some_and(|digest| {
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            })
        };
        if object.get("schema").and_then(Value::as_str) == Some("pointbreak.inspect-event-history")
        {
            if object.get("version").and_then(Value::as_u64) != Some(1) {
                return Err("Timeline success document version drifted".to_owned());
            }
            for (field, placeholder) in [
                (
                    "sourceChangeProjectionStamp",
                    "<source-change-projection-stamp>",
                ),
                ("timelineProjectionStamp", "<timeline-projection-stamp>"),
            ] {
                if !object
                    .get(field)
                    .and_then(Value::as_str)
                    .is_some_and(valid_hash)
                {
                    return Err(format!("Timeline success document {field} was not SHA-256"));
                }
                object.insert(field.to_owned(), json!(placeholder));
            }
        }
        for field in ["previous", "next"] {
            if let Some(token) = object.get_mut(field) {
                if !token.is_string() {
                    return Err(format!("Timeline document {field} was not an opaque token"));
                }
                *token = json!("<opaque-continuation>");
            }
        }
        Ok(value)
    }

    fn timeline_success_authority(value: &Value) -> Result<TimelineSuccessAuthority, String> {
        if value.get("schema").and_then(Value::as_str) != Some("pointbreak.inspect-event-history") {
            return Err("Timeline phase response was not a v1 success document".to_owned());
        }
        normalize_timeline_semantic_checked(value.clone())?;
        let stamp = |field: &str| {
            value
                .get(field)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("Timeline phase response omitted {field}"))
        };
        let authority_cursor = value
            .get("authorityCursor")
            .ok_or_else(|| "Timeline phase response omitted authorityCursor".to_owned())?;
        if authority_cursor.get("schema").and_then(Value::as_str)
            != Some("pointbreak.authority-cursor.v2")
        {
            return Err("Timeline phase response authority cursor drifted".to_owned());
        }
        Ok(TimelineSuccessAuthority {
            source_change_projection_stamp: stamp("sourceChangeProjectionStamp")?,
            timeline_projection_stamp: stamp("timelineProjectionStamp")?,
            authority_cursor_sha256: canonical_json_bytes(authority_cursor)
                .map(|bytes| sha256_bytes_hex(&bytes))
                .map_err(|error| error.to_string())?,
            authority_cursor: authority_cursor.clone(),
        })
    }

    fn normalized_timeline_sha256(value: &Value) -> Result<(Value, String), String> {
        let normalized = normalize_timeline_semantic_checked(value.clone())?;
        let sha256 = canonical_json_bytes(&normalized)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())?;
        Ok((normalized, sha256))
    }

    fn timeline_signature_phase_authority(
        derived: &InspectorChild,
        derived_status: u16,
        derived_document: &Value,
        strict: &InspectorChild,
        strict_status: u16,
        strict_document: &Value,
    ) -> Result<TimelineSignaturePhaseAuthority, String> {
        let strict_authority = timeline_success_authority(strict_document)?;
        let derived_authority = (derived_status == 200)
            .then(|| timeline_success_authority(derived_document))
            .transpose()?;
        Ok(TimelineSignaturePhaseAuthority {
            process_identity_sha256: [
                derived.process_identity_sha256.clone(),
                strict.process_identity_sha256.clone(),
            ],
            http_status: [derived_status, strict_status],
            authority: [derived_authority, Some(strict_authority)],
        })
    }

    fn spawn_timeline_signature_pair(
        request: &QualificationDerivedChangeReadRunRequestV1,
    ) -> Result<(InspectorChild, InspectorChild), String> {
        let derived = InspectorChild::spawn(request, "sqlite-wal-bodyless-v1")?;
        let strict = InspectorChild::spawn(request, "off")?;
        Ok((derived, strict))
    }

    fn timeline_signature_success_phase(
        request: &QualificationDerivedChangeReadRunRequestV1,
        target: &str,
        event_id: &str,
        label: &str,
    ) -> Result<(TimelineSignaturePhaseAuthority, String, String, String), String> {
        let (derived, strict) = spawn_timeline_signature_pair(request)?;
        let (derived_status, derived_document) = derived.endpoint.json(target)?;
        let (strict_status, strict_document) = strict.endpoint.json(target)?;
        let (derived_normalized, derived_sha256) = normalized_timeline_sha256(&derived_document)?;
        let (strict_normalized, strict_sha256) = normalized_timeline_sha256(&strict_document)?;
        if derived_status != 200 || strict_status != 200 || derived_normalized != strict_normalized
        {
            return Err(format!(
                "Timeline signature {label} parity drifted: derivedStatus={derived_status} \
                 strictStatus={strict_status} derivedSemantic={derived_sha256} \
                 strictSemantic={strict_sha256} firstDifference={}",
                first_json_difference(&derived_normalized, &strict_normalized, String::new())
                    .unwrap_or_else(|| "<status-only>".to_owned())
            ));
        }
        require_timeline_event_status(&derived_document, event_id, EventVerificationStatus::Valid)?;
        require_timeline_event_status(&strict_document, event_id, EventVerificationStatus::Valid)?;
        let phase = timeline_signature_phase_authority(
            &derived,
            derived_status,
            &derived_document,
            &strict,
            strict_status,
            &strict_document,
        )?;
        let cursor = phase.success(0)?.authority_cursor_sha256.clone();
        if cursor != phase.success(1)?.authority_cursor_sha256 {
            return Err(format!("Timeline signature {label} cursors drifted"));
        }
        Ok((phase, derived_sha256, strict_sha256, cursor))
    }

    pub(super) fn first_json_difference(
        left: &Value,
        right: &Value,
        path: String,
    ) -> Option<String> {
        match (left, right) {
            (Value::Object(left), Value::Object(right)) => left
                .keys()
                .chain(right.keys())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .find_map(|key| {
                    let next = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
                    match (left.get(key), right.get(key)) {
                        (Some(left), Some(right)) => first_json_difference(left, right, next),
                        _ => Some(next),
                    }
                }),
            (Value::Array(left), Value::Array(right)) => (0..left.len().min(right.len()))
                .find_map(|index| {
                    first_json_difference(&left[index], &right[index], format!("{path}/{index}"))
                })
                .or_else(|| {
                    (left.len() != right.len())
                        .then(|| format!("{path}/{}", left.len().min(right.len())))
                }),
            _ => (left != right).then_some(path),
        }
    }

    fn timeline_operation_is_expected_failure(operation: &str) -> bool {
        matches!(
            operation,
            "timeline_invalid_query"
                | "timeline_token_query_mismatch"
                | "timeline_token_direction_limit_mismatch"
                | "timeline_at_token_exclusive"
                | "timeline_trust_stale_token"
                | "timeline_k_stale_token"
                | "timeline_fault_outcome"
        )
    }

    fn timeline_typed_document(
        value: &Value,
    ) -> Result<QualificationDerivedChangeTypedDocumentV1, String> {
        let schema = value
            .get("schema")
            .and_then(Value::as_str)
            .ok_or_else(|| "Timeline failure omitted schema".to_owned())?;
        let version = value
            .get("version")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| "Timeline failure omitted version".to_owned())?;
        let code = value
            .get("code")
            .and_then(Value::as_str)
            .ok_or_else(|| "Timeline failure omitted code".to_owned())?;
        let retryable = value.get("retryable").and_then(Value::as_bool);
        let canonical_sha256 = canonical_json_bytes(value)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())?;
        Ok(QualificationDerivedChangeTypedDocumentV1 {
            schema: schema.to_owned(),
            version,
            code: code.to_owned(),
            retryable,
            canonical_sha256,
        })
    }

    fn timeline_authority_point(
        request: &QualificationDerivedChangeReadRunRequestV1,
        endpoint: &InspectorEndpoint,
        storage: Option<&QualificationDerivedChangeStorageEvidenceV1>,
    ) -> Result<TimelineAuthorityPoint, String> {
        let storage =
            storage.ok_or_else(|| "Timeline authority omitted storage witness".to_owned())?;
        let (status, history) = endpoint.json("/api/v2/history?limit=1&order=asc")?;
        let stamp = if status == 200 {
            history
                .get("timelineProjectionStamp")
                .and_then(Value::as_str)
                .map(|stamp| sha256_bytes_hex(stamp.as_bytes()))
                .ok_or_else(|| "Timeline response omitted its projection stamp".to_owned())?
        } else {
            canonical_json_bytes(&history)
                .map(|bytes| sha256_bytes_hex(&bytes))
                .map_err(|error| error.to_string())?
        };
        // The live checkpoint document carries a `sha256:`-prefixed identity,
        // but every receipt/evaluator authority field is bare 64-hex; both
        // branches must agree or the receipt-layer digest checks reject the
        // row.
        let checkpoint = storage
            .witness
            .live_checkpoint
            .as_ref()
            .map(|checkpoint| {
                checkpoint
                    .checkpoint_sha256
                    .strip_prefix("sha256:")
                    .unwrap_or(&checkpoint.checkpoint_sha256)
                    .to_owned()
            })
            .unwrap_or_else(|| {
                sha256_bytes_hex(
                    &canonical_json_bytes(&history["authorityCursor"])
                        .expect("authority cursor is canonical"),
                )
            });
        let trust_path = allowed_signers_path_for_repo(&request.repository)
            .map_err(|error| error.to_string())?;
        let trust = if trust_path.exists() {
            TrustSet::from_allowed_signers_file(&trust_path).map_err(|error| error.to_string())?
        } else {
            TrustSet::default()
        };
        Ok(TimelineAuthorityPoint {
            generation: storage.witness.publication.generation_id_sha256.clone(),
            checkpoint,
            stamp,
            trust: trust
                .identity_sha256()
                .map_err(|error| error.to_string())?
                .trim_start_matches("sha256:")
                .to_owned(),
        })
    }

    type TimelineFamilyAuthority = (
        BTreeMap<String, u64>,
        BTreeMap<String, u64>,
        BTreeMap<String, u64>,
        BTreeMap<String, QualificationDerivedTimelineExclusionCountsV1>,
    );

    fn timeline_family_authority(
        request: &QualificationDerivedChangeReadRunRequestV1,
    ) -> Result<TimelineFamilyAuthority, String> {
        let (events, _) =
            read_events_for_display(&request.repository).map_err(|error| error.to_string())?;
        let mut source = QUALIFICATION_TIMELINE_SOURCE_EVENT_FAMILIES_V1
            .into_iter()
            .map(|kind| (kind.to_owned(), 0_u64))
            .collect::<BTreeMap<_, _>>();
        let mut exclusions = QUALIFICATION_TIMELINE_EXCLUDED_CASES_V1
            .into_iter()
            .map(|case| {
                (
                    case.to_owned(),
                    QualificationDerivedTimelineExclusionCountsV1 {
                        source_count: 0,
                        strict_output_count: 0,
                        derived_output_count: 0,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut excluded_event_ids = QUALIFICATION_TIMELINE_EXCLUDED_CASES_V1
            .into_iter()
            .map(|case| (case.to_owned(), Vec::<String>::new()))
            .collect::<BTreeMap<_, _>>();
        for event in events {
            *source
                .entry(event.event_type.as_str().to_owned())
                .or_default() += 1;
            let direct = match event.event_type {
                EventType::TaskCheckpointCaptured => Some("task_checkpoint_captured"),
                EventType::TaskObservationRecorded => Some("task_observation_recorded"),
                EventType::EventSignatureRecorded => Some("event_signature_recorded"),
                EventType::ArtifactRemoved => Some("artifact_removed"),
                _ => None,
            };
            let conditional = match event.event_type {
                EventType::WorkObjectProposed => {
                    let payload: WorkObjectProposedPayload =
                        serde_json::from_value(event.payload.clone())
                            .map_err(|error| error.to_string())?;
                    matches!(payload.work_object, WorkObjectProposal::TaskAttempt { .. })
                        .then_some("work_object_proposed_task_attempt")
                }
                EventType::InputRequestOpened => {
                    let payload: InputRequestOpenedPayload =
                        serde_json::from_value(event.payload.clone())
                            .map_err(|error| error.to_string())?;
                    payload
                        .task_target
                        .is_some()
                        .then_some("input_request_opened_task")
                }
                EventType::InputRequestResponded => {
                    let payload: InputRequestRespondedPayload =
                        serde_json::from_value(event.payload.clone())
                            .map_err(|error| error.to_string())?;
                    payload
                        .task_target
                        .is_some()
                        .then_some("input_request_responded_task")
                }
                _ => None,
            };
            if let Some(case) = direct.or(conditional) {
                exclusions
                    .get_mut(case)
                    .expect("known exclusion")
                    .source_count += 1;
                excluded_event_ids
                    .get_mut(case)
                    .expect("known exclusion")
                    .push(event.event_id.as_str().to_owned());
            }
        }
        if source.values().any(|count| *count == 0)
            || exclusions.values().any(|counts| counts.source_count == 0)
        {
            return Err(
                "public Timeline fixture omits an authoritative event-family witness".to_owned(),
            );
        }
        let derived_child = InspectorChild::spawn(request, "sqlite-wal-bodyless-v1")?;
        derived_child.ensure_ready()?;
        let strict_child = InspectorChild::spawn(request, "off")?;
        let (derived, derived_ids) = timeline_public_inventory(&derived_child.endpoint)?;
        let (strict, strict_ids) = timeline_public_inventory(&strict_child.endpoint)?;
        for (case, event_ids) in &excluded_event_ids {
            let counts = exclusions.get_mut(case).expect("known exclusion");
            counts.strict_output_count = event_ids
                .iter()
                .filter(|event_id| strict_ids.contains(*event_id))
                .count() as u64;
            counts.derived_output_count = event_ids
                .iter()
                .filter(|event_id| derived_ids.contains(*event_id))
                .count() as u64;
        }
        let exact = QUALIFICATION_TIMELINE_ADMITTED_EVENT_FAMILIES_V1
            .into_iter()
            .collect::<BTreeSet<_>>();
        if derived.keys().map(String::as_str).collect::<BTreeSet<_>>() != exact
            || strict != derived
            || derived.values().any(|count| *count == 0)
        {
            return Err(
                "public Timeline fixture omits an admitted event-family witness".to_owned(),
            );
        }
        Ok((source, strict, derived, exclusions))
    }

    fn timeline_public_inventory(
        endpoint: &InspectorEndpoint,
    ) -> Result<(BTreeMap<String, u64>, BTreeSet<String>), String> {
        let mut target = "/api/v2/history?limit=100&order=asc".to_owned();
        let mut counts = BTreeMap::new();
        let mut event_ids = BTreeSet::new();
        loop {
            let (status, document) = endpoint.json(&target)?;
            if status != 200 {
                return Err("Timeline family inventory returned a typed failure".to_owned());
            }
            for entry in document
                .get("entries")
                .and_then(Value::as_array)
                .ok_or_else(|| "Timeline family inventory omitted entries".to_owned())?
            {
                let kind = entry
                    .get("eventType")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "Timeline family inventory omitted event type".to_owned())?;
                *counts.entry(kind.to_owned()).or_insert(0) += 1;
                let event_id = entry
                    .get("eventId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "Timeline family inventory omitted event ID".to_owned())?;
                event_ids.insert(event_id.to_owned());
            }
            let Some(next) = document.get("next").and_then(Value::as_str) else {
                break;
            };
            target = format!(
                "/api/v2/history?limit=100&order=asc&after={}",
                percent_encode(next)
            );
        }
        Ok((counts, event_ids))
    }

    type TimelineTrustStage = (
        OptionalFileRestore,
        QualificationDerivedTimelineTrustTransitionV1,
    );

    fn stage_timeline_trust(
        request: &QualificationDerivedChangeReadRunRequestV1,
        endpoint: &InspectorEndpoint,
    ) -> Result<TimelineTrustStage, String> {
        let path = allowed_signers_path_for_repo(&request.repository)
            .map_err(|error| error.to_string())?;
        let backup = std::fs::read(&path).ok();
        let fixture_witness: Value = read_json(&request.fixture_witness)?;
        let signed_witness = fixture_witness
            .pointer("/timeline/trust/signedEvent")
            .and_then(Value::as_str)
            .ok_or_else(|| "Timeline fixture witness omitted signedEvent".to_owned())?;
        let unsigned_witness = fixture_witness
            .pointer("/timeline/trust/unsignedEvent")
            .and_then(Value::as_str)
            .ok_or_else(|| "Timeline fixture witness omitted unsignedEvent".to_owned())?;
        let before_entries = timeline_entries_by_id(endpoint, &[signed_witness, unsigned_witness])?;
        let signed = before_entries
            .get(signed_witness)
            .ok_or_else(|| "Timeline fixture omitted an untrusted signed event".to_owned())?;
        let unsigned = before_entries
            .get(unsigned_witness)
            .ok_or_else(|| "Timeline fixture omitted an unsigned event".to_owned())?;
        if signed.get("verificationStatus").and_then(Value::as_str) != Some("untrusted_key")
            || unsigned.get("verificationStatus").and_then(Value::as_str) != Some("unsigned")
        {
            return Err("Timeline trust witnesses did not start untrusted and unsigned".to_owned());
        }
        let signed_event_id = signed
            .get("eventId")
            .and_then(Value::as_str)
            .unwrap()
            .to_owned();
        let unsigned_event_id = unsigned
            .get("eventId")
            .and_then(Value::as_str)
            .unwrap()
            .to_owned();
        let signer_identity = signed
            .get("signer")
            .and_then(Value::as_str)
            .unwrap()
            .to_owned();
        let actor = signed
            .pointer("/writer/actorId")
            .and_then(Value::as_str)
            .ok_or_else(|| "Timeline signed witness omitted writer actor".to_owned())?;
        let trust_bytes = timeline_allowed_signers_bytes(actor, &signer_identity)?;
        let restore = OptionalFileRestore {
            path: path.clone(),
            bytes: backup,
            armed: true,
        };
        let stage_result = (|| {
            write_qualification_file_atomically(&path, &trust_bytes)?;
            let after_entries =
                timeline_entries_by_id(endpoint, &[signed_witness, unsigned_witness])?;
            let status_map =
                |entries: &BTreeMap<String, Value>| -> Result<BTreeMap<String, String>, String> {
                    [signed_event_id.as_str(), unsigned_event_id.as_str()]
                        .into_iter()
                        .map(|id| {
                            let entry = entries.get(id).ok_or_else(|| {
                                format!("Timeline trust response omitted event {id}")
                            })?;
                            let status = entry
                                .get("verificationStatus")
                                .and_then(Value::as_str)
                                .ok_or_else(|| {
                                    "Timeline trust response omitted status".to_owned()
                                })?;
                            Ok((id.to_owned(), status.to_owned()))
                        })
                        .collect()
                };
            Ok::<_, String>(QualificationDerivedTimelineTrustTransitionV1 {
                unsigned_event_id: unsigned_event_id.clone(),
                signed_event_id: signed_event_id.clone(),
                signer_identity,
                status_before_by_event: status_map(&before_entries)?,
                status_after_by_event: status_map(&after_entries)?,
            })
        })();
        match stage_result {
            Ok(transition) => Ok((restore, transition)),
            Err(primary) => match restore.restore() {
                Ok(()) => Err(primary),
                Err(restoration) => Err(format!(
                    "{primary}; explicit Timeline trust stage restoration also failed: {restoration}"
                )),
            },
        }
    }

    fn timeline_entries_by_id(
        endpoint: &InspectorEndpoint,
        required: &[&str],
    ) -> Result<BTreeMap<String, Value>, String> {
        let required = required.iter().copied().collect::<BTreeSet<_>>();
        let mut found = BTreeMap::new();
        let mut target = "/api/v2/history?limit=100&order=asc".to_owned();
        loop {
            let (status, document) = endpoint.json(&target)?;
            if status != 200 {
                return Err("Timeline trust inventory returned a typed failure".to_owned());
            }
            for entry in document
                .get("entries")
                .and_then(Value::as_array)
                .ok_or_else(|| "Timeline trust inventory omitted entries".to_owned())?
            {
                if let Some(event_id) = entry.get("eventId").and_then(Value::as_str)
                    && required.contains(event_id)
                {
                    found.insert(event_id.to_owned(), entry.clone());
                }
            }
            if found.len() == required.len() {
                return Ok(found);
            }
            let Some(next) = document.get("next").and_then(Value::as_str) else {
                break;
            };
            target = format!(
                "/api/v2/history?limit=100&order=asc&after={}",
                percent_encode(next)
            );
        }
        Err("Timeline trust inventory omitted a bound witness".to_owned())
    }

    fn restore_optional_file(path: &Path, bytes: Option<&[u8]>) -> Result<(), String> {
        match bytes {
            Some(bytes) => write_qualification_file_atomically(path, bytes),
            None if path.exists() => std::fs::remove_file(path).map_err(|error| error.to_string()),
            None => Ok(()),
        }
    }

    fn read_optional_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn timeline_trust_identity_sha256(
        request: &QualificationDerivedChangeReadRunRequestV1,
    ) -> Result<String, String> {
        let path = allowed_signers_path_for_repo(&request.repository)
            .map_err(|error| error.to_string())?;
        let trust = if path.exists() {
            TrustSet::from_allowed_signers_file(&path).map_err(|error| error.to_string())?
        } else {
            TrustSet::default()
        };
        trust
            .identity_sha256()
            .map_err(|error| error.to_string())
            .map(|identity| identity.trim_start_matches("sha256:").to_owned())
    }

    fn timeline_allowed_signers_bytes(actor: &str, signer: &str) -> Result<Vec<u8>, String> {
        let mut allowed = serde_json::Map::new();
        allowed.insert(actor.to_owned(), json!([signer]));
        canonical_json_bytes(&json!({"allowedSigners": Value::Object(allowed)}))
            .map_err(|error| error.to_string())
    }

    fn write_qualification_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or_else(|| "qualification file path omitted parent".to_owned())?;
        let file_name = path
            .file_name()
            .ok_or_else(|| "qualification file path omitted file name".to_owned())?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        LocalStorage::new(parent)
            .write_bytes_atomic(Path::new(file_name), bytes, Durability::Durable)
            .map_err(|error| error.to_string())
    }

    fn capture_timeline_storage_row(
        request: &QualificationDerivedChangeReadRunRequestV1,
        endpoint: &InspectorEndpoint,
        phase: QualificationDerivedChangeStoragePhaseV1,
        product_identity_sha256: &str,
        execution_identity_sha256: &str,
        fixture_inventory_sha256: &str,
    ) -> Result<QualificationDerivedTimelineStorageEvidenceV1, String> {
        let (status, response) = endpoint.json("/api/v2/history?limit=2&order=asc")?;
        let response_document =
            String::from_utf8(canonical_json_bytes(&response).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        let (prose, payload_document, trust_result, continuation_token) = if status == 200 {
            let (_, prose_response) = endpoint.json("/api/v2/history?limit=100&order=asc")?;
            let entry = prose_response
                .get("entries")
                .and_then(Value::as_array)
                .and_then(|entries| {
                    entries.iter().find(|entry| {
                        entry
                            .get("summary")
                            .and_then(first_timeline_prose)
                            .is_some()
                    })
                })
                .ok_or_else(|| {
                    format!("{phase:?} Timeline storage probe response omitted a prose entry")
                })?;
            let summary = entry.get("summary").ok_or_else(|| {
                format!("{phase:?} Timeline storage probe response omitted a summary")
            })?;
            let payload_document = String::from_utf8(
                canonical_json_bytes(summary).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            let prose = first_timeline_prose(summary).ok_or_else(|| {
                format!("{phase:?} Timeline storage probe response omitted prose")
            })?;
            let trust_result = String::from_utf8(
                canonical_json_bytes(&json!({
                    "eventId": entry.get("eventId"),
                    "verificationStatus": entry.get("verificationStatus"),
                }))
                .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            let continuation_token = response
                .get("next")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    format!("{phase:?} Timeline storage probe response omitted continuation")
                })?
                .to_owned();
            (
                prose,
                payload_document,
                trust_result,
                Some(continuation_token),
            )
        } else {
            let prose = response
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{phase:?} Timeline fault storage probe omitted prose"))?
                .to_owned();
            let trust_result = response
                .get("code")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{phase:?} Timeline fault storage probe omitted code"))?
                .to_owned();
            (prose, response_document.clone(), trust_result, None)
        };
        let sentinels = [prose, payload_document, response_document, trust_result];
        let first = QualificationDerivedStorageForbiddenProbeInputV1::new(
            &sentinels[0],
            &sentinels[1],
            &sentinels[2],
            &sentinels[3],
        )?;
        let store_root =
            store_dir_for_repo(&request.repository).map_err(|error| error.to_string())?;
        // This row RECORDS retention as evidence for the evaluator to judge;
        // it must never reuse the fail-closed fixture-probe capture, whose
        // found-probe rejection makes a recorded hit unrepresentable. The
        // scan classifies hits into SQLite-body versus file carriers and the
        // error path names this row's phase, which the shared capture cannot.
        let (generation_sequence, first_matches) =
            scan_qualification_derived_storage_v1(&store_root, &first)
                .map_err(|error| format!("{phase:?} timeline storage scan: {error}"))?;
        // Non-evidence diagnostic: a recorded hit costs an evaluator rejection
        // later, so surface the carrier paths and generation now rather than
        // spending another invocation to recover them. Never receipt data.
        for entry in &first_matches {
            if entry.sqlite_carrier_matches > 0 || entry.file_carrier_matches > 0 {
                eprintln!(
                    "{phase:?} timeline storage scan diagnostic at generation sequence \
                     {generation_sequence}: {:?} matched sqlite={} file={} in {:?}",
                    entry.kind,
                    entry.sqlite_carrier_matches,
                    entry.file_carrier_matches,
                    entry.matched_relative_paths,
                );
            }
        }
        let first_counts = first_matches
            .iter()
            .take(4)
            .map(|entry| (entry.sqlite_carrier_matches, entry.file_carrier_matches))
            .collect::<Vec<_>>();
        let continuation_counts = continuation_token
            .as_ref()
            .map(|token| {
                let continuation = QualificationDerivedStorageForbiddenProbeInputV1::new(
                    token, token, token, token,
                )?;
                let (continuation_sequence, matches) =
                    scan_qualification_derived_storage_v1(&store_root, &continuation).map_err(
                        |error| format!("{phase:?} timeline continuation scan: {error}"),
                    )?;
                for entry in &matches {
                    if entry.sqlite_carrier_matches > 0 || entry.file_carrier_matches > 0 {
                        eprintln!(
                            "{phase:?} timeline continuation scan diagnostic at generation \
                             sequence {continuation_sequence}: {:?} matched sqlite={} file={} \
                             in {:?}",
                            entry.kind,
                            entry.sqlite_carrier_matches,
                            entry.file_carrier_matches,
                            entry.matched_relative_paths,
                        );
                    }
                }
                matches
                    .first()
                    .map(|entry| (entry.sqlite_carrier_matches, entry.file_carrier_matches))
                    .ok_or_else(|| {
                        format!("{phase:?} timeline continuation scan returned no probe entries")
                    })
            })
            .transpose()?;
        let forbidden_probes = QualificationDerivedTimelineForbiddenProbeKindV1::ALL
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
                let (sentinel_sha256, (sqlite_match_count, file_match_count)) = if index == 4 {
                    (
                        continuation_token
                            .as_ref()
                            .map(|token| sha256_bytes_hex(token.as_bytes())),
                        continuation_counts.unwrap_or((0, 0)),
                    )
                } else {
                    (
                        Some(sha256_bytes_hex(sentinels[index].as_bytes())),
                        first_counts.get(index).copied().unwrap_or((1, 1)),
                    )
                };
                QualificationDerivedTimelineForbiddenProbeEvidenceV1 {
                    kind,
                    sentinel_sha256,
                    sqlite_match_count,
                    file_match_count,
                }
            })
            .collect();
        Ok(QualificationDerivedTimelineStorageEvidenceV1 {
            platform: request.execution.platform,
            fixture: request.fixture,
            phase,
            fixture_inventory_sha256: fixture_inventory_sha256.to_owned(),
            fixture_witness_sha256: request.fixture_witness_sha256.clone(),
            product_identity_sha256: product_identity_sha256.to_owned(),
            execution_identity_sha256: execution_identity_sha256.to_owned(),
            forbidden_probes,
        })
    }

    /// Select genuine prose from a Timeline entry summary: multi-word human
    /// text. Identity-shaped strings — event-family names, ids, tracks,
    /// digests — contain no spaces and legitimately live in the derived
    /// index, so feeding one as a retention sentinel would make the storage
    /// probe test whether the index contains its own vocabulary.
    fn first_timeline_prose(value: &Value) -> Option<String> {
        match value {
            Value::String(value) if value.len() >= 8 && value.contains(' ') => Some(value.clone()),
            Value::Array(values) => values.iter().find_map(first_timeline_prose),
            Value::Object(values) => values.values().find_map(first_timeline_prose),
            _ => None,
        }
    }

    #[cfg(test)]
    mod timeline_prose_sentinel_tests {
        use super::*;

        #[test]
        fn prose_selection_skips_identity_tokens() {
            let summary = json!({
                "kind": "work_object_proposed",
                "details": {
                    "revisionId": "rev:sha256:0123456789abcdef0123456789abcdef",
                    "track": "agent:matrix-facts",
                    "summary": "Decision continuity matrix",
                },
            });
            assert_eq!(
                first_timeline_prose(&summary).as_deref(),
                Some("Decision continuity matrix"),
                "prose selection must skip event-family, id, and track tokens"
            );
            let identity_only = json!({
                "kind": "work_object_proposed",
                "eventId": "evt:sha256:0123456789abcdef0123456789abcdef",
            });
            assert_eq!(first_timeline_prose(&identity_only), None);
        }
    }

    impl From<String> for SemanticPairFailure {
        fn from(detail: String) -> Self {
            Self {
                detail,
                typed_witness: None,
            }
        }
    }

    fn semantic_pair(
        request: &QualificationDerivedChangeReadRunRequestV1,
        case: QualificationDerivedChangeReadCaseV1,
        summary_query: &str,
        derived: &InspectorEndpoint,
        authoritative: Option<&InspectorEndpoint>,
        expected: &ExpectedFixtureOutcome,
    ) -> Result<SemanticPair, String> {
        semantic_pair_observed(
            request,
            case,
            summary_query,
            derived,
            authoritative,
            expected,
        )
        .map_err(|failure| failure.detail)
    }

    fn semantic_pair_observed(
        request: &QualificationDerivedChangeReadRunRequestV1,
        case: QualificationDerivedChangeReadCaseV1,
        summary_query: &str,
        derived: &InspectorEndpoint,
        authoritative: Option<&InspectorEndpoint>,
        expected: &ExpectedFixtureOutcome,
    ) -> Result<SemanticPair, SemanticPairFailure> {
        let (derived_status, derived_value, derived_consistent) =
            semantic_case(request, case, summary_query, derived)?;
        let observed_code = derived_value
            .get("code")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let typed_document =
            if expected.oracle == QualificationDerivedChangeReadOracleV1::TypedFailure {
                match typed_failure_document(&derived_value, expected) {
                    Ok(document) => Some(document),
                    Err(detail) => {
                        return Err(SemanticPairFailure {
                            detail,
                            typed_witness: Some(Box::new(
                                diagnostic_typed_witness(&derived_value).map_err(|detail| {
                                    SemanticPairFailure {
                                        detail,
                                        typed_witness: None,
                                    }
                                })?,
                            )),
                        });
                    }
                }
            } else {
                None
            };
        let normalize = |value| match expected.oracle {
            QualificationDerivedChangeReadOracleV1::ReadyProfileParity => {
                normalize_ready_profile_semantic(value)
            }
            _ => Ok(normalize_change_semantic(value)),
        };
        let derived_normalized = normalize(derived_value)?;
        let derived_bytes =
            canonical_json_bytes(&derived_normalized).map_err(|error| error.to_string())?;
        let derived_sha256 = sha256_bytes_hex(&derived_bytes);
        let (strict_sha256, strict_http_status, strict_code, wire_contract_matches) =
            match expected.oracle {
                QualificationDerivedChangeReadOracleV1::StrictParity
                | QualificationDerivedChangeReadOracleV1::ReadyProfileParity => {
                    let authoritative = authoritative.ok_or_else(|| {
                        "authoritative Change oracle omitted its strict child".to_owned()
                    })?;
                    let (strict_status, strict_value, strict_consistent) =
                        semantic_case(request, case, summary_query, authoritative)?;
                    let strict_normalized = normalize(strict_value)?;
                    let strict_code = strict_normalized
                        .get("code")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let strict_bytes = canonical_json_bytes(&strict_normalized)
                        .map_err(|error| error.to_string())?;
                    let strict_sha256 = sha256_bytes_hex(&strict_bytes);
                    (
                        Some(strict_sha256.clone()),
                        Some(strict_status),
                        strict_code,
                        derived_status == expected.http_status
                            && strict_status == expected.http_status
                            && observed_code.as_deref() == expected.code
                            && derived_consistent
                            && strict_consistent
                            && strict_sha256 == derived_sha256,
                    )
                }
                QualificationDerivedChangeReadOracleV1::TypedFailure => (
                    None,
                    None,
                    None,
                    authoritative.is_none()
                        && derived_status == expected.http_status
                        && observed_code.as_deref() == expected.code
                        && derived_consistent,
                ),
            };
        Ok(SemanticPair {
            strict_sha256,
            strict_http_status,
            strict_code,
            derived_sha256,
            wire_contract_matches,
            observed_http_status: derived_status,
            observed_code,
            typed_document,
        })
    }

    fn typed_failure_document(
        value: &Value,
        expected: &ExpectedFixtureOutcome,
    ) -> Result<QualificationDerivedChangeTypedDocumentV1, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "typed Change failure is not a document".to_owned())?;
        let schema = object
            .get("schema")
            .and_then(Value::as_str)
            .ok_or_else(|| "typed Change failure omitted schema".to_owned())?;
        let version = object
            .get("version")
            .and_then(Value::as_u64)
            .and_then(|version| u32::try_from(version).ok())
            .ok_or_else(|| "typed Change failure omitted version".to_owned())?;
        let code = object
            .get("code")
            .and_then(Value::as_str)
            .ok_or_else(|| "typed Change failure omitted code".to_owned())?;
        let message = object
            .get("message")
            .and_then(Value::as_str)
            .ok_or_else(|| "typed Change failure omitted message".to_owned())?;
        let retryable = object.get("retryable").and_then(Value::as_bool);
        let expected_schema = if code == "stale_projection" {
            "pointbreak.inspect-change-page-error"
        } else {
            "pointbreak.inspect-change-projection-error"
        };
        let expected_keys = if retryable.is_some() {
            BTreeSet::from(["code", "message", "retryable", "schema", "version"])
        } else {
            BTreeSet::from(["code", "message", "schema", "version"])
        };
        let observed_keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
        if schema != expected_schema
            || version != 1
            || code != expected.code.unwrap_or_default()
            || message.trim().is_empty()
            || retryable.is_some_and(|retryable| retryable)
            || observed_keys != expected_keys
        {
            return Err("typed Change failure document drifted".to_owned());
        }
        let canonical_sha256 = canonical_json_bytes(value)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())?;
        Ok(QualificationDerivedChangeTypedDocumentV1 {
            schema: schema.to_owned(),
            version,
            code: code.to_owned(),
            retryable,
            canonical_sha256,
        })
    }

    fn diagnostic_typed_witness(
        value: &Value,
    ) -> Result<DerivedChangeReadDiagnosticTypedWitnessV1, String> {
        let object = value.as_object();
        let canonical_sha256 = canonical_json_bytes(value)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())?;
        Ok(DerivedChangeReadDiagnosticTypedWitnessV1 {
            schema: object
                .and_then(|document| document.get("schema"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            version: object
                .and_then(|document| document.get("version"))
                .and_then(Value::as_u64),
            code: object
                .and_then(|document| document.get("code"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            retryable: object
                .and_then(|document| document.get("retryable"))
                .and_then(Value::as_bool),
            key_set: object
                .map(|document| document.keys().cloned().collect())
                .unwrap_or_default(),
            canonical_sha256,
        })
    }

    fn diagnostic_expected_typed_witness(
        document: &QualificationDerivedChangeTypedDocumentV1,
    ) -> DerivedChangeReadDiagnosticTypedWitnessV1 {
        let mut key_set = vec![
            "code".to_owned(),
            "message".to_owned(),
            "schema".to_owned(),
            "version".to_owned(),
        ];
        if document.retryable.is_some() {
            key_set.push("retryable".to_owned());
            key_set.sort();
        }
        DerivedChangeReadDiagnosticTypedWitnessV1 {
            schema: Some(document.schema.clone()),
            version: Some(u64::from(document.version)),
            code: Some(document.code.clone()),
            retryable: document.retryable,
            key_set,
            canonical_sha256: document.canonical_sha256.clone(),
        }
    }

    fn strict_diagnostic_witness(
        semantic: &SemanticPair,
        expected: &ExpectedFixtureOutcome,
    ) -> Result<DerivedChangeReadDiagnosticFailureWitnessV1, String> {
        Ok(DerivedChangeReadDiagnosticFailureWitnessV1::StrictParity {
            derived: DerivedChangeReadDiagnosticSemanticWitnessV1 {
                http_status: semantic.observed_http_status,
                code: semantic.observed_code.clone(),
                normalized_document_sha256: semantic.derived_sha256.clone(),
            },
            strict: DerivedChangeReadDiagnosticSemanticWitnessV1 {
                http_status: semantic
                    .strict_http_status
                    .ok_or_else(|| "strict diagnostic witness omitted HTTP status".to_owned())?,
                code: semantic.strict_code.clone(),
                normalized_document_sha256: semantic
                    .strict_sha256
                    .clone()
                    .ok_or_else(|| "strict diagnostic witness omitted document hash".to_owned())?,
            },
            expected_http_status: expected.http_status,
            expected_code: expected.code.map(str::to_owned),
        })
    }

    fn fresh_process_semantic_pair(
        request: &QualificationDerivedChangeReadRunRequestV1,
        case: QualificationDerivedChangeReadCaseV1,
        expected: &ExpectedFixtureOutcome,
    ) -> Result<SemanticPair, String> {
        let derived = InspectorChild::spawn(request, "sqlite-wal-bodyless-v1")?;
        derived.ensure_ready()?;
        let authoritative = InspectorChild::spawn(request, "off")?;
        semantic_pair(
            request,
            case,
            &request.summary_query,
            &derived.endpoint,
            Some(&authoritative.endpoint),
            expected,
        )
    }

    fn post_append_semantic_pair(
        request: &QualificationDerivedChangeReadRunRequestV1,
        derived: &InspectorEndpoint,
        authoritative: &InspectorEndpoint,
    ) -> Result<(SemanticPair, String), String> {
        let (_, before, before_consistent) = process_suite_semantic(derived, false)?;
        let before_stamp = before
            .pointer("/changes/projectionStamp")
            .and_then(Value::as_str)
            .ok_or_else(|| "post-append Change suite omitted its initial stamp".to_owned())?
            .to_owned();
        let before_event_count = before
            .pointer("/profile/authorityCursor/eventCount")
            .and_then(Value::as_u64)
            .ok_or_else(|| "post-append Change suite omitted its initial cursor".to_owned())?;
        let stale_token = before
            .pointer("/changes/next")
            .and_then(Value::as_str)
            .ok_or_else(|| "post-append Change suite omitted its signed continuation".to_owned())?
            .to_owned();

        let generation_sha256 = append_governed_qualification_event(
            request,
            QualificationDerivedChangeReadCaseV1::PostAppendSuite,
        )?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let (derived_after, derived_consistent) = loop {
            let (_, candidate, consistent) = process_suite_semantic(derived, false)?;
            if consistent && post_append_has_advanced(&candidate, &before_stamp, before_event_count)
            {
                break (candidate, consistent);
            }
            if Instant::now() >= deadline {
                return Err(
                    "post-append Change suite did not publish its advanced stamp and cursor"
                        .to_owned(),
                );
            }
            thread::sleep(Duration::from_millis(20));
        };
        let (_, strict_after, strict_consistent) = process_suite_semantic(authoritative, false)?;
        let after_stamp = derived_after
            .pointer("/changes/projectionStamp")
            .and_then(Value::as_str)
            .ok_or_else(|| "post-append Change suite omitted its advanced stamp".to_owned())?;
        let after_event_count = derived_after
            .pointer("/profile/authorityCursor/eventCount")
            .and_then(Value::as_u64)
            .ok_or_else(|| "post-append Change suite omitted its advanced cursor".to_owned())?;
        let (stale_status, stale_body) = derived.json(&format!(
            "/api/v2/changes?limit=2&after={}",
            percent_encode(&stale_token)
        ))?;
        let stale_consistent = stale_status == 409
            && stale_body.get("schema").and_then(Value::as_str)
                == Some("pointbreak.inspect-change-page-error")
            && stale_body.get("version").and_then(Value::as_u64) == Some(1)
            && stale_body.get("code").and_then(Value::as_str) == Some("stale_projection");

        let derived_envelope = json!({
            "after": derived_after,
            "stale": {"status": stale_status, "body": stale_body},
            "generationSha256": generation_sha256,
        });
        let strict_envelope = json!({
            "after": strict_after,
            "stale": derived_envelope["stale"].clone(),
            "generationSha256": generation_sha256,
        });
        let derived_normalized = normalize_change_semantic(derived_envelope);
        let strict_normalized = normalize_change_semantic(strict_envelope);
        let derived_sha256 = sha256_bytes_hex(
            &canonical_json_bytes(&derived_normalized).map_err(|error| error.to_string())?,
        );
        let strict_sha256 = sha256_bytes_hex(
            &canonical_json_bytes(&strict_normalized).map_err(|error| error.to_string())?,
        );
        let consistent = before_consistent
            && derived_consistent
            && strict_consistent
            && stale_consistent
            && after_stamp != before_stamp
            && after_event_count == before_event_count.saturating_add(1)
            && derived_sha256 == strict_sha256;
        Ok((
            SemanticPair {
                strict_sha256: Some(strict_sha256),
                strict_http_status: Some(200),
                strict_code: None,
                derived_sha256,
                wire_contract_matches: consistent,
                observed_http_status: 200,
                observed_code: None,
                typed_document: None,
            },
            generation_sha256,
        ))
    }

    pub(super) fn post_append_has_advanced(
        value: &Value,
        before_stamp: &str,
        before_event_count: u64,
    ) -> bool {
        value
            .pointer("/changes/projectionStamp")
            .and_then(Value::as_str)
            .is_some_and(|stamp| stamp != before_stamp)
            && value
                .pointer("/profile/authorityCursor/eventCount")
                .and_then(Value::as_u64)
                == Some(before_event_count.saturating_add(1))
    }

    fn append_governed_qualification_event(
        request: &QualificationDerivedChangeReadRunRequestV1,
        case: QualificationDerivedChangeReadCaseV1,
    ) -> Result<String, String> {
        let (journal_id, occurred_at, label) = governed_qualification_event_identity(case)?;
        let lifecycle = qualification_change_lifecycle(request)?;
        let before_generation = lifecycle
            .published_generation_id()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "post-append Change suite has no published generation".to_owned())?;
        let journal_id = crate::model::JournalId::new(journal_id);
        let event = ShoreEvent::new(
            EventType::ReviewInitialized,
            ReviewInitializedPayload::idempotency_key(&journal_id),
            EventTarget::for_journal(journal_id),
            Writer::shore_local("qualification-change-read"),
            ReviewInitializedPayload {},
            occurred_at,
        )
        .map_err(|error| error.to_string())?;
        let coordinator = DerivedWriteCoordinator::new_for_qualification(lifecycle.clone())
            .map_err(|error| error.to_string())?;
        let store = EventStore::open(lifecycle.store_root()).with_coordinator(coordinator);
        if store
            .record_event_once_for_qualification(&event)
            .map_err(|error| error.to_string())?
            != EventWriteOutcome::Created
        {
            return Err(format!("{label} Change event was not newly admitted"));
        }
        repeat_governed_qualification_event_until_current(&store, &lifecycle, &event, label)?;
        let after_generation = lifecycle
            .published_generation_id()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "post-append Change suite lost its published generation".to_owned())?;
        if after_generation != before_generation {
            return Err("governed Change append replaced its immutable generation".to_owned());
        }
        Ok(sha256_bytes_hex(before_generation.as_bytes()))
    }

    fn qualification_change_lifecycle(
        request: &QualificationDerivedChangeReadRunRequestV1,
    ) -> Result<DerivedAccessLifecycle, String> {
        let store_root =
            store_dir_for_repo(&request.repository).map_err(|error| error.to_string())?;
        let store_identity =
            opaque_path_identity("store", &store_root).map_err(|error| error.to_string())?;
        DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            &store_root,
            store_identity,
        )
        .map_err(|error| error.to_string())
    }

    fn repeat_governed_qualification_event_until_current(
        store: &EventStore,
        lifecycle: &DerivedAccessLifecycle,
        event: &ShoreEvent,
        label: &str,
    ) -> Result<(), String> {
        const RECOVERY_ATTEMPTS: usize = 32;
        let mut last_availability = None;
        for attempt in 0..RECOVERY_ATTEMPTS {
            if store
                .record_event_once_for_qualification(event)
                .map_err(|error| error.to_string())?
                != EventWriteOutcome::Existing
            {
                return Err(format!("repeated {label} Change event was not idempotent"));
            }
            let status = lifecycle.status().map_err(|error| error.to_string())?;
            if status.availability == DerivedAccessAvailability::Current {
                return Ok(());
            }
            last_availability = Some(status.availability);
            if attempt + 1 < RECOVERY_ATTEMPTS {
                thread::sleep(Duration::from_millis(20));
            }
        }
        Err(format!(
            "repeated {label} Change event left its generation {:?}",
            last_availability.unwrap_or(DerivedAccessAvailability::Unavailable)
        ))
    }

    fn governed_qualification_event_identity(
        case: QualificationDerivedChangeReadCaseV1,
    ) -> Result<(&'static str, &'static str, &'static str), String> {
        match case {
            QualificationDerivedChangeReadCaseV1::StalePageToken => Ok((
                "journal:qualification-change-stale-token",
                "2026-08-10T04:29:00Z",
                "stale-token",
            )),
            QualificationDerivedChangeReadCaseV1::PostAppendSuite => Ok((
                "journal:qualification-change-post-append",
                "2026-08-10T04:30:00Z",
                "post-append",
            )),
            _ => Err("unsupported governed Change qualification event".to_owned()),
        }
    }

    fn semantic_case(
        request: &QualificationDerivedChangeReadRunRequestV1,
        case: QualificationDerivedChangeReadCaseV1,
        summary_query: &str,
        endpoint: &InspectorEndpoint,
    ) -> Result<(u16, Value, bool), String> {
        match case {
            QualificationDerivedChangeReadCaseV1::Profile => endpoint
                .json("/api/v2/profile")
                .map(|(status, value)| (status, value, true)),
            QualificationDerivedChangeReadCaseV1::ChangesBare => endpoint
                .json("/api/v2/changes")
                .map(|(status, value)| (status, value, true)),
            QualificationDerivedChangeReadCaseV1::ChangesBounded => endpoint
                .json("/api/v2/changes?limit=2")
                .map(|(status, value)| (status, value, true)),
            QualificationDerivedChangeReadCaseV1::AttentionBare => endpoint
                .json("/api/v2/attention")
                .map(|(status, value)| (status, value, true)),
            QualificationDerivedChangeReadCaseV1::AttentionBounded => endpoint
                .json("/api/v2/attention?limit=2")
                .map(|(status, value)| (status, value, true)),
            QualificationDerivedChangeReadCaseV1::BodylessFilterSuite => {
                bodyless_filter_semantic(endpoint)
            }
            QualificationDerivedChangeReadCaseV1::SummaryQuery => endpoint
                .json(&format!(
                    "/api/v2/changes?limit=2&q={}",
                    percent_encode(summary_query)
                ))
                .map(|(status, value)| (status, value, true)),
            QualificationDerivedChangeReadCaseV1::SummaryFilterSuite => {
                summary_filter_semantic(endpoint, summary_query)
            }
            QualificationDerivedChangeReadCaseV1::PageTokenSuite => paged_semantic(endpoint),
            QualificationDerivedChangeReadCaseV1::ConcurrentReaders => {
                concurrent_semantic(endpoint)
            }
            QualificationDerivedChangeReadCaseV1::FreshProcessSuite => {
                process_suite_semantic(endpoint, false)
            }
            QualificationDerivedChangeReadCaseV1::PostAppendFreshProcessSuite => {
                process_suite_semantic(endpoint, false)
            }
            QualificationDerivedChangeReadCaseV1::WarmReuseSuite => {
                process_suite_semantic(endpoint, true)
            }
            QualificationDerivedChangeReadCaseV1::StalePageToken => {
                stale_page_token_semantic(request, endpoint)
            }
            QualificationDerivedChangeReadCaseV1::PostAppendSuite => {
                unreachable!("post-append semantics require both children and governed mutation")
            }
        }
    }

    fn process_suite_semantic(
        endpoint: &InspectorEndpoint,
        repeat: bool,
    ) -> Result<(u16, Value, bool), String> {
        fn read(endpoint: &InspectorEndpoint) -> Result<(Value, bool), String> {
            let (profile_status, profile) = endpoint.json("/api/v2/profile")?;
            let (changes_status, changes) = endpoint.json("/api/v2/changes?limit=2")?;
            let (attention_status, attention) = endpoint.json("/api/v2/attention?limit=2")?;
            Ok((
                json!({
                    "profile": profile,
                    "changes": changes,
                    "attention": attention,
                }),
                profile_status == 200 && changes_status == 200 && attention_status == 200,
            ))
        }

        let (first, first_consistent) = read(endpoint)?;
        if !repeat {
            return Ok((200, first, first_consistent));
        }
        let (second, second_consistent) = read(endpoint)?;
        let equal =
            normalize_change_semantic(first.clone()) == normalize_change_semantic(second.clone());
        Ok((
            200,
            json!({"first": first, "second": second}),
            first_consistent && second_consistent && equal,
        ))
    }

    fn bodyless_filter_semantic(
        endpoint: &InspectorEndpoint,
    ) -> Result<(u16, Value, bool), String> {
        let mut responses = Vec::new();
        let mut consistent = true;
        for lens in ["changes", "attention"] {
            for (field, values) in CHANGE_FILTER_VALUES {
                for value in *values {
                    let target = format!("/api/v2/{lens}?limit=100&{field}={value}");
                    let (status, body) = endpoint.json(&target)?;
                    consistent &= status == 200;
                    responses.push(json!({
                        "lens": lens,
                        "filter": field,
                        "value": value,
                        "status": status,
                        "body": body,
                    }));
                }
            }
        }
        Ok((200, Value::Array(responses), consistent))
    }

    const CHANGE_FILTER_VALUES: &[(&str, &[&str])] = &[
        (
            "topology",
            &[
                "initial",
                "replacement",
                "replacement_divergent",
                "consolidation",
                "parallel_current",
                "mixed",
                "incomplete",
                "cycle_conflicted",
            ],
        ),
        (
            "lifecycle",
            &["incomplete", "conflicted", "in_progress", "accepted"],
        ),
        (
            "attention",
            &["clear", "in_progress", "incomplete", "conflicted"],
        ),
        ("availability", &["available", "incomplete"]),
    ];

    fn summary_filter_semantic(
        endpoint: &InspectorEndpoint,
        summary_query: &str,
    ) -> Result<(u16, Value, bool), String> {
        let encoded_query = percent_encode(summary_query);
        let mut responses = Vec::new();
        let mut consistent = true;
        for lens in ["changes", "attention"] {
            let (status, body) =
                endpoint.json(&format!("/api/v2/{lens}?limit=100&q={encoded_query}"))?;
            consistent &= status == 200;
            responses.push(json!({
                "lens": lens,
                "filter": null,
                "value": null,
                "status": status,
                "body": body,
            }));
            for (field, values) in CHANGE_FILTER_VALUES {
                for value in *values {
                    let target =
                        format!("/api/v2/{lens}?limit=100&q={encoded_query}&{field}={value}");
                    let (status, body) = endpoint.json(&target)?;
                    consistent &= status == 200;
                    responses.push(json!({
                        "lens": lens,
                        "filter": field,
                        "value": value,
                        "status": status,
                        "body": body,
                    }));
                }
            }
        }
        Ok((200, Value::Array(responses), consistent))
    }

    fn paged_semantic(endpoint: &InspectorEndpoint) -> Result<(u16, Value, bool), String> {
        let (first_status, first) = endpoint.json("/api/v2/changes?limit=2")?;
        let next = first
            .get("next")
            .and_then(Value::as_str)
            .ok_or_else(|| "Change fixture first page omitted its next token".to_owned())?;
        let last = first
            .get("last")
            .and_then(Value::as_str)
            .ok_or_else(|| "Change fixture first page omitted its last token".to_owned())?;
        let (second_status, second) = endpoint.json(&format!(
            "/api/v2/changes?limit=2&after={}",
            percent_encode(next)
        ))?;
        let (last_status, last_page) = endpoint.json(&format!(
            "/api/v2/changes?limit=2&after={}",
            percent_encode(last)
        ))?;
        let previous = last_page
            .get("previous")
            .and_then(Value::as_str)
            .ok_or_else(|| "Change fixture last page omitted its previous token".to_owned())?;
        let (previous_status, previous_page) = endpoint.json(&format!(
            "/api/v2/changes?limit=2&after={}",
            percent_encode(previous)
        ))?;

        let mut tampered = next.to_owned();
        let replacement = if tampered.ends_with('A') { 'B' } else { 'A' };
        tampered.pop();
        tampered.push(replacement);
        let (tampered_status, tampered_body) = endpoint.json(&format!(
            "/api/v2/changes?limit=2&after={}",
            percent_encode(&tampered)
        ))?;
        let (cross_lens_status, cross_lens_body) = endpoint.json(&format!(
            "/api/v2/attention?limit=2&after={}",
            percent_encode(next)
        ))?;
        let (mismatched_query_status, mismatched_query_body) = endpoint.json(&format!(
            "/api/v2/changes?limit=2&q=other&after={}",
            percent_encode(next)
        ))?;
        let invalid_query = |status: u16, body: &Value| {
            status == 400 && body.get("code").and_then(Value::as_str) == Some("invalid_query")
        };
        let consistent = first_status == 200
            && second_status == 200
            && last_status == 200
            && previous_status == 200
            && last_page.get("next").is_some_and(Value::is_null)
            && invalid_query(tampered_status, &tampered_body)
            && invalid_query(cross_lens_status, &cross_lens_body)
            && invalid_query(mismatched_query_status, &mismatched_query_body);
        Ok((
            200,
            json!({
                "absent": first,
                "next": second,
                "last": last_page,
                "previous": previous_page,
                "tampered": {"status": tampered_status, "body": tampered_body},
                "crossLens": {"status": cross_lens_status, "body": cross_lens_body},
                "mismatchedQuery": {
                    "status": mismatched_query_status,
                    "body": mismatched_query_body,
                },
            }),
            consistent,
        ))
    }

    fn stale_page_token_semantic(
        request: &QualificationDerivedChangeReadRunRequestV1,
        endpoint: &InspectorEndpoint,
    ) -> Result<(u16, Value, bool), String> {
        let (first_status, first) = endpoint.json("/api/v2/changes?limit=2")?;
        if first_status != 200 {
            return Ok((first_status, first, false));
        }
        let token = first
            .get("next")
            .and_then(Value::as_str)
            .ok_or_else(|| "Change fixture stale-token page omitted its next token".to_owned())?
            .to_owned();
        let initial_stamp = first
            .get("projectionStamp")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                "Change fixture stale-token page omitted its projection stamp".to_owned()
            })?
            .to_owned();
        append_governed_qualification_event(
            request,
            QualificationDerivedChangeReadCaseV1::StalePageToken,
        )?;

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let (status, current) = endpoint.json("/api/v2/changes?limit=2")?;
            let current_stamp = current.get("projectionStamp").and_then(Value::as_str);
            if status == 200 && current_stamp.is_some_and(|stamp| stamp != initial_stamp) {
                break;
            }
            if Instant::now() >= deadline {
                return Err(
                    "derived Change projection stamp did not advance for stale-token proof"
                        .to_owned(),
                );
            }
            thread::sleep(Duration::from_millis(20));
        }

        let (status, body) = endpoint.json(&format!(
            "/api/v2/changes?limit=2&after={}",
            percent_encode(&token)
        ))?;
        let consistent = status == 409
            && body.get("schema").and_then(Value::as_str)
                == Some("pointbreak.inspect-change-page-error")
            && body.get("version").and_then(Value::as_u64) == Some(1)
            && body.get("code").and_then(Value::as_str) == Some("stale_projection");
        Ok((status, body, consistent))
    }

    fn concurrent_semantic(endpoint: &InspectorEndpoint) -> Result<(u16, Value, bool), String> {
        let mut joins = Vec::new();
        for _ in 0..4 {
            let endpoint = endpoint.clone();
            joins.push(thread::spawn(move || {
                endpoint.json("/api/v2/changes?limit=2")
            }));
        }
        let mut responses = Vec::new();
        for join in joins {
            responses.push(
                join.join()
                    .map_err(|_| "concurrent Inspector reader panicked".to_owned())??,
            );
        }
        let (status, value) = responses
            .first()
            .cloned()
            .ok_or_else(|| "concurrent Inspector reader set is empty".to_owned())?;
        let normalized = normalize_change_semantic(value.clone());
        let consistent = responses.iter().all(|(candidate_status, candidate)| {
            *candidate_status == status
                && normalize_change_semantic(candidate.clone()) == normalized
        });
        Ok((status, value, consistent))
    }

    pub(super) fn normalize_change_semantic(mut value: Value) -> Value {
        fn visit(value: &mut Value) {
            match value {
                Value::Array(values) => {
                    for value in values {
                        visit(value);
                    }
                }
                Value::Object(object) => {
                    object.remove("projectionStamp");
                    for field in ["previous", "next", "last"] {
                        if object.get(field).is_some_and(Value::is_string) {
                            object.insert(
                                field.to_owned(),
                                Value::String("<signed-token>".to_owned()),
                            );
                        }
                    }
                    for value in object.values_mut() {
                        visit(value);
                    }
                }
                _ => {}
            }
        }
        visit(&mut value);
        value
    }

    pub(super) fn normalize_ready_profile_semantic(value: Value) -> Result<Value, String> {
        let mut value = normalize_change_semantic(value);
        let cursor = value
            .get_mut("authorityCursor")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "ready Profile omitted its authority cursor".to_owned())?;
        for field in ["eventSetHash", "journalRecordSetHash"] {
            if !cursor.get(field).is_some_and(Value::is_string) {
                return Err(format!("ready Profile authority cursor omitted {field}"));
            }
            cursor.insert(
                field.to_owned(),
                Value::String("<live-set-hash>".to_owned()),
            );
        }
        Ok(value)
    }

    struct MeasuredCase {
        counters: LongitudinalCountersV1,
        expected_typed_document: Option<QualificationDerivedChangeTypedDocumentV1>,
    }

    fn measure_case(
        access: &DerivedChangeAccess,
        repository: &Path,
        case: QualificationDerivedChangeReadCaseV1,
        summary_query: &str,
        expected: &ExpectedFixtureOutcome,
    ) -> Result<MeasuredCase, String> {
        let run_identity = sha256_bytes_hex(format!("derived-change-read:{case:?}").as_bytes());
        let scope = LongitudinalCountingScopeV1::new(run_identity)?;
        if expected.oracle == QualificationDerivedChangeReadOracleV1::TypedFailure {
            let guard = scope.enter();
            let expected_typed_document = match case {
                QualificationDerivedChangeReadCaseV1::Profile => validate_typed_outcome(
                    access.profile().map_err(|error| error.to_string())?,
                    expected,
                )?,
                QualificationDerivedChangeReadCaseV1::ChangesBare => validate_typed_outcome(
                    access
                        .changes(&DerivedChangePageRequestV1::Bare)
                        .map_err(|error| error.to_string())?,
                    expected,
                )?,
                QualificationDerivedChangeReadCaseV1::ChangesBounded => validate_typed_outcome(
                    access
                        .changes(&bounded_request(None, None)?)
                        .map_err(|error| error.to_string())?,
                    expected,
                )?,
                QualificationDerivedChangeReadCaseV1::AttentionBare => validate_typed_outcome(
                    access
                        .attention(&DerivedChangePageRequestV1::Bare)
                        .map_err(|error| error.to_string())?,
                    expected,
                )?,
                QualificationDerivedChangeReadCaseV1::AttentionBounded => validate_typed_outcome(
                    access
                        .attention(&bounded_request(None, None)?)
                        .map_err(|error| error.to_string())?,
                    expected,
                )?,
                QualificationDerivedChangeReadCaseV1::SummaryQuery => validate_typed_outcome(
                    access
                        .changes(&bounded_request(Some(summary_query.to_owned()), None)?)
                        .map_err(|error| error.to_string())?,
                    expected,
                )?,
                QualificationDerivedChangeReadCaseV1::StalePageToken => {
                    let first = require_change_page(
                        access
                            .changes(&bounded_request(None, None)?)
                            .map_err(|error| error.to_string())?,
                    )?;
                    let next = first.window.and_then(|window| window.next).ok_or_else(|| {
                        "bounded derived Change page omitted its stale-token boundary".to_owned()
                    })?;
                    let stale = DerivedChangePageContinuationV1::new("sha256:stale", next)
                        .map_err(|error| error.to_string())?;
                    let outcome = access
                        .changes(&bounded_request(None, Some(stale))?)
                        .map_err(|error| error.to_string())?;
                    validate_typed_outcome_status(&outcome, expected)?;
                    expected_stale_page_document()?
                }
                _ => {
                    return Err(
                        "typed-failure Change fixture requested an unsupported probe".to_owned(),
                    );
                }
            };
            drop(guard);
            return Ok(MeasuredCase {
                counters: scope.snapshot().counters,
                expected_typed_document: Some(expected_typed_document),
            });
        }

        if matches!(
            case,
            QualificationDerivedChangeReadCaseV1::FreshProcessSuite
                | QualificationDerivedChangeReadCaseV1::PostAppendFreshProcessSuite
        ) {
            let fresh = DerivedChangeAccess::resolve_for_inspector(repository)
                .map_err(|error| error.to_string())?;
            if !fresh.is_active() {
                return Err(
                    "fresh Change qualification adapter resolved explicit-off state".to_owned(),
                );
            }
            let guard = scope.enter();
            measure_process_suite(&fresh)?;
            drop(guard);
            return Ok(MeasuredCase {
                counters: scope.snapshot().counters,
                expected_typed_document: None,
            });
        }

        if case == QualificationDerivedChangeReadCaseV1::ConcurrentReaders {
            let mut joins = Vec::new();
            for _ in 0..4 {
                let access = access.clone();
                let scope = scope.clone();
                joins.push(thread::spawn(move || {
                    let _guard = scope.enter();
                    let request = bounded_request(None, None).map_err(|error| error.to_string())?;
                    require_change_page(
                        access
                            .changes(&request)
                            .map_err(|error| error.to_string())?,
                    )?;
                    Ok::<_, String>(())
                }));
            }
            for join in joins {
                join.join()
                    .map_err(|_| "concurrent derived Change reader panicked".to_owned())??;
            }
            return Ok(MeasuredCase {
                counters: scope.snapshot().counters,
                expected_typed_document: None,
            });
        }

        let guard = scope.enter();
        match case {
            QualificationDerivedChangeReadCaseV1::Profile => {
                require_ready(access.profile().map_err(|error| error.to_string())?)?;
            }
            QualificationDerivedChangeReadCaseV1::ChangesBare => {
                require_change_page(
                    access
                        .changes(&DerivedChangePageRequestV1::Bare)
                        .map_err(|error| error.to_string())?,
                )?;
            }
            QualificationDerivedChangeReadCaseV1::ChangesBounded => {
                require_change_page(
                    access
                        .changes(&bounded_request(None, None)?)
                        .map_err(|error| error.to_string())?,
                )?;
            }
            QualificationDerivedChangeReadCaseV1::AttentionBare => {
                require_attention_page(
                    access
                        .attention(&DerivedChangePageRequestV1::Bare)
                        .map_err(|error| error.to_string())?,
                )?;
            }
            QualificationDerivedChangeReadCaseV1::AttentionBounded => {
                require_attention_page(
                    access
                        .attention(&bounded_request(None, None)?)
                        .map_err(|error| error.to_string())?,
                )?;
            }
            QualificationDerivedChangeReadCaseV1::BodylessFilterSuite => {
                measure_bodyless_filter_suite(access)?;
            }
            QualificationDerivedChangeReadCaseV1::SummaryQuery => {
                require_change_page(
                    access
                        .changes(&bounded_request(Some(summary_query.to_owned()), None)?)
                        .map_err(|error| error.to_string())?,
                )?;
            }
            QualificationDerivedChangeReadCaseV1::SummaryFilterSuite => {
                measure_summary_filter_suite(access, summary_query)?;
            }
            QualificationDerivedChangeReadCaseV1::PageTokenSuite => {
                let first = require_change_page(
                    access
                        .changes(&bounded_request(None, None)?)
                        .map_err(|error| error.to_string())?,
                )?;
                let window = first
                    .window
                    .ok_or_else(|| "bounded derived Change page omitted its window".to_owned())?;
                let next = window.next.ok_or_else(|| {
                    "bounded derived Change page omitted its next boundary".to_owned()
                })?;
                let continuation =
                    DerivedChangePageContinuationV1::new(window.projection_stamp, next)
                        .map_err(|error| error.to_string())?;
                require_change_page(
                    access
                        .changes(&bounded_request(None, Some(continuation))?)
                        .map_err(|error| error.to_string())?,
                )?;
            }
            QualificationDerivedChangeReadCaseV1::WarmReuseSuite => {
                measure_process_suite(access)?;
                measure_process_suite(access)?;
            }
            QualificationDerivedChangeReadCaseV1::PostAppendSuite => {
                measure_process_suite(access)?;
            }
            QualificationDerivedChangeReadCaseV1::ConcurrentReaders
            | QualificationDerivedChangeReadCaseV1::FreshProcessSuite
            | QualificationDerivedChangeReadCaseV1::PostAppendFreshProcessSuite
            | QualificationDerivedChangeReadCaseV1::StalePageToken => unreachable!(),
        }
        drop(guard);
        Ok(MeasuredCase {
            counters: scope.snapshot().counters,
            expected_typed_document: None,
        })
    }

    fn measure_process_suite(access: &DerivedChangeAccess) -> Result<(), String> {
        require_ready(access.profile().map_err(|error| error.to_string())?)?;
        require_change_page(
            access
                .changes(&bounded_request(None, None)?)
                .map_err(|error| error.to_string())?,
        )?;
        require_attention_page(
            access
                .attention(&bounded_request(None, None)?)
                .map_err(|error| error.to_string())?,
        )?;
        Ok(())
    }

    fn bounded_request(
        summary_query: Option<String>,
        continuation: Option<DerivedChangePageContinuationV1>,
    ) -> Result<DerivedChangePageRequestV1, String> {
        DerivedChangePageSelectionV1::new(2, continuation, summary_query, None, None, None, None)
            .map(DerivedChangePageRequestV1::Bounded)
            .map_err(|error| error.to_string())
    }

    fn measure_bodyless_filter_suite(access: &DerivedChangeAccess) -> Result<(), String> {
        let topologies = [
            ChangeTopologyV1::Initial,
            ChangeTopologyV1::Replacement,
            ChangeTopologyV1::ReplacementDivergent,
            ChangeTopologyV1::Consolidation,
            ChangeTopologyV1::ParallelCurrent,
            ChangeTopologyV1::Mixed,
            ChangeTopologyV1::Incomplete,
            ChangeTopologyV1::CycleConflicted,
        ];
        let lifecycles = [
            ChangeLifecycleV1::Incomplete,
            ChangeLifecycleV1::Conflicted,
            ChangeLifecycleV1::InProgress,
            ChangeLifecycleV1::Accepted,
        ];
        let attention = [
            DerivedChangeAttentionFilterV1::Clear,
            DerivedChangeAttentionFilterV1::InProgress,
            DerivedChangeAttentionFilterV1::Incomplete,
            DerivedChangeAttentionFilterV1::Conflicted,
        ];
        let availability = [
            DerivedChangeAvailabilityFilterV1::Available,
            DerivedChangeAvailabilityFilterV1::Incomplete,
        ];
        for topology in topologies {
            measure_filter_pair(access, None, Some(topology), None, None, None)?;
        }
        for lifecycle in lifecycles {
            measure_filter_pair(access, None, None, Some(lifecycle), None, None)?;
        }
        for attention in attention {
            measure_filter_pair(access, None, None, None, Some(attention), None)?;
        }
        for availability in availability {
            measure_filter_pair(access, None, None, None, None, Some(availability))?;
        }
        Ok(())
    }

    fn measure_summary_filter_suite(
        access: &DerivedChangeAccess,
        summary_query: &str,
    ) -> Result<(), String> {
        measure_filter_pair(
            access,
            Some(summary_query.to_owned()),
            None,
            None,
            None,
            None,
        )?;
        for topology in [
            ChangeTopologyV1::Initial,
            ChangeTopologyV1::Replacement,
            ChangeTopologyV1::ReplacementDivergent,
            ChangeTopologyV1::Consolidation,
            ChangeTopologyV1::ParallelCurrent,
            ChangeTopologyV1::Mixed,
            ChangeTopologyV1::Incomplete,
            ChangeTopologyV1::CycleConflicted,
        ] {
            measure_filter_pair(
                access,
                Some(summary_query.to_owned()),
                Some(topology),
                None,
                None,
                None,
            )?;
        }
        for lifecycle in [
            ChangeLifecycleV1::Incomplete,
            ChangeLifecycleV1::Conflicted,
            ChangeLifecycleV1::InProgress,
            ChangeLifecycleV1::Accepted,
        ] {
            measure_filter_pair(
                access,
                Some(summary_query.to_owned()),
                None,
                Some(lifecycle),
                None,
                None,
            )?;
        }
        for attention in [
            DerivedChangeAttentionFilterV1::Clear,
            DerivedChangeAttentionFilterV1::InProgress,
            DerivedChangeAttentionFilterV1::Incomplete,
            DerivedChangeAttentionFilterV1::Conflicted,
        ] {
            measure_filter_pair(
                access,
                Some(summary_query.to_owned()),
                None,
                None,
                Some(attention),
                None,
            )?;
        }
        for availability in [
            DerivedChangeAvailabilityFilterV1::Available,
            DerivedChangeAvailabilityFilterV1::Incomplete,
        ] {
            measure_filter_pair(
                access,
                Some(summary_query.to_owned()),
                None,
                None,
                None,
                Some(availability),
            )?;
        }
        Ok(())
    }

    fn measure_filter_pair(
        access: &DerivedChangeAccess,
        summary_query: Option<String>,
        topology: Option<ChangeTopologyV1>,
        lifecycle: Option<ChangeLifecycleV1>,
        attention: Option<DerivedChangeAttentionFilterV1>,
        availability: Option<DerivedChangeAvailabilityFilterV1>,
    ) -> Result<(), String> {
        let selection = DerivedChangePageSelectionV1::new(
            100,
            None,
            summary_query,
            topology,
            lifecycle,
            attention,
            availability,
        )
        .map_err(|error| error.to_string())?;
        let request = DerivedChangePageRequestV1::Bounded(selection);
        require_change_page(
            access
                .changes(&request)
                .map_err(|error| error.to_string())?,
        )?;
        require_attention_page(
            access
                .attention(&request)
                .map_err(|error| error.to_string())?,
        )?;
        Ok(())
    }

    fn validate_typed_outcome<T>(
        outcome: DerivedChangeOutcomeV1<T>,
        expected: &ExpectedFixtureOutcome,
    ) -> Result<QualificationDerivedChangeTypedDocumentV1, String> {
        validate_typed_outcome_status(&outcome, expected)?;
        let value = match outcome {
            DerivedChangeOutcomeV1::Ready(_) => {
                return Err("typed Change outcome unexpectedly remained ready".to_owned());
            }
            DerivedChangeOutcomeV1::AuthorityUnavailable(document) => {
                serde_json::to_value(document)
            }
            DerivedChangeOutcomeV1::AuthorityConflicted(document)
            | DerivedChangeOutcomeV1::AuthorityInvalid(document) => serde_json::to_value(document),
            DerivedChangeOutcomeV1::ReaderUpgradeRequired(document) => {
                serde_json::to_value(document)
            }
            DerivedChangeOutcomeV1::ProjectionUnavailable(document)
            | DerivedChangeOutcomeV1::Retryable(document) => serde_json::to_value(document),
        }
        .map_err(|error| error.to_string())?;
        typed_failure_document(&value, expected)
    }

    fn validate_typed_outcome_status<T>(
        outcome: &DerivedChangeOutcomeV1<T>,
        expected: &ExpectedFixtureOutcome,
    ) -> Result<(), String> {
        let (status, code) = classify_outcome(outcome)?;
        if status != expected.http_status || code.as_deref() != expected.code {
            return Err(format!(
                "derived Change typed outcome drifted: status={status}, code={code:?}"
            ));
        }
        Ok(())
    }

    fn expected_stale_page_document() -> Result<QualificationDerivedChangeTypedDocumentV1, String> {
        let expected = ExpectedFixtureOutcome {
            oracle: QualificationDerivedChangeReadOracleV1::TypedFailure,
            http_status: 409,
            code: Some("stale_projection"),
        };
        typed_failure_document(
            &json!({
                "schema": "pointbreak.inspect-change-page-error",
                "version": 1,
                "code": "stale_projection",
                "message": "continuation belongs to a stale projection",
            }),
            &expected,
        )
    }

    fn classify_outcome<T>(
        outcome: &DerivedChangeOutcomeV1<T>,
    ) -> Result<(u16, Option<String>), String> {
        match outcome {
            DerivedChangeOutcomeV1::Ready(_) => Ok((200, None)),
            DerivedChangeOutcomeV1::AuthorityUnavailable(document) => {
                Ok((409, serialized_code(document)?))
            }
            DerivedChangeOutcomeV1::AuthorityConflicted(document)
            | DerivedChangeOutcomeV1::AuthorityInvalid(document) => {
                Ok((409, serialized_code(document)?))
            }
            DerivedChangeOutcomeV1::ReaderUpgradeRequired(document) => {
                Ok((426, serialized_code(document)?))
            }
            DerivedChangeOutcomeV1::ProjectionUnavailable(document)
                if document.code() == DerivedProjectionFailureCodeV1::ProjectionStale =>
            {
                Ok((409, Some("stale_projection".to_owned())))
            }
            DerivedChangeOutcomeV1::ProjectionUnavailable(document)
            | DerivedChangeOutcomeV1::Retryable(document) => Ok((503, serialized_code(document)?)),
        }
    }

    fn serialized_code(value: &impl Serialize) -> Result<Option<String>, String> {
        serde_json::to_value(value)
            .map_err(|error| error.to_string())
            .map(|value| value.get("code").and_then(Value::as_str).map(str::to_owned))
    }

    fn require_ready<T>(outcome: DerivedChangeOutcomeV1<T>) -> Result<T, String> {
        match outcome {
            DerivedChangeOutcomeV1::Ready(value) => Ok(value),
            DerivedChangeOutcomeV1::AuthorityUnavailable(_) => {
                Err("derived Change read reported unavailable authority".to_owned())
            }
            DerivedChangeOutcomeV1::AuthorityConflicted(_) => {
                Err("derived Change read reported conflicted authority".to_owned())
            }
            DerivedChangeOutcomeV1::AuthorityInvalid(_) => {
                Err("derived Change read reported invalid authority".to_owned())
            }
            DerivedChangeOutcomeV1::ReaderUpgradeRequired(_) => {
                Err("derived Change read required a reader upgrade".to_owned())
            }
            DerivedChangeOutcomeV1::ProjectionUnavailable(_) => {
                Err("derived Change read reported unavailable projection".to_owned())
            }
            DerivedChangeOutcomeV1::Retryable(_) => {
                Err("derived Change read reported retryable projection state".to_owned())
            }
        }
    }

    fn require_change_page(
        outcome: DerivedChangeOutcomeV1<DerivedChangePageV1>,
    ) -> Result<DerivedChangePageV1, String> {
        require_ready(outcome)
    }

    fn require_attention_page(
        outcome: DerivedChangeOutcomeV1<DerivedAttentionPageV1>,
    ) -> Result<DerivedAttentionPageV1, String> {
        require_ready(outcome)
    }

    fn validate_environment(
        request: &QualificationDerivedChangeReadRunRequestV1,
    ) -> Result<(), String> {
        let observed_home = std::env::var_os("POINTBREAK_HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "Change read evidence requires POINTBREAK_HOME".to_owned())?;
        let observed_home =
            std::fs::canonicalize(observed_home).map_err(|error| error.to_string())?;
        let expected_home =
            std::fs::canonicalize(&request.pointbreak_home).map_err(|error| error.to_string())?;
        if observed_home != expected_home
            || std::env::var("POINTBREAK_DERIVED_ACCESS").ok().as_deref()
                != Some("sqlite-wal-bodyless-v1")
        {
            return Err("Change read evidence environment differs from its request".to_owned());
        }
        Ok(())
    }

    fn validate_public_fixture_shape(
        request: &QualificationDerivedChangeReadRunRequestV1,
    ) -> Result<(), String> {
        let repository =
            std::fs::canonicalize(&request.repository).map_err(|error| error.to_string())?;
        let home =
            std::fs::canonicalize(&request.pointbreak_home).map_err(|error| error.to_string())?;
        if home != repository.join(".git/pointbreak-home") {
            return Err(
                "Change read evidence requires the disposable public matrix home".to_owned(),
            );
        }
        let email = Command::new("git")
            .args(["-C"])
            .arg(&repository)
            .args(["config", "user.email"])
            .output()
            .map_err(|error| error.to_string())?;
        if !email.status.success()
            || String::from_utf8_lossy(&email.stdout).trim() != "pointbreak-matrix@example.com"
        {
            return Err("Change read fixture is not the public decision matrix".to_owned());
        }
        let store_root = store_dir_for_repo(&repository).map_err(|error| error.to_string())?;
        let expected_store_root = repository.join(".git/pointbreak");
        if !same_existing_path(&store_root, &expected_store_root)? {
            return Err(
                "Change read evidence requires the disposable shared-store root".to_owned(),
            );
        }
        let events = store_root.join("events");
        let source = request
            .source_checkout
            .join("tests/support/assets/change-ready-store");
        for fixture in [
            QUALIFICATION_DERIVED_CHANGE_ACTIVATION_FIXTURE_V1,
            QUALIFICATION_DERIVED_CHANGE_COMPLETION_FIXTURE_V1,
        ] {
            if sha256_file(&events.join(fixture))? != sha256_file(&source.join(fixture))? {
                return Err("public Change activation fixture drifted".to_owned());
            }
        }
        Ok(())
    }

    fn same_existing_path(left: &Path, right: &Path) -> Result<bool, String> {
        let left = std::fs::canonicalize(left).map_err(|error| error.to_string())?;
        let right = std::fs::canonicalize(right).map_err(|error| error.to_string())?;
        Ok(left == right)
    }

    fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
        let file = File::open(path).map_err(|error| error.to_string())?;
        serde_json::from_reader(file).map_err(|error| error.to_string())
    }

    fn sha256_file(path: &Path) -> Result<String, String> {
        let mut file = File::open(path).map_err(|error| error.to_string())?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    pub(super) fn percent_encode(value: &str) -> String {
        let mut encoded = String::new();
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                encoded.push(char::from(byte));
            } else {
                encoded.push('%');
                encoded.push_str(&format!("{byte:02X}"));
            }
        }
        encoded
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn exact_control_parser_requires_one_named_passing_test() {
            let test_name = "module::tests::exact_control";
            let passing = format!(
                "running 1 test\ntest {test_name} ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out\n"
            );
            assert!(exact_libtest_passed(passing.as_bytes(), test_name).expect("valid output"));

            let interleaved = format!(
                "running 1 test\ntest {test_name} ... warning from the test\nok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured\n"
            );
            assert!(exact_libtest_passed(interleaved.as_bytes(), test_name).expect("valid output"));

            let zero = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out\n";
            assert!(!exact_libtest_passed(zero.as_bytes(), test_name).expect("valid output"));

            let wrong = "running 1 test\ntest module::tests::other ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured\n";
            assert!(!exact_libtest_passed(wrong.as_bytes(), test_name).expect("valid output"));

            let multiple = format!(
                "running 2 tests\ntest {test_name} ... ok\ntest module::tests::other ... ok\n\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured\n"
            );
            assert!(!exact_libtest_passed(multiple.as_bytes(), test_name).expect("valid output"));
        }

        #[test]
        fn existing_path_identity_ignores_equivalent_lexical_spellings() {
            let root = tempfile::tempdir().expect("fixture root");
            let store = root.path().join("store");
            let sibling = root.path().join("sibling");
            std::fs::create_dir_all(&store).expect("store directory");
            std::fs::create_dir_all(&sibling).expect("sibling directory");

            assert!(
                same_existing_path(&store, &sibling.join("..").join("store"))
                    .expect("compare existing paths")
            );
        }

        #[cfg(unix)]
        #[test]
        fn timeline_fault_authority_detects_a_symlink_alias() {
            use std::os::unix::fs::symlink;

            let root = tempfile::tempdir().expect("authority root");
            let authority = root.path().join("authority");
            let alias = root.path().join("authority-alias");
            std::fs::create_dir(&authority).expect("authority directory");
            symlink(&authority, &alias).expect("authority symlink");

            let canonical_authority = canonical_authority_path(&authority, "test authority")
                .expect("canonical authority");
            let canonical_alias =
                canonical_authority_path(&alias, "test alias").expect("canonical alias");
            assert!(paths_overlap(&canonical_authority, &canonical_alias));
            assert!(!paths_overlap(&authority, &alias));
        }

        #[test]
        fn qualification_typed_document_freezes_the_complete_direct_or_page_document() {
            let projection_expected = ExpectedFixtureOutcome {
                oracle: QualificationDerivedChangeReadOracleV1::TypedFailure,
                http_status: 503,
                code: Some("projection_invalid"),
            };
            let first = validate_typed_outcome(
                DerivedChangeOutcomeV1::<()>::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    "first complete failure message",
                ),
                &projection_expected,
            )
            .expect("first direct typed document");
            let second = validate_typed_outcome(
                DerivedChangeOutcomeV1::<()>::projection_unavailable(
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    "second complete failure message",
                ),
                &projection_expected,
            )
            .expect("second direct typed document");
            assert_ne!(first.canonical_sha256, second.canonical_sha256);

            let stale = expected_stale_page_document().expect("frozen stale page document");
            let stale_expected = ExpectedFixtureOutcome {
                oracle: QualificationDerivedChangeReadOracleV1::TypedFailure,
                http_status: 409,
                code: Some("stale_projection"),
            };
            let changed = typed_failure_document(
                &json!({
                    "schema": "pointbreak.inspect-change-page-error",
                    "version": 1,
                    "code": "stale_projection",
                    "message": "different stale projection message",
                }),
                &stale_expected,
            )
            .expect("changed stale page document");
            assert_ne!(stale.canonical_sha256, changed.canonical_sha256);
        }

        #[test]
        fn diagnostic_typed_witness_hashes_but_does_not_retain_the_message() {
            let witness = diagnostic_typed_witness(&json!({
                "schema": "pointbreak.inspect-change-projection-error",
                "version": 1,
                "code": "projection_invalid",
                "message": "raw diagnostic message must not escape",
            }))
            .expect("bounded typed witness");

            assert_eq!(
                witness.key_set,
                ["code", "message", "schema", "version"].map(str::to_owned)
            );
            assert_eq!(witness.canonical_sha256.len(), 64);
            assert!(
                !serde_json::to_string(&witness)
                    .expect("serialize witness")
                    .contains("raw diagnostic message")
            );
        }

        #[test]
        fn stale_token_oracle_uses_a_distinct_governed_append_instead_of_ready_retry() {
            let source = include_str!("change_read.rs");
            let stale_token_body = source
                .split("fn stale_page_token_semantic(")
                .nth(1)
                .and_then(|source| source.split("fn concurrent_semantic(").next())
                .expect("stale-token qualification helper");
            assert!(
                !stale_token_body.contains("/api/derived-access/retry"),
                "a ready retry may preserve the current projection stamp"
            );
            assert!(
                stale_token_body.contains("append_governed_qualification_event"),
                "stale-token proof must advance truth through governed append"
            );

            let append_body = source
                .split("fn append_governed_qualification_event(")
                .nth(1)
                .and_then(|source| source.split("fn semantic_case(").next())
                .expect("governed qualification append helper");
            assert!(append_body.contains("StalePageToken"));
            assert!(append_body.contains("PostAppendSuite"));

            let stale = governed_qualification_event_identity(
                QualificationDerivedChangeReadCaseV1::StalePageToken,
            )
            .expect("stale-token append identity");
            let post_append = governed_qualification_event_identity(
                QualificationDerivedChangeReadCaseV1::PostAppendSuite,
            )
            .expect("post-append suite identity");
            assert_ne!(stale.0, post_append.0);
            assert_ne!(stale.1, post_append.1);
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "longitudinal-counting")]
    use super::instrumented::{
        copy_public_fixture_tree, diagnostic_template_postflight,
        materialize_diagnostic_fixture_at_root, normalize_change_semantic,
        normalize_ready_profile_semantic, normalize_timeline_semantic_checked, percent_encode,
        post_append_has_advanced, replace_unique_bytes, requires_pre_mutation_measurement,
        requires_semantic_fixture_preflight, timeline_exact_revision_target_from_document,
        validate_fixture_authoritative_inventory, validate_topology_fixture_semantics,
    };
    use super::*;

    #[cfg(not(feature = "longitudinal-counting"))]
    #[test]
    fn evidence_mode_refuses_an_uninstrumented_build_before_reading_a_request() {
        let error = run_qualification_derived_change_read_v1(Path::new("missing-request.json"))
            .expect_err("uninstrumented Change evidence must fail closed");
        assert!(error.contains("longitudinal-counting"));
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn semantic_normalization_preserves_shape_and_removes_only_bound_identities() {
        let normalized = normalize_change_semantic(json!({
            "schema": "pointbreak.inspect-changes-page",
            "version": 1,
            "projectionStamp": "sha256:one",
            "next": "signed-one",
            "previous": null,
            "changes": [{
                "changeId": "change:one",
                "projectionStamp": "sha256:one",
                "titleAssertions": []
            }]
        }));
        assert_eq!(
            normalized,
            json!({
                "schema": "pointbreak.inspect-changes-page",
                "version": 1,
                "next": "<signed-token>",
                "previous": null,
                "changes": [{
                    "changeId": "change:one",
                    "titleAssertions": []
                }]
            })
        );

        let mut drifted = normalized.clone();
        drifted["changes"][0]["titleAssertions"] = json!(["changed"]);
        assert_ne!(normalized, drifted);
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn ready_profile_parity_masks_only_live_authority_set_hashes() {
        let profile = json!({
            "schema": "pointbreak.inspect-reader-profile",
            "version": 1,
            "availability": "ready",
            "authorityCursor": {
                "schema": "pointbreak.authority-cursor.v2",
                "journalRecordCount": 98,
                "eventCount": 96,
                "journalRecordSetHash": "sha256:one",
                "eventSetHash": "sha256:two",
                "capabilitySetHash": "sha256:three"
            }
        });
        let mut live = profile.clone();
        live["authorityCursor"]["journalRecordSetHash"] = json!("sha256:four");
        live["authorityCursor"]["eventSetHash"] = json!("sha256:five");

        assert_ne!(
            normalize_change_semantic(profile.clone()),
            normalize_change_semantic(live.clone())
        );
        assert_eq!(
            normalize_ready_profile_semantic(profile).expect("receipt-backed Profile"),
            normalize_ready_profile_semantic(live.clone()).expect("live strict Profile")
        );

        live["authorityCursor"]["eventCount"] = json!(97);
        assert_ne!(
            normalize_ready_profile_semantic(live).expect("drifted Profile"),
            normalize_ready_profile_semantic(json!({
                "schema": "pointbreak.inspect-reader-profile",
                "version": 1,
                "availability": "ready",
                "authorityCursor": {
                    "schema": "pointbreak.authority-cursor.v2",
                    "journalRecordCount": 98,
                    "eventCount": 96,
                    "journalRecordSetHash": "sha256:any",
                    "eventSetHash": "sha256:any",
                    "capabilitySetHash": "sha256:three"
                }
            }))
            .expect("stable Profile")
        );
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn percent_encoding_is_query_component_strict() {
        assert_eq!(percent_encode("matrix / ä"), "matrix%20%2F%20%C3%A4");
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn timeline_exact_revision_target_maps_the_wire_ref_to_query_names() {
        let document = json!({
            "entries": [{
                "revisionRefs": [{
                    "revisionId": "rev:sha256:one",
                    "objectArtifactContentHash": "sha256:two",
                    "artifactHash": "sha256:query-alias-decoy",
                }]
            }]
        });

        assert_eq!(
            timeline_exact_revision_target_from_document(&document).expect("exact Revision target"),
            "/api/v2/history?limit=100&order=asc&revision=rev%3Asha256%3Aone&artifactHash=sha256%3Atwo"
        );

        let mut drifted = document;
        drifted["entries"][0]["revisionRefs"][0]
            .as_object_mut()
            .expect("Revision ref object")
            .remove("objectArtifactContentHash");
        assert!(timeline_exact_revision_target_from_document(&drifted).is_err());
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn timeline_semantic_normalization_validates_and_masks_lane_stamps() {
        let document = |source: char, timeline: char| {
            json!({
                "schema": "pointbreak.inspect-event-history",
                "version": 1,
                "sourceChangeProjectionStamp": format!("sha256:{}", source.to_string().repeat(64)),
                "timelineProjectionStamp": format!("sha256:{}", timeline.to_string().repeat(64)),
                "entries": [],
            })
        };
        let valid = document('1', '2');
        let normalized = normalize_timeline_semantic_checked(valid.clone()).expect("valid stamps");
        assert_eq!(
            normalized,
            normalize_timeline_semantic_checked(document('3', '4')).expect("lane-local stamps")
        );
        assert_eq!(
            normalized["sourceChangeProjectionStamp"],
            "<source-change-projection-stamp>"
        );
        assert_eq!(
            normalized["timelineProjectionStamp"],
            "<timeline-projection-stamp>"
        );
        let mut missing = valid.clone();
        missing
            .as_object_mut()
            .expect("Timeline document")
            .remove("sourceChangeProjectionStamp");
        let mut malformed = valid;
        malformed["timelineProjectionStamp"] = json!("sha256:not-a-digest");
        assert!(normalize_timeline_semantic_checked(missing).is_err());
        assert!(normalize_timeline_semantic_checked(malformed).is_err());
        assert_eq!(
            super::instrumented::first_json_difference(
                &json!({"entries": [{"status": "valid"}]}),
                &json!({"entries": [{"status": "invalid"}]}),
                String::new(),
            )
            .as_deref(),
            Some("/entries/0/status")
        );
        assert_eq!(
            super::instrumented::first_json_difference(
                &json!({"entries": []}),
                &json!({"entries": [{"status": "valid"}]}),
                String::new(),
            )
            .as_deref(),
            Some("/entries/0")
        );
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn timeline_invalid_signature_recipe_changes_only_the_inline_signature() {
        let clean_bytes =
            include_bytes!("../../../tests/fixtures/event_signatures/friendly-valid-event.json");
        let clean: ShoreEvent = serde_json::from_slice(clean_bytes).expect("signed event fixture");
        let trust = crate::session::event_signature_trust_set(
            serde_json::from_str(include_str!(
                "../../../tests/fixtures/event_signatures/did-key-ed25519.json"
            ))
            .expect("trust fixture JSON"),
        )
        .expect("trust fixture");
        assert_eq!(
            verify_event_signature(&clean, &trust).expect("verify clean event"),
            EventVerificationStatus::Valid
        );

        let clean_signature = clean
            .signature
            .as_ref()
            .expect("inline signature")
            .sig
            .as_str();
        let clean_signature_bytes = BASE64_STANDARD
            .decode(clean_signature.as_bytes())
            .expect("base64 signature");
        let mut mutated_signature_bytes = clean_signature_bytes.clone();
        mutated_signature_bytes[0] ^= 1;
        let invalid_signature = EventSignatureBytes::from_bytes(&mutated_signature_bytes);
        let mutated_bytes = replace_unique_bytes(
            clean_bytes,
            clean_signature.as_bytes(),
            invalid_signature.as_str().as_bytes(),
        )
        .expect("unique inline signature replacement");
        let mutated: ShoreEvent =
            serde_json::from_slice(&mutated_bytes).expect("mutated event fixture");
        let mut expected = clean.clone();
        expected.signature.as_mut().expect("inline signature").sig = invalid_signature;

        assert_eq!(mutated, expected);
        assert_eq!(
            clean_signature_bytes
                .iter()
                .zip(&mutated_signature_bytes)
                .map(|(clean, mutated)| (clean ^ mutated).count_ones())
                .sum::<u32>(),
            1
        );
        assert_eq!(
            mutated.event_record_hash().expect("mutated record hash"),
            clean.event_record_hash().expect("clean record hash")
        );
        assert_eq!(
            verify_event_signature(&mutated, &trust).expect("verify mutated event"),
            EventVerificationStatus::Invalid
        );
        assert_ne!(
            sha256_bytes_hex(clean_bytes),
            sha256_bytes_hex(&mutated_bytes)
        );

        let recipe_sha256 = sha256_bytes_hex(
            &canonical_json_bytes(&json!({
                "schema": "pointbreak.timeline-invalid-inline-signature-mutation-recipe.v1",
                "target": "topology-valid-inline-signature-carrier",
                "mutation": "flip-one-bit-in-inline-signature",
                "byteIndex": 0,
                "bitMask": 1,
            }))
            .expect("canonical mutation recipe"),
        );
        assert_eq!(
            recipe_sha256,
            QUALIFICATION_TIMELINE_INVALID_SIGNATURE_MUTATION_RECIPE_SHA256_V1
        );
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn topology_witness_preflight_binds_classification_and_exact_current_revisions() {
        let witness = json!({
            "topology": {
                "initial": {
                    "change": "change:initial",
                    "current": {"revision": "rev:initial", "artifact": "sha256:initial"}
                },
                "replacement": {
                    "change": "change:replacement",
                    "current": {"revision": "rev:left", "artifact": "sha256:left"}
                },
                "parallel_current": {
                    "change": "change:parallel",
                    "current": [
                        {"revision": "rev:left", "artifact": "sha256:left"},
                        {"revision": "rev:right", "artifact": "sha256:right"}
                    ]
                },
                "replacement_divergent": {
                    "change": "change:divergent",
                    "current": [
                        {"revision": "rev:left", "artifact": "sha256:left"},
                        {"revision": "rev:right", "artifact": "sha256:right"}
                    ]
                },
                "consolidation": {
                    "change": "change:consolidation",
                    "current": {"revision": "rev:merged", "artifact": "sha256:merged"}
                }
            }
        });
        let changes = [
            (
                "initial",
                "change:initial",
                vec![("rev:initial", "sha256:initial")],
            ),
            (
                "replacement",
                "change:replacement",
                vec![("rev:left", "sha256:left")],
            ),
            (
                "parallel_current",
                "change:parallel",
                vec![("rev:right", "sha256:right"), ("rev:left", "sha256:left")],
            ),
            (
                "replacement_divergent",
                "change:divergent",
                vec![("rev:left", "sha256:left"), ("rev:right", "sha256:right")],
            ),
            (
                "consolidation",
                "change:consolidation",
                vec![("rev:merged", "sha256:merged")],
            ),
        ]
        .into_iter()
        .map(|(topology, change, current)| {
            json!({
                "changeId": change,
                "topology": topology,
                "currentRevisionRefs": current.into_iter().map(|(revision, artifact)| json!({
                    "revisionId": revision,
                    "objectArtifactContentHash": artifact,
                })).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
        validate_topology_fixture_semantics(&witness, &changes)
            .expect("exact topology witness must pass");

        let mut drifted = changes;
        drifted[0]["topology"] = json!("replacement");
        assert!(validate_topology_fixture_semantics(&witness, &drifted).is_err());
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn topology_witness_binds_the_complete_authoritative_inventory() {
        let inventory = "1".repeat(64);
        let witness = json!({
            "authoritativeInventorySha256": inventory,
        });
        validate_fixture_authoritative_inventory(&witness, &inventory)
            .expect("exact topology inventory authority");

        let mut drifted = witness.clone();
        drifted["authoritativeInventorySha256"] = json!("2".repeat(64));
        assert!(validate_fixture_authoritative_inventory(&drifted, &inventory).is_err());
        assert!(validate_fixture_authoritative_inventory(&json!({}), &inventory).is_err());

        let materializer =
            include_str!("../../../scripts/materialize-inspector-decision-matrix.sh");
        assert!(materializer.contains("authoritativeInventorySha256"));
        assert!(materializer.contains("authoritative_inventory_sha256"));
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn diagnostic_fixture_copy_is_isolated_and_refuses_an_existing_destination() {
        let root = tempfile::tempdir().expect("diagnostic fixture root");
        let source = root.path().join("source");
        let destination = root.path().join("workspace/clone");
        std::fs::create_dir_all(source.join("nested")).expect("create source");
        std::fs::write(source.join("nested/input.txt"), b"public fixture")
            .expect("write source fixture");
        copy_public_fixture_tree(&source, &destination).expect("copy public fixture");
        std::fs::write(destination.join("nested/input.txt"), b"mutated clone")
            .expect("mutate clone");
        assert_eq!(
            std::fs::read(source.join("nested/input.txt")).expect("read source fixture"),
            b"public fixture"
        );
        assert!(copy_public_fixture_tree(&source, &destination).is_err());
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn diagnostic_fault_fixture_is_materialized_at_its_case_root() {
        let parent = tempfile::tempdir().expect("diagnostic fixture parent");
        let root = parent.path().join("missing-carrier-case");
        let witness = materialize_diagnostic_fixture_at_root(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &root,
            QualificationDerivedChangeFixtureV1::MissingCarrierV1,
        )
        .expect("materialize diagnostic fixture at final case root");

        assert_eq!(
            witness.kind,
            QualificationDerivedChangeFixtureKindV1::MissingSelectedCarrier
        );
        let access = DerivedChangeAccess::resolve_for_inspector(&root)
            .expect("resolve final-root diagnostic fixture");
        let outcome = access
            .changes(&DerivedChangePageRequestV1::Bare)
            .expect("read final-root diagnostic fixture");
        let DerivedChangeOutcomeV1::ProjectionUnavailable(document) = outcome else {
            panic!("missing-carrier diagnostic fixture unexpectedly became ready")
        };
        assert_eq!(
            document.code(),
            DerivedProjectionFailureCodeV1::ProjectionRebuildRequired
        );
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn diagnostic_case_ordering_is_explicitly_bounded() {
        for case in QualificationDerivedChangeReadCaseV1::ALL {
            assert_eq!(
                requires_pre_mutation_measurement(case),
                case == QualificationDerivedChangeReadCaseV1::StalePageToken
            );
        }
        assert!(!requires_semantic_fixture_preflight(
            QualificationDerivedChangeReadCaseV1::Profile
        ));
        assert!(requires_semantic_fixture_preflight(
            QualificationDerivedChangeReadCaseV1::ChangesBare
        ));
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn stale_token_measurement_waits_for_a_ready_derived_generation() {
        let source = include_str!("change_read.rs");
        let diagnostic = source
            .split("fn run_diagnostic_read_case(")
            .nth(1)
            .and_then(|source| source.split("fn establish_diagnostic_post_append(").next())
            .expect("diagnostic read-case helper");
        let ready = diagnostic
            .find("derived.ensure_ready()?")
            .expect("stale-token derived readiness");
        let measurement = diagnostic
            .find("measure_diagnostic_read_case(&request, case, &expected)?")
            .expect("stale-token pre-mutation measurement");

        assert!(ready < measurement);
        assert!(diagnostic.contains("prepared_derived.take()"));
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn apfs_existing_carrier_profiles_use_a_distinct_ready_profile_oracle() {
        for fixture in [
            QualificationDerivedChangeFixtureV1::MutatedCarrierV1,
            QualificationDerivedChangeFixtureV1::WrongFamilyCarrierV1,
        ] {
            let (oracle, status, code) = qualification_derived_change_expected_outcome_v1(
                QualificationDerivedAccessPlatformV1::MacosApfs,
                fixture,
                QualificationDerivedChangeReadCaseV1::Profile,
            );
            assert_eq!(
                oracle,
                QualificationDerivedChangeReadOracleV1::ReadyProfileParity
            );
            assert_eq!(status, 200);
            assert_eq!(code, None);
        }
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn post_append_advancement_requires_both_stamp_and_cursor() {
        let advanced = json!({
            "changes": {"projectionStamp": "sha256:after"},
            "profile": {"authorityCursor": {"eventCount": 8}}
        });
        assert!(post_append_has_advanced(&advanced, "sha256:before", 7));

        let same_stamp = json!({
            "changes": {"projectionStamp": "sha256:before"},
            "profile": {"authorityCursor": {"eventCount": 8}}
        });
        assert!(!post_append_has_advanced(&same_stamp, "sha256:before", 7));

        let stale_cursor = json!({
            "changes": {"projectionStamp": "sha256:after"},
            "profile": {"authorityCursor": {"eventCount": 7}}
        });
        assert!(!post_append_has_advanced(&stale_cursor, "sha256:before", 7));
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn governed_append_keeps_one_qualified_coordinator_through_idempotent_recovery() {
        let source = include_str!("change_read.rs");
        let append = source
            .split("fn append_governed_qualification_event(")
            .nth(1)
            .and_then(|source| source.split("fn qualification_change_lifecycle(").next())
            .expect("governed qualification append helper");

        assert!(append.contains("new_for_qualification"));
        assert!(append.contains("with_coordinator(coordinator)"));
        assert!(append.contains("record_event_once_for_qualification"));
        assert!(append.contains("repeat_governed_qualification_event_until_current"));
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn diagnostic_template_postflight_records_inventory_failure() {
        let (source_unchanged, postflight) =
            diagnostic_template_postflight(Err("template inventory diagnostic".to_owned()));
        assert!(!source_unchanged);
        assert_eq!(
            postflight.kind,
            DerivedChangeReadDiagnosticPreflightKindV1::TemplatePostflight
        );
        assert_eq!(
            postflight.status,
            DerivedChangeReadDiagnosticStatusV1::Failed
        );
        assert_eq!(
            postflight.failure_detail.as_deref(),
            Some("template inventory diagnostic")
        );
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn failed_diagnostic_read_row_retains_a_bounded_oracle_witness() {
        let witness = DerivedChangeReadDiagnosticFailureWitnessV1::StrictParity {
            derived: DerivedChangeReadDiagnosticSemanticWitnessV1 {
                http_status: 200,
                code: None,
                normalized_document_sha256: "1".repeat(64),
            },
            strict: DerivedChangeReadDiagnosticSemanticWitnessV1 {
                http_status: 200,
                code: None,
                normalized_document_sha256: "2".repeat(64),
            },
            expected_http_status: 200,
            expected_code: None,
        };
        let rows = collect_derived_change_read_diagnostic_rows_for_cases_v1(
            &[QualificationDerivedChangeReadCaseV1::SummaryQuery],
            |_| {
                Err(DiagnosticReadFailure {
                    detail: "generic mismatch".to_owned(),
                    witness: Some(Box::new(witness.clone())),
                })
            },
        );

        assert_eq!(rows[0].failure_witness.as_ref(), Some(&witness));
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn typed_diagnostic_failure_does_not_replay_the_semantic_probe() {
        let source = include_str!("change_read.rs");
        let diagnostic = source
            .split("fn run_diagnostic_read_case(")
            .nth(1)
            .expect("diagnostic read case")
            .split("fn establish_diagnostic_post_append(")
            .next()
            .expect("bounded diagnostic read case");
        let failure_branch = diagnostic
            .split("match semantic_pair_observed(")
            .nth(1)
            .expect("diagnostic semantic-pair branch")
            .split("Err(failure) => return Err(failure.detail.into())")
            .next()
            .expect("diagnostic semantic-pair failure branch");

        assert_eq!(diagnostic.matches("semantic_pair_observed(").count(), 1);
        assert!(!failure_branch.contains("semantic_case("));
    }
}
