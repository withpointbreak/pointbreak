use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Process-local signer for opaque Inspector paging tokens.
///
/// Token schemas and semantic binding remain owned by each route. This type
/// supplies only the shared authenticated-envelope mechanism, so Change and
/// Timeline paging cannot accidentally share cursor meaning.
pub(super) struct PageTokenSigner(SigningKey);

impl PageTokenSigner {
    pub(super) fn generate() -> Result<Self, getrandom::Error> {
        let mut seed = [0_u8; 32];
        getrandom::fill(&mut seed)?;
        Ok(Self(SigningKey::from_bytes(&seed)))
    }

    #[cfg(test)]
    pub(super) fn from_seed(seed: [u8; 32]) -> Self {
        Self(SigningKey::from_bytes(&seed))
    }

    pub(super) fn encode<T: Serialize>(&self, token: &T) -> String {
        let payload = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(token).expect("Inspector page token must serialize"));
        let signature = self.0.sign(payload.as_bytes());
        format!("{payload}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
    }

    pub(super) fn decode<T: DeserializeOwned>(&self, raw: &str) -> Result<T, ()> {
        if raw.len() > 4096 {
            return Err(());
        }
        let (payload, encoded_signature) = raw
            .split_once('.')
            .filter(|(_, signature)| !signature.contains('.'))
            .ok_or(())?;
        let signature_bytes = URL_SAFE_NO_PAD.decode(encoded_signature).map_err(|_| ())?;
        let signature = Signature::from_slice(&signature_bytes).map_err(|_| ())?;
        self.0
            .verifying_key()
            .verify(payload.as_bytes(), &signature)
            .map_err(|_| ())?;
        let bytes = URL_SAFE_NO_PAD.decode(payload).map_err(|_| ())?;
        serde_json::from_slice(&bytes).map_err(|_| ())
    }
}
