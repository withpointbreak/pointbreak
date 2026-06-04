use super::{EventVerificationPolicy, TrustSet, verify_event_signature};
use crate::crypto::EventVerificationStatus;
use crate::error::{Result, ShoreError};
use crate::model::EventId;
use crate::session::event::ShoreEvent;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestEventVerification {
    pub event_id: EventId,
    pub status: EventVerificationStatus,
    pub message: Option<String>,
}

pub(crate) fn verify_events_for_ingest(
    events: &[ShoreEvent],
    policy: EventVerificationPolicy,
    trust_set: &TrustSet,
) -> Result<Vec<IngestEventVerification>> {
    let mut verification = Vec::with_capacity(events.len());

    for event in events {
        let status = verify_event_signature(event, trust_set)?;
        let report = IngestEventVerification {
            event_id: event.event_id.clone(),
            status,
            message: verification_message(status),
        };

        if policy.rejects(status) {
            return Err(ShoreError::WorkflowInputInvalid {
                reason: format!(
                    "event signature verification rejected event {} with status {}",
                    event.event_id.as_str(),
                    status_code(status)
                ),
            });
        }

        verification.push(report);
    }

    Ok(verification)
}

fn verification_message(status: EventVerificationStatus) -> Option<String> {
    match status {
        EventVerificationStatus::Valid => None,
        EventVerificationStatus::Invalid => Some("event signature is invalid".to_owned()),
        EventVerificationStatus::UntrustedKey => {
            Some("event signer is not authorized by the trust set".to_owned())
        }
        EventVerificationStatus::Unsigned => Some("event is unsigned".to_owned()),
    }
}

fn status_code(status: EventVerificationStatus) -> &'static str {
    match status {
        EventVerificationStatus::Valid => "valid",
        EventVerificationStatus::Invalid => "invalid",
        EventVerificationStatus::UntrustedKey => "untrusted_key",
        EventVerificationStatus::Unsigned => "unsigned",
    }
}
