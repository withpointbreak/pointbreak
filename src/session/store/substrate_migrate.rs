//! One-shot store migrator: lifts a legacy flat-v1 store into the reshaped
//! envelope in a single pass.
//!
//! This is throwaway, run-once tooling, not a shipped command. It reads each
//! legacy event as raw JSON (bypassing the strict read path, which loudly rejects
//! the old envelope shape), projects it into the reshaped envelope, separates the
//! content-only object from the revision position, folds each lineage round's
//! predecessor into a `supersedes` pointer on the successor's generative move,
//! recomputes every identity digest, re-signs with the original signer's held key,
//! re-attests each held-key detached co-signature, and writes the result into a
//! fresh store the strict read path accepts. Already-reshaped events (a store
//! captured partly under the new binary) pass through unchanged. A detached
//! co-signature whose attester key is not held cannot be re-attested and is
//! dropped with a warning.
//!
//! Identity moves: the legacy `review_unit_id` reshapes into the `revision_id`
//! (re-derived from the content object plus git provenance, not a field rename),
//! so every event that referenced it is remapped; the legacy `snapshot_id`
//! reshapes into a content-only object id; the snapshot artifact is re-emitted as
//! a clean v2 body under the new object key.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use super::EventStore;
use super::fingerprint::{engagement_id_from_root, object_identity, revision_id_from};
use super::snapshot_artifact::{build_snapshot_artifact_v2, snapshot_artifact_path};
use crate::canonical_hash::{sha256_bytes_hex, sha256_json_prefixed};
use crate::crypto::EventSigner;
use crate::error::{Result, ShoreError};
use crate::keys::{FileEd25519Signer, KeyCustody, list_keys_in, load_signer_in};
use crate::model::{
    DiffSnapshot, EngagementId, EventId, LedgerId, ObjectId, ReviewEndpoint, ReviewUnitSource,
    RevisionId, TargetRef, TaskTargetRef, TrackId, ValidationTarget, WorkObjectId,
};
use crate::session::event::{
    AssertionMode, EventSignature, EventSignatureRecordedPayload, EventTarget, EventToBeSigned,
    EventType, GitProvenance, IngestProvenance, Revision, ShoreEvent, SourceRef, SourceSpeaker,
    ValidationCheckRecordedPayload, WorkObjectProposal, WorkObjectProposedPayload, Writer,
    event_signature_pre_authentication_encoding,
};
use crate::session::workflow::util::sorted_unique;
use crate::session::workflow::{ValidationCheckIdMaterial, build_validation_check_id};
use crate::session::{EventSigningOptions, sign_event_if_requested};

/// Inputs for one migration pass. Generic: all three locations are parameters,
/// with no built-in repo, key, or path assumptions.
#[derive(Clone, Debug)]
pub struct MigrateOptions {
    /// The legacy store directory to read (the dir holding `events/` and
    /// `artifacts/`).
    pub source_store_dir: PathBuf,
    /// A fresh, empty store directory to write the reshaped store into.
    pub target_store_dir: PathBuf,
    /// The keystore directory holding the signers' private keys, used to re-sign
    /// inline signatures and re-attest held-key co-signatures.
    pub keystore_dir: PathBuf,
}

/// What one migration pass did. The owner-run step reads this to confirm the
/// migration was lossless and the reshaped store self-validated.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateSummary {
    /// Events written to the reshaped store (transformed legacy events,
    /// passed-through already-reshaped events, and re-attested co-signatures).
    pub events_migrated: usize,
    /// Lineage round events folded into `supersedes` pointers (translated, not
    /// re-emitted).
    pub lineage_rounds_folded: usize,
    /// Inline signatures re-signed with the original signer's held key.
    pub inline_signatures_resigned: usize,
    /// Detached co-signatures re-attested with the attester's held key.
    pub cosignatures_reattested: usize,
    /// Detached co-signatures dropped because the attester's key is not held (or
    /// the target did not survive), counted and warned, never silent.
    pub cosignatures_dropped: usize,
    /// Whether the reshaped store passed its self-check (`list_events` rebuilds
    /// cleanly under the strict read path and `SessionState::from_events`
    /// succeeds).
    pub self_check_passed: bool,
}

/// The resolved identity for one migrated capture, computed in the first pass and
/// consumed when the generative move is re-emitted.
#[derive(Clone, Debug)]
struct CapturePlan {
    revision_id: RevisionId,
    object_id: ObjectId,
    git_provenance: Option<GitProvenance>,
    snapshot_artifact_content_hash: String,
    supersedes: Vec<RevisionId>,
    engagement_id: EngagementId,
}

/// Migrate the legacy store at `source_store_dir` into a fresh reshaped store at
/// `target_store_dir`, re-signing with keys from `keystore_dir`.
pub fn migrate_substrate_store(options: MigrateOptions) -> Result<MigrateSummary> {
    let raw = read_raw_events(&options.source_store_dir)?;
    let keystore = build_keystore_index(&options.keystore_dir)?;

    // First pass: re-derive each capture's content object + revision position,
    // re-emit its snapshot artifact, and build the old-id -> new-id remap that
    // every dependent event is rewritten against.
    let mut review_remap: BTreeMap<String, String> = BTreeMap::new();
    let mut capture_plans: BTreeMap<String, CapturePlan> = BTreeMap::new();
    for value in &raw {
        if value["eventType"] != "review_unit_captured" || !is_legacy(value) {
            continue;
        }
        let old_review_unit_id = legacy_review_unit_id(value)?;
        let plan = plan_review_capture(value, &options)?;
        review_remap.insert(
            old_review_unit_id.clone(),
            plan.revision_id.as_str().to_owned(),
        );
        capture_plans.insert(old_review_unit_id, plan);
    }

    // Translate lineage rounds into supersedes edges on the successor's plan.
    let mut summary = MigrateSummary::default();
    for value in &raw {
        if value["eventType"] != "review_unit_lineage_round_recorded" {
            continue;
        }
        let successor = value["payload"]["reviewUnitId"]
            .as_str()
            .ok_or_else(|| migrate_error("lineage round is missing reviewUnitId"))?;
        if let Some(predecessor) = value["payload"]["predecessorReviewUnitId"].as_str()
            && let Some(new_predecessor) = review_remap.get(predecessor)
            && let Some(plan) = capture_plans.get_mut(successor)
        {
            plan.supersedes
                .push(RevisionId::new(new_predecessor.clone()));
        }
        summary.lineage_rounds_folded += 1;
    }
    for plan in capture_plans.values_mut() {
        plan.supersedes = sorted_unique(std::mem::take(&mut plan.supersedes));
        // The engagement id is a write-time-derived grouping hint; the read
        // projection is authoritative and self-heals. A root seeds from its own
        // revision; a successor seeds deterministically from its earliest
        // predecessor so the thread groups stably.
        plan.engagement_id = match plan.supersedes.first() {
            None => engagement_id_from_root(&plan.revision_id),
            Some(first_predecessor) => engagement_id_from_root(first_predecessor),
        };
    }

    // Second pass: re-emit every non-lineage, non-co-signature event.
    let target = EventStore::open(&options.target_store_dir);
    let mut old_to_new: BTreeMap<String, ShoreEvent> = BTreeMap::new();
    for value in &raw {
        let event_type = value["eventType"].as_str().unwrap_or_default();
        if event_type == "review_unit_lineage_declared"
            || event_type == "review_unit_lineage_round_recorded"
        {
            continue; // folded into supersedes; the carriers are not re-emitted
        }
        if event_type == "event_signature_recorded" {
            // Every co-signature is re-homed in the third pass, after the full
            // old->new event-id map is built. A current-envelope carrier cannot be
            // passed through here because its target may still be re-keyed below
            // (a current-envelope association event left with the stale event type).
            continue;
        }

        if !is_legacy(value)
            && reshaped_event_type(event_type) == event_type
            && !carries_stale_review_unit_wire(value)
        {
            // A fully reshaped event passes through verbatim (provenance + mode are
            // already on it). A current-envelope event can still carry the old
            // `review_unit` spelling and is re-keyed below: an association/withdrawal
            // event keeps the old event type (the `reshaped_event_type` guard), and a
            // validation event keeps the old `kind: review_unit` payload target until
            // the ValidationTarget rename (the stale-wire guard).
            let event = passthrough_event(value)?;
            record_into(&target, &event)?;
            old_to_new.insert(event_id_of(value)?, event);
            summary.events_migrated += 1;
            continue;
        }

        let mut new_event = if event_type == "review_unit_captured" {
            transform_review_capture(value, &capture_plans)?
        } else if event_type == "task_attempt_captured" {
            transform_task_capture(value)?
        } else if event_type == "validation_check_recorded" {
            transform_validation_check(value, &review_remap)?
        } else {
            transform_generic(value, &review_remap)?
        };

        // Carry the legacy event's top-level provenance + assertion mode onto the
        // reshaped event before re-signing (these ride outside every identity
        // digest, so the order is free).
        carry_legacy_metadata(&mut new_event, value)?;
        let new_event = resign_if_signed(new_event, value, &keystore, &options, &mut summary)?;
        record_into(&target, &new_event)?;
        old_to_new.insert(event_id_of(value)?, new_event);
        summary.events_migrated += 1;
    }

    // Third pass: re-home detached co-signatures, in dependency order (every
    // target is written above). A current-envelope carrier whose target was left
    // untouched is preserved verbatim; any carrier whose target was re-keyed (every
    // legacy target, and a current-envelope association left with the stale type) is
    // re-attested over the reshaped target or dropped.
    for value in &raw {
        if value["eventType"] != "event_signature_recorded" {
            continue;
        }
        rehome_cosignature(
            value,
            &target,
            &old_to_new,
            &keystore,
            &options,
            &mut summary,
        )?;
    }

    // Copy note/body artifacts verbatim: they are content-addressed by a body
    // hash the reshape never changes, so the migrated events still resolve them.
    copy_dir_verbatim(
        &options.source_store_dir.join("artifacts/notes"),
        &options.target_store_dir.join("artifacts/notes"),
    )?;

    // Self-check: the reshaped store must list cleanly under the strict read path
    // and rebuild its projection.
    let events = target.list_events()?;
    let _state = crate::session::SessionState::from_events(&events)?;
    summary.self_check_passed = true;

    Ok(summary)
}

fn read_raw_events(source_store_dir: &Path) -> Result<Vec<Value>> {
    let store = EventStore::open(source_store_dir);
    let mut events = Vec::new();
    for name in store.list_event_file_names()? {
        let path = source_store_dir.join("events").join(&name);
        let bytes = std::fs::read(&path)
            .map_err(|error| migrate_error(&format!("read {}: {error}", path.display())))?;
        events.push(serde_json::from_slice(&bytes)?);
    }
    Ok(events)
}

fn build_keystore_index(keystore_dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut index = BTreeMap::new();
    for info in list_keys_in(keystore_dir)? {
        if info.custody() == KeyCustody::File {
            index.insert(info.signer_id().as_str().to_owned(), info.name().to_owned());
        }
    }
    Ok(index)
}

/// A legacy envelope carries `sessionId`; a reshaped one carries `ledgerId`.
fn is_legacy(value: &Value) -> bool {
    value["target"].get("ledgerId").is_none()
}

fn event_id_of(value: &Value) -> Result<String> {
    value["eventId"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| migrate_error("event is missing eventId"))
}

fn legacy_review_unit_id(value: &Value) -> Result<String> {
    value["target"]["reviewUnitId"]
        .as_str()
        .or_else(|| value["payload"]["reviewUnitId"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| migrate_error("legacy capture is missing reviewUnitId"))
}

fn ledger_id_of(value: &Value) -> Result<LedgerId> {
    value["target"]["sessionId"]
        .as_str()
        .or_else(|| value["target"]["ledgerId"].as_str())
        .map(LedgerId::new)
        .ok_or_else(|| migrate_error("event target is missing sessionId/ledgerId"))
}

fn track_id_of(value: &Value) -> Option<TrackId> {
    value["target"]["trackId"].as_str().map(TrackId::new)
}

fn writer_of(value: &Value) -> Result<Writer> {
    Ok(serde_json::from_value(value["writer"].clone())?)
}

/// Re-derive a review capture's content object and revision position, re-emit its
/// snapshot artifact as v2, and stamp the artifact binding hash.
fn plan_review_capture(value: &Value, options: &MigrateOptions) -> Result<CapturePlan> {
    let payload = &value["payload"];
    let git_provenance = parse_git_provenance(payload)?;
    let old_snapshot_id = payload["snapshotId"]
        .as_str()
        .or_else(|| value["target"]["snapshotId"].as_str())
        .ok_or_else(|| migrate_error("legacy capture is missing snapshotId"))?;

    let source_path =
        snapshot_artifact_path(&options.source_store_dir, &ObjectId::new(old_snapshot_id));
    let (object_id, snapshot_artifact_content_hash) = match std::fs::read(&source_path) {
        Ok(bytes) => {
            let artifact_value: Value = serde_json::from_slice(&bytes)?;
            // Refuse to launder a corrupt or swapped source artifact into a clean
            // v2 body: validate the artifact's own stored content hash over its raw
            // body, and confirm it is the artifact the capture event bound. The
            // re-emit recomputes a fresh v2 hash, which would otherwise silently
            // bless tampered source bytes.
            validate_legacy_artifact_integrity(&artifact_value, old_snapshot_id)?;
            let snapshot: DiffSnapshot =
                serde_json::from_value(artifact_value["snapshot"].clone())?;
            let object_id = object_identity(&snapshot.files);
            // Re-emit a clean v2 body keyed by the new content object id.
            let reshaped = DiffSnapshot::new(
                snapshot.review_id.clone(),
                object_id.clone(),
                snapshot.files.clone(),
            );
            let artifact = build_snapshot_artifact_v2(reshaped)?;
            let target_path = snapshot_artifact_path(&options.target_store_dir, &object_id);
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    migrate_error(&format!("create {}: {error}", parent.display()))
                })?;
            }
            std::fs::write(&target_path, serde_json::to_vec(&artifact)?).map_err(|error| {
                migrate_error(&format!("write {}: {error}", target_path.display()))
            })?;
            (object_id, artifact.content_hash)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // The capture's content was removed (ADR-0016) or never present: keep
            // the legacy object id and binding hash so the event still records,
            // and skip the re-emit (there is nothing to re-key).
            eprintln!(
                "warning: snapshot artifact for {old_snapshot_id} is absent; preserving its legacy object id"
            );
            let binding = payload["snapshotArtifactContentHash"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            (ObjectId::new(old_snapshot_id), binding)
        }
        Err(error) => {
            return Err(migrate_error(&format!(
                "read {}: {error}",
                source_path.display()
            )));
        }
    };

    let revision_id = revision_id_from(&object_id, git_provenance.as_ref())?;
    let engagement_id = engagement_id_from_root(&revision_id);

    Ok(CapturePlan {
        revision_id,
        object_id,
        git_provenance,
        snapshot_artifact_content_hash,
        supersedes: Vec::new(),
        engagement_id,
    })
}

fn parse_git_provenance(payload: &Value) -> Result<Option<GitProvenance>> {
    let (Some(source), Some(base), Some(target)) = (
        payload.get("source"),
        payload.get("base"),
        payload.get("target"),
    ) else {
        return Ok(None);
    };
    let source: ReviewUnitSource = serde_json::from_value(source.clone())?;
    let base: ReviewEndpoint = serde_json::from_value(base.clone())?;
    let target: ReviewEndpoint = serde_json::from_value(target.clone())?;
    Ok(Some(GitProvenance {
        source,
        base,
        target,
    }))
}

/// Validate a legacy source snapshot artifact before re-emitting it as v2:
///
/// 1. its stored `contentHash` must match a fresh hash of its raw body
///    (version-agnostic, covering legacy v1 and v2); and
/// 2. its body `snapshot.snapshot_id` must equal `snapshot_id` — the id the
///    capture event bound and that keys the artifact's path.
///
/// Without (1) the re-emit recomputes a clean v2 hash and would silently launder
/// corrupt or tampered bytes; without (2) a valid-but-foreign artifact placed at
/// the capture-bound path would be accepted and re-emitted as the capture's
/// content, swapping the reviewed bytes. The re-emit hides both, so they must be
/// caught here.
fn validate_legacy_artifact_integrity(artifact_value: &Value, snapshot_id: &str) -> Result<()> {
    let mut material = artifact_value.clone();
    let object = material.as_object_mut().ok_or_else(|| {
        migrate_error(&format!(
            "snapshot artifact {snapshot_id} is not a JSON object"
        ))
    })?;
    let stored = object
        .remove("contentHash")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| {
            migrate_error(&format!(
                "snapshot artifact {snapshot_id} is missing contentHash"
            ))
        })?;
    if sha256_json_prefixed(&material)? != stored {
        return Err(migrate_error(&format!(
            "snapshot artifact {snapshot_id} content hash mismatch (corrupt or tampered source); refusing to migrate"
        )));
    }
    let body_snapshot_id = artifact_value["snapshot"]["snapshot_id"]
        .as_str()
        .unwrap_or_default();
    if body_snapshot_id != snapshot_id {
        return Err(migrate_error(&format!(
            "snapshot artifact at {snapshot_id} carries a mismatched snapshot_id {body_snapshot_id:?} (swapped or corrupt source); refusing to migrate"
        )));
    }
    Ok(())
}

/// Carry the legacy event's top-level provenance and assertion mode onto the
/// reshaped event. These ride outside the identity digests, so the reshape never
/// touches them — but dropping them is a real loss: `ingest` is load-bearing for
/// the not-locally-possessed binding arm, `sourceRef` records the originating
/// system, and a non-default (`Operative`) assertion mode must survive.
fn carry_legacy_metadata(event: &mut ShoreEvent, legacy: &Value) -> Result<()> {
    if let Some(ingest) = legacy.get("ingest") {
        event.ingest = Some(serde_json::from_value::<IngestProvenance>(ingest.clone())?);
    }
    if let Some(source_ref) = legacy.get("sourceRef") {
        event.source_ref = Some(serde_json::from_value::<SourceRef>(source_ref.clone())?);
    }
    if let Some(mode) = legacy.get("assertionMode") {
        event.assertion_mode = serde_json::from_value::<AssertionMode>(mode.clone())?;
    }
    Ok(())
}

/// Re-emit a legacy `review_unit_captured` as the reshaped generative move.
fn transform_review_capture(
    value: &Value,
    capture_plans: &BTreeMap<String, CapturePlan>,
) -> Result<ShoreEvent> {
    let old_review_unit_id = legacy_review_unit_id(value)?;
    let plan = capture_plans
        .get(&old_review_unit_id)
        .ok_or_else(|| migrate_error("internal: capture plan missing for review unit"))?;

    let payload = WorkObjectProposedPayload {
        engagement_id: plan.engagement_id.clone(),
        work_object: WorkObjectProposal::Revision {
            revision: Revision {
                id: plan.revision_id.clone(),
                object_id: plan.object_id.clone(),
                git_provenance: plan.git_provenance.clone(),
            },
            snapshot_artifact_content_hash: plan.snapshot_artifact_content_hash.clone(),
            supersedes: plan.supersedes.clone(),
        },
    };
    let target = EventTarget::for_revision(
        ledger_id_of(value)?,
        plan.revision_id.clone(),
        track_id_of(value),
    );
    ShoreEvent::new(
        EventType::WorkObjectProposed,
        format!("work_object_proposed:{}", plan.revision_id.as_str()),
        target,
        writer_of(value)?,
        payload,
        occurred_at_of(value)?,
    )
}

/// Re-emit a legacy `task_attempt_captured` as the reshaped generative move's
/// task arm.
fn transform_task_capture(value: &Value) -> Result<ShoreEvent> {
    let payload = &value["payload"];
    let task_attempt_id = WorkObjectId::new(
        payload["taskAttemptId"]
            .as_str()
            .ok_or_else(|| migrate_error("task capture is missing taskAttemptId"))?,
    );
    let work_object = WorkObjectProposal::TaskAttempt {
        task_attempt_id: task_attempt_id.clone(),
        project_path: string_field(payload, "projectPath")?,
        claude_session_uuid: string_field(payload, "claudeSessionUuid")?,
        initial_prompt_hash: string_field(payload, "initialPromptHash")?,
        predecessor: payload
            .get("predecessor")
            .and_then(Value::as_str)
            .map(WorkObjectId::new),
        base_snapshot_fingerprint: payload
            .get("baseSnapshotFingerprint")
            .and_then(Value::as_str)
            .map(str::to_owned),
        source_speaker: payload
            .get("sourceSpeaker")
            .map(|value| serde_json::from_value::<SourceSpeaker>(value.clone()))
            .transpose()?,
    };
    let engagement_id = EngagementId::new(format!(
        "engagement:sha256:{}",
        sha256_bytes_hex(task_attempt_id.as_str().as_bytes())
    ));
    let new_payload = WorkObjectProposedPayload {
        engagement_id,
        work_object,
    };
    let target = EventTarget::for_subject(
        ledger_id_of(value)?,
        TargetRef::Task(TaskTargetRef::TaskAttempt),
        track_id_of(value),
    );
    ShoreEvent::new(
        EventType::WorkObjectProposed,
        format!("work_object_proposed:{}", task_attempt_id.as_str()),
        target,
        writer_of(value)?,
        new_payload,
        occurred_at_of(value)?,
    )
}

/// Reshape a generic legacy event (observation, assessment, validation, input
/// request, association, withdrawal): rewrite its envelope, remap every
/// `reviewUnitId` reference to the new `revisionId`, and recompute its identity
/// digests.
fn transform_generic(value: &Value, remap: &BTreeMap<String, String>) -> Result<ShoreEvent> {
    let mut subject = value["target"]
        .get("subject")
        .cloned()
        .unwrap_or_else(|| Value::String("ledger".to_owned()));
    remap_review_unit_ids(&mut subject, remap);

    let mut new_target = serde_json::Map::new();
    new_target.insert(
        "ledgerId".to_owned(),
        Value::String(ledger_id_of(value)?.as_str().to_owned()),
    );
    new_target.insert("subject".to_owned(), subject);
    if let Some(track) = value["target"].get("trackId") {
        new_target.insert("trackId".to_owned(), track.clone());
    }

    let mut payload = value["payload"].clone();
    remap_review_unit_ids(&mut payload, remap);

    // The association/withdrawal events serialize the old `review_unit_*` spelling
    // in their event type and lead their idempotency key with the same token. The
    // bodies already reshape through `remap_review_unit_ids`, so rewrite the event
    // type and the key prefix to `revision_*` and re-derive the id from the
    // reshaped key, matching what a native write produces.
    let legacy_event_type = value["eventType"]
        .as_str()
        .ok_or_else(|| migrate_error("event is missing eventType"))?;
    let event_type = reshaped_event_type(legacy_event_type);
    let legacy_key = value["idempotencyKey"]
        .as_str()
        .ok_or_else(|| migrate_error("event is missing idempotencyKey"))?;
    let reprefixed_key = match legacy_key.strip_prefix(legacy_event_type) {
        Some(rest) if event_type != legacy_event_type => format!("{event_type}{rest}"),
        _ => legacy_key.to_owned(),
    };
    let idempotency_key = remap_idempotency_key(&reprefixed_key, remap);
    let event_id = format!(
        "evt:sha256:{}",
        sha256_bytes_hex(idempotency_key.as_bytes())
    );
    let payload_hash = sha256_json_prefixed(&payload)?;

    let mut new_event = serde_json::Map::new();
    new_event.insert("schema".to_owned(), Value::String("shore.event".to_owned()));
    new_event.insert("version".to_owned(), Value::from(1));
    new_event.insert("eventId".to_owned(), Value::String(event_id));
    new_event.insert("eventType".to_owned(), Value::String(event_type.to_owned()));
    new_event.insert("idempotencyKey".to_owned(), Value::String(idempotency_key));
    new_event.insert("target".to_owned(), Value::Object(new_target));
    new_event.insert("writer".to_owned(), value["writer"].clone());
    new_event.insert("occurredAt".to_owned(), value["occurredAt"].clone());
    new_event.insert("payloadHash".to_owned(), Value::String(payload_hash));
    // assertionMode, sourceRef, and ingest are carried uniformly by
    // carry_legacy_metadata after the event is built.
    new_event.insert("payload".to_owned(), payload);

    Ok(serde_json::from_value(Value::Object(new_event))?)
}

/// Reshape a legacy `validation_check_recorded` event, then re-mint its
/// `validationCheckId` over the now-`revision` target so a migrated check is
/// byte-equivalent to a native post-rename write (re-validation dedupes rather
/// than duplicating). When the idempotency key was derived from the check id (the
/// default), re-derive the key, event id, and payload hash from the re-minted id;
/// a caller-supplied custom key is left untouched.
fn transform_validation_check(
    value: &Value,
    remap: &BTreeMap<String, String>,
) -> Result<ShoreEvent> {
    let mut event = transform_generic(value, remap)?;

    let mut payload: ValidationCheckRecordedPayload =
        serde_json::from_value(event.payload.clone())?;
    let ValidationTarget::Revision { revision_id } = payload.target.clone();
    let track_id = event
        .target
        .track_id
        .clone()
        .ok_or_else(|| migrate_error("validation event is missing a track"))?;
    let old_check_id = payload.validation_check_id.clone();

    let new_check_id = build_validation_check_id(ValidationCheckIdMaterial {
        review_unit_id: &revision_id,
        track_id: &track_id,
        target: &payload.target,
        check_name: &payload.check_name,
        command: payload.command.as_deref(),
        status: payload.status,
        exit_code: payload.exit_code,
        trigger: payload.trigger,
        source_fingerprint: payload.source_fingerprint.as_deref(),
        summary_content_hash: payload.summary_content_hash.as_deref(),
        started_at: payload.started_at.as_deref(),
        completed_at: payload.completed_at.as_deref(),
        log_artifact_content_hashes: &payload.log_artifact_content_hashes,
        writer_actor_id: event.writer.actor_id.as_str(),
    })?;

    if new_check_id == old_check_id {
        return Ok(event);
    }

    // The default key carries the check id as its source key; re-derive it (and the
    // event id) so the migrated event keys exactly as a native write. A custom key
    // does not embed the check id and stays as the generic remap left it.
    let default_old_key = ValidationCheckRecordedPayload::idempotency_key(
        &revision_id,
        &track_id,
        old_check_id.as_str(),
    );
    if event.idempotency_key == default_old_key {
        event.idempotency_key = ValidationCheckRecordedPayload::idempotency_key(
            &revision_id,
            &track_id,
            new_check_id.as_str(),
        );
        event.event_id = EventId::new(format!(
            "evt:sha256:{}",
            sha256_bytes_hex(event.idempotency_key.as_bytes())
        ));
    }

    payload.validation_check_id = new_check_id;
    event.payload = serde_json::to_value(&payload)?;
    event.payload_hash = sha256_json_prefixed(&event.payload)?;

    Ok(event)
}

/// Deserialize an already-reshaped event verbatim.
fn passthrough_event(value: &Value) -> Result<ShoreEvent> {
    Ok(serde_json::from_value(value.clone())?)
}

/// Recursively rename `reviewUnitId` keys to `revisionId` (remapping the value
/// through the table) and the `review_unit` subject kind to `revision`.
fn remap_review_unit_ids(value: &mut Value, remap: &BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(kind)) = map.get("kind")
                && kind == "review_unit"
            {
                map.insert("kind".to_owned(), Value::String("revision".to_owned()));
            }
            if let Some(old) = map.remove("reviewUnitId") {
                let remapped = match &old {
                    Value::String(id) => {
                        Value::String(remap.get(id).cloned().unwrap_or_else(|| id.clone()))
                    }
                    other => other.clone(),
                };
                map.insert("revisionId".to_owned(), remapped);
            }
            for child in map.values_mut() {
                remap_review_unit_ids(child, remap);
            }
        }
        Value::Array(items) => {
            for child in items.iter_mut() {
                remap_review_unit_ids(child, remap);
            }
        }
        _ => {}
    }
}

/// Whether a current-envelope event still carries a stale `review_unit` wire
/// marker — a `kind: review_unit` value or a `reviewUnitId` key — in its target or
/// payload. Phase-3 renamed the review-target wire, but a validation event written
/// before the ValidationTarget rename keeps the stale spelling under a current
/// envelope, so it must be reshaped rather than passed through. Matches only
/// structural markers, never free-text bodies that mention "review_unit".
fn carries_stale_review_unit_wire(value: &Value) -> bool {
    fn walk(value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                map.get("kind").and_then(Value::as_str) == Some("review_unit")
                    || map.contains_key("reviewUnitId")
                    || map.values().any(walk)
            }
            Value::Array(items) => items.iter().any(walk),
            _ => false,
        }
    }
    walk(&value["target"]) || walk(&value["payload"])
}

/// Map a legacy association/withdrawal `eventType` value to its reshaped
/// `revision_*` spelling. The idempotency key for these events leads with the
/// same token, so the one mapping rewrites both. Every other event type passes
/// through unchanged.
fn reshaped_event_type(event_type: &str) -> &str {
    match event_type {
        "review_unit_ref_associated" => "revision_ref_associated",
        "review_unit_ref_withdrawn" => "revision_ref_withdrawn",
        "review_unit_commit_associated" => "revision_commit_associated",
        "review_unit_commit_withdrawn" => "revision_commit_withdrawn",
        other => other,
    }
}

/// Rewrite the old `review_unit_id` substrings in an idempotency key to the new
/// `revision_id`. Ids are long unique hashes, so a substring replace is exact.
fn remap_idempotency_key(key: &str, remap: &BTreeMap<String, String>) -> String {
    let mut key = key.to_owned();
    for (old, new) in remap {
        if key.contains(old.as_str()) {
            key = key.replace(old.as_str(), new.as_str());
        }
    }
    key
}

/// Re-sign an event that was inline-signed, with the original signer's held key.
fn resign_if_signed(
    mut event: ShoreEvent,
    legacy: &Value,
    keystore: &BTreeMap<String, String>,
    options: &MigrateOptions,
    summary: &mut MigrateSummary,
) -> Result<ShoreEvent> {
    let Some(signer_did) = original_signer_did(legacy) else {
        return Ok(event);
    };
    match held_signer(keystore, options, &signer_did)? {
        Some(signer) => {
            sign_event_if_requested(&mut event, &EventSigningOptions::sign_with(signer))?;
            summary.inline_signatures_resigned += 1;
        }
        None => {
            eprintln!(
                "warning: inline signer {signer_did} is not held; leaving event {} unsigned",
                event.event_id.as_str()
            );
        }
    }
    Ok(event)
}

/// Re-home a detached co-signature: preserve a current-envelope carrier whose
/// target the reshape left untouched, otherwise re-attest it over the reshaped
/// target (held attester key) or drop it.
fn rehome_cosignature(
    value: &Value,
    target_store: &EventStore,
    old_to_new: &BTreeMap<String, ShoreEvent>,
    keystore: &BTreeMap<String, String>,
    options: &MigrateOptions,
    summary: &mut MigrateSummary,
) -> Result<()> {
    let attester_did = value["payload"]["attestingSigner"]
        .as_str()
        .ok_or_else(|| migrate_error("co-signature is missing attestingSigner"))?;
    let old_target_event_id = value["payload"]["targetEventId"]
        .as_str()
        .ok_or_else(|| migrate_error("co-signature is missing targetEventId"))?;

    let Some(new_target) = old_to_new.get(old_target_event_id) else {
        eprintln!(
            "warning: co-signature target {old_target_event_id} did not survive migration; dropping the carrier"
        );
        summary.cosignatures_dropped += 1;
        return Ok(());
    };

    // A current-envelope carrier whose target kept its event id is already valid
    // and binds the unchanged record hash; preserve it verbatim even if the
    // attester key is not held. Only a re-keyed target (every legacy event, and a
    // current-envelope association left with the stale type) needs re-attestation.
    if !is_legacy(value) && new_target.event_id.as_str() == old_target_event_id {
        let event = passthrough_event(value)?;
        record_into(target_store, &event)?;
        summary.events_migrated += 1;
        return Ok(());
    }
    let Some(signer) = held_signer(keystore, options, attester_did)? else {
        eprintln!(
            "warning: co-signature attester {attester_did} is not held; dropping the carrier"
        );
        summary.cosignatures_dropped += 1;
        return Ok(());
    };

    // Re-attest over the reshaped target: the attestation signs the target's
    // signer-inclusive TBS view (naming the attester), and the carrier binds the
    // target's signer-exclusive record hash. Both recompute against the new target,
    // so the migrated carrier verifies in the reshaped store.
    let attester_id = signer.signer_id().clone();
    let tbs = EventToBeSigned::from_event(new_target, &attester_id)?;
    let pae = event_signature_pre_authentication_encoding(&tbs)?;
    let attestation = EventSignature::ed25519_v1(signer.sign_event_message(&pae)?);

    let target_event_record_hash = new_target.event_record_hash()?;
    let idempotency_key = EventSignatureRecordedPayload::idempotency_key(
        &target_event_record_hash,
        &attester_id,
        attestation.sig.as_str(),
    );
    let payload = EventSignatureRecordedPayload {
        target_event_id: new_target.event_id.clone(),
        target_event_record_hash,
        attesting_signer: attester_id,
        attestation,
        inclusion_proof: None,
    };
    let carrier = ShoreEvent::new(
        EventType::EventSignatureRecorded,
        idempotency_key,
        EventTarget::for_ledger(new_target.target.ledger_id.clone()),
        writer_of(value)?,
        payload,
        occurred_at_of(value)?,
    )?;
    record_into(target_store, &carrier)?;
    summary.cosignatures_reattested += 1;
    summary.events_migrated += 1;
    Ok(())
}

fn original_signer_did(value: &Value) -> Option<String> {
    value.get("signature")?;
    if let Some(signer) = value.get("signer").and_then(Value::as_str) {
        return Some(signer.to_owned());
    }
    value["writer"]["actorId"].as_str().map(str::to_owned)
}

fn held_signer(
    keystore: &BTreeMap<String, String>,
    options: &MigrateOptions,
    did: &str,
) -> Result<Option<FileEd25519Signer>> {
    match keystore.get(did) {
        Some(name) => Ok(Some(load_signer_in(&options.keystore_dir, name)?)),
        None => Ok(None),
    }
}

fn record_into(store: &EventStore, event: &ShoreEvent) -> Result<()> {
    store.record_event_once(event)?;
    Ok(())
}

fn occurred_at_of(value: &Value) -> Result<String> {
    value["occurredAt"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| migrate_error("event is missing occurredAt"))
}

fn string_field(payload: &Value, field: &str) -> Result<String> {
    payload[field]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| migrate_error(&format!("payload is missing {field}")))
}

fn copy_dir_verbatim(source: &Path, target: &Path) -> Result<()> {
    let entries = match std::fs::read_dir(source) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(migrate_error(&format!(
                "read {}: {error}",
                source.display()
            )));
        }
    };
    std::fs::create_dir_all(target)
        .map_err(|error| migrate_error(&format!("create {}: {error}", target.display())))?;
    for entry in entries {
        let entry = entry.map_err(|error| migrate_error(&format!("read dir entry: {error}")))?;
        if entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            let to = target.join(entry.file_name());
            std::fs::copy(entry.path(), &to)
                .map_err(|error| migrate_error(&format!("copy {}: {error}", to.display())))?;
        }
    }
    Ok(())
}

fn migrate_error(message: &str) -> ShoreError {
    ShoreError::Message(format!("substrate migrate: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{KeyName, generate_key_in};
    use crate::session::SessionState;

    struct Fixture {
        _root: tempfile::TempDir,
        source: PathBuf,
        target: PathBuf,
        keystore: PathBuf,
        signer_did: String,
        attester_did: String,
    }

    fn fixture() -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        let keystore = root.path().join("keys");
        std::fs::create_dir_all(source.join("events")).unwrap();
        std::fs::create_dir_all(source.join("artifacts/snapshots")).unwrap();
        std::fs::create_dir_all(&keystore).unwrap();

        let signer = generate_key_in(&keystore, &KeyName::parse("agent-claude-code").unwrap())
            .unwrap()
            .signer_id()
            .as_str()
            .to_owned();
        let attester = generate_key_in(&keystore, &KeyName::parse("reviewer").unwrap())
            .unwrap()
            .signer_id()
            .as_str()
            .to_owned();

        Fixture {
            _root: root,
            source,
            target,
            keystore,
            signer_did: signer,
            attester_did: attester,
        }
    }

    impl Fixture {
        fn options(&self) -> MigrateOptions {
            MigrateOptions {
                source_store_dir: self.source.clone(),
                target_store_dir: self.target.clone(),
                keystore_dir: self.keystore.clone(),
            }
        }

        fn write_event(&self, value: &Value) {
            let key = value["idempotencyKey"].as_str().unwrap();
            let stem = sha256_bytes_hex(key.as_bytes());
            std::fs::write(
                self.source.join("events").join(format!("{stem}.json")),
                serde_json::to_vec(value).unwrap(),
            )
            .unwrap();
        }

        fn write_snapshot_artifact(&self, snapshot_id: &str, files: Value) {
            let mut body = serde_json::json!({
                "schema": "shore.snapshot",
                "version": 2,
                "snapshot": { "review_id": "review:default", "snapshot_id": snapshot_id, "files": files },
            });
            let hash = sha256_json_prefixed(&body).unwrap();
            body.as_object_mut()
                .unwrap()
                .insert("contentHash".to_owned(), Value::String(hash));
            let stem = sha256_bytes_hex(snapshot_id.as_bytes());
            std::fs::write(
                self.source
                    .join("artifacts/snapshots")
                    .join(format!("{stem}.json")),
                serde_json::to_vec(&body).unwrap(),
            )
            .unwrap();
        }
    }

    fn diff_files(text: &str) -> Value {
        serde_json::json!([{
            "id": "fileid:1",
            "status": "modified",
            "old_path": "src/lib.rs",
            "new_path": "src/lib.rs",
            "old_mode": "100644",
            "new_mode": "100644",
            "old_oid": "aaa",
            "new_oid": "bbb",
            "similarity": null,
            "is_binary": false,
            "is_submodule": false,
            "is_mode_only": false,
            "synthetic": false,
            "metadata_rows": [],
            "hunks": [{
                "id": "fileid:1#hunk",
                "header": "@@ -1 +1 @@",
                "old_start": 1,
                "old_lines": 1,
                "new_start": 1,
                "new_lines": 1,
                "rows": [{ "kind": "added", "old_line": null, "new_line": 1, "text": text }]
            }]
        }])
    }

    fn legacy_capture(review_unit_id: &str, snapshot_id: &str, signer_did: Option<&str>) -> Value {
        let mut event = serde_json::json!({
            "schema": "shore.event",
            "version": 1,
            "eventId": format!("evt:sha256:{}", "0".repeat(64)),
            "eventType": "review_unit_captured",
            "idempotencyKey": format!("review_unit_captured:{review_unit_id}"),
            "target": {
                "sessionId": "session:default",
                "reviewUnitId": review_unit_id,
                "revisionId": "rev:git:sha256:legacyrev",
                "snapshotId": snapshot_id,
                "subject": { "review": { "kind": "review_unit", "reviewUnitId": review_unit_id } }
            },
            "writer": { "actorId": "actor:agent:claude-code", "producer": { "name": "shore", "version": "0.1.0" } },
            "occurredAt": "unix-ms:1781808954225",
            "payloadHash": "sha256:legacy",
            "payload": {
                "base": { "kind": "git_commit", "commitOid": "abc", "treeOid": "def" },
                "reviewUnitId": review_unit_id,
                "revisionId": "rev:git:sha256:legacyrev",
                "snapshotArtifactContentHash": "sha256:legacyartifact",
                "snapshotId": snapshot_id,
                "source": { "kind": "git_worktree", "mode": "combined_head_to_working_tree", "includeUntracked": true },
                "target": { "kind": "git_working_tree", "worktreeRoot": "/repo" }
            }
        });
        if let Some(did) = signer_did {
            event["signer"] = Value::String(did.to_owned());
            event["signature"] =
                serde_json::json!({ "alg": "ed25519", "sigVersion": 1, "sig": "AAAA" });
        }
        event
    }

    fn legacy_observation(
        review_unit_id: &str,
        observation_id: &str,
        signer_did: Option<&str>,
    ) -> Value {
        let key =
            format!("review_observation_recorded:{review_unit_id}:agent:codex:{observation_id}");
        let mut event = serde_json::json!({
            "schema": "shore.event",
            "version": 1,
            "eventId": format!("evt:sha256:{}", "1".repeat(64)),
            "eventType": "review_observation_recorded",
            "idempotencyKey": key,
            "target": {
                "sessionId": "session:default",
                "reviewUnitId": review_unit_id,
                "trackId": "agent:codex",
                "subject": { "review": { "kind": "review_unit", "reviewUnitId": review_unit_id } }
            },
            "writer": { "actorId": "actor:agent:claude-code", "producer": { "name": "shore", "version": "0.1.0" } },
            "occurredAt": "unix-ms:1781808954300",
            "payloadHash": "sha256:legacy",
            "payload": {
                "observationId": observation_id,
                "target": { "kind": "review_unit", "reviewUnitId": review_unit_id },
                "title": "Check this",
                "tags": [],
                "supersedesObservationIds": []
            }
        });
        if let Some(did) = signer_did {
            event["signer"] = Value::String(did.to_owned());
            event["signature"] =
                serde_json::json!({ "alg": "ed25519", "sigVersion": 1, "sig": "AAAA" });
        }
        event
    }

    fn legacy_cosignature(attester_did: &str, target_event_id: &str) -> Value {
        let key = format!("event_signature_recorded:sha256:legacyrecord:{attester_did}:ZZZZ");
        serde_json::json!({
            "schema": "shore.event",
            "version": 1,
            "eventId": format!("evt:sha256:{}", "2".repeat(64)),
            "eventType": "event_signature_recorded",
            "idempotencyKey": key,
            "target": { "sessionId": "session:default" },
            "writer": { "actorId": "actor:git-email:reviewer@example.com", "producer": { "name": "shore", "version": "0.1.0" } },
            "occurredAt": "unix-ms:1781821504936",
            "payloadHash": "sha256:legacy",
            "payload": {
                "attestation": { "alg": "ed25519", "sigVersion": 1, "sig": "ZZZZ" },
                "attestingSigner": attester_did,
                "targetEventId": target_event_id,
                "targetEventRecordHash": "sha256:legacyrecord"
            }
        })
    }

    /// A legacy association/withdrawal or validation event carrying the old
    /// `review_unit` spelling. `event_type` is the legacy wire string and the
    /// idempotency key leads with it, mirroring a real pre-rename store.
    fn legacy_event(
        event_type: &str,
        idempotency_key: &str,
        stem_seed: char,
        payload: Value,
    ) -> Value {
        serde_json::json!({
            "schema": "shore.event",
            "version": 1,
            "eventId": format!("evt:sha256:{}", stem_seed.to_string().repeat(64)),
            "eventType": event_type,
            "idempotencyKey": idempotency_key,
            "target": {
                "sessionId": "session:default",
                "reviewUnitId": "review-unit:sha256:aaaa",
                "trackId": "agent:codex",
                "subject": { "review": { "kind": "review_unit", "reviewUnitId": "review-unit:sha256:aaaa" } }
            },
            "writer": { "actorId": "actor:agent:claude-code", "producer": { "name": "shore", "version": "0.1.0" } },
            "occurredAt": "unix-ms:1781808954400",
            "payloadHash": "sha256:legacy",
            "payload": payload,
        })
    }

    #[test]
    fn reshapes_legacy_review_unit_wire_surfaces_to_revision() {
        let fx = fixture();
        let review_unit = "review-unit:sha256:aaaa";
        let snapshot = "snap:git:sha256:aaaa";
        fx.write_snapshot_artifact(snapshot, diff_files("pub fn value() -> u32 { 2 }"));
        fx.write_event(&legacy_capture(review_unit, snapshot, None));

        fx.write_event(&legacy_event(
            "review_unit_commit_associated",
            &format!("review_unit_commit_associated:{review_unit}:oidcommit"),
            '3',
            serde_json::json!({
                "commitAssociationId": "assoc-commit:sha256:cccc",
                "target": { "kind": "review_unit", "reviewUnitId": review_unit },
                "commit": { "kind": "git_commit", "commitOid": "oidcommit", "treeOid": "treeoid" }
            }),
        ));
        fx.write_event(&legacy_event(
            "review_unit_ref_associated",
            &format!("review_unit_ref_associated:{review_unit}:refs/heads/main@oidhead"),
            '4',
            serde_json::json!({
                "refAssociationId": "assoc-ref:sha256:rrrr",
                "target": { "kind": "review_unit", "reviewUnitId": review_unit },
                "refName": "refs/heads/main",
                "headOid": "oidhead"
            }),
        ));
        fx.write_event(&legacy_event(
            "review_unit_commit_withdrawn",
            "review_unit_commit_withdrawn:assoc-commit:sha256:cccc",
            '5',
            serde_json::json!({
                "commitWithdrawalId": "withdraw-commit:sha256:wwww",
                "target": { "kind": "review_unit", "reviewUnitId": review_unit },
                "commitAssociationId": "assoc-commit:sha256:cccc"
            }),
        ));
        fx.write_event(&legacy_event(
            "review_unit_ref_withdrawn",
            "review_unit_ref_withdrawn:assoc-ref:sha256:rrrr",
            '6',
            serde_json::json!({
                "refWithdrawalId": "withdraw-ref:sha256:vvvv",
                "target": { "kind": "review_unit", "reviewUnitId": review_unit },
                "refAssociationId": "assoc-ref:sha256:rrrr"
            }),
        ));
        fx.write_event(&legacy_event(
            "validation_check_recorded",
            &format!("validation_check_recorded:{review_unit}:agent:codex:validation:sha256:vvvv"),
            '7',
            serde_json::json!({
                "validationCheckId": "validation:sha256:vvvv",
                "target": { "kind": "review_unit", "reviewUnitId": review_unit },
                "checkName": "cargo test",
                "status": "passed",
                "trigger": "manual",
                "logArtifactContentHashes": []
            }),
        ));

        let summary = migrate_substrate_store(fx.options()).unwrap();
        assert!(summary.self_check_passed);

        let events = EventStore::open(&fx.target).list_events().unwrap();

        // Each association/withdrawal event reshapes to its revision_* event type
        // with a matching idempotency-key prefix.
        for (event_type, prefix) in [
            (
                EventType::RevisionCommitAssociated,
                "revision_commit_associated:",
            ),
            (EventType::RevisionRefAssociated, "revision_ref_associated:"),
            (
                EventType::RevisionCommitWithdrawn,
                "revision_commit_withdrawn:",
            ),
            (EventType::RevisionRefWithdrawn, "revision_ref_withdrawn:"),
        ] {
            let event = events
                .iter()
                .find(|event| event.event_type == event_type)
                .unwrap_or_else(|| panic!("{event_type:?} is present after migration"));
            assert!(
                event.idempotency_key.starts_with(prefix),
                "{:?} idempotency key {} should lead with {prefix}",
                event_type,
                event.idempotency_key
            );
        }

        // The validation event's target reshapes from review_unit to a revision
        // pointer whose id was re-minted by the capture reshape.
        let validation = events
            .iter()
            .find(|event| event.event_type == EventType::ValidationCheckRecorded)
            .expect("the validation event is present");
        assert_eq!(validation.payload["target"]["kind"], "revision");
        assert!(
            validation.payload["target"]["revisionId"]
                .as_str()
                .is_some_and(|id| id.starts_with("rev:")),
            "validation target should bind a reshaped revision id, got {:?}",
            validation.payload["target"]
        );

        // No `review_unit` spelling survives anywhere in the reshaped store.
        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            assert!(
                !json.contains("review_unit") && !json.contains("reviewUnit"),
                "reshaped event still carries the review_unit spelling: {json}"
            );
        }
    }

    #[test]
    fn re_mints_validation_check_id_over_the_reshaped_target() {
        let fx = fixture();
        let review_unit = "review-unit:sha256:aaaa";
        let snapshot = "snap:git:sha256:aaaa";
        fx.write_snapshot_artifact(snapshot, diff_files("pub fn value() -> u32 { 2 }"));
        fx.write_event(&legacy_capture(review_unit, snapshot, None));
        fx.write_event(&legacy_event(
            "validation_check_recorded",
            &format!("validation_check_recorded:{review_unit}:agent:codex:validation:sha256:old"),
            '7',
            serde_json::json!({
                "validationCheckId": "validation:sha256:old",
                "target": { "kind": "review_unit", "reviewUnitId": review_unit },
                "checkName": "cargo test",
                "status": "passed",
                "trigger": "manual",
                "logArtifactContentHashes": []
            }),
        ));

        let summary = migrate_substrate_store(fx.options()).unwrap();
        assert!(summary.self_check_passed);

        let events = EventStore::open(&fx.target).list_events().unwrap();
        let validation = events
            .iter()
            .find(|event| event.event_type == EventType::ValidationCheckRecorded)
            .expect("the validation event is present");
        let payload: ValidationCheckRecordedPayload =
            serde_json::from_value(validation.payload.clone()).unwrap();

        // The migrated id is re-minted, not the legacy carry-over.
        assert_ne!(
            payload.validation_check_id.as_str(),
            "validation:sha256:old"
        );

        // It equals what a native write over the reshaped target produces.
        let ValidationTarget::Revision { revision_id } = &payload.target;
        let track_id = validation
            .target
            .track_id
            .clone()
            .expect("a validation event carries a track");
        let expected = build_validation_check_id(ValidationCheckIdMaterial {
            review_unit_id: revision_id,
            track_id: &track_id,
            target: &payload.target,
            check_name: &payload.check_name,
            command: payload.command.as_deref(),
            status: payload.status,
            exit_code: payload.exit_code,
            trigger: payload.trigger,
            source_fingerprint: payload.source_fingerprint.as_deref(),
            summary_content_hash: payload.summary_content_hash.as_deref(),
            started_at: payload.started_at.as_deref(),
            completed_at: payload.completed_at.as_deref(),
            log_artifact_content_hashes: &payload.log_artifact_content_hashes,
            writer_actor_id: validation.writer.actor_id.as_str(),
        })
        .unwrap();
        assert_eq!(payload.validation_check_id, expected);

        // The default-derived idempotency key (and its event id) track the re-minted id.
        assert_eq!(
            validation.idempotency_key,
            ValidationCheckRecordedPayload::idempotency_key(
                revision_id,
                &track_id,
                expected.as_str()
            )
        );
        assert_eq!(
            validation.event_id.as_str(),
            format!(
                "evt:sha256:{}",
                sha256_bytes_hex(validation.idempotency_key.as_bytes())
            )
        );
    }

    #[test]
    fn reshapes_a_current_validation_event_left_stale_by_the_target_rename() {
        // A validation event written after the envelope reshape but before the
        // ValidationTarget rename: a current envelope (ledgerId) yet a stale
        // { kind: review_unit } payload target. It must be reshaped and re-minted,
        // not passed through — the new reader rejects the stale target.
        let fx = fixture();
        let revision = "rev:sha256:already";
        let vcheck_key =
            format!("validation_check_recorded:{revision}:agent:codex:validation:sha256:stale");
        let payload = serde_json::json!({
            "validationCheckId": "validation:sha256:stale",
            "target": { "kind": "review_unit", "reviewUnitId": revision },
            "checkName": "cargo test",
            "status": "passed",
            "trigger": "manual",
            "logArtifactContentHashes": []
        });
        let payload_hash = sha256_json_prefixed(&payload).unwrap();
        fx.write_event(&serde_json::json!({
            "schema": "shore.event",
            "version": 1,
            "eventId": format!("evt:sha256:{}", sha256_bytes_hex(vcheck_key.as_bytes())),
            "eventType": "validation_check_recorded",
            "idempotencyKey": vcheck_key,
            "target": {
                "ledgerId": "ledger:default",
                "trackId": "agent:codex",
                "subject": { "review": { "kind": "revision", "revisionId": revision } }
            },
            "writer": { "actorId": "actor:agent:claude-code", "producer": { "name": "shore", "version": "0.1.0" } },
            "occurredAt": "unix-ms:1781808954600",
            "payloadHash": payload_hash,
            "payload": payload
        }));

        let summary = migrate_substrate_store(fx.options()).unwrap();
        assert!(summary.self_check_passed);

        let events = EventStore::open(&fx.target).list_events().unwrap();
        let validation = events
            .iter()
            .find(|event| event.event_type == EventType::ValidationCheckRecorded)
            .expect("the validation event is present");
        let parsed: ValidationCheckRecordedPayload =
            serde_json::from_value(validation.payload.clone()).unwrap();
        assert!(matches!(parsed.target, ValidationTarget::Revision { .. }));
        assert_ne!(
            parsed.validation_check_id.as_str(),
            "validation:sha256:stale"
        );
        let json = serde_json::to_string(validation).unwrap();
        assert!(
            !json.contains("review_unit") && !json.contains("reviewUnit"),
            "reshaped validation event still carries the review_unit spelling: {json}"
        );
    }

    #[test]
    fn reshapes_an_association_event_left_stale_by_the_envelope_reshape() {
        // An association event written after the envelope reshape but before the
        // event-type rename: a current envelope (ledgerId) and revision subject,
        // but still the old `review_unit_ref_associated` event type and key prefix.
        // It is not legacy, so it must not pass through verbatim.
        let fx = fixture();
        let revision = "rev:sha256:already";
        fx.write_event(&serde_json::json!({
            "schema": "shore.event",
            "version": 1,
            "eventId": format!("evt:sha256:{}", "9".repeat(64)),
            "eventType": "review_unit_ref_associated",
            "idempotencyKey": format!("review_unit_ref_associated:{revision}:refs/heads/main@oidhead"),
            "target": {
                "ledgerId": "ledger:default",
                "subject": { "review": { "kind": "revision", "revisionId": revision } }
            },
            "writer": { "actorId": "actor:agent:claude-code", "producer": { "name": "shore", "version": "0.1.0" } },
            "occurredAt": "unix-ms:1781808954500",
            "payloadHash": "sha256:legacy",
            "payload": {
                "refAssociationId": "assoc-ref:sha256:stale",
                "target": { "kind": "revision", "revisionId": revision },
                "refName": "refs/heads/main",
                "headOid": "oidhead"
            }
        }));

        let summary = migrate_substrate_store(fx.options()).unwrap();
        assert!(summary.self_check_passed);
        assert_eq!(summary.events_migrated, 1);

        let events = EventStore::open(&fx.target).list_events().unwrap();
        let assoc = events
            .iter()
            .find(|event| event.event_type == EventType::RevisionRefAssociated)
            .expect("the stale association event reshaped to the revision wire");
        assert!(
            assoc
                .idempotency_key
                .starts_with("revision_ref_associated:")
        );
        // The event id is re-derived from the reshaped idempotency key.
        assert_eq!(
            assoc.event_id.as_str(),
            format!(
                "evt:sha256:{}",
                sha256_bytes_hex(assoc.idempotency_key.as_bytes())
            )
        );
    }

    #[test]
    fn re_homes_a_current_cosignature_when_its_association_target_is_re_keyed() {
        // A current-envelope detached co-signature attesting a current-envelope
        // association event that still carries the stale review_unit_ref_associated
        // type. Migrating re-keys the association (new event id), so the carrier must
        // be re-homed onto the new id, never passed through still pointing at the
        // vanished old id.
        let fx = fixture();
        let revision = "rev:sha256:already";
        let assoc_old_id = format!("evt:sha256:{}", "8".repeat(64));
        fx.write_event(&serde_json::json!({
            "schema": "shore.event",
            "version": 1,
            "eventId": assoc_old_id,
            "eventType": "review_unit_ref_associated",
            "idempotencyKey": format!("review_unit_ref_associated:{revision}:refs/heads/main@oidhead"),
            "target": {
                "ledgerId": "ledger:default",
                "subject": { "review": { "kind": "revision", "revisionId": revision } }
            },
            "writer": { "actorId": "actor:agent:claude-code", "producer": { "name": "shore", "version": "0.1.0" } },
            "occurredAt": "unix-ms:1781808954500",
            "payloadHash": "sha256:legacy",
            "payload": {
                "refAssociationId": "assoc-ref:sha256:stale",
                "target": { "kind": "revision", "revisionId": revision },
                "refName": "refs/heads/main",
                "headOid": "oidhead"
            }
        }));
        let cosig_key = format!(
            "event_signature_recorded:sha256:oldrecord:{}:ZZZZ",
            fx.attester_did
        );
        let cosig_id = format!("evt:sha256:{}", sha256_bytes_hex(cosig_key.as_bytes()));
        let cosig_payload = serde_json::json!({
            "attestation": { "alg": "ed25519", "sigVersion": 1, "sig": "ZZZZ" },
            "attestingSigner": fx.attester_did.as_str(),
            "targetEventId": assoc_old_id,
            "targetEventRecordHash": "sha256:oldrecord"
        });
        let cosig_payload_hash = sha256_json_prefixed(&cosig_payload).unwrap();
        fx.write_event(&serde_json::json!({
            "schema": "shore.event",
            "version": 1,
            "eventId": cosig_id,
            "eventType": "event_signature_recorded",
            "idempotencyKey": cosig_key,
            "target": { "ledgerId": "ledger:default", "subject": "ledger" },
            "writer": { "actorId": "actor:git-email:reviewer@example.com", "producer": { "name": "shore", "version": "0.1.0" } },
            "occurredAt": "unix-ms:1781821504936",
            "payloadHash": cosig_payload_hash,
            "payload": cosig_payload
        }));

        let summary = migrate_substrate_store(fx.options()).unwrap();
        assert!(summary.self_check_passed);
        assert_eq!(summary.cosignatures_reattested, 1);
        assert_eq!(summary.cosignatures_dropped, 0);

        // No detached co-signature dangles: every carrier's targetEventId resolves to
        // a migrated event, and the re-keyed association is its new home.
        let events = EventStore::open(&fx.target).list_events().unwrap();
        let ids: std::collections::BTreeSet<&str> =
            events.iter().map(|event| event.event_id.as_str()).collect();
        for event in &events {
            if event.event_type == EventType::EventSignatureRecorded {
                let target_id = event.payload["targetEventId"].as_str().unwrap();
                assert!(
                    ids.contains(target_id),
                    "co-signature targetEventId {target_id} dangles"
                );
                assert_ne!(
                    target_id, assoc_old_id,
                    "carrier still points at the pre-reshape id"
                );
            }
        }
    }

    #[test]
    fn migrates_a_signed_multi_actor_store_losslessly() {
        let fx = fixture();
        let review_unit = "review-unit:sha256:aaaa";
        let snapshot = "snap:git:sha256:aaaa";
        fx.write_snapshot_artifact(snapshot, diff_files("pub fn value() -> u32 { 2 }"));
        let capture = legacy_capture(review_unit, snapshot, Some(&fx.signer_did));
        let capture_event_id = capture["eventId"].as_str().unwrap().to_owned();
        fx.write_event(&capture);
        fx.write_event(&legacy_observation(
            review_unit,
            "obs:sha256:one",
            Some(&fx.signer_did),
        ));
        fx.write_event(&legacy_cosignature(&fx.attester_did, &capture_event_id));

        let summary = migrate_substrate_store(fx.options()).unwrap();

        // capture + observation + re-attested co-signature; no lineage.
        assert_eq!(summary.events_migrated, 3);
        assert_eq!(summary.cosignatures_reattested, 1);
        assert_eq!(summary.cosignatures_dropped, 0);
        assert_eq!(summary.inline_signatures_resigned, 2);
        assert!(summary.self_check_passed);

        // The re-attested co-signature genuinely verifies against the reshaped
        // target: its attestation signs the new target's signer-inclusive view.
        use crate::crypto::{EventVerificationStatus, verify_ed25519_strict};
        let events = EventStore::open(&fx.target).list_events().unwrap();
        let carrier = events
            .iter()
            .find(|event| event.event_type == EventType::EventSignatureRecorded)
            .expect("the re-attested carrier is present");
        let payload: EventSignatureRecordedPayload =
            serde_json::from_value(carrier.payload.clone()).unwrap();
        let target = events
            .iter()
            .find(|event| event.event_id == payload.target_event_id)
            .expect("the carrier's reshaped target is present");
        assert_eq!(
            payload.target_event_record_hash,
            target.event_record_hash().unwrap()
        );
        let tbs = EventToBeSigned::from_event(target, &payload.attesting_signer).unwrap();
        let pae = event_signature_pre_authentication_encoding(&tbs).unwrap();
        let status = verify_ed25519_strict(
            &payload.attesting_signer,
            &pae,
            payload.attestation.sig.as_str(),
        )
        .unwrap();
        assert_eq!(status, EventVerificationStatus::Valid);
    }

    #[test]
    fn translates_lineage_round_predecessor_to_supersedes() {
        let fx = fixture();
        let pred = "review-unit:sha256:pred";
        let succ = "review-unit:sha256:succ";
        fx.write_snapshot_artifact("snap:git:sha256:pred", diff_files("fn a() {}"));
        fx.write_snapshot_artifact("snap:git:sha256:succ", diff_files("fn b() {}"));
        fx.write_event(&legacy_capture(pred, "snap:git:sha256:pred", None));
        fx.write_event(&legacy_capture(succ, "snap:git:sha256:succ", None));
        fx.write_event(&serde_json::json!({
            "schema": "shore.event", "version": 1,
            "eventId": format!("evt:sha256:{}", "3".repeat(64)),
            "eventType": "review_unit_lineage_declared",
            "idempotencyKey": "review_unit_lineage_declared:lineage:sha256:l",
            "target": { "sessionId": "session:default" },
            "writer": { "actorId": "actor:agent:claude-code", "producer": { "name": "shore", "version": "0.1.0" } },
            "occurredAt": "unix-ms:1781808954400",
            "payloadHash": "sha256:legacy",
            "payload": { "lineageId": "lineage:sha256:l", "basis": { "source": { "kind": "git_working_tree" }, "base": { "kind": "git_working_tree", "worktreeRoot": "/repo" } } }
        }));
        fx.write_event(&serde_json::json!({
            "schema": "shore.event", "version": 1,
            "eventId": format!("evt:sha256:{}", "4".repeat(64)),
            "eventType": "review_unit_lineage_round_recorded",
            "idempotencyKey": "review_unit_lineage_round_recorded:lineage:sha256:l:".to_owned() + succ,
            "target": { "sessionId": "session:default" },
            "writer": { "actorId": "actor:agent:claude-code", "producer": { "name": "shore", "version": "0.1.0" } },
            "occurredAt": "unix-ms:1781808954500",
            "payloadHash": "sha256:legacy",
            "payload": { "lineageId": "lineage:sha256:l", "roundId": "round:sha256:r", "reviewUnitId": succ, "predecessorReviewUnitId": pred }
        }));

        let summary = migrate_substrate_store(fx.options()).unwrap();

        assert_eq!(summary.lineage_rounds_folded, 1);
        // two captures; the two lineage carriers are not re-emitted.
        assert_eq!(summary.events_migrated, 2);

        let events = EventStore::open(&fx.target).list_events().unwrap();
        let proposals: Vec<_> = events
            .iter()
            .filter(|event| event.event_type == EventType::WorkObjectProposed)
            .collect();
        assert_eq!(proposals.len(), 2);
        let successor = proposals
            .iter()
            .find(|event| {
                event.payload["workObject"]["supersedes"]
                    .as_array()
                    .map(|array| !array.is_empty())
                    .unwrap_or(false)
            })
            .expect("the successor carries a supersedes pointer");
        let supersedes = successor.payload["workObject"]["supersedes"]
            .as_array()
            .unwrap();
        assert_eq!(supersedes.len(), 1);
        assert!(supersedes[0].as_str().unwrap().starts_with("rev:sha256:"));
    }

    #[test]
    fn projects_legacy_target_into_the_reshaped_envelope() {
        let fx = fixture();
        let review_unit = "review-unit:sha256:envelope";
        fx.write_snapshot_artifact("snap:git:sha256:env", diff_files("fn c() {}"));
        fx.write_event(&legacy_capture(review_unit, "snap:git:sha256:env", None));
        fx.write_event(&legacy_observation(review_unit, "obs:sha256:env", None));

        migrate_substrate_store(fx.options()).unwrap();

        let events = EventStore::open(&fx.target).list_events().unwrap();
        let observation = events
            .iter()
            .find(|event| event.event_type == EventType::ReviewObservationRecorded)
            .expect("observation migrated");
        let target = serde_json::to_value(&observation.target).unwrap();
        assert_eq!(target["ledgerId"], "session:default");
        assert_eq!(target["subject"]["review"]["kind"], "revision");
        assert!(target.get("sessionId").is_none());
        assert!(target.get("reviewUnitId").is_none());
        assert!(target.get("snapshotId").is_none());
        // the observation now keys on the capture's new revision id.
        let revision = target["subject"]["review"]["revisionId"].as_str().unwrap();
        assert!(revision.starts_with("rev:sha256:"));
    }

    #[test]
    fn collapses_both_legacy_capture_events_into_work_object_proposed() {
        let fx = fixture();
        fx.write_snapshot_artifact("snap:git:sha256:rev", diff_files("fn d() {}"));
        fx.write_event(&legacy_capture(
            "review-unit:sha256:rev",
            "snap:git:sha256:rev",
            None,
        ));
        fx.write_event(&serde_json::json!({
            "schema": "shore.event", "version": 1,
            "eventId": format!("evt:sha256:{}", "5".repeat(64)),
            "eventType": "task_attempt_captured",
            "idempotencyKey": "task_attempt_captured:task-attempt:sha256:t",
            "target": {
                "sessionId": "session:default",
                "workObjectId": "task-attempt:sha256:t",
                "workObjectType": "task_attempt",
                "subject": { "task": { "kind": "task_attempt" } }
            },
            "writer": { "actorId": "actor:agent:claude-code", "producer": { "name": "shore", "version": "0.1.0" } },
            "occurredAt": "unix-ms:1781808954600",
            "payloadHash": "sha256:legacy",
            "payload": {
                "taskAttemptId": "task-attempt:sha256:t",
                "projectPath": "/repo",
                "claudeSessionUuid": "uuid-1",
                "initialPromptHash": "sha256:prompt"
            }
        }));

        migrate_substrate_store(fx.options()).unwrap();

        let events = EventStore::open(&fx.target).list_events().unwrap();
        let proposals: Vec<_> = events
            .iter()
            .filter(|event| event.event_type == EventType::WorkObjectProposed)
            .collect();
        assert_eq!(proposals.len(), 2);
        let kinds: std::collections::BTreeSet<&str> = proposals
            .iter()
            .map(|event| event.payload["workObject"]["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["revision", "task_attempt"].into_iter().collect());
    }

    #[test]
    fn foreign_key_cosignature_is_dropped_with_a_warning() {
        let fx = fixture();
        let review_unit = "review-unit:sha256:foreign";
        fx.write_snapshot_artifact("snap:git:sha256:foreign", diff_files("fn e() {}"));
        let capture = legacy_capture(review_unit, "snap:git:sha256:foreign", None);
        let capture_event_id = capture["eventId"].as_str().unwrap().to_owned();
        fx.write_event(&capture);
        // an attester whose key is NOT in the keystore.
        fx.write_event(&legacy_cosignature(
            "did:key:z6MkfFraudNotHeldAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            &capture_event_id,
        ));

        let summary = migrate_substrate_store(fx.options()).unwrap();

        assert_eq!(summary.cosignatures_dropped, 1);
        assert_eq!(summary.cosignatures_reattested, 0);
        // only the capture migrated; the foreign carrier was dropped.
        assert_eq!(summary.events_migrated, 1);
    }

    #[test]
    fn reshaped_store_lists_and_rebuilds_cleanly() {
        let fx = fixture();
        let review_unit = "review-unit:sha256:selfcheck";
        fx.write_snapshot_artifact("snap:git:sha256:sc", diff_files("fn f() {}"));
        fx.write_event(&legacy_capture(
            review_unit,
            "snap:git:sha256:sc",
            Some(&fx.signer_did),
        ));
        fx.write_event(&legacy_observation(
            review_unit,
            "obs:sha256:sc",
            Some(&fx.signer_did),
        ));

        let summary = migrate_substrate_store(fx.options()).unwrap();
        assert!(summary.self_check_passed);

        let events = EventStore::open(&fx.target).list_events().unwrap();
        let state = SessionState::from_events(&events).unwrap();
        assert_eq!(state.event_count, events.len());
    }

    #[test]
    fn a_half_migrated_store_is_rejected_loudly() {
        let fx = fixture();
        let review_unit = "review-unit:sha256:stray";
        fx.write_snapshot_artifact("snap:git:sha256:stray", diff_files("fn g() {}"));
        fx.write_event(&legacy_capture(review_unit, "snap:git:sha256:stray", None));
        migrate_substrate_store(fx.options()).unwrap();

        // Inject a stray old-shape event file into the reshaped store.
        let stray = legacy_observation(review_unit, "obs:sha256:stray", None);
        let stem = sha256_bytes_hex(stray["idempotencyKey"].as_str().unwrap().as_bytes());
        std::fs::write(
            fx.target.join("events").join(format!("{stem}.json")),
            serde_json::to_vec(&stray).unwrap(),
        )
        .unwrap();

        let error = EventStore::open(&fx.target).list_events().unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("ledgerId")
                || message.contains("subject")
                || message.contains("review_unit"),
            "stray old-shape file must be rejected loudly, got: {message}"
        );
    }

    #[test]
    fn passes_through_already_reshaped_events() {
        let fx = fixture();
        // A store captured under the new binary: an already-reshaped event.
        let new_event = serde_json::json!({
            "schema": "shore.event", "version": 1,
            "eventId": format!("evt:sha256:{}", sha256_bytes_hex(b"work_object_proposed:rev:sha256:new")),
            "eventType": "work_object_proposed",
            "idempotencyKey": "work_object_proposed:rev:sha256:new",
            "target": { "ledgerId": "ledger:default", "subject": { "review": { "kind": "revision", "revisionId": "rev:sha256:new" } } },
            "writer": { "actorId": "actor:agent:claude-code", "producer": { "name": "shore", "version": "0.1.0" } },
            "occurredAt": "unix-ms:1782010720298",
            "payloadHash": "sha256:placeholder",
            "payload": {
                "engagementId": "engagement:sha256:e",
                "workObject": {
                    "kind": "revision",
                    "revision": { "id": "rev:sha256:new", "objectId": "obj:sha256:o" },
                    "snapshotArtifactContentHash": "sha256:a"
                }
            }
        });
        // fix the payload hash to be valid.
        let payload_hash = sha256_json_prefixed(&new_event["payload"]).unwrap();
        let mut new_event = new_event;
        new_event["payloadHash"] = Value::String(payload_hash);
        fx.write_event(&new_event);

        let summary = migrate_substrate_store(fx.options()).unwrap();
        assert_eq!(summary.events_migrated, 1);

        let events = EventStore::open(&fx.target).list_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::WorkObjectProposed);
    }

    #[test]
    fn rejects_a_tampered_source_artifact() {
        let fx = fixture();
        let review_unit = "review-unit:sha256:tamper";
        let snapshot = "snap:git:sha256:tamper";
        fx.write_snapshot_artifact(snapshot, diff_files("fn h() {}"));
        // Corrupt the source artifact's stored contentHash so its body no longer
        // matches: the migrator must refuse rather than launder it into a clean v2.
        let stem = sha256_bytes_hex(snapshot.as_bytes());
        let path = fx
            .source
            .join("artifacts/snapshots")
            .join(format!("{stem}.json"));
        let mut body: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        body["contentHash"] = Value::String("sha256:not-the-hash".to_owned());
        std::fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
        fx.write_event(&legacy_capture(review_unit, snapshot, None));

        let error = migrate_substrate_store(fx.options())
            .expect_err("a tampered source artifact must be refused");
        assert!(
            error.to_string().contains("content hash mismatch"),
            "expected a refusal naming the hash mismatch, got: {error}"
        );
    }

    #[test]
    fn rejects_a_swapped_source_artifact() {
        // A valid, internally-consistent artifact (body matches its own contentHash)
        // placed at the capture-bound path but carrying a DIFFERENT snapshot_id must
        // be refused — otherwise the re-emit would swap the reviewed bytes.
        let fx = fixture();
        let review_unit = "review-unit:sha256:swap";
        let bound_snapshot = "snap:git:sha256:swap";
        // Write a valid artifact whose body snapshot_id is FOREIGN, then move it to
        // the path keyed by the capture-bound snapshot id.
        fx.write_snapshot_artifact("snap:git:sha256:foreign", diff_files("fn j() {}"));
        let foreign_stem = sha256_bytes_hex("snap:git:sha256:foreign".as_bytes());
        let bound_stem = sha256_bytes_hex(bound_snapshot.as_bytes());
        let dir = fx.source.join("artifacts/snapshots");
        std::fs::rename(
            dir.join(format!("{foreign_stem}.json")),
            dir.join(format!("{bound_stem}.json")),
        )
        .unwrap();
        fx.write_event(&legacy_capture(review_unit, bound_snapshot, None));

        let error = migrate_substrate_store(fx.options())
            .expect_err("a swapped source artifact must be refused");
        assert!(
            error.to_string().contains("mismatched snapshot_id"),
            "expected a refusal naming the swapped snapshot_id, got: {error}"
        );
    }

    #[test]
    fn carries_top_level_provenance_and_assertion_mode() {
        let fx = fixture();
        let review_unit = "review-unit:sha256:prov";
        fx.write_snapshot_artifact("snap:git:sha256:prov", diff_files("fn i() {}"));
        fx.write_event(&legacy_capture(review_unit, "snap:git:sha256:prov", None));
        // A legacy event carrying ingest + sourceRef + an explicit assertion mode:
        // all three ride outside identity and must survive the reshape.
        let mut observation = legacy_observation(review_unit, "obs:sha256:prov", None);
        observation["ingest"] =
            serde_json::json!({ "via": "bundle-apply", "receivedAt": "unix-ms:1781810525631" });
        observation["sourceRef"] =
            serde_json::json!({ "sourceSystem": "claude_code", "sourceId": "tool_result:7" });
        observation["assertionMode"] = Value::String("operative".to_owned());
        fx.write_event(&observation);

        migrate_substrate_store(fx.options()).unwrap();

        let events = EventStore::open(&fx.target).list_events().unwrap();
        let migrated = events
            .iter()
            .find(|event| event.event_type == EventType::ReviewObservationRecorded)
            .expect("observation migrated");
        let ingest =
            serde_json::to_value(migrated.ingest.as_ref().expect("ingest preserved")).unwrap();
        assert_eq!(ingest["via"], "bundle-apply");
        let source_ref =
            serde_json::to_value(migrated.source_ref.as_ref().expect("sourceRef preserved"))
                .unwrap();
        assert_eq!(source_ref["sourceSystem"], "claude_code");
        assert_eq!(migrated.assertion_mode, AssertionMode::Operative);
    }
}
