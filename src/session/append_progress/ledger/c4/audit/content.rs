use super::*;
use crate::session::event::{
    EventSignatureRecordedPayload, EventType, RevisionRelationAttestedPayload, WorkObjectProposal,
    WorkObjectProposedPayload,
};
use crate::session::evidence::RelationProofManifestV1;
use crate::session::store::body_artifact::{
    body_artifact_field, validate_note_body_artifact_bytes,
};
use crate::session::store::object_artifact::decode_and_validate_object_artifact;

pub(super) fn validate_resource(
    family: &str,
    stem: &str,
    relative: &str,
    bytes: &[u8],
) -> Result<()> {
    budget::check()?;
    let hash = format!("sha256:{stem}");
    match family {
        "objects" => {
            let object = decode_and_validate_object_artifact(bytes)?;
            if object.schema != "shore.object"
                || !object.content_encoding.is_empty()
                || object.content_hash != hash
            {
                return Err(fail("object resource binding"));
            }
        }
        "notes" => {
            let body = validate_note_body_artifact_bytes(relative, &hash, bytes)?;
            if !body.content_encoding.is_empty() {
                return Err(fail("unsupported body encoding"));
            }
        }
        "proofs" => {
            let proof: RelationProofManifestV1 = serde_json::from_slice(bytes)?;
            proof.validate()?;
            if proof.evidence_sha256 != hash {
                return Err(fail("proof resource binding"));
            }
        }
        _ => return Err(fail("unknown resource family")),
    }
    budget::check()
}
fn required<'a>(inventory: &'a Inventory, path: &str) -> Result<&'a [u8]> {
    budget::visit()?;
    inventory
        .content
        .get(path)
        .map(Vec::as_slice)
        .ok_or_else(|| fail("missing required local content"))
}
pub(super) fn validate_events(
    inventory: &Inventory,
    entries: Vec<crate::session::store::backend::JournalEntry>,
) -> Result<()> {
    let mut events = Vec::new();
    let mut records = BTreeMap::new();
    for entry in entries {
        budget::check()?;
        let event = EventStore::decode_qualification_entry(entry.key_digest, entry.bytes)?;
        validate_payload(&event)?;
        let hash = event.event_record_hash()?;
        if records.insert(hash, events.len()).is_some() {
            return Err(fail("duplicate event record"));
        }
        events.push(event);
    }
    for event in &events {
        budget::check()?;
        if event.event_type == EventType::EventSignatureRecorded {
            use crate::crypto::EventVerificationStatus;
            use crate::session::signing::{CosignatureVerification, TrustSet, verify_cosignature};
            let carrier: EventSignatureRecordedPayload =
                serde_json::from_value(event.payload.clone())?;
            budget::visit()?;
            let target = records
                .get(&carrier.target_event_record_hash)
                .map(|i| &events[*i])
                .ok_or_else(|| fail("detached signature target missing"))?;
            if carrier.target_event_id != target.event_id
                || event.idempotency_key
                    != EventSignatureRecordedPayload::idempotency_key(
                        &carrier.target_event_record_hash,
                        &carrier.attesting_signer,
                        carrier.attestation.sig.as_str(),
                    )
            {
                return Err(fail("detached carrier target/key binding"));
            }
            if !matches!(
                verify_cosignature(&carrier, target, &TrustSet::default())?,
                CosignatureVerification::Attested(
                    EventVerificationStatus::Valid | EventVerificationStatus::UntrustedKey
                )
            ) {
                return Err(fail("invalid detached signature"));
            }
        } else {
            // Same private signed-event integrity contract as C4; no trust policy.
            super::super::verify_signature(event)?;
        }
        if event.event_type == EventType::WorkObjectProposed {
            let payload: WorkObjectProposedPayload = serde_json::from_value(event.payload.clone())?;
            if let WorkObjectProposal::Revision {
                revision,
                object_artifact_content_hash,
                ..
            } = payload.work_object
            {
                let stem = prefixed(&object_artifact_content_hash)?;
                let bytes = required(inventory, &format!("artifacts/objects/{stem}.json"))?;
                let object = decode_and_validate_object_artifact(bytes)?;
                if object.content_hash != object_artifact_content_hash
                    || object.snapshot.object_id != revision.object_id
                {
                    return Err(fail("required object binding mismatch"));
                }
            }
        }
        validate_body(inventory, event)?;
        if event.event_type == EventType::RevisionRelationAttested {
            let attestation: RevisionRelationAttestedPayload =
                serde_json::from_value(event.payload.clone())?;
            if let Some(hash) = &attestation.evidence_content_hash {
                let stem = prefixed(hash)?;
                let proof: RelationProofManifestV1 = serde_json::from_slice(required(
                    inventory,
                    &format!("artifacts/proofs/{stem}.json"),
                )?)?;
                proof.validate()?;
                if proof.evidence_sha256 != *hash
                    || proof.revision != attestation.revision
                    || proof.association_id != attestation.commit_association_id
                    || proof.result.semantic_relation != attestation.semantic_relation
                    || proof.result.proof_status != attestation.proof_status
                    || proof.algorithm_version != attestation.proof_algorithm_version
                    || proof.result_digest()? != attestation.result_digest
                {
                    return Err(fail("required relation proof binding"));
                }
            }
        }
        budget::check()?;
    }
    Ok(())
}
/// Extract from the registered concrete payload, then bind the entire body
/// tuple. Legacy inline payloads may omit size; external content must name it.
fn validate_body(inventory: &Inventory, event: &ShoreEvent) -> Result<()> {
    use crate::session::event::*;
    if body_artifact_field(event.event_type).is_none() {
        return Ok(());
    }
    macro_rules! fields {
        ($ty:ty, $body:ident, $path:ident, $hash:ident, $size:ident) => {{
            let p: $ty = serde_json::from_value(event.payload.clone())?;
            (p.$body, p.$path, p.$hash, p.$size)
        }};
    }
    let tuple = match event.event_type {
        EventType::ReviewObservationRecorded => fields!(
            ReviewObservationRecordedPayload,
            body,
            body_artifact_path,
            body_content_hash,
            body_byte_size
        ),
        EventType::InputRequestOpened => fields!(
            InputRequestOpenedPayload,
            body,
            body_artifact_path,
            body_content_hash,
            body_byte_size
        ),
        EventType::TaskObservationRecorded => fields!(
            TaskObservationRecordedPayload,
            body,
            body_artifact_path,
            body_content_hash,
            body_byte_size
        ),
        EventType::InputRequestResponded => fields!(
            InputRequestRespondedPayload,
            reason,
            reason_artifact_path,
            reason_content_hash,
            reason_byte_size
        ),
        EventType::ReviewAssessmentRecorded => fields!(
            ReviewAssessmentRecordedPayload,
            summary,
            summary_artifact_path,
            summary_content_hash,
            summary_byte_size
        ),
        EventType::ValidationCheckRecorded => fields!(
            ValidationCheckRecordedPayload,
            summary,
            summary_artifact_path,
            summary_content_hash,
            summary_byte_size
        ),
        _ => return Err(fail("unregistered audit body payload")),
    };
    match tuple {
        (None, None, None, None) => Ok(()),
        (Some(body), None, None, size) if size.is_none_or(|n| n == body.len() as u64) => Ok(()),
        (None, Some(path), Some(hash), Some(size)) => {
            let body =
                validate_note_body_artifact_bytes(&path, &hash, required(inventory, &path)?)?;
            if !body.content_encoding.is_empty() || body.body.len() as u64 != size {
                return Err(fail("required body size or encoding mismatch"));
            }
            Ok(())
        }
        _ => Err(fail("invalid inline/external body tuple")),
    }
}

fn prefixed(hash: &str) -> Result<&str> {
    hash.strip_prefix("sha256:")
        .filter(|stem| hash64(stem))
        .ok_or_else(|| fail("invalid content hash"))
}

// Exhaustive typed decoding is separate from EventStore's envelope/hash check.
// A new registered family must explicitly declare its audit payload contract.
fn validate_payload(event: &ShoreEvent) -> Result<()> {
    use crate::session::event::*;
    fn decode<T: serde::de::DeserializeOwned>(event: &ShoreEvent) -> Result<T> {
        serde_json::from_value(event.payload.clone()).map_err(Into::into)
    }
    if event.payload_version != 1 || !event.content_encoding.is_empty() {
        return Err(fail("unsupported event interpretation or encoding"));
    }
    match event.event_type {
        EventType::ReviewInitialized => {
            decode::<ReviewInitializedPayload>(event)?;
        }
        EventType::WorkObjectProposed => {
            decode::<WorkObjectProposedPayload>(event)?;
        }
        EventType::ReviewObservationRecorded => {
            decode::<ReviewObservationRecordedPayload>(event)?;
        }
        EventType::ReviewAssessmentRecorded => {
            decode::<ReviewAssessmentRecordedPayload>(event)?;
        }
        EventType::InputRequestOpened => {
            decode_input_request_opened_payload(event.payload.clone())?;
        }
        EventType::InputRequestResponded => {
            decode::<InputRequestRespondedPayload>(event)?;
        }
        EventType::ReviewNoteImported => return Err(fail("retired review-note payload")),
        EventType::RevisionRefAssociated => {
            decode::<RevisionRefAssociatedPayload>(event)?;
        }
        EventType::RevisionRefWithdrawn => {
            decode::<RevisionRefWithdrawnPayload>(event)?;
        }
        EventType::RevisionCommitAssociated => {
            decode::<RevisionCommitAssociatedPayload>(event)?;
        }
        EventType::RevisionCommitWithdrawn => {
            decode::<RevisionCommitWithdrawnPayload>(event)?;
        }
        EventType::ValidationCheckRecorded => {
            decode::<ValidationCheckRecordedPayload>(event)?;
        }
        EventType::TaskCheckpointCaptured => {
            decode::<TaskCheckpointCapturedPayload>(event)?;
        }
        EventType::TaskObservationRecorded => {
            decode::<TaskObservationRecordedPayload>(event)?;
        }
        EventType::EventSignatureRecorded => {
            decode::<EventSignatureRecordedPayload>(event)?;
        }
        EventType::ArtifactRemoved => return Err(fail("content removal is not audit repair")),
        EventType::ChangeDeclared => decode::<ChangeDeclaredPayload>(event)?.validate()?,
        EventType::ChangeMembershipAsserted => {
            decode::<ChangeMembershipAssertedPayload>(event)?.validate()?
        }
        EventType::ChangeMembershipWithdrawn => {
            decode::<ChangeMembershipWithdrawnPayload>(event)?.validate()?
        }
        EventType::ChangeLinkAsserted => decode::<ChangeLinkAssertedPayload>(event)?.validate()?,
        EventType::ChangeRevisionRelationAsserted => {
            decode::<ChangeRevisionRelationAssertedPayload>(event)?.validate()?
        }
        EventType::ChangeRevisionRelationWithdrawn => {
            decode::<ChangeRevisionRelationWithdrawnPayload>(event)?.validate()?
        }
        EventType::RevisionRelationAttested => {
            decode::<RevisionRelationAttestedPayload>(event)?.validate()?
        }
        EventType::ReviewFactPorted => {
            let track = event
                .target
                .track_id
                .as_ref()
                .ok_or_else(|| fail("fact port has no attribution track"))?;
            decode::<ReviewFactPortedPayload>(event)?
                .validate_attribution(&event.writer.actor_id, track)?;
        }
    }
    Ok(())
}
