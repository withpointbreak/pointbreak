use serde::{Deserialize, Serialize};

use super::kind::EventType;
use super::payload::EventPayload;
use crate::model::{
    InterventionId, InterventionResolutionId, ReviewTargetRef, ReviewUnitId, TrackId, WorkObjectId,
    WorkObjectType,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionMode {
    Blocking,
    Advisory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionReasonCode {
    AmbiguousState,
    UnsafeAction,
    StaleRevision,
    FailedGate,
    ExternalSideEffect,
    ConflictingEvent,
    MissingPermission,
    ManualDecisionRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionResolutionOutcome {
    Approved,
    Rejected,
    Dismissed,
    Superseded,
    Abandoned,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionRequestedPayload {
    pub intervention_id: InterventionId,
    pub target: ReviewTargetRef,
    pub mode: InterventionMode,
    pub reason_code: InterventionReasonCode,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_artifact_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_byte_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_content_hash: Option<String>,
}

impl InterventionRequestedPayload {
    // The two idempotency-key constructors materialize one shared pattern:
    // `<event-kind>:<work-object-identity-in-domain-appropriate-form>:<source_key>`.
    // Review-domain identity is `(review_unit_id, track_id)`; task-domain is
    // `(work_object_id, work_object_type)`. Two serializations of one pattern --
    // callers pick the constructor that matches their work-object kind.

    pub fn idempotency_key(
        review_unit_id: &ReviewUnitId,
        track_id: &TrackId,
        source_key: &str,
    ) -> String {
        format!(
            "intervention_requested:{}:{}:{}",
            review_unit_id.as_str(),
            track_id.as_str(),
            source_key
        )
    }

    pub fn idempotency_key_for_work_object(
        work_object_id: &WorkObjectId,
        work_object_type: WorkObjectType,
        source_key: &str,
    ) -> String {
        let kind = match work_object_type {
            WorkObjectType::ReviewUnit => "review_unit",
            WorkObjectType::TaskAttempt => "task_attempt",
        };
        format!(
            "intervention_requested:{}:{}:{}",
            work_object_id.as_str(),
            kind,
            source_key
        )
    }
}

impl EventPayload for InterventionRequestedPayload {
    fn event_type(&self) -> EventType {
        EventType::InterventionRequested
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionResolvedPayload {
    pub intervention_resolution_id: InterventionResolutionId,
    pub intervention_id: InterventionId,
    pub outcome: InterventionResolutionOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_artifact_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_byte_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_content_hash: Option<String>,
}

impl InterventionResolvedPayload {
    pub fn idempotency_key(intervention_id: &InterventionId, source_key: &str) -> String {
        format!(
            "intervention_resolved:{}:{}",
            intervention_id.as_str(),
            source_key
        )
    }
}

impl EventPayload for InterventionResolvedPayload {
    fn event_type(&self) -> EventType {
        EventType::InterventionResolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{WorkObjectId, WorkObjectType};

    #[test]
    fn idempotency_key_for_review_unit_keeps_existing_format() {
        // Backward-compat guard. The existing review-domain constructor is
        // unchanged; existing on-disk events stay matchable on retry.
        let key = InterventionRequestedPayload::idempotency_key(
            &ReviewUnitId::new("ru-1"),
            &TrackId::new("track-a"),
            "source-1",
        );
        assert_eq!(key, "intervention_requested:ru-1:track-a:source-1");
    }

    #[test]
    fn idempotency_key_for_work_object_uses_substrate_form() {
        let key = InterventionRequestedPayload::idempotency_key_for_work_object(
            &WorkObjectId::new("task-attempt:sha256:abc"),
            WorkObjectType::TaskAttempt,
            "source-1",
        );
        assert_eq!(
            key,
            "intervention_requested:task-attempt:sha256:abc:task_attempt:source-1"
        );
    }

    #[test]
    fn idempotency_key_constructors_do_not_collide_on_shared_source_key() {
        // The two constructors materialize one substrate-shaped pattern but
        // are intentionally byte-distinct so review-domain and task-domain
        // never produce the same key for unrelated work.
        let review = InterventionRequestedPayload::idempotency_key(
            &ReviewUnitId::new("shared"),
            &TrackId::new("track-a"),
            "source-1",
        );
        let task = InterventionRequestedPayload::idempotency_key_for_work_object(
            &WorkObjectId::new("shared"),
            WorkObjectType::TaskAttempt,
            "source-1",
        );
        assert_ne!(review, task);
    }
}
