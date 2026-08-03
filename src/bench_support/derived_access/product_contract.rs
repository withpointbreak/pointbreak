use serde::Serialize;

use crate::session::derived_access::product_contract::{
    DERIVED_ACCESS_ROLLOUT_CONTRACT_SHA256_V1, PRODUCT_INTEGRATION_CONTRACT_SHA256_V1,
    derived_access_rollout_contract_publication_v1, derived_access_rollout_contract_smoke_v1,
    product_integration_contract_publication_v1, product_integration_contract_smoke_v1,
    product_integration_contract_v1, production_readiness_contract_publication_v1,
    production_readiness_contract_smoke_v1, verify_derived_access_rollout_contract_fixture_v1,
    verify_product_integration_contract_fixture_v1,
    verify_production_readiness_contract_fixture_v1,
};
#[cfg(test)]
use crate::session::derived_access::product_contract::{
    PRODUCTION_READINESS_CONTRACT_SHA256_V1, PRODUCTION_READINESS_FIXTURE_SHA256_V1,
};

pub const DERIVED_ACCESS_PRODUCT_CONTRACT_MODE_V1: &str = "--derived-access-product-contract";
pub const DERIVED_ACCESS_PRODUCT_CONTRACT_VERIFY_MODE_V1: &str =
    "--derived-access-product-contract-verify";
pub const DERIVED_ACCESS_PRODUCT_CONTRACT_SMOKE_MODE_V1: &str =
    "--derived-access-product-contract-smoke";
pub const DERIVED_ACCESS_READINESS_CONTRACT_MODE_V1: &str = "--derived-access-readiness-contract";
pub const DERIVED_ACCESS_READINESS_CONTRACT_VERIFY_MODE_V1: &str =
    "--derived-access-readiness-contract-verify";
pub const DERIVED_ACCESS_READINESS_CONTRACT_SMOKE_MODE_V1: &str =
    "--derived-access-readiness-contract-smoke";
pub const DERIVED_ACCESS_ROLLOUT_CONTRACT_MODE_V1: &str = "--derived-access-rollout-contract";
pub const DERIVED_ACCESS_ROLLOUT_CONTRACT_VERIFY_MODE_V1: &str =
    "--derived-access-rollout-contract-verify";
pub const DERIVED_ACCESS_ROLLOUT_CONTRACT_SMOKE_MODE_V1: &str =
    "--derived-access-rollout-contract-smoke";

pub fn derived_access_product_contract_publication_json_v1() -> Result<String, String> {
    let publication = product_integration_contract_publication_v1();
    publication.contract.validate()?;
    serde_json::to_string(&publication)
        .map_err(|error| format!("product-integration publication failed: {error}"))
}

pub fn derived_access_product_contract_verify_json_v1() -> Result<String, String> {
    verify_product_integration_contract_fixture_v1()?;
    mode_receipt_json("verify", 0, false)
}

pub fn derived_access_product_contract_smoke_json_v1() -> Result<String, String> {
    let receipt = product_integration_contract_smoke_v1()?;
    mode_receipt_json(
        "non_timing_smoke",
        receipt.filesystem_actions,
        receipt.physical_implementation_opened,
    )
}

pub fn derived_access_readiness_contract_publication_json_v1() -> Result<String, String> {
    let publication = production_readiness_contract_publication_v1();
    publication.contract.validate()?;
    serde_json::to_string(&publication)
        .map_err(|error| format!("production-readiness publication failed: {error}"))
}

pub fn derived_access_readiness_contract_verify_json_v1() -> Result<String, String> {
    verify_production_readiness_contract_fixture_v1()?;
    let receipt = production_readiness_contract_smoke_v1()?;
    readiness_mode_receipt_json("verify", &receipt)
}

pub fn derived_access_readiness_contract_smoke_json_v1() -> Result<String, String> {
    let receipt = production_readiness_contract_smoke_v1()?;
    readiness_mode_receipt_json("non_timing_smoke", &receipt)
}

pub fn derived_access_rollout_contract_publication_json_v1() -> Result<String, String> {
    let publication = derived_access_rollout_contract_publication_v1();
    publication.contract.validate()?;
    serde_json::to_string(&publication)
        .map_err(|error| format!("derived-access rollout publication failed: {error}"))
}

pub fn derived_access_rollout_contract_verify_json_v1() -> Result<String, String> {
    verify_derived_access_rollout_contract_fixture_v1()?;
    rollout_mode_receipt_json("verify")
}

pub fn derived_access_rollout_contract_smoke_json_v1() -> Result<String, String> {
    derived_access_rollout_contract_smoke_v1()?;
    rollout_mode_receipt_json("non_timing_smoke")
}

fn rollout_mode_receipt_json(mode: &'static str) -> Result<String, String> {
    serde_json::to_string(&DerivedAccessRolloutContractModeReceiptV1 {
        schema: "pointbreak.derived-access-rollout-mode-receipt.v1",
        mode,
        contract_sha256: DERIVED_ACCESS_ROLLOUT_CONTRACT_SHA256_V1,
        filesystem_actions: 0,
        store_roots_opened: 0,
        evidence_collected: false,
    })
    .map_err(|error| format!("derived-access rollout mode receipt failed: {error}"))
}

fn readiness_mode_receipt_json(
    mode: &'static str,
    smoke: &crate::session::derived_access::product_contract::ProductionReadinessContractSmokeV1,
) -> Result<String, String> {
    let receipt = ProductionReadinessContractModeReceiptV1 {
        schema: "pointbreak.derived-access-production-readiness-mode-receipt.v1",
        mode,
        contract_sha256: &smoke.contract_sha256,
        fixture_sha256: &smoke.fixture_sha256,
        authority_scenario_count: smoke.scenario_count,
        gate_count: smoke.gate_count,
        filesystem_actions: smoke.filesystem_actions,
        store_roots_opened: smoke.store_roots_opened,
        expensive_scale_work_run: smoke.expensive_scale_work_run,
        physical_implementation_opened: smoke.physical_implementation_opened,
        evidence_collected: smoke.evidence_collected,
    };
    serde_json::to_string(&receipt)
        .map_err(|error| format!("production-readiness mode receipt failed: {error}"))
}

fn mode_receipt_json(
    mode: &'static str,
    filesystem_actions: u64,
    physical_implementation_opened: bool,
) -> Result<String, String> {
    let contract = product_integration_contract_v1();
    let receipt = ProductIntegrationContractModeReceiptV1 {
        schema: "pointbreak.derived-access-product-integration-mode-receipt.v1",
        mode,
        contract_sha256: PRODUCT_INTEGRATION_CONTRACT_SHA256_V1,
        profile_count: contract.profiles.len(),
        availability_state_count: contract.availability_states.len(),
        route_count: contract.routes.len(),
        gate_count: contract.gates.len(),
        filesystem_actions,
        physical_implementation_opened,
    };
    serde_json::to_string(&receipt)
        .map_err(|error| format!("product-integration mode receipt failed: {error}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProductIntegrationContractModeReceiptV1 {
    schema: &'static str,
    mode: &'static str,
    contract_sha256: &'static str,
    profile_count: usize,
    availability_state_count: usize,
    route_count: usize,
    gate_count: usize,
    filesystem_actions: u64,
    physical_implementation_opened: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProductionReadinessContractModeReceiptV1<'a> {
    schema: &'static str,
    mode: &'static str,
    contract_sha256: &'a str,
    fixture_sha256: &'a str,
    authority_scenario_count: usize,
    gate_count: usize,
    filesystem_actions: u64,
    store_roots_opened: u64,
    expensive_scale_work_run: bool,
    physical_implementation_opened: bool,
    evidence_collected: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DerivedAccessRolloutContractModeReceiptV1 {
    schema: &'static str,
    mode: &'static str,
    contract_sha256: &'static str,
    filesystem_actions: u64,
    store_roots_opened: u64,
    evidence_collected: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench_support::derived_access::qualification_derived_access_contract_v1;

    #[test]
    fn product_contract_modes_emit_frozen_receipts_and_self_verify() {
        let publication: serde_json::Value = serde_json::from_str(
            &derived_access_product_contract_publication_json_v1().expect("publication"),
        )
        .expect("publication JSON");
        assert_eq!(
            publication["contractSha256"],
            PRODUCT_INTEGRATION_CONTRACT_SHA256_V1
        );

        for json in [
            derived_access_product_contract_verify_json_v1().expect("verify"),
            derived_access_product_contract_smoke_json_v1().expect("smoke"),
        ] {
            let receipt: serde_json::Value =
                serde_json::from_str(&json).expect("mode receipt JSON");
            assert_eq!(receipt["filesystemActions"], 0);
            assert_eq!(receipt["physicalImplementationOpened"], false);
        }
    }

    #[test]
    fn product_contract_reuses_the_qualified_matched_operation_ceilings() {
        let product = product_integration_contract_v1();
        let qualification = qualification_derived_access_contract_v1();
        assert_eq!(
            product.matched_operation_limits.len(),
            qualification.operations.len()
        );
        for (product, qualification) in product
            .matched_operation_limits
            .iter()
            .zip(&qualification.operations)
        {
            assert_eq!(
                serde_json::to_value(product.operation).expect("product operation"),
                serde_json::to_value(qualification.operation).expect("qualification operation")
            );
            assert_eq!(
                product.l100_wall_p95_ceiling_ms,
                qualification.l100_wall_p95_ceiling_ms
            );
            assert_eq!(
                product.l100_process_cpu_p95_ceiling_ms,
                qualification.l100_process_cpu_p95_ceiling_ms
            );
            assert!(!product.release_promise);
        }
    }

    #[test]
    fn readiness_contract_modes_emit_the_frozen_inert_receipts() {
        let publication: serde_json::Value = serde_json::from_str(
            &derived_access_readiness_contract_publication_json_v1().expect("publication"),
        )
        .expect("publication JSON");
        assert_eq!(
            publication["contractSha256"],
            PRODUCTION_READINESS_CONTRACT_SHA256_V1
        );
        assert_eq!(
            publication["fixtureSha256"],
            PRODUCTION_READINESS_FIXTURE_SHA256_V1
        );

        for json in [
            derived_access_readiness_contract_verify_json_v1().expect("verify"),
            derived_access_readiness_contract_smoke_json_v1().expect("smoke"),
        ] {
            let receipt: serde_json::Value =
                serde_json::from_str(&json).expect("mode receipt JSON");
            assert_eq!(
                receipt["contractSha256"],
                PRODUCTION_READINESS_CONTRACT_SHA256_V1
            );
            assert_eq!(
                receipt["fixtureSha256"],
                PRODUCTION_READINESS_FIXTURE_SHA256_V1
            );
            assert_eq!(receipt["filesystemActions"], 0);
            assert_eq!(receipt["storeRootsOpened"], 0);
            assert_eq!(receipt["expensiveScaleWorkRun"], false);
            assert_eq!(receipt["physicalImplementationOpened"], false);
            assert_eq!(receipt["evidenceCollected"], false);
        }
    }

    #[test]
    fn rollout_contract_modes_emit_the_current_inert_receipts() {
        let publication: serde_json::Value = serde_json::from_str(
            &derived_access_rollout_contract_publication_json_v1().expect("publication"),
        )
        .expect("publication JSON");
        assert_eq!(
            publication["contractSha256"],
            DERIVED_ACCESS_ROLLOUT_CONTRACT_SHA256_V1
        );
        for json in [
            derived_access_rollout_contract_verify_json_v1().expect("verify"),
            derived_access_rollout_contract_smoke_json_v1().expect("smoke"),
        ] {
            let receipt: serde_json::Value =
                serde_json::from_str(&json).expect("mode receipt JSON");
            assert_eq!(receipt["filesystemActions"], 0);
            assert_eq!(receipt["storeRootsOpened"], 0);
            assert_eq!(receipt["evidenceCollected"], false);
        }
    }
}
