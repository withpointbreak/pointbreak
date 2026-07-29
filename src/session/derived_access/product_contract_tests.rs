use std::ffi::{OsStr, OsString};

use super::product_contract::{
    DERIVED_ACCESS_PROFILE_ENV, DerivedAccessAvailability, DerivedAccessFallback,
    DerivedAccessProfile, PRODUCT_INTEGRATION_CONTRACT_SHA256_V1,
    ProductIntegrationContractFixtureV1, ProductParityFixtureId, ProductRouteId, ProductWorkClass,
    ProjectionStampComponent, ProjectionVersionDecision, WireProjectionVersion,
    product_integration_contract_fixture_v1, product_integration_contract_publication_v1,
    product_integration_contract_smoke_v1, product_integration_contract_v1,
    verify_product_integration_contract_fixture_v1,
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
