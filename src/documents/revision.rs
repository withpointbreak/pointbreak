// Document builders for `shore revision show` and `list`.
use std::collections::BTreeMap;

use crate::documents::{
    AssessmentViewDocument, CurrentAssessmentDocument, DiagnosticDocument,
    InputRequestViewDocument, ObservationViewDocument, ValidationCheckViewDocument,
};
use crate::model::{EventId, ReviewTargetRef, Side};
use crate::session::{
    AdapterNoteView, BodyContentState, CurrentCommitAssociation, CurrentRefAssociation,
    EndorsementReadback, EventVerificationStatus, MemberReadback, RevisionCommitRangeView,
    RevisionListEntry, RevisionListResult, RevisionProjectionIdentity, RevisionProjectionRow,
    RevisionProjectionSummary, RevisionShowFilters, RevisionShowResult, WithdrawnCommitAssociation,
    WithdrawnRefAssociation,
};

/// Documented body for `shore.review-revision`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionShowBody {
    event_set_hash: String,
    event_count: usize,
    revision: ShowRevisionDocument,
    filters: RevisionShowFiltersDocument,
    summary: RevisionShowSummaryDocument,
    current_assessment: CurrentAssessmentDocument,
    observations: Vec<ObservationViewDocument>,
    input_requests: Vec<InputRequestViewDocument>,
    assessments: Vec<AssessmentViewDocument>,
    validation_checks: Vec<ValidationCheckViewDocument>,
    adapter_notes: Vec<AdapterNoteDocument>,
    rows: Vec<RevisionProjectionRowDocument>,
    commit_range: CommitRangeDocument,
}

/// Documented body for `shore.review-revision-list`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionListBody {
    event_set_hash: String,
    event_count: usize,
    revision_count: usize,
    entries: Vec<RevisionListEntry>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ShowRevisionDocument {
    id: String,
    journal_id: String,
    revision_id: String,
    object_id: String,
    source: crate::model::RevisionSource,
    base: crate::model::ReviewEndpoint,
    target: crate::model::ReviewEndpoint,
    object_artifact_content_hash: String,
    /// The capture event id, kept only to key the readback side table; never
    /// serialized (the identity renders no `eventId` of its own).
    #[serde(skip)]
    capture_event_id: EventId,
    #[serde(skip_serializing_if = "Option::is_none")]
    verification_status: Option<EventVerificationStatus>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    endorsements: Vec<EndorsementReadback>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RevisionShowFiltersDocument {
    revision_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    track_id: Option<String>,
    include_body: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RevisionShowSummaryDocument {
    file_count: usize,
    row_count: usize,
    narrative_row_count: usize,
    snapshot_row_count: usize,
    snapshot_remainder_row_count: usize,
    observation_count: usize,
    input_request_count: usize,
    assessment_count: usize,
    validation_check_count: usize,
    adapter_note_count: usize,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AdapterNoteDocument {
    id: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    #[serde(skip_serializing_if = "BodyContentState::is_present")]
    body_content_state: BodyContentState,
    /// The removal key when the body is removed: the imported-note payload
    /// carries no body content hash, so this is the surface's twin of the
    /// snapshot result's removed-content-hash field; absent while present.
    #[serde(skip_serializing_if = "Option::is_none")]
    removed_body_content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<AdapterNoteTargetDocument>,
    status: &'static str,
    file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_old_path: Option<String>,
    tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<String>,
    sidecar_content_hash: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AdapterNoteTargetDocument {
    side: Side,
    start_line: u32,
    end_line: u32,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RevisionProjectionRowDocument {
    id: String,
    kind: &'static str,
    projection_phase: &'static str,
    projection_order: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_order: Option<SnapshotOrderDocument>,
    coverage: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<ReviewTargetRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_path: Option<String>,
    related_observation_ids: Vec<String>,
    related_input_request_ids: Vec<String>,
    related_assessment_ids: Vec<String>,
    related_validation_check_ids: Vec<String>,
}

/// Events-only commit-range lifecycle block. Liveness (merged/live/orphaned) is
/// layered by repo-holding callers, never here. The view's `revisionId` and
/// `diagnostics` are omitted: the id renders on the revision identity and the
/// diagnostics merge into the document's top-level diagnostics.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CommitRangeDocument {
    anchored: bool,
    current_commits: Vec<CurrentCommitAssociation>,
    current_refs: Vec<CurrentRefAssociation>,
    withdrawn_commits: Vec<WithdrawnCommitAssociation>,
    withdrawn_refs: Vec<WithdrawnRefAssociation>,
}

impl From<RevisionCommitRangeView> for CommitRangeDocument {
    fn from(view: RevisionCommitRangeView) -> Self {
        Self {
            anchored: view.anchored,
            current_commits: view.current_commits,
            current_refs: view.current_refs,
            withdrawn_commits: view.withdrawn_commits,
            withdrawn_refs: view.withdrawn_refs,
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotOrderDocument {
    file_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hunk_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    row_index: Option<usize>,
}

/// Build the `shore.review-revision` composite document from a show result.
pub fn revision_show_document(
    mut result: RevisionShowResult,
) -> DiagnosticDocument<RevisionShowBody> {
    // The readback side table is keyed by event id; attach it to each member and to
    // the capture identity at the document layer. Take it out before the by-value
    // moves below.
    let readbacks = std::mem::take(&mut result.member_readbacks);
    DiagnosticDocument::new(
        "shore.review-revision",
        RevisionShowBody {
            event_set_hash: result.event_set_hash,
            event_count: result.event_count,
            revision: ShowRevisionDocument::from(result.revision).with_readback(&readbacks),
            filters: RevisionShowFiltersDocument::from(result.filters),
            summary: RevisionShowSummaryDocument::from(result.summary),
            current_assessment: CurrentAssessmentDocument::from(result.current_assessment),
            observations: result
                .observations
                .into_iter()
                .map(|view| ObservationViewDocument::from(view).with_readback(&readbacks))
                .collect(),
            input_requests: result
                .input_requests
                .into_iter()
                .map(|view| InputRequestViewDocument::from(view).with_readback(&readbacks))
                .collect(),
            assessments: result
                .assessments
                .into_iter()
                .map(|view| AssessmentViewDocument::from(view).with_readback(&readbacks))
                .collect(),
            validation_checks: result
                .validation_checks
                .into_iter()
                .map(|view| ValidationCheckViewDocument::from(view).with_readback(&readbacks))
                .collect(),
            adapter_notes: result
                .adapter_notes
                .into_iter()
                .map(AdapterNoteDocument::from)
                .collect(),
            rows: result
                .rows
                .into_iter()
                .map(RevisionProjectionRowDocument::from)
                .collect(),
            commit_range: CommitRangeDocument::from(result.commit_range),
        },
        result.diagnostics,
    )
}

/// Build the `shore.review-revision-list` document from a list result.
pub fn revision_list_document(result: RevisionListResult) -> DiagnosticDocument<RevisionListBody> {
    DiagnosticDocument::new(
        "shore.review-revision-list",
        RevisionListBody {
            event_set_hash: result.event_set_hash,
            event_count: result.event_count,
            revision_count: result.revision_count,
            entries: result.entries,
        },
        result.diagnostics,
    )
}

impl From<RevisionProjectionIdentity> for ShowRevisionDocument {
    fn from(identity: RevisionProjectionIdentity) -> Self {
        Self {
            id: identity.id.as_str().to_owned(),
            journal_id: identity.journal_id.as_str().to_owned(),
            revision_id: identity.revision_id.as_str().to_owned(),
            object_id: identity.object_id.as_str().to_owned(),
            source: identity.source,
            base: identity.base,
            target: identity.target,
            object_artifact_content_hash: identity.object_artifact_content_hash,
            capture_event_id: identity.capture_event_id,
            verification_status: None,
            endorsements: Vec::new(),
        }
    }
}

impl ShowRevisionDocument {
    /// Attach the reader-relative readback for the capture event. The identity has
    /// no `eventId` of its own, so it keys the side table on `capture_event_id`.
    fn with_readback(mut self, table: &BTreeMap<EventId, MemberReadback>) -> Self {
        if let Some(readback) = table.get(&self.capture_event_id) {
            self.verification_status = readback.verification_status;
            self.endorsements = readback.endorsements.clone();
        }
        self
    }
}

impl From<RevisionShowFilters> for RevisionShowFiltersDocument {
    fn from(filters: RevisionShowFilters) -> Self {
        Self {
            revision_id: filters.revision_id.as_str().to_owned(),
            track_id: filters
                .track_id
                .map(|track_id| track_id.as_str().to_owned()),
            include_body: filters.include_body,
        }
    }
}

impl From<RevisionProjectionSummary> for RevisionShowSummaryDocument {
    fn from(summary: RevisionProjectionSummary) -> Self {
        Self {
            file_count: summary.file_count,
            row_count: summary.row_count,
            narrative_row_count: summary.narrative_row_count,
            snapshot_row_count: summary.snapshot_row_count,
            snapshot_remainder_row_count: summary.snapshot_remainder_row_count,
            observation_count: summary.observation_count,
            input_request_count: summary.input_request_count,
            assessment_count: summary.assessment_count,
            validation_check_count: summary.validation_check_count,
            adapter_note_count: summary.adapter_note_count,
        }
    }
}

impl From<AdapterNoteView> for AdapterNoteDocument {
    fn from(view: AdapterNoteView) -> Self {
        Self {
            id: view.id,
            title: view.title,
            body: view.body,
            body_content_state: view.body_content_state,
            removed_body_content_hash: view.removed_body_content_hash,
            target: view.target.map(AdapterNoteTargetDocument::from),
            status: view.status.as_str(),
            file_path: view.file_path,
            file_old_path: view.file_old_path,
            tags: view.tags,
            confidence: view.confidence,
            external_source: view.external_source,
            author: view.author,
            created_at: view.created_at,
            sidecar_content_hash: view.sidecar_content_hash,
        }
    }
}

impl From<crate::session::event::ImportedNoteTarget> for AdapterNoteTargetDocument {
    fn from(target: crate::session::event::ImportedNoteTarget) -> Self {
        Self {
            side: target.side,
            start_line: target.start_line,
            end_line: target.end_line,
        }
    }
}

impl From<RevisionProjectionRow> for RevisionProjectionRowDocument {
    fn from(row: RevisionProjectionRow) -> Self {
        Self {
            id: row.id.as_str().to_owned(),
            kind: row.kind.as_str(),
            projection_phase: row.projection_phase.as_str(),
            projection_order: row.projection_order,
            snapshot_order: row.snapshot_order.map(SnapshotOrderDocument::from),
            coverage: row.coverage.as_str(),
            target: row.target,
            file_path: row.file_path,
            old_path: row.old_path,
            related_observation_ids: row
                .related_observation_ids
                .into_iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
            related_input_request_ids: row
                .related_input_request_ids
                .into_iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
            related_assessment_ids: row
                .related_assessment_ids
                .into_iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
            related_validation_check_ids: row
                .related_validation_check_ids
                .into_iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
        }
    }
}

impl From<crate::session::SnapshotOrder> for SnapshotOrderDocument {
    fn from(order: crate::session::SnapshotOrder) -> Self {
        Self {
            file_index: order.file_index,
            metadata_index: order.metadata_index,
            hunk_index: order.hunk_index,
            row_index: order.row_index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::AdapterNoteStatus;

    fn adapter_note_view(
        body: Option<String>,
        state: BodyContentState,
        removed_hash: Option<String>,
    ) -> AdapterNoteView {
        AdapterNoteView {
            id: "note:one".to_owned(),
            title: "t".to_owned(),
            body,
            body_content_state: state,
            removed_body_content_hash: removed_hash,
            target: None,
            status: AdapterNoteStatus::Exact,
            file_path: "src/lib.rs".to_owned(),
            file_old_path: None,
            tags: vec![],
            confidence: None,
            external_source: None,
            author: None,
            created_at: None,
            sidecar_content_hash: "sha256:sidecar".to_owned(),
        }
    }

    #[test]
    fn removed_adapter_note_document_carries_state_and_removed_hash() {
        let view = adapter_note_view(
            None,
            BodyContentState::PhysicallyRemoved,
            Some("sha256:x".to_owned()),
        );

        let json = serde_json::to_value(AdapterNoteDocument::from(view)).unwrap();

        assert_eq!(json["bodyContentState"], "physically_removed");
        assert_eq!(json["removedBodyContentHash"], "sha256:x");
        assert!(json.get("body").is_none());
    }

    #[test]
    fn present_adapter_note_document_omits_removal_fields() {
        let view = adapter_note_view(Some("b".to_owned()), BodyContentState::Present, None);

        let json = serde_json::to_value(AdapterNoteDocument::from(view)).unwrap();

        assert!(json.get("bodyContentState").is_none());
        assert!(json.get("removedBodyContentHash").is_none());
        assert_eq!(json["body"], "b");
    }
}
