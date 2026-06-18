use super::identity::ReviewUnitProjectionIdentity;
use crate::error::{Result, ShoreError};
use crate::session::event::{EventType, ReviewUnitCapturedPayload, ShoreEvent};
use crate::session::observation::ResolvedReviewUnit;

pub(super) fn selected_review_unit_capture(
    events: &[ShoreEvent],
    resolved: &ResolvedReviewUnit,
) -> Result<ReviewUnitProjectionIdentity> {
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::ReviewUnitCaptured)
    {
        let payload: ReviewUnitCapturedPayload = serde_json::from_value(event.payload.clone())?;
        if payload.review_unit_id == resolved.review_unit_id {
            return Ok(ReviewUnitProjectionIdentity {
                id: payload.review_unit_id,
                session_id: event.target.session_id.clone(),
                source: payload.source,
                base: payload.base,
                target: payload.target,
                revision_id: payload.revision_id,
                snapshot_id: payload.snapshot_id,
                snapshot_artifact_content_hash: payload.snapshot_artifact_content_hash,
                capture_event_id: event.event_id.clone(),
            });
        }
    }

    Err(ShoreError::Message(format!(
        "captured review unit event missing for {}",
        resolved.review_unit_id.as_str()
    )))
}
