//! Candidate-independent compact semantic facts and projection receipts.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::cursor::TruthCursor;
use crate::canonical_hash::{sha256_bytes_hex, sha256_json_prefixed};
use crate::error::ShoreError;
use crate::model::{ReviewEndpoint, ReviewTargetRef, RevisionId, ValidationTarget};
use crate::session::event::{
    ArtifactRemovedPayload, AssertionMode, EventType, InputRequestReasonCode,
    InputRequestRespondedPayload, ReviewAssessment, ReviewAssessmentRecordedPayload,
    ReviewObservationRecordedPayload, RevisionCommitAssociatedPayload,
    RevisionCommitWithdrawnPayload, RevisionRefAssociatedPayload, RevisionRefWithdrawnPayload,
    ShoreEvent, ValidationCheckRecordedPayload, WorkObjectProposal, WorkObjectProposedPayload,
    decode_input_request_opened_payload,
};

pub(crate) mod attention;
pub(crate) mod revision;
pub(crate) mod state;
pub(crate) mod thread;

use attention::AttentionSemanticSnapshot;
use revision::revision_documents_from_facts;
use state::SemanticStateSnapshot;
use thread::thread_documents_from_facts;

/// The event-intrinsic values needed by the derived families.
///
/// Body text, summaries, reasons, paths, commands, provenance selectors,
/// signatures, and complete event envelopes are intentionally absent. The
/// physical candidate stores each variant in a family-specific table rather
/// than serializing this enum. Request titles and validation check names remain
/// as output-bearing labels; tests distinguish those authorized scalars from
/// forbidden prose bodies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SemanticFactKind {
    Revision(RevisionFact),
    Observation,
    Assessment(AssessmentFact),
    InputRequestOpened(InputRequestFact),
    InputRequestResponded(InputResponseFact),
    Validation(ValidationFact),
    CommitAssociated(CommitAssociationFact),
    CommitWithdrawn(CommitWithdrawalFact),
    RefAssociated(RefAssociationFact),
    RefWithdrawn(RefWithdrawalFact),
    ArtifactRemoved,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RevisionFact {
    pub(crate) object_id: String,
    pub(crate) engagement_id: String,
    pub(crate) supersedes: Vec<String>,
    pub(crate) base_commit_oid: Option<String>,
    pub(crate) capture_commit_oid: Option<String>,
    pub(crate) capture_tree_oid: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AssessmentFact {
    pub(crate) assessment: ReviewAssessment,
    pub(crate) replaces: Vec<String>,
    pub(crate) related_observations: Vec<String>,
    pub(crate) related_requests: Vec<String>,
    pub(crate) revision_scoped: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InputRequestFact {
    pub(crate) reason_code: InputRequestReasonCode,
    pub(crate) title: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InputResponseFact {
    pub(crate) request_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidationFact {
    pub(crate) check_name: String,
    pub(crate) status: crate::model::ValidationStatus,
    pub(crate) exit_code: Option<i64>,
    pub(crate) completed_at: Option<String>,
    pub(crate) log_artifact_content_hashes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommitAssociationFact {
    pub(crate) commit_oid: String,
    pub(crate) tree_oid: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommitWithdrawalFact {
    pub(crate) association_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RefAssociationFact {
    pub(crate) ref_name: String,
    pub(crate) head_oid: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RefWithdrawalFact {
    pub(crate) association_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SemanticFact {
    pub(crate) cursor: TruthCursor,
    pub(crate) logical_reread_key: String,
    pub(crate) replay_key: String,
    pub(crate) event_id: String,
    pub(crate) event_type: String,
    pub(crate) journal_id: String,
    pub(crate) revision_id: Option<String>,
    pub(crate) semantic_id: Option<String>,
    pub(crate) content_hash: Option<String>,
    pub(crate) payload_hash: String,
    pub(crate) occurred_at: String,
    pub(crate) assertion_mode: AssertionMode,
    pub(crate) track_id: Option<String>,
    pub(crate) actor_id: String,
    pub(crate) validation_witness: String,
    pub(crate) kind: SemanticFactKind,
}

impl SemanticFact {
    pub(crate) fn from_event(
        cursor: TruthCursor,
        event: &ShoreEvent,
        validation_witness: impl Into<String>,
    ) -> Result<Self, SemanticModelError> {
        if cursor.epoch == 0 || cursor.sequence == 0 {
            return Err(SemanticModelError::InvalidCursor(cursor));
        }
        let validation_witness = validation_witness.into();
        if validation_witness.len() != 64
            || !validation_witness
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(SemanticModelError::InvalidWitness);
        }

        let classified = classify_event(event)?;
        Ok(Self {
            cursor,
            logical_reread_key: event.idempotency_key.clone(),
            replay_key: sha256_bytes_hex(event.idempotency_key.as_bytes()),
            event_id: event.event_id.as_str().to_owned(),
            event_type: event.event_type.as_str().to_owned(),
            journal_id: event.target.journal_id.as_str().to_owned(),
            revision_id: classified.revision_id,
            semantic_id: classified.semantic_id,
            content_hash: classified.content_hash,
            payload_hash: event.payload_hash.clone(),
            occurred_at: event.occurred_at.clone(),
            assertion_mode: event.assertion_mode,
            track_id: event
                .target
                .track_id
                .as_ref()
                .map(|track| track.as_str().to_owned()),
            actor_id: event.writer.actor_id.as_str().to_owned(),
            validation_witness,
            kind: classified.kind,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SemanticSnapshot {
    pub(crate) as_of: TruthCursor,
    pub(crate) state: SemanticStateSnapshot,
    pub(crate) revisions: serde_json::Value,
    pub(crate) threads: serde_json::Value,
    pub(crate) attention: AttentionSemanticSnapshot,
    pub(crate) removed_content: BTreeSet<String>,
    pub(crate) semantic_receipt: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MaterializedAttentionSnapshot {
    pub(crate) as_of: TruthCursor,
    pub(crate) state: SemanticStateSnapshot,
    pub(crate) supersession: crate::session::SupersessionView,
    pub(crate) attention: AttentionSemanticSnapshot,
}

impl SemanticSnapshot {
    /// Strict full-replay oracle/rebuild path.
    pub(crate) fn from_events(
        as_of: TruthCursor,
        events: &[ShoreEvent],
    ) -> Result<Self, SemanticModelError> {
        let state = SemanticStateSnapshot::from_events(events)?;
        let revisions = revision::revision_documents(events)?;
        let threads = thread::thread_documents(events)?;
        let attention = AttentionSemanticSnapshot::from_events(events)?;
        let removed_content =
            crate::session::projection::ArtifactRemovalProjection::from_events(events)?
                .claimed_hashes()
                .map(str::to_owned)
                .collect();
        Self::finish(as_of, state, revisions, threads, attention, removed_content)
    }

    /// Exact differential audit over compact facts.
    ///
    /// This deliberately scans the normalized fact set to produce public-equivalent
    /// aggregate documents and `eventSetHash`. Ordinary append, restart, point
    /// lookup, and revision-detail paths never call it.
    pub(crate) fn audit_from_facts(
        as_of: TruthCursor,
        facts: &[SemanticFact],
    ) -> Result<Self, SemanticModelError> {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        {
            crate::bench_support::longitudinal::record_state_rebuild();
            for _ in 0..4 {
                crate::bench_support::longitudinal::record_projection_rebuild();
            }
            crate::bench_support::longitudinal::record_event_folds(facts.len().saturating_mul(5));
        }
        let state = SemanticStateSnapshot::from_facts(facts)?;
        let revisions = revision_documents_from_facts(facts)?;
        let threads = thread_documents_from_facts(facts)?;
        let attention = AttentionSemanticSnapshot::from_facts(facts)?;
        let removed_content = facts
            .iter()
            .filter_map(|fact| match fact.kind {
                SemanticFactKind::ArtifactRemoved => fact.content_hash.clone(),
                _ => None,
            })
            .collect();
        Self::finish(as_of, state, revisions, threads, attention, removed_content)
    }

    /// Ordinary-operation assembly over incrementally maintained current rows.
    ///
    /// The input contains one representative per output-bearing semantic unit,
    /// not the retained event history. The state counters are maintained by
    /// the delta transaction, so this path performs no event fold or projection
    /// rebuild.
    pub(crate) fn from_materialized(
        as_of: TruthCursor,
        state: SemanticStateSnapshot,
        facts: &[SemanticFact],
    ) -> Result<Self, SemanticModelError> {
        let revisions = revision_documents_from_facts(facts)?;
        let threads = thread_documents_from_facts(facts)?;
        let attention = AttentionSemanticSnapshot::from_facts(facts)?;
        let removed_content = facts
            .iter()
            .filter_map(|fact| match fact.kind {
                SemanticFactKind::ArtifactRemoved => fact.content_hash.clone(),
                _ => None,
            })
            .collect();
        Self::finish(as_of, state, revisions, threads, attention, removed_content)
    }

    /// Strict full replay projected onto the ordinary-operation shape.
    pub(crate) fn materialized_oracle_from_events(
        as_of: TruthCursor,
        events: &[ShoreEvent],
    ) -> Result<Self, SemanticModelError> {
        let mut state = SemanticStateSnapshot::from_events(events)?;
        state.event_set_hash = None;
        let revisions = revision::revision_documents(events)?;
        let threads = thread::thread_documents(events)?;
        let attention = AttentionSemanticSnapshot::from_events(events)?;
        let removed_content =
            crate::session::projection::ArtifactRemovalProjection::from_events(events)?
                .claimed_hashes()
                .map(str::to_owned)
                .collect();
        Self::finish(as_of, state, revisions, threads, attention, removed_content)
    }

    /// Strict full replay projected onto one selected engagement.
    ///
    /// The state row remains store-wide. Revision, thread, attention, and
    /// removal families are limited to the selected engagement so fixed-output
    /// operations do not assemble every retained semantic unit.
    pub(crate) fn materialized_engagement_oracle_from_events(
        as_of: TruthCursor,
        events: &[ShoreEvent],
        engagement_id: &str,
    ) -> Result<Self, SemanticModelError> {
        let mut revision_ids = BTreeSet::new();
        let mut content_hashes = BTreeSet::new();
        for event in events {
            if event.event_type != EventType::WorkObjectProposed {
                continue;
            }
            let payload: WorkObjectProposedPayload = serde_json::from_value(event.payload.clone())?;
            if payload.engagement_id.as_str() != engagement_id {
                continue;
            }
            if let WorkObjectProposal::Revision {
                revision,
                object_artifact_content_hash,
                ..
            } = payload.work_object
            {
                revision_ids.insert(revision.id);
                content_hashes.insert(object_artifact_content_hash);
            }
        }

        let mut selected = Vec::new();
        for event in events {
            let include = match event.event_type {
                EventType::WorkObjectProposed => {
                    let payload: WorkObjectProposedPayload =
                        serde_json::from_value(event.payload.clone())?;
                    matches!(
                        payload.work_object,
                        WorkObjectProposal::Revision { revision, .. }
                            if revision_ids.contains(&revision.id)
                    )
                }
                EventType::ReviewObservationRecorded
                | EventType::ReviewAssessmentRecorded
                | EventType::InputRequestOpened
                | EventType::InputRequestResponded
                | EventType::RevisionRefAssociated
                | EventType::RevisionRefWithdrawn
                | EventType::RevisionCommitAssociated
                | EventType::RevisionCommitWithdrawn
                | EventType::ValidationCheckRecorded => event
                    .subject_revision_id()?
                    .is_some_and(|revision_id| revision_ids.contains(&revision_id)),
                EventType::ArtifactRemoved => {
                    let payload: ArtifactRemovedPayload =
                        serde_json::from_value(event.payload.clone())?;
                    content_hashes.contains(&payload.content_hash)
                }
                _ => false,
            };
            if include {
                selected.push(event.clone());
            }
        }

        let mut state = SemanticStateSnapshot::from_events(events)?;
        state.event_set_hash = None;
        let revisions = revision::revision_documents(&selected)?;
        let threads = thread::thread_documents(&selected)?;
        let attention = AttentionSemanticSnapshot::from_events(&selected)?;
        let removed_content =
            crate::session::projection::ArtifactRemovalProjection::from_events(&selected)?
                .claimed_hashes()
                .map(str::to_owned)
                .collect();
        Self::finish(as_of, state, revisions, threads, attention, removed_content)
    }

    fn finish(
        as_of: TruthCursor,
        state: SemanticStateSnapshot,
        revisions: serde_json::Value,
        threads: serde_json::Value,
        attention: AttentionSemanticSnapshot,
        removed_content: BTreeSet<String>,
    ) -> Result<Self, SemanticModelError> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ReceiptMaterial<'a> {
            epoch: u64,
            sequence: u64,
            state: &'a SemanticStateSnapshot,
            revisions: &'a serde_json::Value,
            threads: &'a serde_json::Value,
            attention: &'a AttentionSemanticSnapshot,
            removed_content: &'a BTreeSet<String>,
        }
        let semantic_receipt = sha256_json_prefixed(&serde_json::to_value(ReceiptMaterial {
            epoch: as_of.epoch,
            sequence: as_of.sequence,
            state: &state,
            revisions: &revisions,
            threads: &threads,
            attention: &attention,
            removed_content: &removed_content,
        })?)?;

        Ok(Self {
            as_of,
            state,
            revisions,
            threads,
            attention,
            removed_content,
            semantic_receipt,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HydratedRevisionDetail {
    pub(crate) as_of: TruthCursor,
    pub(crate) revision_id: RevisionId,
    pub(crate) object_content_hash: String,
    pub(crate) object_content_removed: bool,
    pub(crate) authoritative_events: Vec<ShoreEvent>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SemanticModelError {
    #[error("semantic fact cursor must be nonzero: {0:?}")]
    InvalidCursor(TruthCursor),
    #[error("semantic fact validation witness must be 64 hexadecimal characters")]
    InvalidWitness,
    #[error("semantic fact is missing required field {0}")]
    MissingField(&'static str),
    #[error("semantic fact has invalid event instant {0}")]
    InvalidEventInstant(String),
    #[error(transparent)]
    Product(#[from] ShoreError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

struct ClassifiedFact {
    revision_id: Option<String>,
    semantic_id: Option<String>,
    content_hash: Option<String>,
    kind: SemanticFactKind,
}

fn classify_event(event: &ShoreEvent) -> Result<ClassifiedFact, SemanticModelError> {
    let mut revision_id = match event.event_type {
        EventType::ReviewObservationRecorded
        | EventType::ReviewAssessmentRecorded
        | EventType::InputRequestOpened
        | EventType::InputRequestResponded
        | EventType::RevisionRefAssociated
        | EventType::RevisionRefWithdrawn
        | EventType::RevisionCommitAssociated
        | EventType::RevisionCommitWithdrawn
        | EventType::ValidationCheckRecorded => event
            .subject_revision_id()?
            .map(|id| id.as_str().to_owned()),
        _ => None,
    };
    let mut semantic_id = None;
    let mut content_hash = None;
    let kind = match event.event_type {
        EventType::WorkObjectProposed => {
            let payload: WorkObjectProposedPayload = serde_json::from_value(event.payload.clone())?;
            match payload.work_object {
                WorkObjectProposal::Revision {
                    revision,
                    object_artifact_content_hash,
                    supersedes,
                    ..
                } => {
                    revision_id = Some(revision.id.as_str().to_owned());
                    semantic_id = revision_id.clone();
                    content_hash = Some(object_artifact_content_hash);
                    let (base_commit_oid, capture_commit_oid, capture_tree_oid) = revision
                        .git_provenance
                        .as_ref()
                        .map_or((None, None, None), |provenance| {
                            let base = match &provenance.base {
                                ReviewEndpoint::GitCommit { commit_oid, .. } => {
                                    Some(commit_oid.clone())
                                }
                                _ => None,
                            };
                            let (target_commit, target_tree) = match &provenance.target {
                                ReviewEndpoint::GitCommit {
                                    commit_oid,
                                    tree_oid,
                                } => (Some(commit_oid.clone()), Some(tree_oid.clone())),
                                _ => (None, None),
                            };
                            (base, target_commit, target_tree)
                        });
                    SemanticFactKind::Revision(RevisionFact {
                        object_id: revision.object_id.as_str().to_owned(),
                        engagement_id: payload.engagement_id.as_str().to_owned(),
                        supersedes: supersedes.iter().map(|id| id.as_str().to_owned()).collect(),
                        base_commit_oid,
                        capture_commit_oid,
                        capture_tree_oid,
                    })
                }
                WorkObjectProposal::TaskAttempt { .. } => SemanticFactKind::Other,
            }
        }
        EventType::ReviewObservationRecorded => {
            let payload: ReviewObservationRecordedPayload =
                serde_json::from_value(event.payload.clone())?;
            semantic_id = Some(payload.observation_id.as_str().to_owned());
            SemanticFactKind::Observation
        }
        EventType::ReviewAssessmentRecorded => {
            let payload: ReviewAssessmentRecordedPayload =
                serde_json::from_value(event.payload.clone())?;
            semantic_id = Some(payload.assessment_id.as_str().to_owned());
            SemanticFactKind::Assessment(AssessmentFact {
                assessment: payload.assessment,
                replaces: payload
                    .replaces_assessment_ids
                    .iter()
                    .map(|id| id.as_str().to_owned())
                    .collect(),
                related_observations: payload
                    .related_observation_ids
                    .iter()
                    .map(|id| id.as_str().to_owned())
                    .collect(),
                related_requests: payload
                    .related_input_request_ids
                    .iter()
                    .map(|id| id.as_str().to_owned())
                    .collect(),
                revision_scoped: matches!(payload.target, ReviewTargetRef::Revision { .. }),
            })
        }
        EventType::InputRequestOpened => {
            let payload = decode_input_request_opened_payload(event.payload.clone())?;
            semantic_id = Some(payload.input_request_id.as_str().to_owned());
            SemanticFactKind::InputRequestOpened(InputRequestFact {
                reason_code: payload.reason_code,
                title: payload.title,
            })
        }
        EventType::InputRequestResponded => {
            let payload: InputRequestRespondedPayload =
                serde_json::from_value(event.payload.clone())?;
            if revision_id.is_none() {
                revision_id = payload.revision_id.map(|id| id.as_str().to_owned());
            }
            semantic_id = Some(payload.input_request_response_id.as_str().to_owned());
            SemanticFactKind::InputRequestResponded(InputResponseFact {
                request_id: payload.input_request_id.as_str().to_owned(),
            })
        }
        EventType::ValidationCheckRecorded => {
            let payload: ValidationCheckRecordedPayload =
                serde_json::from_value(event.payload.clone())?;
            let ValidationTarget::Revision {
                revision_id: target_revision,
            } = &payload.target;
            revision_id = Some(target_revision.as_str().to_owned());
            semantic_id = Some(payload.validation_check_id.as_str().to_owned());
            SemanticFactKind::Validation(ValidationFact {
                check_name: payload.check_name,
                status: payload.status,
                exit_code: payload.exit_code,
                completed_at: payload.completed_at,
                log_artifact_content_hashes: payload.log_artifact_content_hashes,
            })
        }
        EventType::RevisionCommitAssociated => {
            let payload: RevisionCommitAssociatedPayload =
                serde_json::from_value(event.payload.clone())?;
            semantic_id = Some(payload.commit_association_id.as_str().to_owned());
            match payload.commit {
                ReviewEndpoint::GitCommit {
                    commit_oid,
                    tree_oid,
                } => SemanticFactKind::CommitAssociated(CommitAssociationFact {
                    commit_oid,
                    tree_oid,
                }),
                _ => SemanticFactKind::Other,
            }
        }
        EventType::RevisionCommitWithdrawn => {
            let payload: RevisionCommitWithdrawnPayload =
                serde_json::from_value(event.payload.clone())?;
            semantic_id = Some(payload.commit_withdrawal_id.as_str().to_owned());
            SemanticFactKind::CommitWithdrawn(CommitWithdrawalFact {
                association_id: payload.commit_association_id.as_str().to_owned(),
            })
        }
        EventType::RevisionRefAssociated => {
            let payload: RevisionRefAssociatedPayload =
                serde_json::from_value(event.payload.clone())?;
            semantic_id = Some(payload.ref_association_id.as_str().to_owned());
            SemanticFactKind::RefAssociated(RefAssociationFact {
                ref_name: payload.ref_name,
                head_oid: payload.head_oid,
            })
        }
        EventType::RevisionRefWithdrawn => {
            let payload: RevisionRefWithdrawnPayload =
                serde_json::from_value(event.payload.clone())?;
            semantic_id = Some(payload.ref_withdrawal_id.as_str().to_owned());
            SemanticFactKind::RefWithdrawn(RefWithdrawalFact {
                association_id: payload.ref_association_id.as_str().to_owned(),
            })
        }
        EventType::ArtifactRemoved => {
            let payload: ArtifactRemovedPayload = serde_json::from_value(event.payload.clone())?;
            semantic_id = Some(payload.content_hash.clone());
            content_hash = Some(payload.content_hash);
            SemanticFactKind::ArtifactRemoved
        }
        EventType::ReviewInitialized
        | EventType::ReviewNoteImported
        | EventType::TaskCheckpointCaptured
        | EventType::TaskObservationRecorded
        | EventType::EventSignatureRecorded => SemanticFactKind::Other,
    };
    Ok(ClassifiedFact {
        revision_id,
        semantic_id,
        content_hash,
        kind,
    })
}

pub(crate) fn encode_string_list(values: &[String]) -> Result<String, SemanticModelError> {
    Ok(serde_json::to_string(values)?)
}

pub(crate) fn decode_string_list(value: &str) -> Result<Vec<String>, SemanticModelError> {
    Ok(serde_json::from_str(value)?)
}

pub(crate) fn encode_enum<T: Serialize>(value: T) -> Result<String, SemanticModelError> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .ok_or(SemanticModelError::MissingField("enum wire value"))
}

pub(crate) fn decode_enum<T: for<'de> Deserialize<'de>>(
    value: &str,
) -> Result<T, SemanticModelError> {
    Ok(serde_json::from_value(serde_json::Value::String(
        value.to_owned(),
    ))?)
}
