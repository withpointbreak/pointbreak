use std::path::{Path, PathBuf};
#[cfg(feature = "longitudinal-counting")]
use std::process::{Child, Command, Stdio};
#[cfg(feature = "longitudinal-counting")]
use std::sync::{Arc, Mutex, PoisonError};
#[cfg(feature = "longitudinal-counting")]
use std::thread;
#[cfg(feature = "longitudinal-counting")]
use std::time::{Duration, Instant};
#[cfg(feature = "longitudinal-counting")]
use std::{
    collections::BTreeSet,
    fs::File,
    io::{BufRead as _, BufReader, Read as _, Write as _},
    net::{Shutdown, TcpStream},
};

use serde::{Deserialize, Serialize};
#[cfg(feature = "longitudinal-counting")]
use serde_json::{Value, json};
#[cfg(feature = "longitudinal-counting")]
use sha2::{Digest as _, Sha256};

use super::evidence::QualificationDerivedChangeReadReceiptV1;
#[cfg(feature = "longitudinal-counting")]
use super::evidence::validate_current_execution_identity_v1;
#[cfg(feature = "longitudinal-counting")]
use super::{
    QUALIFICATION_DERIVED_CHANGE_ACTIVATION_FIXTURE_V1,
    QUALIFICATION_DERIVED_CHANGE_COMPLETION_FIXTURE_V1,
    QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V1, QualificationDerivedAccessPlatformV1,
    QualificationDerivedAccessProcessScopeV1, QualificationDerivedAccessStatusV1,
    QualificationDerivedChangeControlBinaryIdentityV1, QualificationDerivedChangeControlEvidenceV1,
    QualificationDerivedChangeFixtureKindV1, QualificationDerivedChangeFixtureRequestV1,
    QualificationDerivedChangeFixtureWitnessV1, QualificationDerivedChangeReadEvidenceV1,
    QualificationDerivedChangeReadOracleV1, QualificationDerivedChangeStorageEvidenceV1,
    QualificationDerivedChangeStoragePhaseV1, QualificationDerivedChangeTypedDocumentV1,
    QualificationDerivedStorageForbiddenProbeHashesV1,
    capture_qualification_derived_storage_witness_v1,
    materialize_qualification_derived_change_fixture_v1,
    qualification_derived_change_control_attestation_test_v1,
    qualification_derived_change_control_command_sha256_v1,
    qualification_derived_change_expected_outcome_v1,
    qualification_derived_change_storage_probe_hashes_v1,
};
use super::{
    QualificationDerivedAccessExecutionIdentityV1, QualificationDerivedAccessProductIdentityV1,
    QualificationDerivedChangeControlBinaryKindV1, QualificationDerivedChangeControlCaseV1,
    QualificationDerivedChangeEvidencePurposeV1, QualificationDerivedChangeFixtureV1,
    QualificationDerivedChangeReadCaseV1, QualificationDerivedStorageForbiddenProbeInputV1,
    qualification_derived_change_control_build_command_sha256_v1,
    qualification_derived_change_control_test_v1,
};
#[cfg(feature = "longitudinal-counting")]
use crate::bench_support::longitudinal::{
    LongitudinalCountersV1, LongitudinalCountingScopeV1,
    longitudinal_authoritative_store_data_inventory_v1,
};
#[cfg(feature = "longitudinal-counting")]
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
#[cfg(feature = "longitudinal-counting")]
use crate::session::derived_access::lifecycle::DerivedAccessLifecycle;
#[cfg(feature = "longitudinal-counting")]
use crate::session::derived_access::product_contract::{
    DerivedAccessAvailability, DerivedAccessProfile,
};
#[cfg(feature = "longitudinal-counting")]
use crate::session::derived_access::writer::DerivedWriteCoordinator;
#[cfg(feature = "longitudinal-counting")]
use crate::session::event::{EventTarget, EventType, ReviewInitializedPayload, ShoreEvent, Writer};
#[cfg(feature = "longitudinal-counting")]
use crate::session::{
    ChangeLifecycleV1, ChangeTopologyV1, DerivedAttentionPageV1, DerivedChangeAccess,
    DerivedChangeAttentionFilterV1, DerivedChangeAvailabilityFilterV1, DerivedChangeOutcomeV1,
    DerivedChangePageContinuationV1, DerivedChangePageRequestV1, DerivedChangePageSelectionV1,
    DerivedChangePageV1, DerivedProjectionFailureCodeV1, EventStore, EventWriteOutcome,
    opaque_path_identity, store_dir_for_repo,
};

pub const QUALIFICATION_DERIVED_CHANGE_READ_REQUEST_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-change-read-request.v1";
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
) -> Result<QualificationDerivedChangeReadReceiptV1, String> {
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

#[cfg(feature = "longitudinal-counting")]
mod instrumented {
    use super::*;

    pub(super) fn run_qualification_derived_change_read_v1(
        request_path: &Path,
    ) -> Result<QualificationDerivedChangeReadReceiptV1, String> {
        let request: QualificationDerivedChangeReadRunRequestV1 = read_json(request_path)?;
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
                witness: capture_qualification_derived_storage_witness_v1(&store_root, probes)?,
            }]
        } else {
            Vec::new()
        };

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
                witness: capture_qualification_derived_storage_witness_v1(&store_root, probes)?,
            });
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
        Ok(receipt)
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

    impl InspectorEndpoint {
        fn request(&self, method: &str, target: &str) -> Result<(u16, String), String> {
            let mut last_error = String::new();
            for attempt in 0..12 {
                match self.try_request(method, target) {
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

        fn try_request(&self, method: &str, target: &str) -> Result<(u16, String), String> {
            let mut stream =
                TcpStream::connect(&self.address).map_err(|error| error.to_string())?;
            let request = format!(
                "{method} {target} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                self.address, self.token
            );
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
            Ok((status, body.to_owned()))
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
    }

    impl InspectorChild {
        fn spawn(
            request: &QualificationDerivedChangeReadRunRequestV1,
            profile: &str,
        ) -> Result<Self, String> {
            let mut child = Command::new(&request.product_binary)
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
                .env_remove("POINTBREAK_LOG")
                .env_remove("RUST_LOG")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
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
        normalize_ready_profile_semantic, percent_encode, post_append_has_advanced,
        requires_pre_mutation_measurement, requires_semantic_fixture_preflight,
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
