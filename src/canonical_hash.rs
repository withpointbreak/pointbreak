use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::Result;

pub(crate) fn sha256_json_hex<T>(value: &T) -> Result<String>
where
    T: Serialize,
{
    let value = serde_json::to_value(value)?;
    let bytes = canonical_json_bytes(&value)?;
    Ok(sha256_bytes_hex(&bytes))
}

pub(crate) fn sha256_json_prefixed(value: &serde_json::Value) -> Result<String> {
    let bytes = canonical_json_bytes(value)?;
    Ok(format!("sha256:{}", sha256_bytes_hex(&bytes)))
}

pub(crate) fn sha256_bytes_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_lower(hasher.finalize().as_slice())
}

/// Returns a portable filename stem for a prefixed SHA-256 identity.
///
/// Valid `sha256:<64 hex digits>` identities retain their digest component so
/// filenames remain recognizable. Any other input is hashed as an opaque
/// identity, keeping separators and platform-reserved punctuation out of the
/// resulting path.
pub(crate) fn portable_sha256_file_stem(identity: &str) -> String {
    identity
        .strip_prefix("sha256:")
        .filter(|stem| stem.len() == 64 && stem.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_owned)
        .unwrap_or_else(|| sha256_bytes_hex(identity.as_bytes()))
}

pub(crate) fn canonical_json_bytes(value: &serde_json::Value) -> Result<Vec<u8>> {
    let canonical = canonical_json_value(value);
    Ok(serde_json::to_vec(&canonical)?)
}

fn canonical_json_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(canonical_json_value).collect())
        }
        serde_json::Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort_unstable();

            let mut canonical = serde_json::Map::new();
            for key in keys {
                let value = object
                    .get(key)
                    .expect("key collected from object remains present");
                canonical.insert(key.clone(), canonical_json_value(value));
            }

            serde_json::Value::Object(canonical)
        }
        _ => value.clone(),
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn canonical_json_hash_sorts_object_keys_recursively() {
        let value = json!({
            "b": 2,
            "arr": [{ "z": 1, "y": 2 }],
            "a": { "d": 4, "c": 3 }
        });

        let bytes = canonical_json_bytes(&value).expect("canonical json bytes");

        assert_eq!(
            String::from_utf8(bytes).expect("canonical json is utf-8"),
            r#"{"a":{"c":3,"d":4},"arr":[{"y":2,"z":1}],"b":2}"#
        );
        assert_eq!(
            sha256_json_prefixed(&value).expect("canonical hash"),
            "sha256:b0d4c7b49807651a24243b0ebe264541166d8aa9fd31719c6e461a0118e4dd2f"
        );
    }

    #[test]
    fn portable_sha256_file_stem_retains_only_a_valid_digest_component() {
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

        assert_eq!(
            portable_sha256_file_stem(&format!("sha256:{digest}")),
            digest
        );
        assert_eq!(
            portable_sha256_file_stem("sha256:not-a-digest"),
            sha256_bytes_hex(b"sha256:not-a-digest")
        );
        assert_eq!(
            portable_sha256_file_stem("other:0123456789abcdef"),
            sha256_bytes_hex(b"other:0123456789abcdef")
        );
    }
}
