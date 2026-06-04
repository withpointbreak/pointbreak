use std::fmt;
use std::sync::Arc;

use crate::crypto::{EventSigner, SignerId};
use crate::error::Result;
use crate::session::event::{
    EventSignature, EventToBeSigned, ShoreEvent, event_signature_pre_authentication_encoding,
};

type SharedEventSigner = Arc<dyn EventSigner + Send + Sync>;

#[derive(Clone, Default)]
pub struct EventSigningOptions {
    signer_id: Option<SignerId>,
    signer: Option<SharedEventSigner>,
}

impl EventSigningOptions {
    pub fn sign_with<S>(signer: S) -> Self
    where
        S: EventSigner + Send + Sync + 'static,
    {
        let signer_id = signer.signer_id().clone();

        Self {
            signer_id: Some(signer_id),
            signer: Some(Arc::new(signer)),
        }
    }

    fn signer(&self) -> Option<&SharedEventSigner> {
        self.signer.as_ref()
    }
}

impl fmt::Debug for EventSigningOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventSigningOptions")
            .field("signer_id", &self.signer_id)
            .finish_non_exhaustive()
    }
}

impl PartialEq for EventSigningOptions {
    fn eq(&self, other: &Self) -> bool {
        self.signer_id == other.signer_id
    }
}

impl Eq for EventSigningOptions {}

pub(crate) fn sign_event_if_requested(
    event: &mut ShoreEvent,
    signing: &EventSigningOptions,
) -> Result<()> {
    let Some(signer) = signing.signer() else {
        return Ok(());
    };

    let signer_id = signer.signer_id();
    let signing_view = EventToBeSigned::from_event(event, signer_id)?;
    let message = event_signature_pre_authentication_encoding(&signing_view)?;
    let signature = signer.sign_event_message(&message)?;

    if event.writer.actor_id.as_str() == signer_id.as_str() {
        event.signer = None;
    } else {
        event.signer = Some(signer_id.clone());
    }
    event.signature = Some(EventSignature::ed25519_v1(signature));

    Ok(())
}
