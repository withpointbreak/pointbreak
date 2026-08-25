use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::model::{
    ActorId, AssessmentId, ChangeId, ChangeIdentityDescriptorV1, ChangeLinkClaimId,
    ChangeMembershipClaimId, ChangeRevisionRelationClaimId, EventId, InputRequestId,
    ReviewTargetRef, RevisionId, RevisionRefV1, TrackId, current_revisions,
    replacement_heads_diverge, revision_graph_has_cycle,
};
use crate::session::event::{
    AssertionMode, ChangeDeclaredPayload, ChangeLinkAssertedPayload,
    ChangeMembershipAssertedPayload, ChangeMembershipWithdrawnPayload,
    ChangeRevisionRelationAssertedPayload, ChangeRevisionRelationWithdrawnPayload, EventType,
    FactPortRelationV1, FactRefV1, InputRequestRespondedPayload, ReviewAssessment,
    ReviewAssessmentRecordedPayload, ReviewFactPortedPayload, ShoreEvent, WorkObjectProposal,
    WorkObjectProposedPayload, decode_input_request_opened_payload,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeLifecycleV1 {
    Incomplete,
    Conflicted,
    InProgress,
    Accepted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeTopologyV1 {
    Initial,
    Replacement,
    ReplacementDivergent,
    Consolidation,
    ParallelCurrent,
    Mixed,
    Incomplete,
    CycleConflicted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeView {
    pub change_id: ChangeId,
    pub members: BTreeSet<RevisionId>,
    pub current_revisions: BTreeSet<RevisionId>,
    /// Effective `(successor, predecessor)` edges for this Change only.
    pub supersedes: BTreeSet<(RevisionId, RevisionId)>,
    pub topology: ChangeTopologyV1,
    pub lifecycle: ChangeLifecycleV1,
    /// Current Revisions with exactly one effective accepting assessment.
    /// Aggregate acceptance additionally requires every current Revision to
    /// appear here and no unresolved operative obligations.
    pub qualified_current_revisions: BTreeSet<RevisionId>,
    pub operative_obligations: BTreeSet<InputRequestId>,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeLinkView {
    pub left_change_id: ChangeId,
    pub right_change_id: ChangeId,
    pub relation: crate::session::event::ChangeLinkRelationV1,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeProjection {
    pub changes: BTreeMap<ChangeId, ChangeView>,
    pub links: Vec<ChangeLinkView>,
}

/// One event that supports or withdraws a Change claim.
///
/// Effective Change state intentionally ignores duplicate carriers, arrival
/// order, and actor locality. Headless readers still need the complete support
/// provenance so they can explain why a claim is active, withdrawn, duplicated,
/// or withdrawn by a different actor without reconstructing that policy.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeClaimSupportV1 {
    pub event_id: EventId,
    pub actor_id: ActorId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<TrackId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeMembershipClaimViewV1 {
    pub claim_id: ChangeMembershipClaimId,
    pub change_id: ChangeId,
    pub revision_id: RevisionId,
    pub supports: Vec<ChangeClaimSupportV1>,
    pub withdrawals: Vec<ChangeClaimSupportV1>,
    pub active: bool,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRelationClaimViewV1 {
    pub claim_id: ChangeRevisionRelationClaimId,
    pub change_id: ChangeId,
    pub successor: RevisionRefV1,
    pub predecessor: RevisionRefV1,
    pub supports: Vec<ChangeClaimSupportV1>,
    pub withdrawals: Vec<ChangeClaimSupportV1>,
    pub active: bool,
    pub diagnostics: Vec<String>,
}

/// Bodyless, deterministic provenance needed by Change documents.
///
/// This projection is derived directly from validated journal events and is
/// safe to persist beside [`ChangeProjection`]. It deliberately excludes prose,
/// commands, paths, signatures, timestamps, and ingest metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeDocumentProjectionV1 {
    pub revision_refs: BTreeMap<RevisionId, Vec<RevisionRefV1>>,
    pub unavailable_revision_refs: BTreeMap<RevisionId, RevisionRefUnavailableReasonV1>,
    pub membership_claims: Vec<ChangeMembershipClaimViewV1>,
    pub relation_claims: Vec<ChangeRelationClaimViewV1>,
    pub diagnostics: Vec<String>,
    pub projection_stamp: String,
}

/// Typed reason an older proposal cannot furnish the strict exact-Revision
/// reference required by Change documents. The semantic projection remains
/// readable; document consumers expose the unavailable member instead of
/// turning one legacy carrier into a store-wide error.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionRefUnavailableReasonV1 {
    InvalidRevisionId,
    InvalidObjectArtifactContentHash,
}

/// Bodyless Change fact plus the exact carrier provenance needed by documents.
///
/// The semantic fact remains the sole input to effective Change policy. The
/// support envelope retains only stable identity needed to explain duplicate
/// and cross-actor claim carriers; it deliberately excludes prose and storage
/// details.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangeDocumentProjectionFact {
    pub(crate) change: ChangeProjectionFact,
    pub(crate) support: ChangeClaimSupportV1,
}

impl ChangeDocumentProjectionFact {
    pub(crate) fn new(
        change: ChangeProjectionFact,
        event_id: EventId,
        actor_id: ActorId,
        track_id: Option<TrackId>,
    ) -> Self {
        Self {
            change,
            support: ChangeClaimSupportV1 {
                event_id,
                actor_id,
                track_id,
            },
        }
    }
}

#[derive(Default)]
struct ClaimProvenanceInput {
    revisions: BTreeMap<RevisionId, BTreeSet<String>>,
    memberships:
        BTreeMap<ChangeMembershipClaimId, (ChangeId, RevisionId, BTreeSet<ChangeClaimSupportV1>)>,
    membership_withdrawals: BTreeMap<ChangeMembershipClaimId, BTreeSet<ChangeClaimSupportV1>>,
    relations: BTreeMap<
        ChangeRevisionRelationClaimId,
        (
            ChangeId,
            RevisionRefV1,
            RevisionRefV1,
            BTreeSet<ChangeClaimSupportV1>,
        ),
    >,
    relation_withdrawals: BTreeMap<ChangeRevisionRelationClaimId, BTreeSet<ChangeClaimSupportV1>>,
}

/// Project complete Change claim provenance without changing effective-state
/// policy or requiring a document consumer to inspect raw events.
pub fn project_change_documents(events: &[ShoreEvent]) -> Result<ChangeDocumentProjectionV1> {
    let mut facts = Vec::new();
    for event in events {
        if let Some(change) = extract_change_projection_fact(event)? {
            facts.push(ChangeDocumentProjectionFact::new(
                change,
                event.event_id.clone(),
                event.writer.actor_id.clone(),
                event.target.track_id.clone(),
            ));
        }
    }
    project_change_documents_from_facts(&facts)
}

/// Project complete Change document provenance from compact persisted facts.
///
/// Effective Change state is always delegated to [`project_changes_from_facts`]
/// so strict replay and derived reads cannot acquire separate lifecycle or
/// topology policy.
pub(crate) fn project_change_documents_from_facts(
    facts: &[ChangeDocumentProjectionFact],
) -> Result<ChangeDocumentProjectionV1> {
    #[cfg(any(test, feature = "longitudinal-counting"))]
    crate::bench_support::longitudinal::record_change_projection_construction();
    let semantic_facts = facts
        .iter()
        .map(|fact| fact.change.clone())
        .collect::<Vec<_>>();
    let semantic = project_changes_from_facts(&semantic_facts)?;
    let mut input = ClaimProvenanceInput::default();
    for fact in facts {
        match &fact.change {
            ChangeProjectionFact::Revision {
                revision_id,
                object_artifact_content_hash,
            } => {
                input
                    .revisions
                    .entry(revision_id.clone())
                    .or_default()
                    .insert(object_artifact_content_hash.clone());
            }
            ChangeProjectionFact::MembershipAsserted {
                claim_id,
                change_id,
                revision_id,
            } => {
                let entry = input
                    .memberships
                    .entry(claim_id.clone())
                    .or_insert_with(|| (change_id.clone(), revision_id.clone(), BTreeSet::new()));
                entry.2.insert(fact.support.clone());
            }
            ChangeProjectionFact::MembershipWithdrawn { claim_id } => {
                input
                    .membership_withdrawals
                    .entry(claim_id.clone())
                    .or_default()
                    .insert(fact.support.clone());
            }
            ChangeProjectionFact::RelationAsserted {
                claim_id,
                change_id,
                successor,
                predecessor,
            } => {
                let entry = input.relations.entry(claim_id.clone()).or_insert_with(|| {
                    (
                        change_id.clone(),
                        successor.clone(),
                        predecessor.clone(),
                        BTreeSet::new(),
                    )
                });
                entry.3.insert(fact.support.clone());
            }
            ChangeProjectionFact::RelationWithdrawn { claim_id } => {
                input
                    .relation_withdrawals
                    .entry(claim_id.clone())
                    .or_default()
                    .insert(fact.support.clone());
            }
            _ => {}
        }
    }

    let mut revision_refs = BTreeMap::new();
    let mut unavailable_revision_refs = BTreeMap::new();
    for (revision_id, hashes) in input.revisions {
        if !revision_id.as_str().starts_with("rev:") {
            unavailable_revision_refs.insert(
                revision_id,
                RevisionRefUnavailableReasonV1::InvalidRevisionId,
            );
            continue;
        }
        let refs = hashes
            .into_iter()
            .map(|hash| RevisionRefV1::new(revision_id.clone(), hash))
            .collect::<Result<Vec<_>>>();
        match refs {
            Ok(refs) => {
                revision_refs.insert(revision_id, refs);
            }
            Err(_) => {
                unavailable_revision_refs.insert(
                    revision_id,
                    RevisionRefUnavailableReasonV1::InvalidObjectArtifactContentHash,
                );
            }
        }
    }

    let mut diagnostics = unavailable_revision_refs
        .iter()
        .map(|(revision_id, reason)| {
            format!(
                "change_revision_ref_unavailable:{}:{}",
                revision_id.as_str(),
                match reason {
                    RevisionRefUnavailableReasonV1::InvalidRevisionId => "invalid_revision_id",
                    RevisionRefUnavailableReasonV1::InvalidObjectArtifactContentHash => {
                        "invalid_object_artifact_content_hash"
                    }
                }
            )
        })
        .collect::<BTreeSet<_>>();
    diagnostics.extend(
        input
            .membership_withdrawals
            .keys()
            .filter(|claim_id| !input.memberships.contains_key(*claim_id))
            .map(|claim_id| {
                format!(
                    "change_membership_withdrawal_claim_missing:{}",
                    claim_id.as_str()
                )
            }),
    );
    diagnostics.extend(
        input
            .relation_withdrawals
            .keys()
            .filter(|claim_id| !input.relations.contains_key(*claim_id))
            .map(|claim_id| {
                format!(
                    "change_relation_withdrawal_claim_missing:{}",
                    claim_id.as_str()
                )
            }),
    );

    let membership_claims = input
        .memberships
        .into_iter()
        .map(|(claim_id, (change_id, revision_id, supports))| {
            let withdrawals = input
                .membership_withdrawals
                .get(&claim_id)
                .cloned()
                .unwrap_or_default();
            ChangeMembershipClaimViewV1 {
                claim_id,
                change_id,
                revision_id,
                diagnostics: claim_diagnostics(&supports, &withdrawals),
                active: withdrawals.is_empty(),
                supports: supports.into_iter().collect(),
                withdrawals: withdrawals.into_iter().collect(),
            }
        })
        .collect::<Vec<_>>();

    let relation_claims = input
        .relations
        .into_iter()
        .map(
            |(claim_id, (change_id, successor, predecessor, supports))| {
                let withdrawals = input
                    .relation_withdrawals
                    .get(&claim_id)
                    .cloned()
                    .unwrap_or_default();
                ChangeRelationClaimViewV1 {
                    claim_id,
                    change_id,
                    successor,
                    predecessor,
                    diagnostics: claim_diagnostics(&supports, &withdrawals),
                    active: withdrawals.is_empty(),
                    supports: supports.into_iter().collect(),
                    withdrawals: withdrawals.into_iter().collect(),
                }
            },
        )
        .collect::<Vec<_>>();

    let mut projection = ChangeDocumentProjectionV1 {
        revision_refs,
        unavailable_revision_refs,
        membership_claims,
        relation_claims,
        diagnostics: diagnostics.into_iter().collect(),
        projection_stamp: String::new(),
    };
    projection.projection_stamp = change_document_projection_stamp(&semantic, &projection)?;
    Ok(projection)
}

pub fn change_document_projection_stamp(
    semantic: &ChangeProjection,
    projection: &ChangeDocumentProjectionV1,
) -> Result<String> {
    crate::canonical_hash::sha256_json_prefixed(&serde_json::json!({
        "semantic": semantic,
        "revisionRefs": projection.revision_refs,
        "unavailableRevisionRefs": projection.unavailable_revision_refs,
        "membershipClaims": projection.membership_claims,
        "relationClaims": projection.relation_claims,
        "diagnostics": projection.diagnostics,
    }))
}

fn claim_diagnostics(
    supports: &BTreeSet<ChangeClaimSupportV1>,
    withdrawals: &BTreeSet<ChangeClaimSupportV1>,
) -> Vec<String> {
    let mut diagnostics = BTreeSet::new();
    if supports.len() > 1 {
        diagnostics.insert("duplicate_claim_support".to_owned());
    }
    if !withdrawals.is_empty()
        && withdrawals.iter().any(|withdrawal| {
            !supports
                .iter()
                .any(|support| support.actor_id == withdrawal.actor_id)
        })
    {
        diagnostics.insert("cross_actor_withdrawal".to_owned());
    }
    diagnostics.into_iter().collect()
}

/// Bodyless semantic input shared by strict replay and disposable projections.
///
/// This is deliberately smaller than a [`ShoreEvent`]. It retains only the
/// values that can affect Change membership, topology, lifecycle, or links;
/// prose bodies, summaries, commands, paths, signatures, and ingest provenance
/// never enter the derived semantic layer. A source event ID is retained only
/// where semantic duplicates need the same deterministic representative rule
/// as the store-wide projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ChangeProjectionFact {
    Revision {
        revision_id: RevisionId,
        object_artifact_content_hash: String,
    },
    Declaration {
        change_id: ChangeId,
        identity_descriptor: ChangeIdentityDescriptorV1,
    },
    MembershipAsserted {
        claim_id: ChangeMembershipClaimId,
        change_id: ChangeId,
        revision_id: RevisionId,
    },
    MembershipWithdrawn {
        claim_id: ChangeMembershipClaimId,
    },
    RelationAsserted {
        claim_id: ChangeRevisionRelationClaimId,
        change_id: ChangeId,
        successor: RevisionRefV1,
        predecessor: RevisionRefV1,
    },
    RelationWithdrawn {
        claim_id: ChangeRevisionRelationClaimId,
    },
    LinkAsserted {
        claim_id: ChangeLinkClaimId,
        left_change_id: ChangeId,
        right_change_id: ChangeId,
        relation: crate::session::event::ChangeLinkRelationV1,
    },
    Assessment {
        source_event_id: EventId,
        revision_id: RevisionId,
        assessment_id: AssessmentId,
        assessment: ReviewAssessment,
        replaces: Vec<AssessmentId>,
    },
    OperativeRequest {
        request_id: InputRequestId,
        revision_id: RevisionId,
    },
    RequestResponse {
        request_id: InputRequestId,
    },
    FactPort {
        port: ReviewFactPortedPayload,
    },
}

/// Extract the one compact semantic fact an event contributes to Change state.
///
/// Payload validation and fact-port attribution happen here so every consumer
/// observes the same fail-closed interpretation. `None` means the event has no
/// Change-level semantic effect; it is not a parse or validation failure.
pub(crate) fn extract_change_projection_fact(
    event: &ShoreEvent,
) -> Result<Option<ChangeProjectionFact>> {
    let fact = match event.event_type {
        EventType::WorkObjectProposed => {
            let payload: WorkObjectProposedPayload = serde_json::from_value(event.payload.clone())?;
            match payload.work_object {
                WorkObjectProposal::Revision {
                    revision,
                    object_artifact_content_hash,
                    ..
                } => Some(ChangeProjectionFact::Revision {
                    // Legacy proposal carriers predate `RevisionRefV1`'s strict
                    // constructor. The pure projection preserves their exact
                    // recorded binding here; new Change claims validate their
                    // exact references in their own payload contract.
                    revision_id: revision.id,
                    object_artifact_content_hash,
                }),
                WorkObjectProposal::TaskAttempt { .. } => None,
            }
        }
        EventType::ChangeDeclared => {
            let payload: ChangeDeclaredPayload = serde_json::from_value(event.payload.clone())?;
            payload.validate()?;
            Some(ChangeProjectionFact::Declaration {
                change_id: payload.change_id,
                identity_descriptor: payload.identity_descriptor,
            })
        }
        EventType::ChangeMembershipAsserted => {
            let payload: ChangeMembershipAssertedPayload =
                serde_json::from_value(event.payload.clone())?;
            payload.validate()?;
            Some(ChangeProjectionFact::MembershipAsserted {
                claim_id: payload.membership_claim_id,
                change_id: payload.change_id,
                revision_id: payload.revision_id,
            })
        }
        EventType::ChangeMembershipWithdrawn => {
            let payload: ChangeMembershipWithdrawnPayload =
                serde_json::from_value(event.payload.clone())?;
            payload.validate()?;
            Some(ChangeProjectionFact::MembershipWithdrawn {
                claim_id: payload.membership_claim_id,
            })
        }
        EventType::ChangeRevisionRelationAsserted => {
            let payload: ChangeRevisionRelationAssertedPayload =
                serde_json::from_value(event.payload.clone())?;
            payload.validate()?;
            Some(ChangeProjectionFact::RelationAsserted {
                claim_id: payload.relation_claim_id,
                change_id: payload.change_id,
                successor: payload.successor,
                predecessor: payload.predecessor,
            })
        }
        EventType::ChangeRevisionRelationWithdrawn => {
            let payload: ChangeRevisionRelationWithdrawnPayload =
                serde_json::from_value(event.payload.clone())?;
            payload.validate()?;
            Some(ChangeProjectionFact::RelationWithdrawn {
                claim_id: payload.relation_claim_id,
            })
        }
        EventType::ChangeLinkAsserted => {
            let payload: ChangeLinkAssertedPayload = serde_json::from_value(event.payload.clone())?;
            payload.validate()?;
            Some(ChangeProjectionFact::LinkAsserted {
                claim_id: payload.link_claim_id,
                left_change_id: payload.left_change_id,
                right_change_id: payload.right_change_id,
                relation: payload.relation,
            })
        }
        EventType::ReviewAssessmentRecorded => {
            let payload: ReviewAssessmentRecordedPayload =
                serde_json::from_value(event.payload.clone())?;
            Some(ChangeProjectionFact::Assessment {
                source_event_id: event.event_id.clone(),
                revision_id: review_target_revision(&payload.target),
                assessment_id: payload.assessment_id,
                assessment: payload.assessment,
                replaces: payload.replaces_assessment_ids,
            })
        }
        EventType::InputRequestOpened if event.assertion_mode == AssertionMode::Operative => {
            let payload = decode_input_request_opened_payload(event.payload.clone())?;
            payload
                .task_target
                .is_none()
                .then(|| ChangeProjectionFact::OperativeRequest {
                    request_id: payload.input_request_id,
                    revision_id: review_target_revision(&payload.target),
                })
        }
        EventType::InputRequestResponded => {
            let payload: InputRequestRespondedPayload =
                serde_json::from_value(event.payload.clone())?;
            Some(ChangeProjectionFact::RequestResponse {
                request_id: payload.input_request_id,
            })
        }
        EventType::ReviewFactPorted => {
            let payload: ReviewFactPortedPayload = serde_json::from_value(event.payload.clone())?;
            let track_id = event.target.track_id.as_ref().ok_or_else(|| {
                crate::error::ShoreError::InvalidEvent {
                    message: "review_fact_ported requires an attributed review track".to_owned(),
                }
            })?;
            payload.validate_attribution(&event.writer.actor_id, track_id)?;
            Some(ChangeProjectionFact::FactPort { port: payload })
        }
        EventType::ReviewInitialized
        | EventType::ReviewNoteImported
        | EventType::ReviewObservationRecorded
        | EventType::ValidationCheckRecorded
        | EventType::RevisionRefAssociated
        | EventType::RevisionRefWithdrawn
        | EventType::RevisionCommitAssociated
        | EventType::RevisionCommitWithdrawn
        | EventType::TaskCheckpointCaptured
        | EventType::TaskObservationRecorded
        | EventType::EventSignatureRecorded
        | EventType::ArtifactRemoved
        | EventType::RevisionRelationAttested
        | EventType::InputRequestOpened => None,
    };
    Ok(fact)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChangeDeclarationFact {
    identity_descriptor: ChangeIdentityDescriptorV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChangeMembershipFact {
    change_id: ChangeId,
    revision_id: RevisionId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChangeRelationFact {
    change_id: ChangeId,
    successor: RevisionRefV1,
    predecessor: RevisionRefV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChangeAssessmentFact {
    assessment_id: AssessmentId,
    assessment: ReviewAssessment,
    replaces: Vec<AssessmentId>,
}

#[derive(Default)]
struct FoldInput {
    revisions: BTreeMap<RevisionId, BTreeSet<String>>,
    declarations: BTreeMap<ChangeId, Vec<ChangeDeclarationFact>>,
    memberships: BTreeMap<ChangeMembershipClaimId, ChangeMembershipFact>,
    membership_withdrawals: BTreeSet<ChangeMembershipClaimId>,
    relations: BTreeMap<ChangeRevisionRelationClaimId, ChangeRelationFact>,
    relation_withdrawals: BTreeSet<ChangeRevisionRelationClaimId>,
    links: BTreeMap<ChangeLinkClaimId, ChangeLinkView>,
    assessments: BTreeMap<RevisionId, BTreeMap<AssessmentId, (EventId, ChangeAssessmentFact)>>,
    operative_requests: BTreeMap<InputRequestId, RevisionId>,
    request_response_count: BTreeMap<InputRequestId, usize>,
    fact_ports: Vec<ReviewFactPortedPayload>,
}

/// Fold a complete unordered event set into Change state.
///
/// Event order, timestamps, actor locality, and lexical Revision order never
/// choose membership, replacement currency, or a winning current Revision.
pub fn project_changes(events: &[ShoreEvent]) -> Result<ChangeProjection> {
    let mut facts = Vec::new();
    for event in events {
        if let Some(fact) = extract_change_projection_fact(event)? {
            facts.push(fact);
        }
    }
    project_changes_from_facts(&facts)
}

pub(crate) fn project_changes_from_facts(
    facts: &[ChangeProjectionFact],
) -> Result<ChangeProjection> {
    #[cfg(any(test, feature = "longitudinal-counting"))]
    crate::bench_support::longitudinal::record_change_semantic_construction();
    let mut input = FoldInput::default();
    for fact in facts {
        match fact {
            ChangeProjectionFact::Revision {
                revision_id,
                object_artifact_content_hash,
            } => {
                input
                    .revisions
                    .entry(revision_id.clone())
                    .or_default()
                    .insert(object_artifact_content_hash.clone());
            }
            ChangeProjectionFact::Declaration {
                change_id,
                identity_descriptor,
            } => input
                .declarations
                .entry(change_id.clone())
                .or_default()
                .push(ChangeDeclarationFact {
                    identity_descriptor: identity_descriptor.clone(),
                }),
            ChangeProjectionFact::MembershipAsserted {
                claim_id,
                change_id,
                revision_id,
            } => {
                input.memberships.insert(
                    claim_id.clone(),
                    ChangeMembershipFact {
                        change_id: change_id.clone(),
                        revision_id: revision_id.clone(),
                    },
                );
            }
            ChangeProjectionFact::MembershipWithdrawn { claim_id } => {
                input.membership_withdrawals.insert(claim_id.clone());
            }
            ChangeProjectionFact::RelationAsserted {
                claim_id,
                change_id,
                successor,
                predecessor,
            } => {
                input.relations.insert(
                    claim_id.clone(),
                    ChangeRelationFact {
                        change_id: change_id.clone(),
                        successor: successor.clone(),
                        predecessor: predecessor.clone(),
                    },
                );
            }
            ChangeProjectionFact::RelationWithdrawn { claim_id } => {
                input.relation_withdrawals.insert(claim_id.clone());
            }
            ChangeProjectionFact::LinkAsserted {
                claim_id,
                left_change_id,
                right_change_id,
                relation,
            } => {
                input.links.insert(
                    claim_id.clone(),
                    ChangeLinkView {
                        left_change_id: left_change_id.clone(),
                        right_change_id: right_change_id.clone(),
                        relation: *relation,
                    },
                );
            }
            ChangeProjectionFact::Assessment {
                source_event_id,
                revision_id,
                assessment_id,
                assessment,
                replaces,
            } => {
                let records = input.assessments.entry(revision_id.clone()).or_default();
                let candidate = ChangeAssessmentFact {
                    assessment_id: assessment_id.clone(),
                    assessment: *assessment,
                    replaces: replaces.clone(),
                };
                match records.entry(assessment_id.clone()) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert((source_event_id.clone(), candidate));
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry)
                        if source_event_id < &entry.get().0 =>
                    {
                        entry.insert((source_event_id.clone(), candidate));
                    }
                    std::collections::btree_map::Entry::Occupied(_) => {}
                }
            }
            ChangeProjectionFact::OperativeRequest {
                request_id,
                revision_id,
            } => {
                input
                    .operative_requests
                    .insert(request_id.clone(), revision_id.clone());
            }
            ChangeProjectionFact::RequestResponse { request_id } => {
                *input
                    .request_response_count
                    .entry(request_id.clone())
                    .or_default() += 1;
            }
            ChangeProjectionFact::FactPort { port } => input.fact_ports.push(port.clone()),
        }
    }

    let mut change_ids: BTreeSet<ChangeId> = input.declarations.keys().cloned().collect();
    change_ids.extend(
        input
            .memberships
            .values()
            .map(|claim| claim.change_id.clone()),
    );
    change_ids.extend(
        input
            .relations
            .values()
            .map(|claim| claim.change_id.clone()),
    );

    let mut projection = ChangeProjection::default();
    for change_id in change_ids {
        let mut diagnostics = BTreeSet::new();
        let mut incomplete = false;
        let mut declaration_conflict = false;
        let mut revision_conflict = false;
        match input.declarations.get(&change_id) {
            None => {
                incomplete = true;
                diagnostics.insert("change_declaration_missing".to_owned());
            }
            Some(declarations) => {
                for declaration in declarations {
                    if crate::model::derive_change_id(&declaration.identity_descriptor)?
                        != change_id
                    {
                        declaration_conflict = true;
                        diagnostics.insert("change_declaration_identity_mismatch".to_owned());
                    }
                }
                let distinct: BTreeSet<String> = declarations
                    .iter()
                    .map(|item| serde_json::to_string(&item.identity_descriptor))
                    .collect::<std::result::Result<_, _>>()?;
                if distinct.len() > 1 {
                    declaration_conflict = true;
                    diagnostics.insert("change_declaration_conflict".to_owned());
                }
            }
        }

        let members: BTreeSet<RevisionId> = input
            .memberships
            .iter()
            .filter(|(claim_id, claim)| {
                claim.change_id == change_id
                    && !input.membership_withdrawals.contains(*claim_id)
                    && input.revisions.contains_key(&claim.revision_id)
            })
            .map(|(_, claim)| claim.revision_id.clone())
            .collect();
        if input.memberships.iter().any(|(claim_id, claim)| {
            claim.change_id == change_id
                && !input.membership_withdrawals.contains(claim_id)
                && !input.revisions.contains_key(&claim.revision_id)
        }) {
            incomplete = true;
            diagnostics.insert("change_membership_revision_missing".to_owned());
        }
        if members.is_empty() {
            incomplete = true;
            diagnostics.insert("change_membership_empty".to_owned());
        }
        if members.iter().any(|revision_id| {
            input
                .revisions
                .get(revision_id)
                .is_some_and(|hashes| hashes.len() > 1)
        }) {
            revision_conflict = true;
            diagnostics.insert("change_member_revision_artifact_conflict".to_owned());
        }

        let mut supersedes = BTreeSet::new();
        for (claim_id, claim) in &input.relations {
            if claim.change_id != change_id || input.relation_withdrawals.contains(claim_id) {
                continue;
            }
            let successor = &claim.successor.revision_id;
            let predecessor = &claim.predecessor.revision_id;
            let successor_hashes = input.revisions.get(successor);
            let predecessor_hashes = input.revisions.get(predecessor);
            let endpoint_conflict = successor_hashes.is_some_and(|hashes| hashes.len() > 1)
                || predecessor_hashes.is_some_and(|hashes| hashes.len() > 1);
            let exact = successor_hashes.is_some_and(|hashes| {
                hashes.len() == 1 && hashes.contains(&claim.successor.object_artifact_content_hash)
            }) && predecessor_hashes.is_some_and(|hashes| {
                hashes.len() == 1
                    && hashes.contains(&claim.predecessor.object_artifact_content_hash)
            });
            if endpoint_conflict {
                revision_conflict = true;
                diagnostics.insert("change_relation_revision_artifact_conflict".to_owned());
            } else if !exact {
                incomplete = true;
                diagnostics.insert("change_relation_target_missing_or_mismatched".to_owned());
            } else if !members.contains(successor) || !members.contains(predecessor) {
                incomplete = true;
                diagnostics.insert("change_relation_membership_incomplete".to_owned());
            } else {
                supersedes.insert((successor.clone(), predecessor.clone()));
            }
        }

        let cycle = revision_graph_has_cycle(&members, &supersedes);
        let current_revisions = current_revisions(&members, &supersedes);
        let divergent = !cycle && replacement_heads_diverge(&current_revisions, &supersedes);
        let obligation_status = operative_obligation_status(&members, &current_revisions, &input);
        if obligation_status.ambiguous_response {
            diagnostics.insert("operative_request_response_ambiguous".to_owned());
        }
        let topology = if incomplete {
            ChangeTopologyV1::Incomplete
        } else if cycle {
            ChangeTopologyV1::CycleConflicted
        } else if divergent {
            ChangeTopologyV1::ReplacementDivergent
        } else if supersedes.is_empty() {
            if current_revisions.len() <= 1 {
                ChangeTopologyV1::Initial
            } else {
                ChangeTopologyV1::ParallelCurrent
            }
        } else if current_revisions.len() == 1 {
            let current = current_revisions
                .iter()
                .next()
                .expect("one current Revision");
            if supersedes
                .iter()
                .filter(|(successor, _)| successor == current)
                .count()
                > 1
            {
                ChangeTopologyV1::Consolidation
            } else {
                ChangeTopologyV1::Replacement
            }
        } else {
            ChangeTopologyV1::Mixed
        };

        let qualified_current_revisions = current_revisions
            .iter()
            .filter(|revision| has_one_accepting_assessment(revision, &input.assessments))
            .cloned()
            .collect::<BTreeSet<_>>();
        let lifecycle = if incomplete {
            ChangeLifecycleV1::Incomplete
        } else if cycle || divergent || declaration_conflict || revision_conflict {
            ChangeLifecycleV1::Conflicted
        } else if current_revisions.is_empty()
            || qualified_current_revisions.len() != current_revisions.len()
            || !obligation_status.unresolved_requests.is_empty()
        {
            ChangeLifecycleV1::InProgress
        } else {
            ChangeLifecycleV1::Accepted
        };

        projection.changes.insert(
            change_id.clone(),
            ChangeView {
                change_id,
                members,
                current_revisions,
                supersedes,
                topology,
                lifecycle,
                qualified_current_revisions,
                operative_obligations: obligation_status.unresolved_requests,
                diagnostics: diagnostics.into_iter().collect(),
            },
        );
    }

    projection.links = input.links.into_values().collect();
    projection.links.sort_by(|left, right| {
        (&left.left_change_id, &left.right_change_id)
            .cmp(&(&right.left_change_id, &right.right_change_id))
    });
    Ok(projection)
}

fn review_target_revision(target: &ReviewTargetRef) -> RevisionId {
    match target {
        ReviewTargetRef::Revision { revision_id }
        | ReviewTargetRef::File { revision_id, .. }
        | ReviewTargetRef::Range { revision_id, .. }
        | ReviewTargetRef::Observation { revision_id, .. }
        | ReviewTargetRef::InputRequest { revision_id, .. }
        | ReviewTargetRef::Assessment { revision_id, .. }
        | ReviewTargetRef::Event { revision_id, .. } => revision_id.clone(),
    }
}

fn has_one_accepting_assessment(
    revision_id: &RevisionId,
    assessments: &BTreeMap<RevisionId, BTreeMap<AssessmentId, (EventId, ChangeAssessmentFact)>>,
) -> bool {
    let Some(records) = assessments.get(revision_id) else {
        return false;
    };
    let replaced: BTreeSet<AssessmentId> = records
        .values()
        .map(|(_, record)| record)
        .flat_map(|record| record.replaces.iter().cloned())
        .collect();
    let current: Vec<_> = records
        .values()
        .map(|(_, record)| record)
        .filter(|record| !replaced.contains(&record.assessment_id))
        .collect();
    current.len() == 1
        && matches!(
            current[0].assessment,
            ReviewAssessment::Accepted | ReviewAssessment::AcceptedWithFollowUp
        )
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct OperativeObligationStatus {
    unresolved_requests: BTreeSet<InputRequestId>,
    ambiguous_response: bool,
}

fn operative_obligation_status(
    members: &BTreeSet<RevisionId>,
    current: &BTreeSet<RevisionId>,
    input: &FoldInput,
) -> OperativeObligationStatus {
    let mut status = OperativeObligationStatus::default();
    for (request_id, origin) in &input.operative_requests {
        if !members.contains(origin) {
            continue;
        }
        match input.request_response_count.get(request_id).copied() {
            Some(1) => continue,
            Some(_) => {
                status.unresolved_requests.insert(request_id.clone());
                status.ambiguous_response = true;
                continue;
            }
            None => {}
        }
        let carried_to: BTreeSet<RevisionId> = input
            .fact_ports
            .iter()
            .filter_map(
                |port| match (&port.origin_fact, port.relation, &port.target_fact) {
                    (
                        FactRefV1::InputRequest {
                            input_request_id: origin_request,
                        },
                        FactPortRelationV1::CarriedOpenAs,
                        Some(FactRefV1::InputRequest {
                            input_request_id: target_request,
                        }),
                    ) if origin_request == request_id
                        && input.operative_requests.get(target_request)
                            == Some(&port.target_revision.revision_id) =>
                    {
                        Some(port.target_revision.revision_id.clone())
                    }
                    _ => None,
                },
            )
            .collect();
        if carried_to != *current {
            status.unresolved_requests.insert(request_id.clone());
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ChangeIdentityDescriptorV1, EngagementId, InputRequestId, InputRequestResponseId,
        JournalId, ObjectId, ReviewTargetRef, RevisionId, RevisionRefV1,
    };
    use crate::session::event::{
        AssertionMode, ChangeRevisionRelationV1, EventPayload, EventTarget,
        InputRequestOpenedPayload, InputRequestReasonCode, InputRequestRespondedPayload,
        InputRequestResponseOutcome, ReviewAssessment, ReviewAssessmentRecordedPayload,
        ReviewFactPortDraftV1, Revision, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload,
        Writer, build_change_declared, build_change_link_asserted, build_membership_asserted,
        build_membership_withdrawn, build_review_fact_ported, build_revision_relation_asserted,
        build_revision_relation_withdrawn,
    };

    fn revision(name: &str, byte: char) -> (RevisionId, RevisionRefV1, ShoreEvent) {
        let revision_id = RevisionId::new(format!("rev:sha256:{name}"));
        let hash = format!("sha256:{}", byte.to_string().repeat(64));
        let reference = RevisionRefV1::new(revision_id.clone(), hash.clone()).unwrap();
        let payload = WorkObjectProposedPayload {
            engagement_id: EngagementId::new("engagement:sha256:test"),
            work_object: WorkObjectProposal::Revision {
                revision: Revision {
                    id: revision_id.clone(),
                    object_id: ObjectId::new(format!("obj:sha256:{name}")),
                    git_provenance: None,
                },
                summary: None,
                object_artifact_content_hash: hash,
                supersedes: Vec::new(),
            },
        };
        let event = event(payload, format!("revision:{name}"));
        (revision_id, reference, event)
    }

    fn event<P: EventPayload>(payload: P, key: impl Into<String>) -> ShoreEvent {
        event_with_actor(payload, key, crate::model::ActorId::new("actor:local"))
    }

    fn event_with_actor<P: EventPayload>(
        payload: P,
        key: impl Into<String>,
        actor_id: crate::model::ActorId,
    ) -> ShoreEvent {
        let mut writer = Writer::shore_local("test");
        writer.actor_id = actor_id;
        ShoreEvent::new(
            payload.event_type(),
            key,
            EventTarget::for_journal(JournalId::new("journal:test")),
            writer,
            payload,
            "2026-08-04T00:00:00Z",
        )
        .unwrap()
    }

    fn declaration() -> (crate::model::ChangeId, ShoreEvent) {
        declaration_with_nonce(0x31)
    }

    fn declaration_with_nonce(nonce: u8) -> (crate::model::ChangeId, ShoreEvent) {
        let payload = build_change_declared(
            ChangeIdentityDescriptorV1::opaque_nonce([nonce; 32]),
            [nonce.wrapping_add(1); 32],
        )
        .unwrap();
        let id = payload.change_id.clone();
        (id, event(payload, format!("change:declared:{nonce}")))
    }

    fn membership(
        change_id: &crate::model::ChangeId,
        revision_id: &RevisionId,
        nonce: u8,
    ) -> (crate::model::ChangeMembershipClaimId, ShoreEvent) {
        let payload = build_membership_asserted(change_id, revision_id, [nonce; 32]).unwrap();
        let id = payload.membership_claim_id.clone();
        (id, event(payload, format!("membership:{nonce}")))
    }

    fn relation(
        change_id: &crate::model::ChangeId,
        successor: RevisionRefV1,
        predecessor: RevisionRefV1,
        nonce: u8,
    ) -> ShoreEvent {
        let payload =
            build_revision_relation_asserted(change_id, successor, predecessor, [nonce; 32])
                .unwrap();
        assert_eq!(payload.relation, ChangeRevisionRelationV1::Supersedes);
        event(payload, format!("relation:{nonce}"))
    }

    fn presented_revision_ids(
        events: &[ShoreEvent],
        change_id: &crate::model::ChangeId,
    ) -> BTreeSet<RevisionId> {
        let semantic = project_changes(events).unwrap();
        let documents = project_change_documents(events).unwrap();
        crate::documents::change_presentation_projection(
            &semantic,
            &documents,
            events,
            "sha256:test-event-set",
        )
        .unwrap()
        .presentations[change_id]
            .current_revisions
            .iter()
            .map(|current| current.revision.revision_id.clone())
            .collect()
    }

    fn assessment(revision_id: RevisionId, nonce: u8) -> ShoreEvent {
        assessment_for_target(
            ReviewTargetRef::Revision { revision_id },
            AssessmentId::new(format!("assess:sha256:{nonce}")),
            ReviewAssessment::Accepted,
            format!("assessment:{nonce}"),
        )
    }

    fn assessment_for_target(
        target: ReviewTargetRef,
        assessment_id: AssessmentId,
        assessment: ReviewAssessment,
        key: impl Into<String>,
    ) -> ShoreEvent {
        event(
            ReviewAssessmentRecordedPayload {
                assessment_id,
                target,
                assessment,
                summary: None,
                summary_content_type: Default::default(),
                summary_artifact_path: None,
                summary_byte_size: None,
                summary_content_hash: None,
                replaces_assessment_ids: Vec::new(),
                related_observation_ids: Vec::new(),
                related_input_request_ids: Vec::new(),
            },
            key,
        )
    }

    fn compact_document_facts(events: &[ShoreEvent]) -> Result<Vec<ChangeDocumentProjectionFact>> {
        let mut facts = Vec::new();
        for event in events {
            if let Some(change) = extract_change_projection_fact(event)? {
                facts.push(ChangeDocumentProjectionFact::new(
                    change,
                    event.event_id.clone(),
                    event.writer.actor_id.clone(),
                    event.target.track_id.clone(),
                ));
            }
        }
        Ok(facts)
    }

    fn assert_compact_document_parity(events: &[ShoreEvent]) {
        let strict_semantic = project_changes(events).expect("strict Change projection");
        let strict = project_change_documents(events).expect("strict document projection");
        let facts = compact_document_facts(events).expect("compact document facts");
        let compact_semantic = project_changes_from_facts(
            &facts
                .iter()
                .map(|fact| fact.change.clone())
                .collect::<Vec<_>>(),
        )
        .expect("compact Change projection");
        let compact =
            project_change_documents_from_facts(&facts).expect("compact document projection");
        assert_eq!(compact_semantic, strict_semantic);
        assert_eq!(compact, strict);
        assert_eq!(compact.projection_stamp, strict.projection_stamp);

        let strict_facade =
            crate::documents::ChangeDocumentFacadeV1::new(strict_semantic.clone(), strict)
                .expect("strict Change facade");
        let compact_facade =
            crate::documents::ChangeDocumentFacadeV1::new(compact_semantic, compact)
                .expect("compact Change facade");
        assert_eq!(
            compact_facade.list_document(),
            strict_facade.list_document()
        );
        assert_eq!(
            compact_facade.attention_document(false),
            strict_facade.attention_document(false)
        );
        assert_eq!(
            compact_facade.attention_document(true),
            strict_facade.attention_document(true)
        );
        for change_id in strict_semantic.changes.keys() {
            assert_eq!(
                compact_facade.detail_document(change_id).unwrap(),
                strict_facade.detail_document(change_id).unwrap()
            );
        }
    }

    #[test]
    fn unordered_claim_union_and_exact_withdrawal_preserve_independent_support() {
        let (change_id, declared) = declaration();
        let (revision_id, _, revision) = revision("a", 'a');
        let (first_id, first) = membership(&change_id, &revision_id, 1);
        let (_, second) = membership(&change_id, &revision_id, 2);
        let withdrawn = event(
            build_membership_withdrawn(&first_id, [3; 32]).unwrap(),
            "membership:withdrawn",
        );
        let events = vec![withdrawn, second, revision, first, declared];
        let expected = project_changes(&events).unwrap();
        let mut reversed = events.clone();
        reversed.reverse();
        assert_eq!(project_changes(&reversed).unwrap(), expected);
        assert_eq!(expected.changes[&change_id].members, [revision_id].into());
    }

    #[test]
    fn document_projection_retains_duplicate_and_cross_actor_claim_provenance() {
        let (change_id, declared) = declaration_with_nonce(41);
        let (revision_id, revision_ref, revision_event) = revision("provenance-a", 'a');
        let (_, membership_event) = membership(&change_id, &revision_id, 42);
        let membership_payload: ChangeMembershipAssertedPayload =
            serde_json::from_value(membership_event.payload.clone()).unwrap();
        let duplicate_membership = event(membership_payload.clone(), "membership:duplicate");
        let membership_withdrawal = event_with_actor(
            build_membership_withdrawn(&membership_payload.membership_claim_id, [43; 32]).unwrap(),
            "membership:cross-actor-withdrawal",
            crate::model::ActorId::new("actor:reviewer"),
        );

        let relation_event = relation(&change_id, revision_ref.clone(), revision_ref, 44);
        let relation_payload: ChangeRevisionRelationAssertedPayload =
            serde_json::from_value(relation_event.payload.clone()).unwrap();
        let duplicate_relation = event(relation_payload.clone(), "relation:duplicate");
        let relation_withdrawal = event_with_actor(
            build_revision_relation_withdrawn(&relation_payload.relation_claim_id, [45; 32])
                .unwrap(),
            "relation:cross-actor-withdrawal",
            crate::model::ActorId::new("actor:reviewer"),
        );

        let events = vec![
            declared,
            revision_event,
            membership_event,
            duplicate_membership,
            membership_withdrawal,
            relation_event,
            duplicate_relation,
            relation_withdrawal,
        ];
        let projection = project_change_documents(&events).unwrap();
        assert_eq!(projection.membership_claims[0].supports.len(), 2);
        assert_eq!(projection.membership_claims[0].withdrawals.len(), 1);
        assert!(!projection.membership_claims[0].active);
        assert_eq!(
            projection.membership_claims[0].diagnostics,
            ["cross_actor_withdrawal", "duplicate_claim_support"]
        );
        assert_eq!(projection.relation_claims[0].supports.len(), 2);
        assert_eq!(
            projection.relation_claims[0].diagnostics,
            ["cross_actor_withdrawal", "duplicate_claim_support"]
        );

        let mut reversed = events;
        reversed.reverse();
        assert_eq!(project_change_documents(&reversed).unwrap(), projection);
    }

    #[test]
    fn legacy_revision_bindings_become_typed_unavailable_refs_without_aborting_projection() {
        let revision_id = RevisionId::new("review-unit:sha256:legacy");
        let payload = WorkObjectProposedPayload {
            engagement_id: EngagementId::new("engagement:sha256:test"),
            work_object: WorkObjectProposal::Revision {
                revision: Revision {
                    id: revision_id.clone(),
                    object_id: ObjectId::new("obj:sha256:legacy"),
                    git_provenance: None,
                },
                summary: None,
                object_artifact_content_hash: "legacy-artifact-hash".to_owned(),
                supersedes: Vec::new(),
            },
        };

        let projection = project_change_documents(&[event(payload, "revision:legacy")]).unwrap();

        assert!(projection.revision_refs.is_empty());
        assert_eq!(
            projection.unavailable_revision_refs[&revision_id],
            RevisionRefUnavailableReasonV1::InvalidRevisionId
        );
        assert!(projection.diagnostics.iter().any(|diagnostic| {
            diagnostic.contains("change_revision_ref_unavailable:review-unit:sha256:legacy")
        }));
    }

    #[test]
    fn document_projection_diagnoses_orphan_claim_withdrawals() {
        let membership_id = crate::model::ChangeMembershipClaimId::new("membership:sha256:missing");
        let relation_id =
            crate::model::ChangeRevisionRelationClaimId::new("change-relation:sha256:missing");
        let events = vec![
            event(
                build_membership_withdrawn(&membership_id, [46; 32]).unwrap(),
                "membership:orphan-withdrawal",
            ),
            event(
                build_revision_relation_withdrawn(&relation_id, [47; 32]).unwrap(),
                "relation:orphan-withdrawal",
            ),
        ];
        let projection = project_change_documents(&events).unwrap();
        assert_eq!(projection.membership_claims.len(), 0);
        assert_eq!(projection.relation_claims.len(), 0);
        assert!(
            projection
                .diagnostics
                .iter()
                .any(|code| code.starts_with("change_membership_withdrawal_claim_missing:"))
        );
        assert!(
            projection
                .diagnostics
                .iter()
                .any(|code| code.starts_with("change_relation_withdrawal_claim_missing:"))
        );
    }

    #[test]
    fn replacement_divergence_survives_descendants_and_consolidation_resolves_it() {
        let (change_id, declared) = declaration();
        let (a, a_ref, a_event) = revision("a", 'a');
        let (b, b_ref, b_event) = revision("b", 'b');
        let (c, c_ref, c_event) = revision("c", 'c');
        let (d, d_ref, d_event) = revision("d", 'd');
        let (_, ma) = membership(&change_id, &a, 10);
        let (_, mb) = membership(&change_id, &b, 11);
        let (_, mc) = membership(&change_id, &c, 12);
        let (_, md) = membership(&change_id, &d, 13);
        let mut events = vec![
            declared,
            a_event,
            b_event,
            c_event,
            d_event,
            ma,
            mb,
            mc,
            md,
            relation(&change_id, b_ref.clone(), a_ref.clone(), 20),
            relation(&change_id, c_ref.clone(), a_ref, 21),
        ];
        assert_compact_document_parity(&events);
        let divergent = project_changes(&events).unwrap();
        assert_eq!(
            divergent.changes[&change_id].topology,
            ChangeTopologyV1::ReplacementDivergent
        );
        assert_eq!(
            divergent.changes[&change_id].lifecycle,
            ChangeLifecycleV1::Conflicted
        );
        assert_eq!(
            presented_revision_ids(&events, &change_id),
            divergent.changes[&change_id].current_revisions
        );
        events.push(relation(&change_id, d_ref.clone(), b_ref, 22));
        assert_compact_document_parity(&events);
        assert_eq!(
            project_changes(&events).unwrap().changes[&change_id].topology,
            ChangeTopologyV1::ReplacementDivergent
        );
        events.push(relation(&change_id, d_ref, c_ref, 23));
        assert_compact_document_parity(&events);
        let consolidated = project_changes(&events).unwrap();
        assert_eq!(
            consolidated.changes[&change_id].topology,
            ChangeTopologyV1::Consolidation
        );
        assert_eq!(
            consolidated.changes[&change_id].current_revisions,
            [d].into()
        );
        assert_eq!(
            presented_revision_ids(&events, &change_id),
            consolidated.changes[&change_id].current_revisions
        );
    }

    #[test]
    fn assessment_never_transfers_to_a_replacement_revision() {
        let (change_id, declared) = declaration();
        let (a, a_ref, a_event) = revision("a", 'a');
        let (b, b_ref, b_event) = revision("b", 'b');
        let (_, ma) = membership(&change_id, &a, 30);
        let (_, mb) = membership(&change_id, &b, 31);
        let events = vec![
            declared,
            a_event,
            b_event,
            ma,
            mb,
            relation(&change_id, b_ref, a_ref, 32),
            assessment(a, 33),
        ];
        assert_compact_document_parity(&events);
        let view = &project_changes(&events).unwrap().changes[&change_id];
        assert_eq!(view.current_revisions, [b].into());
        assert!(view.qualified_current_revisions.is_empty());
        assert_eq!(view.lifecycle, ChangeLifecycleV1::InProgress);
    }

    #[test]
    fn compact_change_facts_are_the_single_projection_input() {
        let (change_id, declared) = declaration_with_nonce(90);
        let (first, first_ref, first_event) = revision("compact-first", '3');
        let (second, second_ref, second_event) = revision("compact-second", '4');
        let (_, first_membership) = membership(&change_id, &first, 91);
        let (_, second_membership) = membership(&change_id, &second, 92);
        let events = vec![
            second_event,
            relation(&change_id, second_ref, first_ref, 93),
            first_membership,
            declared,
            second_membership,
            first_event,
            assessment(second, 94),
        ];
        let facts = events
            .iter()
            .map(extract_change_projection_fact)
            .collect::<Result<Vec<_>>>()
            .expect("compact facts")
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        assert_compact_document_parity(&events);
        assert_eq!(
            project_changes_from_facts(&facts).unwrap(),
            project_changes(&events).unwrap()
        );
    }

    #[test]
    fn compact_change_document_projection_matches_strict_matrix_and_permutations() {
        let (change_id, declared) = declaration_with_nonce(100);
        let (first, first_ref, first_event) = revision("document-parity-first", '7');
        let (second, second_ref, second_event) = revision("document-parity-second", '8');
        let (_, first_membership) = membership(&change_id, &first, 101);
        let (_, second_membership) = membership(&change_id, &second, 102);

        let initial = vec![
            declared.clone(),
            first_event.clone(),
            first_membership.clone(),
            assessment(first.clone(), 103),
        ];
        let parallel = vec![
            declared.clone(),
            first_event.clone(),
            second_event.clone(),
            first_membership.clone(),
            second_membership.clone(),
            assessment(first.clone(), 104),
            assessment(second.clone(), 105),
        ];
        let mut replacement = parallel.clone();
        replacement.push(relation(
            &change_id,
            second_ref.clone(),
            first_ref.clone(),
            106,
        ));

        let (third, _, third_event) = revision("document-parity-third", '9');
        let (_, third_membership) = membership(&change_id, &third, 107);
        let mut mixed = parallel.clone();
        mixed.extend([
            third_event,
            third_membership,
            assessment(third, 108),
            relation(&change_id, second_ref.clone(), first_ref.clone(), 109),
        ]);

        let mismatched_first =
            RevisionRefV1::new(first, format!("sha256:{}", "f".repeat(64))).unwrap();
        let mut missing_exact_relation = parallel.clone();
        missing_exact_relation.push(relation(&change_id, second_ref, mismatched_first, 110));

        for events in [
            initial,
            parallel,
            replacement,
            mixed,
            missing_exact_relation,
        ] {
            assert_compact_document_parity(&events);

            let mut reversed = events.clone();
            reversed.reverse();
            assert_compact_document_parity(&reversed);
            assert_eq!(
                project_change_documents(&reversed).unwrap(),
                project_change_documents(&events).unwrap(),
            );

            let mut rotated = events.clone();
            rotated.rotate_left(2);
            assert_compact_document_parity(&rotated);
            assert_eq!(
                project_change_documents(&rotated).unwrap(),
                project_change_documents(&events).unwrap(),
            );
        }
    }

    #[test]
    fn compact_document_provenance_preserves_duplicates_cross_actor_and_orphans() {
        let (change_id, declared) = declaration_with_nonce(110);
        let (revision_id, revision_ref, revision_event) = revision("compact-provenance", '9');
        let (_, membership_event) = membership(&change_id, &revision_id, 111);
        let membership_payload: ChangeMembershipAssertedPayload =
            serde_json::from_value(membership_event.payload.clone()).unwrap();
        let duplicate_membership =
            event(membership_payload.clone(), "compact-membership:duplicate");
        let membership_withdrawal = event_with_actor(
            build_membership_withdrawn(&membership_payload.membership_claim_id, [112; 32]).unwrap(),
            "compact-membership:cross-actor-withdrawal",
            crate::model::ActorId::new("actor:reviewer"),
        );

        let relation_event = relation(&change_id, revision_ref.clone(), revision_ref, 113);
        let relation_payload: ChangeRevisionRelationAssertedPayload =
            serde_json::from_value(relation_event.payload.clone()).unwrap();
        let duplicate_relation = event(relation_payload.clone(), "compact-relation:duplicate");
        let relation_withdrawal = event_with_actor(
            build_revision_relation_withdrawn(&relation_payload.relation_claim_id, [114; 32])
                .unwrap(),
            "compact-relation:cross-actor-withdrawal",
            crate::model::ActorId::new("actor:reviewer"),
        );
        let orphan_membership = event(
            build_membership_withdrawn(
                &crate::model::ChangeMembershipClaimId::new("membership:sha256:orphan"),
                [115; 32],
            )
            .unwrap(),
            "compact-membership:orphan-withdrawal",
        );
        let orphan_relation = event(
            build_revision_relation_withdrawn(
                &crate::model::ChangeRevisionRelationClaimId::new("change-relation:sha256:orphan"),
                [116; 32],
            )
            .unwrap(),
            "compact-relation:orphan-withdrawal",
        );

        let events = vec![
            declared,
            revision_event,
            membership_event,
            duplicate_membership,
            membership_withdrawal,
            relation_event,
            duplicate_relation,
            relation_withdrawal,
            orphan_membership,
            orphan_relation,
        ];
        assert_compact_document_parity(&events);

        let compact =
            project_change_documents_from_facts(&compact_document_facts(&events).unwrap()).unwrap();
        assert_eq!(compact.membership_claims[0].supports.len(), 2);
        assert_eq!(compact.membership_claims[0].withdrawals.len(), 1);
        assert_eq!(
            compact.membership_claims[0].diagnostics,
            ["cross_actor_withdrawal", "duplicate_claim_support"]
        );
        assert_eq!(compact.relation_claims[0].supports.len(), 2);
        assert_eq!(compact.relation_claims[0].withdrawals.len(), 1);
        assert_eq!(
            compact.relation_claims[0].diagnostics,
            ["cross_actor_withdrawal", "duplicate_claim_support"]
        );
        assert!(compact.diagnostics.iter().any(|diagnostic| {
            diagnostic.starts_with("change_membership_withdrawal_claim_missing:")
        }));
        assert!(compact.diagnostics.iter().any(|diagnostic| {
            diagnostic.starts_with("change_relation_withdrawal_claim_missing:")
        }));

        let mut reversed = events;
        reversed.reverse();
        assert_compact_document_parity(&reversed);
        assert_eq!(
            project_change_documents_from_facts(&compact_document_facts(&reversed).unwrap())
                .unwrap(),
            compact,
        );
    }

    #[test]
    fn compact_document_revision_refs_preserve_equal_conflicting_and_legacy_bindings() {
        let (revision_id, _, first) = revision("compact-document-conflict", '1');
        let (_, _, conflicting) = revision("compact-document-conflict", '2');
        let first_payload: WorkObjectProposedPayload =
            serde_json::from_value(first.payload.clone()).unwrap();
        let equal_duplicate = event(first_payload, "compact-revision:equal-duplicate");

        let legacy_revision_id = RevisionId::new("review-unit:sha256:compact-legacy");
        let legacy = event(
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new("engagement:sha256:test"),
                work_object: WorkObjectProposal::Revision {
                    revision: Revision {
                        id: legacy_revision_id.clone(),
                        object_id: ObjectId::new("obj:sha256:compact-legacy"),
                        git_provenance: None,
                    },
                    summary: None,
                    object_artifact_content_hash: "legacy-artifact-hash".to_owned(),
                    supersedes: Vec::new(),
                },
            },
            "compact-revision:legacy",
        );
        let events = vec![first, equal_duplicate, conflicting, legacy];

        assert_compact_document_parity(&events);
        let compact =
            project_change_documents_from_facts(&compact_document_facts(&events).unwrap()).unwrap();
        assert_eq!(compact.revision_refs[&revision_id].len(), 2);
        assert_eq!(
            compact.unavailable_revision_refs[&legacy_revision_id],
            RevisionRefUnavailableReasonV1::InvalidRevisionId,
        );

        let mut reversed = events;
        reversed.reverse();
        assert_compact_document_parity(&reversed);
        assert_eq!(
            project_change_documents_from_facts(&compact_document_facts(&reversed).unwrap())
                .unwrap(),
            compact,
        );
    }

    #[test]
    fn duplicate_assessment_carriers_fold_once_by_semantic_id() {
        let (change_id, declared) = declaration_with_nonce(95);
        let (revision_id, _, revision_event) = revision("duplicate-assessment", '5');
        let (_, membership) = membership(&change_id, &revision_id, 96);
        let assessment_id = AssessmentId::new("assess:sha256:duplicate");
        let first = assessment_for_target(
            ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            },
            assessment_id.clone(),
            ReviewAssessment::Accepted,
            "assessment:duplicate:first",
        );
        let second = assessment_for_target(
            ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            },
            assessment_id,
            ReviewAssessment::Accepted,
            "assessment:duplicate:second",
        );
        let events = vec![declared, revision_event, membership, first, second];
        assert_compact_document_parity(&events);
        let projection = project_changes(&events).unwrap();
        let mut reversed = events;
        reversed.reverse();
        assert_eq!(project_changes(&reversed).unwrap(), projection);
        assert_eq!(
            projection.changes[&change_id].lifecycle,
            ChangeLifecycleV1::Accepted
        );
        assert_eq!(
            projection.changes[&change_id].qualified_current_revisions,
            [revision_id].into()
        );
    }

    #[test]
    fn file_targeted_assessment_contributes_to_revision_lifecycle() {
        let (change_id, declared) = declaration_with_nonce(97);
        let (revision_id, _, revision_event) = revision("file-assessment", '6');
        let (_, membership) = membership(&change_id, &revision_id, 98);
        let assessment = assessment_for_target(
            ReviewTargetRef::File {
                revision_id,
                file_path: "src/lib.rs".to_owned(),
            },
            AssessmentId::new("assess:sha256:file-target"),
            ReviewAssessment::Accepted,
            "assessment:file-target",
        );
        let events = [declared, revision_event, membership, assessment];
        assert_compact_document_parity(&events);
        let projection = project_changes(&events).unwrap();
        assert_eq!(
            projection.changes[&change_id].lifecycle,
            ChangeLifecycleV1::Accepted
        );
    }

    #[test]
    fn missing_revision_is_incomplete_and_self_heals_when_exact_backfill_arrives() {
        let (change_id, declared) = declaration_with_nonce(34);
        let (revision_id, _, revision_event) = revision("backfill", 'f');
        let (_, membership) = membership(&change_id, &revision_id, 35);

        let incomplete_events = [declared.clone(), membership.clone()];
        assert_compact_document_parity(&incomplete_events);
        let incomplete = project_changes(&incomplete_events).unwrap();
        let view = &incomplete.changes[&change_id];
        assert_eq!(view.topology, ChangeTopologyV1::Incomplete);
        assert_eq!(view.lifecycle, ChangeLifecycleV1::Incomplete);
        assert!(
            view.diagnostics
                .contains(&"change_membership_revision_missing".to_owned())
        );
        assert!(
            presented_revision_ids(&[declared.clone(), membership.clone()], &change_id).is_empty(),
            "presentation must not fabricate an exact identity for a missing Revision"
        );

        let complete_events = [
            declared,
            membership,
            revision_event,
            assessment(revision_id.clone(), 36),
        ];
        assert_compact_document_parity(&complete_events);
        let complete = project_changes(&complete_events).unwrap();
        let view = &complete.changes[&change_id];
        assert_eq!(view.members, [revision_id.clone()].into());
        assert_eq!(view.topology, ChangeTopologyV1::Initial);
        assert_eq!(view.lifecycle, ChangeLifecycleV1::Accepted);
        assert!(view.diagnostics.is_empty());
        assert_eq!(
            presented_revision_ids(&complete_events, &change_id),
            [revision_id].into()
        );
    }

    #[test]
    fn conflicting_revision_artifact_bindings_are_order_independent_and_conflicted() {
        let (change_id, declared) = declaration_with_nonce(80);
        let (revision_id, _, first) = revision("same", '1');
        let (_, _, second) = revision("same", '2');
        let (_, membership) = membership(&change_id, &revision_id, 81);
        let events = vec![declared, first, second, membership];
        assert_compact_document_parity(&events);
        let projected = project_changes(&events).unwrap();
        let mut reversed = events;
        reversed.reverse();
        assert_eq!(project_changes(&reversed).unwrap(), projected);
        let view = &projected.changes[&change_id];
        assert_eq!(view.lifecycle, ChangeLifecycleV1::Conflicted);
        assert!(
            view.diagnostics
                .contains(&"change_member_revision_artifact_conflict".to_owned())
        );
    }

    #[test]
    fn exactly_one_response_discharges_while_multiple_responses_are_ambiguous() {
        let (change_id, declared) = declaration_with_nonce(37);
        let (revision_id, _, revision_event) = revision("responses", 'e');
        let (_, membership) = membership(&change_id, &revision_id, 38);
        let request_id = InputRequestId::new("input-request:sha256:multiple-responses");
        let opened = event(
            InputRequestOpenedPayload {
                input_request_id: request_id.clone(),
                target: ReviewTargetRef::Revision {
                    revision_id: revision_id.clone(),
                },
                task_target: None,
                reason_code: InputRequestReasonCode::ManualDecisionRequired,
                title: "choose".to_owned(),
                body: None,
                body_content_type: Default::default(),
                body_artifact_path: None,
                body_byte_size: None,
                body_content_hash: None,
                target_fingerprint: None,
            },
            "request:open",
        )
        .with_assertion_mode(AssertionMode::Operative);
        let response = |suffix: &str, outcome| {
            event(
                InputRequestRespondedPayload {
                    input_request_response_id: InputRequestResponseId::new(format!(
                        "input-request-response:sha256:{suffix}"
                    )),
                    input_request_id: request_id.clone(),
                    revision_id: Some(revision_id.clone()),
                    task_target: None,
                    outcome,
                    reason: None,
                    reason_content_type: Default::default(),
                    reason_artifact_path: None,
                    reason_byte_size: None,
                    reason_content_hash: None,
                    target_fingerprint: None,
                },
                format!("request:response:{suffix}"),
            )
        };
        let mut events = vec![
            declared,
            revision_event,
            membership,
            assessment(revision_id.clone(), 39),
            opened,
            response("first", InputRequestResponseOutcome::Approved),
        ];
        assert_compact_document_parity(&events);
        assert_eq!(
            project_changes(&events).unwrap().changes[&change_id].lifecycle,
            ChangeLifecycleV1::Accepted
        );

        events.push(response("second", InputRequestResponseOutcome::Rejected));
        assert_compact_document_parity(&events);
        let projection = project_changes(&events).unwrap();
        let view = &projection.changes[&change_id];
        assert_eq!(view.lifecycle, ChangeLifecycleV1::InProgress);
        assert!(
            view.diagnostics
                .contains(&"operative_request_response_ambiguous".to_owned())
        );
    }

    #[test]
    fn compact_documents_preserve_links_and_fact_port_policy_without_prose() {
        const TITLE_SENTINEL: &str = "PRIVATE OPERATIVE REQUEST TITLE COMPACT READER";

        let (change_id, declared) = declaration_with_nonce(120);
        let (linked_change_id, linked_declared) = declaration_with_nonce(121);
        let (first, first_ref, first_event) = revision("fact-port-first", 'a');
        let (second, second_ref, second_event) = revision("fact-port-second", 'b');
        let (_, first_membership) = membership(&change_id, &first, 122);
        let (_, second_membership) = membership(&change_id, &second, 123);
        let origin_request = InputRequestId::new("input-request:sha256:fact-port-origin");
        let target_request = InputRequestId::new("input-request:sha256:fact-port-target");
        let request = |request_id: InputRequestId, revision_id: RevisionId, key: &str| {
            event(
                InputRequestOpenedPayload {
                    input_request_id: request_id,
                    target: ReviewTargetRef::Revision { revision_id },
                    task_target: None,
                    reason_code: InputRequestReasonCode::ManualDecisionRequired,
                    title: TITLE_SENTINEL.to_owned(),
                    body: None,
                    body_content_type: Default::default(),
                    body_artifact_path: None,
                    body_byte_size: None,
                    body_content_hash: None,
                    target_fingerprint: None,
                },
                key,
            )
            .with_assertion_mode(AssertionMode::Operative)
        };
        let writer = Writer::shore_local("test");
        let track_id = crate::model::TrackId::new("track:fact-port");
        let fact_port = build_review_fact_ported(
            ReviewFactPortDraftV1 {
                origin_revision: first_ref.clone(),
                origin_fact: FactRefV1::InputRequest {
                    input_request_id: origin_request.clone(),
                },
                target_revision: second_ref.clone(),
                relation: FactPortRelationV1::CarriedOpenAs,
                target_fact: Some(FactRefV1::InputRequest {
                    input_request_id: target_request.clone(),
                }),
                rationale_content_hash: None,
                context_change_id: Some(change_id.clone()),
            },
            &writer.actor_id,
            &track_id,
        )
        .unwrap();
        let fact_port = ShoreEvent::new(
            EventType::ReviewFactPorted,
            "fact-port:carried-open",
            EventTarget::for_revision(
                JournalId::new("journal:test"),
                first_ref.revision_id.clone(),
                Some(track_id),
            )
            .unwrap(),
            writer,
            fact_port,
            "2026-08-04T00:00:00Z",
        )
        .unwrap();
        let link = event(
            build_change_link_asserted(
                &change_id,
                &linked_change_id,
                crate::session::event::ChangeLinkRelationV1::RelatedWork,
                [124; 32],
            )
            .unwrap(),
            "change-link:related",
        );
        let events = vec![
            declared,
            linked_declared,
            first_event,
            second_event,
            first_membership,
            second_membership,
            relation(&change_id, second_ref, first_ref, 125),
            request(origin_request, first, "request:origin"),
            request(target_request, second, "request:target"),
            fact_port,
            link,
        ];

        assert_compact_document_parity(&events);
        let semantic = project_changes(&events).unwrap();
        assert_eq!(semantic.links.len(), 1);
        let encoded = serde_json::to_string(&compact_document_facts(&events).unwrap()).unwrap();
        assert!(
            !encoded.contains(TITLE_SENTINEL),
            "compact Change facts retained request prose"
        );
    }

    #[test]
    fn one_pair_can_replace_in_one_change_and_remain_parallel_in_another() {
        let (first_change, first_declared) = declaration_with_nonce(40);
        let (second_change, second_declared) = declaration_with_nonce(50);
        let (a, a_ref, a_event) = revision("a", 'a');
        let (b, b_ref, b_event) = revision("b", 'b');
        let (_, first_a) = membership(&first_change, &a, 41);
        let (_, first_b) = membership(&first_change, &b, 42);
        let (_, second_a) = membership(&second_change, &a, 51);
        let (_, second_b) = membership(&second_change, &b, 52);
        let events = vec![
            first_declared,
            second_declared,
            a_event,
            b_event,
            first_a,
            first_b,
            second_a,
            second_b,
            relation(&first_change, b_ref.clone(), a_ref.clone(), 43),
        ];
        assert_compact_document_parity(&events);
        let projection = project_changes(&events).unwrap();
        assert_eq!(
            projection.changes[&first_change].topology,
            ChangeTopologyV1::Replacement
        );
        assert_eq!(
            projection.changes[&second_change].topology,
            ChangeTopologyV1::ParallelCurrent
        );
        assert_eq!(
            projection.changes[&second_change].current_revisions,
            [a.clone(), b.clone()].into()
        );
        assert_eq!(
            presented_revision_ids(&events, &first_change),
            [b.clone()].into()
        );
        assert_eq!(
            presented_revision_ids(&events, &second_change),
            [a.clone(), b].into()
        );

        let documents = project_change_documents(&events).unwrap();
        let facade = crate::documents::ChangeDocumentFacadeV1::new(projection, documents).unwrap();
        let resource = crate::documents::RevisionResourceDocumentV1::unavailable(
            crate::documents::RevisionResourceRefV1 {
                revision: a_ref.clone(),
                object_id: ObjectId::new("obj:sha256:a"),
            },
            crate::documents::RevisionResourceProjectionV1 {
                track_id: None,
                include_body: false,
            },
            crate::session::ContentAvailabilityV1::Missing,
        )
        .unwrap();
        let fact = crate::documents::FactPresentationV1 {
            fact_id: "observation:sha256:shared".to_owned(),
            family: "observation".to_owned(),
            origin_revision: a_ref.clone(),
            target: None,
            context_change_id: None,
            presented_in_revision: None,
            port_relation: None,
            actor_id: crate::model::ActorId::new("actor:test"),
            track_id: None,
            family_state: crate::documents::FactFamilyStateV1::Current,
            revision_currency: crate::documents::ChangeRevisionCurrencyV1::Current,
            availability: crate::session::ContentAvailabilityV1::Available,
        };
        let replacement = facade
            .contextual_revision_document(
                &first_change,
                &a_ref,
                resource.clone(),
                vec![fact.clone()],
                Vec::new(),
            )
            .unwrap();
        let parallel = facade
            .contextual_revision_document(&second_change, &a_ref, resource, vec![fact], Vec::new())
            .unwrap();
        assert_eq!(
            replacement.detail.revision_currency,
            crate::documents::ChangeRevisionCurrencyV1::StaleBySupersession
        );
        assert_eq!(
            replacement.detail.fact_presentations[0].revision_currency,
            crate::documents::ChangeRevisionCurrencyV1::StaleBySupersession
        );
        assert_eq!(
            parallel.detail.revision_currency,
            crate::documents::ChangeRevisionCurrencyV1::Current
        );
        assert_eq!(
            parallel.detail.fact_presentations[0].revision_currency,
            crate::documents::ChangeRevisionCurrencyV1::Current
        );
    }

    #[test]
    fn exact_relation_withdrawal_preserves_independent_support_for_the_same_edge() {
        let (change_id, declared) = declaration_with_nonce(70);
        let (a, a_ref, a_event) = revision("a", 'a');
        let (b, b_ref, b_event) = revision("b", 'b');
        let (_, ma) = membership(&change_id, &a, 71);
        let (_, mb) = membership(&change_id, &b, 72);
        let first =
            build_revision_relation_asserted(&change_id, b_ref.clone(), a_ref.clone(), [73; 32])
                .unwrap();
        let withdrawal =
            build_revision_relation_withdrawn(&first.relation_claim_id, [74; 32]).unwrap();
        let second = build_revision_relation_asserted(&change_id, b_ref, a_ref, [75; 32]).unwrap();
        let events = vec![
            declared,
            a_event,
            b_event,
            ma,
            mb,
            event(first, "relation:first"),
            event(withdrawal, "relation:withdrawal"),
            event(second, "relation:second"),
        ];
        assert_compact_document_parity(&events);
        let projection = project_changes(&events).unwrap();
        assert_eq!(
            projection.changes[&change_id].topology,
            ChangeTopologyV1::Replacement
        );
    }

    #[test]
    fn cycles_conflict_while_complete_parallel_members_can_be_accepted() {
        let (change_id, declared) = declaration_with_nonce(60);
        let (a, a_ref, a_event) = revision("a", 'a');
        let (b, b_ref, b_event) = revision("b", 'b');
        let (_, ma) = membership(&change_id, &a, 61);
        let (_, mb) = membership(&change_id, &b, 62);
        let base = vec![declared, a_event, b_event, ma, mb];
        let mut accepted = base.clone();
        accepted.push(assessment(a.clone(), 63));
        accepted.push(assessment(b.clone(), 64));
        assert_compact_document_parity(&accepted);
        assert_eq!(
            project_changes(&accepted).unwrap().changes[&change_id].lifecycle,
            ChangeLifecycleV1::Accepted
        );

        let mut cyclic = base;
        cyclic.push(relation(&change_id, b_ref.clone(), a_ref.clone(), 65));
        cyclic.push(relation(&change_id, a_ref, b_ref, 66));
        assert_compact_document_parity(&cyclic);
        let projection = project_changes(&cyclic).unwrap();
        let view = &projection.changes[&change_id];
        assert_eq!(view.topology, ChangeTopologyV1::CycleConflicted);
        assert_eq!(view.lifecycle, ChangeLifecycleV1::Conflicted);
    }
}
