use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::model::{
    AssessmentId, ChangeId, ChangeMembershipClaimId, InputRequestId, ReviewTargetRef, RevisionId,
    current_revisions, replacement_heads_diverge, revision_graph_has_cycle,
};
use crate::session::event::{
    AssertionMode, ChangeDeclaredPayload, ChangeLinkAssertedPayload,
    ChangeMembershipAssertedPayload, ChangeMembershipWithdrawnPayload,
    ChangeRevisionRelationAssertedPayload, ChangeRevisionRelationWithdrawnPayload, EventType,
    FactPortRelationV1, FactRefV1, InputRequestRespondedPayload, ReviewAssessment,
    ReviewAssessmentRecordedPayload, ReviewFactPortedPayload, ShoreEvent, WorkObjectProposal,
    WorkObjectProposedPayload,
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

#[derive(Default)]
struct FoldInput {
    revisions: BTreeMap<RevisionId, BTreeSet<String>>,
    declarations: BTreeMap<ChangeId, Vec<ChangeDeclaredPayload>>,
    memberships: BTreeMap<ChangeMembershipClaimId, ChangeMembershipAssertedPayload>,
    membership_withdrawals: BTreeSet<ChangeMembershipClaimId>,
    relations: BTreeMap<
        crate::model::ChangeRevisionRelationClaimId,
        ChangeRevisionRelationAssertedPayload,
    >,
    relation_withdrawals: BTreeSet<crate::model::ChangeRevisionRelationClaimId>,
    links: BTreeMap<crate::model::ChangeLinkClaimId, ChangeLinkAssertedPayload>,
    assessments: BTreeMap<RevisionId, Vec<ReviewAssessmentRecordedPayload>>,
    operative_requests: BTreeMap<InputRequestId, RevisionId>,
    request_response_count: BTreeMap<InputRequestId, usize>,
    fact_ports: Vec<ReviewFactPortedPayload>,
}

/// Fold a complete unordered event set into Change state.
///
/// Event order, timestamps, actor locality, and lexical Revision order never
/// choose membership, replacement currency, or a winning current Revision.
pub fn project_changes(events: &[ShoreEvent]) -> Result<ChangeProjection> {
    let mut input = FoldInput::default();
    for event in events {
        match event.event_type {
            EventType::WorkObjectProposed => {
                let payload: WorkObjectProposedPayload =
                    serde_json::from_value(event.payload.clone())?;
                if let WorkObjectProposal::Revision {
                    revision,
                    object_artifact_content_hash,
                    ..
                } = payload.work_object
                {
                    input
                        .revisions
                        .entry(revision.id)
                        .or_default()
                        .insert(object_artifact_content_hash);
                }
            }
            EventType::ChangeDeclared => {
                let payload: ChangeDeclaredPayload = serde_json::from_value(event.payload.clone())?;
                payload.validate()?;
                input
                    .declarations
                    .entry(payload.change_id.clone())
                    .or_default()
                    .push(payload);
            }
            EventType::ChangeMembershipAsserted => {
                let payload: ChangeMembershipAssertedPayload =
                    serde_json::from_value(event.payload.clone())?;
                payload.validate()?;
                input
                    .memberships
                    .insert(payload.membership_claim_id.clone(), payload);
            }
            EventType::ChangeMembershipWithdrawn => {
                let payload: ChangeMembershipWithdrawnPayload =
                    serde_json::from_value(event.payload.clone())?;
                payload.validate()?;
                input
                    .membership_withdrawals
                    .insert(payload.membership_claim_id);
            }
            EventType::ChangeRevisionRelationAsserted => {
                let payload: ChangeRevisionRelationAssertedPayload =
                    serde_json::from_value(event.payload.clone())?;
                payload.validate()?;
                input
                    .relations
                    .insert(payload.relation_claim_id.clone(), payload);
            }
            EventType::ChangeRevisionRelationWithdrawn => {
                let payload: ChangeRevisionRelationWithdrawnPayload =
                    serde_json::from_value(event.payload.clone())?;
                payload.validate()?;
                input.relation_withdrawals.insert(payload.relation_claim_id);
            }
            EventType::ChangeLinkAsserted => {
                let payload: ChangeLinkAssertedPayload =
                    serde_json::from_value(event.payload.clone())?;
                payload.validate()?;
                input.links.insert(payload.link_claim_id.clone(), payload);
            }
            EventType::ReviewAssessmentRecorded => {
                let payload: ReviewAssessmentRecordedPayload =
                    serde_json::from_value(event.payload.clone())?;
                if let ReviewTargetRef::Revision { revision_id } = &payload.target {
                    input
                        .assessments
                        .entry(revision_id.clone())
                        .or_default()
                        .push(payload);
                }
            }
            EventType::InputRequestOpened if event.assertion_mode == AssertionMode::Operative => {
                let payload = crate::session::event::decode_input_request_opened_payload(
                    event.payload.clone(),
                )?;
                if payload.task_target.is_none() {
                    input.operative_requests.insert(
                        payload.input_request_id,
                        review_target_revision(&payload.target),
                    );
                }
            }
            EventType::InputRequestResponded => {
                let payload: InputRequestRespondedPayload =
                    serde_json::from_value(event.payload.clone())?;
                *input
                    .request_response_count
                    .entry(payload.input_request_id)
                    .or_default() += 1;
            }
            EventType::ReviewFactPorted => {
                let payload: ReviewFactPortedPayload =
                    serde_json::from_value(event.payload.clone())?;
                let track_id = event.target.track_id.as_ref().ok_or_else(|| {
                    crate::error::ShoreError::InvalidEvent {
                        message: "review_fact_ported requires an attributed review track"
                            .to_owned(),
                    }
                })?;
                payload.validate_attribution(&event.writer.actor_id, track_id)?;
                input.fact_ports.push(payload);
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
            | EventType::RevisionRelationAttested => {}
            EventType::InputRequestOpened => {}
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

        let lifecycle = if incomplete {
            ChangeLifecycleV1::Incomplete
        } else if cycle || divergent || declaration_conflict || revision_conflict {
            ChangeLifecycleV1::Conflicted
        } else if current_revisions.is_empty()
            || !current_revisions
                .iter()
                .all(|revision| has_one_accepting_assessment(revision, &input.assessments))
            || obligation_status.unresolved
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
                diagnostics: diagnostics.into_iter().collect(),
            },
        );
    }

    projection.links = input
        .links
        .into_values()
        .map(|link| ChangeLinkView {
            left_change_id: link.left_change_id,
            right_change_id: link.right_change_id,
            relation: link.relation,
        })
        .collect();
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
    assessments: &BTreeMap<RevisionId, Vec<ReviewAssessmentRecordedPayload>>,
) -> bool {
    let Some(records) = assessments.get(revision_id) else {
        return false;
    };
    let replaced: BTreeSet<AssessmentId> = records
        .iter()
        .flat_map(|record| record.replaces_assessment_ids.iter().cloned())
        .collect();
    let current: Vec<_> = records
        .iter()
        .filter(|record| !replaced.contains(&record.assessment_id))
        .collect();
    current.len() == 1
        && matches!(
            current[0].assessment,
            ReviewAssessment::Accepted | ReviewAssessment::AcceptedWithFollowUp
        )
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct OperativeObligationStatus {
    unresolved: bool,
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
                status.unresolved = true;
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
        status.unresolved |= carried_to != *current;
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
        InputRequestResponseOutcome, ReviewAssessment, ReviewAssessmentRecordedPayload, Revision,
        ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload, Writer, build_change_declared,
        build_membership_asserted, build_membership_withdrawn, build_revision_relation_asserted,
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
        ShoreEvent::new(
            payload.event_type(),
            key,
            EventTarget::for_journal(JournalId::new("journal:test")),
            Writer::shore_local("test"),
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

    fn assessment(revision_id: RevisionId, nonce: u8) -> ShoreEvent {
        event(
            ReviewAssessmentRecordedPayload {
                assessment_id: crate::model::AssessmentId::new(format!("assess:sha256:{nonce}")),
                target: ReviewTargetRef::Revision { revision_id },
                assessment: ReviewAssessment::Accepted,
                summary: None,
                summary_content_type: Default::default(),
                summary_artifact_path: None,
                summary_byte_size: None,
                summary_content_hash: None,
                replaces_assessment_ids: Vec::new(),
                related_observation_ids: Vec::new(),
                related_input_request_ids: Vec::new(),
            },
            format!("assessment:{nonce}"),
        )
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
        let divergent = project_changes(&events).unwrap();
        assert_eq!(
            divergent.changes[&change_id].topology,
            ChangeTopologyV1::ReplacementDivergent
        );
        assert_eq!(
            divergent.changes[&change_id].lifecycle,
            ChangeLifecycleV1::Conflicted
        );
        events.push(relation(&change_id, d_ref.clone(), b_ref, 22));
        assert_eq!(
            project_changes(&events).unwrap().changes[&change_id].topology,
            ChangeTopologyV1::ReplacementDivergent
        );
        events.push(relation(&change_id, d_ref, c_ref, 23));
        let consolidated = project_changes(&events).unwrap();
        assert_eq!(
            consolidated.changes[&change_id].topology,
            ChangeTopologyV1::Consolidation
        );
        assert_eq!(
            consolidated.changes[&change_id].current_revisions,
            [d].into()
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
        let view = &project_changes(&events).unwrap().changes[&change_id];
        assert_eq!(view.current_revisions, [b].into());
        assert_eq!(view.lifecycle, ChangeLifecycleV1::InProgress);
    }

    #[test]
    fn missing_revision_is_incomplete_and_self_heals_when_exact_backfill_arrives() {
        let (change_id, declared) = declaration_with_nonce(34);
        let (revision_id, _, revision_event) = revision("backfill", 'f');
        let (_, membership) = membership(&change_id, &revision_id, 35);

        let incomplete = project_changes(&[declared.clone(), membership.clone()]).unwrap();
        let view = &incomplete.changes[&change_id];
        assert_eq!(view.topology, ChangeTopologyV1::Incomplete);
        assert_eq!(view.lifecycle, ChangeLifecycleV1::Incomplete);
        assert!(
            view.diagnostics
                .contains(&"change_membership_revision_missing".to_owned())
        );

        let complete = project_changes(&[
            declared,
            membership,
            revision_event,
            assessment(revision_id.clone(), 36),
        ])
        .unwrap();
        let view = &complete.changes[&change_id];
        assert_eq!(view.members, [revision_id].into());
        assert_eq!(view.topology, ChangeTopologyV1::Initial);
        assert_eq!(view.lifecycle, ChangeLifecycleV1::Accepted);
        assert!(view.diagnostics.is_empty());
    }

    #[test]
    fn conflicting_revision_artifact_bindings_are_order_independent_and_conflicted() {
        let (change_id, declared) = declaration_with_nonce(80);
        let (revision_id, _, first) = revision("same", '1');
        let (_, _, second) = revision("same", '2');
        let (_, membership) = membership(&change_id, &revision_id, 81);
        let events = vec![declared, first, second, membership];
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
        assert_eq!(
            project_changes(&events).unwrap().changes[&change_id].lifecycle,
            ChangeLifecycleV1::Accepted
        );

        events.push(response("second", InputRequestResponseOutcome::Rejected));
        let projection = project_changes(&events).unwrap();
        let view = &projection.changes[&change_id];
        assert_eq!(view.lifecycle, ChangeLifecycleV1::InProgress);
        assert!(
            view.diagnostics
                .contains(&"operative_request_response_ambiguous".to_owned())
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
        let projection = project_changes(&[
            first_declared,
            second_declared,
            a_event,
            b_event,
            first_a,
            first_b,
            second_a,
            second_b,
            relation(&first_change, b_ref, a_ref, 43),
        ])
        .unwrap();
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
            [a, b].into()
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
        let projection = project_changes(&[
            declared,
            a_event,
            b_event,
            ma,
            mb,
            event(first, "relation:first"),
            event(withdrawal, "relation:withdrawal"),
            event(second, "relation:second"),
        ])
        .unwrap();
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
        assert_eq!(
            project_changes(&accepted).unwrap().changes[&change_id].lifecycle,
            ChangeLifecycleV1::Accepted
        );

        let mut cyclic = base;
        cyclic.push(relation(&change_id, b_ref.clone(), a_ref.clone(), 65));
        cyclic.push(relation(&change_id, a_ref, b_ref, 66));
        let projection = project_changes(&cyclic).unwrap();
        let view = &projection.changes[&change_id];
        assert_eq!(view.topology, ChangeTopologyV1::CycleConflicted);
        assert_eq!(view.lifecycle, ChangeLifecycleV1::Conflicted);
    }
}
