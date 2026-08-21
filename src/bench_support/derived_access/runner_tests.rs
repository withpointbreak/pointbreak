use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;

#[cfg(feature = "longitudinal-counting")]
use super::adapter::QualificationDerivedAccessAdapter;
#[cfg(feature = "longitudinal-counting")]
use super::sqlite_cursor::{BootstrapControl, CursorLedgerIdentity, SqliteCursorLedger};
use super::*;

fn digest(value: u8) -> String {
    format!("{value:02x}").repeat(32)
}

#[test]
fn diagnostic_harness_identity_is_schema_less_and_build_bound() {
    let host_identity_sha256 = digest(31);
    let identity = super::diagnostic::derived_change_diagnostic_harness_identity_for_test_v1(
        host_identity_sha256.clone(),
    );
    let value = serde_json::to_value(identity).expect("diagnostic harness identity serializes");

    assert_eq!(value["mode"], DERIVED_CHANGE_DIAGNOSTIC_IDENTITY_MODE_V1);
    assert!(value.get("schema").is_none());
    assert_eq!(value["buildSource"], env!("POINTBREAK_BUILD_SOURCE"));
    assert_eq!(value["sourceCommit"], env!("POINTBREAK_BUILD_COMMIT"));
    assert_eq!(
        value["hostIdentitySha256"]
            .as_str()
            .expect("host identity hash"),
        host_identity_sha256
    );
}

fn product_identity(
    execution: &QualificationDerivedAccessExecutionIdentityV1,
) -> QualificationDerivedAccessProductIdentityV1 {
    QualificationDerivedAccessProductIdentityV1 {
        platform: execution.platform,
        source_commit: execution.source_commit.clone(),
        source_tree: execution.source_tree.clone(),
        cargo_lock_sha256: execution.cargo_lock_sha256.clone(),
        binary_sha256: digest(16),
        version_sha256: digest(22),
        build_profile: "release".to_owned(),
        enabled_features: vec!["default".to_owned()],
        build_command_sha256: digest(17),
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
        binary_sha256: digest(match kind {
            QualificationDerivedChangeControlBinaryKindV1::Library => 29,
            QualificationDerivedChangeControlBinaryKindV1::Cli => 30,
        }),
        build_command_sha256: qualification_derived_change_control_build_command_sha256_v1(kind),
        operating_system: execution.operating_system.clone(),
        architecture: execution.architecture.clone(),
        source_dirty: false,
        attestation_command_sha256: qualification_derived_change_control_command_sha256_v1(
            &attestation_test,
        ),
        attestation_stdout_sha256: digest(33),
        attestation_stderr_sha256: digest(34),
        attestation_test,
    }
}

#[test]
fn execution_identity_diagnostic_names_every_drifted_field() {
    let expected = QualificationDerivedAccessExpectedAuthorityV1::test_fixture().execution;
    let mut observed = expected.clone();
    observed.platform = QualificationDerivedAccessPlatformV1::WindowsNtfs;
    observed.binary_sha256 = digest(9);
    observed.architecture = "x86_64".to_owned();

    assert_eq!(
        super::evidence::execution_identity_mismatches(&expected, &observed),
        ["platform", "binary_sha256", "architecture"]
    );
}

#[test]
fn execution_identity_reports_only_campaign_host_authority_drift() {
    let expected = QualificationDerivedAccessExpectedAuthorityV1::test_fixture().execution;
    let mut observed = expected.clone();
    observed.host_identity_sha256 = digest(35);

    assert_eq!(
        super::evidence::execution_identity_mismatches(&expected, &observed),
        ["host_identity_sha256"]
    );
}

#[test]
fn diagnostic_documents_are_rejected_by_fragment_and_package_evidence_boundaries() {
    let root = tempfile::tempdir().expect("diagnostic evidence root");
    let execution = QualificationDerivedAccessExpectedAuthorityV1::test_fixture().execution;
    let reserved_paths = [
        root.path()
            .join(DERIVED_CHANGE_DIAGNOSTIC_REPORT_BASENAME_V1),
        root.path()
            .join(DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1)
            .join("receipt.json"),
    ];
    for (index, path) in reserved_paths.into_iter().enumerate() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create reserved diagnostic parent");
        }
        std::fs::write(&path, b"{}").expect("write reserved diagnostic document");
        let request_path = root.path().join(format!("reserved-fragment-{index}.json"));
        std::fs::write(
            &request_path,
            serde_json::to_vec(&QualificationDerivedAccessFragmentRequestV1 {
                schema: QUALIFICATION_DERIVED_ACCESS_FRAGMENT_REQUEST_SCHEMA_V1.to_owned(),
                execution: execution.clone(),
                receipt_paths: vec![path.clone()],
                evaluator_revision: QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3.to_owned(),
            })
            .expect("serialize reserved fragment request"),
        )
        .expect("write reserved fragment request");
        assert_eq!(
            build_qualification_derived_access_fragment_v1(&request_path).unwrap_err(),
            DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
        );
        assert_eq!(
            assemble_qualification_derived_access_package_v1(
                &[path],
                &root.path().join(format!("reserved-package-{index}")),
            )
            .unwrap_err(),
            DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
        );
    }

    for (name, document) in [
        (
            "report",
            serde_json::json!({
                "schema": DERIVED_CHANGE_DIAGNOSTIC_REPORT_SCHEMA_V1,
                "version": 1,
                "admissible": false,
            }),
        ),
        (
            "fragment",
            serde_json::json!({
                "schema": DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1,
                "version": 1,
            }),
        ),
        (
            "collection",
            serde_json::json!({
                "schema": DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
                "cases": [],
            }),
        ),
        (
            "readiness",
            serde_json::json!({
                "schema": DERIVED_CHANGE_DIAGNOSTIC_READINESS_SCHEMA_V1,
                "admissible": false,
                "ready": false,
            }),
        ),
        (
            "change-read-child",
            serde_json::json!({
                "mode": DERIVED_CHANGE_READ_DIAGNOSTIC_MODE_V1,
                "sourceUnchanged": true,
                "preflight": [],
                "rows": [],
                "controls": [],
                "storage": [],
            }),
        ),
        (
            "lifecycle-child",
            serde_json::json!({
                "mode": DERIVED_ACCESS_LIFECYCLE_DIAGNOSTIC_MODE_V1,
                "sourceUnchanged": true,
                "cases": [],
            }),
        ),
        (
            "native-child",
            serde_json::json!({
                "mode": DERIVED_CHANGE_DIAGNOSTIC_NATIVE_MODE_V1,
                "sourceUnchanged": true,
            }),
        ),
        (
            "identity-child",
            serde_json::json!({
                "mode": DERIVED_CHANGE_DIAGNOSTIC_IDENTITY_MODE_V1,
                "sourceCommit": "0".repeat(40),
            }),
        ),
    ] {
        let path = root.path().join(format!("renamed-{name}.json"));
        let bytes = serde_json::to_vec(&document).expect("serialize diagnostic document");
        std::fs::write(&path, &bytes).expect("write diagnostic document");
        let request_path = root.path().join(format!("fragment-{name}.json"));
        std::fs::write(
            &request_path,
            serde_json::to_vec(&QualificationDerivedAccessFragmentRequestV1 {
                schema: QUALIFICATION_DERIVED_ACCESS_FRAGMENT_REQUEST_SCHEMA_V1.to_owned(),
                execution: execution.clone(),
                receipt_paths: vec![path.clone()],
                evaluator_revision: QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3.to_owned(),
            })
            .expect("serialize fragment request"),
        )
        .expect("write fragment request");
        assert_eq!(
            build_qualification_derived_access_fragment_v1(&request_path).unwrap_err(),
            DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
        );
        assert_eq!(
            assemble_qualification_derived_access_package_v1(
                &[path],
                &root.path().join(format!("package-{name}")),
            )
            .unwrap_err(),
            DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
        );
        assert_eq!(
            validate_summaries_against_raw(
                &QualificationDerivedAccessPackageV1::test_fixture(),
                &[document],
            )
            .unwrap_err(),
            DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
        );
        assert_eq!(
            publish_qualification_derived_access_package_v1(
                &root.path().join(format!("published-package-{name}")),
                &QualificationDerivedAccessPackageV1::test_fixture(),
                &[("renamed.json", bytes.as_slice())],
            )
            .unwrap_err(),
            DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
        );
    }

    assert_eq!(
        validate_summaries_against_raw(
            &QualificationDerivedAccessPackageV1::test_fixture(),
            &[serde_json::json!({
                "schema": "pointbreak.unknown-terminal-receipt.v1"
            })],
        )
        .unwrap_err(),
        "unsupported derived-access raw receipt schema: pointbreak.unknown-terminal-receipt.v1",
    );
    assert_eq!(
        validate_summaries_against_raw(
            &QualificationDerivedAccessPackageV1::test_fixture(),
            &[serde_json::json!({"ordinary": true})],
        )
        .unwrap_err(),
        "derived-access raw receipt omitted schema",
    );

    assert_eq!(
        reject_derived_change_diagnostic_evidence_path_v1(Path::new(
            DERIVED_CHANGE_DIAGNOSTIC_REPORT_BASENAME_V1,
        ))
        .unwrap_err(),
        DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
    );
    assert_eq!(
        reject_derived_change_diagnostic_evidence_path_v1(Path::new(
            "nested/derived-change-diagnostic/receipt.json",
        ))
        .unwrap_err(),
        DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
    );

    let reserved_root = root
        .path()
        .join(DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1);
    std::fs::create_dir_all(&reserved_root).expect("create reserved request root");
    let ordinary_receipt = root.path().join("ordinary-receipt.json");
    std::fs::write(&ordinary_receipt, b"{}").expect("write ordinary receipt");
    let reserved_request = reserved_root.join("fragment-request.json");
    std::fs::write(
        &reserved_request,
        serde_json::to_vec(&QualificationDerivedAccessFragmentRequestV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_FRAGMENT_REQUEST_SCHEMA_V1.to_owned(),
            execution: execution.clone(),
            receipt_paths: vec![ordinary_receipt],
            evaluator_revision: QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3.to_owned(),
        })
        .expect("serialize reserved-path fragment request"),
    )
    .expect("write reserved-path fragment request");
    assert_eq!(
        build_qualification_derived_access_fragment_v1(&reserved_request).unwrap_err(),
        DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
    );
    assert_eq!(
        publish_qualification_derived_access_package_v1(
            &reserved_root,
            &QualificationDerivedAccessPackageV1::test_fixture(),
            &[],
        )
        .unwrap_err(),
        DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
    );
    assert_eq!(
        verify_qualification_derived_access_package_v1(&reserved_root).unwrap_err(),
        DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
    );
    assert_eq!(
        assemble_qualification_derived_access_package_v1(&[], &reserved_root).unwrap_err(),
        DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
    );
    assert_eq!(
        verify_qualification_derived_access_phase_v1(
            &root.path().join("phase-request.json"),
            &reserved_root.join("phase-bundle.json"),
        )
        .unwrap_err(),
        DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1,
    );
}

#[test]
fn diagnostic_lifecycle_collection_continues_after_an_isolated_vector_failure() {
    let mut attempted = Vec::new();
    let cases = collect_qualification_derived_access_lifecycle_diagnostics_v1(|criterion| {
        attempted.push(criterion);
        if criterion == QualificationDerivedAccessLifecycleCriterionV1::WrongRoot {
            Err("wrong-root diagnostic".to_owned())
        } else {
            Ok(format!("{criterion:?}"))
        }
    });

    assert_eq!(
        attempted.len(),
        QualificationDerivedAccessLifecycleCriterionV1::ALL.len(),
    );
    assert_eq!(cases.len(), attempted.len());
    assert_eq!(
        cases
            .iter()
            .filter(|case| {
                case.status == QualificationDerivedAccessLifecycleDiagnosticStatusV1::Failed
            })
            .count(),
        1,
    );
    assert!(cases.iter().any(|case| {
        case.criterion == QualificationDerivedAccessLifecycleCriterionV1::WrongRoot
            && case.status == QualificationDerivedAccessLifecycleDiagnosticStatusV1::Failed
            && case.failure_detail.as_deref() == Some("wrong-root diagnostic")
    }));

    let collection = QualificationDerivedAccessLifecycleDiagnosticCollectionV1 {
        cases,
        source_unchanged: true,
    };
    let value = serde_json::to_value(collection).expect("serialize diagnostic collection");
    assert_eq!(
        value
            .as_object()
            .expect("diagnostic collection object")
            .len(),
        2,
    );
    assert_eq!(value["sourceUnchanged"], true);
    assert_eq!(
        value["cases"].as_array().expect("diagnostic cases").len(),
        18
    );
    assert!(value.get("schema").is_none());
}

#[test]
fn native_diagnostic_result_exposes_only_the_admitted_root() {
    let result = DerivedChangeDiagnosticNativeResultV1 {
        mode: DERIVED_CHANGE_DIAGNOSTIC_NATIVE_MODE_V1.to_owned(),
        tier: QualificationDerivedAccessTierV1::L1,
        admitted_root_path: PathBuf::from("/tmp/diagnostic-root/root-a"),
        admitted_root_sha256: digest(53),
        source_unchanged: true,
    };
    let value = serde_json::to_value(result).expect("serialize native diagnostic result");
    assert_eq!(
        value
            .as_object()
            .expect("native diagnostic result object")
            .len(),
        5,
    );
    assert_eq!(value["mode"], DERIVED_CHANGE_DIAGNOSTIC_NATIVE_MODE_V1);
    assert_eq!(value["tier"], "L1");
    assert_eq!(value["sourceUnchanged"], true);
    for forbidden in [
        "schema", "payload", "receipt", "fragment", "package", "report",
    ] {
        assert!(
            value.get(forbidden).is_none(),
            "native result leaked {forbidden}"
        );
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn native_diagnostic_bridge_admits_post_smoke_authoritative_roots_for_each_tier() {
    let workspace = tempfile::tempdir().expect("native diagnostic bridge workspace");
    let source_checkout = workspace.path().join("source");
    initialize_clean_test_source_checkout(&source_checkout);

    for tier in [
        QualificationDerivedAccessTierV1::D0_128,
        QualificationDerivedAccessTierV1::L1,
        QualificationDerivedAccessTierV1::L7,
    ] {
        let tier_root = workspace.path().join(format!("native-{tier:?}"));
        let execution = super::evidence::observe_current_execution_identity_for_test_v1(
            native_test_platform(),
            digest(54),
            &source_checkout,
            &tier_root,
            digest(31),
        )
        .expect("observe exact diagnostic execution");
        let request = QualificationDerivedAccessNativeSmokeRunRequestV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_NATIVE_SMOKE_REQUEST_SCHEMA_V1.to_owned(),
            source_checkout: source_checkout.clone(),
            workspace_root: tier_root,
            execution,
            tier,
        };
        let request_path = workspace.path().join(format!("native-{tier:?}.json"));
        std::fs::write(
            &request_path,
            serde_json::to_vec_pretty(&request).expect("serialize native diagnostic request"),
        )
        .expect("write native diagnostic request");

        let result = super::evidence::run_derived_change_diagnostic_native_for_test_v1(
            &request_path,
            digest(31),
        )
        .expect("diagnostic bridge retains the post-smoke admitted root");
        let observed =
            crate::bench_support::longitudinal::longitudinal_authoritative_store_data_inventory_v1(
                &result.admitted_root_path,
            )
            .expect("inventory retained admitted root");
        assert_eq!(result.tier, tier);
        assert_eq!(result.admitted_root_sha256, observed.inventory_sha256);
        assert!(result.source_unchanged);
    }
}

#[test]
fn change_read_diagnostic_collection_continues_rows_controls_and_stays_schema_less() {
    let mut attempted_rows = Vec::new();
    let rows = collect_derived_change_read_diagnostic_rows_v1(|case| {
        attempted_rows.push(case);
        if case == QualificationDerivedChangeReadCaseV1::ChangesBare {
            Err("changes diagnostic".to_owned())
        } else {
            Ok(())
        }
    });
    assert_eq!(
        attempted_rows.len(),
        QualificationDerivedChangeReadCaseV1::ALL.len()
    );
    assert!(rows.iter().any(|row| {
        row.case == QualificationDerivedChangeReadCaseV1::ChangesBare
            && row.status == DerivedChangeReadDiagnosticStatusV1::Failed
            && row.failure_detail.as_deref() == Some("changes diagnostic")
    }));

    let mut attempted_controls = Vec::new();
    let controls = collect_derived_change_read_diagnostic_controls_v1(
        [
            DerivedChangeReadDiagnosticPreflightV1::passed(
                DerivedChangeReadDiagnosticPreflightKindV1::LibraryControl,
            ),
            DerivedChangeReadDiagnosticPreflightV1::failed(
                DerivedChangeReadDiagnosticPreflightKindV1::CliControl,
                "cli attestation diagnostic".to_owned(),
            ),
        ],
        |case| {
            attempted_controls.push(qualification_derived_change_control_test_v1(case));
            if case == QualificationDerivedChangeControlCaseV1::L0NoGeneration {
                Err("library control diagnostic".to_owned())
            } else {
                Ok(())
            }
        },
    );
    assert!(
        attempted_controls.contains(&qualification_derived_change_control_test_v1(
            QualificationDerivedChangeControlCaseV1::L0NoGeneration
        ))
    );
    let expected_library_tests = QualificationDerivedChangeControlCaseV1::ALL
        .into_iter()
        .map(qualification_derived_change_control_test_v1)
        .filter(|(kind, _)| *kind == QualificationDerivedChangeControlBinaryKindV1::Library)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(attempted_controls.len(), expected_library_tests.len());
    let shared_checkpoint_test = qualification_derived_change_control_test_v1(
        QualificationDerivedChangeControlCaseV1::CheckpointAuthorityMismatch,
    );
    assert_eq!(
        attempted_controls
            .iter()
            .filter(|attempt| **attempt == shared_checkpoint_test)
            .count(),
        1
    );
    assert!(controls.iter().any(|control| {
        control.case == QualificationDerivedChangeControlCaseV1::L0NoGeneration
            && control.status == DerivedChangeReadDiagnosticStatusV1::Failed
    }));
    assert!(controls.iter().any(|control| {
        qualification_derived_change_control_test_v1(control.case).0
            == QualificationDerivedChangeControlBinaryKindV1::Cli
            && control.status == DerivedChangeReadDiagnosticStatusV1::Skipped
            && control.failure_detail.as_deref() == Some("cli attestation diagnostic")
    }));

    let collection = DerivedChangeReadDiagnosticCollectionV1 {
        mode: DERIVED_CHANGE_READ_DIAGNOSTIC_MODE_V1.to_owned(),
        source_unchanged: true,
        preflight: vec![DerivedChangeReadDiagnosticPreflightV1::passed(
            DerivedChangeReadDiagnosticPreflightKindV1::Fixture,
        )],
        rows,
        controls,
        storage: vec![DerivedChangeReadDiagnosticStorageV1 {
            case: DerivedChangeReadDiagnosticStorageCaseV1::Initial,
            status: DerivedChangeReadDiagnosticStatusV1::Passed,
            failure_detail: None,
        }],
    };
    let value = serde_json::to_value(collection).expect("serialize diagnostic collection");
    assert_eq!(value["mode"], DERIVED_CHANGE_READ_DIAGNOSTIC_MODE_V1);
    assert_eq!(value["sourceUnchanged"], true);
    assert!(value.get("schema").is_none());
    for forbidden in [
        "receipt",
        "fragment",
        "package",
        "report",
        "storageRows",
        "controlBinaryIdentities",
    ] {
        assert!(
            value.get(forbidden).is_none(),
            "collection leaked {forbidden}"
        );
    }
}

#[test]
fn change_read_diagnostic_storage_collection_continues_after_initial_failure() {
    let mut attempted = Vec::new();
    let storage = collect_derived_change_read_diagnostic_storage_v1(|case| {
        attempted.push(case);
        if case == DerivedChangeReadDiagnosticStorageCaseV1::Initial {
            Err("initial storage diagnostic".to_owned())
        } else {
            Ok(())
        }
    });
    assert_eq!(
        attempted,
        DerivedChangeReadDiagnosticStorageCaseV1::ALL.to_vec()
    );
    assert!(storage.iter().any(|row| {
        row.case == DerivedChangeReadDiagnosticStorageCaseV1::Initial
            && row.status == DerivedChangeReadDiagnosticStatusV1::Failed
            && row.failure_detail.as_deref() == Some("initial storage diagnostic")
    }));
    assert!(storage.iter().any(|row| {
        row.case == DerivedChangeReadDiagnosticStorageCaseV1::PostAppend
            && row.status == DerivedChangeReadDiagnosticStatusV1::Passed
    }));
}

#[test]
fn change_read_diagnostic_fixture_inventory_is_fixture_scoped() {
    assert_eq!(
        QualificationDerivedChangeFixtureV1::TopologyV1
            .required_cases()
            .len(),
        QualificationDerivedChangeReadCaseV1::ALL.len()
    );
    assert_eq!(
        DerivedChangeReadDiagnosticStorageCaseV1::required_for(
            QualificationDerivedChangeFixtureV1::TopologyV1,
        ),
        &DerivedChangeReadDiagnosticStorageCaseV1::ALL,
    );
    for fixture in [
        QualificationDerivedChangeFixtureV1::DuplicateEqualV1,
        QualificationDerivedChangeFixtureV1::MissingCarrierV1,
        QualificationDerivedChangeFixtureV1::IncompleteV1,
    ] {
        assert_ne!(
            fixture.required_cases().len(),
            QualificationDerivedChangeReadCaseV1::ALL.len(),
        );
        assert_eq!(
            DerivedChangeReadDiagnosticStorageCaseV1::required_for(fixture),
            &DerivedChangeReadDiagnosticStorageCaseV1::INITIAL_ONLY,
        );
    }
}

#[test]
fn change_read_diagnostic_fixture_matrix_has_complete_case_inventory() {
    assert_eq!(
        QualificationDerivedChangeFixtureV1::ALL
            .into_iter()
            .map(|fixture| fixture.required_cases().len())
            .sum::<usize>(),
        71
    );
    assert_eq!(QualificationDerivedChangeControlCaseV1::ALL.len(), 27);
    assert_eq!(
        QualificationDerivedChangeFixtureV1::ALL
            .into_iter()
            .map(|fixture| DerivedChangeReadDiagnosticStorageCaseV1::required_for(fixture).len())
            .sum::<usize>(),
        10
    );
}

#[test]
fn change_read_diagnostic_uses_public_kebab_fixture_identifiers() {
    assert_eq!(
        serde_json::to_value(QualificationDerivedChangeFixtureV1::TopologyV1)
            .expect("serialize public fixture"),
        serde_json::json!("topology-v1"),
    );
    assert_eq!(
        serde_json::from_value::<QualificationDerivedChangeFixtureV1>(serde_json::json!(
            "topology-v1"
        ))
        .expect("parse public fixture"),
        QualificationDerivedChangeFixtureV1::TopologyV1,
    );
    assert!(
        serde_json::from_value::<QualificationDerivedChangeFixtureV1>(serde_json::json!(
            "topology_v1"
        ))
        .is_err()
    );
}

#[test]
fn change_read_diagnostic_fixture_failure_skips_only_read_rows() {
    let preflight = DerivedChangeReadDiagnosticPreflightV1::failed(
        DerivedChangeReadDiagnosticPreflightKindV1::Fixture,
        "fixture diagnostic".to_owned(),
    );
    let mut attempted = false;
    let rows = collect_derived_change_read_diagnostic_rows_after_preflight_v1(&preflight, |_| {
        attempted = true;
        Ok(())
    });
    assert!(!attempted);
    assert_eq!(rows.len(), QualificationDerivedChangeReadCaseV1::ALL.len());
    assert!(rows.iter().all(|row| {
        row.status == DerivedChangeReadDiagnosticStatusV1::Skipped
            && row.failure_detail.as_deref() == Some("fixture diagnostic")
    }));
}

#[cfg(feature = "longitudinal-counting")]
#[test]
fn bounded_bootstrap_smoke_reports_two_pass_work_and_completed_phases() {
    let receipt = run_qualification_derived_access_bootstrap_smoke_v1(
        QualificationDerivedAccessTierV1::D0_128,
    )
    .expect("bounded D0 bootstrap smoke");

    receipt.validate().expect("valid bootstrap receipt");
    assert_eq!(receipt.event_count, 128);
    assert_eq!(receipt.counters.carrier_opens, 343);
    #[cfg(target_os = "macos")]
    assert!(receipt.rss_observed);
    #[cfg(not(target_os = "macos"))]
    assert!(!receipt.rss_observed);
    assert_eq!(
        receipt
            .phases
            .iter()
            .map(|progress| progress.phase.as_str())
            .collect::<Vec<_>>(),
        vec![
            "cursor_population",
            "projection_population",
            "strict_verification",
            "finalizing",
        ]
    );
}

#[cfg(all(feature = "longitudinal-counting", target_os = "windows"))]
#[test]
fn native_l1_bootstrap_stays_within_the_ntfs_journal_work_budget() {
    let receipt =
        run_qualification_derived_access_bootstrap_smoke_v1(QualificationDerivedAccessTierV1::L1)
            .expect("bounded native L1 bootstrap smoke");

    receipt.validate().expect("valid L1 bootstrap receipt");
    assert_eq!(receipt.event_count, 1_024);
    assert_eq!(receipt.head_sequence, 1_024);
}

#[test]
fn raw_samples_reject_ambiguous_cpu_units_and_scope() {
    let mut sample = QualificationDerivedAccessRawSampleV1::test_fixture();
    sample.process_cpu_unit = QualificationDerivedAccessCpuUnitV1::NativeTicks;
    assert!(sample.validate().is_err());

    sample.process_cpu_unit = QualificationDerivedAccessCpuUnitV1::Nanoseconds;
    sample.process_scope = None;
    assert!(sample.validate().is_err());
}

fn phase_receipt(
    tier: QualificationDerivedAccessTierV1,
    operation: QualificationDerivedAccessPhaseOperationV1,
    source_identity_sha256: String,
) -> QualificationDerivedAccessPhaseReceiptV1 {
    let phases = operation
        .expected_phases()
        .iter()
        .enumerate()
        .map(|(ordinal, phase)| {
            crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseSampleV1 {
                phase: *phase,
                ownership: phase.ownership(),
                actor: None,
                ordinal: ordinal.try_into().expect("small phase list"),
                parent_ordinal: operation.expected_parent_ordinal(ordinal),
                wall_nanos: 1,
                process_cpu_nanos: Some(1),
                resident_bytes_before: Some(100),
                resident_bytes_after: Some(101),
                resident_bytes_observed_max: Some(101),
                counters: crate::bench_support::longitudinal::LongitudinalCountersV1::default(),
            }
        })
        .collect();
    QualificationDerivedAccessPhaseReceiptV1::new(
        tier,
        operation,
        source_identity_sha256,
        digest(41),
        digest(42),
        digest(43),
        phases,
    )
    .expect("valid phase receipt")
}

#[test]
fn change_page_phase_contract_names_bounded_work_and_zero_fallbacks() {
    use crate::bench_support::longitudinal::{
        LongitudinalCountersV1, LongitudinalDerivedAccessPhaseOwnershipV1 as Ownership,
        LongitudinalDerivedAccessPhaseV1 as Phase,
    };

    let phases = [
        Phase::ChangePageSnapshotAcquisition,
        Phase::ChangePageBodylessSelection,
        Phase::ChangePageProposalLocatorExpansion,
        Phase::ChangePageCarrierHydrationValidation,
        Phase::ChangePageSupportExpansion,
        Phase::ChangePagePresentationProjection,
    ];
    assert_eq!(
        phases
            .iter()
            .map(|phase| phase.ownership())
            .collect::<Vec<_>>(),
        vec![
            Ownership::MixedDerivedAndTruth,
            Ownership::DerivedAccess,
            Ownership::DerivedAccess,
            Ownership::AuthoritativeTruth,
            Ownership::MixedDerivedAndTruth,
            Ownership::ProductProjection,
        ]
    );
    assert_eq!(
        Phase::ChangePageExhaustiveProposalSearch.ownership(),
        Ownership::ProductProjection
    );
    assert_eq!(
        serde_json::to_string(&Phase::ChangePageExhaustiveProposalSearch)
            .expect("serialize exhaustive Change search phase"),
        "\"change_page_exhaustive_proposal_search\""
    );
    let counters = LongitudinalCountersV1::default();
    assert_eq!(counters.authoritative_fallbacks, 0);
    assert_eq!(counters.full_history_fallbacks, 0);
    let json = serde_json::to_value(counters).expect("default counters JSON");
    assert!(json.get("authoritativeFallbacks").is_none());
    assert!(json.get("fullHistoryFallbacks").is_none());
}

#[test]
fn phase_receipts_reject_missing_duplicate_wrong_operation_and_source_samples() {
    let source = digest(40);
    let operation = QualificationDerivedAccessPhaseOperationV1::RevisionPage;
    let receipt = phase_receipt(
        QualificationDerivedAccessTierV1::L7,
        operation,
        source.clone(),
    );
    receipt
        .validate_against(&source, QualificationDerivedAccessTierV1::L7, operation)
        .expect("valid phase receipt");

    let mut missing = receipt.clone();
    missing.phases.pop();
    missing.refresh_sha256().expect("rehash missing receipt");
    assert!(
        missing
            .validate_against(&source, QualificationDerivedAccessTierV1::L7, operation)
            .is_err()
    );

    let mut duplicate = receipt.clone();
    duplicate.phases[1].phase = duplicate.phases[0].phase;
    duplicate
        .refresh_sha256()
        .expect("rehash duplicate receipt");
    assert!(
        duplicate
            .validate_against(&source, QualificationDerivedAccessTierV1::L7, operation)
            .is_err()
    );

    assert!(
        receipt
            .validate_against(
                &source,
                QualificationDerivedAccessTierV1::L7,
                QualificationDerivedAccessPhaseOperationV1::Bootstrap,
            )
            .is_err()
    );
    assert!(
        receipt
            .validate_against(&digest(43), QualificationDerivedAccessTierV1::L7, operation)
            .is_err()
    );

    let mut flattened = receipt.clone();
    flattened.phases[6].parent_ordinal = None;
    flattened
        .refresh_sha256()
        .expect("rehash flattened receipt");
    assert!(flattened.validate().is_err());
}

#[test]
fn governed_write_phase_receipt_accepts_counted_nested_authority_maintenance() {
    use crate::bench_support::longitudinal::{
        LongitudinalCountersV1, LongitudinalDerivedAccessPhaseSampleV1,
        LongitudinalDerivedAccessPhaseV1,
    };

    let mut receipt = phase_receipt(
        QualificationDerivedAccessTierV1::L7,
        QualificationDerivedAccessPhaseOperationV1::GovernedWrite,
        digest(47),
    );
    receipt.phases[3].ordinal = 4;
    let counters = LongitudinalCountersV1 {
        authority_identity_rows_scanned: 17,
        ..LongitudinalCountersV1::default()
    };
    let phase = LongitudinalDerivedAccessPhaseV1::GovernedWriteAuthorityCursorMaintenance;
    receipt.phases.insert(
        3,
        LongitudinalDerivedAccessPhaseSampleV1 {
            phase,
            ownership: phase.ownership(),
            actor: None,
            ordinal: 3,
            parent_ordinal: Some(2),
            wall_nanos: 1,
            process_cpu_nanos: Some(1),
            resident_bytes_before: Some(100),
            resident_bytes_after: Some(101),
            resident_bytes_observed_max: Some(101),
            counters,
        },
    );
    receipt
        .refresh_sha256()
        .expect("rehash maintenance receipt");
    receipt.validate().expect("counted maintenance is valid");

    let serialized = serde_json::to_value(&receipt).expect("maintenance receipt JSON");
    assert_eq!(
        serialized["phases"][3]["counters"]["authorityIdentityRowsScanned"],
        17
    );

    receipt.phases[3].counters.authority_identity_rows_scanned = 0;
    receipt
        .refresh_sha256()
        .expect("rehash uncounted maintenance receipt");
    assert!(receipt.validate().is_err());
}

#[test]
fn phase_receipt_json_rejects_negative_and_overflowed_measurements() {
    use crate::bench_support::longitudinal::LongitudinalCountersV1;

    let receipt = phase_receipt(
        QualificationDerivedAccessTierV1::D0_128,
        QualificationDerivedAccessPhaseOperationV1::Bootstrap,
        digest(44),
    );
    let mut negative = serde_json::to_value(&receipt).expect("phase receipt JSON");
    negative["phases"][0]["wallNanos"] = serde_json::json!(-1);
    assert!(serde_json::from_value::<QualificationDerivedAccessPhaseReceiptV1>(negative).is_err());

    let mut overflowed = receipt.clone();
    overflowed.phases[0].wall_nanos = u64::MAX;
    overflowed
        .refresh_sha256()
        .expect("rehash overflowed receipt");
    assert!(overflowed.validate().is_err());

    let setters: [fn(&mut LongitudinalCountersV1); 7] = [
        |counters| counters.change_candidates = u64::MAX,
        |counters| counters.change_candidate_current_revisions = u64::MAX,
        |counters| counters.change_proposal_carriers_opened = u64::MAX,
        |counters| counters.change_proposal_carriers_validated = u64::MAX,
        |counters| counters.change_support_carriers_opened = u64::MAX,
        |counters| counters.change_matches = u64::MAX,
        |counters| counters.change_rows_emitted = u64::MAX,
    ];
    for set_overflow in setters {
        let mut overflowed = receipt.clone();
        set_overflow(&mut overflowed.phases[0].counters);
        overflowed
            .refresh_sha256()
            .expect("rehash overflowed Change counter receipt");
        assert!(overflowed.validate().is_err());
    }
}

#[test]
fn phase_bundle_hash_binds_raw_receipts_to_their_tier_and_operation() {
    let source = digest(45);
    let revision_page = phase_receipt(
        QualificationDerivedAccessTierV1::L100,
        QualificationDerivedAccessPhaseOperationV1::RevisionPage,
        source.clone(),
    );
    let bootstrap = phase_receipt(
        QualificationDerivedAccessTierV1::L100,
        QualificationDerivedAccessPhaseOperationV1::Bootstrap,
        source.clone(),
    );
    let governed_write = phase_receipt(
        QualificationDerivedAccessTierV1::L100,
        QualificationDerivedAccessPhaseOperationV1::GovernedWrite,
        source.clone(),
    );
    let mut bundle = QualificationDerivedAccessPhaseBundleV1::new(
        source.clone(),
        QualificationDerivedAccessTierV1::L100,
        vec![revision_page, bootstrap, governed_write],
    )
    .expect("valid phase bundle");
    bundle.validate().expect("phase bundle validates");

    bundle.raw_receipts.swap(0, 1);
    bundle.refresh_sha256().expect("rehash substituted bundle");
    assert!(bundle.validate().is_err());
}

#[test]
fn phase_request_and_separate_verifier_bind_source_tier_and_root() {
    let execution = QualificationDerivedAccessExpectedAuthorityV1::test_fixture().execution;
    let root = std::env::temp_dir().join("pointbreak-phase-request");
    let mut request = QualificationDerivedAccessPhaseRunRequestV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_PHASE_REQUEST_SCHEMA_V1.to_owned(),
        source_checkout: root.join("source"),
        execution,
        tier: QualificationDerivedAccessTierV1::L7,
        immutable_input_root: root.join("immutable-input"),
        root: root.join("mutable-root"),
        root_identity_sha256: digest(46),
        request_sha256: String::new(),
    };
    request.refresh_sha256().expect("hash request");
    request.validate().expect("valid request");
    let mut aliased = request.clone();
    aliased.immutable_input_root = aliased.root.clone();
    assert!(aliased.validate().is_err());
    let source = request.source_identity_sha256().expect("source identity");
    let receipts = QualificationDerivedAccessPhaseOperationV1::ALL
        .into_iter()
        .map(|operation| phase_receipt(request.tier, operation, source.clone()))
        .map(|mut receipt| {
            receipt.root_identity_sha256 = request.root_identity_sha256.clone();
            receipt.refresh_sha256().expect("rehash bound receipt");
            receipt
        })
        .collect();
    let bundle = QualificationDerivedAccessPhaseBundleV1::new(source, request.tier, receipts)
        .expect("valid bundle");
    let workspace = tempfile::tempdir().expect("phase verifier root");
    let request_path = workspace.path().join("request.json");
    let bundle_path = workspace.path().join("bundle.json");
    std::fs::write(
        &request_path,
        serde_json::to_vec_pretty(&request).expect("request JSON"),
    )
    .expect("write request");
    std::fs::write(
        &bundle_path,
        serde_json::to_vec_pretty(&bundle).expect("bundle JSON"),
    )
    .expect("write bundle");
    verify_qualification_derived_access_phase_v1(&request_path, &bundle_path)
        .expect("separate verifier accepts exact request and bundle");

    request.tier = QualificationDerivedAccessTierV1::D0_128;
    request.refresh_sha256().expect("rehash drifted request");
    std::fs::write(
        &request_path,
        serde_json::to_vec_pretty(&request).expect("drifted request JSON"),
    )
    .expect("write drifted request");
    assert!(verify_qualification_derived_access_phase_v1(&request_path, &bundle_path).is_err());
}

#[test]
fn raw_samples_reject_semantic_failure_and_hidden_whole_history_work() {
    let mut sample = QualificationDerivedAccessRawSampleV1::test_fixture();
    sample.semantic_receipt_matches = false;
    assert!(sample.validate().is_err());

    sample.semantic_receipt_matches = true;
    sample.whole_history_work = true;
    sample.complexity = QualificationDerivedAccessComplexityV1::BoundedSelectedWork;
    assert!(sample.validate().is_err());
}

#[test]
fn derived_inventory_rejects_body_or_object_ownership() {
    let mut inventory = QualificationDerivedAccessDerivedInventoryV1 {
        database_bytes: 1,
        wal_bytes: 0,
        shared_memory_bytes: 0,
        temporary_bytes: 0,
        row_count: 1,
        page_count: 1,
        body_bytes: 1,
        object_bytes: 0,
        high_water_bytes: 1,
    };
    assert!(inventory.validate_bodyless().is_err());
    inventory.body_bytes = 0;
    inventory.object_bytes = 1;
    assert!(inventory.validate_bodyless().is_err());
}

#[test]
fn shard_authority_rejects_every_identity_drift() {
    let authority = QualificationDerivedAccessExpectedAuthorityV1::test_fixture();
    let shard = QualificationDerivedAccessEvidenceShardV1::test_fixture();
    shard
        .validate_against(&authority)
        .expect("matching authority");

    for mutate in [
        |row: &mut QualificationDerivedAccessExecutionIdentityV1| {
            row.source_commit = "0".repeat(40)
        },
        |row: &mut QualificationDerivedAccessExecutionIdentityV1| row.source_tree = "0".repeat(40),
        |row: &mut QualificationDerivedAccessExecutionIdentityV1| {
            row.cargo_lock_sha256 = digest(10)
        },
        |row: &mut QualificationDerivedAccessExecutionIdentityV1| row.binary_sha256 = digest(11),
        |row: &mut QualificationDerivedAccessExecutionIdentityV1| row.contract_sha256 = digest(12),
        |row: &mut QualificationDerivedAccessExecutionIdentityV1| {
            row.root_provenance_sha256 = digest(13)
        },
        |row: &mut QualificationDerivedAccessExecutionIdentityV1| {
            row.host_identity_sha256 = digest(14)
        },
    ] {
        let mut drifted = shard.clone();
        mutate(&mut drifted.execution);
        assert!(drifted.validate_against(&authority).is_err());
    }
}

#[test]
fn package_manifest_is_completion_last_and_rejects_unlisted_or_corrupt_files() {
    let root = tempfile::tempdir().expect("package root");
    let package = QualificationDerivedAccessPackageV1::test_fixture();
    publish_qualification_derived_access_package_v1(root.path(), &package, &[])
        .expect("publish package");
    verify_qualification_derived_access_package_v1(root.path()).expect("verify package");

    std::fs::write(root.path().join("unexpected.json"), b"{}").expect("unexpected file");
    assert!(verify_qualification_derived_access_package_v1(root.path()).is_err());
    std::fs::remove_file(root.path().join("unexpected.json")).expect("remove unexpected");

    std::fs::write(root.path().join("package.json"), b"{}").expect("corrupt package");
    assert!(verify_qualification_derived_access_package_v1(root.path()).is_err());

    let renamed_root = tempfile::tempdir().expect("renamed diagnostic package root");
    publish_qualification_derived_access_package_v1(renamed_root.path(), &package, &[])
        .expect("publish renamed diagnostic package");
    let renamed_relative = "extras/renamed.json";
    let renamed_bytes = serde_json::to_vec(&serde_json::json!({
        "mode": DERIVED_CHANGE_READ_DIAGNOSTIC_MODE_V1,
        "sourceUnchanged": true,
    }))
    .expect("serialize renamed diagnostic child");
    std::fs::create_dir(renamed_root.path().join("extras")).expect("create extras directory");
    std::fs::write(renamed_root.path().join(renamed_relative), &renamed_bytes)
        .expect("write renamed diagnostic child");
    let manifest_path = renamed_root.path().join("manifest.json");
    let mut manifest: QualificationDerivedAccessPackageManifestV1 =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read package manifest"))
            .expect("parse package manifest");
    manifest
        .entries
        .push(QualificationDerivedAccessPackageEntryV1 {
            relative_path: renamed_relative.to_owned(),
            byte_count: renamed_bytes.len() as u64,
            sha256: crate::canonical_hash::sha256_bytes_hex(&renamed_bytes),
        });
    manifest
        .entries
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    std::fs::write(
        &manifest_path,
        serde_json::to_vec(&manifest).expect("serialize tampered package manifest"),
    )
    .expect("write tampered package manifest");
    assert_eq!(
        verify_qualification_derived_access_package_v1(renamed_root.path()).unwrap_err(),
        "derived-access package inventory is outside its closed schema",
    );
}

#[test]
fn package_summaries_require_raw_receipt_authority() {
    let mut package = QualificationDerivedAccessPackageV1::test_fixture();
    package.lifecycle_rows.push(
        crate::bench_support::derived_access::QualificationDerivedAccessLifecycleEvidenceV1 {
            tier: QualificationDerivedAccessTierV1::D0_128,
            platform: QualificationDerivedAccessPlatformV1::MacosApfs,
            criterion:
                QualificationDerivedAccessLifecycleCriterionV1::OpenBootstrapReopenReplayEquality,
            status: QualificationDerivedAccessStatusV1::Passed,
        },
    );
    assert!(validate_summaries_against_raw(&package, &[]).is_err());

    let root = tempfile::tempdir().expect("assembly root");
    let package_path = root.path().join("unbound-package.json");
    std::fs::write(
        &package_path,
        serde_json::to_vec_pretty(&package).expect("serialize unbound package"),
    )
    .expect("write unbound package");
    assert!(
        assemble_qualification_derived_access_package_v1(
            &[package_path],
            &root.path().join("output"),
        )
        .is_err()
    );
    assert!(!root.path().join("output").exists());
}

#[test]
fn change_read_summaries_require_raw_receipt_authority() {
    let mut package = QualificationDerivedAccessPackageV1::test_fixture();
    package.evaluator_revision = QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3.to_owned();
    package
        .change_read_rows
        .push(QualificationDerivedChangeReadEvidenceV1 {
            platform: QualificationDerivedAccessPlatformV1::MacosApfs,
            fixture: QualificationDerivedChangeFixtureV1::TopologyV1,
            fixture_inventory_sha256: digest(13),
            fixture_witness_sha256: digest(14),
            case: QualificationDerivedChangeReadCaseV1::Profile,
            semantic_process_scope: QualificationDerivedAccessProcessScopeV1::InspectorServiceChild,
            counter_process_scope: QualificationDerivedAccessProcessScopeV1::QualificationHarness,
            product_identity_sha256: digest(26),
            counter_execution_identity_sha256: digest(27),
            status: QualificationDerivedAccessStatusV1::Passed,
            oracle: QualificationDerivedChangeReadOracleV1::StrictParity,
            strict_semantic_sha256: Some(digest(15)),
            derived_semantic_sha256: digest(15),
            wire_contract_matches: true,
            expected_http_status: 200,
            observed_http_status: 200,
            expected_code: None,
            observed_code: None,
            expected_typed_document: None,
            observed_typed_document: None,
            counters: crate::bench_support::longitudinal::LongitudinalCountersV1::default(),
        });

    assert!(validate_summaries_against_raw(&package, &[]).is_err());
}

#[test]
fn bound_change_read_receipt_crosses_the_fragment_boundary() {
    let root = tempfile::tempdir().expect("Change receipt workspace");
    let execution = QualificationDerivedAccessExpectedAuthorityV1::test_fixture().execution;
    let product = product_identity(&execution);
    let product_identity_sha256 = product.canonical_sha256().expect("product identity hash");
    let execution_identity_sha256 = execution
        .canonical_sha256()
        .expect("execution identity hash");
    let rows = QualificationDerivedChangeFixtureV1::TopologyV1
        .required_cases()
        .iter()
        .copied()
        .map(|case| {
            let (oracle, http_status, code) = qualification_derived_change_expected_outcome_v1(
                execution.platform,
                QualificationDerivedChangeFixtureV1::TopologyV1,
                case,
            );
            let typed_document = code.map(|code| QualificationDerivedChangeTypedDocumentV1 {
                schema: if code == "stale_projection" {
                    "pointbreak.inspect-change-page-error"
                } else {
                    "pointbreak.inspect-change-projection-error"
                }
                .to_owned(),
                version: 1,
                code: code.to_owned(),
                retryable: (code != "stale_projection").then_some(false),
                canonical_sha256: digest(28),
            });
            QualificationDerivedChangeReadEvidenceV1 {
                platform: execution.platform,
                fixture: QualificationDerivedChangeFixtureV1::TopologyV1,
                fixture_inventory_sha256: digest(25),
                fixture_witness_sha256: digest(20),
                case,
                semantic_process_scope:
                    QualificationDerivedAccessProcessScopeV1::InspectorServiceChild,
                counter_process_scope:
                    QualificationDerivedAccessProcessScopeV1::QualificationHarness,
                product_identity_sha256: product_identity_sha256.clone(),
                counter_execution_identity_sha256: execution_identity_sha256.clone(),
                status: QualificationDerivedAccessStatusV1::Passed,
                oracle,
                strict_semantic_sha256: (oracle
                    != QualificationDerivedChangeReadOracleV1::TypedFailure)
                    .then(|| digest(21)),
                derived_semantic_sha256: digest(21),
                wire_contract_matches: true,
                expected_http_status: http_status,
                observed_http_status: http_status,
                expected_code: code.map(str::to_owned),
                observed_code: code.map(str::to_owned),
                expected_typed_document: typed_document.clone(),
                observed_typed_document: typed_document,
                counters: crate::bench_support::longitudinal::LongitudinalCountersV1::default(),
            }
        })
        .collect();
    let control_binary_identities = QualificationDerivedChangeControlBinaryKindV1::ALL
        .into_iter()
        .map(|kind| control_binary_identity(&execution, kind))
        .collect::<Vec<_>>();
    let control_rows = QualificationDerivedChangeControlCaseV1::ALL
        .into_iter()
        .map(|case| {
            let (binary_kind, test_name) = qualification_derived_change_control_test_v1(case);
            let binary_identity = control_binary_identities
                .iter()
                .find(|identity| identity.kind == binary_kind)
                .expect("control binary identity");
            QualificationDerivedChangeControlEvidenceV1 {
                platform: execution.platform,
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
                command_sha256: qualification_derived_change_control_command_sha256_v1(test_name),
                stdout_sha256: digest(35),
                stderr_sha256: digest(36),
                exit_code: 0,
                tests_run: 1,
                tests_passed: 1,
            }
        })
        .collect();
    let storage_rows = [
        (
            QualificationDerivedChangeStoragePhaseV1::InitialPublication,
            "initial checkpoint",
        ),
        (
            QualificationDerivedChangeStoragePhaseV1::PostAppendCheckpoint,
            "post-append checkpoint",
        ),
    ]
    .into_iter()
    .map(
        |(phase, checkpoint)| QualificationDerivedChangeStorageEvidenceV1 {
            platform: execution.platform,
            fixture: QualificationDerivedChangeFixtureV1::TopologyV1,
            phase,
            fixture_inventory_sha256: digest(25),
            fixture_witness_sha256: digest(20),
            product_identity_sha256: product_identity_sha256.clone(),
            execution_identity_sha256: execution_identity_sha256.clone(),
            witness: QualificationDerivedStorageWitnessV1::test_fixture(checkpoint),
        },
    )
    .collect();
    let mut receipt = QualificationDerivedChangeReadReceiptV1 {
        schema: QUALIFICATION_DERIVED_CHANGE_READ_RECEIPT_SCHEMA_V1.to_owned(),
        purpose: QualificationDerivedChangeEvidencePurposeV1::ExactSourceQualification,
        execution: execution.clone(),
        product,
        fixture: QualificationDerivedChangeFixtureV1::TopologyV1,
        fixture_builder_sha256: digest(17),
        activation_fixture_sha256: digest(18),
        completion_fixture_sha256: digest(19),
        fixture_inventory_sha256: digest(25),
        fixture_after_inventory_sha256: digest(23),
        fixture_witness_sha256: digest(20),
        post_append_generation_sha256: Some(digest(24)),
        rows,
        pre_cut_deficiencies: Vec::new(),
        control_binary_identities,
        control_rows,
        storage_rows,
        complete: true,
        receipt_sha256: String::new(),
    };
    receipt.refresh_sha256().expect("hash Change read receipt");
    receipt
        .validate()
        .expect("valid exact-source Change receipt");

    let mut pre_cut = receipt.clone();
    pre_cut.purpose = QualificationDerivedChangeEvidencePurposeV1::PreCutFalsifier;
    pre_cut.product.source_commit = if pre_cut.product.source_commit == "1".repeat(40) {
        "2".repeat(40)
    } else {
        "1".repeat(40)
    };
    let pre_cut_product_sha256 = pre_cut
        .product
        .canonical_sha256()
        .expect("pre-cut product identity hash");
    for row in &mut pre_cut.rows {
        row.product_identity_sha256 = pre_cut_product_sha256.clone();
    }
    pre_cut.control_binary_identities.clear();
    pre_cut.control_rows.clear();
    pre_cut.storage_rows.clear();
    pre_cut.pre_cut_deficiencies.clear();
    pre_cut
        .refresh_sha256()
        .expect("hash all-pass pre-cut receipt");
    assert!(pre_cut.validate().is_err());

    let deficiency = pre_cut.rows[0].case;
    pre_cut.rows[0].status = QualificationDerivedAccessStatusV1::Failed;
    pre_cut.pre_cut_deficiencies.push(deficiency);
    pre_cut
        .refresh_sha256()
        .expect("hash deficient pre-cut receipt");
    pre_cut
        .validate()
        .expect("pre-cut receipt must identify its failed row");

    let receipt_path = root.path().join("change-read.json");
    std::fs::write(
        &receipt_path,
        serde_json::to_vec_pretty(&receipt).expect("serialize Change read receipt"),
    )
    .expect("write Change read receipt");
    let request_path = root.path().join("fragment-request.json");
    std::fs::write(
        &request_path,
        serde_json::to_vec_pretty(&QualificationDerivedAccessFragmentRequestV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_FRAGMENT_REQUEST_SCHEMA_V1.to_owned(),
            execution,
            receipt_paths: vec![receipt_path.clone()],
            evaluator_revision: QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3.to_owned(),
        })
        .expect("serialize fragment request"),
    )
    .expect("write fragment request");

    let fragment = build_qualification_derived_access_fragment_v1(&request_path)
        .expect("build raw-bound Change fragment");
    assert_eq!(fragment.package.change_read_rows, receipt.rows);
    assert_eq!(
        fragment.package.evaluator_revision,
        QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3
    );

    let mut incomplete = receipt.clone();
    incomplete.rows.pop();
    incomplete
        .refresh_sha256()
        .expect("rehash incomplete Change read receipt");
    assert_eq!(
        incomplete.validate().unwrap_err(),
        "derived Change read receipt omitted required cases"
    );

    receipt.rows[0].wire_contract_matches = false;
    std::fs::write(
        receipt_path,
        serde_json::to_vec_pretty(&receipt).expect("serialize forged Change read receipt"),
    )
    .expect("write forged Change read receipt");
    assert!(build_qualification_derived_access_fragment_v1(&request_path).is_err());
}

#[test]
fn evidence_paths_reject_source_build_and_private_roots() {
    for path in [
        PathBuf::from("target/release/store_foundation"),
        PathBuf::from("src/bench_support/derived_access/evidence.rs"),
        PathBuf::from(".git/objects/00/0000"),
        PathBuf::from(".pointbreak/stores/private/events/example.json"),
    ] {
        assert!(validate_qualification_evidence_relative_path_v1(&path).is_err());
    }
    assert!(
        validate_qualification_evidence_relative_path_v1(PathBuf::from("raw/d0.json").as_path())
            .is_ok()
    );
}

#[test]
fn d0_contract_requires_a_distinct_ordered_materialization_schedule() {
    let contract = qualification_derived_access_contract_v1();
    let receipt = qualification_derived_access_d0_schedule_v1().expect("D0 schedule");
    assert_eq!(receipt.stored_events, contract.d0.stored_events);
    assert_eq!(receipt.revisions, contract.d0.revisions);
    assert_eq!(
        receipt.independently_referenced_objects,
        contract.d0.independently_referenced_objects
    );
    assert_ne!(receipt.ordered_schedule_sha256, contract.d0.schedule_sha256);
}

#[test]
fn d0_materializer_proves_two_root_byte_identity() {
    let root = tempfile::tempdir().expect("D0 parent");
    let receipt = materialize_qualification_derived_access_d0_pair_v1(
        root.path().join("root-a"),
        root.path().join("root-b"),
    )
    .expect("materialize D0 pair");
    receipt.validate().expect("validate D0 pair");
    assert!(receipt.byte_identical);
    assert_eq!(receipt.root_a.event_count, 128);
    assert_eq!(receipt.root_a.revision_count, 16);
    assert_eq!(receipt.root_a.independently_referenced_objects, 16);
}

#[test]
fn lifecycle_registry_covers_every_frozen_criterion_once() {
    let vectors = qualification_derived_access_lifecycle_vectors_v1();
    assert_eq!(
        vectors
            .iter()
            .map(|vector| vector.criterion)
            .collect::<std::collections::BTreeSet<_>>(),
        QualificationDerivedAccessLifecycleCriterionV1::ALL
            .into_iter()
            .collect()
    );
    assert_eq!(
        vectors.len(),
        QualificationDerivedAccessLifecycleCriterionV1::ALL.len()
    );
}

#[test]
fn retained_bootstrap_modes_never_materialize_roots() {
    for tier in [
        QualificationDerivedAccessTierV1::L7,
        QualificationDerivedAccessTierV1::L100,
        QualificationDerivedAccessTierV1::C262,
    ] {
        let request = QualificationDerivedAccessRetainedRootRequestV1::new(
            "/tmp/source-checkout",
            QualificationDerivedAccessExpectedAuthorityV1::test_fixture().execution,
            tier,
            "/tmp/immutable-input",
            "/tmp/qualification-clone",
            digest(20),
        );
        assert!(request.validate().is_ok());
        assert!(!request.materialize);
    }

    for tier in [
        QualificationDerivedAccessTierV1::D0_128,
        QualificationDerivedAccessTierV1::L1,
    ] {
        let request = QualificationDerivedAccessRetainedRootRequestV1::new(
            "/tmp/source-checkout",
            QualificationDerivedAccessExpectedAuthorityV1::test_fixture().execution,
            tier,
            "/tmp/immutable-input",
            "/tmp/qualification-clone",
            digest(20),
        );
        assert!(request.validate().is_err());
    }
}

#[test]
fn scale_mode_remains_limited_to_l100_and_c262() {
    let execution = QualificationDerivedAccessExpectedAuthorityV1::test_fixture().execution;
    let request = QualificationDerivedAccessScaleRunRequestV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_SCALE_REQUEST_SCHEMA_V1.to_owned(),
        tier: QualificationDerivedAccessTierV1::L7,
        source_checkout: PathBuf::from("/tmp/source-checkout"),
        root_authority_sha256: execution.root_provenance_sha256.clone(),
        execution,
        roots: vec![
            QualificationDerivedAccessScaleRootV1 {
                root: PathBuf::from("/tmp/root-a"),
                admitted_root_sha256: digest(20),
            },
            QualificationDerivedAccessScaleRootV1 {
                root: PathBuf::from("/tmp/root-b"),
                admitted_root_sha256: digest(21),
            },
        ],
        l100_selected_work: std::collections::BTreeMap::new(),
    };
    assert!(request.validate().is_err());
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn retained_bootstrap_runner_preserves_authoritative_inventory() {
    let workspace = tempfile::tempdir().expect("retained bootstrap workspace");
    let immutable = workspace.path().join("immutable");
    let qualification_clone = workspace.path().join("qualification-clone");
    let materialized =
        materialize_qualification_derived_access_d0_pair_v1(&immutable, &qualification_clone)
            .expect("materialize deterministic roots");
    let source_checkout = workspace.path().join("source");
    initialize_clean_test_source_checkout(&source_checkout);
    let execution = super::evidence::observe_current_execution_identity_for_test_v1(
        native_test_platform(),
        digest(21),
        &source_checkout,
        &qualification_clone,
        digest(31),
    )
    .expect("observe exact test execution");
    let request = QualificationDerivedAccessRetainedRootRequestV1::new(
        &source_checkout,
        execution,
        QualificationDerivedAccessTierV1::L7,
        &immutable,
        &qualification_clone,
        materialized.root_a.store_inventory.inventory_sha256,
    );
    let request_path = workspace.path().join("retained-request.json");
    std::fs::write(
        &request_path,
        serde_json::to_vec_pretty(&request).expect("serialize retained request"),
    )
    .expect("write retained request");

    let receipt =
        super::evidence::bootstrap_qualification_derived_access_retained_root_for_test_v1(
            &request_path,
            digest(31),
        )
        .expect("governed derived publication preserves authoritative truth");
    assert_eq!(receipt.tier, QualificationDerivedAccessTierV1::L7);
    assert_eq!(receipt.immutable_before, receipt.immutable_after);
    assert_eq!(receipt.clone_truth_before, receipt.clone_truth_after);
    assert!(receipt.full_replay_matches_incremental);
    assert_eq!(receipt.progress_completed, receipt.progress_total);
}

#[cfg(feature = "longitudinal-counting")]
#[test]
fn candidate_open_preserves_admitted_truth_and_accounts_for_governed_namespaces() {
    let workspace = tempfile::tempdir().expect("candidate inventory workspace");
    let root_a = workspace.path().join("root-a");
    let root_b = workspace.path().join("root-b");
    let materialized = materialize_qualification_derived_access_d0_pair_v1(&root_a, &root_b)
        .expect("materialize deterministic roots");
    let admitted = materialized.root_a.store_inventory;
    let store = crate::session::store_dir_for_repo(&root_a).expect("resolve store");
    let store_id = CursorLedgerIdentity::new(format!(
        "store:derived-access:{}",
        admitted.inventory_sha256
    ));

    let cursor = SqliteCursorLedger::bootstrap_from_truth(&store, store_id.clone(), 1, |_| {
        BootstrapControl::Continue
    })
    .expect("bootstrap candidate");
    drop(cursor);
    assert_eq!(
        crate::bench_support::longitudinal::longitudinal_authoritative_store_data_inventory_v1(
            &root_a
        )
        .expect("authoritative inventory after bootstrap"),
        admitted
    );

    let before_open =
        crate::bench_support::longitudinal::longitudinal_authoritative_store_data_inventory_v1(
            &root_a,
        )
        .expect("authoritative inventory before open");
    drop(
        QualificationDerivedAccessAdapter::open(&store, store_id)
            .expect("open existing candidate adapter"),
    );
    let after_open =
        crate::bench_support::longitudinal::longitudinal_authoritative_store_data_inventory_v1(
            &root_a,
        )
        .expect("authoritative inventory after open");
    assert_eq!(
        before_open, after_open,
        "resource before/after and scale admitted-root authority remain stable"
    );

    let active_bytes =
        super::evidence::governed_derived_state_bytes(&store).expect("active derived bytes");
    let layout = super::DerivedStorageLayout::resolve(&store).expect("derived layout");
    let quarantine = layout.quarantine("42-7");
    std::fs::create_dir_all(&quarantine).expect("create governed quarantine");
    std::fs::write(quarantine.join("cursor.sqlite3"), b"quarantine")
        .expect("write governed quarantine");
    let with_quarantine =
        super::evidence::governed_derived_state_bytes(&store).expect("quarantine derived bytes");
    assert_eq!(
        with_quarantine,
        active_bytes + b"quarantine".len() as u64,
        "well-formed quarantine bytes enter derived high-water accounting"
    );

    let malformed = layout.quarantine("pid-7");
    std::fs::create_dir_all(&malformed).expect("create malformed quarantine");
    std::fs::write(malformed.join("cursor.sqlite3"), b"not derived")
        .expect("write malformed quarantine");
    assert_eq!(
        super::evidence::governed_derived_state_bytes(&store)
            .expect("malformed quarantine accounting"),
        with_quarantine,
        "malformed lookalikes do not enter derived accounting"
    );

    let event = std::fs::read_dir(store.join("events"))
        .expect("read event carriers")
        .next()
        .expect("event carrier")
        .expect("read event carrier")
        .path();
    let mut bytes = std::fs::read(&event).expect("read event carrier bytes");
    bytes.push(b'\n');
    std::fs::write(&event, bytes).expect("mutate authoritative carrier");
    assert_ne!(
        crate::bench_support::longitudinal::longitudinal_authoritative_store_data_inventory_v1(
            &root_a
        )
        .expect("authoritative inventory after carrier mutation"),
        admitted,
        "real authoritative carrier mutation remains visible"
    );
}

#[cfg(target_os = "macos")]
fn native_test_platform() -> QualificationDerivedAccessPlatformV1 {
    QualificationDerivedAccessPlatformV1::MacosApfs
}

#[cfg(target_os = "windows")]
fn native_test_platform() -> QualificationDerivedAccessPlatformV1 {
    QualificationDerivedAccessPlatformV1::WindowsNtfs
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn initialize_clean_test_source_checkout(root: &std::path::Path) {
    std::fs::create_dir_all(root).expect("create test source checkout");
    std::fs::write(root.join("Cargo.lock"), b"test lock\n").expect("write test Cargo.lock");
    for arguments in [
        vec!["init", "-q"],
        vec!["add", "Cargo.lock"],
        vec![
            "-c",
            "user.name=Pointbreak Tests",
            "-c",
            "user.email=tests@pointbreak.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "test source",
        ],
    ] {
        assert!(
            Command::new("git")
                .args(arguments)
                .current_dir(root)
                .status()
                .expect("run git for test source")
                .success()
        );
    }
}

#[test]
fn scale_receipts_rederive_aggregates_and_reject_missing_raw_samples() {
    let execution = QualificationDerivedAccessExpectedAuthorityV1::test_fixture().execution;
    let tier = QualificationDerivedAccessTierV1::L100;
    let mut raw_samples = Vec::new();
    for operation in QualificationDerivedAccessOperationV1::ALL {
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
        for root in [0_u16, 100_u16] {
            for index in local.clone() {
                let mut sample = QualificationDerivedAccessRawSampleV1::test_fixture();
                sample.tier = tier;
                sample.operation = operation;
                sample.sample_index = root + index;
                sample.retained_cardinality = 102_400;
                sample.authoritative_bytes_published =
                    u64::from(operation == QualificationDerivedAccessOperationV1::AppendOne);
                sample.sqlite.database_size_delta_bytes =
                    u64::from(operation == QualificationDerivedAccessOperationV1::AppendOne);
                raw_samples.push(sample);
            }
        }
    }
    let mut active_samples = raw_samples
        .iter_mut()
        .filter(|sample| {
            sample.operation == QualificationDerivedAccessOperationV1::RevisionDetailActive
        })
        .take(2)
        .collect::<Vec<_>>();
    active_samples[0].selected_output_count = 10;
    active_samples[0].selected_work_count = 15;
    active_samples[1].selected_output_count = 20;
    active_samples[1].selected_work_count = 20;
    let l100_selected_work = std::collections::BTreeMap::new();
    let operation_rows =
        aggregate_scale_rows(tier, execution.platform, &l100_selected_work, &raw_samples)
            .expect("derive scale aggregates");
    let active = operation_rows
        .iter()
        .find(|row| row.operation == QualificationDerivedAccessOperationV1::RevisionDetailActive)
        .expect("active revision-detail aggregate");
    assert_eq!(active.selected_work_count, 20);
    assert_eq!(active.selected_output_count, Some(20));
    assert_eq!(
        active.unselected_work_count,
        Some(5),
        "the aggregate must retain the worst per-sample unexplained work"
    );
    let inventories = vec![
        QualificationDerivedAccessDerivedInventoryV1 {
            database_bytes: 1,
            wal_bytes: 0,
            shared_memory_bytes: 0,
            temporary_bytes: 0,
            row_count: 1,
            page_count: 1,
            body_bytes: 0,
            object_bytes: 0,
            high_water_bytes: 1,
        };
        2
    ];
    let allocation =
        derive_scale_allocation(tier, &inventories, &raw_samples).expect("derive allocation");
    let mut receipt = QualificationDerivedAccessScaleReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_SCALE_RECEIPT_SCHEMA_V1.to_owned(),
        execution,
        tier,
        l100_selected_work,
        raw_samples,
        operation_rows,
        allocation,
        derived_inventories: inventories,
        root_before: vec![
            crate::bench_support::longitudinal::LongitudinalStoreDataInventoryV1 {
                file_count: 1,
                byte_count: 1,
                inventory_sha256: digest(30),
            };
            2
        ],
        root_after: vec![
            crate::bench_support::longitudinal::LongitudinalStoreDataInventoryV1 {
                file_count: 1,
                byte_count: 1,
                inventory_sha256: digest(30),
            };
            2
        ],
    };
    validate_scale_receipt(&receipt).expect("valid scale receipt");

    let fragment_root = tempfile::tempdir().expect("fragment root");
    let receipt_path = fragment_root.path().join("scale-receipt.json");
    let request_path = fragment_root.path().join("fragment-request.json");
    std::fs::write(
        &receipt_path,
        serde_json::to_vec_pretty(&receipt).expect("serialize scale receipt"),
    )
    .expect("write scale receipt");
    std::fs::write(
        &request_path,
        serde_json::to_vec_pretty(&QualificationDerivedAccessFragmentRequestV1 {
            schema: QUALIFICATION_DERIVED_ACCESS_FRAGMENT_REQUEST_SCHEMA_V1.to_owned(),
            execution: receipt.execution.clone(),
            receipt_paths: vec![receipt_path.clone()],
            evaluator_revision: QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3.to_owned(),
        })
        .expect("serialize fragment request"),
    )
    .expect("write fragment request");
    build_qualification_derived_access_fragment_v1(&request_path)
        .expect("valid scale receipt crosses fragment boundary");

    receipt.operation_rows[0].wall_p95_ms = Some(999_000);
    assert!(validate_scale_receipt(&receipt).is_err());
    std::fs::write(
        &receipt_path,
        serde_json::to_vec_pretty(&receipt).expect("serialize forged receipt"),
    )
    .expect("write forged receipt");
    assert!(build_qualification_derived_access_fragment_v1(&request_path).is_err());
    receipt.operation_rows = aggregate_scale_rows(
        tier,
        receipt.execution.platform,
        &receipt.l100_selected_work,
        &receipt.raw_samples,
    )
    .expect("restore scale aggregates");
    receipt.raw_samples.clear();
    assert!(validate_scale_receipt(&receipt).is_err());
}

#[cfg(feature = "longitudinal-counting")]
#[test]
fn bound_smoke_fragment_assembles_into_a_verified_incomplete_evidence_package() {
    let root = tempfile::tempdir().expect("fragment workspace");
    let smoke =
        run_qualification_derived_access_non_timing_smoke_at_v1(&root.path().join("d0-workspace"))
            .expect("D0 smoke");
    let execution = QualificationDerivedAccessExpectedAuthorityV1::test_fixture().execution;
    let receipt = QualificationDerivedAccessNativeSmokeRunReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_NATIVE_SMOKE_RECEIPT_SCHEMA_V1.to_owned(),
        execution: execution.clone(),
        payload: QualificationDerivedAccessNativeSmokePayloadV1::D0_128(Box::new(smoke)),
    };
    let receipt_path = root.path().join("native-smoke.json");
    std::fs::write(
        &receipt_path,
        serde_json::to_vec_pretty(&receipt).expect("serialize smoke receipt"),
    )
    .expect("write smoke receipt");
    let request = QualificationDerivedAccessFragmentRequestV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_FRAGMENT_REQUEST_SCHEMA_V1.to_owned(),
        execution,
        receipt_paths: vec![receipt_path],
        evaluator_revision: QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3.to_owned(),
    };
    let request_path = root.path().join("fragment-request.json");
    std::fs::write(
        &request_path,
        serde_json::to_vec_pretty(&request).expect("serialize fragment request"),
    )
    .expect("write fragment request");
    let fragment =
        build_qualification_derived_access_fragment_v1(&request_path).expect("build fragment");
    let fragment_path = root.path().join("fragment.json");
    std::fs::write(
        &fragment_path,
        serde_json::to_vec_pretty(&fragment).expect("serialize fragment"),
    )
    .expect("write fragment");
    let package_root = root.path().join("package");
    let evaluation =
        assemble_qualification_derived_access_package_v1(&[fragment_path], &package_root)
            .expect("assemble package");
    assert_eq!(
        evaluation.outcome,
        QualificationDerivedAccessTerminalOutcomeV1::InsufficientEvidence
    );
    verify_qualification_derived_access_package_v1(&package_root).expect("verify package");
}

#[cfg(feature = "longitudinal-counting")]
#[test]
fn fragment_request_revision_authors_v4_and_assembly_still_refuses_mixed_authority() {
    let root = tempfile::tempdir().expect("fragment workspace");
    let smoke =
        run_qualification_derived_access_non_timing_smoke_at_v1(&root.path().join("d0-workspace"))
            .expect("D0 smoke");
    let execution = QualificationDerivedAccessExpectedAuthorityV1::test_fixture().execution;
    let receipt = QualificationDerivedAccessNativeSmokeRunReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_NATIVE_SMOKE_RECEIPT_SCHEMA_V1.to_owned(),
        execution: execution.clone(),
        payload: QualificationDerivedAccessNativeSmokePayloadV1::D0_128(Box::new(smoke)),
    };
    let receipt_path = root.path().join("native-smoke.json");
    std::fs::write(
        &receipt_path,
        serde_json::to_vec_pretty(&receipt).expect("serialize smoke receipt"),
    )
    .expect("write smoke receipt");

    let write_request = |name: &str, revision: &str| {
        let request_path = root.path().join(name);
        std::fs::write(
            &request_path,
            serde_json::to_vec_pretty(&QualificationDerivedAccessFragmentRequestV1 {
                schema: QUALIFICATION_DERIVED_ACCESS_FRAGMENT_REQUEST_SCHEMA_V1.to_owned(),
                execution: execution.clone(),
                receipt_paths: vec![receipt_path.clone()],
                evaluator_revision: revision.to_owned(),
            })
            .expect("serialize fragment request"),
        )
        .expect("write fragment request");
        request_path
    };

    // An unsupported revision is refused before any receipt is read.
    let unsupported = write_request("fragment-request-unsupported.json", "evaluator.v9");
    let error = build_qualification_derived_access_fragment_v1(&unsupported)
        .expect_err("unsupported revision must refuse");
    assert!(error.contains("unsupported evaluator revision"), "{error}");

    // A request that omits the field keeps the historical v3 meaning.
    let default_path = root.path().join("fragment-request-default.json");
    std::fs::write(
        &default_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": QUALIFICATION_DERIVED_ACCESS_FRAGMENT_REQUEST_SCHEMA_V1,
            "execution": execution,
            "receiptPaths": [receipt_path],
        }))
        .expect("serialize default request"),
    )
    .expect("write default request");
    let v3_fragment =
        build_qualification_derived_access_fragment_v1(&default_path).expect("build v3 fragment");
    assert_eq!(
        v3_fragment.package.evaluator_revision,
        QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V3
    );

    // A change-read-free receipt can now be authored under v4 with the v4
    // procedure binding, so it can join the v2 Change-read fragment's package.
    let v4_request = write_request(
        "fragment-request-v4.json",
        QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4,
    );
    let v4_fragment =
        build_qualification_derived_access_fragment_v1(&v4_request).expect("build v4 fragment");
    assert_eq!(
        v4_fragment.package.evaluator_revision,
        QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4
    );
    assert_eq!(
        v4_fragment.package.evaluator_procedure_sha256,
        qualification_derived_access_evaluator_v4_procedure_sha256()
    );

    let v3_path = root.path().join("fragment-v3.json");
    std::fs::write(
        &v3_path,
        serde_json::to_vec_pretty(&v3_fragment).expect("serialize v3 fragment"),
    )
    .expect("write v3 fragment");
    let v4_path = root.path().join("fragment-v4.json");
    std::fs::write(
        &v4_path,
        serde_json::to_vec_pretty(&v4_fragment).expect("serialize v4 fragment"),
    )
    .expect("write v4 fragment");

    // Mixed revisions still refuse assembly.
    let mixed = assemble_qualification_derived_access_package_v1(
        &[v4_path.clone(), v3_path],
        &root.path().join("mixed-package"),
    )
    .expect_err("mixed revisions must refuse assembly");
    assert!(mixed.contains("mix package authority"), "{mixed}");

    // A v4 package assembles and independently verifies from the v4 fragment.
    let package_root = root.path().join("v4-package");
    let evaluation = assemble_qualification_derived_access_package_v1(&[v4_path], &package_root)
        .expect("assemble v4 package");
    assert_eq!(
        evaluation.evaluator_revision,
        QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4
    );
    assert_eq!(
        evaluation.outcome,
        QualificationDerivedAccessTerminalOutcomeV1::InsufficientEvidence
    );
    verify_qualification_derived_access_package_v1(&package_root).expect("verify v4 package");
}

#[cfg(feature = "longitudinal-counting")]
#[test]
fn shared_campaign_provenance_lets_both_host_lanes_co_assemble() {
    // The static co-assembly falsifier: stub identities for every planned
    // host lane run through the real package guard before an evidence
    // campaign spends machine time, so an identity-design gap surfaces here
    // instead of after both lanes' receipts are retained.
    let root = tempfile::tempdir().expect("falsifier workspace");
    let smoke =
        run_qualification_derived_access_non_timing_smoke_at_v1(&root.path().join("d0-workspace"))
            .expect("D0 smoke");
    let campaign_provenance = digest(0xca);
    let mut apfs = QualificationDerivedAccessExpectedAuthorityV1::test_fixture().execution;
    apfs.root_provenance_sha256 = campaign_provenance.clone();
    apfs.validate().expect("apfs lane identity is admissible");
    let mut ntfs = apfs.clone();
    ntfs.platform = QualificationDerivedAccessPlatformV1::WindowsNtfs;
    ntfs.operating_system = "windows".to_owned();
    ntfs.filesystem = "ntfs".to_owned();
    ntfs.architecture = "x86_64".to_owned();
    ntfs.binary_sha256 = digest(0x99);
    ntfs.command_sha256 = digest(0x88);
    ntfs.host_identity_sha256 = digest(0x78);
    ntfs.validate().expect("ntfs lane identity is admissible");

    let lane_fragment = |name: &str, execution: &QualificationDerivedAccessExecutionIdentityV1| {
        let receipt_path = root.path().join(format!("{name}-receipt.json"));
        std::fs::write(
            &receipt_path,
            serde_json::to_vec_pretty(&QualificationDerivedAccessNativeSmokeRunReceiptV1 {
                schema: QUALIFICATION_DERIVED_ACCESS_NATIVE_SMOKE_RECEIPT_SCHEMA_V1.to_owned(),
                execution: execution.clone(),
                payload: QualificationDerivedAccessNativeSmokePayloadV1::D0_128(Box::new(
                    smoke.clone(),
                )),
            })
            .expect("serialize lane receipt"),
        )
        .expect("write lane receipt");
        let request_path = root.path().join(format!("{name}-request.json"));
        std::fs::write(
            &request_path,
            serde_json::to_vec_pretty(&QualificationDerivedAccessFragmentRequestV1 {
                schema: QUALIFICATION_DERIVED_ACCESS_FRAGMENT_REQUEST_SCHEMA_V1.to_owned(),
                execution: execution.clone(),
                receipt_paths: vec![receipt_path],
                evaluator_revision: QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4.to_owned(),
            })
            .expect("serialize lane fragment request"),
        )
        .expect("write lane fragment request");
        let fragment = build_qualification_derived_access_fragment_v1(&request_path)
            .expect("build lane fragment");
        let fragment_path = root.path().join(format!("{name}-fragment.json"));
        std::fs::write(
            &fragment_path,
            serde_json::to_vec_pretty(&fragment).expect("serialize lane fragment"),
        )
        .expect("write lane fragment");
        fragment_path
    };

    // Both lanes share the campaign provenance, so they co-assemble into one
    // verified package.
    let apfs_fragment = lane_fragment("apfs", &apfs);
    let ntfs_fragment = lane_fragment("ntfs", &ntfs);
    let package_root = root.path().join("package");
    let evaluation = assemble_qualification_derived_access_package_v1(
        &[apfs_fragment.clone(), ntfs_fragment],
        &package_root,
    )
    .expect("shared-provenance lanes must co-assemble");
    assert_eq!(
        evaluation.evaluator_revision,
        QUALIFICATION_DERIVED_ACCESS_EVALUATOR_REVISION_V4
    );
    verify_qualification_derived_access_package_v1(&package_root).expect("verify package");

    // The control: a lane keeping its own per-root provenance still refuses
    // with the exact mixing guard.
    let mut per_root = ntfs.clone();
    per_root.root_provenance_sha256 = digest(0x56);
    let per_root_fragment = lane_fragment("ntfs-per-root", &per_root);
    let refused = assemble_qualification_derived_access_package_v1(
        &[apfs_fragment, per_root_fragment],
        &root.path().join("refused-package"),
    )
    .expect_err("per-lane provenance must refuse assembly");
    assert!(refused.contains("mix source authority"), "{refused}");
}

#[test]
fn change_read_root_provenance_override_binds_the_reference_root_only() {
    let sha256_hex = |bytes: &[u8]| -> String {
        use sha2::{Digest as _, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    };
    let authority_sha256 = digest(41);
    let reference_root = Path::new("/campaign/root/repo");
    let fault_root = Path::new("/campaign/root/fault-repo");
    let derived =
        |path: &Path| sha256_hex(format!("{authority_sha256}:{}", path.display()).as_bytes());

    // Without an override both roots derive per-root provenance from the
    // authority record and their exact paths, and the values stay distinct.
    let (reference, fault) = qualification_change_read_root_provenances_v1(
        &authority_sha256,
        reference_root,
        fault_root,
        None,
    )
    .expect("derive per-root provenance");
    assert_eq!(reference, derived(reference_root));
    assert_eq!(fault, derived(fault_root));
    assert_ne!(reference, fault);

    // A campaign override replaces the reference root's provenance only; the
    // fault root keeps its derived per-root value.
    let campaign = digest(7);
    let (reference, fault_with_override) = qualification_change_read_root_provenances_v1(
        &authority_sha256,
        reference_root,
        fault_root,
        Some(&campaign),
    )
    .expect("apply campaign override");
    assert_eq!(reference, campaign);
    assert_eq!(fault_with_override, fault);

    // Malformed overrides are refused before any value is minted.
    let truncated = &digest(7)[..63];
    let uppercase = digest(0xab).to_uppercase();
    let overlong = format!("{}0", digest(7));
    for malformed in ["not-hex", truncated, &uppercase, &overlong] {
        let error = qualification_change_read_root_provenances_v1(
            &authority_sha256,
            reference_root,
            fault_root,
            Some(malformed),
        )
        .expect_err("malformed override must refuse");
        assert!(
            error.contains("campaign root provenance override"),
            "{error}"
        );
    }

    // An override colliding with the fault root's derived provenance is
    // refused: the clone protocol requires per-root distinctness.
    let collision = qualification_change_read_root_provenances_v1(
        &authority_sha256,
        reference_root,
        fault_root,
        Some(&fault),
    )
    .expect_err("fault collision must refuse");
    assert!(collision.contains("fault root provenance"), "{collision}");

    // A malformed authority digest is refused in every mode.
    let error =
        qualification_change_read_root_provenances_v1("bogus", reference_root, fault_root, None)
            .expect_err("malformed authority must refuse");
    assert!(error.contains("campaign authority"), "{error}");
}
