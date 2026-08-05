// Document builders for `pointbreak revision show` and `list`.
use std::collections::BTreeMap;

use crate::documents::{
    AssessmentViewDocument, CurrentAssessmentDocument, DiagnosticDocument,
    InputRequestViewDocument, ObservationViewDocument, ValidationCheckViewDocument,
};
use crate::model::{EventId, ReviewTargetRef, RevisionRefV1};
use crate::session::{
    ChangeMembershipClaimViewV1, CurrentCommitAssociation, CurrentRefAssociation,
    EndorsementReadback, EventVerificationStatus, MemberReadback, RevisionCommitRangeView,
    RevisionListEntry, RevisionListResult, RevisionProjectionIdentity, RevisionProjectionRow,
    RevisionProjectionSummary, RevisionShowFilters, RevisionShowResult, WithdrawnCommitAssociation,
    WithdrawnRefAssociation,
};

/// Documented body for `pointbreak.review-revision`.
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
    rows: Vec<RevisionProjectionRowDocument>,
    commit_range: CommitRangeDocument,
}

/// Context-free exact Revision document for the Change-capable reader cohort.
/// Membership is complete but currency requires a named Change context.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionShowBodyV3 {
    revision_ref: RevisionRefV1,
    change_memberships: Vec<ChangeMembershipClaimViewV1>,
    change_currency: &'static str,
    exact_revision: RevisionShowBody,
}

/// Documented body for `pointbreak.review-revision-list`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionListBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    event_set_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    projection_stamp: Option<String>,
    event_count: usize,
    revision_count: usize,
    entries: Vec<RevisionListEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ShowRevisionDocument {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    journal_id: String,
    revision_id: String,
    object_id: String,
    #[serde(flatten)]
    git_provenance: Option<crate::session::event::GitProvenance>,
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

/// Events-only commit-range lifecycle block. Liveness
/// (merged/live/unreachable/missing) is
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

/// Build the `pointbreak.review-revision` composite document from a show result.
pub fn revision_show_document(result: RevisionShowResult) -> DiagnosticDocument<RevisionShowBody> {
    let (body, diagnostics) = revision_show_parts(result);
    // Version 2 remains the historical pre-Change document. Capable callers
    // use `revision_show_document_v3` and never infer one scalar Change.
    DiagnosticDocument::with_version("pointbreak.review-revision", 2, body, diagnostics)
}

/// Build the Change-capable context-free exact Revision document.
pub fn revision_show_document_v3(
    result: RevisionShowResult,
    revision_ref: RevisionRefV1,
    change_memberships: Vec<ChangeMembershipClaimViewV1>,
) -> crate::error::Result<DiagnosticDocument<RevisionShowBodyV3>> {
    if result.revision.revision_id != revision_ref.revision_id
        || result.revision.object_artifact_content_hash != revision_ref.object_artifact_content_hash
    {
        return Err(crate::error::ShoreError::Message(
            "context-free Revision document requires an exact matching RevisionRefV1".to_owned(),
        ));
    }
    let (exact_revision, diagnostics) = revision_show_parts(result);
    Ok(DiagnosticDocument::with_version(
        "pointbreak.review-revision",
        3,
        RevisionShowBodyV3 {
            revision_ref,
            change_memberships,
            change_currency: "requires_change_context",
            exact_revision,
        },
        diagnostics,
    ))
}

fn revision_show_parts(
    mut result: RevisionShowResult,
) -> (RevisionShowBody, Vec<crate::session::ProjectionDiagnostic>) {
    // The readback side table is keyed by event id; attach it to each member and to
    // the capture identity at the document layer. Take it out before the by-value
    // moves below.
    let readbacks = std::mem::take(&mut result.member_readbacks);
    let body = RevisionShowBody {
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
        rows: result
            .rows
            .into_iter()
            .map(RevisionProjectionRowDocument::from)
            .collect(),
        commit_range: CommitRangeDocument::from(result.commit_range),
    };
    (body, result.diagnostics)
}

/// Build the `pointbreak.review-revision-list` document from a list result.
pub fn revision_list_document(result: RevisionListResult) -> DiagnosticDocument<RevisionListBody> {
    revision_list_page_document(result, None)
}

/// Build an authoritative, output-bounded revision-list page.
#[doc(hidden)]
pub fn revision_list_page_document(
    result: RevisionListResult,
    next_cursor: Option<String>,
) -> DiagnosticDocument<RevisionListBody> {
    revision_list_document_with_identity(result, None, next_cursor)
}

/// Build the same bounded revision-list document from a validated derived
/// projection. Projection and event-set identities are never relabeled.
#[doc(hidden)]
pub fn derived_revision_list_page_document(
    result: RevisionListResult,
    projection_stamp: String,
    next_cursor: Option<String>,
) -> DiagnosticDocument<RevisionListBody> {
    revision_list_document_with_identity(result, Some(projection_stamp), next_cursor)
}

fn revision_list_document_with_identity(
    result: RevisionListResult,
    projection_stamp: Option<String>,
    next_cursor: Option<String>,
) -> DiagnosticDocument<RevisionListBody> {
    let event_set_hash = projection_stamp.is_none().then_some(result.event_set_hash);
    DiagnosticDocument::new(
        "pointbreak.review-revision-list",
        RevisionListBody {
            event_set_hash,
            projection_stamp,
            event_count: result.event_count,
            revision_count: result.revision_count,
            entries: result.entries,
            next_cursor,
        },
        result.diagnostics,
    )
}

impl From<RevisionProjectionIdentity> for ShowRevisionDocument {
    fn from(identity: RevisionProjectionIdentity) -> Self {
        Self {
            id: identity.id.as_str().to_owned(),
            summary: identity.summary,
            journal_id: identity.journal_id.as_str().to_owned(),
            revision_id: identity.revision_id.as_str().to_owned(),
            object_id: identity.object_id.as_str().to_owned(),
            git_provenance: identity.git_provenance,
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
    use super::ShowRevisionDocument;
    use crate::model::{EventId, JournalId, ObjectId, RevisionId};
    use crate::session::RevisionProjectionIdentity;

    #[test]
    fn provenance_free_show_identity_omits_the_complete_git_triple() {
        let document = ShowRevisionDocument::from(RevisionProjectionIdentity {
            id: RevisionId::new("rev:sha256:non-git"),
            summary: Some("generated revision".to_owned()),
            journal_id: JournalId::new("journal:default"),
            git_provenance: None,
            revision_id: RevisionId::new("rev:sha256:non-git"),
            object_id: ObjectId::new("obj:sha256:non-git"),
            object_artifact_content_hash: "sha256:artifact".to_owned(),
            capture_event_id: EventId::new("evt:sha256:capture"),
        });

        let json = serde_json::to_value(document).unwrap();

        assert_eq!(json["id"], "rev:sha256:non-git");
        assert!(json.get("source").is_none());
        assert!(json.get("base").is_none());
        assert!(json.get("target").is_none());
        assert_eq!(json["objectId"], "obj:sha256:non-git");
    }
}
