use std::collections::{BTreeMap, BTreeSet};

use serde::de::DeserializeOwned;

use crate::canonical_hash::sha256_json_prefixed;
use crate::crypto::EventVerificationStatus;
use crate::documents::{
    EventHistoryCompletionV1, EventHistoryDocumentV1, EventHistoryEntryV1, EventHistoryFacadeV1,
    EventHistoryOrderV1, EventHistoryRelationTargetV1, EventHistorySubjectV1,
    EventHistorySummaryV1, INSPECT_EVENT_HISTORY_SCHEMA,
};
use crate::error::Result;
use crate::model::{
    ChangeId, ChangeIdentityDescriptorV1, ChangeMembershipClaimId, ChangeRevisionRelationClaimId,
    EventId, ReviewTargetRef, RevisionId, RevisionRefV1, ValidationTarget,
};
use crate::session::event::{
    ChangeDeclaredPayload, ChangeLinkAssertedPayload, ChangeMembershipAssertedPayload,
    ChangeMembershipWithdrawnPayload, ChangeRevisionRelationAssertedPayload,
    ChangeRevisionRelationWithdrawnPayload, EventType, InputRequestRespondedPayload,
    ReviewAssessmentRecordedPayload, ReviewFactPortedPayload, ReviewObservationRecordedPayload,
    RevisionCommitAssociatedPayload, RevisionCommitWithdrawnPayload, RevisionRefAssociatedPayload,
    RevisionRefWithdrawnPayload, RevisionRelationAttestedPayload, ShoreEvent,
    ValidationCheckRecordedPayload, WorkObjectProposal, WorkObjectProposedPayload,
    decode_input_request_opened_payload,
};
use crate::session::{
    AuthorityCursorV2, ChangeDocumentProjectionV1, TrustSet, compare_event_instants,
    verify_event_signature,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EventHistoryTypeClassV1 {
    ReviewDomain,
    SubjectDependent,
    TaskDomain,
    Carrier,
}

/// Rebind a complete strict Timeline to the Change generation identity
/// selected by the Inspector, without changing its authoritative entries.
#[doc(hidden)]
pub fn rebind_event_history_source_projection_stamp(
    mut document: EventHistoryDocumentV1,
    source_change_projection_stamp: String,
    trust_set: &TrustSet,
) -> Result<EventHistoryDocumentV1> {
    document.source_change_projection_stamp = source_change_projection_stamp;
    document.timeline_projection_stamp = timeline_projection_stamp_v1(
        &document.source_change_projection_stamp,
        &trust_set.identity_sha256()?,
    )?;
    Ok(document)
}

/// Mint the page-independent Timeline identity used by both strict and
/// derived adapters. The source Change stamp already binds the exact live
/// reader checkpoint; this layer adds the request-local TrustSet identity and
/// the frozen normalized chronology contract without hashing page entries.
pub(crate) fn timeline_projection_stamp_v1(
    source_change_projection_stamp: &str,
    trust_set_sha256: &str,
) -> Result<String> {
    sha256_json_prefixed(&serde_json::json!({
        "schema": "pointbreak.inspect-event-history-generation.v1",
        "sourceChangeProjectionStamp": source_change_projection_stamp,
        "runtimeTrustSetSha256": trust_set_sha256,
        "order": "normalized_occurred_at_event_id_asc.v1",
    }))
}

pub(crate) fn event_history_type_class(event_type: EventType) -> EventHistoryTypeClassV1 {
    match event_type {
        EventType::ReviewInitialized
        | EventType::ReviewObservationRecorded
        | EventType::ReviewAssessmentRecorded
        | EventType::ReviewNoteImported
        | EventType::RevisionRefAssociated
        | EventType::RevisionRefWithdrawn
        | EventType::RevisionCommitAssociated
        | EventType::RevisionCommitWithdrawn
        | EventType::ValidationCheckRecorded
        | EventType::ChangeDeclared
        | EventType::ChangeMembershipAsserted
        | EventType::ChangeMembershipWithdrawn
        | EventType::ChangeLinkAsserted
        | EventType::ChangeRevisionRelationAsserted
        | EventType::ChangeRevisionRelationWithdrawn
        | EventType::RevisionRelationAttested
        | EventType::ReviewFactPorted => EventHistoryTypeClassV1::ReviewDomain,
        EventType::WorkObjectProposed
        | EventType::InputRequestOpened
        | EventType::InputRequestResponded => EventHistoryTypeClassV1::SubjectDependent,
        EventType::TaskCheckpointCaptured | EventType::TaskObservationRecorded => {
            EventHistoryTypeClassV1::TaskDomain
        }
        EventType::EventSignatureRecorded | EventType::ArtifactRemoved => {
            EventHistoryTypeClassV1::Carrier
        }
    }
}

#[derive(Default)]
struct HistoricalCorrelationV1 {
    change_ids_by_revision: BTreeMap<RevisionId, BTreeSet<ChangeId>>,
    membership_claims: BTreeMap<ChangeMembershipClaimId, (ChangeId, RevisionId)>,
    relation_claims:
        BTreeMap<ChangeRevisionRelationClaimId, (ChangeId, RevisionRefV1, RevisionRefV1)>,
}

impl HistoricalCorrelationV1 {
    fn from_projection(projection: &ChangeDocumentProjectionV1) -> Self {
        let mut history = Self::default();
        for claim in &projection.membership_claims {
            history
                .change_ids_by_revision
                .entry(claim.revision_id.clone())
                .or_default()
                .insert(claim.change_id.clone());
            history.membership_claims.insert(
                claim.claim_id.clone(),
                (claim.change_id.clone(), claim.revision_id.clone()),
            );
        }
        for claim in &projection.relation_claims {
            history
                .change_ids_by_revision
                .entry(claim.successor.revision_id.clone())
                .or_default()
                .insert(claim.change_id.clone());
            history
                .change_ids_by_revision
                .entry(claim.predecessor.revision_id.clone())
                .or_default()
                .insert(claim.change_id.clone());
            history.relation_claims.insert(
                claim.claim_id.clone(),
                (
                    claim.change_id.clone(),
                    claim.successor.clone(),
                    claim.predecessor.clone(),
                ),
            );
        }
        history
    }
}

pub(crate) fn project_event_history(
    events: &[ShoreEvent],
    change_projection: &ChangeDocumentProjectionV1,
    authority_cursor: AuthorityCursorV2,
    source_change_projection_stamp: String,
    trust_set: &TrustSet,
) -> Result<EventHistoryFacadeV1> {
    let (drafts, diagnostics) =
        project_selected_event_history_without_trust(events, change_projection)?;
    let mut entries = bind_selected_event_history_trust(drafts, trust_set)?;
    entries.sort_by(|left, right| {
        compare_event_instants(&left.occurred_at, &right.occurred_at)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    let relation_records = strict_relation_records(events, &entries)?;
    attach_event_history_relation_targets(&mut entries, |family, fact_id| {
        Ok::<_, std::convert::Infallible>(
            relation_records.get(&(family, fact_id.to_owned())).cloned(),
        )
    })
    .unwrap_or_else(|never| match never {});

    let mut facets = BTreeMap::new();
    let mut completion = EventHistoryCompletionV1::default();
    for entry in &entries {
        *facets
            .entry(entry.event_type.as_str().to_owned())
            .or_insert(0) += 1;
        if !completion.event_types.contains(&entry.event_type) {
            completion.event_types.push(entry.event_type);
        }
        if let Some(track_id) = &entry.track_id {
            push_sorted_unique(&mut completion.track_ids, track_id.clone());
        }
        for change_id in &entry.change_ids {
            push_sorted_unique(&mut completion.change_ids, change_id.clone());
        }
        for revision in &entry.revision_refs {
            push_sorted_unique(&mut completion.revision_refs, revision.clone());
        }
        for revision_id in &entry.unresolved_revision_ids {
            push_sorted_unique(&mut completion.unresolved_revision_ids, revision_id.clone());
        }
    }
    let document = rebind_event_history_source_projection_stamp(
        EventHistoryDocumentV1 {
            schema: INSPECT_EVENT_HISTORY_SCHEMA.to_owned(),
            version: 1,
            event_count: authority_cursor.event_count,
            authority_cursor,
            source_change_projection_stamp: String::new(),
            timeline_projection_stamp: String::new(),
            order: EventHistoryOrderV1::Asc,
            match_count: entries.len(),
            offset: 0,
            match_index: None,
            facets,
            completion,
            diagnostics,
            query_notices: Vec::new(),
            entries,
            previous: None,
            next: None,
        },
        source_change_projection_stamp,
        trust_set,
    )?;
    Ok(EventHistoryFacadeV1::new(document))
}

#[derive(Clone, Debug)]
pub(crate) struct EventHistoryEntryDraftV1 {
    event: ShoreEvent,
    entry: EventHistoryEntryV1,
}

impl EventHistoryEntryDraftV1 {
    pub(crate) fn event(&self) -> &ShoreEvent {
        &self.event
    }

    pub(crate) fn entry(&self) -> &EventHistoryEntryV1 {
        &self.entry
    }
}

/// Decode and correlate selected Timeline events without consulting runtime
/// trust. Callers validate every selected/support carrier and normalized
/// relation against these drafts before invoking
/// [`bind_selected_event_history_trust`].
pub(crate) fn project_selected_event_history_without_trust(
    events: &[ShoreEvent],
    change_projection: &ChangeDocumentProjectionV1,
) -> Result<(Vec<EventHistoryEntryDraftV1>, Vec<String>)> {
    let history = HistoricalCorrelationV1::from_projection(change_projection);
    let mut diagnostics = BTreeSet::new();
    let drafts = events
        .iter()
        .filter_map(|event| {
            project_event_without_trust(event, change_projection, &history, &mut diagnostics)
                .transpose()
                .map(|result| {
                    result.map(|entry| EventHistoryEntryDraftV1 {
                        event: event.clone(),
                        entry,
                    })
                })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((drafts, diagnostics.into_iter().collect()))
}

/// Apply runtime trust only after the caller's carrier/relation validation is
/// complete. This is the sole selected-entry trust boundary.
pub(crate) fn bind_selected_event_history_trust(
    drafts: Vec<EventHistoryEntryDraftV1>,
    trust_set: &TrustSet,
) -> Result<Vec<EventHistoryEntryV1>> {
    drafts
        .into_iter()
        .map(|mut draft| {
            draft.entry.verification_status = verify_event_signature(&draft.event, trust_set)?;
            Ok(draft.entry)
        })
        .collect()
}

fn project_event_without_trust(
    event: &ShoreEvent,
    change_projection: &ChangeDocumentProjectionV1,
    history: &HistoricalCorrelationV1,
    diagnostics: &mut BTreeSet<String>,
) -> Result<Option<EventHistoryEntryV1>> {
    if matches!(
        event_history_type_class(event.event_type),
        EventHistoryTypeClassV1::TaskDomain | EventHistoryTypeClassV1::Carrier
    ) {
        return Ok(None);
    }
    let mut change_ids = BTreeSet::new();
    let mut revision_refs = BTreeSet::new();
    let mut unresolved_revision_ids = BTreeSet::new();

    let (subject, summary) = match event.event_type {
        EventType::ReviewInitialized => (
            EventHistorySubjectV1::Journal {
                journal_id: event.target.journal_id.clone(),
            },
            EventHistorySummaryV1::ReviewInitialized,
        ),
        EventType::WorkObjectProposed => {
            let payload: WorkObjectProposedPayload = decode(event)?;
            let WorkObjectProposal::Revision {
                revision,
                summary,
                object_artifact_content_hash,
                supersedes,
            } = payload.work_object
            else {
                return Ok(None);
            };
            let target = ReviewTargetRef::Revision {
                revision_id: revision.id.clone(),
            };
            add_direct_revision(
                revision.id.clone(),
                object_artifact_content_hash.clone(),
                &mut revision_refs,
                &mut unresolved_revision_ids,
            );
            add_historical_changes(&revision.id, history, &mut change_ids);
            for predecessor in &supersedes {
                add_revision_id(
                    predecessor,
                    change_projection,
                    &mut revision_refs,
                    &mut unresolved_revision_ids,
                );
                add_historical_changes(predecessor, history, &mut change_ids);
            }
            (
                EventHistorySubjectV1::Review { target },
                EventHistorySummaryV1::WorkObjectProposed {
                    engagement_id: payload.engagement_id,
                    revision,
                    summary,
                    object_artifact_content_hash,
                    supersedes,
                },
            )
        }
        EventType::ReviewObservationRecorded => {
            let payload: ReviewObservationRecordedPayload = decode(event)?;
            correlate_target(
                &payload.target,
                change_projection,
                history,
                &mut change_ids,
                &mut revision_refs,
                &mut unresolved_revision_ids,
            );
            (
                EventHistorySubjectV1::Review {
                    target: payload.target.clone(),
                },
                EventHistorySummaryV1::ReviewObservationRecorded(payload),
            )
        }
        EventType::ReviewAssessmentRecorded => {
            let payload: ReviewAssessmentRecordedPayload = decode(event)?;
            correlate_target(
                &payload.target,
                change_projection,
                history,
                &mut change_ids,
                &mut revision_refs,
                &mut unresolved_revision_ids,
            );
            (
                EventHistorySubjectV1::Review {
                    target: payload.target.clone(),
                },
                EventHistorySummaryV1::ReviewAssessmentRecorded(payload),
            )
        }
        EventType::InputRequestOpened => {
            let payload = decode_input_request_opened_payload(event.payload.clone())?;
            if payload.task_target.is_some() {
                return Ok(None);
            }
            correlate_target(
                &payload.target,
                change_projection,
                history,
                &mut change_ids,
                &mut revision_refs,
                &mut unresolved_revision_ids,
            );
            (
                EventHistorySubjectV1::Review {
                    target: payload.target.clone(),
                },
                EventHistorySummaryV1::InputRequestOpened(payload),
            )
        }
        EventType::InputRequestResponded => {
            let payload: InputRequestRespondedPayload = decode(event)?;
            let Some(revision_id) = payload.revision_id.clone() else {
                return Ok(None);
            };
            let target = ReviewTargetRef::InputRequest {
                revision_id,
                input_request_id: payload.input_request_id.clone(),
            };
            correlate_target(
                &target,
                change_projection,
                history,
                &mut change_ids,
                &mut revision_refs,
                &mut unresolved_revision_ids,
            );
            (
                EventHistorySubjectV1::Review { target },
                EventHistorySummaryV1::InputRequestResponded(payload),
            )
        }
        EventType::ReviewNoteImported => (
            EventHistorySubjectV1::Journal {
                journal_id: event.target.journal_id.clone(),
            },
            EventHistorySummaryV1::ReviewNoteImported,
        ),
        EventType::RevisionRefAssociated => {
            let payload: RevisionRefAssociatedPayload = decode(event)?;
            review_target_payload(
                payload.target.clone(),
                EventHistorySummaryV1::RevisionRefAssociated(payload),
                change_projection,
                history,
                &mut change_ids,
                &mut revision_refs,
                &mut unresolved_revision_ids,
            )
        }
        EventType::RevisionRefWithdrawn => {
            let payload: RevisionRefWithdrawnPayload = decode(event)?;
            review_target_payload(
                payload.target.clone(),
                EventHistorySummaryV1::RevisionRefWithdrawn(payload),
                change_projection,
                history,
                &mut change_ids,
                &mut revision_refs,
                &mut unresolved_revision_ids,
            )
        }
        EventType::RevisionCommitAssociated => {
            let payload: RevisionCommitAssociatedPayload = decode(event)?;
            review_target_payload(
                payload.target.clone(),
                EventHistorySummaryV1::RevisionCommitAssociated(payload),
                change_projection,
                history,
                &mut change_ids,
                &mut revision_refs,
                &mut unresolved_revision_ids,
            )
        }
        EventType::RevisionCommitWithdrawn => {
            let payload: RevisionCommitWithdrawnPayload = decode(event)?;
            review_target_payload(
                payload.target.clone(),
                EventHistorySummaryV1::RevisionCommitWithdrawn(payload),
                change_projection,
                history,
                &mut change_ids,
                &mut revision_refs,
                &mut unresolved_revision_ids,
            )
        }
        EventType::ValidationCheckRecorded => {
            let payload: ValidationCheckRecordedPayload = decode(event)?;
            let ValidationTarget::Revision { revision_id } = &payload.target;
            let target = ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            };
            correlate_target(
                &target,
                change_projection,
                history,
                &mut change_ids,
                &mut revision_refs,
                &mut unresolved_revision_ids,
            );
            (
                EventHistorySubjectV1::Review { target },
                EventHistorySummaryV1::ValidationCheckRecorded(payload),
            )
        }
        EventType::TaskCheckpointCaptured
        | EventType::TaskObservationRecorded
        | EventType::EventSignatureRecorded
        | EventType::ArtifactRemoved => return Ok(None),
        EventType::ChangeDeclared => {
            let payload: ChangeDeclaredPayload = decode(event)?;
            payload.validate()?;
            change_ids.insert(payload.change_id.clone());
            if let ChangeIdentityDescriptorV1::RootRevision { revision_id, .. } =
                &payload.identity_descriptor
            {
                add_revision_id(
                    revision_id,
                    change_projection,
                    &mut revision_refs,
                    &mut unresolved_revision_ids,
                );
            }
            (
                EventHistorySubjectV1::Change {
                    change_id: payload.change_id.clone(),
                },
                EventHistorySummaryV1::ChangeDeclared(payload),
            )
        }
        EventType::ChangeMembershipAsserted => {
            let payload: ChangeMembershipAssertedPayload = decode(event)?;
            payload.validate()?;
            change_ids.insert(payload.change_id.clone());
            add_revision_id(
                &payload.revision_id,
                change_projection,
                &mut revision_refs,
                &mut unresolved_revision_ids,
            );
            (
                EventHistorySubjectV1::ChangeMembershipClaim {
                    membership_claim_id: payload.membership_claim_id.clone(),
                },
                EventHistorySummaryV1::ChangeMembershipAsserted(payload),
            )
        }
        EventType::ChangeMembershipWithdrawn => {
            let payload: ChangeMembershipWithdrawnPayload = decode(event)?;
            payload.validate()?;
            if let Some((change_id, revision_id)) =
                history.membership_claims.get(&payload.membership_claim_id)
            {
                change_ids.insert(change_id.clone());
                add_revision_id(
                    revision_id,
                    change_projection,
                    &mut revision_refs,
                    &mut unresolved_revision_ids,
                );
            } else {
                diagnostics.insert(format!(
                    "event_history_membership_claim_missing:{}",
                    payload.membership_claim_id.as_str()
                ));
            }
            (
                EventHistorySubjectV1::ChangeMembershipClaim {
                    membership_claim_id: payload.membership_claim_id.clone(),
                },
                EventHistorySummaryV1::ChangeMembershipWithdrawn(payload),
            )
        }
        EventType::ChangeLinkAsserted => {
            let payload: ChangeLinkAssertedPayload = decode(event)?;
            payload.validate()?;
            change_ids.insert(payload.left_change_id.clone());
            change_ids.insert(payload.right_change_id.clone());
            (
                EventHistorySubjectV1::ChangeLinkClaim {
                    link_claim_id: payload.link_claim_id.clone(),
                },
                EventHistorySummaryV1::ChangeLinkAsserted(payload),
            )
        }
        EventType::ChangeRevisionRelationAsserted => {
            let payload: ChangeRevisionRelationAssertedPayload = decode(event)?;
            payload.validate()?;
            change_ids.insert(payload.change_id.clone());
            revision_refs.insert(payload.successor.clone());
            revision_refs.insert(payload.predecessor.clone());
            (
                EventHistorySubjectV1::ChangeRevisionRelationClaim {
                    relation_claim_id: payload.relation_claim_id.clone(),
                },
                EventHistorySummaryV1::ChangeRevisionRelationAsserted(payload),
            )
        }
        EventType::ChangeRevisionRelationWithdrawn => {
            let payload: ChangeRevisionRelationWithdrawnPayload = decode(event)?;
            payload.validate()?;
            if let Some((change_id, successor, predecessor)) =
                history.relation_claims.get(&payload.relation_claim_id)
            {
                change_ids.insert(change_id.clone());
                revision_refs.insert(successor.clone());
                revision_refs.insert(predecessor.clone());
            } else {
                diagnostics.insert(format!(
                    "event_history_relation_claim_missing:{}",
                    payload.relation_claim_id.as_str()
                ));
            }
            (
                EventHistorySubjectV1::ChangeRevisionRelationClaim {
                    relation_claim_id: payload.relation_claim_id.clone(),
                },
                EventHistorySummaryV1::ChangeRevisionRelationWithdrawn(payload),
            )
        }
        EventType::RevisionRelationAttested => {
            let payload: RevisionRelationAttestedPayload = decode(event)?;
            payload.validate()?;
            revision_refs.insert(payload.revision.clone());
            add_historical_changes(&payload.revision.revision_id, history, &mut change_ids);
            (
                EventHistorySubjectV1::RevisionRelationAttestation {
                    relation_attestation_id: payload.relation_attestation_id.clone(),
                    revision: payload.revision.clone(),
                },
                EventHistorySummaryV1::RevisionRelationAttested(payload),
            )
        }
        EventType::ReviewFactPorted => {
            let payload: ReviewFactPortedPayload = decode(event)?;
            let track_id = event.target.track_id.as_ref().ok_or_else(|| {
                crate::error::ShoreError::InvalidEvent {
                    message: "review_fact_ported requires an attributed review track".to_owned(),
                }
            })?;
            payload.validate_attribution(&event.writer.actor_id, track_id)?;
            revision_refs.insert(payload.origin_revision.clone());
            revision_refs.insert(payload.target_revision.clone());
            add_historical_changes(
                &payload.origin_revision.revision_id,
                history,
                &mut change_ids,
            );
            add_historical_changes(
                &payload.target_revision.revision_id,
                history,
                &mut change_ids,
            );
            if let Some(change_id) = &payload.context_change_id {
                change_ids.insert(change_id.clone());
            }
            (
                EventHistorySubjectV1::ReviewFactPort {
                    port_id: payload.port_id.clone(),
                    origin_revision: payload.origin_revision.clone(),
                    origin_fact: payload.origin_fact.clone(),
                },
                EventHistorySummaryV1::ReviewFactPorted(payload),
            )
        }
    };

    Ok(Some(EventHistoryEntryV1 {
        event_id: event.event_id.clone(),
        event_type: event.event_type,
        occurred_at: event.occurred_at.clone(),
        payload_hash: event.payload_hash.clone(),
        journal_id: event.target.journal_id.clone(),
        track_id: event.target.track_id.clone(),
        writer: event.writer.clone(),
        verification_status: EventVerificationStatus::Unsigned,
        assertion_mode: event.assertion_mode,
        signer: event.signer.clone(),
        source_ref: event.source_ref.clone(),
        ingest: event.ingest.clone(),
        subject,
        change_ids: change_ids.into_iter().collect(),
        revision_refs: revision_refs.into_iter().collect(),
        unresolved_revision_ids: unresolved_revision_ids.into_iter().collect(),
        summary,
        relation_targets: Vec::new(),
    }))
}

/// The fact family a relationship field names. Each family's fact ids are a
/// keyed identity in both the strict and derived Timeline lanes.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum EventHistoryRelationFamilyV1 {
    Observation,
    Assessment,
    InputRequest,
}

impl EventHistoryRelationFamilyV1 {
    /// The event type that records a fact of this family.
    pub(crate) const fn recording_event_type(self) -> EventType {
        match self {
            Self::Observation => EventType::ReviewObservationRecorded,
            Self::Assessment => EventType::ReviewAssessmentRecorded,
            Self::InputRequest => EventType::InputRequestOpened,
        }
    }

    /// The semantic family name the derived representative table keys by.
    pub(crate) const fn semantic_family(self) -> &'static str {
        match self {
            Self::Observation => "observation",
            Self::Assessment => "assessment",
            Self::InputRequest => "request",
        }
    }
}

/// A referenced fact's representative recording event: of every carrier that
/// records the fact id in its family, the one with the smallest event id. The
/// Revision is present only when that carrier is a Timeline entry, and names
/// the Revision it recorded the fact against.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EventHistoryRelationRecordV1 {
    pub(crate) event_id: EventId,
    pub(crate) timeline_revision_id: Option<RevisionId>,
}

/// The fact family and id one recording event carries, if any.
fn recorded_fact_of_event(
    event: &ShoreEvent,
) -> Result<Option<(EventHistoryRelationFamilyV1, String)>> {
    Ok(match event.event_type {
        EventType::ReviewObservationRecorded => {
            let payload: ReviewObservationRecordedPayload = decode(event)?;
            Some((
                EventHistoryRelationFamilyV1::Observation,
                payload.observation_id.as_str().to_owned(),
            ))
        }
        EventType::ReviewAssessmentRecorded => {
            let payload: ReviewAssessmentRecordedPayload = decode(event)?;
            Some((
                EventHistoryRelationFamilyV1::Assessment,
                payload.assessment_id.as_str().to_owned(),
            ))
        }
        EventType::InputRequestOpened => {
            let payload = decode_input_request_opened_payload(event.payload.clone())?;
            Some((
                EventHistoryRelationFamilyV1::InputRequest,
                payload.input_request_id.as_str().to_owned(),
            ))
        }
        _ => None,
    })
}

/// The Revision a Timeline entry recorded its fact against, if it records a
/// fact that relationship fields can reference.
fn event_history_recorded_revision(entry: &EventHistoryEntryV1) -> Option<&RevisionId> {
    let EventHistorySubjectV1::Review { target } = &entry.subject else {
        return None;
    };
    matches!(
        entry.summary,
        EventHistorySummaryV1::ReviewObservationRecorded(_)
            | EventHistorySummaryV1::ReviewAssessmentRecorded(_)
            | EventHistorySummaryV1::InputRequestOpened(_)
    )
    .then(|| revision_id_of(target))
}

/// Representative recording events for every recorded fact in a complete
/// generation, built by the strict projection with the same smallest-event-id
/// rule the derived representative table applies.
fn strict_relation_records(
    events: &[ShoreEvent],
    entries: &[EventHistoryEntryV1],
) -> Result<BTreeMap<(EventHistoryRelationFamilyV1, String), EventHistoryRelationRecordV1>> {
    let mut representatives = BTreeMap::<(EventHistoryRelationFamilyV1, String), EventId>::new();
    for event in events {
        if let Some(key) = recorded_fact_of_event(event)? {
            representatives
                .entry(key)
                .and_modify(|current| {
                    if event.event_id < *current {
                        *current = event.event_id.clone();
                    }
                })
                .or_insert_with(|| event.event_id.clone());
        }
    }
    let timeline = entries
        .iter()
        .filter_map(|entry| {
            event_history_recorded_revision(entry).map(|revision| (&entry.event_id, revision))
        })
        .collect::<BTreeMap<_, _>>();
    Ok(representatives
        .into_iter()
        .map(|(key, event_id)| {
            let timeline_revision_id = timeline.get(&event_id).map(|revision| (*revision).clone());
            (
                key,
                EventHistoryRelationRecordV1 {
                    event_id,
                    timeline_revision_id,
                },
            )
        })
        .collect())
}

/// The Revision an entry's relationship fields are resolved against and the
/// family and id of each fact those fields name.
pub(crate) fn event_history_relation_references(
    entry: &EventHistoryEntryV1,
) -> Option<(&RevisionId, BTreeSet<(EventHistoryRelationFamilyV1, &str)>)> {
    use EventHistoryRelationFamilyV1::{Assessment, InputRequest, Observation};
    let EventHistorySubjectV1::Review { target } = &entry.subject else {
        return None;
    };
    let references = match &entry.summary {
        EventHistorySummaryV1::ReviewObservationRecorded(payload) => payload
            .supersedes_observation_ids
            .iter()
            .chain(&payload.responds_to_observation_ids)
            .map(|id| (Observation, id.as_str()))
            .collect(),
        EventHistorySummaryV1::ReviewAssessmentRecorded(payload) => payload
            .replaces_assessment_ids
            .iter()
            .map(|id| (Assessment, id.as_str()))
            .chain(
                payload
                    .related_observation_ids
                    .iter()
                    .map(|id| (Observation, id.as_str())),
            )
            .chain(
                payload
                    .related_input_request_ids
                    .iter()
                    .map(|id| (InputRequest, id.as_str())),
            )
            .collect(),
        EventHistorySummaryV1::InputRequestResponded(payload) => {
            BTreeSet::from([(InputRequest, payload.input_request_id.as_str())])
        }
        _ => return None,
    };
    (!references.is_empty()).then(|| (revision_id_of(target), references))
}

/// Attach a Timeline target to every relationship id whose representative
/// recording event is a Timeline entry recorded against the same Revision as
/// the referencing entry. `resolve` performs exactly one keyed lookup per
/// distinct referenced fact; any other id receives no target.
pub(crate) fn attach_event_history_relation_targets<E>(
    entries: &mut [EventHistoryEntryV1],
    mut resolve: impl FnMut(
        EventHistoryRelationFamilyV1,
        &str,
    ) -> std::result::Result<Option<EventHistoryRelationRecordV1>, E>,
) -> std::result::Result<(), E> {
    let mut resolved = BTreeMap::new();
    for entry in entries {
        let Some((revision_id, references)) = event_history_relation_references(entry) else {
            continue;
        };
        let revision_id = revision_id.clone();
        let mut targets = Vec::new();
        for (family, fact_id) in references {
            let key = (family, fact_id.to_owned());
            let record = match resolved.get(&key) {
                Some(record) => record,
                None => {
                    let record = resolve(family, fact_id)?;
                    resolved.entry(key).or_insert(record)
                }
            };
            if let Some(record) = record
                && record.timeline_revision_id.as_ref() == Some(&revision_id)
            {
                targets.push(EventHistoryRelationTargetV1 {
                    fact_id: fact_id.to_owned(),
                    event_id: record.event_id.clone(),
                });
            }
        }
        targets.sort();
        entry.relation_targets = targets;
    }
    Ok(())
}

fn decode<T: DeserializeOwned>(event: &ShoreEvent) -> Result<T> {
    Ok(serde_json::from_value(event.payload.clone())?)
}

fn review_target_payload(
    target: ReviewTargetRef,
    summary: EventHistorySummaryV1,
    projection: &ChangeDocumentProjectionV1,
    history: &HistoricalCorrelationV1,
    change_ids: &mut BTreeSet<ChangeId>,
    revision_refs: &mut BTreeSet<RevisionRefV1>,
    unresolved_revision_ids: &mut BTreeSet<RevisionId>,
) -> (EventHistorySubjectV1, EventHistorySummaryV1) {
    correlate_target(
        &target,
        projection,
        history,
        change_ids,
        revision_refs,
        unresolved_revision_ids,
    );
    (EventHistorySubjectV1::Review { target }, summary)
}

fn correlate_target(
    target: &ReviewTargetRef,
    projection: &ChangeDocumentProjectionV1,
    history: &HistoricalCorrelationV1,
    change_ids: &mut BTreeSet<ChangeId>,
    revision_refs: &mut BTreeSet<RevisionRefV1>,
    unresolved_revision_ids: &mut BTreeSet<RevisionId>,
) {
    let revision_id = revision_id_of(target);
    add_revision_id(
        revision_id,
        projection,
        revision_refs,
        unresolved_revision_ids,
    );
    add_historical_changes(revision_id, history, change_ids);
}

fn revision_id_of(target: &ReviewTargetRef) -> &RevisionId {
    match target {
        ReviewTargetRef::Revision { revision_id }
        | ReviewTargetRef::File { revision_id, .. }
        | ReviewTargetRef::Range { revision_id, .. }
        | ReviewTargetRef::Observation { revision_id, .. }
        | ReviewTargetRef::InputRequest { revision_id, .. }
        | ReviewTargetRef::Assessment { revision_id, .. }
        | ReviewTargetRef::Event { revision_id, .. } => revision_id,
    }
}

fn add_historical_changes(
    revision_id: &RevisionId,
    history: &HistoricalCorrelationV1,
    change_ids: &mut BTreeSet<ChangeId>,
) {
    if let Some(ids) = history.change_ids_by_revision.get(revision_id) {
        change_ids.extend(ids.iter().cloned());
    }
}

fn add_revision_id(
    revision_id: &RevisionId,
    projection: &ChangeDocumentProjectionV1,
    revision_refs: &mut BTreeSet<RevisionRefV1>,
    unresolved_revision_ids: &mut BTreeSet<RevisionId>,
) {
    match projection.revision_refs.get(revision_id) {
        Some(refs) if refs.len() == 1 => {
            revision_refs.insert(refs[0].clone());
        }
        _ => {
            unresolved_revision_ids.insert(revision_id.clone());
        }
    }
}

fn add_direct_revision(
    revision_id: RevisionId,
    object_artifact_content_hash: String,
    revision_refs: &mut BTreeSet<RevisionRefV1>,
    unresolved_revision_ids: &mut BTreeSet<RevisionId>,
) {
    match RevisionRefV1::new(revision_id.clone(), object_artifact_content_hash) {
        Ok(reference) => {
            revision_refs.insert(reference);
        }
        Err(_) => {
            unresolved_revision_ids.insert(revision_id);
        }
    }
}

fn push_sorted_unique<T: Ord>(values: &mut Vec<T>, value: T) {
    match values.binary_search(&value) {
        Ok(_) => {}
        Err(index) => values.insert(index, value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::{EventHistorySubjectV1, EventHistorySummaryV1};
    use crate::model::{
        ChangeIdentityDescriptorV1, CommitAssociationId, EngagementId, InputRequestId,
        InputRequestResponseId, JournalId, ObjectId, ObservationId, ReviewTargetRef, RevisionId,
        RevisionRefV1, TaskTargetRef, TrackId, WorkObjectId,
    };
    use crate::session::event::{
        EventTarget, EventType, FactPortRelationV1, FactRefV1, InputRequestOpenedPayload,
        InputRequestReasonCode, InputRequestRespondedPayload, InputRequestResponseOutcome,
        RelationProofStatusV1, ReviewFactPortDraftV1, ReviewInitializedPayload,
        ReviewObservationRecordedPayload, RevisionRelationAttestationDraftV1,
        SemanticRevisionRelationV1, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload,
        Writer, build_change_declared, build_change_link_asserted, build_membership_asserted,
        build_membership_withdrawn, build_review_fact_ported, build_revision_relation_asserted,
        build_revision_relation_attested, build_revision_relation_withdrawn,
    };
    use crate::session::{AuthorityCursorV2, ChangeDocumentProjectionV1, project_change_documents};

    fn hash(byte: char) -> String {
        format!("sha256:{}", byte.to_string().repeat(64))
    }

    fn cursor(events: &[ShoreEvent]) -> AuthorityCursorV2 {
        AuthorityCursorV2 {
            schema: "pointbreak.authority-cursor.v2".to_owned(),
            journal_record_count: events.len() as u64,
            event_count: events.len() as u64,
            journal_record_set_hash: hash('a'),
            event_set_hash: hash('b'),
            capability_set_hash: hash('c'),
        }
    }

    fn journal_target() -> EventTarget {
        EventTarget::for_journal(JournalId::new("journal:test"))
    }

    fn event<P: crate::session::event::EventPayload>(
        event_type: EventType,
        key: &str,
        payload: P,
        occurred_at: &str,
    ) -> ShoreEvent {
        ShoreEvent::new(
            event_type,
            key,
            journal_target(),
            Writer::shore_local("timeline-test"),
            payload,
            occurred_at,
        )
        .unwrap()
    }

    fn revision_proposal(key: &str, revision_id: &str, artifact_hash: &str) -> ShoreEvent {
        event(
            EventType::WorkObjectProposed,
            key,
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new(format!("engagement:sha256:{key}")),
                work_object: WorkObjectProposal::Revision {
                    revision: crate::session::event::Revision {
                        id: RevisionId::new(revision_id),
                        object_id: ObjectId::new(format!("object:sha256:{key}")),
                        git_provenance: None,
                    },
                    summary: Some(format!("proposal {key}")),
                    object_artifact_content_hash: artifact_hash.to_owned(),
                    supersedes: vec![],
                },
            },
            "unix-ms:1000",
        )
    }

    fn observation(key: &str, revision_id: &str, occurred_at: &str) -> ShoreEvent {
        event(
            EventType::ReviewObservationRecorded,
            key,
            ReviewObservationRecordedPayload {
                observation_id: ObservationId::new(format!("observation:sha256:{key}")),
                target: ReviewTargetRef::Revision {
                    revision_id: RevisionId::new(revision_id),
                },
                title: format!("observation {key}"),
                body: Some("typed prose".to_owned()),
                body_content_type: Default::default(),
                body_artifact_path: None,
                body_byte_size: None,
                body_content_hash: None,
                tags: vec![],
                confidence: None,
                supersedes_observation_ids: vec![],
                responds_to_observation_ids: vec![],
            },
            occurred_at,
        )
    }

    #[test]
    fn every_event_type_has_one_explicit_timeline_classification() {
        let expected = [
            (
                EventType::ReviewInitialized,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::WorkObjectProposed,
                EventHistoryTypeClassV1::SubjectDependent,
            ),
            (
                EventType::ReviewObservationRecorded,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::ReviewAssessmentRecorded,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::InputRequestOpened,
                EventHistoryTypeClassV1::SubjectDependent,
            ),
            (
                EventType::InputRequestResponded,
                EventHistoryTypeClassV1::SubjectDependent,
            ),
            (
                EventType::ReviewNoteImported,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::RevisionRefAssociated,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::RevisionRefWithdrawn,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::RevisionCommitAssociated,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::RevisionCommitWithdrawn,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::ValidationCheckRecorded,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::TaskCheckpointCaptured,
                EventHistoryTypeClassV1::TaskDomain,
            ),
            (
                EventType::TaskObservationRecorded,
                EventHistoryTypeClassV1::TaskDomain,
            ),
            (
                EventType::EventSignatureRecorded,
                EventHistoryTypeClassV1::Carrier,
            ),
            (EventType::ArtifactRemoved, EventHistoryTypeClassV1::Carrier),
            (
                EventType::ChangeDeclared,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::ChangeMembershipAsserted,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::ChangeMembershipWithdrawn,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::ChangeLinkAsserted,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::ChangeRevisionRelationAsserted,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::ChangeRevisionRelationWithdrawn,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::RevisionRelationAttested,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
            (
                EventType::ReviewFactPorted,
                EventHistoryTypeClassV1::ReviewDomain,
            ),
        ];

        assert_eq!(expected.len(), EventType::ALL.len());
        for (event_type, class) in expected {
            assert_eq!(event_history_type_class(event_type), class);
        }
    }

    #[test]
    fn change_and_correction_families_keep_their_actual_subjects() {
        let first =
            build_change_declared(ChangeIdentityDescriptorV1::opaque_nonce([1; 32]), [2; 32])
                .unwrap();
        let second =
            build_change_declared(ChangeIdentityDescriptorV1::opaque_nonce([3; 32]), [4; 32])
                .unwrap();
        let revision = RevisionId::new("rev:sha256:one");
        let exact = RevisionRefV1::new(revision.clone(), hash('d')).unwrap();
        let membership = build_membership_asserted(&first.change_id, &revision, [5; 32]).unwrap();
        let membership_withdrawal =
            build_membership_withdrawn(&membership.membership_claim_id, [6; 32]).unwrap();
        let link = build_change_link_asserted(
            &first.change_id,
            &second.change_id,
            crate::session::event::ChangeLinkRelationV1::RelatedWork,
            [7; 32],
        )
        .unwrap();
        let relation = build_revision_relation_asserted(
            &first.change_id,
            exact.clone(),
            RevisionRefV1::new(RevisionId::new("rev:sha256:zero"), hash('e')).unwrap(),
            [8; 32],
        )
        .unwrap();
        let relation_withdrawal =
            build_revision_relation_withdrawn(&relation.relation_claim_id, [9; 32]).unwrap();
        let attestation = build_revision_relation_attested(RevisionRelationAttestationDraftV1 {
            revision: exact.clone(),
            commit_association_id: CommitAssociationId::new("commit-association:sha256:one"),
            semantic_relation: SemanticRevisionRelationV1::Unknown,
            proof_status: RelationProofStatusV1::Unverified,
            proof_method: "manual".into(),
            proof_algorithm_version: "v1".into(),
            capture_scope: vec!["worktree".into()],
            comparison_base_or_parent: None,
            endpoint_oids: vec!["abc".into()],
            evidence_content_hash: None,
            result_digest: hash('a'),
        })
        .unwrap();
        let track_id = TrackId::new("agent:timeline-test");
        let writer = Writer::shore_local("timeline-test");
        let fact_port = build_review_fact_ported(
            ReviewFactPortDraftV1 {
                origin_revision: exact.clone(),
                origin_fact: FactRefV1::Observation {
                    observation_id: ObservationId::new("observation:sha256:origin"),
                },
                target_revision: RevisionRefV1::new(
                    RevisionId::new("rev:sha256:target"),
                    hash('f'),
                )
                .unwrap(),
                relation: FactPortRelationV1::ContextOnly,
                target_fact: None,
                rationale_content_hash: None,
                context_change_id: Some(first.change_id.clone()),
            },
            &writer.actor_id,
            &track_id,
        )
        .unwrap();
        let events = vec![
            event(
                EventType::ChangeDeclared,
                "decl-1",
                first.clone(),
                "unix-ms:1",
            ),
            event(EventType::ChangeDeclared, "decl-2", second, "unix-ms:2"),
            event(
                EventType::ChangeMembershipAsserted,
                "member",
                membership.clone(),
                "unix-ms:3",
            ),
            event(
                EventType::ChangeMembershipWithdrawn,
                "member-w",
                membership_withdrawal,
                "unix-ms:4",
            ),
            event(
                EventType::ChangeLinkAsserted,
                "link",
                link.clone(),
                "unix-ms:5",
            ),
            event(
                EventType::ChangeRevisionRelationAsserted,
                "rel",
                relation.clone(),
                "unix-ms:6",
            ),
            event(
                EventType::ChangeRevisionRelationWithdrawn,
                "rel-w",
                relation_withdrawal,
                "unix-ms:7",
            ),
            event(
                EventType::RevisionRelationAttested,
                "attested",
                attestation.clone(),
                "unix-ms:8",
            ),
            ShoreEvent::new(
                EventType::ReviewFactPorted,
                "ported",
                EventTarget::for_revision(
                    JournalId::new("journal:test"),
                    exact.revision_id.clone(),
                    Some(track_id),
                )
                .unwrap(),
                writer,
                fact_port.clone(),
                "unix-ms:9",
            )
            .unwrap(),
        ];
        let documents = project_change_documents(&events).unwrap();
        let facade = project_event_history(
            &events,
            &documents,
            cursor(&events),
            "change-stamp".into(),
            &TrustSet::default(),
        )
        .unwrap();
        let subjects = facade
            .entries()
            .iter()
            .map(|entry| &entry.subject)
            .collect::<Vec<_>>();

        assert!(subjects.contains(&&EventHistorySubjectV1::Change {
            change_id: first.change_id
        }));
        assert!(
            subjects.contains(&&EventHistorySubjectV1::ChangeMembershipClaim {
                membership_claim_id: membership.membership_claim_id,
            })
        );
        assert!(subjects.contains(&&EventHistorySubjectV1::ChangeLinkClaim {
            link_claim_id: link.link_claim_id
        }));
        assert!(
            subjects.contains(&&EventHistorySubjectV1::ChangeRevisionRelationClaim {
                relation_claim_id: relation.relation_claim_id,
            })
        );
        assert!(
            subjects.contains(&&EventHistorySubjectV1::RevisionRelationAttestation {
                relation_attestation_id: attestation.relation_attestation_id,
                revision: exact,
            })
        );
        assert!(subjects.contains(&&EventHistorySubjectV1::ReviewFactPort {
            port_id: fact_port.port_id,
            origin_revision: fact_port.origin_revision,
            origin_fact: fact_port.origin_fact,
        }));
        assert!(facade.entries().iter().any(|entry| matches!(
            entry.summary,
            EventHistorySummaryV1::ChangeMembershipWithdrawn(_)
        )));
        assert!(facade.entries().iter().any(|entry| matches!(
            entry.summary,
            EventHistorySummaryV1::ChangeRevisionRelationWithdrawn(_)
        )));
    }

    #[test]
    fn historical_withdrawn_membership_preserves_many_to_many_change_correlation() {
        let proposal = revision_proposal("proposal", "rev:sha256:r", &hash('d'));
        let first =
            build_change_declared(ChangeIdentityDescriptorV1::opaque_nonce([1; 32]), [2; 32])
                .unwrap();
        let second =
            build_change_declared(ChangeIdentityDescriptorV1::opaque_nonce([3; 32]), [4; 32])
                .unwrap();
        let member_one =
            build_membership_asserted(&first.change_id, &RevisionId::new("rev:sha256:r"), [5; 32])
                .unwrap();
        let member_two =
            build_membership_asserted(&second.change_id, &RevisionId::new("rev:sha256:r"), [6; 32])
                .unwrap();
        let withdrawn =
            build_membership_withdrawn(&member_one.membership_claim_id, [7; 32]).unwrap();
        let observed = observation("observed", "rev:sha256:r", "unix-ms:9");
        let events = vec![
            proposal,
            event(
                EventType::ChangeDeclared,
                "first",
                first.clone(),
                "unix-ms:2",
            ),
            event(
                EventType::ChangeDeclared,
                "second",
                second.clone(),
                "unix-ms:3",
            ),
            event(
                EventType::ChangeMembershipAsserted,
                "m1",
                member_one,
                "unix-ms:4",
            ),
            event(
                EventType::ChangeMembershipAsserted,
                "m2",
                member_two,
                "unix-ms:5",
            ),
            event(
                EventType::ChangeMembershipWithdrawn,
                "withdraw",
                withdrawn,
                "unix-ms:6",
            ),
            observed.clone(),
        ];
        let documents = project_change_documents(&events).unwrap();
        let facade = project_event_history(
            &events,
            &documents,
            cursor(&events),
            "change-stamp".into(),
            &TrustSet::default(),
        )
        .unwrap();
        let entry = facade
            .entries()
            .iter()
            .find(|entry| entry.event_id == observed.event_id)
            .unwrap();
        assert_eq!(entry.change_ids, vec![first.change_id, second.change_id]);
    }

    #[test]
    fn exact_revision_resolution_distinguishes_direct_singleton_missing_and_ambiguous() {
        let direct = revision_proposal("direct", "rev:sha256:direct", &hash('d'));
        let singleton = observation("singleton", "rev:sha256:singleton", "unix-ms:2");
        let missing = observation("missing", "rev:sha256:missing", "unix-ms:3");
        let ambiguous = observation("ambiguous", "rev:sha256:ambiguous", "unix-ms:4");
        let singleton_ref =
            RevisionRefV1::new(RevisionId::new("rev:sha256:singleton"), hash('e')).unwrap();
        let mut documents = ChangeDocumentProjectionV1::default();
        documents.revision_refs.insert(
            singleton_ref.revision_id.clone(),
            vec![singleton_ref.clone()],
        );
        documents.revision_refs.insert(
            RevisionId::new("rev:sha256:ambiguous"),
            vec![
                RevisionRefV1::new(RevisionId::new("rev:sha256:ambiguous"), hash('f')).unwrap(),
                RevisionRefV1::new(RevisionId::new("rev:sha256:ambiguous"), hash('0')).unwrap(),
            ],
        );
        let events = vec![
            direct.clone(),
            singleton.clone(),
            missing.clone(),
            ambiguous.clone(),
        ];
        let facade = project_event_history(
            &events,
            &documents,
            cursor(&events),
            "change-stamp".into(),
            &TrustSet::default(),
        )
        .unwrap();
        let by_id = |event: &ShoreEvent| {
            facade
                .entries()
                .iter()
                .find(|entry| entry.event_id == event.event_id)
                .unwrap()
        };
        assert_eq!(
            by_id(&direct).revision_refs[0].object_artifact_content_hash,
            hash('d')
        );
        assert_eq!(by_id(&singleton).revision_refs, vec![singleton_ref]);
        assert_eq!(
            by_id(&missing).unresolved_revision_ids,
            vec![RevisionId::new("rev:sha256:missing")]
        );
        assert_eq!(
            by_id(&ambiguous).unresolved_revision_ids,
            vec![RevisionId::new("rev:sha256:ambiguous")]
        );
    }

    fn responding_observation(
        key: &str,
        revision_id: &str,
        occurred_at: &str,
        responds_to: &[&str],
    ) -> ShoreEvent {
        let mut payload: ReviewObservationRecordedPayload =
            serde_json::from_value(observation(key, revision_id, occurred_at).payload).unwrap();
        payload.responds_to_observation_ids = responds_to
            .iter()
            .map(|id| ObservationId::new(*id))
            .collect();
        event(
            EventType::ReviewObservationRecorded,
            key,
            payload,
            occurred_at,
        )
    }

    #[test]
    fn relation_targets_name_the_same_revision_representative_recording_event() {
        let root = observation("root", "rev:sha256:r", "unix-ms:2");
        let duplicate = event(
            EventType::ReviewObservationRecorded,
            "root-duplicate",
            serde_json::from_value::<ReviewObservationRecordedPayload>(root.payload.clone())
                .unwrap(),
            "unix-ms:3",
        );
        let duplicate_id = duplicate.event_id.clone();
        let other = observation("other", "rev:sha256:other", "unix-ms:1");
        let reply = responding_observation(
            "reply",
            "rev:sha256:r",
            "unix-ms:4",
            &[
                "observation:sha256:root",
                "observation:sha256:other",
                "observation:sha256:absent",
            ],
        );
        let cross = responding_observation(
            "cross",
            "rev:sha256:other",
            "unix-ms:5",
            &["observation:sha256:root"],
        );
        let events = vec![reply.clone(), cross.clone(), duplicate, other, root.clone()];
        let history = project_event_history(
            &events,
            &ChangeDocumentProjectionV1::default(),
            cursor(&events),
            "change-stamp".into(),
            &TrustSet::default(),
        )
        .unwrap();
        let entry = |event: &ShoreEvent| {
            history
                .entries()
                .iter()
                .find(|entry| entry.event_id == event.event_id)
                .unwrap()
                .clone()
        };
        // Two carriers record the root; the representative is the smaller
        // event id, the rule the derived representative table applies.
        let representative = root.event_id.clone().min(duplicate_id.clone());
        assert_eq!(
            entry(&reply).relation_targets,
            vec![EventHistoryRelationTargetV1 {
                fact_id: "observation:sha256:root".to_owned(),
                event_id: representative.clone(),
            }],
            "only the same-Revision parent resolves, to its representative recording event"
        );
        assert!(
            entry(&cross).relation_targets.is_empty(),
            "a parent recorded on another Revision has no target"
        );
        assert!(entry(&root).relation_targets.is_empty());
        let json = serde_json::to_value(entry(&reply)).unwrap();
        assert_eq!(
            json["relationTargets"],
            serde_json::json!([{
                "factId": "observation:sha256:root",
                "eventId": representative.as_str(),
            }])
        );
        assert!(
            serde_json::to_value(entry(&cross))
                .unwrap()
                .get("relationTargets")
                .is_none(),
            "an entry without targets omits the field"
        );
        let search = crate::session::event_history_search_record(&entry(&reply));
        assert!(
            !search.text.contains(representative.as_str()),
            "free text never matches through a resolved target"
        );
    }

    #[test]
    fn normalized_order_typed_summaries_and_stamp_follow_the_source_generation() {
        let a = observation("a", "rev:sha256:r", "1970-01-01T00:00:01Z");
        let b = observation("b", "rev:sha256:r", "unix-ms:1000");
        let early = observation("early", "rev:sha256:r", "unix-ms:1");
        let events = vec![b.clone(), a.clone(), early.clone()];
        let documents = ChangeDocumentProjectionV1::default();
        let first = project_event_history(
            &events,
            &documents,
            cursor(&events),
            "change-stamp".into(),
            &TrustSet::default(),
        )
        .unwrap();
        let mut changed_cursor = cursor(&events);
        changed_cursor.journal_record_set_hash = hash('9');
        let second = project_event_history(
            &events,
            &documents,
            changed_cursor,
            "changed-change-stamp".into(),
            &TrustSet::default(),
        )
        .unwrap();
        assert_eq!(first.entries()[0].event_id, early.event_id);
        let tied = &first.entries()[1..];
        assert!(tied[0].event_id < tied[1].event_id);
        assert_ne!(first.projection_stamp(), second.projection_stamp());
        let mut reversed = events.clone();
        reversed.reverse();
        let reordered = project_event_history(
            &reversed,
            &documents,
            cursor(&events),
            "change-stamp".into(),
            &TrustSet::default(),
        )
        .unwrap();
        assert_eq!(first.entries(), reordered.entries());
        assert_eq!(first.projection_stamp(), reordered.projection_stamp());
        let json = serde_json::to_value(first.document()).unwrap();
        assert!(
            json["entries"]
                .as_array()
                .unwrap()
                .iter()
                .all(|entry| entry.get("payload").is_none())
        );
        assert!(json["entries"][0]["summary"].get("kind").is_some());
        assert_eq!(json["entries"][0]["verificationStatus"], "unsigned");
    }

    #[test]
    fn timeline_source_stamp_rebind_recomputes_only_the_timeline_identity() {
        let event = observation("rebind", "rev:sha256:r", "unix-ms:1");
        let original = project_event_history(
            std::slice::from_ref(&event),
            &ChangeDocumentProjectionV1::default(),
            cursor(std::slice::from_ref(&event)),
            "sha256:legacy".to_owned(),
            &TrustSet::default(),
        )
        .unwrap()
        .document();

        let rebound = rebind_event_history_source_projection_stamp(
            original.clone(),
            "sha256:checkpoint".to_owned(),
            &TrustSet::default(),
        )
        .unwrap();

        assert_eq!(rebound.source_change_projection_stamp, "sha256:checkpoint");
        assert_ne!(
            rebound.timeline_projection_stamp,
            original.timeline_projection_stamp
        );
        assert_eq!(rebound.authority_cursor, original.authority_cursor);
        assert_eq!(rebound.entries, original.entries);
        assert_eq!(rebound.diagnostics, original.diagnostics);
        assert_eq!(rebound.facets, original.facets);
        assert_eq!(rebound.completion, original.completion);
    }

    #[test]
    fn a_later_generation_can_reveal_an_earlier_presentation_event() {
        let middle = observation("middle", "rev:sha256:r", "unix-ms:2000");
        let latest = observation("latest", "rev:sha256:r", "unix-ms:3000");
        let initial_events = vec![middle.clone(), latest.clone()];
        let documents = ChangeDocumentProjectionV1::default();
        let initial = project_event_history(
            &initial_events,
            &documents,
            cursor(&initial_events),
            "change-stamp".into(),
            &TrustSet::default(),
        )
        .unwrap();

        // Physical append order is not presentation order. A newly admitted
        // record may carry an older occurredAt than every previously admitted
        // record, so generation N+1 must re-project it into the earlier
        // position instead of treating the old tail as an append-only UI key.
        let backfilled = observation("backfilled", "rev:sha256:r", "unix-ms:1000");
        let mut next_events = initial_events.clone();
        next_events.push(backfilled.clone());
        let next = project_event_history(
            &next_events,
            &documents,
            cursor(&next_events),
            "next-change-stamp".into(),
            &TrustSet::default(),
        )
        .unwrap();

        assert_eq!(
            initial
                .entries()
                .iter()
                .map(|entry| &entry.event_id)
                .collect::<Vec<_>>(),
            vec![&middle.event_id, &latest.event_id]
        );
        assert_eq!(
            next.entries()
                .iter()
                .map(|entry| &entry.event_id)
                .collect::<Vec<_>>(),
            vec![&backfilled.event_id, &middle.event_id, &latest.event_id]
        );
        assert_ne!(initial.projection_stamp(), next.projection_stamp());
    }

    #[test]
    fn task_subjects_and_carrier_events_are_explicitly_excluded() {
        let task = event(
            EventType::WorkObjectProposed,
            "task-proposal",
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new("engagement:sha256:task"),
                work_object: WorkObjectProposal::TaskAttempt {
                    task_attempt_id: WorkObjectId::new("task-attempt:sha256:one"),
                    project_path: "/repo".into(),
                    claude_session_uuid: "uuid".into(),
                    initial_prompt_hash: hash('1'),
                    predecessor: None,
                    base_state_fingerprint: None,
                    source_speaker: None,
                },
            },
            "unix-ms:1",
        );
        let mut task_checkpoint = event(
            EventType::ReviewInitialized,
            "task-static",
            ReviewInitializedPayload {},
            "unix-ms:2",
        );
        task_checkpoint.event_type = EventType::TaskCheckpointCaptured;
        let mut signature = event(
            EventType::ReviewInitialized,
            "carrier",
            ReviewInitializedPayload {},
            "unix-ms:3",
        );
        signature.event_type = EventType::EventSignatureRecorded;
        let task_target = TaskTargetRef::TaskAttempt {
            task_attempt_id: WorkObjectId::new("task-attempt:sha256:one"),
        };
        let task_open = event(
            EventType::InputRequestOpened,
            "task-open",
            InputRequestOpenedPayload {
                input_request_id: InputRequestId::new("input-request:sha256:task"),
                target: ReviewTargetRef::Revision {
                    revision_id: RevisionId::new("rev:sha256:placeholder"),
                },
                task_target: Some(task_target.clone()),
                reason_code: InputRequestReasonCode::ManualDecisionRequired,
                title: "task input".into(),
                body: None,
                body_content_type: Default::default(),
                body_artifact_path: None,
                body_byte_size: None,
                body_content_hash: None,
                target_fingerprint: None,
            },
            "unix-ms:4",
        );
        let task_response = event(
            EventType::InputRequestResponded,
            "task-response",
            InputRequestRespondedPayload {
                input_request_response_id: InputRequestResponseId::new(
                    "input-request-response:sha256:task",
                ),
                input_request_id: InputRequestId::new("input-request:sha256:task"),
                revision_id: None,
                task_target: Some(task_target),
                outcome: InputRequestResponseOutcome::Approved,
                reason: None,
                reason_content_type: Default::default(),
                reason_artifact_path: None,
                reason_byte_size: None,
                reason_content_hash: None,
                target_fingerprint: None,
            },
            "unix-ms:5",
        );
        let events = vec![task, task_checkpoint, signature, task_open, task_response];
        let facade = project_event_history(
            &events,
            &ChangeDocumentProjectionV1::default(),
            cursor(&events),
            "change-stamp".into(),
            &TrustSet::default(),
        )
        .unwrap();
        assert!(facade.entries().is_empty());
    }
}
