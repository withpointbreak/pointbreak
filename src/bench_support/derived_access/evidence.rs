use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
#[cfg(feature = "longitudinal-counting")]
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::adapter::QualificationDerivedAccessAdapter;
use super::diagnostic::{
    reject_derived_change_diagnostic_evidence_document_v1,
    reject_derived_change_diagnostic_evidence_input_v1,
    reject_derived_change_diagnostic_evidence_path_v1,
};
use super::sqlite_cursor::{BootstrapControl, CursorLedgerIdentity, SqliteCursorLedger};
use super::{
    QUALIFICATION_DERIVED_ACCESS_EVALUATION_SCHEMA_V1, QualificationDerivedAccessComplexityV1,
    QualificationDerivedAccessCountersV1, QualificationDerivedAccessEvaluationV1,
    QualificationDerivedAccessExecutionIdentityV1, QualificationDerivedAccessOperationV1,
    QualificationDerivedAccessPackageV1, QualificationDerivedAccessProcessScopeV1,
    QualificationDerivedAccessProductIdentityV1, QualificationDerivedAccessStatusV1,
    QualificationDerivedAccessTierV1, QualificationDerivedChangeControlBinaryIdentityV1,
    QualificationDerivedChangeControlCaseV1, QualificationDerivedChangeControlEvidenceV1,
    QualificationDerivedChangeEvidencePurposeV1, QualificationDerivedChangeFixtureV1,
    QualificationDerivedChangeReadCaseV1, QualificationDerivedChangeReadEvidenceV1,
    QualificationDerivedChangeStorageEvidenceV1, QualificationDerivedChangeStoragePhaseV1,
    QualificationDerivedTimelineReadEvidenceV1, QualificationDerivedTimelineStorageEvidenceV1,
    evaluate_qualification_derived_access_v1,
};
use crate::bench_support::foundation::qualification_host_identity_sha256;
#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::{
    LongitudinalCountersV1, LongitudinalDerivedAccessPhaseSampleV1,
    LongitudinalDerivedAccessPhaseV1,
};
#[cfg(feature = "longitudinal-counting")]
use crate::bench_support::longitudinal::{
    LongitudinalCountingScopeV1, capture_longitudinal_process_snapshot_v1,
};
use crate::bench_support::longitudinal::{
    LongitudinalStoreDataInventoryV1, is_governed_derived_store_entry_v1,
    longitudinal_authoritative_store_data_inventory_v1,
};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
#[cfg(feature = "longitudinal-counting")]
use crate::model::{JournalId, RevisionId};
#[cfg(feature = "longitudinal-counting")]
use crate::session::derived_access::cursor::AppendResolution;
use crate::session::derived_access::locator::LocatorRead;
#[cfg(feature = "longitudinal-counting")]
use crate::session::derived_access::locator::{ChronologicalWindowRequest, WindowContinuation};
use crate::session::derived_access::oracle::strict_bodyless_materialized_snapshot;
#[cfg(feature = "longitudinal-counting")]
use crate::session::event::{
    EventTarget, EventType, ReviewInitializedPayload, ShoreEvent, WorkObjectProposal,
    WorkObjectProposedPayload, Writer,
};
use crate::session::{EventStore, store_dir_for_repo};

pub const QUALIFICATION_DERIVED_ACCESS_RAW_SAMPLE_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-raw-sample.v1";
pub const QUALIFICATION_DERIVED_ACCESS_EVIDENCE_SHARD_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-evidence-shard.v1";
pub const QUALIFICATION_DERIVED_ACCESS_PACKAGE_MANIFEST_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-package-manifest.v1";
pub const QUALIFICATION_DERIVED_ACCESS_RETAINED_ROOT_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-retained-root-request.v1";
pub const QUALIFICATION_DERIVED_ACCESS_RETAINED_PREFLIGHT_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-retained-preflight.v1";
pub const QUALIFICATION_DERIVED_ACCESS_RETAINED_BOOTSTRAP_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-retained-bootstrap.v1";
pub const QUALIFICATION_DERIVED_ACCESS_SCALE_REQUEST_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-scale-request.v1";
pub const QUALIFICATION_DERIVED_ACCESS_SCALE_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-scale-receipt.v1";
pub const QUALIFICATION_DERIVED_ACCESS_RESOURCE_REQUEST_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-resource-request.v1";
pub const QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_REQUEST_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-resource-child-request.v1";
pub const QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-resource-child-receipt.v1";
pub const QUALIFICATION_DERIVED_ACCESS_RESOURCE_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-resource-receipt.v1";
pub const QUALIFICATION_DERIVED_ACCESS_RESTART_CHILD_REQUEST_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-restart-child-request.v1";
pub const QUALIFICATION_DERIVED_ACCESS_RESTART_CHILD_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-restart-child-receipt.v1";
pub const QUALIFICATION_DERIVED_ACCESS_FRAGMENT_REQUEST_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-fragment-request.v1";
pub const QUALIFICATION_DERIVED_ACCESS_FRAGMENT_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-fragment.v1";
pub const QUALIFICATION_DERIVED_ACCESS_NATIVE_SMOKE_REQUEST_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-native-smoke-request.v1";
pub const QUALIFICATION_DERIVED_ACCESS_NATIVE_SMOKE_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-native-smoke-receipt.v1";
pub const QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-change-read-receipt.v1";
pub const QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V2: &str =
    "pointbreak.qualification-derived-change-read-receipt.v2";
#[cfg(any(test, feature = "longitudinal-counting"))]
pub const QUALIFICATION_DERIVED_ACCESS_PHASE_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-phase-receipt.v1";
#[cfg(any(test, feature = "longitudinal-counting"))]
pub const QUALIFICATION_DERIVED_ACCESS_PHASE_BUNDLE_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-phase-bundle.v1";
#[cfg(any(test, feature = "longitudinal-counting"))]
pub const QUALIFICATION_DERIVED_ACCESS_PHASE_REQUEST_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-phase-request.v1";

pub const QUALIFICATION_DERIVED_ACCESS_HELP_MODE_V1: &str = "--derived-access-help";
pub const QUALIFICATION_DERIVED_ACCESS_SMOKE_MODE_V1: &str = "--derived-access-smoke";
pub const QUALIFICATION_DERIVED_ACCESS_BOOTSTRAP_SMOKE_MODE_V1: &str =
    "--derived-access-bootstrap-smoke";
pub const QUALIFICATION_DERIVED_ACCESS_PHASE_MODE_V1: &str = "--derived-access-phase-evidence";
pub const QUALIFICATION_DERIVED_ACCESS_PHASE_VERIFY_MODE_V1: &str = "--derived-access-phase-verify";
pub const QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_MODE_V1: &str = "--derived-access-lifecycle";
pub const QUALIFICATION_DERIVED_ACCESS_RETAINED_PREFLIGHT_MODE_V1: &str =
    "--derived-access-retained-preflight";
pub const QUALIFICATION_DERIVED_ACCESS_RETAINED_BOOTSTRAP_MODE_V1: &str =
    "--derived-access-retained-bootstrap";
pub const QUALIFICATION_DERIVED_ACCESS_SCALE_MODE_V1: &str = "--derived-access-scale-evidence";
pub const QUALIFICATION_DERIVED_ACCESS_RESOURCE_MODE_V1: &str =
    "--derived-access-resource-evidence";
pub const QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_MODE_V1: &str =
    "--derived-access-resource-child";
pub const QUALIFICATION_DERIVED_ACCESS_RESTART_CHILD_MODE_V1: &str =
    "--derived-access-restart-child";
pub const QUALIFICATION_DERIVED_ACCESS_FRAGMENT_MODE_V1: &str = "--derived-access-fragment";
pub const QUALIFICATION_DERIVED_ACCESS_PACKAGE_MODE_V1: &str = "--derived-access-package";
pub const QUALIFICATION_DERIVED_ACCESS_VERIFY_PACKAGE_MODE_V1: &str =
    "--derived-access-verify-package";

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedChangeReadReceiptV1 {
    pub schema: String,
    pub purpose: QualificationDerivedChangeEvidencePurposeV1,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub product: QualificationDerivedAccessProductIdentityV1,
    pub fixture: QualificationDerivedChangeFixtureV1,
    pub fixture_builder_sha256: String,
    pub activation_fixture_sha256: String,
    pub completion_fixture_sha256: String,
    pub fixture_inventory_sha256: String,
    pub fixture_after_inventory_sha256: String,
    pub fixture_witness_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_append_generation_sha256: Option<String>,
    pub rows: Vec<QualificationDerivedChangeReadEvidenceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_cut_deficiencies: Vec<QualificationDerivedChangeReadCaseV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub control_binary_identities: Vec<QualificationDerivedChangeControlBinaryIdentityV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub control_rows: Vec<QualificationDerivedChangeControlEvidenceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub storage_rows: Vec<QualificationDerivedChangeStorageEvidenceV1>,
    pub complete: bool,
    pub receipt_sha256: String,
}

impl QualificationDerivedChangeReadReceiptV1 {
    fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.receipt_sha256.clear();
        serde_json::to_vec(&preimage)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())
    }

    pub fn refresh_sha256(&mut self) -> Result<(), String> {
        self.receipt_sha256 = self.canonical_sha256()?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V1
            || !self.complete
            || self.rows.is_empty()
        {
            return Err("derived Change read receipt is incomplete".to_owned());
        }
        self.execution.validate()?;
        self.product.validate()?;
        let exact_source = self.product.is_exact_source_for(&self.execution);
        match self.purpose {
            QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification
                if !exact_source || !self.pre_cut_deficiencies.is_empty() =>
            {
                return Err("Change read product differs from its exact harness source".to_owned());
            }
            QualificationDerivedChangeEvidencePurposeV1::PreCutFalsifier
                if exact_source
                    || !self.control_rows.is_empty()
                    || !self.control_binary_identities.is_empty()
                    || !self.storage_rows.is_empty() =>
            {
                return Err("pre-cut Change falsifier claims exact-source controls".to_owned());
            }
            _ => {}
        }
        if self.purpose == QualificationDerivedChangeEvidencePurposeV1::PreCutFalsifier {
            let failed_cases = self
                .rows
                .iter()
                .filter(|row| row.status == QualificationDerivedAccessStatusV1::Failed)
                .map(|row| row.case)
                .collect::<BTreeSet<_>>();
            let declared_deficiencies = self
                .pre_cut_deficiencies
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            if failed_cases.is_empty()
                || failed_cases.len() != self.pre_cut_deficiencies.len()
                || failed_cases != declared_deficiencies
            {
                return Err(
                    "pre-cut Change falsifier did not identify a failed matrix row".to_owned(),
                );
            }
        }
        validate_digest(&self.fixture_builder_sha256, "Change read fixture builder")?;
        validate_digest(
            &self.activation_fixture_sha256,
            "Change read activation fixture",
        )?;
        validate_digest(
            &self.completion_fixture_sha256,
            "Change read completion fixture",
        )?;
        validate_digest(
            &self.fixture_inventory_sha256,
            "Change read fixture inventory",
        )?;
        validate_digest(
            &self.fixture_after_inventory_sha256,
            "Change read fixture after-inventory",
        )?;
        validate_digest(&self.fixture_witness_sha256, "Change read fixture witness")?;
        match self.fixture {
            QualificationDerivedChangeFixtureV1::TopologyV1 => {
                if self.fixture_after_inventory_sha256 == self.fixture_inventory_sha256 {
                    return Err("derived Change post-append fixture did not advance".to_owned());
                }
                validate_digest(
                    self.post_append_generation_sha256
                        .as_deref()
                        .unwrap_or_default(),
                    "Change read post-append generation",
                )?;
            }
            _ if self.fixture_after_inventory_sha256 != self.fixture_inventory_sha256
                || self.post_append_generation_sha256.is_some() =>
            {
                return Err("non-topology Change fixture mutated during evidence".to_owned());
            }
            _ => {}
        }
        validate_digest(&self.receipt_sha256, "Change read receipt")?;
        if self.receipt_sha256 != self.canonical_sha256()? {
            return Err("derived Change read receipt hash drifted".to_owned());
        }
        let identities = self
            .rows
            .iter()
            .map(|row| (row.platform, row.fixture, row.case))
            .collect::<BTreeSet<_>>();
        if identities.len() != self.rows.len()
            || self.rows.iter().any(|row| {
                row.platform != self.execution.platform
                    || row.fixture != self.fixture
                    || row.fixture_inventory_sha256 != self.fixture_inventory_sha256
                    || row.fixture_witness_sha256 != self.fixture_witness_sha256
            })
        {
            return Err("derived Change read receipt mixes row authority".to_owned());
        }
        let product_identity_sha256 = self.product.canonical_sha256()?;
        let execution_identity_sha256 = self.execution.canonical_sha256()?;
        if self.rows.iter().any(|row| {
            row.product_identity_sha256 != product_identity_sha256
                || row.counter_execution_identity_sha256 != execution_identity_sha256
                || row.counter_process_scope
                    != QualificationDerivedAccessProcessScopeV1::QualificationHarness
        }) {
            return Err("derived Change read row identity drifted".to_owned());
        }
        let observed_cases = self
            .rows
            .iter()
            .map(|row| row.case)
            .collect::<BTreeSet<_>>();
        let expected_cases = self
            .fixture
            .required_cases()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if observed_cases != expected_cases {
            return Err("derived Change read receipt omitted required cases".to_owned());
        }
        if self.purpose == QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification
            && self.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
        {
            let observed_control_binaries = self
                .control_binary_identities
                .iter()
                .map(|identity| identity.kind)
                .collect::<BTreeSet<_>>();
            let expected_control_binaries =
                super::QualificationDerivedChangeControlBinaryKindV1::ALL
                    .into_iter()
                    .collect::<BTreeSet<_>>();
            let observed_controls = self
                .control_rows
                .iter()
                .map(|row| row.case)
                .collect::<BTreeSet<_>>();
            let expected_controls = QualificationDerivedChangeControlCaseV1::ALL
                .into_iter()
                .collect::<BTreeSet<_>>();
            if observed_control_binaries != expected_control_binaries
                || self.control_binary_identities.len() != expected_control_binaries.len()
                || self.control_binary_identities.iter().any(|identity| {
                    identity.validate().is_err() || !identity.is_exact_source_for(&self.execution)
                })
                || observed_controls != expected_controls
                || self.control_rows.iter().any(|row| {
                    let (expected_kind, expected_test_name) =
                        super::qualification_derived_change_control_test_v1(row.case);
                    let identity_sha256 = self
                        .control_binary_identities
                        .iter()
                        .find(|identity| identity.kind == row.binary_kind)
                        .and_then(|identity| identity.canonical_sha256().ok());
                    let identity_binary_sha256 = self
                        .control_binary_identities
                        .iter()
                        .find(|identity| identity.kind == row.binary_kind)
                        .map(|identity| identity.binary_sha256.as_str());
                    row.platform != self.execution.platform
                        || row.status != QualificationDerivedAccessStatusV1::Passed
                        || row.binary_kind != expected_kind
                        || row.test_name != expected_test_name
                        || row.command_sha256
                            != super::qualification_derived_change_control_command_sha256_v1(
                                expected_test_name,
                            )
                        || row.exit_code != 0
                        || row.tests_run != 1
                        || row.tests_passed != 1
                        || row.product_identity_sha256 != product_identity_sha256
                        || row.execution_identity_sha256 != execution_identity_sha256
                        || identity_sha256.as_deref()
                            != Some(row.test_binary_identity_sha256.as_str())
                        || identity_binary_sha256 != Some(row.test_binary_sha256.as_str())
                })
            {
                return Err("derived Change control receipt is incomplete".to_owned());
            }
        } else if !self.control_rows.is_empty() || !self.control_binary_identities.is_empty() {
            return Err("non-topology Change receipt carries control evidence".to_owned());
        }
        let observed_storage_phases = self
            .storage_rows
            .iter()
            .map(|row| row.phase)
            .collect::<BTreeSet<_>>();
        let expected_storage_phases = match (self.purpose, self.fixture) {
            (
                QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification,
                QualificationDerivedChangeFixtureV1::TopologyV1,
            ) => BTreeSet::from([
                QualificationDerivedChangeStoragePhaseV1::InitialPublication,
                QualificationDerivedChangeStoragePhaseV1::PostAppendCheckpoint,
            ]),
            (QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification, _) => {
                BTreeSet::from([QualificationDerivedChangeStoragePhaseV1::InitialPublication])
            }
            (QualificationDerivedChangeEvidencePurposeV1::PreCutFalsifier, _) => BTreeSet::new(),
        };
        if observed_storage_phases != expected_storage_phases
            || self.storage_rows.iter().any(|row| {
                row.platform != self.execution.platform
                    || row.fixture != self.fixture
                    || row.fixture_inventory_sha256 != self.fixture_inventory_sha256
                    || row.fixture_witness_sha256 != self.fixture_witness_sha256
                    || row.product_identity_sha256 != product_identity_sha256
                    || row.execution_identity_sha256 != execution_identity_sha256
                    || row.witness.validate().is_err()
            })
        {
            return Err("derived Change storage receipt is incomplete".to_owned());
        }
        Ok(())
    }
}

/// Extends an immutable V1 Change-read receipt with Timeline successor
/// evidence. The embedded receipt remains the authority for the frozen Change
/// controls, rows, and storage witnesses.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedChangeReadReceiptV2 {
    pub schema: String,
    pub base: QualificationDerivedChangeReadReceiptV1,
    pub timeline_read_rows: Vec<QualificationDerivedTimelineReadEvidenceV1>,
    pub timeline_storage_rows: Vec<QualificationDerivedTimelineStorageEvidenceV1>,
    pub complete: bool,
    pub receipt_sha256: String,
}

impl QualificationDerivedChangeReadReceiptV2 {
    fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.receipt_sha256.clear();
        serde_json::to_vec(&preimage)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())
    }

    pub fn refresh_sha256(&mut self) -> Result<(), String> {
        self.receipt_sha256 = self.canonical_sha256()?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V2
            || !self.complete
            || self.timeline_read_rows.is_empty()
            || self.timeline_storage_rows.is_empty()
        {
            return Err("derived Change read successor receipt is incomplete".to_owned());
        }
        self.base.validate()?;
        if self.base.purpose
            != QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification
            || !self.base.product.is_exact_source_for(&self.base.execution)
            || !self
                .base
                .product
                .enabled_features
                .iter()
                .any(|feature| feature == "longitudinal-counting")
        {
            return Err("Change read successor receipt is not exact-source evidence".to_owned());
        }
        validate_digest(&self.receipt_sha256, "Change read successor receipt")?;
        if self.receipt_sha256 != self.canonical_sha256()? {
            return Err("derived Change read successor receipt hash drifted".to_owned());
        }

        let product_identity_sha256 = self.base.product.canonical_sha256()?;
        let execution_identity_sha256 = self.base.execution.canonical_sha256()?;
        let mut run_identities = BTreeSet::new();
        let expected_cases = super::required_timeline_cases_v1(self.base.fixture)
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let observed_cases = self
            .timeline_read_rows
            .iter()
            .map(|row| row.case)
            .collect::<BTreeSet<_>>();
        if observed_cases != expected_cases || observed_cases.len() != self.timeline_read_rows.len()
        {
            return Err("derived Timeline receipt omitted required cases".to_owned());
        }
        for row in &self.timeline_read_rows {
            let schedule = super::timeline_request_schedule_v1(self.base.fixture, row.case);
            if row.platform != self.base.execution.platform
                || row.fixture != self.base.fixture
                || row.fixture_inventory_sha256 != self.base.fixture_inventory_sha256
                || row.fixture_witness_sha256 != self.base.fixture_witness_sha256
                || row.product_identity_sha256 != product_identity_sha256
                || row.counter_execution_identity_sha256 != execution_identity_sha256
                || row.semantic_process_scope
                    != QualificationDerivedAccessProcessScopeV1::InspectorServiceChild
                || row.counter_process_scope
                    != QualificationDerivedAccessProcessScopeV1::InspectorServiceChild
                || !timeline_authority_digests_valid(&row.authority)
                || row.authority.request_schedule_sha256
                    != super::timeline_request_schedule_sha256_v1(self.base.fixture, row.case)
            {
                return Err("derived Timeline read receipt authority drifted".to_owned());
            }
            validate_digest(
                &row.derived_semantic_sha256,
                "derived Timeline semantic receipt",
            )?;
            if let Some(strict) = &row.strict_semantic_sha256 {
                validate_digest(strict, "strict Timeline semantic receipt")?;
            }
            if !timeline_typed_documents_valid_v1(row) {
                return Err("derived Timeline typed-document receipt drifted".to_owned());
            }
            if row.counter_receipts.len() != schedule.len() {
                return Err("derived Timeline counter receipt schedule is incomplete".to_owned());
            }
            for (receipt, operation) in row.counter_receipts.iter().zip(schedule) {
                receipt.validate().map_err(|error| error.to_string())?;
                if !run_identities.insert(receipt.run_identity.clone())
                    || receipt.operation != *operation
                    || receipt.root_identity != self.base.execution.root_provenance_sha256
                    || receipt.base_execution_identity_sha256 != execution_identity_sha256
                    || receipt.derivative_execution_identity_sha256 != product_identity_sha256
                    || receipt.manifest_sha256 != self.base.fixture_inventory_sha256
                    || receipt.schedule_sha256 != row.authority.request_schedule_sha256
                    || receipt.phase != row.case.as_str()
                {
                    return Err("derived Timeline counter receipt authority drifted".to_owned());
                }
            }
            if timeline_semantic_receipt_sha256(&row.counter_receipts)?
                != row.derived_semantic_sha256
            {
                return Err("derived Timeline semantic receipt aggregate drifted".to_owned());
            }
            if row
                .invalid_signature_failure
                .as_ref()
                .is_some_and(|failure| {
                    !run_identities.insert(failure.counter_receipt.run_identity.clone())
                })
                || !timeline_invalid_signature_failure_valid_v1(
                    row,
                    &self.base.execution,
                    &product_identity_sha256,
                    &execution_identity_sha256,
                )
                || !timeline_concurrent_trust_valid_v1(row)
            {
                return Err("derived Timeline invalid-signature receipt drifted".to_owned());
            }
        }

        let expected_phases = match self.base.fixture {
            QualificationDerivedChangeFixtureV1::TopologyV1 => BTreeSet::from([
                QualificationDerivedChangeStoragePhaseV1::InitialPublication,
                QualificationDerivedChangeStoragePhaseV1::PostAppendCheckpoint,
            ]),
            _ => BTreeSet::from([QualificationDerivedChangeStoragePhaseV1::InitialPublication]),
        };
        let observed_phases = self
            .timeline_storage_rows
            .iter()
            .map(|row| row.phase)
            .collect::<BTreeSet<_>>();
        if observed_phases != expected_phases
            || observed_phases.len() != self.timeline_storage_rows.len()
            || self.timeline_storage_rows.iter().any(|row| {
                let base_storage_matches = self.base.storage_rows.iter().any(|base| {
                    base.platform == row.platform
                        && base.fixture == row.fixture
                        && base.phase == row.phase
                        && base.fixture_inventory_sha256 == row.fixture_inventory_sha256
                        && base.fixture_witness_sha256 == row.fixture_witness_sha256
                        && base.product_identity_sha256 == row.product_identity_sha256
                        && base.execution_identity_sha256 == row.execution_identity_sha256
                });
                let probe_kinds = row
                    .forbidden_probes
                    .iter()
                    .map(|probe| probe.kind)
                    .collect::<BTreeSet<_>>();
                row.platform != self.base.execution.platform
                    || row.fixture != self.base.fixture
                    || row.fixture_inventory_sha256 != self.base.fixture_inventory_sha256
                    || row.fixture_witness_sha256 != self.base.fixture_witness_sha256
                    || row.product_identity_sha256 != product_identity_sha256
                    || row.execution_identity_sha256 != execution_identity_sha256
                    || !base_storage_matches
                    || row.forbidden_probes.len()
                        != super::QualificationDerivedTimelineForbiddenProbeKindV1::ALL.len()
                    || probe_kinds
                        != super::QualificationDerivedTimelineForbiddenProbeKindV1::ALL
                            .into_iter()
                            .collect()
                    || row.forbidden_probes.iter().any(|probe| {
                        let token_sentinel_expected = super::qualification_derived_change_expected_outcome_v1(
                            row.platform,
                            row.fixture,
                            super::QualificationDerivedChangeReadCaseV1::ChangesBare,
                        )
                        .0 != super::QualificationDerivedChangeReadOracleV1::TypedFailure;
                        let sentinel_valid = match (&probe.kind, &probe.sentinel_sha256) {
                            (
                                super::QualificationDerivedTimelineForbiddenProbeKindV1::TimelineContinuationToken,
                                None,
                            ) => !token_sentinel_expected,
                            (
                                super::QualificationDerivedTimelineForbiddenProbeKindV1::TimelineContinuationToken,
                                Some(sentinel),
                            ) => {
                                token_sentinel_expected
                                    && validate_digest(sentinel, "Timeline storage probe").is_ok()
                            }
                            (_, Some(sentinel)) => {
                                validate_digest(sentinel, "Timeline storage probe").is_ok()
                            }
                            (_, None) => false,
                        };
                        !sentinel_valid
                            || probe.sqlite_match_count != 0
                            || probe.file_match_count != 0
                    })
            })
        {
            return Err("derived Timeline storage receipt is incomplete".to_owned());
        }
        Ok(())
    }
}

fn timeline_semantic_receipt_sha256(
    receipts: &[crate::bench_support::longitudinal::LongitudinalCounterReceiptV1],
) -> Result<String, String> {
    let semantic_receipts = receipts
        .iter()
        .map(|receipt| receipt.semantic_result_sha256.clone())
        .collect::<Vec<_>>();
    let value = serde_json::to_value(semantic_receipts).map_err(|error| error.to_string())?;
    canonical_json_bytes(&value)
        .map(|bytes| sha256_bytes_hex(&bytes))
        .map_err(|error| error.to_string())
}

fn timeline_authority_digests_valid(
    authority: &super::QualificationDerivedTimelineAuthorityEvidenceV1,
) -> bool {
    [
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
    .all(|digest| validate_digest(digest, "Timeline authority witness").is_ok())
        && authority
            .continuation_token_set_sha256
            .as_ref()
            .is_none_or(|digest| validate_digest(digest, "Timeline continuation-token set").is_ok())
}

fn timeline_typed_documents_valid_v1(row: &QualificationDerivedTimelineReadEvidenceV1) -> bool {
    let expected = super::expected_timeline_typed_documents_v1(row.platform, row.fixture, row.case);
    row.expected_typed_documents == expected
        && row.observed_typed_documents.len() == expected.len()
        && row
            .observed_typed_documents
            .iter()
            .zip(&expected)
            .all(|(observed, expected)| {
                observed.operation == expected.operation
                    && observed.http_status == expected.http_status
                    && observed.document.schema == expected.schema
                    && observed.document.version == expected.version
                    && observed.document.code == expected.code
                    && observed.document.retryable == expected.retryable
                    && validate_digest(
                        &observed.document.canonical_sha256,
                        "Timeline typed failure document",
                    )
                    .is_ok()
            })
}

fn timeline_concurrent_trust_valid_v1(row: &QualificationDerivedTimelineReadEvidenceV1) -> bool {
    let expected = row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
        && row.case == super::QualificationDerivedTimelineReadCaseV1::ProcessLifecycleSuite;
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
        .all(|identity| validate_digest(identity, "concurrent Timeline trust identity").is_ok())
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

fn timeline_invalid_signature_failure_valid_v1(
    row: &QualificationDerivedTimelineReadEvidenceV1,
    execution: &QualificationDerivedAccessExecutionIdentityV1,
    product_identity_sha256: &str,
    execution_identity_sha256: &str,
) -> bool {
    let expected = row.fixture == QualificationDerivedChangeFixtureV1::TopologyV1
        && row.case == super::QualificationDerivedTimelineReadCaseV1::TrustSuite;
    let Some(failure) = &row.invalid_signature_failure else {
        return !expected;
    };
    let counter = &failure.counter_receipt;
    let counters = &counter.counters;
    expected
        && row.status == QualificationDerivedAccessStatusV1::Passed
        && row.oracle == super::QualificationDerivedTimelineReadOracleV1::StrictParity
        && !failure.carrier_event_id.trim().is_empty()
        && validate_digest(
            &failure.clean_inventory_sha256,
            "clean invalid-signature inventory",
        )
        .is_ok()
        && validate_digest(
            &failure.derivative_inventory_sha256,
            "derivative invalid-signature inventory",
        )
        .is_ok()
        && validate_digest(
            &failure.restored_inventory_sha256,
            "restored invalid-signature inventory",
        )
        .is_ok()
        && failure.clean_inventory_sha256 == row.fixture_inventory_sha256
        && failure.clean_inventory_sha256 == failure.restored_inventory_sha256
        && failure.clean_inventory_sha256 != failure.derivative_inventory_sha256
        && validate_digest(&failure.clean_carrier_sha256, "clean signature carrier").is_ok()
        && validate_digest(&failure.mutated_carrier_sha256, "mutated signature carrier").is_ok()
        && failure.clean_carrier_sha256 != failure.mutated_carrier_sha256
        && failure.mutation_recipe_sha256
            == super::QUALIFICATION_TIMELINE_INVALID_SIGNATURE_MUTATION_RECIPE_SHA256_V1
        && failure.clean_signature_status == "valid"
        && failure.mutated_signature_status == "invalid"
        && failure.observed_http_status == 503
        && failure.observed_typed_document.schema == "pointbreak.inspect-change-projection-error"
        && failure.observed_typed_document.version == 1
        && failure.observed_typed_document.code == "projection_invalid"
        && failure.observed_typed_document.retryable == Some(false)
        && validate_digest(
            &failure.observed_typed_document.canonical_sha256,
            "invalid-signature typed failure document",
        )
        .is_ok()
        && validate_digest(
            &failure.clean_semantic_sha256,
            "invalid-signature clean semantic receipt",
        )
        .is_ok()
        && validate_digest(
            &failure.strict_semantic_sha256,
            "invalid-signature strict semantic receipt",
        )
        .is_ok()
        && validate_digest(
            &failure.derived_semantic_sha256,
            "invalid-signature derived semantic receipt",
        )
        .is_ok()
        && validate_digest(
            &failure.strict_recovery_semantic_sha256,
            "invalid-signature strict recovery semantic receipt",
        )
        .is_ok()
        && validate_digest(
            &failure.derived_recovery_semantic_sha256,
            "invalid-signature derived recovery semantic receipt",
        )
        .is_ok()
        && failure.clean_semantic_sha256 == failure.strict_recovery_semantic_sha256
        && failure.clean_semantic_sha256 == failure.derived_recovery_semantic_sha256
        && failure.clean_semantic_sha256 != failure.derived_semantic_sha256
        && failure.strict_semantic_sha256 == failure.derived_semantic_sha256
        && counter.validate().is_ok()
        && !counter.success
        && counter.operation == "timeline_invalid_signature_fault"
        && counter.phase == super::QualificationDerivedTimelineReadCaseV1::TrustSuite.as_str()
        && counter.root_identity == execution.root_provenance_sha256
        && counter.base_execution_identity_sha256 == execution_identity_sha256
        && counter.derivative_execution_identity_sha256 == product_identity_sha256
        && counter.manifest_sha256 == failure.derivative_inventory_sha256
        && counter.schedule_sha256
            == super::timeline_request_schedule_sha256_v1(
                QualificationDerivedChangeFixtureV1::TopologyV1,
                super::QualificationDerivedTimelineReadCaseV1::TrustSuite,
            )
        && counter.semantic_result_sha256 == failure.derived_semantic_sha256
        && counters.authoritative_fallbacks == 0
        && counters.full_history_fallbacks == 0
        && counters.event_folds == 0
        && counters.projection_rebuilds == 0
        && counters.state_rebuilds == 0
        && counters.body_artifact_reads == 0
        && counters.object_artifact_reads == 0
        && counters.timeline_trust_support_carriers == 0
        && counters.timeline_entries_emitted == 0
        && failure.recovery_signature_status == "valid"
        && failure.trust_bindings_observed == 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedAccessCpuUnitV1 {
    Nanoseconds,
    NativeTicks,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedAccessPhaseOperationV1 {
    RevisionPage,
    Bootstrap,
    GovernedWrite,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
impl QualificationDerivedAccessPhaseOperationV1 {
    pub const ALL: [Self; 3] = [Self::RevisionPage, Self::Bootstrap, Self::GovernedWrite];

    pub fn expected_phases(self) -> &'static [LongitudinalDerivedAccessPhaseV1] {
        use LongitudinalDerivedAccessPhaseV1 as Phase;
        match self {
            Self::RevisionPage => &[
                Phase::RevisionPageSqlSelection,
                Phase::RevisionPageEventIdExpansion,
                Phase::RevisionPageCarrierHydrationValidation,
                Phase::RevisionPageListProjection,
                Phase::RevisionPageSupersederSupportExpansion,
                Phase::RevisionPageOverviewConstruction,
                Phase::RevisionPageSnapshotSummaries,
            ],
            Self::Bootstrap => &[
                Phase::BootstrapPopulation,
                Phase::BootstrapOracle,
                Phase::BootstrapFinalization,
            ],
            Self::GovernedWrite => &[
                Phase::GovernedWriteAdmission,
                Phase::GovernedWriteTruth,
                Phase::GovernedWriteCatchUp,
                Phase::GovernedWriteResponse,
            ],
        }
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessPhaseReceiptV1 {
    pub schema: String,
    pub tier: QualificationDerivedAccessTierV1,
    pub operation: QualificationDerivedAccessPhaseOperationV1,
    pub source_identity_sha256: String,
    pub root_identity_sha256: String,
    pub run_identity: String,
    pub semantic_result_sha256: String,
    pub phases: Vec<LongitudinalDerivedAccessPhaseSampleV1>,
    pub receipt_sha256: String,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
impl QualificationDerivedAccessPhaseReceiptV1 {
    pub fn new(
        tier: QualificationDerivedAccessTierV1,
        operation: QualificationDerivedAccessPhaseOperationV1,
        source_identity_sha256: String,
        root_identity_sha256: String,
        run_identity: String,
        semantic_result_sha256: String,
        phases: Vec<LongitudinalDerivedAccessPhaseSampleV1>,
    ) -> Result<Self, String> {
        let mut receipt = Self {
            schema: QUALIFICATION_DERIVED_ACCESS_PHASE_RECEIPT_SCHEMA_V1.to_owned(),
            tier,
            operation,
            source_identity_sha256,
            root_identity_sha256,
            run_identity,
            semantic_result_sha256,
            phases,
            receipt_sha256: String::new(),
        };
        receipt.refresh_sha256()?;
        receipt.validate()?;
        Ok(receipt)
    }

    fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.receipt_sha256.clear();
        serde_json::to_vec(&preimage)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())
    }

    pub fn refresh_sha256(&mut self) -> Result<(), String> {
        self.receipt_sha256 = self.canonical_sha256()?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_ACCESS_PHASE_RECEIPT_SCHEMA_V1 {
            return Err("unsupported derived-access phase receipt".to_owned());
        }
        for (value, label) in [
            (&self.source_identity_sha256, "phase source identity"),
            (&self.root_identity_sha256, "phase root identity"),
            (&self.run_identity, "phase run identity"),
            (&self.semantic_result_sha256, "phase semantic result"),
            (&self.receipt_sha256, "phase receipt"),
        ] {
            validate_digest(value, label)?;
        }
        if self.receipt_sha256 != self.canonical_sha256()? {
            return Err("derived-access phase receipt hash drifted".to_owned());
        }
        let expected = self.operation.expected_phases();
        let maintenance_index = self.phases.iter().position(|sample| {
            sample.phase
                == LongitudinalDerivedAccessPhaseV1::GovernedWriteAuthorityCursorMaintenance
        });
        if self
            .phases
            .iter()
            .filter(|sample| {
                sample.phase
                    == LongitudinalDerivedAccessPhaseV1::GovernedWriteAuthorityCursorMaintenance
            })
            .count()
            > 1
            || maintenance_index.is_some()
                && self.operation != QualificationDerivedAccessPhaseOperationV1::GovernedWrite
        {
            return Err("derived-access authority maintenance phase drifted".to_owned());
        }
        let baseline = self
            .phases
            .iter()
            .filter(|sample| {
                sample.phase
                    != LongitudinalDerivedAccessPhaseV1::GovernedWriteAuthorityCursorMaintenance
            })
            .collect::<Vec<_>>();
        if baseline.len() != expected.len() {
            return Err("derived-access phase receipt is incomplete".to_owned());
        }
        if baseline
            .iter()
            .zip(expected.iter())
            .any(|(sample, expected_phase)| sample.phase != *expected_phase)
        {
            return Err("derived-access phase receipt order drifted".to_owned());
        }
        if let Some(index) = maintenance_index
            && (index == 0
                || index + 1 >= self.phases.len()
                || self.phases[index - 1].phase
                    != LongitudinalDerivedAccessPhaseV1::GovernedWriteCatchUp
                || self.phases[index + 1].phase
                    != LongitudinalDerivedAccessPhaseV1::GovernedWriteResponse
                || self.phases[index].counters.authority_identity_rows_scanned == 0)
        {
            return Err("derived-access authority maintenance phase drifted".to_owned());
        }
        for (index, sample) in self.phases.iter().enumerate() {
            if usize::from(sample.ordinal) != index {
                return Err("derived-access phase receipt order drifted".to_owned());
            }
            let expected_parent = match sample.phase {
                LongitudinalDerivedAccessPhaseV1::RevisionPageSnapshotSummaries => Some(5_u16),
                LongitudinalDerivedAccessPhaseV1::GovernedWriteAuthorityCursorMaintenance => {
                    Some(self.phases[index - 1].ordinal)
                }
                _ => None,
            };
            if sample.parent_ordinal != expected_parent {
                return Err("derived-access phase nesting drifted".to_owned());
            }
            if sample.ownership != sample.phase.ownership() {
                return Err("derived-access phase ownership drifted".to_owned());
            }
            if sample.wall_nanos == u64::MAX
                || sample.process_cpu_nanos == Some(u64::MAX)
                || phase_counters_overflowed(&sample.counters)
            {
                return Err("derived-access phase measurement overflowed".to_owned());
            }
            match (
                sample.resident_bytes_before,
                sample.resident_bytes_after,
                sample.resident_bytes_observed_max,
            ) {
                (None, None, None) => {}
                (Some(before), Some(after), Some(high_water))
                    if high_water >= before && high_water >= after => {}
                _ => return Err("derived-access phase RSS snapshot is inconsistent".to_owned()),
            }
        }
        Ok(())
    }

    pub fn validate_against(
        &self,
        source_identity_sha256: &str,
        tier: QualificationDerivedAccessTierV1,
        operation: QualificationDerivedAccessPhaseOperationV1,
    ) -> Result<(), String> {
        self.validate()?;
        if self.source_identity_sha256 != source_identity_sha256
            || self.tier != tier
            || self.operation != operation
        {
            return Err("derived-access phase receipt authority drifted".to_owned());
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
fn phase_counters_overflowed(counters: &LongitudinalCountersV1) -> bool {
    [
        counters.directory_entries_walked,
        counters.carrier_opens,
        counters.carrier_bytes_read,
        counters.authority_identity_rows_scanned,
        counters.change_candidates,
        counters.change_candidate_current_revisions,
        counters.change_capability_carriers_opened,
        counters.change_proposal_carriers_opened,
        counters.change_proposal_carriers_validated,
        counters.change_support_carriers_opened,
        counters.change_matches,
        counters.change_rows_emitted,
        counters.authoritative_fallbacks,
        counters.full_history_fallbacks,
        counters.event_decodes,
        counters.event_validations,
        counters.event_folds,
        counters.chronological_sort_items,
        counters.body_artifact_reads,
        counters.body_bytes_read,
        counters.object_artifact_reads,
        counters.object_bytes_read,
        counters.projection_rebuilds,
        counters.state_rebuilds,
        counters.response_bytes,
    ]
    .contains(&u64::MAX)
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessPhaseBundleEntryV1 {
    pub tier: QualificationDerivedAccessTierV1,
    pub operation: QualificationDerivedAccessPhaseOperationV1,
    pub receipt_sha256: String,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessPhaseBundleV1 {
    pub schema: String,
    pub source_identity_sha256: String,
    pub tier: QualificationDerivedAccessTierV1,
    pub entries: Vec<QualificationDerivedAccessPhaseBundleEntryV1>,
    pub raw_receipts: Vec<QualificationDerivedAccessPhaseReceiptV1>,
    pub complete: bool,
    pub bundle_sha256: String,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
impl QualificationDerivedAccessPhaseBundleV1 {
    pub fn new(
        source_identity_sha256: String,
        tier: QualificationDerivedAccessTierV1,
        raw_receipts: Vec<QualificationDerivedAccessPhaseReceiptV1>,
    ) -> Result<Self, String> {
        let entries = raw_receipts
            .iter()
            .map(|receipt| QualificationDerivedAccessPhaseBundleEntryV1 {
                tier: receipt.tier,
                operation: receipt.operation,
                receipt_sha256: receipt.receipt_sha256.clone(),
            })
            .collect();
        let mut bundle = Self {
            schema: QUALIFICATION_DERIVED_ACCESS_PHASE_BUNDLE_SCHEMA_V1.to_owned(),
            source_identity_sha256,
            tier,
            entries,
            raw_receipts,
            complete: true,
            bundle_sha256: String::new(),
        };
        bundle.refresh_sha256()?;
        bundle.validate()?;
        Ok(bundle)
    }

    fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.bundle_sha256.clear();
        serde_json::to_vec(&preimage)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())
    }

    pub fn refresh_sha256(&mut self) -> Result<(), String> {
        self.bundle_sha256 = self.canonical_sha256()?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_ACCESS_PHASE_BUNDLE_SCHEMA_V1
            || !self.complete
            || self.entries.is_empty()
            || self.entries.len() != self.raw_receipts.len()
        {
            return Err("derived-access phase bundle is incomplete".to_owned());
        }
        validate_digest(&self.source_identity_sha256, "phase bundle source identity")?;
        validate_digest(&self.bundle_sha256, "phase bundle")?;
        if self.bundle_sha256 != self.canonical_sha256()? {
            return Err("derived-access phase bundle hash drifted".to_owned());
        }
        let mut subjects = BTreeSet::new();
        for (entry, receipt) in self.entries.iter().zip(&self.raw_receipts) {
            validate_digest(&entry.receipt_sha256, "phase bundle entry")?;
            receipt.validate_against(&self.source_identity_sha256, entry.tier, entry.operation)?;
            if entry.receipt_sha256 != receipt.receipt_sha256
                || entry.tier != self.tier
                || !subjects.insert((entry.tier, entry.operation))
            {
                return Err("derived-access phase bundle entry drifted".to_owned());
            }
        }
        if subjects
            != QualificationDerivedAccessPhaseOperationV1::ALL
                .into_iter()
                .map(|operation| (self.tier, operation))
                .collect()
        {
            return Err("derived-access phase bundle omitted an operation".to_owned());
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessPhaseRunRequestV1 {
    pub schema: String,
    pub source_checkout: PathBuf,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub tier: QualificationDerivedAccessTierV1,
    pub immutable_input_root: PathBuf,
    pub root: PathBuf,
    pub root_identity_sha256: String,
    pub request_sha256: String,
}

#[cfg(any(test, feature = "longitudinal-counting"))]
impl QualificationDerivedAccessPhaseRunRequestV1 {
    fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.request_sha256.clear();
        serde_json::to_vec(&preimage)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())
    }

    pub fn refresh_sha256(&mut self) -> Result<(), String> {
        self.request_sha256 = self.canonical_sha256()?;
        Ok(())
    }

    pub fn source_identity_sha256(&self) -> Result<String, String> {
        serde_json::to_vec(&self.execution)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_ACCESS_PHASE_REQUEST_SCHEMA_V1
            || !self.source_checkout.is_absolute()
            || !self.immutable_input_root.is_absolute()
            || !self.root.is_absolute()
            || self.source_checkout == self.root
            || self.source_checkout == self.immutable_input_root
            || self.immutable_input_root == self.root
            || !matches!(
                self.tier,
                QualificationDerivedAccessTierV1::D0_128
                    | QualificationDerivedAccessTierV1::L7
                    | QualificationDerivedAccessTierV1::L100
            )
        {
            return Err("invalid derived-access phase request".to_owned());
        }
        self.execution.validate()?;
        validate_digest(&self.root_identity_sha256, "phase request root identity")?;
        validate_digest(&self.request_sha256, "phase request")?;
        if self.request_sha256 != self.canonical_sha256()? {
            return Err("derived-access phase request hash drifted".to_owned());
        }
        Ok(())
    }
}

#[cfg(feature = "longitudinal-counting")]
pub fn run_qualification_derived_access_phase_v1(
    request_path: &Path,
) -> Result<QualificationDerivedAccessPhaseBundleV1, String> {
    use std::sync::Arc;

    use crate::session::derived_access::history::DerivedHistoryAccess;
    use crate::session::derived_access::lifecycle::{DerivedAccessLifecycle, LifecycleControl};
    use crate::session::derived_access::product_contract::DerivedAccessProfile;
    use crate::session::derived_access::revisions::{
        DerivedRevisionPageRoute, RevisionPageRequest,
    };
    use crate::session::derived_access::writer::DerivedWriteCoordinator;
    use crate::session::{EventWriteOutcome, SnapshotSummaryCache, TrustSet, opaque_path_identity};

    let request: QualificationDerivedAccessPhaseRunRequestV1 = read_json(request_path)?;
    request.validate()?;
    if DerivedAccessProfile::from_environment().map_err(|error| error.to_string())?
        != DerivedAccessProfile::SqliteWalBodylessV1
    {
        return Err(
            "phase evidence requires POINTBREAK_DERIVED_ACCESS=sqlite-wal-bodyless-v1".to_owned(),
        );
    }
    let immutable_input_root =
        std::fs::canonicalize(&request.immutable_input_root).map_err(|error| error.to_string())?;
    let mutable_root = std::fs::canonicalize(&request.root).map_err(|error| error.to_string())?;
    if immutable_input_root == mutable_root
        || immutable_input_root.starts_with(&mutable_root)
        || mutable_root.starts_with(&immutable_input_root)
    {
        return Err("derived-access phase roots must be filesystem-disjoint".to_owned());
    }
    validate_current_execution_identity_v1(
        &request.execution,
        &request.source_checkout,
        &request.root,
    )?;
    let inventory = longitudinal_authoritative_store_data_inventory_v1(&request.root)
        .map_err(|error| error.to_string())?;
    let immutable_inventory =
        longitudinal_authoritative_store_data_inventory_v1(&request.immutable_input_root)
            .map_err(|error| error.to_string())?;
    if inventory.inventory_sha256 != request.root_identity_sha256
        || immutable_inventory.inventory_sha256 != request.root_identity_sha256
        || inventory != immutable_inventory
    {
        return Err("derived-access phase root identity drifted".to_owned());
    }
    let source_identity_sha256 = request.source_identity_sha256()?;
    let store_root = store_dir_for_repo(&request.root).map_err(|error| error.to_string())?;
    let store_identity =
        opaque_path_identity("store", &store_root).map_err(|error| error.to_string())?;
    let root_identity_sha256 = request.root_identity_sha256.clone();
    let mut receipts = Vec::new();

    // Bootstrap is intentionally first: the later page and governed-write
    // probes must observe the exact generation produced by this same-source
    // attribution run rather than inherited disposable state.
    let bootstrap_run_identity = phase_run_identity(
        request.tier,
        QualificationDerivedAccessPhaseOperationV1::Bootstrap,
        &request.request_sha256,
    );
    let bootstrap_scope = LongitudinalCountingScopeV1::new(bootstrap_run_identity.clone())?;
    let bootstrap_guard = bootstrap_scope.enter();
    let bootstrap = DerivedAccessLifecycle::new(
        DerivedAccessProfile::SqliteWalBodylessV1,
        &store_root,
        store_identity.clone(),
    )
    .map_err(|error| error.to_string())?
    .rebuild(|_| LifecycleControl::Continue)
    .map_err(|error| error.to_string())?;
    drop(bootstrap_guard);
    let bootstrap_semantic = bootstrap
        .semantic_receipt
        .ok_or_else(|| "phase bootstrap omitted its semantic receipt".to_owned())?;
    receipts.push(QualificationDerivedAccessPhaseReceiptV1::new(
        request.tier,
        QualificationDerivedAccessPhaseOperationV1::Bootstrap,
        source_identity_sha256.clone(),
        root_identity_sha256.clone(),
        bootstrap_run_identity,
        digest_text(&bootstrap_semantic),
        bootstrap_scope.snapshot().derived_access_phases,
    )?);

    let page_run_identity = phase_run_identity(
        request.tier,
        QualificationDerivedAccessPhaseOperationV1::RevisionPage,
        &request.request_sha256,
    );
    let page_scope = LongitudinalCountingScopeV1::new(page_run_identity.clone())?;
    let access = DerivedHistoryAccess::resolve(&request.root)?;
    debug_assert!(access.is_active());
    let page_guard = page_scope.enter();
    let page = access.revisions_page(
        &request.root,
        TrustSet::default(),
        Arc::new(SnapshotSummaryCache::new()),
        &RevisionPageRequest::new(Some(100), None)
            .map_err(|_| "phase revision-page request is invalid".to_owned())?,
    )?;
    drop(page_guard);
    let DerivedRevisionPageRoute::Ready(page) = page else {
        return Err("phase revision-page route was not ready".to_owned());
    };
    let page_semantic = serde_json::to_vec(&serde_json::json!({
        "projectionStamp": page.projection_stamp,
        "next": page.next,
        "rowsSelected": page.work.rows_selected,
        "entries": page
            .result
            .entries
            .iter()
            .map(|entry| entry.revision_id.as_str())
            .collect::<Vec<_>>(),
        "overviews": page
            .overviews
            .keys()
            .map(|revision_id| revision_id.as_str())
            .collect::<Vec<_>>(),
    }))
    .map_err(|error| error.to_string())?;
    receipts.push(QualificationDerivedAccessPhaseReceiptV1::new(
        request.tier,
        QualificationDerivedAccessPhaseOperationV1::RevisionPage,
        source_identity_sha256.clone(),
        root_identity_sha256.clone(),
        page_run_identity,
        sha256_bytes_hex(&page_semantic),
        page_scope.snapshot().derived_access_phases,
    )?);

    // Event construction is outside the scope; the governed-write phases begin
    // at admission and retain the authoritative loose publish as a separately
    // typed truth-owned sample.
    let write_event = phase_attribution_event(&request.request_sha256)?;
    let write_run_identity = phase_run_identity(
        request.tier,
        QualificationDerivedAccessPhaseOperationV1::GovernedWrite,
        &request.request_sha256,
    );
    let write_scope = LongitudinalCountingScopeV1::new(write_run_identity.clone())?;
    let write_lifecycle = DerivedAccessLifecycle::new(
        DerivedAccessProfile::SqliteWalBodylessV1,
        &store_root,
        store_identity,
    )
    .map_err(|error| error.to_string())?;
    let coordinator =
        DerivedWriteCoordinator::new(write_lifecycle).map_err(|error| error.to_string())?;
    let governed = EventStore::open(&store_root).with_coordinator(coordinator);
    let write_guard = write_scope.enter();
    let outcome = governed
        .record_event_once(&write_event)
        .map_err(|error| error.to_string())?;
    drop(write_guard);
    if outcome != EventWriteOutcome::Created {
        return Err("phase governed write did not create exactly one event".to_owned());
    }
    receipts.push(QualificationDerivedAccessPhaseReceiptV1::new(
        request.tier,
        QualificationDerivedAccessPhaseOperationV1::GovernedWrite,
        source_identity_sha256.clone(),
        root_identity_sha256,
        write_run_identity,
        digest_text(&format!("{}:{outcome:?}", write_event.event_id.as_str())),
        write_scope.snapshot().derived_access_phases,
    )?);

    let immutable_after =
        longitudinal_authoritative_store_data_inventory_v1(&request.immutable_input_root)
            .map_err(|error| error.to_string())?;
    if immutable_after != immutable_inventory {
        return Err("derived-access phase immutable input changed during the run".to_owned());
    }

    QualificationDerivedAccessPhaseBundleV1::new(source_identity_sha256, request.tier, receipts)
}

#[cfg(not(feature = "longitudinal-counting"))]
pub fn run_qualification_derived_access_phase_v1(
    _request_path: &Path,
) -> Result<serde_json::Value, String> {
    Err("phase evidence requires --features longitudinal-counting".to_owned())
}

#[cfg(any(test, feature = "longitudinal-counting"))]
pub fn verify_qualification_derived_access_phase_v1(
    request_path: &Path,
    bundle_path: &Path,
) -> Result<QualificationDerivedAccessPhaseBundleV1, String> {
    reject_derived_change_diagnostic_evidence_path_v1(bundle_path)?;
    let request: QualificationDerivedAccessPhaseRunRequestV1 = read_json(request_path)?;
    let bundle: QualificationDerivedAccessPhaseBundleV1 = read_json(bundle_path)?;
    request.validate()?;
    bundle.validate()?;
    if bundle.source_identity_sha256 != request.source_identity_sha256()?
        || bundle.tier != request.tier
        || bundle
            .raw_receipts
            .iter()
            .any(|receipt| receipt.root_identity_sha256 != request.root_identity_sha256)
    {
        return Err("derived-access phase bundle does not match its request".to_owned());
    }
    Ok(bundle)
}

#[cfg(not(any(test, feature = "longitudinal-counting")))]
pub fn verify_qualification_derived_access_phase_v1(
    _request_path: &Path,
    _bundle_path: &Path,
) -> Result<serde_json::Value, String> {
    Err("phase verification requires --features longitudinal-counting".to_owned())
}

#[cfg(feature = "longitudinal-counting")]
fn phase_run_identity(
    tier: QualificationDerivedAccessTierV1,
    operation: QualificationDerivedAccessPhaseOperationV1,
    request_sha256: &str,
) -> String {
    digest_text(&format!("{tier:?}/{operation:?}/{request_sha256}"))
}

#[cfg(feature = "longitudinal-counting")]
fn digest_text(value: &str) -> String {
    sha256_bytes_hex(value.as_bytes())
}

#[cfg(feature = "longitudinal-counting")]
fn phase_attribution_event(request_sha256: &str) -> Result<ShoreEvent, String> {
    let journal_id = JournalId::new(format!(
        "journal:phase-attribution:{}",
        &request_sha256[..16]
    ));
    ShoreEvent::new(
        EventType::ReviewInitialized,
        ReviewInitializedPayload::idempotency_key(&journal_id),
        EventTarget::for_journal(journal_id),
        Writer::shore_local("phase-attribution"),
        ReviewInitializedPayload {},
        "2026-08-01T00:00:00Z",
    )
    .map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessSqliteCountersV1 {
    pub selected_rows: u64,
    pub page_count: u64,
    pub database_size_delta_bytes: u64,
    pub wal_size_delta_bytes: u64,
    pub shared_memory_size_delta_bytes: u64,
    pub temporary_size_delta_bytes: u64,
    pub checkpoint_rows_written: u64,
    pub delta_rows_applied: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessRawSampleV1 {
    pub schema: String,
    pub tier: QualificationDerivedAccessTierV1,
    pub operation: QualificationDerivedAccessOperationV1,
    pub sample_index: u16,
    pub retained: bool,
    pub status: QualificationDerivedAccessStatusV1,
    pub semantic_receipt_sha256: String,
    pub semantic_receipt_matches: bool,
    pub wall_nanos: u64,
    pub process_cpu_nanos: Option<u64>,
    pub process_cpu_unit: QualificationDerivedAccessCpuUnitV1,
    pub process_scope: Option<QualificationDerivedAccessProcessScopeV1>,
    pub selected_output_count: u64,
    pub selected_work_count: u64,
    pub retained_cardinality: u64,
    pub authoritative_bytes_published: u64,
    pub whole_history_work: bool,
    pub complexity: QualificationDerivedAccessComplexityV1,
    pub counters: QualificationDerivedAccessCountersV1,
    pub sqlite: QualificationDerivedAccessSqliteCountersV1,
    pub response_sha256: String,
}

impl QualificationDerivedAccessRawSampleV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_ACCESS_RAW_SAMPLE_SCHEMA_V1 {
            return Err("unsupported derived-access raw sample".to_owned());
        }
        validate_digest(&self.semantic_receipt_sha256, "semantic receipt")?;
        validate_digest(&self.response_sha256, "response")?;
        if !self.semantic_receipt_matches
            || self.status != QualificationDerivedAccessStatusV1::Passed
        {
            return Err("derived-access semantics failed before timing admission".to_owned());
        }
        if self.process_cpu_unit != QualificationDerivedAccessCpuUnitV1::Nanoseconds
            || self.process_scope.is_none()
            || self.process_cpu_nanos.is_none()
        {
            return Err(
                "derived-access process CPU must name nanosecond units and process scope"
                    .to_owned(),
            );
        }
        let proportional = self.whole_history_work
            || self.selected_output_count == 0
            || (self.retained_cardinality > self.selected_output_count
                && self.selected_work_count >= self.retained_cardinality);
        let expected = if proportional {
            QualificationDerivedAccessComplexityV1::HistoryOrCardinalityProportional
        } else {
            QualificationDerivedAccessComplexityV1::BoundedSelectedWork
        };
        if self.complexity != expected {
            return Err(
                "derived-access complexity classification does not match measured work".to_owned(),
            );
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn test_fixture() -> Self {
        Self {
            schema: QUALIFICATION_DERIVED_ACCESS_RAW_SAMPLE_SCHEMA_V1.to_owned(),
            tier: QualificationDerivedAccessTierV1::D0_128,
            operation: QualificationDerivedAccessOperationV1::FreshNoChange,
            sample_index: 0,
            retained: true,
            status: QualificationDerivedAccessStatusV1::Passed,
            semantic_receipt_sha256: "11".repeat(32),
            semantic_receipt_matches: true,
            wall_nanos: 1,
            process_cpu_nanos: Some(1),
            process_cpu_unit: QualificationDerivedAccessCpuUnitV1::Nanoseconds,
            process_scope: Some(QualificationDerivedAccessProcessScopeV1::InspectorServiceChild),
            selected_output_count: 1,
            selected_work_count: 1,
            retained_cardinality: 128,
            authoritative_bytes_published: 0,
            whole_history_work: false,
            complexity: QualificationDerivedAccessComplexityV1::BoundedSelectedWork,
            counters: QualificationDerivedAccessCountersV1 {
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
            },
            sqlite: QualificationDerivedAccessSqliteCountersV1 {
                selected_rows: 1,
                page_count: 1,
                database_size_delta_bytes: 0,
                wal_size_delta_bytes: 0,
                shared_memory_size_delta_bytes: 0,
                temporary_size_delta_bytes: 0,
                checkpoint_rows_written: 0,
                delta_rows_applied: 0,
            },
            response_sha256: "22".repeat(32),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessDerivedInventoryV1 {
    pub database_bytes: u64,
    pub wal_bytes: u64,
    pub shared_memory_bytes: u64,
    pub temporary_bytes: u64,
    pub row_count: u64,
    pub page_count: u64,
    pub body_bytes: u64,
    pub object_bytes: u64,
    pub high_water_bytes: u64,
}

impl QualificationDerivedAccessDerivedInventoryV1 {
    pub fn validate_bodyless(&self) -> Result<(), String> {
        if self.body_bytes != 0 || self.object_bytes != 0 {
            return Err("derived-access inventory contains body or object bytes".to_owned());
        }
        if self.high_water_bytes
            < self
                .database_bytes
                .saturating_add(self.wal_bytes)
                .saturating_add(self.shared_memory_bytes)
                .saturating_add(self.temporary_bytes)
        {
            return Err("derived-access high-water inventory is inconsistent".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessExpectedAuthorityV1 {
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessEvidenceShardV1 {
    pub schema: String,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub raw_samples: Vec<QualificationDerivedAccessRawSampleV1>,
    pub inventory: QualificationDerivedAccessDerivedInventoryV1,
    pub source_root_before_sha256: String,
    pub source_root_after_sha256: String,
}

impl QualificationDerivedAccessEvidenceShardV1 {
    pub fn validate_against(
        &self,
        authority: &QualificationDerivedAccessExpectedAuthorityV1,
    ) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_ACCESS_EVIDENCE_SHARD_SCHEMA_V1 {
            return Err("unsupported derived-access evidence shard".to_owned());
        }
        if self.execution != authority.execution {
            return Err("derived-access evidence authority drifted".to_owned());
        }
        validate_digest(&self.source_root_before_sha256, "source root before")?;
        validate_digest(&self.source_root_after_sha256, "source root after")?;
        if self.source_root_before_sha256 != self.source_root_after_sha256 {
            return Err("derived-access source root mutated during evidence collection".to_owned());
        }
        self.inventory.validate_bodyless()?;
        for sample in &self.raw_samples {
            sample.validate()?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn test_fixture() -> Self {
        let execution = test_execution_identity();
        Self {
            schema: QUALIFICATION_DERIVED_ACCESS_EVIDENCE_SHARD_SCHEMA_V1.to_owned(),
            execution,
            raw_samples: vec![QualificationDerivedAccessRawSampleV1::test_fixture()],
            inventory: QualificationDerivedAccessDerivedInventoryV1 {
                database_bytes: 1,
                wal_bytes: 0,
                shared_memory_bytes: 0,
                temporary_bytes: 0,
                row_count: 1,
                page_count: 1,
                body_bytes: 0,
                object_bytes: 0,
                high_water_bytes: 1,
            },
            source_root_before_sha256: "33".repeat(32),
            source_root_after_sha256: "33".repeat(32),
        }
    }
}

impl QualificationDerivedAccessExpectedAuthorityV1 {
    #[cfg(test)]
    pub(crate) fn test_fixture() -> Self {
        Self {
            execution: test_execution_identity(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessRetainedRootRequestV1 {
    pub schema: String,
    pub source_checkout: PathBuf,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub root_authority_sha256: String,
    pub tier: QualificationDerivedAccessTierV1,
    pub immutable_input_root: PathBuf,
    pub qualification_clone_root: PathBuf,
    pub admitted_root_sha256: String,
    pub immutable_input: bool,
    pub materialize: bool,
}

impl QualificationDerivedAccessRetainedRootRequestV1 {
    pub fn new(
        source_checkout: impl AsRef<Path>,
        execution: QualificationDerivedAccessExecutionIdentityV1,
        tier: QualificationDerivedAccessTierV1,
        immutable_input_root: impl AsRef<Path>,
        qualification_clone_root: impl AsRef<Path>,
        admitted_root_sha256: String,
    ) -> Self {
        Self {
            schema: QUALIFICATION_DERIVED_ACCESS_RETAINED_ROOT_SCHEMA_V1.to_owned(),
            source_checkout: source_checkout.as_ref().to_path_buf(),
            root_authority_sha256: execution.root_provenance_sha256.clone(),
            execution,
            tier,
            immutable_input_root: immutable_input_root.as_ref().to_path_buf(),
            qualification_clone_root: qualification_clone_root.as_ref().to_path_buf(),
            admitted_root_sha256,
            immutable_input: true,
            materialize: false,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_ACCESS_RETAINED_ROOT_SCHEMA_V1
            || !matches!(
                self.tier,
                QualificationDerivedAccessTierV1::L7
                    | QualificationDerivedAccessTierV1::L100
                    | QualificationDerivedAccessTierV1::C262
            )
            || !self.immutable_input
            || self.materialize
            || self.immutable_input_root == self.qualification_clone_root
        {
            return Err("invalid retained derived-access root request".to_owned());
        }
        self.execution.validate()?;
        validate_digest(&self.root_authority_sha256, "retained-root authority")?;
        if self.root_authority_sha256 != self.execution.root_provenance_sha256 {
            return Err("retained-root request and execution authority differ".to_owned());
        }
        validate_digest(&self.admitted_root_sha256, "admitted root")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessRetainedPreflightReceiptV1 {
    pub schema: String,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub tier: QualificationDerivedAccessTierV1,
    pub admitted_root_sha256: String,
    pub immutable_inventory: LongitudinalStoreDataInventoryV1,
    pub qualification_clone_inventory: Option<LongitudinalStoreDataInventoryV1>,
    pub originals_unchanged: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessRetainedBootstrapReceiptV1 {
    pub schema: String,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub tier: QualificationDerivedAccessTierV1,
    pub admitted_root_sha256: String,
    pub immutable_before: LongitudinalStoreDataInventoryV1,
    pub immutable_after: LongitudinalStoreDataInventoryV1,
    pub clone_truth_before: LongitudinalStoreDataInventoryV1,
    pub clone_truth_after: LongitudinalStoreDataInventoryV1,
    pub progress_updates: u64,
    pub progress_completed: u64,
    pub progress_total: u64,
    pub elapsed_millis: u64,
    pub high_water_derived_bytes: u64,
    pub semantic_receipt_sha256: String,
    pub full_replay_matches_incremental: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessScaleRootV1 {
    pub root: PathBuf,
    pub admitted_root_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessScaleRunRequestV1 {
    pub schema: String,
    pub tier: QualificationDerivedAccessTierV1,
    pub source_checkout: PathBuf,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub root_authority_sha256: String,
    pub roots: Vec<QualificationDerivedAccessScaleRootV1>,
    pub l100_selected_work: BTreeMap<QualificationDerivedAccessOperationV1, u64>,
}

impl QualificationDerivedAccessScaleRunRequestV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_ACCESS_SCALE_REQUEST_SCHEMA_V1
            || !matches!(
                self.tier,
                QualificationDerivedAccessTierV1::L100 | QualificationDerivedAccessTierV1::C262
            )
            || self.roots.len() != 2
            || self.roots[0].root == self.roots[1].root
        {
            return Err("invalid derived-access scale run request".to_owned());
        }
        self.execution.validate()?;
        validate_digest(&self.root_authority_sha256, "scale root authority")?;
        if self.root_authority_sha256 != self.execution.root_provenance_sha256 {
            return Err("scale request and execution root authority differ".to_owned());
        }
        for root in &self.roots {
            validate_digest(&root.admitted_root_sha256, "scale root")?;
        }
        match self.tier {
            QualificationDerivedAccessTierV1::L100 if !self.l100_selected_work.is_empty() => {
                return Err("L100 scale evidence cannot supply L100 comparison scalars".to_owned());
            }
            QualificationDerivedAccessTierV1::C262
                if self.l100_selected_work.len()
                    != QualificationDerivedAccessOperationV1::ALL.len()
                    || QualificationDerivedAccessOperationV1::ALL
                        .iter()
                        .any(|operation| !self.l100_selected_work.contains_key(operation)) =>
            {
                return Err(
                    "C262 scale evidence requires every L100 selected-work scalar".to_owned(),
                );
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessScaleReceiptV1 {
    pub schema: String,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub tier: QualificationDerivedAccessTierV1,
    pub l100_selected_work: BTreeMap<QualificationDerivedAccessOperationV1, u64>,
    pub raw_samples: Vec<QualificationDerivedAccessRawSampleV1>,
    pub operation_rows: Vec<super::QualificationDerivedAccessOperationEvidenceV1>,
    pub allocation: super::QualificationDerivedAccessAllocationEvidenceV1,
    pub derived_inventories: Vec<QualificationDerivedAccessDerivedInventoryV1>,
    pub root_before: Vec<LongitudinalStoreDataInventoryV1>,
    pub root_after: Vec<LongitudinalStoreDataInventoryV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessNativeSmokeRunRequestV1 {
    pub schema: String,
    pub source_checkout: PathBuf,
    pub workspace_root: PathBuf,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub tier: QualificationDerivedAccessTierV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "tier", content = "receipt")]
pub enum QualificationDerivedAccessNativeSmokePayloadV1 {
    #[serde(rename = "D0-128")]
    D0_128(Box<super::QualificationDerivedAccessSmokeReceiptV1>),
    #[serde(rename = "L1-L7")]
    Longitudinal(super::QualificationDerivedAccessLongitudinalSmokeReceiptV1),
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessNativeSmokeRunReceiptV1 {
    pub schema: String,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub payload: QualificationDerivedAccessNativeSmokePayloadV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedChangeDiagnosticNativeResultV1 {
    pub mode: String,
    pub tier: QualificationDerivedAccessTierV1,
    pub admitted_root_path: PathBuf,
    pub admitted_root_sha256: String,
    pub source_unchanged: bool,
}

pub fn run_qualification_derived_access_native_smoke_v1(
    request_path: &Path,
) -> Result<QualificationDerivedAccessNativeSmokeRunReceiptV1, String> {
    run_qualification_derived_access_native_smoke_with_validator_v1(
        request_path,
        validate_current_execution_identity_v1,
    )
}

fn run_qualification_derived_access_native_smoke_with_validator_v1(
    request_path: &Path,
    validate_execution: impl FnOnce(
        &QualificationDerivedAccessExecutionIdentityV1,
        &Path,
        &Path,
    ) -> Result<(), String>,
) -> Result<QualificationDerivedAccessNativeSmokeRunReceiptV1, String> {
    let request: QualificationDerivedAccessNativeSmokeRunRequestV1 = read_json(request_path)?;
    if request.schema != QUALIFICATION_DERIVED_ACCESS_NATIVE_SMOKE_REQUEST_SCHEMA_V1
        || !matches!(
            request.tier,
            QualificationDerivedAccessTierV1::D0_128
                | QualificationDerivedAccessTierV1::L1
                | QualificationDerivedAccessTierV1::L7
        )
    {
        return Err("invalid derived-access native smoke request".to_owned());
    }
    validate_execution(
        &request.execution,
        &request.source_checkout,
        &request.workspace_root,
    )?;
    let payload = match request.tier {
        QualificationDerivedAccessTierV1::D0_128 => {
            QualificationDerivedAccessNativeSmokePayloadV1::D0_128(Box::new(
                super::run_qualification_derived_access_non_timing_smoke_at_v1(
                    &request.workspace_root,
                )?,
            ))
        }
        QualificationDerivedAccessTierV1::L1 | QualificationDerivedAccessTierV1::L7 => {
            QualificationDerivedAccessNativeSmokePayloadV1::Longitudinal(
                super::run_qualification_derived_access_longitudinal_smoke_at_v1(
                    request.tier,
                    &request.workspace_root,
                )?,
            )
        }
        _ => unreachable!("validated native smoke tier"),
    };
    Ok(QualificationDerivedAccessNativeSmokeRunReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_NATIVE_SMOKE_RECEIPT_SCHEMA_V1.to_owned(),
        execution: request.execution,
        payload,
    })
}

/// Run one disposable native smoke tier and expose only the admitted root
/// needed by the separate diagnostic collector.
pub fn run_derived_change_diagnostic_native_v1(
    request_path: &Path,
) -> Result<DerivedChangeDiagnosticNativeResultV1, String> {
    run_derived_change_diagnostic_native_with_validator_v1(
        request_path,
        validate_current_execution_identity_v1,
    )
}

fn run_derived_change_diagnostic_native_with_validator_v1(
    request_path: &Path,
    validate_execution: impl FnOnce(
        &QualificationDerivedAccessExecutionIdentityV1,
        &Path,
        &Path,
    ) -> Result<(), String>,
) -> Result<DerivedChangeDiagnosticNativeResultV1, String> {
    let request: QualificationDerivedAccessNativeSmokeRunRequestV1 = read_json(request_path)?;
    if request.schema != QUALIFICATION_DERIVED_ACCESS_NATIVE_SMOKE_REQUEST_SCHEMA_V1
        || !matches!(
            request.tier,
            QualificationDerivedAccessTierV1::D0_128
                | QualificationDerivedAccessTierV1::L1
                | QualificationDerivedAccessTierV1::L7
        )
    {
        return Err("invalid derived-Change diagnostic native request".to_owned());
    }
    run_qualification_derived_access_native_smoke_with_validator_v1(
        request_path,
        validate_execution,
    )?;
    let admitted_root_path = request.workspace_root.join("root-a");
    let source_before = longitudinal_authoritative_store_data_inventory_v1(&admitted_root_path)
        .map_err(|error| error.to_string())?;
    let admitted_root_sha256 = source_before.inventory_sha256.clone();
    let source_after = longitudinal_authoritative_store_data_inventory_v1(&admitted_root_path)
        .map_err(|error| error.to_string())?;
    let source_unchanged = source_before == source_after;
    if !source_unchanged {
        return Err(
            "derived-Change diagnostic native collector mutated its admitted root".to_owned(),
        );
    }
    Ok(DerivedChangeDiagnosticNativeResultV1 {
        mode: super::DERIVED_CHANGE_DIAGNOSTIC_NATIVE_MODE_V1.to_owned(),
        tier: request.tier,
        admitted_root_path,
        admitted_root_sha256,
        source_unchanged,
    })
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
pub(super) fn run_derived_change_diagnostic_native_for_test_v1(
    request_path: &Path,
    host_identity_sha256: String,
) -> Result<DerivedChangeDiagnosticNativeResultV1, String> {
    run_derived_change_diagnostic_native_with_validator_v1(
        request_path,
        move |expected, source_checkout, evidence_root| {
            validate_current_execution_identity_with_host_authority_v1(
                expected,
                source_checkout,
                evidence_root,
                &host_identity_sha256,
            )
        },
    )
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessResourceRootV1 {
    pub tier: QualificationDerivedAccessTierV1,
    pub root: PathBuf,
    pub admitted_root_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessResourceRunRequestV1 {
    pub schema: String,
    pub source_checkout: PathBuf,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub root_authority_sha256: String,
    pub roots: Vec<QualificationDerivedAccessResourceRootV1>,
}

impl QualificationDerivedAccessResourceRunRequestV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_ACCESS_RESOURCE_REQUEST_SCHEMA_V1
            || self.execution.platform != super::QualificationDerivedAccessPlatformV1::MacosApfs
            || self.roots.len() != 2
            || self.roots[0].root == self.roots[1].root
            || self
                .roots
                .iter()
                .map(|root| root.tier)
                .collect::<BTreeSet<_>>()
                != BTreeSet::from([
                    QualificationDerivedAccessTierV1::L7,
                    QualificationDerivedAccessTierV1::L100,
                ])
        {
            return Err("invalid derived-access resource request".to_owned());
        }
        self.execution.validate()?;
        validate_digest(&self.root_authority_sha256, "resource root authority")?;
        if self.root_authority_sha256 != self.execution.root_provenance_sha256 {
            return Err("resource request and execution root authority differ".to_owned());
        }
        for root in &self.roots {
            validate_digest(&root.admitted_root_sha256, "resource root")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessResourceChildRequestV1 {
    pub schema: String,
    pub root: Option<PathBuf>,
    pub store_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessResourceChildReceiptV1 {
    pub schema: String,
    pub baseline_only: bool,
    pub steady_rss_bytes: u64,
    pub peak_rss_bytes: u64,
    pub retained_body_object_bytes: u64,
    pub semantic_receipt_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessRestartChildRequestV1 {
    pub schema: String,
    pub root: PathBuf,
    pub store_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessRestartChildReceiptV1 {
    pub schema: String,
    pub process_cpu_nanos: u64,
    pub resident_bytes: u64,
    pub counters: QualificationDerivedAccessCountersV1,
    pub semantic_receipt_sha256: String,
}

#[cfg(feature = "longitudinal-counting")]
pub fn run_qualification_derived_access_restart_child_v1(
    request_path: &Path,
) -> Result<QualificationDerivedAccessRestartChildReceiptV1, String> {
    let request: QualificationDerivedAccessRestartChildRequestV1 = read_json(request_path)?;
    if request.schema != QUALIFICATION_DERIVED_ACCESS_RESTART_CHILD_REQUEST_SCHEMA_V1
        || request.root.as_os_str().is_empty()
        || request.store_id.trim().is_empty()
    {
        return Err("invalid derived-access restart child request".to_owned());
    }
    let before = capture_longitudinal_process_snapshot_v1(std::process::id())
        .map_err(|error| error.to_string())?;
    let scope = LongitudinalCountingScopeV1::new(sha256_bytes_hex(
        format!("{}/{}", request.root.display(), request.store_id).as_bytes(),
    ))?;
    let guard = scope.enter();
    let store = store_dir_for_repo(&request.root).map_err(|error| error.to_string())?;
    let adapter = QualificationDerivedAccessAdapter::open(
        &store,
        CursorLedgerIdentity::new(request.store_id),
    )
    .map_err(|error| error.to_string())?;
    let head = adapter.truth_head().map_err(|error| error.to_string())?;
    let checkpoint = adapter
        .locator_checkpoint()
        .map_err(|error| error.to_string())?;
    if head.cursor != checkpoint {
        return Err("restart child opened stale derived state".to_owned());
    }
    drop(guard);
    let counters = qualification_counters(&scope.snapshot().counters, 1);
    let after = capture_longitudinal_process_snapshot_v1(std::process::id())
        .map_err(|error| error.to_string())?;
    Ok(QualificationDerivedAccessRestartChildReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_RESTART_CHILD_RECEIPT_SCHEMA_V1.to_owned(),
        process_cpu_nanos: after
            .user_cpu_nanos
            .saturating_add(after.system_cpu_nanos)
            .saturating_sub(
                before
                    .user_cpu_nanos
                    .saturating_add(before.system_cpu_nanos),
            ),
        resident_bytes: after.resident_bytes,
        counters,
        semantic_receipt_sha256: sha256_bytes_hex(format!("{head:?}/{checkpoint:?}").as_bytes()),
    })
}

#[cfg(not(feature = "longitudinal-counting"))]
pub fn run_qualification_derived_access_restart_child_v1(
    _request_path: &Path,
) -> Result<QualificationDerivedAccessRestartChildReceiptV1, String> {
    Err("restart evidence requires --features longitudinal-counting".to_owned())
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessResourceReceiptV1 {
    pub schema: String,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub baseline: QualificationDerivedAccessResourceChildReceiptV1,
    pub l7: QualificationDerivedAccessResourceChildReceiptV1,
    pub l100: QualificationDerivedAccessResourceChildReceiptV1,
    pub resources: super::QualificationDerivedAccessResourceEvidenceV1,
    pub root_tiers: Vec<QualificationDerivedAccessTierV1>,
    pub roots_before: Vec<LongitudinalStoreDataInventoryV1>,
    pub roots_after: Vec<LongitudinalStoreDataInventoryV1>,
}

#[cfg(feature = "longitudinal-counting")]
pub fn run_qualification_derived_access_resource_child_v1(
    request_path: &Path,
) -> Result<QualificationDerivedAccessResourceChildReceiptV1, String> {
    let request: QualificationDerivedAccessResourceChildRequestV1 = read_json(request_path)?;
    if request.schema != QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_REQUEST_SCHEMA_V1
        || request.root.is_some() != request.store_id.is_some()
    {
        return Err("invalid derived-access resource child request".to_owned());
    }
    let initial = capture_longitudinal_process_snapshot_v1(std::process::id())
        .map_err(|error| error.to_string())?;
    let Some(root) = request.root else {
        return Ok(QualificationDerivedAccessResourceChildReceiptV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_RECEIPT_SCHEMA_V1.to_owned(),
            baseline_only: true,
            steady_rss_bytes: initial.resident_bytes,
            peak_rss_bytes: initial.resident_bytes,
            retained_body_object_bytes: 0,
            semantic_receipt_sha256: sha256_bytes_hex(b"empty-derived-access-resource-child"),
        });
    };
    let store = store_dir_for_repo(&root).map_err(|error| error.to_string())?;
    let adapter = QualificationDerivedAccessAdapter::open(
        &store,
        CursorLedgerIdentity::new(request.store_id.expect("validated resource store id")),
    )
    .map_err(|error| error.to_string())?;
    adapter
        .catch_up_to_head(512)
        .map_err(|error| error.to_string())?;
    let steady = capture_longitudinal_process_snapshot_v1(std::process::id())
        .map_err(|error| error.to_string())?;
    let head = ready_scale_window(
        adapter
            .chronological_window(ChronologicalWindowRequest::head(100))
            .map_err(|error| error.to_string())?,
    )?;
    let after_head = capture_longitudinal_process_snapshot_v1(std::process::id())
        .map_err(|error| error.to_string())?;
    let tail = ready_scale_window(
        adapter
            .chronological_window(ChronologicalWindowRequest::tail(100))
            .map_err(|error| error.to_string())?,
    )?;
    let after_tail = capture_longitudinal_process_snapshot_v1(std::process::id())
        .map_err(|error| error.to_string())?;
    let retained_body_object_bytes = adapter
        .semantic_inventory()
        .map_err(|error| error.to_string())?
        .retained_body_object_bytes;
    Ok(QualificationDerivedAccessResourceChildReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_RECEIPT_SCHEMA_V1.to_owned(),
        baseline_only: false,
        steady_rss_bytes: steady.resident_bytes,
        peak_rss_bytes: [
            steady.resident_bytes,
            after_head.resident_bytes,
            after_tail.resident_bytes,
        ]
        .into_iter()
        .max()
        .unwrap_or(steady.resident_bytes),
        retained_body_object_bytes,
        semantic_receipt_sha256: sha256_bytes_hex(
            &serde_json::to_vec(&(
                head.events
                    .iter()
                    .map(|event| event.event_id.as_str())
                    .collect::<Vec<_>>(),
                tail.events
                    .iter()
                    .map(|event| event.event_id.as_str())
                    .collect::<Vec<_>>(),
            ))
            .map_err(|error| error.to_string())?,
        ),
    })
}

#[cfg(not(feature = "longitudinal-counting"))]
pub fn run_qualification_derived_access_resource_child_v1(
    _request_path: &Path,
) -> Result<QualificationDerivedAccessResourceChildReceiptV1, String> {
    Err("resource evidence requires --features longitudinal-counting".to_owned())
}

#[cfg(feature = "longitudinal-counting")]
pub fn run_qualification_derived_access_resource_v1(
    request_path: &Path,
) -> Result<QualificationDerivedAccessResourceReceiptV1, String> {
    let request: QualificationDerivedAccessResourceRunRequestV1 = read_json(request_path)?;
    request.validate()?;
    validate_current_execution_identity_v1(
        &request.execution,
        &request.source_checkout,
        &request.roots[0].root,
    )?;
    let mut roots_before = Vec::new();
    for root in &request.roots {
        let inventory = longitudinal_authoritative_store_data_inventory_v1(&root.root)
            .map_err(|error| error.to_string())?;
        if inventory.inventory_sha256 != root.admitted_root_sha256 {
            return Err(format!(
                "resource root {:?} does not match admitted truth",
                root.tier
            ));
        }
        roots_before.push(inventory);
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let control = std::env::temp_dir().join(format!(
        "pointbreak-derived-resource-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    std::fs::create_dir(&control).map_err(|error| error.to_string())?;
    let result = (|| {
        let baseline = spawn_resource_child(&executable, &control, "baseline", None, None)?;
        let l7_root = request
            .roots
            .iter()
            .find(|root| root.tier == QualificationDerivedAccessTierV1::L7)
            .expect("validated L7 root");
        let l100_root = request
            .roots
            .iter()
            .find(|root| root.tier == QualificationDerivedAccessTierV1::L100)
            .expect("validated L100 root");
        let l7 = spawn_resource_child(
            &executable,
            &control,
            "l7",
            Some(&l7_root.root),
            Some(&format!(
                "store:derived-access:{}",
                l7_root.admitted_root_sha256
            )),
        )?;
        let l100 = spawn_resource_child(
            &executable,
            &control,
            "l100",
            Some(&l100_root.root),
            Some(&format!(
                "store:derived-access:{}",
                l100_root.admitted_root_sha256
            )),
        )?;
        let l7_steady = l7
            .steady_rss_bytes
            .saturating_sub(baseline.steady_rss_bytes);
        let l100_steady = l100
            .steady_rss_bytes
            .saturating_sub(baseline.steady_rss_bytes);
        let l100_peak = l100.peak_rss_bytes.saturating_sub(baseline.peak_rss_bytes);
        let slope = l100_steady
            .saturating_sub(l7_steady)
            .checked_div(102_400 - 7_168)
            .unwrap_or(u64::MAX);
        let roots_after = request
            .roots
            .iter()
            .map(|root| {
                longitudinal_authoritative_store_data_inventory_v1(&root.root)
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        if roots_before != roots_after {
            return Err("resource evidence mutated authoritative roots".to_owned());
        }
        let retained_body_object_bytes = l7
            .retained_body_object_bytes
            .max(l100.retained_body_object_bytes);
        Ok(QualificationDerivedAccessResourceReceiptV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_RESOURCE_RECEIPT_SCHEMA_V1.to_owned(),
            execution: request.execution,
            baseline,
            l7,
            l100,
            resources: super::QualificationDerivedAccessResourceEvidenceV1 {
                l100_steady_rss_bytes: l100_steady,
                l100_peak_rss_bytes: l100_peak,
                l7_to_l100_steady_slope_bytes_per_event: slope,
                retained_body_object_bytes_outside_active_window: retained_body_object_bytes,
            },
            root_tiers: request.roots.iter().map(|root| root.tier).collect(),
            roots_before,
            roots_after,
        })
    })();
    let cleanup = std::fs::remove_dir_all(&control);
    match (result, cleanup) {
        (Ok(receipt), Ok(())) => Ok(receipt),
        (Ok(_), Err(error)) => Err(format!(
            "resource evidence succeeded but cleanup failed: {error}"
        )),
        (Err(error), _) => Err(error),
    }
}

#[cfg(not(feature = "longitudinal-counting"))]
pub fn run_qualification_derived_access_resource_v1(
    _request_path: &Path,
) -> Result<QualificationDerivedAccessResourceReceiptV1, String> {
    Err("resource evidence requires --features longitudinal-counting".to_owned())
}

#[cfg(feature = "longitudinal-counting")]
fn spawn_resource_child(
    executable: &Path,
    control_root: &Path,
    label: &str,
    root: Option<&Path>,
    store_id: Option<&str>,
) -> Result<QualificationDerivedAccessResourceChildReceiptV1, String> {
    let request_path = control_root.join(format!("{label}.json"));
    let request = QualificationDerivedAccessResourceChildRequestV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_REQUEST_SCHEMA_V1.to_owned(),
        root: root.map(Path::to_path_buf),
        store_id: store_id.map(str::to_owned),
    };
    std::fs::write(
        &request_path,
        serde_json::to_vec_pretty(&request).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let output = Command::new(executable)
        .arg(QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_MODE_V1)
        .arg(&request_path)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "resource child {label} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let receipt: QualificationDerivedAccessResourceChildReceiptV1 =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    if receipt.schema != QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_RECEIPT_SCHEMA_V1
        || receipt.baseline_only != root.is_none()
    {
        return Err(format!("resource child {label} receipt drifted"));
    }
    validate_digest(
        &receipt.semantic_receipt_sha256,
        "resource child semantic receipt",
    )?;
    Ok(receipt)
}

#[cfg(feature = "longitudinal-counting")]
pub fn run_qualification_derived_access_scale_v1(
    request_path: &Path,
) -> Result<QualificationDerivedAccessScaleReceiptV1, String> {
    let request: QualificationDerivedAccessScaleRunRequestV1 = read_json(request_path)?;
    request.validate()?;
    validate_current_execution_identity_v1(
        &request.execution,
        &request.source_checkout,
        &request.roots[0].root,
    )?;
    if request.execution.platform != super::QualificationDerivedAccessPlatformV1::MacosApfs {
        return Err("scale evidence is native APFS evidence".to_owned());
    }

    let mut raw_samples = Vec::new();
    let mut root_before = Vec::new();
    let mut root_after = Vec::new();
    let mut inventories = Vec::new();
    for (root_ordinal, root) in request.roots.iter().enumerate() {
        let before = longitudinal_authoritative_store_data_inventory_v1(&root.root)
            .map_err(|error| error.to_string())?;
        if before.inventory_sha256 != root.admitted_root_sha256 {
            return Err(format!(
                "scale root {root_ordinal} does not match admitted truth"
            ));
        }
        root_before.push(before);
        let mut state = ScaleRootStateV1::open(
            &root.root,
            &format!("store:derived-access:{}", root.admitted_root_sha256),
        )?;
        state.verify_full_replay()?;
        let initial_inventory = derived_inventory(&state)?;
        let mut high_water = initial_inventory.high_water_bytes;

        for operation in [
            QualificationDerivedAccessOperationV1::SemanticId,
            QualificationDerivedAccessOperationV1::FreshNoChange,
            QualificationDerivedAccessOperationV1::NewCountZero,
            QualificationDerivedAccessOperationV1::WindowHead,
            QualificationDerivedAccessOperationV1::WindowMiddle,
            QualificationDerivedAccessOperationV1::WindowTail,
            QualificationDerivedAccessOperationV1::RevisionDetailActive,
            QualificationDerivedAccessOperationV1::RevisionDetailRemoved,
        ] {
            for sample_index in 0..34_u16 {
                let retained = sample_index >= 4;
                let sample = state.measure(
                    request.tier,
                    operation,
                    sample_index,
                    retained,
                    root_ordinal as u64,
                )?;
                if retained {
                    raw_samples.push(sample);
                }
            }
        }

        for sample_index in 0..30_u16 {
            let append = state.measure(
                request.tier,
                QualificationDerivedAccessOperationV1::AppendOne,
                sample_index,
                true,
                root_ordinal as u64,
            )?;
            let after_append = derived_inventory(&state)?;
            high_water = high_water.max(after_append.high_water_bytes);
            raw_samples.push(append);
            raw_samples.push(state.measure(
                request.tier,
                QualificationDerivedAccessOperationV1::PostOne,
                sample_index,
                true,
                root_ordinal as u64,
            )?);
        }

        for sample_index in 0..10_u16 {
            raw_samples.push(state.measure(
                request.tier,
                QualificationDerivedAccessOperationV1::Restart,
                sample_index,
                true,
                root_ordinal as u64,
            )?);
        }
        state.verify_full_replay()?;
        let mut inventory = derived_inventory(&state)?;
        inventory.high_water_bytes = inventory.high_water_bytes.max(high_water);
        inventory.validate_bodyless()?;
        inventories.push(inventory);
        drop(state);
        root_after.push(
            longitudinal_authoritative_store_data_inventory_v1(&root.root)
                .map_err(|error| error.to_string())?,
        );
    }

    let operation_rows = aggregate_scale_rows(
        request.tier,
        request.execution.platform,
        &request.l100_selected_work,
        &raw_samples,
    )?;
    let allocation = derive_scale_allocation(request.tier, &inventories, &raw_samples)?;
    let receipt = QualificationDerivedAccessScaleReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_SCALE_RECEIPT_SCHEMA_V1.to_owned(),
        execution: request.execution,
        tier: request.tier,
        l100_selected_work: request.l100_selected_work,
        raw_samples,
        operation_rows,
        allocation,
        derived_inventories: inventories,
        root_before,
        root_after,
    };
    validate_scale_receipt(&receipt)?;
    Ok(receipt)
}

#[cfg(not(feature = "longitudinal-counting"))]
pub fn run_qualification_derived_access_scale_v1(
    _request_path: &Path,
) -> Result<QualificationDerivedAccessScaleReceiptV1, String> {
    Err("scale evidence requires --features longitudinal-counting".to_owned())
}

#[cfg(feature = "longitudinal-counting")]
struct ScaleRootStateV1 {
    repo_root: PathBuf,
    store_root: PathBuf,
    store_id: String,
    adapter: QualificationDerivedAccessAdapter,
    first_event_id: String,
    active_revision: RevisionId,
    removed_revision: RevisionId,
    middle_continuation: WindowContinuation,
    retained_cardinality: u64,
    append_ordinal: u64,
}

#[cfg(feature = "longitudinal-counting")]
impl ScaleRootStateV1 {
    fn open(repo_root: &Path, store_id: &str) -> Result<Self, String> {
        let store_root = store_dir_for_repo(repo_root).map_err(|error| error.to_string())?;
        let adapter = QualificationDerivedAccessAdapter::open(
            &store_root,
            CursorLedgerIdentity::new(store_id),
        )
        .map_err(|error| error.to_string())?;
        adapter
            .catch_up_to_head(512)
            .map_err(|error| error.to_string())?;
        let events = EventStore::open(&store_root)
            .list_events()
            .map_err(|error| error.to_string())?;
        let first_event_id = events
            .first()
            .map(|event| event.event_id.as_str().to_owned())
            .ok_or_else(|| "scale root contains no events".to_owned())?;
        let (removed_revision, active_revision) = scale_revision_selectors(&events)?;
        let window = ready_scale_window(
            adapter
                .chronological_window(ChronologicalWindowRequest::head(100))
                .map_err(|error| error.to_string())?,
        )?;
        let middle_continuation = window
            .continuation
            .ok_or_else(|| "scale root cannot produce a middle continuation".to_owned())?;
        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            store_root,
            store_id: store_id.to_owned(),
            adapter,
            first_event_id,
            active_revision,
            removed_revision,
            middle_continuation,
            retained_cardinality: events.len() as u64,
            append_ordinal: 0,
        })
    }

    fn measure(
        &mut self,
        tier: QualificationDerivedAccessTierV1,
        operation: QualificationDerivedAccessOperationV1,
        sample_index: u16,
        retained: bool,
        root_ordinal: u64,
    ) -> Result<QualificationDerivedAccessRawSampleV1, String> {
        if operation == QualificationDerivedAccessOperationV1::Restart {
            return self.measure_restart(tier, sample_index, retained, root_ordinal);
        }
        let scope = LongitudinalCountingScopeV1::new(sha256_bytes_hex(
            format!("{tier:?}/{operation:?}/{root_ordinal}/{sample_index}").as_bytes(),
        ))?;
        let before_inventory = derived_inventory(self)?;
        let before = capture_longitudinal_process_snapshot_v1(std::process::id())
            .map_err(|error| error.to_string())?;
        let started = Instant::now();
        let guard = scope.enter();
        let (response, selected_output_count, authoritative_bytes_published) =
            self.execute(operation)?;
        drop(guard);
        let wall_nanos = started.elapsed().as_nanos().try_into().unwrap_or(u64::MAX);
        let after = capture_longitudinal_process_snapshot_v1(std::process::id())
            .map_err(|error| error.to_string())?;
        let after_inventory = derived_inventory(self)?;
        let counters = qualification_counters(&scope.snapshot().counters, response.len() as u64);
        let selected_work_count = selected_work(&counters, selected_output_count);
        let whole_history_work = selected_work_count >= self.retained_cardinality
            && self.retained_cardinality > selected_output_count;
        let complexity = classify_complexity(
            selected_output_count,
            selected_work_count,
            self.retained_cardinality,
            whole_history_work,
        );
        let process_scope = if operation == QualificationDerivedAccessOperationV1::AppendOne {
            QualificationDerivedAccessProcessScopeV1::Driver
        } else {
            QualificationDerivedAccessProcessScopeV1::InspectorServiceChild
        };
        let semantic_receipt_sha256 = sha256_bytes_hex(response.as_bytes());
        let sample = QualificationDerivedAccessRawSampleV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_RAW_SAMPLE_SCHEMA_V1.to_owned(),
            tier,
            operation,
            sample_index: global_sample_index(root_ordinal, sample_index)?,
            retained,
            status: QualificationDerivedAccessStatusV1::Passed,
            semantic_receipt_sha256: semantic_receipt_sha256.clone(),
            semantic_receipt_matches: true,
            wall_nanos,
            process_cpu_nanos: Some(
                after
                    .user_cpu_nanos
                    .saturating_add(after.system_cpu_nanos)
                    .saturating_sub(
                        before
                            .user_cpu_nanos
                            .saturating_add(before.system_cpu_nanos),
                    ),
            ),
            process_cpu_unit: QualificationDerivedAccessCpuUnitV1::Nanoseconds,
            process_scope: Some(process_scope),
            selected_output_count,
            selected_work_count,
            retained_cardinality: self.retained_cardinality,
            authoritative_bytes_published,
            whole_history_work,
            complexity,
            counters,
            sqlite: QualificationDerivedAccessSqliteCountersV1 {
                selected_rows: selected_output_count,
                page_count: after_inventory.page_count,
                database_size_delta_bytes: after_inventory
                    .database_bytes
                    .saturating_sub(before_inventory.database_bytes),
                wal_size_delta_bytes: after_inventory
                    .wal_bytes
                    .saturating_sub(before_inventory.wal_bytes),
                shared_memory_size_delta_bytes: after_inventory
                    .shared_memory_bytes
                    .saturating_sub(before_inventory.shared_memory_bytes),
                temporary_size_delta_bytes: after_inventory
                    .temporary_bytes
                    .saturating_sub(before_inventory.temporary_bytes),
                checkpoint_rows_written: 0,
                delta_rows_applied: if operation == QualificationDerivedAccessOperationV1::AppendOne
                {
                    1
                } else {
                    0
                },
            },
            response_sha256: semantic_receipt_sha256,
        };
        sample.validate()?;
        Ok(sample)
    }

    fn measure_restart(
        &self,
        tier: QualificationDerivedAccessTierV1,
        sample_index: u16,
        retained: bool,
        root_ordinal: u64,
    ) -> Result<QualificationDerivedAccessRawSampleV1, String> {
        let control = std::env::temp_dir().join(format!(
            "pointbreak-derived-restart-{}-{root_ordinal}-{sample_index}.json",
            std::process::id()
        ));
        let request = QualificationDerivedAccessRestartChildRequestV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_RESTART_CHILD_REQUEST_SCHEMA_V1.to_owned(),
            root: self.repo_root.clone(),
            store_id: self.store_id.clone(),
        };
        std::fs::write(
            &control,
            serde_json::to_vec_pretty(&request).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let started = Instant::now();
        let output = Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
            .arg(QUALIFICATION_DERIVED_ACCESS_RESTART_CHILD_MODE_V1)
            .arg(&control)
            .output()
            .map_err(|error| error.to_string());
        let wall_nanos = started.elapsed().as_nanos().try_into().unwrap_or(u64::MAX);
        let cleanup = std::fs::remove_file(&control);
        let output = output?;
        cleanup.map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "restart child failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let receipt: QualificationDerivedAccessRestartChildReceiptV1 =
            serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
        if receipt.schema != QUALIFICATION_DERIVED_ACCESS_RESTART_CHILD_RECEIPT_SCHEMA_V1 {
            return Err("restart child receipt drifted".to_owned());
        }
        validate_digest(&receipt.semantic_receipt_sha256, "restart semantic receipt")?;
        let selected_work_count = selected_work(&receipt.counters, 1);
        let whole_history_work =
            selected_work_count >= self.retained_cardinality && self.retained_cardinality > 1;
        let sample = QualificationDerivedAccessRawSampleV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_RAW_SAMPLE_SCHEMA_V1.to_owned(),
            tier,
            operation: QualificationDerivedAccessOperationV1::Restart,
            sample_index: global_sample_index(root_ordinal, sample_index)?,
            retained,
            status: QualificationDerivedAccessStatusV1::Passed,
            semantic_receipt_sha256: receipt.semantic_receipt_sha256.clone(),
            semantic_receipt_matches: true,
            wall_nanos,
            process_cpu_nanos: Some(receipt.process_cpu_nanos),
            process_cpu_unit: QualificationDerivedAccessCpuUnitV1::Nanoseconds,
            process_scope: Some(QualificationDerivedAccessProcessScopeV1::InspectorServiceChild),
            selected_output_count: 1,
            selected_work_count,
            retained_cardinality: self.retained_cardinality,
            authoritative_bytes_published: 0,
            whole_history_work,
            complexity: classify_complexity(
                1,
                selected_work_count,
                self.retained_cardinality,
                whole_history_work,
            ),
            counters: receipt.counters.clone(),
            sqlite: QualificationDerivedAccessSqliteCountersV1 {
                selected_rows: 1,
                page_count: 0,
                database_size_delta_bytes: 0,
                wal_size_delta_bytes: 0,
                shared_memory_size_delta_bytes: 0,
                temporary_size_delta_bytes: 0,
                checkpoint_rows_written: 0,
                delta_rows_applied: 0,
            },
            response_sha256: receipt.semantic_receipt_sha256,
        };
        sample.validate()?;
        Ok(sample)
    }

    fn execute(
        &mut self,
        operation: QualificationDerivedAccessOperationV1,
    ) -> Result<(String, u64, u64), String> {
        use QualificationDerivedAccessOperationV1 as Operation;
        match operation {
            Operation::SemanticId => match self
                .adapter
                .semantic_id(&self.first_event_id)
                .map_err(|error| error.to_string())?
            {
                LocatorRead::Ready(Some(event)) => Ok((event.event_id.as_str().to_owned(), 1, 0)),
                _ => Err("scale semantic-id lookup did not return its event".to_owned()),
            },
            Operation::FreshNoChange => Ok((
                format!(
                    "{:?}",
                    self.adapter
                        .freshness()
                        .map_err(|error| error.to_string())?
                ),
                1,
                0,
            )),
            Operation::NewCountZero => Ok((
                format!(
                    "{:?}",
                    self.adapter
                        .new_event_count()
                        .map_err(|error| error.to_string())?
                ),
                1,
                0,
            )),
            Operation::WindowHead => self.window(ChronologicalWindowRequest::head(100)),
            Operation::WindowMiddle => self.window(ChronologicalWindowRequest::continue_from(
                self.middle_continuation.clone(),
                100,
            )),
            Operation::WindowTail | Operation::PostOne => {
                self.window(ChronologicalWindowRequest::tail(100))
            }
            Operation::RevisionDetailActive => {
                self.revision_detail(self.active_revision.clone(), false)
            }
            Operation::RevisionDetailRemoved => {
                self.revision_detail(self.removed_revision.clone(), true)
            }
            Operation::AppendOne => {
                let event = scale_append_event(self.append_ordinal)?;
                self.append_ordinal = self.append_ordinal.saturating_add(1);
                let serialized = serde_json::to_vec(&event).map_err(|error| error.to_string())?;
                let outcome = self
                    .adapter
                    .append_event(
                        &event,
                        &format!("derived-access-scale-{}", self.append_ordinal),
                    )
                    .map_err(|error| error.to_string())?;
                if matches!(&outcome, AppendResolution::Created(_)) {
                    self.retained_cardinality = self.retained_cardinality.saturating_add(1);
                }
                Ok((
                    format!("{outcome:?};eventBytes={}", serialized.len()),
                    1,
                    serialized.len() as u64,
                ))
            }
            Operation::Restart => unreachable!("restart runs in a fresh child process"),
        }
    }

    fn window(&self, request: ChronologicalWindowRequest) -> Result<(String, u64, u64), String> {
        let window = ready_scale_window(
            self.adapter
                .chronological_window(request)
                .map_err(|error| error.to_string())?,
        )?;
        let count = window.events.len() as u64;
        Ok((
            sha256_bytes_hex(
                &serde_json::to_vec(
                    &window
                        .events
                        .iter()
                        .map(|event| event.event_id.as_str())
                        .collect::<Vec<_>>(),
                )
                .map_err(|error| error.to_string())?,
            ),
            count,
            0,
        ))
    }

    fn revision_detail(
        &self,
        revision_id: RevisionId,
        expected_removed: bool,
    ) -> Result<(String, u64, u64), String> {
        match self
            .adapter
            .revision_detail(&revision_id)
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(Some(detail))
                if detail.object_content_removed == expected_removed =>
            {
                let count = detail.authoritative_events.len() as u64;
                Ok((
                    sha256_bytes_hex(
                        &serde_json::to_vec(&detail.authoritative_events)
                            .map_err(|error| error.to_string())?,
                    ),
                    count.max(1),
                    0,
                ))
            }
            _ => Err(format!(
                "scale revision detail did not match removal state {expected_removed}"
            )),
        }
    }

    fn verify_full_replay(&self) -> Result<(), String> {
        let events = EventStore::open(&self.store_root)
            .list_events()
            .map_err(|error| error.to_string())?;
        let strict =
            strict_bodyless_materialized_snapshot(&events).map_err(|error| error.to_string())?;
        let incremental = match self
            .adapter
            .semantic_materialized_audit_snapshot()
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(snapshot) => snapshot,
            LocatorRead::CatchUpRequired { .. } => {
                return Err("scale adapter remained behind truth".to_owned());
            }
        };
        if strict != incremental {
            return Err("scale adapter differs from strict full replay".to_owned());
        }
        Ok(())
    }
}

#[cfg(feature = "longitudinal-counting")]
fn scale_revision_selectors(events: &[ShoreEvent]) -> Result<(RevisionId, RevisionId), String> {
    let removed_hashes = events
        .iter()
        .filter(|event| event.event_type == EventType::ArtifactRemoved)
        .filter_map(|event| {
            event
                .payload
                .get("contentHash")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect::<BTreeSet<_>>();
    let revisions = events
        .iter()
        .filter(|event| event.event_type == EventType::WorkObjectProposed)
        .filter_map(|event| {
            serde_json::from_value::<WorkObjectProposedPayload>(event.payload.clone())
                .ok()
                .and_then(|payload| match payload.work_object {
                    WorkObjectProposal::Revision {
                        revision,
                        object_artifact_content_hash,
                        ..
                    } => Some((revision.id, object_artifact_content_hash)),
                    WorkObjectProposal::TaskAttempt { .. } => None,
                })
        })
        .collect::<Vec<_>>();
    let removed = revisions
        .iter()
        .find(|(_, hash)| removed_hashes.contains(hash))
        .map(|(revision, _)| revision.clone())
        .ok_or_else(|| "scale root has no removed revision detail selector".to_owned())?;
    let active = revisions
        .iter()
        .find(|(_, hash)| !removed_hashes.contains(hash))
        .map(|(revision, _)| revision.clone())
        .ok_or_else(|| "scale root has no active revision detail selector".to_owned())?;
    Ok((removed, active))
}

#[cfg(feature = "longitudinal-counting")]
fn scale_append_event(index: u64) -> Result<ShoreEvent, String> {
    let journal_id = JournalId::new(format!("journal:derived-access-scale:{index}"));
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

#[cfg(feature = "longitudinal-counting")]
fn ready_scale_window(
    read: LocatorRead<crate::session::derived_access::locator::HydratedWindow>,
) -> Result<crate::session::derived_access::locator::HydratedWindow, String> {
    match read {
        LocatorRead::Ready(window) => Ok(window),
        LocatorRead::CatchUpRequired { .. } => {
            Err("scale query remained behind authoritative truth".to_owned())
        }
    }
}

#[cfg(feature = "longitudinal-counting")]
fn derived_inventory(
    state: &ScaleRootStateV1,
) -> Result<QualificationDerivedAccessDerivedInventoryV1, String> {
    let cursor = state
        .adapter
        .cursor_inventory()
        .map_err(|error| error.to_string())?;
    let locator = state
        .adapter
        .locator_inventory()
        .map_err(|error| error.to_string())?;
    let semantic = state
        .adapter
        .semantic_inventory()
        .map_err(|error| error.to_string())?;
    if semantic
        .columns
        .iter()
        .any(|column| column.contains("body") || column.contains("object_bytes"))
    {
        return Err("derived schema contains body/object persistence".to_owned());
    }
    let sidecar = super::DerivedStorageLayout::resolve(&state.store_root)
        .map_err(|error| error.to_string())?
        .root();
    let database = sidecar.join("cursor.sqlite3");
    let page_count = rusqlite::Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .and_then(|connection| {
        connection.query_row("PRAGMA page_count", [], |row| row.get::<_, i64>(0))
    })
    .map_err(|error| error.to_string())
    .and_then(|count| u64::try_from(count).map_err(|_| "negative SQLite page count".to_owned()))?;
    let core_bytes = cursor
        .database_bytes
        .saturating_add(cursor.wal_bytes)
        .saturating_add(cursor.shared_memory_bytes);
    let steady = governed_derived_state_bytes(&state.store_root)?;
    if steady < core_bytes {
        return Err("derived filesystem inventory is smaller than SQLite inventory".to_owned());
    }
    let temporary_bytes = steady - core_bytes;
    Ok(QualificationDerivedAccessDerivedInventoryV1 {
        database_bytes: cursor.database_bytes,
        wal_bytes: cursor.wal_bytes,
        shared_memory_bytes: cursor.shared_memory_bytes,
        temporary_bytes,
        row_count: cursor
            .receipt_count
            .saturating_add(locator.row_count)
            .saturating_add(semantic.fact_count),
        page_count,
        body_bytes: semantic.retained_body_object_bytes,
        object_bytes: 0,
        high_water_bytes: steady,
    })
}

#[cfg(feature = "longitudinal-counting")]
fn classify_complexity(
    selected_output_count: u64,
    selected_work_count: u64,
    retained_cardinality: u64,
    whole_history_work: bool,
) -> QualificationDerivedAccessComplexityV1 {
    classify_complexity_portable(
        selected_output_count,
        selected_work_count,
        retained_cardinality,
        whole_history_work,
    )
}

fn classify_complexity_portable(
    selected_output_count: u64,
    selected_work_count: u64,
    retained_cardinality: u64,
    whole_history_work: bool,
) -> QualificationDerivedAccessComplexityV1 {
    if whole_history_work
        || selected_output_count == 0
        || (retained_cardinality > selected_output_count
            && selected_work_count >= retained_cardinality)
    {
        QualificationDerivedAccessComplexityV1::HistoryOrCardinalityProportional
    } else {
        QualificationDerivedAccessComplexityV1::BoundedSelectedWork
    }
}

#[cfg(feature = "longitudinal-counting")]
fn selected_work(counters: &QualificationDerivedAccessCountersV1, selected_output: u64) -> u64 {
    selected_work_portable(counters, selected_output)
}

#[cfg(feature = "longitudinal-counting")]
fn qualification_counters(
    observed: &LongitudinalCountersV1,
    response_bytes: u64,
) -> QualificationDerivedAccessCountersV1 {
    QualificationDerivedAccessCountersV1 {
        directory_entries_walked: observed.directory_entries_walked,
        carrier_opens: observed.carrier_opens,
        carrier_bytes_read: observed.carrier_bytes_read,
        event_decodes: observed.event_decodes,
        event_validations: observed.event_validations,
        event_folds: observed.event_folds,
        chronological_sort_items: observed.chronological_sort_items,
        body_artifact_reads: observed.body_artifact_reads,
        body_bytes_read: observed.body_bytes_read,
        object_artifact_reads: observed.object_artifact_reads,
        object_bytes_read: observed.object_bytes_read,
        unselected_body_artifact_reads: 0,
        unselected_object_artifact_reads: 0,
        projection_rebuilds: observed.projection_rebuilds,
        state_rebuilds: observed.state_rebuilds,
        response_bytes,
    }
}

fn selected_work_portable(
    counters: &QualificationDerivedAccessCountersV1,
    selected_output: u64,
) -> u64 {
    [
        counters.carrier_opens,
        counters.event_decodes,
        counters.event_validations,
        counters.event_folds,
        counters.chronological_sort_items,
        counters.projection_rebuilds,
        counters.state_rebuilds,
        selected_output,
    ]
    .into_iter()
    .max()
    .unwrap_or_default()
}

#[cfg(feature = "longitudinal-counting")]
fn global_sample_index(root_ordinal: u64, sample_index: u16) -> Result<u16, String> {
    root_ordinal
        .saturating_mul(100)
        .saturating_add(u64::from(sample_index))
        .try_into()
        .map_err(|_| "scale sample index overflow".to_owned())
}

pub(super) fn validate_scale_receipt(
    receipt: &QualificationDerivedAccessScaleReceiptV1,
) -> Result<(), String> {
    if receipt.schema != QUALIFICATION_DERIVED_ACCESS_SCALE_RECEIPT_SCHEMA_V1
        || receipt.raw_samples.is_empty()
        || receipt.operation_rows.len() != QualificationDerivedAccessOperationV1::ALL.len()
        || receipt.derived_inventories.len() != 2
        || receipt.root_before.len() != 2
        || receipt.root_after.len() != 2
        || receipt.allocation.tier != receipt.tier
    {
        return Err("derived-access scale receipt is incomplete".to_owned());
    }
    receipt.execution.validate()?;
    match receipt.tier {
        QualificationDerivedAccessTierV1::L100 if !receipt.l100_selected_work.is_empty() => {
            return Err("L100 scale receipt contains comparison scalars".to_owned());
        }
        QualificationDerivedAccessTierV1::C262
            if receipt.l100_selected_work.len()
                != QualificationDerivedAccessOperationV1::ALL.len()
                || QualificationDerivedAccessOperationV1::ALL
                    .iter()
                    .any(|operation| !receipt.l100_selected_work.contains_key(operation)) =>
        {
            return Err("C262 scale receipt omitted comparison scalars".to_owned());
        }
        _ => {}
    }
    for sample in &receipt.raw_samples {
        sample.validate()?;
        if sample.tier != receipt.tier || !sample.retained {
            return Err("derived-access scale receipt mixes samples".to_owned());
        }
    }
    let expected_rows = aggregate_scale_rows(
        receipt.tier,
        receipt.execution.platform,
        &receipt.l100_selected_work,
        &receipt.raw_samples,
    )?;
    if receipt.operation_rows != expected_rows {
        return Err("derived-access scale aggregates do not match raw samples".to_owned());
    }
    for inventory in &receipt.derived_inventories {
        inventory.validate_bodyless()?;
    }
    let expected_allocation = derive_scale_allocation(
        receipt.tier,
        &receipt.derived_inventories,
        &receipt.raw_samples,
    )?;
    if receipt.allocation != expected_allocation {
        return Err("derived-access allocation summary does not match raw evidence".to_owned());
    }
    Ok(())
}

pub(super) fn derive_scale_allocation(
    tier: QualificationDerivedAccessTierV1,
    inventories: &[QualificationDerivedAccessDerivedInventoryV1],
    samples: &[QualificationDerivedAccessRawSampleV1],
) -> Result<super::QualificationDerivedAccessAllocationEvidenceV1, String> {
    let contract = super::qualification_derived_access_contract_v1();
    let event_count = match tier {
        QualificationDerivedAccessTierV1::L100 => contract.scale_profiles.l100_event_count,
        QualificationDerivedAccessTierV1::C262 => contract.scale_profiles.c262_event_count,
        _ => return Err("allocation evidence requires a retained scale tier".to_owned()),
    };
    let steady_derived_bytes = inventories
        .iter()
        .map(|inventory| {
            inventory
                .database_bytes
                .saturating_add(inventory.wal_bytes)
                .saturating_add(inventory.shared_memory_bytes)
                .saturating_add(inventory.temporary_bytes)
        })
        .max()
        .unwrap_or_default();
    let high_water_derived_bytes = inventories
        .iter()
        .map(|inventory| inventory.high_water_bytes)
        .max()
        .unwrap_or(steady_derived_bytes);
    let append_samples = samples
        .iter()
        .filter(|sample| sample.operation == QualificationDerivedAccessOperationV1::AppendOne);
    let (authoritative_bytes, derived_bytes) =
        append_samples.fold((0_u64, 0_u64), |(authoritative, derived), sample| {
            (
                authoritative.saturating_add(sample.authoritative_bytes_published.max(1)),
                derived
                    .saturating_add(sample.sqlite.database_size_delta_bytes)
                    .saturating_add(sample.sqlite.wal_size_delta_bytes)
                    .saturating_add(sample.sqlite.shared_memory_size_delta_bytes)
                    .saturating_add(sample.sqlite.temporary_size_delta_bytes),
            )
        });
    Ok(super::QualificationDerivedAccessAllocationEvidenceV1 {
        tier,
        event_count,
        steady_derived_bytes,
        high_water_derived_bytes,
        append_write_amplification_ratio_milli: derived_bytes
            .saturating_mul(1_000)
            .checked_div(authoritative_bytes.max(1))
            .unwrap_or(u64::MAX)
            .min(u64::from(u16::MAX)) as u16,
    })
}

pub(super) fn aggregate_scale_rows(
    tier: QualificationDerivedAccessTierV1,
    platform: super::QualificationDerivedAccessPlatformV1,
    l100_selected_work: &BTreeMap<QualificationDerivedAccessOperationV1, u64>,
    samples: &[QualificationDerivedAccessRawSampleV1],
) -> Result<Vec<super::QualificationDerivedAccessOperationEvidenceV1>, String> {
    let mut rows = Vec::new();
    for operation in QualificationDerivedAccessOperationV1::ALL {
        let operation_samples = samples
            .iter()
            .filter(|sample| sample.operation == operation)
            .collect::<Vec<_>>();
        let expected = if operation == QualificationDerivedAccessOperationV1::Restart {
            20
        } else {
            60
        };
        if operation_samples.len() != expected {
            return Err(format!(
                "derived-access {operation:?} retained {} samples instead of {expected}",
                operation_samples.len()
            ));
        }
        let observed_indices = operation_samples
            .iter()
            .map(|sample| sample.sample_index)
            .collect::<BTreeSet<_>>();
        if observed_indices != expected_scale_sample_indices(operation) {
            return Err(format!(
                "derived-access {operation:?} sample identities are incomplete or duplicated"
            ));
        }
        let counters = operation_samples
            .iter()
            .map(|sample| &sample.counters)
            .fold(zero_counters(), max_counters);
        let selected_work = operation_samples
            .iter()
            .map(|sample| sample.selected_work_count)
            .max()
            .unwrap_or_default();
        let ratio = if tier == QualificationDerivedAccessTierV1::C262 {
            let baseline = l100_selected_work
                .get(&operation)
                .copied()
                .ok_or_else(|| format!("C262 request omitted L100 {operation:?} work"))?;
            Some(selected_work_ratio_milli(selected_work, baseline))
        } else {
            None
        };
        rows.push(super::QualificationDerivedAccessOperationEvidenceV1 {
            tier,
            platform,
            operation,
            status: if operation_samples
                .iter()
                .all(|sample| sample.status == QualificationDerivedAccessStatusV1::Passed)
            {
                QualificationDerivedAccessStatusV1::Passed
            } else {
                QualificationDerivedAccessStatusV1::Failed
            },
            process_scope: operation_samples[0]
                .process_scope
                .ok_or_else(|| "scale process scope is absent".to_owned())?,
            semantic_receipt_matches: operation_samples
                .iter()
                .all(|sample| sample.semantic_receipt_matches),
            complexity: operation_samples
                .iter()
                .map(|sample| sample.complexity)
                .max()
                .unwrap_or(QualificationDerivedAccessComplexityV1::Unknown),
            retained_samples: expected as u16,
            wall_p95_ms: Some(nanos_to_ceil_millis(nearest_rank_p95(
                operation_samples
                    .iter()
                    .map(|sample| sample.wall_nanos)
                    .collect(),
            )?)),
            process_cpu_p95_ms: Some(nanos_to_ceil_millis(nearest_rank_p95(
                operation_samples
                    .iter()
                    .map(|sample| sample.process_cpu_nanos.unwrap_or_default())
                    .collect(),
            )?)),
            selected_output_count: Some(
                operation_samples
                    .iter()
                    .map(|sample| sample.selected_output_count)
                    .max()
                    .unwrap_or_default(),
            ),
            unselected_work_count: Some(
                operation_samples
                    .iter()
                    .map(|sample| {
                        sample
                            .selected_work_count
                            .saturating_sub(sample.selected_output_count)
                    })
                    .max()
                    .unwrap_or_default(),
            ),
            selected_work_count: selected_work,
            retained_cardinality: operation_samples
                .iter()
                .map(|sample| sample.retained_cardinality)
                .max()
                .unwrap_or_default(),
            l100_to_c262_selected_work_ratio_milli: ratio,
            counters,
        });
    }
    Ok(rows)
}

fn expected_scale_sample_indices(
    operation: QualificationDerivedAccessOperationV1,
) -> BTreeSet<u16> {
    let local = if operation == QualificationDerivedAccessOperationV1::Restart {
        0..10
    } else if matches!(
        operation,
        QualificationDerivedAccessOperationV1::AppendOne
            | QualificationDerivedAccessOperationV1::PostOne
    ) {
        0..30
    } else {
        4..34
    };
    [0_u16, 100_u16]
        .into_iter()
        .flat_map(|root| local.clone().map(move |index| root + index))
        .collect()
}

fn nearest_rank_p95(mut values: Vec<u64>) -> Result<u64, String> {
    if values.is_empty() {
        return Err("nearest-rank p95 requires samples".to_owned());
    }
    values.sort_unstable();
    let rank = values.len().saturating_mul(95).div_ceil(100).max(1);
    Ok(values[rank - 1])
}

fn nanos_to_ceil_millis(nanos: u64) -> u64 {
    nanos.div_ceil(1_000_000)
}

fn selected_work_ratio_milli(selected_work: u64, baseline_work: u64) -> u16 {
    selected_work
        .saturating_mul(1_000)
        .checked_div(baseline_work.max(1))
        .unwrap_or(u64::MAX)
        .min(u64::from(u16::MAX)) as u16
}

fn zero_counters() -> QualificationDerivedAccessCountersV1 {
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
        response_bytes: 0,
    }
}

fn max_counters(
    mut left: QualificationDerivedAccessCountersV1,
    right: &QualificationDerivedAccessCountersV1,
) -> QualificationDerivedAccessCountersV1 {
    left.directory_entries_walked = left
        .directory_entries_walked
        .max(right.directory_entries_walked);
    left.carrier_opens = left.carrier_opens.max(right.carrier_opens);
    left.carrier_bytes_read = left.carrier_bytes_read.max(right.carrier_bytes_read);
    left.event_decodes = left.event_decodes.max(right.event_decodes);
    left.event_validations = left.event_validations.max(right.event_validations);
    left.event_folds = left.event_folds.max(right.event_folds);
    left.chronological_sort_items = left
        .chronological_sort_items
        .max(right.chronological_sort_items);
    left.body_artifact_reads = left.body_artifact_reads.max(right.body_artifact_reads);
    left.body_bytes_read = left.body_bytes_read.max(right.body_bytes_read);
    left.object_artifact_reads = left.object_artifact_reads.max(right.object_artifact_reads);
    left.object_bytes_read = left.object_bytes_read.max(right.object_bytes_read);
    left.unselected_body_artifact_reads = left
        .unselected_body_artifact_reads
        .max(right.unselected_body_artifact_reads);
    left.unselected_object_artifact_reads = left
        .unselected_object_artifact_reads
        .max(right.unselected_object_artifact_reads);
    left.projection_rebuilds = left.projection_rebuilds.max(right.projection_rebuilds);
    left.state_rebuilds = left.state_rebuilds.max(right.state_rebuilds);
    left.response_bytes = left.response_bytes.max(right.response_bytes);
    left
}

pub fn preflight_qualification_derived_access_retained_root_v1(
    request_path: &Path,
) -> Result<QualificationDerivedAccessRetainedPreflightReceiptV1, String> {
    preflight_qualification_derived_access_retained_root_with_validator_v1(
        request_path,
        &validate_current_execution_identity_v1,
    )
}

fn preflight_qualification_derived_access_retained_root_with_validator_v1(
    request_path: &Path,
    validate_execution: &impl Fn(
        &QualificationDerivedAccessExecutionIdentityV1,
        &Path,
        &Path,
    ) -> Result<(), String>,
) -> Result<QualificationDerivedAccessRetainedPreflightReceiptV1, String> {
    let request: QualificationDerivedAccessRetainedRootRequestV1 = read_json(request_path)?;
    request.validate()?;
    validate_execution(
        &request.execution,
        &request.source_checkout,
        &request.immutable_input_root,
    )?;
    let immutable_before =
        longitudinal_authoritative_store_data_inventory_v1(&request.immutable_input_root)
            .map_err(|error| error.to_string())?;
    if immutable_before.inventory_sha256 != request.admitted_root_sha256 {
        return Err("retained input does not match its admitted root identity".to_owned());
    }
    let clone_inventory = if request.qualification_clone_root.exists() {
        let inventory =
            longitudinal_authoritative_store_data_inventory_v1(&request.qualification_clone_root)
                .map_err(|error| error.to_string())?;
        if inventory != immutable_before {
            return Err("retained qualification clone differs from admitted truth".to_owned());
        }
        Some(inventory)
    } else {
        None
    };
    let immutable_after =
        longitudinal_authoritative_store_data_inventory_v1(&request.immutable_input_root)
            .map_err(|error| error.to_string())?;
    if immutable_before != immutable_after {
        return Err("retained input changed during preflight".to_owned());
    }
    Ok(QualificationDerivedAccessRetainedPreflightReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_RETAINED_PREFLIGHT_SCHEMA_V1.to_owned(),
        execution: request.execution,
        tier: request.tier,
        admitted_root_sha256: request.admitted_root_sha256,
        immutable_inventory: immutable_before,
        qualification_clone_inventory: clone_inventory,
        originals_unchanged: true,
    })
}

pub fn bootstrap_qualification_derived_access_retained_root_v1(
    request_path: &Path,
) -> Result<QualificationDerivedAccessRetainedBootstrapReceiptV1, String> {
    bootstrap_qualification_derived_access_retained_root_with_validator_v1(
        request_path,
        &validate_current_execution_identity_v1,
    )
}

fn bootstrap_qualification_derived_access_retained_root_with_validator_v1(
    request_path: &Path,
    validate_execution: &impl Fn(
        &QualificationDerivedAccessExecutionIdentityV1,
        &Path,
        &Path,
    ) -> Result<(), String>,
) -> Result<QualificationDerivedAccessRetainedBootstrapReceiptV1, String> {
    let request: QualificationDerivedAccessRetainedRootRequestV1 = read_json(request_path)?;
    request.validate()?;
    if !request.qualification_clone_root.exists() {
        return Err("retained bootstrap requires a separately created clone".to_owned());
    }
    let preflight = preflight_qualification_derived_access_retained_root_with_validator_v1(
        request_path,
        validate_execution,
    )?;
    validate_execution(
        &request.execution,
        &request.source_checkout,
        &request.qualification_clone_root,
    )?;
    let immutable_before = preflight.immutable_inventory;
    let clone_truth_before =
        longitudinal_authoritative_store_data_inventory_v1(&request.qualification_clone_root)
            .map_err(|error| error.to_string())?;
    let store =
        store_dir_for_repo(&request.qualification_clone_root).map_err(|error| error.to_string())?;
    let store_id = format!("store:derived-access:{}", request.admitted_root_sha256);
    let started = std::time::Instant::now();
    let mut progress_updates = 0_u64;
    let mut progress_completed = 0_u64;
    let mut progress_total = 0_u64;
    let mut high_water_derived_bytes = 0_u64;
    let cursor = SqliteCursorLedger::bootstrap_from_truth(
        &store,
        CursorLedgerIdentity::new(&store_id),
        1,
        |progress| {
            progress_updates = progress_updates.saturating_add(1);
            progress_completed = progress.completed as u64;
            progress_total = progress.total as u64;
            high_water_derived_bytes = high_water_derived_bytes
                .max(governed_derived_state_bytes(&store).unwrap_or_default());
            BootstrapControl::Continue
        },
    )
    .map_err(|error| error.to_string())?;
    drop(cursor);
    let adapter =
        QualificationDerivedAccessAdapter::open(&store, CursorLedgerIdentity::new(&store_id))
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
        LocatorRead::CatchUpRequired { .. } => {
            return Err("retained bootstrap remained behind observed truth".to_owned());
        }
    };
    let full_replay_matches_incremental = incremental == strict && incremental.as_of == applied;
    let semantic_receipt_sha256 = incremental.semantic_receipt;
    drop(adapter);
    let immutable_after =
        longitudinal_authoritative_store_data_inventory_v1(&request.immutable_input_root)
            .map_err(|error| error.to_string())?;
    let clone_truth_after =
        longitudinal_authoritative_store_data_inventory_v1(&request.qualification_clone_root)
            .map_err(|error| error.to_string())?;
    if immutable_after != immutable_before || clone_truth_after != clone_truth_before {
        return Err(format!(
            "retained truth changed during derived bootstrap: immutable {} files/{} bytes/{} -> {} files/{} bytes/{}; clone {} files/{} bytes/{} -> {} files/{} bytes/{}",
            immutable_before.file_count,
            immutable_before.byte_count,
            immutable_before.inventory_sha256,
            immutable_after.file_count,
            immutable_after.byte_count,
            immutable_after.inventory_sha256,
            clone_truth_before.file_count,
            clone_truth_before.byte_count,
            clone_truth_before.inventory_sha256,
            clone_truth_after.file_count,
            clone_truth_after.byte_count,
            clone_truth_after.inventory_sha256
        ));
    }
    Ok(QualificationDerivedAccessRetainedBootstrapReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_RETAINED_BOOTSTRAP_SCHEMA_V1.to_owned(),
        execution: request.execution,
        tier: request.tier,
        admitted_root_sha256: request.admitted_root_sha256,
        immutable_before,
        immutable_after,
        clone_truth_before,
        clone_truth_after,
        progress_updates,
        progress_completed,
        progress_total,
        elapsed_millis: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        high_water_derived_bytes: high_water_derived_bytes
            .max(governed_derived_state_bytes(&store)?),
        semantic_receipt_sha256,
        full_replay_matches_incremental,
    })
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
pub(super) fn bootstrap_qualification_derived_access_retained_root_for_test_v1(
    request_path: &Path,
    host_identity_sha256: String,
) -> Result<QualificationDerivedAccessRetainedBootstrapReceiptV1, String> {
    let validate_execution = move |expected: &QualificationDerivedAccessExecutionIdentityV1,
                                   source_checkout: &Path,
                                   evidence_root: &Path| {
        validate_current_execution_identity_with_host_authority_v1(
            expected,
            source_checkout,
            evidence_root,
            &host_identity_sha256,
        )
    };
    bootstrap_qualification_derived_access_retained_root_with_validator_v1(
        request_path,
        &validate_execution,
    )
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessPackageEntryV1 {
    pub relative_path: String,
    pub byte_count: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessPackageManifestV1 {
    pub schema: String,
    pub package_path: String,
    pub evaluation_path: String,
    pub entries: Vec<QualificationDerivedAccessPackageEntryV1>,
    pub complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessFragmentRequestV1 {
    pub schema: String,
    pub execution: QualificationDerivedAccessExecutionIdentityV1,
    pub receipt_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessEvidenceFragmentV1 {
    pub schema: String,
    pub package: QualificationDerivedAccessPackageV1,
    pub raw_receipts: Vec<serde_json::Value>,
    pub fragment_sha256: String,
}

impl QualificationDerivedAccessEvidenceFragmentV1 {
    fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.fragment_sha256.clear();
        let bytes = serde_json::to_vec(&preimage).map_err(|error| error.to_string())?;
        Ok(sha256_bytes_hex(&bytes))
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_ACCESS_FRAGMENT_SCHEMA_V1
            || self.package.schema != super::QUALIFICATION_DERIVED_ACCESS_PACKAGE_SCHEMA_V1
            || self.package.complete
            || self.package.execution_identities.len() != 1
            || self.raw_receipts.is_empty()
            || self.fragment_sha256 != self.canonical_sha256()?
        {
            return Err("derived-access evidence fragment drifted".to_owned());
        }
        self.package.execution_identities[0].validate()?;
        validate_summaries_against_raw(&self.package, &self.raw_receipts)
    }
}

pub(super) fn validate_summaries_against_raw(
    package: &QualificationDerivedAccessPackageV1,
    raw_receipts: &[serde_json::Value],
) -> Result<(), String> {
    let mut expected_operations = Vec::new();
    let mut expected_allocations = Vec::new();
    let mut expected_d0 = Vec::new();
    let mut expected_lifecycle = Vec::new();
    let mut expected_resources = None;
    let mut expected_bootstrap = Vec::new();
    let mut expected_change_reads = Vec::new();
    let mut expected_change_controls = Vec::new();
    let mut expected_change_storage = Vec::new();
    let mut expected_timeline_reads = Vec::new();
    let mut expected_timeline_storage = Vec::new();
    let mut expected_products = Vec::new();
    let mut expected_control_binaries = Vec::new();
    let mut change_receipt_schema = None;
    for value in raw_receipts {
        reject_derived_change_diagnostic_evidence_document_v1(value)?;
        match value.get("schema").and_then(serde_json::Value::as_str) {
            Some(QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V1) => {
                record_change_receipt_schema(&mut change_receipt_schema, "v1")?;
                let receipt: QualificationDerivedChangeReadReceiptV1 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                receipt.validate()?;
                if receipt.purpose
                    != QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification
                {
                    return Err("pre-cut Change falsifier is not package evidence".to_owned());
                }
                require_packaged_execution(package, &receipt.execution)?;
                if !expected_products.contains(&receipt.product) {
                    expected_products.push(receipt.product.clone());
                }
                expected_control_binaries.extend(receipt.control_binary_identities);
                expected_change_controls.extend(receipt.control_rows);
                expected_change_storage.extend(receipt.storage_rows);
                expected_change_reads.extend(receipt.rows);
            }
            Some(QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V2) => {
                record_change_receipt_schema(&mut change_receipt_schema, "v2")?;
                let receipt: QualificationDerivedChangeReadReceiptV2 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                receipt.validate()?;
                require_packaged_execution(package, &receipt.base.execution)?;
                append_exact_source_change_read_receipt(
                    &mut expected_products,
                    &mut expected_control_binaries,
                    &mut expected_change_controls,
                    &mut expected_change_storage,
                    &mut expected_change_reads,
                    &receipt.base,
                );
                expected_timeline_reads.extend(receipt.timeline_read_rows);
                expected_timeline_storage.extend(receipt.timeline_storage_rows);
            }
            Some(QUALIFICATION_DERIVED_ACCESS_NATIVE_SMOKE_RECEIPT_SCHEMA_V1) => {
                let receipt: QualificationDerivedAccessNativeSmokeRunReceiptV1 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                receipt.execution.validate()?;
                require_packaged_execution(package, &receipt.execution)?;
                match &receipt.payload {
                    QualificationDerivedAccessNativeSmokePayloadV1::D0_128(smoke) => {
                        expected_d0.push(d0_smoke_row(receipt.execution.platform, smoke)?);
                        expected_operations.extend(smoke_operation_rows(
                            QualificationDerivedAccessTierV1::D0_128,
                            receipt.execution.platform,
                            &smoke.operation_receipts,
                        )?);
                    }
                    QualificationDerivedAccessNativeSmokePayloadV1::Longitudinal(smoke) => {
                        smoke.validate()?;
                        expected_operations.extend(smoke_operation_rows(
                            smoke.tier,
                            receipt.execution.platform,
                            &smoke.operation_receipts,
                        )?);
                    }
                }
            }
            Some(QUALIFICATION_DERIVED_ACCESS_SCALE_RECEIPT_SCHEMA_V1) => {
                let receipt: QualificationDerivedAccessScaleReceiptV1 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                validate_scale_receipt(&receipt)?;
                require_packaged_execution(package, &receipt.execution)?;
                expected_operations.extend(receipt.operation_rows);
                expected_allocations.push(receipt.allocation);
            }
            Some(super::QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_RUN_SCHEMA_V1) => {
                let receipt: super::QualificationDerivedAccessLifecycleRunReceiptV1 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                require_packaged_execution(package, &receipt.execution)?;
                if receipt.platform != receipt.execution.platform
                    || !receipt.source_unchanged
                    || receipt.source_before != receipt.source_after
                {
                    return Err("lifecycle receipt authority drifted".to_owned());
                }
                expected_lifecycle.extend(receipt.rows);
            }
            Some(QUALIFICATION_DERIVED_ACCESS_RETAINED_BOOTSTRAP_SCHEMA_V1) => {
                let receipt: QualificationDerivedAccessRetainedBootstrapReceiptV1 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                require_packaged_execution(package, &receipt.execution)?;
                if !receipt.full_replay_matches_incremental
                    || receipt.immutable_before != receipt.immutable_after
                    || receipt.clone_truth_before != receipt.clone_truth_after
                    || receipt.progress_completed != receipt.progress_total
                {
                    return Err("retained bootstrap receipt failed equivalence".to_owned());
                }
                expected_bootstrap.push(bootstrap_evidence(&receipt));
            }
            Some(QUALIFICATION_DERIVED_ACCESS_RESOURCE_RECEIPT_SCHEMA_V1) => {
                let receipt: QualificationDerivedAccessResourceReceiptV1 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                require_packaged_execution(package, &receipt.execution)?;
                validate_resource_receipt(&receipt)?;
                if expected_resources.replace(receipt.resources).is_some() {
                    return Err("raw receipts duplicate resource evidence".to_owned());
                }
            }
            Some(schema) => {
                return Err(format!(
                    "unsupported derived-access raw receipt schema: {schema}"
                ));
            }
            None => return Err("derived-access raw receipt omitted schema".to_owned()),
        }
    }
    match change_receipt_schema {
        Some("v1")
            if package.evaluator_revision
                != super::QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3 =>
        {
            return Err("V1 Change receipt requires evaluator v3".to_owned());
        }
        Some("v2")
            if package.evaluator_revision
                != super::QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4 =>
        {
            return Err("V2 Change receipt requires evaluator v4".to_owned());
        }
        _ => {}
    }
    if package.operation_rows.len() != expected_operations.len()
        || package.allocation_rows.len() != expected_allocations.len()
        || package.d0_rows.len() != expected_d0.len()
        || unordered_json_rows(&package.operation_rows)?
            != unordered_json_rows(&expected_operations)?
        || unordered_json_rows(&package.allocation_rows)?
            != unordered_json_rows(&expected_allocations)?
        || unordered_json_rows(&package.d0_rows)? != unordered_json_rows(&expected_d0)?
        || unordered_json_rows(&package.lifecycle_rows)?
            != unordered_json_rows(&expected_lifecycle)?
        || package.resources != expected_resources
        || unordered_json_rows(&package.bootstrap_rows)?
            != unordered_json_rows(&expected_bootstrap)?
        || unordered_json_rows(&package.change_read_rows)?
            != unordered_json_rows(&expected_change_reads)?
        || unordered_json_rows(&package.change_control_rows)?
            != unordered_json_rows(&expected_change_controls)?
        || unordered_json_rows(&package.change_storage_rows)?
            != unordered_json_rows(&expected_change_storage)?
        || unordered_json_rows(&package.timeline_read_rows)?
            != unordered_json_rows(&expected_timeline_reads)?
        || unordered_json_rows(&package.timeline_storage_rows)?
            != unordered_json_rows(&expected_timeline_storage)?
        || unordered_json_rows(&package.product_identities)?
            != unordered_json_rows(&expected_products)?
        || unordered_json_rows(&package.change_control_binary_identities)?
            != unordered_json_rows(&expected_control_binaries)?
    {
        return Err("derived-access package summaries differ from raw receipts".to_owned());
    }
    Ok(())
}

fn record_change_receipt_schema(
    observed: &mut Option<&'static str>,
    schema: &'static str,
) -> Result<(), String> {
    if observed.is_some_and(|existing| existing != schema) {
        return Err("derived-access raw inputs mix V1 and V2 Change receipts".to_owned());
    }
    *observed = Some(schema);
    Ok(())
}

fn append_exact_source_change_read_receipt(
    products: &mut Vec<QualificationDerivedAccessProductIdentityV1>,
    control_binaries: &mut Vec<QualificationDerivedChangeControlBinaryIdentityV1>,
    controls: &mut Vec<QualificationDerivedChangeControlEvidenceV1>,
    storage: &mut Vec<QualificationDerivedChangeStorageEvidenceV1>,
    reads: &mut Vec<QualificationDerivedChangeReadEvidenceV1>,
    receipt: &QualificationDerivedChangeReadReceiptV1,
) {
    if !products.contains(&receipt.product) {
        products.push(receipt.product.clone());
    }
    control_binaries.extend(receipt.control_binary_identities.clone());
    controls.extend(receipt.control_rows.clone());
    storage.extend(receipt.storage_rows.clone());
    reads.extend(receipt.rows.clone());
}

fn require_packaged_execution(
    package: &QualificationDerivedAccessPackageV1,
    execution: &QualificationDerivedAccessExecutionIdentityV1,
) -> Result<(), String> {
    if package.execution_identities.contains(execution) {
        Ok(())
    } else {
        Err("raw receipt execution identity is absent from its package".to_owned())
    }
}

fn bootstrap_evidence(
    receipt: &QualificationDerivedAccessRetainedBootstrapReceiptV1,
) -> super::QualificationDerivedAccessBootstrapEvidenceV1 {
    super::QualificationDerivedAccessBootstrapEvidenceV1 {
        tier: receipt.tier,
        status: QualificationDerivedAccessStatusV1::Passed,
        elapsed_seconds: receipt
            .elapsed_millis
            .div_ceil(1_000)
            .try_into()
            .unwrap_or(u32::MAX),
        progress_reported: receipt.progress_updates > 0,
        high_water_derived_bytes: receipt.high_water_derived_bytes,
    }
}

fn validate_resource_receipt(
    receipt: &QualificationDerivedAccessResourceReceiptV1,
) -> Result<(), String> {
    if receipt.roots_before != receipt.roots_after
        || receipt.baseline.schema != QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_RECEIPT_SCHEMA_V1
        || receipt.l7.schema != QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_RECEIPT_SCHEMA_V1
        || receipt.l100.schema != QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_RECEIPT_SCHEMA_V1
        || receipt.root_tiers.len() != receipt.roots_before.len()
    {
        Err("resource receipt authority drifted".to_owned())
    } else {
        Ok(())
    }
}

fn unordered_json_rows<T: Serialize>(rows: &[T]) -> Result<BTreeSet<String>, String> {
    rows.iter()
        .map(|row| serde_json::to_string(row).map_err(|error| error.to_string()))
        .collect()
}

pub fn build_qualification_derived_access_fragment_v1(
    request_path: &Path,
) -> Result<QualificationDerivedAccessEvidenceFragmentV1, String> {
    reject_derived_change_diagnostic_evidence_path_v1(request_path)?;
    let request: QualificationDerivedAccessFragmentRequestV1 = read_json(request_path)?;
    if request.schema != QUALIFICATION_DERIVED_ACCESS_FRAGMENT_REQUEST_SCHEMA_V1
        || request.receipt_paths.is_empty()
    {
        return Err("invalid derived-access fragment request".to_owned());
    }
    request.execution.validate()?;
    let mut package = QualificationDerivedAccessPackageV1 {
        schema: super::QUALIFICATION_DERIVED_ACCESS_PACKAGE_SCHEMA_V1.to_owned(),
        evaluator_revision: super::QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3.to_owned(),
        evaluator_procedure_sha256:
            super::qualification_derived_access_evaluator_v3_procedure_sha256(),
        proposed_profile_id: "sqlite-wal-bodyless-v1".to_owned(),
        execution_identities: vec![request.execution.clone()],
        product_identities: Vec::new(),
        change_control_binary_identities: Vec::new(),
        root_bindings: Vec::new(),
        d0_rows: Vec::new(),
        operation_rows: Vec::new(),
        lifecycle_rows: Vec::new(),
        resources: None,
        allocation_rows: Vec::new(),
        bootstrap_rows: Vec::new(),
        change_read_rows: Vec::new(),
        change_control_rows: Vec::new(),
        change_storage_rows: Vec::new(),
        timeline_read_rows: Vec::new(),
        timeline_storage_rows: Vec::new(),
        complete: false,
    };
    let mut raw_receipts = Vec::new();
    let mut change_receipt_schema = None;
    for path in &request.receipt_paths {
        validate_receipt_input_path(path)?;
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        reject_derived_change_diagnostic_evidence_input_v1(path, &value)?;
        let schema = value
            .get("schema")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "derived-access receipt omitted schema".to_owned())?;
        match schema {
            QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V1 => {
                record_change_receipt_schema(&mut change_receipt_schema, "v1")?;
                let receipt: QualificationDerivedChangeReadReceiptV1 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                receipt.validate()?;
                if receipt.execution != request.execution {
                    return Err("Change read receipt authority drifted".to_owned());
                }
                if receipt.purpose
                    != QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification
                {
                    return Err(
                        "pre-cut Change falsifier is not admissible package evidence".to_owned(),
                    );
                }
                if !package.product_identities.contains(&receipt.product) {
                    package.product_identities.push(receipt.product);
                }
                package
                    .change_control_binary_identities
                    .extend(receipt.control_binary_identities);
                package.change_control_rows.extend(receipt.control_rows);
                package.change_storage_rows.extend(receipt.storage_rows);
                package.change_read_rows.extend(receipt.rows);
            }
            QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V2 => {
                record_change_receipt_schema(&mut change_receipt_schema, "v2")?;
                let receipt: QualificationDerivedChangeReadReceiptV2 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                receipt.validate()?;
                if receipt.base.execution != request.execution {
                    return Err("Change read successor receipt authority drifted".to_owned());
                }
                package.evaluator_revision =
                    super::QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4.to_owned();
                package.evaluator_procedure_sha256 =
                    super::qualification_derived_access_evaluator_v4_procedure_sha256();
                if !package.product_identities.contains(&receipt.base.product) {
                    package
                        .product_identities
                        .push(receipt.base.product.clone());
                }
                package
                    .change_control_binary_identities
                    .extend(receipt.base.control_binary_identities.clone());
                package
                    .change_control_rows
                    .extend(receipt.base.control_rows.clone());
                package
                    .change_storage_rows
                    .extend(receipt.base.storage_rows.clone());
                package.change_read_rows.extend(receipt.base.rows.clone());
                package
                    .timeline_read_rows
                    .extend(receipt.timeline_read_rows);
                package
                    .timeline_storage_rows
                    .extend(receipt.timeline_storage_rows);
            }
            QUALIFICATION_DERIVED_ACCESS_NATIVE_SMOKE_RECEIPT_SCHEMA_V1 => {
                let receipt: QualificationDerivedAccessNativeSmokeRunReceiptV1 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                if receipt.execution != request.execution {
                    return Err("native smoke receipt authority drifted".to_owned());
                }
                match &receipt.payload {
                    QualificationDerivedAccessNativeSmokePayloadV1::D0_128(receipt) => {
                        append_d0_smoke_rows(&mut package, request.execution.platform, receipt)?;
                    }
                    QualificationDerivedAccessNativeSmokePayloadV1::Longitudinal(receipt) => {
                        append_longitudinal_smoke_rows(
                            &mut package,
                            request.execution.platform,
                            receipt,
                        )?;
                    }
                }
            }
            super::QUALIFICATION_DERIVED_ACCESS_SMOKE_SCHEMA_V1
            | super::QUALIFICATION_DERIVED_ACCESS_LONGITUDINAL_SMOKE_SCHEMA_V1 => {
                return Err("unbound smoke receipt is not admissible evidence".to_owned());
            }
            super::QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_RUN_SCHEMA_V1 => {
                let receipt: super::QualificationDerivedAccessLifecycleRunReceiptV1 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                if receipt.execution != request.execution
                    || receipt.platform != request.execution.platform
                    || !receipt.source_unchanged
                    || receipt.source_before != receipt.source_after
                {
                    return Err("lifecycle receipt authority drifted".to_owned());
                }
                append_root_binding(
                    &mut package,
                    receipt.tier,
                    "lifecycle-source",
                    &receipt.source_before.inventory_sha256,
                )?;
                package.lifecycle_rows.extend(receipt.rows);
            }
            QUALIFICATION_DERIVED_ACCESS_RETAINED_BOOTSTRAP_SCHEMA_V1 => {
                let receipt: QualificationDerivedAccessRetainedBootstrapReceiptV1 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                if receipt.execution != request.execution
                    || !receipt.full_replay_matches_incremental
                    || receipt.immutable_before != receipt.immutable_after
                    || receipt.clone_truth_before != receipt.clone_truth_after
                    || receipt.progress_completed != receipt.progress_total
                {
                    return Err("retained bootstrap receipt failed equivalence".to_owned());
                }
                append_root_binding(
                    &mut package,
                    receipt.tier,
                    "retained-bootstrap-clone",
                    &receipt.admitted_root_sha256,
                )?;
                package.bootstrap_rows.push(bootstrap_evidence(&receipt));
            }
            QUALIFICATION_DERIVED_ACCESS_SCALE_RECEIPT_SCHEMA_V1 => {
                let receipt: QualificationDerivedAccessScaleReceiptV1 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                if receipt.execution != request.execution {
                    return Err("scale receipt authority drifted".to_owned());
                }
                validate_scale_receipt(&receipt)?;
                for (ordinal, inventory) in receipt.root_before.iter().enumerate() {
                    append_root_binding(
                        &mut package,
                        receipt.tier,
                        &format!("scale-root-{ordinal}"),
                        &inventory.inventory_sha256,
                    )?;
                }
                package.operation_rows.extend(receipt.operation_rows);
                package.allocation_rows.push(receipt.allocation);
            }
            QUALIFICATION_DERIVED_ACCESS_RESOURCE_RECEIPT_SCHEMA_V1 => {
                let receipt: QualificationDerivedAccessResourceReceiptV1 =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
                if receipt.execution != request.execution {
                    return Err("resource receipt authority drifted".to_owned());
                }
                validate_resource_receipt(&receipt)?;
                for (ordinal, (tier, inventory)) in receipt
                    .root_tiers
                    .iter()
                    .zip(&receipt.roots_before)
                    .enumerate()
                {
                    append_root_binding(
                        &mut package,
                        *tier,
                        &format!("resource-root-{ordinal}"),
                        &inventory.inventory_sha256,
                    )?;
                }
                if package.resources.replace(receipt.resources).is_some() {
                    return Err("fragment duplicated resource evidence".to_owned());
                }
            }
            _ => {
                return Err(format!(
                    "unsupported derived-access receipt schema: {schema}"
                ));
            }
        }
        raw_receipts.push(value);
    }
    let mut fragment = QualificationDerivedAccessEvidenceFragmentV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_FRAGMENT_SCHEMA_V1.to_owned(),
        package,
        raw_receipts,
        fragment_sha256: String::new(),
    };
    fragment.fragment_sha256 = fragment.canonical_sha256()?;
    fragment.validate()?;
    Ok(fragment)
}

fn append_d0_smoke_rows(
    package: &mut QualificationDerivedAccessPackageV1,
    platform: super::QualificationDerivedAccessPlatformV1,
    receipt: &super::QualificationDerivedAccessSmokeReceiptV1,
) -> Result<(), String> {
    receipt.validate()?;
    if !receipt.counters_captured {
        return Err("native smoke omitted counting instrumentation".to_owned());
    }
    append_root_binding(
        package,
        QualificationDerivedAccessTierV1::D0_128,
        "native-smoke-root-a",
        &receipt.d0_pair.root_a.store_inventory.inventory_sha256,
    )?;
    append_root_binding(
        package,
        QualificationDerivedAccessTierV1::D0_128,
        "native-smoke-root-b",
        &receipt.d0_pair.root_b.store_inventory.inventory_sha256,
    )?;
    package.d0_rows.push(d0_smoke_row(platform, receipt)?);
    package.operation_rows.extend(smoke_operation_rows(
        QualificationDerivedAccessTierV1::D0_128,
        platform,
        &receipt.operation_receipts,
    )?);
    Ok(())
}

fn d0_smoke_row(
    platform: super::QualificationDerivedAccessPlatformV1,
    receipt: &super::QualificationDerivedAccessSmokeReceiptV1,
) -> Result<super::QualificationDerivedAccessD0EvidenceV1, String> {
    receipt.validate()?;
    if !receipt.counters_captured {
        return Err("native smoke omitted counting instrumentation".to_owned());
    }
    let root = &receipt.d0_pair.root_a;
    Ok(super::QualificationDerivedAccessD0EvidenceV1 {
        platform,
        stored_events: root.event_count,
        revisions: root.revision_count,
        independently_referenced_objects: root.independently_referenced_objects,
        schedule_sha256: root.coverage_schedule_sha256.clone(),
        ordered_schedule_sha256: root.ordered_schedule_sha256.clone(),
        root_a_sha256: receipt
            .d0_pair
            .root_a
            .store_inventory
            .inventory_sha256
            .clone(),
        root_b_sha256: receipt
            .d0_pair
            .root_b
            .store_inventory
            .inventory_sha256
            .clone(),
        byte_identical: receipt.d0_pair.byte_identical,
    })
}

fn append_longitudinal_smoke_rows(
    package: &mut QualificationDerivedAccessPackageV1,
    platform: super::QualificationDerivedAccessPlatformV1,
    receipt: &super::QualificationDerivedAccessLongitudinalSmokeReceiptV1,
) -> Result<(), String> {
    receipt.validate()?;
    if !receipt.counters_captured {
        return Err("native smoke omitted counting instrumentation".to_owned());
    }
    append_root_binding(
        package,
        receipt.tier,
        "native-smoke-root-a",
        &receipt.root_a_sha256,
    )?;
    append_root_binding(
        package,
        receipt.tier,
        "native-smoke-root-b",
        &receipt.root_b_sha256,
    )?;
    package.operation_rows.extend(smoke_operation_rows(
        receipt.tier,
        platform,
        &receipt.operation_receipts,
    )?);
    Ok(())
}

fn append_root_binding(
    package: &mut QualificationDerivedAccessPackageV1,
    tier: QualificationDerivedAccessTierV1,
    role: &str,
    admitted_root_sha256: &str,
) -> Result<(), String> {
    validate_digest(admitted_root_sha256, "admitted root binding")?;
    let execution = package
        .execution_identities
        .first()
        .ok_or_else(|| "root binding lacks execution identity".to_owned())?;
    package
        .root_bindings
        .push(super::QualificationDerivedAccessRootBindingV1 {
            platform: execution.platform,
            tier,
            role: role.to_owned(),
            command_sha256: execution.command_sha256.clone(),
            admitted_root_sha256: admitted_root_sha256.to_owned(),
        });
    Ok(())
}

fn smoke_operation_rows(
    tier: QualificationDerivedAccessTierV1,
    platform: super::QualificationDerivedAccessPlatformV1,
    receipts: &[super::QualificationDerivedAccessSmokeOperationReceiptV1],
) -> Result<Vec<super::QualificationDerivedAccessOperationEvidenceV1>, String> {
    let contract = super::qualification_derived_access_contract_v1();
    if receipts.len() != QualificationDerivedAccessOperationV1::ALL.len() {
        return Err("native smoke omitted operation receipts".to_owned());
    }
    QualificationDerivedAccessOperationV1::ALL
        .into_iter()
        .map(|operation| {
            let receipt = receipts
                .iter()
                .find(|receipt| receipt.operation == operation)
                .ok_or_else(|| format!("native smoke omitted {operation:?}"))?;
            validate_digest(&receipt.semantic_receipt_sha256, "native semantic receipt")?;
            let requirement = contract
                .operations
                .iter()
                .find(|requirement| requirement.operation == operation)
                .expect("contract operation");
            let selected_output = match operation {
                QualificationDerivedAccessOperationV1::WindowHead
                | QualificationDerivedAccessOperationV1::WindowMiddle
                | QualificationDerivedAccessOperationV1::WindowTail
                | QualificationDerivedAccessOperationV1::PostOne => {
                    receipt.counters.carrier_opens.max(1)
                }
                QualificationDerivedAccessOperationV1::RevisionDetailActive
                | QualificationDerivedAccessOperationV1::RevisionDetailRemoved => {
                    receipt.counters.carrier_opens.max(1)
                }
                _ => 1,
            };
            let work = selected_work_portable(&receipt.counters, selected_output);
            Ok(super::QualificationDerivedAccessOperationEvidenceV1 {
                tier,
                platform,
                operation,
                status: QualificationDerivedAccessStatusV1::Passed,
                process_scope: requirement.process_scope,
                semantic_receipt_matches: true,
                complexity: classify_complexity_portable(selected_output, work, u64::MAX, false),
                retained_samples: 1,
                wall_p95_ms: None,
                process_cpu_p95_ms: None,
                selected_output_count: Some(selected_output),
                unselected_work_count: Some(work.saturating_sub(selected_output)),
                selected_work_count: work,
                retained_cardinality: match tier {
                    QualificationDerivedAccessTierV1::D0_128 => 128,
                    QualificationDerivedAccessTierV1::L1 => 1_024,
                    QualificationDerivedAccessTierV1::L7 => 7_168,
                    QualificationDerivedAccessTierV1::L100
                    | QualificationDerivedAccessTierV1::C262 => {
                        return Err("native smoke used a retained scale tier".to_owned());
                    }
                },
                l100_to_c262_selected_work_ratio_milli: None,
                counters: receipt.counters.clone(),
            })
        })
        .collect()
}

pub fn publish_qualification_derived_access_package_v1(
    root: &Path,
    package: &QualificationDerivedAccessPackageV1,
    raw_receipts: &[(&str, &[u8])],
) -> Result<QualificationDerivedAccessEvaluationV1, String> {
    reject_derived_change_diagnostic_evidence_path_v1(root)?;
    let validated_raw_paths = raw_receipts
        .iter()
        .map(|(name, bytes)| {
            let relative = PathBuf::from("raw").join(name);
            validate_qualification_evidence_relative_path_v1(&relative)?;
            let value: serde_json::Value =
                serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
            reject_derived_change_diagnostic_evidence_input_v1(&relative, &value)?;
            match value.get("schema").and_then(serde_json::Value::as_str) {
                Some(QUALIFICATION_DERIVED_ACCESS_FRAGMENT_SCHEMA_V1) => Ok(relative),
                Some(schema) => Err(format!(
                    "unsupported derived-access package raw input schema: {schema}"
                )),
                None => Err("derived-access package raw input omitted schema".to_owned()),
            }
        })
        .collect::<Result<Vec<_>, String>>()?;
    if root.exists() {
        if root
            .read_dir()
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
        {
            return Err("derived-access package root must be empty".to_owned());
        }
    } else {
        std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    }
    let evaluation = evaluate_qualification_derived_access_v1(package)?;
    let mut entries = Vec::new();
    write_json_entry(root, Path::new("package.json"), package, &mut entries)?;
    write_json_entry(
        root,
        Path::new("evaluation.json"),
        &evaluation,
        &mut entries,
    )?;
    for ((_, bytes), relative) in raw_receipts.iter().zip(&validated_raw_paths) {
        write_bytes_entry(root, relative, bytes, &mut entries)?;
    }
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let manifest = QualificationDerivedAccessPackageManifestV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_PACKAGE_MANIFEST_SCHEMA_V1.to_owned(),
        package_path: "package.json".to_owned(),
        evaluation_path: "evaluation.json".to_owned(),
        entries,
        complete: true,
    };
    write_json_create_new(root.join("manifest.json").as_path(), &manifest)?;
    verify_qualification_derived_access_package_v1(root)?;
    Ok(evaluation)
}

pub fn assemble_qualification_derived_access_package_v1(
    inputs: &[PathBuf],
    output_root: &Path,
) -> Result<QualificationDerivedAccessEvaluationV1, String> {
    reject_derived_change_diagnostic_evidence_path_v1(output_root)?;
    if inputs.is_empty() {
        return Err("derived-access package assembly requires evidence inputs".to_owned());
    }
    let mut packages = inputs
        .iter()
        .map(|path| {
            validate_receipt_input_path(path)?;
            let value: serde_json::Value = read_json(path)?;
            reject_derived_change_diagnostic_evidence_input_v1(path, &value)?;
            match value.get("schema").and_then(serde_json::Value::as_str) {
                Some(QUALIFICATION_DERIVED_ACCESS_FRAGMENT_SCHEMA_V1) => {
                    let fragment: QualificationDerivedAccessEvidenceFragmentV1 =
                        serde_json::from_value(value).map_err(|error| error.to_string())?;
                    fragment.validate()?;
                    Ok(fragment.package)
                }
                Some(super::QUALIFICATION_DERIVED_ACCESS_PACKAGE_SCHEMA_V1) => {
                    Err("derived-access assembly accepts only raw-bound fragments".to_owned())
                }
                _ => Err("unsupported derived-access package input".to_owned()),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let first = packages
        .first()
        .ok_or_else(|| "derived-access evidence inputs are absent".to_owned())?;
    let schema = first.schema.clone();
    let evaluator_revision = first.evaluator_revision.clone();
    let evaluator_procedure_sha256 = first.evaluator_procedure_sha256.clone();
    let proposed_profile_id = first.proposed_profile_id.clone();
    if packages.iter().any(|package| {
        package.schema != schema
            || package.evaluator_revision != evaluator_revision
            || package.evaluator_procedure_sha256 != evaluator_procedure_sha256
            || package.proposed_profile_id != proposed_profile_id
    }) {
        return Err("derived-access evidence inputs mix package authority".to_owned());
    }
    let mut resources = packages
        .iter_mut()
        .filter_map(|package| package.resources.take())
        .collect::<Vec<_>>();
    if resources.len() > 1 {
        return Err("derived-access evidence inputs duplicate resource inventory".to_owned());
    }
    let execution_identities = packages
        .iter_mut()
        .flat_map(|package| std::mem::take(&mut package.execution_identities))
        .collect();
    let combined = QualificationDerivedAccessPackageV1 {
        schema,
        evaluator_revision,
        evaluator_procedure_sha256,
        proposed_profile_id,
        execution_identities,
        product_identities: packages
            .iter_mut()
            .flat_map(|package| std::mem::take(&mut package.product_identities))
            .collect(),
        change_control_binary_identities: packages
            .iter_mut()
            .flat_map(|package| std::mem::take(&mut package.change_control_binary_identities))
            .collect(),
        root_bindings: packages
            .iter_mut()
            .flat_map(|package| std::mem::take(&mut package.root_bindings))
            .collect(),
        d0_rows: packages
            .iter_mut()
            .flat_map(|package| std::mem::take(&mut package.d0_rows))
            .collect(),
        operation_rows: packages
            .iter_mut()
            .flat_map(|package| std::mem::take(&mut package.operation_rows))
            .collect(),
        lifecycle_rows: packages
            .iter_mut()
            .flat_map(|package| std::mem::take(&mut package.lifecycle_rows))
            .collect(),
        resources: resources.pop(),
        allocation_rows: packages
            .iter_mut()
            .flat_map(|package| std::mem::take(&mut package.allocation_rows))
            .collect(),
        bootstrap_rows: packages
            .iter_mut()
            .flat_map(|package| std::mem::take(&mut package.bootstrap_rows))
            .collect(),
        change_read_rows: packages
            .iter_mut()
            .flat_map(|package| std::mem::take(&mut package.change_read_rows))
            .collect(),
        change_control_rows: packages
            .iter_mut()
            .flat_map(|package| std::mem::take(&mut package.change_control_rows))
            .collect(),
        change_storage_rows: packages
            .iter_mut()
            .flat_map(|package| std::mem::take(&mut package.change_storage_rows))
            .collect(),
        timeline_read_rows: packages
            .iter_mut()
            .flat_map(|package| std::mem::take(&mut package.timeline_read_rows))
            .collect(),
        timeline_storage_rows: packages
            .iter_mut()
            .flat_map(|package| std::mem::take(&mut package.timeline_storage_rows))
            .collect(),
        complete: true,
    };
    // Evaluation is deliberately performed before any output is published.
    // Duplicates and mixed identities are package errors; missing platform
    // evidence remains an evaluable insufficient-evidence outcome.
    evaluate_qualification_derived_access_v1(&combined)?;
    let raw = inputs
        .iter()
        .enumerate()
        .map(|(index, path)| {
            std::fs::read(path)
                .map(|bytes| (format!("input-{index:03}.json"), bytes))
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let raw_refs = raw
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    publish_qualification_derived_access_package_v1(output_root, &combined, &raw_refs)
}

pub fn verify_qualification_derived_access_package_v1(
    root: &Path,
) -> Result<QualificationDerivedAccessEvaluationV1, String> {
    reject_derived_change_diagnostic_evidence_path_v1(root)?;
    let manifest_bytes =
        std::fs::read(root.join("manifest.json")).map_err(|error| error.to_string())?;
    let manifest: QualificationDerivedAccessPackageManifestV1 =
        serde_json::from_slice(&manifest_bytes).map_err(|error| error.to_string())?;
    if manifest.schema != QUALIFICATION_DERIVED_ACCESS_PACKAGE_MANIFEST_SCHEMA_V1
        || !manifest.complete
        || manifest.package_path != "package.json"
        || manifest.evaluation_path != "evaluation.json"
    {
        return Err("incomplete derived-access package manifest".to_owned());
    }
    let expected_paths = manifest
        .entries
        .iter()
        .map(|entry| entry.relative_path.clone())
        .collect::<BTreeSet<_>>();
    if expected_paths.len() != manifest.entries.len() {
        return Err("duplicate derived-access package entry".to_owned());
    }
    if !expected_paths.contains(&manifest.package_path)
        || !expected_paths.contains(&manifest.evaluation_path)
        || expected_paths.iter().any(|path| {
            path != &manifest.package_path
                && path != &manifest.evaluation_path
                && !path.starts_with("raw/")
        })
    {
        return Err("derived-access package inventory is outside its closed schema".to_owned());
    }
    for entry in &manifest.entries {
        let relative = Path::new(&entry.relative_path);
        validate_qualification_evidence_relative_path_v1(relative)?;
        let bytes = std::fs::read(root.join(relative)).map_err(|error| error.to_string())?;
        if bytes.len() as u64 != entry.byte_count || sha256_bytes_hex(&bytes) != entry.sha256 {
            return Err(format!(
                "derived-access package entry failed verification: {}",
                entry.relative_path
            ));
        }
    }
    let actual_paths = list_package_files(root)?;
    let mut allowed = expected_paths;
    allowed.insert("manifest.json".to_owned());
    if actual_paths != allowed {
        return Err("derived-access package contains unlisted files".to_owned());
    }
    let package: QualificationDerivedAccessPackageV1 =
        read_json(root.join(&manifest.package_path).as_path())?;
    let recorded: QualificationDerivedAccessEvaluationV1 =
        read_json(root.join(&manifest.evaluation_path).as_path())?;
    let mut raw_receipts = Vec::new();
    for entry in &manifest.entries {
        if !entry.relative_path.starts_with("raw/") {
            continue;
        }
        let value: serde_json::Value = read_json(root.join(&entry.relative_path).as_path())?;
        reject_derived_change_diagnostic_evidence_input_v1(
            Path::new(&entry.relative_path),
            &value,
        )?;
        match value.get("schema").and_then(serde_json::Value::as_str) {
            Some(QUALIFICATION_DERIVED_ACCESS_FRAGMENT_SCHEMA_V1) => {
                let fragment: QualificationDerivedAccessEvidenceFragmentV1 =
                    serde_json::from_value(value).map_err(|error| error.to_string())?;
                fragment.validate()?;
                raw_receipts.extend(fragment.raw_receipts);
            }
            Some(schema) => {
                return Err(format!(
                    "unsupported derived-access package raw input schema: {schema}"
                ));
            }
            None => return Err("derived-access package raw input omitted schema".to_owned()),
        }
    }
    validate_summaries_against_raw(&package, &raw_receipts)?;
    let evaluated = evaluate_qualification_derived_access_v1(&package)?;
    if evaluated != recorded
        || evaluated.schema != QUALIFICATION_DERIVED_ACCESS_EVALUATION_SCHEMA_V1
    {
        return Err("derived-access package evaluation drifted".to_owned());
    }
    Ok(evaluated)
}

pub fn validate_qualification_evidence_relative_path_v1(path: &Path) -> Result<(), String> {
    reject_derived_change_diagnostic_evidence_path_v1(path)?;
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err("evidence path must be a non-empty relative path".to_owned());
    }
    let mut lowered = Vec::new();
    for component in path.components() {
        let Component::Normal(value) = component else {
            return Err("evidence path contains a non-normal component".to_owned());
        };
        lowered.push(value.to_string_lossy().to_ascii_lowercase());
    }
    if lowered.iter().any(|component| {
        matches!(
            component.as_str(),
            ".git" | "src" | "target" | "build" | ".pointbreak" | "node_modules"
        )
    }) {
        return Err("source, build, and private store paths are not evidence".to_owned());
    }
    Ok(())
}

fn validate_receipt_input_path(path: &Path) -> Result<(), String> {
    reject_derived_change_diagnostic_evidence_path_v1(path)?;
    if path.as_os_str().is_empty() {
        return Err("evidence receipt path is empty".to_owned());
    }
    for component in path.components() {
        if let Component::Normal(value) = component {
            let value = value.to_string_lossy().to_ascii_lowercase();
            if matches!(
                value.as_str(),
                ".git" | "src" | "target" | "build" | ".pointbreak" | "node_modules"
            ) {
                return Err("source, build, and private store paths are not evidence".to_owned());
            }
        }
    }
    Ok(())
}

fn write_json_entry<T: Serialize>(
    root: &Path,
    relative: &Path,
    value: &T,
    entries: &mut Vec<QualificationDerivedAccessPackageEntryV1>,
) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    write_bytes_entry(root, relative, &bytes, entries)
}

fn write_bytes_entry(
    root: &Path,
    relative: &Path,
    bytes: &[u8],
    entries: &mut Vec<QualificationDerivedAccessPackageEntryV1>,
) -> Result<(), String> {
    validate_qualification_evidence_relative_path_v1(relative)?;
    let destination = root.join(relative);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&destination)
        .map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    entries.push(QualificationDerivedAccessPackageEntryV1 {
        relative_path: relative.to_string_lossy().replace('\\', "/"),
        byte_count: bytes.len() as u64,
        sha256: sha256_bytes_hex(bytes),
    });
    Ok(())
}

fn write_json_create_new<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    serde_json::from_reader(file).map_err(|error| error.to_string())
}

fn list_package_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut paths = BTreeSet::new();
    collect_package_files(root, root, &mut paths)?;
    Ok(paths)
}

pub(super) fn governed_derived_state_bytes(store_root: &Path) -> Result<u64, String> {
    if !store_root.exists() {
        return Ok(0);
    }
    let mut total = 0_u64;
    for entry in std::fs::read_dir(store_root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if is_governed_derived_store_entry_v1(name, file_type.is_dir(), file_type.is_file()) {
            total = total.saturating_add(directory_file_bytes(&entry.path())?);
        }
    }
    Ok(total)
}

fn directory_file_bytes(root: &Path) -> Result<u64, String> {
    if !root.exists() {
        return Ok(0);
    }
    let metadata = std::fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Err("derived-state path is neither a file nor directory".to_owned());
    }
    let mut total = 0_u64;
    for entry in std::fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            total = total.saturating_add(directory_file_bytes(&entry.path())?);
        } else if file_type.is_file() {
            total =
                total.saturating_add(entry.metadata().map_err(|error| error.to_string())?.len());
        } else {
            return Err("derived-state directory contains a non-file entry".to_owned());
        }
    }
    Ok(total)
}

fn collect_package_files(
    root: &Path,
    directory: &Path,
    paths: &mut BTreeSet<String>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            collect_package_files(root, &entry.path(), paths)?;
        } else if file_type.is_file() {
            let path = entry.path();
            let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
            paths.insert(relative.to_string_lossy().replace('\\', "/"));
        } else {
            return Err("derived-access package contains a non-file entry".to_owned());
        }
    }
    Ok(())
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

pub(super) fn validate_current_execution_identity_v1(
    expected: &QualificationDerivedAccessExecutionIdentityV1,
    source_checkout: &Path,
    evidence_root: &Path,
) -> Result<(), String> {
    let host_identity_sha256 = qualification_host_identity_sha256()?;
    validate_current_execution_identity_with_host_authority_v1(
        expected,
        source_checkout,
        evidence_root,
        &host_identity_sha256,
    )
}

fn validate_current_execution_identity_with_host_authority_v1(
    expected: &QualificationDerivedAccessExecutionIdentityV1,
    source_checkout: &Path,
    evidence_root: &Path,
    host_identity_sha256: &str,
) -> Result<(), String> {
    expected.validate()?;
    let observed = observe_current_execution_identity_with_host_authority_v1(
        expected.platform,
        expected.root_provenance_sha256.clone(),
        source_checkout,
        evidence_root,
        host_identity_sha256.to_owned(),
    )?;
    if &observed != expected {
        return Err(format!(
            "derived-access execution identity differs in: {}",
            execution_identity_mismatches(expected, &observed).join(", ")
        ));
    }
    Ok(())
}

pub(super) fn execution_identity_mismatches(
    expected: &QualificationDerivedAccessExecutionIdentityV1,
    observed: &QualificationDerivedAccessExecutionIdentityV1,
) -> Vec<&'static str> {
    let QualificationDerivedAccessExecutionIdentityV1 {
        platform: _,
        source_commit: _,
        source_tree: _,
        cargo_lock_sha256: _,
        binary_sha256: _,
        contract_schema: _,
        contract_sha256: _,
        root_provenance_sha256: _,
        command_sha256: _,
        operating_system: _,
        architecture: _,
        filesystem: _,
        host_identity_sha256: _,
        source_dirty: _,
        private_corpus_configured: _,
    } = expected;
    let mut fields = Vec::new();
    macro_rules! compare {
        ($field:ident) => {
            if expected.$field != observed.$field {
                fields.push(stringify!($field));
            }
        };
    }
    compare!(platform);
    compare!(source_commit);
    compare!(source_tree);
    compare!(cargo_lock_sha256);
    compare!(binary_sha256);
    compare!(contract_schema);
    compare!(contract_sha256);
    compare!(root_provenance_sha256);
    compare!(command_sha256);
    compare!(operating_system);
    compare!(architecture);
    compare!(filesystem);
    compare!(host_identity_sha256);
    compare!(source_dirty);
    compare!(private_corpus_configured);
    fields
}

fn observe_current_execution_identity_with_host_authority_v1(
    platform: super::QualificationDerivedAccessPlatformV1,
    root_provenance_sha256: String,
    source_checkout: &Path,
    evidence_root: &Path,
    host_identity_sha256: String,
) -> Result<QualificationDerivedAccessExecutionIdentityV1, String> {
    if !source_checkout.is_dir() {
        return Err("derived-access source checkout is absent".to_owned());
    }
    let source_commit = git_output(source_checkout, &["rev-parse", "HEAD"])?;
    let source_tree = git_output(source_checkout, &["rev-parse", "HEAD^{tree}"])?;
    let source_dirty = !git_output(source_checkout, &["status", "--porcelain=v1"])?.is_empty();
    let cargo_lock_sha256 = sha256_file(&source_checkout.join("Cargo.lock"))?;
    let binary_sha256 = sha256_file(&std::env::current_exe().map_err(|error| error.to_string())?)?;
    let command_sha256 = sha256_bytes_hex(
        &serde_json::to_vec(
            &std::env::args_os()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
        )
        .map_err(|error| error.to_string())?,
    );
    Ok(QualificationDerivedAccessExecutionIdentityV1 {
        platform,
        source_commit,
        source_tree,
        cargo_lock_sha256,
        binary_sha256,
        contract_schema: super::QUALIFICATION_DERIVED_ACCESS_CONTRACT_SCHEMA_V1.to_owned(),
        contract_sha256: super::QUALIFICATION_DERIVED_ACCESS_CONTRACT_SHA256_V1.to_owned(),
        root_provenance_sha256,
        command_sha256,
        operating_system: std::env::consts::OS.to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
        filesystem: crate::bench_support::foundation::qualification_filesystem_name(
            nearest_existing_ancestor(evidence_root)?,
        )
        .to_ascii_lowercase(),
        host_identity_sha256,
        source_dirty,
        private_corpus_configured: std::env::var_os("POINTBREAK_QUALIFICATION_CORPUS").is_some(),
    })
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
pub(super) fn observe_current_execution_identity_for_test_v1(
    platform: super::QualificationDerivedAccessPlatformV1,
    root_provenance_sha256: String,
    source_checkout: &Path,
    evidence_root: &Path,
    host_identity_sha256: String,
) -> Result<QualificationDerivedAccessExecutionIdentityV1, String> {
    observe_current_execution_identity_with_host_authority_v1(
        platform,
        root_provenance_sha256,
        source_checkout,
        evidence_root,
        host_identity_sha256,
    )
}

fn nearest_existing_ancestor(path: &Path) -> Result<&Path, String> {
    let mut candidate = path;
    loop {
        if candidate.exists() {
            return Ok(candidate);
        }
        candidate = candidate
            .parent()
            .ok_or_else(|| "derived-access evidence path has no existing ancestor".to_owned())?;
    }
}

fn git_output(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|error| error.to_string())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
fn test_execution_identity() -> QualificationDerivedAccessExecutionIdentityV1 {
    QualificationDerivedAccessExecutionIdentityV1 {
        platform: super::QualificationDerivedAccessPlatformV1::MacosApfs,
        source_commit: "1".repeat(40),
        source_tree: "2".repeat(40),
        cargo_lock_sha256: "33".repeat(32),
        binary_sha256: "44".repeat(32),
        contract_schema: super::QUALIFICATION_DERIVED_ACCESS_CONTRACT_SCHEMA_V1.to_owned(),
        contract_sha256: super::QUALIFICATION_DERIVED_ACCESS_CONTRACT_SHA256_V1.to_owned(),
        root_provenance_sha256: "55".repeat(32),
        command_sha256: "66".repeat(32),
        operating_system: "macos".to_owned(),
        architecture: "aarch64".to_owned(),
        filesystem: "apfs".to_owned(),
        host_identity_sha256: "77".repeat(32),
        source_dirty: false,
        private_corpus_configured: false,
    }
}

#[cfg(test)]
impl QualificationDerivedAccessPackageV1 {
    pub(crate) fn test_fixture() -> Self {
        Self {
            schema: super::QUALIFICATION_DERIVED_ACCESS_PACKAGE_SCHEMA_V1.to_owned(),
            evaluator_revision: super::QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V2
                .to_owned(),
            evaluator_procedure_sha256: String::new(),
            proposed_profile_id: "sqlite-wal-bodyless-v1".to_owned(),
            execution_identities: Vec::new(),
            product_identities: Vec::new(),
            change_control_binary_identities: Vec::new(),
            root_bindings: Vec::new(),
            d0_rows: Vec::new(),
            operation_rows: Vec::new(),
            lifecycle_rows: Vec::new(),
            resources: None,
            allocation_rows: Vec::new(),
            bootstrap_rows: Vec::new(),
            change_read_rows: Vec::new(),
            change_control_rows: Vec::new(),
            change_storage_rows: Vec::new(),
            timeline_read_rows: Vec::new(),
            timeline_storage_rows: Vec::new(),
            complete: false,
        }
    }
}
