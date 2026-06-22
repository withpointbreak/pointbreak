use serde::{Deserialize, Serialize};

use crate::error::{Result, ShoreError};
use crate::model::{
    EngagementType, JournalId, ReviewTargetRef, RevisionId, TargetRef, TrackId,
    engagement_type_of_subject,
};

/// The addressed triple every event envelope carries: the journal it files into,
/// the non-optional `subject` it addresses, and an optional review track.
///
/// `subject` is never absent. Genuinely subject-less carriers (the detached
/// co-signature carrier and content removal) address their real target by
/// payload content and ride the fieldless `TargetRef::Journal`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTarget {
    pub journal_id: JournalId,
    pub subject: TargetRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<TrackId>,
}

impl EventTarget {
    /// Address an explicit subject in a journal, optionally on a review track.
    pub fn for_subject(
        journal_id: JournalId,
        subject: TargetRef,
        track_id: Option<TrackId>,
    ) -> Self {
        Self {
            journal_id,
            subject,
            track_id,
        }
    }

    /// Carrier for a genuinely subject-less event: the detached co-signature
    /// carrier (addresses its target by the payload `target_event_id` /
    /// `target_event_record_hash`) and content removal (addresses its blob by
    /// the payload `content_hash`). The envelope files the fact into its journal
    /// by `journal_id`; the target stays addressed by payload content and is
    /// never duplicated onto the envelope.
    pub fn for_journal(journal_id: JournalId) -> Self {
        Self {
            journal_id,
            subject: TargetRef::Journal,
            track_id: None,
        }
    }

    /// Checked constructor for a generative move: the engagement's activity
    /// (`EngagementType`) must match the subject's derived domain. A `Review`
    /// engagement cannot mint a `Task` subject and vice versa — the single
    /// domain axis enforced at the write boundary rather than asserted as a
    /// free wire field.
    pub fn for_generative_move(
        journal_id: JournalId,
        engagement_type: EngagementType,
        subject: TargetRef,
        track_id: Option<TrackId>,
    ) -> Result<Self> {
        match engagement_type_of_subject(&subject) {
            Some(subject_domain) if subject_domain == engagement_type => {
                Ok(Self::for_subject(journal_id, subject, track_id))
            }
            other => Err(ShoreError::Message(format!(
                "generative move domain mismatch: a {engagement_type:?} engagement cannot address a {other:?} subject"
            ))),
        }
    }

    /// Convenience for addressing a review-domain revision subject, optionally on
    /// a track. Sugar over [`Self::for_subject`] with the `Review(Revision)`
    /// subject — the common review-event target.
    pub fn for_revision(
        journal_id: JournalId,
        revision_id: RevisionId,
        track_id: Option<TrackId>,
    ) -> Self {
        Self::for_subject(
            journal_id,
            TargetRef::Review(ReviewTargetRef::Revision { revision_id }),
            track_id,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ObjectId, TaskTargetRef};

    fn journal_id() -> JournalId {
        JournalId::new("journal:default")
    }

    fn revision_ref() -> ReviewTargetRef {
        ReviewTargetRef::Revision {
            revision_id: RevisionId::new("rev:sha256:abc"),
        }
    }

    #[test]
    fn event_target_is_the_addressed_triple() {
        let target =
            EventTarget::for_subject(journal_id(), TargetRef::Review(revision_ref()), None);

        assert_eq!(target.journal_id, journal_id());
        assert!(matches!(
            target.subject,
            TargetRef::Review(ReviewTargetRef::Revision { .. })
        ));
        assert!(target.track_id.is_none());
    }

    #[test]
    fn journal_carrier_addresses_target_by_payload_not_envelope() {
        let target = EventTarget::for_journal(journal_id());

        assert!(matches!(target.subject, TargetRef::Journal));
        assert!(target.track_id.is_none());
    }

    #[test]
    fn event_target_names_the_container_journal() {
        // The store-level container is the append-only Journal: the envelope
        // field/type/wire-key and the carrier subject tag all read "journal", and
        // the container-id value carries the `journal:` prefix.
        let target = EventTarget::for_journal(JournalId::new("journal:claude:uuid"));
        assert!(matches!(target.subject, TargetRef::Journal));

        let json = serde_json::to_value(&target).unwrap();
        assert!(json.get("journalId").is_some(), "wire key is journalId");
        assert!(
            json.get("ledgerId").is_none(),
            "legacy ledgerId key is gone"
        );
        assert!(json["journalId"].as_str().unwrap().starts_with("journal:"));
        assert_eq!(json["subject"], serde_json::json!("journal"));
    }

    #[test]
    fn for_journal_serializes_subject_as_bare_journal_tag_and_round_trips() {
        let target = EventTarget::for_journal(JournalId::new("journal:fixture"));

        let json = serde_json::to_value(&target).unwrap();
        assert_eq!(json["journalId"], "journal:fixture");
        assert_eq!(json["subject"], "journal");
        assert!(json.get("trackId").is_none());

        // Path-free: the carrier files into the journal by identity, not path.
        let text = json.to_string();
        assert!(!text.contains("/Users/"));
        assert!(!text.contains("worktreeRoot"));

        let parsed: EventTarget = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, target);
    }

    #[test]
    fn for_subject_serializes_the_review_subject_with_external_tag() {
        let target =
            EventTarget::for_subject(journal_id(), TargetRef::Review(revision_ref()), None);

        let json = serde_json::to_value(&target).unwrap();
        assert_eq!(json["subject"]["review"]["kind"], "revision");
        assert_eq!(json["subject"]["review"]["revisionId"], "rev:sha256:abc");
        assert!(json.get("workUnitId").is_none());
        assert!(json.get("workObjectId").is_none());
        assert!(json.get("workObjectType").is_none());
        assert!(json.get("reviewUnitId").is_none());
        assert!(json.get("snapshotId").is_none());
    }

    #[test]
    fn the_envelope_has_no_independent_domain_field() {
        // The domain is derived from the subject variant, never a standalone
        // wire field.
        let target =
            EventTarget::for_subject(journal_id(), TargetRef::Review(revision_ref()), None);
        let json = serde_json::to_value(&target).unwrap();

        assert!(json["subject"].get("workObjectType").is_none());
        assert!(json.get("workObjectType").is_none());
        assert!(json.get("domain").is_none());
        assert_eq!(
            crate::model::work_object_type_of_subject(&target.subject),
            Some(crate::model::WorkObjectType::Revision)
        );
    }

    #[test]
    fn a_review_engagement_refuses_a_task_subject() {
        let err = EventTarget::for_generative_move(
            journal_id(),
            EngagementType::Review,
            TargetRef::Task(TaskTargetRef::TaskAttempt),
            None,
        )
        .unwrap_err();

        assert!(matches!(err, ShoreError::Message(_)));
    }

    #[test]
    fn for_generative_move_accepts_a_matching_domain() {
        let target = EventTarget::for_generative_move(
            journal_id(),
            EngagementType::Review,
            TargetRef::Review(revision_ref()),
            None,
        )
        .unwrap();

        assert!(matches!(
            target.subject,
            TargetRef::Review(ReviewTargetRef::Revision { .. })
        ));
    }

    #[test]
    fn for_generative_move_refuses_a_journal_subject() {
        // A `Journal` carrier has no domain, so it cannot be a generative move.
        let err = EventTarget::for_generative_move(
            journal_id(),
            EngagementType::Review,
            TargetRef::Journal,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, ShoreError::Message(_)));
    }

    #[test]
    fn rejects_legacy_envelope_with_no_subject() {
        // The old envelope shape (a sessionId/workUnitId pair with no `subject`)
        // must fail to deserialize: subject is now non-optional.
        let legacy = r#"{"sessionId":"session:default","workUnitId":"work:default"}"#;
        let result: Result<EventTarget> = serde_json::from_str(legacy).map_err(Into::into);
        assert!(
            result.is_err(),
            "legacy subject-less envelope must not deserialize, got {:?}",
            result.ok()
        );
    }

    #[test]
    fn rejects_legacy_envelope_with_review_id() {
        let _ = ObjectId::new("obj:sha256:unused"); // keep ObjectId import exercised
        let legacy = r#"{"reviewId":"review:default","subject":"ledger"}"#;
        let result: Result<EventTarget> = serde_json::from_str(legacy).map_err(Into::into);
        assert!(
            result.is_err(),
            "legacy reviewId envelope must not deserialize, got {:?}",
            result.ok()
        );
    }
}
