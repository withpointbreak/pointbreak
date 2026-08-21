use std::io::{Read, Write};

use sha2::{Digest, Sha256};

use super::QualificationRecordKindV1;

pub const PHYSICAL_STORE_HEADER_LEN_V1: usize = 128;
pub const PHYSICAL_RECORD_HEADER_LEN_V1: usize = 192;
pub const MAX_PHYSICAL_HEADER_LEN_V1: usize = 4 * 1024;
pub const MAX_ZSTD_WINDOW_V1: u64 = 8 * 1024 * 1024;

const STORE_MAGIC: &[u8; 4] = b"PBST";
const RECORD_MAGIC: &[u8; 4] = b"PBRF";
const STORE_FORMAT_VERSION_V1: u16 = 1;
const ENVELOPE_VERSION_V1: u16 = 1;
const TRANSFORM_ZSTD: u16 = 1;
const TRANSFORM_PROFILE_ZSTD_1: u16 = 1;
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];
const DECODE_CHUNK_LEN: usize = 128 * 1024;

pub trait QualificationCancellation {
    fn is_cancelled(&self) -> bool;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NeverCancelled;

impl QualificationCancellation for NeverCancelled {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AlwaysCancelled;

impl QualificationCancellation for AlwaysCancelled {
    fn is_cancelled(&self) -> bool {
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalTransformV1 {
    Raw,
    Zstd1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PhysicalRecordKindV1 {
    Event = 1,
    ObjectArtifact = 2,
    NoteBody = 3,
    DocumentManifest = 4,
    DocumentBlob = 5,
    RelationProof = 6,
}

impl PhysicalRecordKindV1 {
    pub fn from_qualification(kind: QualificationRecordKindV1) -> Self {
        match kind {
            QualificationRecordKindV1::LegacyEvent
            | QualificationRecordKindV1::GenerationProposal
            | QualificationRecordKindV1::RelationAttestation
            | QualificationRecordKindV1::FactPort => Self::Event,
            QualificationRecordKindV1::ObjectArtifact => Self::ObjectArtifact,
            QualificationRecordKindV1::NoteBody => Self::NoteBody,
            QualificationRecordKindV1::RelationProof => Self::RelationProof,
            QualificationRecordKindV1::DocumentManifest => Self::DocumentManifest,
            QualificationRecordKindV1::DocumentBlob => Self::DocumentBlob,
        }
    }

    fn from_byte(value: u8) -> Result<Self, PhysicalCodecError> {
        match value {
            1 => Ok(Self::Event),
            2 => Ok(Self::ObjectArtifact),
            3 => Ok(Self::NoteBody),
            4 => Ok(Self::DocumentManifest),
            5 => Ok(Self::DocumentBlob),
            6 => Ok(Self::RelationProof),
            _ => Err(PhysicalCodecError::UnsupportedRecordKind { value }),
        }
    }

    fn decoded_cap(self) -> u64 {
        match self {
            Self::Event => 1024 * 1024,
            Self::ObjectArtifact => 64 * 1024 * 1024,
            Self::NoteBody | Self::DocumentManifest | Self::RelationProof => 16 * 1024 * 1024,
            Self::DocumentBlob => 256 * 1024 * 1024,
        }
    }

    pub fn directory_name(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::ObjectArtifact => "object",
            Self::NoteBody => "note",
            Self::DocumentManifest => "document-manifest",
            Self::DocumentBlob => "document-blob",
            Self::RelationProof => "relation-proof",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhysicalRecordV1 {
    pub record_kind: PhysicalRecordKindV1,
    pub logical_key_digest: [u8; 32],
    pub decoded_sha256: [u8; 32],
    pub decoded_bytes: Vec<u8>,
    pub transform: PhysicalTransformV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhysicalStoreHeaderV1 {
    pub profile_id: u32,
    pub store_uuid: [u8; 16],
}

impl PhysicalStoreHeaderV1 {
    pub fn new(profile_id: u32, store_uuid: [u8; 16]) -> Self {
        Self {
            profile_id,
            store_uuid,
        }
    }

    pub fn encode(&self) -> [u8; PHYSICAL_STORE_HEADER_LEN_V1] {
        let mut bytes = [0_u8; PHYSICAL_STORE_HEADER_LEN_V1];
        bytes[0..4].copy_from_slice(STORE_MAGIC);
        bytes[4..6].copy_from_slice(&STORE_FORMAT_VERSION_V1.to_le_bytes());
        bytes[6..8].copy_from_slice(&(PHYSICAL_STORE_HEADER_LEN_V1 as u16).to_le_bytes());
        bytes[8..12].copy_from_slice(&self.profile_id.to_le_bytes());
        bytes[16..32].copy_from_slice(&self.store_uuid);
        let hash = Sha256::digest(&bytes[..96]);
        bytes[96..128].copy_from_slice(&hash);
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, PhysicalCodecError> {
        if bytes.len() != PHYSICAL_STORE_HEADER_LEN_V1 {
            return Err(PhysicalCodecError::Truncated {
                needed: PHYSICAL_STORE_HEADER_LEN_V1,
                actual: bytes.len(),
            });
        }
        if &bytes[0..4] != STORE_MAGIC {
            return Err(PhysicalCodecError::InvalidPhysicalHeader {
                message: "invalid PBST magic".to_owned(),
            });
        }
        if read_u16(bytes, 4)? != STORE_FORMAT_VERSION_V1 {
            return Err(PhysicalCodecError::UnsupportedVersion {
                version: read_u16(bytes, 4)?,
            });
        }
        if read_u16(bytes, 6)? as usize != PHYSICAL_STORE_HEADER_LEN_V1 {
            return Err(PhysicalCodecError::InvalidPhysicalHeader {
                message: "invalid PBST header length".to_owned(),
            });
        }
        let profile_id = read_u32(bytes, 8)?;
        if !matches!(profile_id, 1 | 2) {
            return Err(PhysicalCodecError::UnsupportedStoreProfile { profile_id });
        }
        if read_u32(bytes, 12)? != 0 || bytes[32..96].iter().any(|byte| *byte != 0) {
            return Err(PhysicalCodecError::InvalidPhysicalHeader {
                message: "PBST reserved fields must be zero".to_owned(),
            });
        }
        if Sha256::digest(&bytes[..96]).as_slice() != &bytes[96..128] {
            return Err(PhysicalCodecError::HeaderHashMismatch);
        }
        let mut store_uuid = [0_u8; 16];
        store_uuid.copy_from_slice(&bytes[16..32]);
        Ok(Self {
            profile_id,
            store_uuid,
        })
    }
}

impl PhysicalRecordV1 {
    pub fn encode(
        logical_key: &str,
        record_kind: QualificationRecordKindV1,
        decoded_bytes: &[u8],
        cancellation: &dyn QualificationCancellation,
    ) -> Result<Vec<u8>, PhysicalCodecError> {
        ensure_not_cancelled(cancellation)?;
        let compressed = encode_zstd1(decoded_bytes)?;
        let (transform, encoded_payload) = if compressed.len() < decoded_bytes.len() {
            (PhysicalTransformV1::Zstd1, compressed)
        } else {
            (PhysicalTransformV1::Raw, decoded_bytes.to_vec())
        };
        Self::encode_with_payload(
            logical_key,
            record_kind,
            decoded_bytes,
            transform,
            encoded_payload,
            cancellation,
        )
    }

    pub fn encode_with_transform(
        logical_key: &str,
        record_kind: QualificationRecordKindV1,
        decoded_bytes: &[u8],
        transform: PhysicalTransformV1,
        cancellation: &dyn QualificationCancellation,
    ) -> Result<Vec<u8>, PhysicalCodecError> {
        ensure_not_cancelled(cancellation)?;
        let encoded_payload = match transform {
            PhysicalTransformV1::Raw => decoded_bytes.to_vec(),
            PhysicalTransformV1::Zstd1 => encode_zstd1(decoded_bytes)?,
        };
        Self::encode_with_payload(
            logical_key,
            record_kind,
            decoded_bytes,
            transform,
            encoded_payload,
            cancellation,
        )
    }

    fn encode_with_payload(
        logical_key: &str,
        record_kind: QualificationRecordKindV1,
        decoded_bytes: &[u8],
        transform: PhysicalTransformV1,
        encoded_payload: Vec<u8>,
        cancellation: &dyn QualificationCancellation,
    ) -> Result<Vec<u8>, PhysicalCodecError> {
        ensure_not_cancelled(cancellation)?;
        let physical_kind = PhysicalRecordKindV1::from_qualification(record_kind);
        check_record_bounds(
            physical_kind,
            decoded_bytes.len() as u64,
            encoded_payload.len() as u64,
        )?;

        let mut header = vec![0_u8; PHYSICAL_RECORD_HEADER_LEN_V1];
        header[0..4].copy_from_slice(RECORD_MAGIC);
        header[4..6].copy_from_slice(&ENVELOPE_VERSION_V1.to_le_bytes());
        header[6..8].copy_from_slice(&(PHYSICAL_RECORD_HEADER_LEN_V1 as u16).to_le_bytes());
        header[8] = physical_kind as u8;
        header[9] = u8::from(transform == PhysicalTransformV1::Zstd1);
        header[16..24].copy_from_slice(&(decoded_bytes.len() as u64).to_le_bytes());
        header[24..32].copy_from_slice(&(encoded_payload.len() as u64).to_le_bytes());
        header[32..64].copy_from_slice(&logical_key_digest(logical_key));
        header[64..96].copy_from_slice(&Sha256::digest(decoded_bytes));
        header[96..128].copy_from_slice(&Sha256::digest(&encoded_payload));
        if transform == PhysicalTransformV1::Zstd1 {
            header[128..130].copy_from_slice(&TRANSFORM_ZSTD.to_le_bytes());
            header[130..132].copy_from_slice(&TRANSFORM_PROFILE_ZSTD_1.to_le_bytes());
        }
        let hash = Sha256::digest(&header[..160]);
        header[160..192].copy_from_slice(&hash);
        header.extend_from_slice(&encoded_payload);
        ensure_not_cancelled(cancellation)?;
        Ok(header)
    }

    pub fn decode(
        bytes: &[u8],
        cancellation: &dyn QualificationCancellation,
    ) -> Result<Self, PhysicalCodecError> {
        ensure_not_cancelled(cancellation)?;
        if bytes.len() < 8 {
            return Err(PhysicalCodecError::Truncated {
                needed: 8,
                actual: bytes.len(),
            });
        }
        if &bytes[0..4] != RECORD_MAGIC {
            return Err(PhysicalCodecError::InvalidPhysicalHeader {
                message: "invalid PBRF magic".to_owned(),
            });
        }
        let version = read_u16(bytes, 4)?;
        if version != ENVELOPE_VERSION_V1 {
            return Err(PhysicalCodecError::UnsupportedVersion { version });
        }
        let header_len = read_u16(bytes, 6)? as usize;
        if !(PHYSICAL_RECORD_HEADER_LEN_V1..=MAX_PHYSICAL_HEADER_LEN_V1).contains(&header_len)
            || !header_len.is_multiple_of(8)
        {
            return Err(PhysicalCodecError::InvalidPhysicalHeader {
                message: format!("invalid PBRF header length {header_len}"),
            });
        }
        if bytes.len() < header_len {
            return Err(PhysicalCodecError::Truncated {
                needed: header_len,
                actual: bytes.len(),
            });
        }
        let header_hash_offset = header_len - 32;
        if Sha256::digest(&bytes[..header_hash_offset]).as_slice()
            != &bytes[header_hash_offset..header_len]
        {
            return Err(PhysicalCodecError::HeaderHashMismatch);
        }
        if read_u16(bytes, 10)? != 0 || read_u32(bytes, 12)? != 0 {
            return Err(PhysicalCodecError::InvalidPhysicalHeader {
                message: "PBRF reserved fields must be zero".to_owned(),
            });
        }
        validate_extensions(&bytes[160..header_hash_offset])?;
        let record_kind = PhysicalRecordKindV1::from_byte(bytes[8])?;
        let transform = parse_transform(&bytes[..header_len])?;
        let decoded_len = read_u64(bytes, 16)?;
        let encoded_len = read_u64(bytes, 24)?;
        check_record_bounds(record_kind, decoded_len, encoded_len)?;
        let total_len_u64 =
            (header_len as u64)
                .checked_add(encoded_len)
                .ok_or(PhysicalCodecError::SizeLimit {
                    kind: record_kind,
                    decoded_len,
                    encoded_len,
                })?;
        let total_len =
            usize::try_from(total_len_u64).map_err(|_| PhysicalCodecError::SizeLimit {
                kind: record_kind,
                decoded_len,
                encoded_len,
            })?;
        if bytes.len() != total_len {
            return Err(PhysicalCodecError::LengthMismatch {
                expected: total_len,
                actual: bytes.len(),
            });
        }
        let payload = &bytes[header_len..];
        if Sha256::digest(payload).as_slice() != &bytes[96..128] {
            return Err(PhysicalCodecError::EncodedHashMismatch);
        }
        let decoded_bytes = match transform {
            PhysicalTransformV1::Raw => payload.to_vec(),
            PhysicalTransformV1::Zstd1 => {
                validate_zstd1_header(payload, decoded_len)?;
                decode_zstd1(payload, decoded_len, cancellation)?
            }
        };
        ensure_not_cancelled(cancellation)?;
        if decoded_bytes.len() as u64 != decoded_len {
            return Err(PhysicalCodecError::DecodedLengthMismatch {
                expected: decoded_len,
                actual: decoded_bytes.len() as u64,
            });
        }
        let decoded_hash = Sha256::digest(&decoded_bytes);
        if decoded_hash.as_slice() != &bytes[64..96] {
            return Err(PhysicalCodecError::DecodedHashMismatch);
        }
        let mut logical_key_digest = [0_u8; 32];
        logical_key_digest.copy_from_slice(&bytes[32..64]);
        let mut decoded_sha256 = [0_u8; 32];
        decoded_sha256.copy_from_slice(&decoded_hash);
        Ok(Self {
            record_kind,
            logical_key_digest,
            decoded_sha256,
            decoded_bytes,
            transform,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PhysicalCodecError {
    #[error("physical input is truncated: need {needed} bytes, have {actual}")]
    Truncated { needed: usize, actual: usize },
    #[error("invalid physical header: {message}")]
    InvalidPhysicalHeader { message: String },
    #[error("unsupported physical version {version}")]
    UnsupportedVersion { version: u16 },
    #[error("unsupported store profile {profile_id}")]
    UnsupportedStoreProfile { profile_id: u32 },
    #[error("physical header hash does not match")]
    HeaderHashMismatch,
    #[error("unsupported physical record kind {value}")]
    UnsupportedRecordKind { value: u8 },
    #[error("unsupported physical transform")]
    UnsupportedTransform,
    #[error("unsupported critical extension")]
    UnsupportedCriticalExtension,
    #[error("record exceeds its size limit")]
    SizeLimit {
        kind: PhysicalRecordKindV1,
        decoded_len: u64,
        encoded_len: u64,
    },
    #[error("physical length mismatch: expected {expected}, got {actual}")]
    LengthMismatch { expected: usize, actual: usize },
    #[error("encoded payload hash does not match")]
    EncodedHashMismatch,
    #[error("invalid zstd-1 profile: {message}")]
    InvalidZstdProfile { message: String },
    #[error("zstd codec failed: {message}")]
    Codec { message: String },
    #[error("decoded length mismatch: expected {expected}, got {actual}")]
    DecodedLengthMismatch { expected: u64, actual: u64 },
    #[error("decoded payload hash does not match")]
    DecodedHashMismatch,
    #[error("operation cancelled")]
    Cancelled,
}

impl PhysicalCodecError {
    pub fn stage(&self) -> &'static str {
        match self {
            Self::Truncated { .. } => "truncated",
            Self::InvalidPhysicalHeader { .. } => "invalid_physical_header",
            Self::UnsupportedVersion { .. } => "unsupported_version",
            Self::UnsupportedStoreProfile { .. } => "unsupported_store_profile",
            Self::HeaderHashMismatch => "header_hash_mismatch",
            Self::UnsupportedRecordKind { .. } => "unsupported_record_kind",
            Self::UnsupportedTransform | Self::UnsupportedCriticalExtension => {
                "unsupported_transform"
            }
            Self::SizeLimit { .. } => "size_limit",
            Self::LengthMismatch { .. } => "length_mismatch",
            Self::EncodedHashMismatch => "encoded_hash_mismatch",
            Self::InvalidZstdProfile { .. } => "invalid_zstd_profile",
            Self::Codec { .. } => "codec_error",
            Self::DecodedLengthMismatch { .. } | Self::DecodedHashMismatch => {
                "decoded_hash_mismatch"
            }
            Self::Cancelled => "cancelled",
        }
    }
}

fn parse_transform(header: &[u8]) -> Result<PhysicalTransformV1, PhysicalCodecError> {
    match header[9] {
        0 if header[128..160].iter().all(|byte| *byte == 0) => Ok(PhysicalTransformV1::Raw),
        1 if read_u16(header, 128)? == TRANSFORM_ZSTD
            && read_u16(header, 130)? == TRANSFORM_PROFILE_ZSTD_1
            && read_u32(header, 132)? == 0
            && header[136..160].iter().all(|byte| *byte == 0) =>
        {
            Ok(PhysicalTransformV1::Zstd1)
        }
        _ => Err(PhysicalCodecError::UnsupportedTransform),
    }
}

fn check_record_bounds(
    kind: PhysicalRecordKindV1,
    decoded_len: u64,
    encoded_len: u64,
) -> Result<(), PhysicalCodecError> {
    let cap = kind.decoded_cap();
    if decoded_len > cap || encoded_len > cap {
        return Err(PhysicalCodecError::SizeLimit {
            kind,
            decoded_len,
            encoded_len,
        });
    }
    Ok(())
}

fn validate_extensions(mut bytes: &[u8]) -> Result<(), PhysicalCodecError> {
    while !bytes.is_empty() {
        if bytes.len() < 8 {
            return Err(PhysicalCodecError::InvalidPhysicalHeader {
                message: "truncated PBRF extension header".to_owned(),
            });
        }
        let flags = u16::from_le_bytes([bytes[2], bytes[3]]);
        if flags & !1 != 0 {
            return Err(PhysicalCodecError::InvalidPhysicalHeader {
                message: "PBRF extension reserved flags are set".to_owned(),
            });
        }
        let value_len = usize::try_from(u32::from_le_bytes(
            bytes[4..8].try_into().expect("extension length"),
        ))
        .map_err(|_| PhysicalCodecError::InvalidPhysicalHeader {
            message: "PBRF extension length does not fit this platform".to_owned(),
        })?;
        let padded_len = value_len.checked_add(7).map(|length| length & !7).ok_or(
            PhysicalCodecError::InvalidPhysicalHeader {
                message: "PBRF extension length overflows".to_owned(),
            },
        )?;
        let total_len =
            8_usize
                .checked_add(padded_len)
                .ok_or(PhysicalCodecError::InvalidPhysicalHeader {
                    message: "PBRF extension length overflows".to_owned(),
                })?;
        let extension =
            bytes
                .get(..total_len)
                .ok_or(PhysicalCodecError::InvalidPhysicalHeader {
                    message: "truncated PBRF extension value".to_owned(),
                })?;
        if extension[8 + value_len..].iter().any(|byte| *byte != 0) {
            return Err(PhysicalCodecError::InvalidPhysicalHeader {
                message: "PBRF extension padding must be zero".to_owned(),
            });
        }
        if flags & 1 != 0 {
            return Err(PhysicalCodecError::UnsupportedCriticalExtension);
        }
        bytes = &bytes[total_len..];
    }
    Ok(())
}

fn encode_zstd1(decoded: &[u8]) -> Result<Vec<u8>, PhysicalCodecError> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 1).map_err(codec_error)?;
    encoder.include_checksum(true).map_err(codec_error)?;
    encoder
        .set_pledged_src_size(Some(decoded.len() as u64))
        .map_err(codec_error)?;
    encoder.write_all(decoded).map_err(codec_error)?;
    encoder.finish().map_err(codec_error)
}

fn decode_zstd1(
    payload: &[u8],
    decoded_len: u64,
    cancellation: &dyn QualificationCancellation,
) -> Result<Vec<u8>, PhysicalCodecError> {
    let capacity = usize::try_from(decoded_len.min(DECODE_CHUNK_LEN as u64)).map_err(|_| {
        PhysicalCodecError::InvalidZstdProfile {
            message: "decoded size does not fit this platform".to_owned(),
        }
    })?;
    let mut output = Vec::with_capacity(capacity);
    let mut decoder = zstd::stream::read::Decoder::new(payload).map_err(codec_error)?;
    let mut chunk = [0_u8; DECODE_CHUNK_LEN];
    loop {
        ensure_not_cancelled(cancellation)?;
        let read = decoder.read(&mut chunk).map_err(codec_error)?;
        if read == 0 {
            break;
        }
        let next_len = (output.len() as u64).checked_add(read as u64).ok_or(
            PhysicalCodecError::InvalidZstdProfile {
                message: "decoded byte count overflow".to_owned(),
            },
        )?;
        if next_len > decoded_len {
            return Err(PhysicalCodecError::DecodedLengthMismatch {
                expected: decoded_len,
                actual: next_len,
            });
        }
        output.extend_from_slice(&chunk[..read]);
    }
    Ok(output)
}

fn validate_zstd1_header(payload: &[u8], decoded_len: u64) -> Result<(), PhysicalCodecError> {
    if payload.len() < 5 || payload[..4] != ZSTD_MAGIC {
        return Err(PhysicalCodecError::InvalidZstdProfile {
            message: "payload is not one standard zstd frame".to_owned(),
        });
    }
    let descriptor = payload[4];
    if descriptor & 0x18 != 0 {
        return Err(PhysicalCodecError::InvalidZstdProfile {
            message: "reserved zstd descriptor bits are set".to_owned(),
        });
    }
    if descriptor & 0x04 == 0 {
        return Err(PhysicalCodecError::InvalidZstdProfile {
            message: "zstd content checksum is required".to_owned(),
        });
    }
    let single_segment = descriptor & 0x20 != 0;
    let dictionary_flag = descriptor & 0x03;
    let content_size_flag = descriptor >> 6;
    let mut cursor = 5_usize;
    let window_size = if single_segment {
        None
    } else {
        let window_descriptor =
            *payload
                .get(cursor)
                .ok_or_else(|| PhysicalCodecError::InvalidZstdProfile {
                    message: "missing zstd window descriptor".to_owned(),
                })?;
        cursor += 1;
        let exponent = u32::from(window_descriptor >> 3);
        let base = 1_u64 << (10 + exponent);
        Some(base + (base / 8) * u64::from(window_descriptor & 7))
    };
    let dictionary_size = match dictionary_flag {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 4,
        _ => unreachable!(),
    };
    let dictionary_bytes = payload
        .get(cursor..cursor + dictionary_size)
        .ok_or_else(|| PhysicalCodecError::InvalidZstdProfile {
            message: "truncated zstd dictionary id".to_owned(),
        })?;
    if dictionary_bytes.iter().any(|byte| *byte != 0) {
        return Err(PhysicalCodecError::InvalidZstdProfile {
            message: "zstd dictionaries are not supported".to_owned(),
        });
    }
    cursor += dictionary_size;
    let content_size_bytes = match content_size_flag {
        0 if single_segment => 1,
        0 => 0,
        1 => 2,
        2 => 4,
        3 => 8,
        _ => unreachable!(),
    };
    if content_size_bytes == 0 {
        return Err(PhysicalCodecError::InvalidZstdProfile {
            message: "zstd frame content size is required".to_owned(),
        });
    }
    let content_size_slice = payload
        .get(cursor..cursor + content_size_bytes)
        .ok_or_else(|| PhysicalCodecError::InvalidZstdProfile {
            message: "truncated zstd content size".to_owned(),
        })?;
    let mut little_endian = [0_u8; 8];
    little_endian[..content_size_bytes].copy_from_slice(content_size_slice);
    let mut content_size = u64::from_le_bytes(little_endian);
    if content_size_bytes == 2 {
        content_size += 256;
    }
    if content_size != decoded_len {
        return Err(PhysicalCodecError::InvalidZstdProfile {
            message: format!(
                "zstd content size {content_size} does not match decoded length {decoded_len}"
            ),
        });
    }
    let effective_window = window_size.unwrap_or(content_size);
    if effective_window > MAX_ZSTD_WINDOW_V1 {
        return Err(PhysicalCodecError::InvalidZstdProfile {
            message: format!("zstd window {effective_window} exceeds {MAX_ZSTD_WINDOW_V1}"),
        });
    }
    let frame_size = zstd::zstd_safe::find_frame_compressed_size(payload).map_err(|error| {
        PhysicalCodecError::InvalidZstdProfile {
            message: format!("invalid zstd frame: {error:?}"),
        }
    })?;
    if frame_size != payload.len() {
        return Err(PhysicalCodecError::InvalidZstdProfile {
            message: "zstd payload must contain exactly one frame".to_owned(),
        });
    }
    Ok(())
}

pub(super) fn logical_key_digest(logical_key: &str) -> [u8; 32] {
    if let Some(hex) = logical_key.strip_prefix("sha256:")
        && hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        let mut decoded = [0_u8; 32];
        let valid = hex
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .enumerate()
            .all(|(index, pair)| {
                std::str::from_utf8(pair)
                    .ok()
                    .and_then(|value| u8::from_str_radix(value, 16).ok())
                    .map(|value| decoded[index] = value)
                    .is_some()
            });
        if valid {
            return decoded;
        }
    }
    Sha256::digest(logical_key.as_bytes()).into()
}

fn ensure_not_cancelled(
    cancellation: &dyn QualificationCancellation,
) -> Result<(), PhysicalCodecError> {
    if cancellation.is_cancelled() {
        Err(PhysicalCodecError::Cancelled)
    } else {
        Ok(())
    }
}

fn codec_error(error: std::io::Error) -> PhysicalCodecError {
    PhysicalCodecError::Codec {
        message: error.to_string(),
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, PhysicalCodecError> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or(PhysicalCodecError::Truncated {
            needed: offset + 2,
            actual: bytes.len(),
        })?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, PhysicalCodecError> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or(PhysicalCodecError::Truncated {
            needed: offset + 4,
            actual: bytes.len(),
        })?;
    Ok(u32::from_le_bytes(
        value.try_into().expect("four-byte slice"),
    ))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, PhysicalCodecError> {
    let value = bytes
        .get(offset..offset + 8)
        .ok_or(PhysicalCodecError::Truncated {
            needed: offset + 8,
            actual: bytes.len(),
        })?;
    Ok(u64::from_le_bytes(
        value.try_into().expect("eight-byte slice"),
    ))
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use sha2::{Digest, Sha256};

    use super::*;

    const GOLDEN_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/store-foundation/codec/golden.json"
    ));
    const CORRUPT_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/store-foundation/codec/corrupt.json"
    ));

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GoldenFixture {
        decoded_hex: String,
        logical_key: String,
        raw_envelope_hex: String,
        raw_envelope_sha256: String,
        zstd_envelope_hex: String,
        zstd_envelope_sha256: String,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CorruptFixture {
        mutations: Vec<CorruptMutation>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CorruptMutation {
        name: String,
        base: String,
        offset: Option<usize>,
        replacement_hex: Option<String>,
        truncate_to: Option<usize>,
        append_hex: Option<String>,
        rehash_header: bool,
        expected_stage: String,
    }

    fn fixture() -> GoldenFixture {
        serde_json::from_str(GOLDEN_FIXTURE).expect("valid golden fixture")
    }

    fn hex_decode(value: &str) -> Vec<u8> {
        assert_eq!(value.len() % 2, 0);
        value
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).expect("hex pair"), 16).expect("hex")
            })
            .collect()
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    #[test]
    fn sha256_logical_key_fast_path_requires_canonical_lowercase_hex() {
        let canonical = format!("sha256:{:064x}", 10);
        let uppercase = canonical.replace('a', "A");
        let signed_pair = format!("sha256:+a{}", "0".repeat(62));

        assert_eq!(logical_key_digest(&canonical)[31], 10);
        assert_eq!(
            logical_key_digest(&uppercase),
            Sha256::digest(uppercase.as_bytes()).as_slice()
        );
        assert_eq!(
            logical_key_digest(&signed_pair),
            Sha256::digest(signed_pair.as_bytes()).as_slice()
        );
    }

    fn rehash_header(bytes: &mut [u8]) {
        let header_len = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        let hash_offset = header_len - 32;
        let hash = Sha256::digest(&bytes[..hash_offset]);
        bytes[hash_offset..header_len].copy_from_slice(&hash);
    }

    fn replace_payload(bytes: &mut Vec<u8>, payload: &[u8]) {
        let header_len = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        bytes.truncate(header_len);
        bytes[24..32].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        bytes[96..128].copy_from_slice(&Sha256::digest(payload));
        rehash_header(bytes);
        bytes.extend_from_slice(payload);
    }

    fn add_empty_extension(bytes: &mut Vec<u8>, critical: bool) {
        let payload = bytes.split_off(PHYSICAL_RECORD_HEADER_LEN_V1);
        bytes.truncate(160);
        bytes.extend_from_slice(&99_u16.to_le_bytes());
        bytes.extend_from_slice(&u16::from(critical).to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&[0_u8; 32]);
        bytes[6..8].copy_from_slice(&200_u16.to_le_bytes());
        rehash_header(bytes);
        bytes.extend_from_slice(&payload);
    }

    #[test]
    fn independent_raw_and_zstd_golden_vectors_decode_exactly() {
        let fixture = fixture();
        let decoded = hex_decode(&fixture.decoded_hex);
        let raw = hex_decode(&fixture.raw_envelope_hex);
        let zstd = hex_decode(&fixture.zstd_envelope_hex);

        assert_eq!(sha256_hex(&raw), fixture.raw_envelope_sha256);
        assert_eq!(sha256_hex(&zstd), fixture.zstd_envelope_sha256);

        let raw_record = PhysicalRecordV1::decode(&raw, &NeverCancelled).expect("raw vector");
        let zstd_record = PhysicalRecordV1::decode(&zstd, &NeverCancelled).expect("zstd vector");

        assert_eq!(raw_record.decoded_bytes, decoded);
        assert_eq!(zstd_record.decoded_bytes, decoded);
        assert_eq!(raw_record.decoded_sha256, zstd_record.decoded_sha256);
        assert_eq!(
            raw_record.logical_key_digest,
            zstd_record.logical_key_digest
        );
        assert_eq!(raw_record.transform, PhysicalTransformV1::Raw);
        assert_eq!(zstd_record.transform, PhysicalTransformV1::Zstd1);
    }

    #[test]
    fn writer_matches_golden_vectors_and_applies_the_canonical_raw_fallback() {
        let fixture = fixture();
        let decoded = hex_decode(&fixture.decoded_hex);

        let raw = PhysicalRecordV1::encode_with_transform(
            &fixture.logical_key,
            QualificationRecordKindV1::DocumentBlob,
            &decoded,
            PhysicalTransformV1::Raw,
            &NeverCancelled,
        )
        .expect("raw encode");
        let zstd = PhysicalRecordV1::encode_with_transform(
            &fixture.logical_key,
            QualificationRecordKindV1::DocumentBlob,
            &decoded,
            PhysicalTransformV1::Zstd1,
            &NeverCancelled,
        )
        .expect("zstd encode");
        let canonical = PhysicalRecordV1::encode(
            &fixture.logical_key,
            QualificationRecordKindV1::DocumentBlob,
            &decoded,
            &NeverCancelled,
        )
        .expect("canonical encode");

        assert_eq!(raw, hex_decode(&fixture.raw_envelope_hex));
        assert_eq!(zstd, hex_decode(&fixture.zstd_envelope_hex));
        assert_eq!(
            canonical, raw,
            "expanded zstd payload must fall back to raw"
        );

        let compressible = vec![b'a'; 4096];
        let compressed = PhysicalRecordV1::encode(
            "compressible",
            QualificationRecordKindV1::DocumentBlob,
            &compressible,
            &NeverCancelled,
        )
        .expect("compressed encode");
        assert_eq!(
            PhysicalRecordV1::decode(&compressed, &NeverCancelled)
                .expect("compressed decode")
                .transform,
            PhysicalTransformV1::Zstd1
        );
    }

    #[test]
    fn corrupt_fixture_rejects_each_failure_at_its_declared_stage() {
        let fixture = fixture();
        let corrupt: CorruptFixture =
            serde_json::from_str(CORRUPT_FIXTURE).expect("valid corrupt fixture");

        for mutation in corrupt.mutations {
            let mut bytes = match mutation.base.as_str() {
                "raw" => hex_decode(&fixture.raw_envelope_hex),
                "zstd" => hex_decode(&fixture.zstd_envelope_hex),
                other => panic!("unknown base {other}"),
            };
            if let Some(offset) = mutation.offset {
                let replacement =
                    hex_decode(mutation.replacement_hex.as_deref().expect("replacement"));
                bytes[offset..offset + replacement.len()].copy_from_slice(&replacement);
            }
            if let Some(length) = mutation.truncate_to {
                bytes.truncate(length);
            }
            if let Some(append) = mutation.append_hex.as_deref() {
                bytes.extend_from_slice(&hex_decode(append));
            }
            if mutation.rehash_header {
                rehash_header(&mut bytes);
            }

            let error = PhysicalRecordV1::decode(&bytes, &NeverCancelled)
                .expect_err(&format!("{} must fail", mutation.name));
            assert_eq!(error.stage(), mutation.expected_stage, "{}", mutation.name);
        }
    }

    #[test]
    fn decode_rejects_excessive_declared_lengths_before_allocation() {
        let fixture = fixture();
        let mut bytes = hex_decode(&fixture.raw_envelope_hex);
        bytes[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        rehash_header(&mut bytes);

        assert!(matches!(
            PhysicalRecordV1::decode(&bytes, &NeverCancelled),
            Err(PhysicalCodecError::SizeLimit { .. })
        ));
    }

    #[test]
    fn decoder_rejects_nonconforming_zstd_frames_before_logical_output() {
        let fixture = fixture();
        let zstd = hex_decode(&fixture.zstd_envelope_hex);
        let golden_payload = hex_decode("28b52ffd2403190000616263990977ad");

        let cases = [
            ("invalid magic", vec![0_u8; 8]),
            ("skippable", hex_decode("502a4d1800000000")),
            ("missing content size", hex_decode("28b52ffd040000")),
            ("checksum disabled", {
                let mut payload = golden_payload.clone();
                payload[4] &= !0x04;
                payload
            }),
            ("content size mismatch", {
                let mut payload = golden_payload.clone();
                payload[5] = 4;
                payload
            }),
            ("concatenated frames", {
                let mut payload = golden_payload.clone();
                payload.extend_from_slice(&golden_payload);
                payload
            }),
        ];

        for (name, payload) in cases {
            let mut bytes = zstd.clone();
            replace_payload(&mut bytes, &payload);
            let error = PhysicalRecordV1::decode(&bytes, &NeverCancelled)
                .expect_err(&format!("{name} must fail"));
            assert_eq!(error.stage(), "invalid_zstd_profile", "{name}");
        }

        let mut oversized_window = zstd;
        oversized_window[16..24].copy_from_slice(&256_u64.to_le_bytes());
        replace_payload(&mut oversized_window, &hex_decode("28b52ffd44690000010000"));
        assert_eq!(
            PhysicalRecordV1::decode(&oversized_window, &NeverCancelled)
                .expect_err("oversized window")
                .stage(),
            "invalid_zstd_profile"
        );
    }

    #[test]
    fn unknown_transform_and_critical_extensions_fail_closed() {
        let fixture = fixture();
        let raw = hex_decode(&fixture.raw_envelope_hex);

        let mut unknown_transform = raw.clone();
        unknown_transform[9] = 1;
        unknown_transform[128..130].copy_from_slice(&99_u16.to_le_bytes());
        rehash_header(&mut unknown_transform);
        assert!(matches!(
            PhysicalRecordV1::decode(&unknown_transform, &NeverCancelled),
            Err(PhysicalCodecError::UnsupportedTransform)
        ));

        let mut noncritical = raw.clone();
        add_empty_extension(&mut noncritical, false);
        assert!(PhysicalRecordV1::decode(&noncritical, &NeverCancelled).is_ok());

        let mut critical = raw;
        add_empty_extension(&mut critical, true);
        assert!(matches!(
            PhysicalRecordV1::decode(&critical, &NeverCancelled),
            Err(PhysicalCodecError::UnsupportedCriticalExtension)
        ));
    }

    #[test]
    fn every_record_kind_rejects_one_byte_over_its_bound() {
        let fixture = fixture();
        let raw = hex_decode(&fixture.raw_envelope_hex);
        for kind in [
            PhysicalRecordKindV1::Event,
            PhysicalRecordKindV1::ObjectArtifact,
            PhysicalRecordKindV1::NoteBody,
            PhysicalRecordKindV1::DocumentManifest,
            PhysicalRecordKindV1::DocumentBlob,
            PhysicalRecordKindV1::RelationProof,
        ] {
            let mut oversized = raw.clone();
            oversized[8] = kind as u8;
            oversized[16..24].copy_from_slice(&(kind.decoded_cap() + 1).to_le_bytes());
            rehash_header(&mut oversized);
            assert!(matches!(
                PhysicalRecordV1::decode(&oversized, &NeverCancelled),
                Err(PhysicalCodecError::SizeLimit { .. })
            ));
        }
    }

    #[test]
    fn decoded_digest_mismatch_is_distinct_from_encoded_corruption() {
        let fixture = fixture();
        let mut raw = hex_decode(&fixture.raw_envelope_hex);
        raw[64] ^= 0xff;
        rehash_header(&mut raw);

        assert!(matches!(
            PhysicalRecordV1::decode(&raw, &NeverCancelled),
            Err(PhysicalCodecError::DecodedHashMismatch)
        ));
    }

    #[test]
    fn store_profile_header_round_trips_and_rejects_corruption() {
        let header = PhysicalStoreHeaderV1::new(1, [0x5a; 16]);
        let encoded = header.encode();

        assert_eq!(encoded.len(), PHYSICAL_STORE_HEADER_LEN_V1);
        assert_eq!(
            PhysicalStoreHeaderV1::decode(&encoded).expect("header"),
            header
        );

        let mut corrupt = encoded;
        corrupt[32] = 1;
        assert!(matches!(
            PhysicalStoreHeaderV1::decode(&corrupt),
            Err(PhysicalCodecError::InvalidPhysicalHeader { .. })
        ));
    }

    #[test]
    fn cancellation_never_returns_partial_decoded_bytes() {
        let fixture = fixture();
        let zstd = hex_decode(&fixture.zstd_envelope_hex);

        assert!(matches!(
            PhysicalRecordV1::decode(&zstd, &AlwaysCancelled),
            Err(PhysicalCodecError::Cancelled)
        ));
    }
}
