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
        let document = Self {
            schema: ASSOCIATION_COMPARISON_SCHEMA.to_owned(),
            version: 1,
            comparison,
            state,
            proof_availability,
            diagnostics,
            cache_key: String::new(),
        };
        let cache_key = association_comparison_cache_key(
            &document.comparison,
            document.state,
            document.proof_availability,
            &document.diagnostics,
        )?;
        let document = Self {
            cache_key,
            ..document
        };
        document.validate_integrity()?;
        Ok(document)
    }

    /// Revalidate a transported or otherwise mutated association comparison
    /// before a Change facade admits it into an exact-Revision response.
    /// Qualified relation states require available, named proof; opaque Git
    /// identifiers remain non-empty selectors rather than inferred evidence.
    pub fn validate_integrity(&self) -> Result<()> {
        if self.schema != ASSOCIATION_COMPARISON_SCHEMA || self.version != 1 {
            return Err(crate::error::ShoreError::Message(
                "association comparison document schema or version is unsupported".to_owned(),
            ));
        }
        RevisionRefV1::new(
            self.comparison.revision.revision_id.clone(),
            self.comparison
                .revision
                .object_artifact_content_hash
                .clone(),
        )?;
        if self.comparison.association_id.as_str().trim().is_empty()
            || self.comparison.commit_oid.trim().is_empty()
            || self.comparison.comparison_base.trim().is_empty()
            || self.comparison.view_kind.trim().is_empty()
        {
            return Err(crate::error::ShoreError::Message(
                "association comparison requires non-empty association, commit, base, and view identifiers"
                    .to_owned(),
            ));
        }
        match self.comparison.proof_ref.as_deref() {
            None if self.proof_availability != AssociationProofAvailabilityV1::NotRequested => {
                return Err(crate::error::ShoreError::Message(
                    "association proof availability requires a proof reference".to_owned(),
                ));
            }
            Some(proof_ref) if proof_ref.trim().is_empty() => {
                return Err(crate::error::ShoreError::Message(
                    "association proof reference must not be empty".to_owned(),
                ));
            }
            _ => {}
        }
        if matches!(
            self.state,
            AssociationComparisonStateV1::Exact
                | AssociationComparisonStateV1::Equivalent
                | AssociationComparisonStateV1::Extension
        ) && self.proof_availability != AssociationProofAvailabilityV1::Available
        {
            return Err(crate::error::ShoreError::Message(
                "qualified association comparison states require available proof".to_owned(),
            ));
        }
        let expected_cache_key = association_comparison_cache_key(
            &self.comparison,
            self.state,
            self.proof_availability,
            &self.diagnostics,
        )?;
        if self.cache_key != expected_cache_key {
            return Err(crate::error::ShoreError::Message(
                "association comparison cache identity does not match its exact document"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

fn association_comparison_cache_key(
    comparison: &AssociationComparisonRefV1,
    state: AssociationComparisonStateV1,
    proof_availability: AssociationProofAvailabilityV1,
    diagnostics: &[String],
) -> Result<String> {
    exact_cache_key(
        ASSOCIATION_COMPARISON_SCHEMA,
        1,
        &serde_json::json!({
            "comparison": comparison,
            "state": state,
            "proofAvailability": proof_availability,
            "diagnostics": diagnostics,
        }),
    )
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
        changed.proof_ref = Some("proof:sha256:changed".to_owned());
        mutations.push(changed);
        for mutation in mutations {
            assert_ne!(
                AssociationComparisonDocumentV1::new(
                    mutation,
                    AssociationComparisonStateV1::Exact,
                    AssociationProofAvailabilityV1::Available,
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

    fn exact_document() -> AssociationComparisonDocumentV1 {
        AssociationComparisonDocumentV1::new(
            identity(),
            AssociationComparisonStateV1::Exact,
            AssociationProofAvailabilityV1::Available,
            vec!["comparison_verified".to_owned()],
        )
        .unwrap()
    }

    #[test]
    fn deserialized_comparisons_reestablish_schema_proof_and_cache_integrity() {
        let document = exact_document();
        let mut wrong_schema = serde_json::to_value(&document).unwrap();
        wrong_schema["schema"] = serde_json::Value::String("wrong.schema".to_owned());
        let wrong_schema: AssociationComparisonDocumentV1 =
            serde_json::from_value(wrong_schema).unwrap();
        assert!(wrong_schema.validate_integrity().is_err());

        let mut missing_proof = serde_json::to_value(&document).unwrap();
        missing_proof["comparison"]
            .as_object_mut()
            .unwrap()
            .remove("proofRef");
        let missing_proof: AssociationComparisonDocumentV1 =
            serde_json::from_value(missing_proof).unwrap();
        assert!(missing_proof.validate_integrity().is_err());

        let mut stale_cache = serde_json::to_value(document).unwrap();
        stale_cache["cacheKey"] = serde_json::Value::String("sha256:forged".to_owned());
        let stale_cache: AssociationComparisonDocumentV1 =
            serde_json::from_value(stale_cache).unwrap();
        assert!(stale_cache.validate_integrity().is_err());
    }

    #[test]
    fn mutable_comparisons_cannot_bypass_exact_revision_state_or_commit_authority() {
        let mut wrong_revision = exact_document();
        wrong_revision
            .comparison
            .revision
            .object_artifact_content_hash = "sha256:wrong".to_owned();
        assert!(wrong_revision.validate_integrity().is_err());

        let mut unproven_state = exact_document();
        unproven_state.proof_availability = AssociationProofAvailabilityV1::NotRequested;
        assert!(unproven_state.validate_integrity().is_err());

        let mut empty_proof_ref = exact_document();
        empty_proof_ref.comparison.proof_ref = Some(" ".to_owned());
        assert!(empty_proof_ref.validate_integrity().is_err());

        let mut empty_commit = exact_document();
        empty_commit.comparison.commit_oid = String::new();
        assert!(empty_commit.validate_integrity().is_err());

        let mut changed_diagnostics = exact_document();
        changed_diagnostics.diagnostics.push("forged".to_owned());
        assert!(changed_diagnostics.validate_integrity().is_err());
    }

    #[test]
    fn qualified_states_cannot_be_constructed_without_available_proof() {
        for state in [
            AssociationComparisonStateV1::Exact,
            AssociationComparisonStateV1::Equivalent,
            AssociationComparisonStateV1::Extension,
        ] {
            assert!(
                AssociationComparisonDocumentV1::new(
                    identity(),
                    state,
                    AssociationProofAvailabilityV1::Missing,
                    Vec::new(),
                )
                .is_err()
            );
        }
    }
}
