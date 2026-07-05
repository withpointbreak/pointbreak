use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

use super::protocol::read_ssh_string;
use crate::crypto::SignerId;
use crate::error::{Result, ShoreError};

/// The SSH-wire type tag for a plain Ed25519 key — the only type accepted here.
const SSH_ED25519_TYPE: &str = "ssh-ed25519";
/// git's `user.signingKey` literal sentinel: `key::<type> <base64-blob>`.
const GIT_LITERAL_PREFIX: &str = "key::";
const ED25519_PUBLIC_KEY_LEN: usize = 32;

/// Parse an `ssh-ed25519 AAAA… [comment]` authorized-keys line or a
/// `key::ssh-ed25519 AAAA…` git literal into the signer's `did:key`.
///
/// Only plain `ssh-ed25519` keys are accepted. `sk-ssh-ed25519@openssh.com`
/// (FIDO), `ssh-rsa`, and `ecdsa-*` are rejected with a typed, actionable error:
/// an `-sk` key signs a hash+flags+counter construction (never the raw message),
/// so it can never verify under the strict Ed25519 path; RSA/ECDSA are not
/// Ed25519 at all. No crypto and no I/O happen here — base64 decode + wire walk.
pub fn parse_ssh_ed25519_public_key(input: &str) -> Result<SignerId> {
    let line = input.trim();
    let line = line.strip_prefix(GIT_LITERAL_PREFIX).unwrap_or(line);

    let mut fields = line.split_whitespace();
    let type_tag = fields
        .next()
        .ok_or_else(|| invalid("empty SSH public key input"))?;
    reject_unsupported_type(type_tag)?;

    let blob_b64 = fields
        .next()
        .ok_or_else(|| invalid("SSH public key is missing its base64 key blob"))?;
    // Any remaining field(s) are the comment; ignored.

    let blob = BASE64_STANDARD
        .decode(blob_b64.as_bytes())
        .map_err(|error| invalid(format!("SSH key blob is not valid base64: {error}")))?;

    let public_key = recover_ed25519_public_key(&blob)?;
    Ok(SignerId::from_ed25519_public_key(public_key))
}

/// Reject every non-plain-`ssh-ed25519` key type with a message that names the
/// type and tells the human what to do instead.
fn reject_unsupported_type(type_tag: &str) -> Result<()> {
    match type_tag {
        SSH_ED25519_TYPE => Ok(()),
        "sk-ssh-ed25519@openssh.com" => Err(invalid(
            "sk-ssh-ed25519 (FIDO/-sk) keys sign a hash+flags+counter construction, not the raw \
             message, so they can never verify under the strict Ed25519 path; use a plain \
             ssh-ed25519 key or `shore key init`",
        )),
        tag if tag.starts_with("ssh-rsa") || tag.starts_with("rsa-") => Err(invalid(format!(
            "{tag} is an RSA key, not Ed25519; adopt a plain ssh-ed25519 key or run `shore key init`"
        ))),
        tag if tag.starts_with("ecdsa-") => Err(invalid(format!(
            "{tag} is an ECDSA key, not Ed25519; adopt a plain ssh-ed25519 key or run `shore key init`"
        ))),
        other => Err(invalid(format!(
            "unsupported SSH key type {other:?}: only plain ssh-ed25519 keys are supported (or run \
             `shore key init`)"
        ))),
    }
}

/// Walk the SSH-wire blob `string "ssh-ed25519"` + `string <32 bytes>` and return
/// the 32 raw public-key bytes. The inner type string MUST also be `ssh-ed25519`
/// (a blob that disagrees with the line prefix is rejected).
fn recover_ed25519_public_key(blob: &[u8]) -> Result<[u8; ED25519_PUBLIC_KEY_LEN]> {
    let mut cursor = blob;
    let inner_type =
        read_ssh_string(&mut cursor).ok_or_else(|| invalid("SSH key blob is truncated"))?;
    if inner_type != SSH_ED25519_TYPE.as_bytes() {
        return Err(invalid(
            "SSH key blob's inner type does not match ssh-ed25519",
        ));
    }
    let key = read_ssh_string(&mut cursor)
        .ok_or_else(|| invalid("SSH key blob is missing its public-key field"))?;
    let key: [u8; ED25519_PUBLIC_KEY_LEN] = key
        .try_into()
        .map_err(|_| invalid("ssh-ed25519 public key is not 32 bytes"))?;
    if !cursor.is_empty() {
        return Err(invalid(
            "trailing bytes after the ssh-ed25519 public key blob",
        ));
    }
    Ok(key)
}

fn invalid(message: impl Into<String>) -> ShoreError {
    ShoreError::WorkflowInputInvalid {
        reason: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Golden vector. Regenerate with:
    //   ssh-keygen -t ed25519 -N '' -C 'dev@example' -f /tmp/k && cat /tmp/k.pub
    // The .pub line is the `ssh-ed25519 AAAA… comment` form below; the 32 raw
    // public-key bytes it encodes are PUBKEY_BYTES (the inverse `did:key` is
    // EXPECTED_DID). These three are one keypair's three representations.
    const SSH_LINE: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAID7lnwK7O5CFXew1hBuUnXz1+zK2pQtYEtxsbRMiOyvP dev@example";
    const PUBKEY_BYTES: [u8; 32] = [
        0x3e, 0xe5, 0x9f, 0x02, 0xbb, 0x3b, 0x90, 0x85, 0x5d, 0xec, 0x35, 0x84, 0x1b, 0x94, 0x9d,
        0x7c, 0xf5, 0xfb, 0x32, 0xb6, 0xa5, 0x0b, 0x58, 0x12, 0xdc, 0x6c, 0x6d, 0x13, 0x22, 0x3b,
        0x2b, 0xcf,
    ];

    fn expected_did() -> crate::crypto::SignerId {
        crate::crypto::SignerId::from_ed25519_public_key(PUBKEY_BYTES)
    }

    #[test]
    fn parses_authorized_keys_line_form_to_did_key() {
        let signer = parse_ssh_ed25519_public_key(SSH_LINE).unwrap();
        assert_eq!(signer, expected_did());
        assert_eq!(signer.ed25519_public_key().unwrap(), PUBKEY_BYTES);
    }

    #[test]
    fn parses_git_signing_key_literal_form_to_the_same_did_key() {
        // git's user.signingKey literal: `key::` sentinel + the type+blob (no comment).
        let blob = SSH_LINE
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        let literal = format!("key::{blob}");
        let signer = parse_ssh_ed25519_public_key(&literal).unwrap();
        assert_eq!(signer, expected_did());
    }

    #[test]
    fn ignores_a_trailing_comment_and_surrounding_whitespace() {
        let padded = format!("   {SSH_LINE}   \n");
        assert_eq!(
            parse_ssh_ed25519_public_key(&padded).unwrap(),
            expected_did()
        );
    }

    #[test]
    fn rejects_sk_fido_key_with_a_verify_strict_explanation() {
        // ed25519-sk signs a hash+flags+counter construction, never the raw
        // message, so it can NEVER verify under verify_ed25519_strict.
        let line =
            "sk-ssh-ed25519@openssh.com AAAAGnNrLXNzaC1lZDI1NTE5QG9wZW5zc2guY29t dev@example";
        let err = parse_ssh_ed25519_public_key(line).unwrap_err().to_string();
        assert!(err.contains("sk-ssh-ed25519") || err.contains("FIDO"));
        assert!(
            err.contains("verify"),
            "must explain it cannot verify strictly: {err}"
        );
    }

    #[test]
    fn rejects_rsa_key_pointing_at_key_init() {
        let line = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAAB dev@example";
        let err = parse_ssh_ed25519_public_key(line).unwrap_err().to_string();
        assert!(err.contains("ssh-rsa") || err.contains("RSA"));
        assert!(
            err.contains("key init"),
            "must point at `shore key init`: {err}"
        );
    }

    #[test]
    fn rejects_ecdsa_key_pointing_at_key_init() {
        let line = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTY dev@example";
        let err = parse_ssh_ed25519_public_key(line).unwrap_err().to_string();
        assert!(err.contains("ecdsa") || err.contains("ECDSA"));
        assert!(err.contains("key init"));
    }

    #[test]
    fn rejects_blob_whose_inner_type_disagrees_with_the_prefix() {
        // The line claims ssh-ed25519 but the wire blob's inner `string` names a
        // different algorithm — a malformed/spoofed key. Reject it.
        let mismatched = "ssh-ed25519 AAAAB3NzaC1yc2EAAAADAQAB dev@example";
        assert!(parse_ssh_ed25519_public_key(mismatched).is_err());
    }

    #[test]
    fn rejects_truncated_or_malformed_base64_and_short_blobs() {
        for bad in [
            "",                                           // empty
            "ssh-ed25519",                                // no blob
            "ssh-ed25519 not-base64!!",                   // not base64
            "ssh-ed25519 AAAA", // decodes but far too short for the wire frame
            "key::",            // literal sentinel, nothing after
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIE3Qz9", // truncated mid-blob
        ] {
            assert!(
                parse_ssh_ed25519_public_key(bad).is_err(),
                "{bad:?} must be rejected"
            );
        }
    }
}
