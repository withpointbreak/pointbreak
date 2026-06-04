use serde::{Deserialize, Serialize};

use crate::crypto::EventVerificationStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactAvailability {
    Available,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventVerificationView {
    pub verification_status: EventVerificationStatus,
    pub artifact_availability: ArtifactAvailability,
}

pub fn verification_view(
    verification_status: EventVerificationStatus,
    artifact_availability: ArtifactAvailability,
) -> EventVerificationView {
    EventVerificationView {
        verification_status,
        artifact_availability,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventVerificationPolicy {
    pub reject_invalid_signatures: bool,
    pub require_trusted_signer: bool,
    pub allow_unsigned: bool,
}

impl EventVerificationPolicy {
    pub fn advisory() -> Self {
        Self {
            reject_invalid_signatures: false,
            require_trusted_signer: false,
            allow_unsigned: true,
        }
    }

    pub fn integrity_strict() -> Self {
        Self {
            reject_invalid_signatures: true,
            require_trusted_signer: false,
            allow_unsigned: true,
        }
    }

    pub fn trusted_strict() -> Self {
        Self {
            reject_invalid_signatures: true,
            require_trusted_signer: true,
            allow_unsigned: false,
        }
    }

    pub fn with_allow_unsigned(mut self, allow_unsigned: bool) -> Self {
        self.allow_unsigned = allow_unsigned;
        self
    }

    pub fn rejects(&self, status: EventVerificationStatus) -> bool {
        match status {
            EventVerificationStatus::Valid => false,
            EventVerificationStatus::Invalid => self.reject_invalid_signatures,
            EventVerificationStatus::UntrustedKey => self.require_trusted_signer,
            EventVerificationStatus::Unsigned => !self.allow_unsigned,
        }
    }
}
