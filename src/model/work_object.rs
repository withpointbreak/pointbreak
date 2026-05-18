use serde::{Deserialize, Serialize};

use super::review_unit::ReviewTargetRef;

/// Substrate-level discriminator for which domain a work object belongs to.
///
/// Used alongside `WorkObjectId` to give substrate-shaped events polymorphic
/// identity without forcing every domain to share a serialization layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkObjectType {
    ReviewUnit,
    TaskAttempt,
}

/// Reserved for Phase 3 (`task-supervision-prototype-proposal.md` §4.2).
///
/// Currently a placeholder that serializes as `{}` so substrate-shaped code
/// can refer to a task-domain target without a domain-specific shape baked in.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskTargetRef {}

/// Substrate-level target reference. Externally tagged so that each domain's
/// own internal shape (e.g., `ReviewTargetRef`'s `kind` discriminator) is
/// preserved unchanged inside the variant payload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetRef {
    Review(ReviewTargetRef),
    Task(TaskTargetRef),
}

#[cfg(test)]
mod tests {
    use crate::model::{
        ReviewTargetRef, ReviewUnitId, TargetRef, TaskTargetRef, WorkObjectId, WorkObjectType,
    };

    #[test]
    fn work_object_id_round_trips_through_serde_and_string() {
        let id = WorkObjectId::new("task-attempt:sha256:abc");

        let json = serde_json::to_string(&id).unwrap();
        let parsed: WorkObjectId = serde_json::from_str(&json).unwrap();

        assert_eq!(json, "\"task-attempt:sha256:abc\"");
        assert_eq!(parsed, id);
        assert_eq!(parsed.as_str(), "task-attempt:sha256:abc");
    }

    #[test]
    fn work_object_type_serializes_with_snake_case_kind() {
        let review = serde_json::to_string(&WorkObjectType::ReviewUnit).unwrap();
        let task = serde_json::to_string(&WorkObjectType::TaskAttempt).unwrap();

        assert_eq!(review, "\"review_unit\"");
        assert_eq!(task, "\"task_attempt\"");

        let parsed_review: WorkObjectType = serde_json::from_str(&review).unwrap();
        let parsed_task: WorkObjectType = serde_json::from_str(&task).unwrap();
        assert_eq!(parsed_review, WorkObjectType::ReviewUnit);
        assert_eq!(parsed_task, WorkObjectType::TaskAttempt);
    }

    #[test]
    fn task_target_ref_serializes_as_empty_object_placeholder() {
        let task = TaskTargetRef::default();

        let json = serde_json::to_value(&task).unwrap();
        assert_eq!(json, serde_json::json!({}));
    }

    #[test]
    fn target_ref_review_wraps_review_target_ref_externally_tagged() {
        let target = TargetRef::Review(ReviewTargetRef::ReviewUnit {
            review_unit_id: ReviewUnitId::new("review-unit:sha256:abc"),
        });

        let json = serde_json::to_value(&target).unwrap();

        assert_eq!(json["review"]["kind"], "review_unit");
        assert_eq!(json["review"]["reviewUnitId"], "review-unit:sha256:abc");
        assert!(json.get("task").is_none());
    }

    #[test]
    fn target_ref_task_carries_task_target_ref_externally_tagged() {
        let target = TargetRef::Task(TaskTargetRef::default());

        let json = serde_json::to_value(&target).unwrap();

        assert_eq!(json["task"], serde_json::json!({}));
        assert!(json.get("review").is_none());
    }
}
