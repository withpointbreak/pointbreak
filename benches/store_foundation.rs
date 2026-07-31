//! Deterministic smoke entry point for durable-store qualification workloads.
//!
//! The default modes establish workload and transfer identity. The explicit
//! candidate smoke modes exercise only developer-gated qualification profiles.

use std::process::{Command, ExitCode};

use pointbreak::bench_support::derived_access::{
    DERIVED_ACCESS_AUTHORITY_STAMP_CHILD_MODE_V1, DERIVED_ACCESS_AUTHORITY_STAMP_MODE_V1,
    DERIVED_ACCESS_AUTHORITY_STAMP_VERIFY_MODE_V1, DERIVED_ACCESS_PRODUCT_CONTRACT_MODE_V1,
    DERIVED_ACCESS_PRODUCT_CONTRACT_SMOKE_MODE_V1, DERIVED_ACCESS_PRODUCT_CONTRACT_VERIFY_MODE_V1,
    DERIVED_ACCESS_READINESS_CONTRACT_MODE_V1, DERIVED_ACCESS_READINESS_CONTRACT_SMOKE_MODE_V1,
    DERIVED_ACCESS_READINESS_CONTRACT_VERIFY_MODE_V1,
    QUALIFICATION_DERIVED_ACCESS_CONTRACT_MODE_V1, QUALIFICATION_DERIVED_ACCESS_FRAGMENT_MODE_V1,
    QUALIFICATION_DERIVED_ACCESS_HELP_MODE_V1,
    QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_CHILD_MODE_V1,
    QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_MODE_V1, QUALIFICATION_DERIVED_ACCESS_PACKAGE_MODE_V1,
    QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_MODE_V1,
    QUALIFICATION_DERIVED_ACCESS_RESOURCE_MODE_V1,
    QUALIFICATION_DERIVED_ACCESS_RESTART_CHILD_MODE_V1,
    QUALIFICATION_DERIVED_ACCESS_RETAINED_BOOTSTRAP_MODE_V1,
    QUALIFICATION_DERIVED_ACCESS_RETAINED_PREFLIGHT_MODE_V1,
    QUALIFICATION_DERIVED_ACCESS_SCALE_MODE_V1, QUALIFICATION_DERIVED_ACCESS_SMOKE_MODE_V1,
    QUALIFICATION_DERIVED_ACCESS_VERIFY_PACKAGE_MODE_V1, QualificationDerivedAccessTierV1,
    assemble_qualification_derived_access_package_v1,
    bootstrap_qualification_derived_access_retained_root_v1,
    build_qualification_derived_access_fragment_v1,
    derived_access_product_contract_publication_json_v1,
    derived_access_product_contract_smoke_json_v1, derived_access_product_contract_verify_json_v1,
    derived_access_readiness_contract_publication_json_v1,
    derived_access_readiness_contract_smoke_json_v1,
    derived_access_readiness_contract_verify_json_v1,
    preflight_qualification_derived_access_retained_root_v1,
    qualification_derived_access_contract_v1_publication, run_authority_stamp_child_v1,
    run_authority_stamp_native_probe_v1, run_qualification_derived_access_lifecycle_child_v1,
    run_qualification_derived_access_lifecycle_v1,
    run_qualification_derived_access_longitudinal_smoke_at_v1,
    run_qualification_derived_access_longitudinal_smoke_v1,
    run_qualification_derived_access_native_smoke_v1,
    run_qualification_derived_access_non_timing_smoke_at_v1,
    run_qualification_derived_access_non_timing_smoke_v1,
    run_qualification_derived_access_resource_child_v1,
    run_qualification_derived_access_resource_v1,
    run_qualification_derived_access_restart_child_v1, run_qualification_derived_access_scale_v1,
    verify_authority_stamp_native_receipts_v1, verify_qualification_derived_access_package_v1,
};
use pointbreak::bench_support::foundation::{
    DisposableBundleDestinationV2, ExactBundleClosureV2, ExactBundleFailurePointV2,
    ExactBundleManifestV2, ExactBundlePublicationReportV2, ImportReceiptPolicyPrototypeV1,
    ImportReceiptPrototypeV1, LogicalCapabilityEpochV1,
    QUALIFICATION_CONTENT_ONLY_CONTRACT_PUBLICATION_MODE_V1,
    QUALIFICATION_LMDB_PROSPECTIVE_EVIDENCE_MODE_V1,
    QUALIFICATION_LMDB_PROSPECTIVE_PACKAGE_MODE_V1, QUALIFICATION_LMDB_PROSPECTIVE_SMOKE_MODE_V1,
    QUALIFICATION_LOOSE_BASELINE_EVIDENCE_MODE_V1, QUALIFICATION_LOOSE_BASELINE_SMOKE_MODE_V1,
    QUALIFICATION_PROSPECTIVE_CONTRACT_PUBLICATION_MODE_V1, QualificationCorpusError,
    QualificationCorpusSummaryV1, QualificationLooseBaselineEvidenceConfigurationV1,
    QualificationLooseBaselineSmokeConfigurationV1,
    QualificationPerformanceCampaignConfigurationV2,
    QualificationPerformanceDiagnosticConfigurationV1, QualificationPerformanceEvidenceV2,
    QualificationPerformancePackageV2, QualificationPerformancePairOrderV1,
    QualificationRunConfigurationV1, QualificationSnapshotTotalsV1, ReceiptBackupConsequenceV1,
    ReceiptProjectionConsequenceV1, SegmentWorkloadEvidenceV1, SnapshotDriftReportV1,
    SqliteWorkloadEvidenceV1, load_external_workload_v2_manifest_from_env,
    modeled_post_foundation_manifest, publish_exact_bundle_v2, qualification_cargo_lock_sha256,
    qualification_content_only_contract_v1_publication, qualification_filesystem_name,
    qualification_generated_workload_smoke_v1, qualification_performance_contract_v2_publication,
    qualification_prospective_contract_v1_publication, qualification_source_commit,
    run_qualification_child, run_qualification_loose_baseline_evidence_v1,
    run_qualification_loose_baseline_open_child_v1, run_qualification_loose_baseline_smoke_v1,
    run_qualification_performance_campaign_v2, run_qualification_performance_diagnostics,
    run_qualification_performance_open_child, run_qualification_platform_matrix,
    run_segment_workload, run_sqlite_workload, synthetic_legacy_manifest,
};
#[cfg(feature = "lmdb-proof")]
use pointbreak::bench_support::foundation::{
    QualificationLmdbProspectiveEvidenceConfigurationV1, QualificationLmdbProspectivePackageV1,
    parse_qualification_lmdb_prospective_shard_v1, qualification_lmdb_prospective_execution_v1,
    run_lmdb_proof_open_close_v1, run_qualification_lmdb_lifecycle_child_v1,
    run_qualification_lmdb_lifecycle_smoke_v1, run_qualification_lmdb_prospective_evidence_v1,
    run_qualification_lmdb_prospective_open_child_v1, run_qualification_lmdb_prospective_smoke_v1,
    run_qualification_lmdb_smoke_v1,
};
use pointbreak::bench_support::longitudinal::{
    LONGITUDINAL_CAPACITY_PACKAGE_FILE_V1, LONGITUDINAL_CARRY_FORWARD_MODE_V1,
    LONGITUDINAL_CARRY_FORWARD_SMOKE_MODE_V1, LONGITUDINAL_CONTRACT_MODE_V1,
    LONGITUDINAL_EVIDENCE_PACKAGE_FILE_V1, LONGITUDINAL_HELP_MODE_V1, LONGITUDINAL_SMOKE_MODE_V1,
    LONGITUDINAL_VERIFY_CARRY_FORWARD_MODE_V1, LONGITUDINAL_VERIFY_PACKAGE_MODE_V1,
    LONGITUDINAL_VERIFY_PACKAGE_RECEIPT_MODE_V1, LongitudinalCarryForwardRequestV1,
    LongitudinalProcessCaptureError, LongitudinalProcessSnapshotV1,
    capture_longitudinal_process_snapshot_v1, longitudinal_carry_forward_non_timing_smoke_v1,
    longitudinal_contract_publication_v1, longitudinal_help_v1,
    longitudinal_mach_ticks_to_nanos_v1, longitudinal_non_timing_smoke_v1,
    verify_longitudinal_capacity_package_v1,
    verify_longitudinal_carry_forward_authority_package_v1,
    verify_longitudinal_evidence_package_v1, verify_longitudinal_package_receipt_v1,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

const USAGE: &str = "\
Usage: cargo bench --features bench --bench store_foundation -- [--smoke|--generated-workload-smoke|--longitudinal-contract|--longitudinal-help|--longitudinal-smoke|--longitudinal-carry-forward|--longitudinal-carry-forward-smoke|--longitudinal-verify-package|--longitudinal-verify-package-receipt|--longitudinal-verify-carry-forward|--derived-access-product-contract|--derived-access-product-contract-verify|--derived-access-product-contract-smoke|--derived-access-readiness-contract|--derived-access-readiness-contract-verify|--derived-access-readiness-contract-smoke|--derived-access-authority-stamp|--derived-access-authority-stamp-verify|--derived-access-contract|--derived-access-help|--derived-access-smoke|--derived-access-lifecycle|--derived-access-retained-preflight|--derived-access-retained-bootstrap|--derived-access-scale-evidence|--derived-access-resource-evidence|--derived-access-fragment|--derived-access-package|--derived-access-verify-package|--loose-baseline-smoke|--loose-baseline-evidence|--prospective-contract|--content-only-contract|--transfer-smoke|--sqlite-smoke|--segments-smoke|--lmdb-proof-open-close|--lmdb-smoke|--lmdb-lifecycle-smoke|--lmdb-prospective-smoke|--lmdb-prospective-evidence|--lmdb-prospective-package|--qualification-smoke|--qualification-evidence|--qualification-diagnostics|--qualification-contract|--qualification-final-evidence|--qualification-package|--help]\n\
       --longitudinal-carry-forward --longitudinal-carry-forward-request=<path>\n\
       --longitudinal-verify-package --longitudinal-package-root=<path>\n\
       --longitudinal-verify-package-receipt --longitudinal-package-root=<path>\n\
       --longitudinal-verify-carry-forward --longitudinal-authority-package=<path> --longitudinal-package-root=<path>\n\
       --derived-access-smoke [--derived-access-tier=D0-128|L1|L7] [--derived-access-root=<empty-path>]\n\
                              [--derived-access-request=<path>]\n\
       --derived-access-authority-stamp --derived-access-source=<clean-checkout> --derived-access-root=<empty-path> --derived-access-output=<receipt.json>\n\
       --derived-access-authority-stamp-verify --derived-access-input=<apfs.json> --derived-access-input=<ntfs.json>\n\
       (authority-stamp modes require --features longitudinal-counting)\n\
       --derived-access-verify-package --derived-access-package-root=<path>\n\
       --qualification-diagnostics [--qualification-pair-order=alternating|candidate_then_baseline|baseline_then_candidate]\n\
       --qualification-package --qualification-input=<path> [--qualification-input=<path> ...]\n\
       --lmdb-prospective-package --lmdb-prospective-input=<path> [--lmdb-prospective-input=<path> ...]\n\
\n\
Validates deterministic workload, transfer, candidate, or native-platform qualification contracts and prints JSON.\n\
Qualification modes use disposable roots and never select or activate production storage.\n";

const _: fn(u64, u32, u32) -> Result<u64, LongitudinalProcessCaptureError> =
    longitudinal_mach_ticks_to_nanos_v1;
const _: fn(u32) -> Result<LongitudinalProcessSnapshotV1, LongitudinalProcessCaptureError> =
    capture_longitudinal_process_snapshot_v1;

#[derive(Serialize)]
struct SmokeMetadataV1 {
    schema: &'static str,
    build: BuildMetadataV1,
    dependencies: DependencyMetadataV1,
    runtime: RuntimeMetadataV1,
    configuration: ConfigurationMetadataV1,
    capabilities: LogicalCapabilityEpochV1,
    synthetic_legacy: QualificationCorpusSummaryV1,
    modeled_workload: QualificationCorpusSummaryV1,
    external_legacy: ExternalCorpusMetadataV1,
}

#[derive(Serialize)]
struct BuildMetadataV1 {
    package_version: &'static str,
    source: &'static str,
    commit: Option<&'static str>,
    describe: &'static str,
    dirty: bool,
}

#[derive(Serialize)]
struct DependencyMetadataV1 {
    cargo_lock_sha256: String,
    rustc: String,
}

#[derive(Serialize)]
struct RuntimeMetadataV1 {
    os: &'static str,
    architecture: &'static str,
    filesystem: String,
}

#[derive(Serialize)]
struct ConfigurationMetadataV1 {
    mode: &'static str,
    external_corpus_variable: &'static str,
    external_corpus_configured: bool,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ExternalCorpusMetadataV1 {
    NotConfigured,
    Validated {
        summary: QualificationCorpusSummaryV1,
        snapshot: QualificationSnapshotTotalsV1,
    },
    SnapshotDrift {
        report: SnapshotDriftReportV1,
    },
    Invalid {
        message: String,
    },
}

#[derive(Serialize)]
struct TransferSmokeMetadataV1 {
    schema: &'static str,
    mode: &'static str,
    bundle_sha256: String,
    event_set_sha256: String,
    event_count: usize,
    content_count: usize,
    closure_count: usize,
    interrupted_publication: InterruptedPublicationMetadataV1,
    completion: ExactBundlePublicationReportV2,
    idempotent_retry: ExactBundlePublicationReportV2,
    exact_bytes_verified: bool,
    receipt_alternatives: Vec<ReceiptAlternativeMetadataV1>,
}

#[derive(Serialize)]
struct SqliteSmokeMetadataV1 {
    schema: &'static str,
    mode: &'static str,
    workloads: Vec<SqliteWorkloadEvidenceV1>,
}

#[derive(Serialize)]
struct SegmentSmokeMetadataV1 {
    schema: &'static str,
    mode: &'static str,
    workloads: Vec<SegmentWorkloadEvidenceV1>,
}

#[derive(Serialize)]
struct InterruptedPublicationMetadataV1 {
    content_count: usize,
    event_count: usize,
}

#[derive(Serialize)]
struct ReceiptAlternativeMetadataV1 {
    policy: ImportReceiptPolicyPrototypeV1,
    receipt_sha256: String,
    projection: ReceiptProjectionConsequenceV1,
    backup: ReceiptBackupConsequenceV1,
    emits_local_provenance_event: bool,
}

fn main() -> ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_some_and(|argument| argument == DERIVED_ACCESS_AUTHORITY_STAMP_CHILD_MODE_V1)
    {
        let actions = arguments
            .iter()
            .filter_map(|argument| argument.strip_prefix("--derived-access-authority-action="))
            .collect::<Vec<_>>();
        let roots = arguments
            .iter()
            .filter_map(|argument| argument.strip_prefix("--derived-access-root="))
            .collect::<Vec<_>>();
        if actions.len() != 1 || roots.len() != 1 {
            eprintln!("authority-stamp child requires one action and one root");
            return ExitCode::from(2);
        }
        return match run_authority_stamp_child_v1(actions[0], std::path::Path::new(roots[0])) {
            Ok(output) => {
                println!("{output}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("store foundation authority-stamp child stopped: {error}");
                ExitCode::from(1)
            }
        };
    }
    if arguments
        .first()
        .is_some_and(|argument| argument == QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_CHILD_MODE_V1)
    {
        if arguments.len() != 2 {
            eprintln!("derived-access lifecycle child requires exactly one request path");
            return ExitCode::from(2);
        }
        return match run_qualification_derived_access_lifecycle_child_v1(std::path::Path::new(
            &arguments[1],
        )) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("store foundation derived-access lifecycle child failed: {error}");
                ExitCode::from(1)
            }
        };
    }
    if arguments
        .first()
        .is_some_and(|argument| argument == QUALIFICATION_DERIVED_ACCESS_RESOURCE_CHILD_MODE_V1)
    {
        if arguments.len() != 2 {
            eprintln!("derived-access resource child requires exactly one request path");
            return ExitCode::from(2);
        }
        return match run_qualification_derived_access_resource_child_v1(std::path::Path::new(
            &arguments[1],
        )) {
            Ok(receipt) => {
                println!(
                    "{}",
                    serde_json::to_string(&receipt)
                        .expect("derived-access resource child receipt serializes")
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("store foundation derived-access resource child failed: {error}");
                ExitCode::from(1)
            }
        };
    }
    if arguments
        .first()
        .is_some_and(|argument| argument == QUALIFICATION_DERIVED_ACCESS_RESTART_CHILD_MODE_V1)
    {
        if arguments.len() != 2 {
            eprintln!("derived-access restart child requires exactly one request path");
            return ExitCode::from(2);
        }
        return match run_qualification_derived_access_restart_child_v1(std::path::Path::new(
            &arguments[1],
        )) {
            Ok(receipt) => {
                println!(
                    "{}",
                    serde_json::to_string(&receipt)
                        .expect("derived-access restart child receipt serializes")
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("store foundation derived-access restart child failed: {error}");
                ExitCode::from(1)
            }
        };
    }
    if arguments
        .first()
        .is_some_and(|argument| argument == "--loose-baseline-open-child")
    {
        if arguments.len() != 2 {
            eprintln!("loose baseline open child requires exactly one request path");
            return ExitCode::from(2);
        }
        return match run_qualification_loose_baseline_open_child_v1(std::path::Path::new(
            &arguments[1],
        )) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("store foundation loose baseline open child failed: {error}");
                ExitCode::from(1)
            }
        };
    }
    if arguments
        .first()
        .is_some_and(|argument| argument == "--qualification-performance-open-child")
    {
        if arguments.len() != 2 {
            eprintln!("qualification performance child requires exactly one request path");
            return ExitCode::from(2);
        }
        return match run_qualification_performance_open_child(std::path::Path::new(&arguments[1])) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("store foundation qualification performance child failed: {error}");
                ExitCode::from(1)
            }
        };
    }
    if arguments
        .first()
        .is_some_and(|argument| argument == "--qualification-child")
    {
        if arguments.len() != 2 {
            eprintln!("qualification child requires exactly one request path");
            return ExitCode::from(2);
        }
        return match run_qualification_child(std::path::Path::new(&arguments[1])) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("store foundation qualification child failed: {error}");
                ExitCode::from(1)
            }
        };
    }
    #[cfg(feature = "lmdb-proof")]
    if arguments
        .first()
        .is_some_and(|argument| argument == "--lmdb-lifecycle-child")
    {
        if arguments.len() != 2 {
            eprintln!("plain LMDB lifecycle child requires exactly one request path");
            return ExitCode::from(2);
        }
        return match run_qualification_lmdb_lifecycle_child_v1(std::path::Path::new(&arguments[1]))
        {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("store foundation plain LMDB lifecycle child failed: {error}");
                ExitCode::from(1)
            }
        };
    }
    #[cfg(feature = "lmdb-proof")]
    if arguments
        .first()
        .is_some_and(|argument| argument == "--lmdb-prospective-open-child")
    {
        if arguments.len() != 2 {
            eprintln!("plain LMDB prospective open child requires exactly one request path");
            return ExitCode::from(2);
        }
        return match run_qualification_lmdb_prospective_open_child_v1(std::path::Path::new(
            &arguments[1],
        )) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("store foundation plain LMDB prospective open child failed: {error}");
                ExitCode::from(1)
            }
        };
    }
    if arguments.iter().any(|argument| argument == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let requested_modes = [
        "--smoke",
        "--generated-workload-smoke",
        LONGITUDINAL_CONTRACT_MODE_V1,
        LONGITUDINAL_HELP_MODE_V1,
        LONGITUDINAL_SMOKE_MODE_V1,
        LONGITUDINAL_CARRY_FORWARD_MODE_V1,
        LONGITUDINAL_CARRY_FORWARD_SMOKE_MODE_V1,
        LONGITUDINAL_VERIFY_PACKAGE_MODE_V1,
        LONGITUDINAL_VERIFY_PACKAGE_RECEIPT_MODE_V1,
        LONGITUDINAL_VERIFY_CARRY_FORWARD_MODE_V1,
        DERIVED_ACCESS_PRODUCT_CONTRACT_MODE_V1,
        DERIVED_ACCESS_PRODUCT_CONTRACT_VERIFY_MODE_V1,
        DERIVED_ACCESS_PRODUCT_CONTRACT_SMOKE_MODE_V1,
        DERIVED_ACCESS_READINESS_CONTRACT_MODE_V1,
        DERIVED_ACCESS_READINESS_CONTRACT_VERIFY_MODE_V1,
        DERIVED_ACCESS_READINESS_CONTRACT_SMOKE_MODE_V1,
        DERIVED_ACCESS_AUTHORITY_STAMP_MODE_V1,
        DERIVED_ACCESS_AUTHORITY_STAMP_VERIFY_MODE_V1,
        QUALIFICATION_DERIVED_ACCESS_CONTRACT_MODE_V1,
        QUALIFICATION_DERIVED_ACCESS_FRAGMENT_MODE_V1,
        QUALIFICATION_DERIVED_ACCESS_HELP_MODE_V1,
        QUALIFICATION_DERIVED_ACCESS_SMOKE_MODE_V1,
        QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_MODE_V1,
        QUALIFICATION_DERIVED_ACCESS_RETAINED_PREFLIGHT_MODE_V1,
        QUALIFICATION_DERIVED_ACCESS_RETAINED_BOOTSTRAP_MODE_V1,
        QUALIFICATION_DERIVED_ACCESS_SCALE_MODE_V1,
        QUALIFICATION_DERIVED_ACCESS_RESOURCE_MODE_V1,
        QUALIFICATION_DERIVED_ACCESS_PACKAGE_MODE_V1,
        QUALIFICATION_DERIVED_ACCESS_VERIFY_PACKAGE_MODE_V1,
        QUALIFICATION_LOOSE_BASELINE_SMOKE_MODE_V1,
        QUALIFICATION_LOOSE_BASELINE_EVIDENCE_MODE_V1,
        QUALIFICATION_PROSPECTIVE_CONTRACT_PUBLICATION_MODE_V1,
        QUALIFICATION_CONTENT_ONLY_CONTRACT_PUBLICATION_MODE_V1,
        "--transfer-smoke",
        "--sqlite-smoke",
        "--segments-smoke",
        "--lmdb-proof-open-close",
        "--lmdb-smoke",
        "--lmdb-lifecycle-smoke",
        QUALIFICATION_LMDB_PROSPECTIVE_SMOKE_MODE_V1,
        QUALIFICATION_LMDB_PROSPECTIVE_EVIDENCE_MODE_V1,
        QUALIFICATION_LMDB_PROSPECTIVE_PACKAGE_MODE_V1,
        "--qualification-smoke",
        "--qualification-evidence",
        "--qualification-diagnostics",
        "--qualification-contract",
        "--qualification-final-evidence",
        "--qualification-package",
    ]
    .into_iter()
    .filter(|mode| arguments.iter().any(|argument| argument == mode))
    .count();
    let diagnostic_pair_order = match qualification_pair_order(&arguments) {
        Ok(order) => order,
        Err(()) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let diagnostics_requested = arguments
        .iter()
        .any(|argument| argument == "--qualification-diagnostics");
    let package_requested = arguments
        .iter()
        .any(|argument| argument == "--qualification-package");
    let package_inputs = arguments
        .iter()
        .filter_map(|argument| argument.strip_prefix("--qualification-input="))
        .collect::<Vec<_>>();
    let longitudinal_package_requested = arguments
        .iter()
        .any(|argument| argument == LONGITUDINAL_VERIFY_PACKAGE_MODE_V1);
    let longitudinal_package_receipt_requested = arguments
        .iter()
        .any(|argument| argument == LONGITUDINAL_VERIFY_PACKAGE_RECEIPT_MODE_V1);
    let longitudinal_carry_forward_requested = arguments
        .iter()
        .any(|argument| argument == LONGITUDINAL_CARRY_FORWARD_MODE_V1);
    let longitudinal_carry_forward_requests = arguments
        .iter()
        .filter_map(|argument| argument.strip_prefix("--longitudinal-carry-forward-request="))
        .collect::<Vec<_>>();
    let longitudinal_authority_verify_requested = arguments
        .iter()
        .any(|argument| argument == LONGITUDINAL_VERIFY_CARRY_FORWARD_MODE_V1);
    let longitudinal_authority_packages = arguments
        .iter()
        .filter_map(|argument| argument.strip_prefix("--longitudinal-authority-package="))
        .collect::<Vec<_>>();
    let longitudinal_package_roots = arguments
        .iter()
        .filter_map(|argument| argument.strip_prefix("--longitudinal-package-root="))
        .collect::<Vec<_>>();
    let lmdb_prospective_package_requested = arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_LMDB_PROSPECTIVE_PACKAGE_MODE_V1);
    let lmdb_prospective_inputs = arguments
        .iter()
        .filter_map(|argument| argument.strip_prefix("--lmdb-prospective-input="))
        .collect::<Vec<_>>();
    if arguments.iter().any(|argument| {
        argument != "--smoke"
            && argument != "--generated-workload-smoke"
            && argument != LONGITUDINAL_CONTRACT_MODE_V1
            && argument != LONGITUDINAL_HELP_MODE_V1
            && argument != LONGITUDINAL_SMOKE_MODE_V1
            && argument != LONGITUDINAL_CARRY_FORWARD_MODE_V1
            && argument != LONGITUDINAL_CARRY_FORWARD_SMOKE_MODE_V1
            && argument != LONGITUDINAL_VERIFY_PACKAGE_MODE_V1
            && argument != LONGITUDINAL_VERIFY_PACKAGE_RECEIPT_MODE_V1
            && argument != LONGITUDINAL_VERIFY_CARRY_FORWARD_MODE_V1
            && argument != DERIVED_ACCESS_PRODUCT_CONTRACT_MODE_V1
            && argument != DERIVED_ACCESS_PRODUCT_CONTRACT_VERIFY_MODE_V1
            && argument != DERIVED_ACCESS_PRODUCT_CONTRACT_SMOKE_MODE_V1
            && argument != DERIVED_ACCESS_READINESS_CONTRACT_MODE_V1
            && argument != DERIVED_ACCESS_READINESS_CONTRACT_VERIFY_MODE_V1
            && argument != DERIVED_ACCESS_READINESS_CONTRACT_SMOKE_MODE_V1
            && argument != DERIVED_ACCESS_AUTHORITY_STAMP_MODE_V1
            && argument != DERIVED_ACCESS_AUTHORITY_STAMP_VERIFY_MODE_V1
            && argument != QUALIFICATION_DERIVED_ACCESS_CONTRACT_MODE_V1
            && argument != QUALIFICATION_DERIVED_ACCESS_FRAGMENT_MODE_V1
            && argument != QUALIFICATION_DERIVED_ACCESS_HELP_MODE_V1
            && argument != QUALIFICATION_DERIVED_ACCESS_SMOKE_MODE_V1
            && argument != QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_MODE_V1
            && argument != QUALIFICATION_DERIVED_ACCESS_RETAINED_PREFLIGHT_MODE_V1
            && argument != QUALIFICATION_DERIVED_ACCESS_RETAINED_BOOTSTRAP_MODE_V1
            && argument != QUALIFICATION_DERIVED_ACCESS_SCALE_MODE_V1
            && argument != QUALIFICATION_DERIVED_ACCESS_RESOURCE_MODE_V1
            && argument != QUALIFICATION_DERIVED_ACCESS_PACKAGE_MODE_V1
            && argument != QUALIFICATION_DERIVED_ACCESS_VERIFY_PACKAGE_MODE_V1
            && argument != QUALIFICATION_LOOSE_BASELINE_SMOKE_MODE_V1
            && argument != QUALIFICATION_LOOSE_BASELINE_EVIDENCE_MODE_V1
            && argument != QUALIFICATION_PROSPECTIVE_CONTRACT_PUBLICATION_MODE_V1
            && argument != QUALIFICATION_CONTENT_ONLY_CONTRACT_PUBLICATION_MODE_V1
            && argument != "--transfer-smoke"
            && argument != "--sqlite-smoke"
            && argument != "--segments-smoke"
            && argument != "--lmdb-proof-open-close"
            && argument != "--lmdb-smoke"
            && argument != "--lmdb-lifecycle-smoke"
            && argument != QUALIFICATION_LMDB_PROSPECTIVE_SMOKE_MODE_V1
            && argument != QUALIFICATION_LMDB_PROSPECTIVE_EVIDENCE_MODE_V1
            && argument != QUALIFICATION_LMDB_PROSPECTIVE_PACKAGE_MODE_V1
            && argument != "--qualification-smoke"
            && argument != "--qualification-evidence"
            && argument != "--qualification-diagnostics"
            && argument != "--qualification-contract"
            && argument != "--qualification-final-evidence"
            && argument != "--qualification-package"
            && argument != "--bench"
            && !argument.starts_with("--qualification-pair-order=")
            && !argument.starts_with("--qualification-input=")
            && !argument.starts_with("--longitudinal-package-root=")
            && !argument.starts_with("--longitudinal-carry-forward-request=")
            && !argument.starts_with("--longitudinal-authority-package=")
            && !argument.starts_with("--lmdb-prospective-input=")
            && !argument.starts_with("--derived-access-tier=")
            && !argument.starts_with("--derived-access-request=")
            && !argument.starts_with("--derived-access-input=")
            && !argument.starts_with("--derived-access-package-root=")
            && !argument.starts_with("--derived-access-root=")
            && !argument.starts_with("--derived-access-source=")
            && !argument.starts_with("--derived-access-output=")
    }) || requested_modes > 1
        || (!diagnostics_requested && diagnostic_pair_order.is_some())
        || (!package_requested && !package_inputs.is_empty())
        || (package_requested && package_inputs.is_empty())
        || (!(longitudinal_package_requested
            || longitudinal_package_receipt_requested
            || longitudinal_authority_verify_requested)
            && !longitudinal_package_roots.is_empty())
        || ((longitudinal_package_requested
            || longitudinal_package_receipt_requested
            || longitudinal_authority_verify_requested)
            && longitudinal_package_roots.len() != 1)
        || (!longitudinal_carry_forward_requested
            && !longitudinal_carry_forward_requests.is_empty())
        || (longitudinal_carry_forward_requested && longitudinal_carry_forward_requests.len() != 1)
        || (!longitudinal_authority_verify_requested && !longitudinal_authority_packages.is_empty())
        || (longitudinal_authority_verify_requested && longitudinal_authority_packages.len() != 1)
        || (!lmdb_prospective_package_requested && !lmdb_prospective_inputs.is_empty())
        || (lmdb_prospective_package_requested && lmdb_prospective_inputs.is_empty())
    {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }

    if arguments
        .iter()
        .any(|argument| argument == "--generated-workload-smoke")
    {
        return match qualification_generated_workload_smoke_v1() {
            Ok(report) => {
                println!(
                    "{}",
                    serde_json::to_string(&report)
                        .expect("generated workload smoke report serializes")
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("store foundation generated workload smoke failed: {error}");
                ExitCode::from(1)
            }
        };
    }

    if arguments
        .iter()
        .any(|argument| argument == DERIVED_ACCESS_AUTHORITY_STAMP_MODE_V1)
    {
        let roots = arguments
            .iter()
            .filter_map(|argument| argument.strip_prefix("--derived-access-root="))
            .collect::<Vec<_>>();
        let sources = arguments
            .iter()
            .filter_map(|argument| argument.strip_prefix("--derived-access-source="))
            .collect::<Vec<_>>();
        let outputs = arguments
            .iter()
            .filter_map(|argument| argument.strip_prefix("--derived-access-output="))
            .collect::<Vec<_>>();
        if roots.len() != 1 || sources.len() != 1 || outputs.len() != 1 {
            eprintln!("authority-stamp probe requires one source, root, and output");
            return ExitCode::from(2);
        }
        return report_derived_access_smoke(run_authority_stamp_native_probe_v1(
            std::path::Path::new(sources[0]),
            std::path::Path::new(roots[0]),
            std::path::Path::new(outputs[0]),
        ));
    }

    if arguments
        .iter()
        .any(|argument| argument == DERIVED_ACCESS_AUTHORITY_STAMP_VERIFY_MODE_V1)
    {
        let inputs = arguments
            .iter()
            .filter_map(|argument| argument.strip_prefix("--derived-access-input="))
            .map(std::path::PathBuf::from)
            .collect::<Vec<_>>();
        return report_derived_access_smoke(verify_authority_stamp_native_receipts_v1(&inputs));
    }

    if arguments
        .iter()
        .any(|argument| argument == LONGITUDINAL_CONTRACT_MODE_V1)
    {
        let publication = longitudinal_contract_publication_v1();
        if let Err(error) = publication.validate() {
            eprintln!("store foundation longitudinal contract failed: {error}");
            return ExitCode::from(1);
        }
        println!(
            "{}",
            serde_json::to_string(&publication)
                .expect("longitudinal contract publication serializes")
        );
        return ExitCode::SUCCESS;
    }

    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_DERIVED_ACCESS_HELP_MODE_V1)
    {
        println!(
            "Derived-access qualification modes:\n\
             --derived-access-product-contract\n\
             --derived-access-product-contract-verify\n\
             --derived-access-product-contract-smoke\n\
             --derived-access-readiness-contract\n\
             --derived-access-readiness-contract-verify\n\
             --derived-access-readiness-contract-smoke\n\
             --derived-access-contract\n\
             --derived-access-smoke [--derived-access-tier=D0-128|L1|L7] [--derived-access-root=<empty-path>]\n\
                                      [--derived-access-request=<path>]\n\
             --derived-access-lifecycle --derived-access-request=<path>\n\
             --derived-access-retained-preflight --derived-access-request=<path>\n\
             --derived-access-retained-bootstrap --derived-access-request=<path>\n\
             --derived-access-scale-evidence --derived-access-request=<path>\n\
             --derived-access-resource-evidence --derived-access-request=<path>\n\
             --derived-access-fragment --derived-access-request=<path>\n\
             --derived-access-package --derived-access-input=<path> ...\n\
             --derived-access-verify-package --derived-access-package-root=<path>\n\
             \n\
             Retained L7, L100, and C262 modes consume verified admitted roots and never materialize them.\n\
             Scale evidence remains limited to L100 and C262."
        );
        return ExitCode::SUCCESS;
    }

    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_DERIVED_ACCESS_SMOKE_MODE_V1)
    {
        let requests = arguments
            .iter()
            .filter_map(|argument| argument.strip_prefix("--derived-access-request="))
            .collect::<Vec<_>>();
        let requested_tier = arguments
            .iter()
            .find_map(|argument| argument.strip_prefix("--derived-access-tier="))
            .unwrap_or("D0-128");
        let roots = arguments
            .iter()
            .filter_map(|argument| argument.strip_prefix("--derived-access-root="))
            .collect::<Vec<_>>();
        if requests.len() > 1
            || (!requests.is_empty()
                && (!roots.is_empty()
                    || arguments
                        .iter()
                        .any(|argument| argument.starts_with("--derived-access-tier="))))
        {
            eprintln!("typed native smoke requests cannot be combined with tier/root flags");
            return ExitCode::from(2);
        }
        if let Some(request) = requests.first() {
            return report_derived_access_smoke(run_qualification_derived_access_native_smoke_v1(
                std::path::Path::new(request),
            ));
        }
        if roots.len() > 1 {
            eprintln!("a derived-access smoke root is supported only once");
            return ExitCode::from(2);
        }
        return match requested_tier {
            "D0-128" => report_derived_access_smoke(if let Some(root) = roots.first() {
                run_qualification_derived_access_non_timing_smoke_at_v1(std::path::Path::new(root))
            } else {
                run_qualification_derived_access_non_timing_smoke_v1()
            }),
            "L1" => report_derived_access_smoke(if let Some(root) = roots.first() {
                run_qualification_derived_access_longitudinal_smoke_at_v1(
                    QualificationDerivedAccessTierV1::L1,
                    std::path::Path::new(root),
                )
            } else {
                run_qualification_derived_access_longitudinal_smoke_v1(
                    QualificationDerivedAccessTierV1::L1,
                )
            }),
            "L7" => report_derived_access_smoke(if let Some(root) = roots.first() {
                run_qualification_derived_access_longitudinal_smoke_at_v1(
                    QualificationDerivedAccessTierV1::L7,
                    std::path::Path::new(root),
                )
            } else {
                run_qualification_derived_access_longitudinal_smoke_v1(
                    QualificationDerivedAccessTierV1::L7,
                )
            }),
            _ => {
                eprintln!("derived-access smoke tier must be D0-128, L1, or L7");
                ExitCode::from(2)
            }
        };
    }

    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_DERIVED_ACCESS_VERIFY_PACKAGE_MODE_V1)
    {
        let roots = arguments
            .iter()
            .filter_map(|argument| argument.strip_prefix("--derived-access-package-root="))
            .collect::<Vec<_>>();
        if roots.len() != 1 {
            eprintln!(
                "derived-access package verification requires exactly one --derived-access-package-root"
            );
            return ExitCode::from(2);
        }
        return match verify_qualification_derived_access_package_v1(std::path::Path::new(roots[0]))
        {
            Ok(evaluation) => {
                println!(
                    "{}",
                    serde_json::to_string(&evaluation)
                        .expect("derived-access evaluation serializes")
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("store foundation derived-access package failed: {error}");
                ExitCode::from(1)
            }
        };
    }

    let derived_requests = arguments
        .iter()
        .filter_map(|argument| argument.strip_prefix("--derived-access-request="))
        .collect::<Vec<_>>();
    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_DERIVED_ACCESS_RETAINED_PREFLIGHT_MODE_V1)
    {
        if derived_requests.len() != 1 {
            eprintln!("derived-access retained preflight requires exactly one typed request");
            return ExitCode::from(2);
        }
        return report_derived_access_smoke(
            preflight_qualification_derived_access_retained_root_v1(std::path::Path::new(
                derived_requests[0],
            )),
        );
    }
    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_DERIVED_ACCESS_RETAINED_BOOTSTRAP_MODE_V1)
    {
        if derived_requests.len() != 1 {
            eprintln!("derived-access retained bootstrap requires exactly one typed request");
            return ExitCode::from(2);
        }
        return report_derived_access_smoke(
            bootstrap_qualification_derived_access_retained_root_v1(std::path::Path::new(
                derived_requests[0],
            )),
        );
    }
    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_DERIVED_ACCESS_FRAGMENT_MODE_V1)
    {
        if derived_requests.len() != 1 {
            eprintln!("derived-access fragment requires exactly one typed request");
            return ExitCode::from(2);
        }
        return report_derived_access_smoke(build_qualification_derived_access_fragment_v1(
            std::path::Path::new(derived_requests[0]),
        ));
    }
    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_DERIVED_ACCESS_PACKAGE_MODE_V1)
    {
        let inputs = arguments
            .iter()
            .filter_map(|argument| argument.strip_prefix("--derived-access-input="))
            .map(std::path::PathBuf::from)
            .collect::<Vec<_>>();
        let roots = arguments
            .iter()
            .filter_map(|argument| argument.strip_prefix("--derived-access-package-root="))
            .collect::<Vec<_>>();
        if inputs.is_empty() || roots.len() != 1 {
            eprintln!(
                "derived-access package assembly requires inputs and one package output root"
            );
            return ExitCode::from(2);
        }
        return report_derived_access_smoke(assemble_qualification_derived_access_package_v1(
            &inputs,
            std::path::Path::new(roots[0]),
        ));
    }
    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_DERIVED_ACCESS_LIFECYCLE_MODE_V1)
    {
        if derived_requests.len() != 1 {
            eprintln!("derived-access lifecycle requires exactly one typed request");
            return ExitCode::from(2);
        }
        return report_derived_access_smoke(run_qualification_derived_access_lifecycle_v1(
            std::path::Path::new(derived_requests[0]),
        ));
    }
    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_DERIVED_ACCESS_SCALE_MODE_V1)
    {
        if derived_requests.len() != 1 {
            eprintln!("derived-access scale evidence requires exactly one typed request");
            return ExitCode::from(2);
        }
        return report_derived_access_smoke(run_qualification_derived_access_scale_v1(
            std::path::Path::new(derived_requests[0]),
        ));
    }
    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_DERIVED_ACCESS_RESOURCE_MODE_V1)
    {
        if derived_requests.len() != 1 {
            eprintln!("derived-access resource evidence requires exactly one typed request");
            return ExitCode::from(2);
        }
        return report_derived_access_smoke(run_qualification_derived_access_resource_v1(
            std::path::Path::new(derived_requests[0]),
        ));
    }

    if arguments
        .iter()
        .any(|argument| argument == LONGITUDINAL_HELP_MODE_V1)
    {
        println!(
            "{}",
            serde_json::to_string(&longitudinal_help_v1()).expect("longitudinal help serializes")
        );
        return ExitCode::SUCCESS;
    }

    if arguments
        .iter()
        .any(|argument| argument == LONGITUDINAL_SMOKE_MODE_V1)
    {
        return match longitudinal_non_timing_smoke_v1() {
            Ok(receipt) => {
                println!(
                    "{}",
                    serde_json::to_string(&receipt).expect("longitudinal smoke receipt serializes")
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("store foundation longitudinal smoke failed: {error}");
                ExitCode::from(1)
            }
        };
    }

    if arguments
        .iter()
        .any(|argument| argument == LONGITUDINAL_CARRY_FORWARD_SMOKE_MODE_V1)
    {
        return match longitudinal_carry_forward_non_timing_smoke_v1() {
            Ok(receipt) => {
                println!(
                    "{}",
                    serde_json::to_string(&receipt)
                        .expect("longitudinal carry-forward smoke receipt serializes")
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("store foundation longitudinal carry-forward smoke failed: {error}");
                ExitCode::from(1)
            }
        };
    }

    if longitudinal_carry_forward_requested {
        return longitudinal_carry_forward_report(std::path::Path::new(
            longitudinal_carry_forward_requests[0],
        ));
    }

    if longitudinal_package_requested {
        return longitudinal_package_report(std::path::Path::new(longitudinal_package_roots[0]));
    }

    if longitudinal_package_receipt_requested {
        return longitudinal_package_receipt_report(std::path::Path::new(
            longitudinal_package_roots[0],
        ));
    }

    if longitudinal_authority_verify_requested {
        return longitudinal_carry_forward_authority_report(
            std::path::Path::new(longitudinal_authority_packages[0]),
            std::path::Path::new(longitudinal_package_roots[0]),
        );
    }

    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_LOOSE_BASELINE_SMOKE_MODE_V1)
    {
        return qualification_loose_baseline_smoke_report();
    }

    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_LOOSE_BASELINE_EVIDENCE_MODE_V1)
    {
        return qualification_loose_baseline_evidence_report();
    }

    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_PROSPECTIVE_CONTRACT_PUBLICATION_MODE_V1)
    {
        println!(
            "{}",
            serde_json::to_string(&qualification_prospective_contract_v1_publication())
                .expect("prospective contract publication serializes")
        );
        return ExitCode::SUCCESS;
    }

    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_CONTENT_ONLY_CONTRACT_PUBLICATION_MODE_V1)
    {
        println!(
            "{}",
            serde_json::to_string(&qualification_content_only_contract_v1_publication())
                .expect("content-only contract publication serializes")
        );
        return ExitCode::SUCCESS;
    }

    for (mode, run) in [
        (
            DERIVED_ACCESS_PRODUCT_CONTRACT_MODE_V1,
            derived_access_product_contract_publication_json_v1 as fn() -> Result<String, String>,
        ),
        (
            DERIVED_ACCESS_PRODUCT_CONTRACT_VERIFY_MODE_V1,
            derived_access_product_contract_verify_json_v1,
        ),
        (
            DERIVED_ACCESS_PRODUCT_CONTRACT_SMOKE_MODE_V1,
            derived_access_product_contract_smoke_json_v1,
        ),
        (
            DERIVED_ACCESS_READINESS_CONTRACT_MODE_V1,
            derived_access_readiness_contract_publication_json_v1,
        ),
        (
            DERIVED_ACCESS_READINESS_CONTRACT_VERIFY_MODE_V1,
            derived_access_readiness_contract_verify_json_v1,
        ),
        (
            DERIVED_ACCESS_READINESS_CONTRACT_SMOKE_MODE_V1,
            derived_access_readiness_contract_smoke_json_v1,
        ),
    ] {
        if arguments.iter().any(|argument| argument == mode) {
            return match run() {
                Ok(json) => {
                    println!("{json}");
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("store foundation product-integration contract failed: {error}");
                    ExitCode::from(1)
                }
            };
        }
    }

    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_DERIVED_ACCESS_CONTRACT_MODE_V1)
    {
        let publication = qualification_derived_access_contract_v1_publication();
        if let Err(error) = publication.contract.validate() {
            eprintln!("store foundation derived-access contract failed: {error}");
            return ExitCode::from(1);
        }
        println!(
            "{}",
            serde_json::to_string(&publication)
                .expect("derived-access contract publication serializes")
        );
        return ExitCode::SUCCESS;
    }

    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_LMDB_PROSPECTIVE_SMOKE_MODE_V1)
    {
        return lmdb_prospective_smoke_report();
    }

    if arguments
        .iter()
        .any(|argument| argument == QUALIFICATION_LMDB_PROSPECTIVE_EVIDENCE_MODE_V1)
    {
        return lmdb_prospective_evidence_report();
    }

    if lmdb_prospective_package_requested {
        return lmdb_prospective_package_report(&lmdb_prospective_inputs);
    }

    if arguments
        .iter()
        .any(|argument| argument == "--qualification-smoke")
    {
        return qualification_report(1);
    }

    if arguments
        .iter()
        .any(|argument| argument == "--qualification-evidence")
    {
        return qualification_report(5);
    }

    if diagnostics_requested {
        return qualification_diagnostics_report(
            diagnostic_pair_order.unwrap_or(QualificationPerformancePairOrderV1::Alternating),
        );
    }

    if arguments
        .iter()
        .any(|argument| argument == "--qualification-final-evidence")
    {
        return qualification_final_evidence_report();
    }

    if package_requested {
        return qualification_performance_package_report(&package_inputs);
    }

    if arguments
        .iter()
        .any(|argument| argument == "--qualification-contract")
    {
        println!(
            "{}",
            serde_json::to_string(&qualification_performance_contract_v2_publication())
                .expect("qualification contract publication serializes")
        );
        return ExitCode::SUCCESS;
    }

    if arguments
        .iter()
        .any(|argument| argument == "--transfer-smoke")
    {
        return match transfer_smoke_metadata() {
            Ok(metadata) => {
                println!(
                    "{}",
                    serde_json::to_string(&metadata).expect("transfer smoke metadata serializes")
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("store foundation transfer smoke failed: {error}");
                ExitCode::from(1)
            }
        };
    }

    if arguments
        .iter()
        .any(|argument| argument == "--sqlite-smoke")
    {
        return match sqlite_smoke_metadata() {
            Ok(metadata) => {
                println!(
                    "{}",
                    serde_json::to_string(&metadata).expect("SQLite smoke metadata serializes")
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("store foundation SQLite smoke failed: {error}");
                ExitCode::from(1)
            }
        };
    }

    if arguments
        .iter()
        .any(|argument| argument == "--segments-smoke")
    {
        return match segment_smoke_metadata() {
            Ok(metadata) => {
                println!(
                    "{}",
                    serde_json::to_string(&metadata).expect("segment smoke metadata serializes")
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("store foundation segment smoke failed: {error}");
                ExitCode::from(1)
            }
        };
    }

    if arguments
        .iter()
        .any(|argument| argument == "--lmdb-proof-open-close")
    {
        return lmdb_proof_open_close_report();
    }

    if arguments.iter().any(|argument| argument == "--lmdb-smoke") {
        return lmdb_smoke_report();
    }

    if arguments
        .iter()
        .any(|argument| argument == "--lmdb-lifecycle-smoke")
    {
        return lmdb_lifecycle_smoke_report();
    }

    match smoke_metadata() {
        Ok((metadata, external_is_valid)) => {
            println!(
                "{}",
                serde_json::to_string(&metadata).expect("smoke metadata serializes")
            );
            if external_is_valid {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            }
        }
        Err(error) => {
            eprintln!("store foundation smoke failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn report_derived_access_smoke<T: Serialize>(result: Result<T, String>) -> ExitCode {
    match result {
        Ok(receipt) => {
            println!(
                "{}",
                serde_json::to_string(&receipt).expect("derived-access smoke receipt serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("store foundation derived-access smoke failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn longitudinal_package_report(root: &std::path::Path) -> ExitCode {
    let workload = root.join(LONGITUDINAL_EVIDENCE_PACKAGE_FILE_V1).is_file();
    let capacity = root.join(LONGITUDINAL_CAPACITY_PACKAGE_FILE_V1).is_file();
    match (workload, capacity) {
        (true, false) => match verify_longitudinal_evidence_package_v1(root) {
            Ok(package) => {
                println!(
                    "{}",
                    serde_json::to_string(&package)
                        .expect("longitudinal evidence package serializes")
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("store foundation longitudinal package failed: {error}");
                ExitCode::from(1)
            }
        },
        (false, true) => match verify_longitudinal_capacity_package_v1(root) {
            Ok(package) => {
                println!(
                    "{}",
                    serde_json::to_string(&package)
                        .expect("longitudinal capacity package serializes")
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("store foundation longitudinal package failed: {error}");
                ExitCode::from(1)
            }
        },
        _ => {
            eprintln!(
                "store foundation longitudinal package requires exactly one recognized package document"
            );
            ExitCode::from(2)
        }
    }
}

fn longitudinal_carry_forward_report(request_path: &std::path::Path) -> ExitCode {
    let request = match std::fs::read(request_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LongitudinalCarryForwardRequestV1>(&bytes).ok())
    {
        Some(request) => request,
        None => {
            eprintln!("store foundation longitudinal carry-forward request is invalid");
            return ExitCode::from(2);
        }
    };
    match request.execute() {
        Ok(artifacts) => {
            println!(
                "{}",
                serde_json::to_string(&artifacts)
                    .expect("longitudinal carry-forward artifacts serialize")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("store foundation longitudinal carry-forward failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn longitudinal_package_receipt_report(root: &std::path::Path) -> ExitCode {
    match verify_longitudinal_package_receipt_v1(root) {
        Ok(receipt) => {
            println!(
                "{}",
                serde_json::to_string(&receipt)
                    .expect("longitudinal package verification receipt serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("store foundation longitudinal package receipt failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn longitudinal_carry_forward_authority_report(
    authority_package: &std::path::Path,
    workload_package_root: &std::path::Path,
) -> ExitCode {
    match verify_longitudinal_carry_forward_authority_package_v1(
        authority_package,
        workload_package_root,
    ) {
        Ok(package) => {
            println!(
                "{}",
                serde_json::to_string(&package)
                    .expect("longitudinal carry-forward authority package serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("store foundation longitudinal carry-forward authority failed: {error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(feature = "lmdb-proof")]
fn lmdb_proof_open_close_report() -> ExitCode {
    let disposable = match tempfile::tempdir() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("LMDB proof failed to create a disposable root: {error}");
            return ExitCode::from(1);
        }
    };
    match run_lmdb_proof_open_close_v1(disposable.path()) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string(&report).expect("LMDB proof report serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("LMDB proof open/close failed: {error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(feature = "lmdb-proof")]
fn lmdb_smoke_report() -> ExitCode {
    let disposable = match tempfile::tempdir() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("plain LMDB smoke failed to create a disposable root: {error}");
            return ExitCode::from(1);
        }
    };
    match run_qualification_lmdb_smoke_v1(disposable.path()) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string(&report).expect("plain LMDB smoke report serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("plain LMDB smoke failed: {error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(feature = "lmdb-proof"))]
fn lmdb_smoke_report() -> ExitCode {
    eprintln!("--lmdb-smoke requires --features bench,lmdb-proof");
    ExitCode::from(2)
}

#[cfg(feature = "lmdb-proof")]
fn lmdb_lifecycle_smoke_report() -> ExitCode {
    let disposable = match tempfile::tempdir() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("plain LMDB lifecycle smoke failed to create a disposable root: {error}");
            return ExitCode::from(1);
        }
    };
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            eprintln!("plain LMDB lifecycle smoke could not resolve its executable: {error}");
            return ExitCode::from(1);
        }
    };
    match run_qualification_lmdb_lifecycle_smoke_v1(&executable, disposable.path()) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string(&report)
                    .expect("plain LMDB lifecycle smoke report serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("plain LMDB lifecycle smoke failed: {error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(feature = "lmdb-proof"))]
fn lmdb_lifecycle_smoke_report() -> ExitCode {
    eprintln!("--lmdb-lifecycle-smoke requires --features bench,lmdb-proof");
    ExitCode::from(2)
}

#[cfg(feature = "lmdb-proof")]
fn lmdb_prospective_smoke_report() -> ExitCode {
    let disposable = match tempfile::tempdir() {
        Ok(root) => root,
        Err(_) => {
            eprintln!("plain LMDB prospective smoke could not create a disposable root");
            return ExitCode::from(1);
        }
    };
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(_) => {
            eprintln!("plain LMDB prospective smoke could not resolve its executable");
            return ExitCode::from(1);
        }
    };
    match run_qualification_lmdb_prospective_smoke_v1(
        &executable,
        &disposable.path().join("lmdb-prospective-smoke"),
    ) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string(&report)
                    .expect("plain LMDB prospective smoke report serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("plain LMDB prospective smoke failed: {error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(feature = "lmdb-proof"))]
fn lmdb_prospective_smoke_report() -> ExitCode {
    eprintln!("--lmdb-prospective-smoke requires --features bench,lmdb-proof");
    ExitCode::from(2)
}

#[cfg(feature = "lmdb-proof")]
fn lmdb_prospective_evidence_report() -> ExitCode {
    let disposable = match tempfile::tempdir() {
        Ok(root) => root,
        Err(_) => {
            eprintln!("plain LMDB prospective evidence could not create a disposable root");
            return ExitCode::from(1);
        }
    };
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(_) => {
            eprintln!("plain LMDB prospective evidence could not resolve its executable");
            return ExitCode::from(1);
        }
    };
    let execution = match qualification_lmdb_prospective_execution_v1() {
        Ok(execution) => execution,
        Err(error) => {
            eprintln!("plain LMDB prospective evidence provenance failed: {error}");
            return ExitCode::from(1);
        }
    };
    let configuration = QualificationLmdbProspectiveEvidenceConfigurationV1 {
        executable,
        root: disposable.path().join("lmdb-prospective-evidence"),
        execution,
        quiesced_host: std::env::var("POINTBREAK_QUALIFICATION_QUIESCED")
            .is_ok_and(|value| value == "1"),
    };
    match run_qualification_lmdb_prospective_evidence_v1(&configuration) {
        Ok(shard) => {
            println!(
                "{}",
                serde_json::to_string(&shard).expect("plain LMDB prospective evidence serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("plain LMDB prospective evidence failed: {error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(feature = "lmdb-proof"))]
fn lmdb_prospective_evidence_report() -> ExitCode {
    eprintln!("--lmdb-prospective-evidence requires --features bench,lmdb-proof");
    ExitCode::from(2)
}

#[cfg(feature = "lmdb-proof")]
fn lmdb_prospective_package_report(inputs: &[&str]) -> ExitCode {
    let mut shards = Vec::new();
    for input in inputs {
        let bytes = match std::fs::read(input) {
            Ok(bytes) => bytes,
            Err(_) => {
                eprintln!("plain LMDB prospective package input could not be read");
                return ExitCode::from(1);
            }
        };
        match parse_qualification_lmdb_prospective_shard_v1(&bytes) {
            Ok(shard) => shards.push(shard),
            Err(error) => {
                eprintln!("plain LMDB prospective package input failed validation: {error}");
                return ExitCode::from(1);
            }
        }
    }
    let execution = match qualification_lmdb_prospective_execution_v1() {
        Ok(execution) => execution,
        Err(error) => {
            eprintln!("plain LMDB prospective package provenance failed: {error}");
            return ExitCode::from(1);
        }
    };
    match QualificationLmdbProspectivePackageV1::assemble_for_execution(&shards, &execution) {
        Ok(package) => {
            println!(
                "{}",
                serde_json::to_string(&package).expect("plain LMDB prospective package serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("plain LMDB prospective package failed: {error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(feature = "lmdb-proof"))]
fn lmdb_prospective_package_report(_inputs: &[&str]) -> ExitCode {
    eprintln!("--lmdb-prospective-package requires --features bench,lmdb-proof");
    ExitCode::from(2)
}

#[cfg(not(feature = "lmdb-proof"))]
fn lmdb_proof_open_close_report() -> ExitCode {
    eprintln!("--lmdb-proof-open-close requires --features bench,lmdb-proof");
    ExitCode::from(2)
}

fn qualification_loose_baseline_smoke_report() -> ExitCode {
    let disposable = match tempfile::tempdir() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("store foundation loose baseline smoke root failed: {error}");
            return ExitCode::from(1);
        }
    };
    let configuration = QualificationLooseBaselineSmokeConfigurationV1 {
        executable: match std::env::current_exe() {
            Ok(executable) => executable,
            Err(error) => {
                eprintln!("store foundation loose baseline smoke executable failed: {error}");
                return ExitCode::from(1);
            }
        },
        root: disposable.path().join("loose-baseline-smoke"),
    };
    match run_qualification_loose_baseline_smoke_v1(&configuration) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string(&report).expect("loose baseline smoke serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("store foundation loose baseline smoke failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn qualification_loose_baseline_evidence_report() -> ExitCode {
    let disposable = match tempfile::tempdir() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("store foundation loose baseline evidence root failed: {error}");
            return ExitCode::from(1);
        }
    };
    let source_commit = match qualification_source_commit() {
        Ok(commit) => commit,
        Err(error) => {
            eprintln!("store foundation loose baseline provenance failed: {error}");
            return ExitCode::from(1);
        }
    };
    let configuration = QualificationLooseBaselineEvidenceConfigurationV1 {
        executable: match std::env::current_exe() {
            Ok(executable) => executable,
            Err(error) => {
                eprintln!("store foundation loose baseline evidence executable failed: {error}");
                return ExitCode::from(1);
            }
        },
        root: disposable.path().join("loose-baseline-evidence"),
        source_commit,
        cargo_lock_sha256: qualification_cargo_lock_sha256(),
        quiesced_host: std::env::var("POINTBREAK_QUALIFICATION_QUIESCED")
            .is_ok_and(|value| value == "1"),
    };
    match run_qualification_loose_baseline_evidence_v1(&configuration) {
        Ok(evidence) => {
            println!(
                "{}",
                serde_json::to_string(&evidence).expect("loose baseline evidence serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("store foundation loose baseline evidence failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn qualification_final_evidence_report() -> ExitCode {
    let disposable = match tempfile::tempdir() {
        Ok(root) => root,
        Err(error) => {
            eprintln!(
                "store foundation final evidence failed to create a disposable root: {error}"
            );
            return ExitCode::from(1);
        }
    };
    let source_commit = match qualification_source_commit() {
        Ok(commit) => commit,
        Err(error) => {
            eprintln!("store foundation final evidence provenance failed: {error}");
            return ExitCode::from(1);
        }
    };
    let configuration = QualificationPerformanceCampaignConfigurationV2 {
        executable: match std::env::current_exe() {
            Ok(executable) => executable,
            Err(error) => {
                eprintln!("store foundation final evidence executable lookup failed: {error}");
                return ExitCode::from(1);
            }
        },
        root: disposable.path().join("performance-campaign"),
        source_commit,
        cargo_lock_sha256: qualification_cargo_lock_sha256(),
        external_corpus_root: std::env::var_os("POINTBREAK_QUALIFICATION_CORPUS")
            .map(std::path::PathBuf::from),
        quiesced_host: std::env::var("POINTBREAK_QUALIFICATION_QUIESCED")
            .is_ok_and(|value| value == "1"),
    };
    match run_qualification_performance_campaign_v2(&configuration) {
        Ok(evidence) => {
            println!(
                "{}",
                serde_json::to_string(&evidence).expect("final performance evidence serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("store foundation final evidence failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn qualification_performance_package_report(inputs: &[&str]) -> ExitCode {
    let mut shards = Vec::new();
    for input in inputs {
        let bytes = match std::fs::read(input) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("store foundation performance package input failed: {error}");
                return ExitCode::from(1);
            }
        };
        match serde_json::from_slice::<QualificationPerformanceEvidenceV2>(&bytes) {
            Ok(shard) => shards.push(shard),
            Err(error) => {
                eprintln!("store foundation performance package input is invalid: {error}");
                return ExitCode::from(1);
            }
        }
    }
    match QualificationPerformancePackageV2::assemble(&shards) {
        Ok(package) => {
            println!(
                "{}",
                serde_json::to_string(&package).expect("performance package serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("store foundation performance package failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn qualification_report(performance_samples: u32) -> ExitCode {
    let disposable = match tempfile::tempdir() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("store foundation qualification failed to create a disposable root: {error}");
            return ExitCode::from(1);
        }
    };
    let source_commit = match qualification_source_commit() {
        Ok(commit) => commit,
        Err(error) => {
            eprintln!("store foundation qualification provenance failed: {error}");
            return ExitCode::from(1);
        }
    };
    let configuration = QualificationRunConfigurationV1 {
        executable: match std::env::current_exe() {
            Ok(executable) => executable,
            Err(error) => {
                eprintln!("store foundation qualification executable lookup failed: {error}");
                return ExitCode::from(1);
            }
        },
        root: disposable.path().join("qualification-run"),
        source_commit,
        cargo_lock_sha256: qualification_cargo_lock_sha256(),
        performance_samples,
    };
    match run_qualification_platform_matrix(&configuration) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string(&report).expect("qualification report serializes")
            );
            if report.completeness.all_results_passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            }
        }
        Err(error) => {
            eprintln!("store foundation qualification failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn qualification_pair_order(
    arguments: &[String],
) -> Result<Option<QualificationPerformancePairOrderV1>, ()> {
    let values = arguments
        .iter()
        .filter_map(|argument| argument.strip_prefix("--qualification-pair-order="))
        .collect::<Vec<_>>();
    if values.len() > 1 {
        return Err(());
    }
    values
        .first()
        .map(|value| match *value {
            "alternating" => Ok(QualificationPerformancePairOrderV1::Alternating),
            "candidate_then_baseline" => {
                Ok(QualificationPerformancePairOrderV1::CandidateThenBaseline)
            }
            "baseline_then_candidate" => {
                Ok(QualificationPerformancePairOrderV1::BaselineThenCandidate)
            }
            _ => Err(()),
        })
        .transpose()
}

fn qualification_diagnostics_report(pair_order: QualificationPerformancePairOrderV1) -> ExitCode {
    let disposable = match tempfile::tempdir() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("store foundation diagnostics failed to create a disposable root: {error}");
            return ExitCode::from(1);
        }
    };
    let source_commit = match qualification_source_commit() {
        Ok(commit) => commit,
        Err(error) => {
            eprintln!("store foundation diagnostics provenance failed: {error}");
            return ExitCode::from(1);
        }
    };
    let configuration = QualificationPerformanceDiagnosticConfigurationV1 {
        executable: match std::env::current_exe() {
            Ok(executable) => executable,
            Err(error) => {
                eprintln!("store foundation diagnostics executable lookup failed: {error}");
                return ExitCode::from(1);
            }
        },
        root: disposable.path().join("performance-diagnostics"),
        source_commit,
        cargo_lock_sha256: qualification_cargo_lock_sha256(),
        warmup_samples: 3,
        measured_samples: 21,
        pair_order,
        external_corpus_root: std::env::var_os("POINTBREAK_QUALIFICATION_CORPUS")
            .map(std::path::PathBuf::from),
    };
    match run_qualification_performance_diagnostics(&configuration) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string(&report).expect("performance diagnostics serialize")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("store foundation diagnostics failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn sqlite_smoke_metadata() -> Result<SqliteSmokeMetadataV1, String> {
    let roots = tempfile::tempdir().map_err(|error| error.to_string())?;
    let legacy = synthetic_legacy_manifest().map_err(|error| error.to_string())?;
    let modeled = modeled_post_foundation_manifest().map_err(|error| error.to_string())?;
    let workloads = vec![
        run_sqlite_workload(&roots.path().join("synthetic-legacy"), &legacy)?,
        run_sqlite_workload(&roots.path().join("modeled-foundation"), &modeled)?,
    ];
    Ok(SqliteSmokeMetadataV1 {
        schema: "pointbreak.store-foundation-sqlite-smoke.v1",
        mode: "non_timing_sqlite_qualification",
        workloads,
    })
}

fn segment_smoke_metadata() -> Result<SegmentSmokeMetadataV1, String> {
    let roots = tempfile::tempdir().map_err(|error| error.to_string())?;
    let legacy = synthetic_legacy_manifest().map_err(|error| error.to_string())?;
    let modeled = modeled_post_foundation_manifest().map_err(|error| error.to_string())?;
    let workloads = vec![
        run_segment_workload(&roots.path().join("synthetic-legacy"), &legacy)?,
        run_segment_workload(&roots.path().join("modeled-foundation"), &modeled)?,
    ];
    Ok(SegmentSmokeMetadataV1 {
        schema: "pointbreak.store-foundation-segment-smoke.v1",
        mode: "non_timing_segment_qualification",
        workloads,
    })
}

fn transfer_smoke_metadata() -> Result<TransferSmokeMetadataV1, String> {
    let workload = modeled_post_foundation_manifest().map_err(|error| error.to_string())?;
    let manifest = ExactBundleManifestV2::new(
        workload.manifest_sha256,
        LogicalCapabilityEpochV1::foundation(),
        workload.records,
        vec![
            ExactBundleClosureV2 {
                event_logical_key: "events/000-root.json".to_owned(),
                required_content_keys: vec![
                    "artifacts/documents/blob-guide.md".to_owned(),
                    "artifacts/documents/manifest-guide.json".to_owned(),
                    "artifacts/objects/round-001.json".to_owned(),
                ],
            },
            ExactBundleClosureV2 {
                event_logical_key: "events/003-attestation-verified.json".to_owned(),
                required_content_keys: vec!["artifacts/proofs/relation-001.json".to_owned()],
            },
        ],
    )
    .map_err(|error| error.to_string())?;
    let mut destination =
        DisposableBundleDestinationV2::new(LogicalCapabilityEpochV1::foundation());
    match publish_exact_bundle_v2(
        &mut destination,
        &manifest,
        ExactBundleFailurePointV2::BeforeFirstEvent,
    ) {
        Err(pointbreak::bench_support::foundation::ExactBundleError::InjectedBeforeFirstEvent) => {}
        result => {
            return Err(format!(
                "unexpected interrupted publication result: {result:?}"
            ));
        }
    }
    let interrupted_publication = InterruptedPublicationMetadataV1 {
        content_count: destination.content_count(),
        event_count: destination.event_count(),
    };
    let completion =
        publish_exact_bundle_v2(&mut destination, &manifest, ExactBundleFailurePointV2::None)
            .map_err(|error| error.to_string())?;
    let idempotent_retry =
        publish_exact_bundle_v2(&mut destination, &manifest, ExactBundleFailurePointV2::None)
            .map_err(|error| error.to_string())?;

    let exact_bytes_verified = manifest
        .events
        .iter()
        .chain(&manifest.content)
        .all(|record| {
            destination
                .record(&record.logical_key)
                .is_some_and(|stored| {
                    stored.decoded_sha256 == record.decoded_sha256
                        && stored.decoded_bytes == record.decoded_bytes
                })
        });
    if !exact_bytes_verified {
        return Err("destination bytes differ from the selected manifest".to_owned());
    }

    let receipt_alternatives = [
        ImportReceiptPolicyPrototypeV1::DurableOperational,
        ImportReceiptPolicyPrototypeV1::LocalProvenanceEvent,
    ]
    .into_iter()
    .map(|policy| {
        let receipt = ImportReceiptPrototypeV1::new(policy, &manifest, "transfer-smoke")
            .map_err(|error| error.to_string())?;
        Ok(ReceiptAlternativeMetadataV1 {
            policy,
            receipt_sha256: receipt.receipt_sha256.clone(),
            projection: receipt.projection_consequence(),
            backup: receipt.backup_consequence(),
            emits_local_provenance_event: receipt.local_provenance_event().is_some(),
        })
    })
    .collect::<Result<Vec<_>, String>>()?;

    Ok(TransferSmokeMetadataV1 {
        schema: "pointbreak.store-foundation-transfer-smoke.v1",
        mode: "non_timing_exact_transfer",
        bundle_sha256: manifest.bundle_sha256,
        event_set_sha256: manifest.event_set_sha256,
        event_count: manifest.events.len(),
        content_count: manifest.content.len(),
        closure_count: manifest.closure.len(),
        interrupted_publication,
        completion,
        idempotent_retry,
        exact_bytes_verified,
        receipt_alternatives,
    })
}

fn smoke_metadata() -> Result<(SmokeMetadataV1, bool), QualificationCorpusError> {
    let synthetic_legacy = synthetic_legacy_manifest()?;
    let modeled_workload = modeled_post_foundation_manifest()?;
    let capabilities = LogicalCapabilityEpochV1::foundation();
    capabilities.validate()?;

    let external_corpus_configured = std::env::var_os("POINTBREAK_QUALIFICATION_CORPUS").is_some();
    let (external_legacy, external_is_valid) = if external_corpus_configured {
        match load_external_workload_v2_manifest_from_env() {
            Ok(manifest) => (
                ExternalCorpusMetadataV1::Validated {
                    summary: QualificationCorpusSummaryV1::from_manifest(&manifest),
                    snapshot: QualificationSnapshotTotalsV1::external_v2(),
                },
                true,
            ),
            Err(QualificationCorpusError::SnapshotDrift(report)) => (
                ExternalCorpusMetadataV1::SnapshotDrift { report: *report },
                false,
            ),
            Err(error) => (
                ExternalCorpusMetadataV1::Invalid {
                    message: error.to_string(),
                },
                false,
            ),
        }
    } else {
        (ExternalCorpusMetadataV1::NotConfigured, true)
    };

    Ok((
        SmokeMetadataV1 {
            schema: "pointbreak.store-foundation-smoke.v1",
            build: BuildMetadataV1 {
                package_version: env!("CARGO_PKG_VERSION"),
                source: env!("POINTBREAK_BUILD_SOURCE"),
                commit: match env!("POINTBREAK_BUILD_COMMIT") {
                    "" => None,
                    commit => Some(commit),
                },
                describe: env!("POINTBREAK_BUILD_DESCRIBE"),
                dirty: env!("POINTBREAK_BUILD_DIRTY") == "true",
            },
            dependencies: DependencyMetadataV1 {
                cargo_lock_sha256: sha256_hex(include_bytes!("../Cargo.lock")),
                rustc: rustc_version(),
            },
            runtime: RuntimeMetadataV1 {
                os: std::env::consts::OS,
                architecture: std::env::consts::ARCH,
                filesystem: qualification_filesystem_name(std::path::Path::new(env!(
                    "CARGO_MANIFEST_DIR"
                ))),
            },
            configuration: ConfigurationMetadataV1 {
                mode: "non_timing_smoke",
                external_corpus_variable: "POINTBREAK_QUALIFICATION_CORPUS",
                external_corpus_configured,
            },
            capabilities,
            synthetic_legacy: QualificationCorpusSummaryV1::from_manifest(&synthetic_legacy),
            modeled_workload: QualificationCorpusSummaryV1::from_manifest(&modeled_workload),
            external_legacy,
        },
        external_is_valid,
    ))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|version| version.trim().to_owned())
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| "unavailable".to_owned())
}
