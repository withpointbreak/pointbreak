use crate::bench_support::derived_access::DerivedStorageLayout;
use crate::bench_support::derived_access::adapter::QualificationDerivedAccessAdapter;
use crate::bench_support::derived_access::sqlite_cursor::{
    CursorLedgerIdentity, SqliteCursorLedger,
};
use crate::bench_support::derived_access::sqlite_locator::ProposalCarrierLocator;
use crate::bench_support::longitudinal::LongitudinalCountingScopeV1;
use crate::crypto::SignerId;
use crate::model::{
    AssessmentId, ChangeIdentityDescriptorV1, CheckpointId, CommitAssociationId, EngagementId,
    InputRequestId, InputRequestResponseId, JournalId, ObjectId, ObservationId, ReviewEndpoint,
    ReviewTargetRef, RevisionId, RevisionRefV1, RevisionSource, TargetRef, TaskTargetRef, TrackId,
    ValidationCheckId, ValidationStatus, ValidationTarget, ValidationTrigger, WorkObjectId,
    WorkObjectType, WorktreeCaptureMode,
};
use crate::session::derived_access::cursor::{AppendResolution, TruthCursor};
use crate::session::derived_access::locator::LocatorRead;
use crate::session::derived_access::oracle::{
    strict_bodyless_materialized_engagement_snapshot, strict_bodyless_materialized_snapshot,
    strict_bodyless_semantic_snapshot,
};
use crate::session::event::{
    ArtifactRemovedPayload, BodyContentType, EventPayload, EventSignature,
    EventSignatureRecordedPayload, EventTarget, EventType, FactPortRelationV1, FactRefV1,
    GitProvenance, InputRequestOpenedPayload, InputRequestReasonCode, InputRequestRespondedPayload,
    InputRequestResponseOutcome, RelationProofStatusV1, ReviewAssessment,
    ReviewAssessmentRecordedPayload, ReviewFactPortDraftV1, ReviewInitializedPayload,
    ReviewObservationRecordedPayload, Revision, RevisionCommitAssociatedPayload,
    RevisionCommitWithdrawnPayload, RevisionRefAssociatedPayload, RevisionRefWithdrawnPayload,
    RevisionRelationAttestationDraftV1, SemanticRevisionRelationV1, ShoreEvent,
    TaskCheckpointCapturedPayload, TaskObservationRecordedPayload, ValidationCheckRecordedPayload,
    WorkObjectProposal, WorkObjectProposedPayload, Writer, build_change_declared,
    build_change_link_asserted, build_membership_asserted, build_membership_withdrawn,
    build_review_fact_ported, build_revision_relation_asserted, build_revision_relation_attested,
    build_revision_relation_withdrawn,
};
use crate::session::{AuthorityCursorV2, EventStore, TrustSet, project_change_documents};

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

fn valid_hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
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

fn proposal_carrier_event(
    exact: &RevisionRefV1,
    summary: Option<&str>,
    idempotency_key: &str,
    occurred_at: &str,
) -> ShoreEvent {
    proposal_carrier_event_with_supersedes(exact, summary, idempotency_key, occurred_at, Vec::new())
}

fn proposal_carrier_event_with_supersedes(
    exact: &RevisionRefV1,
    summary: Option<&str>,
    idempotency_key: &str,
    occurred_at: &str,
    supersedes: Vec<RevisionId>,
) -> ShoreEvent {
    ShoreEvent::new(
        EventType::WorkObjectProposed,
        idempotency_key,
        EventTarget::for_revision(
            JournalId::new(JOURNAL),
            exact.revision_id.clone(),
            Some(TrackId::new(TRACK)),
        )
        .expect("proposal target"),
        Writer::shore_local("0.9.0"),
        WorkObjectProposedPayload {
            engagement_id: EngagementId::new(ENGAGEMENT),
            work_object: WorkObjectProposal::Revision {
                revision: Revision {
                    id: exact.revision_id.clone(),
                    object_id: ObjectId::new("obj:sha256:proposal-carrier"),
                    git_provenance: None,
                },
                summary: summary.map(str::to_owned),
                object_artifact_content_hash: exact.object_artifact_content_hash.clone(),
                supersedes,
            },
        },
        occurred_at,
    )
    .expect("proposal carrier")
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
    keyed_observation_event(revision_id, "semantic-test", "2026-07-27T16:00:11Z")
}

fn keyed_observation_event(
    revision_id: &RevisionId,
    source_key: &str,
    occurred_at: &str,
) -> ShoreEvent {
    ShoreEvent::new(
        EventType::ReviewObservationRecorded,
        format!("review_observation_recorded:{source_key}"),
        revision_target(revision_id),
        Writer::shore_local("0.8.0"),
        ReviewObservationRecordedPayload {
            observation_id: ObservationId::new(format!("obs:sha256:{source_key}")),
            target: ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            },
            title: "Semantic observation".to_owned(),
            body: Some("PRIVATE OBSERVATION BODY".to_owned()),
            body_content_type: BodyContentType::TextPlain,
            body_artifact_path: None,
            body_byte_size: Some(24),
            body_content_hash: None,
            tags: vec!["Issue:Semantic-History".to_owned()],
            confidence: None,
            supersedes_observation_ids: Vec::new(),
            responds_to_observation_ids: Vec::new(),
        },
        occurred_at,
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

#[test]
fn product_history_v3_relations_preserve_bodyless_selection_and_checkpoint_behavior() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let schedule = required_schedule();
    for (attempt, event) in schedule.iter().enumerate() {
        append(&adapter, event, attempt);
    }
    drop(adapter);

    let database = derived_database(root.path());
    let connection = rusqlite::Connection::open(&database).expect("open sidecar");
    let (profile, schema_version, product_applied, semantic_applied, locator_applied) = connection
        .query_row(
            "SELECT product.profile_id, product.schema_version, product.applied_sequence,
                    semantic.applied_sequence, locator.applied_sequence
             FROM product_history_meta AS product
             CROSS JOIN semantic_meta AS semantic
             CROSS JOIN locator_checkpoint AS locator
             WHERE product.singleton = 1
               AND semantic.singleton = 1
               AND locator.singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .expect("read shared checkpoint");
    assert_eq!(profile, "pointbreak.sqlite-derived-access-history.v1");
    assert!(schema_version >= 3);
    assert_eq!(product_applied, schedule.len() as i64);
    assert_eq!(semantic_applied, product_applied);
    assert_eq!(locator_applied, product_applied);

    let chronology = connection
        .prepare(
            "SELECT normalized_occurred_at, event_id
             FROM locator_event_text
             ORDER BY normalized_occurred_at, event_id",
        )
        .expect("prepare chronology")
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("query chronology")
        .collect::<Result<Vec<_>, _>>()
        .expect("read chronology");
    assert_eq!(chronology.len(), schedule.len());
    assert!(chronology.windows(2).all(|pair| pair[0] <= pair[1]));

    let selected = connection
        .query_row(
            "SELECT locator.event_type, locator.track_id, event.actor_id, event.revision_id
             FROM semantic_event_fact_text AS event
             JOIN locator_event_text AS locator ON locator.sequence = event.sequence
             WHERE locator.event_type = 'review_assessment_recorded'
             ORDER BY locator.sequence
             LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .expect("read bodyless selection fields");
    assert_eq!(selected.0, "review_assessment_recorded");
    assert_eq!(selected.1, TRACK);
    assert!(!selected.2.is_empty());
    assert_eq!(selected.3, revision_id("b").as_str());

    assert_eq!(
        connection
            .query_row("SELECT tag_key FROM product_history_tag", [], |row| row
                .get::<_, String>(
                0
            ),)
            .expect("read tag facet"),
        "issue"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT target_event_id FROM product_history_signature",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("read detached signature target"),
        "evt:sha256:semantic-target"
    );
    let revision = connection
        .query_row(
            "SELECT revision.revision_id, revision.captured_at,
                    edge.superseded_revision_id
             FROM product_revision AS revision
             JOIN product_revision_edge AS edge ON edge.sequence = revision.sequence
             WHERE revision.revision_id = ?1",
            [revision_id("b").as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .expect("read captured Revision edge");
    assert_eq!(revision.0, revision_id("b").as_str());
    assert_eq!(revision.1, "2026-07-27T16:00:01Z");
    assert_eq!(revision.2, revision_id("a").as_str());
    drop(connection);

    let reopened =
        QualificationDerivedAccessAdapter::open(root.path(), CursorLedgerIdentity::new(STORE_ID))
            .expect("reopen adapter");
    assert_eq!(
        reopened.locator_checkpoint().expect("restarted checkpoint"),
        TruthCursor::new(1, schedule.len() as u64)
    );
}

#[test]
fn timeline_revision_references_keep_direct_bindings_and_recompute_ambiguous_candidates() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let candidate_id = revision_id("timeline-candidate");
    let first = RevisionRefV1::new(candidate_id.clone(), valid_hash('a')).expect("first exact");
    let second = RevisionRefV1::new(candidate_id.clone(), valid_hash('b')).expect("second exact");
    let first_proposal = proposal_carrier_event(
        &first,
        None,
        "work_object_proposed:timeline-candidate:first",
        "2026-08-18T12:00:00Z",
    );
    let observed = observation_event(&candidate_id);
    let missing_id = revision_id("timeline-missing");
    let missing =
        keyed_observation_event(&missing_id, "timeline-missing", "2026-08-18T12:00:00.500Z");
    append(&adapter, &first_proposal, 0);
    append(&adapter, &observed, 1);
    append(&adapter, &missing, 2);

    let database = derived_database(root.path());
    let connection = rusqlite::Connection::open(&database).expect("open sidecar");
    let resolved = connection
        .query_row(
            "SELECT reference_role, resolution, object_artifact_content_hash
             FROM product_history_revision_reference AS reference
             JOIN locator_event_text AS locator ON locator.sequence = reference.sequence
             WHERE locator.event_id = ?1 AND reference.source_kind = 'review_target'",
            [observed.event_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .expect("read singleton candidate");
    assert_eq!(
        resolved,
        (
            "candidate".to_owned(),
            "exact".to_owned(),
            Some(valid_hash('a'))
        )
    );
    let unresolved = connection
        .query_row(
            "SELECT reference_role, resolution, object_artifact_content_hash
             FROM product_history_revision_reference AS reference
             JOIN locator_event_text AS locator ON locator.sequence = reference.sequence
             WHERE locator.event_id = ?1 AND reference.source_kind = 'review_target'",
            [missing.event_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .expect("read missing candidate");
    assert_eq!(
        unresolved,
        ("candidate".to_owned(), "unresolved".to_owned(), None)
    );
    drop(connection);

    let conflicting_proposal = proposal_carrier_event(
        &second,
        None,
        "work_object_proposed:timeline-candidate:second",
        "2026-08-18T12:00:01Z",
    );
    append(&adapter, &conflicting_proposal, 3);
    let connection = rusqlite::Connection::open(database).expect("reopen sidecar");
    let ambiguous = connection
        .query_row(
            "SELECT resolution, object_artifact_content_hash
             FROM product_history_revision_reference AS reference
             JOIN locator_event_text AS locator ON locator.sequence = reference.sequence
             WHERE locator.event_id = ?1 AND reference.source_kind = 'review_target'",
            [observed.event_id.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .expect("read ambiguous candidate");
    assert_eq!(ambiguous, ("unresolved".to_owned(), None));
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM semantic_revision_proposal_carrier
                 WHERE revision_id = ?1",
                [candidate_id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .expect("count all proposal bindings"),
        2
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*)
                 FROM product_history_revision_reference
                 WHERE source_kind = 'proposal'
                   AND reference_role = 'direct'
                   AND resolution = 'exact'
                   AND revision_id = ?1",
                [candidate_id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .expect("count direct proposal references"),
        2,
        "a later conflict must not rewrite either event's direct exact reference"
    );
}

#[test]
fn overlapping_revision_roles_deduplicate_one_membership_correlation() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let revision = revision_id("timeline-overlap");
    let exact = RevisionRefV1::new(revision.clone(), valid_hash('9')).expect("exact Revision");
    let proposal = proposal_carrier_event_with_supersedes(
        &exact,
        None,
        "work_object_proposed:timeline-overlap",
        "2026-08-18T12:00:10Z",
        vec![revision.clone()],
    );
    let declaration =
        build_change_declared(ChangeIdentityDescriptorV1::opaque_nonce([49; 32]), [50; 32])
            .expect("declaration");
    let membership =
        build_membership_asserted(&declaration.change_id, &revision, [51; 32]).expect("membership");
    append(&adapter, &proposal, 0);
    append(&adapter, &change_event(1, declaration), 1);
    append(&adapter, &change_event(2, membership.clone()), 2);

    let connection =
        rusqlite::Connection::open(derived_database(root.path())).expect("open sidecar");
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*)
                 FROM product_history_change_correlation AS correlation
                 JOIN locator_event_text AS locator ON locator.sequence = correlation.sequence
                 WHERE locator.event_id = ?1
                   AND correlation.change_id = ?2
                   AND correlation.correlation_role = 'historical'
                   AND correlation.source_kind = 'membership_claim'
                   AND correlation.source_id = ?3",
                rusqlite::params![
                    proposal.event_id.as_str(),
                    membership.change_id.as_str(),
                    membership.membership_claim_id.as_str(),
                ],
                |row| row.get::<_, i64>(0),
            )
            .expect("overlapping-role correlation count"),
        1,
        "one event/revision pair must produce one correlation per membership claim"
    );
}

#[test]
fn later_membership_and_withdrawal_materialize_historical_change_support() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let member_id = revision_id("timeline-member");
    let exact = RevisionRefV1::new(member_id.clone(), valid_hash('c')).expect("exact Revision");
    let proposal = proposal_carrier_event(
        &exact,
        None,
        "work_object_proposed:timeline-member",
        "2026-08-18T12:01:00Z",
    );
    let observed = observation_event(&member_id);
    let declaration =
        build_change_declared(ChangeIdentityDescriptorV1::opaque_nonce([51; 32]), [52; 32])
            .expect("declaration");
    let membership = build_membership_asserted(&declaration.change_id, &member_id, [53; 32])
        .expect("membership");
    let withdrawal =
        build_membership_withdrawn(&membership.membership_claim_id, [54; 32]).expect("withdrawal");
    let declaration_event = change_event(2, declaration.clone());
    let membership_event = change_event(3, membership.clone());
    let withdrawal_event = change_event(4, withdrawal);
    for (attempt, event) in [&proposal, &observed, &declaration_event]
        .into_iter()
        .enumerate()
    {
        append(&adapter, event, attempt);
    }

    let database = derived_database(root.path());
    let connection = rusqlite::Connection::open(&database).expect("open pre-claim sidecar");
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*)
                 FROM product_history_change_correlation AS correlation
                 JOIN locator_event_text AS locator ON locator.sequence = correlation.sequence
                 WHERE locator.event_id = ?1",
                [observed.event_id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .expect("pre-claim correlation count"),
        0,
        "a declaration alone is not historical Revision membership"
    );
    drop(connection);

    append(&adapter, &withdrawal_event, 3);
    append(&adapter, &membership_event, 4);
    let relation = build_revision_relation_asserted(
        &declaration.change_id,
        exact.clone(),
        RevisionRefV1::new(revision_id("timeline-member-predecessor"), valid_hash('d'))
            .expect("relation predecessor"),
        [55; 32],
    )
    .expect("relation");
    let relation_withdrawal =
        build_revision_relation_withdrawn(&relation.relation_claim_id, [56; 32])
            .expect("relation withdrawal");
    let relation_event = change_event(5, relation.clone());
    let relation_withdrawal_event = change_event(6, relation_withdrawal);
    append(&adapter, &relation_withdrawal_event, 5);
    append(&adapter, &relation_event, 6);

    let connection = rusqlite::Connection::open(database).expect("open sidecar");
    let membership_sequence = connection
        .query_row(
            "SELECT sequence FROM locator_event_text WHERE event_id = ?1",
            [membership_event.event_id.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .expect("membership sequence");
    let historical = connection
        .query_row(
            "SELECT correlation_role, source_kind, source_id, support_sequence
             FROM product_history_change_correlation AS correlation
             JOIN locator_event_text AS locator ON locator.sequence = correlation.sequence
             WHERE locator.event_id = ?1 AND correlation.change_id = ?2",
            [observed.event_id.as_str(), declaration.change_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .expect("historical Change correlation");
    assert_eq!(historical.0, "historical");
    assert_eq!(historical.1, "membership_claim");
    assert_eq!(historical.2, membership.membership_claim_id.as_str());
    assert_eq!(historical.3, membership_sequence);

    let withdrawal_support = connection
        .query_row(
            "SELECT correlation_role, source_id, support_sequence
             FROM product_history_change_correlation AS correlation
             JOIN locator_event_text AS locator ON locator.sequence = correlation.sequence
             WHERE locator.event_id = ?1 AND correlation.change_id = ?2",
            [
                withdrawal_event.event_id.as_str(),
                declaration.change_id.as_str(),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .expect("withdrawal Change support");
    assert_eq!(withdrawal_support.0, "direct");
    assert_eq!(
        withdrawal_support.1,
        membership.membership_claim_id.as_str()
    );
    assert_eq!(withdrawal_support.2, membership_sequence);
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM product_history_membership_withdrawal
                 WHERE claim_id = ?1",
                [membership.membership_claim_id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .expect("withdrawal claim identity"),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM product_history_change_correlation
                 WHERE sequence = (
                     SELECT sequence FROM locator_event_text WHERE event_id = ?1
                 ) AND change_id = ?2
                   AND correlation_role = 'historical'
                   AND source_kind = 'membership_claim'",
                [observed.event_id.as_str(), declaration.change_id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .expect("withdrawn history remains correlated"),
        1
    );
    let relation_sequence = connection
        .query_row(
            "SELECT sequence FROM locator_event_text WHERE event_id = ?1",
            [relation_event.event_id.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .expect("relation sequence");
    let relation_history = connection
        .query_row(
            "SELECT source_id, support_sequence
             FROM product_history_change_correlation AS correlation
             JOIN locator_event_text AS locator ON locator.sequence = correlation.sequence
             WHERE locator.event_id = ?1
               AND correlation.change_id = ?2
               AND correlation.correlation_role = 'historical'
               AND correlation.source_kind = 'relation_claim'",
            [observed.event_id.as_str(), declaration.change_id.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .expect("historical relation correlation");
    assert_eq!(relation_history.0, relation.relation_claim_id.as_str());
    assert_eq!(relation_history.1, relation_sequence);
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*)
                 FROM product_history_change_correlation AS correlation
                 JOIN locator_event_text AS locator ON locator.sequence = correlation.sequence
                 WHERE locator.event_id = ?1
                   AND correlation.source_id = ?2
                   AND correlation.correlation_role = 'direct'
                   AND correlation.support_sequence = ?3",
                rusqlite::params![
                    relation_withdrawal_event.event_id.as_str(),
                    relation.relation_claim_id.as_str(),
                    relation_sequence,
                ],
                |row| row.get::<_, i64>(0),
            )
            .expect("relation withdrawal support"),
        1
    );
}

#[test]
fn change_capable_timeline_families_and_structured_facets_are_bodyless_and_indexed() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let first = build_change_declared(ChangeIdentityDescriptorV1::opaque_nonce([61; 32]), [62; 32])
        .expect("first declaration");
    let second =
        build_change_declared(ChangeIdentityDescriptorV1::opaque_nonce([63; 32]), [64; 32])
            .expect("second declaration");
    let revision = revision_id("timeline-family");
    let exact = RevisionRefV1::new(revision.clone(), valid_hash('d')).expect("exact Revision");
    let predecessor = RevisionRefV1::new(revision_id("timeline-predecessor"), valid_hash('e'))
        .expect("predecessor");
    let membership =
        build_membership_asserted(&first.change_id, &revision, [65; 32]).expect("membership");
    let membership_withdrawal =
        build_membership_withdrawn(&membership.membership_claim_id, [66; 32])
            .expect("membership withdrawal");
    let link = build_change_link_asserted(
        &first.change_id,
        &second.change_id,
        crate::session::event::ChangeLinkRelationV1::RelatedWork,
        [67; 32],
    )
    .expect("link");
    let relation =
        build_revision_relation_asserted(&first.change_id, exact.clone(), predecessor, [68; 32])
            .expect("relation");
    let relation_withdrawal =
        build_revision_relation_withdrawn(&relation.relation_claim_id, [69; 32])
            .expect("relation withdrawal");
    let attestation = build_revision_relation_attested(RevisionRelationAttestationDraftV1 {
        revision: exact.clone(),
        commit_association_id: CommitAssociationId::new(
            "commit-association:sha256:timeline-family",
        ),
        semantic_relation: SemanticRevisionRelationV1::Unknown,
        proof_status: RelationProofStatusV1::Unverified,
        proof_method: "manual".to_owned(),
        proof_algorithm_version: "v1".to_owned(),
        capture_scope: vec!["worktree".to_owned()],
        comparison_base_or_parent: None,
        endpoint_oids: vec!["abc".to_owned()],
        evidence_content_hash: None,
        result_digest: valid_hash('f'),
    })
    .expect("attestation");
    let track_id = TrackId::new(TRACK);
    let writer = Writer::shore_local("0.9.0");
    let fact_port = build_review_fact_ported(
        ReviewFactPortDraftV1 {
            origin_revision: exact.clone(),
            origin_fact: FactRefV1::Observation {
                observation_id: ObservationId::new("obs:sha256:timeline-origin"),
            },
            target_revision: RevisionRefV1::new(revision_id("timeline-target"), valid_hash('0'))
                .expect("target Revision"),
            relation: FactPortRelationV1::ContextOnly,
            target_fact: None,
            rationale_content_hash: None,
            context_change_id: Some(first.change_id.clone()),
        },
        &writer.actor_id,
        &track_id,
    )
    .expect("fact port");
    let fact_port_event = ShoreEvent::new(
        EventType::ReviewFactPorted,
        "review_fact_ported:timeline-family",
        EventTarget::for_revision(JournalId::new(JOURNAL), revision.clone(), Some(track_id))
            .expect("fact port target"),
        writer,
        fact_port,
        "2026-08-18T12:02:09Z",
    )
    .expect("fact port event");
    let request_id = InputRequestId::new("input-request:sha256:timeline-family");
    let proposal = proposal_carrier_event(
        &exact,
        None,
        "work_object_proposed:timeline-family",
        "2026-08-18T12:02:00Z",
    );
    let assessment = assessment_event(
        &revision,
        "timeline-family",
        "assess:sha256:timeline-family",
        ReviewAssessment::NeedsChanges,
        Vec::new(),
        Some("BODYLESS TIMELINE ASSESSMENT SENTINEL"),
        "2026-08-18T12:02:12Z",
    );
    let mut events = vec![
        proposal,
        change_event(10, first.clone()),
        change_event(11, second),
        change_event(12, membership),
        change_event(13, membership_withdrawal),
        change_event(14, link),
        change_event(15, relation),
        change_event(16, relation_withdrawal),
        change_event(17, attestation),
        fact_port_event,
        observation_event(&revision),
        assessment,
        request_opened(&revision, &request_id),
        request_responded(&revision, &request_id),
        validation_event(&revision),
        task_checkpoint_event(),
        signature_event(),
        removal_event("sha256:timeline-family-removed"),
    ];
    events.extend(task_events());
    for (attempt, event) in events.iter().enumerate() {
        append(&adapter, event, attempt);
    }

    let strict_changes = project_change_documents(&events).expect("strict Change documents");
    let strict = crate::session::project_event_history(
        &events,
        &strict_changes,
        AuthorityCursorV2 {
            schema: "pointbreak.authority-cursor.v2".to_owned(),
            journal_record_count: events.len() as u64,
            event_count: events.len() as u64,
            journal_record_set_hash: valid_hash('1'),
            event_set_hash: valid_hash('2'),
            capability_set_hash: valid_hash('3'),
        },
        valid_hash('4'),
        &TrustSet::default(),
    )
    .expect("strict Timeline");

    let connection =
        rusqlite::Connection::open(derived_database(root.path())).expect("open sidecar");
    let product_types = connection
        .prepare(
            "SELECT DISTINCT locator.event_type
             FROM product_history_event AS product
             JOIN locator_event_text AS locator ON locator.sequence = product.sequence
             ORDER BY locator.event_type",
        )
        .expect("prepare Timeline types")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query Timeline types")
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .expect("read Timeline types");
    let product_event_ids = connection
        .prepare(
            "SELECT locator.event_id
             FROM product_history_event AS product
             JOIN locator_event_text AS locator ON locator.sequence = product.sequence",
        )
        .expect("prepare Timeline event ids")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query Timeline event ids")
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .expect("read Timeline event ids");
    assert_eq!(
        product_event_ids,
        strict
            .entries()
            .iter()
            .map(|entry| entry.event_id.as_str().to_owned())
            .collect(),
        "the bodyless product event set must equal the strict Timeline event set"
    );
    for entry in strict.entries() {
        let exact = connection
            .prepare(
                "SELECT reference.revision_id, reference.object_artifact_content_hash
                 FROM product_history_revision_reference AS reference
                 JOIN locator_event_text AS locator ON locator.sequence = reference.sequence
                 WHERE locator.event_id = ?1 AND reference.resolution = 'exact'",
            )
            .expect("prepare exact Timeline references")
            .query_map([entry.event_id.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .expect("query exact Timeline references")
            .collect::<Result<std::collections::BTreeSet<_>, _>>()
            .expect("read exact Timeline references");
        assert_eq!(
            exact,
            entry
                .revision_refs
                .iter()
                .map(|reference| {
                    (
                        reference.revision_id.as_str().to_owned(),
                        reference.object_artifact_content_hash.clone(),
                    )
                })
                .collect(),
            "exact Revision parity for {}",
            entry.event_id.as_str()
        );
        let unresolved = connection
            .prepare(
                "SELECT reference.revision_id
                 FROM product_history_revision_reference AS reference
                 JOIN locator_event_text AS locator ON locator.sequence = reference.sequence
                 WHERE locator.event_id = ?1 AND reference.resolution = 'unresolved'",
            )
            .expect("prepare unresolved Timeline references")
            .query_map([entry.event_id.as_str()], |row| row.get::<_, String>(0))
            .expect("query unresolved Timeline references")
            .collect::<Result<std::collections::BTreeSet<_>, _>>()
            .expect("read unresolved Timeline references");
        assert_eq!(
            unresolved,
            entry
                .unresolved_revision_ids
                .iter()
                .map(|revision| revision.as_str().to_owned())
                .collect(),
            "unresolved Revision parity for {}",
            entry.event_id.as_str()
        );
        let changes = connection
            .prepare(
                "SELECT DISTINCT correlation.change_id
                 FROM product_history_change_correlation AS correlation
                 JOIN locator_event_text AS locator ON locator.sequence = correlation.sequence
                 WHERE locator.event_id = ?1",
            )
            .expect("prepare Timeline Change correlations")
            .query_map([entry.event_id.as_str()], |row| row.get::<_, String>(0))
            .expect("query Timeline Change correlations")
            .collect::<Result<std::collections::BTreeSet<_>, _>>()
            .expect("read Timeline Change correlations");
        assert_eq!(
            changes,
            entry
                .change_ids
                .iter()
                .map(|change| change.as_str().to_owned())
                .collect(),
            "Change correlation parity for {}",
            entry.event_id.as_str()
        );
    }
    for required in [
        "change_declared",
        "change_membership_asserted",
        "change_membership_withdrawn",
        "change_link_asserted",
        "change_revision_relation_asserted",
        "change_revision_relation_withdrawn",
        "revision_relation_attested",
        "review_fact_ported",
    ] {
        assert!(
            product_types.contains(required),
            "missing Change-capable Timeline family {required}"
        );
    }
    for excluded in [
        "task_checkpoint_captured",
        "task_observation_recorded",
        "event_signature_recorded",
        "artifact_removed",
    ] {
        assert!(!product_types.contains(excluded));
    }
    let request_states = connection
        .prepare(
            "SELECT request_state FROM product_history_event
             WHERE request_state IS NOT NULL ORDER BY request_state",
        )
        .expect("prepare request states")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query request states")
        .collect::<Result<Vec<_>, _>>()
        .expect("read request states");
    assert_eq!(request_states, vec!["answered", "open"]);
    assert_eq!(
        connection
            .query_row(
                "SELECT tag_value FROM product_history_tag_value",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("observation tag value"),
        "issue:semantic-history"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT assessment FROM semantic_assessment_fact
                 WHERE sequence = (
                     SELECT sequence FROM locator_event_text
                     WHERE event_type = 'review_assessment_recorded'
                 )",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("assessment facet"),
        "needs_changes"
    );
    assert_eq!(
        connection
            .query_row("SELECT status FROM semantic_validation_fact", [], |row| row
                .get::<_, String>(0),)
            .expect("check-status facet"),
        "failed"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*)
                 FROM product_history_revision_reference AS reference
                 JOIN locator_event_text AS locator ON locator.sequence = reference.sequence
                 WHERE locator.event_type = 'change_revision_relation_withdrawn'
                   AND reference.reference_role = 'direct'
                   AND reference.resolution = 'exact'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("withdrawn relation exact references"),
        2
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*)
                 FROM product_history_change_correlation AS correlation
                 JOIN locator_event_text AS locator ON locator.sequence = correlation.sequence
                 WHERE locator.event_type = 'change_link_asserted'
                   AND correlation.source_kind = 'link_claim'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("link Change references"),
        2
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM product_history_relation_claim",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .expect("relation claims"),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM product_history_relation_withdrawal",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("relation withdrawals"),
        1
    );

    let plan = |sql: &str| {
        connection
            .prepare(sql)
            .expect("prepare query plan")
            .query_map([], |row| row.get::<_, String>(3))
            .expect("query plan")
            .collect::<Result<Vec<_>, _>>()
            .expect("read query plan")
    };
    assert!(plan(
        "EXPLAIN QUERY PLAN
         SELECT sequence FROM product_history_revision_reference
         WHERE revision_id = 'rev:sha256:timeline-family'
           AND object_artifact_content_hash = 'sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd'
           AND resolution = 'exact'"
    )
    .iter()
    .any(|detail| detail.contains("product_history_revision_reference_exact")));
    assert!(
        plan(
            "EXPLAIN QUERY PLAN
         SELECT sequence FROM product_history_change_correlation
         WHERE change_id = 'change:sha256:timeline'"
        )
        .iter()
        .any(|detail| detail.contains("product_history_change_correlation_change"))
    );
    assert!(
        plan(
            "EXPLAIN QUERY PLAN
         SELECT support_sequence, sequence
         FROM product_history_change_correlation
         WHERE source_kind = 'membership_claim'
           AND source_id = 'change-membership:sha256:timeline'
         ORDER BY support_sequence, sequence"
        )
        .iter()
        .any(|detail| detail.contains("product_history_change_correlation_support"))
    );
    assert!(
        plan(
            "EXPLAIN QUERY PLAN
         SELECT sequence FROM product_history_tag_value
         WHERE tag_value = 'issue:semantic-history'"
        )
        .iter()
        .any(|detail| detail.contains("product_history_tag_value_lookup"))
    );
    assert!(
        plan(
            "EXPLAIN QUERY PLAN
         SELECT sequence FROM locator_event
         WHERE epoch = 1
         ORDER BY normalized_occurred_at, event_hash, sequence"
        )
        .iter()
        .any(|detail| detail.contains("locator_event_display"))
    );
    drop(connection);

    let inventory = adapter.semantic_inventory().expect("semantic inventory");
    assert_eq!(
        inventory.product_history_profile_id,
        "pointbreak.sqlite-derived-access-history.v1"
    );
    assert_eq!(inventory.product_history_schema_version, 5);
    let (frozen_profile_id, frozen_schema_version) =
        crate::session::derived_access::semantic::change::frozen_product_history_identity();
    assert_eq!(frozen_profile_id, inventory.product_history_profile_id);
    assert_eq!(
        frozen_schema_version,
        inventory.product_history_schema_version
    );
    assert_eq!(
        crate::session::derived_access::history::product_history_stamp_schema(),
        format!(
            "{}.v{}",
            inventory
                .product_history_profile_id
                .strip_suffix(".v1")
                .expect("versioned product-history profile"),
            inventory.product_history_schema_version
        )
    );
    assert!(inventory.product_history_event_count > 0);
    for required in [
        "product_history_event",
        "product_history_revision_reference",
        "product_history_change_correlation",
        "product_history_membership_claim",
        "product_history_membership_withdrawal",
        "product_history_relation_claim",
        "product_history_relation_withdrawal",
        "product_history_tag_value",
        "product_history_tag",
        "product_history_signature",
        "product_revision",
        "product_revision_edge",
    ] {
        assert!(
            inventory
                .product_history_tables
                .iter()
                .any(|name| name == required)
        );
    }
    for name in inventory
        .product_history_tables
        .iter()
        .chain(&inventory.product_history_columns)
        .chain(&inventory.product_history_indexes)
    {
        let name = name.to_ascii_lowercase();
        for forbidden in [
            "payload",
            "summary",
            "reason",
            "snippet",
            "prose",
            "document",
            "trust",
            "token",
            "embedding",
            "fts",
        ] {
            assert!(
                !name.contains(forbidden),
                "product schema contains {forbidden}: {name}"
            );
        }
    }
    assert_eq!(inventory.retained_body_object_bytes, 0);
    let bytes = std::fs::read(derived_database(root.path())).expect("read sidecar");
    assert!(
        !bytes
            .windows(b"BODYLESS TIMELINE ASSESSMENT SENTINEL".len())
            .any(|window| window == b"BODYLESS TIMELINE ASSESSMENT SENTINEL")
    );
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
fn proposal_carrier_locators_preserve_every_duplicate_at_one_exact_revision() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let exact = RevisionRefV1::new(
        revision_id("proposal-carriers"),
        format!("sha256:{}", "a".repeat(64)),
    )
    .expect("exact Revision");
    let carriers = [
        proposal_carrier_event(
            &exact,
            Some("equal proposal summary"),
            "work_object_proposed:proposal-carrier:first",
            "2026-08-04T00:02:01Z",
        ),
        proposal_carrier_event(
            &exact,
            Some("equal proposal summary"),
            "work_object_proposed:proposal-carrier:equal-duplicate",
            "2026-08-04T00:02:02Z",
        ),
        proposal_carrier_event(
            &exact,
            Some("conflicting proposal summary"),
            "work_object_proposed:proposal-carrier:conflicting-duplicate",
            "2026-08-04T00:02:03Z",
        ),
    ];
    let other_artifact = RevisionRefV1::new(
        exact.revision_id.clone(),
        format!("sha256:{}", "b".repeat(64)),
    )
    .expect("other exact Revision");
    let conflicting_binding = proposal_carrier_event(
        &other_artifact,
        Some("conflicting artifact binding"),
        "work_object_proposed:proposal-carrier:other-artifact",
        "2026-08-04T00:02:04Z",
    );
    for (attempt, carrier) in carriers.iter().enumerate() {
        append(&adapter, carrier, attempt);
    }
    append(&adapter, &conflicting_binding, carriers.len());

    let rows = ready(
        adapter
            .proposal_carrier_locators(&exact, TruthCursor::new(1, 4))
            .expect("proposal carrier locators"),
    );
    assert_eq!(rows.len(), carriers.len());
    assert_eq!(
        rows.iter()
            .map(|row| row.event_id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        carriers
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        "equal and conflicting duplicates must remain independently discoverable"
    );
    for (index, (row, event)) in rows.iter().zip(&carriers).enumerate() {
        assert_eq!(row.cursor, TruthCursor::new(1, index as u64 + 1));
        assert_eq!(row.event_id.as_str(), event.event_id.as_str());
        assert_eq!(row.event_type, EventType::WorkObjectProposed.as_str());
        assert_eq!(row.payload_hash, event.payload_hash);
        assert_eq!(row.revision, exact);
        let expected_locator =
            crate::canonical_hash::sha256_bytes_hex(event.idempotency_key.as_bytes());
        assert_eq!(row.logical_reread_key_hash, expected_locator);
        assert_eq!(row.replay_key, expected_locator);
        assert_eq!(row.validation_witness.len(), 64);
        assert!(
            row.validation_witness
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
    }

    let prefix = ready(
        adapter
            .proposal_carrier_locators(&exact, TruthCursor::new(1, 2))
            .expect("as-of proposal carrier locators"),
    );
    assert_eq!(prefix.len(), 2, "the exact lookup must respect its as-of");

    let other_rows = ready(
        adapter
            .proposal_carrier_locators(&other_artifact, TruthCursor::new(1, 4))
            .expect("other proposal carrier locators"),
    );
    assert_eq!(
        other_rows.len(),
        1,
        "RevisionId equality alone must not weaken exact Revision binding"
    );
    assert_eq!(other_rows[0].event_id, conflicting_binding.event_id);
    assert!(matches!(
        adapter
            .proposal_carrier_locators(&exact, TruthCursor::new(1, 5))
            .expect("future proposal checkpoint"),
        LocatorRead::CatchUpRequired {
            applied: TruthCursor { sequence: 4, .. },
            observed: TruthCursor { sequence: 5, .. }
        }
    ));

    let expected_event_ids = carriers
        .iter()
        .map(|event| event.event_id.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    drop(adapter);
    let reopened =
        QualificationDerivedAccessAdapter::open(root.path(), CursorLedgerIdentity::new(STORE_ID))
            .expect("reopen proposal carrier relation");
    let restarted = ready(
        reopened
            .proposal_carrier_locators(&exact, TruthCursor::new(1, 4))
            .expect("restarted proposal carrier locators"),
    );
    assert_eq!(
        restarted
            .iter()
            .map(|row| row.event_id.as_str().to_owned())
            .collect::<std::collections::BTreeSet<_>>(),
        expected_event_ids
    );
}

#[test]
fn proposal_carrier_locators_for_exact_revisions_group_every_selected_exact_binding_at_one_cursor()
{
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let revision = revision_id("proposal-carrier-batch");
    let exact_a = RevisionRefV1::new(revision.clone(), format!("sha256:{}", "a".repeat(64)))
        .expect("first exact Revision");
    let exact_b = RevisionRefV1::new(revision, format!("sha256:{}", "b".repeat(64)))
        .expect("second exact Revision");
    let unselected_binding = RevisionRefV1::new(
        exact_a.revision_id.clone(),
        format!("sha256:{}", "c".repeat(64)),
    )
    .expect("unselected exact Revision");
    let carriers = [
        proposal_carrier_event(
            &exact_a,
            Some("equal summary"),
            "work_object_proposed:proposal-carrier:batch:a:first",
            "2026-08-04T00:04:01Z",
        ),
        proposal_carrier_event(
            &exact_a,
            Some("equal summary"),
            "work_object_proposed:proposal-carrier:batch:a:duplicate",
            "2026-08-04T00:04:02Z",
        ),
        proposal_carrier_event(
            &exact_b,
            Some("other exact binding"),
            "work_object_proposed:proposal-carrier:batch:b",
            "2026-08-04T00:04:03Z",
        ),
        proposal_carrier_event(
            &unselected_binding,
            Some("must remain unselected"),
            "work_object_proposed:proposal-carrier:batch:unselected",
            "2026-08-04T00:04:04Z",
        ),
    ];
    for (attempt, carrier) in carriers.iter().enumerate() {
        append(&adapter, carrier, attempt);
    }

    let selected = std::collections::BTreeSet::from([exact_a.clone(), exact_b.clone()]);
    // Smallest intended service seam: one selected exact-Revision set and one
    // explicit truth cursor produce typed, per-exact-ref locator groups. Empty
    // groups remain present so the hydrator can fail a selected absent carrier.
    let prefix: std::collections::BTreeMap<RevisionRefV1, Vec<ProposalCarrierLocator>> = ready(
        adapter
            .proposal_carrier_locators_for_exact_revisions(&selected, TruthCursor::new(1, 2))
            .expect("batched proposal carrier prefix"),
    );
    assert_eq!(
        prefix.keys().collect::<Vec<_>>(),
        vec![&exact_a, &exact_b],
        "the result must retain every selected exact binding in stable order"
    );
    assert_eq!(
        prefix[&exact_a]
            .iter()
            .map(|row| row.event_id.as_str())
            .collect::<Vec<_>>(),
        carriers[..2]
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<Vec<_>>(),
        "every duplicate at the selected exact Revision must survive"
    );
    assert!(
        prefix[&exact_b].is_empty(),
        "a selected exact Revision beyond the as-of remains an explicit empty group"
    );

    let complete: std::collections::BTreeMap<RevisionRefV1, Vec<ProposalCarrierLocator>> = ready(
        adapter
            .proposal_carrier_locators_for_exact_revisions(&selected, TruthCursor::new(1, 4))
            .expect("batched proposal carrier locators"),
    );
    assert_eq!(complete[&exact_a].len(), 2);
    assert_eq!(complete[&exact_b].len(), 1);
    assert_eq!(complete[&exact_b][0].revision, exact_b);
    assert!(
        complete
            .values()
            .flatten()
            .all(|row| row.revision != unselected_binding),
        "RevisionId equality must not admit an unselected artifact binding"
    );
}

#[test]
fn proposal_carrier_locators_for_exact_revisions_are_portable_past_bind_variable_limits() {
    const PORTABLE_SQLITE_VARIABLE_LIMIT: usize = 999;

    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let selected = (0..=PORTABLE_SQLITE_VARIABLE_LIMIT)
        .map(|index| {
            RevisionRefV1::new(
                revision_id(&format!("proposal-batch-limit-{index:04}")),
                format!("sha256:{index:064x}"),
            )
            .expect("selected exact Revision")
        })
        .collect::<std::collections::BTreeSet<_>>();
    let first = selected.first().expect("first selected exact").clone();
    let last = selected.last().expect("last selected exact").clone();
    let carriers = [
        proposal_carrier_event(
            &first,
            None,
            "work_object_proposed:proposal-carrier:batch-limit:first",
            "2026-08-04T00:05:01Z",
        ),
        proposal_carrier_event(
            &last,
            None,
            "work_object_proposed:proposal-carrier:batch-limit:last",
            "2026-08-04T00:05:02Z",
        ),
    ];
    for (attempt, carrier) in carriers.iter().enumerate() {
        append(&adapter, carrier, attempt);
    }

    let grouped: std::collections::BTreeMap<RevisionRefV1, Vec<ProposalCarrierLocator>> = ready(
        adapter
            .proposal_carrier_locators_for_exact_revisions(&selected, TruthCursor::new(1, 2))
            .expect("portable batched proposal carrier locators"),
    );
    assert_eq!(grouped.len(), PORTABLE_SQLITE_VARIABLE_LIMIT + 1);
    assert_eq!(grouped[&first][0].event_id, carriers[0].event_id);
    assert_eq!(grouped[&last][0].event_id, carriers[1].event_id);
    assert_eq!(
        grouped.values().map(Vec::len).sum::<usize>(),
        carriers.len(),
        "only selected persisted carriers are returned despite the complete selected set"
    );
}

#[test]
fn proposal_carrier_locators_for_exact_revisions_fail_closed_on_cross_epoch_receipt() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let exact_a = RevisionRefV1::new(
        revision_id("proposal-batch-epoch-a"),
        format!("sha256:{}", "d".repeat(64)),
    )
    .expect("first exact Revision");
    let exact_b = RevisionRefV1::new(
        revision_id("proposal-batch-epoch-b"),
        format!("sha256:{}", "e".repeat(64)),
    )
    .expect("second exact Revision");
    append(
        &adapter,
        &proposal_carrier_event(
            &exact_a,
            None,
            "work_object_proposed:proposal-carrier:batch-epoch:a",
            "2026-08-04T00:06:01Z",
        ),
        0,
    );
    append(
        &adapter,
        &proposal_carrier_event(
            &exact_b,
            None,
            "work_object_proposed:proposal-carrier:batch-epoch:b",
            "2026-08-04T00:06:02Z",
        ),
        1,
    );
    let selected = std::collections::BTreeSet::from([exact_a, exact_b]);

    let connection =
        rusqlite::Connection::open(derived_database(root.path())).expect("open corrupt sidecar");
    connection
        .execute("UPDATE cursor_receipt SET epoch = 2 WHERE sequence = 2", [])
        .expect("corrupt one selected receipt epoch");
    drop(connection);

    let error = adapter
        .proposal_carrier_locators_for_exact_revisions(&selected, TruthCursor::new(1, 2))
        .expect_err("one foreign receipt epoch must fail the complete batch closed");
    assert!(
        error.to_string().contains("receipt epoch 2"),
        "unexpected failure: {error}"
    );
}

#[test]
fn proposal_carrier_locator_rejects_a_mismatched_receipt_epoch() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let exact = RevisionRefV1::new(
        revision_id("proposal-receipt-epoch"),
        format!("sha256:{}", "e".repeat(64)),
    )
    .expect("exact Revision");
    append(
        &adapter,
        &proposal_carrier_event(
            &exact,
            None,
            "work_object_proposed:proposal-carrier:receipt-epoch",
            "2026-08-04T00:02:05Z",
        ),
        0,
    );

    let connection =
        rusqlite::Connection::open(derived_database(root.path())).expect("open corrupt sidecar");
    connection
        .execute("UPDATE cursor_receipt SET epoch = 2 WHERE sequence = 1", [])
        .expect("corrupt receipt epoch");
    drop(connection);

    let error = adapter
        .proposal_carrier_locators(&exact, TruthCursor::new(1, 1))
        .expect_err("foreign receipt epoch must fail closed");
    assert!(
        error.to_string().contains("receipt epoch 2"),
        "unexpected failure: {error}"
    );
}

#[test]
fn materialized_change_projection_rejects_a_mismatched_receipt_epoch() {
    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let schedule = change_schedule();
    for (attempt, event) in schedule.iter().take(3).enumerate() {
        append(&adapter, event, attempt);
    }

    let connection =
        rusqlite::Connection::open(derived_database(root.path())).expect("open corrupt sidecar");
    connection
        .execute("UPDATE cursor_receipt SET epoch = 2 WHERE sequence = 2", [])
        .expect("corrupt receipt epoch");
    drop(connection);

    let error = adapter
        .semantic_materialized_change_projection()
        .expect_err("foreign receipt epoch must fail closed");
    assert!(
        error.to_string().contains("receipt epoch 2"),
        "unexpected failure: {error}"
    );
}

#[test]
fn proposal_carrier_inventory_is_indexed_and_retains_no_summary_material() {
    const SUMMARY_SENTINEL: &str = "PRIVATE PROPOSAL SUMMARY SENTINEL COMPACT READER";

    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let exact = RevisionRefV1::new(
        revision_id("proposal-inventory"),
        format!("sha256:{}", "c".repeat(64)),
    )
    .expect("exact Revision");
    append(
        &adapter,
        &proposal_carrier_event(
            &exact,
            Some(SUMMARY_SENTINEL),
            "work_object_proposed:proposal-carrier:inventory",
            "2026-08-04T00:03:01Z",
        ),
        0,
    );

    let inventory = adapter.semantic_inventory().expect("semantic inventory");
    assert_eq!(inventory.schema_version, 8);
    assert_eq!(inventory.proposal_carrier_count, 1);
    assert!(
        inventory
            .tables
            .iter()
            .any(|table| table == "semantic_revision_proposal_carrier")
    );
    assert_eq!(
        inventory.proposal_carrier_columns,
        vec!["object_artifact_content_hash", "revision_id", "sequence",]
    );
    assert_eq!(
        inventory.proposal_carrier_indexes,
        vec!["semantic_revision_proposal_exact"]
    );
    assert_eq!(inventory.retained_body_object_bytes, 0);
    for name in inventory
        .proposal_carrier_columns
        .iter()
        .chain(&inventory.proposal_carrier_indexes)
    {
        let name = name.to_ascii_lowercase();
        for forbidden in [
            "summary",
            "payload_json",
            "event_json",
            "document",
            "prose",
            "search",
            "fts",
        ] {
            assert!(
                !name.contains(forbidden),
                "proposal schema contains {forbidden}"
            );
        }
    }

    let database = derived_database(root.path());
    let query_connection = rusqlite::Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .expect("open proposal index evidence");
    let mut plan = query_connection
        .prepare(
            "EXPLAIN QUERY PLAN
             SELECT sequence
             FROM semantic_revision_proposal_carrier
             WHERE revision_id = ?1
               AND object_artifact_content_hash = ?2
               AND sequence <= ?3",
        )
        .expect("prepare proposal index evidence");
    let plan_details = plan
        .query_map(
            rusqlite::params![
                exact.revision_id.as_str(),
                exact.object_artifact_content_hash,
                1_i64
            ],
            |row| row.get::<_, String>(3),
        )
        .expect("query proposal index evidence")
        .collect::<Result<Vec<_>, _>>()
        .expect("read proposal index evidence");
    assert!(
        plan_details
            .iter()
            .any(|detail| detail.contains("semantic_revision_proposal_exact")),
        "exact proposal lookup must use its covering index: {plan_details:?}"
    );
    drop(plan);

    query_connection
        .execute_batch(
            "CREATE TEMP TABLE pointbreak_proposal_exact_lookup (
                 revision_id TEXT NOT NULL,
                 object_artifact_content_hash TEXT NOT NULL,
                 PRIMARY KEY (revision_id, object_artifact_content_hash)
             ) STRICT, WITHOUT ROWID;",
        )
        .expect("create selected exact-Revision TEMP relation");
    query_connection
        .execute(
            "INSERT INTO temp.pointbreak_proposal_exact_lookup (
                 revision_id, object_artifact_content_hash
             ) VALUES (?1, ?2)",
            rusqlite::params![
                exact.revision_id.as_str(),
                exact.object_artifact_content_hash
            ],
        )
        .expect("populate selected exact-Revision TEMP relation");
    let mut batch_plan = query_connection
        .prepare(
            "EXPLAIN QUERY PLAN
             SELECT proposal.sequence
             FROM temp.pointbreak_proposal_exact_lookup AS selected
             JOIN semantic_revision_proposal_carrier AS proposal
                  INDEXED BY semantic_revision_proposal_exact
               ON proposal.revision_id = selected.revision_id
              AND proposal.object_artifact_content_hash =
                  selected.object_artifact_content_hash
             WHERE proposal.sequence <= ?1
             ORDER BY proposal.sequence",
        )
        .expect("prepare batched proposal index evidence");
    let batch_plan_details = batch_plan
        .query_map([1_i64], |row| row.get::<_, String>(3))
        .expect("query batched proposal index evidence")
        .collect::<Result<Vec<_>, _>>()
        .expect("read batched proposal index evidence");
    assert!(
        batch_plan_details
            .iter()
            .any(|detail| detail.contains("semantic_revision_proposal_exact")),
        "TEMP exact-set join must retain covering-index lookup: {batch_plan_details:?}"
    );
    drop(batch_plan);
    drop(query_connection);

    drop(adapter);
    let sidecar_directory = database.parent().expect("sidecar directory");
    let database_name = database
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .expect("database name");
    for entry in std::fs::read_dir(sidecar_directory).expect("read sidecar directory") {
        let path = entry.expect("sidecar entry").path();
        if !path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| name.starts_with(database_name))
        {
            continue;
        }
        let bytes = std::fs::read(&path).expect("read SQLite carrier");
        assert!(
            !bytes
                .windows(SUMMARY_SENTINEL.len())
                .any(|window| window == SUMMARY_SENTINEL.as_bytes()),
            "{} retained proposal summary prose",
            path.display()
        );
    }
}

/// The batch locator read must advance from the selected TEMP set through the
/// exact proposal index and point-read locator and receipt rows by sequence.
/// The inverted plan — locator range outermost with a full proposal-index
/// rescan per event row — is `O(journal * proposals)` and turns the bodyless
/// Change page into minutes of work at retained scale while every
/// small-fixture correctness matrix stays green.
#[test]
fn proposal_carrier_locator_batch_plan_advances_from_the_selected_set() {
    use crate::bench_support::derived_access::sqlite_locator::PROPOSAL_CARRIER_LOCATOR_BATCH_SQL;

    let root = tempfile::tempdir().expect("root");
    let adapter = open_adapter(root.path());
    let exact = RevisionRefV1::new(
        revision_id("proposal-batch-plan"),
        format!("sha256:{}", "d".repeat(64)),
    )
    .expect("exact Revision");
    append(
        &adapter,
        &proposal_carrier_event(
            &exact,
            None,
            "work_object_proposed:proposal-carrier:batch-plan",
            "2026-08-04T00:04:01Z",
        ),
        0,
    );
    drop(adapter);

    let database = derived_database(root.path());
    let query_connection = rusqlite::Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .expect("open batch plan evidence");
    query_connection
        .execute_batch(
            "CREATE TEMP TABLE pointbreak_proposal_exact_lookup (
                 revision_id TEXT NOT NULL,
                 object_artifact_content_hash TEXT NOT NULL,
                 PRIMARY KEY (revision_id, object_artifact_content_hash)
             ) STRICT, WITHOUT ROWID;",
        )
        .expect("create selected exact-Revision TEMP relation");
    let mut plan = query_connection
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {PROPOSAL_CARRIER_LOCATOR_BATCH_SQL}"
        ))
        .expect("prepare batch plan evidence");
    let plan_details = plan
        .query_map(rusqlite::params![1_i64, 1_i64], |row| {
            row.get::<_, String>(3)
        })
        .expect("query batch plan evidence")
        .collect::<Result<Vec<_>, _>>()
        .expect("read batch plan evidence");
    assert!(
        plan_details
            .first()
            .is_some_and(|detail| detail.contains("SCAN selected")),
        "batch locator read must begin from the selected TEMP set: {plan_details:?}"
    );
    assert!(
        plan_details.iter().any(|detail| {
            detail.contains("SEARCH proposal")
                && detail.contains("semantic_revision_proposal_exact")
                && detail.contains("revision_id=?")
        }),
        "batch locator read must probe the exact proposal index: {plan_details:?}"
    );
    assert!(
        !plan_details
            .iter()
            .any(|detail| detail.contains("SCAN proposal")),
        "batch locator read must never rescan the proposal index per row: {plan_details:?}"
    );
    for point_read in ["SEARCH locator USING", "SEARCH cursor_receipt USING"] {
        assert!(
            plan_details.iter().any(|detail| detail.contains(point_read)
                && detail.contains("INTEGER PRIMARY KEY")),
            "batch locator read must point-read by sequence: {plan_details:?}"
        );
    }
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
    let exact = RevisionRefV1::new(revision_id("atomic"), format!("sha256:{}", "d".repeat(64)))
        .expect("exact Revision");
    let event = proposal_carrier_event(
        &exact,
        None,
        "work_object_proposed:proposal-carrier:atomic",
        "2026-07-27T16:00:00Z",
    );
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
    let rolled_back = adapter.semantic_inventory().expect("rolled-back inventory");
    assert_eq!(
        rolled_back.proposal_carrier_count, 0,
        "the proposal relation must roll back with its locator checkpoint"
    );
    assert_eq!(
        rolled_back.product_history_event_count, 0,
        "the Timeline relation must roll back with its locator checkpoint"
    );

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
    let committed = adapter.semantic_inventory().expect("committed inventory");
    assert_eq!(committed.proposal_carrier_count, 1);
    assert_eq!(committed.product_history_event_count, 1);
    assert_eq!(
        ready(
            adapter
                .proposal_carrier_locators(&exact, TruthCursor::new(1, 1))
                .expect("committed proposal carrier")
        )
        .len(),
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
    assert_eq!(inventory.schema_version, 8);
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
            "semantic_revision_proposal_carrier",
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

/// One store shape exercising every content-reference class: a Revision
/// proposal (raw-form object hash), an externalized observation body
/// (digest-form note-body hash), an inline observation body, external
/// validation log hashes, and a removal claim.
fn content_reference_fixture() -> (tempfile::TempDir, QualificationDerivedAccessAdapter) {
    let root = tempfile::tempdir().expect("temp root");
    let adapter = open_adapter(root.path());
    let subject = revision_id("aa");
    let body_hex = "f".repeat(64);

    let external_observation = ShoreEvent::new(
        EventType::ReviewObservationRecorded,
        "review_observation_recorded:external-body",
        revision_target(&subject),
        Writer::shore_local("0.8.0"),
        ReviewObservationRecordedPayload {
            observation_id: ObservationId::new("obs:sha256:external-body"),
            target: ReviewTargetRef::Revision {
                revision_id: subject.clone(),
            },
            title: "Externalized observation".to_owned(),
            body: None,
            body_content_type: BodyContentType::TextPlain,
            body_artifact_path: Some(format!("artifacts/notes/{body_hex}.json")),
            body_byte_size: Some(5000),
            body_content_hash: Some(format!("sha256:{body_hex}")),
            tags: Vec::new(),
            confidence: None,
            supersedes_observation_ids: Vec::new(),
            responds_to_observation_ids: Vec::new(),
        },
        "2026-07-27T16:00:12Z",
    )
    .expect("externalized observation");

    let logged_validation = ShoreEvent::new(
        EventType::ValidationCheckRecorded,
        ValidationCheckRecordedPayload::idempotency_key(
            &subject,
            &TrackId::new(TRACK),
            "content-reference-log",
        ),
        revision_target(&subject),
        Writer::shore_local("0.8.0"),
        ValidationCheckRecordedPayload {
            validation_check_id: ValidationCheckId::new("validation:sha256:content-reference"),
            target: ValidationTarget::Revision {
                revision_id: subject.clone(),
            },
            check_name: "content-reference-log".to_owned(),
            command: None,
            status: ValidationStatus::Passed,
            exit_code: Some(0),
            trigger: ValidationTrigger::Manual,
            source_fingerprint: None,
            summary: None,
            summary_content_type: BodyContentType::TextPlain,
            summary_artifact_path: None,
            summary_byte_size: None,
            summary_content_hash: None,
            started_at: None,
            completed_at: Some("2026-07-27T16:00:13Z".to_owned()),
            log_artifact_content_hashes: vec![valid_hash('e')],
        },
        "2026-07-27T16:00:13Z",
    )
    .expect("logged validation");

    for (attempt, event) in [
        initialized_event(JOURNAL),
        revision_event("aa", Vec::new(), "2026-07-27T16:00:01Z"),
        external_observation,
        observation_event(&subject),
        logged_validation,
        removal_event(&format!("sha256:{body_hex}")),
    ]
    .iter()
    .enumerate()
    {
        append(&adapter, event, attempt);
    }
    (root, adapter)
}

#[test]
fn content_reference_rows_cover_every_externalized_reference() {
    let (root, _adapter) = content_reference_fixture();

    let connection =
        rusqlite::Connection::open(derived_database(root.path())).expect("open sidecar");
    let rows: std::collections::BTreeSet<(String, String)> = connection
        .prepare(
            "SELECT locator.event_id,
                    coalesce(
                        reference.content_raw,
                        prefix.value || lower(hex(reference.content_digest))
                    ) AS content_hash
             FROM product_history_content_reference AS reference
             JOIN locator_event_text AS locator
               ON locator.sequence = reference.sequence
             LEFT JOIN semantic_identity_prefix AS prefix
               ON prefix.id = reference.content_prefix_id",
        )
        .expect("prepare content-reference read")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query content references")
        .collect::<Result<_, _>>()
        .expect("read content references");

    // Exactly two rows: the proposal's object hash (raw form — the fixture
    // hash is not a canonical 64-hex digest) and the externalized note-body
    // hash (digest form). The inline body, the external validation log hash,
    // and the removal claim itself contribute nothing.
    let hashes: std::collections::BTreeSet<&str> =
        rows.iter().map(|(_, hash)| hash.as_str()).collect();
    assert_eq!(rows.len(), 2, "reference rows: {rows:?}");
    assert!(hashes.contains("sha256:artifact:aa"), "{hashes:?}");
    assert!(
        hashes.contains(format!("sha256:{}", "f".repeat(64)).as_str()),
        "{hashes:?}"
    );
}

#[test]
fn content_reference_table_retains_no_body_bytes() {
    let (_root, adapter) = content_reference_fixture();

    let inventory = adapter.semantic_inventory().expect("semantic inventory");
    assert_eq!(
        inventory.retained_body_object_bytes, 0,
        "the content-reference table must hold no body/object bytes"
    );
}

#[test]
fn removal_audit_reference_seek_uses_the_lookup_index() {
    use crate::session::derived_access::sqlite::removal_audit_reference_seek_sql;

    let (root, _adapter) = content_reference_fixture();
    let connection =
        rusqlite::Connection::open(derived_database(root.path())).expect("open sidecar");
    for raw in [false, true] {
        let parameters: Vec<rusqlite::types::Value> = if raw {
            vec![
                "sha256:artifact:aa".to_owned().into(),
                1_i64.into(),
                64_i64.into(),
            ]
        } else {
            vec![
                "sha256:".to_owned().into(),
                vec![0_u8; 32].into(),
                1_i64.into(),
                64_i64.into(),
            ]
        };
        let details: Vec<String> = connection
            .prepare(&format!(
                "EXPLAIN QUERY PLAN {}",
                removal_audit_reference_seek_sql(raw)
            ))
            .expect("prepare reference-seek plan")
            .query_map(rusqlite::params_from_iter(parameters), |row| {
                row.get::<_, String>(3)
            })
            .expect("query reference-seek plan")
            .collect::<Result<_, _>>()
            .expect("read reference-seek plan");
        assert!(
            details.iter().any(|detail| {
                detail.contains("SEARCH reference")
                    && detail.contains("INDEX product_history_content_reference_lookup")
            }),
            "raw={raw}: the reference seek must use the lookup index: {details:?}"
        );
        assert!(
            !details
                .iter()
                .any(|detail| detail.contains("SCAN") && detail.contains("reference")),
            "raw={raw}: the reference table must never be scanned: {details:?}"
        );
    }
}
