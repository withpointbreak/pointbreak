mod did_key;
mod ed25519;

pub use did_key::SignerId;
pub use ed25519::{
    EventSignatureBytes, EventSigner, EventVerificationStatus, verify_ed25519_strict,
};
