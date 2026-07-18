use serde::{Deserialize, Serialize};

use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};

pub const QUALIFICATION_CORPUS_SCHEMA_V1: &str = "pointbreak.qualification-corpus.v1";
pub const FOUNDATION_CAPABILITY_EPOCH_V1: &str = "pointbreak.foundation.v1";

const FOUNDATION_REQUIRED_CAPABILITIES: [&str; 5] = [
    "auxiliary_document_v1",
    "commit_relation_attestation_v1",
    "relation_proof_evidence_v1",
    "review_continuation_v1",
    "review_fact_port_v1",
];

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct QualificationCorpusManifestV1 {
    pub schema: String,
    pub source: String,
    pub records: Vec<QualificationRecordV1>,
    pub manifest_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct QualificationRecordV1 {
    pub logical_key: String,
    pub record_kind: QualificationRecordKindV1,
    pub decoded_sha256: String,
    pub decoded_bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationRecordKindV1 {
    LegacyEvent,
    GenerationProposal,
    RelationAttestation,
    FactPort,
    ObjectArtifact,
    NoteBody,
    RelationProof,
    DocumentManifest,
    DocumentBlob,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct LogicalCapabilityEpochV1 {
    pub epoch: String,
    pub required: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualificationCreateOutcome {
    Created,
    AlreadyExists,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualificationEntry {
    pub logical_key: String,
    pub decoded_sha256: String,
    pub decoded_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct QualificationInventoryV1 {
    pub carriers: Vec<String>,
    pub logical_bytes: u64,
    pub encoded_bytes: u64,
    pub allocated_bytes: u64,
    pub high_water_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct QualificationProfileDescriptorV1 {
    pub physical_profile_id: String,
    pub logical_capabilities: LogicalCapabilityEpochV1,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum QualificationContractError {
    #[error("record {logical_key} has decoded hash {actual}, expected {expected}")]
    DecodedHashMismatch {
        logical_key: String,
        expected: String,
        actual: String,
    },
    #[error("logical key {logical_key} appears more than once")]
    DuplicateLogicalKey { logical_key: String },
    #[error("manifest is not sorted: {current} follows {previous}")]
    UnsortedManifest { previous: String, current: String },
    #[error("manifest hash {actual} does not match {expected}")]
    ManifestHashMismatch { expected: String, actual: String },
    #[error("unsupported qualification manifest schema {schema}")]
    UnsupportedSchema { schema: String },
    #[error("qualification manifest source must be a non-empty label")]
    EmptySource,
    #[error("unsupported logical capability epoch {epoch}")]
    UnsupportedCapabilityEpoch { epoch: String },
    #[error("required capabilities are not sorted: {current} follows {previous}")]
    UnsortedRequiredCapabilities { previous: String, current: String },
    #[error("required capability {capability} appears more than once")]
    DuplicateRequiredCapability { capability: String },
    #[error("unknown required capability {capability}")]
    UnknownRequiredCapability { capability: String },
    #[error("missing required capability {capability}")]
    MissingRequiredCapability { capability: String },
    #[error("qualification manifest could not be canonicalized: {message}")]
    Canonicalization { message: String },
}

#[derive(Serialize)]
struct ManifestHashPreimage<'a> {
    schema: &'a str,
    source: &'a str,
    records: &'a [QualificationRecordV1],
}

impl QualificationRecordV1 {
    pub fn new(
        logical_key: impl Into<String>,
        record_kind: QualificationRecordKindV1,
        decoded_bytes: Vec<u8>,
    ) -> Self {
        let decoded_sha256 = sha256_bytes_hex(&decoded_bytes);
        Self {
            logical_key: logical_key.into(),
            record_kind,
            decoded_sha256,
            decoded_bytes,
        }
    }

    pub fn validate(&self) -> Result<(), QualificationContractError> {
        let expected = sha256_bytes_hex(&self.decoded_bytes);
        if self.decoded_sha256 != expected {
            return Err(QualificationContractError::DecodedHashMismatch {
                logical_key: self.logical_key.clone(),
                expected,
                actual: self.decoded_sha256.clone(),
            });
        }
        Ok(())
    }
}

impl QualificationCorpusManifestV1 {
    pub fn new(
        source: impl Into<String>,
        records: Vec<QualificationRecordV1>,
    ) -> Result<Self, QualificationContractError> {
        let mut manifest = Self {
            schema: QUALIFICATION_CORPUS_SCHEMA_V1.to_owned(),
            source: source.into(),
            records,
            manifest_sha256: String::new(),
        };
        manifest.manifest_sha256 = manifest.computed_manifest_sha256()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), QualificationContractError> {
        if self.schema != QUALIFICATION_CORPUS_SCHEMA_V1 {
            return Err(QualificationContractError::UnsupportedSchema {
                schema: self.schema.clone(),
            });
        }
        if self.source.trim().is_empty() {
            return Err(QualificationContractError::EmptySource);
        }

        for record in &self.records {
            record.validate()?;
        }
        for records in self.records.windows(2) {
            let previous = &records[0].logical_key;
            let current = &records[1].logical_key;
            if current == previous {
                return Err(QualificationContractError::DuplicateLogicalKey {
                    logical_key: current.clone(),
                });
            }
            if current < previous {
                return Err(QualificationContractError::UnsortedManifest {
                    previous: previous.clone(),
                    current: current.clone(),
                });
            }
        }

        let expected = self.computed_manifest_sha256()?;
        if self.manifest_sha256 != expected {
            return Err(QualificationContractError::ManifestHashMismatch {
                expected,
                actual: self.manifest_sha256.clone(),
            });
        }
        Ok(())
    }

    fn computed_manifest_sha256(&self) -> Result<String, QualificationContractError> {
        let preimage = ManifestHashPreimage {
            schema: &self.schema,
            source: &self.source,
            records: &self.records,
        };
        let value = serde_json::to_value(preimage).map_err(|error| {
            QualificationContractError::Canonicalization {
                message: error.to_string(),
            }
        })?;
        let bytes = canonical_json_bytes(&value).map_err(|error| {
            QualificationContractError::Canonicalization {
                message: error.to_string(),
            }
        })?;
        Ok(sha256_bytes_hex(&bytes))
    }
}

impl LogicalCapabilityEpochV1 {
    pub fn foundation() -> Self {
        Self {
            epoch: FOUNDATION_CAPABILITY_EPOCH_V1.to_owned(),
            required: FOUNDATION_REQUIRED_CAPABILITIES
                .iter()
                .map(|capability| (*capability).to_owned())
                .collect(),
        }
    }

    pub fn validate(&self) -> Result<(), QualificationContractError> {
        if self.epoch != FOUNDATION_CAPABILITY_EPOCH_V1 {
            return Err(QualificationContractError::UnsupportedCapabilityEpoch {
                epoch: self.epoch.clone(),
            });
        }
        for capabilities in self.required.windows(2) {
            let previous = &capabilities[0];
            let current = &capabilities[1];
            if current == previous {
                return Err(QualificationContractError::DuplicateRequiredCapability {
                    capability: current.clone(),
                });
            }
            if current < previous {
                return Err(QualificationContractError::UnsortedRequiredCapabilities {
                    previous: previous.clone(),
                    current: current.clone(),
                });
            }
        }
        for capability in &self.required {
            if !FOUNDATION_REQUIRED_CAPABILITIES.contains(&capability.as_str()) {
                return Err(QualificationContractError::UnknownRequiredCapability {
                    capability: capability.clone(),
                });
            }
        }
        for capability in FOUNDATION_REQUIRED_CAPABILITIES {
            if !self.required.iter().any(|required| required == capability) {
                return Err(QualificationContractError::MissingRequiredCapability {
                    capability: capability.to_owned(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(logical_key: &str) -> QualificationRecordV1 {
        QualificationRecordV1::new(
            logical_key,
            QualificationRecordKindV1::LegacyEvent,
            br#"{"event":"fixture"}"#.to_vec(),
        )
    }

    fn manifest(records: Vec<QualificationRecordV1>) -> QualificationCorpusManifestV1 {
        QualificationCorpusManifestV1::new("synthetic-test", records).expect("valid manifest")
    }

    #[test]
    fn record_rejects_a_decoded_hash_mismatch() {
        let mut record = record("events/one.json");
        record.decoded_sha256 = "0".repeat(64);

        assert!(matches!(
            record.validate(),
            Err(QualificationContractError::DecodedHashMismatch { .. })
        ));
    }

    #[test]
    fn manifest_rejects_duplicate_and_unsorted_logical_keys() {
        let duplicate = vec![record("events/one.json"), record("events/one.json")];
        assert!(matches!(
            manifest(duplicate).validate(),
            Err(QualificationContractError::DuplicateLogicalKey { .. })
        ));

        let unsorted = vec![record("events/two.json"), record("events/one.json")];
        assert!(matches!(
            manifest(unsorted).validate(),
            Err(QualificationContractError::UnsortedManifest { .. })
        ));
    }

    #[test]
    fn capability_epoch_rejects_unknown_required_capabilities() {
        let mut capabilities = LogicalCapabilityEpochV1::foundation();
        capabilities
            .required
            .push("future_required_capability_v1".to_owned());
        capabilities.required.sort();

        assert_eq!(
            capabilities.validate(),
            Err(QualificationContractError::UnknownRequiredCapability {
                capability: "future_required_capability_v1".to_owned(),
            })
        );
    }

    #[test]
    fn manifest_rejects_a_bad_manifest_hash() {
        let mut manifest = manifest(vec![record("events/one.json")]);
        manifest.records[0] = QualificationRecordV1::new(
            "events/one.json",
            QualificationRecordKindV1::LegacyEvent,
            br#"{"event":"changed-after-hashing"}"#.to_vec(),
        );

        assert!(matches!(
            manifest.validate(),
            Err(QualificationContractError::ManifestHashMismatch { .. })
        ));
    }
}
