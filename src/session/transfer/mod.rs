//! Exact logical transfer contracts.
//!
//! The manifest and receipt are public data contracts. Import publication is
//! intentionally not routed: the Change cohort remains writer-dark until the
//! later activation plan exposes an operator-authorized destination writer.

mod exact_bundle;
mod import_receipt;

pub use exact_bundle::{
    EXACT_BUNDLE_SCHEMA_V2, ExactBundleCapabilityV2, ExactBundleClosureV2, ExactBundleManifestV2,
    ExactBundleRecordKindV2, ExactBundleRecordV2, ExactBundleSelectionV2, ExactTransferError,
};
pub use import_receipt::{IMPORT_RECEIPT_SCHEMA_V1, ImportReceiptV1};
