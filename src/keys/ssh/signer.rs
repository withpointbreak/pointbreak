use crate::crypto::{EventSignatureBytes, EventSigner, SignerId};
use crate::error::{Result, ShoreError};
use crate::keys::ssh::protocol::{
    ed25519_key_blob, parse_identities_answer, parse_sign_response, request_identities_bytes,
    sign_request_bytes,
};
use crate::keys::ssh::transport::connect_agent;

/// Cap on a single agent message so a corrupt or hostile length prefix cannot
/// drive an unbounded allocation. Real agent replies (identities list, a single
/// signature) are far smaller than this.
const MAX_AGENT_FRAME: usize = 1 << 20;

/// The minimal duplex the signer needs: a framed-message `Read + Write`. The
/// stream-framing helpers ride on the trait (rather than `protocol.rs`) so they
/// are callable directly on a `Box<dyn AgentDuplex>` without a trait-object upcast.
pub(crate) trait AgentDuplex: std::io::Read + std::io::Write {
    /// Write a fully-framed agent message (the outer length prefix is already
    /// part of `frame`).
    fn send_frame(&mut self, frame: &[u8]) -> Result<()> {
        self.write_all(frame).map_err(stream_error)?;
        self.flush().map_err(stream_error)?;
        Ok(())
    }

    /// Read one framed agent message: the 4-byte outer length prefix then exactly
    /// that many bytes, returning the whole frame (prefix included) the codec parses.
    fn recv_frame(&mut self) -> Result<Vec<u8>> {
        let mut len_buf = [0u8; 4];
        self.read_exact(&mut len_buf).map_err(stream_error)?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > MAX_AGENT_FRAME {
            return Err(ShoreError::Message(
                "ssh-agent reply exceeds the maximum message size".to_owned(),
            ));
        }
        let mut frame = vec![0u8; 4 + len];
        frame[..4].copy_from_slice(&len_buf);
        self.read_exact(&mut frame[4..]).map_err(stream_error)?;
        Ok(frame)
    }
}

impl<T: std::io::Read + std::io::Write> AgentDuplex for T {}

/// A connector yields a fresh duplex to the agent. Production connects via the
/// platform transport; tests inject a fake-agent duplex through the same seam.
type AgentConnector = Box<dyn Fn() -> Result<Box<dyn AgentDuplex>> + Send + Sync>;

/// Connect through the platform transport, type-erased to the duplex the signer
/// drives.
fn real_connector() -> Result<Box<dyn AgentDuplex>> {
    connect_agent().map(|stream| Box::new(stream) as Box<dyn AgentDuplex>)
}

/// The second production `EventSigner`: signs by shipping the DSSE PAE bytes to a
/// running ssh-agent and unwrapping the SSH-wire signature to the raw 64 bytes.
///
/// Unlike the file-backed signer — whose `sign_event_message` is infallible
/// because the key is loaded and the crypto is local — this signer's
/// `sign_event_message` performs a NETWORK round-trip that CAN fail (agent
/// unavailable, locked, key no longer held). That fallibility is deliberate and
/// load-bearing: the `Err` is surfaced here, not swallowed. The resolve layer's
/// identities-only pre-flight catches the obvious failures up front, and the
/// write seam's best-effort degrade turns a sign-time failure into an unsigned
/// write (exit 0) rather than a gated one — so signing never gates even for a
/// network-backed signer. There is deliberately NO retry and NO fallback-to-file
/// path here; degradation is the resolve layer's job, not the signer's.
///
/// The agent custodies the private key — it is never read here. The bytes are
/// sent raw (flags = 0, no SSHSIG wrapper); the DSSE PAE prefix supplies domain
/// separation.
// `pub`: the binary CLI crate receives this from the resolver as a boxed
// `dyn EventSigner` and threads it into `.sign_with(...)`.
pub struct SshAgentSigner {
    signer_id: SignerId,
    public_key: [u8; 32],
    connect: AgentConnector,
}

impl SshAgentSigner {
    /// Production constructor: connect via the platform transport.
    pub(crate) fn new(signer_id: SignerId, public_key: [u8; 32]) -> Self {
        Self {
            signer_id,
            public_key,
            connect: Box::new(real_connector),
        }
    }

    /// Seam constructor: inject a connector (e.g. a fake-agent duplex) for tests.
    #[cfg(test)]
    pub(crate) fn with_connector(
        signer_id: SignerId,
        public_key: [u8; 32],
        connect: impl Fn() -> Result<Box<dyn AgentDuplex>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            signer_id,
            public_key,
            connect: Box::new(connect),
        }
    }
}

impl EventSigner for SshAgentSigner {
    fn signer_id(&self) -> &SignerId {
        &self.signer_id
    }

    fn sign_event_message(&self, message: &[u8]) -> Result<EventSignatureBytes> {
        // Fallible by design: connect → write a SIGN_REQUEST → read + unwrap the
        // SSH-wire signature. Any failure propagates as `Err` (kept off the write
        // path by the resolve-layer pre-flight, degraded by the write seam).
        let mut stream = (self.connect)()?;
        stream.send_frame(&sign_request_bytes(&self.public_key, message))?;
        let response = stream.recv_frame()?;
        parse_sign_response(&response)
    }
}

/// Why an agent-backed signer could not be resolved at pre-flight. `Socket` = no
/// or unreachable agent; `KeyAbsent` = the target key is not in the identities
/// list (which also covers a globally-locked agent that lists zero).
pub enum AgentUnavailable {
    Socket,
    KeyAbsent,
}

/// Identities-only pre-flight (NO probe-sign): connect, confirm the key is listed,
/// and return a ready signer. The resolver calls this and maps the typed error to
/// a named diagnostic. A probe-sign is deliberately avoided so a confirmation or
/// hardware agent is never prompted at resolve time.
pub fn preflight_ssh_agent_signer(
    public_key: [u8; 32],
) -> std::result::Result<SshAgentSigner, AgentUnavailable> {
    preflight_with(public_key, real_connector)
}

/// Best-effort probe for a listing's loaded-in-agent status: `Some(true)` =
/// loaded, `Some(false)` = agent reachable but the key is absent, `None` =
/// unknown (no/unreachable/locked agent). Swallows every error into `None`; never
/// panics and never gates a read.
pub fn agent_has_key(public_key: [u8; 32]) -> Option<bool> {
    agent_has_key_with(public_key, real_connector)
}

/// Pre-flight over an injected connector, so the fake agent can drive it in-crate.
pub(crate) fn preflight_with(
    public_key: [u8; 32],
    connect: impl Fn() -> Result<Box<dyn AgentDuplex>>,
) -> std::result::Result<SshAgentSigner, AgentUnavailable> {
    let blobs = list_identities(&connect).map_err(|_| AgentUnavailable::Socket)?;
    if !blobs.contains(&ed25519_key_blob(&public_key)) {
        return Err(AgentUnavailable::KeyAbsent);
    }
    Ok(SshAgentSigner::new(
        SignerId::from_ed25519_public_key(public_key),
        public_key,
    ))
}

/// Loaded-in-agent probe over an injected connector. Any error → `None`.
pub(crate) fn agent_has_key_with(
    public_key: [u8; 32],
    connect: impl Fn() -> Result<Box<dyn AgentDuplex>>,
) -> Option<bool> {
    let blobs = list_identities(&connect).ok()?;
    Some(blobs.contains(&ed25519_key_blob(&public_key)))
}

/// Connect and list the agent's identities (no sign). Shared by both pre-flight
/// helpers so neither reaches the codec/transport plumbing directly.
fn list_identities(connect: &impl Fn() -> Result<Box<dyn AgentDuplex>>) -> Result<Vec<Vec<u8>>> {
    let mut stream = connect()?;
    stream.send_frame(&request_identities_bytes())?;
    let answer = stream.recv_frame()?;
    parse_identities_answer(&answer)
}

fn stream_error(error: std::io::Error) -> ShoreError {
    ShoreError::Message(format!("ssh-agent stream error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{EventVerificationStatus, SignerId, verify_ed25519_strict};
    use crate::keys::ssh::protocol::fake::FakeSshAgent;

    const SEED: [u8; 32] = [42_u8; 32];

    fn pubkey() -> [u8; 32] {
        ed25519_dalek::SigningKey::from_bytes(&SEED)
            .verifying_key()
            .to_bytes()
    }
    fn expected_signer() -> SignerId {
        SignerId::from_ed25519_public_key(pubkey())
    }

    fn duplex_connector(
        make: impl Fn() -> FakeSshAgent + Send + Sync + 'static,
    ) -> impl Fn() -> Result<Box<dyn AgentDuplex>> + Send + Sync {
        move || Ok(Box::new(make().into_duplex()) as Box<dyn AgentDuplex>)
    }

    #[test]
    fn sign_event_message_round_trips_and_verifies_strict() {
        // A signer whose connector hands back a duplex backed by a fake agent that
        // real-signs with SEED. No live agent, no socket.
        let signer = SshAgentSigner::with_connector(
            expected_signer(),
            pubkey(),
            duplex_connector(|| FakeSshAgent::with_key(SEED)),
        );

        let message = crate::session::event::pre_authentication_encoding(
            crate::session::event::EVENT_TO_BE_SIGNED_V1_PAYLOAD_TYPE,
            br#"{"schema":"shore.event","version":1}"#,
        );

        let sig = signer.sign_event_message(&message).unwrap();
        assert!(sig.is_base64());
        assert_eq!(
            verify_ed25519_strict(signer.signer_id(), &message, sig.as_str()).unwrap(),
            EventVerificationStatus::Valid
        );
    }

    #[test]
    fn signer_id_is_the_parsed_did_key() {
        let signer = SshAgentSigner::with_connector(
            expected_signer(),
            pubkey(),
            duplex_connector(|| FakeSshAgent::with_key(SEED)),
        );
        assert_eq!(signer.signer_id(), &expected_signer());
    }

    #[test]
    fn a_locked_agent_surfaces_as_err_from_sign_event_message() {
        // CONTRAST to the file signer: this signer's sign call is fallible. A
        // locked agent answers SIGN with FAILURE; sign_event_message returns Err.
        let signer = SshAgentSigner::with_connector(
            expected_signer(),
            pubkey(),
            duplex_connector(|| FakeSshAgent::with_key(SEED).locked()),
        );
        assert!(signer.sign_event_message(b"DSSEv1 4 test 5 hi").is_err());
    }

    #[test]
    fn a_connect_failure_surfaces_as_err_from_sign_event_message() {
        // The connector itself failing (agent unavailable) is also an Err, not a
        // panic — the same fallibility the pre-flight is built around.
        let signer = SshAgentSigner::with_connector(expected_signer(), pubkey(), || {
            Err(ShoreError::Message("no agent".to_owned()))
        });
        assert!(signer.sign_event_message(b"DSSEv1 4 test 5 hi").is_err());
    }

    #[test]
    fn preflight_returns_a_signer_for_a_loaded_key_without_probing_sign() {
        // A refuses-sign agent LISTS the key but FAILS any sign. Pre-flight
        // returning Ok proves it resolves on the identities list alone and never
        // depends on (probes) a sign.
        let result = preflight_with(
            pubkey(),
            duplex_connector(|| FakeSshAgent::with_key(SEED).refuses_sign()),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn preflight_reports_key_absent_for_an_empty_or_locked_agent() {
        let absent = preflight_with(pubkey(), duplex_connector(FakeSshAgent::empty));
        assert!(matches!(absent, Err(AgentUnavailable::KeyAbsent)));

        let locked = preflight_with(
            pubkey(),
            duplex_connector(|| FakeSshAgent::with_key(SEED).locked()),
        );
        assert!(matches!(locked, Err(AgentUnavailable::KeyAbsent)));
    }

    #[test]
    fn preflight_reports_socket_on_a_connect_failure() {
        let result = preflight_with(pubkey(), || Err(ShoreError::Message("no agent".to_owned())));
        assert!(matches!(result, Err(AgentUnavailable::Socket)));
    }

    #[test]
    fn agent_has_key_distinguishes_loaded_absent_and_unknown() {
        let loaded =
            agent_has_key_with(pubkey(), duplex_connector(|| FakeSshAgent::with_key(SEED)));
        assert_eq!(loaded, Some(true));

        let absent = agent_has_key_with(pubkey(), duplex_connector(FakeSshAgent::empty));
        assert_eq!(absent, Some(false));

        let unknown =
            agent_has_key_with(pubkey(), || Err(ShoreError::Message("no agent".to_owned())));
        assert_eq!(unknown, None);
    }
}
