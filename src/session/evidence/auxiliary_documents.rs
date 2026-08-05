use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::EvidenceAvailabilityV1;
use crate::canonical_hash::{sha256_bytes_hex, sha256_json_prefixed};
use crate::error::{Result, ShoreError};
use crate::model::RevisionRefV1;

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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentRetentionPolicyV1 {
    pub retain: bool,
    pub consent: Option<String>,
    pub sensitivity: DocumentSensitivityV1,
    pub max_file_bytes: u64,
    pub max_capture_bytes: u64,
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuxiliaryDocumentEntryV1 {
    /// Hash of the canonical relative path; raw private paths are not retained.
    pub path_identity: String,
    pub side: DocumentSideV1,
    pub state: AuxiliaryDocumentStateV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuxiliaryDocumentManifestV1 {
    pub schema: String,
    pub version: u32,
    pub revision: RevisionRefV1,
    pub retention_policy: DocumentRetentionPolicyV1,
    pub entries: Vec<AuxiliaryDocumentEntryV1>,
    pub child_content_hashes: Vec<String>,
    pub retained_decoded_bytes: u64,
    pub manifest_sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestPreimage<'a> {
    schema: &'a str,
    version: u32,
    revision: &'a RevisionRefV1,
    retention_policy: &'a DocumentRetentionPolicyV1,
    entries: &'a [AuxiliaryDocumentEntryV1],
    child_content_hashes: &'a [String],
    retained_decoded_bytes: u64,
}

impl AuxiliaryDocumentManifestV1 {
    pub fn new(
        revision: RevisionRefV1,
        retention_policy: DocumentRetentionPolicyV1,
        mut entries: Vec<AuxiliaryDocumentEntryV1>,
    ) -> Result<Self> {
        entries.sort_by(|left, right| {
            (&left.path_identity, left.side).cmp(&(&right.path_identity, right.side))
        });
        let child_content_hashes: Vec<String> = entries
            .iter()
            .filter_map(|entry| match &entry.state {
                AuxiliaryDocumentStateV1::Retained { decoded_sha256, .. } => {
                    Some(decoded_sha256.clone())
                }
                AuxiliaryDocumentStateV1::Absent { .. } => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let retained_decoded_bytes = entries.iter().try_fold(0_u64, |sum, entry| {
            let bytes = match entry.state {
                AuxiliaryDocumentStateV1::Retained { decoded_bytes, .. } => decoded_bytes,
                AuxiliaryDocumentStateV1::Absent { .. } => 0,
            };
            sum.checked_add(bytes).ok_or_else(|| {
                ShoreError::Message("auxiliary-document byte count overflow".to_owned())
            })
        })?;
        let mut manifest = Self {
            schema: AUXILIARY_DOCUMENT_SCHEMA_V1.to_owned(),
            version: 1,
            revision,
            retention_policy,
            entries,
            child_content_hashes,
            retained_decoded_bytes,
            manifest_sha256: String::new(),
        };
        manifest.validate_shape()?;
        manifest.manifest_sha256 = manifest.computed_manifest_sha256()?;
        Ok(manifest)
    }

    pub fn verify_children(
        &self,
        children: &BTreeMap<String, Vec<u8>>,
    ) -> Result<EvidenceAvailabilityV1> {
        self.validate_shape()?;
        if self.manifest_sha256 != self.computed_manifest_sha256()? {
            return Err(ShoreError::Message(
                "auxiliary-document manifest hash mismatch".to_owned(),
            ));
        }
        if children.keys().cloned().collect::<Vec<_>>() != self.child_content_hashes {
            return Err(ShoreError::Message(
                "auxiliary-document child closure differs from the manifest".to_owned(),
            ));
        }
        for entry in &self.entries {
            let AuxiliaryDocumentStateV1::Retained {
                decoded_sha256,
                decoded_bytes,
                content_kind,
                encoding,
            } = &entry.state
            else {
                continue;
            };
            let bytes = children.get(decoded_sha256).ok_or_else(|| {
                ShoreError::Message(format!(
                    "auxiliary-document child {decoded_sha256} is missing"
                ))
            })?;
            let actual = format!("sha256:{}", sha256_bytes_hex(bytes));
            if actual != *decoded_sha256 || u64::try_from(bytes.len()).ok() != Some(*decoded_bytes)
            {
                return Err(ShoreError::Message(
                    "auxiliary-document child hash or length mismatch".to_owned(),
                ));
            }
            if *content_kind == DocumentContentKindV1::Text
                && (*encoding != DocumentEncodingV1::Utf8 || std::str::from_utf8(bytes).is_err())
            {
                return Err(ShoreError::Message(
                    "text auxiliary-document child is not valid UTF-8".to_owned(),
                ));
            }
        }
        Ok(EvidenceAvailabilityV1::Available)
    }

    fn validate_shape(&self) -> Result<()> {
        let consent_valid = !self.retention_policy.retain
            || self
                .retention_policy
                .consent
                .as_deref()
                .is_some_and(|consent| !consent.trim().is_empty());
        if self.schema != AUXILIARY_DOCUMENT_SCHEMA_V1
            || self.version != 1
            || !consent_valid
            || self.retained_decoded_bytes > self.retention_policy.max_capture_bytes
        {
            return Err(ShoreError::Message(
                "auxiliary-document schema, consent, or capture bound is invalid".to_owned(),
            ));
        }
        if !self.entries.windows(2).all(|pair| {
            (&pair[0].path_identity, pair[0].side) < (&pair[1].path_identity, pair[1].side)
        }) {
            return Err(ShoreError::Message(
                "auxiliary-document entries must be canonically ordered and unique".to_owned(),
            ));
        }
        for entry in &self.entries {
            if !is_prefixed_sha256(&entry.path_identity) {
                return Err(ShoreError::Message(
                    "auxiliary-document path identity must be a prefixed SHA-256".to_owned(),
                ));
            }
            let retained_invalid = match &entry.state {
                AuxiliaryDocumentStateV1::Retained {
                    decoded_sha256,
                    decoded_bytes,
                    content_kind,
                    encoding,
                } => {
                    !self.retention_policy.retain
                        || !is_prefixed_sha256(decoded_sha256)
                        || *decoded_bytes > self.retention_policy.max_file_bytes
                        || (*content_kind == DocumentContentKindV1::Text
                            && *encoding != DocumentEncodingV1::Utf8)
                        || (*content_kind != DocumentContentKindV1::Text
                            && *encoding != DocumentEncodingV1::Raw)
                }
                AuxiliaryDocumentStateV1::Absent { .. } => false,
            };
            if retained_invalid {
                return Err(ShoreError::Message(
                    "auxiliary-document retained entry violates policy or encoding bounds"
                        .to_owned(),
                ));
            }
        }
        let expected_hashes: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| match &entry.state {
                AuxiliaryDocumentStateV1::Retained { decoded_sha256, .. } => {
                    Some(decoded_sha256.clone())
                }
                AuxiliaryDocumentStateV1::Absent { .. } => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let expected_bytes = self.entries.iter().try_fold(0_u64, |sum, entry| {
            let bytes = match entry.state {
                AuxiliaryDocumentStateV1::Retained { decoded_bytes, .. } => decoded_bytes,
                AuxiliaryDocumentStateV1::Absent { .. } => 0,
            };
            sum.checked_add(bytes).ok_or_else(|| {
                ShoreError::Message("auxiliary-document byte count overflow".to_owned())
            })
        })?;
        if self.child_content_hashes != expected_hashes
            || self.retained_decoded_bytes != expected_bytes
        {
            return Err(ShoreError::Message(
                "auxiliary-document closure or retained byte count is inconsistent".to_owned(),
            ));
        }
        Ok(())
    }

    fn computed_manifest_sha256(&self) -> Result<String> {
        sha256_json_prefixed(&serde_json::to_value(ManifestPreimage {
            schema: &self.schema,
            version: self.version,
            revision: &self.revision,
            retention_policy: &self.retention_policy,
            entries: &self.entries,
            child_content_hashes: &self.child_content_hashes,
            retained_decoded_bytes: self.retained_decoded_bytes,
        })?)
    }
}

fn is_prefixed_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RevisionId, RevisionRefV1};

    fn revision() -> RevisionRefV1 {
        RevisionRefV1::new(
            RevisionId::new("rev:sha256:test"),
            format!("sha256:{}", "a".repeat(64)),
        )
        .unwrap()
    }

    #[test]
    fn explicit_absence_is_valid_while_missing_or_hash_mismatched_children_fail_closed() {
        let manifest = AuxiliaryDocumentManifestV1::new(
            revision(),
            DocumentRetentionPolicyV1::not_retained(DocumentSensitivityV1::Internal, 1024, 4096),
            vec![AuxiliaryDocumentEntryV1 {
                path_identity: format!("sha256:{}", "b".repeat(64)),
                side: DocumentSideV1::Before,
                state: AuxiliaryDocumentStateV1::Absent {
                    reason: DocumentAbsenceReasonV1::PolicyNotRetained,
                },
            }],
        )
        .unwrap();
        assert_eq!(
            manifest.verify_children(&Default::default()).unwrap(),
            EvidenceAvailabilityV1::Available
        );

        let retained = AuxiliaryDocumentManifestV1::new(
            revision(),
            DocumentRetentionPolicyV1::retained(
                "owner",
                DocumentSensitivityV1::Internal,
                1024,
                4096,
            ),
            vec![AuxiliaryDocumentEntryV1 {
                path_identity: format!("sha256:{}", "c".repeat(64)),
                side: DocumentSideV1::After,
                state: AuxiliaryDocumentStateV1::Retained {
                    decoded_sha256: format!("sha256:{}", "d".repeat(64)),
                    decoded_bytes: 3,
                    content_kind: DocumentContentKindV1::Text,
                    encoding: DocumentEncodingV1::Utf8,
                },
            }],
        )
        .unwrap();
        assert!(retained.verify_children(&Default::default()).is_err());
        let mismatched_child = [(format!("sha256:{}", "d".repeat(64)), b"bad".to_vec())].into();
        assert!(retained.verify_children(&mismatched_child).is_err());
    }
}
