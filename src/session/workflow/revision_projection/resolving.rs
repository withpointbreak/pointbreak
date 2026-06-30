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
            object_artifact_content_hash,
            ..
        } = payload.work_object
        else {
            continue;
        };
        if revision.id == resolved.revision_id {
            // Provenance is enforced only for the matching capture, so a malformed
            // sibling (e.g. a fabricated identity-reuse capture with no provenance)
            // never masks the target the caller asked for.
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
                journal_id: event.target.journal_id.clone(),
                source,
                base,
                target,
                revision_id: revision.id,
                object_id: revision.object_id,
                object_artifact_content_hash,
                capture_event_id: event.event_id.clone(),
            });
        }
    }

    Err(ShoreError::Message(format!(
        "captured review unit event missing for {}",
        resolved.revision_id.as_str()
    )))
}

/// Every captured revision identity in the event set, in event order — the
/// single-pass enumeration the overview batch folds over. Mirrors
/// `list_from_events`' `WorkObjectProposed` scan and shares its provenance
/// requirement: a captured revision without git provenance is an error here, the
/// same way `entry_from_event` rejects it on the list path (so the batch and the
/// `/api/revisions` list it serves agree on which captures are listable). Task
/// proposals are skipped, exactly as the review listing skips them.
pub(super) fn enumerate_revision_identities(
    events: &[ShoreEvent],
) -> Result<Vec<RevisionProjectionIdentity>> {
    events
        .iter()
        .filter(|event| event.event_type == EventType::WorkObjectProposed)
        .filter_map(|event| revision_identity_from_capture_event(event).transpose())
        .collect()
}

/// Decode one `WorkObjectProposed` event into a [`RevisionProjectionIdentity`],
/// or `None` when the move proposes a task attempt rather than a review revision.
/// Errors when a captured revision lacks git provenance — matching the list
/// path's `entry_from_event`. Used by [`enumerate_revision_identities`].
fn revision_identity_from_capture_event(
    event: &ShoreEvent,
) -> Result<Option<RevisionProjectionIdentity>> {
    let payload: WorkObjectProposedPayload = serde_json::from_value(event.payload.clone())?;
    let WorkObjectProposal::Revision {
        revision,
        object_artifact_content_hash,
        ..
    } = payload.work_object
    else {
        return Ok(None);
    };
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
    Ok(Some(RevisionProjectionIdentity {
        id: revision.id.clone(),
        journal_id: event.target.journal_id.clone(),
        source,
        base,
        target,
        revision_id: revision.id,
        object_id: revision.object_id,
        object_artifact_content_hash,
        capture_event_id: event.event_id.clone(),
    }))
}
