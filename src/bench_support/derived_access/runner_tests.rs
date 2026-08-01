use std::path::PathBuf;
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

#[test]
fn raw_samples_reject_ambiguous_cpu_units_and_scope() {
    let mut sample = QualificationDerivedAccessRawSampleV1::test_fixture();
    sample.process_cpu_unit = QualificationDerivedAccessCpuUnitV1::NativeTicks;
    assert!(sample.validate().is_err());

    sample.process_cpu_unit = QualificationDerivedAccessCpuUnitV1::Nanoseconds;
    sample.process_scope = None;
    assert!(sample.validate().is_err());
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
    let execution = super::evidence::observe_current_execution_identity_v1(
        native_test_platform(),
        digest(21),
        &source_checkout,
        &qualification_clone,
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

    let receipt = bootstrap_qualification_derived_access_retained_root_v1(&request_path)
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
    let quarantine = store.join(".pointbreak-derived.quarantine-42-7");
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

    let malformed = store.join(".pointbreak-derived.quarantine-pid-7");
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
