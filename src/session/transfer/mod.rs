//! Exact logical transfer contracts.
//!
//! The manifest and receipt are public data contracts. Import publication is
//! routed only through an already-complete Change destination and never installs
//! source control records as destination authority.

mod exact_bundle;
mod import_receipt;

pub use exact_bundle::{
    EXACT_BUNDLE_SCHEMA_V2, ExactBundleCapabilityV2, ExactBundleClosureV2, ExactBundleManifestV2,
    ExactBundleRecordKindV2, ExactBundleRecordV2, ExactBundleSelectionV2, ExactTransferError,
    import_exact_bundle_v2,
};
pub use import_receipt::{IMPORT_RECEIPT_SCHEMA_V1, ImportReceiptV1};
