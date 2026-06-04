use serde::{Deserialize, Serialize};

use crate::error::{Result, ShoreError};

const DID_KEY_PREFIX: &str = "did:key:";
const BASE58_BTC_PREFIX: char = 'z';
const ED25519_MULTICODEC_PREFIX: [u8; 2] = [0xed, 0x01];
const ED25519_PUBLIC_KEY_LEN: usize = 32;
const ED25519_DID_KEY_BYTES_LEN: usize = ED25519_MULTICODEC_PREFIX.len() + ED25519_PUBLIC_KEY_LEN;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SignerId(String);

impl SignerId {
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        decode_ed25519_public_key(&value)?;
        Ok(Self(value))
    }

    pub fn from_ed25519_public_key(bytes: [u8; ED25519_PUBLIC_KEY_LEN]) -> Self {
        let mut payload = Vec::with_capacity(ED25519_DID_KEY_BYTES_LEN);
        payload.extend_from_slice(&ED25519_MULTICODEC_PREFIX);
        payload.extend_from_slice(&bytes);
        Self(format!(
            "{DID_KEY_PREFIX}{BASE58_BTC_PREFIX}{}",
            bs58::encode(payload).into_string()
        ))
    }

    pub fn ed25519_public_key(&self) -> Result<[u8; ED25519_PUBLIC_KEY_LEN]> {
        decode_ed25519_public_key(&self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn decode_ed25519_public_key(value: &str) -> Result<[u8; ED25519_PUBLIC_KEY_LEN]> {
    let encoded = value
        .strip_prefix(DID_KEY_PREFIX)
        .ok_or_else(|| invalid_did_key("unsupported DID method"))?;
    let multibase = encoded
        .strip_prefix(BASE58_BTC_PREFIX)
        .ok_or_else(|| invalid_did_key("missing base58btc multibase prefix"))?;
    if multibase.is_empty() {
        return Err(invalid_did_key("empty base58btc payload"));
    }

    let decoded = bs58::decode(multibase)
        .into_vec()
        .map_err(|_| invalid_did_key("malformed base58btc payload"))?;
    if decoded.len() != ED25519_DID_KEY_BYTES_LEN {
        return Err(invalid_did_key("unexpected Ed25519 did:key byte length"));
    }
    if !decoded.starts_with(&ED25519_MULTICODEC_PREFIX) {
        return Err(invalid_did_key("unsupported did:key multicodec"));
    }

    let key = decoded[ED25519_MULTICODEC_PREFIX.len()..]
        .try_into()
        .expect("length checked above");
    Ok(key)
}

fn invalid_did_key(message: impl Into<String>) -> ShoreError {
    ShoreError::WorkflowInputInvalid {
        reason: format!("invalid Ed25519 did:key: {}", message.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn did_key_ed25519_round_trips_public_key_bytes() {
        let public_key = [1u8; 32];
        let signer = SignerId::from_ed25519_public_key(public_key);

        assert!(signer.as_str().starts_with("did:key:z6Mk"));
        assert_eq!(signer.ed25519_public_key().unwrap(), public_key);
    }

    #[test]
    fn did_key_rejects_unsupported_or_malformed_values() {
        let unsupported_multicodec = format!(
            "did:key:z{}",
            bs58::encode([[0x00, 0x01].as_slice(), &[1u8; 32]].concat()).into_string()
        );
        let wrong_key_length = format!(
            "did:key:z{}",
            bs58::encode([[0xed, 0x01].as_slice(), &[1u8; 31]].concat()).into_string()
        );

        for bad in [
            "did:web:example.com".to_owned(),
            "did:key:".to_owned(),
            "did:key:not-multibase".to_owned(),
            "did:key:zbad".to_owned(),
            unsupported_multicodec,
            wrong_key_length,
        ] {
            assert!(SignerId::parse(&bad).is_err(), "{bad:?} must reject");
        }
    }

    #[test]
    fn did_key_fixture_decodes_to_expected_public_key_bytes() {
        let signer = SignerId::parse("did:key:z6MkehRgf7yJbgaGfYsdoAsKdBPE3dj2CYhowQdcjqSJgvVd")
            .expect("fixture did:key parses");

        assert_eq!(
            signer.ed25519_public_key().unwrap(),
            [
                0x03, 0xa1, 0x07, 0xbf, 0xf3, 0xce, 0x10, 0xbe, 0x1d, 0x70, 0xdd, 0x18, 0xe7, 0x4b,
                0xc0, 0x99, 0x67, 0xe4, 0xd6, 0x30, 0x9b, 0xa5, 0x0d, 0x5f, 0x1d, 0xdc, 0x86, 0x64,
                0x12, 0x55, 0x31, 0xb8,
            ]
        );
    }
}
