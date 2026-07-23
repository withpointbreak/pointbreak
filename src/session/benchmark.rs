//! Bench-gated production bridge for deterministic longitudinal stores.
//!
//! The public workload module owns only frozen generator inputs and public
//! receipts. This module is the one-way adapter into production identity,
//! artifact, ingest, replay, and projection primitives.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use super::event::{
    ArtifactRemovedPayload, AssertionMode, BodyContentType, EventSignature,
    EventSignatureRecordedPayload, EventTarget, EventToBeSigned, EventType,
    InputRequestOpenedPayload, InputRequestReasonCode, InputRequestRespondedPayload,
    InputRequestResponseOutcome, ReviewAssessment, ReviewAssessmentRecordedPayload,
    ReviewInitializedPayload, ReviewObservationRecordedPayload, Revision,
    RevisionCommitAssociatedPayload, RevisionCommitWithdrawnPayload, RevisionRefAssociatedPayload,
    RevisionRefWithdrawnPayload, ShoreEvent, SourceSpeaker, TaskCheckpointCapturedPayload,
    TaskObservationRecordedPayload, ValidationCheckRecordedPayload, WorkObjectProposal,
    WorkObjectProposedPayload, Writer, WriterProducer, build_commit_association_id,
    build_commit_withdrawal_id, build_ref_association_id, build_ref_withdrawal_id,
    event_signature_pre_authentication_encoding,
};
use super::projection::freshness::event_set_hash_for_events;
use super::store::body_artifact::{NoteBodyEnvelope, parse_note_body_artifact};
use super::store::content::ContentArtifacts;
use super::store::object_artifact::{
    build_object_artifact_v2, decode_and_validate_object_artifact, object_artifact_path_for_hash,
};
use super::store::resolution::{prepare_write_landing, resolve_read_store, resolve_write_store};
use super::workflow::assessment::add::{AssessmentIdMaterial, build_assessment_id};
use super::workflow::ingest_events_with_clock;
use super::workflow::input_request::open::{InputRequestIdMaterial, build_input_request_id};
use super::workflow::input_request::respond::{
    InputRequestResponseIdMaterial, build_input_request_response_id,
};
use super::workflow::observation::add::{ObservationIdMaterial, build_observation_id};
use super::workflow::observation::staged_body;
use super::workflow::validation::add::{ValidationCheckIdMaterial, build_validation_check_id};
use super::{
    EventStore, IngestClock, IngestEventsOptions, IngestVia, SessionState, TrustSet,
    event_signature_trust_set,
};
use crate::bench_support::longitudinal::{
    FixedLongitudinalClockV1, LONGITUDINAL_FIXED_INGEST_RECEIVED_AT_V1,
    LONGITUDINAL_PUBLIC_SEED_HEX_V1, LongitudinalContentKindV1, LongitudinalEventCarrierV1,
    LongitudinalEventIdentityV1, LongitudinalInventoryEntryV1, LongitudinalStrictSemanticReceiptV1,
};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
use crate::crypto::EventSigner;
use crate::error::{Result, ShoreError};
use crate::keys::FileEd25519Signer;
use crate::model::{
    ActorId, CheckpointId, DiffFile, DiffRow, DiffRowKind, DiffSnapshot, EngagementId,
    EngagementType, FileId, FileStatus, HunkId, JournalId, ObjectId, ReviewEndpoint, ReviewHunk,
    ReviewId, ReviewTargetRef, RevisionId, TargetRef, TaskTargetRef, TrackId, ValidationStatus,
    ValidationTarget, ValidationTrigger, WorkObjectId, WorkObjectType, id_prefix,
};
use crate::storage::{LocalStorage, RemoveOutcome};

const V1_BLOCK_EVENT_COUNT: u64 = 256;
const V1_BLOCK_BODY_COUNT: u64 = 180;
const V1_BLOCK_OBJECT_COUNT: u64 = 12;
const V1_BLOCK_LOG_COUNT: u64 = 5;
const CAPACITY_SUPERBLOCK_EVENT_COUNT: u64 = 1_024;
const CAPACITY_SUPERBLOCK_BODY_COUNT: u64 = 658;
const CAPACITY_SUPERBLOCK_OBJECT_COUNT: u64 = 100;
const CAPACITY_SUPERBLOCK_LOG_COUNT: u64 = 10;
const BODY_SIZES: [usize; 8] = [512, 1_024, 2_048, 4_096, 8_192, 12_288, 16_384, 20_992];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LongitudinalRecordShapeV1 {
    Workload,
    CapacityV1,
    CapacityL100O10K,
}

impl LongitudinalRecordShapeV1 {
    fn namespace(self) -> &'static str {
        match self {
            Self::Workload => "workload",
            Self::CapacityV1 => "capacity-v1",
            Self::CapacityL100O10K => "capacity-l100-o10k",
        }
    }

    fn event_count(self) -> u64 {
        match self {
            Self::Workload | Self::CapacityV1 => V1_BLOCK_EVENT_COUNT,
            Self::CapacityL100O10K => CAPACITY_SUPERBLOCK_EVENT_COUNT,
        }
    }

    fn body_count(self) -> u64 {
        match self {
            Self::Workload | Self::CapacityV1 => V1_BLOCK_BODY_COUNT,
            Self::CapacityL100O10K => CAPACITY_SUPERBLOCK_BODY_COUNT,
        }
    }

    fn object_count(self) -> u64 {
        match self {
            Self::Workload | Self::CapacityV1 => V1_BLOCK_OBJECT_COUNT,
            Self::CapacityL100O10K => CAPACITY_SUPERBLOCK_OBJECT_COUNT,
        }
    }

    fn log_count(self) -> u64 {
        match self {
            Self::Workload | Self::CapacityV1 => V1_BLOCK_LOG_COUNT,
            Self::CapacityL100O10K => CAPACITY_SUPERBLOCK_LOG_COUNT,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LongitudinalRecordSpecV1 {
    shape: LongitudinalRecordShapeV1,
    block: u64,
}

impl LongitudinalRecordSpecV1 {
    pub(crate) fn new(shape: LongitudinalRecordShapeV1, block: u64) -> Self {
        Self { shape, block }
    }
}

#[derive(Clone, Debug)]
enum PreparedContentV1 {
    ExternalBody {
        shape: LongitudinalRecordShapeV1,
        block: u64,
        global_ordinal: u64,
        domain: &'static str,
        relative_path: String,
        content_hash: String,
    },
    Object {
        shape: LongitudinalRecordShapeV1,
        block: u64,
        global_ordinal: u64,
        relative_path: String,
        content_hash: String,
    },
    ValidationLog {
        shape: LongitudinalRecordShapeV1,
        block: u64,
        ordinal: u64,
        global_ordinal: u64,
        relative_path: String,
        content_hash: String,
    },
}

impl PreparedContentV1 {
    fn relative_path(&self) -> &str {
        match self {
            Self::ExternalBody { relative_path, .. }
            | Self::Object { relative_path, .. }
            | Self::ValidationLog { relative_path, .. } => relative_path,
        }
    }

    fn content_hash(&self) -> &str {
        match self {
            Self::ExternalBody { content_hash, .. }
            | Self::Object { content_hash, .. }
            | Self::ValidationLog { content_hash, .. } => content_hash,
        }
    }

    fn kind(&self) -> LongitudinalContentKindV1 {
        match self {
            Self::ExternalBody { .. } => LongitudinalContentKindV1::ExternalBody,
            Self::Object { .. } => LongitudinalContentKindV1::ObjectArtifact,
            Self::ValidationLog { .. } => LongitudinalContentKindV1::ValidationLog,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedLongitudinalRecordV1 {
    events: Vec<ShoreEvent>,
    content: Vec<PreparedContentV1>,
    removed: Vec<PreparedContentV1>,
    revision_count: u64,
    task_attempt_count: u64,
    body_fact_count: u64,
    external_body_count: u64,
    object_artifact_count: u64,
    decoded_body_bytes: u64,
    decoded_object_target_bytes: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct LongitudinalWriteReceiptV1 {
    pub(crate) ordered_events: Vec<LongitudinalEventIdentityV1>,
    pub(crate) event_carriers: Vec<LongitudinalEventCarrierV1>,
    pub(crate) content_inventory: Vec<LongitudinalInventoryEntryV1>,
    pub(crate) removed_content_sha256: Vec<String>,
    pub(crate) strict: LongitudinalStrictSemanticReceiptV1,
    pub(crate) events_created: u64,
    pub(crate) events_existing: u64,
    pub(crate) event_count: u64,
    pub(crate) revision_count: u64,
    pub(crate) task_attempt_count: u64,
    pub(crate) body_fact_count: u64,
    pub(crate) external_body_count: u64,
    pub(crate) object_artifact_count: u64,
    pub(crate) decoded_body_bytes: u64,
    pub(crate) decoded_object_target_bytes: u64,
    pub(crate) by_type: BTreeMap<String, u64>,
}

#[derive(Clone, Copy)]
struct FixedBenchmarkIngestClock;

impl IngestClock for FixedBenchmarkIngestClock {
    fn received_at(&self) -> String {
        LONGITUDINAL_FIXED_INGEST_RECEIVED_AT_V1.to_owned()
    }
}

#[derive(Clone)]
struct RevisionFixtureV1 {
    revision_id: RevisionId,
    object_id: ObjectId,
    object_content_hash: String,
    engagement_id: EngagementId,
    supersedes: Vec<RevisionId>,
}

#[derive(Clone)]
struct TaskFixtureV1 {
    task_attempt_id: WorkObjectId,
    predecessor: Option<WorkObjectId>,
}

struct BodyFixtureV1 {
    inline: Option<String>,
    artifact_path: Option<String>,
    byte_size: u64,
    content_hash: String,
    prepared: Option<PreparedContentV1>,
}

pub(crate) fn prepare_longitudinal_record_v1(
    spec: LongitudinalRecordSpecV1,
) -> Result<PreparedLongitudinalRecordV1> {
    match spec.shape {
        LongitudinalRecordShapeV1::Workload | LongitudinalRecordShapeV1::CapacityV1 => {
            prepare_v1_block(spec)
        }
        LongitudinalRecordShapeV1::CapacityL100O10K => prepare_capacity_superblock(spec),
    }
}

fn prepare_v1_block(spec: LongitudinalRecordSpecV1) -> Result<PreparedLongitudinalRecordV1> {
    let mut builder = BlockBuilderV1::new(spec);
    builder.push_review_initialized()?;
    builder.prepare_revisions(&[8, 4])?;
    builder.prepare_tasks(4)?;
    builder.push_revision_proposals()?;
    builder.push_task_proposals()?;
    builder.push_review_observations(64)?;
    builder.push_assessments(24)?;
    builder.push_input_requests(24, 6)?;
    builder.push_input_responses(16, 4)?;
    builder.push_ref_associations(12, 4)?;
    builder.push_commit_associations(16, 4)?;
    builder.push_validations(40)?;
    builder.push_task_checkpoints(12)?;
    builder.push_task_observations(12)?;
    builder.push_signature_carriers(8)?;
    builder.push_removals()?;
    builder.finish()
}

fn prepare_capacity_superblock(
    spec: LongitudinalRecordSpecV1,
) -> Result<PreparedLongitudinalRecordV1> {
    let mut builder = BlockBuilderV1::new(spec);
    builder.push_review_initialized()?;
    builder.prepare_revisions(&[10; 10])?;
    builder.prepare_tasks(4)?;
    builder.push_revision_proposals()?;
    builder.push_task_proposals()?;
    builder.push_review_observations(300)?;
    builder.push_assessments(100)?;
    builder.push_input_requests(100, 12)?;
    builder.push_input_responses(70, 8)?;
    builder.push_ref_associations(100, 20)?;
    builder.push_commit_associations(100, 20)?;
    builder.push_validations(80)?;
    builder.push_task_checkpoints(8)?;
    builder.push_task_observations(8)?;
    builder.push_signature_carriers(10)?;
    builder.push_removals()?;
    builder.finish()
}

struct BlockBuilderV1 {
    spec: LongitudinalRecordSpecV1,
    journal_id: JournalId,
    events: Vec<ShoreEvent>,
    content: Vec<PreparedContentV1>,
    removed: Vec<PreparedContentV1>,
    revisions: Vec<RevisionFixtureV1>,
    tasks: Vec<TaskFixtureV1>,
    observations: Vec<crate::model::ObservationId>,
    assessments: Vec<crate::model::AssessmentId>,
    input_requests: Vec<crate::model::InputRequestId>,
    ref_associations: Vec<crate::model::RefAssociationId>,
    commit_associations: Vec<crate::model::CommitAssociationId>,
    checkpoints: Vec<CheckpointId>,
    body_ordinal: u64,
    object_ordinal: u64,
    log_ordinal: u64,
    decoded_body_bytes: u64,
    decoded_object_target_bytes: u64,
    external_body_count: u64,
}

impl BlockBuilderV1 {
    fn new(spec: LongitudinalRecordSpecV1) -> Self {
        Self {
            journal_id: JournalId::new(format!(
                "journal:longitudinal:{}:{:06}",
                spec.shape.namespace(),
                spec.block
            )),
            spec,
            events: Vec::with_capacity(spec.shape.event_count() as usize),
            content: Vec::new(),
            removed: Vec::new(),
            revisions: Vec::new(),
            tasks: Vec::new(),
            observations: Vec::new(),
            assessments: Vec::new(),
            input_requests: Vec::new(),
            ref_associations: Vec::new(),
            commit_associations: Vec::new(),
            checkpoints: Vec::new(),
            body_ordinal: 0,
            object_ordinal: 0,
            log_ordinal: 0,
            decoded_body_bytes: 0,
            decoded_object_target_bytes: 0,
            external_body_count: 0,
        }
    }

    fn push_review_initialized(&mut self) -> Result<()> {
        let payload = ReviewInitializedPayload {};
        let event = ShoreEvent::new(
            EventType::ReviewInitialized,
            ReviewInitializedPayload::idempotency_key(&self.journal_id),
            EventTarget::for_journal(self.journal_id.clone()),
            self.writer(),
            payload,
            self.occurred_at()?,
        )?;
        self.push(event)
    }

    fn prepare_revisions(&mut self, engagement_sizes: &[usize]) -> Result<()> {
        for (engagement_ordinal, &size) in engagement_sizes.iter().enumerate() {
            let engagement_start = self.revisions.len();
            let mut engagement_ids = Vec::with_capacity(size);
            for local in 0..size {
                let object_global = self.global_object_ordinal();
                let target_size = object_target_size(object_global);
                let (files, decoded_target_bytes) =
                    object_files(self.spec.shape, self.spec.block, object_global, target_size);
                if decoded_target_bytes != target_size {
                    return Err(ShoreError::Message(format!(
                        "longitudinal object target size drift: expected {target_size}, got {decoded_target_bytes}"
                    )));
                }
                let object_id = super::fingerprint::object_identity(&files);
                let revision_id = super::fingerprint::revision_id_from(&object_id, None)?;
                let supersedes = engagement_supersedes(local, &engagement_ids);
                let engagement_id = if local == 0 {
                    super::fingerprint::engagement_id_from_root(&revision_id)
                } else {
                    self.revisions[engagement_start].engagement_id.clone()
                };
                let snapshot = DiffSnapshot::new(
                    object_review_id(self.spec.shape, object_global),
                    object_id.clone(),
                    files,
                );
                let artifact = build_object_artifact_v2(snapshot)?;
                let relative_path = object_content_ref(&artifact.content_hash)?;
                let prepared = PreparedContentV1::Object {
                    shape: self.spec.shape,
                    block: self.spec.block,
                    global_ordinal: object_global,
                    relative_path,
                    content_hash: artifact.content_hash.clone(),
                };
                if local == 0 && engagement_ordinal == 0 {
                    self.removed.push(prepared.clone());
                }
                self.content.push(prepared);
                self.decoded_object_target_bytes += target_size as u64;
                self.object_ordinal += 1;
                engagement_ids.push(revision_id.clone());
                self.revisions.push(RevisionFixtureV1 {
                    revision_id,
                    object_id,
                    object_content_hash: artifact.content_hash,
                    engagement_id,
                    supersedes,
                });
            }
        }
        Ok(())
    }

    fn prepare_tasks(&mut self, count: usize) -> Result<()> {
        for ordinal in 0..count {
            let task_attempt_id = WorkObjectId::new(format!(
                "{}:sha256:{}",
                id_prefix::TASK_ATTEMPT,
                deterministic_digest(
                    self.spec.shape,
                    self.spec.block,
                    ordinal as u64,
                    "task-attempt"
                )?
            ));
            let predecessor =
                (ordinal % 2 == 1).then(|| self.tasks[ordinal - 1].task_attempt_id.clone());
            self.tasks.push(TaskFixtureV1 {
                task_attempt_id,
                predecessor,
            });
        }
        Ok(())
    }

    fn push_revision_proposals(&mut self) -> Result<()> {
        for revision in self.revisions.clone() {
            let target = ReviewTargetRef::Revision {
                revision_id: revision.revision_id.clone(),
            };
            let payload = WorkObjectProposedPayload {
                engagement_id: revision.engagement_id,
                work_object: WorkObjectProposal::Revision {
                    revision: Revision {
                        id: revision.revision_id.clone(),
                        object_id: revision.object_id,
                        git_provenance: None,
                    },
                    summary: Some("deterministic longitudinal revision".to_owned()),
                    object_artifact_content_hash: revision.object_content_hash,
                    supersedes: revision.supersedes,
                },
            };
            let event = ShoreEvent::new(
                EventType::WorkObjectProposed,
                WorkObjectProposedPayload::idempotency_key(&TargetRef::Review(target.clone()))?,
                EventTarget::for_generative_move(
                    self.journal_id.clone(),
                    EngagementType::Review,
                    TargetRef::Review(target),
                    Some(self.track()),
                )?,
                self.writer(),
                payload,
                self.occurred_at()?,
            )?;
            self.push(event)?;
        }
        Ok(())
    }

    fn push_task_proposals(&mut self) -> Result<()> {
        for task in self.tasks.clone() {
            let subject = TargetRef::Task(TaskTargetRef::TaskAttempt {
                task_attempt_id: task.task_attempt_id.clone(),
            });
            let payload = WorkObjectProposedPayload {
                engagement_id: EngagementId::new(format!(
                    "{}:sha256:{}",
                    id_prefix::ENGAGEMENT,
                    sha256_bytes_hex(task.task_attempt_id.as_str().as_bytes())
                )),
                work_object: WorkObjectProposal::TaskAttempt {
                    task_attempt_id: task.task_attempt_id.clone(),
                    project_path: "pointbreak-longitudinal-fixture".to_owned(),
                    claude_session_uuid: format!(
                        "longitudinal-{}-{:06}-{}",
                        self.spec.shape.namespace(),
                        self.spec.block,
                        self.events.len()
                    ),
                    initial_prompt_hash: format!(
                        "sha256:{}",
                        deterministic_digest(
                            self.spec.shape,
                            self.spec.block,
                            self.events.len() as u64,
                            "task-prompt"
                        )?
                    ),
                    predecessor: task.predecessor,
                    base_state_fingerprint: None,
                    source_speaker: Some(SourceSpeaker::User),
                },
            };
            let event = ShoreEvent::new(
                EventType::WorkObjectProposed,
                WorkObjectProposedPayload::idempotency_key(&subject)?,
                EventTarget::for_generative_move(
                    self.journal_id.clone(),
                    EngagementType::Task,
                    subject,
                    Some(self.track()),
                )?,
                self.writer(),
                payload,
                self.occurred_at()?,
            )?;
            self.push(event)?;
        }
        Ok(())
    }

    fn push_review_observations(&mut self, count: usize) -> Result<()> {
        for ordinal in 0..count {
            let revision = self.revisions[ordinal % self.revisions.len()].clone();
            let target = ReviewTargetRef::Revision {
                revision_id: revision.revision_id.clone(),
            };
            let body = self.next_body("observation")?;
            if self.removed.len() == 1
                && let Some(prepared) = body.prepared.clone()
            {
                self.removed.push(prepared);
            }
            let supersedes_observation_ids = if (16..32).contains(&ordinal) {
                vec![self.observations[ordinal - 16].clone()]
            } else {
                Vec::new()
            };
            let responds_to_observation_ids = if (32..48).contains(&ordinal) {
                vec![self.observations[ordinal - 32].clone()]
            } else {
                Vec::new()
            };
            let title = format!("Longitudinal observation {ordinal:04}");
            let track = self.track();
            let writer = self.writer();
            let observation_id = build_observation_id(ObservationIdMaterial {
                track_id: &track,
                target: &target,
                title: &title,
                body_content_hash: Some(&body.content_hash),
                body_content_type: None,
                tags: &[format!("tag-{}", ordinal % 4)],
                confidence: Some(if ordinal % 2 == 0 { "high" } else { "medium" }),
                supersedes_observation_ids: &supersedes_observation_ids,
                responds_to_observation_ids: &responds_to_observation_ids,
                writer_actor_id: writer.actor_id.as_str(),
            })?;
            let event = ShoreEvent::new(
                EventType::ReviewObservationRecorded,
                ReviewObservationRecordedPayload::idempotency_key(
                    &revision.revision_id,
                    &track,
                    observation_id.as_str(),
                ),
                EventTarget::for_subject(
                    self.journal_id.clone(),
                    TargetRef::Review(target.clone()),
                    Some(track),
                )?,
                writer,
                ReviewObservationRecordedPayload {
                    observation_id: observation_id.clone(),
                    target,
                    title,
                    body: body.inline,
                    body_content_type: BodyContentType::TextPlain,
                    body_artifact_path: body.artifact_path,
                    body_byte_size: Some(body.byte_size),
                    body_content_hash: Some(body.content_hash),
                    tags: vec![format!("tag-{}", ordinal % 4)],
                    confidence: Some(if ordinal % 2 == 0 {
                        "high".to_owned()
                    } else {
                        "medium".to_owned()
                    }),
                    supersedes_observation_ids,
                    responds_to_observation_ids,
                },
                self.occurred_at()?,
            )?;
            self.observations.push(observation_id);
            self.push(event)?;
        }
        Ok(())
    }

    fn push_assessments(&mut self, count: usize) -> Result<()> {
        for ordinal in 0..count {
            let revision = self.revisions[ordinal % self.revisions.len()].clone();
            let target = ReviewTargetRef::Revision {
                revision_id: revision.revision_id.clone(),
            };
            let body = self.next_body("assessment")?;
            let assessment = match ordinal % 4 {
                0 => ReviewAssessment::Accepted,
                1 => ReviewAssessment::AcceptedWithFollowUp,
                2 => ReviewAssessment::NeedsChanges,
                _ => ReviewAssessment::NeedsClarification,
            };
            let replaces_assessment_ids = if ordinal >= self.revisions.len() {
                vec![self.assessments[ordinal - self.revisions.len()].clone()]
            } else {
                Vec::new()
            };
            let track = self.track();
            let writer = self.writer();
            let assessment_id = build_assessment_id(AssessmentIdMaterial {
                track_id: &track,
                target: &target,
                assessment,
                summary_content_hash: Some(&body.content_hash),
                summary_content_type: None,
                replaces_assessment_ids: &replaces_assessment_ids,
                related_observation_ids: &[],
                related_input_request_ids: &[],
                writer_actor_id: writer.actor_id.as_str(),
            })?;
            let event = ShoreEvent::new(
                EventType::ReviewAssessmentRecorded,
                ReviewAssessmentRecordedPayload::idempotency_key(
                    &revision.revision_id,
                    &track,
                    assessment_id.as_str(),
                ),
                EventTarget::for_revision(
                    self.journal_id.clone(),
                    revision.revision_id,
                    Some(track),
                )?,
                writer,
                ReviewAssessmentRecordedPayload {
                    assessment_id: assessment_id.clone(),
                    target,
                    assessment,
                    summary: body.inline,
                    summary_content_type: BodyContentType::TextPlain,
                    summary_artifact_path: body.artifact_path,
                    summary_byte_size: Some(body.byte_size),
                    summary_content_hash: Some(body.content_hash),
                    replaces_assessment_ids,
                    related_observation_ids: Vec::new(),
                    related_input_request_ids: Vec::new(),
                },
                self.occurred_at()?,
            )?;
            self.assessments.push(assessment_id);
            self.push(event)?;
        }
        Ok(())
    }

    fn push_input_requests(&mut self, count: usize, task_count: usize) -> Result<()> {
        for ordinal in 0..count {
            let body = self.next_body("input-request")?;
            let reason_code = match ordinal % 9 {
                0 => InputRequestReasonCode::AmbiguousState,
                1 => InputRequestReasonCode::UnsafeAction,
                2 => InputRequestReasonCode::StaleRevision,
                3 => InputRequestReasonCode::FailedGate,
                4 => InputRequestReasonCode::ExternalSideEffect,
                5 => InputRequestReasonCode::ConflictingEvent,
                6 => InputRequestReasonCode::MissingPermission,
                7 => InputRequestReasonCode::ManualDecisionRequired,
                _ => InputRequestReasonCode::InsufficientEvidence,
            };
            let title = format!("Longitudinal input request {ordinal:04}");
            let track = self.track();
            let writer = self.writer();
            let assertion_mode = if ordinal % 3 == 0 {
                AssertionMode::Operative
            } else {
                AssertionMode::Advisory
            };
            let task_target = (ordinal < task_count).then(|| {
                let task = &self.tasks[ordinal % self.tasks.len()];
                TaskTargetRef::TaskAttempt {
                    task_attempt_id: task.task_attempt_id.clone(),
                }
            });
            let (target, revision_id) = if task_target.is_some() {
                (
                    ReviewTargetRef::Revision {
                        revision_id: self.revisions[0].revision_id.clone(),
                    },
                    None,
                )
            } else {
                let revision =
                    self.revisions[(ordinal - task_count) % self.revisions.len()].clone();
                (
                    ReviewTargetRef::Revision {
                        revision_id: revision.revision_id.clone(),
                    },
                    Some(revision.revision_id),
                )
            };
            let input_request_id = build_input_request_id(InputRequestIdMaterial {
                track_id: &track,
                target: &target,
                assertion_mode,
                reason_code,
                title: &title,
                body_content_hash: Some(&body.content_hash),
                body_content_type: None,
                writer_actor_id: writer.actor_id.as_str(),
            })?;
            let idempotency_key = if let Some(task_target) = &task_target {
                let task_attempt_id = match task_target {
                    TaskTargetRef::TaskAttempt { task_attempt_id } => task_attempt_id,
                    TaskTargetRef::Checkpoint { .. } => unreachable!(),
                };
                InputRequestOpenedPayload::idempotency_key_for_work_object(
                    task_attempt_id,
                    WorkObjectType::TaskAttempt,
                    input_request_id.as_str(),
                )
            } else {
                InputRequestOpenedPayload::idempotency_key(
                    revision_id.as_ref().expect("review request revision"),
                    &track,
                    input_request_id.as_str(),
                )
            };
            let event_target = if let Some(task_target) = &task_target {
                EventTarget::for_subject(
                    self.journal_id.clone(),
                    TargetRef::Task(task_target.clone()),
                    Some(track),
                )?
            } else {
                EventTarget::for_subject(
                    self.journal_id.clone(),
                    TargetRef::Review(target.clone()),
                    Some(track),
                )?
            };
            let event = ShoreEvent::new(
                EventType::InputRequestOpened,
                idempotency_key,
                event_target,
                writer,
                InputRequestOpenedPayload {
                    input_request_id: input_request_id.clone(),
                    target,
                    task_target,
                    reason_code,
                    title,
                    body: body.inline,
                    body_content_type: BodyContentType::TextPlain,
                    body_artifact_path: body.artifact_path,
                    body_byte_size: Some(body.byte_size),
                    body_content_hash: Some(body.content_hash),
                    target_fingerprint: Some(format!(
                        "sha256:{}",
                        deterministic_digest(
                            self.spec.shape,
                            self.spec.block,
                            ordinal as u64,
                            "request-fingerprint"
                        )?
                    )),
                },
                self.occurred_at()?,
            )?
            .with_assertion_mode(assertion_mode);
            self.input_requests.push(input_request_id);
            self.push(event)?;
        }
        Ok(())
    }

    fn push_input_responses(&mut self, count: usize, task_count: usize) -> Result<()> {
        for ordinal in 0..count {
            let request_index = if count == 16 {
                match ordinal {
                    0..=3 => ordinal,
                    4 | 5 => 6,
                    6 | 7 => 7,
                    _ => ordinal,
                }
            } else {
                ordinal
            };
            let input_request_id = self.input_requests[request_index].clone();
            let body = self.next_body("input-response")?;
            let outcome = match ordinal % 5 {
                0 => InputRequestResponseOutcome::Approved,
                1 => InputRequestResponseOutcome::Rejected,
                2 => InputRequestResponseOutcome::Dismissed,
                3 => InputRequestResponseOutcome::Superseded,
                _ => InputRequestResponseOutcome::Abandoned,
            };
            let writer = self.writer();
            let response_id = build_input_request_response_id(InputRequestResponseIdMaterial {
                input_request_id: &input_request_id,
                outcome,
                reason_content_hash: Some(&body.content_hash),
                reason_content_type: None,
                writer_actor_id: writer.actor_id.as_str(),
            })?;
            let is_task = ordinal < task_count;
            let task_target = is_task.then(|| {
                let task = &self.tasks[ordinal % self.tasks.len()];
                TaskTargetRef::TaskAttempt {
                    task_attempt_id: task.task_attempt_id.clone(),
                }
            });
            let revision_id = (!is_task).then(|| {
                self.revisions[ordinal % self.revisions.len()]
                    .revision_id
                    .clone()
            });
            let event_target = if let Some(task_target) = &task_target {
                EventTarget::for_subject(
                    self.journal_id.clone(),
                    TargetRef::Task(task_target.clone()),
                    Some(self.track()),
                )?
            } else {
                EventTarget::for_revision(
                    self.journal_id.clone(),
                    revision_id.clone().expect("review response revision"),
                    Some(self.track()),
                )?
            };
            let event = ShoreEvent::new(
                EventType::InputRequestResponded,
                InputRequestRespondedPayload::idempotency_key(
                    &input_request_id,
                    response_id.as_str(),
                ),
                event_target,
                writer,
                InputRequestRespondedPayload {
                    input_request_response_id: response_id,
                    input_request_id,
                    revision_id,
                    task_target,
                    outcome,
                    reason: body.inline,
                    reason_content_type: BodyContentType::TextPlain,
                    reason_artifact_path: body.artifact_path,
                    reason_byte_size: Some(body.byte_size),
                    reason_content_hash: Some(body.content_hash),
                    target_fingerprint: None,
                },
                self.occurred_at()?,
            )?;
            self.push(event)?;
        }
        Ok(())
    }

    fn push_ref_associations(&mut self, count: usize, withdrawal_count: usize) -> Result<()> {
        for ordinal in 0..count {
            let revision = self.revisions[ordinal % self.revisions.len()].clone();
            let ref_name = format!(
                "refs/heads/longitudinal/{}/{:06}/{ordinal:03}",
                self.spec.shape.namespace(),
                self.spec.block
            );
            let head_oid =
                deterministic_oid(self.spec.shape, self.spec.block, ordinal as u64, "ref");
            let association_id =
                build_ref_association_id(&revision.revision_id, &ref_name, &head_oid)?;
            let target = ReviewTargetRef::Revision {
                revision_id: revision.revision_id.clone(),
            };
            let event = ShoreEvent::new(
                EventType::RevisionRefAssociated,
                RevisionRefAssociatedPayload::idempotency_key(
                    &revision.revision_id,
                    &ref_name,
                    &head_oid,
                ),
                EventTarget::for_revision(
                    self.journal_id.clone(),
                    revision.revision_id,
                    Some(self.track()),
                )?,
                self.writer(),
                RevisionRefAssociatedPayload {
                    ref_association_id: association_id.clone(),
                    target,
                    ref_name,
                    head_oid,
                },
                self.occurred_at()?,
            )?;
            self.ref_associations.push(association_id);
            self.push(event)?;
        }
        for ordinal in 0..withdrawal_count {
            let revision = self.revisions[ordinal % self.revisions.len()].clone();
            let association_id = self.ref_associations[ordinal].clone();
            let event = ShoreEvent::new(
                EventType::RevisionRefWithdrawn,
                RevisionRefWithdrawnPayload::idempotency_key(&association_id),
                EventTarget::for_revision(
                    self.journal_id.clone(),
                    revision.revision_id.clone(),
                    Some(self.track()),
                )?,
                self.writer(),
                RevisionRefWithdrawnPayload {
                    ref_withdrawal_id: build_ref_withdrawal_id(
                        &revision.revision_id,
                        &association_id,
                    )?,
                    target: ReviewTargetRef::Revision {
                        revision_id: revision.revision_id,
                    },
                    ref_association_id: association_id,
                },
                self.occurred_at()?,
            )?;
            self.push(event)?;
        }
        Ok(())
    }

    fn push_commit_associations(&mut self, count: usize, withdrawal_count: usize) -> Result<()> {
        for ordinal in 0..count {
            let revision = self.revisions[ordinal % self.revisions.len()].clone();
            let commit_oid =
                deterministic_oid(self.spec.shape, self.spec.block, ordinal as u64, "commit");
            let tree_oid =
                deterministic_oid(self.spec.shape, self.spec.block, ordinal as u64, "tree");
            let association_id = build_commit_association_id(&revision.revision_id, &commit_oid)?;
            let event = ShoreEvent::new(
                EventType::RevisionCommitAssociated,
                RevisionCommitAssociatedPayload::idempotency_key(
                    &revision.revision_id,
                    &commit_oid,
                ),
                EventTarget::for_revision(
                    self.journal_id.clone(),
                    revision.revision_id.clone(),
                    Some(self.track()),
                )?,
                self.writer(),
                RevisionCommitAssociatedPayload {
                    commit_association_id: association_id.clone(),
                    target: ReviewTargetRef::Revision {
                        revision_id: revision.revision_id,
                    },
                    commit: ReviewEndpoint::GitCommit {
                        commit_oid,
                        tree_oid,
                    },
                },
                self.occurred_at()?,
            )?;
            self.commit_associations.push(association_id);
            self.push(event)?;
        }
        for ordinal in 0..withdrawal_count {
            let revision = self.revisions[ordinal % self.revisions.len()].clone();
            let association_id =
                self.commit_associations[count - withdrawal_count + ordinal].clone();
            let event = ShoreEvent::new(
                EventType::RevisionCommitWithdrawn,
                RevisionCommitWithdrawnPayload::idempotency_key(&association_id),
                EventTarget::for_revision(
                    self.journal_id.clone(),
                    revision.revision_id.clone(),
                    Some(self.track()),
                )?,
                self.writer(),
                RevisionCommitWithdrawnPayload {
                    commit_withdrawal_id: build_commit_withdrawal_id(
                        &revision.revision_id,
                        &association_id,
                    )?,
                    target: ReviewTargetRef::Revision {
                        revision_id: revision.revision_id,
                    },
                    commit_association_id: association_id,
                },
                self.occurred_at()?,
            )?;
            self.push(event)?;
        }
        Ok(())
    }

    fn push_validations(&mut self, count: usize) -> Result<()> {
        let log_count = self.spec.shape.log_count() as usize;
        let logs = (0..log_count)
            .map(|ordinal| self.prepare_validation_log(ordinal as u64))
            .collect::<Result<Vec<_>>>()?;
        self.removed.push(logs[0].clone());
        self.content.extend(logs.clone());

        for ordinal in 0..count {
            let revision = self.revisions[ordinal % self.revisions.len()].clone();
            let body = self.next_body("validation")?;
            let status = match ordinal % 20 {
                0..=13 => ValidationStatus::Passed,
                14..=16 => ValidationStatus::Failed,
                17..=18 => ValidationStatus::Errored,
                _ => ValidationStatus::Skipped,
            };
            let trigger = match ordinal % 3 {
                0 => ValidationTrigger::Manual,
                1 => ValidationTrigger::Push,
                _ => ValidationTrigger::PullRequest,
            };
            let log_hashes = if let Some(log) = logs.get(ordinal) {
                vec![log.content_hash().to_owned()]
            } else {
                Vec::new()
            };
            let track = self.track();
            let writer = self.writer();
            let check_name = format!("longitudinal-check-{}", ordinal % 8);
            let validation_check_id = build_validation_check_id(ValidationCheckIdMaterial {
                revision_id: &revision.revision_id,
                track_id: &track,
                check_name: &check_name,
                command: Some("pointbreak longitudinal check"),
                status,
                exit_code: (status == ValidationStatus::Passed).then_some(0),
                trigger,
                source_fingerprint: None,
                summary_content_hash: Some(&body.content_hash),
                summary_content_type: None,
                started_at: None,
                completed_at: None,
                log_artifact_content_hashes: &log_hashes,
                writer_actor_id: writer.actor_id.as_str(),
            })?;
            let event = ShoreEvent::new(
                EventType::ValidationCheckRecorded,
                ValidationCheckRecordedPayload::idempotency_key(
                    &revision.revision_id,
                    &track,
                    validation_check_id.as_str(),
                ),
                EventTarget::for_revision(
                    self.journal_id.clone(),
                    revision.revision_id.clone(),
                    Some(track),
                )?,
                writer,
                ValidationCheckRecordedPayload {
                    validation_check_id,
                    target: ValidationTarget::Revision {
                        revision_id: revision.revision_id,
                    },
                    check_name,
                    command: Some("pointbreak longitudinal check".to_owned()),
                    status,
                    exit_code: (status == ValidationStatus::Passed).then_some(0),
                    trigger,
                    source_fingerprint: None,
                    summary: body.inline,
                    summary_content_type: BodyContentType::TextPlain,
                    summary_artifact_path: body.artifact_path,
                    summary_byte_size: Some(body.byte_size),
                    summary_content_hash: Some(body.content_hash),
                    started_at: None,
                    completed_at: None,
                    log_artifact_content_hashes: log_hashes,
                },
                self.occurred_at()?,
            )?;
            self.push(event)?;
        }
        Ok(())
    }

    fn push_task_checkpoints(&mut self, count: usize) -> Result<()> {
        for ordinal in 0..count {
            let task = self.tasks[ordinal % self.tasks.len()].clone();
            let checkpoint_id = CheckpointId::new(format!(
                "{}:sha256:{}",
                id_prefix::CHECKPOINT,
                deterministic_digest(
                    self.spec.shape,
                    self.spec.block,
                    ordinal as u64,
                    "checkpoint"
                )?
            ));
            let event = ShoreEvent::new(
                EventType::TaskCheckpointCaptured,
                TaskCheckpointCapturedPayload::idempotency_key_for_work_object(
                    &task.task_attempt_id,
                    WorkObjectType::TaskAttempt,
                    checkpoint_id.as_str(),
                ),
                EventTarget::for_subject(
                    self.journal_id.clone(),
                    TargetRef::Task(TaskTargetRef::Checkpoint {
                        checkpoint_id: checkpoint_id.clone(),
                    }),
                    Some(self.track()),
                )?,
                self.writer(),
                TaskCheckpointCapturedPayload {
                    checkpoint_id: checkpoint_id.clone(),
                    parent_task_attempt_id: task.task_attempt_id,
                    assistant_message_id: format!("assistant-{ordinal:04}"),
                    tool_use_ids: vec![format!("tool-{ordinal:04}")],
                    checkpoint_fingerprint: Some(format!(
                        "sha256:{}",
                        deterministic_digest(
                            self.spec.shape,
                            self.spec.block,
                            ordinal as u64,
                            "checkpoint-fingerprint"
                        )?
                    )),
                    source_speaker: Some(SourceSpeaker::Agent),
                },
                self.occurred_at()?,
            )?;
            self.checkpoints.push(checkpoint_id);
            self.push(event)?;
        }
        Ok(())
    }

    fn push_task_observations(&mut self, count: usize) -> Result<()> {
        for ordinal in 0..count {
            let task = self.tasks[ordinal % self.tasks.len()].clone();
            let checkpoint_id = self.checkpoints[ordinal % self.checkpoints.len()].clone();
            let body = self.next_body("task-observation")?;
            let observation_id = crate::model::ObservationId::new(format!(
                "{}:sha256:{}",
                id_prefix::OBSERVATION,
                deterministic_digest(
                    self.spec.shape,
                    self.spec.block,
                    ordinal as u64,
                    "task-observation"
                )?
            ));
            let event = ShoreEvent::new(
                EventType::TaskObservationRecorded,
                TaskObservationRecordedPayload::idempotency_key_for_work_object(
                    &task.task_attempt_id,
                    WorkObjectType::TaskAttempt,
                    observation_id.as_str(),
                ),
                EventTarget::for_subject(
                    self.journal_id.clone(),
                    TargetRef::Task(TaskTargetRef::Checkpoint {
                        checkpoint_id: checkpoint_id.clone(),
                    }),
                    Some(self.track()),
                )?,
                self.writer(),
                TaskObservationRecordedPayload {
                    observation_id,
                    checkpoint_id: Some(checkpoint_id),
                    title: format!("Longitudinal task observation {ordinal:04}"),
                    body: body.inline,
                    body_artifact_path: body.artifact_path,
                    body_byte_size: Some(body.byte_size),
                    body_content_hash: Some(body.content_hash),
                    source_speaker: Some(SourceSpeaker::Agent),
                },
                self.occurred_at()?,
            )?;
            self.push(event)?;
        }
        Ok(())
    }

    fn push_signature_carriers(&mut self, count: usize) -> Result<()> {
        let targets = self.events[1..self.events.len().min(9)].to_vec();
        for ordinal in 0..count {
            let target_index = if ordinal == 1 {
                0
            } else {
                ordinal % targets.len()
            };
            let target = &targets[target_index];
            let signer = deterministic_signer(ordinal % 4);
            let attesting_signer = signer.signer_id().clone();
            let tbs = EventToBeSigned::from_event(target, &attesting_signer)?;
            let pae = event_signature_pre_authentication_encoding(&tbs)?;
            let attestation = EventSignature::ed25519_v1(signer.sign_event_message(&pae)?);
            let target_record_hash = target.event_record_hash()?;
            let payload = EventSignatureRecordedPayload {
                target_event_id: target.event_id.clone(),
                target_event_record_hash: target_record_hash.clone(),
                attesting_signer: attesting_signer.clone(),
                attestation: attestation.clone(),
                inclusion_proof: None,
            };
            let event = ShoreEvent::new(
                EventType::EventSignatureRecorded,
                EventSignatureRecordedPayload::idempotency_key(
                    &target_record_hash,
                    &attesting_signer,
                    attestation.sig.as_str(),
                ),
                EventTarget::for_journal(self.journal_id.clone()),
                self.writer(),
                payload,
                self.occurred_at()?,
            )?;
            self.push_unsigned(event);
        }
        Ok(())
    }

    fn push_removals(&mut self) -> Result<()> {
        if self.removed.len() != 3 {
            return Err(ShoreError::Message(format!(
                "longitudinal block must remove one object, body, and log; got {} targets",
                self.removed.len()
            )));
        }
        let removed = self.removed.clone();
        for content in removed {
            let event = ShoreEvent::new(
                EventType::ArtifactRemoved,
                ArtifactRemovedPayload::idempotency_key(content.content_hash()),
                EventTarget::for_journal(self.journal_id.clone()),
                self.writer(),
                ArtifactRemovedPayload {
                    content_hash: content.content_hash().to_owned(),
                },
                self.occurred_at()?,
            )?;
            self.push_unsigned(event);
        }
        Ok(())
    }

    fn finish(self) -> Result<PreparedLongitudinalRecordV1> {
        if self.events.len() as u64 != self.spec.shape.event_count()
            || self.body_ordinal != self.spec.shape.body_count()
            || self.object_ordinal != self.spec.shape.object_count()
            || self.log_ordinal != self.spec.shape.log_count()
        {
            return Err(ShoreError::Message(format!(
                "longitudinal block drift for {} block {}: events={}, bodies={}, objects={}, logs={}",
                self.spec.shape.namespace(),
                self.spec.block,
                self.events.len(),
                self.body_ordinal,
                self.object_ordinal,
                self.log_ordinal
            )));
        }
        let revision_count = self.revisions.len() as u64;
        let task_attempt_count = self.tasks.len() as u64;
        Ok(PreparedLongitudinalRecordV1 {
            events: self.events,
            content: self.content,
            removed: self.removed,
            revision_count,
            task_attempt_count,
            body_fact_count: self.body_ordinal,
            external_body_count: self.external_body_count,
            object_artifact_count: self.object_ordinal,
            decoded_body_bytes: self.decoded_body_bytes,
            decoded_object_target_bytes: self.decoded_object_target_bytes,
        })
    }

    fn next_body(&mut self, domain: &str) -> Result<BodyFixtureV1> {
        let global_ordinal = self.global_body_ordinal();
        let size = BODY_SIZES[(global_ordinal % BODY_SIZES.len() as u64) as usize];
        let body = deterministic_text(
            self.spec.shape,
            self.spec.block,
            global_ordinal,
            domain,
            size,
        )?;
        let content_hash = format!("sha256:{}", sha256_bytes_hex(body.as_bytes()));
        let (inline, artifact_path, artifact_bytes, byte_size) = staged_body(Some(body.as_str()))?;
        let prepared = match (artifact_path.clone(), artifact_bytes) {
            (Some(relative_path), Some(_)) => {
                self.external_body_count += 1;
                let prepared = PreparedContentV1::ExternalBody {
                    shape: self.spec.shape,
                    block: self.spec.block,
                    global_ordinal,
                    domain: body_domain_owned(domain),
                    relative_path,
                    content_hash: content_hash.clone(),
                };
                self.content.push(prepared.clone());
                Some(prepared)
            }
            (None, None) => None,
            _ => {
                return Err(ShoreError::Message(
                    "staged longitudinal body returned partial artifact fields".to_owned(),
                ));
            }
        };
        self.body_ordinal += 1;
        self.decoded_body_bytes += size as u64;
        Ok(BodyFixtureV1 {
            inline,
            artifact_path,
            byte_size: byte_size.expect("present body has byte size"),
            content_hash,
            prepared,
        })
    }

    fn prepare_validation_log(&mut self, ordinal: u64) -> Result<PreparedContentV1> {
        let global_ordinal = self.spec.block * self.spec.shape.log_count() + self.log_ordinal;
        let body = deterministic_text(
            self.spec.shape,
            self.spec.block,
            global_ordinal,
            "validation-log",
            1_024,
        )?;
        let content_hash = format!("sha256:{}", sha256_bytes_hex(body.as_bytes()));
        let relative_path = format!(
            "artifacts/notes/{}.json",
            content_hash.trim_start_matches("sha256:")
        );
        self.log_ordinal += 1;
        Ok(PreparedContentV1::ValidationLog {
            shape: self.spec.shape,
            block: self.spec.block,
            ordinal,
            global_ordinal,
            relative_path,
            content_hash,
        })
    }

    fn global_body_ordinal(&self) -> u64 {
        let offset = match self.spec.shape {
            LongitudinalRecordShapeV1::CapacityL100O10K => 3,
            LongitudinalRecordShapeV1::Workload | LongitudinalRecordShapeV1::CapacityV1 => 0,
        };
        offset + self.spec.block * self.spec.shape.body_count() + self.body_ordinal
    }

    fn global_object_ordinal(&self) -> u64 {
        self.spec.block * self.spec.shape.object_count() + self.object_ordinal
    }

    fn occurred_at(&self) -> Result<String> {
        FixedLongitudinalClockV1::new()
            .occurred_at(
                self.spec.block,
                self.events.len() as u64,
                self.spec.shape.event_count(),
            )
            .map_err(|error| ShoreError::Message(error.to_string()))
    }

    fn writer(&self) -> Writer {
        let actor_ordinal = self.events.len() % 8;
        Writer {
            actor_id: ActorId::new(format!("actor:agent:longitudinal-{actor_ordinal}")),
            producer: WriterProducer {
                name: "pointbreak".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
            },
        }
    }

    fn track(&self) -> TrackId {
        TrackId::new(format!("agent:longitudinal-{}", self.events.len() % 6))
    }

    fn push(&mut self, mut event: ShoreEvent) -> Result<()> {
        if (1..=48).contains(&self.events.len()) {
            let signer = deterministic_signer(self.events.len() % 4);
            super::sign_event_if_requested(
                &mut event,
                &super::EventSigningOptions::sign_with(signer),
            )?;
        }
        self.events.push(event);
        Ok(())
    }

    fn push_unsigned(&mut self, event: ShoreEvent) {
        self.events.push(event);
    }
}

pub(crate) fn write_longitudinal_records_v1(
    repo: &Path,
    records: &[PreparedLongitudinalRecordV1],
) -> Result<LongitudinalWriteReceiptV1> {
    let write_store = resolve_write_store(repo)?;
    let storage = LocalStorage::new(write_store.store_dir());
    prepare_write_landing(&write_store, &storage)?;
    let content_store = ContentArtifacts::from_backend(write_store.backend());

    for record in records {
        for content in &record.content {
            publish_content(&content_store, content)?;
        }
    }

    let events = records
        .iter()
        .flat_map(|record| record.events.iter().cloned())
        .collect::<Vec<_>>();
    let result = ingest_events_with_clock(
        IngestEventsOptions::new(repo, events.clone()).with_trust_set(longitudinal_trust_set()?),
        &FixedBenchmarkIngestClock,
    )?;

    for record in records {
        for content in &record.removed {
            match content_store.remove(content.relative_path())? {
                RemoveOutcome::Removed | RemoveOutcome::Missing => {}
            }
        }
    }

    let read_store = resolve_read_store(repo)?;
    let event_store = EventStore::from_backend(read_store.backend());
    let listed = event_store.list_events()?;
    let recomputed_state = SessionState::from_events(&listed)?;
    let stored_state: SessionState =
        LocalStorage::new(read_store.store_dir()).read_json(Path::new("state.json"))?;
    if recomputed_state != stored_state {
        return Err(ShoreError::Message(
            "longitudinal state.json does not match strict replay".to_owned(),
        ));
    }
    if listed.len() != events.len() {
        return Err(ShoreError::Message(format!(
            "longitudinal strict replay count mismatch: expected {}, got {}",
            events.len(),
            listed.len()
        )));
    }
    if listed.iter().any(|event| {
        event.ingest.as_ref().is_none_or(|ingest| {
            ingest.via != IngestVia::IngestEvents
                || ingest.received_at != LONGITUDINAL_FIXED_INGEST_RECEIVED_AT_V1
        })
    }) {
        return Err(ShoreError::Message(
            "longitudinal event is missing the fixed ingest provenance".to_owned(),
        ));
    }

    let stored_by_id = listed
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect::<BTreeMap<_, _>>();
    let mut ordered_events = Vec::with_capacity(events.len());
    let mut event_carriers = Vec::with_capacity(events.len());
    for generated in &events {
        let stored = stored_by_id
            .get(generated.event_id.as_str())
            .ok_or_else(|| ShoreError::Message("stored longitudinal event missing".to_owned()))?;
        let decoded = canonical_json_bytes(&serde_json::to_value(stored)?)?;
        let raw = serde_json::to_vec(stored)?;
        ordered_events.push(LongitudinalEventIdentityV1 {
            event_id: stored.event_id.as_str().to_owned(),
            canonical_decoded_sha256: sha256_bytes_hex(&decoded),
        });
        event_carriers.push(LongitudinalEventCarrierV1 {
            event_id: stored.event_id.as_str().to_owned(),
            logical_key_sha256: sha256_bytes_hex(stored.idempotency_key.as_bytes()),
            raw_sha256: sha256_bytes_hex(&raw),
            raw_bytes: raw.len() as u64,
        });
    }

    let removed_refs = records
        .iter()
        .flat_map(|record| record.removed.iter().map(PreparedContentV1::relative_path))
        .collect::<BTreeSet<_>>();
    let mut content_inventory = Vec::new();
    for content in records.iter().flat_map(|record| &record.content) {
        if removed_refs.contains(content.relative_path()) {
            if content_store
                .get_if_exists(content.relative_path())?
                .is_some()
            {
                return Err(ShoreError::Message(format!(
                    "removed longitudinal content still exists: {}",
                    content.relative_path()
                )));
            }
            continue;
        }
        let raw = content_store
            .get_if_exists(content.relative_path())?
            .ok_or_else(|| {
                ShoreError::Message(format!(
                    "present longitudinal content is missing: {}",
                    content.relative_path()
                ))
            })?;
        content_inventory.push(inventory_entry(content, &raw)?);
    }
    content_inventory.sort_by(|left, right| left.logical_key.cmp(&right.logical_key));

    let mut removed_content_sha256 = records
        .iter()
        .flat_map(|record| record.removed.iter())
        .map(|content| {
            content
                .content_hash()
                .trim_start_matches("sha256:")
                .to_owned()
        })
        .collect::<Vec<_>>();
    removed_content_sha256.sort();

    let event_set_sha256 = event_set_hash_for_events(&listed)?
        .trim_start_matches("sha256:")
        .to_owned();
    let ordered_journal_sha256 = canonical_sha256(
        &listed
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<Vec<_>>(),
    )?;
    let state_sha256 = canonical_sha256(&stored_state)?;
    let projection_sha256 = canonical_sha256(&ProjectionReceiptV1::from(&stored_state))?;
    let content_inventory_sha256 = canonical_sha256(&content_inventory)?;
    let strict = LongitudinalStrictSemanticReceiptV1 {
        event_set_sha256,
        ordered_journal_sha256,
        state_sha256,
        projection_sha256,
        content_inventory_sha256,
    };

    let mut by_type = BTreeMap::new();
    for event in &listed {
        *by_type
            .entry(event.event_type.as_str().to_owned())
            .or_default() += 1;
    }

    Ok(LongitudinalWriteReceiptV1 {
        ordered_events,
        event_carriers,
        content_inventory,
        removed_content_sha256,
        strict,
        events_created: result.events_created as u64,
        events_existing: result.events_existing as u64,
        event_count: listed.len() as u64,
        revision_count: records.iter().map(|record| record.revision_count).sum(),
        task_attempt_count: records.iter().map(|record| record.task_attempt_count).sum(),
        body_fact_count: records.iter().map(|record| record.body_fact_count).sum(),
        external_body_count: records
            .iter()
            .map(|record| record.external_body_count)
            .sum(),
        object_artifact_count: records
            .iter()
            .map(|record| record.object_artifact_count)
            .sum(),
        decoded_body_bytes: records.iter().map(|record| record.decoded_body_bytes).sum(),
        decoded_object_target_bytes: records
            .iter()
            .map(|record| record.decoded_object_target_bytes)
            .sum(),
        by_type,
    })
}

fn publish_content(content_store: &ContentArtifacts, content: &PreparedContentV1) -> Result<()> {
    match content {
        PreparedContentV1::ExternalBody {
            shape,
            block,
            global_ordinal,
            domain,
            relative_path,
            content_hash,
            ..
        } => {
            let size = BODY_SIZES[(*global_ordinal % BODY_SIZES.len() as u64) as usize];
            let body = deterministic_text(*shape, *block, *global_ordinal, domain, size)?;
            let (inline, staged_path, bytes, _) = staged_body(Some(body.as_str()))?;
            if inline.is_some()
                || staged_path.as_deref() != Some(relative_path)
                || format!("sha256:{}", sha256_bytes_hex(body.as_bytes())) != *content_hash
            {
                return Err(ShoreError::Message(
                    "longitudinal external body regeneration drifted".to_owned(),
                ));
            }
            content_store.put_note_body(
                relative_path,
                bytes.as_deref().expect("external body has envelope bytes"),
            )?;
        }
        PreparedContentV1::Object {
            shape,
            block,
            global_ordinal,
            relative_path,
            content_hash,
            ..
        } => {
            let target_size = object_target_size(*global_ordinal);
            let (files, decoded_target_bytes) =
                object_files(*shape, *block, *global_ordinal, target_size);
            if decoded_target_bytes != target_size {
                return Err(ShoreError::Message(
                    "longitudinal object regeneration size drifted".to_owned(),
                ));
            }
            let object_id = super::fingerprint::object_identity(&files);
            let artifact = build_object_artifact_v2(DiffSnapshot::new(
                object_review_id(*shape, *global_ordinal),
                object_id,
                files,
            ))?;
            if artifact.content_hash != *content_hash
                || object_content_ref(content_hash)? != *relative_path
            {
                return Err(ShoreError::Message(
                    "longitudinal object artifact regeneration drifted".to_owned(),
                ));
            }
            let bytes = serde_json::to_vec(&artifact)?;
            content_store.put_object(relative_path, &bytes, artifact)?;
        }
        PreparedContentV1::ValidationLog {
            shape,
            block,
            ordinal,
            global_ordinal,
            relative_path,
            content_hash,
        } => {
            let body =
                deterministic_text(*shape, *block, *global_ordinal, "validation-log", 1_024)?;
            if format!("sha256:{}", sha256_bytes_hex(body.as_bytes())) != *content_hash {
                return Err(ShoreError::Message(format!(
                    "longitudinal validation log {ordinal} regeneration drifted"
                )));
            }
            let bytes = NoteBodyEnvelope::new(body).to_json_bytes()?;
            content_store.put_note_body(relative_path, &bytes)?;
        }
    }
    Ok(())
}

fn inventory_entry(
    content: &PreparedContentV1,
    raw: &[u8],
) -> Result<LongitudinalInventoryEntryV1> {
    let (decoded, decoded_bytes) = match content {
        PreparedContentV1::ExternalBody { .. } | PreparedContentV1::ValidationLog { .. } => {
            let body = parse_note_body_artifact(raw)?.body;
            (sha256_bytes_hex(body.as_bytes()), body.len() as u64)
        }
        PreparedContentV1::Object { global_ordinal, .. } => {
            let artifact = decode_and_validate_object_artifact(raw)?;
            let target = artifact
                .snapshot
                .files
                .iter()
                .flat_map(|file| &file.hunks)
                .flat_map(|hunk| &hunk.rows)
                .map(|row| row.text.as_bytes())
                .collect::<Vec<_>>();
            let decoded_bytes = object_target_size(*global_ordinal) as u64;
            let mut hasher = Sha256::new();
            for bytes in target {
                hasher.update(bytes);
            }
            (format!("{:x}", hasher.finalize()), decoded_bytes)
        }
    };
    Ok(LongitudinalInventoryEntryV1 {
        logical_key: content.relative_path().to_owned(),
        kind: content.kind(),
        raw_sha256: sha256_bytes_hex(raw),
        decoded_sha256: decoded,
        raw_bytes: raw.len() as u64,
        decoded_bytes,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectionReceiptV1<'a> {
    journal_id: &'a JournalId,
    current_revision_id: &'a Option<RevisionId>,
    current_object_id: &'a Option<ObjectId>,
    revision_count: usize,
    event_count: usize,
    observation_count: usize,
    assessment_count: usize,
    validation_check_count: usize,
    input_request_count: usize,
    open_input_request_count: usize,
    open_operative_input_request_count: usize,
    diagnostics: &'a [super::ProjectionDiagnostic],
}

impl<'a> From<&'a SessionState> for ProjectionReceiptV1<'a> {
    fn from(state: &'a SessionState) -> Self {
        Self {
            journal_id: &state.journal_id,
            current_revision_id: &state.current_revision_id,
            current_object_id: &state.current_object_id,
            revision_count: state.revision_count,
            event_count: state.event_count,
            observation_count: state.observation_count,
            assessment_count: state.assessment_count,
            validation_check_count: state.validation_check_count,
            input_request_count: state.input_request_count,
            open_input_request_count: state.open_input_request_count,
            open_operative_input_request_count: state.open_operative_input_request_count,
            diagnostics: &state.diagnostics,
        }
    }
}

fn deterministic_signer(ordinal: usize) -> FileEd25519Signer {
    let mut hasher = Sha256::new();
    hasher.update(LONGITUDINAL_PUBLIC_SEED_HEX_V1.as_bytes());
    hasher.update(b"\npointbreak.longitudinal-workload.v1/signing-key/");
    hasher.update(ordinal.to_string().as_bytes());
    let seed: [u8; 32] = hasher.finalize().into();
    FileEd25519Signer::from_seed(seed)
}

fn longitudinal_trust_set() -> Result<TrustSet> {
    let signers = (0..4)
        .map(|ordinal| {
            deterministic_signer(ordinal)
                .signer_id()
                .as_str()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let allowed_signers = (0..8)
        .map(|ordinal| {
            (
                format!("actor:agent:longitudinal-{ordinal}"),
                serde_json::to_value(&signers).expect("signer ids serialize"),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    event_signature_trust_set(serde_json::Value::Object(
        [(
            "allowedSigners".to_owned(),
            serde_json::Value::Object(allowed_signers),
        )]
        .into_iter()
        .collect(),
    ))
}

fn deterministic_digest(
    shape: LongitudinalRecordShapeV1,
    block: u64,
    ordinal: u64,
    domain: &str,
) -> Result<String> {
    #[derive(Serialize)]
    struct Material<'a> {
        block: u64,
        domain: &'a str,
        ordinal: u64,
        seed: &'static str,
        shape: &'static str,
    }
    let bytes = canonical_json_bytes(&serde_json::to_value(Material {
        block,
        domain,
        ordinal,
        seed: LONGITUDINAL_PUBLIC_SEED_HEX_V1,
        shape: shape.namespace(),
    })?)?;
    Ok(sha256_bytes_hex(&bytes))
}

fn deterministic_text(
    shape: LongitudinalRecordShapeV1,
    block: u64,
    ordinal: u64,
    domain: &str,
    size: usize,
) -> Result<String> {
    let digest = deterministic_digest(shape, block, ordinal, domain)?;
    let prefix = format!(
        "lh1-needle-{:02}|{}|{:06}|{:012}|",
        ordinal % 64,
        shape.namespace(),
        block,
        ordinal
    );
    if prefix.len() > size {
        return Err(ShoreError::Message(
            "longitudinal deterministic text prefix exceeds target size".to_owned(),
        ));
    }
    let mut text = String::with_capacity(size);
    text.push_str(&prefix);
    while text.len() < size {
        let remaining = size - text.len();
        text.push_str(&digest[..remaining.min(digest.len())]);
    }
    Ok(text)
}

fn deterministic_oid(
    shape: LongitudinalRecordShapeV1,
    block: u64,
    ordinal: u64,
    domain: &str,
) -> String {
    deterministic_digest(shape, block, ordinal, domain)
        .expect("deterministic OID material serializes")[..40]
        .to_owned()
}

fn canonical_sha256<T: Serialize>(value: &T) -> Result<String> {
    Ok(sha256_bytes_hex(&canonical_json_bytes(
        &serde_json::to_value(value)?,
    )?))
}

fn object_target_size(global_ordinal: u64) -> usize {
    BODY_SIZES[(global_ordinal % BODY_SIZES.len() as u64) as usize] * 8
}

fn object_files(
    shape: LongitudinalRecordShapeV1,
    block: u64,
    global_ordinal: u64,
    target_size: usize,
) -> (Vec<DiffFile>, usize) {
    let case = (global_ordinal % 8) as usize;
    let marker = format!("{}-{:06}-{global_ordinal:012}", shape.namespace(), block);
    let mut files = Vec::new();
    match case {
        1 => files.push(diff_file(
            global_ordinal,
            0,
            FileStatus::Modified,
            (
                Some(format!("binary-{marker}.bin")),
                Some(format!("binary-{marker}.bin")),
            ),
            true,
            false,
            Vec::new(),
        )),
        2 => files.push(diff_file(
            global_ordinal,
            0,
            FileStatus::Renamed,
            (
                Some(format!("old-{marker}.txt")),
                Some(format!("new-{marker}.txt")),
            ),
            false,
            false,
            vec![(DiffRowKind::Removed, "old".to_owned())],
        )),
        3 => files.push(diff_file(
            global_ordinal,
            0,
            FileStatus::Modified,
            (
                Some(format!("mode-{marker}.sh")),
                Some(format!("mode-{marker}.sh")),
            ),
            false,
            true,
            Vec::new(),
        )),
        4 => {
            files.push(diff_file(
                global_ordinal,
                0,
                FileStatus::Added,
                (None, Some(format!("multi-a-{marker}.txt"))),
                false,
                false,
                vec![(DiffRowKind::Added, "multi-a".to_owned())],
            ));
            files.push(diff_file(
                global_ordinal,
                1,
                FileStatus::Added,
                (None, Some(format!("multi-b-{marker}.txt"))),
                false,
                false,
                vec![(DiffRowKind::Added, "multi-b".to_owned())],
            ));
        }
        6 => files.push(diff_file(
            global_ordinal,
            0,
            FileStatus::Modified,
            (
                Some(format!("context-{marker}.txt")),
                Some(format!("context-{marker}.txt")),
            ),
            false,
            false,
            vec![(DiffRowKind::Context, "context".to_owned())],
        )),
        _ => {}
    }
    let used = files
        .iter()
        .flat_map(|file| &file.hunks)
        .flat_map(|hunk| &hunk.rows)
        .map(|row| row.text.len())
        .sum::<usize>();
    let padding = "x".repeat(target_size - used);
    files.push(diff_file(
        global_ordinal,
        files.len(),
        FileStatus::Modified,
        (
            Some(format!("padding-{marker}.txt")),
            Some(format!("padding-{marker}.txt")),
        ),
        false,
        false,
        vec![(DiffRowKind::Added, padding)],
    ));
    let decoded = files
        .iter()
        .flat_map(|file| &file.hunks)
        .flat_map(|hunk| &hunk.rows)
        .map(|row| row.text.len())
        .sum();
    (files, decoded)
}

fn diff_file(
    global_ordinal: u64,
    file_ordinal: usize,
    status: FileStatus,
    paths: (Option<String>, Option<String>),
    is_binary: bool,
    is_mode_only: bool,
    rows: Vec<(DiffRowKind, String)>,
) -> DiffFile {
    let hunk_rows = rows
        .into_iter()
        .enumerate()
        .map(|(ordinal, (kind, text))| DiffRow {
            old_line: matches!(kind, DiffRowKind::Removed | DiffRowKind::Context)
                .then_some(ordinal as u32 + 1),
            new_line: matches!(kind, DiffRowKind::Added | DiffRowKind::Context)
                .then_some(ordinal as u32 + 1),
            kind,
            text,
        })
        .collect::<Vec<_>>();
    let hunks = if hunk_rows.is_empty() {
        Vec::new()
    } else {
        vec![ReviewHunk {
            id: HunkId::new(format!(
                "hunk:longitudinal:{global_ordinal:012}:{file_ordinal:02}"
            )),
            header: format!("@@ -1,{} +1,{} @@", hunk_rows.len(), hunk_rows.len()),
            old_start: 1,
            old_lines: hunk_rows
                .iter()
                .filter(|row| row.old_line.is_some())
                .count() as u32,
            new_start: 1,
            new_lines: hunk_rows
                .iter()
                .filter(|row| row.new_line.is_some())
                .count() as u32,
            rows: hunk_rows,
        }]
    };
    DiffFile {
        id: FileId::new(format!(
            "file:longitudinal:{global_ordinal:012}:{file_ordinal:02}"
        )),
        status,
        old_path: paths.0,
        new_path: paths.1,
        old_mode: Some("100644".to_owned()),
        new_mode: Some(if is_mode_only { "100755" } else { "100644" }.to_owned()),
        old_oid: None,
        new_oid: None,
        similarity: None,
        is_binary,
        is_submodule: false,
        is_mode_only,
        synthetic: true,
        metadata_rows: Vec::new(),
        hunks,
    }
}

fn engagement_supersedes(local: usize, ids: &[RevisionId]) -> Vec<RevisionId> {
    match local {
        0 => Vec::new(),
        1..=3 => vec![ids[local - 1].clone()],
        4 if ids.len() >= 3 => vec![ids[2].clone()],
        5 if ids.len() >= 5 => vec![ids[3].clone(), ids[4].clone()],
        6..=7 if ids.len() >= 6 => vec![ids[5].clone()],
        _ => vec![ids[local - 1].clone()],
    }
}

fn body_domain_owned(domain: &str) -> &'static str {
    match domain {
        "observation" => "observation",
        "assessment" => "assessment",
        "input-request" => "input-request",
        "input-response" => "input-response",
        "validation" => "validation",
        "task-observation" => "task-observation",
        _ => unreachable!("all longitudinal body domains are fixed"),
    }
}

fn object_review_id(shape: LongitudinalRecordShapeV1, global_ordinal: u64) -> ReviewId {
    ReviewId::new(format!(
        "review:longitudinal:{}:object:{global_ordinal:012}",
        shape.namespace()
    ))
}

fn object_content_ref(content_hash: &str) -> Result<String> {
    object_artifact_path_for_hash(Path::new(""), content_hash)
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| {
            ShoreError::Message("longitudinal object locator is not valid UTF-8".to_owned())
        })
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    #[test]
    fn longitudinal_materialize_one_block_has_the_frozen_topology_and_identity_mix() {
        let record = prepare_longitudinal_record_v1(LongitudinalRecordSpecV1::new(
            LongitudinalRecordShapeV1::Workload,
            0,
        ))
        .unwrap();
        let by_type =
            record
                .events
                .iter()
                .fold(BTreeMap::<&str, usize>::new(), |mut counts, event| {
                    *counts.entry(event.event_type.as_str()).or_default() += 1;
                    counts
                });
        let actors = record
            .events
            .iter()
            .map(|event| event.writer.actor_id.as_str())
            .collect::<BTreeSet<_>>();
        let tracks = record
            .events
            .iter()
            .filter_map(|event| event.target.track_id.as_ref().map(TrackId::as_str))
            .collect::<BTreeSet<_>>();
        let inline_signers = record
            .events
            .iter()
            .filter(|event| event.signature.is_some())
            .filter_map(|event| event.signer.as_ref().map(|signer| signer.as_str()))
            .collect::<BTreeSet<_>>();
        let proposals = record
            .events
            .iter()
            .filter(|event| event.event_type == EventType::WorkObjectProposed)
            .map(|event| {
                serde_json::from_value::<WorkObjectProposedPayload>(event.payload.clone()).unwrap()
            })
            .collect::<Vec<_>>();
        let supersession_edges = proposals
            .iter()
            .map(|payload| match &payload.work_object {
                WorkObjectProposal::Revision { supersedes, .. } => supersedes.len(),
                WorkObjectProposal::TaskAttempt { .. } => 0,
            })
            .sum::<usize>();
        let engagements = proposals
            .iter()
            .filter_map(|payload| match payload.work_object {
                WorkObjectProposal::Revision { .. } => Some(payload.engagement_id.as_str()),
                WorkObjectProposal::TaskAttempt { .. } => None,
            })
            .collect::<BTreeSet<_>>();
        let removed_kinds = record
            .removed
            .iter()
            .map(PreparedContentV1::kind)
            .collect::<BTreeSet<_>>();

        assert_eq!(record.events.len(), 256);
        assert_eq!(record.revision_count, 12);
        assert_eq!(record.task_attempt_count, 4);
        assert_eq!(record.body_fact_count, 180);
        assert_eq!(record.external_body_count, 88);
        assert_eq!(record.object_artifact_count, 12);
        assert_eq!(record.removed.len(), 3);
        assert_eq!(by_type["review_initialized"], 1);
        assert_eq!(by_type["work_object_proposed"], 16);
        assert_eq!(by_type["review_observation_recorded"], 64);
        assert_eq!(by_type["review_assessment_recorded"], 24);
        assert_eq!(by_type["input_request_opened"], 24);
        assert_eq!(by_type["input_request_responded"], 16);
        assert_eq!(by_type["revision_ref_associated"], 12);
        assert_eq!(by_type["revision_ref_withdrawn"], 4);
        assert_eq!(by_type["revision_commit_associated"], 16);
        assert_eq!(by_type["revision_commit_withdrawn"], 4);
        assert_eq!(by_type["validation_check_recorded"], 40);
        assert_eq!(by_type["task_checkpoint_captured"], 12);
        assert_eq!(by_type["task_observation_recorded"], 12);
        assert_eq!(by_type["event_signature_recorded"], 8);
        assert_eq!(by_type["artifact_removed"], 3);
        assert_eq!(actors.len(), 8);
        assert_eq!(tracks.len(), 6);
        assert_eq!(
            record
                .events
                .iter()
                .filter(|event| event.signature.is_some())
                .count(),
            48
        );
        assert_eq!(inline_signers.len(), 4);
        assert_eq!(engagements.len(), 2);
        assert_eq!(supersession_edges, 11);
        assert_eq!(
            removed_kinds,
            BTreeSet::from([
                LongitudinalContentKindV1::ExternalBody,
                LongitudinalContentKindV1::ObjectArtifact,
                LongitudinalContentKindV1::ValidationLog,
            ])
        );
    }

    #[test]
    fn longitudinal_materialize_l100_o10k_superblock_has_real_object_bindings() {
        let record = prepare_longitudinal_record_v1(LongitudinalRecordSpecV1::new(
            LongitudinalRecordShapeV1::CapacityL100O10K,
            0,
        ))
        .unwrap();
        let proposals = record
            .events
            .iter()
            .filter(|event| event.event_type == EventType::WorkObjectProposed)
            .collect::<Vec<_>>();
        let object_ids = proposals
            .iter()
            .filter_map(|event| {
                let payload: WorkObjectProposedPayload =
                    serde_json::from_value(event.payload.clone()).unwrap();
                match payload.work_object {
                    WorkObjectProposal::Revision { revision, .. } => Some(revision.object_id),
                    WorkObjectProposal::TaskAttempt { .. } => None,
                }
            })
            .collect::<BTreeSet<_>>();

        assert_eq!(record.events.len(), 1_024);
        assert_eq!(record.revision_count, 100);
        assert_eq!(record.object_artifact_count, 100);
        assert_eq!(record.external_body_count, 329);
        assert_eq!(object_ids.len(), 100);
    }

    #[test]
    fn longitudinal_materialize_writer_preserves_create_once_and_conflict_semantics() {
        let repo = initialized_repo();
        let record = one_event_record("sha256:first");
        let first =
            write_longitudinal_records_v1(repo.path(), std::slice::from_ref(&record)).unwrap();
        let retry = write_longitudinal_records_v1(repo.path(), &[record]).unwrap();

        assert_eq!((first.events_created, first.events_existing), (1, 0));
        assert_eq!((retry.events_created, retry.events_existing), (0, 1));

        let conflict = one_event_record("sha256:changed");
        assert!(
            write_longitudinal_records_v1(repo.path(), &[conflict])
                .unwrap_err()
                .to_string()
                .contains("event conflict")
        );
    }

    #[test]
    fn longitudinal_materialize_writer_rejects_invalid_envelopes_before_receipt() {
        for mutate in [
            |event: &mut ShoreEvent| {
                event.event_id = crate::model::EventId::new("evt:sha256:invalid");
            },
            |event: &mut ShoreEvent| {
                event.payload_hash = "sha256:invalid".to_owned();
            },
            |event: &mut ShoreEvent| {
                event.writer.actor_id = ActorId::new("invalid-actor");
            },
            |event: &mut ShoreEvent| {
                event.schema = "invalid.event".to_owned();
            },
        ] {
            let repo = initialized_repo();
            let mut record = one_event_record("sha256:first");
            mutate(&mut record.events[0]);

            assert!(write_longitudinal_records_v1(repo.path(), &[record]).is_err());
            assert!(crate::session::read_events(repo.path()).unwrap().is_empty());
        }
    }

    #[test]
    fn longitudinal_materialize_content_lands_before_a_pre_event_failure() {
        let repo = initialized_repo();
        let shape = LongitudinalRecordShapeV1::Workload;
        let body = deterministic_text(shape, 0, 4, "observation", BODY_SIZES[4]).unwrap();
        let (_, artifact_path, _, _) = staged_body(Some(&body)).unwrap();
        let artifact_path = artifact_path.expect("large body is external");
        let content_hash = format!("sha256:{}", sha256_bytes_hex(body.as_bytes()));
        let content = PreparedContentV1::ExternalBody {
            shape,
            block: 0,
            global_ordinal: 4,
            domain: "observation",
            relative_path: artifact_path.clone(),
            content_hash,
        };
        let mut record = one_event_record("sha256:first");
        record.content.push(content);
        record.events[0].writer.actor_id = ActorId::new("invalid-actor");

        assert!(write_longitudinal_records_v1(repo.path(), &[record]).is_err());
        assert!(crate::session::read_events(repo.path()).unwrap().is_empty());
        let read_store = resolve_read_store(repo.path()).unwrap();
        assert!(
            ContentArtifacts::from_backend(read_store.backend())
                .get_if_exists(&artifact_path)
                .unwrap()
                .is_some()
        );
    }

    fn initialized_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        repo
    }

    fn one_event_record(content_hash: &str) -> PreparedLongitudinalRecordV1 {
        let event = ShoreEvent::new(
            EventType::ArtifactRemoved,
            "t:16:longitudinal-conflict",
            EventTarget::for_journal(JournalId::new("journal:longitudinal:test")),
            Writer {
                actor_id: ActorId::new("actor:agent:longitudinal-test"),
                producer: WriterProducer {
                    name: "pointbreak".to_owned(),
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                },
            },
            ArtifactRemovedPayload {
                content_hash: content_hash.to_owned(),
            },
            "2026-01-01T00:00:00.000Z",
        )
        .unwrap();
        PreparedLongitudinalRecordV1 {
            events: vec![event],
            content: Vec::new(),
            removed: Vec::new(),
            revision_count: 0,
            task_attempt_count: 0,
            body_fact_count: 0,
            external_body_count: 0,
            object_artifact_count: 0,
            decoded_body_bytes: 0,
            decoded_object_target_bytes: 0,
        }
    }
}
