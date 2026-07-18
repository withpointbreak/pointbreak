use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    CanonicalContentKindV1, CanonicalRawEntryV1, QualificationContractError,
    QualificationCorpusManifestV1, QualificationRecordKindV1, QualificationRecordV1,
    RelationProofError, RelationProofManifestV1,
};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};

pub const AUXILIARY_DOCUMENT_SCHEMA_V1: &str = "pointbreak.auxiliary-documents.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentSideV1 {
    Before,
    After,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentContentKindV1 {
    Text,
    Binary,
    Symlink,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentEncodingV1 {
    Utf8,
    Raw,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentAbsenceReasonV1 {
    Added,
    Deleted,
    Unavailable,
    PolicyNotRetained,
    FileLimitExceeded,
    CaptureLimitExceeded,
    MutableTargetChanged,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentSensitivityV1 {
    Public,
    Internal,
    Confidential,
    Restricted,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct DocumentRetentionPolicyV1 {
    pub retain: bool,
    pub consent: Option<String>,
    pub sensitivity: DocumentSensitivityV1,
    pub max_file_bytes: u64,
    pub max_capture_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AuxiliaryDocumentManifestV1 {
    pub schema: String,
    pub generation_revision_id: String,
    pub object_artifact_content_hash: String,
    pub retention_policy: DocumentRetentionPolicyV1,
    pub entries: Vec<AuxiliaryDocumentEntryV1>,
    pub child_content_hashes: Vec<String>,
    pub retained_decoded_bytes: u64,
    pub manifest_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AuxiliaryDocumentEntryV1 {
    pub raw_entry: CanonicalRawEntryV1,
    pub side: DocumentSideV1,
    pub state: AuxiliaryDocumentStateV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AuxiliaryDocumentStateV1 {
    Retained {
        decoded_sha256: String,
        decoded_bytes: u64,
        content_kind: DocumentContentKindV1,
        encoding: DocumentEncodingV1,
    },
    Absent {
        reason: DocumentAbsenceReasonV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuxiliaryDocumentCaptureV1 {
    pub manifest: AuxiliaryDocumentManifestV1,
    pub blobs: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentSideInputV1 {
    raw_entry: CanonicalRawEntryV1,
    side: DocumentSideV1,
    source: DocumentSideSourceV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DocumentSideSourceV1 {
    Retained {
        bytes: Vec<u8>,
        content_kind: DocumentContentKindV1,
        encoding: DocumentEncodingV1,
    },
    Absent {
        reason: DocumentAbsenceReasonV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuxiliaryDocumentError {
    #[error("document identity fields must not be empty")]
    EmptyIdentity,
    #[error("document retention requires explicit consent")]
    ExplicitConsentRequired,
    #[error("mutable document {path} changed during capture")]
    ConcurrentMutation { path: String },
    #[error("document {path} {side:?} hash {actual} does not match canonical entry {expected}")]
    CanonicalEntryHashMismatch {
        path: String,
        side: DocumentSideV1,
        expected: String,
        actual: String,
    },
    #[error("document {path} has no canonical hash for side {side:?}")]
    MissingCanonicalHash { path: String, side: DocumentSideV1 },
    #[error("document kind {document_kind:?} does not match canonical kind {canonical_kind:?}")]
    ContentKindMismatch {
        document_kind: DocumentContentKindV1,
        canonical_kind: CanonicalContentKindV1,
    },
    #[error("document kind {content_kind:?} cannot use encoding {encoding:?}")]
    EncodingMismatch {
        content_kind: DocumentContentKindV1,
        encoding: DocumentEncodingV1,
    },
    #[error("document {path} {side:?} is not valid UTF-8")]
    InvalidUtf8 { path: String, side: DocumentSideV1 },
    #[error("document manifest could not be canonicalized: {message}")]
    Canonicalization { message: String },
    #[error(transparent)]
    Contract(#[from] QualificationContractError),
    #[error(transparent)]
    Proof(#[from] RelationProofError),
}

#[derive(Serialize)]
struct AuxiliaryDocumentHashPreimage<'a> {
    schema: &'a str,
    generation_revision_id: &'a str,
    object_artifact_content_hash: &'a str,
    retention_policy: &'a DocumentRetentionPolicyV1,
    entries: &'a [AuxiliaryDocumentEntryV1],
    child_content_hashes: &'a [String],
    retained_decoded_bytes: u64,
}

impl DocumentRetentionPolicyV1 {
    pub fn retained(
        consent: impl Into<String>,
        sensitivity: DocumentSensitivityV1,
        max_file_bytes: u64,
        max_capture_bytes: u64,
    ) -> Self {
        Self {
            retain: true,
            consent: Some(consent.into()),
            sensitivity,
            max_file_bytes,
            max_capture_bytes,
        }
    }

    pub fn not_retained(
        sensitivity: DocumentSensitivityV1,
        max_file_bytes: u64,
        max_capture_bytes: u64,
    ) -> Self {
        Self {
            retain: false,
            consent: None,
            sensitivity,
            max_file_bytes,
            max_capture_bytes,
        }
    }
}

impl DocumentSideInputV1 {
    pub fn retained(
        raw_entry: CanonicalRawEntryV1,
        side: DocumentSideV1,
        bytes: Vec<u8>,
        content_kind: DocumentContentKindV1,
        encoding: DocumentEncodingV1,
    ) -> Self {
        Self {
            raw_entry,
            side,
            source: DocumentSideSourceV1::Retained {
                bytes,
                content_kind,
                encoding,
            },
        }
    }

    pub fn absent(
        raw_entry: CanonicalRawEntryV1,
        side: DocumentSideV1,
        reason: DocumentAbsenceReasonV1,
    ) -> Self {
        Self {
            raw_entry,
            side,
            source: DocumentSideSourceV1::Absent { reason },
        }
    }
}

pub fn snapshot_and_verify_mutable_document_v1(
    raw_entry: CanonicalRawEntryV1,
    side: DocumentSideV1,
    snapshot_before_diff: Vec<u8>,
    bytes_after_capture: Vec<u8>,
    content_kind: DocumentContentKindV1,
    encoding: DocumentEncodingV1,
) -> Result<DocumentSideInputV1, AuxiliaryDocumentError> {
    if snapshot_before_diff != bytes_after_capture {
        return Err(AuxiliaryDocumentError::ConcurrentMutation {
            path: raw_entry.path,
        });
    }
    validate_side_hash(&raw_entry, side, &snapshot_before_diff)?;
    validate_content_kind(raw_entry.content_kind, content_kind)?;
    validate_encoding(
        &raw_entry.path,
        side,
        &snapshot_before_diff,
        content_kind,
        encoding,
    )?;
    Ok(DocumentSideInputV1::retained(
        raw_entry,
        side,
        snapshot_before_diff,
        content_kind,
        encoding,
    ))
}

pub fn capture_auxiliary_documents_v1(
    generation_revision_id: impl Into<String>,
    object_artifact_content_hash: impl Into<String>,
    retention_policy: DocumentRetentionPolicyV1,
    mut inputs: Vec<DocumentSideInputV1>,
) -> Result<AuxiliaryDocumentCaptureV1, AuxiliaryDocumentError> {
    let generation_revision_id = generation_revision_id.into();
    let object_artifact_content_hash = object_artifact_content_hash.into();
    if generation_revision_id.trim().is_empty() || object_artifact_content_hash.trim().is_empty() {
        return Err(AuxiliaryDocumentError::EmptyIdentity);
    }
    if retention_policy.retain && !retention_policy.has_explicit_consent() {
        return Err(AuxiliaryDocumentError::ExplicitConsentRequired);
    }

    inputs.sort_by(|left, right| {
        (&left.raw_entry.path, left.side).cmp(&(&right.raw_entry.path, right.side))
    });
    let mut entries = Vec::with_capacity(inputs.len());
    let mut blobs = BTreeMap::new();
    let mut retained_decoded_bytes = 0_u64;
    for input in inputs {
        let state = if retention_policy.retain {
            capture_document_side(
                &input,
                &retention_policy,
                &mut retained_decoded_bytes,
                &mut blobs,
            )?
        } else {
            state_without_retention(&input.source)
        };
        entries.push(AuxiliaryDocumentEntryV1 {
            raw_entry: input.raw_entry,
            side: input.side,
            state,
        });
    }
    let child_content_hashes = blobs.keys().cloned().collect::<Vec<_>>();
    let mut manifest = AuxiliaryDocumentManifestV1 {
        schema: AUXILIARY_DOCUMENT_SCHEMA_V1.to_owned(),
        generation_revision_id,
        object_artifact_content_hash,
        retention_policy,
        entries,
        child_content_hashes,
        retained_decoded_bytes,
        manifest_sha256: String::new(),
    };
    manifest.manifest_sha256 = manifest.computed_manifest_sha256()?;
    Ok(AuxiliaryDocumentCaptureV1 { manifest, blobs })
}

pub fn extend_modeled_workload_v1(
    base: QualificationCorpusManifestV1,
    proof: &RelationProofManifestV1,
    documents: &AuxiliaryDocumentCaptureV1,
) -> Result<QualificationCorpusManifestV1, AuxiliaryDocumentError> {
    let mut records = base.records;
    records.push(proof.to_qualification_record("artifacts/proofs/qualification-relation-v1.json")?);
    records.extend(documents.to_qualification_records()?);
    records.sort_by(|left, right| left.logical_key.cmp(&right.logical_key));
    Ok(QualificationCorpusManifestV1::new(base.source, records)?)
}

impl AuxiliaryDocumentCaptureV1 {
    pub fn to_qualification_records(
        &self,
    ) -> Result<Vec<QualificationRecordV1>, AuxiliaryDocumentError> {
        let value = serde_json::to_value(&self.manifest).map_err(canonicalization_error)?;
        let manifest_bytes = canonical_json_bytes(&value).map_err(canonicalization_error)?;
        let mut records = vec![QualificationRecordV1::new(
            "artifacts/documents/manifests/qualification-v1.json",
            QualificationRecordKindV1::DocumentManifest,
            manifest_bytes,
        )];
        records.extend(self.blobs.iter().map(|(hash, bytes)| {
            QualificationRecordV1::new(
                format!("artifacts/documents/blobs/{hash}.bin"),
                QualificationRecordKindV1::DocumentBlob,
                bytes.clone(),
            )
        }));
        records.sort_by(|left, right| left.logical_key.cmp(&right.logical_key));
        Ok(records)
    }
}

impl AuxiliaryDocumentManifestV1 {
    fn computed_manifest_sha256(&self) -> Result<String, AuxiliaryDocumentError> {
        let preimage = AuxiliaryDocumentHashPreimage {
            schema: &self.schema,
            generation_revision_id: &self.generation_revision_id,
            object_artifact_content_hash: &self.object_artifact_content_hash,
            retention_policy: &self.retention_policy,
            entries: &self.entries,
            child_content_hashes: &self.child_content_hashes,
            retained_decoded_bytes: self.retained_decoded_bytes,
        };
        let value = serde_json::to_value(preimage).map_err(canonicalization_error)?;
        let bytes = canonical_json_bytes(&value).map_err(canonicalization_error)?;
        Ok(sha256_bytes_hex(&bytes))
    }
}

impl DocumentRetentionPolicyV1 {
    fn has_explicit_consent(&self) -> bool {
        self.consent
            .as_deref()
            .map(str::trim)
            .is_some_and(|consent| !consent.is_empty())
    }
}

fn capture_document_side(
    input: &DocumentSideInputV1,
    policy: &DocumentRetentionPolicyV1,
    retained_decoded_bytes: &mut u64,
    blobs: &mut BTreeMap<String, Vec<u8>>,
) -> Result<AuxiliaryDocumentStateV1, AuxiliaryDocumentError> {
    let (bytes, content_kind, encoding) = match &input.source {
        DocumentSideSourceV1::Retained {
            bytes,
            content_kind,
            encoding,
        } => (bytes, *content_kind, *encoding),
        DocumentSideSourceV1::Absent { reason } => {
            return Ok(AuxiliaryDocumentStateV1::Absent { reason: *reason });
        }
    };

    validate_side_hash(&input.raw_entry, input.side, bytes)?;
    validate_content_kind(input.raw_entry.content_kind, content_kind)?;
    validate_encoding(
        &input.raw_entry.path,
        input.side,
        bytes,
        content_kind,
        encoding,
    )?;
    let decoded_bytes = bytes.len() as u64;
    if decoded_bytes > policy.max_file_bytes {
        return Ok(AuxiliaryDocumentStateV1::Absent {
            reason: DocumentAbsenceReasonV1::FileLimitExceeded,
        });
    }
    if retained_decoded_bytes.saturating_add(decoded_bytes) > policy.max_capture_bytes {
        return Ok(AuxiliaryDocumentStateV1::Absent {
            reason: DocumentAbsenceReasonV1::CaptureLimitExceeded,
        });
    }
    let decoded_sha256 = sha256_bytes_hex(bytes);
    *retained_decoded_bytes += decoded_bytes;
    blobs
        .entry(decoded_sha256.clone())
        .or_insert_with(|| bytes.clone());
    Ok(AuxiliaryDocumentStateV1::Retained {
        decoded_sha256,
        decoded_bytes,
        content_kind,
        encoding,
    })
}

fn state_without_retention(source: &DocumentSideSourceV1) -> AuxiliaryDocumentStateV1 {
    let reason = match source {
        DocumentSideSourceV1::Retained { .. } => DocumentAbsenceReasonV1::PolicyNotRetained,
        DocumentSideSourceV1::Absent { reason } => *reason,
    };
    AuxiliaryDocumentStateV1::Absent { reason }
}

fn validate_side_hash(
    raw_entry: &CanonicalRawEntryV1,
    side: DocumentSideV1,
    bytes: &[u8],
) -> Result<(), AuxiliaryDocumentError> {
    let expected = match side {
        DocumentSideV1::Before => raw_entry.old_decoded_sha256.as_ref(),
        DocumentSideV1::After => raw_entry.new_decoded_sha256.as_ref(),
    }
    .ok_or_else(|| AuxiliaryDocumentError::MissingCanonicalHash {
        path: raw_entry.path.clone(),
        side,
    })?;
    let actual = sha256_bytes_hex(bytes);
    if &actual != expected {
        return Err(AuxiliaryDocumentError::CanonicalEntryHashMismatch {
            path: raw_entry.path.clone(),
            side,
            expected: expected.clone(),
            actual,
        });
    }
    Ok(())
}

fn validate_content_kind(
    canonical_kind: CanonicalContentKindV1,
    document_kind: DocumentContentKindV1,
) -> Result<(), AuxiliaryDocumentError> {
    let matches = matches!(
        (canonical_kind, document_kind),
        (CanonicalContentKindV1::Text, DocumentContentKindV1::Text)
            | (
                CanonicalContentKindV1::Binary,
                DocumentContentKindV1::Binary
            )
            | (
                CanonicalContentKindV1::Symlink,
                DocumentContentKindV1::Symlink
            )
    );
    if matches {
        Ok(())
    } else {
        Err(AuxiliaryDocumentError::ContentKindMismatch {
            document_kind,
            canonical_kind,
        })
    }
}

fn validate_encoding(
    path: &str,
    side: DocumentSideV1,
    bytes: &[u8],
    content_kind: DocumentContentKindV1,
    encoding: DocumentEncodingV1,
) -> Result<(), AuxiliaryDocumentError> {
    match (content_kind, encoding) {
        (DocumentContentKindV1::Text, DocumentEncodingV1::Utf8) => std::str::from_utf8(bytes)
            .map(|_| ())
            .map_err(|_| AuxiliaryDocumentError::InvalidUtf8 {
                path: path.to_owned(),
                side,
            }),
        (
            DocumentContentKindV1::Binary | DocumentContentKindV1::Symlink,
            DocumentEncodingV1::Raw,
        ) => Ok(()),
        _ => Err(AuxiliaryDocumentError::EncodingMismatch {
            content_kind,
            encoding,
        }),
    }
}

fn canonicalization_error(error: impl std::fmt::Display) -> AuxiliaryDocumentError {
    AuxiliaryDocumentError::Canonicalization {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::bench_support::foundation::{
        CanonicalChangeV1, CanonicalContentKindV1, CanonicalProofInputV1, CanonicalRawEntryV1,
        ProofCaptureModeV1, ProofGitAvailabilityV1, QualificationRecordKindV1,
        RelationProofAlgorithmV1, evaluate_relation_proof_v1, modeled_post_foundation_manifest,
    };
    use crate::canonical_hash::sha256_bytes_hex;

    const DOCUMENT_MATRIX: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/store-foundation/documents/matrix.json"
    ));

    #[derive(Deserialize)]
    struct DocumentMatrixRowV1 {
        scenario: String,
        side: DocumentSideV1,
        content_kind: Option<DocumentContentKindV1>,
        encoding: Option<DocumentEncodingV1>,
        absence_reason: Option<DocumentAbsenceReasonV1>,
    }

    fn raw_entry(path: &str, bytes: &[u8], kind: CanonicalContentKindV1) -> CanonicalRawEntryV1 {
        CanonicalRawEntryV1 {
            path: path.to_owned(),
            previous_path: None,
            change: CanonicalChangeV1::Modified,
            old_oid: Some(format!("oid:old:{path}")),
            new_oid: None,
            old_mode: Some("100644".to_owned()),
            new_mode: Some("100644".to_owned()),
            old_decoded_sha256: Some(sha256_bytes_hex(b"old")),
            new_decoded_sha256: Some(sha256_bytes_hex(bytes)),
            content_kind: kind,
            untracked: false,
        }
    }

    fn retained_policy(max_file_bytes: u64, max_capture_bytes: u64) -> DocumentRetentionPolicyV1 {
        DocumentRetentionPolicyV1::retained(
            "consent:synthetic-fixture",
            DocumentSensitivityV1::Internal,
            max_file_bytes,
            max_capture_bytes,
        )
    }

    #[test]
    fn public_document_fixture_covers_required_truth_table_rows() {
        let rows = serde_json::from_str::<Vec<DocumentMatrixRowV1>>(DOCUMENT_MATRIX)
            .expect("valid document matrix");
        let scenarios = rows
            .iter()
            .map(|row| row.scenario.as_str())
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(
            scenarios,
            std::collections::BTreeSet::from([
                "binary",
                "deleted",
                "empty",
                "policy_not_retained",
                "textual",
                "unavailable",
            ])
        );
        assert!(rows.iter().all(|row| row.side == DocumentSideV1::After));
        assert!(rows.iter().any(|row| {
            row.content_kind == Some(DocumentContentKindV1::Binary)
                && row.encoding == Some(DocumentEncodingV1::Raw)
        }));
        assert!(
            rows.iter().any(|row| {
                row.absence_reason == Some(DocumentAbsenceReasonV1::PolicyNotRetained)
            })
        );
    }

    #[test]
    fn mutable_snapshot_fails_closed_on_concurrent_change() {
        let snapshot = b"before mutation".to_vec();
        let entry = raw_entry("src/lib.rs", &snapshot, CanonicalContentKindV1::Text);
        let error = snapshot_and_verify_mutable_document_v1(
            entry,
            DocumentSideV1::After,
            snapshot,
            b"after mutation".to_vec(),
            DocumentContentKindV1::Text,
            DocumentEncodingV1::Utf8,
        )
        .expect_err("concurrent mutation must fail");

        assert!(matches!(
            error,
            AuxiliaryDocumentError::ConcurrentMutation { .. }
        ));
    }

    #[test]
    fn retained_document_encoding_must_match_kind_and_bytes() {
        let invalid_utf8 = vec![0xff, 0xfe];
        let invalid_utf8_error = capture_auxiliary_documents_v1(
            "rev:fixture",
            "sha256:object-fixture",
            retained_policy(1024, 4096),
            vec![DocumentSideInputV1::retained(
                raw_entry("invalid.txt", &invalid_utf8, CanonicalContentKindV1::Text),
                DocumentSideV1::After,
                invalid_utf8,
                DocumentContentKindV1::Text,
                DocumentEncodingV1::Utf8,
            )],
        )
        .expect_err("invalid UTF-8 must fail closed");
        assert!(matches!(
            invalid_utf8_error,
            AuxiliaryDocumentError::InvalidUtf8 { .. }
        ));

        let bytes = b"text mislabeled as raw".to_vec();
        let encoding_error = capture_auxiliary_documents_v1(
            "rev:fixture",
            "sha256:object-fixture",
            retained_policy(1024, 4096),
            vec![DocumentSideInputV1::retained(
                raw_entry("mislabeled.txt", &bytes, CanonicalContentKindV1::Text),
                DocumentSideV1::After,
                bytes,
                DocumentContentKindV1::Text,
                DocumentEncodingV1::Raw,
            )],
        )
        .expect_err("kind/encoding mismatch must fail closed");
        assert!(matches!(
            encoding_error,
            AuxiliaryDocumentError::EncodingMismatch { .. }
        ));
    }

    #[test]
    fn manifest_preserves_text_binary_empty_and_typed_absence_rows() {
        let text = b"hello\n".to_vec();
        let binary = vec![0, 159, 146, 150];
        let empty = Vec::new();
        let mut deleted = raw_entry("deleted.txt", b"", CanonicalContentKindV1::Text);
        deleted.change = CanonicalChangeV1::Deleted;
        deleted.new_decoded_sha256 = None;
        let inputs = vec![
            DocumentSideInputV1::retained(
                raw_entry("text.txt", &text, CanonicalContentKindV1::Text),
                DocumentSideV1::After,
                text,
                DocumentContentKindV1::Text,
                DocumentEncodingV1::Utf8,
            ),
            DocumentSideInputV1::retained(
                raw_entry("image.bin", &binary, CanonicalContentKindV1::Binary),
                DocumentSideV1::After,
                binary,
                DocumentContentKindV1::Binary,
                DocumentEncodingV1::Raw,
            ),
            DocumentSideInputV1::retained(
                raw_entry("empty.txt", &empty, CanonicalContentKindV1::Text),
                DocumentSideV1::After,
                empty,
                DocumentContentKindV1::Text,
                DocumentEncodingV1::Utf8,
            ),
            DocumentSideInputV1::absent(
                deleted,
                DocumentSideV1::After,
                DocumentAbsenceReasonV1::Deleted,
            ),
            DocumentSideInputV1::absent(
                raw_entry("missing.txt", b"", CanonicalContentKindV1::Text),
                DocumentSideV1::After,
                DocumentAbsenceReasonV1::Unavailable,
            ),
        ];
        let capture = capture_auxiliary_documents_v1(
            "rev:fixture",
            "sha256:object-fixture",
            retained_policy(1024, 4096),
            inputs,
        )
        .expect("document capture");

        assert_eq!(capture.manifest.entries.len(), 5);
        assert_eq!(capture.blobs.len(), 3);
        assert_eq!(capture.manifest.child_content_hashes.len(), 3);
        assert!(capture.manifest.entries.iter().any(|entry| {
            matches!(
                entry.state,
                AuxiliaryDocumentStateV1::Absent {
                    reason: DocumentAbsenceReasonV1::Deleted
                }
            )
        }));
        assert!(capture.manifest.entries.iter().any(|entry| {
            matches!(
                entry.state,
                AuxiliaryDocumentStateV1::Absent {
                    reason: DocumentAbsenceReasonV1::Unavailable
                }
            )
        }));
    }

    #[test]
    fn retention_requires_consent_and_enforces_file_and_capture_bounds() {
        let entry = raw_entry("large.txt", b"12345", CanonicalContentKindV1::Text);
        let missing_consent = DocumentRetentionPolicyV1 {
            retain: true,
            consent: None,
            sensitivity: DocumentSensitivityV1::Confidential,
            max_file_bytes: 10,
            max_capture_bytes: 10,
        };
        assert!(matches!(
            capture_auxiliary_documents_v1(
                "rev:fixture",
                "sha256:object-fixture",
                missing_consent,
                vec![]
            ),
            Err(AuxiliaryDocumentError::ExplicitConsentRequired)
        ));

        let capture = capture_auxiliary_documents_v1(
            "rev:fixture",
            "sha256:object-fixture",
            retained_policy(4, 4),
            vec![DocumentSideInputV1::retained(
                entry,
                DocumentSideV1::After,
                b"12345".to_vec(),
                DocumentContentKindV1::Text,
                DocumentEncodingV1::Utf8,
            )],
        )
        .expect("bounded capture");
        assert!(matches!(
            capture.manifest.entries[0].state,
            AuxiliaryDocumentStateV1::Absent {
                reason: DocumentAbsenceReasonV1::FileLimitExceeded
            }
        ));
        assert!(capture.blobs.is_empty());

        let first = raw_entry("first.txt", b"1234", CanonicalContentKindV1::Text);
        let second = raw_entry("second.txt", b"5678", CanonicalContentKindV1::Text);
        let capture = capture_auxiliary_documents_v1(
            "rev:fixture",
            "sha256:object-fixture",
            retained_policy(4, 4),
            vec![
                DocumentSideInputV1::retained(
                    first,
                    DocumentSideV1::After,
                    b"1234".to_vec(),
                    DocumentContentKindV1::Text,
                    DocumentEncodingV1::Utf8,
                ),
                DocumentSideInputV1::retained(
                    second,
                    DocumentSideV1::After,
                    b"5678".to_vec(),
                    DocumentContentKindV1::Text,
                    DocumentEncodingV1::Utf8,
                ),
            ],
        )
        .expect("capture-bound omission");
        assert!(matches!(
            capture.manifest.entries[1].state,
            AuxiliaryDocumentStateV1::Absent {
                reason: DocumentAbsenceReasonV1::CaptureLimitExceeded
            }
        ));
    }

    #[test]
    fn policy_not_retained_is_explicit_and_manifest_contains_no_child_bytes() {
        let private_sentinel = b"private fixture sentinel".to_vec();
        let capture = capture_auxiliary_documents_v1(
            "rev:fixture",
            "sha256:object-fixture",
            DocumentRetentionPolicyV1::not_retained(DocumentSensitivityV1::Restricted, 1024, 4096),
            vec![DocumentSideInputV1::retained(
                raw_entry(
                    "secret.txt",
                    &private_sentinel,
                    CanonicalContentKindV1::Text,
                ),
                DocumentSideV1::After,
                private_sentinel.clone(),
                DocumentContentKindV1::Text,
                DocumentEncodingV1::Utf8,
            )],
        )
        .expect("policy omission");
        let serialized = serde_json::to_vec(&capture.manifest).expect("manifest JSON");

        assert!(matches!(
            capture.manifest.entries[0].state,
            AuxiliaryDocumentStateV1::Absent {
                reason: DocumentAbsenceReasonV1::PolicyNotRetained
            }
        ));
        assert!(capture.blobs.is_empty());
        assert!(
            !serialized
                .windows(private_sentinel.len())
                .any(|window| window == private_sentinel)
        );
    }

    #[test]
    fn proof_and_document_content_extend_the_modeled_workload() {
        let bytes = b"document fixture".to_vec();
        let entry = raw_entry("guide.md", &bytes, CanonicalContentKindV1::Text);
        let input = CanonicalProofInputV1 {
            capture_mode: ProofCaptureModeV1::CombinedWorktree,
            base: Some("commit:base".to_owned()),
            parent: Some("commit:base".to_owned()),
            path_scope: vec!["guide.md".to_owned()],
            git_availability: ProofGitAvailabilityV1::Available,
            entries: vec![entry.clone()],
        };
        let proof = evaluate_relation_proof_v1(
            "rev:fixture",
            "sha256:object-fixture",
            "association:fixture",
            RelationProofAlgorithmV1::ExactMaterialization,
            input.clone(),
            input,
        )
        .expect("proof");
        let documents = capture_auxiliary_documents_v1(
            "rev:fixture",
            "sha256:object-fixture",
            retained_policy(1024, 4096),
            vec![DocumentSideInputV1::retained(
                entry,
                DocumentSideV1::After,
                bytes,
                DocumentContentKindV1::Text,
                DocumentEncodingV1::Utf8,
            )],
        )
        .expect("documents");
        let base = modeled_post_foundation_manifest().expect("modeled workload");
        let extended = extend_modeled_workload_v1(base.clone(), &proof, &documents)
            .expect("extended workload");

        assert!(extended.records.len() > base.records.len());
        for kind in [
            QualificationRecordKindV1::RelationProof,
            QualificationRecordKindV1::DocumentManifest,
            QualificationRecordKindV1::DocumentBlob,
        ] {
            assert!(
                extended
                    .records
                    .iter()
                    .any(|record| record.record_kind == kind)
            );
        }
        assert!(extended.validate().is_ok());
    }
}
