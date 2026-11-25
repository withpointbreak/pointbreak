use std::fs;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use super::super::tests as c4tests;
use super::*;
use crate::crypto::{EventSigner, TestEd25519Signer};
use crate::session::event::*;
use crate::session::store::backend::JournalEntry;
const REQUEST: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const AUDIT: &str = "9999999999999999999999999999999999999999999999999999999999999999";
const ACTOR: &str = "actor:local";

fn obligation(state: &State) -> Obligation {
    Obligation {
        version: 1,
        binding: state.binding.clone(),
        minimum_reader_profile: "review_change_revision_progress_v1".into(),
        attempt_id: "b".repeat(64),
        key: "c6:pending".into(),
        expected_sha256: "c".repeat(64),
        expected_length: 71,
        next_head: state.head + 1,
        previous_head: state.head,
        previous_prefix: state.prefix.clone(),
        previous_native: state.native.clone(),
    }
}
fn intent() -> State {
    let mut state = c4tests::pure_state();
    state.obligation = Some(obligation(&state));
    state.phase = Phase::Intent;
    state
}
fn census_proof(state: &State, present: bool) -> Proof {
    let o = state.obligation().unwrap();
    let census = CensusBinding {
        version: 1,
        audit_request: AUDIT.into(),
        obligation_sha256: sha256_json_hex(&serde_json::to_value(o).unwrap()).unwrap(),
        previous_head: state.head,
        previous_prefix: state.prefix.clone(),
        native: state.native.clone(),
        inventory_sha256: "d".repeat(64),
        totals: budget::Totals {
            visits: 10,
            bytes: 100,
        },
    };
    if present {
        Proof::CensusPresent {
            census,
            stored_sha256: o.expected_sha256.clone(),
            stored_length: o.expected_length,
        }
    } else {
        Proof::CensusAbsent { census }
    }
}
#[test]
fn audit_p01_absent_proof_roundtrip_binds_original_obligation() {
    let state = intent();
    let proof = census_proof(&state, false);
    let encoded = text(&serde_json::to_value(&proof).unwrap()).unwrap();
    let decoded: Proof = closed(&encoded).unwrap();
    assert_eq!(decoded, proof);
    assert!(!decoded.present());
    decoded.validate(state.obligation().unwrap()).unwrap();
}
#[test]
fn audit_p02_present_proof_requires_exact_stored_witness() {
    let state = intent();
    let mut proof = census_proof(&state, true);
    proof.validate(state.obligation().unwrap()).unwrap();
    if let Proof::CensusPresent { stored_length, .. } = &mut proof {
        *stored_length += 1;
    }
    assert!(proof.validate(state.obligation().unwrap()).is_err());
}
#[test]
fn audit_p03_unknown_census_version_and_fields_refuse() {
    let state = intent();
    let mut value = serde_json::to_value(census_proof(&state, false)).unwrap();
    value["census"]["version"] = json!(2);
    let proof: Proof = closed(&text(&value).unwrap()).unwrap();
    assert!(proof.validate(state.obligation().unwrap()).is_err());
    value["census"]["version"] = json!(1);
    value["census"]["fallback"] = json!(true);
    assert!(closed::<Proof>(&text(&value).unwrap()).is_err());
}
#[test]
fn audit_p04_binding_changes_cannot_port_census_proof() {
    let state = intent();
    let proof = census_proof(&state, false);
    for field in [
        "attemptId",
        "key",
        "expectedSha256",
        "previousPrefix",
        "binding",
    ] {
        let mut value = serde_json::to_value(state.obligation().unwrap()).unwrap();
        value[field] = if field == "binding" {
            let mut binding = value[field].clone();
            binding["epoch"] = json!("7".repeat(64));
            binding
        } else {
            json!("7".repeat(64))
        };
        let changed: Obligation = serde_json::from_value(value).unwrap();
        assert!(proof.validate(&changed).is_err(), "{field}");
    }
}
#[test]
fn audit_p05_phase_rejects_wrong_proof_polarity_and_missing_proof() {
    let mut state = intent();
    state.phase = Phase::ProvedAbsent;
    state.proof = Some(census_proof(&state, true));
    assert!(state.validate(&state.prefix).is_err());
    state.proof = None;
    assert!(state.validate(&state.prefix).is_err());
}
#[test]
fn audit_p06_reproof_preserves_o_and_never_downgrades_presence() {
    let mut old = intent();
    old.phase = Phase::ProvedPresent;
    old.proof = Some(census_proof(&old, true));
    let mut new = old.clone();
    new.proof = Some(census_proof(&new, true));
    let mut change = Change {
        old,
        new,
        attempt: None,
        receipt: None,
    };
    validate_change(&change, ScopeProfile::Reconcile).unwrap();
    change.new.phase = Phase::ProvedAbsent;
    change.new.proof = Some(census_proof(&change.new, false));
    assert!(validate_change(&change, ScopeProfile::Reconcile).is_err());
    change.new.phase = Phase::ProvedPresent;
    change.new.proof = change.old.proof.clone();
    change.new.obligation.as_mut().unwrap().attempt_id = "e".repeat(64);
    assert!(validate_change(&change, ScopeProfile::Reconcile).is_err());
}
fn prefix_fixture() -> (Connection, InitialInputs, Snapshot) {
    let db = Connection::open_in_memory().unwrap();
    db.pragma_update(None, "foreign_keys", false).unwrap();
    for (_, ddl) in schema() {
        db.execute_batch(&ddl).unwrap();
    }
    let inputs = super::super::super::fixture_inputs(json!({"test":"pure-prefix"}));
    let bindings = inputs.bindings().unwrap();
    let mut state = c4tests::pure_state();
    state.binding = Binding::from_inputs(&inputs).unwrap();
    state.prefix = sha256_json_hex(&bindings.prefix).unwrap();
    let receipt = Receipt {
        sequence: 1,
        key: "c6:receipted".into(),
        stored_sha256: "a".repeat(64),
        stored_length: 77,
        original_attempt: "a".repeat(64),
    };
    db.execute(
        "INSERT INTO attempt VALUES(?1,?2,?3,?4)",
        params![
            receipt.original_attempt,
            receipt.key,
            receipt.stored_sha256,
            receipt.stored_length
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO receipt VALUES(?1,?2,?3,?4,?5)",
        params![
            receipt.sequence,
            receipt.key,
            receipt.stored_sha256,
            receipt.stored_length,
            receipt.original_attempt
        ],
    )
    .unwrap();
    state.head = 1;
    state.prefix = chain(&state.prefix, &receipt).unwrap();
    let o = obligation(&state);
    db.execute(
        "INSERT INTO attempt VALUES(?1,?2,?3,?4)",
        params![o.attempt_id, o.key, o.expected_sha256, o.expected_length],
    )
    .unwrap();
    state.phase = Phase::Intent;
    state.obligation = Some(o);
    db.execute(
        "INSERT INTO meta VALUES(1,1,?1,?2,?3,?4,?5,?6,?7,'ready')",
        params![
            state.binding.selection_sha256,
            state.binding.baseline_sha256,
            state.binding.capability_pair_sha256,
            state.binding.epoch,
            state.head,
            state.prefix,
            text(&serde_json::to_value(&state.native).unwrap()).unwrap()
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO current VALUES(1,'INTENT',?1,NULL)",
        [canonical_option(&state.obligation).unwrap()],
    )
    .unwrap();
    let snapshot = Snapshot {
        state,
        tail: Some(receipt),
        requested: None,
        attempt_exists: false,
        prefix_inventory: None,
    };
    (db, inputs, snapshot)
}
fn prefix_result(
    db: &Connection,
    inputs: &InitialInputs,
    snapshot: &Snapshot,
) -> Result<ValidatedPrefix> {
    read_prefix(db, inputs, snapshot, &snapshot.state.native)
}
#[test]
fn audit_p07_prefix_gap_refuses_without_mutation() {
    let (db, inputs, snapshot) = prefix_fixture();
    db.execute("UPDATE receipt SET sequence=2", []).unwrap();
    let before = db.total_changes();
    assert!(prefix_result(&db, &inputs, &snapshot).is_err());
    assert_eq!(db.total_changes(), before);
}
#[test]
fn audit_p08_prefix_excess_row_refuses() {
    let (db, inputs, snapshot) = prefix_fixture();
    db.execute(
        "INSERT INTO receipt VALUES(7,'c6:extra',?1,2,?1)",
        ["a".repeat(64)],
    )
    .unwrap();
    assert!(prefix_result(&db, &inputs, &snapshot).is_err());
}
#[test]
fn audit_p09_prefix_chain_mismatch_refuses() {
    let (db, inputs, mut snapshot) = prefix_fixture();
    snapshot.state.prefix = "f".repeat(64);
    assert!(prefix_result(&db, &inputs, &snapshot).is_err());
}
#[test]
fn audit_p10_prefix_missing_original_attempt_refuses() {
    let (db, inputs, snapshot) = prefix_fixture();
    db.execute("DELETE FROM attempt WHERE attempt_id=?1", ["a".repeat(64)])
        .unwrap();
    assert!(prefix_result(&db, &inputs, &snapshot).is_err());
}
#[test]
fn audit_p11_prefix_wrong_attempt_key_hash_length_refuses() {
    for sql in [
        "UPDATE attempt SET key='foreign' WHERE key='c6:receipted'",
        "UPDATE attempt SET expected_sha256=lower(hex(zeroblob(32))) WHERE key='c6:receipted'",
        "UPDATE attempt SET expected_length=0 WHERE key='c6:receipted'",
    ] {
        let (db, inputs, snapshot) = prefix_fixture();
        db.execute(sql, []).unwrap();
        assert!(prefix_result(&db, &inputs, &snapshot).is_err(), "{sql}");
    }
}
#[test]
fn audit_p12_valid_orphan_attempt_is_preserved() {
    let (db, inputs, snapshot) = prefix_fixture();
    let expected = prefix_result(&db, &inputs, &snapshot).unwrap().expected;
    db.execute(
        "INSERT INTO attempt VALUES(?1,'c6:abandoned',?1,4)",
        ["e".repeat(64)],
    )
    .unwrap();
    let before = db.total_changes();
    assert_eq!(
        prefix_result(&db, &inputs, &snapshot).unwrap().expected,
        expected
    );
    assert_eq!(db.total_changes(), before);
}
#[test]
fn audit_p13_maximum_head_does_not_wrap_or_guess_prefix() {
    let (db, inputs, mut snapshot) = prefix_fixture();
    snapshot.state.head = i64::MAX;
    assert!(prefix_result(&db, &inputs, &snapshot).is_err());
    let mut o = snapshot.state.obligation().unwrap().clone();
    o.previous_head = i64::MAX;
    o.next_head = i64::MAX;
    assert!(o.validate(&snapshot.state.binding).is_err());
}
#[test]
fn audit_p14_baseline_never_becomes_a_receipt() {
    let (db, inputs, snapshot) = prefix_fixture();
    let key = inputs.carriers()[0].0;
    db.execute("UPDATE receipt SET key=?1", [key]).unwrap();
    db.execute(
        "UPDATE attempt SET key=?1 WHERE attempt_id=?2",
        params![key, "a".repeat(64)],
    )
    .unwrap();
    assert!(prefix_result(&db, &inputs, &snapshot).is_err());
}
fn entry(event: &ShoreEvent) -> JournalEntry {
    JournalEntry {
        key_digest: sha256_bytes_hex(event.idempotency_key.as_bytes()),
        bytes: serde_json::to_vec(event).unwrap(),
    }
}
fn payload_event(kind: EventType, payload: Value, key: &str) -> ShoreEvent {
    let mut event = c4tests::event(key, 1);
    event.event_type = kind;
    event.payload = payload;
    event.payload_hash = crate::canonical_hash::sha256_json_prefixed(&event.payload).unwrap();
    c4tests::sign(&mut event, &TestEd25519Signer::from_seed([1; 32]));
    event
}
fn cosign(target: &ShoreEvent, invalid: bool) -> ShoreEvent {
    let signer = TestEd25519Signer::from_seed([2; 32]);
    let tbs = EventToBeSigned::from_event(target, signer.signer_id()).unwrap();
    let message = event_signature_pre_authentication_encoding(&tbs).unwrap();
    let signature = signer
        .sign_event_message(if invalid { b"wrong" } else { &message })
        .unwrap();
    let payload = EventSignatureRecordedPayload {
        target_event_id: target.event_id.clone(),
        target_event_record_hash: target.event_record_hash().unwrap(),
        attesting_signer: signer.signer_id().clone(),
        attestation: EventSignature::ed25519_v1(signature),
        inclusion_proof: None,
    };
    let key = EventSignatureRecordedPayload::idempotency_key(
        &payload.target_event_record_hash,
        &payload.attesting_signer,
        payload.attestation.sig.as_str(),
    );
    ShoreEvent::new(
        EventType::EventSignatureRecorded,
        key,
        target.target.clone(),
        target.writer.clone(),
        payload,
        target.occurred_at.clone(),
    )
    .unwrap()
}
#[test]
fn audit_p15_detached_signature_requires_its_target() {
    let target = c4tests::event("c6:target", 1);
    let carrier = cosign(&target, false);
    assert!(carrier.signature.is_none());
    content::validate_events(&Inventory::new(), vec![entry(&target), entry(&carrier)]).unwrap();
    assert!(content::validate_events(&Inventory::new(), vec![entry(&carrier)]).is_err());
}
#[test]
fn audit_p16_detached_invalid_attestation_refuses() {
    let target = c4tests::event("c6:target", 1);
    let carrier = cosign(&target, true);
    assert!(
        content::validate_events(&Inventory::new(), vec![entry(&target), entry(&carrier)]).is_err()
    );
}
#[test]
fn audit_p17_unsigned_ordinary_and_malformed_payload_refuse() {
    let mut event = c4tests::event("c6:unsigned", 1);
    event.signature = None;
    assert!(content::validate_events(&Inventory::new(), vec![entry(&event)]).is_err());
    let malformed = payload_event(
        EventType::ReviewObservationRecorded,
        json!({}),
        "c6:malformed",
    );
    assert!(content::validate_events(&Inventory::new(), vec![entry(&malformed)]).is_err());
}
fn body_fixture() -> (ShoreEvent, String, Vec<u8>) {
    use crate::model::{ObservationId, ReviewTargetRef, RevisionId};
    use crate::session::store::body_artifact::NoteBodyEnvelope;
    let body = "bounded audit body".repeat(300);
    let hash = format!("sha256:{}", sha256_bytes_hex(body.as_bytes()));
    let path = format!("artifacts/notes/{}.json", &hash[7..]);
    let payload = ReviewObservationRecordedPayload {
        observation_id: ObservationId::new("obs:c6"),
        target: ReviewTargetRef::Revision {
            revision_id: RevisionId::new("rev:c6"),
        },
        title: "audit content".into(),
        body: None,
        body_content_type: BodyContentType::TextPlain,
        body_artifact_path: Some(path.clone()),
        body_byte_size: Some(body.len() as u64),
        body_content_hash: Some(hash),
        tags: vec![],
        confidence: None,
        supersedes_observation_ids: vec![],
        responds_to_observation_ids: vec![],
    };
    (
        payload_event(
            EventType::ReviewObservationRecorded,
            serde_json::to_value(payload).unwrap(),
            "c6:body",
        ),
        path,
        NoteBodyEnvelope::new(body).to_json_bytes().unwrap(),
    )
}
#[test]
fn audit_p18_required_body_missing_or_wrong_hash_refuses() {
    let (event, path, bytes) = body_fixture();
    let mut inventory = Inventory::new();
    assert!(content::validate_events(&inventory, vec![entry(&event)]).is_err());
    inventory.content.insert(path.clone(), bytes);
    content::validate_events(&inventory, vec![entry(&event)]).unwrap();
    for (field, value) in [
        ("bodyByteSize", json!(1)),
        ("body", json!("also inline")),
        ("bodyByteSize", Value::Null),
        ("bodyArtifactPath", Value::Null),
        ("bodyContentHash", Value::Null),
    ] {
        let mut payload = event.payload.clone();
        payload[field] = value;
        let changed = payload_event(event.event_type, payload, "c6:body-invalid");
        assert!(
            content::validate_events(&inventory, vec![entry(&changed)]).is_err(),
            "{field}"
        );
    }
    let mut inline = event.payload.clone();
    inline["bodyArtifactPath"] = Value::Null;
    inline["bodyContentHash"] = Value::Null;
    inline["body"] = json!("é");
    inline["bodyByteSize"] = json!(2);
    let valid = payload_event(event.event_type, inline.clone(), "c6:inline");
    content::validate_events(&inventory, vec![entry(&valid)]).unwrap();
    inline["bodyByteSize"] = json!(1);
    let invalid = payload_event(event.event_type, inline.clone(), "c6:inline-size");
    assert!(content::validate_events(&inventory, vec![entry(&invalid)]).is_err());
    inline["body"] = Value::Null;
    inline["bodyByteSize"] = Value::Null;
    let absent = payload_event(event.event_type, inline, "c6:absent");
    content::validate_events(&inventory, vec![entry(&absent)]).unwrap();
    inventory.content.insert(
        path,
        b"{\"schema\":\"shore.note-body\",\"version\":1,\"body\":\"wrong\"}".to_vec(),
    );
    assert!(content::validate_events(&inventory, vec![entry(&event)]).is_err());
}
fn object_fixture() -> (ShoreEvent, String, Vec<u8>) {
    use crate::model::{DiffSnapshot, EngagementId, ObjectId, ReviewId, RevisionId};
    let object = crate::session::store::object_artifact::build_object_artifact_v2(
        DiffSnapshot::new(ReviewId::new("review:c6"), ObjectId::new("obj:c6"), vec![]),
    )
    .unwrap();
    let path = format!("artifacts/objects/{}.json", &object.content_hash[7..]);
    let payload = WorkObjectProposedPayload {
        engagement_id: EngagementId::new("eng:c6"),
        work_object: WorkObjectProposal::Revision {
            revision: Revision {
                id: RevisionId::new("rev:c6"),
                object_id: object.snapshot.object_id.clone(),
                git_provenance: None,
            },
            summary: None,
            object_artifact_content_hash: object.content_hash.clone(),
            supersedes: vec![],
        },
    };
    (
        payload_event(
            EventType::WorkObjectProposed,
            serde_json::to_value(payload).unwrap(),
            "c6:object",
        ),
        path,
        serde_json::to_vec(&object).unwrap(),
    )
}
#[test]
fn audit_p19_required_object_id_and_hash_are_bound() {
    let (mut event, path, bytes) = object_fixture();
    let mut inventory = Inventory::new();
    inventory.content.insert(path, bytes);
    content::validate_events(&inventory, vec![entry(&event)]).unwrap();
    event.payload["workObject"]["revision"]["objectId"] = json!("obj:wrong");
    event = payload_event(event.event_type, event.payload, "c6:object");
    assert!(content::validate_events(&inventory, vec![entry(&event)]).is_err());
}
fn proof_fixture() -> crate::session::evidence::RelationProofManifestV1 {
    use crate::model::{CommitAssociationId, RevisionId, RevisionRefV1};
    use crate::session::evidence::*;
    let input = CanonicalProofInputV1 {
        capture_mode: ProofCaptureModeV1::CommitRange,
        base_or_parent: Some("abc".into()),
        path_scope: vec![],
        git_availability: ProofGitAvailabilityV1::Available,
        entries: vec![],
    };
    RelationProofManifestV1::new(
        RevisionRefV1::new(
            RevisionId::new("rev:c6"),
            format!("sha256:{}", "a".repeat(64)),
        )
        .unwrap(),
        CommitAssociationId::new("assoc-commit:c6"),
        RelationProofAlgorithmV1::ExactMaterialization,
        input.clone(),
        input,
        RelationProofResultV1 {
            semantic_relation: SemanticRevisionRelationV1::ExactMaterialization,
            proof_status: RelationProofStatusV1::Verified,
            additions: vec![],
        },
    )
    .unwrap()
}
#[test]
fn audit_p20_relation_proof_resource_checks_address_and_result() {
    let mut proof = proof_fixture();
    let stem = proof.evidence_sha256[7..].to_owned();
    content::validate_resource(
        "proofs",
        &stem,
        &format!("artifacts/proofs/{stem}.json"),
        &serde_json::to_vec(&proof).unwrap(),
    )
    .unwrap();
    let path = format!("artifacts/proofs/{stem}.json");
    let mut inventory = Inventory::new();
    inventory
        .content
        .insert(path, serde_json::to_vec(&proof).unwrap());
    for wrong in [false, true] {
        let payload = build_revision_relation_attested(RevisionRelationAttestationDraftV1 {
            revision: proof.revision.clone(),
            commit_association_id: proof.association_id.clone(),
            semantic_relation: proof.result.semantic_relation,
            proof_status: proof.result.proof_status,
            proof_method: "private-c6-control".into(),
            proof_algorithm_version: proof.algorithm_version.clone(),
            capture_scope: vec![],
            comparison_base_or_parent: None,
            endpoint_oids: vec![],
            evidence_content_hash: Some(proof.evidence_sha256.clone()),
            result_digest: if wrong {
                format!("sha256:{}", "f".repeat(64))
            } else {
                proof.result_digest().unwrap()
            },
        })
        .unwrap();
        let event = payload_event(
            EventType::RevisionRelationAttested,
            serde_json::to_value(payload).unwrap(),
            "c6:attestation",
        );
        let result = content::validate_events(&inventory, vec![entry(&event)]);
        assert_eq!(result.is_err(), wrong, "{result:?}");
    }
    proof.result.proof_status = RelationProofStatusV1::Refuted;
    assert!(
        content::validate_resource(
            "proofs",
            &stem,
            "unused",
            &serde_json::to_vec(&proof).unwrap()
        )
        .is_err()
    );
}
#[test]
fn audit_p21_valid_prepared_orphan_content_is_not_an_extra_event() {
    let (_, path, bytes) = object_fixture();
    let mut inventory = Inventory::new();
    content::validate_resource(
        "objects",
        json_stem(Path::new(&path).file_name().unwrap().to_str().unwrap()).unwrap(),
        &path,
        &bytes,
    )
    .unwrap();
    inventory.content.insert(path, bytes);
    content::validate_events(&inventory, vec![]).unwrap();
    assert_eq!(inventory.content.len(), 1);
}
#[test]
fn audit_p22_retired_or_unknown_interpretations_refuse() {
    for kind in [EventType::ReviewNoteImported, EventType::ArtifactRemoved] {
        assert!(
            content::validate_events(
                &Inventory::new(),
                vec![entry(&payload_event(kind, json!({}), "c6:retired"))]
            )
            .is_err()
        );
    }
    let mut event = c4tests::event("c6:version", 1);
    event.payload_version = 2;
    assert!(content::validate_events(&Inventory::new(), vec![entry(&event)]).is_err());
}
#[test]
fn audit_p23_exact_absence_excludes_b0_from_head() {
    let (db, inputs, snapshot) = prefix_fixture();
    let prefix = prefix_result(&db, &inputs, &snapshot).unwrap();
    assert_eq!(prefix.expected.len(), 3);
    assert!(!exact_set(&prefix, &snapshot.state, &prefix.expected).unwrap());
    assert_eq!(snapshot.state.head, 1);
}
#[test]
fn audit_p24_exact_presence_adds_only_original_stored_witness() {
    let (db, inputs, snapshot) = prefix_fixture();
    let prefix = prefix_result(&db, &inputs, &snapshot).unwrap();
    let o = snapshot.state.obligation().unwrap();
    let mut actual = prefix.expected.clone();
    actual.insert(
        sha256_bytes_hex(o.key.as_bytes()),
        (o.expected_sha256.clone(), o.expected_length),
    );
    assert!(exact_set(&prefix, &snapshot.state, &actual).unwrap());
    actual
        .get_mut(&sha256_bytes_hex(o.key.as_bytes()))
        .unwrap()
        .1 += 1;
    assert!(exact_set(&prefix, &snapshot.state, &actual).is_err());
}
#[test]
fn audit_p25_extra_missing_and_changed_carriers_refuse() {
    let (db, inputs, snapshot) = prefix_fixture();
    let prefix = prefix_result(&db, &inputs, &snapshot).unwrap();
    for mode in 0..3 {
        let mut actual = prefix.expected.clone();
        match mode {
            0 => {
                actual.insert("e".repeat(64), ("e".repeat(64), 1));
            }
            1 => {
                actual.pop_first();
            }
            _ => {
                actual.first_entry().unwrap().get_mut().0 = "f".repeat(64);
            }
        };
        assert!(exact_set(&prefix, &snapshot.state, &actual).is_err());
    }
}
fn pair_entries() -> Vec<JournalEntry> {
    let (a, c) = crate::session::store::capabilities::build_progress_pair(
        &TestEd25519Signer::from_seed([71; 32]),
        Writer::shore_local("test"),
        "2026-10-01T00:00:00Z".into(),
        "1".repeat(64),
    )
    .unwrap();
    let pair = crate::session::store::capabilities::validate_progress_pair(&a, &c).unwrap();
    vec![
        JournalEntry {
            key_digest: sha256_bytes_hex(pair.activation.logical_key().as_bytes()),
            bytes: a,
        },
        JournalEntry {
            key_digest: sha256_bytes_hex(pair.completion.logical_key().as_bytes()),
            bytes: c,
        },
    ]
}
#[test]
fn audit_p26_legacy_router_cannot_autodetect_progress_profile() {
    let entries = pair_entries();
    assert!(crate::session::store::capabilities::route_journal_entries(entries.clone()).is_err());
    crate::session::store::capabilities::route_audited_progress_entries(
        entries,
        &JournalAdmission(()),
    )
    .unwrap();
}
#[test]
fn audit_p27_router_rejects_unknown_schema_and_profile() {
    let mut entries = pair_entries();
    let mut value: Value = serde_json::from_slice(&entries[0].bytes).unwrap();
    value["schema"] = json!("foreign.record");
    entries[0].bytes = serde_json::to_vec(&value).unwrap();
    assert!(
        crate::session::store::capabilities::route_audited_progress_entries(
            entries,
            &JournalAdmission(())
        )
        .is_err()
    );
    let mut entries = pair_entries();
    let mut value: Value = serde_json::from_slice(&entries[0].bytes).unwrap();
    value["minimumReaderProfile"] = json!("unknown");
    entries[0].bytes = serde_json::to_vec(&value).unwrap();
    assert!(
        crate::session::store::capabilities::route_audited_progress_entries(
            entries,
            &JournalAdmission(())
        )
        .is_err()
    );
}
#[test]
fn audit_p28_unknown_proof_tags_fail_closed() {
    let state = intent();
    let mut proof = serde_json::to_value(census_proof(&state, false)).unwrap();
    proof["kind"] = json!("census_future");
    assert!(closed::<Proof>(&text(&proof).unwrap()).is_err());
}

// Every native case is ignored by the ordinary suite and receives one exact
// operator-created root. Only N13 has a second explicitly allocated limit root.
fn assigned(name: &str) -> PathBuf {
    let parent = PathBuf::from(
        std::env::var_os("POINTBREAK_C6_FIXTURE_PARENT").expect("assigned C6 parent"),
    );
    assert!(parent.is_absolute());
    let root = parent.join(name);
    assert!(root.is_dir(), "operator-created root {}", root.display());
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        0,
        "never reuse a consumed root"
    );
    root.canonicalize().unwrap()
}
fn setup(root: &Path, head: bool, present: bool, body: bool) {
    c4tests::enroll(root);
    if head {
        assert!(c4tests::publish(root, "c6:prior", 1, 1).unwrap().finalized);
    }
    let event = if body {
        body_fixture().0
    } else {
        c4tests::event("c6:pending", 1)
    };
    let stopped = super::super::super::with_hook(
        move |at| {
            if at
                == if present {
                    "after-publication"
                } else {
                    "before-publication"
                }
            {
                Err(fail("C6 fixture retained INTENT"))
            } else {
                Ok(())
            }
        },
        || {
            c4tests::guarded(root, |r, g| {
                append(
                    r,
                    g,
                    REQUEST,
                    ACTOR,
                    &format!("{:064x}", 2),
                    prepare(event).unwrap(),
                )
            })
        },
    );
    assert!(stopped.is_err());
    assert_eq!(
        c4tests::counts(root),
        (
            i64::from(head),
            i64::from(head),
            if head { 2 } else { 1 },
            "INTENT".into()
        )
    );
}
fn sql_state(root: &Path) -> (String, String, String, i64, i64, i64) {
    c4tests::oracle(root, |db| {
        db.query_row("SELECT coalesce(original_obligation_json,''),coalesce(proof_json,''),phase,(SELECT head FROM meta),(SELECT count(*) FROM receipt),(SELECT count(*) FROM attempt) FROM current",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).unwrap()
    })
}
fn audit_with(
    root: &Path,
    limits: budget::AuditLimits,
    cancel: Arc<AtomicBool>,
) -> (Result<AuditExit>, budget::Totals) {
    let scope = budget::Scope::enter(limits, cancel).unwrap();
    let result = c4tests::guarded(root, |r, g| run(r, g, REQUEST, ACTOR, AUDIT, &scope));
    (result, scope.totals())
}
fn audit_default(root: &Path) -> AuditExit {
    audit_with(
        root,
        budget::AuditLimits::default(),
        Arc::new(AtomicBool::new(false)),
    )
    .0
    .unwrap()
}
fn assert_settled(root: &Path, original: &str, head: i64, present: bool) {
    let state = sql_state(root);
    assert_eq!(state.0, original);
    assert_eq!(state.3, head);
    assert_eq!(state.4, head);
    assert_eq!(
        state.2,
        if present {
            "SETTLED_PRESENT"
        } else {
            "SETTLED_ABSENT"
        }
    );
    let proof: Proof = closed(&state.1).unwrap();
    assert!(proof.census());
    assert_eq!(proof.present(), present);
    if present {
        let attempt = c4tests::oracle(root, |db| {
            db.query_row(
                "SELECT original_attempt FROM receipt ORDER BY sequence DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
        });
        assert_eq!(attempt, format!("{:064x}", 2));
    }
    assert_eq!(c4tests::resume(root).unwrap().head, Some(head as u64));
    assert!(!budget::active());
}
fn create_file(path: &Path, bytes: &[u8]) {
    // This fixture writer follows the same atomic/durable local storage path.
    crate::storage::LocalStorage::new(path.parent().unwrap())
        .create_file_exclusive(path, bytes, crate::storage::Durability::Durable)
        .unwrap();
}
fn native_case(id: u8) {
    if (17..=24).contains(&id) {
        crash_case(id);
        return;
    }
    // SAFETY: operator selects precisely one ignored case in a disposable process.
    let _owner = unsafe { super::super::super::super::owner::isolated_test_owner() };
    let root = assigned(&format!("n{id:02}"));
    let head = matches!(id, 3 | 4 | 8);
    let present = matches!(id, 2 | 4 | 6 | 9 | 10);
    let body = id == 10;
    setup(&root, head, present, body);
    if id == 5 || id == 6 {
        let result = super::super::super::with_hook(
            move |at| {
                if at
                    == if id == 5 {
                        "SETTLED_ABSENT-before-commit"
                    } else {
                        "SETTLED_PRESENT-before-commit"
                    }
                {
                    Err(fail("C6 fixture durable proof"))
                } else {
                    Ok(())
                }
            },
            || {
                if id == 5 {
                    c4tests::resume(&root)
                } else {
                    // Complete presence through an actual census, then stop settlement.
                    let exit = audit_default(&root);
                    assert!(!exit.finalized);
                    Err(fail("deliberate durable census proof stop"))
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(
            sql_state(&root).2,
            if id == 5 {
                "PROVED_ABSENT"
            } else {
                "PROVED_PRESENT"
            }
        );
        create_file(
            &root.join("events/.shore-write.c6-stale.tmp"),
            b"retained temporary bytes",
        );
        assert!(
            c4tests::resume(&root).is_err(),
            "stale proof must require explicit audit"
        );
    }
    let before = sql_state(&root);
    match id {
        1..=6 => {
            let exit = audit_default(&root);
            assert!(exit.finalized, "{:?}", exit);
            assert_settled(
                &root,
                &before.0,
                i64::from(head) + i64::from(present),
                present,
            );
        }
        7 => {
            let extra = c4tests::event("c6:foreign", 1);
            LocalJournal::new(&root)
                .create_event_once(&extra.idempotency_key, &serde_json::to_vec(&extra).unwrap())
                .unwrap();
            assert!(
                audit_with(
                    &root,
                    budget::AuditLimits::default(),
                    Arc::new(AtomicBool::new(false))
                )
                .0
                .is_err()
            );
            assert_eq!(sql_state(&root), before);
        }
        8 => {
            let path = root
                .join("events")
                .join(format!("{}.json", sha256_bytes_hex(b"c6:prior")));
            let retained = root.parent().unwrap().join("n08-missing-carrier.json");
            fs::rename(path, &retained).unwrap();
            assert!(
                audit_with(
                    &root,
                    budget::AuditLimits::default(),
                    Arc::new(AtomicBool::new(false))
                )
                .0
                .is_err()
            );
            assert_eq!(sql_state(&root), before);
            assert!(retained.exists());
        }
        9 => {
            let path = root
                .join("events")
                .join(format!("{}.json", sha256_bytes_hex(b"c6:pending")));
            let retained = root.parent().unwrap().join("n09-original-carrier.json");
            fs::rename(&path, &retained).unwrap();
            create_file(
                &path,
                &serde_json::to_vec(&c4tests::event("c6:pending", 2)).unwrap(),
            );
            assert!(
                audit_with(
                    &root,
                    budget::AuditLimits::default(),
                    Arc::new(AtomicBool::new(false))
                )
                .0
                .is_err()
            );
            assert_eq!(sql_state(&root), before);
        }
        10 => {
            assert!(
                audit_with(
                    &root,
                    budget::AuditLimits::default(),
                    Arc::new(AtomicBool::new(false))
                )
                .0
                .is_err()
            );
            assert_eq!(sql_state(&root), before);
        }
        11 => {
            let moved = Rc::new(Cell::new(false));
            let hook_moved = moved.clone();
            let active = root.join("append-progress/active.json");
            let retained = root.parent().unwrap().join("n11-original-active.json");
            let result = super::super::super::with_hook(
                move |at| {
                    if at == "audit-inventory-entry" && !hook_moved.replace(true) {
                        fs::rename(&active, &retained).unwrap();
                        let bytes = fs::read(&retained).unwrap();
                        create_file(&active, &bytes);
                    }
                    Ok(())
                },
                || {
                    audit_with(
                        &root,
                        budget::AuditLimits::default(),
                        Arc::new(AtomicBool::new(false)),
                    )
                    .0
                },
            );
            assert!(moved.get());
            assert!(result.is_err());
            assert_eq!(sql_state(&root), before);
        }
        12 => {
            c4tests::guarded(&root, |_, g| {
                let wrong = native::physical_root(&root.join("events")).unwrap();
                let scope = budget::Scope::enter(
                    budget::AuditLimits::default(),
                    Arc::new(AtomicBool::new(false)),
                )
                .unwrap();
                assert!(run(&wrong, g, REQUEST, ACTOR, AUDIT, &scope).is_err());
            });
            assert_eq!(sql_state(&root), before);
        }
        13 => {
            let measured = audit_default(&root);
            assert!(measured.finalized);
            let second = assigned("n13-limit");
            setup(&second, false, false, false);
            let original = sql_state(&second).0;
            let (result, totals) = audit_with(
                &second,
                budget::AuditLimits {
                    visits: measured.visits,
                    ..budget::AuditLimits::default()
                },
                Arc::new(AtomicBool::new(false)),
            );
            let result = result.unwrap();
            assert!(result.finalized, "{result:?}");
            assert_eq!(totals.visits, measured.visits);
            assert_settled(&second, &original, 0, false);
        }
        14 => {
            let (_, path, bytes) = body_fixture();
            create_file(&root.join(path), &bytes);
            let measured = Rc::new(Cell::new(None));
            let hook_measured = measured.clone();
            let first = super::super::super::with_hook(
                move |at| {
                    if at == "audit-before-content-read" {
                        hook_measured.set(Some(budget::totals_for_test()));
                        Err(fail("measure before actual content read"))
                    } else {
                        Ok(())
                    }
                },
                || {
                    audit_with(
                        &root,
                        budget::AuditLimits::default(),
                        Arc::new(AtomicBool::new(false)),
                    )
                    .0
                },
            );
            assert!(first.is_err());
            assert_eq!(sql_state(&root), before);
            let prefix = measured.get().expect("content read reached");
            let limit = prefix.bytes + 32;
            let (result, totals) = audit_with(
                &root,
                budget::AuditLimits {
                    bytes: limit,
                    ..budget::AuditLimits::default()
                },
                Arc::new(AtomicBool::new(false)),
            );
            assert!(result.is_err());
            assert_eq!(totals.bytes, limit);
            assert_eq!(sql_state(&root), before);
            println!(
                "C6 actual partial content read prefix={} delivered=32 total={}",
                prefix.bytes, totals.bytes
            );
        }
        15 => {
            let cancel = Arc::new(AtomicBool::new(false));
            let hook_cancel = cancel.clone();
            let reached = Rc::new(Cell::new(false));
            let hook_reached = reached.clone();
            let result = super::super::super::with_hook(
                move |at| {
                    if at == "audit-inventory-entry" {
                        hook_reached.set(true);
                        hook_cancel.store(true, std::sync::atomic::Ordering::Release);
                    }
                    Ok(())
                },
                || audit_with(&root, budget::AuditLimits::default(), cancel).0,
            );
            assert!(reached.get());
            assert!(result.is_err());
            assert_eq!(sql_state(&root), before);
        }
        16 => {
            let reached = Rc::new(Cell::new(false));
            let hook_reached = reached.clone();
            let result = super::super::super::with_hook(
                move |at| {
                    if at == "audit-before-proof" {
                        hook_reached.set(true);
                        budget::expire_for_test();
                    }
                    Ok(())
                },
                || {
                    audit_with(
                        &root,
                        budget::AuditLimits::default(),
                        Arc::new(AtomicBool::new(false)),
                    )
                    .0
                },
            );
            assert!(reached.get());
            assert!(result.is_err());
            assert_eq!(sql_state(&root), before);
        }
        25..=28 => {
            let profile = match id {
                25 => ScopeProfile::Selected,
                26 => ScopeProfile::CreateInit,
                27 => ScopeProfile::Certify,
                _ => ScopeProfile::Append,
            };
            c4tests::guarded(&root, |r, g| {
                let session = Session::new(r, g, REQUEST, ACTOR).unwrap();
                let result = session.step(
                    profile,
                    Command {
                        full_prefix: true,
                        key: None,
                        attempt: None,
                        change: None,
                        required_native: None,
                    },
                );
                assert!(
                    result
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("closed Reconcile")
                );
            });
            assert_eq!(sql_state(&root), before);
        }
        _ => unreachable!(),
    }
    println!("C6 native n{id:02} PASS state={:?}", sql_state(&root));
}
fn child(root: &Path, id: u8, mode: &str, cut: Option<&str>) -> std::process::Output {
    use std::process::Command as ProcessCommand;
    let mut command = ProcessCommand::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            &format!("session::append_progress::ledger::c4::audit::tests::native_n{id:02}"),
            "--ignored",
            "--nocapture",
        ])
        .env("POINTBREAK_C6_CHILD", mode)
        .env("POINTBREAK_C6_CHILD_ROOT", root)
        .env("POINTBREAK_C4_CHILD", "1")
        .env_remove("POINTBREAK_C4_CUT");
    if let Some(cut) = cut {
        command.env("POINTBREAK_C4_CUT", cut);
    }
    capture_child(command, root, &format!("n{id:02}-{mode}"), None)
}

/// Redirect output to retained files before launch, so a full pipe cannot block
/// the child. Every timeout, stdin error and wait error kills and reaps this
/// exact child before the parent reports failure. No unrelated PID is signalled.
fn capture_child(
    mut command: std::process::Command,
    root: &Path,
    label: &str,
    input: Option<Vec<u8>>,
) -> std::process::Output {
    use std::io::Write as _;
    use std::process::Stdio;
    let parent = root.parent().unwrap();
    let stdout = parent.join(format!("{label}.stdout"));
    let stderr = parent.join(format!("{label}.stderr"));
    command.stdout(fs::File::create_new(&stdout).unwrap());
    command.stderr(fs::File::create_new(&stderr).unwrap());
    command.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = command.spawn().unwrap();
    println!(
        "C6 child pid={} root={} stdout={} stderr={}",
        child.id(),
        root.display(),
        stdout.display(),
        stderr.display()
    );
    let mut failure = None;
    if let Some(input) = input
        && let Err(error) = child.stdin.take().unwrap().write_all(&input)
    {
        failure = Some(format!("bootstrap write: {error}"));
    }
    let until = Instant::now() + Duration::from_secs(180);
    let status = loop {
        if failure.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                Ok(None) => failure = Some("child deadline exhausted".to_owned()),
                Err(error) => failure = Some(format!("child wait: {error}")),
            }
        }
        // The child may have exited between polling and kill. Always wait even
        // when kill reports that race; a successful wait establishes collection.
        let killed = child.kill();
        let reaped = child.wait();
        println!("C6 child failure={failure:?} kill={killed:?} reaped={reaped:?}");
        break reaped.expect("UNKNOWN unreaped C6 child; preserve output and stop");
    };
    let output = std::process::Output {
        status,
        stdout: fs::read(&stdout).unwrap(),
        stderr: fs::read(&stderr).unwrap(),
    };
    println!(
        "C6 child terminal {} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        failure.is_none(),
        "C6 child failed: {failure:?}; retained {} and {}",
        stdout.display(),
        stderr.display()
    );
    output
}

fn crash_case(id: u8) {
    let present = id >= 21;
    let cut_index = (id - 17) % 4;
    if let Ok(mode) = std::env::var("POINTBREAK_C6_CHILD") {
        // SAFETY: parent selected exactly this one-test disposable native child.
        let _owner = unsafe { super::super::super::super::owner::isolated_test_owner() };
        let root = PathBuf::from(std::env::var_os("POINTBREAK_C6_CHILD_ROOT").unwrap());
        match mode.as_str() {
            "setup" => {
                setup(&root, false, present, false);
                println!("C6 original={}", sql_state(&root).0);
            }
            "audit" => {
                let result = audit_default(&root);
                panic!("crash boundary did not fire: {result:?}");
            }
            "resume" => {
                let state = sql_state(&root);
                let original = state.0.clone();
                let o: Obligation = closed(&original).unwrap();
                assert_eq!(o.attempt_id, format!("{:064x}", 2));
                assert_eq!(o.key, "c6:pending");
                if cut_index == 0 {
                    assert_eq!(state.2, "INTENT");
                    assert!(state.1.is_empty());
                    assert!(audit_default(&root).finalized);
                } else {
                    assert!(matches!(
                        state.2.as_str(),
                        "PROVED_ABSENT" | "PROVED_PRESENT" | "SETTLED_ABSENT" | "SETTLED_PRESENT"
                    ));
                    assert!(c4tests::resume(&root).unwrap().finalized);
                }
                assert_settled(&root, &original, i64::from(present), present);
                assert_eq!(sql_state(&root).5, 1);
            }
            _ => panic!("unknown C6 child mode"),
        }
        return;
    }
    let root = assigned(&format!("n{id:02}"));
    assert!(child(&root, id, "setup", None).status.success());
    let phase = if present {
        "PROVED_PRESENT"
    } else {
        "PROVED_ABSENT"
    };
    let settled = if present {
        "SETTLED_PRESENT"
    } else {
        "SETTLED_ABSENT"
    };
    let cut = match cut_index {
        0 => format!("{phase}-before-commit"),
        1 => format!("{phase}-after-commit"),
        2 => "audit-durable-proof".into(),
        _ => format!("{settled}-after-commit"),
    };
    assert_eq!(
        child(&root, id, "audit", Some(&cut)).status.code(),
        Some(79)
    );
    assert!(child(&root, id, "resume", None).status.success());
    println!("C6 native n{id:02} crash/restart PASS");
}
macro_rules! native_tests {($($name:ident=>$id:literal),* $(,)?)=>{$(
    #[test]
    #[ignore="requires exact assigned private C6 roots and disposable native owner"]
    fn $name(){native_case($id);}
)*};}
native_tests!(native_n01=>1,native_n02=>2,native_n03=>3,native_n04=>4,native_n05=>5,native_n06=>6,native_n07=>7,native_n08=>8,native_n09=>9,native_n10=>10,native_n11=>11,native_n12=>12,native_n13=>13,native_n14=>14,native_n15=>15,native_n16=>16,native_n17=>17,native_n18=>18,native_n19=>19,native_n20=>20,native_n21=>21,native_n22=>22,native_n23=>23,native_n24=>24,native_n25=>25,native_n26=>26,native_n27=>27,native_n28=>28);

#[test]
#[ignore = "operator-bound actual private audit binary and disposable canary root"]
fn native_binary_canary() {
    use std::process::Command as ProcessCommand;
    // SAFETY: one isolated canary process; every fixture SQL owner is finished
    // before the whole-command binary child receives the root.
    let _owner = unsafe { super::super::super::super::owner::isolated_test_owner() };
    let root = assigned("binary-canary");
    setup(&root, false, true, false);
    let original = sql_state(&root).0;
    let binary =
        PathBuf::from(std::env::var_os("POINTBREAK_C6_BINARY").expect("exact candidate binary"));
    assert!(binary.is_absolute());
    let mut command = ProcessCommand::new(binary);
    command.arg("__owned-audit-v1");
    let input = serde_json::to_vec(&json!({
        "version":1, "root":root, "requestId":REQUEST, "enrollmentActor":ACTOR,
        "auditRequest":AUDIT, "limits":budget::AuditLimits::default()
    }))
    .unwrap();
    let output = capture_child(command, &root, "binary-canary", Some(input));
    assert!(output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["auditRequest"], AUDIT);
    assert_eq!(result["finalized"], true);
    assert_eq!(result["head"], 1);
    assert_settled(&root, &original, 1, true);
}
