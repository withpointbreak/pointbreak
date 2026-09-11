//! Exercise the write cohort contract on real JSON responses across CLI fixtures.
use std::collections::BTreeMap;

use pointbreak::session::{
    DerivedWriteAcknowledgementV1, OperationReceiptAcknowledgementV1, WriteAcknowledgementV1,
};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

struct UniqueObject(BTreeMap<String, Value>);
impl<'de> Deserialize<'de> for UniqueObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = UniqueObject;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an object with unique top-level keys")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut fields = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    if fields.insert(key.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!("duplicate key {key}")));
                    }
                }
                Ok(UniqueObject(fields))
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

pub fn assert_contract(bytes: &[u8]) {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return;
    };
    let Some(schema) = value["schema"].as_str() else {
        return;
    };
    if !matches!(
        schema,
        "pointbreak.review-capture"
            | "pointbreak.change-capture-receipt.v1"
            | "pointbreak.review-association-commit"
            | "pointbreak.review-association-commit-withdrawn"
            | "pointbreak.review-association-ref"
            | "pointbreak.review-association-ref-withdrawn"
            | "pointbreak.review-assessment-add"
            | "pointbreak.review-observation-add"
            | "pointbreak.store-remove"
            | "pointbreak.review-input-request-open"
            | "pointbreak.review-input-request-respond"
            | "pointbreak.review-endorse"
            | "pointbreak.review-validation-add"
    ) {
        return;
    }
    let UniqueObject(fields) =
        serde_json::from_slice(bytes).expect("write document has unique top-level keys");
    assert!(fields["diagnostics"].is_array(), "{schema} diagnostics");
    let acknowledgement: WriteAcknowledgementV1 = serde_json::from_value(
        fields
            .get("acknowledgement")
            .unwrap_or_else(|| panic!("missing {schema} acknowledgement"))
            .clone(),
    )
    .expect("common typed acknowledgement");
    DerivedWriteAcknowledgementV1::new(
        acknowledgement.derived.availability,
        acknowledgement.derived.token,
    )
    .expect("valid derived token pairing");
    OperationReceiptAcknowledgementV1::new(
        acknowledgement.operation_receipt.state,
        acknowledgement.operation_receipt.receipt_id.clone(),
    )
    .expect("valid receipt pairing");
    if schema == "pointbreak.change-capture-receipt.v1" {
        assert_eq!(
            acknowledgement.operation_receipt.receipt_id.as_deref(),
            fields["operationId"].as_str()
        );
    } else {
        assert!(acknowledgement.operation_receipt.receipt_id.is_none());
    }
}
