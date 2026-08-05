//! Derived comparison documents between two exact immutable Revisions.

use serde::{Deserialize, Serialize};

use super::revision_resource::exact_cache_key;
use crate::error::Result;
use crate::model::RevisionRefV1;

pub const REVISION_INTERDIFF_SCHEMA: &str = "pointbreak.review-revision-interdiff";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionInterdiffRefV1 {
    pub from: RevisionRefV1,
    pub to: RevisionRefV1,
    pub algorithm_version: String,
    pub scope: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionInterdiffAvailabilityV1 {
    Available,
    EndpointMissing,
    EndpointMismatch,
    NonTextual,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionInterdiffDocumentV1 {
    pub schema: String,
    pub version: u32,
    pub interdiff: RevisionInterdiffRefV1,
    pub availability: RevisionInterdiffAvailabilityV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<serde_json::Value>,
    pub diagnostics: Vec<String>,
    pub cache_key: String,
}

impl RevisionInterdiffDocumentV1 {
    pub fn new(
        interdiff: RevisionInterdiffRefV1,
        availability: RevisionInterdiffAvailabilityV1,
        comparison: Option<serde_json::Value>,
        diagnostics: Vec<String>,
    ) -> Result<Self> {
        if (availability == RevisionInterdiffAvailabilityV1::Available) != comparison.is_some() {
            return Err(crate::error::ShoreError::Message(
                "available interdiffs require comparison material and unavailable interdiffs forbid it"
                    .to_owned(),
            ));
        }
        let cache_key = exact_cache_key(
            REVISION_INTERDIFF_SCHEMA,
            1,
            &serde_json::json!({
                "interdiff": interdiff,
                "availability": availability,
                "comparison": comparison,
                "diagnostics": diagnostics,
            }),
        )?;
        Ok(Self {
            schema: REVISION_INTERDIFF_SCHEMA.to_owned(),
            version: 1,
            interdiff,
            availability,
            comparison,
            diagnostics,
            cache_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RevisionId;

    fn reference(name: &str, byte: char) -> RevisionRefV1 {
        RevisionRefV1::new(
            RevisionId::new(format!("rev:sha256:{name}")),
            format!("sha256:{}", byte.to_string().repeat(64)),
        )
        .unwrap()
    }

    #[test]
    fn ordered_endpoints_and_algorithm_are_part_of_the_cache_identity() {
        let first = RevisionInterdiffRefV1 {
            from: reference("a", 'a'),
            to: reference("b", 'b'),
            algorithm_version: "rows-v1".to_owned(),
            scope: vec!["src".to_owned()],
        };
        let mut reversed = first.clone();
        std::mem::swap(&mut reversed.from, &mut reversed.to);
        let left = RevisionInterdiffDocumentV1::new(
            first,
            RevisionInterdiffAvailabilityV1::Available,
            Some(serde_json::json!([])),
            Vec::new(),
        )
        .unwrap();
        let right = RevisionInterdiffDocumentV1::new(
            reversed,
            RevisionInterdiffAvailabilityV1::Available,
            Some(serde_json::json!([])),
            Vec::new(),
        )
        .unwrap();
        assert_ne!(left.cache_key, right.cache_key);
    }

    #[test]
    fn availability_and_comparison_bytes_participate_in_the_cache_key() {
        let identity = RevisionInterdiffRefV1 {
            from: reference("a", 'a'),
            to: reference("b", 'b'),
            algorithm_version: "rows-v1".to_owned(),
            scope: vec!["src".to_owned()],
        };
        let first = RevisionInterdiffDocumentV1::new(
            identity.clone(),
            RevisionInterdiffAvailabilityV1::Available,
            Some(serde_json::json!(["one"])),
            Vec::new(),
        )
        .unwrap();
        let second = RevisionInterdiffDocumentV1::new(
            identity.clone(),
            RevisionInterdiffAvailabilityV1::Available,
            Some(serde_json::json!(["two"])),
            Vec::new(),
        )
        .unwrap();
        let missing = RevisionInterdiffDocumentV1::new(
            identity,
            RevisionInterdiffAvailabilityV1::EndpointMissing,
            None,
            Vec::new(),
        )
        .unwrap();
        assert_ne!(first.cache_key, second.cache_key);
        assert_ne!(first.cache_key, missing.cache_key);
    }
}
