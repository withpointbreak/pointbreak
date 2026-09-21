//! The authoritative lane: fold the store's events into the summary's input
//! rows.

use serde::de::DeserializeOwned;

use super::model::{
    AssessmentInput, CaptureInput, CommitAssociationInput, CountedEventRef, MembershipClaimInput,
    MembershipWithdrawalInput, ReviewSummaryInputs,
};
use crate::error::{Result, ShoreError};
use crate::model::RevisionId;
use crate::session::event::{
    ChangeMembershipAssertedPayload, ChangeMembershipWithdrawnPayload, EventType,
    ReviewAssessmentRecordedPayload, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload,
};
use crate::session::parse_event_instant;
use crate::session::store::capabilities::BackfillCohort;

/// Fold events into input rows. A mapped event whose payload does not decode
/// fails the read and names the event; dropping it would silently change a
/// reported measure.
pub(crate) fn review_summary_inputs_from_events(
    events: &[ShoreEvent],
    cohort: &BackfillCohort,
) -> Result<ReviewSummaryInputs> {
    let mut inputs = ReviewSummaryInputs {
        manifest_hash: cohort.manifest_hash().map(str::to_owned),
        ..ReviewSummaryInputs::default()
    };
    for event in events {
        match event.event_type {
            EventType::ChangeMembershipAsserted => {
                let payload: ChangeMembershipAssertedPayload = decode(event)?;
                inputs.memberships.push(MembershipClaimInput {
                    source: source_of(event),
                    claim_id: payload.membership_claim_id.as_str().to_owned(),
                    change_id: payload.change_id.as_str().to_owned(),
                    revision_id: payload.revision_id.as_str().to_owned(),
                    backfill: cohort.contains(event.event_id.as_str()),
                });
            }
            EventType::ChangeMembershipWithdrawn => {
                let payload: ChangeMembershipWithdrawnPayload = decode(event)?;
                inputs.withdrawals.push(MembershipWithdrawalInput {
                    source: source_of(event),
                    claim_id: payload.membership_claim_id.as_str().to_owned(),
                });
            }
            EventType::WorkObjectProposed => {
                let payload: WorkObjectProposedPayload = decode(event)?;
                if let WorkObjectProposal::Revision { revision, .. } = payload.work_object {
                    inputs.captures.push(CaptureInput {
                        source: source_of(event),
                        revision_id: revision.id.as_str().to_owned(),
                        actor: event.writer.actor_id.as_str().to_owned(),
                        captured_at_millis: parse_event_instant(&event.occurred_at),
                    });
                }
            }
            EventType::ReviewAssessmentRecorded => {
                let payload: ReviewAssessmentRecordedPayload = decode(event)?;
                inputs.assessments.push(AssessmentInput {
                    source: source_of(event),
                    assessment_id: payload.assessment_id.as_str().to_owned(),
                    revision_id: enclosing_revision(event)?.map(|id| id.as_str().to_owned()),
                    actor: event.writer.actor_id.as_str().to_owned(),
                    verdict: payload.assessment,
                    replaces: payload
                        .replaces_assessment_ids
                        .iter()
                        .map(|id| id.as_str().to_owned())
                        .collect(),
                });
            }
            EventType::RevisionCommitAssociated => {
                if let Some(revision_id) = enclosing_revision(event)? {
                    inputs.commit_associations.push(CommitAssociationInput {
                        source: source_of(event),
                        revision_id: revision_id.as_str().to_owned(),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(inputs)
}

fn source_of(event: &ShoreEvent) -> CountedEventRef {
    CountedEventRef {
        event_id: event.event_id.as_str().to_owned(),
        payload_hash: event.payload_hash.clone(),
    }
}

fn decode<P: DeserializeOwned>(event: &ShoreEvent) -> Result<P> {
    serde_json::from_value(event.payload.clone()).map_err(|error| undecodable(event, &error))
}

/// The Revision enclosing the event's target, whatever its scope — the same
/// rule the derived index records.
fn enclosing_revision(event: &ShoreEvent) -> Result<Option<RevisionId>> {
    event
        .subject_revision_id()
        .map_err(|error| undecodable(event, &error))
}

fn undecodable(event: &ShoreEvent, error: &dyn std::fmt::Display) -> ShoreError {
    ShoreError::InvalidEvent {
        message: format!(
            "review summary cannot read event {}: its {} payload does not decode: {error}",
            event.event_id.as_str(),
            event.event_type.as_str(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical_hash::sha256_json_prefixed;
    use crate::model::{
        ActorId, AssessmentId, ChangeId, CommitAssociationId, EngagementId, JournalId, ObjectId,
        ObservationId, ReviewEndpoint, ReviewTargetRef, RevisionId, Side, WorkObjectId,
    };
    use crate::session::event::{
        EventPayload, EventTarget, ReviewAssessment, ReviewAssessmentRecordedPayload, Revision,
        RevisionCommitAssociatedPayload, WorkObjectProposal, WorkObjectProposedPayload, Writer,
        WriterProducer, build_membership_asserted, build_membership_withdrawn,
    };
    use crate::session::workflow::review_summary::model::{
        Count, FirstCaptureResults, Measure, RoundCount, RoundProfile, compute_review_summary,
    };

    fn writer(actor: &str) -> Writer {
        Writer {
            actor_id: ActorId::new(actor.to_owned()),
            producer: WriterProducer {
                name: "pointbreak".to_owned(),
                version: String::new(),
            },
        }
    }

    fn event<P: EventPayload>(key: &str, actor: &str, payload: P, occurred_at: &str) -> ShoreEvent {
        ShoreEvent::new(
            payload.event_type(),
            key,
            EventTarget::for_journal(JournalId::new("journal:default")),
            writer(actor),
            payload,
            occurred_at,
        )
        .unwrap()
    }

    fn rev(suffix: &str) -> RevisionId {
        RevisionId::new(format!("rev:sha256:{suffix}"))
    }

    fn membership(key: &str, change: &str, revision: &str, nonce: u8) -> ShoreEvent {
        let payload = build_membership_asserted(
            &ChangeId::new(change.to_owned()),
            &rev(revision),
            [nonce; 32],
        )
        .unwrap();
        event(key, "actor:author", payload, "2026-06-01T00:00:00Z")
    }

    fn capture(revision: &str, actor: &str, occurred_at: &str) -> ShoreEvent {
        capture_proposal(
            &format!("capture:{revision}:{occurred_at}"),
            actor,
            WorkObjectProposal::Revision {
                revision: Revision {
                    id: rev(revision),
                    object_id: ObjectId::new(format!("obj:sha256:{revision}")),
                    git_provenance: None,
                },
                summary: None,
                object_artifact_content_hash: format!("sha256:artifact:{revision}"),
                supersedes: Vec::new(),
            },
            occurred_at,
        )
    }

    fn capture_proposal(
        key: &str,
        actor: &str,
        work_object: WorkObjectProposal,
        occurred_at: &str,
    ) -> ShoreEvent {
        event(
            key,
            actor,
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new("engagement:sha256:e".to_owned()),
                work_object,
            },
            occurred_at,
        )
    }

    fn assessment(
        key: &str,
        target: ReviewTargetRef,
        verdict: ReviewAssessment,
        replaces: &[&str],
    ) -> ShoreEvent {
        event(
            key,
            "actor:reviewer",
            ReviewAssessmentRecordedPayload {
                assessment_id: AssessmentId::new(format!("assess:sha256:{key}")),
                target,
                assessment: verdict,
                summary: None,
                summary_content_type: Default::default(),
                summary_artifact_path: None,
                summary_byte_size: None,
                summary_content_hash: None,
                replaces_assessment_ids: replaces
                    .iter()
                    .map(|id| AssessmentId::new(format!("assess:sha256:{id}")))
                    .collect(),
                related_observation_ids: Vec::new(),
                related_input_request_ids: Vec::new(),
            },
            "2026-06-02T00:00:00Z",
        )
    }

    fn revision_target(revision: &str) -> ReviewTargetRef {
        ReviewTargetRef::Revision {
            revision_id: rev(revision),
        }
    }

    fn association(key: &str, revision: &str) -> ShoreEvent {
        event(
            key,
            "actor:author",
            RevisionCommitAssociatedPayload {
                commit_association_id: CommitAssociationId::new(format!(
                    "commit-association:sha256:{key}"
                )),
                target: revision_target(revision),
                commit: ReviewEndpoint::GitCommit {
                    commit_oid: "c0ffee".to_owned(),
                    tree_oid: "c0ffee-tree".to_owned(),
                },
            },
            "2026-06-03T00:00:00Z",
        )
    }

    fn source_of(event: &ShoreEvent) -> CountedEventRef {
        CountedEventRef {
            event_id: event.event_id.as_str().to_owned(),
            payload_hash: event.payload_hash.clone(),
        }
    }

    fn inputs(events: &[ShoreEvent]) -> ReviewSummaryInputs {
        review_summary_inputs_from_events(events, &BackfillCohort::empty()).unwrap()
    }

    #[test]
    fn each_mapped_event_yields_one_row_with_its_source() {
        let claim = membership("m1", "change:c1", "r1", 1);
        let claim_id = serde_json::from_value::<
            crate::session::event::ChangeMembershipAssertedPayload,
        >(claim.payload.clone())
        .unwrap()
        .membership_claim_id;
        let withdrawal = event(
            "w1",
            "actor:author",
            build_membership_withdrawn(&claim_id, [2; 32]).unwrap(),
            "2026-06-01T00:00:01Z",
        );
        let captured = capture("r1", "actor:author", "2026-06-01T00:00:02Z");
        let assessed = assessment(
            "a1",
            revision_target("r1"),
            ReviewAssessment::Accepted,
            &["a0"],
        );
        let associated = association("x1", "r1");

        let rows = inputs(&[
            claim.clone(),
            withdrawal.clone(),
            captured.clone(),
            assessed.clone(),
            associated.clone(),
        ]);

        assert_eq!(rows.memberships.len(), 1);
        let row = &rows.memberships[0];
        assert_eq!(row.source, source_of(&claim));
        assert_eq!(row.claim_id, claim_id.as_str());
        assert_eq!(row.change_id, "change:c1");
        assert_eq!(row.revision_id, rev("r1").as_str());
        assert!(!row.backfill);

        assert_eq!(rows.withdrawals.len(), 1);
        assert_eq!(rows.withdrawals[0].source, source_of(&withdrawal));
        assert_eq!(rows.withdrawals[0].claim_id, claim_id.as_str());

        assert_eq!(rows.captures.len(), 1);
        assert_eq!(rows.captures[0].source, source_of(&captured));
        assert_eq!(rows.captures[0].revision_id, rev("r1").as_str());
        assert_eq!(rows.captures[0].actor, "actor:author");

        assert_eq!(rows.assessments.len(), 1);
        let row = &rows.assessments[0];
        assert_eq!(row.source, source_of(&assessed));
        assert_eq!(row.assessment_id, "assess:sha256:a1");
        assert_eq!(row.revision_id.as_deref(), Some(rev("r1").as_str()));
        assert_eq!(row.actor, "actor:reviewer");
        assert_eq!(row.verdict, ReviewAssessment::Accepted);
        assert_eq!(row.replaces, vec!["assess:sha256:a0".to_owned()]);

        assert_eq!(rows.commit_associations.len(), 1);
        assert_eq!(rows.commit_associations[0].source, source_of(&associated));
        assert_eq!(rows.commit_associations[0].revision_id, rev("r1").as_str());
        assert_eq!(rows.manifest_hash, None);
    }

    #[test]
    fn task_attempt_proposal_yields_no_capture_row() {
        let proposal = capture_proposal(
            "task-capture",
            "actor:author",
            WorkObjectProposal::TaskAttempt {
                task_attempt_id: WorkObjectId::new("task-attempt:sha256:t"),
                project_path: "/repo".to_owned(),
                claude_session_uuid: "session".to_owned(),
                initial_prompt_hash: "sha256:prompt".to_owned(),
                predecessor: None,
                base_state_fingerprint: None,
                source_speaker: None,
            },
            "2026-06-01T00:00:00Z",
        );

        assert!(inputs(&[proposal]).captures.is_empty());
    }

    #[test]
    fn scoped_assessments_land_on_their_enclosing_revision() {
        let scoped = [
            ReviewTargetRef::File {
                revision_id: rev("r1"),
                file_path: "src/lib.rs".to_owned(),
            },
            ReviewTargetRef::Range {
                revision_id: rev("r1"),
                file_path: "src/lib.rs".to_owned(),
                side: Side::New,
                start_line: 1,
                end_line: 2,
            },
            ReviewTargetRef::Observation {
                revision_id: rev("r1"),
                observation_id: ObservationId::new("obs:sha256:o".to_owned()),
            },
        ];
        let whole = inputs(&[assessment(
            "whole",
            revision_target("r1"),
            ReviewAssessment::NeedsChanges,
            &[],
        )]);
        for (index, target) in scoped.into_iter().enumerate() {
            let rows = inputs(&[assessment(
                &format!("scoped-{index}"),
                target,
                ReviewAssessment::NeedsChanges,
                &[],
            )]);
            assert_eq!(
                rows.assessments[0].revision_id,
                whole.assessments[0].revision_id
            );
        }
    }

    #[test]
    fn undecodable_mapped_payload_fails_the_read_and_names_the_event() {
        let mut broken = assessment("a1", revision_target("r1"), ReviewAssessment::Accepted, &[]);
        broken.payload = serde_json::json!({});
        broken.payload_hash = sha256_json_prefixed(&broken.payload).unwrap();
        let good = capture("r1", "actor:author", "2026-06-01T00:00:00Z");

        let error =
            review_summary_inputs_from_events(&[good, broken.clone()], &BackfillCohort::empty())
                .expect_err("an undecodable assessment payload fails the read");

        assert!(
            error.to_string().contains(broken.event_id.as_str()),
            "error names the event: {error}"
        );
    }

    #[test]
    fn legacy_and_rfc3339_capture_instants_order_as_instants() {
        let legacy = capture("r-legacy", "actor:author", "unix-ms:1760000000000");
        let modern = capture("r-modern", "actor:author", "2025-10-09T08:53:20.001Z");

        let rows = inputs(&[modern, legacy]);
        let instant_of = |revision: &str| {
            rows.captures
                .iter()
                .find(|row| row.revision_id == rev(revision).as_str())
                .and_then(|row| row.captured_at_millis)
                .unwrap()
        };

        assert_eq!(instant_of("r-legacy"), 1_760_000_000_000);
        assert_eq!(instant_of("r-modern"), 1_760_000_000_001);
    }

    #[test]
    fn membership_in_the_cohort_is_backfill() {
        let backfill = membership("bulk-key", "change:c1", "r1", 1);
        let ordinary = membership("ordinary-key", "change:c1", "r1", 1);
        let cohort = BackfillCohort::from_event_ids_for_test(
            [backfill.event_id.as_str().to_owned()],
            Some("sha256:manifest".to_owned()),
        );

        let rows =
            review_summary_inputs_from_events(&[backfill.clone(), ordinary.clone()], &cohort)
                .unwrap();

        let flag = |event: &ShoreEvent| {
            rows.memberships
                .iter()
                .find(|row| row.source.event_id == event.event_id.as_str())
                .unwrap()
                .backfill
        };
        assert!(flag(&backfill));
        assert!(!flag(&ordinary));
        assert_eq!(rows.manifest_hash.as_deref(), Some("sha256:manifest"));
    }

    #[test]
    fn five_events_fold_into_the_hand_computed_summary() {
        let events = [
            membership("m1", "change:c1", "r1", 1),
            membership("m2", "change:c1", "r2", 2),
            capture("r1", "actor:author", "2026-06-01T00:00:00Z"),
            capture("r2", "actor:author", "2026-06-02T00:00:00Z"),
            assessment(
                "a1",
                revision_target("r1"),
                ReviewAssessment::NeedsChanges,
                &[],
            ),
        ];

        let summary = compute_review_summary(&inputs(&events));

        assert_eq!(summary.population.changes_counted, 1);
        assert_eq!(
            summary.profile,
            Measure::Computed(RoundProfile {
                of: 1,
                distribution: vec![RoundCount {
                    rounds: 2,
                    changes: 1
                }],
            })
        );
        assert_eq!(
            summary.first_capture,
            Measure::Computed(FirstCaptureResults {
                of: 1,
                needs_changes: 1,
                ..FirstCaptureResults::default()
            })
        );
        assert_eq!(
            summary.assessed_capture,
            Measure::Computed(Count { count: 1, of: 2 })
        );
        assert_eq!(
            summary.commit_associated_capture,
            Measure::Computed(Count { count: 0, of: 2 })
        );
        assert_eq!(summary.counted_inputs().len(), 5);
    }
}
