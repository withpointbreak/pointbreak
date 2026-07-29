use serde::Serialize;

use crate::session::derived_access::product_contract::{
    PRODUCT_INTEGRATION_CONTRACT_SHA256_V1, product_integration_contract_publication_v1,
    product_integration_contract_smoke_v1, product_integration_contract_v1,
    verify_product_integration_contract_fixture_v1,
};

pub const DERIVED_ACCESS_PRODUCT_CONTRACT_MODE_V1: &str = "--derived-access-product-contract";
pub const DERIVED_ACCESS_PRODUCT_CONTRACT_VERIFY_MODE_V1: &str =
    "--derived-access-product-contract-verify";
pub const DERIVED_ACCESS_PRODUCT_CONTRACT_SMOKE_MODE_V1: &str =
    "--derived-access-product-contract-smoke";

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
}
