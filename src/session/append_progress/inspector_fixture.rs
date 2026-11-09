//! Test-only input construction. This helper never appends authority or opens SQL.
use std::io::{Read as _, Write as _};
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::canonical_hash::sha256_bytes_hex;
use crate::crypto::{EventSigner, EventVerificationStatus};
use crate::model::*;
use crate::session::event::*;
use crate::session::signing::{CosignatureVerification, EventSigningOptions, TrustSet};
use crate::session::store::body_artifact::{
    BodyArtifactOutcome, stage_body_artifact, validate_note_body_artifact_bytes,
};
use crate::session::store::object_artifact::{
    build_object_artifact_v2, decode_and_validate_object_artifact,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    version: u32,
    parent: PathBuf,
    change_id: ChangeId,
    key: PathBuf,
}

fn artifact(path: String, bytes: Vec<u8>) -> Value {
    json!({"path":path,"sha256":sha256_bytes_hex(&bytes),"bytes":bytes})
}

fn note(body: &str) -> (String, String, u64, Value) {
    let BodyArtifactOutcome::Artifact {
        relative_path,
        byte_size,
        body_envelope,
    } = stage_body_artifact(body.as_bytes()).unwrap()
    else {
        panic!("external body required")
    };
    let hash = format!("sha256:{}", sha256_bytes_hex(body.as_bytes()));
    let bytes = body_envelope.to_json_bytes().unwrap();
    assert_eq!(
        validate_note_body_artifact_bytes(&relative_path, &hash, &bytes)
            .unwrap()
            .body,
        body
    );
    (
        relative_path.clone(),
        hash,
        byte_size,
        artifact(relative_path, bytes),
    )
}

fn signed<P: EventPayload>(
    key: &std::path::Path,
    target: EventTarget,
    idempotency: String,
    payload: P,
    ordinal: usize,
) -> ShoreEvent {
    let mut event = ShoreEvent::new(
        payload.event_type(),
        idempotency,
        target,
        Writer {
            actor_id: ActorId::new("actor:consumer-test"),
            producer: WriterProducer {
                name: "pointbreak-inspector-fixture".into(),
                version: "1".into(),
            },
        },
        payload,
        format!("2026-10-08T00:00:{ordinal:02}Z"),
    )
    .unwrap();
    let signer = crate::keys::load_signer_from_path(key).unwrap();
    crate::session::sign_event_if_requested(&mut event, &EventSigningOptions::sign_with(signer))
        .unwrap();
    assert_eq!(
        crate::session::verify_event_signature(&event, &TrustSet::default()).unwrap(),
        EventVerificationStatus::UntrustedKey
    );
    crate::session::EventStore::prepare_owned_progress_event(event.clone()).unwrap();
    event
}

fn cosign(key: &std::path::Path, target: &ShoreEvent, ordinal: usize) -> ShoreEvent {
    let signer = crate::keys::load_signer_from_path(key).unwrap();
    let tbs = EventToBeSigned::from_event(target, signer.signer_id()).unwrap();
    let message = event_signature_pre_authentication_encoding(&tbs).unwrap();
    let payload = EventSignatureRecordedPayload {
        target_event_id: target.event_id.clone(),
        target_event_record_hash: target.event_record_hash().unwrap(),
        attesting_signer: signer.signer_id().clone(),
        attestation: EventSignature::ed25519_v1(signer.sign_event_message(&message).unwrap()),
        inclusion_proof: None,
    };
    assert_eq!(
        crate::session::verify_cosignature(&payload, target, &TrustSet::default()).unwrap(),
        CosignatureVerification::Attested(EventVerificationStatus::UntrustedKey)
    );
    let idempotency = EventSignatureRecordedPayload::idempotency_key(
        &payload.target_event_record_hash,
        &payload.attesting_signer,
        payload.attestation.sig.as_str(),
    );
    signed(
        key,
        EventTarget::for_journal(target.target.journal_id.clone()),
        idempotency,
        payload,
        ordinal,
    )
}

#[test]
#[ignore = "fixture construction only after exact composed caller review and runtime admission"]
fn prepare_owned_inspector_seed() {
    let path = PathBuf::from(
        std::env::var_os("POINTBREAK_INSPECTOR_SEED_REQUEST").expect("assigned seed input"),
    );
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .unwrap()
        .take(16_385)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= 16_384);
    assert_eq!(
        sha256_bytes_hex(&bytes),
        std::env::var("POINTBREAK_INSPECTOR_SEED_REQUEST_SHA256").unwrap()
    );
    let input: Request = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(input.version, 1);
    assert!(input.parent.is_absolute());
    assert_eq!(std::fs::canonicalize(&input.parent).unwrap(), input.parent);
    assert_eq!(path, input.parent.join("seed-request.json"));
    assert_eq!(
        std::fs::read(input.parent.join("fixture-owner")).unwrap(),
        b"owned-inspector-fixture-v1\n"
    );
    assert_eq!(std::fs::canonicalize(&input.key).unwrap(), input.key);
    assert_eq!(input.key.parent().unwrap(), input.parent.join("home/keys"));
    let repo = input.parent.join("repo");
    assert_eq!(std::fs::canonicalize(&repo).unwrap(), repo);
    assert_eq!(
        std::fs::read(repo.join("fixture.txt")).unwrap(),
        b"after: exact captured fixture\n"
    );
    let files = crate::git::capture_worktree_diff_files(&repo).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].new_path.as_deref(), Some("fixture.txt"));
    assert!(!files[0].hunks.is_empty());
    let fingerprint =
        crate::session::store::fingerprint::revision_fingerprint_for_files(&repo, &files, &[])
            .unwrap();
    let object = build_object_artifact_v2(DiffSnapshot::new(
        ReviewId::new("review:owned-inspector-fixture"),
        fingerprint.object_id.clone(),
        files,
    ))
    .unwrap();
    let object_bytes = serde_json::to_vec(&object).unwrap();
    assert_eq!(
        decode_and_validate_object_artifact(&object_bytes).unwrap(),
        object
    );
    let mut artifacts = vec![artifact(
        format!("artifacts/objects/{}.json", &object.content_hash[7..]),
        object_bytes,
    )];
    let revision = fingerprint.revision_id.clone();
    let journal = JournalId::new("journal:default");
    let track = TrackId::new("agent:inspector-fixture");
    let target =
        EventTarget::for_revision(journal.clone(), revision.clone(), Some(track.clone())).unwrap();
    let proposal = signed(
        &input.key,
        EventTarget::for_revision(journal.clone(), revision.clone(), None).unwrap(),
        format!("owned-inspector:proposal:{}", revision.as_str()),
        WorkObjectProposedPayload {
            engagement_id: fingerprint.engagement_id.clone(),
            work_object: WorkObjectProposal::Revision {
                revision: Revision {
                    id: revision.clone(),
                    object_id: object.snapshot.object_id.clone(),
                    git_provenance: Some(fingerprint.git_provenance()),
                },
                summary: Some("exact captured fixture".into()),
                object_artifact_content_hash: object.content_hash.clone(),
                supersedes: vec![],
            },
        },
        1,
    );
    let membership = build_membership_asserted(&input.change_id, &revision, [0x73; 32]).unwrap();
    membership.validate().unwrap();
    let membership = signed(
        &input.key,
        EventTarget::for_journal(journal.clone()),
        format!(
            "owned-inspector:membership:{}",
            membership.membership_claim_id.as_str()
        ),
        membership,
        2,
    );
    let body = "Selected observation body.\n".repeat(180);
    let (body_path, body_hash, body_size, body_artifact) = note(&body);
    artifacts.push(body_artifact);
    let review_target = ReviewTargetRef::Revision {
        revision_id: revision.clone(),
    };
    let observation_id = crate::session::workflow::observation::add::build_observation_id(
        crate::session::workflow::observation::add::ObservationIdMaterial {
            track_id: &track,
            target: &review_target,
            title: "selected external observation",
            body_content_hash: Some(&body_hash),
            body_content_type: None,
            tags: &[],
            confidence: None,
            supersedes_observation_ids: &[],
            responds_to_observation_ids: &[],
            writer_actor_id: "actor:consumer-test",
        },
    )
    .unwrap();
    let observation = signed(
        &input.key,
        target.clone(),
        ReviewObservationRecordedPayload::idempotency_key(
            &revision,
            &track,
            observation_id.as_str(),
        ),
        ReviewObservationRecordedPayload {
            observation_id: observation_id.clone(),
            target: review_target,
            title: "selected external observation".into(),
            body: None,
            body_content_type: Default::default(),
            body_artifact_path: Some(body_path),
            body_byte_size: Some(body_size),
            body_content_hash: Some(body_hash),
            tags: vec![],
            confidence: None,
            supersedes_observation_ids: vec![],
            responds_to_observation_ids: vec![],
        },
        3,
    );
    let signature = cosign(&input.key, &observation, 4);
    let (_, removed_hash, _, removed_log) =
        note(&"Removal-target validation log fixture.\n".repeat(180));
    let (_, retained_hash, _, retained_log) =
        note(&"Retained validation log fixture.\n".repeat(180));
    artifacts.extend([removed_log, retained_log]);
    let mut logs = vec![removed_hash.clone(), retained_hash];
    logs.sort();
    let validation_id = crate::session::workflow::validation::add::build_validation_check_id(
        crate::session::workflow::validation::add::ValidationCheckIdMaterial {
            revision_id: &revision,
            track_id: &track,
            check_name: "fixture validation",
            command: None,
            status: ValidationStatus::Passed,
            exit_code: Some(0),
            trigger: ValidationTrigger::Manual,
            source_fingerprint: None,
            summary_content_hash: None,
            summary_content_type: None,
            started_at: None,
            completed_at: None,
            log_artifact_content_hashes: &logs,
            writer_actor_id: "actor:consumer-test",
        },
    )
    .unwrap();
    let validation = signed(
        &input.key,
        target,
        ValidationCheckRecordedPayload::idempotency_key(&revision, &track, validation_id.as_str()),
        ValidationCheckRecordedPayload {
            validation_check_id: validation_id.clone(),
            target: ValidationTarget::Revision {
                revision_id: revision.clone(),
            },
            check_name: "fixture validation".into(),
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
            log_artifact_content_hashes: logs,
        },
        5,
    );
    let removal = signed(
        &input.key,
        EventTarget::for_journal(journal),
        ArtifactRemovedPayload::idempotency_key(&removed_hash),
        ArtifactRemovedPayload {
            content_hash: removed_hash.clone(),
        },
        6,
    );
    let removal_signature = cosign(&input.key, &removal, 7);
    let resource = crate::documents::RevisionResourceDocumentV1::available(
        crate::documents::RevisionResourceRefV1 {
            revision: RevisionRefV1::new(revision.clone(), object.content_hash.clone()).unwrap(),
            object_id: object.snapshot.object_id.clone(),
        },
        crate::documents::RevisionResourceProjectionV1 {
            track_id: None,
            include_body: true,
        },
        &object.snapshot,
    )
    .unwrap();
    resource.validate_integrity().unwrap();
    let plan = json!({"version":1,"changeId":input.change_id,"revisionId":revision,"artifactHash":object.content_hash,
        "observationId":observation_id,"validationId":validation_id,"observationBody":body,"removedLogHash":removed_hash,
        "artifacts":artifacts,"events":[proposal,membership,observation,signature,validation],
        "removalEvents":[removal,removal_signature],"removedContentRebuild":"unsupported; separate negative control only; audit refusal unchanged","expectedResource":resource});
    let bytes = serde_json::to_vec(&plan).unwrap();
    assert!(
        bytes.len() < 262_144,
        "seed JSON is {} bytes; limit is strictly below 262144",
        bytes.len()
    );
    let output = input.parent.join("seed.json");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    println!(
        "fixture seed {} sha256:{}",
        output.display(),
        sha256_bytes_hex(&bytes)
    );
}
