//! One-shot store migrator for the opaque-coded signed-identity break.
//!
//! Reads a pre-break store relaxed (raw JSON, bypassing the strict reader),
//! reshapes every event onto the opaque-coded envelope — the `t:NN` type code in
//! place of the renamable snake_case name and the opaque `subjectId` in place of
//! the structural subject — re-derives the content ids that now fold the opaque
//! subject, re-keys every idempotency key, re-signs inline signatures and
//! re-homes detached co-signatures with held keys, then writes a fresh store the
//! strict reader accepts and self-checks it.
//!
//! Because every idempotency key now leads with the type code, every `eventId`
//! and every signature-exclusive `eventRecordHash` moves. Content ids that fold
//! the opaque subject (observations, assessments, validations, and the two
//! review-domain input-request families) move too and are re-derived from the
//! current payload through the live builders, in dependency order via a fixpoint
//! over the reference graph, so each reference is remapped before the event that
//! folds it. Association and withdrawal ids fold only stable material, so they
//! ride through re-enveloped and re-keyed but not re-derived. Object and note
//! artifacts are content-addressed by body hashes the break never touches, so
//! they are copied verbatim.
//!
//! This is throwaway, owner-run tooling, not a shipped command: the migrator
//! (this module plus its driver) is removed once the owner store has been
//! migrated; the durable record is the strict reader. Its one persisted output
//! is the unsigned, local `migration-manifest.json` side file it writes into the
//! target store dir — a maintenance audit record, never a journal event (copying
//! local bytes is not a shared review fact). The manifest *writer* is deleted
//! with the migrator, while the emitted manifest *file* persists as data in the
//! owner's runbook area. Generalizing that side file into shipped maintenance
//! commands is deliberately out of scope here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use super::EventStore;
use super::backend::StoreBackend;
use crate::canonical_hash::{sha256_bytes_hex, sha256_json_prefixed};
use crate::crypto::EventSigner;
use crate::error::{Result, ShoreError};
use crate::keys::{FileEd25519Signer, KeyCustody, list_keys_in, load_signer_in};
use crate::model::{JournalId, TargetRef, TrackId, ValidationTarget, subject_revision_id};
use crate::session::event::{
    AssertionMode, EventSignature, EventSignatureRecordedPayload, EventTarget, EventToBeSigned,
    EventType, InputRequestOpenedPayload, InputRequestRespondedPayload,
    ReviewAssessmentRecordedPayload, ReviewObservationRecordedPayload, ShoreEvent,
    ValidationCheckRecordedPayload, Writer, event_signature_pre_authentication_encoding,
    event_type_from_code, subject_id, type_code,
};
use crate::session::workflow::assessment::add::{AssessmentIdMaterial, build_assessment_id};
use crate::session::workflow::input_request::open::{
    InputRequestIdMaterial, build_input_request_id,
};
use crate::session::workflow::input_request::respond::{
    InputRequestResponseIdMaterial, build_input_request_response_id,
};
use crate::session::workflow::observation::add::{ObservationIdMaterial, build_observation_id};
use crate::session::workflow::validation::add::{
    ValidationCheckIdMaterial, build_validation_check_id,
};
use crate::session::{
    EventSigningOptions, SessionState, current_timestamp, sign_event_if_requested,
};

/// Inputs for one migration pass. Generic: all three locations are parameters,
/// with no built-in repo, key, or path assumptions.
#[derive(Clone, Debug)]
pub struct MigrateOptions {
    /// The pre-break source store directory to read (the dir holding `events/`
    /// and `artifacts/`).
    pub source_store_dir: PathBuf,
    /// A fresh, empty store directory to write the re-shaped store into.
    pub target_store_dir: PathBuf,
    /// The keystore directory holding the signers' private keys, used to re-sign
    /// inline signatures and re-attest held-key co-signatures.
    pub keystore_dir: PathBuf,
}

/// What one migration pass did. The owner-run step reads this (and the manifest
/// it is folded into) to confirm the migration was lossless and the re-shaped
/// store self-validated.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateSummary {
    /// Events written to the re-shaped store (transformed and re-attested
    /// co-signatures).
    pub events_migrated: usize,
    /// Events copied through verbatim because they carry nothing the reshape
    /// touches. Structurally unreachable in this break (every record hash moves),
    /// kept for symmetry with the co-signature re-home matrix.
    pub events_passed_through: usize,
    /// Content ids re-derived under the opaque subject (observations,
    /// assessments, validations, and the two review-domain input-request
    /// families).
    pub content_ids_rederived: usize,
    /// Inline signatures re-signed with the original signer's held key.
    pub inline_signatures_resigned: usize,
    /// Detached co-signatures re-attested with the attester's held key.
    pub cosignatures_reattested: usize,
    /// Detached co-signatures dropped because the attester's key is not held (or
    /// the target did not survive), counted and warned, never silent.
    pub cosignatures_dropped: usize,
    /// Content-removed captures whose binding hash is preserved without a
    /// re-hashable artifact present.
    pub content_removed_preserved: usize,
    /// Whether the re-shaped store passed its self-check (`list_events` rebuilds
    /// cleanly under the strict read path, `SessionState::from_events` succeeds,
    /// no stale wire token survives, and every content id matches its payload).
    pub self_check_passed: bool,
}

/// Migrate the store at `source_store_dir` into a fresh opaque-coded store at
/// `target_store_dir`, re-signing with keys from `keystore_dir`.
pub fn migrate_opaque_identity(options: MigrateOptions) -> Result<MigrateSummary> {
    let raw = read_raw_events(&options.source_store_dir)?;
    let keystore = build_keystore_index(&options.keystore_dir)?;
    let mut summary = MigrateSummary::default();

    // Every moving content id this store actually produces. A moving reference to
    // an id in this set is a genuine dependency the fixpoint orders behind its
    // producer; a moving reference to an id *not* in it is an advisory
    // forward-pointer (ADR-0026 `responds_to`) or other cross-store reference to
    // content this store never held, and must ride through verbatim rather than
    // stall the fixpoint on a reference that can never resolve.
    let produced = collect_produced_content_ids(&raw)?;

    let target = EventStore::from_backend(&StoreBackend::Local(options.target_store_dir.clone()));
    let mut content_remap: BTreeMap<String, String> = BTreeMap::new();
    let mut old_to_new: BTreeMap<String, ShoreEvent> = BTreeMap::new();

    // Pass 1: re-emit every non-co-signature event in dependency order. A content
    // event is processed once every content id it references is already
    // re-derived; the fixpoint guarantees that order regardless of `occurredAt`
    // ties. Captures, associations, and content-id-free events reference nothing
    // and land in the first round.
    let mut pending: Vec<&Value> = raw.iter().filter(|value| !is_cosignature(value)).collect();
    pending.sort_by(|a, b| occurred_at_str(a).cmp(occurred_at_str(b)));

    while !pending.is_empty() {
        let mut progressed = false;
        let mut still_pending: Vec<&Value> = Vec::with_capacity(pending.len());
        for value in pending {
            if !references_resolved(value, &content_remap, &produced)? {
                still_pending.push(value);
                continue;
            }
            let old_event_id = event_id_of(value)?;
            let event = transform_pass_one(
                value,
                &mut content_remap,
                &produced,
                &keystore,
                &options,
                &mut summary,
            )?;
            record_into(&target, &event)?;
            old_to_new.insert(old_event_id, event);
            progressed = true;
        }
        if !progressed {
            return Err(migrate_error(
                "unresolved content-id references or a dependency cycle in the event graph",
            ));
        }
        pending = still_pending;
    }

    // Pass 2: re-home detached co-signatures, after every target is written.
    for value in &raw {
        if !is_cosignature(value) {
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

    // Artifacts are content-addressed by body hashes the reshape never changes, so
    // the migrated events still resolve them: copy both trees verbatim.
    copy_dir_verbatim(
        &options.source_store_dir.join("artifacts/objects"),
        &options.target_store_dir.join("artifacts/objects"),
    )?;
    copy_dir_verbatim(
        &options.source_store_dir.join("artifacts/notes"),
        &options.target_store_dir.join("artifacts/notes"),
    )?;

    // Self-check: the re-shaped store must list cleanly under the strict read
    // path, rebuild its projection, carry no stale wire token, and re-derive every
    // content id to the value it stored.
    let events = target.list_events()?;
    let _state = SessionState::from_events(&events)?;
    verify_no_stale_wire(&events)?;
    verify_inline_convergence(&events)?;
    summary.self_check_passed = true;

    // Emit the maintenance manifest last, once the self-check has populated the
    // summary counts. It is a local side file, never a journal event.
    write_manifest(&options, &summary)?;

    Ok(summary)
}

fn read_raw_events(source_store_dir: &Path) -> Result<Vec<Value>> {
    let entries = StoreBackend::Local(source_store_dir.to_path_buf())
        .journal()
        .list_event_entries()?;
    let mut events = Vec::with_capacity(entries.len());
    for entry in entries {
        events.push(serde_json::from_slice(&entry.bytes)?);
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

// ---------------------------------------------------------------------------
// Pass 1: events
// ---------------------------------------------------------------------------

/// Re-emit one non-co-signature event onto the opaque-coded envelope. The caller
/// has confirmed every content id this event references is already re-derived.
fn transform_pass_one(
    value: &Value,
    content_remap: &mut BTreeMap<String, String>,
    produced: &BTreeSet<String>,
    keystore: &BTreeMap<String, String>,
    options: &MigrateOptions,
    summary: &mut MigrateSummary,
) -> Result<ShoreEvent> {
    let event_type = event_type_of(value)?;
    let mut migrated = value.clone();

    // The structural subject the pre-break envelope carried. Threaded input-request
    // fields are read back from it before it is dropped for the opaque subjectId.
    let legacy_subject: TargetRef = serde_json::from_value(subject_value(&migrated).clone())
        .map_err(|error| {
            migrate_error(&format!("event target.subject does not decode: {error}"))
        })?;
    backfill_threaded_fields(&mut migrated, event_type, &legacy_subject);

    // Remap every moving content id this event references (in the envelope subject
    // and the payload reference fields), then re-parse the reshaped subject.
    remap_references(&mut migrated, content_remap, produced)?;
    let subject: TargetRef =
        serde_json::from_value(subject_value(&migrated).clone()).map_err(|error| {
            migrate_error(&format!("reshaped target.subject does not decode: {error}"))
        })?;
    let addresses_task = matches!(subject, TargetRef::Task(_));

    let track_id = migrated["target"]
        .get("trackId")
        .and_then(Value::as_str)
        .map(TrackId::new);
    let writer_actor_id = migrated["writer"]["actorId"]
        .as_str()
        .ok_or_else(|| migrate_error("event writer is missing actorId"))?
        .to_owned();
    let assertion_mode = assertion_mode_of(value);

    // Content-id re-derivation for the families that fold the opaque subject.
    if let Some(kind) = ContentKind::from_event_type(event_type) {
        let rederive = kind.is_moving() && !(kind.is_input_request() && addresses_task);
        if rederive {
            let track = track_id
                .clone()
                .ok_or_else(|| migrate_error("content event envelope is missing trackId"))?;
            let new_id = rederive_content_id(
                kind,
                &migrated["payload"],
                &track,
                &writer_actor_id,
                assertion_mode,
            )?;
            let old_id = migrated["payload"][kind.own_field()]
                .as_str()
                .ok_or_else(|| {
                    migrate_error(&format!("{} event is missing its id", kind.own_field()))
                })?
                .to_owned();
            content_remap.insert(old_id, new_id.clone());
            migrated["payload"][kind.own_field()] = Value::String(new_id);
            normalize_fact_lists(kind, &mut migrated["payload"]);
            summary.content_ids_rederived += 1;
        }
    }

    // Re-key onto the type code, substituting every re-derived id the old key
    // embedded, then reshape the envelope.
    let legacy_key = value["idempotencyKey"]
        .as_str()
        .ok_or_else(|| migrate_error("event is missing idempotencyKey"))?;
    let new_key = rekey(legacy_key, event_type, &subject, content_remap)?;

    let journal_id = JournalId::new(
        migrated["target"]["journalId"]
            .as_str()
            .ok_or_else(|| migrate_error("event target is missing journalId"))?,
    );
    let target = EventTarget::for_subject(journal_id, subject, track_id)?;

    migrated["eventType"] = Value::String(type_code(event_type).to_owned());
    migrated["idempotencyKey"] = Value::String(new_key);
    migrated["target"] = serde_json::to_value(&target)?;
    migrated["signer"] = Value::Null;
    migrated["signature"] = Value::Null;

    let mut event: ShoreEvent = serde_json::from_value(migrated)?;
    event.payload_hash = sha256_json_prefixed(&event.payload)?;
    event.event_id = derive_event_id(&event.idempotency_key);

    let event = resign_if_signed(event, value, keystore, options, summary)?;
    summary.events_migrated += 1;
    Ok(event)
}

/// Read the threaded input-request fields back from the pre-break envelope
/// subject: after the break, a review-domain response carries its `revisionId`
/// and a task-domain request or response carries its `taskTarget` in the payload
/// (the reconstruction source), which the pre-break payload lacked. Backfilled
/// only when absent, so a payload written just before the break is untouched.
fn backfill_threaded_fields(
    migrated: &mut Value,
    event_type: EventType,
    legacy_subject: &TargetRef,
) {
    let Some(payload) = migrated.get_mut("payload").and_then(Value::as_object_mut) else {
        return;
    };
    match event_type {
        EventType::InputRequestOpened => {
            if let TargetRef::Task(task) = legacy_subject
                && !payload.contains_key("taskTarget")
                && let Ok(task_value) = serde_json::to_value(task)
            {
                payload.insert("taskTarget".to_owned(), task_value);
            }
        }
        EventType::InputRequestResponded => match legacy_subject {
            TargetRef::Review(_) => {
                if !payload.contains_key("revisionId")
                    && let Some(revision_id) = subject_revision_id(legacy_subject)
                {
                    payload.insert(
                        "revisionId".to_owned(),
                        Value::String(revision_id.as_str().to_owned()),
                    );
                }
            }
            TargetRef::Task(task) => {
                if !payload.contains_key("taskTarget")
                    && let Ok(task_value) = serde_json::to_value(task)
                {
                    payload.insert("taskTarget".to_owned(), task_value);
                }
            }
            TargetRef::Journal => {}
        },
        _ => {}
    }
}

/// Re-derive a content event's opaque-subject id from its (reference-remapped)
/// payload through the live builder, so the migrated id is the one a fresh
/// re-record mints today. Only the moving families reach here.
fn rederive_content_id(
    kind: ContentKind,
    payload: &Value,
    track_id: &TrackId,
    writer_actor_id: &str,
    assertion_mode: AssertionMode,
) -> Result<String> {
    let id = match kind {
        ContentKind::Observation => {
            let payload: ReviewObservationRecordedPayload =
                serde_json::from_value(payload.clone())?;
            build_observation_id(ObservationIdMaterial {
                track_id,
                target: &payload.target,
                title: &payload.title,
                body_content_hash: payload.body_content_hash.as_deref(),
                body_content_type: payload.body_content_type.identity_tag(),
                tags: &payload.tags,
                confidence: payload.confidence.as_deref(),
                supersedes_observation_ids: &payload.supersedes_observation_ids,
                responds_to_observation_ids: &payload.responds_to_observation_ids,
                writer_actor_id,
            })?
            .as_str()
            .to_owned()
        }
        ContentKind::Assessment => {
            let payload: ReviewAssessmentRecordedPayload = serde_json::from_value(payload.clone())?;
            build_assessment_id(AssessmentIdMaterial {
                track_id,
                target: &payload.target,
                assessment: payload.assessment,
                summary_content_hash: payload.summary_content_hash.as_deref(),
                summary_content_type: payload.summary_content_type.identity_tag(),
                replaces_assessment_ids: &payload.replaces_assessment_ids,
                related_observation_ids: &payload.related_observation_ids,
                related_input_request_ids: &payload.related_input_request_ids,
                writer_actor_id,
            })?
            .as_str()
            .to_owned()
        }
        ContentKind::Validation => {
            let payload: ValidationCheckRecordedPayload = serde_json::from_value(payload.clone())?;
            let ValidationTarget::Revision { revision_id } = &payload.target;
            build_validation_check_id(ValidationCheckIdMaterial {
                revision_id,
                track_id,
                check_name: &payload.check_name,
                command: payload.command.as_deref(),
                status: payload.status,
                exit_code: payload.exit_code,
                trigger: payload.trigger,
                source_fingerprint: payload.source_fingerprint.as_deref(),
                summary_content_hash: payload.summary_content_hash.as_deref(),
                summary_content_type: payload.summary_content_type.identity_tag(),
                started_at: payload.started_at.as_deref(),
                completed_at: payload.completed_at.as_deref(),
                log_artifact_content_hashes: &payload.log_artifact_content_hashes,
                writer_actor_id,
            })?
            .as_str()
            .to_owned()
        }
        ContentKind::InputRequestOpened => {
            let payload: InputRequestOpenedPayload = serde_json::from_value(payload.clone())?;
            build_input_request_id(InputRequestIdMaterial {
                track_id,
                target: &payload.target,
                assertion_mode,
                reason_code: payload.reason_code,
                title: &payload.title,
                body_content_hash: payload.body_content_hash.as_deref(),
                body_content_type: payload.body_content_type.identity_tag(),
                writer_actor_id,
            })?
            .as_str()
            .to_owned()
        }
        ContentKind::InputRequestResponded => {
            let payload: InputRequestRespondedPayload = serde_json::from_value(payload.clone())?;
            build_input_request_response_id(InputRequestResponseIdMaterial {
                input_request_id: &payload.input_request_id,
                outcome: payload.outcome,
                reason_content_hash: payload.reason_content_hash.as_deref(),
                reason_content_type: payload.reason_content_type.identity_tag(),
                writer_actor_id,
            })?
            .as_str()
            .to_owned()
        }
        // Association and withdrawal ids fold only stable material.
        ContentKind::CommitAssociated
        | ContentKind::RefAssociated
        | ContentKind::CommitWithdrawn
        | ContentKind::RefWithdrawn => {
            return Err(migrate_error(
                "association/withdrawal ids are stable and are not re-derived",
            ));
        }
    };
    Ok(id)
}

/// The content events whose identity this migrator tracks. The five moving
/// families fold the opaque subject and are re-derived; the associations and
/// withdrawals fold only stable material and ride through unchanged, but are
/// still recognized so the reference walk skips each one's own id field.
#[derive(Clone, Copy)]
enum ContentKind {
    Observation,
    Assessment,
    Validation,
    InputRequestOpened,
    InputRequestResponded,
    CommitAssociated,
    RefAssociated,
    CommitWithdrawn,
    RefWithdrawn,
}

impl ContentKind {
    fn from_event_type(event_type: EventType) -> Option<Self> {
        Some(match event_type {
            EventType::ReviewObservationRecorded => Self::Observation,
            EventType::ReviewAssessmentRecorded => Self::Assessment,
            EventType::ValidationCheckRecorded => Self::Validation,
            EventType::InputRequestOpened => Self::InputRequestOpened,
            EventType::InputRequestResponded => Self::InputRequestResponded,
            EventType::RevisionCommitAssociated => Self::CommitAssociated,
            EventType::RevisionRefAssociated => Self::RefAssociated,
            EventType::RevisionCommitWithdrawn => Self::CommitWithdrawn,
            EventType::RevisionRefWithdrawn => Self::RefWithdrawn,
            _ => return None,
        })
    }

    /// Whether this family's content id folds the opaque subject and therefore
    /// moves under the break (so it must be re-derived and re-keyed).
    fn is_moving(self) -> bool {
        matches!(
            self,
            Self::Observation
                | Self::Assessment
                | Self::Validation
                | Self::InputRequestOpened
                | Self::InputRequestResponded
        )
    }

    fn is_input_request(self) -> bool {
        matches!(self, Self::InputRequestOpened | Self::InputRequestResponded)
    }

    fn own_field(self) -> &'static str {
        match self {
            Self::Observation => "observationId",
            Self::Assessment => "assessmentId",
            Self::Validation => "validationCheckId",
            Self::InputRequestOpened => "inputRequestId",
            Self::InputRequestResponded => "inputRequestResponseId",
            Self::CommitAssociated => "commitAssociationId",
            Self::RefAssociated => "refAssociationId",
            Self::CommitWithdrawn => "commitWithdrawalId",
            Self::RefWithdrawn => "refWithdrawalId",
        }
    }
}

/// Whether every moving content id this event references that this store can
/// resolve has already been re-derived. A reference to a moving id this store
/// never produces (`produced`) is an unresolvable forward-pointer, not a pending
/// dependency, so it does not defer the event. A capture, association, or
/// content-id-free event references nothing and resolves immediately.
fn references_resolved(
    value: &Value,
    content_remap: &BTreeMap<String, String>,
    produced: &BTreeSet<String>,
) -> Result<bool> {
    let own_field = own_field_of(value)?;
    let mut resolved = true;
    let mut probe = value.clone();
    visit_reference_strings(&mut probe, own_field, &mut |string| {
        if is_moving_content_id(string)
            && produced.contains(string.as_str())
            && !content_remap.contains_key(string)
        {
            resolved = false;
        }
        Ok(())
    })?;
    Ok(resolved)
}

/// Remap every moving content-id reference this event folds through the old->new
/// map. A reference to an id this store produces but has not yet re-derived is a
/// dependency-order or cycle error and stops the migration (the fixpoint has
/// already deferred a resolvable event, so reaching here means a genuine cycle).
/// A reference to a moving id this store never produces is an advisory
/// forward-pointer (ADR-0026 `responds_to`) or other cross-store reference; it
/// has no re-derived form here and rides through verbatim, which keeps a fresh
/// re-record folding the same literal id convergent.
fn remap_references(
    value: &mut Value,
    content_remap: &BTreeMap<String, String>,
    produced: &BTreeSet<String>,
) -> Result<()> {
    let own_field = own_field_of(value)?;
    visit_reference_strings(value, own_field, &mut |string| {
        if is_moving_content_id(string) {
            match content_remap.get(string.as_str()) {
                Some(new) => *string = new.clone(),
                None if produced.contains(string.as_str()) => {
                    return Err(migrate_error(&format!(
                        "reference to content id {string} that has no re-derived form \
                         (dependency-order or cyclic reference)"
                    )));
                }
                None => {}
            }
        }
        Ok(())
    })
}

/// Every old moving content id an event in the source produces (each moving
/// content family's own-id field). A moving reference to an id in this set is a
/// genuine in-store dependency; a moving reference to an id absent from it points
/// at content this store never held and is preserved verbatim rather than
/// remapped or treated as a fixpoint dependency.
fn collect_produced_content_ids(raw: &[Value]) -> Result<BTreeSet<String>> {
    let mut produced = BTreeSet::new();
    for value in raw {
        let Some(kind) = ContentKind::from_event_type(event_type_of(value)?) else {
            continue;
        };
        if !kind.is_moving() {
            continue;
        }
        if let Some(id) = value["payload"][kind.own_field()].as_str() {
            produced.insert(id.to_owned());
        }
    }
    Ok(produced)
}

/// The event's own content-id field name, if it is a recognized content event.
/// The reference walk skips it: `inputRequestId` is the own id for an opened
/// request but a reference for a response, so scanning the own field as a
/// reference would demand a remap the event mints for itself.
fn own_field_of(value: &Value) -> Result<Option<&'static str>> {
    Ok(ContentKind::from_event_type(event_type_of(value)?).map(ContentKind::own_field))
}

/// Walk the structural id-bearing subtrees of an event — the envelope subject and
/// the payload reference fields except the event's own id — applying `visit` to
/// every string. These subtrees never hold free text, so a content-id
/// substitution there cannot corrupt a body/summary/title.
fn visit_reference_strings(
    value: &mut Value,
    own_field: Option<&str>,
    visit: &mut dyn FnMut(&mut String) -> Result<()>,
) -> Result<()> {
    if let Some(subject) = value.get_mut("target").and_then(|t| t.get_mut("subject")) {
        visit_strings(subject, visit)?;
    }
    if let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) {
        if let Some(target) = payload.get_mut("target") {
            visit_strings(target, visit)?;
        }
        for field in [
            "supersedesObservationIds",
            "respondsToObservationIds",
            "replacesAssessmentIds",
            "relatedObservationIds",
            "relatedInputRequestIds",
            "inputRequestId",
        ] {
            if Some(field) == own_field {
                continue;
            }
            if let Some(reference) = payload.get_mut(field) {
                visit_strings(reference, visit)?;
            }
        }
    }
    Ok(())
}

fn visit_strings(
    value: &mut Value,
    visit: &mut dyn FnMut(&mut String) -> Result<()>,
) -> Result<()> {
    match value {
        Value::String(string) => visit(string)?,
        Value::Array(items) => {
            for item in items {
                visit_strings(item, visit)?;
            }
        }
        Value::Object(object) => {
            for (_, child) in object {
                visit_strings(child, visit)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Whether a string is a content id that moves under this break (so a reference
/// to it must be remapped). The longer `input-request-response:` prefix is
/// distinct from `input-request:` (the byte after `input-request` is `-`, not
/// `:`), so the set is unambiguous. Association ids are deliberately absent: they
/// fold stable material and never move.
fn is_moving_content_id(string: &str) -> bool {
    const PREFIXES: [&str; 5] = [
        "obs:",
        "assess:",
        "input-request-response:",
        "input-request:",
        "validation:",
    ];
    PREFIXES.iter().any(|prefix| string.starts_with(prefix))
}

/// Re-order a fact-pointer list to sorted-unique after its ids were remapped, so
/// the stored payload folds the same canonical copy the id builder does and a
/// fresh re-record converges byte-for-byte instead of conflicting on payloadHash.
fn normalize_fact_lists(kind: ContentKind, payload: &mut Value) {
    let fields: &[&str] = match kind {
        ContentKind::Observation => &["supersedesObservationIds", "respondsToObservationIds"],
        ContentKind::Assessment => &[
            "replacesAssessmentIds",
            "relatedObservationIds",
            "relatedInputRequestIds",
        ],
        _ => &[],
    };
    for field in fields {
        sort_unique_string_array(payload, field);
    }
}

fn sort_unique_string_array(payload: &mut Value, field: &str) {
    if let Some(Value::Array(items)) = payload.get_mut(field) {
        let mut strings: Vec<String> = items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect();
        strings.sort();
        strings.dedup();
        *items = strings.into_iter().map(Value::String).collect();
    }
}

/// Re-key an event onto the type code: strip the legacy snake_case type prefix,
/// substitute every re-derived id the remainder embedded, then prepend the code.
/// An explicit dedupe key embeds no content id, so its remainder is untouched.
///
/// A `work_object_proposed` key is not a prefix swap: its material changed from
/// the revision/attempt id to the opaque subject id, so it is rebuilt directly
/// from the reshaped subject — the value a fresh capture mints today, so a
/// re-capture converges rather than forking.
fn rekey(
    legacy_key: &str,
    event_type: EventType,
    subject: &TargetRef,
    content_remap: &BTreeMap<String, String>,
) -> Result<String> {
    if event_type == EventType::WorkObjectProposed {
        let subject_id = subject_id(subject)?.ok_or_else(|| {
            migrate_error("work_object_proposed subject has no opaque subject id")
        })?;
        return Ok(format!("{}:{subject_id}", type_code(event_type)));
    }
    let legacy_prefix = format!("{}:", event_type.as_str());
    let rest = legacy_key.strip_prefix(&legacy_prefix).ok_or_else(|| {
        migrate_error(&format!(
            "idempotency key {legacy_key} does not start with the legacy type prefix {legacy_prefix}"
        ))
    })?;
    let mut rekeyed = rest.to_owned();
    for (old, new) in content_remap {
        if rekeyed.contains(old.as_str()) {
            rekeyed = rekeyed.replace(old.as_str(), new.as_str());
        }
    }
    Ok(format!("{}:{rekeyed}", type_code(event_type)))
}

// ---------------------------------------------------------------------------
// Signing
// ---------------------------------------------------------------------------

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

/// Re-home a detached co-signature. Its target's signature-exclusive record hash
/// re-keyed (every migrated target's serialized record moved), so the verbatim
/// passthrough branch is structurally dead here: a held-key attester is
/// re-attested over the reshaped target, and anything else is dropped and warned.
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

    // The verbatim passthrough branch (target id and record hash both unchanged)
    // is unreachable in this break — every migrated target re-keys, moving both —
    // so a carrier is re-attested with the attester's held key or dropped.
    let Some(signer) = held_signer(keystore, options, attester_did)? else {
        eprintln!(
            "warning: co-signature attester {attester_did} is not held; dropping the carrier"
        );
        summary.cosignatures_dropped += 1;
        return Ok(());
    };

    // Re-attest over the reshaped target: the attestation signs the target's
    // signer-inclusive TBS view (naming the attester), and the carrier binds the
    // target's signer-exclusive record hash. Both recompute against the new target.
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
        EventTarget::for_journal(new_target.target.journal_id.clone()),
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

// ---------------------------------------------------------------------------
// Self-check
// ---------------------------------------------------------------------------

/// Confirm no stale wire token survives in a migrated event: the envelope binds no
/// structural `target.subject` (retired for the opaque `subjectId`), and the
/// `eventType` is an opaque code, never a snake_case name. Structural wire
/// positions only — a free-text body may legitimately mention the words.
fn verify_no_stale_wire(events: &[ShoreEvent]) -> Result<()> {
    for event in events {
        let value = serde_json::to_value(event)?;
        if value
            .get("target")
            .and_then(|target| target.get("subject"))
            .is_some()
        {
            return Err(migrate_error(&format!(
                "migrated event {} still carries a structural target.subject",
                event.event_id.as_str()
            )));
        }
        match value.get("eventType").and_then(Value::as_str) {
            Some(code) if event_type_from_code(code).is_some() => {}
            other => {
                return Err(migrate_error(&format!(
                    "migrated event {} has a non-opaque eventType {other:?}",
                    event.event_id.as_str()
                )));
            }
        }
    }
    Ok(())
}

/// Every moving content event must be convergence-ready: the recorded id must
/// equal the id the live builder re-derives from its own stored payload, so a
/// fresh re-record reading the same payload dedups rather than forks. This catches
/// an inconsistent reference remap or idempotency-key rewrite.
fn verify_inline_convergence(events: &[ShoreEvent]) -> Result<()> {
    for event in events {
        let Some(kind) = ContentKind::from_event_type(event.event_type) else {
            continue;
        };
        if !kind.is_moving() {
            continue;
        }
        if kind.is_input_request() && event.addresses_task_subject()? {
            continue;
        }
        let track = event
            .target
            .track_id
            .clone()
            .ok_or_else(|| migrate_error("migrated content event is missing trackId"))?;
        let recomputed = rederive_content_id(
            kind,
            &event.payload,
            &track,
            event.writer.actor_id.as_str(),
            event.assertion_mode,
        )?;
        let stored = event.payload[kind.own_field()]
            .as_str()
            .ok_or_else(|| migrate_error("migrated content event is missing its id"))?;
        if recomputed != stored {
            return Err(migrate_error(&format!(
                "migrated {} id {stored} does not match its payload digest {recomputed}",
                kind.own_field()
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

fn write_manifest(options: &MigrateOptions, summary: &MigrateSummary) -> Result<()> {
    let manifest = json!({
        "kind": "opaque-identity-migration",
        "createdAt": current_timestamp(),
        "sourceStore": options.source_store_dir.display().to_string(),
        "targetStore": options.target_store_dir.display().to_string(),
        "summary": serde_json::to_value(summary)?,
    });
    let path = options.target_store_dir.join("migration-manifest.json");
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    std::fs::write(&path, bytes)
        .map_err(|error| migrate_error(&format!("write {}: {error}", path.display())))
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn subject_value(value: &Value) -> &Value {
    &value["target"]["subject"]
}

fn is_cosignature(value: &Value) -> bool {
    value["eventType"] == "event_signature_recorded"
}

fn event_type_of(value: &Value) -> Result<EventType> {
    serde_json::from_value(value["eventType"].clone())
        .map_err(|error| migrate_error(&format!("event has an unknown eventType: {error}")))
}

fn assertion_mode_of(value: &Value) -> AssertionMode {
    value
        .get("assertionMode")
        .and_then(|mode| serde_json::from_value::<AssertionMode>(mode.clone()).ok())
        .unwrap_or(AssertionMode::Advisory)
}

fn derive_event_id(idempotency_key: &str) -> crate::model::EventId {
    crate::model::EventId::new(format!(
        "evt:sha256:{}",
        sha256_bytes_hex(idempotency_key.as_bytes())
    ))
}

fn record_into(store: &EventStore, event: &ShoreEvent) -> Result<()> {
    store.record_event_once(event)?;
    Ok(())
}

fn event_id_of(value: &Value) -> Result<String> {
    value["eventId"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| migrate_error("event is missing eventId"))
}

fn occurred_at_str(value: &Value) -> &str {
    value["occurredAt"].as_str().unwrap_or("")
}

fn writer_of(value: &Value) -> Result<Writer> {
    Ok(serde_json::from_value(value["writer"].clone())?)
}

fn occurred_at_of(value: &Value) -> Result<String> {
    value["occurredAt"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| migrate_error("event is missing occurredAt"))
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
    ShoreError::Message(format!("opaque migrate: {message}"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::{Value, json};

    use super::*;
    use crate::keys::{KeyName, generate_key_in};

    /// A held keystore key: mint it under `keystore_dir` and return its did:key.
    fn held_key(keystore_dir: &Path) -> String {
        generate_key_in(keystore_dir, &KeyName::parse("tester").unwrap())
            .unwrap()
            .signer_id()
            .as_str()
            .to_owned()
    }

    /// Insert one pre-break, old-shape event by its logical idempotency key,
    /// bypassing the strict reader — the raw surface the migrator reads.
    fn seed(store_dir: &Path, value: &Value) {
        let key = value["idempotencyKey"].as_str().expect("idempotencyKey");
        StoreBackend::Local(store_dir.to_path_buf())
            .journal()
            .insert_raw(key, &serde_json::to_vec(value).unwrap())
            .unwrap();
    }

    fn signed(mut value: Value, signer_did: Option<&str>) -> Value {
        if let Some(did) = signer_did {
            value["signer"] = json!(did);
            value["signature"] = json!({ "alg": "ed25519", "sigVersion": 1, "sig": "AA==" });
        }
        value
    }

    /// A pre-break capture (`work_object_proposed`): snake_case eventType, a
    /// structural `target.subject`, and the legacy idempotency key.
    fn legacy_capture(revision_id: &str, object_id: &str, signer_did: Option<&str>) -> Value {
        let key = format!("work_object_proposed:{revision_id}");
        signed(
            json!({
                "schema": "shore.event",
                "version": 1,
                "eventId": derive_event_id(&key).as_str(),
                "eventType": "work_object_proposed",
                "idempotencyKey": key,
                "target": {
                    "journalId": "journal:default",
                    "subject": { "review": { "kind": "revision", "revisionId": revision_id } }
                },
                "writer": {
                    "actorId": "actor:agent:tester",
                    "producer": { "name": "shore", "version": "0.1.0" }
                },
                "occurredAt": "unix-ms:1",
                "payload": {
                    "engagementId": "engagement:sha256:e",
                    "workObject": {
                        "kind": "revision",
                        "revision": { "id": revision_id, "objectId": object_id },
                        "objectArtifactContentHash": "sha256:artifact"
                    }
                },
                "payloadHash": "sha256:placeholder"
            }),
            signer_did,
        )
    }

    /// A pre-break observation: snake_case eventType, a structural
    /// `target.subject`, and the legacy idempotency key. The content id is a frozen
    /// placeholder (a real pre-break store carries ids minted from the structural
    /// target, so they do not reproduce under the opaque-subject builder).
    fn legacy_observation(
        revision_id: &str,
        track: &str,
        observation_id: &str,
        title: &str,
        responds_to: &[&str],
        signer_did: Option<&str>,
    ) -> Value {
        let key = format!("review_observation_recorded:{revision_id}:{track}:{observation_id}");
        let mut payload = json!({
            "observationId": observation_id,
            "target": { "kind": "revision", "revisionId": revision_id },
            "title": title
        });
        if !responds_to.is_empty() {
            payload["respondsToObservationIds"] = json!(responds_to);
        }
        signed(
            json!({
                "schema": "shore.event",
                "version": 1,
                "eventId": derive_event_id(&key).as_str(),
                "eventType": "review_observation_recorded",
                "idempotencyKey": key,
                "target": {
                    "journalId": "journal:default",
                    "subject": { "review": { "kind": "revision", "revisionId": revision_id } },
                    "trackId": track
                },
                "writer": {
                    "actorId": "actor:agent:tester",
                    "producer": { "name": "shore", "version": "0.1.0" }
                },
                "occurredAt": "unix-ms:2",
                "payload": payload,
                "payloadHash": "sha256:placeholder"
            }),
            signer_did,
        )
    }

    /// A pre-break detached co-signature carrier targeting `target_event_id`.
    fn legacy_cosignature(target_event_id: &str, attester_did: &str) -> Value {
        let key = format!("event_signature_recorded:sha256:legacy:{attester_did}:SIG");
        json!({
            "schema": "shore.event",
            "version": 1,
            "eventId": derive_event_id(&key).as_str(),
            "eventType": "event_signature_recorded",
            "idempotencyKey": key,
            "target": { "journalId": "journal:default", "subject": "journal" },
            "writer": {
                "actorId": "actor:agent:tester",
                "producer": { "name": "shore", "version": "0.1.0" }
            },
            "occurredAt": "unix-ms:3",
            "payload": {
                "attestation": { "alg": "ed25519", "sigVersion": 1, "sig": "SIG" },
                "attestingSigner": attester_did,
                "targetEventId": target_event_id,
                "targetEventRecordHash": "sha256:legacy-record-hash"
            },
            "payloadHash": "sha256:placeholder"
        })
    }

    fn migrated_events(target_store_dir: &Path) -> Vec<ShoreEvent> {
        EventStore::from_backend(&StoreBackend::Local(target_store_dir.to_path_buf()))
            .list_events()
            .unwrap()
    }

    fn run(source: &Path, target: &Path, keystore: &Path) -> MigrateSummary {
        migrate_opaque_identity(MigrateOptions {
            source_store_dir: source.to_path_buf(),
            target_store_dir: target.to_path_buf(),
            keystore_dir: keystore.to_path_buf(),
        })
        .unwrap()
    }

    #[test]
    fn a_clean_migrate_lists_cleanly_and_carries_no_stale_wire_token() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let keystore = tempfile::tempdir().unwrap();
        let did = held_key(keystore.path());

        let revision_id = "rev:sha256:r";
        seed(
            source.path(),
            &legacy_capture(revision_id, "obj:sha256:o", Some(&did)),
        );
        seed(
            source.path(),
            &legacy_observation(
                revision_id,
                "agent:tester",
                "obs:sha256:frozen",
                "a finding",
                &[],
                Some(&did),
            ),
        );

        let summary = run(source.path(), target.path(), keystore.path());

        assert!(summary.self_check_passed, "the re-shaped store self-checks");
        assert_eq!(
            summary.content_ids_rederived, 1,
            "the observation re-derives"
        );
        assert_eq!(
            summary.inline_signatures_resigned, 2,
            "the capture and observation re-sign with the held key"
        );

        let events = migrated_events(target.path());
        assert_eq!(events.len(), 2);
        for event in &events {
            let value = serde_json::to_value(event).unwrap();
            assert!(
                value["target"].get("subject").is_none(),
                "no structural target.subject survives"
            );
            let code = value["eventType"].as_str().unwrap();
            assert!(
                event_type_from_code(code).is_some(),
                "eventType is an opaque code, got {code}"
            );
        }
    }

    #[test]
    fn a_reference_edge_remaps_to_the_new_content_id() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let keystore = tempfile::tempdir().unwrap();
        let did = held_key(keystore.path());

        let revision_id = "rev:sha256:r";
        seed(
            source.path(),
            &legacy_capture(revision_id, "obj:sha256:o", Some(&did)),
        );
        let anchor_old_id = "obs:sha256:anchor";
        seed(
            source.path(),
            &legacy_observation(
                revision_id,
                "agent:tester",
                anchor_old_id,
                "finding A",
                &[],
                Some(&did),
            ),
        );
        seed(
            source.path(),
            &legacy_observation(
                revision_id,
                "agent:tester",
                "obs:sha256:responder",
                "finding B",
                &[anchor_old_id],
                Some(&did),
            ),
        );

        run(source.path(), target.path(), keystore.path());

        let events = migrated_events(target.path());
        let anchor = events
            .iter()
            .find(|event| event.payload["title"] == "finding A")
            .expect("anchor observation");
        let responder = events
            .iter()
            .find(|event| event.payload["title"] == "finding B")
            .expect("responder observation");

        let anchor_new_id = anchor.payload["observationId"].as_str().unwrap();
        assert_ne!(anchor_new_id, anchor_old_id, "the anchor's id moved");

        let responds_to = responder.payload["respondsToObservationIds"]
            .as_array()
            .expect("responder carries a response link");
        assert_eq!(responds_to.len(), 1);
        assert_eq!(
            responds_to[0].as_str().unwrap(),
            anchor_new_id,
            "the response link points at the anchor's re-derived id, not the frozen one"
        );
    }

    #[test]
    fn a_dangling_forward_pointer_rides_through_and_does_not_stall() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let keystore = tempfile::tempdir().unwrap();
        let did = held_key(keystore.path());

        let revision_id = "rev:sha256:r";
        seed(
            source.path(),
            &legacy_capture(revision_id, "obj:sha256:o", Some(&did)),
        );
        // An advisory responds_to forward-pointer (ADR-0026) to an observation no
        // event in this store produces — a cross-store or otherwise unresolvable
        // pointer. It must not be treated as a fixpoint dependency (which would
        // stall the whole migration on a reference that can never resolve).
        let dangling = "obs:sha256:231e9f4144c6a5cb0000000000000000000000000000000000000000000000";
        seed(
            source.path(),
            &legacy_observation(
                revision_id,
                "agent:tester",
                "obs:sha256:responder",
                "acknowledgment",
                &[dangling],
                Some(&did),
            ),
        );

        let summary = run(source.path(), target.path(), keystore.path());
        assert!(
            summary.self_check_passed,
            "an unresolvable forward-pointer must not stall the migration"
        );

        let events = migrated_events(target.path());
        let responder = events
            .iter()
            .find(|event| event.payload["title"] == "acknowledgment")
            .expect("responder observation");

        // The responder's own id still moved onto the opaque subject ...
        assert_ne!(
            responder.payload["observationId"].as_str().unwrap(),
            "obs:sha256:responder",
            "the responder's own id re-derives"
        );
        // ... but the unresolvable forward-pointer rides through verbatim, since
        // there is no re-derived form in this store to remap it to.
        let responds_to = responder.payload["respondsToObservationIds"]
            .as_array()
            .expect("responder carries a response link");
        assert_eq!(responds_to.len(), 1);
        assert_eq!(
            responds_to[0].as_str().unwrap(),
            dangling,
            "a forward-pointer to content this store never held is preserved unchanged"
        );
    }

    #[test]
    fn a_held_key_cosignature_is_re_attested() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let keystore = tempfile::tempdir().unwrap();
        let did = held_key(keystore.path());

        let revision_id = "rev:sha256:r";
        seed(
            source.path(),
            &legacy_capture(revision_id, "obj:sha256:o", None),
        );
        let observation = legacy_observation(
            revision_id,
            "agent:tester",
            "obs:sha256:tgt",
            "target",
            &[],
            None,
        );
        let target_old_event_id = observation["eventId"].as_str().unwrap().to_owned();
        seed(source.path(), &observation);
        seed(
            source.path(),
            &legacy_cosignature(&target_old_event_id, &did),
        );

        let summary = run(source.path(), target.path(), keystore.path());

        assert_eq!(summary.cosignatures_reattested, 1);
        assert_eq!(summary.cosignatures_dropped, 0);
        assert!(summary.self_check_passed);

        let events = migrated_events(target.path());
        assert!(
            events
                .iter()
                .any(|event| event.event_type == EventType::EventSignatureRecorded),
            "the re-attested carrier is written to the target store"
        );
    }

    #[test]
    fn a_foreign_key_cosignature_is_dropped_with_a_warning() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let keystore = tempfile::tempdir().unwrap();
        // A held key exists, but the co-signature's attester is a different,
        // unheld did — so its carrier cannot be re-attested and is dropped.
        let _held = held_key(keystore.path());

        let revision_id = "rev:sha256:r";
        let observation = legacy_observation(
            revision_id,
            "agent:tester",
            "obs:sha256:tgt",
            "target",
            &[],
            None,
        );
        let target_old_event_id = observation["eventId"].as_str().unwrap().to_owned();
        seed(source.path(), &observation);
        seed(
            source.path(),
            &legacy_cosignature(
                &target_old_event_id,
                "did:key:z6MkForeignUnheldSignerKeyValue",
            ),
        );

        let summary = run(source.path(), target.path(), keystore.path());

        assert_eq!(summary.cosignatures_dropped, 1);
        assert_eq!(summary.cosignatures_reattested, 0);
        assert!(summary.self_check_passed);

        let events = migrated_events(target.path());
        assert!(
            events
                .iter()
                .all(|event| event.event_type != EventType::EventSignatureRecorded),
            "the dropped carrier is not written"
        );
    }

    #[test]
    fn the_manifest_is_a_local_side_file_not_a_journal_event() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let keystore = tempfile::tempdir().unwrap();
        let did = held_key(keystore.path());

        let revision_id = "rev:sha256:r";
        seed(
            source.path(),
            &legacy_capture(revision_id, "obj:sha256:o", Some(&did)),
        );
        seed(
            source.path(),
            &legacy_observation(
                revision_id,
                "agent:tester",
                "obs:sha256:frozen",
                "a finding",
                &[],
                Some(&did),
            ),
        );

        run(source.path(), target.path(), keystore.path());

        let manifest_path = target.path().join("migration-manifest.json");
        assert!(manifest_path.exists(), "the manifest side file is written");
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        assert_eq!(manifest["kind"], "opaque-identity-migration");
        assert_eq!(manifest["summary"]["selfCheckPassed"], true);
        assert_eq!(manifest["summary"]["eventsMigrated"], 2);
        assert_eq!(manifest["summary"]["contentIdsRederived"], 1);

        // The manifest is a side file at the store root, never a journal event:
        // the event log holds only the migrated capture and observation.
        let events = migrated_events(target.path());
        assert_eq!(events.len(), 2);
        assert!(
            !target
                .path()
                .join("events/migration-manifest.json")
                .exists()
        );
    }
}

/// The decisive convergence gate: a fact the migrated store already holds must
/// *deduplicate* on a fresh re-record through the live workflow, not fork. The
/// migrator self-check cannot catch a wrong id (ids are opaque to the read path),
/// so the live workflow re-recording to `Existing` is the only proof the migrated
/// id and key are the ones the builders mint today.
#[cfg(test)]
mod convergence_tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use serde_json::{Value, json};

    use super::*;
    use crate::model::{ObservationId, ValidationStatus, ValidationTrigger};
    use crate::session::event::{
        AssertionMode, InputRequestReasonCode, InputRequestResponseOutcome, ReviewAssessment,
    };
    use crate::session::{
        AssessmentAddOptions, AssociateCommitOptions, CaptureOptions, InputRequestOpenOptions,
        InputRequestRespondOptions, InputRequestTargetSelector, ObservationAddOptions,
        ValidationAddOptions, associate_commit, capture_worktree_review, open_input_request,
        record_assessment, record_observation, record_validation_check, respond_input_request,
    };

    /// Record one fact of every review family the migrator re-derives, so a loop
    /// over the store exercises each family's id derivation and convergence.
    fn record_every_family(repo: &TestRepo) {
        record_validation_check(
            ValidationAddOptions::new(repo.path())
                .with_track("agent:tester")
                .with_check_name("just test")
                .with_command("cargo test")
                .with_status(ValidationStatus::Passed)
                .with_trigger(ValidationTrigger::Manual),
        )
        .unwrap();
        let request = open_input_request(
            InputRequestOpenOptions::new(repo.path())
                .with_track("agent:tester")
                .with_title("a question")
                .with_body("which way?")
                .with_target(InputRequestTargetSelector::Revision)
                .with_assertion_mode(AssertionMode::Operative)
                .with_reason_code(InputRequestReasonCode::ManualDecisionRequired),
        )
        .unwrap();
        respond_input_request(
            InputRequestRespondOptions::new(repo.path(), request.input_request_id.clone())
                .with_outcome(InputRequestResponseOutcome::Approved)
                .with_reason("approved"),
        )
        .unwrap();
        record_assessment(
            AssessmentAddOptions::new(repo.path())
                .with_track("agent:tester")
                .with_assessment(ReviewAssessment::Accepted)
                .with_summary("looks good"),
        )
        .unwrap();
        associate_commit(
            AssociateCommitOptions::new(repo.path(), "HEAD").with_track("agent:tester"),
        )
        .unwrap();
    }

    struct TestRepo {
        root: tempfile::TempDir,
    }

    impl TestRepo {
        fn new() -> Self {
            let root = tempfile::tempdir().expect("temp git repo dir");
            let repo = Self { root };
            repo.git(["init"]);
            repo.git(["config", "user.name", "Shore Tests"]);
            repo.git(["config", "user.email", "shore-tests@example.com"]);
            repo.git(["config", "commit.gpgsign", "false"]);
            repo
        }
        fn path(&self) -> &Path {
            self.root.path()
        }
        fn store_dir(&self) -> PathBuf {
            crate::git::git_common_dir(self.path())
                .unwrap()
                .join("shore")
        }
        fn write(&self, path: &str, contents: &str) {
            let path = self.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        fn commit_all(&self, message: &str) {
            self.git(["add", "."]);
            self.git(["commit", "-m", message]);
        }
        fn git<I, S>(&self, args: I)
        where
            I: IntoIterator<Item = S>,
            S: AsRef<std::ffi::OsStr>,
        {
            let output = Command::new("git")
                .args(args)
                .current_dir(self.path())
                .output()
                .expect("run git");
            assert!(output.status.success(), "git failed");
        }
    }

    fn modified_repo() -> TestRepo {
        let repo = TestRepo::new();
        repo.write("src/lib.rs", "pub fn value() -> u32 {\n    1\n}\n");
        repo.commit_all("base");
        repo.write("src/lib.rs", "pub fn value() -> u32 {\n    2\n}\n");
        repo
    }

    fn observe(repo: &TestRepo, title: &str, responds_to: &[&ObservationId]) -> ObservationId {
        let mut options = ObservationAddOptions::new(repo.path())
            .with_track("agent:tester")
            .with_title(title);
        for id in responds_to {
            options = options.responding_to((*id).clone());
        }
        record_observation(options).unwrap().observation_id
    }

    /// A frozen, structurally-unrelated stand-in for a content id, so a pre-break
    /// store carries ids the opaque-subject builder does not reproduce and the
    /// migrator must genuinely re-derive them.
    fn frozen_id(id: &str) -> String {
        let prefix = id.split(":sha256:").next().unwrap();
        format!(
            "{prefix}:sha256:{}",
            sha256_bytes_hex(format!("legacy:{id}").as_bytes())
        )
    }

    /// Map every moving content id in the store to a frozen stand-in.
    fn freeze_map(events: &[Value]) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        for value in events {
            let Ok(event_type) = event_type_of(value) else {
                continue;
            };
            if let Some(kind) = ContentKind::from_event_type(event_type)
                && kind.is_moving()
                && let Some(id) = value["payload"][kind.own_field()].as_str()
            {
                map.insert(id.to_owned(), frozen_id(id));
            }
        }
        map
    }

    /// Reverse the opaque-coded reshape for one event, producing the pre-break
    /// shape the migrator reads: snake_case eventType, a structural `target.subject`
    /// reconstructed from the payload, the legacy idempotency key (the
    /// `work_object_proposed` key reverts to folding the revision id), and frozen
    /// content ids throughout. Hashes are re-derived so the raw event is coherent.
    fn downgrade(new_value: &Value, freeze: &BTreeMap<String, String>) -> Value {
        let event: ShoreEvent = serde_json::from_value(new_value.clone()).unwrap();
        let subject = event.reconstruct_subject().unwrap();
        let mut value = new_value.clone();

        value["eventType"] = json!(event.event_type.as_str());

        let mut target = serde_json::Map::new();
        target.insert(
            "journalId".to_owned(),
            new_value["target"]["journalId"].clone(),
        );
        target.insert(
            "subject".to_owned(),
            serde_json::to_value(&subject).unwrap(),
        );
        if let Some(track) = new_value["target"].get("trackId") {
            target.insert("trackId".to_owned(), track.clone());
        }
        value["target"] = Value::Object(target);

        let new_key = new_value["idempotencyKey"].as_str().unwrap();
        let legacy_key = if event.event_type == EventType::WorkObjectProposed {
            let revision_id = crate::model::subject_revision_id(&subject)
                .expect("a review capture folds a revision id");
            format!("work_object_proposed:{}", revision_id.as_str())
        } else {
            let code_prefix = format!("{}:", type_code(event.event_type));
            new_key
                .strip_prefix(&code_prefix)
                .map(|rest| format!("{}:{rest}", event.event_type.as_str()))
                .unwrap_or_else(|| new_key.to_owned())
        };
        value["idempotencyKey"] = json!(legacy_key);

        // Freeze every moving content id across the whole event — the own id, the
        // reference edges, and the (non-work-object-proposed) key that embeds it.
        let mut text = serde_json::to_string(&value).unwrap();
        for (new_id, frozen) in freeze {
            text = text.replace(new_id.as_str(), frozen.as_str());
        }
        let mut value: Value = serde_json::from_str(&text).unwrap();

        let key = value["idempotencyKey"].as_str().unwrap().to_owned();
        value["eventId"] = json!(derive_event_id(&key).as_str());
        value["payloadHash"] = json!(sha256_json_prefixed(&value["payload"]).unwrap());
        value
    }

    /// Downgrade the whole store into a fresh pre-break source store, migrate it
    /// back over the repo's store, and return the migration summary.
    fn downgrade_and_migrate(repo: &TestRepo) -> MigrateSummary {
        let store_dir = repo.store_dir();
        let events = read_raw_events(&store_dir).unwrap();
        let freeze = freeze_map(&events);

        let legacy = tempfile::tempdir().unwrap();
        let legacy_dir = legacy.path();
        let legacy_backend = StoreBackend::Local(legacy_dir.to_path_buf());
        for value in &events {
            let downgraded = downgrade(value, &freeze);
            let key = downgraded["idempotencyKey"].as_str().unwrap();
            legacy_backend
                .journal()
                .insert_raw(key, &serde_json::to_vec(&downgraded).unwrap())
                .unwrap();
        }
        // Artifacts are unchanged by the break: hand the migrator the same trees.
        copy_dir_verbatim(
            &store_dir.join("artifacts/objects"),
            &legacy_dir.join("artifacts/objects"),
        )
        .unwrap();
        copy_dir_verbatim(
            &store_dir.join("artifacts/notes"),
            &legacy_dir.join("artifacts/notes"),
        )
        .unwrap();

        std::fs::remove_dir_all(&store_dir).unwrap();
        let keystore = tempfile::tempdir().unwrap();
        migrate_opaque_identity(MigrateOptions {
            source_store_dir: legacy_dir.to_path_buf(),
            target_store_dir: store_dir,
            keystore_dir: keystore.path().to_path_buf(),
        })
        .unwrap()
    }

    fn count_event_type(store_dir: &Path, code: &str) -> usize {
        read_raw_events(store_dir)
            .unwrap()
            .iter()
            .filter(|value| value["eventType"] == code)
            .count()
    }

    #[test]
    fn a_migrated_review_store_converges_with_fresh_re_records() {
        let repo = modified_repo();
        capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        let observation_id = observe(&repo, "a finding", &[]);
        // Record every review family so the downgrade, migration, and self-check
        // exercise the full family set end to end, not just the observation.
        record_every_family(&repo);

        let summary = downgrade_and_migrate(&repo);
        assert!(summary.self_check_passed);

        // The migrated observation recovered the id the live builder mints.
        let migrated = read_raw_events(&repo.store_dir()).unwrap();
        let observation = migrated
            .iter()
            .find(|value| value["eventType"] == type_code(EventType::ReviewObservationRecorded))
            .unwrap();
        assert_eq!(
            observation["payload"]["observationId"],
            json!(observation_id.as_str()),
            "the migrator must recover the live-builder observation id"
        );

        // The decisive gate: a fresh re-record of the same fact deduplicates.
        let re_recorded = record_observation(
            ObservationAddOptions::new(repo.path())
                .with_track("agent:tester")
                .with_title("a finding"),
        )
        .unwrap();
        assert_eq!(
            re_recorded.events_created, 0,
            "a fresh re-record must converge with the migrated observation, not fork"
        );
        assert_eq!(re_recorded.observation_id, observation_id);

        // A fresh re-capture of the same revision also converges — the
        // work_object_proposed key folds the opaque subject id, so the migrated
        // capture is the one a fresh capture mints (a prefix-swap would fork here).
        capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        assert_eq!(
            count_event_type(&repo.store_dir(), type_code(EventType::WorkObjectProposed)),
            1,
            "a fresh re-capture must converge with the migrated capture, not fork"
        );
    }

    #[test]
    fn a_migrated_responds_to_observation_converges_reordered_and_duplicate() {
        let repo = modified_repo();
        capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        let a = observe(&repo, "finding a", &[]);
        let b = observe(&repo, "finding b", &[]);
        // An acknowledgment folding both fact pointers, exercising the reference
        // edge remap through migration.
        let ack = observe(&repo, "noted", &[&a, &b]);

        let summary = downgrade_and_migrate(&repo);
        assert!(summary.self_check_passed);

        // The remapped responds_to observation recovered its live id.
        let migrated = read_raw_events(&repo.store_dir()).unwrap();
        assert!(
            migrated.iter().any(|value| {
                value["eventType"] == type_code(EventType::ReviewObservationRecorded)
                    && value["payload"]["observationId"] == json!(ack.as_str())
            }),
            "the migrated acknowledgment must recover its live id"
        );

        // A reordered fact-pointer re-record deduplicates.
        let reordered = observe_result(&repo, "noted", &[&b, &a]);
        assert_eq!(
            reordered.events_created, 0,
            "a reordered responds_to re-record must converge, not fork or conflict"
        );
        assert_eq!(reordered.observation_id, ack);

        // A duplicate-bearing fact-pointer re-record deduplicates — proving the
        // migrator re-emitted the sorted_unique-normalized payload (dedup, not just
        // ordering).
        let duplicated = observe_result(&repo, "noted", &[&a, &a, &b]);
        assert_eq!(
            duplicated.events_created, 0,
            "a duplicate-bearing responds_to re-record must converge, not fork or conflict"
        );
        assert_eq!(duplicated.observation_id, ack);
    }

    fn observe_result(
        repo: &TestRepo,
        title: &str,
        responds_to: &[&ObservationId],
    ) -> crate::session::ObservationAddResult {
        let mut options = ObservationAddOptions::new(repo.path())
            .with_track("agent:tester")
            .with_title(title);
        for id in responds_to {
            options = options.responding_to((*id).clone());
        }
        record_observation(options).unwrap()
    }

    #[test]
    fn migrator_id_derivation_pins_to_live_builders() {
        // Every content id the migrator recomputes from a live-recorded payload
        // must equal the id the live builder stored — the migrator and a native
        // write cannot drift, or a migrated store would silently fork.
        let repo = modified_repo();
        capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        observe(&repo, "a finding", &[]);
        record_every_family(&repo);

        let mut families_checked = 0;
        for value in read_raw_events(&repo.store_dir()).unwrap() {
            // The migrated store is opaque-coded, so decode the event through the
            // type-code adapter, then read its family off the decoded event.
            let event: ShoreEvent = serde_json::from_value(value.clone()).unwrap();
            let Some(kind) = ContentKind::from_event_type(event.event_type) else {
                continue;
            };
            if !kind.is_moving() {
                continue;
            }
            if kind.is_input_request() && event.addresses_task_subject().unwrap() {
                continue;
            }
            let track = event.target.track_id.clone().unwrap();
            let recomputed = rederive_content_id(
                kind,
                &event.payload,
                &track,
                event.writer.actor_id.as_str(),
                event.assertion_mode,
            )
            .unwrap();
            let stored = event.payload[kind.own_field()].as_str().unwrap();
            assert_eq!(
                recomputed,
                stored,
                "the migrator's id derivation drifted from the live builder for {}",
                kind.own_field()
            );
            families_checked += 1;
        }
        // Observation, validation, input-request-opened, input-request-responded,
        // and assessment all fold the opaque subject and must be pinned.
        assert_eq!(
            families_checked, 5,
            "every moving review family must be pinned to its live builder"
        );
    }
}
