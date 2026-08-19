//! Assemble a derived Change-read qualification request (V2) for one prepared
//! evidence root, recomputing every execution and product identity from the
//! live artifacts and validating the finished request through the exact
//! production validator before writing it.
//!
//! Identity conventions (each one recomputed here, never trusted from the
//! template):
//! - `commandSha256`: sha256 of the compact JSON array of the exact harness
//!   argv, including the harness path as argv[0].
//! - `hostIdentitySha256`: sha256 of the `POINTBREAK_QUALIFICATION_HOST_IDENTITY`
//!   label bytes.
//! - `versionSha256`: sha256 of the canonical (sorted-key, compact) JSON of the
//!   product `version --format json` document, which must attest the exact
//!   clean source commit.
//! - product `buildCommandSha256`: sha256 of the canonical JSON
//!   `{"arguments": [...]}` of the pinned release build arguments.
//! - `rootProvenanceSha256`: sha256 of `"<sha256(authority.json)>:<root path>"`
//!   per root, so each root's provenance binds the per-cycle authority record
//!   and its exact path while staying distinct across roots.
//!
//! The control build-command hashes are carried verbatim from the template
//! request: they are frozen argument-list digests, not per-cycle values.

use std::path::{Path, PathBuf};
use std::process::Command;

use pointbreak::bench_support::derived_access::{
    QUALIFICATION_DERIVED_CHANGE_READ_MODE_V1, QualificationDerivedChangeReadRunRequestV2,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    Ok(sha256_hex(&bytes))
}

/// Sorted-key, compact JSON — the same canonical form the library's
/// `canonical_json_bytes` produces, implemented locally because that helper is
/// crate-private and the ambient `serde_json` map order must not be trusted.
fn canonical_json(value: &Value) -> Result<Vec<u8>, String> {
    fn canonical_value(value: &Value) -> Value {
        match value {
            Value::Array(values) => Value::Array(values.iter().map(canonical_value).collect()),
            Value::Object(object) => {
                let mut keys = object.keys().collect::<Vec<_>>();
                keys.sort_unstable();
                let mut canonical = serde_json::Map::new();
                for key in keys {
                    canonical.insert(key.clone(), canonical_value(&object[key]));
                }
                Value::Object(canonical)
            }
            _ => value.clone(),
        }
    }
    serde_json::to_vec(&canonical_value(value)).map_err(|error| error.to_string())
}

fn git_output(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .map_err(|error| format!("git {arguments:?}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

struct Arguments {
    template: PathBuf,
    evidence_root: PathBuf,
    source_checkout: PathBuf,
    harness: PathBuf,
    product: PathBuf,
    library_control: PathBuf,
    cli_control: PathBuf,
    host_label: String,
}

fn parse_arguments() -> Result<Arguments, String> {
    let mut template = None;
    let mut evidence_root = None;
    let mut source_checkout = None;
    let mut harness = None;
    let mut product = None;
    let mut library_control = None;
    let mut cli_control = None;
    let mut host_label = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(flag) = arguments.next() {
        let mut value = |flag: &str| {
            arguments
                .next()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag.as_str() {
            "--template" => template = Some(PathBuf::from(value("--template")?)),
            "--evidence-root" => evidence_root = Some(PathBuf::from(value("--evidence-root")?)),
            "--source-checkout" => {
                source_checkout = Some(PathBuf::from(value("--source-checkout")?));
            }
            "--harness" => harness = Some(PathBuf::from(value("--harness")?)),
            "--product" => product = Some(PathBuf::from(value("--product")?)),
            "--library-control" => {
                library_control = Some(PathBuf::from(value("--library-control")?));
            }
            "--cli-control" => cli_control = Some(PathBuf::from(value("--cli-control")?)),
            "--host-label" => host_label = Some(value("--host-label")?),
            other => return Err(format!("unknown flag: {other}")),
        }
    }
    Ok(Arguments {
        template: template.ok_or("--template is required")?,
        evidence_root: evidence_root.ok_or("--evidence-root is required")?,
        source_checkout: source_checkout.ok_or("--source-checkout is required")?,
        harness: harness.ok_or("--harness is required")?,
        product: product.ok_or("--product is required")?,
        library_control: library_control.ok_or("--library-control is required")?,
        cli_control: cli_control.ok_or("--cli-control is required")?,
        host_label: host_label.ok_or("--host-label is required")?,
    })
}

fn template_base(template: &Path) -> Result<Value, String> {
    let document: Value = serde_json::from_slice(
        &std::fs::read(template)
            .map_err(|error| format!("read {}: {error}", template.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", template.display()))?;
    match document.get("schema").and_then(Value::as_str) {
        Some("pointbreak.qualification-derived-change-read-request.v1") => Ok(document),
        Some("pointbreak.qualification-derived-change-read-request.v2") => document
            .get("base")
            .cloned()
            .ok_or_else(|| "v2 template omitted its base request".to_owned()),
        other => Err(format!("unsupported template schema: {other:?}")),
    }
}

fn run() -> Result<(), String> {
    let arguments = parse_arguments()?;
    let root = &arguments.evidence_root;
    let checkout = &arguments.source_checkout;
    let output_path = root.join("request.json");

    let template = template_base(&arguments.template)?;
    let template_str = |pointer: &str| -> Result<String, String> {
        template
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("template omitted {pointer}"))
    };

    let source_commit = git_output(checkout, &["rev-parse", "HEAD"])?;
    let source_tree = git_output(checkout, &["rev-parse", "HEAD^{tree}"])?;
    if !git_output(checkout, &["status", "--porcelain=v1"])?.is_empty() {
        return Err(
            "source checkout is dirty; representative requests require clean exact source"
                .to_owned(),
        );
    }
    let cargo_lock_sha256 = sha256_file(&checkout.join("Cargo.lock"))?;

    let version_output = Command::new(&arguments.product)
        .args(["version", "--format", "json"])
        .env_remove("POINTBREAK_QUALIFICATION_CORPUS")
        .output()
        .map_err(|error| format!("run product version: {error}"))?;
    if !version_output.status.success() {
        return Err(format!(
            "product version failed: {}",
            String::from_utf8_lossy(&version_output.stderr)
        ));
    }
    let version: Value = serde_json::from_slice(&version_output.stdout)
        .map_err(|error| format!("product version is invalid JSON: {error}"))?;
    if version.pointer("/build/commit").and_then(Value::as_str) != Some(source_commit.as_str())
        || version.pointer("/build/dirty").and_then(Value::as_bool) != Some(false)
    {
        return Err("product binary does not attest the exact clean source".to_owned());
    }
    let version_sha256 = sha256_hex(&canonical_json(&version)?);

    let authority_sha256 = sha256_file(&root.join("authority.json"))?;
    let repository = root.join("repo");
    let fault_repository = root.join("fault-repo");
    let provenance = |path: &Path| -> Result<String, String> {
        let path = path
            .to_str()
            .ok_or_else(|| "root path is not UTF-8".to_owned())?;
        Ok(sha256_hex(format!("{authority_sha256}:{path}").as_bytes()))
    };
    let reference_provenance = provenance(&repository)?;
    let fault_provenance = provenance(&fault_repository)?;

    let harness_path = arguments
        .harness
        .to_str()
        .ok_or_else(|| "harness path is not UTF-8".to_owned())?
        .to_owned();
    let harness_binary_sha256 = sha256_file(&arguments.harness)?;
    let contract_schema = template
        .pointer("/execution/contractSchema")
        .cloned()
        .ok_or_else(|| "template omitted /execution/contractSchema".to_owned())?;
    let contract_sha256 = template
        .pointer("/execution/contractSha256")
        .cloned()
        .ok_or_else(|| "template omitted /execution/contractSha256".to_owned())?;
    let request_argument = format!("--derived-access-request={}", output_path.display());
    let command_sha256 = sha256_hex(
        &serde_json::to_vec(&vec![
            harness_path.clone(),
            QUALIFICATION_DERIVED_CHANGE_READ_MODE_V1.to_owned(),
            request_argument,
        ])
        .map_err(|error| error.to_string())?,
    );

    let build_command_sha256 = sha256_hex(&canonical_json(&json!({
        "arguments": [
            "+stable",
            "build",
            "--locked",
            "--release",
            "--features",
            "longitudinal-counting",
        ],
    }))?);

    let witness_sha256 = sha256_file(&root.join("witness.json"))?;
    let fault_witness_sha256 = sha256_file(&root.join("fault-witness.json"))?;
    if witness_sha256 != fault_witness_sha256 {
        return Err("fault witness bytes differ from the reference witness".to_owned());
    }
    let seed: Value = serde_json::from_slice(
        &std::fs::read(root.join("seed-receipt.json"))
            .map_err(|error| format!("read seed-receipt.json: {error}"))?,
    )
    .map_err(|error| format!("parse seed-receipt.json: {error}"))?;
    if seed.get("witnessSha256").and_then(Value::as_str) != Some(witness_sha256.as_str()) {
        return Err("fault-seed receipt witness differs from the live witness bytes".to_owned());
    }

    let platform = if cfg!(target_os = "macos") {
        ("macos_apfs", "macos", "apfs")
    } else if cfg!(target_os = "windows") {
        ("windows_ntfs", "windows", "ntfs")
    } else {
        return Err("representative requests run only on macOS or Windows".to_owned());
    };
    let execution = |provenance: &str| {
        json!({
            "platform": platform.0,
            "sourceCommit": source_commit,
            "sourceTree": source_tree,
            "cargoLockSha256": cargo_lock_sha256,
            "binarySha256": harness_binary_sha256,
            "contractSchema": contract_schema,
            "contractSha256": contract_sha256,
            "rootProvenanceSha256": provenance,
            "commandSha256": command_sha256,
            "operatingSystem": platform.1,
            "architecture": std::env::consts::ARCH,
            "filesystem": platform.2,
            "hostIdentitySha256": sha256_hex(arguments.host_label.as_bytes()),
            "sourceDirty": false,
            "privateCorpusConfigured": false,
        })
    };

    let mut probes = template
        .pointer("/storageForbiddenProbes")
        .cloned()
        .ok_or_else(|| "template omitted storageForbiddenProbes".to_owned())?;
    probes["privatePath"] = Value::String(
        repository
            .to_str()
            .ok_or_else(|| "repository path is not UTF-8".to_owned())?
            .to_owned(),
    );

    let request = json!({
        "schema": "pointbreak.qualification-derived-change-read-request.v2",
        "base": {
            "schema": "pointbreak.qualification-derived-change-read-request.v1",
            "purpose": "exact_source_qualification",
            "sourceCheckout": checkout,
            "execution": execution(&reference_provenance),
            "productSourceCheckout": checkout,
            "product": {
                "platform": platform.0,
                "sourceCommit": source_commit,
                "sourceTree": source_tree,
                "cargoLockSha256": cargo_lock_sha256,
                "binarySha256": sha256_file(&arguments.product)?,
                "versionSha256": version_sha256,
                "buildProfile": "release",
                "enabledFeatures": ["longitudinal-counting"],
                "buildCommandSha256": build_command_sha256,
                "operatingSystem": platform.1,
                "architecture": std::env::consts::ARCH,
                "sourceDirty": false,
            },
            "fixture": "topology-v1",
            "fixtureWitness": root.join("witness.json"),
            "fixtureWitnessSha256": witness_sha256,
            "repository": repository,
            "pointbreakHome": repository.join(".git/pointbreak-home"),
            "productBinary": arguments.product,
            "controlTestBinary": arguments.library_control,
            "controlTestBinarySha256": sha256_file(&arguments.library_control)?,
            "controlTestBuildCommandSha256":
                template_str("/controlTestBuildCommandSha256")?,
            "controlCliTestBinary": arguments.cli_control,
            "controlCliTestBinarySha256": sha256_file(&arguments.cli_control)?,
            "controlCliTestBuildCommandSha256":
                template_str("/controlCliTestBuildCommandSha256")?,
            "storageForbiddenProbes": probes,
            "summaryQuery": template_str("/summaryQuery")?,
        },
        "timelineFaultRoot": {
            "repository": fault_repository,
            "pointbreakHome": fault_repository.join(".git/pointbreak-home"),
            "fixtureWitness": root.join("fault-witness.json"),
            "fixtureWitnessSha256": fault_witness_sha256,
            "barrierRoot": root.join("barrier"),
            "execution": execution(&fault_provenance),
            "faultSeedReceipt": seed,
        },
    });

    // Prove the assembled request through the exact production validator
    // (including live root, store-disjointness, barrier, and seed-binding
    // checks) before writing it.
    let typed: QualificationDerivedChangeReadRunRequestV2 = serde_json::from_value(request.clone())
        .map_err(|error| format!("assembled request does not deserialize: {error}"))?;
    typed
        .validate()
        .map_err(|error| format!("assembled request failed validation: {error}"))?;

    // Write artifacts only after the production validator has accepted the
    // assembled request, so a failed assembly leaves no stale files behind.
    std::fs::write(root.join("version.json"), &version_output.stdout)
        .map_err(|error| format!("write version.json: {error}"))?;
    let mut bytes = serde_json::to_vec_pretty(&request).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    std::fs::write(&output_path, &bytes)
        .map_err(|error| format!("write {}: {error}", output_path.display()))?;
    println!("request written: {}", output_path.display());
    println!("request sha256: {}", sha256_hex(&bytes));
    println!("harness argv[0]: {harness_path}");
    println!("reference rootProvenanceSha256: {reference_provenance}");
    println!("fault rootProvenanceSha256: {fault_provenance}");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("request assembly failed: {error}");
        std::process::exit(1);
    }
}
