use std::ffi::OsStr;
#[cfg(unix)]
use std::ffi::OsString;

use super::product_contract::{
    AuthorityStampExpectation, AuthorityStampMutationLocus, AuthorityStampObservablePolicy,
    AuthorityStampScenarioId, BootstrapAvailabilityCase, CounterCeilingContextV1,
    DERIVED_ACCESS_PROFILE_ENV, DerivedAccessAvailability, DerivedAccessFallback,
    DerivedAccessProfile, PRODUCT_INTEGRATION_CONTRACT_SHA256_V1,
    PRODUCTION_READINESS_CONTRACT_SHA256_V1, ProductIntegrationContractFixtureV1,
    ProductParityFixtureId, ProductRouteId, ProductWorkClass, ProjectionStampComponent,
    ProjectionVersionDecision, ProtectedInputId, ReadinessCounterId, ReadinessGateId,
    ReadinessGateResultV1, ReadinessOperationId, ReadinessOutcome, RevisionCountSource,
    RevisionPageLimitOverflow, WireProjectionVersion, product_integration_contract_fixture_v1,
    product_integration_contract_publication_v1, product_integration_contract_smoke_v1,
    product_integration_contract_v1, production_readiness_contract_fixture_v1,
    production_readiness_contract_publication_v1, production_readiness_contract_smoke_v1,
    production_readiness_contract_v1, verify_product_integration_contract_fixture_v1,
    verify_production_readiness_contract_fixture_v1,
};

#[test]
fn selector_defaults_off_and_accepts_only_the_two_frozen_values() {
    assert_eq!(
        DerivedAccessProfile::parse(None).expect("unset defaults off"),
        DerivedAccessProfile::Off
    );
    assert_eq!(
        DerivedAccessProfile::parse(Some(OsStr::new("off"))).expect("explicit off"),
        DerivedAccessProfile::Off
    );
    assert_eq!(
        DerivedAccessProfile::parse(Some(OsStr::new("sqlite-wal-bodyless-v1")))
            .expect("frozen experiment"),
        DerivedAccessProfile::SqliteWalBodylessV1
    );
    assert_eq!(
        DerivedAccessProfile::SqliteWalBodylessV1.as_str(),
        "sqlite-wal-bodyless-v1"
    );
    assert_eq!(DERIVED_ACCESS_PROFILE_ENV, "POINTBREAK_DERIVED_ACCESS");

    let error = DerivedAccessProfile::parse(Some(OsStr::new("sqlite")))
        .expect_err("unknown values fail loudly");
    assert!(error.to_string().contains("unsupported"));
}

#[cfg(unix)]
#[test]
fn selector_rejects_non_unicode_values() {
    use std::os::unix::ffi::OsStringExt as _;

    let value = OsString::from_vec(vec![0xff]);
    let error =
        DerivedAccessProfile::parse(Some(&value)).expect_err("non-Unicode selector must fail");
    assert!(error.to_string().contains("Unicode"));
}

#[test]
fn availability_transition_matrix_is_complete_and_fail_closed() {
    let contract = product_integration_contract_v1();
    assert_eq!(
        contract.availability_states.len(),
        DerivedAccessAvailability::ALL.len()
    );
    for state in DerivedAccessAvailability::ALL {
        assert_eq!(
            contract
                .availability_states
                .iter()
                .find(|row| row.state == state)
                .expect("every state is frozen")
                .allowed_successors,
            state.allowed_successors()
        );
    }

    assert!(DerivedAccessAvailability::Absent.allows(DerivedAccessAvailability::Bootstrapping));
    assert!(DerivedAccessAvailability::CatchingUp.allows(DerivedAccessAvailability::Current));
    assert!(
        DerivedAccessAvailability::RebuildRequired.allows(DerivedAccessAvailability::Quarantined)
    );
    assert!(!DerivedAccessAvailability::Current.allows(DerivedAccessAvailability::Absent));
}

#[test]
fn only_body_search_is_an_exhaustive_route() {
    let contract = product_integration_contract_v1();
    assert_eq!(contract.routes.len(), ProductRouteId::ALL.len());

    for route_id in ProductRouteId::ALL {
        let route = contract
            .routes
            .iter()
            .find(|row| row.route == route_id)
            .expect("every route is frozen");
        if route_id == ProductRouteId::HistoryBodySearch {
            assert_eq!(
                route.fallback,
                DerivedAccessFallback::IntentionalExhaustiveBodySearch
            );
            assert_eq!(
                route.fallback_work,
                Some(ProductWorkClass::HistoryProportional)
            );
            assert!(route.emits_request_receipt);
        } else {
            assert_ne!(
                route.fallback_work,
                Some(ProductWorkClass::HistoryProportional),
                "{route_id:?} may not hide a whole-history fallback"
            );
        }
    }
}

#[test]
fn active_wires_use_projection_stamp_without_relabeling_event_set_hash() {
    let contract = product_integration_contract_v1();
    assert_eq!(
        contract.projection_version.decision,
        ProjectionVersionDecision::ProjectionStamp
    );
    assert_eq!(
        contract.projection_version.stamp_components,
        ProjectionStampComponent::ALL
    );
    assert!(
        contract
            .wire_consumers
            .iter()
            .all(|consumer| consumer.active_version == WireProjectionVersion::ProjectionStamp)
    );
    assert!(
        contract
            .wire_consumers
            .iter()
            .all(|consumer| !consumer.active_event_set_hash_required)
    );
    assert!(
        contract
            .wire_consumers
            .iter()
            .any(|consumer| consumer.loose_event_set_hash_required)
    );
}

#[test]
fn parity_fixture_matrix_covers_every_frozen_product_surface() {
    let contract = product_integration_contract_v1();
    assert_eq!(
        contract
            .parity_fixtures
            .iter()
            .map(|row| row.fixture)
            .collect::<Vec<_>>(),
        ProductParityFixtureId::ALL
    );
    assert!(
        contract
            .parity_fixtures
            .iter()
            .all(|row| row.default_off_exact
                && row.active_domain_parity
                && row.active_wire_parity
                && row.request_counter_receipt)
    );
}

#[test]
fn contract_fixture_is_canonical_strict_and_self_verifying() {
    let contract = product_integration_contract_v1();
    contract.validate().expect("frozen contract validates");

    let expected = format!(
        "{}\n",
        serde_json::to_string_pretty(&product_integration_contract_fixture_v1())
            .expect("fixture serializes")
    );
    verify_product_integration_contract_fixture_v1().expect("embedded fixture verifies");
    assert_eq!(
        expected,
        include_str!("../../../tests/fixtures/derived-access/product-integration-v1.json")
    );

    let mut value: serde_json::Value =
        serde_json::from_str(&expected).expect("fixture JSON parses");
    value
        .as_object_mut()
        .expect("fixture object")
        .insert("unexpected".to_owned(), serde_json::Value::Bool(true));
    assert!(
        serde_json::from_value::<ProductIntegrationContractFixtureV1>(value).is_err(),
        "unknown fields must be rejected"
    );
}

#[test]
fn documentation_table_is_generated_from_the_frozen_contract() {
    let docs = include_str!("../../../docs/benchmarking.md");
    assert!(
        docs.contains(&format!(
            "canonical SHA-256\nis `{PRODUCT_INTEGRATION_CONTRACT_SHA256_V1}`."
        )),
        "documented contract hash must match the frozen constant"
    );
    let table = docs
        .split_once("<!-- derived-access-product-integration-contract-v1:start -->\n")
        .expect("docs start marker")
        .1
        .split_once("\n<!-- derived-access-product-integration-contract-v1:end -->")
        .expect("docs end marker")
        .0;
    assert_eq!(
        table,
        product_integration_contract_publication_v1().decision_table_markdown
    );
}

#[test]
fn contract_validation_rejects_route_drift() {
    let mut contract = product_integration_contract_v1();
    contract.routes.pop();
    assert!(contract.validate().is_err());
}

#[test]
fn non_timing_smoke_emits_the_frozen_inert_receipt() {
    let receipt = product_integration_contract_smoke_v1().expect("pure smoke passes");
    assert_eq!(receipt.filesystem_actions, 0);
    assert_eq!(receipt.route_count, ProductRouteId::ALL.len());
    assert_eq!(
        receipt.availability_state_count,
        DerivedAccessAvailability::ALL.len()
    );
    assert_eq!(
        receipt.profiles,
        vec![
            DerivedAccessProfile::Off,
            DerivedAccessProfile::SqliteWalBodylessV1,
        ]
    );
}

#[test]
fn production_readiness_contract_freezes_the_four_residual_gate_families() {
    let contract = production_readiness_contract_v1();
    contract.validate().expect("frozen readiness contract");

    assert_eq!(
        contract.parent_contract_sha256,
        PRODUCT_INTEGRATION_CONTRACT_SHA256_V1
    );
    assert_eq!(
        contract
            .canonical_sha256()
            .expect("canonical readiness hash"),
        PRODUCTION_READINESS_CONTRACT_SHA256_V1
    );
    assert!(contract.authority_stamp.scenarios.len() > 1);
    assert!(!contract.bootstrap.phases.is_empty());
    assert_eq!(contract.revisions_page.default_limit, 100);
    assert_eq!(contract.revisions_page.maximum_limit, 500);
    assert_eq!(
        contract.terminal_outcomes,
        vec![
            ReadinessOutcome::ReadyForDefaultDecision,
            ReadinessOutcome::RemainDefaultOff,
            ReadinessOutcome::Reject,
        ]
    );

    let old = product_integration_contract_v1();
    assert!(
        old.routes
            .iter()
            .all(|route| route.route != ProductRouteId::Revisions
                || route.active_work == Some(ProductWorkClass::OutputProportional)),
        "the parent contract classifies a complete collection but has no page/token contract"
    );
}

#[test]
fn authority_stamp_scenarios_freeze_bounded_observation_and_fail_closed_outcomes() {
    let contract = production_readiness_contract_v1();
    assert_eq!(
        contract.authority_stamp.observable_policy,
        AuthorityStampObservablePolicy::SelectedAndFrozenByNativeFalsifierBeforeIntegration
    );
    assert!(contract.authority_stamp.scenarios.iter().all(|scenario| {
        scenario.event_directory_entries_walked_ceiling == 0
            && scenario.event_carrier_opens_ceiling == 0
    }));
    assert_eq!(
        contract
            .authority_stamp
            .scenario(AuthorityStampScenarioId::OutOfBandCreate)
            .expect("out-of-band create"),
        AuthorityStampExpectation::ChangedOrIndeterminate
    );
    let unrelated = contract
        .authority_stamp
        .scenarios
        .iter()
        .find(|row| row.scenario == AuthorityStampScenarioId::UnrelatedFile)
        .expect("unrelated event-directory file");
    assert_eq!(
        unrelated.mutation_locus,
        AuthorityStampMutationLocus::EventDirectoryNonCarrierEntry
    );
    assert_eq!(
        unrelated.expectation,
        AuthorityStampExpectation::StableOrChangedWithoutTruthClaim
    );
    assert!(!unrelated.authoritative_carrier_created);
    assert_eq!(
        contract
            .authority_stamp
            .scenario(AuthorityStampScenarioId::SidecarDeletion)
            .expect("missing authority metadata fails closed"),
        AuthorityStampExpectation::ChangedOrIndeterminate
    );
    assert_eq!(
        contract
            .authority_stamp
            .scenario(AuthorityStampScenarioId::ExperimentOffRollback)
            .expect("profile-off rollback does not observe the stamp"),
        AuthorityStampExpectation::ObservationNotApplicable
    );
    assert_eq!(
        contract
            .authority_stamp
            .scenario(AuthorityStampScenarioId::ExistingCarrierOverwrite)
            .expect("overwrite non-claim"),
        AuthorityStampExpectation::ExplicitNonClaim
    );
    assert!(
        contract
            .authority_stamp
            .changed_or_indeterminate_never_proves_truth
    );
    assert!(
        contract
            .authority_stamp
            .failures
            .iter()
            .all(|failure| !failure.may_report_current)
    );
}

#[test]
fn readiness_counter_ceilings_are_typed_and_do_not_hide_total_history_work() {
    let contract = production_readiness_contract_v1();
    let no_change = contract
        .counter_ceiling(
            ReadinessOperationId::ActiveOrdinaryNoChange,
            ReadinessCounterId::EventDirectoryEntriesWalked,
        )
        .expect("ordinary no-change directory ceiling");
    let context = CounterCeilingContextV1 {
        requested_entries: 100,
        source_carriers: 262_144,
    };
    assert!(no_change.allows(0, context));
    assert!(!no_change.allows(1, context));

    let writer = contract
        .counter_ceiling(
            ReadinessOperationId::FreshWriterAdmissionNoChange,
            ReadinessCounterId::EventDirectoryEntriesWalked,
        )
        .expect("fresh writer directory ceiling");
    assert!(writer.allows(0, context));
    assert!(!writer.allows(1, context));

    let page = contract
        .counter_ceiling(
            ReadinessOperationId::ActiveRevisionsPage,
            ReadinessCounterId::RevisionRowsExamined,
        )
        .expect("active page scan ceiling");
    assert_eq!(page.ceiling(context), 101);
    assert!(page.allows(101, context));
    assert!(!page.allows(102, context));
}

#[test]
fn bootstrap_contract_is_single_population_progressive_and_truthful() {
    let contract = production_readiness_contract_v1();
    assert_eq!(contract.bootstrap.population_carrier_open_ceiling, 1);
    assert_eq!(
        contract
            .bootstrap
            .maximum_overlapping_full_decoded_histories,
        1
    );
    assert!(contract.bootstrap.strict_oracle_is_serial_or_isolated);
    assert!(contract.bootstrap.completion_is_published_last);
    assert_eq!(contract.bootstrap.maximum_automatic_fallbacks, 1);
    assert!(
        contract
            .bootstrap
            .availability
            .iter()
            .find(|row| row.case == BootstrapAvailabilityCase::FirstBootstrap)
            .expect("first bootstrap contract")
            .offers_explicit_fallback_or_wait
    );
    assert!(
        contract
            .bootstrap
            .availability
            .iter()
            .all(|row| !row.may_serve_stale_as_current)
    );
}

#[test]
fn revision_page_contract_has_one_snapshot_bound_wire_shape() {
    let page = &production_readiness_contract_v1().revisions_page;
    assert_eq!(page.schema, "pointbreak.inspect-revisions-page.v1");
    assert_eq!(page.default_limit, 100);
    assert_eq!(page.maximum_limit, 500);
    assert!(page.absent_limit_uses_default);
    assert_eq!(
        page.over_maximum_limit,
        RevisionPageLimitOverflow::InvalidRequest
    );
    assert!(page.requested_entries_are_accepted_limit);
    assert!(page.token_is_opaque);
    assert!(page.token_binds_profile_schema_snapshot_and_order);
    assert!(page.same_wire_shape_for_active_and_default_off);
    assert_eq!(page.active_work, ProductWorkClass::OutputProportional);
    assert_eq!(page.default_off_work, ProductWorkClass::HistoryProportional);
    assert!(page.default_off_exhaustive_comparator_is_explicit);
    assert_eq!(
        page.revision_count_source,
        RevisionCountSource::DerivedIndexedAggregate
    );
    assert!(page.exact_detail_is_entity_primary);
}

#[test]
fn terminal_evaluator_is_fail_closed_and_keeps_the_default_decision_separate() {
    let contract = production_readiness_contract_v1();
    let passing = ReadinessGateId::ALL
        .into_iter()
        .map(ReadinessGateResultV1::passed)
        .collect::<Vec<_>>();
    assert_eq!(
        contract
            .evaluate(&passing)
            .expect("complete passing matrix"),
        ReadinessOutcome::ReadyForDefaultDecision
    );

    let mut operational_residual = passing.clone();
    let bounded = operational_residual
        .iter_mut()
        .find(|result| result.gate == ReadinessGateId::ActiveOrdinaryWorkBounded)
        .expect("bounded-work result");
    *bounded = ReadinessGateResultV1::failed(bounded.gate);
    assert_eq!(
        contract
            .evaluate(&operational_residual)
            .expect("compensable residual"),
        ReadinessOutcome::RemainDefaultOff
    );

    let mut parent_failure = passing.clone();
    let parent = parent_failure
        .iter_mut()
        .find(|result| result.gate == ReadinessGateId::ParentContractSatisfied)
        .expect("parent-contract result");
    *parent = ReadinessGateResultV1::failed(parent.gate);
    assert_eq!(
        contract
            .evaluate(&parent_failure)
            .expect("parent contract remains load-bearing"),
        ReadinessOutcome::RemainDefaultOff
    );

    let mut correctness_failure = passing;
    let exact = correctness_failure
        .iter_mut()
        .find(|result| result.gate == ReadinessGateId::NoFalseCurrentState)
        .expect("false-current result");
    *exact = ReadinessGateResultV1::failed(exact.gate);
    assert_eq!(
        contract
            .evaluate(&correctness_failure)
            .expect("non-compensable failure"),
        ReadinessOutcome::Reject
    );
    assert!(!contract.production_default_change_authorized);
}

#[test]
fn readiness_decision_table_is_derived_from_the_typed_contract() {
    let contract = production_readiness_contract_v1();
    let table = contract.decision_table_markdown();

    let mut changed_counter = contract.clone();
    changed_counter
        .counter_ceilings
        .iter_mut()
        .find(|row| {
            row.operation == ReadinessOperationId::ActiveOrdinaryNoChange
                && row.counter == ReadinessCounterId::EventDirectoryEntriesWalked
        })
        .expect("ordinary counter")
        .additive = 1;
    assert_ne!(table, changed_counter.decision_table_markdown());

    let mut changed_timing = contract.clone();
    changed_timing.numeric_wall_time_thresholds_present = true;
    assert_ne!(table, changed_timing.decision_table_markdown());

    let mut changed_page = contract;
    changed_page.revisions_page.default_limit = 50;
    assert_ne!(table, changed_page.decision_table_markdown());
}

#[test]
fn readiness_fixture_docs_and_non_timing_smoke_share_one_contract_hash() {
    let expected = format!(
        "{}\n",
        serde_json::to_string_pretty(&production_readiness_contract_fixture_v1())
            .expect("readiness fixture serializes")
    );
    verify_production_readiness_contract_fixture_v1().expect("embedded readiness fixture verifies");
    assert_eq!(
        expected,
        include_str!("../../../tests/fixtures/derived-access/production-readiness-v1.json")
    );
    let mut value: serde_json::Value =
        serde_json::from_str(&expected).expect("readiness fixture JSON parses");
    value
        .as_object_mut()
        .expect("readiness fixture object")
        .insert("unexpected".to_owned(), serde_json::Value::Bool(true));
    assert!(
        serde_json::from_value::<super::product_contract::ProductionReadinessContractFixtureV1>(
            value
        )
        .is_err(),
        "unknown readiness fixture fields must be rejected"
    );

    let docs = include_str!("../../../docs/benchmarking.md");
    let table = docs
        .split_once("<!-- derived-access-production-readiness-contract-v1:start -->\n")
        .expect("readiness docs start marker")
        .1
        .split_once("\n<!-- derived-access-production-readiness-contract-v1:end -->")
        .expect("readiness docs end marker")
        .0;
    assert_eq!(
        table,
        production_readiness_contract_publication_v1().decision_table_markdown
    );

    let smoke = production_readiness_contract_smoke_v1().expect("readiness smoke");
    assert_eq!(
        smoke.contract_sha256,
        PRODUCTION_READINESS_CONTRACT_SHA256_V1
    );
    assert_eq!(smoke.filesystem_actions, 0);
    assert_eq!(smoke.store_roots_opened, 0);
    assert!(!smoke.expensive_scale_work_run);

    assert_eq!(
        production_readiness_contract_v1().protected_inputs,
        vec![
            ProtectedInputId::OwnerStore,
            ProtectedInputId::PrivateCorpus,
            ProtectedInputId::PrivateSnapshotBytes,
            ProtectedInputId::QualificationCorpus,
            ProtectedInputId::Credentials,
            ProtectedInputId::InstalledArtifacts,
            ProtectedInputId::InstalledExtensionDogfoodEvidence,
            ProtectedInputId::RemoteMirrorAutomationState,
            ProtectedInputId::ArchivedEvidence,
            ProtectedInputId::ExternalParticipantEvidence,
            ProtectedInputId::ProductionDefault,
            ProtectedInputId::DeferredContentFormatExperiment,
        ]
    );
}
