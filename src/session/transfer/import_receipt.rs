use serde::{Deserialize, Serialize};

use super::exact_bundle::{ExactBundleManifestV2, ExactTransferError};
use crate::canonical_hash::sha256_json_prefixed;

pub const IMPORT_RECEIPT_SCHEMA_V1: &str = "pointbreak.import-receipt.v1";

/// Destination-local proof that one exact logical bundle was reconciled.
///
/// The receipt is operational state, not a Journal event. Its local context is
/// deliberately absent from the source bundle and imported event bytes.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportReceiptV1 {
    pub schema: String,
    pub source_bundle_sha256: String,
    pub source_event_set_sha256: String,
    pub source_event_sha256: Vec<String>,
    pub local_import_context: String,
    pub receipt_sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReceiptPreimage<'a> {
    schema: &'a str,
    source_bundle_sha256: &'a str,
    source_event_set_sha256: &'a str,
    source_event_sha256: &'a [String],
    local_import_context: &'a str,
}

impl ImportReceiptV1 {
    pub fn new(
        manifest: &ExactBundleManifestV2,
        local_import_context: impl Into<String>,
    ) -> Result<Self, ExactTransferError> {
        manifest.validate()?;
        let local_import_context = local_import_context.into();
        if local_import_context.trim().is_empty() {
            return Err(ExactTransferError::Contract(
                "local import context must not be empty".to_owned(),
            ));
        }
        let mut source_event_sha256 = manifest
            .events
            .iter()
            .map(|event| event.decoded_sha256.clone())
            .collect::<Vec<_>>();
        source_event_sha256.sort();
        let mut receipt = Self {
            schema: IMPORT_RECEIPT_SCHEMA_V1.to_owned(),
            source_bundle_sha256: manifest.bundle_sha256.clone(),
            source_event_set_sha256: manifest.event_set_sha256.clone(),
            source_event_sha256,
            local_import_context,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt.computed_sha256()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), ExactTransferError> {
        if self.schema != IMPORT_RECEIPT_SCHEMA_V1
            || self.local_import_context.trim().is_empty()
            || !is_prefixed_sha256(&self.source_bundle_sha256)
            || !is_prefixed_sha256(&self.source_event_set_sha256)
            || self
                .source_event_sha256
                .iter()
                .any(|hash| !is_prefixed_sha256(hash))
            || self
                .source_event_sha256
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self.receipt_sha256 != self.computed_sha256()?
        {
            return Err(ExactTransferError::Contract(
                "invalid exact-import receipt".to_owned(),
            ));
        }
        Ok(())
    }

    fn computed_sha256(&self) -> Result<String, ExactTransferError> {
        sha256_json_prefixed(&serde_json::to_value(ReceiptPreimage {
            schema: &self.schema,
            source_bundle_sha256: &self.source_bundle_sha256,
            source_event_set_sha256: &self.source_event_set_sha256,
            source_event_sha256: &self.source_event_sha256,
            local_import_context: &self.local_import_context,
        })?)
        .map_err(ExactTransferError::from)
    }
}

fn is_prefixed_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
