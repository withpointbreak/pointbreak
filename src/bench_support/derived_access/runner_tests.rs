use std::path::PathBuf;

use super::*;

fn digest(value: u8) -> String {
    format!("{value:02x}").repeat(32)
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
fn retained_scale_modes_never_materialize_roots() {
    for tier in [
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
    let l100_selected_work = std::collections::BTreeMap::new();
    let operation_rows =
        aggregate_scale_rows(tier, execution.platform, &l100_selected_work, &raw_samples)
            .expect("derive scale aggregates");
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
