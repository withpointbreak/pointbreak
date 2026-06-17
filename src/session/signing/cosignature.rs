//! Per-member verification of a detached co-signature carrier's embedded
//! attestation.
//!
//! This is the one-attestation generalization of the inline event verifier
//! (`verify_event_signature`): the inline single-signer verifier is the
//! one-member-set special case. It reuses the same primitives — the DSSE
//! `event-tbs.v1` view, the existing PAE, `verify_ed25519_strict`, and
//! `TrustSet::authorizes` — so the two stay congruent.
//!
//! **Two digests, never confused.** The attestation signs the signer-INCLUSIVE
//! target TBS view (the view naming the *attesting* signer), so each co-signer
//! signs a view naming themselves and neither attestation is replayable as the
//! other. The carrier separately binds the signer-EXCLUSIVE
//! `targetEventRecordHash` — the convergent content identity. These are different
//! digests over different field sets; a carrier whose bound hash does not resolve
//! to the supplied target is a **binding mismatch** (the carrier is not a
//! co-signature *of this record* at all) and must stay distinct from a
//! cryptographically `Invalid` attestation.

use super::TrustSet;
use crate::crypto::{EventVerificationStatus, verify_ed25519_strict};
use crate::error::Result;
use crate::session::event::{
    EventSignatureRecordedPayload, EventToBeSigned, ShoreEvent,
    event_signature_pre_authentication_encoding,
};

const ED25519_SIGNATURE_ALG: &str = "ed25519";
const EVENT_SIGNATURE_VERSION: u32 = 1;

/// The outcome of verifying a detached co-signature carrier against a resolved
/// target. `BindingMismatch` is deliberately NOT an `EventVerificationStatus`
/// value: it is a structural rejection that precedes cryptographic
/// classification, so it can never be confused with `Invalid`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CosignatureVerification {
    /// The carrier binds this target and its attestation classified to a status
    /// (`Valid` / `Invalid` / `UntrustedKey`; never `Unsigned`).
    Attested(EventVerificationStatus),
    /// `carrier.target_event_record_hash` does not equal `target.event_record_hash()`.
    BindingMismatch,
}

/// Verifies a co-signature carrier's embedded attestation against a resolved
/// target and a trust set. See the module docs for the two-digest distinction.
pub fn verify_cosignature(
    carrier: &EventSignatureRecordedPayload,
    target: &ShoreEvent,
    trust: &TrustSet,
) -> Result<CosignatureVerification> {
    // Step 1 — binding check over the signer-exclusive digest. A bad binding is a
    // hard reject, distinct from a malformed signature of this fact.
    if target.event_record_hash()? != carrier.target_event_record_hash {
        return Ok(CosignatureVerification::BindingMismatch);
    }

    // Mirror the inline verifier's alg/sigVersion guard.
    if carrier.attestation.alg != ED25519_SIGNATURE_ALG
        || carrier.attestation.sig_version != EVENT_SIGNATURE_VERSION
    {
        return Ok(CosignatureVerification::Attested(
            EventVerificationStatus::Invalid,
        ));
    }

    // Step 2 — reconstruct the signer-INCLUSIVE TBS with the attesting signer
    // substituted (not the target's own effective signer).
    let tbs = match EventToBeSigned::from_event(target, &carrier.attesting_signer) {
        Ok(tbs) => tbs,
        Err(_) => {
            return Ok(CosignatureVerification::Attested(
                EventVerificationStatus::Invalid,
            ));
        }
    };
    // Step 3 — PAE, reused verbatim.
    let message = match event_signature_pre_authentication_encoding(&tbs) {
        Ok(message) => message,
        Err(_) => {
            return Ok(CosignatureVerification::Attested(
                EventVerificationStatus::Invalid,
            ));
        }
    };
    // Step 4 — strict Ed25519 verify. The ADR-0004 `invalid` set (malformed
    // did:key, wrong length, non-canonical key, signature mismatch) all fall out
    // of `verify_ed25519_strict` returning `Invalid`, without consulting trust.
    let status = verify_ed25519_strict(
        &carrier.attesting_signer,
        &message,
        carrier.attestation.sig.as_str(),
    )?;
    if status != EventVerificationStatus::Valid {
        return Ok(CosignatureVerification::Attested(status));
    }

    // Step 5 — trust classification against the TARGET's claimed actor. This is
    // what makes a `Valid` co-signature mean "verifies AND signer authorized for
    // the claimed writer.actorId".
    if trust.authorizes(
        &target.writer.actor_id,
        &carrier.attesting_signer,
        target.occurred_at.as_str(),
    ) {
        Ok(CosignatureVerification::Attested(
            EventVerificationStatus::Valid,
        ))
    } else {
        Ok(CosignatureVerification::Attested(
            EventVerificationStatus::UntrustedKey,
        ))
    }
}

/// Diagnostic code for a detached co-signature dropped because its attestation
/// is structurally invalid (the ADR-0004 `invalid` set). Reader-independent noise.
pub const COSIGNATURE_INVALID_CODE: &str = "cosignature_invalid";
/// Diagnostic code for a detached co-signature dropped because its target is not
/// present in the store. The carrier is left no trace and is re-offered on a later
/// sync pass; no replay machinery exists in v1 (deferred to the sync plane).
pub const COSIGNATURE_TARGET_PENDING_CODE: &str = "cosignature_target_pending";
/// Diagnostic code for a detached co-signature dropped because its bound
/// `targetEventRecordHash` does not match the present target's recomputed hash —
/// the carrier is a co-signature of a different record, not a bad signature.
pub const COSIGNATURE_BINDING_MISMATCH_CODE: &str = "cosignature_binding_mismatch";
/// Diagnostic code for a stored co-signature whose merged signer is not authorized
/// for the claimed actor in this reader's trust set (`untrusted_key`). This is an
/// authorization observation, never a divergence report: the signature is real and
/// the set unioned cleanly; the signer is merely not (yet) trusted here.
pub const COSIGNATURE_UNTRUSTED_SIGNER_CODE: &str = "cosignature_untrusted_signer";

/// What the verify-before-store gate decides for one detached co-signature carrier.
///
/// The gate is the single rule shared by both producer paths (local construction
/// and ingest). It is **always on** and independent of `EventVerificationPolicy`:
/// the policy governs inline-signature ingest acceptance, while the detached
/// co-signature family has its own asymmetric rule — a structurally `invalid`
/// detached attestation is reader-independent noise and never reaches the store,
/// even under an advisory policy. (The one attestation that may be stored
/// `invalid` is the inline one, which is part of the event's own bytes.)
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CosignatureGateDecision {
    /// Verified `Valid` or `UntrustedKey` against a present target → caller stores it.
    Store(EventVerificationStatus),
    /// Verified `Invalid` → caller drops it and emits `cosignature_invalid`.
    DropInvalid,
    /// The carrier's target is not present → caller drops it and emits
    /// `cosignature_target_pending`. No replay is scheduled here.
    TargetPending,
    /// The carrier binds a present target whose `eventRecordHash` does not match →
    /// caller drops it and emits `cosignature_binding_mismatch`.
    BindingMismatch,
}

impl CosignatureGateDecision {
    /// True when the caller should proceed to store the carrier.
    pub fn stores(&self) -> bool {
        matches!(self, Self::Store(_))
    }

    /// The diagnostic code for a drop decision, or `None` when the carrier stores.
    pub fn drop_diagnostic_code(&self) -> Option<&'static str> {
        match self {
            Self::Store(_) => None,
            Self::DropInvalid => Some(COSIGNATURE_INVALID_CODE),
            Self::TargetPending => Some(COSIGNATURE_TARGET_PENDING_CODE),
            Self::BindingMismatch => Some(COSIGNATURE_BINDING_MISMATCH_CODE),
        }
    }
}

/// The single verify-before-store gate for a detached co-signature carrier, shared
/// by the local construction path and the ingest path so the rule lives in exactly
/// one place. `target` is the resolved target the carrier names (`None` when it is
/// not present in the store: the carrier is dropped with no trace and the
/// replay-after-backfill ordering is deferred to the sync plane, not designed here).
pub fn gate_cosignature_for_store(
    carrier: &EventSignatureRecordedPayload,
    target: Option<&ShoreEvent>,
    trust: &TrustSet,
) -> Result<CosignatureGateDecision> {
    let Some(target) = target else {
        return Ok(CosignatureGateDecision::TargetPending);
    };
    let decision = match verify_cosignature(carrier, target, trust)? {
        CosignatureVerification::BindingMismatch => CosignatureGateDecision::BindingMismatch,
        CosignatureVerification::Attested(EventVerificationStatus::Invalid) => {
            CosignatureGateDecision::DropInvalid
        }
        CosignatureVerification::Attested(status @ EventVerificationStatus::Valid)
        | CosignatureVerification::Attested(status @ EventVerificationStatus::UntrustedKey) => {
            CosignatureGateDecision::Store(status)
        }
        // A carrier always carries an attestation; `Unsigned` is N/A. Treat any
        // unexpected status as a drop rather than silently storing it.
        CosignatureVerification::Attested(EventVerificationStatus::Unsigned) => {
            CosignatureGateDecision::DropInvalid
        }
    };
    Ok(decision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{EventSignatureBytes, EventSigner};
    use crate::session::event::{EventSignature, ShoreEvent};
    use crate::session::signing::test_support::{DeterministicSigner, trust_for_actor};

    fn target_event() -> ShoreEvent {
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/event_signatures/friendly-valid-event.json"
        ))
        .expect("fixture event decodes")
    }

    fn cosign(target: &ShoreEvent, signer: &DeterministicSigner) -> EventSignatureRecordedPayload {
        let attesting_signer = signer.signer_id().clone();
        let tbs = EventToBeSigned::from_event(target, &attesting_signer).unwrap();
        let pae = event_signature_pre_authentication_encoding(&tbs).unwrap();
        let sig = signer.sign_event_message(&pae).unwrap();
        EventSignatureRecordedPayload {
            target_event_id: target.event_id.clone(),
            target_event_record_hash: target.event_record_hash().unwrap(),
            attesting_signer,
            attestation: EventSignature::ed25519_v1(sig),
            inclusion_proof: None,
        }
    }

    #[test]
    fn well_formed_attestation_by_trusted_signer_is_valid() {
        let target = target_event();
        let signer = DeterministicSigner::from_seed([11u8; 32]);
        let trust = trust_for_actor(&target.writer.actor_id, &signer);

        let carrier = cosign(&target, &signer);

        assert_eq!(
            verify_cosignature(&carrier, &target, &trust).unwrap(),
            CosignatureVerification::Attested(EventVerificationStatus::Valid)
        );
    }

    #[test]
    fn well_formed_attestation_by_unknown_signer_is_untrusted_key() {
        let target = target_event();
        let signer = DeterministicSigner::from_seed([12u8; 32]);

        let carrier = cosign(&target, &signer);

        assert_eq!(
            verify_cosignature(&carrier, &target, &TrustSet::default()).unwrap(),
            CosignatureVerification::Attested(EventVerificationStatus::UntrustedKey)
        );
    }

    #[test]
    fn tampered_attestation_signature_is_invalid() {
        let target = target_event();
        let signer = DeterministicSigner::from_seed([13u8; 32]);
        let trust = trust_for_actor(&target.writer.actor_id, &signer);

        let mut carrier = cosign(&target, &signer);
        // A syntactically valid but wrong signature: 64 zero bytes.
        carrier.attestation =
            EventSignature::ed25519_v1(EventSignatureBytes::from_bytes(&[0u8; 64]));

        assert_eq!(
            verify_cosignature(&carrier, &target, &trust).unwrap(),
            CosignatureVerification::Attested(EventVerificationStatus::Invalid)
        );
    }

    #[test]
    fn malformed_attesting_signer_key_is_invalid() {
        let target = target_event();
        let signer = DeterministicSigner::from_seed([14u8; 32]);

        let mut carrier = cosign(&target, &signer);
        carrier.attesting_signer =
            serde_json::from_value(serde_json::json!("did:key:zNotARealEd25519Key")).unwrap();

        assert_eq!(
            verify_cosignature(&carrier, &target, &TrustSet::default()).unwrap(),
            CosignatureVerification::Attested(EventVerificationStatus::Invalid)
        );
    }

    #[test]
    fn wrong_alg_or_sig_version_on_attestation_is_invalid() {
        let target = target_event();
        let signer = DeterministicSigner::from_seed([15u8; 32]);
        let trust = trust_for_actor(&target.writer.actor_id, &signer);

        let mut wrong_alg = cosign(&target, &signer);
        wrong_alg.attestation.alg = "rsa".to_owned();
        assert_eq!(
            verify_cosignature(&wrong_alg, &target, &trust).unwrap(),
            CosignatureVerification::Attested(EventVerificationStatus::Invalid)
        );

        let mut wrong_version = cosign(&target, &signer);
        wrong_version.attestation.sig_version = 2;
        assert_eq!(
            verify_cosignature(&wrong_version, &target, &trust).unwrap(),
            CosignatureVerification::Attested(EventVerificationStatus::Invalid)
        );
    }

    #[test]
    fn carrier_target_record_hash_mismatch_is_binding_mismatch_not_invalid() {
        let target = target_event();
        let signer = DeterministicSigner::from_seed([16u8; 32]);
        let trust = trust_for_actor(&target.writer.actor_id, &signer);

        let mut carrier = cosign(&target, &signer);
        carrier.target_event_record_hash =
            "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned();

        let result = verify_cosignature(&carrier, &target, &trust).unwrap();
        assert_eq!(result, CosignatureVerification::BindingMismatch);
        assert_ne!(
            result,
            CosignatureVerification::Attested(EventVerificationStatus::Invalid),
            "a bad binding is a different fact, not a malformed signature of this fact"
        );
    }

    #[test]
    fn signer_inclusive_view_is_what_is_signed() {
        let target = target_event();
        let attesting = DeterministicSigner::from_seed([17u8; 32]);
        let other = DeterministicSigner::from_seed([18u8; 32]);
        let trust = trust_for_actor(&target.writer.actor_id, &attesting);

        // A genuine attestation over the attesting signer's own view verifies.
        let genuine = cosign(&target, &attesting);
        assert_eq!(
            verify_cosignature(&genuine, &target, &trust).unwrap(),
            CosignatureVerification::Attested(EventVerificationStatus::Valid)
        );

        // A replay: the attestation was made over a DIFFERENT signer's view, but
        // claims the attesting signer. Reconstructing the attesting signer's view
        // yields different bytes, so it fails as Invalid.
        let other_tbs = EventToBeSigned::from_event(&target, other.signer_id()).unwrap();
        let other_pae = event_signature_pre_authentication_encoding(&other_tbs).unwrap();
        let replayed_sig = attesting.sign_event_message(&other_pae).unwrap();
        let mut replay = cosign(&target, &attesting);
        replay.attestation = EventSignature::ed25519_v1(replayed_sig);

        assert_eq!(
            verify_cosignature(&replay, &target, &trust).unwrap(),
            CosignatureVerification::Attested(EventVerificationStatus::Invalid)
        );
    }

    #[test]
    fn gate_stores_valid_and_untrusted_key() {
        let target = target_event();
        let trusted = DeterministicSigner::from_seed([20u8; 32]);
        let trust = trust_for_actor(&target.writer.actor_id, &trusted);

        let valid = cosign(&target, &trusted);
        assert_eq!(
            gate_cosignature_for_store(&valid, Some(&target), &trust).unwrap(),
            CosignatureGateDecision::Store(EventVerificationStatus::Valid)
        );

        let unknown = DeterministicSigner::from_seed([21u8; 32]);
        let untrusted = cosign(&target, &unknown);
        let decision = gate_cosignature_for_store(&untrusted, Some(&target), &trust).unwrap();
        assert_eq!(
            decision,
            CosignatureGateDecision::Store(EventVerificationStatus::UntrustedKey)
        );
        assert!(decision.stores());
        assert_eq!(decision.drop_diagnostic_code(), None);
    }

    #[test]
    fn gate_drops_invalid_with_code() {
        let target = target_event();
        let signer = DeterministicSigner::from_seed([22u8; 32]);
        let trust = trust_for_actor(&target.writer.actor_id, &signer);

        let mut carrier = cosign(&target, &signer);
        carrier.attestation =
            EventSignature::ed25519_v1(EventSignatureBytes::from_bytes(&[0u8; 64]));

        let decision = gate_cosignature_for_store(&carrier, Some(&target), &trust).unwrap();
        assert_eq!(decision, CosignatureGateDecision::DropInvalid);
        assert!(!decision.stores());
        assert_eq!(
            decision.drop_diagnostic_code(),
            Some(COSIGNATURE_INVALID_CODE)
        );
    }

    #[test]
    fn gate_target_pending_when_absent_then_stores_after_backfill() {
        let target = target_event();
        let signer = DeterministicSigner::from_seed([23u8; 32]);
        let trust = trust_for_actor(&target.writer.actor_id, &signer);
        let carrier = cosign(&target, &signer);

        // Target absent: dropped with cosignature_target_pending, no trace.
        let pending = gate_cosignature_for_store(&carrier, None, &trust).unwrap();
        assert_eq!(pending, CosignatureGateDecision::TargetPending);
        assert_eq!(
            pending.drop_diagnostic_code(),
            Some(COSIGNATURE_TARGET_PENDING_CODE)
        );

        // The SAME carrier writes cleanly once the target is present — replay-safe
        // without any replay scheduler.
        assert_eq!(
            gate_cosignature_for_store(&carrier, Some(&target), &trust).unwrap(),
            CosignatureGateDecision::Store(EventVerificationStatus::Valid)
        );
    }

    #[test]
    fn gate_binding_mismatch_distinct_from_invalid() {
        let target = target_event();
        let signer = DeterministicSigner::from_seed([24u8; 32]);
        let trust = trust_for_actor(&target.writer.actor_id, &signer);

        let mut carrier = cosign(&target, &signer);
        carrier.target_event_record_hash =
            "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned();

        let decision = gate_cosignature_for_store(&carrier, Some(&target), &trust).unwrap();
        assert_eq!(decision, CosignatureGateDecision::BindingMismatch);
        assert_ne!(decision, CosignatureGateDecision::DropInvalid);
        assert_eq!(
            decision.drop_diagnostic_code(),
            Some(COSIGNATURE_BINDING_MISMATCH_CODE)
        );
    }
}
