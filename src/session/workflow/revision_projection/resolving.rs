use super::identity::RevisionProjectionIdentity;
use crate::error::{Result, ShoreError};
use crate::session::event::{
    EventType, GitProvenance, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload,
};
use crate::session::observation::ResolvedRevision;

pub(super) fn selected_revision_capture(
    events: &[ShoreEvent],
    resolved: &ResolvedRevision,
) -> Result<RevisionProjectionIdentity> {
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::WorkObjectProposed)
    {
        let payload: WorkObjectProposedPayload = serde_json::from_value(event.payload.clone())?;
        let WorkObjectProposal::Revision {
            revision,
            snapshot_artifact_content_hash,
            ..
        } = payload.work_object
        else {
            continue;
        };
        if revision.id == resolved.revision_id {
            let Some(GitProvenance {
                source,
                base,
                target,
            }) = revision.git_provenance
            else {
                return Err(ShoreError::Message(format!(
                    "captured revision {} has no git provenance",
                    revision.id.as_str()
                )));
            };
            return Ok(RevisionProjectionIdentity {
                id: revision.id.clone(),
                session_id: event.target.journal_id.clone(),
                source,
                base,
                target,
                revision_id: revision.id,
                snapshot_id: revision.object_id,
                snapshot_artifact_content_hash,
                capture_event_id: event.event_id.clone(),
            });
        }
    }

    Err(ShoreError::Message(format!(
        "captured review unit event missing for {}",
        resolved.revision_id.as_str()
    )))
}
