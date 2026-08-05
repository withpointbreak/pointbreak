#![allow(
    dead_code,
    reason = "writer-dark relation constructors are not routed yet"
)]

use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::type_code;
use super::{EventPayload, EventType};
use crate::canonical_hash::sha256_json_prefixed;
use crate::error::Result;
use crate::model::{
    ActorId, ChangeId, CommitAssociationId, InputRequestId, ObservationId, ReviewFactPortId,
    RevisionRefV1, RevisionRelationAttestationId, TrackId, id_prefix,
};

const RELATION_ATTESTED_SCHEMA_V1: &str = "pointbreak.revision-relation-attested";
const FACT_PORTED_SCHEMA_V1: &str = "pointbreak.review-fact-ported";

/// Meaning established between one captured Revision and one associated commit.
///
/// Structural association remains separate; only an available verified proof
/// can authorize the first three content-qualified labels.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRevisionRelationV1 {
    ExactMaterialization,
    EquivalentRewrite,
    ContentPreservingExtension,
    LandingProvenance,
    RelatedProvenance,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationProofStatusV1 {
    Verified,
    Asserted,
    Unverified,
    Indeterminate,
    Refuted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevisionRelationAttestedPayload {
    pub schema: String,
    pub version: u32,
    pub relation_attestation_id: RevisionRelationAttestationId,
    pub revision: RevisionRefV1,
    pub commit_association_id: CommitAssociationId,
    pub semantic_relation: SemanticRevisionRelationV1,
    pub proof_status: RelationProofStatusV1,
    pub proof_method: String,
    pub proof_algorithm_version: String,
    pub capture_scope: Vec<String>,
    pub comparison_base_or_parent: Option<String>,
    pub endpoint_oids: Vec<String>,
    pub evidence_content_hash: Option<String>,
    pub result_digest: String,
}

pub(crate) struct RevisionRelationAttestationDraftV1 {
    pub(crate) revision: RevisionRefV1,
    pub(crate) commit_association_id: CommitAssociationId,
    pub(crate) semantic_relation: SemanticRevisionRelationV1,
    pub(crate) proof_status: RelationProofStatusV1,
    pub(crate) proof_method: String,
    pub(crate) proof_algorithm_version: String,
    pub(crate) capture_scope: Vec<String>,
    pub(crate) comparison_base_or_parent: Option<String>,
    pub(crate) endpoint_oids: Vec<String>,
    pub(crate) evidence_content_hash: Option<String>,
    pub(crate) result_digest: String,
}

impl EventPayload for RevisionRelationAttestedPayload {
    fn event_type(&self) -> EventType {
        EventType::RevisionRelationAttested
    }
}

impl RevisionRelationAttestedPayload {
    pub fn validate(&self) -> Result<()> {
        if self.schema != RELATION_ATTESTED_SCHEMA_V1 || self.version != 1 {
            return invalid_relation_payload("unsupported relation-attestation schema/version");
        }
        if self.proof_method.trim().is_empty()
            || self.proof_algorithm_version.trim().is_empty()
            || !is_sorted_unique(&self.capture_scope)
            || !is_sorted_unique(&self.endpoint_oids)
            || !is_prefixed_sha256(&self.result_digest)
            || self
                .evidence_content_hash
                .as_deref()
                .is_some_and(|hash| !is_prefixed_sha256(hash))
        {
            return invalid_relation_payload(
                "relation-attestation proof metadata is not canonical or integrity-qualified",
            );
        }
        let expected = RevisionRelationAttestationId::new(format!(
            "{}:{}",
            id_prefix::REVISION_RELATION_ATTESTATION,
            sha256_json_prefixed(&serde_json::to_value(payload_without_attestation_id(self))?)?
        ));
        if expected != self.relation_attestation_id {
            return invalid_relation_payload("relation-attestation identity mismatch");
        }
        if self.proof_status == RelationProofStatusV1::Verified
            && matches!(
                self.semantic_relation,
                SemanticRevisionRelationV1::ExactMaterialization
                    | SemanticRevisionRelationV1::EquivalentRewrite
                    | SemanticRevisionRelationV1::ContentPreservingExtension
            )
            && self.evidence_content_hash.is_none()
        {
            return invalid_relation_payload(
                "verified content relation requires an evidence content hash",
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactPortRelationV1 {
    ContextOnly,
    ReanchoredAs,
    CarriedOpenAs,
    ResolvedBy,
}

/// A Revision-local fact which may be referenced for continuity presentation.
///
/// Validation and assessment deliberately have no variants: those facts never
/// transfer or port to another Revision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum FactRefV1 {
    Observation { observation_id: ObservationId },
    InputRequest { input_request_id: InputRequestId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewFactPortedPayload {
    pub schema: String,
    pub version: u32,
    pub port_id: ReviewFactPortId,
    pub origin_revision: RevisionRefV1,
    pub origin_fact: FactRefV1,
    pub target_revision: RevisionRefV1,
    pub relation: FactPortRelationV1,
    pub target_fact: Option<FactRefV1>,
    pub rationale_content_hash: Option<String>,
    /// Optional presentation context only; it never changes fact ownership.
    pub context_change_id: Option<ChangeId>,
}

pub(crate) struct ReviewFactPortDraftV1 {
    pub(crate) origin_revision: RevisionRefV1,
    pub(crate) origin_fact: FactRefV1,
    pub(crate) target_revision: RevisionRefV1,
    pub(crate) relation: FactPortRelationV1,
    pub(crate) target_fact: Option<FactRefV1>,
    pub(crate) rationale_content_hash: Option<String>,
    pub(crate) context_change_id: Option<ChangeId>,
}

impl EventPayload for ReviewFactPortedPayload {
    fn event_type(&self) -> EventType {
        EventType::ReviewFactPorted
    }
}

impl ReviewFactPortedPayload {
    pub fn validate(&self) -> Result<()> {
        if self.schema != FACT_PORTED_SCHEMA_V1 || self.version != 1 {
            return invalid_relation_payload("unsupported fact-port schema/version");
        }
        validate_fact_port(self)
    }

    pub fn validate_attribution(&self, actor_id: &ActorId, track_id: &TrackId) -> Result<()> {
        self.validate()?;
        let expected = ReviewFactPortId::new(format!(
            "{}:{}",
            id_prefix::REVIEW_FACT_PORT,
            sha256_json_prefixed(&serde_json::json!({
                "payload": payload_without_port_id(self),
                "actorId": actor_id,
                "trackId": track_id,
            }))?
        ));
        if expected == self.port_id {
            Ok(())
        } else {
            invalid_relation_payload("fact-port identity or attribution mismatch")
        }
    }
}

pub(crate) fn build_revision_relation_attested(
    mut draft: RevisionRelationAttestationDraftV1,
) -> Result<RevisionRelationAttestedPayload> {
    draft.capture_scope.sort();
    draft.capture_scope.dedup();
    draft.endpoint_oids.sort();
    draft.endpoint_oids.dedup();
    let mut payload = RevisionRelationAttestedPayload {
        schema: RELATION_ATTESTED_SCHEMA_V1.to_owned(),
        version: 1,
        relation_attestation_id: RevisionRelationAttestationId::new("pending"),
        revision: draft.revision,
        commit_association_id: draft.commit_association_id,
        semantic_relation: draft.semantic_relation,
        proof_status: draft.proof_status,
        proof_method: draft.proof_method,
        proof_algorithm_version: draft.proof_algorithm_version,
        capture_scope: draft.capture_scope,
        comparison_base_or_parent: draft.comparison_base_or_parent,
        endpoint_oids: draft.endpoint_oids,
        evidence_content_hash: draft.evidence_content_hash,
        result_digest: draft.result_digest,
    };
    payload.relation_attestation_id = RevisionRelationAttestationId::new(format!(
        "{}:{}",
        id_prefix::REVISION_RELATION_ATTESTATION,
        sha256_json_prefixed(&serde_json::to_value(payload_without_attestation_id(
            &payload
        ))?)?
    ));
    payload.validate()?;
    Ok(payload)
}

pub(crate) fn build_review_fact_ported(
    draft: ReviewFactPortDraftV1,
    actor_id: &ActorId,
    track_id: &TrackId,
) -> Result<ReviewFactPortedPayload> {
    let mut payload = ReviewFactPortedPayload {
        schema: FACT_PORTED_SCHEMA_V1.to_owned(),
        version: 1,
        port_id: ReviewFactPortId::new("pending"),
        origin_revision: draft.origin_revision,
        origin_fact: draft.origin_fact,
        target_revision: draft.target_revision,
        relation: draft.relation,
        target_fact: draft.target_fact,
        rationale_content_hash: draft.rationale_content_hash,
        context_change_id: draft.context_change_id,
    };
    validate_fact_port(&payload)?;
    payload.port_id = ReviewFactPortId::new(format!(
        "{}:{}",
        id_prefix::REVIEW_FACT_PORT,
        sha256_json_prefixed(&serde_json::json!({
            "payload": payload_without_port_id(&payload),
            "actorId": actor_id,
            "trackId": track_id,
        }))?
    ));
    payload.validate()?;
    Ok(payload)
}

fn validate_fact_port(payload: &ReviewFactPortedPayload) -> Result<()> {
    if payload.origin_revision.revision_id == payload.target_revision.revision_id {
        return invalid_relation_payload("fact ports require distinct origin and target Revisions");
    }
    if payload
        .rationale_content_hash
        .as_deref()
        .is_some_and(|hash| !is_prefixed_sha256(hash))
    {
        return invalid_relation_payload("fact-port rationale hash is not a prefixed SHA-256");
    }
    let target_required = matches!(
        payload.relation,
        FactPortRelationV1::ReanchoredAs | FactPortRelationV1::CarriedOpenAs
    );
    if target_required != payload.target_fact.is_some() {
        return Err(crate::error::ShoreError::Message(
            "fact-port targetFact must be present exactly for reanchored_as or carried_open_as"
                .to_owned(),
        ));
    }
    if matches!(payload.relation, FactPortRelationV1::CarriedOpenAs)
        && (!matches!(payload.origin_fact, FactRefV1::InputRequest { .. })
            || !matches!(payload.target_fact, Some(FactRefV1::InputRequest { .. })))
    {
        return Err(crate::error::ShoreError::Message(
            "carried_open_as requires an input-request origin".to_owned(),
        ));
    }
    if matches!(payload.relation, FactPortRelationV1::ReanchoredAs)
        && !matches!(
            (&payload.origin_fact, &payload.target_fact),
            (
                FactRefV1::Observation { .. },
                Some(FactRefV1::Observation { .. })
            )
        )
    {
        return invalid_relation_payload(
            "reanchored_as requires observation facts at both endpoints",
        );
    }
    Ok(())
}

fn is_sorted_unique<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn is_prefixed_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn invalid_relation_payload<T>(message: &str) -> Result<T> {
    Err(crate::error::ShoreError::InvalidEvent {
        message: message.to_owned(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AttestationMaterial<'a> {
    schema: &'a str,
    version: u32,
    revision: &'a RevisionRefV1,
    commit_association_id: &'a CommitAssociationId,
    semantic_relation: SemanticRevisionRelationV1,
    proof_status: RelationProofStatusV1,
    proof_method: &'a str,
    proof_algorithm_version: &'a str,
    capture_scope: &'a [String],
    comparison_base_or_parent: &'a Option<String>,
    endpoint_oids: &'a [String],
    evidence_content_hash: &'a Option<String>,
    result_digest: &'a str,
}

fn payload_without_attestation_id(
    payload: &RevisionRelationAttestedPayload,
) -> AttestationMaterial<'_> {
    AttestationMaterial {
        schema: &payload.schema,
        version: payload.version,
        revision: &payload.revision,
        commit_association_id: &payload.commit_association_id,
        semantic_relation: payload.semantic_relation,
        proof_status: payload.proof_status,
        proof_method: &payload.proof_method,
        proof_algorithm_version: &payload.proof_algorithm_version,
        capture_scope: &payload.capture_scope,
        comparison_base_or_parent: &payload.comparison_base_or_parent,
        endpoint_oids: &payload.endpoint_oids,
        evidence_content_hash: &payload.evidence_content_hash,
        result_digest: &payload.result_digest,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PortMaterial<'a> {
    schema: &'a str,
    version: u32,
    origin_revision: &'a RevisionRefV1,
    origin_fact: &'a FactRefV1,
    target_revision: &'a RevisionRefV1,
    relation: FactPortRelationV1,
    target_fact: &'a Option<FactRefV1>,
    rationale_content_hash: &'a Option<String>,
    context_change_id: &'a Option<ChangeId>,
}

fn payload_without_port_id(payload: &ReviewFactPortedPayload) -> PortMaterial<'_> {
    PortMaterial {
        schema: &payload.schema,
        version: payload.version,
        origin_revision: &payload.origin_revision,
        origin_fact: &payload.origin_fact,
        target_revision: &payload.target_revision,
        relation: payload.relation,
        target_fact: &payload.target_fact,
        rationale_content_hash: &payload.rationale_content_hash,
        context_change_id: &payload.context_change_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn revision(name: &str, byte: char) -> RevisionRefV1 {
        RevisionRefV1::new(
            crate::model::RevisionId::new(format!("rev:sha256:{name}")),
            format!("sha256:{}", byte.to_string().repeat(64)),
        )
        .unwrap()
    }

    #[test]
    fn relation_and_fact_port_use_the_frozen_t23_t24_codes() {
        assert_eq!(type_code(EventType::RevisionRelationAttested), "t:23");
        assert_eq!(type_code(EventType::ReviewFactPorted), "t:24");
    }

    #[test]
    fn fact_port_relation_is_closed_and_never_names_validation_or_assessment() {
        for relation in [
            FactPortRelationV1::ContextOnly,
            FactPortRelationV1::ReanchoredAs,
            FactPortRelationV1::CarriedOpenAs,
            FactPortRelationV1::ResolvedBy,
        ] {
            assert!(!serde_json::to_string(&relation).unwrap().is_empty());
        }
        let fact = FactRefV1::InputRequest {
            input_request_id: crate::model::InputRequestId::new("input-request:sha256:one"),
        };
        assert_eq!(serde_json::to_value(fact).unwrap()["kind"], "input_request");
    }

    #[test]
    fn relation_attestation_canonicalizes_scope_and_is_retry_stable() {
        let build = || {
            build_revision_relation_attested(RevisionRelationAttestationDraftV1 {
                revision: revision("a", 'a'),
                commit_association_id: CommitAssociationId::new("assoc-commit:sha256:a"),
                semantic_relation: SemanticRevisionRelationV1::ExactMaterialization,
                proof_status: RelationProofStatusV1::Verified,
                proof_method: "canonical-tree".to_owned(),
                proof_algorithm_version: "1".to_owned(),
                capture_scope: vec![
                    "src/z.rs".to_owned(),
                    "src/a.rs".to_owned(),
                    "src/a.rs".to_owned(),
                ],
                comparison_base_or_parent: Some("parent".to_owned()),
                endpoint_oids: vec!["target".to_owned(), "base".to_owned()],
                evidence_content_hash: Some(format!("sha256:{}", "b".repeat(64))),
                result_digest: format!("sha256:{}", "c".repeat(64)),
            })
            .unwrap()
        };
        let first = build();
        let retry = build();
        assert_eq!(first, retry);
        assert_eq!(first.capture_scope, ["src/a.rs", "src/z.rs"]);
        assert!(
            first
                .relation_attestation_id
                .as_str()
                .starts_with("relation-attestation:sha256:")
        );
    }

    #[test]
    fn fact_port_builder_requires_a_real_target_fact_for_reanchor_or_carry() {
        let result = build_review_fact_ported(
            ReviewFactPortDraftV1 {
                origin_revision: revision("a", 'a'),
                origin_fact: FactRefV1::Observation {
                    observation_id: ObservationId::new("obs:sha256:a"),
                },
                target_revision: revision("b", 'b'),
                relation: FactPortRelationV1::ReanchoredAs,
                target_fact: None,
                rationale_content_hash: None,
                context_change_id: None,
            },
            &ActorId::new("actor:local"),
            &TrackId::new("track:review"),
        );
        assert!(result.is_err());
    }
}
