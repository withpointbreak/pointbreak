mod did_key;
mod ed25519;

pub use did_key::SignerId;
#[cfg(test)]
pub(crate) use ed25519::TestEd25519Signer;
pub use ed25519::{
    EventSignatureBytes, EventSigner, EventVerificationStatus, verify_ed25519_strict,
};
