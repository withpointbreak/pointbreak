//! Separately identified Git association comparisons.

use serde::{Deserialize, Serialize};

use super::revision_resource::exact_cache_key;
use crate::error::Result;
use crate::model::{CommitAssociationId, RevisionRefV1};

pub const ASSOCIATION_COMPARISON_SCHEMA: &str = "pointbreak.review-association-comparison";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociationComparisonRefV1 {
    pub revision: RevisionRefV1,
    pub association_id: CommitAssociationId,
    pub commit_oid: String,
    pub comparison_base: String,
    pub view_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_ref: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssociationComparisonStateV1 {
    Unknown,
    Exact,
    Equivalent,
    Extension,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssociationProofAvailabilityV1 {
    Available,
    Missing,
    Mismatch,
    NotRequested,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociationComparisonDocumentV1 {
    pub schema: String,
    pub version: u32,
    pub comparison: AssociationComparisonRefV1,
    pub state: AssociationComparisonStateV1,
    pub proof_availability: AssociationProofAvailabilityV1,
    pub diagnostics: Vec<String>,
    pub cache_key: String,
}

impl AssociationComparisonDocumentV1 {
    pub fn new(
        comparison: AssociationComparisonRefV1,
        state: AssociationComparisonStateV1,
        proof_availability: AssociationProofAvailabilityV1,
        diagnostics: Vec<String>,
    ) -> Result<Self> {
        if comparison.proof_ref.is_none()
            && proof_availability != AssociationProofAvailabilityV1::NotRequested
        {
            return Err(crate::error::ShoreError::Message(
                "association proof availability requires a proof reference".to_owned(),
            ));
        }
        let cache_key = exact_cache_key(
            ASSOCIATION_COMPARISON_SCHEMA,
            1,
            &serde_json::json!({
                "comparison": comparison,
                "state": state,
                "proofAvailability": proof_availability,
                "diagnostics": diagnostics,
            }),
        )?;
        Ok(Self {
            schema: ASSOCIATION_COMPARISON_SCHEMA.to_owned(),
            version: 1,
            comparison,
            state,
            proof_availability,
            diagnostics,
            cache_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RevisionId, RevisionRefV1};

    fn identity() -> AssociationComparisonRefV1 {
        AssociationComparisonRefV1 {
            revision: RevisionRefV1::new(
                RevisionId::new("rev:sha256:association"),
                format!("sha256:{}", "a".repeat(64)),
            )
            .unwrap(),
            association_id: CommitAssociationId::new("assoc-commit:sha256:one"),
            commit_oid: "1".repeat(40),
            comparison_base: "0".repeat(40),
            view_kind: "landing".to_owned(),
            proof_ref: Some("proof:sha256:one".to_owned()),
        }
    }

    #[test]
    fn every_association_comparison_identity_axis_participates_in_the_cache_key() {
        let original = identity();
        let original_key = AssociationComparisonDocumentV1::new(
            original.clone(),
            AssociationComparisonStateV1::Exact,
            AssociationProofAvailabilityV1::Available,
            Vec::new(),
        )
        .unwrap()
        .cache_key;
        let mut mutations = Vec::new();
        let mut changed = original.clone();
        changed.association_id = CommitAssociationId::new("assoc-commit:sha256:changed");
        mutations.push(changed);
        let mut changed = original.clone();
        changed.commit_oid = "2".repeat(40);
        mutations.push(changed);
        let mut changed = original.clone();
        changed.comparison_base = "3".repeat(40);
        mutations.push(changed);
        let mut changed = original.clone();
        changed.view_kind = "proof".to_owned();
        mutations.push(changed);
        let mut changed = original;
        changed.proof_ref = None;
        mutations.push(changed);
        for mutation in mutations {
            let proof_availability = if mutation.proof_ref.is_some() {
                AssociationProofAvailabilityV1::Available
            } else {
                AssociationProofAvailabilityV1::NotRequested
            };
            assert_ne!(
                AssociationComparisonDocumentV1::new(
                    mutation,
                    AssociationComparisonStateV1::Exact,
                    proof_availability,
                    Vec::new(),
                )
                .unwrap()
                .cache_key,
                original_key
            );
        }
    }

    #[test]
    fn unavailable_git_and_missing_proof_remain_distinct_typed_states() {
        let comparison = identity();
        let document = AssociationComparisonDocumentV1::new(
            comparison.clone(),
            AssociationComparisonStateV1::Unavailable,
            AssociationProofAvailabilityV1::Missing,
            vec!["git_object_missing".to_owned()],
        )
        .unwrap();
        assert_eq!(document.state, AssociationComparisonStateV1::Unavailable);
        assert_eq!(
            document.proof_availability,
            AssociationProofAvailabilityV1::Missing
        );
        let available = AssociationComparisonDocumentV1::new(
            comparison,
            AssociationComparisonStateV1::Exact,
            AssociationProofAvailabilityV1::Available,
            Vec::new(),
        )
        .unwrap();
        assert_ne!(document.cache_key, available.cache_key);
    }
}
