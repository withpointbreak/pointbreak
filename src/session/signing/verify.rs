use super::TrustSet;
use crate::crypto::{EventVerificationStatus, verify_ed25519_strict};
use crate::error::Result;
use crate::session::event::{
    ShoreEvent, event_signature_pre_authentication_encoding, event_to_be_signed,
};

const ED25519_SIGNATURE_ALG: &str = "ed25519";
const EVENT_SIGNATURE_VERSION: u32 = 1;

pub fn verify_event_signature(
    event: &ShoreEvent,
    trust: &TrustSet,
) -> Result<EventVerificationStatus> {
    let Some(signature) = &event.signature else {
        return Ok(EventVerificationStatus::Unsigned);
    };

    if signature.alg != ED25519_SIGNATURE_ALG || signature.sig_version != EVENT_SIGNATURE_VERSION {
        return Ok(EventVerificationStatus::Invalid);
    }

    let tbs = match event_to_be_signed(event) {
        Ok(tbs) => tbs,
        Err(error) => return Ok(error.status()),
    };
    let message = match event_signature_pre_authentication_encoding(&tbs) {
        Ok(message) => message,
        Err(_) => return Ok(EventVerificationStatus::Invalid),
    };
    let status = verify_ed25519_strict(&tbs.signer, &message, signature.sig.as_str())?;
    if status != EventVerificationStatus::Valid {
        return Ok(status);
    }

    if trust.authorizes(
        &event.writer.actor_id,
        &tbs.signer,
        event.occurred_at.as_str(),
    ) {
        Ok(EventVerificationStatus::Valid)
    } else {
        Ok(EventVerificationStatus::UntrustedKey)
    }
}
