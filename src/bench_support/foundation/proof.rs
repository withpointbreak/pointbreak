use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{QualificationRecordKindV1, QualificationRecordV1};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};

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
pub struct CanonicalRawEntryV1 {
    pub path: String,
    pub previous_path: Option<String>,
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
pub struct CanonicalProofInputV1 {
    pub capture_mode: ProofCaptureModeV1,
    pub base: Option<String>,
    pub parent: Option<String>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationSemanticV1 {
    ExactMaterialization,
    EquivalentRewrite,
    ContentPreservingExtension,
    LandingProvenance,
    RelatedProvenance,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationProofStatusV1 {
    Verified,
    Asserted,
    Unverified,
    Indeterminate,
    Refuted,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct RelationProofResultV1 {
    pub semantic_relation: RelationSemanticV1,
    pub proof_status: RelationProofStatusV1,
    pub additions: Vec<CanonicalRawEntryV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct RelationProofManifestV1 {
    pub schema: String,
    pub algorithm: RelationProofAlgorithmV1,
    pub algorithm_version: String,
    pub generation_revision_id: String,
    pub object_artifact_content_hash: String,
    pub association_id: String,
    pub source: CanonicalProofInputV1,
    pub candidate: CanonicalProofInputV1,
    pub result: RelationProofResultV1,
    pub evidence_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofEvidenceStateV1 {
    Available,
    Removed,
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RelationProofProjectionV1 {
    pub generation_revision_id: String,
    pub object_artifact_content_hash: String,
    pub result: RelationProofResultV1,
    pub evidence_state: ProofEvidenceStateV1,
    pub reproducible: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RelationCandidateSignalsV1 {
    pub ancestry_match: bool,
    pub path_overlap: bool,
    pub stable_patch_id_match: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RelationProofError {
    #[error("relation proof identity fields must not be empty")]
    EmptyIdentity,
    #[error("relation proof could not be canonicalized: {message}")]
    Canonicalization { message: String },
}

#[derive(Serialize)]
struct RelationProofHashPreimage<'a> {
    schema: &'a str,
    algorithm: RelationProofAlgorithmV1,
    algorithm_version: &'a str,
    generation_revision_id: &'a str,
    object_artifact_content_hash: &'a str,
    association_id: &'a str,
    source: &'a CanonicalProofInputV1,
    candidate: &'a CanonicalProofInputV1,
    result: &'a RelationProofResultV1,
}

pub fn evaluate_relation_proof_v1(
    generation_revision_id: impl Into<String>,
    object_artifact_content_hash: impl Into<String>,
    association_id: impl Into<String>,
    algorithm: RelationProofAlgorithmV1,
    source: CanonicalProofInputV1,
    candidate: CanonicalProofInputV1,
) -> Result<RelationProofManifestV1, RelationProofError> {
    let source = source.canonicalized();
    let candidate = candidate.canonicalized();
    let semantic_relation = algorithm.semantic_relation();
    let result = if algorithm == RelationProofAlgorithmV1::AttributionOnly {
        RelationProofResultV1 {
            semantic_relation,
            proof_status: RelationProofStatusV1::Asserted,
            additions: Vec::new(),
        }
    } else if source.git_availability == ProofGitAvailabilityV1::Missing
        || candidate.git_availability == ProofGitAvailabilityV1::Missing
    {
        RelationProofResultV1 {
            semantic_relation,
            proof_status: RelationProofStatusV1::Indeterminate,
            additions: Vec::new(),
        }
    } else {
        evaluate_available_inputs(algorithm, &source, &candidate)
    };

    RelationProofManifestV1::new(
        generation_revision_id,
        object_artifact_content_hash,
        association_id,
        algorithm,
        source,
        candidate,
        result,
    )
}

pub fn asserted_relation_proof_v1(
    generation_revision_id: impl Into<String>,
    object_artifact_content_hash: impl Into<String>,
    association_id: impl Into<String>,
    semantic_relation: RelationSemanticV1,
    source: CanonicalProofInputV1,
    candidate: CanonicalProofInputV1,
) -> Result<RelationProofManifestV1, RelationProofError> {
    let source = source.canonicalized();
    let candidate = candidate.canonicalized();
    RelationProofManifestV1::new(
        generation_revision_id,
        object_artifact_content_hash,
        association_id,
        RelationProofAlgorithmV1::AttributionOnly,
        source,
        candidate,
        RelationProofResultV1 {
            semantic_relation,
            proof_status: RelationProofStatusV1::Asserted,
            additions: Vec::new(),
        },
    )
}

pub fn relation_candidate_signals_v1(
    _signals: RelationCandidateSignalsV1,
) -> RelationProofResultV1 {
    RelationProofResultV1 {
        semantic_relation: RelationSemanticV1::Unknown,
        proof_status: RelationProofStatusV1::Unverified,
        additions: Vec::new(),
    }
}

impl CanonicalProofInputV1 {
    fn canonicalized(mut self) -> Self {
        self.path_scope.sort();
        self.path_scope.dedup();
        self.entries.sort();
        self.entries.dedup();
        self
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

    fn semantic_relation(self) -> RelationSemanticV1 {
        match self {
            Self::ExactMaterialization => RelationSemanticV1::ExactMaterialization,
            Self::CanonicalEquivalentRewrite => RelationSemanticV1::EquivalentRewrite,
            Self::ContentPreservingExtension => RelationSemanticV1::ContentPreservingExtension,
            Self::AttributionOnly => RelationSemanticV1::Unknown,
        }
    }
}

impl RelationProofManifestV1 {
    #[allow(clippy::too_many_arguments)]
    fn new(
        generation_revision_id: impl Into<String>,
        object_artifact_content_hash: impl Into<String>,
        association_id: impl Into<String>,
        algorithm: RelationProofAlgorithmV1,
        source: CanonicalProofInputV1,
        candidate: CanonicalProofInputV1,
        result: RelationProofResultV1,
    ) -> Result<Self, RelationProofError> {
        let generation_revision_id = generation_revision_id.into();
        let object_artifact_content_hash = object_artifact_content_hash.into();
        let association_id = association_id.into();
        if generation_revision_id.trim().is_empty()
            || object_artifact_content_hash.trim().is_empty()
            || association_id.trim().is_empty()
        {
            return Err(RelationProofError::EmptyIdentity);
        }
        let mut manifest = Self {
            schema: RELATION_PROOF_SCHEMA_V1.to_owned(),
            algorithm,
            algorithm_version: algorithm.version().to_owned(),
            generation_revision_id,
            object_artifact_content_hash,
            association_id,
            source,
            candidate,
            result,
            evidence_sha256: String::new(),
        };
        manifest.evidence_sha256 = manifest.computed_evidence_sha256()?;
        Ok(manifest)
    }

    pub fn project_evidence(
        &self,
        evidence_state: ProofEvidenceStateV1,
    ) -> RelationProofProjectionV1 {
        RelationProofProjectionV1 {
            generation_revision_id: self.generation_revision_id.clone(),
            object_artifact_content_hash: self.object_artifact_content_hash.clone(),
            result: self.result.clone(),
            evidence_state,
            reproducible: evidence_state == ProofEvidenceStateV1::Available
                && self.result.proof_status == RelationProofStatusV1::Verified,
        }
    }

    pub fn to_qualification_record(
        &self,
        logical_key: impl Into<String>,
    ) -> Result<QualificationRecordV1, RelationProofError> {
        let value = serde_json::to_value(self).map_err(canonicalization_error)?;
        let bytes = canonical_json_bytes(&value).map_err(canonicalization_error)?;
        Ok(QualificationRecordV1::new(
            logical_key,
            QualificationRecordKindV1::RelationProof,
            bytes,
        ))
    }

    fn computed_evidence_sha256(&self) -> Result<String, RelationProofError> {
        let preimage = RelationProofHashPreimage {
            schema: &self.schema,
            algorithm: self.algorithm,
            algorithm_version: &self.algorithm_version,
            generation_revision_id: &self.generation_revision_id,
            object_artifact_content_hash: &self.object_artifact_content_hash,
            association_id: &self.association_id,
            source: &self.source,
            candidate: &self.candidate,
            result: &self.result,
        };
        let value = serde_json::to_value(preimage).map_err(canonicalization_error)?;
        let bytes = canonical_json_bytes(&value).map_err(canonicalization_error)?;
        Ok(sha256_bytes_hex(&bytes))
    }
}

fn evaluate_available_inputs(
    algorithm: RelationProofAlgorithmV1,
    source: &CanonicalProofInputV1,
    candidate: &CanonicalProofInputV1,
) -> RelationProofResultV1 {
    let semantic_relation = algorithm.semantic_relation();
    let inputs_are_valid = canonical_input_is_valid(source) && canonical_input_is_valid(candidate);
    let (verified, additions) = match algorithm {
        RelationProofAlgorithmV1::ExactMaterialization => {
            (inputs_are_valid && source == candidate, Vec::new())
        }
        RelationProofAlgorithmV1::CanonicalEquivalentRewrite => (
            inputs_are_valid
                && source.capture_mode == candidate.capture_mode
                && source.path_scope == candidate.path_scope
                && source.entries == candidate.entries,
            Vec::new(),
        ),
        RelationProofAlgorithmV1::ContentPreservingExtension => {
            let additions = candidate
                .entries
                .iter()
                .filter(|entry| !source.entries.contains(entry))
                .cloned()
                .collect::<Vec<_>>();
            let preserves_source = source
                .entries
                .iter()
                .all(|entry| candidate.entries.contains(entry));
            (
                inputs_are_valid
                    && source.capture_mode == candidate.capture_mode
                    && source.path_scope == candidate.path_scope
                    && preserves_source
                    && !additions.is_empty(),
                additions,
            )
        }
        RelationProofAlgorithmV1::AttributionOnly => (false, Vec::new()),
    };
    RelationProofResultV1 {
        semantic_relation,
        proof_status: if verified {
            RelationProofStatusV1::Verified
        } else {
            RelationProofStatusV1::Refuted
        },
        additions,
    }
}

fn canonical_input_is_valid(input: &CanonicalProofInputV1) -> bool {
    if input.path_scope.is_empty() || input.path_scope.iter().any(String::is_empty) {
        return false;
    }

    let mut current_paths = BTreeSet::new();
    input.entries.iter().all(|entry| {
        current_paths.insert(entry.path.as_str())
            && path_is_in_scope(&entry.path, &input.path_scope)
            && entry
                .previous_path
                .as_deref()
                .is_none_or(|path| path_is_in_scope(path, &input.path_scope))
    })
}

fn path_is_in_scope(path: &str, scopes: &[String]) -> bool {
    scopes.iter().any(|scope| {
        scope == "."
            || path == scope
            || path
                .strip_prefix(scope)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

fn canonicalization_error(error: impl std::fmt::Display) -> RelationProofError {
    RelationProofError::Canonicalization {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPTURE_MATRIX: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/store-foundation/proof/capture-matrix.json"
    ));

    fn capture_matrix() -> Vec<CanonicalProofInputV1> {
        serde_json::from_str(CAPTURE_MATRIX).expect("valid capture matrix")
    }

    fn source_and_candidate() -> (CanonicalProofInputV1, CanonicalProofInputV1) {
        let source = capture_matrix()
            .into_iter()
            .find(|input| input.capture_mode == ProofCaptureModeV1::CommitRange)
            .expect("commit-range fixture");
        (source.clone(), source)
    }

    #[test]
    fn capture_matrix_covers_modes_and_canonical_git_semantics() {
        let matrix = capture_matrix();
        let modes = matrix
            .iter()
            .map(|input| input.capture_mode)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            modes,
            BTreeSet::from([
                ProofCaptureModeV1::CommitRange,
                ProofCaptureModeV1::Root,
                ProofCaptureModeV1::Staged,
                ProofCaptureModeV1::Unstaged,
                ProofCaptureModeV1::CombinedWorktree,
                ProofCaptureModeV1::SyntheticUntracked,
            ])
        );

        let entries = matrix
            .iter()
            .flat_map(|input| &input.entries)
            .collect::<Vec<_>>();
        let changes = entries
            .iter()
            .map(|entry| entry.change)
            .collect::<BTreeSet<_>>();
        assert!(changes.contains(&CanonicalChangeV1::Added));
        assert!(changes.contains(&CanonicalChangeV1::Deleted));
        assert!(changes.contains(&CanonicalChangeV1::Modified));
        assert!(changes.contains(&CanonicalChangeV1::Renamed));
        assert!(changes.contains(&CanonicalChangeV1::Copied));
        assert!(changes.contains(&CanonicalChangeV1::ModeOnly));
        assert!(
            entries
                .iter()
                .any(|entry| entry.content_kind == CanonicalContentKindV1::Binary)
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.content_kind == CanonicalContentKindV1::Submodule)
        );
        assert!(entries.iter().any(|entry| entry.untracked));
        assert!(matrix.iter().all(|input| !input.path_scope.is_empty()));
        assert!(
            matrix
                .iter()
                .any(|input| input.git_availability == ProofGitAvailabilityV1::Missing)
        );
    }

    #[test]
    fn exact_materialization_requires_identical_base_scope_and_entries() {
        let (source, candidate) = source_and_candidate();
        let verified = evaluate_relation_proof_v1(
            "rev:fixture",
            "sha256:object-fixture",
            "association:fixture",
            RelationProofAlgorithmV1::ExactMaterialization,
            source.clone(),
            candidate,
        )
        .expect("proof evaluates");
        assert_eq!(
            verified.result,
            RelationProofResultV1 {
                semantic_relation: RelationSemanticV1::ExactMaterialization,
                proof_status: RelationProofStatusV1::Verified,
                additions: Vec::new(),
            }
        );

        let mut different_scope = source.clone();
        different_scope.path_scope = vec!["other".to_owned()];
        let refuted = evaluate_relation_proof_v1(
            "rev:fixture",
            "sha256:object-fixture",
            "association:fixture",
            RelationProofAlgorithmV1::ExactMaterialization,
            source,
            different_scope,
        )
        .expect("proof evaluates");
        assert_eq!(refuted.result.proof_status, RelationProofStatusV1::Refuted);
    }

    #[test]
    fn named_canonical_equivalence_and_extension_are_distinct() {
        let (source, mut rewrite) = source_and_candidate();
        rewrite.base = Some("commit:rewritten-base".to_owned());
        rewrite.parent = Some("commit:rewritten-parent".to_owned());
        let equivalent = evaluate_relation_proof_v1(
            "rev:fixture",
            "sha256:object-fixture",
            "association:fixture",
            RelationProofAlgorithmV1::CanonicalEquivalentRewrite,
            source.clone(),
            rewrite,
        )
        .expect("proof evaluates");
        assert_eq!(
            equivalent.result.semantic_relation,
            RelationSemanticV1::EquivalentRewrite
        );
        assert_eq!(
            equivalent.result.proof_status,
            RelationProofStatusV1::Verified
        );

        let mut extension = source.clone();
        extension.entries.push(CanonicalRawEntryV1 {
            path: "src/extra.txt".to_owned(),
            previous_path: None,
            change: CanonicalChangeV1::Added,
            old_oid: None,
            new_oid: Some("oid:extra".to_owned()),
            old_mode: None,
            new_mode: Some("100644".to_owned()),
            old_decoded_sha256: None,
            new_decoded_sha256: Some("sha256:extra".to_owned()),
            content_kind: CanonicalContentKindV1::Text,
            untracked: false,
        });
        let extended = evaluate_relation_proof_v1(
            "rev:fixture",
            "sha256:object-fixture",
            "association:fixture",
            RelationProofAlgorithmV1::ContentPreservingExtension,
            source,
            extension,
        )
        .expect("proof evaluates");
        assert_eq!(
            extended.result.semantic_relation,
            RelationSemanticV1::ContentPreservingExtension
        );
        assert_eq!(extended.result.additions.len(), 1);
    }

    #[test]
    fn verified_proof_rejects_out_of_scope_and_conflicting_entries() {
        let (source, _) = source_and_candidate();
        let mut out_of_scope = source.clone();
        out_of_scope.entries.push(CanonicalRawEntryV1 {
            path: "outside.txt".to_owned(),
            previous_path: None,
            change: CanonicalChangeV1::Added,
            old_oid: None,
            new_oid: Some("oid:outside".to_owned()),
            old_mode: None,
            new_mode: Some("100644".to_owned()),
            old_decoded_sha256: None,
            new_decoded_sha256: Some("sha256:outside".to_owned()),
            content_kind: CanonicalContentKindV1::Text,
            untracked: false,
        });
        let out_of_scope = evaluate_relation_proof_v1(
            "rev:fixture",
            "sha256:object-fixture",
            "association:fixture",
            RelationProofAlgorithmV1::ContentPreservingExtension,
            source.clone(),
            out_of_scope,
        )
        .expect("proof evaluates");
        assert_eq!(
            out_of_scope.result.proof_status,
            RelationProofStatusV1::Refuted
        );

        let mut conflicting = source.clone();
        let mut duplicate_path = conflicting.entries[0].clone();
        duplicate_path.new_oid = Some("oid:conflict".to_owned());
        conflicting.entries.push(duplicate_path);
        let conflicting = evaluate_relation_proof_v1(
            "rev:fixture",
            "sha256:object-fixture",
            "association:fixture",
            RelationProofAlgorithmV1::CanonicalEquivalentRewrite,
            source,
            conflicting,
        )
        .expect("proof evaluates");
        assert_eq!(
            conflicting.result.proof_status,
            RelationProofStatusV1::Refuted
        );
    }

    #[test]
    fn candidate_signals_and_missing_git_never_authorize_verified_proof() {
        let signals = relation_candidate_signals_v1(RelationCandidateSignalsV1 {
            ancestry_match: true,
            path_overlap: true,
            stable_patch_id_match: true,
        });
        assert_eq!(signals.semantic_relation, RelationSemanticV1::Unknown);
        assert_eq!(signals.proof_status, RelationProofStatusV1::Unverified);

        let (source, mut candidate) = source_and_candidate();
        candidate.git_availability = ProofGitAvailabilityV1::Missing;
        let missing = evaluate_relation_proof_v1(
            "rev:fixture",
            "sha256:object-fixture",
            "association:fixture",
            RelationProofAlgorithmV1::CanonicalEquivalentRewrite,
            source,
            candidate,
        )
        .expect("proof evaluates");
        assert_eq!(
            missing.result.proof_status,
            RelationProofStatusV1::Indeterminate
        );
    }

    #[test]
    fn proof_status_is_independent_from_semantic_relation_and_evidence_state() {
        let (source, candidate) = source_and_candidate();
        let asserted = asserted_relation_proof_v1(
            "rev:fixture",
            "sha256:object-fixture",
            "association:fixture",
            RelationSemanticV1::LandingProvenance,
            source.clone(),
            candidate.clone(),
        )
        .expect("assertion builds");
        assert_eq!(
            asserted.result.proof_status,
            RelationProofStatusV1::Asserted
        );

        let verified = evaluate_relation_proof_v1(
            "rev:fixture",
            "sha256:object-fixture",
            "association:fixture",
            RelationProofAlgorithmV1::ExactMaterialization,
            source,
            candidate,
        )
        .expect("proof evaluates");
        let removed = verified.project_evidence(ProofEvidenceStateV1::Removed);
        assert_eq!(removed.result, verified.result);
        assert_eq!(
            removed.generation_revision_id,
            verified.generation_revision_id
        );
        assert_eq!(
            removed.object_artifact_content_hash,
            verified.object_artifact_content_hash
        );
        assert!(!removed.reproducible);
    }
}
