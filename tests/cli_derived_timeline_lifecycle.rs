#![cfg(all(
    feature = "longitudinal-counting",
    any(target_os = "macos", target_os = "windows")
))]

mod support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pointbreak::bench_support::derived_access::{
    DerivedTimelineInvalidSignatureDiagnosticFaultV1,
    QualificationDerivedTimelineFaultSeedInjectionV1, QualificationDerivedTimelineFaultSeedPlanV1,
    QualificationDerivedTimelineFaultSeedReceiptV1,
    run_timeline_invalid_signature_lifecycle_diagnostic_v1,
    seed_qualification_derived_timeline_fault_root_v1,
    seed_qualification_derived_timeline_fault_root_with_injection_v1,
};
use pointbreak::bench_support::longitudinal::longitudinal_authoritative_store_data_inventory_v1;
use pointbreak::session::allowed_signers_path_for_repo;
use support::inspect::{DecisionContinuityMatrix, decision_continuity_matrix};

fn optional_bytes(path: &Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("read {}: {error}", path.display()),
    }
}

fn snapshot_root(repo: &Path) -> (String, Option<Vec<u8>>) {
    let inventory = longitudinal_authoritative_store_data_inventory_v1(repo)
        .expect("authoritative inventory")
        .inventory_sha256;
    let trust_path = allowed_signers_path_for_repo(repo).expect("trust path");
    (inventory, optional_bytes(&trust_path))
}

fn snapshot(matrix: &DecisionContinuityMatrix) -> (String, Option<Vec<u8>>) {
    snapshot_root(matrix.repo())
}

fn assert_barrier_empty(path: &Path) {
    let entries = std::fs::read_dir(path)
        .expect("read barrier root")
        .collect::<Result<Vec<_>, _>>()
        .expect("read barrier entries");
    assert!(entries.is_empty(), "barrier artifacts must be cleaned up");
}

/// One byte-cloned fault authority seeded from the canonical reference matrix
/// before any governed derived state beyond idle zero-byte writer locks and
/// before any staged lifecycle trust exists.
struct ClonedFaultAuthority {
    _root: tempfile::TempDir,
    repo: PathBuf,
    pointbreak_home: PathBuf,
    fixture_witness: PathBuf,
    receipt: QualificationDerivedTimelineFaultSeedReceiptV1,
}

fn cloned_fault_authority(reference: &DecisionContinuityMatrix) -> ClonedFaultAuthority {
    let root = tempfile::tempdir().expect("fault-seed root");
    let repo = root.path().join("repository");
    let fixture_witness = root.path().join("fixture-witness.json");
    let receipt = seed_qualification_derived_timeline_fault_root_v1(
        &QualificationDerivedTimelineFaultSeedPlanV1 {
            reference_repository: reference.repo().to_owned(),
            reference_fixture_witness: reference.fixture_witness().to_owned(),
            fault_repository: repo.clone(),
            fault_fixture_witness: fixture_witness.clone(),
        },
    )
    .expect("seed the byte-cloned fault authority");
    let pointbreak_home = repo.join(".git/pointbreak-home");
    ClonedFaultAuthority {
        _root: root,
        repo,
        pointbreak_home,
        fixture_witness,
        receipt,
    }
}

fn run(
    reference: &DecisionContinuityMatrix,
    fault_authority: &ClonedFaultAuthority,
    barrier_root: &Path,
    fault: DerivedTimelineInvalidSignatureDiagnosticFaultV1,
) -> Result<
    pointbreak::bench_support::derived_access::QualificationDerivedTimelineReadEvidenceV1,
    String,
> {
    run_timeline_invalid_signature_lifecycle_diagnostic_v1(
        reference.repo(),
        reference.pointbreak_home(),
        reference.fixture_witness(),
        &fault_authority.repo,
        &fault_authority.pointbreak_home,
        &fault_authority.fixture_witness,
        barrier_root,
        Path::new(env!("CARGO_BIN_EXE_pointbreak")),
        &fault_authority.receipt,
        fault,
    )
}

#[test]
fn timeline_invalid_signature_lifecycle_uses_fresh_pairs_and_same_counted_child() {
    let reference = decision_continuity_matrix();
    let fault_authority = cloned_fault_authority(&reference);
    let barrier = tempfile::tempdir().expect("barrier root");
    let before = snapshot(&reference);
    let fault_before = snapshot_root(&fault_authority.repo);
    assert_eq!(
        before.0, fault_authority.receipt.authoritative_inventory_sha256,
        "the clone seed must bind the canonical authoritative inventory"
    );
    assert_eq!(
        before, fault_before,
        "the cloned fault root must start from the canonical authority"
    );
    let row = run(
        &reference,
        &fault_authority,
        barrier.path(),
        DerivedTimelineInvalidSignatureDiagnosticFaultV1::None,
    )
    .expect("real Timeline invalid-signature lifecycle");
    let after = snapshot(&reference);
    assert_eq!(
        after, before,
        "reference lifecycle must restore source and trust bytes"
    );
    assert_eq!(
        snapshot_root(&fault_authority.repo),
        fault_before,
        "fault lifecycle must restore source and trust bytes"
    );
    assert_barrier_empty(barrier.path());

    let failure = row
        .invalid_signature_failure
        .expect("invalid-signature lifecycle evidence");
    failure
        .fault_seed_receipt
        .validate()
        .expect("valid fault-seed receipt in evidence");
    assert_eq!(failure.fault_seed_receipt, fault_authority.receipt);
    assert_eq!(
        failure.fault_seed_receipt.authoritative_inventory_sha256,
        failure.reference_inventory_sha256
    );
    assert_eq!(
        failure.fault_seed_receipt.witness_sha256,
        failure.reference_fixture_witness_sha256
    );
    assert_ne!(
        failure.fault_seed_receipt.reference_root_path_sha256,
        failure.fault_seed_receipt.fault_root_path_sha256
    );
    assert_eq!(
        failure
            .phase_process_identity_sha256
            .iter()
            .collect::<BTreeSet<_>>()
            .len(),
        6
    );
    assert_eq!(failure.phase_http_status, [200, 200, 503, 200, 200, 200]);
    assert_ne!(
        failure.reference_root_identity_sha256,
        failure.fault_execution.root_provenance_sha256
    );
    assert_eq!(
        failure.reference_inventory_sha256,
        failure.reference_recovery_inventory_sha256
    );
    assert_eq!(
        failure.reference_inventory_sha256,
        failure.fault_clean_inventory_sha256
    );
    assert_eq!(
        failure.fault_clean_inventory_sha256,
        failure.fault_restored_inventory_sha256
    );
    assert_ne!(
        failure.fault_clean_inventory_sha256,
        failure.fault_derivative_inventory_sha256
    );
    assert_eq!(
        failure.clean_semantic_sha256,
        failure.strict_clean_semantic_sha256
    );
    assert_eq!(
        failure.clean_semantic_sha256,
        failure.derived_recovery_semantic_sha256
    );
    assert_eq!(
        failure.strict_clean_semantic_sha256,
        failure.strict_recovery_semantic_sha256
    );
    assert_eq!(failure.clean_signature_status, "valid");
    assert_eq!(failure.mutated_signature_status, "invalid");
    assert_eq!(failure.strict_observed_signature_status, "invalid");
    assert_eq!(failure.recovery_signature_status, "valid");
    assert_eq!(
        failure.clean_event_record_hash,
        failure.mutated_event_record_hash
    );
    assert_ne!(failure.clean_carrier_sha256, failure.mutated_carrier_sha256);
    for reference_lane in [1, 3, 4] {
        assert_eq!(
            failure.phase_authority_cursor_sha256[reference_lane],
            failure.phase_authority_cursor_sha256[0],
            "reference lanes must share the clean authority cursor"
        );
    }
    assert_ne!(
        failure.phase_authority_cursor_sha256[2], failure.phase_authority_cursor_sha256[0],
        "the mutated strict lane must witness the one-bit carrier mutation"
    );
    for stamps in [
        &failure.phase_source_change_projection_stamp,
        &failure.phase_timeline_projection_stamp,
    ] {
        assert_eq!(stamps[0], stamps[3]);
        assert_eq!(stamps[1], stamps[4]);
    }
    let barrier = &failure.barrier_receipt;
    barrier.validate().expect("valid post-pin barrier receipt");
    assert_eq!(barrier.boundary.as_str(), "carrier_locators_selected");
    assert_eq!(barrier.carrier_opens_before, 0);
    assert!(barrier.selected_carriers_before > 0);
    assert_eq!(
        barrier.expected_carrier_key_digest,
        barrier.observed_mismatch_key_digest
    );
    assert_eq!(barrier.mismatch_kind.as_str(), "validation_witness");
    assert_eq!(failure.counter_receipt.run_identity, barrier.run_identity);
    assert_eq!(
        failure.counter_receipt.manifest_sha256,
        barrier.barrier_identity_sha256
    );
    assert_eq!(
        failure.counter_receipt.semantic_result_sha256,
        failure.observed_typed_document.canonical_sha256
    );
    let counters = &failure.counter_receipt.counters;
    assert!(counters.carrier_opens > 0);
    assert!(counters.timeline_selected_carriers > 0);
    assert_eq!(counters.authoritative_fallbacks, 0);
    assert_eq!(counters.full_history_fallbacks, 0);
    assert_eq!(counters.event_folds, 0);
    assert_eq!(counters.projection_rebuilds, 0);
    assert_eq!(counters.state_rebuilds, 0);
    assert_eq!(counters.body_artifact_reads, 0);
    assert_eq!(counters.body_bytes_read, 0);
    assert_eq!(counters.object_artifact_reads, 0);
    assert_eq!(counters.object_bytes_read, 0);
    assert_eq!(counters.timeline_trust_support_carriers, 0);
    assert_eq!(counters.timeline_entries_emitted, 0);
}

#[test]
fn timeline_invalid_signature_post_mutation_failure_restores_source() {
    let reference = decision_continuity_matrix();
    let fault_authority = cloned_fault_authority(&reference);
    let barrier = tempfile::tempdir().expect("barrier root");
    let before = snapshot(&reference);
    let fault_before = snapshot_root(&fault_authority.repo);
    let error = run(
        &reference,
        &fault_authority,
        barrier.path(),
        DerivedTimelineInvalidSignatureDiagnosticFaultV1::AfterCarrierWrite,
    )
    .expect_err("post-mutation fault must stop the diagnostic");
    assert!(error.contains("injected post-carrier-write failure"));
    assert_eq!(snapshot(&reference), before);
    assert_eq!(snapshot_root(&fault_authority.repo), fault_before);
    assert_barrier_empty(barrier.path());
}

#[test]
fn timeline_trust_prepublication_failures_restore_authority() {
    for fault in [
        DerivedTimelineInvalidSignatureDiagnosticFaultV1::AfterTrustStageIdentityRead,
        DerivedTimelineInvalidSignatureDiagnosticFaultV1::TrustStageReportsUnchangedIdentity,
    ] {
        let reference = decision_continuity_matrix();
        let fault_authority = cloned_fault_authority(&reference);
        let barrier = tempfile::tempdir().expect("barrier root");
        let before = snapshot(&reference);
        let fault_before = snapshot_root(&fault_authority.repo);
        let error = run(&reference, &fault_authority, barrier.path(), fault)
            .expect_err("post-stage fault must stop the diagnostic");
        match fault {
            DerivedTimelineInvalidSignatureDiagnosticFaultV1::AfterTrustStageIdentityRead => {
                assert!(error.contains("injected post-stage identity read failure"));
            }
            DerivedTimelineInvalidSignatureDiagnosticFaultV1::TrustStageReportsUnchangedIdentity => {
                assert!(error.contains("Timeline trust stage did not change authority identity"));
            }
            _ => unreachable!(),
        }
        assert_eq!(snapshot(&reference), before);
        assert_eq!(snapshot_root(&fault_authority.repo), fault_before);
        assert_barrier_empty(barrier.path());
    }
}

#[test]
fn timeline_fault_seed_clone_rejects_unsafe_destinations_and_post_clone_drift() {
    let reference = decision_continuity_matrix();

    // The staging destination must be absent before the clone begins.
    let occupied = tempfile::tempdir().expect("occupied fault-seed root");
    std::fs::create_dir(occupied.path().join("repository")).expect("occupy destination");
    let error = seed_qualification_derived_timeline_fault_root_v1(
        &QualificationDerivedTimelineFaultSeedPlanV1 {
            reference_repository: reference.repo().to_owned(),
            reference_fixture_witness: reference.fixture_witness().to_owned(),
            fault_repository: occupied.path().join("repository"),
            fault_fixture_witness: occupied.path().join("fixture-witness.json"),
        },
    )
    .expect_err("existing destination must be rejected");
    assert!(
        error.contains("must be absent"),
        "unexpected error: {error}"
    );

    // The fault authority must not overlap the reference authority.
    let error = seed_qualification_derived_timeline_fault_root_v1(
        &QualificationDerivedTimelineFaultSeedPlanV1 {
            reference_repository: reference.repo().to_owned(),
            reference_fixture_witness: reference.fixture_witness().to_owned(),
            fault_repository: reference.repo().join("nested-fault-root"),
            fault_fixture_witness: occupied.path().join("nested-witness.json"),
        },
    )
    .expect_err("a destination inside the reference authority must be rejected");
    assert!(error.contains("paths overlap"), "unexpected error: {error}");

    // Symlinked source entries are rejected rather than silently followed.
    // The rejection fires in the pre-copy manifest walk, before any path is
    // created, so the absence assertion below proves no-creation, not the
    // cleanup guard; cleanup is proven by the injected-failure case next.
    #[cfg(unix)]
    {
        let symlink_path = reference.repo().join("fault-seed-symlink-falsifier");
        std::os::unix::fs::symlink(reference.fixture_witness(), &symlink_path)
            .expect("create source symlink");
        let symlinked = tempfile::tempdir().expect("symlink fault-seed root");
        let error = seed_qualification_derived_timeline_fault_root_v1(
            &QualificationDerivedTimelineFaultSeedPlanV1 {
                reference_repository: reference.repo().to_owned(),
                reference_fixture_witness: reference.fixture_witness().to_owned(),
                fault_repository: symlinked.path().join("repository"),
                fault_fixture_witness: symlinked.path().join("fixture-witness.json"),
            },
        )
        .expect_err("a symlinked source entry must be rejected");
        assert!(
            error.contains("contains a symlink"),
            "unexpected error: {error}"
        );
        assert!(
            !symlinked.path().join("repository").exists(),
            "a rejected clone must leave its destination absent"
        );
        std::fs::remove_file(&symlink_path).expect("remove source symlink");
    }

    // An injected failure after the repository rename proves the cleanup
    // guard removes every created path, including a finalized fault
    // repository and both staging paths, leaving the destinations absent.
    {
        let injected = tempfile::tempdir().expect("injected fault-seed root");
        let fault_repository = injected.path().join("repository");
        let fault_witness = injected.path().join("fixture-witness.json");
        let error = seed_qualification_derived_timeline_fault_root_with_injection_v1(
            &QualificationDerivedTimelineFaultSeedPlanV1 {
                reference_repository: reference.repo().to_owned(),
                reference_fixture_witness: reference.fixture_witness().to_owned(),
                fault_repository: fault_repository.clone(),
                fault_fixture_witness: fault_witness.clone(),
            },
            QualificationDerivedTimelineFaultSeedInjectionV1::AfterRepositoryFinalize,
        )
        .expect_err("the injected post-finalize failure must fail the seed");
        assert!(
            error.contains("injected fault-seed failure after repository finalization"),
            "unexpected error: {error}"
        );
        for leftover in [
            fault_repository,
            fault_witness,
            injected.path().join(".repository.pointbreak-fault-seed"),
            injected
                .path()
                .join(".fixture-witness.json.pointbreak-fault-seed"),
        ] {
            assert!(
                std::fs::symlink_metadata(&leftover).is_err(),
                "cleanup must remove {}",
                leftover.display()
            );
        }
    }

    // A cloned root that drifts after seeding must fail the lifecycle binding
    // before any child, publication, trust staging, or mutation work begins.
    let fault_authority = cloned_fault_authority(&reference);
    let drifted_store_file = {
        let inventory_before = snapshot_root(&fault_authority.repo).0;
        let store_dir = pointbreak::session::store_dir_for_repo(&fault_authority.repo)
            .expect("cloned store directory");
        let drifted = store_dir.join("fault-seed-drift-falsifier.json");
        std::fs::write(&drifted, b"{\"drift\":true}").expect("drift the cloned store");
        assert_ne!(
            snapshot_root(&fault_authority.repo).0,
            inventory_before,
            "the drift falsifier must change the authoritative inventory"
        );
        drifted
    };
    let barrier = tempfile::tempdir().expect("barrier root");
    let error = run(
        &reference,
        &fault_authority,
        barrier.path(),
        DerivedTimelineInvalidSignatureDiagnosticFaultV1::None,
    )
    .expect_err("post-clone drift must stop the lifecycle");
    assert!(
        error.contains("drifted from their validated fault-seed clone"),
        "unexpected error: {error}"
    );
    assert_barrier_empty(barrier.path());
    std::fs::remove_file(drifted_store_file).expect("remove drift falsifier");
}
