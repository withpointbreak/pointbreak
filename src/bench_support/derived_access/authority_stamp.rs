use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Barrier};

use serde::{Deserialize, Serialize};

use super::{
    DerivedStorageLayout, reject_derived_change_diagnostic_evidence_document_v1,
    reject_derived_change_diagnostic_evidence_path_v1,
};
#[cfg(feature = "longitudinal-counting")]
use crate::bench_support::foundation::qualification_host_identity_sha256;
use crate::canonical_hash::sha256_bytes_hex;
use crate::model::JournalId;
use crate::session::event::{EventTarget, EventType, ReviewInitializedPayload, ShoreEvent, Writer};
use crate::session::{
    EventStore, EventWriteOutcome, Journal, JournalChangeCheck, JournalChangeStamp,
    JournalChangeVerdict, LocalJournal,
};

pub const DERIVED_ACCESS_AUTHORITY_STAMP_MODE_V1: &str = "--derived-access-authority-stamp";
pub const DERIVED_ACCESS_AUTHORITY_STAMP_VERIFY_MODE_V1: &str =
    "--derived-access-authority-stamp-verify";
pub const DERIVED_ACCESS_AUTHORITY_STAMP_CHILD_MODE_V1: &str =
    "--derived-access-authority-stamp-child";
pub const AUTHORITY_STAMP_NATIVE_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.derived-access-authority-stamp-native-receipt.v1";
pub const AUTHORITY_STAMP_NATIVE_PACKAGE_SCHEMA_V1: &str =
    "pointbreak.derived-access-authority-stamp-native-package.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityStampPlatformV1 {
    MacosApfs,
    WindowsNtfs,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityStampScenarioV1 {
    AbsentDirectory,
    EmptyDirectory,
    GovernedCreate,
    GovernedBurst,
    EqualDuplicateNoCreate,
    ConflictingDuplicateNoCreate,
    OutOfBandCreate,
    TempCreateThenRename,
    ConcurrentCreateObservation,
    CrashBeforeCarrierPublication,
    CrashAfterCarrierPublication,
    RapidMutations,
    CloseReopen,
    MachineOrVmRestart,
    CanonicalPathAlias,
    ProductionDirectoryLayout,
    SidecarDeletion,
    ExperimentOffRollback,
    UnrelatedFile,
    TemporaryFile,
    ExistingCarrierOverwrite,
}

impl AuthorityStampScenarioV1 {
    const ALL: [Self; 21] = [
        Self::AbsentDirectory,
        Self::EmptyDirectory,
        Self::GovernedCreate,
        Self::GovernedBurst,
        Self::EqualDuplicateNoCreate,
        Self::ConflictingDuplicateNoCreate,
        Self::OutOfBandCreate,
        Self::TempCreateThenRename,
        Self::ConcurrentCreateObservation,
        Self::CrashBeforeCarrierPublication,
        Self::CrashAfterCarrierPublication,
        Self::RapidMutations,
        Self::CloseReopen,
        Self::MachineOrVmRestart,
        Self::CanonicalPathAlias,
        Self::ProductionDirectoryLayout,
        Self::SidecarDeletion,
        Self::ExperimentOffRollback,
        Self::UnrelatedFile,
        Self::TemporaryFile,
        Self::ExistingCarrierOverwrite,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::AbsentDirectory => "absent-directory",
            Self::EmptyDirectory => "empty-directory",
            Self::GovernedCreate => "governed-create",
            Self::GovernedBurst => "governed-burst",
            Self::EqualDuplicateNoCreate => "equal-duplicate-no-create",
            Self::ConflictingDuplicateNoCreate => "conflicting-duplicate-no-create",
            Self::OutOfBandCreate => "out-of-band-create",
            Self::TempCreateThenRename => "temp-create-then-rename",
            Self::ConcurrentCreateObservation => "concurrent-create-observation",
            Self::CrashBeforeCarrierPublication => "crash-before-carrier-publication",
            Self::CrashAfterCarrierPublication => "crash-after-carrier-publication",
            Self::RapidMutations => "rapid-mutations",
            Self::CloseReopen => "close-reopen",
            Self::MachineOrVmRestart => "machine-or-vm-restart",
            Self::CanonicalPathAlias => "canonical-path-alias",
            Self::ProductionDirectoryLayout => "production-directory-layout",
            Self::SidecarDeletion => "sidecar-deletion",
            Self::ExperimentOffRollback => "experiment-off-rollback",
            Self::UnrelatedFile => "unrelated-file",
            Self::TemporaryFile => "temporary-file",
            Self::ExistingCarrierOverwrite => "existing-carrier-overwrite",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityStampExpectationV1 {
    Stable,
    ChangedOrIndeterminate,
    StableOrChangedWithoutTruthClaim,
    ObservationNotApplicable,
    ExplicitNonClaim,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityStampObservationV1 {
    Stable,
    Changed,
    Indeterminate,
    NotApplicable,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorityStampScenarioReceiptV1 {
    pub scenario: AuthorityStampScenarioV1,
    pub expectation: AuthorityStampExpectationV1,
    pub observation: AuthorityStampObservationV1,
    pub stamp_before_sha256: Option<String>,
    pub stamp_after_sha256: Option<String>,
    pub event_directory_entries_walked: u64,
    pub event_carrier_opens: u64,
    pub authoritative_carrier_created: bool,
    pub truth_change_proven: bool,
    pub selected_carrier_validation_detected_corruption: Option<bool>,
    pub mechanism: String,
    pub accepted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorityStampExecutionIdentityV1 {
    pub platform: AuthorityStampPlatformV1,
    pub source_commit: String,
    pub source_tree: String,
    pub cargo_lock_sha256: String,
    pub binary_sha256: String,
    pub operating_system: String,
    pub architecture: String,
    pub filesystem: String,
    /// SHA-256 of the explicit logical campaign-host label. This is not a
    /// hardware identifier or a hash of an ambient network hostname.
    pub host_identity_sha256: String,
    pub command_sha256: String,
    pub probe_root_identity_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorityStampNativeReceiptV1 {
    pub schema: String,
    pub execution: AuthorityStampExecutionIdentityV1,
    pub scope: String,
    pub malicious_tamper_detection_claimed: bool,
    pub scenarios: Vec<AuthorityStampScenarioReceiptV1>,
    pub all_scenarios_accepted: bool,
    pub completion_published_last: bool,
    pub receipt_sha256: String,
}

impl AuthorityStampNativeReceiptV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUTHORITY_STAMP_NATIVE_RECEIPT_SCHEMA_V1
            || self.scope
                != "supported local-filesystem accidental and mixed-version event publication detection"
            || self.malicious_tamper_detection_claimed
            || !self.completion_published_last
            || self.scenarios.len() != AuthorityStampScenarioV1::ALL.len()
        {
            return Err("authority-stamp native receipt is incomplete".to_owned());
        }
        validate_execution(&self.execution)?;
        let observed = self
            .scenarios
            .iter()
            .map(|row| row.scenario)
            .collect::<BTreeSet<_>>();
        let expected = AuthorityStampScenarioV1::ALL
            .into_iter()
            .collect::<BTreeSet<_>>();
        if observed != expected {
            return Err("authority-stamp scenario matrix failed".to_owned());
        }
        for row in &self.scenarios {
            let expected_acceptance = expectation_accepts(row.expectation, row.observation)
                && row.event_directory_entries_walked == 0
                && row.event_carrier_opens == 0
                && !row.truth_change_proven
                && match row.scenario {
                    AuthorityStampScenarioV1::ExistingCarrierOverwrite => {
                        row.selected_carrier_validation_detected_corruption == Some(true)
                    }
                    _ => row
                        .selected_carrier_validation_detected_corruption
                        .is_none(),
                };
            let stamp_shape_is_valid = match row.scenario {
                AuthorityStampScenarioV1::SidecarDeletion
                | AuthorityStampScenarioV1::ExperimentOffRollback => {
                    row.stamp_before_sha256.is_none() && row.stamp_after_sha256.is_none()
                }
                _ => [&row.stamp_before_sha256, &row.stamp_after_sha256]
                    .into_iter()
                    .all(|value| {
                        value
                            .as_deref()
                            .is_some_and(|value| is_lower_hex(value, 64))
                    }),
            };
            if row.expectation != scenario_expectation(row.scenario)
                || row.mechanism.is_empty()
                || !stamp_shape_is_valid
                || row.accepted != expected_acceptance
            {
                return Err(format!(
                    "authority-stamp scenario {:?} is internally inconsistent",
                    row.scenario
                ));
            }
        }
        if self.all_scenarios_accepted != self.scenarios.iter().all(|row| row.accepted) {
            return Err("authority-stamp receipt summary differs from its scenarios".to_owned());
        }
        let actual = self.canonical_sha256()?;
        if !is_lower_hex(&self.receipt_sha256, 64) || self.receipt_sha256 != actual {
            return Err("authority-stamp receipt hash differs".to_owned());
        }
        Ok(())
    }

    fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.receipt_sha256.clear();
        serde_json::to_vec(&preimage)
            .map(|bytes| sha256_bytes_hex(&bytes))
            .map_err(|error| error.to_string())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorityStampNativePackageV1 {
    pub schema: String,
    pub source_commit: String,
    pub source_tree: String,
    pub cargo_lock_sha256: String,
    pub apfs_receipt_sha256: String,
    pub ntfs_receipt_sha256: String,
    pub scenario_count_per_platform: usize,
    pub all_scenarios_accepted: bool,
    pub package_sha256: String,
}

impl AuthorityStampNativePackageV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUTHORITY_STAMP_NATIVE_PACKAGE_SCHEMA_V1
            || !is_lower_hex(&self.source_commit, 40)
            || !is_lower_hex(&self.source_tree, 40)
            || !is_lower_hex(&self.cargo_lock_sha256, 64)
            || !is_lower_hex(&self.apfs_receipt_sha256, 64)
            || !is_lower_hex(&self.ntfs_receipt_sha256, 64)
            || self.scenario_count_per_platform != AuthorityStampScenarioV1::ALL.len()
            || !self.all_scenarios_accepted
        {
            return Err("authority-stamp native package is invalid".to_owned());
        }
        let mut preimage = self.clone();
        preimage.package_sha256.clear();
        let actual =
            sha256_bytes_hex(&serde_json::to_vec(&preimage).map_err(|error| error.to_string())?);
        if !is_lower_hex(&self.package_sha256, 64) || actual != self.package_sha256 {
            return Err("authority-stamp native package hash differs".to_owned());
        }
        Ok(())
    }
}

/// Run all native scenarios on one disposable root and publish the structurally
/// validated result by atomic rename, including a qualifying falsifier.
pub fn run_authority_stamp_native_probe_v1(
    source_checkout: &Path,
    probe_root: &Path,
    output_path: &Path,
) -> Result<AuthorityStampNativeReceiptV1, String> {
    reject_derived_change_diagnostic_evidence_path_v1(output_path)?;
    #[cfg(not(feature = "longitudinal-counting"))]
    {
        let _ = (source_checkout, probe_root, output_path);
        return Err(
            "authority-stamp evidence requires --features longitudinal-counting".to_owned(),
        );
    }
    #[cfg(feature = "longitudinal-counting")]
    {
        prepare_empty_root(probe_root)?;
        let execution = observe_execution(source_checkout, probe_root)?;
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let scenarios = run_scenarios(probe_root, Some(&executable))?;
        let mut receipt = AuthorityStampNativeReceiptV1 {
            schema: AUTHORITY_STAMP_NATIVE_RECEIPT_SCHEMA_V1.to_owned(),
            execution,
            scope: "supported local-filesystem accidental and mixed-version event publication detection"
                .to_owned(),
            malicious_tamper_detection_claimed: false,
            all_scenarios_accepted: scenarios.iter().all(|row| row.accepted),
            scenarios,
            completion_published_last: true,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt.canonical_sha256()?;
        receipt.validate()?;
        write_completion_last(output_path, &receipt)?;
        Ok(receipt)
    }
}

pub fn verify_authority_stamp_native_receipts_v1(
    inputs: &[PathBuf],
) -> Result<AuthorityStampNativePackageV1, String> {
    if inputs.len() != 2 {
        return Err("authority-stamp verification requires exactly two native receipts".to_owned());
    }
    let mut receipts = Vec::new();
    for path in inputs {
        reject_derived_change_diagnostic_evidence_path_v1(path)?;
        let document: serde_json::Value = serde_json::from_slice(
            &fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?,
        )
        .map_err(|error| format!("{}: {error}", path.display()))?;
        reject_derived_change_diagnostic_evidence_document_v1(&document)?;
        let receipt: AuthorityStampNativeReceiptV1 = serde_json::from_value(document)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        receipt.validate()?;
        receipts.push(receipt);
    }
    let apfs = receipts
        .iter()
        .find(|receipt| receipt.execution.platform == AuthorityStampPlatformV1::MacosApfs)
        .ok_or_else(|| "authority-stamp package lacks APFS evidence".to_owned())?;
    let ntfs = receipts
        .iter()
        .find(|receipt| receipt.execution.platform == AuthorityStampPlatformV1::WindowsNtfs)
        .ok_or_else(|| "authority-stamp package lacks NTFS evidence".to_owned())?;
    validate_native_pair(apfs, ntfs)?;
    let mut package = AuthorityStampNativePackageV1 {
        schema: AUTHORITY_STAMP_NATIVE_PACKAGE_SCHEMA_V1.to_owned(),
        source_commit: apfs.execution.source_commit.clone(),
        source_tree: apfs.execution.source_tree.clone(),
        cargo_lock_sha256: apfs.execution.cargo_lock_sha256.clone(),
        apfs_receipt_sha256: apfs.receipt_sha256.clone(),
        ntfs_receipt_sha256: ntfs.receipt_sha256.clone(),
        scenario_count_per_platform: AuthorityStampScenarioV1::ALL.len(),
        all_scenarios_accepted: true,
        package_sha256: String::new(),
    };
    let mut preimage = package.clone();
    preimage.package_sha256.clear();
    package.package_sha256 =
        sha256_bytes_hex(&serde_json::to_vec(&preimage).map_err(|error| error.to_string())?);
    package.validate()?;
    Ok(package)
}

fn validate_native_pair(
    apfs: &AuthorityStampNativeReceiptV1,
    ntfs: &AuthorityStampNativeReceiptV1,
) -> Result<(), String> {
    if apfs.execution.source_commit != ntfs.execution.source_commit
        || apfs.execution.source_tree != ntfs.execution.source_tree
        || apfs.execution.cargo_lock_sha256 != ntfs.execution.cargo_lock_sha256
    {
        return Err("authority-stamp native receipts do not share source authority".to_owned());
    }
    if apfs.execution.host_identity_sha256 == ntfs.execution.host_identity_sha256 {
        return Err("authority-stamp native receipts reuse one campaign-host authority".to_owned());
    }
    if !apfs.all_scenarios_accepted || !ntfs.all_scenarios_accepted {
        return Err("authority-stamp native evidence contains a qualifying falsifier".to_owned());
    }
    Ok(())
}

/// Internal subprocess endpoint used to exercise crash and fresh-process
/// boundaries. The caller owns the exit status.
pub fn run_authority_stamp_child_v1(action: &str, store_dir: &Path) -> Result<String, String> {
    if let Some(token) = action.strip_prefix("check,") {
        let before = JournalChangeStamp::from_continuation_token(token)
            .ok_or_else(|| "invalid authority-stamp continuation token".to_owned())?;
        let check = observe_since(&LocalJournal::new(store_dir), &before)?;
        let verdict = match check.verdict {
            JournalChangeVerdict::Stable => "stable",
            JournalChangeVerdict::Changed => "changed",
            JournalChangeVerdict::Indeterminate => "indeterminate",
        };
        return Ok(format!(
            "{verdict}\n{}\n{}",
            check.after.opaque_sha256(),
            check.mechanism
        ));
    }
    match action {
        "capture" => {
            capture_stamp(&LocalJournal::new(store_dir)).map(|stamp| stamp.opaque_sha256())
        }
        "capture-token" => capture_stamp(&LocalJournal::new(store_dir))?
            .continuation_token()
            .ok_or_else(|| "native authority stamp has no continuation token".to_owned()),
        "crash-before" => Err("intentional crash before carrier publication".to_owned()),
        "crash-after" => {
            write_direct_carrier(store_dir, "crash-after", b"published-before-crash")?;
            Err("intentional crash after carrier publication".to_owned())
        }
        _ => Err("unsupported authority-stamp child action".to_owned()),
    }
}

fn run_scenarios(
    root: &Path,
    executable: Option<&Path>,
) -> Result<Vec<AuthorityStampScenarioReceiptV1>, String> {
    AuthorityStampScenarioV1::ALL
        .into_iter()
        .map(|scenario| run_scenario(root, scenario, executable))
        .collect()
}

fn run_scenario(
    root: &Path,
    scenario: AuthorityStampScenarioV1,
    executable: Option<&Path>,
) -> Result<AuthorityStampScenarioReceiptV1, String> {
    let scenario_root = root.join(scenario.label());
    let store_dir = if scenario == AuthorityStampScenarioV1::ProductionDirectoryLayout {
        scenario_root.join("repo/.git/pointbreak")
    } else {
        scenario_root.join("store")
    };
    let journal = LocalJournal::new(&store_dir);
    let mut before = None;
    let mut after = None;
    let mut created = false;
    let mut corruption_detected = None;
    let mut mechanism = "native directory identity plus change metadata".to_owned();
    let (expectation, observation) = match scenario {
        AuthorityStampScenarioV1::AbsentDirectory => {
            let first = capture_stamp(&journal)?;
            let check = observe_since(&journal, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            mechanism = check.mechanism.clone();
            (
                AuthorityStampExpectationV1::Stable,
                receipt_observation(&check),
            )
        }
        AuthorityStampScenarioV1::EmptyDirectory => {
            fs::create_dir_all(store_dir.join("events")).map_err(|error| error.to_string())?;
            let first = capture_stamp(&journal)?;
            let check = observe_since(&journal, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            mechanism = check.mechanism.clone();
            (
                AuthorityStampExpectationV1::Stable,
                receipt_observation(&check),
            )
        }
        AuthorityStampScenarioV1::GovernedCreate => {
            fs::create_dir_all(store_dir.join("events")).map_err(|error| error.to_string())?;
            let first = capture_stamp(&journal)?;
            created = journal
                .create_event_once("governed:create", b"created")
                .map_err(|error| error.to_string())?
                == crate::storage::CreateOutcome::Created;
            let check = observe_since(&journal, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            mechanism = check.mechanism.clone();
            (
                AuthorityStampExpectationV1::ChangedOrIndeterminate,
                receipt_observation(&check),
            )
        }
        AuthorityStampScenarioV1::GovernedBurst => {
            fs::create_dir_all(store_dir.join("events")).map_err(|error| error.to_string())?;
            let first = capture_stamp(&journal)?;
            for index in 0..32 {
                journal
                    .create_event_once(&format!("governed:burst:{index}"), b"created")
                    .map_err(|error| error.to_string())?;
            }
            created = true;
            let check = observe_since(&journal, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            mechanism = check.mechanism.clone();
            (
                AuthorityStampExpectationV1::ChangedOrIndeterminate,
                receipt_observation(&check),
            )
        }
        AuthorityStampScenarioV1::EqualDuplicateNoCreate
        | AuthorityStampScenarioV1::ConflictingDuplicateNoCreate => {
            journal
                .create_event_once("duplicate:key", b"first")
                .map_err(|error| error.to_string())?;
            let first = capture_stamp(&journal)?;
            journal
                .create_event_once(
                    "duplicate:key",
                    if scenario == AuthorityStampScenarioV1::EqualDuplicateNoCreate {
                        b"first"
                    } else {
                        b"different"
                    },
                )
                .map_err(|error| error.to_string())?;
            let check = observe_since(&journal, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            mechanism = check.mechanism.clone();
            (
                AuthorityStampExpectationV1::Stable,
                receipt_observation(&check),
            )
        }
        AuthorityStampScenarioV1::OutOfBandCreate => {
            fs::create_dir_all(store_dir.join("events")).map_err(|error| error.to_string())?;
            let first = capture_stamp(&journal)?;
            write_direct_carrier(&store_dir, "out-of-band", b"direct")?;
            created = true;
            let check = observe_since(&journal, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            mechanism = check.mechanism.clone();
            (
                AuthorityStampExpectationV1::ChangedOrIndeterminate,
                receipt_observation(&check),
            )
        }
        AuthorityStampScenarioV1::TempCreateThenRename => {
            let events = store_dir.join("events");
            fs::create_dir_all(&events).map_err(|error| error.to_string())?;
            let first = capture_stamp(&journal)?;
            let digest = sha256_bytes_hex(b"temp-rename");
            let temporary = events.join(format!(".{digest}.tmp"));
            let final_path = events.join(format!("{digest}.json"));
            fs::write(&temporary, b"temporary").map_err(|error| error.to_string())?;
            fs::rename(&temporary, &final_path).map_err(|error| error.to_string())?;
            created = true;
            let check = observe_since(&journal, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            mechanism = check.mechanism.clone();
            (
                AuthorityStampExpectationV1::ChangedOrIndeterminate,
                receipt_observation(&check),
            )
        }
        AuthorityStampScenarioV1::ConcurrentCreateObservation => {
            fs::create_dir_all(store_dir.join("events")).map_err(|error| error.to_string())?;
            let first = capture_stamp(&journal)?;
            let barrier = Arc::new(Barrier::new(2));
            let child_barrier = Arc::clone(&barrier);
            let child_store = store_dir.clone();
            let writer = std::thread::spawn(move || {
                child_barrier.wait();
                write_direct_carrier(&child_store, "concurrent", b"created")
            });
            barrier.wait();
            writer
                .join()
                .map_err(|_| "concurrent authority writer panicked".to_owned())??;
            created = true;
            let check = observe_since(&journal, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            mechanism = check.mechanism.clone();
            (
                AuthorityStampExpectationV1::ChangedOrIndeterminate,
                receipt_observation(&check),
            )
        }
        AuthorityStampScenarioV1::CrashBeforeCarrierPublication
        | AuthorityStampScenarioV1::CrashAfterCarrierPublication => {
            fs::create_dir_all(store_dir.join("events")).map_err(|error| error.to_string())?;
            let first = capture_stamp(&journal)?;
            let action = if scenario == AuthorityStampScenarioV1::CrashBeforeCarrierPublication {
                "crash-before"
            } else {
                "crash-after"
            };
            if let Some(executable) = executable {
                let output = Command::new(executable)
                    .arg(DERIVED_ACCESS_AUTHORITY_STAMP_CHILD_MODE_V1)
                    .arg(format!("--derived-access-authority-action={action}"))
                    .arg(format!("--derived-access-root={}", store_dir.display()))
                    .output()
                    .map_err(|error| error.to_string())?;
                if output.status.success() {
                    return Err(format!("{action} child unexpectedly succeeded"));
                }
            } else if action == "crash-after" {
                write_direct_carrier(&store_dir, "crash-after", b"published-before-crash")?;
            }
            created = action == "crash-after";
            let check = observe_since(&journal, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            mechanism = check.mechanism.clone();
            let expectation = if created {
                AuthorityStampExpectationV1::ChangedOrIndeterminate
            } else {
                AuthorityStampExpectationV1::Stable
            };
            (expectation, receipt_observation(&check))
        }
        AuthorityStampScenarioV1::RapidMutations => {
            fs::create_dir_all(store_dir.join("events")).map_err(|error| error.to_string())?;
            let first = capture_stamp(&journal)?;
            let mut prior = first.clone();
            let mut preserved_at = None;
            let mut last_observation = AuthorityStampObservationV1::Changed;
            for index in 0..64 {
                write_direct_carrier(
                    &store_dir,
                    &format!("rapid-{index}"),
                    format!("rapid-{index}").as_bytes(),
                )?;
                let check = observe_since(&journal, &prior)?;
                last_observation = receipt_observation(&check);
                if last_observation == AuthorityStampObservationV1::Stable {
                    preserved_at = Some(index);
                    before = Some(prior.opaque_sha256());
                    after = Some(check.after.opaque_sha256());
                    mechanism = check.mechanism;
                    break;
                }
                mechanism = check.mechanism;
                prior = check.after;
            }
            created = true;
            let observation = if let Some(index) = preserved_at {
                mechanism = format!(
                    "completed carrier creation {index} was classified stable: {mechanism}"
                );
                AuthorityStampObservationV1::Stable
            } else {
                before = Some(first.opaque_sha256());
                after = Some(prior.opaque_sha256());
                last_observation
            };
            (
                AuthorityStampExpectationV1::ChangedOrIndeterminate,
                observation,
            )
        }
        AuthorityStampScenarioV1::CloseReopen => {
            write_direct_carrier(&store_dir, "close-reopen", b"created")?;
            let first = capture_stamp(&journal)?;
            drop(journal);
            let reopened = LocalJournal::new(&store_dir);
            let check = observe_since(&reopened, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            mechanism = format!("close and reopen: {}", check.mechanism);
            (
                AuthorityStampExpectationV1::Stable,
                receipt_observation(&check),
            )
        }
        AuthorityStampScenarioV1::MachineOrVmRestart => {
            write_direct_carrier(&store_dir, "fresh-process", b"created")?;
            let first = capture_stamp(&journal)?;
            let first_digest = first.opaque_sha256();
            let (observation, second_digest, child_mechanism) =
                if let (Some(executable), Some(token)) = (executable, first.continuation_token()) {
                    let output = Command::new(executable)
                        .arg(DERIVED_ACCESS_AUTHORITY_STAMP_CHILD_MODE_V1)
                        .arg(format!("--derived-access-authority-action=check,{token}"))
                        .arg(format!("--derived-access-root={}", store_dir.display()))
                        .output()
                        .map_err(|error| error.to_string())?;
                    if !output.status.success() {
                        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
                    }
                    let stdout = String::from_utf8(output.stdout)
                        .map_err(|error| error.to_string())?
                        .trim()
                        .to_owned();
                    let mut lines = stdout.lines();
                    let observation = match lines.next() {
                        Some("stable") => AuthorityStampObservationV1::Stable,
                        Some("changed") => AuthorityStampObservationV1::Changed,
                        Some("indeterminate") => AuthorityStampObservationV1::Indeterminate,
                        _ => {
                            return Err(
                                "fresh-process authority check omitted its verdict".to_owned()
                            );
                        }
                    };
                    let digest = lines
                        .next()
                        .filter(|value| is_lower_hex(value, 64))
                        .ok_or_else(|| {
                            "fresh-process authority check omitted its stamp digest".to_owned()
                        })?
                        .to_owned();
                    let mechanism = lines.collect::<Vec<_>>().join("\n");
                    (observation, digest, mechanism)
                } else if let Some(executable) = executable {
                    let output = Command::new(executable)
                        .arg(DERIVED_ACCESS_AUTHORITY_STAMP_CHILD_MODE_V1)
                        .arg("--derived-access-authority-action=capture")
                        .arg(format!("--derived-access-root={}", store_dir.display()))
                        .output()
                        .map_err(|error| error.to_string())?;
                    if !output.status.success() {
                        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
                    }
                    let digest = String::from_utf8(output.stdout)
                        .map_err(|error| error.to_string())?
                        .trim()
                        .to_owned();
                    let observation = if digest == first_digest {
                        AuthorityStampObservationV1::Stable
                    } else {
                        AuthorityStampObservationV1::Changed
                    };
                    (
                        observation,
                        digest,
                        "fresh-process comparable stamp capture".to_owned(),
                    )
                } else {
                    let check = observe_since(&LocalJournal::new(&store_dir), &first)?;
                    (
                        receipt_observation(&check),
                        check.after.opaque_sha256(),
                        check.mechanism,
                    )
                };
            before = Some(first_digest.clone());
            after = Some(second_digest.clone());
            mechanism = format!(
                "fresh-process reopen: {child_mechanism}; machine restart remains a practical follow-up"
            );
            (AuthorityStampExpectationV1::Stable, observation)
        }
        AuthorityStampScenarioV1::CanonicalPathAlias => {
            fs::create_dir_all(store_dir.join("events")).map_err(|error| error.to_string())?;
            let alias_store = store_dir.join("events/..");
            let first = capture_stamp(&LocalJournal::new(&alias_store))?;
            write_direct_carrier(&store_dir, "canonical-alias", b"created")?;
            created = true;
            let check = observe_since(&journal, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            mechanism = check.mechanism.clone();
            (
                AuthorityStampExpectationV1::ChangedOrIndeterminate,
                receipt_observation(&check),
            )
        }
        AuthorityStampScenarioV1::ProductionDirectoryLayout => {
            fs::create_dir_all(store_dir.join("events")).map_err(|error| error.to_string())?;
            let first = capture_stamp(&journal)?;
            write_direct_carrier(&store_dir, "production-layout", b"created")?;
            created = true;
            let check = observe_since(&journal, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            mechanism = check.mechanism.clone();
            (
                AuthorityStampExpectationV1::ChangedOrIndeterminate,
                receipt_observation(&check),
            )
        }
        AuthorityStampScenarioV1::SidecarDeletion => {
            write_direct_carrier(&store_dir, "sidecar", b"created")?;
            let sidecar = DerivedStorageLayout::resolve(&store_dir)
                .map_err(|error| error.to_string())?
                .root()
                .join("authority.json");
            fs::create_dir_all(sidecar.parent().expect("sidecar parent"))
                .map_err(|error| error.to_string())?;
            fs::write(&sidecar, b"authority").map_err(|error| error.to_string())?;
            fs::remove_file(&sidecar).map_err(|error| error.to_string())?;
            mechanism =
                "the directory-only candidate makes no sidecar-deletion observation".to_owned();
            (
                AuthorityStampExpectationV1::ObservationNotApplicable,
                AuthorityStampObservationV1::NotApplicable,
            )
        }
        AuthorityStampScenarioV1::ExperimentOffRollback => {
            mechanism = "profile-off mode does not observe derived authority metadata".to_owned();
            (
                AuthorityStampExpectationV1::ObservationNotApplicable,
                AuthorityStampObservationV1::NotApplicable,
            )
        }
        AuthorityStampScenarioV1::UnrelatedFile | AuthorityStampScenarioV1::TemporaryFile => {
            let events = store_dir.join("events");
            fs::create_dir_all(&events).map_err(|error| error.to_string())?;
            let first = capture_stamp(&journal)?;
            let name = if scenario == AuthorityStampScenarioV1::UnrelatedFile {
                "README.txt"
            } else {
                ".pointbreak-probe.tmp"
            };
            fs::write(events.join(name), b"not an event carrier")
                .map_err(|error| error.to_string())?;
            let check = observe_since(&journal, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            mechanism = check.mechanism.clone();
            (
                AuthorityStampExpectationV1::StableOrChangedWithoutTruthClaim,
                receipt_observation(&check),
            )
        }
        AuthorityStampScenarioV1::ExistingCarrierOverwrite => {
            let event = probe_event();
            let store = EventStore::open(&store_dir);
            if store
                .record_event_once(&event)
                .map_err(|error| error.to_string())?
                != EventWriteOutcome::Created
            {
                return Err("probe event was not created".to_owned());
            }
            let first = capture_stamp(&journal)?;
            fs::write(
                store.event_path_for_idempotency_key(&event.idempotency_key),
                b"corrupt existing carrier",
            )
            .map_err(|error| error.to_string())?;
            let check = observe_since(&journal, &first)?;
            before = Some(first.opaque_sha256());
            after = Some(check.after.opaque_sha256());
            corruption_detected = Some(EventStore::open(&store_dir).list_events().is_err());
            mechanism = format!(
                "{}; stamp outcome is a non-claim and selected-carrier validation detects corruption",
                check.mechanism
            );
            (
                AuthorityStampExpectationV1::ExplicitNonClaim,
                receipt_observation(&check),
            )
        }
    };
    let accepted = expectation_accepts(expectation, observation)
        && corruption_detected.unwrap_or(true)
        && (!matches!(
            expectation,
            AuthorityStampExpectationV1::ChangedOrIndeterminate
        ) || observation != AuthorityStampObservationV1::Stable);
    Ok(AuthorityStampScenarioReceiptV1 {
        scenario,
        expectation,
        observation,
        stamp_before_sha256: before,
        stamp_after_sha256: after,
        event_directory_entries_walked: 0,
        event_carrier_opens: 0,
        authoritative_carrier_created: created,
        truth_change_proven: false,
        selected_carrier_validation_detected_corruption: corruption_detected,
        mechanism,
        accepted,
    })
}

fn capture_stamp(journal: &LocalJournal) -> Result<JournalChangeStamp, String> {
    #[cfg(any(test, feature = "longitudinal-counting"))]
    {
        use crate::bench_support::longitudinal::LongitudinalCountingScopeV1;
        let scope = LongitudinalCountingScopeV1::new("00".repeat(32))?;
        let guard = scope.enter();
        let stamp = journal.change_stamp().map_err(|error| error.to_string())?;
        drop(guard);
        let counters = scope.snapshot().counters;
        if counters.directory_entries_walked != 0 || counters.carrier_opens != 0 {
            return Err("authority-stamp capture performed event-directory work".to_owned());
        }
        Ok(stamp)
    }
    #[cfg(not(any(test, feature = "longitudinal-counting")))]
    {
        let _ = journal;
        Err("authority-stamp capture requires counting instrumentation".to_owned())
    }
}

fn observe_since(
    journal: &LocalJournal,
    before: &JournalChangeStamp,
) -> Result<JournalChangeCheck, String> {
    #[cfg(any(test, feature = "longitudinal-counting"))]
    {
        use crate::bench_support::longitudinal::LongitudinalCountingScopeV1;
        let scope = LongitudinalCountingScopeV1::new("00".repeat(32))?;
        let guard = scope.enter();
        let check = journal
            .changes_since(before)
            .map_err(|error| error.to_string())?;
        drop(guard);
        let counters = scope.snapshot().counters;
        if counters.directory_entries_walked != 0 || counters.carrier_opens != 0 {
            return Err("authority change check performed event-directory work".to_owned());
        }
        Ok(check)
    }
    #[cfg(not(any(test, feature = "longitudinal-counting")))]
    {
        let _ = (journal, before);
        Err("authority change check requires counting instrumentation".to_owned())
    }
}

fn receipt_observation(check: &JournalChangeCheck) -> AuthorityStampObservationV1 {
    match check.verdict {
        JournalChangeVerdict::Stable => AuthorityStampObservationV1::Stable,
        JournalChangeVerdict::Changed => AuthorityStampObservationV1::Changed,
        JournalChangeVerdict::Indeterminate => AuthorityStampObservationV1::Indeterminate,
    }
}

fn expectation_accepts(
    expectation: AuthorityStampExpectationV1,
    observation: AuthorityStampObservationV1,
) -> bool {
    match expectation {
        AuthorityStampExpectationV1::Stable => observation == AuthorityStampObservationV1::Stable,
        AuthorityStampExpectationV1::ChangedOrIndeterminate => matches!(
            observation,
            AuthorityStampObservationV1::Changed | AuthorityStampObservationV1::Indeterminate
        ),
        AuthorityStampExpectationV1::StableOrChangedWithoutTruthClaim
        | AuthorityStampExpectationV1::ExplicitNonClaim => matches!(
            observation,
            AuthorityStampObservationV1::Stable | AuthorityStampObservationV1::Changed
        ),
        AuthorityStampExpectationV1::ObservationNotApplicable => {
            observation == AuthorityStampObservationV1::NotApplicable
        }
    }
}

fn scenario_expectation(scenario: AuthorityStampScenarioV1) -> AuthorityStampExpectationV1 {
    match scenario {
        AuthorityStampScenarioV1::AbsentDirectory
        | AuthorityStampScenarioV1::EmptyDirectory
        | AuthorityStampScenarioV1::EqualDuplicateNoCreate
        | AuthorityStampScenarioV1::ConflictingDuplicateNoCreate
        | AuthorityStampScenarioV1::CrashBeforeCarrierPublication
        | AuthorityStampScenarioV1::CloseReopen
        | AuthorityStampScenarioV1::MachineOrVmRestart => AuthorityStampExpectationV1::Stable,
        AuthorityStampScenarioV1::GovernedCreate
        | AuthorityStampScenarioV1::GovernedBurst
        | AuthorityStampScenarioV1::OutOfBandCreate
        | AuthorityStampScenarioV1::TempCreateThenRename
        | AuthorityStampScenarioV1::ConcurrentCreateObservation
        | AuthorityStampScenarioV1::CrashAfterCarrierPublication
        | AuthorityStampScenarioV1::RapidMutations
        | AuthorityStampScenarioV1::CanonicalPathAlias
        | AuthorityStampScenarioV1::ProductionDirectoryLayout => {
            AuthorityStampExpectationV1::ChangedOrIndeterminate
        }
        AuthorityStampScenarioV1::SidecarDeletion
        | AuthorityStampScenarioV1::ExperimentOffRollback => {
            AuthorityStampExpectationV1::ObservationNotApplicable
        }
        AuthorityStampScenarioV1::UnrelatedFile | AuthorityStampScenarioV1::TemporaryFile => {
            AuthorityStampExpectationV1::StableOrChangedWithoutTruthClaim
        }
        AuthorityStampScenarioV1::ExistingCarrierOverwrite => {
            AuthorityStampExpectationV1::ExplicitNonClaim
        }
    }
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn write_direct_carrier(store_dir: &Path, label: &str, bytes: &[u8]) -> Result<(), String> {
    let events = store_dir.join("events");
    fs::create_dir_all(&events).map_err(|error| error.to_string())?;
    fs::write(
        events.join(format!("{}.json", sha256_bytes_hex(label.as_bytes()))),
        bytes,
    )
    .map_err(|error| error.to_string())
}

fn probe_event() -> ShoreEvent {
    ShoreEvent::new(
        EventType::ReviewInitialized,
        "review_initialized:journal:authority-probe:work:default",
        EventTarget::for_journal(JournalId::new("journal:authority-probe")),
        Writer::shore_local("0.8.0"),
        ReviewInitializedPayload {},
        "2026-07-31T00:00:00Z",
    )
    .expect("authority probe event is valid")
}

#[cfg(feature = "longitudinal-counting")]
fn prepare_empty_root(root: &Path) -> Result<(), String> {
    if root.exists() {
        let mut entries = fs::read_dir(root).map_err(|error| error.to_string())?;
        if entries
            .next()
            .transpose()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("authority-stamp probe root must be absent or empty".to_owned());
        }
    } else {
        fs::create_dir_all(root).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(feature = "longitudinal-counting")]
fn observe_execution(
    source_checkout: &Path,
    probe_root: &Path,
) -> Result<AuthorityStampExecutionIdentityV1, String> {
    let source_commit = git_output(source_checkout, &["rev-parse", "HEAD"])?;
    let source_tree = git_output(source_checkout, &["rev-parse", "HEAD^{tree}"])?;
    if !git_output(source_checkout, &["status", "--porcelain=v1"])?.is_empty() {
        return Err("authority-stamp evidence requires a clean source checkout".to_owned());
    }
    let filesystem = crate::bench_support::foundation::qualification_filesystem_name(probe_root)
        .to_ascii_lowercase();
    let platform = match (std::env::consts::OS, filesystem.as_str()) {
        ("macos", "apfs") => AuthorityStampPlatformV1::MacosApfs,
        ("windows", "ntfs") => AuthorityStampPlatformV1::WindowsNtfs,
        _ => {
            return Err(format!(
                "unsupported native authority platform: {filesystem}"
            ));
        }
    };
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let command = std::env::args_os()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let canonical_root = fs::canonicalize(probe_root).map_err(|error| error.to_string())?;
    Ok(AuthorityStampExecutionIdentityV1 {
        platform,
        source_commit,
        source_tree,
        cargo_lock_sha256: sha256_file(&source_checkout.join("Cargo.lock"))?,
        binary_sha256: sha256_file(&executable)?,
        operating_system: std::env::consts::OS.to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
        filesystem,
        host_identity_sha256: qualification_host_identity_sha256()?,
        command_sha256: sha256_bytes_hex(
            &serde_json::to_vec(&command).map_err(|error| error.to_string())?,
        ),
        probe_root_identity_sha256: sha256_bytes_hex(canonical_root.to_string_lossy().as_bytes()),
    })
}

fn validate_execution(execution: &AuthorityStampExecutionIdentityV1) -> Result<(), String> {
    let expected = match execution.platform {
        AuthorityStampPlatformV1::MacosApfs => ("macos", "apfs"),
        AuthorityStampPlatformV1::WindowsNtfs => ("windows", "ntfs"),
    };
    if execution.operating_system != expected.0
        || execution.filesystem != expected.1
        || !is_lower_hex(&execution.source_commit, 40)
        || !is_lower_hex(&execution.source_tree, 40)
        || execution.architecture.is_empty()
        || [
            &execution.cargo_lock_sha256,
            &execution.binary_sha256,
            &execution.host_identity_sha256,
            &execution.command_sha256,
            &execution.probe_root_identity_sha256,
        ]
        .into_iter()
        .any(|value| !is_lower_hex(value, 64))
    {
        return Err("authority-stamp execution identity is invalid".to_owned());
    }
    Ok(())
}

#[cfg(feature = "longitudinal-counting")]
fn write_completion_last<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "authority-stamp output has no parent".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let partial = path.with_extension("json.partial");
    fs::write(
        &partial,
        serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::rename(&partial, path).map_err(|error| error.to_string())
}

#[cfg(feature = "longitudinal-counting")]
fn git_output(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8(output.stdout)
        .map_err(|error| error.to_string())?
        .trim()
        .to_owned())
}

#[cfg(feature = "longitudinal-counting")]
fn sha256_file(path: &Path) -> Result<String, String> {
    fs::read(path)
        .map(|bytes| sha256_bytes_hex(&bytes))
        .map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_pair_verifier_rejects_diagnostic_documents_by_schema_and_reserved_path() {
        let root = tempfile::tempdir().expect("diagnostic native-pair root");
        let reserved = root.path().join(
            crate::bench_support::derived_access::DERIVED_CHANGE_DIAGNOSTIC_REPORT_BASENAME_V1,
        );
        fs::write(&reserved, b"{}").expect("write reserved diagnostic report");

        assert_eq!(
            verify_authority_stamp_native_receipts_v1(&[reserved.clone(), reserved])
                .unwrap_err(),
            crate::bench_support::derived_access::DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
        );

        for (name, schema) in [
            (
                "report",
                crate::bench_support::derived_access::DERIVED_CHANGE_DIAGNOSTIC_REPORT_SCHEMA_V1,
            ),
            (
                "fragment",
                crate::bench_support::derived_access::DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1,
            ),
            (
                "collection",
                crate::bench_support::derived_access::DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
            ),
        ] {
            let path = root.path().join(format!("renamed-{name}.json"));
            fs::write(&path, serde_json::json!({ "schema": schema }).to_string())
                .expect("write diagnostic schema document");
            assert_eq!(
                verify_authority_stamp_native_receipts_v1(&[path.clone(), path])
                    .unwrap_err(),
                crate::bench_support::derived_access::DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
            );
        }

        let nested = root
            .path()
            .join(crate::bench_support::derived_access::DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1)
            .join("receipt.json");
        fs::create_dir_all(nested.parent().expect("nested parent"))
            .expect("create reserved diagnostic root");
        fs::write(&nested, b"{}").expect("write nested diagnostic document");
        assert_eq!(
            verify_authority_stamp_native_receipts_v1(&[nested.clone(), nested])
                .unwrap_err(),
            crate::bench_support::derived_access::DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
        );
    }

    #[test]
    fn native_scenario_runner_covers_the_frozen_matrix_with_bounded_stamp_reads() {
        let root = tempfile::tempdir().expect("temp root");
        let scenarios = run_scenarios(root.path(), None).expect("native scenarios");

        assert_eq!(scenarios.len(), AuthorityStampScenarioV1::ALL.len());
        assert!(scenarios.iter().all(|row| {
            row.event_directory_entries_walked == 0
                && row.event_carrier_opens == 0
                && !row.truth_change_proven
        }));
        assert!(scenarios.iter().all(|row| {
            row.accepted == expectation_accepts(row.expectation, row.observation)
                && row.expectation == scenario_expectation(row.scenario)
        }));
    }

    #[test]
    fn changed_stamp_never_claims_an_event_is_valid() {
        let root = tempfile::tempdir().expect("temp root");
        let row = run_scenario(root.path(), AuthorityStampScenarioV1::OutOfBandCreate, None)
            .expect("out-of-band scenario");

        assert!(matches!(
            row.observation,
            AuthorityStampObservationV1::Stable | AuthorityStampObservationV1::Changed
        ));
        assert!(!row.truth_change_proven);
    }

    #[test]
    fn existing_carrier_overwrite_is_a_stamp_non_claim_but_validation_fails() {
        let root = tempfile::tempdir().expect("temp root");
        let row = run_scenario(
            root.path(),
            AuthorityStampScenarioV1::ExistingCarrierOverwrite,
            None,
        )
        .expect("overwrite scenario");

        assert_eq!(
            row.expectation,
            AuthorityStampExpectationV1::ExplicitNonClaim
        );
        assert_eq!(
            row.selected_carrier_validation_detected_corruption,
            Some(true)
        );
        assert!(row.accepted);
    }

    #[test]
    fn sidecar_deletion_is_an_explicit_non_observation() {
        let root = tempfile::tempdir().expect("temp root");
        let row = run_scenario(root.path(), AuthorityStampScenarioV1::SidecarDeletion, None)
            .expect("sidecar scenario");

        assert_eq!(
            row.expectation,
            AuthorityStampExpectationV1::ObservationNotApplicable
        );
        assert_eq!(row.observation, AuthorityStampObservationV1::NotApplicable);
        assert!(row.stamp_before_sha256.is_none());
        assert!(row.stamp_after_sha256.is_none());
        assert!(row.accepted);
    }

    #[test]
    fn native_pair_checks_source_authority_before_the_qualification_result() {
        fn receipt(
            commit: &str,
            platform: AuthorityStampPlatformV1,
            accepted: bool,
        ) -> AuthorityStampNativeReceiptV1 {
            let (operating_system, filesystem) = match platform {
                AuthorityStampPlatformV1::MacosApfs => ("macos", "apfs"),
                AuthorityStampPlatformV1::WindowsNtfs => ("windows", "ntfs"),
            };
            AuthorityStampNativeReceiptV1 {
                schema: String::new(),
                execution: AuthorityStampExecutionIdentityV1 {
                    platform,
                    source_commit: commit.to_owned(),
                    source_tree: "1".repeat(40),
                    cargo_lock_sha256: "2".repeat(64),
                    binary_sha256: "3".repeat(64),
                    operating_system: operating_system.to_owned(),
                    architecture: "test".to_owned(),
                    filesystem: filesystem.to_owned(),
                    host_identity_sha256: match platform {
                        AuthorityStampPlatformV1::MacosApfs => "4".repeat(64),
                        AuthorityStampPlatformV1::WindowsNtfs => "7".repeat(64),
                    },
                    command_sha256: "5".repeat(64),
                    probe_root_identity_sha256: "6".repeat(64),
                },
                scope: String::new(),
                malicious_tamper_detection_claimed: false,
                scenarios: Vec::new(),
                all_scenarios_accepted: accepted,
                completion_published_last: true,
                receipt_sha256: String::new(),
            }
        }

        let apfs = receipt("a", AuthorityStampPlatformV1::MacosApfs, true);
        let mixed_ntfs = receipt("b", AuthorityStampPlatformV1::WindowsNtfs, false);
        assert_eq!(
            validate_native_pair(&apfs, &mixed_ntfs).unwrap_err(),
            "authority-stamp native receipts do not share source authority"
        );

        let rejected_ntfs = receipt("a", AuthorityStampPlatformV1::WindowsNtfs, false);
        assert_eq!(
            validate_native_pair(&apfs, &rejected_ntfs).unwrap_err(),
            "authority-stamp native evidence contains a qualifying falsifier"
        );

        let mut reused_host_ntfs = receipt("a", AuthorityStampPlatformV1::WindowsNtfs, true);
        reused_host_ntfs.execution.host_identity_sha256 =
            apfs.execution.host_identity_sha256.clone();
        assert_eq!(
            validate_native_pair(&apfs, &reused_host_ntfs).unwrap_err(),
            "authority-stamp native receipts reuse one campaign-host authority"
        );
    }
}
