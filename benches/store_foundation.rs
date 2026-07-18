//! Deterministic smoke entry point for durable-store qualification workloads.
//!
//! This target establishes workload identity and metadata only. It does not
//! select or time a storage implementation.

use std::process::{Command, ExitCode};

use pointbreak::bench_support::foundation::{
    LogicalCapabilityEpochV1, QualificationCorpusError, QualificationCorpusSummaryV1,
    QualificationSnapshotTotalsV1, SnapshotDriftReportV1, load_frozen_legacy_manifest_from_env,
    modeled_post_foundation_manifest, qualification_filesystem_name, synthetic_legacy_manifest,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

const USAGE: &str = "\
Usage: cargo bench --features bench --bench store_foundation -- [--smoke|--help]\n\
\n\
Validates deterministic workload manifests and prints one JSON metadata record.\n\
No storage implementation is selected or timed by this target.\n";

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

fn main() -> ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.iter().any(|argument| argument == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if arguments
        .iter()
        .any(|argument| argument != "--smoke" && argument != "--bench")
    {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
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

fn smoke_metadata() -> Result<(SmokeMetadataV1, bool), QualificationCorpusError> {
    let synthetic_legacy = synthetic_legacy_manifest()?;
    let modeled_workload = modeled_post_foundation_manifest()?;
    let capabilities = LogicalCapabilityEpochV1::foundation();
    capabilities.validate()?;

    let external_corpus_configured = std::env::var_os("POINTBREAK_QUALIFICATION_CORPUS").is_some();
    let (external_legacy, external_is_valid) = if external_corpus_configured {
        match load_frozen_legacy_manifest_from_env() {
            Ok(manifest) => (
                ExternalCorpusMetadataV1::Validated {
                    summary: QualificationCorpusSummaryV1::from_manifest(&manifest),
                    snapshot: QualificationSnapshotTotalsV1::frozen_legacy(),
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
