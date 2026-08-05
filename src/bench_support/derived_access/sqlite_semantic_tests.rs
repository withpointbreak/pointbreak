use crate::bench_support::derived_access::DerivedStorageLayout;
use crate::bench_support::derived_access::adapter::QualificationDerivedAccessAdapter;
use crate::bench_support::derived_access::sqlite_cursor::{
    CursorLedgerIdentity, SqliteCursorLedger,
};
use crate::bench_support::longitudinal::LongitudinalCountingScopeV1;
use crate::crypto::SignerId;
use crate::model::{
    AssessmentId, ChangeIdentityDescriptorV1, CheckpointId, EngagementId, InputRequestId,
    InputRequestResponseId, JournalId, ObjectId, ObservationId, ReviewEndpoint, ReviewTargetRef,
    RevisionId, RevisionSource, TargetRef, TaskTargetRef, TrackId, ValidationCheckId,
    ValidationStatus, ValidationTarget, ValidationTrigger, WorkObjectId, WorkObjectType,
    WorktreeCaptureMode,
};
use crate::session::EventStore;
use crate::session::derived_access::cursor::{AppendResolution, TruthCursor};
use crate::session::derived_access::locator::LocatorRead;
use crate::session::derived_access::oracle::{
    strict_bodyless_materialized_engagement_snapshot, strict_bodyless_materialized_snapshot,
    strict_bodyless_semantic_snapshot,
};
use crate::session::event::{
    ArtifactRemovedPayload, BodyContentType, EventPayload, EventSignature,
    EventSignatureRecordedPayload, EventTarget, EventType, GitProvenance,
    InputRequestOpenedPayload, InputRequestReasonCode, InputRequestRespondedPayload,
    InputRequestResponseOutcome, ReviewAssessment, ReviewAssessmentRecordedPayload,
    ReviewInitializedPayload, ReviewObservationRecordedPayload, Revision,
    RevisionCommitAssociatedPayload, RevisionCommitWithdrawnPayload, RevisionRefAssociatedPayload,
    RevisionRefWithdrawnPayload, ShoreEvent, TaskCheckpointCapturedPayload,
    TaskObservationRecordedPayload, ValidationCheckRecordedPayload, WorkObjectProposal,
    WorkObjectProposedPayload, Writer, build_change_declared, build_membership_asserted,
    build_membership_withdrawn,
};

const STORE_ID: &str = "store:semantic-test";
const JOURNAL: &str = "journal:sha256:semantic-test";
const TRACK: &str = "agent:semantic-test";
const ENGAGEMENT: &str = "engagement:sha256:semantic-test";

fn derived_database(root: &std::path::Path) -> std::path::PathBuf {
    DerivedStorageLayout::resolve(root)
        .expect("derived layout")
        .root()
        .join("cursor.sqlite3")
}

fn revision_id(suffix: &str) -> RevisionId {
    RevisionId::new(format!("rev:sha256:{suffix}"))
}

fn open_adapter(root: &std::path::Path) -> QualificationDerivedAccessAdapter {
    SqliteCursorLedger::initialize_empty(root, CursorLedgerIdentity::new(STORE_ID))
        .expect("initialize cursor");
    QualificationDerivedAccessAdapter::open(root, CursorLedgerIdentity::new(STORE_ID))
        .expect("open adapter")
}

fn append(adapter: &QualificationDerivedAccessAdapter, event: &ShoreEvent, attempt: usize) {
    assert!(matches!(
        adapter
            .append_event(event, &format!("semantic-attempt:{attempt}"))
            .expect("append and index"),
        AppendResolution::Created(_)
    ));
}

fn ready<T>(read: LocatorRead<T>) -> T {
    match read {
        LocatorRead::Ready(value) => value,
        LocatorRead::CatchUpRequired { applied, observed } => {
            panic!("unexpected lag: applied={applied:?}, observed={observed:?}")
        }
    }
}

fn initialized_event(journal_id: &str) -> ShoreEvent {
    let journal_id = JournalId::new(journal_id);
    ShoreEvent::new(
        EventType::ReviewInitialized,
        ReviewInitializedPayload::idempotency_key(&journal_id),
        EventTarget::for_journal(journal_id),
        Writer::shore_local("0.8.0"),
        ReviewInitializedPayload {},
        "2026-07-27T00:00:00.000Z",
    )
    .expect("initialized event")
}

fn revision_event(suffix: &str, supersedes: Vec<RevisionId>, occurred_at: &str) -> ShoreEvent {
    revision_event_for_engagement(suffix, supersedes, occurred_at, ENGAGEMENT)
}

fn revision_event_for_engagement(
    suffix: &str,
    supersedes: Vec<RevisionId>,
    occurred_at: &str,
    engagement_id: &str,
) -> ShoreEvent {
    let revision_id = revision_id(suffix);
    ShoreEvent::new(
        EventType::WorkObjectProposed,
        format!("work_object_proposed:{}", revision_id.as_str()),
        EventTarget::for_revision(
            JournalId::new(JOURNAL),
            revision_id.clone(),
            Some(TrackId::new(TRACK)),
        )
        .expect("revision target"),
        Writer::shore_local("0.8.0"),
        WorkObjectProposedPayload {
            engagement_id: EngagementId::new(engagement_id),
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
    .expect("revision event")
}

fn revision_target(revision_id: &RevisionId) -> EventTarget {
    EventTarget::for_subject(
        JournalId::new(JOURNAL),
        TargetRef::Review(ReviewTargetRef::Revision {
            revision_id: revision_id.clone(),
        }),
        Some(TrackId::new(TRACK)),
    )
    .expect("review target")
}

fn assessment_event(
    revision_id: &RevisionId,
    source_key: &str,
    assessment_id: &str,
    assessment: ReviewAssessment,
    replaces: Vec<AssessmentId>,
    summary: Option<&str>,
    occurred_at: &str,
) -> ShoreEvent {
    ShoreEvent::new(
        EventType::ReviewAssessmentRecorded,
        ReviewAssessmentRecordedPayload::idempotency_key(
            revision_id,
            &TrackId::new(TRACK),
            source_key,
        ),
        revision_target(revision_id),
        Writer::shore_local("0.8.0"),
        ReviewAssessmentRecordedPayload {
            assessment_id: AssessmentId::new(assessment_id),
            target: ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            },
            assessment,
            summary: summary.map(str::to_owned),
            summary_content_type: BodyContentType::TextPlain,
            summary_artifact_path: None,
            summary_byte_size: summary.map(|value| value.len() as u64),
            summary_content_hash: None,
            replaces_assessment_ids: replaces,
            related_observation_ids: Vec::new(),
            related_input_request_ids: Vec::new(),
        },
        occurred_at,
    )
    .expect("assessment event")
}

fn request_opened(revision_id: &RevisionId, request_id: &InputRequestId) -> ShoreEvent {
    ShoreEvent::new(
        EventType::InputRequestOpened,
        InputRequestOpenedPayload::idempotency_key(
            revision_id,
            &TrackId::new(TRACK),
            request_id.as_str(),
        ),
        revision_target(revision_id),
        Writer::shore_local("0.8.0"),
        InputRequestOpenedPayload {
            input_request_id: request_id.clone(),
            target: ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            },
            task_target: None,
            reason_code: InputRequestReasonCode::ManualDecisionRequired,
            title: "PUBLIC REQUEST TITLE SENTINEL".to_owned(),
            body: Some("PRIVATE REQUEST BODY".to_owned()),
            body_content_type: BodyContentType::TextPlain,
            body_artifact_path: None,
            body_byte_size: Some(20),
            body_content_hash: None,
            target_fingerprint: None,
        },
        "2026-07-27T16:00:04Z",
    )
    .expect("request event")
}

fn request_responded(revision_id: &RevisionId, request_id: &InputRequestId) -> ShoreEvent {
    let response_id = InputRequestResponseId::new("input-response:sha256:semantic-test");
    ShoreEvent::new(
        EventType::InputRequestResponded,
        InputRequestRespondedPayload::idempotency_key(request_id, response_id.as_str()),
        EventTarget::for_subject(
            JournalId::new(JOURNAL),
            TargetRef::Review(ReviewTargetRef::InputRequest {
                revision_id: revision_id.clone(),
                input_request_id: request_id.clone(),
            }),
            Some(TrackId::new(TRACK)),
        )
        .expect("response target"),
        Writer::shore_local("0.8.0"),
        InputRequestRespondedPayload {
            input_request_response_id: response_id,
            input_request_id: request_id.clone(),
            revision_id: Some(revision_id.clone()),
            task_target: None,
            outcome: InputRequestResponseOutcome::Approved,
            reason: Some("PRIVATE RESPONSE BODY".to_owned()),
            reason_content_type: BodyContentType::TextPlain,
            reason_artifact_path: None,
            reason_byte_size: Some(21),
            reason_content_hash: None,
            target_fingerprint: None,
        },
        "2026-07-27T16:00:05Z",
    )
    .expect("response event")
}

fn validation_event(revision_id: &RevisionId) -> ShoreEvent {
    ShoreEvent::new(
        EventType::ValidationCheckRecorded,
        ValidationCheckRecordedPayload::idempotency_key(
            revision_id,
            &TrackId::new(TRACK),
            "semantic-test",
        ),
        revision_target(revision_id),
        Writer::shore_local("0.8.0"),
        ValidationCheckRecordedPayload {
            validation_check_id: ValidationCheckId::new("validation:sha256:semantic-test"),
            target: ValidationTarget::Revision {
                revision_id: revision_id.clone(),
            },
            check_name: "PUBLIC VALIDATION NAME SENTINEL".to_owned(),
            command: None,
            status: ValidationStatus::Failed,
            exit_code: Some(1),
            trigger: ValidationTrigger::Manual,
            source_fingerprint: None,
            summary: Some("PRIVATE VALIDATION SUMMARY".to_owned()),
            summary_content_type: BodyContentType::TextPlain,
            summary_artifact_path: None,
            summary_byte_size: Some(26),
            summary_content_hash: None,
            started_at: None,
            completed_at: Some("2026-07-27T16:00:06Z".to_owned()),
            log_artifact_content_hashes: Vec::new(),
        },
        "2026-07-27T16:00:06Z",
    )
    .expect("validation event")
}

fn association_event(revision_id: &RevisionId) -> ShoreEvent {
    let commit_oid = "0123456789abcdef0123456789abcdef01234567";
    ShoreEvent::new(
        EventType::RevisionCommitAssociated,
        RevisionCommitAssociatedPayload::idempotency_key(revision_id, commit_oid),
        revision_target(revision_id),
        Writer::shore_local("0.8.0"),
        RevisionCommitAssociatedPayload {
            commit_association_id: crate::model::CommitAssociationId::new(
                "assoc-commit:sha256:semantic-test",
            ),
            target: ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            },
            commit: ReviewEndpoint::GitCommit {
                commit_oid: commit_oid.to_owned(),
                tree_oid: "tree-semantic-test".to_owned(),
            },
        },
        "2026-07-27T16:00:07Z",
    )
    .expect("association event")
}

fn commit_withdrawal_event(revision_id: &RevisionId) -> ShoreEvent {
    ShoreEvent::new(
        EventType::RevisionCommitWithdrawn,
        "revision_commit_withdrawn:assoc-commit:sha256:semantic-test",
        revision_target(revision_id),
        Writer::shore_local("0.8.0"),
        RevisionCommitWithdrawnPayload {
            commit_withdrawal_id: crate::model::CommitWithdrawalId::new(
                "withdraw-commit:sha256:semantic-test",
            ),
            target: ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            },
            commit_association_id: crate::model::CommitAssociationId::new(
                "assoc-commit:sha256:semantic-test",
            ),
        },
        "2026-07-27T16:00:08Z",
    )
    .expect("commit withdrawal")
}

fn ref_events(revision_id: &RevisionId) -> [ShoreEvent; 2] {
    let association_id = crate::model::RefAssociationId::new("assoc-ref:sha256:semantic-test");
    let associated = ShoreEvent::new(
        EventType::RevisionRefAssociated,
        "revision_ref_associated:semantic-test",
        revision_target(revision_id),
        Writer::shore_local("0.8.0"),
        RevisionRefAssociatedPayload {
            ref_association_id: association_id.clone(),
            target: ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            },
            ref_name: "refs/heads/main".to_owned(),
            head_oid: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        },
        "2026-07-27T16:00:09Z",
    )
    .expect("ref association");
    let withdrawn = ShoreEvent::new(
        EventType::RevisionRefWithdrawn,
        "revision_ref_withdrawn:semantic-test",
        revision_target(revision_id),
        Writer::shore_local("0.8.0"),
        RevisionRefWithdrawnPayload {
            ref_withdrawal_id: crate::model::RefWithdrawalId::new(
                "withdraw-ref:sha256:semantic-test",
            ),
            target: ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            },
            ref_association_id: association_id,
        },
        "2026-07-27T16:00:10Z",
    )
    .expect("ref withdrawal");
    [associated, withdrawn]
}

fn observation_event(revision_id: &RevisionId) -> ShoreEvent {
    ShoreEvent::new(
        EventType::ReviewObservationRecorded,
        "review_observation_recorded:semantic-test",
        revision_target(revision_id),
        Writer::shore_local("0.8.0"),
        ReviewObservationRecordedPayload {
            observation_id: ObservationId::new("obs:sha256:semantic-test"),
            target: ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            },
            title: "Semantic observation".to_owned(),
            body: Some("PRIVATE OBSERVATION BODY".to_owned()),
            body_content_type: BodyContentType::TextPlain,
            body_artifact_path: None,
            body_byte_size: Some(24),
            body_content_hash: None,
            tags: Vec::new(),
            confidence: None,
            supersedes_observation_ids: Vec::new(),
            responds_to_observation_ids: Vec::new(),
        },
        "2026-07-27T16:00:11Z",
    )
    .expect("observation")
}

fn task_checkpoint_event() -> ShoreEvent {
    let task = WorkObjectId::new("task-attempt:sha256:semantic-test");
    let checkpoint = CheckpointId::new("checkpoint:sha256:semantic-test");
    ShoreEvent::new(
        EventType::TaskCheckpointCaptured,
        TaskCheckpointCapturedPayload::idempotency_key_for_work_object(
            &task,
            WorkObjectType::TaskAttempt,
            checkpoint.as_str(),
        ),
        EventTarget::for_subject(
            JournalId::new(JOURNAL),
            TargetRef::Task(TaskTargetRef::Checkpoint {
                checkpoint_id: checkpoint.clone(),
            }),
            None,
        )
        .expect("task target"),
        Writer::shore_local("0.8.0"),
        TaskCheckpointCapturedPayload {
            checkpoint_id: checkpoint,
            parent_task_attempt_id: task,
            assistant_message_id: "message-semantic-test".to_owned(),
            tool_use_ids: Vec::new(),
            checkpoint_fingerprint: None,
            source_speaker: None,
        },
        "2026-07-27T16:00:12Z",
    )
    .expect("task checkpoint")
}

fn task_events() -> [ShoreEvent; 2] {
    let task = WorkObjectId::new("task-attempt:sha256:semantic-test");
    let target = EventTarget::for_subject(
        JournalId::new(JOURNAL),
        TargetRef::Task(TaskTargetRef::TaskAttempt {
            task_attempt_id: task.clone(),
        }),
        None,
    )
    .expect("task attempt target");
    let proposed = ShoreEvent::new(
        EventType::WorkObjectProposed,
        format!("work_object_proposed:{}", task.as_str()),
        target.clone(),
        Writer::shore_local("0.8.0"),
        WorkObjectProposedPayload {
            engagement_id: EngagementId::new("engagement:sha256:semantic-task"),
            work_object: WorkObjectProposal::TaskAttempt {
                task_attempt_id: task.clone(),
                project_path: "/public/generated".to_owned(),
                claude_session_uuid: "semantic-test".to_owned(),
                initial_prompt_hash: "sha256:semantic-test".to_owned(),
                predecessor: None,
                base_state_fingerprint: None,
                source_speaker: None,
            },
        },
        "2026-07-27T16:00:13Z",
    )
    .expect("task proposal");
    let observed = ShoreEvent::new(
        EventType::TaskObservationRecorded,
        TaskObservationRecordedPayload::idempotency_key_for_work_object(
            &task,
            WorkObjectType::TaskAttempt,
            "semantic-observation",
        ),
        target,
        Writer::shore_local("0.8.0"),
        TaskObservationRecordedPayload {
            observation_id: ObservationId::new("obs:sha256:semantic-task"),
            checkpoint_id: None,
            title: "Task observation".to_owned(),
            body: Some("PRIVATE TASK BODY".to_owned()),
            body_artifact_path: None,
            body_byte_size: Some(17),
            body_content_hash: None,
            source_speaker: None,
        },
        "2026-07-27T16:00:14Z",
    )
    .expect("task observation");
    [proposed, observed]
}

fn signature_event() -> ShoreEvent {
    let signer = SignerId::parse("did:key:z6MkehRgf7yJbgaGfYsdoAsKdBPE3dj2CYhowQdcjqSJgvVd")
        .expect("signer");
    let signature =
        "EzOVlqmX/g3nHametOmU067NsuvweZEwo73/cOypvT2KfCtNK6BfxsWJQ7Ox9E/MtunGEkJGEMSfn/qdmKSFAg==";
    let payload = EventSignatureRecordedPayload {
        target_event_id: crate::model::EventId::new("evt:sha256:semantic-target"),
        target_event_record_hash: "sha256:semantic-record".to_owned(),
        attesting_signer: signer.clone(),
        attestation: EventSignature::new_ed25519_v1(signature).expect("signature"),
        inclusion_proof: None,
    };
    ShoreEvent::new(
        EventType::EventSignatureRecorded,
        EventSignatureRecordedPayload::idempotency_key(
            &payload.target_event_record_hash,
            &signer,
            signature,
        ),
        EventTarget::for_journal(JournalId::new(JOURNAL)),
        Writer::shore_local("0.8.0"),
        payload,
        "2026-07-27T16:00:15Z",
    )
    .expect("signature event")
}

fn removal_event(content_hash: &str) -> ShoreEvent {
    ShoreEvent::new(
        EventType::ArtifactRemoved,
        ArtifactRemovedPayload::idempotency_key(content_hash),
        EventTarget::for_journal(JournalId::new(JOURNAL)),
        Writer::shore_local("0.8.0"),
        ArtifactRemovedPayload {
            content_hash: content_hash.to_owned(),
        },
        "2026-07-27T16:00:08Z",
    )
    .expect("removal event")
}

fn required_schedule() -> Vec<ShoreEvent> {
    let a = revision_id("a");
    let b = revision_id("b");
    let request_id = InputRequestId::new("input-request:sha256:semantic-test");
    let mut events = vec![
        ShoreEvent::new(
            EventType::ReviewInitialized,
            ReviewInitializedPayload::idempotency_key(&JournalId::new(JOURNAL)),
            EventTarget::for_journal(JournalId::new(JOURNAL)),
            Writer::shore_local("0.8.0"),
            ReviewInitializedPayload {},
            "2026-07-27T16:00:00Z",
        )
        .expect("review initialized"),
        revision_event("a", Vec::new(), "2026-07-27T16:00:02Z"),
        revision_event("b", vec![a.clone()], "2026-07-27T16:00:01Z"),
        revision_event("c", vec![a], "2026-07-27T16:00:02Z"),
        assessment_event(
            &b,
            "assessment-one",
            "assess:sha256:one",
            ReviewAssessment::NeedsChanges,
            Vec::new(),
            Some("PRIVATE ASSESSMENT SUMMARY"),
            "2026-07-27T16:00:03Z",
        ),
        assessment_event(
            &b,
            "assessment-two",
            "assess:sha256:two",
            ReviewAssessment::Accepted,
            vec![AssessmentId::new("assess:sha256:one")],
            None,
            "2026-07-27T16:00:04Z",
        ),
        request_opened(&b, &request_id),
        request_responded(&b, &request_id),
        validation_event(&b),
        association_event(&b),
        commit_withdrawal_event(&b),
        observation_event(&b),
        task_checkpoint_event(),
        signature_event(),
        removal_event("sha256:artifact:b"),
    ];
    events.extend(ref_events(&b));
    events.extend(task_events());
    events
}

fn change_schedule() -> Vec<ShoreEvent> {
    let revision = revision_id("change-member");
    let descriptor = ChangeIdentityDescriptorV1::opaque_nonce([41; 32]);
    let declaration = build_change_declared(descriptor, [42; 32]).unwrap();
    let membership =
        build_membership_asserted(&declaration.change_id, &revision, [43; 32]).unwrap();
    let withdrawal = build_membership_withdrawn(&membership.membership_claim_id, [44; 32]).unwrap();
    let assessment_id = "assess:sha256:change-duplicate";
    vec![
        revision_event_for_engagement(
            "change-member",
            Vec::new(),
            "2026-08-04T00:00:00Z",
            ENGAGEMENT,
        ),
        change_event(1, declaration),
        change_event(2, membership),
        assessment_event(
            &revision,
            "change-assessment-first",
            assessment_id,
            ReviewAssessment::Accepted,
            Vec::new(),
            Some("PRIVATE CHANGE ASSESSMENT SUMMARY"),
            "2026-08-04T00:00:03Z",
        ),
        assessment_event(
            &revision,
            "change-assessment-second",
            assessment_id,
            ReviewAssessment::Accepted,
            Vec::new(),
            None,
            "2026-08-04T00:00:04Z",
        ),
        change_event(5, withdrawal),
    ]
}

fn change_event<P: EventPayload>(index: usize, payload: P) -> ShoreEvent {
    ShoreEvent::new(
        payload.event_type(),
        format!("change-semantic:{index}"),
        EventTarget::for_journal(JournalId::new(JOURNAL)),
        Writer::shore_local("0.9.0"),
        payload,
        format!("2026-08-04T00:00:{index:02}Z"),
    )
    .unwrap()
}

#[test]
fn sqlite_change_facts_match_strict_replay_without_storing_event_bodies() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let schedule = change_schedule();
    let mut stored = Vec::new();

    for (attempt, event) in schedule.iter().enumerate() {
        append(&adapter, event, attempt);
        stored.push(event.clone());
        let candidate = ready(adapter.semantic_audit_snapshot().expect("Change audit"));
        let materialized = ready(
            adapter
                .semantic_materialized_audit_snapshot()
                .expect("materialized Change audit"),
        );
        assert_eq!(
            candidate,
            strict_bodyless_semantic_snapshot(&stored).unwrap()
        );
        assert_eq!(
            materialized,
            strict_bodyless_materialized_snapshot(&stored).unwrap()
        );
    }

    let final_snapshot = ready(adapter.semantic_audit_snapshot().unwrap());
    assert_eq!(final_snapshot.changes.changes.len(), 1);
    assert!(
        !std::fs::read(derived_database(root.path()))
            .unwrap()
            .windows(b"PRIVATE CHANGE ASSESSMENT SUMMARY".len())
            .any(|window| window == b"PRIVATE CHANGE ASSESSMENT SUMMARY"),
        "the SQLite carrier retains compact facts, not public event envelopes"
    );
}

#[test]
fn selected_engagement_retains_store_wide_change_semantics() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let mut schedule = change_schedule();
    let other_revision = revision_id("other-change-member");
    let other_declaration =
        build_change_declared(ChangeIdentityDescriptorV1::opaque_nonce([51; 32]), [52; 32])
            .unwrap();
    let other_membership =
        build_membership_asserted(&other_declaration.change_id, &other_revision, [53; 32]).unwrap();
    schedule.extend([
        revision_event_for_engagement(
            "other-change-member",
            Vec::new(),
            "2026-08-04T00:01:00Z",
            "engagement:sha256:other-change",
        ),
        change_event(11, other_declaration),
        change_event(12, other_membership),
    ]);
    for (attempt, event) in schedule.iter().enumerate() {
        append(&adapter, event, attempt);
    }

    let selected = ready(
        adapter
            .semantic_materialized_engagement_snapshot(ENGAGEMENT)
            .unwrap(),
    );
    let strict = strict_bodyless_materialized_engagement_snapshot(&schedule, ENGAGEMENT).unwrap();
    assert_eq!(selected, strict);
    assert_eq!(selected.changes.changes.len(), 2);
}

#[test]
fn incremental_semantic_snapshot_equals_strict_full_replay_after_every_prefix() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let schedule = required_schedule();
    let mut stored = Vec::new();

    for (attempt, event) in schedule.iter().enumerate() {
        append(&adapter, event, attempt);
        stored.push(event.clone());
        let candidate = ready(
            adapter
                .semantic_audit_snapshot()
                .expect("candidate snapshot"),
        );
        let strict = strict_bodyless_semantic_snapshot(&stored).expect("strict snapshot");
        assert_eq!(candidate, strict, "prefix {}", attempt + 1);
    }
}

#[test]
fn materialized_semantic_families_equal_strict_full_replay_after_every_prefix() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let schedule = required_schedule();
    let mut stored = Vec::new();

    for (attempt, event) in schedule.iter().enumerate() {
        append(&adapter, event, attempt);
        stored.push(event.clone());
        let candidate = ready(
            adapter
                .semantic_materialized_audit_snapshot()
                .expect("materialized candidate"),
        );
        let strict =
            strict_bodyless_materialized_snapshot(&stored).expect("materialized strict oracle");
        assert_eq!(candidate, strict, "materialized prefix {}", attempt + 1);

        let selected = ready(
            adapter
                .semantic_materialized_engagement_snapshot(ENGAGEMENT)
                .expect("selected candidate"),
        );
        let selected_strict = strict_bodyless_materialized_engagement_snapshot(&stored, ENGAGEMENT)
            .expect("selected strict oracle");
        assert_eq!(
            selected,
            selected_strict,
            "selected materialized prefix {}",
            attempt + 1
        );
    }
}

#[test]
fn materialized_journal_identity_follows_canonical_replay_order() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let schedule = [
        initialized_event("journal:derived-access-scale:0"),
        initialized_event("journal:derived-access-scale:1"),
    ];
    let mut stored = Vec::new();

    for (attempt, event) in schedule.iter().enumerate() {
        append(&adapter, event, attempt);
        stored.push(event.clone());
        let candidate = ready(
            adapter
                .semantic_materialized_audit_snapshot()
                .expect("materialized candidate"),
        );
        let strict =
            strict_bodyless_materialized_snapshot(&stored).expect("materialized strict oracle");
        assert_eq!(candidate, strict, "materialized prefix {}", attempt + 1);
    }
}

#[test]
fn selected_engagement_omits_unrelated_materialized_history() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let events = vec![
        revision_event_for_engagement(
            "unrelated",
            Vec::new(),
            "2026-07-27T15:00:00Z",
            "engagement:sha256:unrelated",
        ),
        revision_event("selected", Vec::new(), "2026-07-27T16:00:00Z"),
    ];
    for (attempt, event) in events.iter().enumerate() {
        append(&adapter, event, attempt);
    }

    let selected = ready(
        adapter
            .semantic_materialized_engagement_snapshot(ENGAGEMENT)
            .expect("selected engagement"),
    );
    assert_eq!(
        selected,
        strict_bodyless_materialized_engagement_snapshot(&events, ENGAGEMENT)
            .expect("selected engagement oracle")
    );
    let selected_documents = format!("{} {}", selected.revisions, selected.threads);
    assert!(
        !selected_documents.contains("rev:sha256:unrelated"),
        "selected engagement leaked an unrelated revision"
    );
}

#[test]
fn canonical_earlier_semantic_fact_replaces_a_later_representative() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let revision = revision_id("first-seen");
    append(
        &adapter,
        &revision_event("first-seen", Vec::new(), "2026-07-27T16:00:00Z"),
        0,
    );

    let mut candidates = [
        assessment_event(
            &revision,
            "z-source",
            "assess:sha256:shared",
            ReviewAssessment::Accepted,
            Vec::new(),
            None,
            "2026-07-27T16:00:02Z",
        ),
        assessment_event(
            &revision,
            "a-source",
            "assess:sha256:shared",
            ReviewAssessment::NeedsChanges,
            Vec::new(),
            None,
            "2026-07-27T16:00:03Z",
        ),
    ];
    let expected = if candidates[0].event_id < candidates[1].event_id {
        ReviewAssessment::Accepted
    } else {
        ReviewAssessment::NeedsChanges
    };
    let mut stored = vec![revision_event(
        "first-seen",
        Vec::new(),
        "2026-07-27T16:00:00Z",
    )];
    candidates.sort_by(|left, right| right.event_id.cmp(&left.event_id));
    for (offset, event) in candidates.iter().enumerate() {
        append(&adapter, event, offset + 1);
        stored.push(event.clone());
    }

    let snapshot = ready(
        adapter
            .semantic_materialized_engagement_snapshot(ENGAGEMENT)
            .expect("materialized snapshot"),
    );
    let assessment = snapshot
        .attention
        .current_assessments
        .iter()
        .find(|record| record.assessment_id == "assess:sha256:shared")
        .expect("current representative");
    assert_eq!(assessment.assessment, expected);
    assert_eq!(
        snapshot,
        strict_bodyless_materialized_engagement_snapshot(&stored, ENGAGEMENT)
            .expect("first-seen oracle")
    );
}

#[test]
fn semantic_delta_and_locator_checkpoint_commit_atomically() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let event = revision_event("atomic", Vec::new(), "2026-07-27T16:00:00Z");
    SqliteCursorLedger::open(root.path(), CursorLedgerIdentity::new(STORE_ID))
        .expect("cursor")
        .append_event(&event, "truth-only")
        .expect("publish truth");

    adapter
        .catch_up_with_interruption(32)
        .expect_err("injected semantic failure");
    assert_eq!(
        adapter.locator_checkpoint().expect("locator checkpoint"),
        TruthCursor::new(1, 0)
    );
    assert!(matches!(
        adapter.semantic_audit_snapshot().expect("lag result"),
        LocatorRead::CatchUpRequired {
            applied: TruthCursor { sequence: 0, .. },
            observed: TruthCursor { sequence: 1, .. }
        }
    ));

    adapter.catch_up_to_head(32).expect("retry complete");
    assert_eq!(
        adapter.locator_checkpoint().expect("locator checkpoint"),
        TruthCursor::new(1, 1)
    );
    assert_eq!(
        ready(adapter.semantic_audit_snapshot().expect("snapshot"))
            .state
            .event_count,
        1
    );
}

#[test]
fn semantic_sidecar_is_bodyless_and_restart_stable() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let schedule = required_schedule();
    for (attempt, event) in schedule.iter().enumerate() {
        append(&adapter, event, attempt);
    }
    let before = ready(adapter.semantic_audit_snapshot().expect("before restart"));
    drop(adapter);

    let reopened =
        QualificationDerivedAccessAdapter::open(root.path(), CursorLedgerIdentity::new(STORE_ID))
            .expect("reopen adapter");
    let after = ready(reopened.semantic_audit_snapshot().expect("after restart"));
    assert_eq!(before, after);

    let sidecar = derived_database(root.path());
    let connection = rusqlite::Connection::open(&sidecar).expect("open semantic sidecar");
    let representative_columns = connection
        .prepare("PRAGMA table_info(semantic_representative)")
        .expect("prepare representative columns")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query representative columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("read representative columns");
    assert_eq!(
        representative_columns,
        vec![
            "family_id",
            "semantic_key_prefix_id",
            "semantic_key_digest",
            "semantic_key_raw",
            "semantic_key_hash",
            "sequence",
        ]
    );
    let (representatives, raw_keys, canonical_keys, invalid_digests) = connection
        .query_row(
            "SELECT count(*),
                    count(*) FILTER (WHERE semantic_key_raw IS NOT NULL),
                    count(*) FILTER (WHERE semantic_key_prefix_id IS NOT NULL),
                    count(*) FILTER (WHERE length(semantic_key_digest) != 32)
             FROM semantic_representative",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .expect("read representative identity shape");
    assert!(representatives > 0);
    assert_eq!(raw_keys + canonical_keys, representatives);
    assert_eq!(invalid_digests, 0);
    drop(connection);
    let mut bytes = std::fs::read(&sidecar).expect("read sidecar");
    for suffix in ["-wal", "-shm"] {
        let path = std::path::PathBuf::from(format!("{}{suffix}", sidecar.display()));
        if let Ok(mut auxiliary) = std::fs::read(path) {
            bytes.append(&mut auxiliary);
        }
    }
    for forbidden in [
        b"PRIVATE REQUEST BODY".as_slice(),
        b"PRIVATE RESPONSE BODY".as_slice(),
        b"PRIVATE VALIDATION SUMMARY".as_slice(),
        b"PRIVATE ASSESSMENT SUMMARY".as_slice(),
        b"PRIVATE OBSERVATION BODY".as_slice(),
        b"PRIVATE TASK BODY".as_slice(),
    ] {
        assert!(
            !bytes
                .windows(forbidden.len())
                .any(|window| window == forbidden),
            "semantic sidecar persisted forbidden body text"
        );
    }
    for required in [
        b"PUBLIC REQUEST TITLE SENTINEL".as_slice(),
        b"PUBLIC VALIDATION NAME SENTINEL".as_slice(),
    ] {
        assert!(
            bytes
                .windows(required.len())
                .any(|window| window == required),
            "semantic sidecar omitted an authorized short label"
        );
    }
}

#[test]
fn removed_revision_detail_keeps_facts_without_hydrating_removed_content() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let capture = revision_event("removed", Vec::new(), "2026-07-27T16:00:00Z");
    let removed_hash = "sha256:artifact:removed";
    append(&adapter, &capture, 0);
    append(&adapter, &removal_event(removed_hash), 1);

    let scope = LongitudinalCountingScopeV1::new("5".repeat(64)).expect("scope");
    let detail = {
        let _guard = scope.enter();
        ready(
            adapter
                .revision_detail(&revision_id("removed"))
                .expect("revision detail"),
        )
        .expect("known revision")
    };
    assert!(detail.object_content_removed);
    assert_eq!(detail.object_content_hash, removed_hash);
    assert_eq!(detail.authoritative_events, vec![capture]);
    assert_eq!(scope.snapshot().counters.body_artifact_reads, 0);
    assert_eq!(scope.snapshot().counters.object_artifact_reads, 0);
}

#[test]
fn selected_semantic_fact_revalidates_authoritative_event_meaning() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let capture = revision_event("witness", Vec::new(), "2026-07-27T16:00:00Z");
    append(&adapter, &capture, 0);

    let path =
        EventStore::open(root.path()).event_path_for_idempotency_key(&capture.idempotency_key);
    let mut changed = capture;
    changed.occurred_at = "2026-07-27T16:00:01Z".to_owned();
    std::fs::write(path, serde_json::to_vec(&changed).expect("changed carrier"))
        .expect("replace carrier");

    assert!(
        adapter.revision_detail(&revision_id("witness")).is_err(),
        "semantic rows never bypass authoritative event validation"
    );
}

#[test]
fn append_restart_and_selected_detail_do_not_rebuild_full_projections() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let capture = revision_event("bounded", Vec::new(), "2026-07-27T16:00:00Z");
    let scope = LongitudinalCountingScopeV1::new("6".repeat(64)).expect("scope");
    let inventory = {
        let _guard = scope.enter();
        append(&adapter, &capture, 0);
        ready(
            adapter
                .revision_detail(&revision_id("bounded"))
                .expect("bounded detail"),
        )
        .expect("known revision");
        ready(
            adapter
                .semantic_materialized_engagement_snapshot(ENGAGEMENT)
                .expect("bounded materialized families"),
        );
        drop(adapter);
        let reopened = QualificationDerivedAccessAdapter::open(
            root.path(),
            CursorLedgerIdentity::new(STORE_ID),
        )
        .expect("restart adapter");
        ready(
            reopened
                .semantic_materialized_engagement_snapshot(ENGAGEMENT)
                .expect("restarted materialized families"),
        );
        reopened.semantic_inventory().expect("semantic inventory")
    };
    let counters = scope.snapshot().counters;
    assert_eq!(counters.projection_rebuilds, 0);
    assert_eq!(counters.state_rebuilds, 0);
    assert_eq!(counters.event_folds, 0);
    assert_eq!(
        inventory.profile_id,
        "pointbreak.sqlite-derived-access-semantic.v1"
    );
    assert_eq!(inventory.schema_version, 7);
    assert_eq!(inventory.fact_count, 1);
    assert_eq!(inventory.retained_body_object_bytes, 0);
    assert_eq!(
        inventory.tables,
        vec![
            "semantic_actor",
            "semantic_assessment_fact",
            "semantic_change_fact",
            "semantic_commit_association_fact",
            "semantic_commit_withdrawal_fact",
            "semantic_duplicate_projection",
            "semantic_event_fact",
            "semantic_identity_prefix",
            "semantic_meta",
            "semantic_ref_association_fact",
            "semantic_ref_withdrawal_fact",
            "semantic_representative",
            "semantic_request_fact",
            "semantic_response_fact",
            "semantic_revision_fact",
            "semantic_state_projection",
            "semantic_validation_fact",
        ]
    );
    assert_eq!(
        inventory.indexes,
        vec![
            "semantic_event_fact_content",
            "semantic_event_fact_revision",
        ]
    );
    for forbidden in [
        "body",
        "summary",
        "reason",
        "artifact_path",
        "trust_generation",
        "git_reachability",
        "bodyless_event_json",
        "epoch",
        "logical_reread_key",
        "replay_key",
        "event_id",
        "event_type",
        "journal_id",
        "payload_hash",
        "track_id",
        "validation_witness",
    ] {
        assert!(
            !inventory.columns.iter().any(|column| column == forbidden),
            "forbidden semantic column {forbidden}"
        );
    }
}

#[test]
fn materialized_audit_looks_up_representatives_by_sequence() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    append(
        &adapter,
        &revision_event("audit-plan", Vec::new(), "2026-07-27T16:00:01Z"),
        1,
    );
    drop(adapter);

    let connection =
        rusqlite::Connection::open(derived_database(root.path())).expect("open sidecar");
    let mut statement = connection
        .prepare(
            "EXPLAIN QUERY PLAN
             SELECT representative.sequence
             FROM locator_event_text AS locator
             JOIN semantic_event_fact_text AS event ON event.sequence = locator.sequence
             JOIN cursor_receipt_text AS receipt ON receipt.sequence = event.sequence
             JOIN semantic_representative AS representative
               ON representative.sequence = event.sequence
             WHERE locator.epoch = 1
               AND event.sequence <= 1
               AND representative.family_id != 2
             ORDER BY locator.replay_key, receipt.logical_reread_key_hash",
        )
        .expect("prepare audit plan");
    let details = statement
        .query_map([], |row| row.get::<_, String>(3))
        .expect("query audit plan")
        .collect::<Result<Vec<_>, _>>()
        .expect("read audit plan");

    assert!(
        details.iter().any(|detail| {
            detail.contains("SEARCH representative")
                && detail.contains("semantic_representative_sequence")
                && detail.contains("sequence=?")
        }),
        "materialized audit must use a representative sequence lookup: {details:?}"
    );
    assert!(
        !details
            .iter()
            .any(|detail| detail.contains("SCAN representative")),
        "materialized audit must not rescan all representatives per event: {details:?}"
    );
}

#[test]
fn semantic_inventory_measures_retained_body_or_object_payload_columns() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    drop(adapter);
    let database = derived_database(root.path());
    let connection = rusqlite::Connection::open(database).expect("open sidecar");
    connection
        .execute_batch(
            "CREATE TABLE qualification_payload_probe (
                 id INTEGER PRIMARY KEY,
                 body_bytes BLOB NOT NULL
             ) STRICT;
             INSERT INTO qualification_payload_probe(body_bytes) VALUES (x'01020304');",
        )
        .expect("install payload probe");
    drop(connection);
    let reopened =
        QualificationDerivedAccessAdapter::open(root.path(), CursorLedgerIdentity::new(STORE_ID))
            .expect("reopen adapter");
    assert_eq!(
        reopened
            .semantic_inventory()
            .expect("semantic inventory")
            .retained_body_object_bytes,
        4
    );
}

#[test]
fn append_restart_and_detail_counters_are_flat_after_prior_history() {
    fn measured(prior_events: usize) -> crate::bench_support::longitudinal::LongitudinalCountersV1 {
        let root = tempfile::tempdir().expect("root");
        let adapter = open_adapter(root.path());
        for index in 0..prior_events {
            append(
                &adapter,
                &removal_event(&format!("sha256:prior-{index:04}")),
                index,
            );
        }
        let scope = LongitudinalCountingScopeV1::new("7".repeat(64)).expect("scope");
        {
            let _guard = scope.enter();
            append(
                &adapter,
                &revision_event("flat", Vec::new(), "2026-07-27T17:00:00Z"),
                prior_events,
            );
            drop(adapter);
            let reopened = QualificationDerivedAccessAdapter::open(
                root.path(),
                CursorLedgerIdentity::new(STORE_ID),
            )
            .expect("restart adapter");
            ready(
                reopened
                    .semantic_materialized_engagement_snapshot(ENGAGEMENT)
                    .expect("flat materialized families"),
            );
            ready(
                reopened
                    .revision_detail(&revision_id("flat"))
                    .expect("flat detail"),
            )
            .expect("known revision");
        }
        scope.snapshot().counters
    }

    let empty = measured(0);
    let retained = measured(64);
    assert_eq!(retained.event_folds, empty.event_folds);
    assert_eq!(retained.projection_rebuilds, empty.projection_rebuilds);
    assert_eq!(retained.state_rebuilds, empty.state_rebuilds);
    assert_eq!(retained.carrier_opens, empty.carrier_opens);
    assert_eq!(retained.event_decodes, empty.event_decodes);
    assert_eq!(retained.event_validations, empty.event_validations);
}

#[test]
fn full_audits_are_counted_while_selected_materialized_reads_are_not() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    for (attempt, event) in required_schedule().iter().enumerate() {
        append(&adapter, event, attempt);
    }

    let bounded = LongitudinalCountingScopeV1::new("8".repeat(64)).expect("bounded scope");
    {
        let _guard = bounded.enter();
        ready(
            adapter
                .semantic_materialized_engagement_snapshot(ENGAGEMENT)
                .expect("materialized families"),
        );
    }
    let bounded = bounded.snapshot().counters;
    assert_eq!(bounded.projection_rebuilds, 0);
    assert_eq!(bounded.state_rebuilds, 0);
    assert_eq!(bounded.event_folds, 0);

    let materialized_audit =
        LongitudinalCountingScopeV1::new("9".repeat(64)).expect("materialized audit scope");
    {
        let _guard = materialized_audit.enter();
        ready(
            adapter
                .semantic_materialized_audit_snapshot()
                .expect("materialized audit snapshot"),
        );
    }
    let materialized_audit = materialized_audit.snapshot().counters;
    assert!(materialized_audit.projection_rebuilds > 0);
    assert_eq!(materialized_audit.state_rebuilds, 0);
    assert!(materialized_audit.event_folds > 0);

    let audit = LongitudinalCountingScopeV1::new("a".repeat(64)).expect("audit scope");
    {
        let _guard = audit.enter();
        ready(adapter.semantic_audit_snapshot().expect("audit snapshot"));
    }
    let audit = audit.snapshot().counters;
    assert!(audit.projection_rebuilds > 0);
    assert!(audit.state_rebuilds > 0);
    assert!(audit.event_folds > 0);
}
