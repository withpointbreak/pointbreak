use crate::crypto::EventSignatureBytes;
use crate::error::{Result, ShoreError};

// ssh-agent message types (the subset this client uses; draft-miller-ssh-agent).
const SSH_AGENT_FAILURE: u8 = 5;
const SSH_AGENTC_REQUEST_IDENTITIES: u8 = 11;
const SSH_AGENT_IDENTITIES_ANSWER: u8 = 12;
const SSH_AGENTC_SIGN_REQUEST: u8 = 13;
const SSH_AGENT_SIGN_RESPONSE: u8 = 14;

const SSH_ED25519_TYPE: &[u8] = b"ssh-ed25519";
const ED25519_SIGNATURE_LEN: usize = 64;

// ---- framing -------------------------------------------------------------

/// Append an SSH-wire `string`: `u32` BE length prefix, then `value`.
/// Shared with the public-key parser so wire `string` framing lives in one place.
pub(crate) fn put_ssh_string(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u32).to_be_bytes());
    out.extend_from_slice(value);
}

/// Read one SSH-wire `string`, advancing `cursor`. Returns `None` on a truncated
/// frame. Shared by the agent codec and the public-key parser; each caller wraps
/// a `None` in its own error type (a bad public key is invalid input; a bad agent
/// reply is a protocol error).
pub(crate) fn read_ssh_string<'a>(cursor: &mut &'a [u8]) -> Option<&'a [u8]> {
    if cursor.len() < 4 {
        return None;
    }
    let (len_bytes, rest) = cursor.split_at(4);
    let len = u32::from_be_bytes(len_bytes.try_into().expect("4 bytes")) as usize;
    if rest.len() < len {
        return None;
    }
    let (value, rest) = rest.split_at(len);
    *cursor = rest;
    Some(value)
}

/// Read one SSH-wire `string` or fail with a protocol error.
fn read_agent_string<'a>(cursor: &mut &'a [u8]) -> Result<&'a [u8]> {
    read_ssh_string(cursor).ok_or_else(|| agent_protocol("truncated SSH string"))
}

/// Read a `u32` big-endian count, advancing `cursor`.
fn read_u32(cursor: &mut &[u8]) -> Result<u32> {
    if cursor.len() < 4 {
        return Err(agent_protocol("truncated u32"));
    }
    let (head, rest) = cursor.split_at(4);
    *cursor = rest;
    Ok(u32::from_be_bytes(head.try_into().expect("4 bytes")))
}

/// Wrap a message body (`<type> <body>`) in the outer agent length prefix.
fn frame_message(body: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// Strip the outer agent length prefix, returning the message body
/// (`<type> <body>`). Validates the declared length matches.
fn unframe_message(frame: &[u8]) -> Result<&[u8]> {
    let mut cursor = frame;
    let body = read_ssh_string(&mut cursor)
        .ok_or_else(|| agent_protocol("truncated agent message frame"))?;
    if !cursor.is_empty() {
        return Err(agent_protocol("trailing bytes after agent message"));
    }
    Ok(body)
}

// ---- key blob ------------------------------------------------------------

/// The SSH-wire Ed25519 key blob: `string "ssh-ed25519"` + `string <32 bytes>`.
/// Byte-identical to the blob the public-key parser recovers.
pub(crate) fn ed25519_key_blob(public_key: &[u8; 32]) -> Vec<u8> {
    let mut blob = Vec::new();
    put_ssh_string(&mut blob, SSH_ED25519_TYPE);
    put_ssh_string(&mut blob, public_key);
    blob
}

// ---- requests ------------------------------------------------------------

/// Build a `SSH_AGENTC_REQUEST_IDENTITIES` message (no body).
pub(crate) fn request_identities_bytes() -> Vec<u8> {
    frame_message(vec![SSH_AGENTC_REQUEST_IDENTITIES])
}

/// Build a `SSH_AGENTC_SIGN_REQUEST` for `public_key` over `data`:
/// `<13> string key-blob, string data, u32 flags`. `data` is the DSSE PAE bytes
/// VERBATIM; flags = 0 (no SSHSIG wrapper, no RSA-SHA2 flags).
pub(crate) fn sign_request_bytes(public_key: &[u8; 32], data: &[u8]) -> Vec<u8> {
    let mut body = vec![SSH_AGENTC_SIGN_REQUEST];
    put_ssh_string(&mut body, &ed25519_key_blob(public_key));
    put_ssh_string(&mut body, data);
    body.extend_from_slice(&0_u32.to_be_bytes()); // flags = 0
    frame_message(body)
}

// ---- responses -----------------------------------------------------------

/// Parse a `SSH_AGENT_IDENTITIES_ANSWER` into the list of key blobs the agent
/// holds. A `SSH_AGENT_FAILURE` here is surfaced as a typed agent condition.
pub(crate) fn parse_identities_answer(frame: &[u8]) -> Result<Vec<Vec<u8>>> {
    let body = unframe_message(frame)?;
    let (&msg_type, mut cursor) = body
        .split_first()
        .ok_or_else(|| agent_protocol("empty agent message body"))?;
    if msg_type == SSH_AGENT_FAILURE {
        return Err(agent_failure("agent refused to list identities"));
    }
    if msg_type != SSH_AGENT_IDENTITIES_ANSWER {
        return Err(agent_protocol("unexpected reply to identities request"));
    }
    let nkeys = read_u32(&mut cursor)?;
    // Don't pre-reserve against an untrusted count; grow as the keys are read.
    let mut blobs = Vec::new();
    for _ in 0..nkeys {
        let blob = read_agent_string(&mut cursor)?.to_vec();
        let _comment = read_agent_string(&mut cursor)?;
        blobs.push(blob);
    }
    Ok(blobs)
}

/// Parse a `SSH_AGENT_SIGN_RESPONSE` and UNWRAP its inner SSH-wire signature
/// (`string "ssh-ed25519", string <64 bytes>`) to the bare 64 signature bytes,
/// returned base64-encoded as `EventSignatureBytes`. A `SSH_AGENT_FAILURE`
/// (e.g. a locked agent, or a per-key sign refusal) is surfaced as a typed agent
/// condition the write seam degrades on.
pub(crate) fn parse_sign_response(frame: &[u8]) -> Result<EventSignatureBytes> {
    let body = unframe_message(frame)?;
    let (&msg_type, mut cursor) = body
        .split_first()
        .ok_or_else(|| agent_protocol("empty agent message body"))?;
    if msg_type == SSH_AGENT_FAILURE {
        return Err(agent_failure(
            "agent refused to sign (locked or no matching key)",
        ));
    }
    if msg_type != SSH_AGENT_SIGN_RESPONSE {
        return Err(agent_protocol("unexpected reply to sign request"));
    }
    let signature_blob = read_agent_string(&mut cursor)?;

    // Unwrap the inner SSH-wire signature.
    let mut inner = signature_blob;
    let inner_type = read_agent_string(&mut inner)?;
    if inner_type != SSH_ED25519_TYPE {
        return Err(agent_protocol("signature is not an ssh-ed25519 signature"));
    }
    let raw = read_agent_string(&mut inner)?;
    if raw.len() != ED25519_SIGNATURE_LEN {
        return Err(agent_protocol("ssh-ed25519 signature is not 64 bytes"));
    }
    Ok(EventSignatureBytes::from_bytes(raw))
}

// ---- typed conditions ----------------------------------------------------

/// A malformed/unexpected wire message (a real protocol error).
fn agent_protocol(message: impl Into<String>) -> ShoreError {
    ShoreError::Message(format!("ssh-agent protocol error: {}", message.into()))
}

/// The agent answered FAILURE — locked, or no matching key. The resolve and
/// write layers map this to a named never-gates diagnostic; this keeps it typed
/// and distinguishable from a wire/protocol error.
fn agent_failure(message: impl Into<String>) -> ShoreError {
    ShoreError::Message(format!("ssh-agent failure: {}", message.into()))
}

/// An in-process ssh-agent that speaks the wire subset over byte buffers — no
/// socket, no `$SSH_AUTH_SOCK`. It real-signs with a known seed so unwrapped
/// signatures verify under `verify_ed25519_strict`. Reused across the signer
/// round-trip and the resolve/integration pins, so it is not re-implemented per
/// area.
#[cfg(test)]
pub(crate) mod fake {
    use std::io::{self, Read, Write};

    use ed25519_dalek::{Signer as _, SigningKey};

    use super::*;

    pub(crate) struct FakeSshAgent {
        key: Option<SigningKey>,
        locked: bool,
        refuse_sign: bool,
    }

    impl FakeSshAgent {
        pub(crate) fn with_key(seed: [u8; 32]) -> Self {
            Self {
                key: Some(SigningKey::from_bytes(&seed)),
                locked: false,
                refuse_sign: false,
            }
        }
        pub(crate) fn empty() -> Self {
            Self {
                key: None,
                locked: false,
                refuse_sign: false,
            }
        }
        /// A globally-locked agent (models `ssh-add -x`): lists NO identities and
        /// answers SIGN with FAILURE. Because the identities-only pre-flight checks
        /// the identities list first, a locked agent surfaces there as key-absent —
        /// it never reaches the sign.
        pub(crate) fn locked(mut self) -> Self {
            self.locked = true;
            self
        }
        /// An agent that LISTS the key but FAILS the sign (models a per-key
        /// confirmation deny, or the agent dying/locking in the resolve→sign
        /// window). This is the case the sign-time degrade catches: pre-flight
        /// passes (the key is listed), the real sign returns FAILURE.
        pub(crate) fn refuses_sign(mut self) -> Self {
            self.refuse_sign = true;
            self
        }

        /// Wrap this agent as an in-memory `Read + Write` duplex a transport-shaped
        /// consumer can drive (write a framed request, read the framed reply).
        pub(crate) fn into_duplex(self) -> FakeAgentDuplex {
            FakeAgentDuplex::new(self)
        }

        /// Process one framed client message and return one framed reply — the
        /// exact byte contract the real transport carries.
        pub(crate) fn respond(&self, request: &[u8]) -> Vec<u8> {
            let body = unframe_message(request).expect("test request is framed");
            match body.first().copied() {
                Some(SSH_AGENTC_REQUEST_IDENTITIES) => self.identities_answer(),
                Some(SSH_AGENTC_SIGN_REQUEST) => self.sign_answer(body),
                _ => frame_message(vec![SSH_AGENT_FAILURE]),
            }
        }

        fn identities_answer(&self) -> Vec<u8> {
            let mut out = vec![SSH_AGENT_IDENTITIES_ANSWER];
            match (self.locked, &self.key) {
                (false, Some(key)) => {
                    out.extend_from_slice(&1_u32.to_be_bytes());
                    put_ssh_string(&mut out, &ed25519_key_blob(&key.verifying_key().to_bytes()));
                    put_ssh_string(&mut out, b"fake-agent-key");
                }
                _ => out.extend_from_slice(&0_u32.to_be_bytes()),
            }
            frame_message(out)
        }

        fn sign_answer(&self, body: &[u8]) -> Vec<u8> {
            if self.locked || self.refuse_sign {
                return frame_message(vec![SSH_AGENT_FAILURE]);
            }
            let Some(key) = &self.key else {
                return frame_message(vec![SSH_AGENT_FAILURE]);
            };
            // Parse `<13> string key-blob, string data, u32 flags`.
            let mut cursor = &body[1..];
            let _blob = read_ssh_string(&mut cursor).expect("key blob");
            let data = read_ssh_string(&mut cursor).expect("data");
            // Real Ed25519 sign of the data VERBATIM (no SSHSIG, no hashing).
            let signature = key.sign(data);

            let mut inner = Vec::new();
            put_ssh_string(&mut inner, SSH_ED25519_TYPE);
            put_ssh_string(&mut inner, &signature.to_bytes());

            let mut out = vec![SSH_AGENT_SIGN_RESPONSE];
            put_ssh_string(&mut out, &inner);
            frame_message(out)
        }
    }

    /// Wraps a `FakeSshAgent` as an in-memory `Read + Write` duplex so a
    /// transport-shaped consumer can write a full framed request and read the
    /// framed reply without a real socket or pipe. The read side is length-aware:
    /// a complete outer frame is processed as soon as it is fully written.
    pub(crate) struct FakeAgentDuplex {
        agent: FakeSshAgent,
        inbound: Vec<u8>,
        outbound: Vec<u8>,
        out_pos: usize,
    }

    impl FakeAgentDuplex {
        pub(crate) fn new(agent: FakeSshAgent) -> Self {
            Self {
                agent,
                inbound: Vec::new(),
                outbound: Vec::new(),
                out_pos: 0,
            }
        }

        /// If `inbound` holds at least one complete outer frame, process it and
        /// append the reply to `outbound`.
        fn drain_complete_frames(&mut self) {
            while self.inbound.len() >= 4 {
                let len =
                    u32::from_be_bytes(self.inbound[..4].try_into().expect("4 bytes")) as usize;
                if self.inbound.len() < 4 + len {
                    break;
                }
                let frame: Vec<u8> = self.inbound.drain(..4 + len).collect();
                let reply = self.agent.respond(&frame);
                self.outbound.extend_from_slice(&reply);
            }
        }
    }

    impl Write for FakeAgentDuplex {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.inbound.extend_from_slice(buf);
            self.drain_complete_frames();
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Read for FakeAgentDuplex {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let available = &self.outbound[self.out_pos..];
            let n = available.len().min(buf.len());
            buf[..n].copy_from_slice(&available[..n]);
            self.out_pos += n;
            Ok(n)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeSshAgent;
    use super::*;
    use crate::crypto::{EventVerificationStatus, SignerId, verify_ed25519_strict};

    const SEED: [u8; 32] = [42_u8; 32];

    fn fake_pubkey() -> [u8; 32] {
        ed25519_dalek::SigningKey::from_bytes(&SEED)
            .verifying_key()
            .to_bytes()
    }

    fn expected_signer() -> SignerId {
        SignerId::from_ed25519_public_key(fake_pubkey())
    }

    fn contains_subsequence(haystack: &[u8], needle: &[u8]) -> bool {
        needle.is_empty()
            || haystack
                .windows(needle.len())
                .any(|window| window == needle)
    }

    fn sign_response_with_inner_type(inner_type: &[u8], sig: &[u8]) -> Vec<u8> {
        let mut inner = Vec::new();
        put_ssh_string(&mut inner, inner_type);
        put_ssh_string(&mut inner, sig);
        let mut out = vec![SSH_AGENT_SIGN_RESPONSE];
        put_ssh_string(&mut out, &inner);
        frame_message(out)
    }

    #[test]
    fn sign_request_round_trips_and_the_unwrapped_signature_verifies_strict() {
        let agent = FakeSshAgent::with_key(SEED);
        // The DSSE PAE bytes the write path would hand the signer, verbatim.
        let message = crate::session::event::pre_authentication_encoding(
            crate::session::event::EVENT_TO_BE_SIGNED_V1_PAYLOAD_TYPE,
            br#"{"schema":"shore.event","version":1}"#,
        );

        let request = sign_request_bytes(&fake_pubkey(), &message);
        let response = agent.respond(&request); // fake agent processes one framed message
        let signature = parse_sign_response(&response).unwrap(); // unwrapped raw 64 bytes (base64'd)

        assert_eq!(
            verify_ed25519_strict(&expected_signer(), &message, signature.as_str()).unwrap(),
            EventVerificationStatus::Valid
        );
    }

    #[test]
    fn sign_request_carries_the_data_verbatim_with_no_sshsig_wrapper() {
        // flags = 0, the data is the raw DSSE PAE bytes, and there is NO SSHSIG
        // magic ("SSHSIG" = 0x53 0x53 0x48 0x53 0x49 0x47) anywhere in the
        // request. The agent receives exactly what we want signed.
        let message = b"DSSEv1 4 test 5 hello".to_vec();
        let request = sign_request_bytes(&fake_pubkey(), &message);

        assert!(
            !contains_subsequence(&request, b"SSHSIG"),
            "no SSHSIG envelope may appear in the sign request"
        );
        // The PAE bytes appear verbatim as the request's `data` string.
        assert!(contains_subsequence(&request, &message));
        // flags occupy the final 4 bytes and are zero.
        assert_eq!(&request[request.len() - 4..], &[0, 0, 0, 0]);
    }

    #[test]
    fn identities_answer_lists_the_loaded_key_blob() {
        let agent = FakeSshAgent::with_key(SEED);
        let request = request_identities_bytes();
        let response = agent.respond(&request);

        let blobs = parse_identities_answer(&response).unwrap();
        let expected_blob = ed25519_key_blob(&fake_pubkey());
        assert!(
            blobs.contains(&expected_blob),
            "the loaded key must be listed"
        );
    }

    #[test]
    fn an_absent_key_yields_an_empty_identities_list() {
        let agent = FakeSshAgent::empty(); // no key loaded
        let blobs = parse_identities_answer(&agent.respond(&request_identities_bytes())).unwrap();
        assert!(blobs.is_empty());
    }

    #[test]
    fn globally_locked_agent_lists_zero_identities() {
        // A globally-locked agent (ssh-add -x) hides its keys, so the identities-only
        // pre-flight sees an empty list and degrades to key-absent — it never signs.
        let agent = FakeSshAgent::with_key(SEED).locked();
        let blobs = parse_identities_answer(&agent.respond(&request_identities_bytes())).unwrap();
        assert!(
            blobs.is_empty(),
            "a locked agent lists no usable identities"
        );
    }

    #[test]
    fn an_agent_that_refuses_the_sign_surfaces_a_typed_failure() {
        // Models a per-key confirmation deny, or the agent dying/locking in the
        // resolve→sign window: the key IS listed (pre-flight passes), but SIGN fails.
        // The parser surfaces SSH_AGENT_FAILURE as a typed condition the write-seam
        // degrade names `signing_agent_sign_failed`.
        let agent = FakeSshAgent::with_key(SEED).refuses_sign();
        let listed = parse_identities_answer(&agent.respond(&request_identities_bytes())).unwrap();
        assert!(
            listed.contains(&ed25519_key_blob(&fake_pubkey())),
            "a refuses-sign agent still lists the key"
        );
        let response = agent.respond(&sign_request_bytes(&fake_pubkey(), b"DSSEv1 4 test 5 hi"));
        let err = parse_sign_response(&response).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("agent"));
    }

    #[test]
    fn sign_response_with_wrong_inner_type_or_short_sig_is_rejected() {
        // A response whose inner signature type isn't ssh-ed25519, or whose sig
        // isn't 64 bytes, is malformed.
        assert!(
            parse_sign_response(&sign_response_with_inner_type(b"ssh-rsa", &[0u8; 64])).is_err()
        );
        assert!(
            parse_sign_response(&sign_response_with_inner_type(b"ssh-ed25519", &[0u8; 10]))
                .is_err()
        );
    }
}
