use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::EvidenceAvailabilityV1;
use crate::canonical_hash::sha256_json_prefixed;
use crate::error::{Result, ShoreError};
use crate::model::{CommitAssociationId, RevisionRefV1};
use crate::session::event::{
    RelationProofStatusV1, RevisionRelationAttestedPayload, SemanticRevisionRelationV1,
};

pub const RELATION_PROOF_SCHEMA_V1: &str = "pointbreak.relation-proof.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofCaptureModeV1 {
    CommitRange,
    Root,
    Staged,
    Unstaged,
    CombinedWorktree,
    SyntheticUntracked,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofGitAvailabilityV1 {
    Available,
    Missing,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalChangeV1 {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
    ModeOnly,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalContentKindV1 {
    Text,
    Binary,
    Symlink,
    Submodule,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanonicalRawEntryV1 {
    pub path_identity: String,
    pub previous_path_identity: Option<String>,
    pub change: CanonicalChangeV1,
    pub old_oid: Option<String>,
    pub new_oid: Option<String>,
    pub old_mode: Option<String>,
    pub new_mode: Option<String>,
    pub old_decoded_sha256: Option<String>,
    pub new_decoded_sha256: Option<String>,
    pub content_kind: CanonicalContentKindV1,
    pub untracked: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanonicalProofInputV1 {
    pub capture_mode: ProofCaptureModeV1,
    pub base_or_parent: Option<String>,
    pub path_scope: Vec<String>,
    pub git_availability: ProofGitAvailabilityV1,
    pub entries: Vec<CanonicalRawEntryV1>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationProofAlgorithmV1 {
    ExactMaterialization,
    CanonicalEquivalentRewrite,
    ContentPreservingExtension,
    AttributionOnly,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelationProofResultV1 {
    pub semantic_relation: SemanticRevisionRelationV1,
    pub proof_status: RelationProofStatusV1,
    pub additions: Vec<CanonicalRawEntryV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelationProofManifestV1 {
    pub schema: String,
    pub version: u32,
    pub algorithm: RelationProofAlgorithmV1,
    pub algorithm_version: String,
    pub revision: RevisionRefV1,
    pub association_id: CommitAssociationId,
    pub source: CanonicalProofInputV1,
    pub candidate: CanonicalProofInputV1,
    pub result: RelationProofResultV1,
    pub evidence_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationProofProjectionV1 {
    pub revision: RevisionRefV1,
    pub result: RelationProofResultV1,
    pub evidence_availability: EvidenceAvailabilityV1,
    pub reproducible: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionRelationEvidenceV1 {
    pub revision: RevisionRefV1,
    pub association_id: CommitAssociationId,
    pub semantic_relation: SemanticRevisionRelationV1,
    pub proof_status: RelationProofStatusV1,
    pub conflicting: bool,
    pub content_qualified: bool,
}

impl RevisionRelationEvidenceV1 {
    pub fn unknown(revision: RevisionRefV1, association_id: CommitAssociationId) -> Self {
        Self {
            revision,
            association_id,
            semantic_relation: SemanticRevisionRelationV1::Unknown,
            proof_status: RelationProofStatusV1::Unverified,
            conflicting: false,
            content_qualified: false,
        }
    }
}

pub fn project_revision_relation_evidence_v1(
    revision: RevisionRefV1,
    association_id: CommitAssociationId,
    attestations: &[RevisionRelationAttestedPayload],
    proofs: &BTreeMap<String, (RelationProofManifestV1, EvidenceAvailabilityV1)>,
) -> Result<RevisionRelationEvidenceV1> {
    for attestation in attestations {
        attestation.validate()?;
    }
    let matching: Vec<_> = attestations
        .iter()
        .filter(|attestation| {
            attestation.revision == revision && attestation.commit_association_id == association_id
        })
        .collect();
    if matching.is_empty() {
        return Ok(RevisionRelationEvidenceV1::unknown(
            revision,
            association_id,
        ));
    }
    let distinct: BTreeSet<_> = matching
        .iter()
        .map(|attestation| attestation.relation_attestation_id.clone())
        .collect();
    if distinct.len() != 1 {
        return Ok(RevisionRelationEvidenceV1 {
            revision,
            association_id,
            semantic_relation: SemanticRevisionRelationV1::Unknown,
            proof_status: RelationProofStatusV1::Unverified,
            conflicting: true,
            content_qualified: false,
        });
    }
    let attestation = matching[0];
    let proof_available = attestation
        .evidence_content_hash
        .as_ref()
        .and_then(|hash| proofs.get(hash))
        .is_some_and(|(proof, availability)| {
            *availability == EvidenceAvailabilityV1::Available
                && proof.revision == revision
                && proof.association_id == association_id
                && proof.evidence_sha256
                    == attestation
                        .evidence_content_hash
                        .clone()
                        .unwrap_or_default()
                && proof.result.semantic_relation == attestation.semantic_relation
                && proof.result.proof_status == attestation.proof_status
                && proof.algorithm_version == attestation.proof_algorithm_version
                && proof.result_digest().ok().as_deref() == Some(attestation.result_digest.as_str())
                && proof.validate().is_ok()
        });
    let content_relation = matches!(
        attestation.semantic_relation,
        SemanticRevisionRelationV1::ExactMaterialization
            | SemanticRevisionRelationV1::EquivalentRewrite
            | SemanticRevisionRelationV1::ContentPreservingExtension
    );
    Ok(RevisionRelationEvidenceV1 {
        revision,
        association_id,
        semantic_relation: attestation.semantic_relation,
        proof_status: attestation.proof_status,
        conflicting: false,
        content_qualified: content_relation
            && attestation.proof_status == RelationProofStatusV1::Verified
            && proof_available,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RelationCandidateSignalsV1 {
    pub ancestry_match: bool,
    pub path_overlap: bool,
    pub stable_patch_id_match: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RelationProofHashPreimage<'a> {
    schema: &'a str,
    version: u32,
    algorithm: RelationProofAlgorithmV1,
    algorithm_version: &'a str,
    revision: &'a RevisionRefV1,
    association_id: &'a CommitAssociationId,
    source: &'a CanonicalProofInputV1,
    candidate: &'a CanonicalProofInputV1,
    result: &'a RelationProofResultV1,
}

impl RelationProofManifestV1 {
    pub fn new(
        revision: RevisionRefV1,
        association_id: CommitAssociationId,
        algorithm: RelationProofAlgorithmV1,
        mut source: CanonicalProofInputV1,
        mut candidate: CanonicalProofInputV1,
        mut result: RelationProofResultV1,
    ) -> Result<Self> {
        canonicalize_input(&mut source);
        canonicalize_input(&mut candidate);
        result.additions.sort();
        result.additions.dedup();
        let mut manifest = Self {
            schema: RELATION_PROOF_SCHEMA_V1.to_owned(),
            version: 1,
            algorithm,
            algorithm_version: algorithm.version().to_owned(),
            revision,
            association_id,
            source,
            candidate,
            result,
            evidence_sha256: String::new(),
        };
        manifest.evidence_sha256 = manifest.computed_evidence_sha256()?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != RELATION_PROOF_SCHEMA_V1
            || self.version != 1
            || self.algorithm_version != self.algorithm.version()
            || !is_sorted_unique(&self.source.path_scope)
            || !is_sorted_unique(&self.source.entries)
            || !is_sorted_unique(&self.candidate.path_scope)
            || !is_sorted_unique(&self.candidate.entries)
            || !is_sorted_unique(&self.result.additions)
            || !self.verified_content_relation_matches_algorithm()
            || self.evidence_sha256 != self.computed_evidence_sha256()?
        {
            return Err(ShoreError::Message(
                "relation proof schema, algorithm, or evidence hash is invalid".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn project(&self, availability: EvidenceAvailabilityV1) -> RelationProofProjectionV1 {
        RelationProofProjectionV1 {
            revision: self.revision.clone(),
            result: self.result.clone(),
            evidence_availability: availability,
            reproducible: availability == EvidenceAvailabilityV1::Available
                && self.result.proof_status == RelationProofStatusV1::Verified
                && self.validate().is_ok(),
        }
    }

    fn computed_evidence_sha256(&self) -> Result<String> {
        sha256_json_prefixed(&serde_json::to_value(RelationProofHashPreimage {
            schema: &self.schema,
            version: self.version,
            algorithm: self.algorithm,
            algorithm_version: &self.algorithm_version,
            revision: &self.revision,
            association_id: &self.association_id,
            source: &self.source,
            candidate: &self.candidate,
            result: &self.result,
        })?)
    }

    fn result_digest(&self) -> Result<String> {
        sha256_json_prefixed(&serde_json::to_value(&self.result)?)
    }

    fn verified_content_relation_matches_algorithm(&self) -> bool {
        if self.result.proof_status != RelationProofStatusV1::Verified {
            return true;
        }
        match self.result.semantic_relation {
            SemanticRevisionRelationV1::ExactMaterialization => {
                self.algorithm == RelationProofAlgorithmV1::ExactMaterialization
            }
            SemanticRevisionRelationV1::EquivalentRewrite => {
                self.algorithm == RelationProofAlgorithmV1::CanonicalEquivalentRewrite
            }
            SemanticRevisionRelationV1::ContentPreservingExtension => {
                self.algorithm == RelationProofAlgorithmV1::ContentPreservingExtension
            }
            SemanticRevisionRelationV1::LandingProvenance
            | SemanticRevisionRelationV1::RelatedProvenance
            | SemanticRevisionRelationV1::Unknown => true,
        }
    }
}

impl RelationProofAlgorithmV1 {
    fn version(self) -> &'static str {
        match self {
            Self::ExactMaterialization => "exact-materialization-v1",
            Self::CanonicalEquivalentRewrite => "canonical-equivalent-rewrite-v1",
            Self::ContentPreservingExtension => "content-preserving-extension-v1",
            Self::AttributionOnly => "attribution-only-v1",
        }
    }
}

fn canonicalize_input(input: &mut CanonicalProofInputV1) {
    input.path_scope.sort();
    input.path_scope.dedup();
    input.entries.sort();
    input.entries.dedup();
}

fn is_sorted_unique<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

pub fn relation_candidate_signals_v1(
    _signals: RelationCandidateSignalsV1,
) -> RelationProofResultV1 {
    RelationProofResultV1 {
        semantic_relation: SemanticRevisionRelationV1::Unknown,
        proof_status: RelationProofStatusV1::Unverified,
        additions: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CommitAssociationId, RevisionId, RevisionRefV1};

    fn revision() -> RevisionRefV1 {
        RevisionRefV1::new(
            RevisionId::new("rev:sha256:test"),
            format!("sha256:{}", "a".repeat(64)),
        )
        .unwrap()
    }

    #[test]
    fn candidate_signals_never_self_authorize_and_only_available_proof_is_reproducible() {
        let result = relation_candidate_signals_v1(RelationCandidateSignalsV1 {
            ancestry_match: true,
            path_overlap: true,
            stable_patch_id_match: true,
        });
        assert_eq!(
            result.semantic_relation,
            SemanticRevisionRelationV1::Unknown
        );
        assert_eq!(result.proof_status, RelationProofStatusV1::Unverified);

        let proof = RelationProofManifestV1::new(
            revision(),
            CommitAssociationId::new("assoc-commit:sha256:test"),
            RelationProofAlgorithmV1::ExactMaterialization,
            canonical_input(),
            canonical_input(),
            RelationProofResultV1 {
                semantic_relation: SemanticRevisionRelationV1::ExactMaterialization,
                proof_status: RelationProofStatusV1::Verified,
                additions: Vec::new(),
            },
        )
        .unwrap();
        assert!(
            proof
                .project(EvidenceAvailabilityV1::Available)
                .reproducible
        );
        assert!(!proof.project(EvidenceAvailabilityV1::Removed).reproducible);
    }

    fn canonical_input() -> CanonicalProofInputV1 {
        CanonicalProofInputV1 {
            capture_mode: ProofCaptureModeV1::CommitRange,
            base_or_parent: Some("abc".to_owned()),
            path_scope: vec!["src".to_owned()],
            git_availability: ProofGitAvailabilityV1::Available,
            entries: Vec::new(),
        }
    }

    #[test]
    fn missing_attestation_defaults_unknown_and_conflicting_results_withhold_authorization() {
        let revision = revision();
        let association = CommitAssociationId::new("assoc-commit:sha256:test");
        let empty = project_revision_relation_evidence_v1(
            revision.clone(),
            association.clone(),
            &[],
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(empty.semantic_relation, SemanticRevisionRelationV1::Unknown);
        assert_eq!(empty.proof_status, RelationProofStatusV1::Unverified);

        let exact = crate::session::event::build_revision_relation_attested(
            crate::session::event::RevisionRelationAttestationDraftV1 {
                revision: revision.clone(),
                commit_association_id: association.clone(),
                semantic_relation: SemanticRevisionRelationV1::LandingProvenance,
                proof_status: RelationProofStatusV1::Asserted,
                proof_method: "operator".to_owned(),
                proof_algorithm_version: "1".to_owned(),
                capture_scope: Vec::new(),
                comparison_base_or_parent: None,
                endpoint_oids: Vec::new(),
                evidence_content_hash: None,
                result_digest: format!("sha256:{}", "c".repeat(64)),
            },
        )
        .unwrap();
        let related = crate::session::event::build_revision_relation_attested(
            crate::session::event::RevisionRelationAttestationDraftV1 {
                revision: revision.clone(),
                commit_association_id: association.clone(),
                semantic_relation: SemanticRevisionRelationV1::RelatedProvenance,
                proof_status: RelationProofStatusV1::Asserted,
                proof_method: "operator".to_owned(),
                proof_algorithm_version: "1".to_owned(),
                capture_scope: Vec::new(),
                comparison_base_or_parent: None,
                endpoint_oids: Vec::new(),
                evidence_content_hash: None,
                result_digest: format!("sha256:{}", "d".repeat(64)),
            },
        )
        .unwrap();
        let view = project_revision_relation_evidence_v1(
            revision,
            association,
            &[exact, related],
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(view.conflicting);
        assert!(!view.content_qualified);
    }

    #[test]
    fn content_qualification_requires_the_exact_available_proof_result() {
        let revision = revision();
        let association = CommitAssociationId::new("assoc-commit:sha256:qualified");
        let proof = RelationProofManifestV1::new(
            revision.clone(),
            association.clone(),
            RelationProofAlgorithmV1::ExactMaterialization,
            canonical_input(),
            canonical_input(),
            RelationProofResultV1 {
                semantic_relation: SemanticRevisionRelationV1::ExactMaterialization,
                proof_status: RelationProofStatusV1::Verified,
                additions: Vec::new(),
            },
        )
        .unwrap();
        let attestation = crate::session::event::build_revision_relation_attested(
            crate::session::event::RevisionRelationAttestationDraftV1 {
                revision: revision.clone(),
                commit_association_id: association.clone(),
                semantic_relation: proof.result.semantic_relation,
                proof_status: proof.result.proof_status,
                proof_method: "canonical-tree".to_owned(),
                proof_algorithm_version: proof.algorithm_version.clone(),
                capture_scope: proof.source.path_scope.clone(),
                comparison_base_or_parent: proof.source.base_or_parent.clone(),
                endpoint_oids: Vec::new(),
                evidence_content_hash: Some(proof.evidence_sha256.clone()),
                result_digest: proof.result_digest().unwrap(),
            },
        )
        .unwrap();
        let proofs = [(
            proof.evidence_sha256.clone(),
            (proof, EvidenceAvailabilityV1::Available),
        )]
        .into();

        let qualified = project_revision_relation_evidence_v1(
            revision.clone(),
            association.clone(),
            std::slice::from_ref(&attestation),
            &proofs,
        )
        .unwrap();
        assert!(qualified.content_qualified);

        let mismatched = crate::session::event::build_revision_relation_attested(
            crate::session::event::RevisionRelationAttestationDraftV1 {
                revision: revision.clone(),
                commit_association_id: association.clone(),
                semantic_relation: SemanticRevisionRelationV1::ExactMaterialization,
                proof_status: RelationProofStatusV1::Verified,
                proof_method: "canonical-tree".to_owned(),
                proof_algorithm_version: "exact-materialization-v1".to_owned(),
                capture_scope: vec!["src".to_owned()],
                comparison_base_or_parent: Some("abc".to_owned()),
                endpoint_oids: Vec::new(),
                evidence_content_hash: Some(proofs.keys().next().unwrap().clone()),
                result_digest: format!("sha256:{}", "f".repeat(64)),
            },
        )
        .unwrap();
        let unqualified =
            project_revision_relation_evidence_v1(revision, association, &[mismatched], &proofs)
                .unwrap();
        assert!(!unqualified.content_qualified);
    }
}
