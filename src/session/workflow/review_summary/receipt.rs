//! The counted-input receipt: which exact facts the summary counted, bound by
//! payload and record content, with the reader's verification of each.

use std::collections::BTreeMap;

use super::model::{CountedInput, CountedInputKind};
use crate::canonical_hash::sha256_bytes_hex;
use crate::crypto::EventVerificationStatus;
use crate::error::{Result, ShoreError};
use crate::session::event::ShoreEvent;
use crate::session::signing::{TrustSet, verify_event_signature};

/// The entry shape and ordering the receipt digest follows.
pub const COUNTED_INPUT_DIGEST_ALGORITHM: &str = "shore.event-set.canonical-map.v1";

/// One counted event: its id, the hash of its payload, the hash of its record
/// apart from signatures and transport fields, and how the reader's trust set
/// verifies its signature.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountedInputEntry {
    pub event_id: String,
    pub payload_hash: String,
    pub event_record_hash: String,
    pub verification_status: EventVerificationStatus,
}

/// Counted events by kind.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CountedInputKindCounts {
    pub membership_claims: usize,
    pub membership_withdrawals: usize,
    pub captures: usize,
    pub assessments: usize,
    pub commit_associations: usize,
}

/// Counted events by signature verification outcome.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VerificationTally {
    pub valid: usize,
    pub untrusted_key: usize,
    pub invalid: usize,
    pub unsigned: usize,
}

/// The receipt of the facts a summary counted. It proves which inputs were
/// counted, never that the store is complete.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountedInputReceipt {
    /// `sha256:<hex>` over the entry lines; verification status is outside it.
    pub digest: String,
    /// Sorted by event id, then record hash.
    pub entries: Vec<CountedInputEntry>,
    pub by_kind: CountedInputKindCounts,
    pub verification: VerificationTally,
    /// Whether the reader's trust set names at least one allowed signer.
    pub allowed_signers_configured: bool,
}

/// Build the receipt for `counted` from the envelopes in `events`, which must
/// hold every counted event and may hold more. A counted event with no envelope,
/// or whose envelope's payload hash differs from the counted row's, fails the
/// read.
pub(crate) fn counted_input_receipt(
    counted: &[CountedInput],
    events: &[ShoreEvent],
    trust: &TrustSet,
) -> Result<CountedInputReceipt> {
    let envelopes: BTreeMap<&str, &ShoreEvent> = events
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect();
    let mut unique: BTreeMap<&str, &CountedInput> = BTreeMap::new();
    for input in counted {
        unique
            .entry(input.source.event_id.as_str())
            .or_insert(input);
    }

    let mut by_kind = CountedInputKindCounts::default();
    let mut verification = VerificationTally::default();
    let mut entries = Vec::with_capacity(unique.len());
    for (event_id, input) in unique {
        let event = envelopes.get(event_id).ok_or_else(|| {
            receipt_error(format!("counted event {event_id} has no stored envelope"))
        })?;
        if event.payload_hash != input.source.payload_hash {
            return Err(receipt_error(format!(
                "counted event {event_id} was read with payload hash {} but its stored envelope \
                 carries {}",
                input.source.payload_hash, event.payload_hash
            )));
        }
        let verification_status = verify_event_signature(event, trust)?;
        match input.kind {
            CountedInputKind::MembershipClaim => by_kind.membership_claims += 1,
            CountedInputKind::MembershipWithdrawal => by_kind.membership_withdrawals += 1,
            CountedInputKind::Capture => by_kind.captures += 1,
            CountedInputKind::Assessment => by_kind.assessments += 1,
            CountedInputKind::CommitAssociation => by_kind.commit_associations += 1,
        }
        match verification_status {
            EventVerificationStatus::Valid => verification.valid += 1,
            EventVerificationStatus::UntrustedKey => verification.untrusted_key += 1,
            EventVerificationStatus::Invalid => verification.invalid += 1,
            EventVerificationStatus::Unsigned => verification.unsigned += 1,
        }
        entries.push(CountedInputEntry {
            event_id: event_id.to_owned(),
            payload_hash: event.payload_hash.clone(),
            event_record_hash: event.event_record_hash()?,
            verification_status,
        });
    }
    sort_entries(&mut entries);

    Ok(CountedInputReceipt {
        digest: counted_input_digest(&entries),
        entries,
        by_kind,
        verification,
        allowed_signers_configured: trust
            .allowed_signers()
            .values()
            .any(|signers| !signers.is_empty()),
    })
}

/// The digest over entry lines `eventId SP payloadHash SP eventRecordHash LF`,
/// sorted by event id then record hash.
pub(crate) fn counted_input_digest(entries: &[CountedInputEntry]) -> String {
    let mut sorted: Vec<&CountedInputEntry> = entries.iter().collect();
    sorted.sort_by(|left, right| entry_order(left).cmp(&entry_order(right)));
    let mut bytes = Vec::new();
    for entry in sorted {
        bytes.extend_from_slice(entry.event_id.as_bytes());
        bytes.push(b' ');
        bytes.extend_from_slice(entry.payload_hash.as_bytes());
        bytes.push(b' ');
        bytes.extend_from_slice(entry.event_record_hash.as_bytes());
        bytes.push(b'\n');
    }
    format!("sha256:{}", sha256_bytes_hex(&bytes))
}

fn entry_order(entry: &CountedInputEntry) -> (&str, &str) {
    (entry.event_id.as_str(), entry.event_record_hash.as_str())
}

fn sort_entries(entries: &mut [CountedInputEntry]) {
    entries.sort_by(|left, right| entry_order(left).cmp(&entry_order(right)));
}

fn receipt_error(message: String) -> ShoreError {
    ShoreError::InvalidEvent {
        message: format!("review summary receipt: {message}"),
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::model::{ActorId, JournalId};
    use crate::session::EventSigningOptions;
    use crate::session::event::{
        EventTarget, EventType, ReviewInitializedPayload, Writer, WriterProducer,
    };
    use crate::session::signing::sign_event_if_requested;
    use crate::session::signing::test_support::{DeterministicSigner, trust_for_actor};
    use crate::session::workflow::review_summary::model::{CountedEventRef, CountedInputKind};

    fn entry(event_id: &str, payload_hash: &str, record_hash: &str) -> CountedInputEntry {
        CountedInputEntry {
            event_id: event_id.to_owned(),
            payload_hash: payload_hash.to_owned(),
            event_record_hash: record_hash.to_owned(),
            verification_status: EventVerificationStatus::Unsigned,
        }
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn event(key: &str, actor: &str) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewInitialized,
            key,
            EventTarget::for_journal(JournalId::new("journal:default")),
            Writer {
                actor_id: ActorId::new(actor.to_owned()),
                producer: WriterProducer {
                    name: "pointbreak".to_owned(),
                    version: String::new(),
                },
            },
            ReviewInitializedPayload {},
            "2026-06-01T00:00:00Z",
        )
        .unwrap()
    }

    fn signed(key: &str, actor: &str, signer: &DeterministicSigner) -> ShoreEvent {
        let mut event = event(key, actor);
        sign_event_if_requested(&mut event, &EventSigningOptions::sign_with(signer.clone()))
            .unwrap();
        event
    }

    fn counted(event: &ShoreEvent, kind: CountedInputKind) -> CountedInput {
        CountedInput {
            source: CountedEventRef {
                event_id: event.event_id.as_str().to_owned(),
                payload_hash: event.payload_hash.clone(),
            },
            kind,
        }
    }

    #[test]
    fn counted_input_digest_matches_the_literal_entry_bytes() {
        let first = entry("evt:sha256:aa", "sha256:bb", "sha256:cc");
        let second = entry("evt:sha256:dd", "sha256:ee", "sha256:ff");
        let expected = format!(
            "sha256:{}",
            sha256_hex(b"evt:sha256:aa sha256:bb sha256:cc\nevt:sha256:dd sha256:ee sha256:ff\n")
        );

        assert_eq!(
            counted_input_digest(&[second.clone(), first.clone()]),
            expected
        );
        assert_eq!(counted_input_digest(&[first, second]), expected);
    }

    #[test]
    fn counted_input_digest_of_no_entries_is_the_empty_input_digest() {
        assert_eq!(
            counted_input_digest(&[]),
            format!("sha256:{}", sha256_hex(b""))
        );
    }

    #[test]
    fn counted_input_digest_ignores_verification_status() {
        let mut signed_entry = entry("evt:sha256:aa", "sha256:bb", "sha256:cc");
        let unsigned = counted_input_digest(std::slice::from_ref(&signed_entry));
        signed_entry.verification_status = EventVerificationStatus::Valid;

        assert_eq!(counted_input_digest(&[signed_entry]), unsigned);
    }

    #[test]
    fn counted_input_receipt_ignores_input_order() {
        let events = [
            event("k1", "actor:a"),
            event("k2", "actor:a"),
            event("k3", "actor:a"),
        ];
        let inputs = [
            counted(&events[0], CountedInputKind::Capture),
            counted(&events[1], CountedInputKind::Assessment),
            counted(&events[2], CountedInputKind::MembershipClaim),
        ];
        let trust = TrustSet::default();

        let forward = counted_input_receipt(&inputs, &events, &trust).unwrap();
        let mut reversed_inputs = inputs.to_vec();
        reversed_inputs.reverse();
        let mut reversed_events = events.to_vec();
        reversed_events.reverse();

        assert_eq!(
            counted_input_receipt(&reversed_inputs, &reversed_events, &trust).unwrap(),
            forward
        );
        assert_eq!(forward.entries.len(), 3);
        assert!(
            forward
                .entries
                .windows(2)
                .all(|pair| pair[0].event_id < pair[1].event_id)
        );
        assert_eq!(forward.digest, counted_input_digest(&forward.entries));
        assert_eq!(
            forward.by_kind,
            CountedInputKindCounts {
                membership_claims: 1,
                captures: 1,
                assessments: 1,
                ..CountedInputKindCounts::default()
            }
        );
        for (entry, event) in forward.entries.iter().zip({
            let mut sorted = events.to_vec();
            sorted.sort_by(|left, right| left.event_id.as_str().cmp(right.event_id.as_str()));
            sorted
        }) {
            assert_eq!(entry.payload_hash, event.payload_hash);
            assert_eq!(entry.event_record_hash, event.event_record_hash().unwrap());
        }
    }

    #[test]
    fn counted_input_receipt_tallies_verification_without_changing_the_digest() {
        let signer = DeterministicSigner::from_seed([7; 32]);
        let trusted = signed("k-trusted", "actor:trusted", &signer);
        let untrusted = signed("k-untrusted", "actor:stranger", &signer);
        let unsigned = event("k-unsigned", "actor:trusted");
        let events = [trusted.clone(), untrusted.clone(), unsigned.clone()];
        let inputs: Vec<_> = events
            .iter()
            .map(|event| counted(event, CountedInputKind::Assessment))
            .collect();

        let with_trust = counted_input_receipt(
            &inputs,
            &events,
            &trust_for_actor(&ActorId::new("actor:trusted"), &signer),
        )
        .unwrap();
        let without_trust = counted_input_receipt(&inputs, &events, &TrustSet::default()).unwrap();

        assert_eq!(
            with_trust.verification,
            VerificationTally {
                valid: 1,
                untrusted_key: 1,
                unsigned: 1,
                ..VerificationTally::default()
            }
        );
        assert!(with_trust.allowed_signers_configured);
        assert_eq!(
            without_trust.verification,
            VerificationTally {
                untrusted_key: 2,
                unsigned: 1,
                ..VerificationTally::default()
            }
        );
        assert!(!without_trust.allowed_signers_configured);
        assert_eq!(with_trust.digest, without_trust.digest);
    }

    #[test]
    fn counted_input_receipt_collapses_duplicate_counted_events() {
        let shared = event("k-shared", "actor:a");
        let inputs = [
            counted(&shared, CountedInputKind::Capture),
            counted(&shared, CountedInputKind::Capture),
        ];

        let receipt = counted_input_receipt(&inputs, &[shared], &TrustSet::default()).unwrap();

        assert_eq!(receipt.entries.len(), 1);
        assert_eq!(receipt.by_kind.captures, 1);
    }

    #[test]
    fn counted_input_receipt_requires_every_envelope() {
        let present = event("k-present", "actor:a");
        let missing = event("k-missing", "actor:a");
        let inputs = [
            counted(&present, CountedInputKind::Capture),
            counted(&missing, CountedInputKind::Capture),
        ];

        let error = counted_input_receipt(&inputs, &[present], &TrustSet::default())
            .expect_err("a counted event without an envelope fails");
        assert!(error.to_string().contains(missing.event_id.as_str()));
    }

    #[test]
    fn counted_input_receipt_rejects_a_payload_hash_mismatch() {
        let event = event("k1", "actor:a");
        let mut input = counted(&event, CountedInputKind::Capture);
        input.source.payload_hash = "sha256:not-the-payload".to_owned();

        let error =
            counted_input_receipt(&[input], std::slice::from_ref(&event), &TrustSet::default())
                .expect_err("a payload hash mismatch fails");
        assert!(error.to_string().contains(event.event_id.as_str()));
    }

    #[test]
    fn counted_input_receipt_of_nothing_is_empty() {
        let receipt = counted_input_receipt(&[], &[], &TrustSet::default()).unwrap();

        assert_eq!(receipt.digest, format!("sha256:{}", sha256_hex(b"")));
        assert!(receipt.entries.is_empty());
        assert_eq!(receipt.by_kind, CountedInputKindCounts::default());
        assert_eq!(receipt.verification, VerificationTally::default());
    }
}
