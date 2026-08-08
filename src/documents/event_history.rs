use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::crypto::{EventVerificationStatus, SignerId};
use crate::model::{
    ChangeId, ChangeLinkClaimId, ChangeMembershipClaimId, ChangeRevisionRelationClaimId, EventId,
    JournalId, ReviewFactPortId, ReviewTargetRef, RevisionId, RevisionRefV1,
    RevisionRelationAttestationId, TrackId,
};
use crate::session::AuthorityCursorV2;
use crate::session::event::{
    AssertionMode, ChangeDeclaredPayload, ChangeLinkAssertedPayload,
    ChangeMembershipAssertedPayload, ChangeMembershipWithdrawnPayload,
    ChangeRevisionRelationAssertedPayload, ChangeRevisionRelationWithdrawnPayload, EventType,
    FactRefV1, IngestProvenance, InputRequestOpenedPayload, InputRequestRespondedPayload,
    ReviewAssessmentRecordedPayload, ReviewFactPortedPayload, ReviewObservationRecordedPayload,
    RevisionCommitAssociatedPayload, RevisionCommitWithdrawnPayload, RevisionRefAssociatedPayload,
    RevisionRefWithdrawnPayload, RevisionRelationAttestedPayload, SourceRef,
    ValidationCheckRecordedPayload, Writer,
};

pub const INSPECT_EVENT_HISTORY_SCHEMA: &str = "pointbreak.inspect-event-history";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventHistoryOrderV1 {
    Asc,
    Desc,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum EventHistorySubjectV1 {
    Journal {
        journal_id: JournalId,
    },
    Review {
        target: ReviewTargetRef,
    },
    Change {
        change_id: ChangeId,
    },
    ChangeMembershipClaim {
        membership_claim_id: ChangeMembershipClaimId,
    },
    ChangeLinkClaim {
        link_claim_id: ChangeLinkClaimId,
    },
    ChangeRevisionRelationClaim {
        relation_claim_id: ChangeRevisionRelationClaimId,
    },
    RevisionRelationAttestation {
        relation_attestation_id: RevisionRelationAttestationId,
        revision: RevisionRefV1,
    },
    ReviewFactPort {
        port_id: ReviewFactPortId,
        origin_revision: RevisionRefV1,
        origin_fact: FactRefV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "details",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum EventHistorySummaryV1 {
    ReviewInitialized,
    WorkObjectProposed {
        engagement_id: crate::model::EngagementId,
        revision: crate::session::event::Revision,
        summary: Option<String>,
        object_artifact_content_hash: String,
        supersedes: Vec<RevisionId>,
    },
    ReviewObservationRecorded(ReviewObservationRecordedPayload),
    ReviewAssessmentRecorded(ReviewAssessmentRecordedPayload),
    InputRequestOpened(InputRequestOpenedPayload),
    InputRequestResponded(InputRequestRespondedPayload),
    ReviewNoteImported,
    RevisionRefAssociated(RevisionRefAssociatedPayload),
    RevisionRefWithdrawn(RevisionRefWithdrawnPayload),
    RevisionCommitAssociated(RevisionCommitAssociatedPayload),
    RevisionCommitWithdrawn(RevisionCommitWithdrawnPayload),
    ValidationCheckRecorded(ValidationCheckRecordedPayload),
    ChangeDeclared(ChangeDeclaredPayload),
    ChangeMembershipAsserted(ChangeMembershipAssertedPayload),
    ChangeMembershipWithdrawn(ChangeMembershipWithdrawnPayload),
    ChangeLinkAsserted(ChangeLinkAssertedPayload),
    ChangeRevisionRelationAsserted(ChangeRevisionRelationAssertedPayload),
    ChangeRevisionRelationWithdrawn(ChangeRevisionRelationWithdrawnPayload),
    RevisionRelationAttested(RevisionRelationAttestedPayload),
    ReviewFactPorted(ReviewFactPortedPayload),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventHistoryEntryV1 {
    pub event_id: EventId,
    pub event_type: EventType,
    pub occurred_at: String,
    pub payload_hash: String,
    pub journal_id: JournalId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<TrackId>,
    pub writer: Writer,
    pub verification_status: EventVerificationStatus,
    pub assertion_mode: AssertionMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer: Option<SignerId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<SourceRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingest: Option<IngestProvenance>,
    pub subject: EventHistorySubjectV1,
    pub change_ids: Vec<ChangeId>,
    pub revision_refs: Vec<RevisionRefV1>,
    pub unresolved_revision_ids: Vec<RevisionId>,
    pub summary: EventHistorySummaryV1,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventHistoryCompletionV1 {
    pub event_types: Vec<EventType>,
    pub track_ids: Vec<TrackId>,
    pub change_ids: Vec<ChangeId>,
    pub revision_refs: Vec<RevisionRefV1>,
    pub unresolved_revision_ids: Vec<RevisionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventHistoryDocumentV1 {
    pub schema: String,
    pub version: u32,
    pub authority_cursor: AuthorityCursorV2,
    pub source_change_projection_stamp: String,
    pub timeline_projection_stamp: String,
    pub order: EventHistoryOrderV1,
    pub event_count: u64,
    pub match_count: usize,
    pub offset: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_index: Option<usize>,
    pub facets: BTreeMap<String, usize>,
    pub completion: EventHistoryCompletionV1,
    pub diagnostics: Vec<String>,
    pub query_notices: Vec<String>,
    pub entries: Vec<EventHistoryEntryV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

#[derive(Clone, Debug)]
pub struct EventHistoryFacadeV1 {
    document: EventHistoryDocumentV1,
}

impl EventHistoryFacadeV1 {
    pub(crate) fn new(document: EventHistoryDocumentV1) -> Self {
        Self { document }
    }

    pub fn document(&self) -> EventHistoryDocumentV1 {
        self.document.clone()
    }

    pub fn entries(&self) -> &[EventHistoryEntryV1] {
        &self.document.entries
    }

    pub fn projection_stamp(&self) -> &str {
        &self.document.timeline_projection_stamp
    }
}
