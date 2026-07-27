use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::cursor::{AppendResolution, CursorModelError, ReferenceCursorLedger, TruthCursor};
use crate::canonical_hash::{sha256_bytes_hex, sha256_json_prefixed};
use crate::error::ShoreError;
use crate::session::event::ShoreEvent;
use crate::session::projection::{
    ArtifactRemovalProjection, EngagementGrouping, RevisionCommitRangeProjection, RevisionsByBase,
    SessionState, SupersessionView,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReferenceFact {
    pub(crate) logical_reread_key: String,
    pub(crate) replay_key: String,
    pub(crate) occurred_at: String,
    pub(crate) semantic_family: String,
    pub(crate) semantic_id: String,
    pub(crate) value: String,
}

impl ReferenceFact {
    pub(crate) fn new(
        logical_reread_key: impl Into<String>,
        replay_key: impl Into<String>,
        occurred_at: impl Into<String>,
        semantic_family: impl Into<String>,
        semantic_id: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            logical_reread_key: logical_reread_key.into(),
            replay_key: replay_key.into(),
            occurred_at: occurred_at.into(),
            semantic_family: semantic_family.into(),
            semantic_id: semantic_id.into(),
            value: value.into(),
        }
    }

    fn validation_witness(&self) -> Result<String, ShoreError> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct FactMaterial<'a> {
            logical_reread_key: &'a str,
            replay_key: &'a str,
            occurred_at: &'a str,
            semantic_family: &'a str,
            semantic_id: &'a str,
            value: &'a str,
        }
        sha256_json_prefixed(&serde_json::to_value(FactMaterial {
            logical_reread_key: &self.logical_reread_key,
            replay_key: &self.replay_key,
            occurred_at: &self.occurred_at,
            semantic_family: &self.semantic_family,
            semantic_id: &self.semantic_id,
            value: &self.value,
        })?)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ReferenceOverlays {
    pub(crate) trust_generation: u64,
    pub(crate) git_generation: u64,
    pub(crate) removed_content: BTreeSet<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OverlayMaterial<'a> {
    trust_generation: u64,
    git_generation: u64,
    removed_content: &'a BTreeSet<String>,
}

impl<'a> From<&'a ReferenceOverlays> for OverlayMaterial<'a> {
    fn from(overlays: &'a ReferenceOverlays) -> Self {
        Self {
            trust_generation: overlays.trust_generation,
            git_generation: overlays.git_generation,
            removed_content: &overlays.removed_content,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReferenceOracleReceipt {
    pub(crate) truth_cursor: TruthCursor,
    pub(crate) append_order: Vec<String>,
    pub(crate) replay_order: Vec<String>,
    pub(crate) display_order: Vec<String>,
    pub(crate) selected_semantics: BTreeMap<String, String>,
    pub(crate) semantic_receipt: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ReferenceOracle {
    ledger: ReferenceCursorLedger,
    append_order: Vec<ReferenceFact>,
    next_attempt: u64,
}

impl ReferenceOracle {
    pub(crate) fn new(store_id: impl Into<String>, epoch: u64) -> Self {
        Self {
            ledger: ReferenceCursorLedger::new(store_id, epoch),
            append_order: Vec::new(),
            next_attempt: 1,
        }
    }

    pub(crate) fn append(
        &mut self,
        fact: ReferenceFact,
    ) -> Result<AppendResolution, OracleModelError> {
        let witness = fact.validation_witness()?;
        let attempt = format!("reference-attempt-{}", self.next_attempt);
        self.next_attempt += 1;
        let resolution = self
            .ledger
            .append(&fact.logical_reread_key, &witness, &attempt)?;
        if matches!(resolution, AppendResolution::Created(_)) {
            self.append_order.push(fact);
        }
        Ok(resolution)
    }

    pub(crate) fn receipt(
        &self,
        overlays: &ReferenceOverlays,
    ) -> Result<ReferenceOracleReceipt, OracleModelError> {
        let append_order = self
            .append_order
            .iter()
            .map(|fact| fact.logical_reread_key.clone())
            .collect();

        let mut replay = self.append_order.iter().collect::<Vec<_>>();
        replay.sort_by(|left, right| {
            left.replay_key
                .cmp(&right.replay_key)
                .then_with(|| left.logical_reread_key.cmp(&right.logical_reread_key))
        });
        let replay_order = replay
            .iter()
            .map(|fact| fact.logical_reread_key.clone())
            .collect();

        let mut display = self.append_order.iter().collect::<Vec<_>>();
        display.sort_by(|left, right| {
            left.occurred_at
                .cmp(&right.occurred_at)
                .then_with(|| left.logical_reread_key.cmp(&right.logical_reread_key))
        });
        let display_order = display
            .iter()
            .map(|fact| fact.logical_reread_key.clone())
            .collect();

        // First-seen means first in canonical replay order, never first append.
        let mut selected_semantics = BTreeMap::new();
        for fact in replay {
            selected_semantics
                .entry(format!("{}:{}", fact.semantic_family, fact.semantic_id))
                .or_insert_with(|| fact.value.clone());
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ReceiptMaterial<'a> {
            truth_cursor: TruthCursorMaterial,
            selected_semantics: &'a BTreeMap<String, String>,
            overlays: OverlayMaterial<'a>,
        }
        #[derive(Serialize)]
        struct TruthCursorMaterial {
            epoch: u64,
            sequence: u64,
        }
        let truth_cursor = self.ledger.head().cursor;
        let semantic_receipt = sha256_json_prefixed(&serde_json::to_value(ReceiptMaterial {
            truth_cursor: TruthCursorMaterial {
                epoch: truth_cursor.epoch,
                sequence: truth_cursor.sequence,
            },
            selected_semantics: &selected_semantics,
            overlays: overlays.into(),
        })?)?;

        Ok(ReferenceOracleReceipt {
            truth_cursor,
            append_order,
            replay_order,
            display_order,
            selected_semantics,
            semantic_receipt,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StrictReplayReceipt {
    pub(crate) event_count: usize,
    pub(crate) replay_event_ids: Vec<String>,
    pub(crate) display_event_ids: Vec<String>,
    pub(crate) semantic_receipt: String,
}

/// Run the existing strict session reducer after every append prefix. The
/// wrapper models create-once truth: equal duplicates do not add a carrier and
/// conflicting duplicates fail rather than changing the first stored bytes.
pub(crate) fn strict_session_prefix_receipts(
    append_schedule: &[ShoreEvent],
    overlays: &ReferenceOverlays,
) -> Result<Vec<StrictReplayReceipt>, OracleModelError> {
    let mut stored = BTreeMap::<String, ShoreEvent>::new();
    let mut receipts = Vec::with_capacity(append_schedule.len());
    for event in append_schedule {
        match stored.get(&event.idempotency_key) {
            None => {
                stored.insert(event.idempotency_key.clone(), event.clone());
            }
            Some(existing) if existing.payload_hash == event.payload_hash => {}
            Some(_) => {
                return Err(OracleModelError::ConflictingDuplicate(
                    event.idempotency_key.clone(),
                ));
            }
        }
        receipts.push(strict_session_receipt(
            stored.values().cloned().collect::<Vec<_>>(),
            overlays,
        )?);
    }
    Ok(receipts)
}

fn strict_session_receipt(
    mut events: Vec<ShoreEvent>,
    overlays: &ReferenceOverlays,
) -> Result<StrictReplayReceipt, OracleModelError> {
    events.sort_by(|left, right| {
        replay_key_for(&left.idempotency_key).cmp(&replay_key_for(&right.idempotency_key))
    });
    let replay_event_ids = events
        .iter()
        .map(|event| event.event_id.as_str().to_owned())
        .collect::<Vec<_>>();
    let state = SessionState::from_events(&events)?;
    let supersession = SupersessionView::from_events(&events)?;
    let revisions_by_base = RevisionsByBase::from_events(&events)?;
    let commit_ranges = RevisionCommitRangeProjection::from_events(&events)?;
    let engagements = EngagementGrouping::from_events(&events)?;
    let removal_claims = ArtifactRemovalProjection::from_events(&events)?
        .claimed_hashes()
        .map(str::to_owned)
        .collect::<Vec<_>>();

    let mut display = events.iter().collect::<Vec<_>>();
    display.sort_by(|left, right| {
        left.occurred_at
            .cmp(&right.occurred_at)
            .then_with(|| left.event_id.as_str().cmp(right.event_id.as_str()))
    });
    let display_event_ids = display
        .iter()
        .map(|event| event.event_id.as_str().to_owned())
        .collect::<Vec<_>>();

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct StrictReceiptMaterial<'a> {
        state: &'a SessionState,
        supersession: &'a SupersessionView,
        revisions_by_base: &'a RevisionsByBase,
        commit_ranges: &'a RevisionCommitRangeProjection,
        engagements: &'a EngagementGrouping,
        removal_claims: &'a [String],
        replay_event_ids: &'a [String],
        overlays: OverlayMaterial<'a>,
    }
    let semantic_receipt = sha256_json_prefixed(&serde_json::to_value(StrictReceiptMaterial {
        state: &state,
        supersession: &supersession,
        revisions_by_base: &revisions_by_base,
        commit_ranges: &commit_ranges,
        engagements: &engagements,
        removal_claims: &removal_claims,
        replay_event_ids: &replay_event_ids,
        overlays: overlays.into(),
    })?)?;

    Ok(StrictReplayReceipt {
        event_count: state.event_count,
        replay_event_ids,
        display_event_ids,
        semantic_receipt,
    })
}

fn replay_key_for(logical_reread_key: &str) -> String {
    sha256_bytes_hex(logical_reread_key.as_bytes())
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum OracleModelError {
    #[error(transparent)]
    Cursor(#[from] CursorModelError),
    #[error(transparent)]
    Product(#[from] ShoreError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("conflicting duplicate for logical reread key {0}")]
    ConflictingDuplicate(String),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::model::{
        AssessmentId, CommitAssociationId, EngagementId, InputRequestId, InputRequestResponseId,
        JournalId, ObjectId, ReviewEndpoint, ReviewTargetRef, RevisionId, RevisionSource,
        TargetRef, TrackId, ValidationCheckId, ValidationStatus, ValidationTarget,
        ValidationTrigger, WorktreeCaptureMode,
    };
    use crate::session::derived_access::checkpoint::{
        CheckpointAuthority, DerivedCheckpoint, DerivedMutation, ReferenceDerivedState,
    };
    use crate::session::derived_access::cursor::{CursorReceipt, TruthCursor};
    use crate::session::event::{
        ArtifactRemovedPayload, EventTarget, EventType, GitProvenance, InputRequestOpenedPayload,
        InputRequestReasonCode, InputRequestRespondedPayload, InputRequestResponseOutcome,
        ReviewAssessment, ReviewAssessmentRecordedPayload, ReviewInitializedPayload, Revision,
        RevisionCommitAssociatedPayload, ShoreEvent, ValidationCheckRecordedPayload,
        WorkObjectProposal, WorkObjectProposedPayload, Writer,
    };

    const JOURNAL: &str = "journal:sha256:derived-oracle";
    const TRACK: &str = "agent:oracle";

    fn initialized(idempotency_key: &str, occurred_at: &str) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewInitialized,
            idempotency_key,
            EventTarget::for_journal(JournalId::new("journal:sha256:oracle")),
            Writer::shore_local("test"),
            ReviewInitializedPayload {},
            occurred_at,
        )
        .unwrap()
    }

    fn revision_id(suffix: &str) -> RevisionId {
        RevisionId::new(format!("review-unit:sha256:{suffix}"))
    }

    fn revision_event(suffix: &str, supersedes: Vec<RevisionId>, occurred_at: &str) -> ShoreEvent {
        let revision_id = revision_id(suffix);
        ShoreEvent::new(
            EventType::WorkObjectProposed,
            format!("work_object_proposed:{}", revision_id.as_str()),
            EventTarget::for_revision(
                JournalId::new(JOURNAL),
                revision_id.clone(),
                Some(TrackId::new(TRACK)),
            )
            .unwrap(),
            Writer::shore_local("test"),
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new("engagement:sha256:derived-oracle"),
                work_object: WorkObjectProposal::Revision {
                    revision: Revision {
                        id: revision_id,
                        object_id: ObjectId::new(format!("obj:sha256:{suffix}")),
                        git_provenance: Some(GitProvenance {
                            source: RevisionSource::GitWorktree {
                                mode: WorktreeCaptureMode::CombinedHeadToWorkingTree,
                                include_untracked: true,
                                pathspecs: Vec::new(),
                            },
                            base: ReviewEndpoint::GitCommit {
                                commit_oid: "base".to_owned(),
                                tree_oid: "base-tree".to_owned(),
                            },
                            target: ReviewEndpoint::GitWorkingTree {
                                worktree_root: "/repo".to_owned(),
                            },
                        }),
                    },
                    summary: None,
                    object_artifact_content_hash: format!("sha256:artifact:{suffix}"),
                    supersedes,
                },
            },
            occurred_at,
        )
        .unwrap()
    }

    fn review_event_target(revision_id: &RevisionId) -> EventTarget {
        EventTarget::for_subject(
            JournalId::new(JOURNAL),
            TargetRef::Review(ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            }),
            Some(TrackId::new(TRACK)),
        )
        .unwrap()
    }

    fn assessment_event(
        revision_id: &RevisionId,
        suffix: &str,
        assessment: ReviewAssessment,
        replaces_assessment_ids: Vec<AssessmentId>,
        occurred_at: &str,
    ) -> ShoreEvent {
        let assessment_id = AssessmentId::new(format!("assess:sha256:{suffix}"));
        let target = ReviewTargetRef::Revision {
            revision_id: revision_id.clone(),
        };
        ShoreEvent::new(
            EventType::ReviewAssessmentRecorded,
            ReviewAssessmentRecordedPayload::idempotency_key(
                revision_id,
                &TrackId::new(TRACK),
                suffix,
            ),
            review_event_target(revision_id),
            Writer::shore_local("test"),
            ReviewAssessmentRecordedPayload {
                assessment_id,
                target,
                assessment,
                summary: None,
                summary_content_type: Default::default(),
                summary_artifact_path: None,
                summary_byte_size: None,
                summary_content_hash: None,
                replaces_assessment_ids,
                related_observation_ids: Vec::new(),
                related_input_request_ids: Vec::new(),
            },
            occurred_at,
        )
        .unwrap()
    }

    fn input_request_event(
        revision_id: &RevisionId,
        input_request_id: &InputRequestId,
    ) -> ShoreEvent {
        ShoreEvent::new(
            EventType::InputRequestOpened,
            InputRequestOpenedPayload::idempotency_key(
                revision_id,
                &TrackId::new(TRACK),
                input_request_id.as_str(),
            ),
            review_event_target(revision_id),
            Writer::shore_local("test"),
            InputRequestOpenedPayload {
                input_request_id: input_request_id.clone(),
                target: ReviewTargetRef::Revision {
                    revision_id: revision_id.clone(),
                },
                task_target: None,
                reason_code: InputRequestReasonCode::ManualDecisionRequired,
                title: "Choose the next action".to_owned(),
                body: None,
                body_content_type: Default::default(),
                body_artifact_path: None,
                body_byte_size: None,
                body_content_hash: None,
                target_fingerprint: None,
            },
            "2026-07-27T00:00:04Z",
        )
        .unwrap()
    }

    fn input_response_event(
        revision_id: &RevisionId,
        input_request_id: &InputRequestId,
    ) -> ShoreEvent {
        let response_id = InputRequestResponseId::new("input-response:sha256:derived-oracle");
        ShoreEvent::new(
            EventType::InputRequestResponded,
            InputRequestRespondedPayload::idempotency_key(input_request_id, response_id.as_str()),
            EventTarget::for_subject(
                JournalId::new(JOURNAL),
                TargetRef::Review(ReviewTargetRef::InputRequest {
                    revision_id: revision_id.clone(),
                    input_request_id: input_request_id.clone(),
                }),
                Some(TrackId::new(TRACK)),
            )
            .unwrap(),
            Writer::shore_local("test"),
            InputRequestRespondedPayload {
                input_request_response_id: response_id,
                input_request_id: input_request_id.clone(),
                revision_id: Some(revision_id.clone()),
                task_target: None,
                outcome: InputRequestResponseOutcome::Approved,
                reason: None,
                reason_content_type: Default::default(),
                reason_artifact_path: None,
                reason_byte_size: None,
                reason_content_hash: None,
                target_fingerprint: None,
            },
            "2026-07-27T00:00:05Z",
        )
        .unwrap()
    }

    fn validation_event(revision_id: &RevisionId) -> ShoreEvent {
        let validation_id = ValidationCheckId::new("validation:sha256:derived-oracle");
        ShoreEvent::new(
            EventType::ValidationCheckRecorded,
            ValidationCheckRecordedPayload::idempotency_key(
                revision_id,
                &TrackId::new(TRACK),
                validation_id.as_str(),
            ),
            review_event_target(revision_id),
            Writer::shore_local("test"),
            ValidationCheckRecordedPayload {
                validation_check_id: validation_id,
                target: ValidationTarget::Revision {
                    revision_id: revision_id.clone(),
                },
                check_name: "oracle".to_owned(),
                command: None,
                status: ValidationStatus::Passed,
                exit_code: Some(0),
                trigger: ValidationTrigger::Manual,
                source_fingerprint: None,
                summary: None,
                summary_content_type: Default::default(),
                summary_artifact_path: None,
                summary_byte_size: None,
                summary_content_hash: None,
                started_at: None,
                completed_at: None,
                log_artifact_content_hashes: Vec::new(),
            },
            "2026-07-27T00:00:06Z",
        )
        .unwrap()
    }

    fn association_event(revision_id: &RevisionId) -> ShoreEvent {
        let commit_oid = "0123456789abcdef0123456789abcdef01234567";
        ShoreEvent::new(
            EventType::RevisionCommitAssociated,
            RevisionCommitAssociatedPayload::idempotency_key(revision_id, commit_oid),
            review_event_target(revision_id),
            Writer::shore_local("test"),
            RevisionCommitAssociatedPayload {
                commit_association_id: CommitAssociationId::new(
                    "assoc-commit:sha256:derived-oracle",
                ),
                target: ReviewTargetRef::Revision {
                    revision_id: revision_id.clone(),
                },
                commit: ReviewEndpoint::GitCommit {
                    commit_oid: commit_oid.to_owned(),
                    tree_oid: "tree-derived-oracle".to_owned(),
                },
            },
            "2026-07-27T00:00:07Z",
        )
        .unwrap()
    }

    fn removal_event() -> ShoreEvent {
        let content_hash = "sha256:artifact:b";
        ShoreEvent::new(
            EventType::ArtifactRemoved,
            ArtifactRemovedPayload::idempotency_key(content_hash),
            EventTarget::for_journal(JournalId::new(JOURNAL)),
            Writer::shore_local("test"),
            ArtifactRemovedPayload {
                content_hash: content_hash.to_owned(),
            },
            "2026-07-27T00:00:08Z",
        )
        .unwrap()
    }

    fn required_event_schedule() -> Vec<ShoreEvent> {
        let revision_a = revision_id("a");
        let revision_b = revision_id("b");
        let input_request_id = InputRequestId::new("input-request:sha256:derived-oracle");
        vec![
            revision_event("a", Vec::new(), "2026-07-27T00:00:02Z"),
            revision_event("b", vec![revision_a.clone()], "2026-07-27T00:00:01Z"),
            revision_event("c", vec![revision_a], "2026-07-27T00:00:02Z"),
            assessment_event(
                &revision_b,
                "assessment-one",
                ReviewAssessment::NeedsChanges,
                Vec::new(),
                "2026-07-27T00:00:03Z",
            ),
            assessment_event(
                &revision_b,
                "assessment-two",
                ReviewAssessment::Accepted,
                vec![AssessmentId::new("assess:sha256:assessment-one")],
                "2026-07-27T00:00:04Z",
            ),
            input_request_event(&revision_b, &input_request_id),
            input_response_event(&revision_b, &input_request_id),
            validation_event(&revision_b),
            association_event(&revision_b),
            removal_event(),
        ]
    }

    #[test]
    fn strict_existing_reducer_is_replayed_after_every_prefix() {
        // The second key's SHA-256 replay key sorts before the first, and its
        // occurredAt is backdated, so neither replay nor display order follows
        // append order.
        let first = initialized("review-initialized:a", "2026-07-27T00:00:02Z");
        let second = initialized("review-initialized:z", "2026-07-27T00:00:01Z");
        let receipts = strict_session_prefix_receipts(
            &[first.clone(), second.clone(), first],
            &ReferenceOverlays::default(),
        )
        .unwrap();

        assert_eq!(
            receipts
                .iter()
                .map(|receipt| receipt.event_count)
                .collect::<Vec<_>>(),
            vec![1, 2, 2]
        );
        assert_eq!(receipts[1].replay_event_ids[0], second.event_id.as_str());
        assert_eq!(receipts[1].display_event_ids[0], second.event_id.as_str());
        assert_eq!(receipts[1], receipts[2]);
    }

    #[test]
    fn strict_receipt_binds_external_overlays_without_changing_truth() {
        let event = initialized("review-initialized:one", "2026-07-27T00:00:00Z");
        let plain = strict_session_prefix_receipts(
            std::slice::from_ref(&event),
            &ReferenceOverlays::default(),
        )
        .unwrap();
        let enriched = strict_session_prefix_receipts(
            &[event],
            &ReferenceOverlays {
                trust_generation: 1,
                git_generation: 1,
                removed_content: ["sha256:removed".to_owned()].into_iter().collect(),
            },
        )
        .unwrap();

        assert_eq!(plain[0].event_count, enriched[0].event_count);
        assert_eq!(plain[0].replay_event_ids, enriched[0].replay_event_ids);
        assert_ne!(plain[0].semantic_receipt, enriched[0].semantic_receipt);
    }

    #[test]
    fn required_real_event_matrix_drives_every_strict_reducer_after_each_prefix() {
        let schedule = required_event_schedule();
        let mut with_equal_duplicate = schedule.clone();
        with_equal_duplicate.push(schedule[3].clone());
        let receipts =
            strict_session_prefix_receipts(&with_equal_duplicate, &ReferenceOverlays::default())
                .unwrap();

        assert_eq!(receipts.len(), with_equal_duplicate.len());
        assert_eq!(
            receipts
                .iter()
                .map(|receipt| receipt.event_count)
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 10]
        );
        assert_eq!(receipts[9], receipts[10]);
        assert!(
            receipts[..10]
                .windows(2)
                .all(|pair| pair[0].semantic_receipt != pair[1].semantic_receipt)
        );

        let final_unique = &receipts[9];
        let append_event_ids = schedule
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<Vec<_>>();
        assert_ne!(
            final_unique.replay_event_ids, append_event_ids,
            "the real matrix must contain a digest-earlier append"
        );
        assert_eq!(
            final_unique.display_event_ids[0],
            schedule[1].event_id.as_str(),
            "the backdated second append must lead chronological display order"
        );

        let observed_types = schedule
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            observed_types,
            [
                "work_object_proposed",
                "review_assessment_recorded",
                "input_request_opened",
                "input_request_responded",
                "validation_check_recorded",
                "revision_commit_associated",
                "artifact_removed",
            ]
            .into_iter()
            .collect()
        );

        let mut conflicting = schedule[3].clone();
        conflicting.payload_hash = "sha256:conflicting-payload".to_owned();
        let mut conflict_schedule = schedule;
        conflict_schedule.push(conflicting);
        assert!(matches!(
            strict_session_prefix_receipts(&conflict_schedule, &ReferenceOverlays::default()),
            Err(OracleModelError::ConflictingDuplicate(_))
        ));
    }

    #[test]
    fn checkpoint_receipt_matches_strict_full_replay_at_every_real_event_prefix() {
        let schedule = required_event_schedule();
        let strict =
            strict_session_prefix_receipts(&schedule, &ReferenceOverlays::default()).unwrap();
        let authority = CheckpointAuthority::new(
            JOURNAL,
            "profile:reference-v1",
            1,
            BTreeMap::from([("strict-full-replay".to_owned(), 1)]),
        );
        let mut derived =
            ReferenceDerivedState::new(DerivedCheckpoint::empty(authority, TruthCursor::new(1, 0)));

        for (offset, (event, strict_receipt)) in schedule.iter().zip(&strict).enumerate() {
            let sequence = u64::try_from(offset + 1).unwrap();
            let cursor = TruthCursor::new(1, sequence);
            let receipt = CursorReceipt {
                cursor,
                logical_reread_key: event.idempotency_key.clone(),
                validation_witness: event.payload_hash.clone(),
                attempt_token: format!("attempt:{sequence}"),
            };
            derived
                .apply_delta_atomic(
                    cursor,
                    &[receipt],
                    &[DerivedMutation::put(
                        "strict-full-replay",
                        "semantic-receipt",
                        &strict_receipt.semantic_receipt,
                    )],
                    None,
                )
                .unwrap();
            derived
                .mark_family_covered("strict-full-replay", cursor)
                .unwrap();
            assert_eq!(
                derived.row("strict-full-replay", "semantic-receipt"),
                Some(strict_receipt.semantic_receipt.as_str())
            );
            assert_eq!(derived.checkpoint().applied_cursor, cursor);
        }
    }
}
