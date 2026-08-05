#![cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "writer-dark capability constructors are not routed yet"
    )
)]

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex, sha256_json_prefixed};
#[cfg(test)]
use crate::crypto::EventSigner;
use crate::crypto::{
    EventSignatureBytes, EventVerificationStatus, SignerId, verify_ed25519_strict,
};
use crate::error::{Result, ShoreError};
#[cfg(test)]
use crate::model::ActorId;
#[cfg(test)]
use crate::session::event::WriterProducer;
use crate::session::event::{Writer, event_type_from_code, pre_authentication_encoding};
use crate::session::store::backend::{Journal, JournalEntry};
use crate::session::store::event_store::EventStore;

pub(crate) const REVIEW_CHANGE_REVISION_COHORT_V1: &str = "review_change_revision_v1";
const CAPABILITY_MANIFEST_SCHEMA_V1: &str = "pointbreak.logical-capability-manifest";
const ACTIVATION_SCHEMA_V1: &str = "pointbreak.store-capability-activation";
const COMPLETION_SCHEMA_V1: &str = "pointbreak.bulk-adoption-completion";
const BULK_MANIFEST_SCHEMA_V1: &str = "pointbreak.bulk-adoption-manifest";
pub(crate) const AUTHORITY_CURSOR_SCHEMA_V2: &str = "pointbreak.authority-cursor.v2";
const CAPABILITY_SET_SCHEMA_V1: &str = "pointbreak.capability-set.v1";
const JOURNAL_RECORD_SET_SCHEMA_V1: &str = "pointbreak.journal-record-set.v1";
const EVENT_SET_SCHEMA_V1: &str = "shore.event-set.v1";
const ACTIVATION_TBS_SCHEMA_V1: &str = "pointbreak.store-capability-activation-tbs";
const COMPLETION_TBS_SCHEMA_V1: &str = "pointbreak.bulk-adoption-completion-tbs";
const ACTIVATION_TBS_PAYLOAD_TYPE_V1: &str =
    "application/vnd.pointbreak.store-capability-activation-tbs.v1+json";
const COMPLETION_TBS_PAYLOAD_TYPE_V1: &str =
    "application/vnd.pointbreak.bulk-adoption-completion-tbs.v1+json";
const ROOT_ACTIVATION_LOGICAL_KEY_V1: &str =
    "store_capability_activation:review_change_revision_v1:root";

pub(crate) const REVIEW_CHANGE_REVISION_REQUIRED_CAPABILITIES_V1: &[&str] = &[
    "auxiliary_document_blob_v1",
    "auxiliary_document_manifest_v1",
    "relation_proof_v1",
    "review_fact_port_v1",
    "revision_relation_attestation_v1",
    "stable_change_membership_v1",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecordFamilyReservationV1 {
    pub(crate) name: &'static str,
    pub(crate) event_type_code: &'static str,
    pub(crate) payload_schema: &'static str,
    pub(crate) payload_version: u32,
}

pub(crate) const REVIEW_CHANGE_REVISION_RECORD_FAMILIES_V1: &[RecordFamilyReservationV1] = &[
    RecordFamilyReservationV1 {
        name: "change_declared_v1",
        event_type_code: "t:17",
        payload_schema: "pointbreak.change-declared",
        payload_version: 1,
    },
    RecordFamilyReservationV1 {
        name: "change_membership_asserted_v1",
        event_type_code: "t:18",
        payload_schema: "pointbreak.change-membership-asserted",
        payload_version: 1,
    },
    RecordFamilyReservationV1 {
        name: "change_membership_withdrawn_v1",
        event_type_code: "t:19",
        payload_schema: "pointbreak.change-membership-withdrawn",
        payload_version: 1,
    },
    RecordFamilyReservationV1 {
        name: "change_link_asserted_v1",
        event_type_code: "t:20",
        payload_schema: "pointbreak.change-link-asserted",
        payload_version: 1,
    },
    RecordFamilyReservationV1 {
        name: "change_revision_relation_asserted_v1",
        event_type_code: "t:21",
        payload_schema: "pointbreak.change-revision-relation-asserted",
        payload_version: 1,
    },
    RecordFamilyReservationV1 {
        name: "change_revision_relation_withdrawn_v1",
        event_type_code: "t:22",
        payload_schema: "pointbreak.change-revision-relation-withdrawn",
        payload_version: 1,
    },
    RecordFamilyReservationV1 {
        name: "revision_relation_attested_v1",
        event_type_code: "t:23",
        payload_schema: "pointbreak.revision-relation-attested",
        payload_version: 1,
    },
    RecordFamilyReservationV1 {
        name: "review_fact_ported_v1",
        event_type_code: "t:24",
        payload_schema: "pointbreak.review-fact-ported",
        payload_version: 1,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentVersionReservationV1 {
    pub(crate) schema: &'static str,
    pub(crate) version: u32,
}

pub(crate) const READER_PROFILE_DOCUMENT_VERSIONS_V1: &[DocumentVersionReservationV1] = &[
    DocumentVersionReservationV1 {
        schema: "pointbreak.inspect-reader-profile",
        version: 1,
    },
    DocumentVersionReservationV1 {
        schema: "pointbreak.review-change-list",
        version: 1,
    },
    DocumentVersionReservationV1 {
        schema: "pointbreak.inspect-changes-page",
        version: 1,
    },
    DocumentVersionReservationV1 {
        schema: "pointbreak.review-change",
        version: 1,
    },
    DocumentVersionReservationV1 {
        schema: "pointbreak.review-change-revision",
        version: 1,
    },
    DocumentVersionReservationV1 {
        schema: "pointbreak.review-revision",
        version: 3,
    },
    DocumentVersionReservationV1 {
        schema: "pointbreak.review-revision-resource",
        version: 1,
    },
    DocumentVersionReservationV1 {
        schema: "pointbreak.review-association-comparison",
        version: 1,
    },
    DocumentVersionReservationV1 {
        schema: "pointbreak.review-revision-interdiff",
        version: 1,
    },
    DocumentVersionReservationV1 {
        schema: "pointbreak.attention-list",
        version: 2,
    },
    DocumentVersionReservationV1 {
        schema: "pointbreak.inspect-attention",
        version: 2,
    },
    DocumentVersionReservationV1 {
        schema: "pointbreak.reader-upgrade-required",
        version: 1,
    },
];

pub(crate) fn reader_profile_versions_v1() -> &'static [DocumentVersionReservationV1] {
    READER_PROFILE_DOCUMENT_VERSIONS_V1
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewChangeRevisionManifestV1 {
    schema: &'static str,
    version: u32,
    pub(crate) cohort: &'static str,
    pub(crate) minimum_reader_profile: &'static str,
    pub(crate) required_capabilities: &'static [&'static str],
    pub(crate) record_families: &'static [RecordFamilyReservationV1],
}

impl ReviewChangeRevisionManifestV1 {
    pub(crate) const fn canonical() -> Self {
        Self {
            schema: CAPABILITY_MANIFEST_SCHEMA_V1,
            version: 1,
            cohort: REVIEW_CHANGE_REVISION_COHORT_V1,
            minimum_reader_profile: REVIEW_CHANGE_REVISION_COHORT_V1,
            required_capabilities: REVIEW_CHANGE_REVISION_REQUIRED_CAPABILITIES_V1,
            record_families: REVIEW_CHANGE_REVISION_RECORD_FAMILIES_V1,
        }
    }

    pub(crate) fn canonical_hash(&self) -> Result<String> {
        sha256_json_prefixed(&serde_json::to_value(self)?)
    }
}

// Frozen after the canonical-manifest test first records the implementation's
// canonical bytes. Any later drift is a compatibility break, not a test update.
pub(crate) const REVIEW_CHANGE_REVISION_MANIFEST_HASH_V1: &str =
    "sha256:3ed9b86dfba44b448df98090073dced4a67d26e37e1f71276ce07ff1c5bbb948";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorityCursorV2 {
    pub schema: String,
    pub journal_record_count: u64,
    pub event_count: u64,
    pub journal_record_set_hash: String,
    pub event_set_hash: String,
    pub capability_set_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReservedCohortRecordV1 {
    pub(crate) logical_key: String,
    pub(crate) record_family: String,
    pub(crate) record_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BulkAdoptionManifestV1 {
    pub(crate) schema: String,
    pub(crate) version: u32,
    pub(crate) cohort_manifest_hash: String,
    pub(crate) source_authority_cursor: AuthorityCursorV2,
    pub(crate) reserved_records: Vec<ReservedCohortRecordV1>,
}

impl BulkAdoptionManifestV1 {
    pub(crate) fn from_reserved_records(
        source_authority_cursor: AuthorityCursorV2,
        mut reserved_records: Vec<ReservedCohortRecordV1>,
    ) -> Result<Self> {
        reserved_records.sort_by(|left, right| left.logical_key.cmp(&right.logical_key));
        let manifest = Self {
            schema: BULK_MANIFEST_SCHEMA_V1.to_owned(),
            version: 1,
            cohort_manifest_hash: REVIEW_CHANGE_REVISION_MANIFEST_HASH_V1.to_owned(),
            source_authority_cursor,
            reserved_records,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub(crate) fn canonical_hash(&self) -> Result<String> {
        sha256_json_prefixed(&serde_json::to_value(self)?)
    }

    fn validate(&self) -> Result<()> {
        if self.schema != BULK_MANIFEST_SCHEMA_V1 || self.version != 1 {
            return capability_error("unsupported bulk-adoption manifest schema/version");
        }
        if self.cohort_manifest_hash != REVIEW_CHANGE_REVISION_MANIFEST_HASH_V1 {
            return capability_error(
                "bulk-adoption manifest names an unsupported capability manifest",
            );
        }
        validate_cursor_shape(&self.source_authority_cursor)?;
        ensure_sorted_unique_by(
            &self.reserved_records,
            |record| record.logical_key.as_str(),
            "reserved record logical keys",
        )?;
        for record in &self.reserved_records {
            if !is_prefixed_sha256(&record.record_hash) {
                return capability_error("bulk-adoption manifest has an invalid record hash");
            }
            if !REVIEW_CHANGE_REVISION_RECORD_FAMILIES_V1
                .iter()
                .any(|family| family.name == record.record_family)
            {
                return capability_error("bulk-adoption manifest names an unknown record family");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JournalRecordSignatureV1 {
    alg: String,
    sig_version: u32,
    sig: EventSignatureBytes,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoreCapabilityActivationV1 {
    schema: String,
    version: u32,
    activation_id: String,
    nonce: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    predecessor_activation_id: Option<String>,
    cohort_manifest_hash: String,
    minimum_reader_profile: String,
    required_capabilities: Vec<String>,
    bulk_adoption_manifest_hash: String,
    bulk_adoption_manifest: BulkAdoptionManifestV1,
    writer: Writer,
    occurred_at: String,
    signer: SignerId,
    signature: JournalRecordSignatureV1,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivationIdentityView<'a> {
    schema: &'static str,
    version: u32,
    nonce: &'a str,
    predecessor_activation_id: Option<&'a str>,
    cohort_manifest_hash: &'a str,
    minimum_reader_profile: &'a str,
    required_capabilities: &'a [String],
    bulk_adoption_manifest_hash: &'a str,
    actor_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivationSigningView<'a> {
    schema: &'static str,
    version: u32,
    activation_id: &'a str,
    logical_key: String,
    cohort_manifest_hash: &'a str,
    minimum_reader_profile: &'a str,
    required_capabilities: &'a [String],
    bulk_adoption_manifest_hash: &'a str,
    actor_id: &'a str,
    signer: &'a SignerId,
    occurred_at: &'a str,
}

impl StoreCapabilityActivationV1 {
    fn logical_key(&self) -> String {
        self.predecessor_activation_id.as_ref().map_or_else(
            || ROOT_ACTIVATION_LOGICAL_KEY_V1.to_owned(),
            |predecessor| {
                format!("store_capability_activation:review_change_revision_v1:after:{predecessor}")
            },
        )
    }

    fn identity_view(&self) -> ActivationIdentityView<'_> {
        ActivationIdentityView {
            schema: "pointbreak.store-capability-activation-identity",
            version: 1,
            nonce: &self.nonce,
            predecessor_activation_id: self.predecessor_activation_id.as_deref(),
            cohort_manifest_hash: &self.cohort_manifest_hash,
            minimum_reader_profile: &self.minimum_reader_profile,
            required_capabilities: &self.required_capabilities,
            bulk_adoption_manifest_hash: &self.bulk_adoption_manifest_hash,
            actor_id: self.writer.actor_id.as_str(),
        }
    }

    fn signing_view(&self) -> ActivationSigningView<'_> {
        ActivationSigningView {
            schema: ACTIVATION_TBS_SCHEMA_V1,
            version: 1,
            activation_id: &self.activation_id,
            logical_key: self.logical_key(),
            cohort_manifest_hash: &self.cohort_manifest_hash,
            minimum_reader_profile: &self.minimum_reader_profile,
            required_capabilities: &self.required_capabilities,
            bulk_adoption_manifest_hash: &self.bulk_adoption_manifest_hash,
            actor_id: self.writer.actor_id.as_str(),
            signer: &self.signer,
            occurred_at: &self.occurred_at,
        }
    }

    fn validate(&self) -> Result<()> {
        if self.schema != ACTIVATION_SCHEMA_V1 || self.version != 1 {
            return capability_error("unsupported capability activation schema/version");
        }
        ensure_sorted_unique(&self.required_capabilities, "required capabilities")?;
        if self.cohort_manifest_hash != REVIEW_CHANGE_REVISION_MANIFEST_HASH_V1 {
            return capability_error("activation names an unsupported capability manifest");
        }
        if self.minimum_reader_profile != REVIEW_CHANGE_REVISION_COHORT_V1 {
            return capability_error("activation names an unsupported minimum reader profile");
        }
        if self.required_capabilities != supported_capabilities() {
            return capability_error("activation requires an unsupported capability set");
        }
        self.bulk_adoption_manifest.validate()?;
        if self.bulk_adoption_manifest.canonical_hash()? != self.bulk_adoption_manifest_hash {
            return capability_error("activation bulk-adoption manifest hash mismatch");
        }
        let expected_id = format!(
            "capability-activation:sha256:{}",
            sha256_bytes_hex(&canonical_json_bytes(&serde_json::to_value(
                self.identity_view()
            )?)?)
        );
        if self.activation_id != expected_id {
            return capability_error("capability activation identity mismatch");
        }
        validate_record_signature(
            &self.signature,
            &self.signer,
            ACTIVATION_TBS_PAYLOAD_TYPE_V1,
            &self.signing_view(),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BulkAdoptionCompletionV1 {
    schema: String,
    version: u32,
    completion_id: String,
    activation_id: String,
    bulk_adoption_manifest_hash: String,
    covered_record_set_hash: String,
    writer: Writer,
    occurred_at: String,
    signer: SignerId,
    signature: JournalRecordSignatureV1,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompletionIdentityView<'a> {
    schema: &'static str,
    version: u32,
    activation_id: &'a str,
    bulk_adoption_manifest_hash: &'a str,
    covered_record_set_hash: &'a str,
    actor_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompletionSigningView<'a> {
    schema: &'static str,
    version: u32,
    completion_id: &'a str,
    logical_key: String,
    activation_id: &'a str,
    bulk_adoption_manifest_hash: &'a str,
    covered_record_set_hash: &'a str,
    actor_id: &'a str,
    signer: &'a SignerId,
    occurred_at: &'a str,
}

impl BulkAdoptionCompletionV1 {
    fn logical_key(&self) -> String {
        format!("bulk_adoption_completion:{}", self.completion_id)
    }

    fn identity_view(&self) -> CompletionIdentityView<'_> {
        CompletionIdentityView {
            schema: "pointbreak.bulk-adoption-completion-identity",
            version: 1,
            activation_id: &self.activation_id,
            bulk_adoption_manifest_hash: &self.bulk_adoption_manifest_hash,
            covered_record_set_hash: &self.covered_record_set_hash,
            actor_id: self.writer.actor_id.as_str(),
        }
    }

    fn signing_view(&self) -> CompletionSigningView<'_> {
        CompletionSigningView {
            schema: COMPLETION_TBS_SCHEMA_V1,
            version: 1,
            completion_id: &self.completion_id,
            logical_key: self.logical_key(),
            activation_id: &self.activation_id,
            bulk_adoption_manifest_hash: &self.bulk_adoption_manifest_hash,
            covered_record_set_hash: &self.covered_record_set_hash,
            actor_id: self.writer.actor_id.as_str(),
            signer: &self.signer,
            occurred_at: &self.occurred_at,
        }
    }

    fn validate(&self) -> Result<()> {
        if self.schema != COMPLETION_SCHEMA_V1 || self.version != 1 {
            return capability_error("unsupported bulk-adoption completion schema/version");
        }
        let expected_id = format!(
            "bulk-adoption-completion:sha256:{}",
            sha256_bytes_hex(&canonical_json_bytes(&serde_json::to_value(
                self.identity_view()
            )?)?)
        );
        if self.completion_id != expected_id {
            return capability_error("bulk-adoption completion identity mismatch");
        }
        validate_record_signature(
            &self.signature,
            &self.signer,
            COMPLETION_TBS_PAYLOAD_TYPE_V1,
            &self.signing_view(),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StoreCapabilityStatus {
    MigrationRequired,
    MigrationInProgress {
        activation_id: String,
        manifest_hash: String,
    },
    Ready {
        activation_id: String,
        manifest_hash: String,
        completion_id: String,
    },
}

#[derive(Debug)]
pub(crate) struct JournalInspection {
    pub(crate) status: StoreCapabilityStatus,
    pub(crate) cursor: AuthorityCursorV2,
    pub(crate) minimum_reader_profile: Option<String>,
    #[cfg(any(test, feature = "bench"))]
    pub(crate) record_entries: Vec<JournalEntry>,
    pub(crate) event_entries: Vec<JournalEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransferControlRecordKind {
    CapabilityActivation,
    BulkAdoptionCompletion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TransferControlRecord {
    pub(crate) kind: TransferControlRecordKind,
    pub(crate) logical_key: String,
    pub(crate) minimum_reader_profile: Option<String>,
    pub(crate) required_capabilities: Vec<String>,
}

/// Validate a transfer-carried Journal control record through the same typed
/// signature and manifest checks used by the authoritative router.
pub(crate) fn transfer_control_record(bytes: &[u8]) -> Result<TransferControlRecord> {
    let probe: SchemaProbe = serde_json::from_slice(bytes)?;
    match probe.schema.as_str() {
        ACTIVATION_SCHEMA_V1 => {
            let activation: StoreCapabilityActivationV1 = serde_json::from_slice(bytes)?;
            activation.validate()?;
            Ok(TransferControlRecord {
                kind: TransferControlRecordKind::CapabilityActivation,
                logical_key: activation.logical_key(),
                minimum_reader_profile: Some(activation.minimum_reader_profile),
                required_capabilities: activation.required_capabilities,
            })
        }
        COMPLETION_SCHEMA_V1 => {
            let completion: BulkAdoptionCompletionV1 = serde_json::from_slice(bytes)?;
            completion.validate()?;
            Ok(TransferControlRecord {
                kind: TransferControlRecordKind::BulkAdoptionCompletion,
                logical_key: completion.logical_key(),
                minimum_reader_profile: None,
                required_capabilities: Vec::new(),
            })
        }
        other => capability_error(format!(
            "Journal record schema {other:?} is not an exact-transfer control record"
        )),
    }
}

/// The capability authority exposed at the store/domain boundary.
///
/// This view deliberately omits raw Journal entries and every construction
/// primitive. Callers can negotiate the store state and bind caches or
/// administrative plans to one exact authority without gaining a way to
/// activate a store.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoreCapabilityInspection {
    pub status: StoreCapabilityStatus,
    pub cursor: AuthorityCursorV2,
    /// Durable capability profile required to interpret this root.
    ///
    /// `None` identifies an untouched L0 root. Once activation enters M1 this
    /// is `review_change_revision_v1`. Product version strings intentionally
    /// do not participate in store authority.
    pub minimum_reader_profile: Option<String>,
}

#[derive(Deserialize)]
struct SchemaProbe {
    schema: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventProbe {
    schema: String,
    version: u32,
    event_id: String,
    event_type: String,
    idempotency_key: String,
    payload_hash: String,
    payload: serde_json::Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EventSetMaterial<'a> {
    schema: &'static str,
    events: Vec<EventSetEntry<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EventSetEntry<'a> {
    event_id: &'a str,
    payload_hash: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JournalRecordSetMaterial {
    schema: &'static str,
    records: Vec<JournalRecordSetEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JournalRecordSetEntry {
    key_digest: String,
    record_hash: String,
}

pub(crate) fn inspect_journal_records(journal: &dyn Journal) -> Result<JournalInspection> {
    route_journal_entries(journal.list_record_entries()?)
}

/// Inspect one Journal for the Change-capable reader.
///
/// Activated M1/L2 roots always take the strict complete router. An untouched
/// L0 root uses the legacy reader's existing, narrow schema-break allowance:
/// recognized retired event encodings contribute to the raw Journal-record
/// cursor but are excluded from the supported event-set cursor. Genuine
/// corruption, unknown control records, and Change-cohort records without an
/// activation still fail closed.
pub(crate) fn inspect_change_reader_journal_records(
    journal: &dyn Journal,
) -> Result<JournalInspection> {
    if journal.record_exists(ROOT_ACTIVATION_LOGICAL_KEY_V1)? {
        return inspect_journal_records(journal);
    }
    route_unactivated_journal_entries(journal.list_record_entries()?)
}

fn route_unactivated_journal_entries(entries: Vec<JournalEntry>) -> Result<JournalInspection> {
    #[cfg(any(test, feature = "bench"))]
    let record_entries = entries.clone();
    let mut event_entries = Vec::new();
    let mut event_probes = Vec::new();
    let mut record_set = Vec::with_capacity(entries.len());

    for entry in entries {
        record_set.push(JournalRecordSetEntry {
            key_digest: entry.key_digest.clone(),
            record_hash: format!("sha256:{}", sha256_bytes_hex(&entry.bytes)),
        });

        if let Some(code) = serde_json::from_slice::<serde_json::Value>(&entry.bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("eventType")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            && code.starts_with("t:")
            && event_type_from_code(&code).is_none()
        {
            if is_reserved_event_code(&code) {
                return capability_error(
                    "Change-cohort records exist without a capability activation",
                );
            }
            return capability_error(format!("unknown Journal event type code {code:?}"));
        }

        match EventStore::decode_qualification_entry(entry.key_digest.clone(), entry.bytes.clone())
        {
            Ok(_) => {
                let probe: EventProbe = serde_json::from_slice(&entry.bytes)?;
                validate_event_probe(&probe, &entry.key_digest)?;
                event_probes.push(probe);
                event_entries.push(entry);
            }
            Err(ShoreError::UnsupportedEventType(_) | ShoreError::UnsupportedEventEnvelope(_)) => {}
            Err(error) => return Err(error),
        }
    }

    record_set.sort_by(|left, right| left.key_digest.cmp(&right.key_digest));
    event_probes.sort_by(|left, right| {
        (left.event_id.as_str(), left.payload_hash.as_str())
            .cmp(&(right.event_id.as_str(), right.payload_hash.as_str()))
    });
    let journal_record_count = record_set.len();
    let journal_record_set_hash =
        sha256_json_prefixed(&serde_json::to_value(JournalRecordSetMaterial {
            schema: JOURNAL_RECORD_SET_SCHEMA_V1,
            records: record_set,
        })?)?;
    let cursor = authority_cursor(
        journal_record_count,
        event_entries.len(),
        journal_record_set_hash,
        event_set_hash(&event_probes)?,
        &[],
    )?;
    Ok(JournalInspection {
        status: StoreCapabilityStatus::MigrationRequired,
        cursor,
        minimum_reader_profile: None,
        #[cfg(any(test, feature = "bench"))]
        record_entries,
        event_entries,
    })
}

/// Inspect capability authority only when the fixed activation record exists.
///
/// This is the compatibility choke point for legacy semantic readers. An
/// untouched L0 journal may contain event encodings that the old lenient
/// display path can diagnose and skip, so probing every record before that
/// display path would accidentally make the compatibility check stricter than
/// the reader it protects. Once the activation key exists, the complete router
/// remains mandatory and validates every record before returning authority.
pub(crate) fn inspect_activated_journal_records(
    journal: &dyn Journal,
) -> Result<Option<JournalInspection>> {
    if !journal.record_exists(ROOT_ACTIVATION_LOGICAL_KEY_V1)? {
        return Ok(None);
    }
    inspect_journal_records(journal).map(Some)
}

/// The temporary writer-dark gate for the already-shipped event-only product.
/// A fixed root key makes the normal L0 check O(1); once it exists, the complete
/// router validates the activated store and then refuses the old semantic path.
pub(crate) fn preflight_event_only_product(journal: &dyn Journal) -> Result<()> {
    let Some(inspection) = inspect_activated_journal_records(journal)? else {
        return Ok(());
    };
    capability_error(match inspection.status {
        StoreCapabilityStatus::MigrationRequired => {
            "activation root disappeared during capability preflight".to_owned()
        }
        StoreCapabilityStatus::MigrationInProgress { .. } => {
            "migration_in_progress; this event-only product route cannot consume partial Change authority"
                .to_owned()
        }
        StoreCapabilityStatus::Ready { .. } => {
            "reader_upgrade_required; this event-only product route cannot consume the activated Change cohort"
                .to_owned()
        }
    })
}

pub(crate) fn event_entries_for_event_only_product(
    journal: &dyn Journal,
) -> Result<Vec<JournalEntry>> {
    // Preserve the exact v0.9 event decoder and its typed error surface on L0.
    // The fixed activation key is the compatibility boundary: only an activated
    // root enters the new router before any event decode.
    if !journal.record_exists(ROOT_ACTIVATION_LOGICAL_KEY_V1)? {
        return journal.list_event_entries();
    }
    let inspection = inspect_journal_records(journal)?;
    match inspection.status {
        StoreCapabilityStatus::MigrationRequired => Ok(inspection.event_entries),
        StoreCapabilityStatus::MigrationInProgress { .. } => capability_error(
            "migration_in_progress; refusing partial semantic output from an activated store",
        ),
        StoreCapabilityStatus::Ready { .. } => capability_error(
            "reader_upgrade_required; refusing legacy semantic output from an activated store",
        ),
    }
}

pub(crate) fn route_journal_entries(entries: Vec<JournalEntry>) -> Result<JournalInspection> {
    #[cfg(any(test, feature = "bench"))]
    let record_entries = entries.clone();
    let mut event_entries = Vec::new();
    let mut event_probes = Vec::new();
    let mut activations = Vec::new();
    let mut completions = Vec::new();
    let mut record_set = Vec::new();

    for entry in entries {
        let probe: SchemaProbe = serde_json::from_slice(&entry.bytes).map_err(|error| {
            ShoreError::Message(format!("Journal record has no valid typed schema: {error}"))
        })?;
        record_set.push(JournalRecordSetEntry {
            key_digest: entry.key_digest.clone(),
            record_hash: format!("sha256:{}", sha256_bytes_hex(&entry.bytes)),
        });
        match probe.schema.as_str() {
            "shore.event" => {
                let event: EventProbe = serde_json::from_slice(&entry.bytes)?;
                validate_event_probe(&event, &entry.key_digest)?;
                event_probes.push(event);
                event_entries.push(entry);
            }
            ACTIVATION_SCHEMA_V1 => {
                let activation: StoreCapabilityActivationV1 = serde_json::from_slice(&entry.bytes)?;
                activation.validate()?;
                validate_logical_key_digest(&activation.logical_key(), &entry.key_digest)?;
                activations.push(activation);
            }
            COMPLETION_SCHEMA_V1 => {
                let completion: BulkAdoptionCompletionV1 = serde_json::from_slice(&entry.bytes)?;
                completion.validate()?;
                validate_logical_key_digest(&completion.logical_key(), &entry.key_digest)?;
                completions.push(completion);
            }
            other => return capability_error(format!("unknown Journal record schema {other:?}")),
        }
    }

    record_set.sort_by(|left, right| left.key_digest.cmp(&right.key_digest));
    event_probes.sort_by(|left, right| {
        (left.event_id.as_str(), left.payload_hash.as_str())
            .cmp(&(right.event_id.as_str(), right.payload_hash.as_str()))
    });
    let event_set_hash = event_set_hash(&event_probes)?;
    let journal_record_set_hash =
        sha256_json_prefixed(&serde_json::to_value(JournalRecordSetMaterial {
            schema: JOURNAL_RECORD_SET_SCHEMA_V1,
            records: record_set,
        })?)?;

    if activations.is_empty() {
        if !completions.is_empty()
            || event_probes
                .iter()
                .any(|event| is_reserved_event_code(&event.event_type))
        {
            return capability_error("Change-cohort records exist without a capability activation");
        }
        let cursor = authority_cursor(
            event_entries.len(),
            event_entries.len(),
            journal_record_set_hash,
            event_set_hash,
            &[],
        )?;
        return Ok(JournalInspection {
            status: StoreCapabilityStatus::MigrationRequired,
            cursor,
            minimum_reader_profile: None,
            #[cfg(any(test, feature = "bench"))]
            record_entries,
            event_entries,
        });
    }

    let activation_count = activations.len();
    let active = validate_activation_chain(&activations)?;
    if completions.len() > 1 {
        return capability_error("multiple bulk-adoption completion records are unsupported");
    }
    let status = if let Some(completion) = completions.first() {
        validate_completion(completion, active, &event_entries)?;
        StoreCapabilityStatus::Ready {
            activation_id: active.activation_id.clone(),
            manifest_hash: active.bulk_adoption_manifest_hash.clone(),
            completion_id: completion.completion_id.clone(),
        }
    } else {
        validate_migration_prefix(active, &event_entries)?;
        StoreCapabilityStatus::MigrationInProgress {
            activation_id: active.activation_id.clone(),
            manifest_hash: active.bulk_adoption_manifest_hash.clone(),
        }
    };
    let cursor = authority_cursor(
        event_entries.len() + activation_count + completions.len(),
        event_entries.len(),
        journal_record_set_hash,
        event_set_hash,
        &active.required_capabilities,
    )?;
    Ok(JournalInspection {
        status,
        cursor,
        minimum_reader_profile: Some(active.minimum_reader_profile.clone()),
        #[cfg(any(test, feature = "bench"))]
        record_entries,
        event_entries,
    })
}

pub(crate) fn validate_monotonic_successor(previous: &[String], next: &[String]) -> Result<()> {
    ensure_sorted_unique(previous, "previous required capabilities")?;
    ensure_sorted_unique(next, "successor required capabilities")?;
    let previous = previous.iter().collect::<BTreeSet<_>>();
    let next = next.iter().collect::<BTreeSet<_>>();
    if previous == next || !previous.is_subset(&next) {
        return capability_error("capability successor must be a strict monotonic superset");
    }
    Ok(())
}

fn validate_activation_chain(
    activations: &[StoreCapabilityActivationV1],
) -> Result<&StoreCapabilityActivationV1> {
    // This reader knows one exact capability set, so v1 admits only its root
    // activation. The separately frozen strict-superset rule is for a future
    // reader whose supported set actually expands; pretending to walk successors
    // here would be misleading because `StoreCapabilityActivationV1::validate`
    // correctly rejects every capability this reader does not understand.
    let [root] = activations else {
        return capability_error(
            "the review_change_revision_v1 reader requires exactly one root activation",
        );
    };
    if root.predecessor_activation_id.is_some() {
        return capability_error(
            "the review_change_revision_v1 activation must not name a predecessor",
        );
    }
    Ok(root)
}

fn validate_migration_prefix(
    active: &StoreCapabilityActivationV1,
    entries: &[JournalEntry],
) -> Result<()> {
    let reservations = active
        .bulk_adoption_manifest
        .reserved_records
        .iter()
        .map(|record| (record.logical_key.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    for entry in entries {
        let event: EventProbe = serde_json::from_slice(&entry.bytes)?;
        if is_reserved_event_code(&event.event_type) {
            let reservation = reservations
                .get(event.idempotency_key.as_str())
                .ok_or_else(|| {
                    ShoreError::Message(
                        "migration prefix contains an unreserved Change-cohort record".to_owned(),
                    )
                })?;
            validate_reserved_bytes(reservation, &entry.bytes)?;
        }
    }
    Ok(())
}

fn validate_completion(
    completion: &BulkAdoptionCompletionV1,
    active: &StoreCapabilityActivationV1,
    entries: &[JournalEntry],
) -> Result<()> {
    if completion.activation_id != active.activation_id
        || completion.bulk_adoption_manifest_hash != active.bulk_adoption_manifest_hash
    {
        return capability_error("bulk-adoption completion does not bind the active manifest");
    }
    let by_digest = entries
        .iter()
        .map(|entry| (entry.key_digest.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    for reservation in &active.bulk_adoption_manifest.reserved_records {
        let digest = sha256_bytes_hex(reservation.logical_key.as_bytes());
        let entry = by_digest.get(digest.as_str()).ok_or_else(|| {
            ShoreError::Message(format!(
                "bulk-adoption completion is missing reserved record {}",
                reservation.logical_key
            ))
        })?;
        validate_reserved_bytes(reservation, &entry.bytes)?;
    }
    let expected = reserved_record_set_hash(&active.bulk_adoption_manifest.reserved_records)?;
    if completion.covered_record_set_hash != expected {
        return capability_error("bulk-adoption completion coverage hash mismatch");
    }
    Ok(())
}

fn validate_reserved_bytes(reservation: &ReservedCohortRecordV1, bytes: &[u8]) -> Result<()> {
    let actual = format!("sha256:{}", sha256_bytes_hex(bytes));
    if actual != reservation.record_hash {
        return capability_error(format!(
            "reserved record {} has divergent bytes",
            reservation.logical_key
        ));
    }
    Ok(())
}

fn validate_event_probe(event: &EventProbe, key_digest: &str) -> Result<()> {
    if event.schema != "shore.event" || event.version != 1 {
        return capability_error("unsupported event schema/version in Journal router");
    }
    let expected_digest = sha256_bytes_hex(event.idempotency_key.as_bytes());
    if key_digest != expected_digest {
        return capability_error("Journal event logical key does not match its carrier address");
    }
    let expected_event_id = format!("evt:sha256:{expected_digest}");
    if event.event_id != expected_event_id {
        return capability_error("Journal event ID does not match its logical key");
    }
    if sha256_json_prefixed(&event.payload)? != event.payload_hash {
        return capability_error("Journal event payload hash mismatch");
    }
    if !is_known_or_reserved_event_code(&event.event_type) {
        return capability_error(format!(
            "unknown Journal event type code {:?}",
            event.event_type
        ));
    }
    if let Some(family) = REVIEW_CHANGE_REVISION_RECORD_FAMILIES_V1
        .iter()
        .find(|family| family.event_type_code == event.event_type)
    {
        let schema = event
            .payload
            .get("schema")
            .and_then(serde_json::Value::as_str);
        let version = event
            .payload
            .get("version")
            .and_then(serde_json::Value::as_u64);
        if schema != Some(family.payload_schema)
            || version != Some(u64::from(family.payload_version))
        {
            return capability_error(
                "reserved Change-cohort payload does not match its frozen schema/version",
            );
        }
    }
    Ok(())
}

fn is_known_or_reserved_event_code(code: &str) -> bool {
    event_type_from_code(code).is_some() || is_reserved_event_code(code)
}

fn is_reserved_event_code(code: &str) -> bool {
    REVIEW_CHANGE_REVISION_RECORD_FAMILIES_V1
        .iter()
        .any(|family| family.event_type_code == code)
}

fn authority_cursor(
    record_count: usize,
    event_count: usize,
    journal_record_set_hash: String,
    event_set_hash: String,
    capabilities: &[String],
) -> Result<AuthorityCursorV2> {
    Ok(AuthorityCursorV2 {
        schema: AUTHORITY_CURSOR_SCHEMA_V2.to_owned(),
        journal_record_count: u64::try_from(record_count).unwrap_or(u64::MAX),
        event_count: u64::try_from(event_count).unwrap_or(u64::MAX),
        journal_record_set_hash,
        event_set_hash,
        capability_set_hash: capability_set_hash(capabilities)?,
    })
}

fn event_set_hash(events: &[EventProbe]) -> Result<String> {
    let material = EventSetMaterial {
        schema: EVENT_SET_SCHEMA_V1,
        events: events
            .iter()
            .map(|event| EventSetEntry {
                event_id: &event.event_id,
                payload_hash: &event.payload_hash,
            })
            .collect(),
    };
    sha256_json_prefixed(&serde_json::to_value(material)?)
}

fn capability_set_hash(capabilities: &[String]) -> Result<String> {
    sha256_json_prefixed(&serde_json::json!({
        "schema": CAPABILITY_SET_SCHEMA_V1,
        "capabilities": capabilities,
    }))
}

fn reserved_record_set_hash(records: &[ReservedCohortRecordV1]) -> Result<String> {
    sha256_json_prefixed(&serde_json::json!({
        "schema": "pointbreak.bulk-adoption-record-set.v1",
        "records": records,
    }))
}

fn validate_cursor_shape(cursor: &AuthorityCursorV2) -> Result<()> {
    if cursor.schema != AUTHORITY_CURSOR_SCHEMA_V2
        || !is_prefixed_sha256(&cursor.journal_record_set_hash)
        || !is_prefixed_sha256(&cursor.event_set_hash)
        || !is_prefixed_sha256(&cursor.capability_set_hash)
    {
        return capability_error("bulk-adoption manifest has an invalid source authority cursor");
    }
    if cursor.event_count > cursor.journal_record_count {
        return capability_error("authority cursor event count exceeds Journal record count");
    }
    Ok(())
}

fn validate_record_signature<T: Serialize>(
    signature: &JournalRecordSignatureV1,
    signer: &SignerId,
    payload_type: &str,
    signing_view: &T,
) -> Result<()> {
    // This proves record-local key possession and byte integrity. It does not
    // authorize activation: the future production writer must apply operator
    // policy before publication. The writer-dark cohort exposes no such writer.
    if signature.alg != "ed25519" || signature.sig_version != 1 || !signature.sig.is_base64() {
        return capability_error("Journal control record has an unsupported signature");
    }
    let body = canonical_json_bytes(&serde_json::to_value(signing_view)?)?;
    let pae = pre_authentication_encoding(payload_type, &body);
    if verify_ed25519_strict(signer, &pae, signature.sig.as_str())?
        != EventVerificationStatus::Valid
    {
        return capability_error("Journal control record signature is invalid");
    }
    Ok(())
}

fn validate_logical_key_digest(logical_key: &str, digest: &str) -> Result<()> {
    if sha256_bytes_hex(logical_key.as_bytes()) != digest {
        return capability_error(
            "Journal control record logical key does not match its carrier address",
        );
    }
    Ok(())
}

fn ensure_sorted_unique(values: &[String], label: &str) -> Result<()> {
    ensure_sorted_unique_by(values, String::as_str, label)
}

fn ensure_sorted_unique_by<T, F>(values: &[T], key: F, label: &str) -> Result<()>
where
    F: Fn(&T) -> &str,
{
    for pair in values.windows(2) {
        if key(&pair[0]) >= key(&pair[1]) {
            return capability_error(format!("{label} must be sorted and unique"));
        }
    }
    Ok(())
}

fn supported_capabilities() -> Vec<String> {
    REVIEW_CHANGE_REVISION_REQUIRED_CAPABILITIES_V1
        .iter()
        .map(|value| (*value).to_owned())
        .collect()
}

fn is_prefixed_sha256(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn capability_error<T>(message: impl Into<String>) -> Result<T> {
    Err(ShoreError::Message(format!(
        "unsupported Pointbreak store capability state: {}",
        message.into()
    )))
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(in crate::session) enum CapabilityFixtureState {
    M1,
    L2,
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum CapabilityFixtureCorruption {
    Malformed,
    UnknownSchema,
    UnsupportedMinimumReaderProfile,
    DivergentLogicalKey,
    CompletionWithoutCoverage,
}

#[cfg(test)]
pub(in crate::session) fn write_capability_fixture_for_test(
    journal: &dyn Journal,
    state: CapabilityFixtureState,
) -> Result<()> {
    use crate::crypto::TestEd25519Signer;

    let signer = TestEd25519Signer::from_seed([29; 32]);
    let source_inspection = inspect_journal_records(journal)?;
    if !matches!(
        source_inspection.status,
        StoreCapabilityStatus::MigrationRequired
    ) {
        return capability_error("qualification activation requires an L0 source root");
    }
    let source = source_inspection.cursor;
    let events = qualification_events();
    let mut reservations = events
        .iter()
        .map(|(logical_key, _, bytes)| ReservedCohortRecordV1 {
            logical_key: logical_key.clone(),
            record_family: REVIEW_CHANGE_REVISION_RECORD_FAMILIES_V1
                .iter()
                .find(|family| {
                    family.event_type_code
                        == serde_json::from_slice::<EventProbe>(bytes)
                            .expect("fixture event probe")
                            .event_type
                })
                .expect("reserved fixture family")
                .name
                .to_owned(),
            record_hash: format!("sha256:{}", sha256_bytes_hex(bytes)),
        })
        .collect::<Vec<_>>();
    reservations.sort_by(|left, right| left.logical_key.cmp(&right.logical_key));
    let manifest = BulkAdoptionManifestV1 {
        schema: BULK_MANIFEST_SCHEMA_V1.to_owned(),
        version: 1,
        cohort_manifest_hash: REVIEW_CHANGE_REVISION_MANIFEST_HASH_V1.to_owned(),
        source_authority_cursor: source,
        reserved_records: reservations,
    };
    let activation = signed_activation(&signer, manifest)?;
    publish_test_record(journal, &activation.logical_key(), &activation)?;
    if matches!(state, CapabilityFixtureState::L2) {
        for (logical_key, _, bytes) in &events {
            match journal.create_record_once(logical_key, bytes)? {
                crate::storage::CreateOutcome::Created
                | crate::storage::CreateOutcome::AlreadyExists => {}
            }
        }
        let completion = signed_completion(&signer, &activation)?;
        publish_test_record(journal, &completion.logical_key(), &completion)?;
    }
    Ok(())
}

#[cfg(test)]
fn write_corrupt_capability_fixture_for_test(
    journal: &dyn Journal,
    corruption: CapabilityFixtureCorruption,
) -> Result<()> {
    match corruption {
        CapabilityFixtureCorruption::Malformed => {
            journal.create_record_once("malformed", b"not-json")?;
        }
        CapabilityFixtureCorruption::UnknownSchema => {
            journal
                .create_record_once("unknown", br#"{"schema":"pointbreak.unknown","version":1}"#)?;
        }
        CapabilityFixtureCorruption::UnsupportedMinimumReaderProfile => {
            use crate::crypto::TestEd25519Signer;
            let signer = TestEd25519Signer::from_seed([29; 32]);
            let manifest = BulkAdoptionManifestV1 {
                schema: BULK_MANIFEST_SCHEMA_V1.to_owned(),
                version: 1,
                cohort_manifest_hash: REVIEW_CHANGE_REVISION_MANIFEST_HASH_V1.to_owned(),
                source_authority_cursor: route_journal_entries(Vec::new())?.cursor,
                reserved_records: Vec::new(),
            };
            let mut activation = signed_activation(&signer, manifest)?;
            activation.minimum_reader_profile = "pointbreak.product.v0.10.0".to_owned();
            publish_test_record(journal, &activation.logical_key(), &activation)?;
        }
        CapabilityFixtureCorruption::DivergentLogicalKey => {
            use crate::crypto::TestEd25519Signer;
            let signer = TestEd25519Signer::from_seed([29; 32]);
            let manifest = BulkAdoptionManifestV1 {
                schema: BULK_MANIFEST_SCHEMA_V1.to_owned(),
                version: 1,
                cohort_manifest_hash: REVIEW_CHANGE_REVISION_MANIFEST_HASH_V1.to_owned(),
                source_authority_cursor: route_journal_entries(Vec::new())?.cursor,
                reserved_records: Vec::new(),
            };
            let activation = signed_activation(&signer, manifest)?;
            publish_test_record(journal, "wrong-logical-key", &activation)?;
        }
        CapabilityFixtureCorruption::CompletionWithoutCoverage => {
            use crate::crypto::TestEd25519Signer;
            let signer = TestEd25519Signer::from_seed([29; 32]);
            let events = qualification_events();
            let mut reservations = events
                .iter()
                .map(|(logical_key, family, bytes)| ReservedCohortRecordV1 {
                    logical_key: logical_key.clone(),
                    record_family: (*family).to_owned(),
                    record_hash: format!("sha256:{}", sha256_bytes_hex(bytes)),
                })
                .collect::<Vec<_>>();
            reservations.sort_by(|left, right| left.logical_key.cmp(&right.logical_key));
            let manifest = BulkAdoptionManifestV1 {
                schema: BULK_MANIFEST_SCHEMA_V1.to_owned(),
                version: 1,
                cohort_manifest_hash: REVIEW_CHANGE_REVISION_MANIFEST_HASH_V1.to_owned(),
                source_authority_cursor: route_journal_entries(Vec::new())?.cursor,
                reserved_records: reservations,
            };
            let activation = signed_activation(&signer, manifest)?;
            publish_test_record(journal, &activation.logical_key(), &activation)?;
            let completion = signed_completion(&signer, &activation)?;
            publish_test_record(journal, &completion.logical_key(), &completion)?;
        }
    }
    Ok(())
}

#[cfg(test)]
fn signed_activation(
    signer: &impl EventSigner,
    manifest: BulkAdoptionManifestV1,
) -> Result<StoreCapabilityActivationV1> {
    let manifest_hash = manifest.canonical_hash()?;
    let writer = Writer {
        actor_id: ActorId::new("actor:qualification"),
        producer: WriterProducer {
            name: "pointbreak-qualification".to_owned(),
            version: "1".to_owned(),
        },
    };
    let mut activation = StoreCapabilityActivationV1 {
        schema: ACTIVATION_SCHEMA_V1.to_owned(),
        version: 1,
        activation_id: String::new(),
        nonce: "qualification-activation-0001".to_owned(),
        predecessor_activation_id: None,
        cohort_manifest_hash: REVIEW_CHANGE_REVISION_MANIFEST_HASH_V1.to_owned(),
        minimum_reader_profile: REVIEW_CHANGE_REVISION_COHORT_V1.to_owned(),
        required_capabilities: supported_capabilities(),
        bulk_adoption_manifest_hash: manifest_hash,
        bulk_adoption_manifest: manifest,
        writer,
        occurred_at: "2026-08-04T00:00:00Z".to_owned(),
        signer: signer.signer_id().clone(),
        signature: JournalRecordSignatureV1 {
            alg: "ed25519".to_owned(),
            sig_version: 1,
            sig: EventSignatureBytes::parse("")?,
        },
    };
    activation.activation_id = format!(
        "capability-activation:sha256:{}",
        sha256_bytes_hex(&canonical_json_bytes(&serde_json::to_value(
            activation.identity_view()
        )?)?)
    );
    let body = canonical_json_bytes(&serde_json::to_value(activation.signing_view())?)?;
    activation.signature.sig = signer.sign_event_message(&pre_authentication_encoding(
        ACTIVATION_TBS_PAYLOAD_TYPE_V1,
        &body,
    ))?;
    Ok(activation)
}

#[cfg(test)]
fn signed_completion(
    signer: &impl EventSigner,
    activation: &StoreCapabilityActivationV1,
) -> Result<BulkAdoptionCompletionV1> {
    let writer = Writer {
        actor_id: ActorId::new("actor:qualification"),
        producer: WriterProducer {
            name: "pointbreak-qualification".to_owned(),
            version: "1".to_owned(),
        },
    };
    let mut completion = BulkAdoptionCompletionV1 {
        schema: COMPLETION_SCHEMA_V1.to_owned(),
        version: 1,
        completion_id: String::new(),
        activation_id: activation.activation_id.clone(),
        bulk_adoption_manifest_hash: activation.bulk_adoption_manifest_hash.clone(),
        covered_record_set_hash: reserved_record_set_hash(
            &activation.bulk_adoption_manifest.reserved_records,
        )?,
        writer,
        occurred_at: "2026-08-04T00:00:01Z".to_owned(),
        signer: signer.signer_id().clone(),
        signature: JournalRecordSignatureV1 {
            alg: "ed25519".to_owned(),
            sig_version: 1,
            sig: EventSignatureBytes::parse("")?,
        },
    };
    completion.completion_id = format!(
        "bulk-adoption-completion:sha256:{}",
        sha256_bytes_hex(&canonical_json_bytes(&serde_json::to_value(
            completion.identity_view()
        )?)?)
    );
    let body = canonical_json_bytes(&serde_json::to_value(completion.signing_view())?)?;
    completion.signature.sig = signer.sign_event_message(&pre_authentication_encoding(
        COMPLETION_TBS_PAYLOAD_TYPE_V1,
        &body,
    ))?;
    Ok(completion)
}

#[cfg(test)]
fn publish_test_record<T: Serialize>(
    journal: &dyn Journal,
    logical_key: &str,
    record: &T,
) -> Result<()> {
    let bytes = canonical_json_bytes(&serde_json::to_value(record)?)?;
    journal.create_record_once(logical_key, &bytes)?;
    Ok(())
}

#[cfg(test)]
fn qualification_events() -> Vec<(String, &'static str, Vec<u8>)> {
    use crate::model::{
        ActorId, ChangeIdentityDescriptorV1, CommitAssociationId, InputRequestId, JournalId,
        ReviewTargetRef, RevisionId, RevisionRefV1, TargetRef, TrackId,
    };
    use crate::session::event::{
        ChangeLinkRelationV1, EventTarget, FactPortRelationV1, FactRefV1, RelationProofStatusV1,
        ReviewFactPortDraftV1, RevisionRelationAttestationDraftV1, SemanticRevisionRelationV1,
        WriterProducer, build_change_declared, build_change_link_asserted,
        build_membership_asserted, build_membership_withdrawn, build_review_fact_ported,
        build_revision_relation_asserted, build_revision_relation_attested,
        build_revision_relation_withdrawn,
    };

    let journal_id = JournalId::new("journal:qualification");
    let track_id = TrackId::new("track:qualification");
    let writer = Writer {
        actor_id: ActorId::new("actor:qualification"),
        producer: WriterProducer {
            name: "pointbreak-qualification".to_owned(),
            version: "1".to_owned(),
        },
    };
    let declared =
        build_change_declared(ChangeIdentityDescriptorV1::opaque_nonce([1; 32]), [2; 32])
            .expect("Change declaration");
    let linked = crate::model::derive_change_id(&ChangeIdentityDescriptorV1::opaque_nonce([3; 32]))
        .expect("linked Change identity");
    let revision_a = RevisionRefV1::new(
        RevisionId::new("rev:sha256:qualification-a"),
        format!("sha256:{}", "a".repeat(64)),
    )
    .expect("Revision A");
    let revision_b = RevisionRefV1::new(
        RevisionId::new("rev:sha256:qualification-b"),
        format!("sha256:{}", "b".repeat(64)),
    )
    .expect("Revision B");
    let attestation_revision_id = revision_b.revision_id.clone();
    let membership =
        build_membership_asserted(&declared.change_id, &revision_a.revision_id, [4; 32])
            .expect("membership assertion");
    let membership_withdrawal =
        build_membership_withdrawn(&membership.membership_claim_id, [5; 32])
            .expect("membership withdrawal");
    let link = build_change_link_asserted(
        &declared.change_id,
        &linked,
        ChangeLinkRelationV1::RelatedWork,
        [6; 32],
    )
    .expect("Change link");
    let relation = build_revision_relation_asserted(
        &declared.change_id,
        revision_b.clone(),
        revision_a.clone(),
        [7; 32],
    )
    .expect("Revision relation");
    let relation_withdrawal =
        build_revision_relation_withdrawn(&relation.relation_claim_id, [8; 32])
            .expect("Revision relation withdrawal");
    let attestation = build_revision_relation_attested(RevisionRelationAttestationDraftV1 {
        revision: revision_b.clone(),
        commit_association_id: CommitAssociationId::new("assoc-commit:sha256:qualification"),
        semantic_relation: SemanticRevisionRelationV1::LandingProvenance,
        proof_status: RelationProofStatusV1::Asserted,
        proof_method: "qualification".to_owned(),
        proof_algorithm_version: "1".to_owned(),
        capture_scope: Vec::new(),
        comparison_base_or_parent: None,
        endpoint_oids: Vec::new(),
        evidence_content_hash: None,
        result_digest: format!("sha256:{}", "c".repeat(64)),
    })
    .expect("relation attestation");
    let fact_port = build_review_fact_ported(
        ReviewFactPortDraftV1 {
            origin_revision: revision_a.clone(),
            origin_fact: FactRefV1::InputRequest {
                input_request_id: InputRequestId::new("input-request:sha256:qualification"),
            },
            target_revision: revision_b,
            relation: FactPortRelationV1::ContextOnly,
            target_fact: None,
            rationale_content_hash: None,
            context_change_id: Some(declared.change_id.clone()),
        },
        &writer.actor_id,
        &track_id,
    )
    .expect("fact port");

    vec![
        qualification_event(
            0,
            "change_declared_v1",
            declared,
            EventTarget::for_journal(journal_id.clone()),
            &writer,
        ),
        qualification_event(
            1,
            "change_membership_asserted_v1",
            membership,
            EventTarget::for_journal(journal_id.clone()),
            &writer,
        ),
        qualification_event(
            2,
            "change_membership_withdrawn_v1",
            membership_withdrawal,
            EventTarget::for_journal(journal_id.clone()),
            &writer,
        ),
        qualification_event(
            3,
            "change_link_asserted_v1",
            link,
            EventTarget::for_journal(journal_id.clone()),
            &writer,
        ),
        qualification_event(
            4,
            "change_revision_relation_asserted_v1",
            relation,
            EventTarget::for_journal(journal_id.clone()),
            &writer,
        ),
        qualification_event(
            5,
            "change_revision_relation_withdrawn_v1",
            relation_withdrawal,
            EventTarget::for_journal(journal_id.clone()),
            &writer,
        ),
        qualification_event(
            6,
            "revision_relation_attested_v1",
            attestation,
            EventTarget::for_revision(
                journal_id.clone(),
                attestation_revision_id,
                Some(track_id.clone()),
            )
            .expect("attestation target"),
            &writer,
        ),
        qualification_event(
            7,
            "review_fact_ported_v1",
            fact_port,
            EventTarget::for_subject(
                journal_id,
                TargetRef::Review(ReviewTargetRef::InputRequest {
                    revision_id: revision_a.revision_id,
                    input_request_id: InputRequestId::new("input-request:sha256:qualification"),
                }),
                Some(track_id),
            )
            .expect("fact-port target"),
            &writer,
        ),
    ]
}

#[cfg(test)]
fn qualification_event<P: crate::session::event::EventPayload>(
    index: usize,
    family: &'static str,
    payload: P,
    target: crate::session::event::EventTarget,
    writer: &Writer,
) -> (String, &'static str, Vec<u8>) {
    let logical_key = format!("qualification:{family}:{index}");
    let event = crate::session::event::ShoreEvent::new(
        payload.event_type(),
        logical_key.clone(),
        target,
        writer.clone(),
        payload,
        format!("2026-08-04T00:00:{:02}Z", index + 2),
    )
    .expect("qualification event");
    (
        logical_key,
        family,
        serde_json::to_vec(&event).expect("fixture bytes"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::store::backend::StoreBackend;

    #[test]
    fn umbrella_manifest_and_consumer_versions_are_frozen() {
        let manifest = ReviewChangeRevisionManifestV1::canonical();

        assert_eq!(manifest.cohort, "review_change_revision_v1");
        assert_eq!(manifest.minimum_reader_profile, "review_change_revision_v1");
        assert_eq!(
            manifest.required_capabilities,
            REVIEW_CHANGE_REVISION_REQUIRED_CAPABILITIES_V1
        );
        assert_eq!(
            manifest.record_families,
            REVIEW_CHANGE_REVISION_RECORD_FAMILIES_V1
        );
        assert_eq!(
            manifest.canonical_hash().unwrap(),
            REVIEW_CHANGE_REVISION_MANIFEST_HASH_V1
        );
        assert_eq!(
            reader_profile_versions_v1(),
            READER_PROFILE_DOCUMENT_VERSIONS_V1
        );
        assert!(
            REVIEW_CHANGE_REVISION_RECORD_FAMILIES_V1
                .iter()
                .all(|family| event_type_from_code(family.event_type_code).is_some()),
            "the frozen reservations must resolve after the writer-dark semantic core lands"
        );
    }

    #[test]
    fn authority_cursor_distinguishes_journal_records_from_events() {
        let root = tempfile::tempdir().unwrap();
        let backend = StoreBackend::Local(root.path().to_path_buf());

        let l0 = inspect_journal_records(backend.journal().as_ref()).unwrap();
        assert!(matches!(
            l0.status,
            StoreCapabilityStatus::MigrationRequired
        ));
        assert_eq!(l0.cursor.journal_record_count, 0);
        assert_eq!(l0.cursor.event_count, 0);
        assert_eq!(l0.minimum_reader_profile, None);

        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::M1)
            .unwrap();
        let m1 = inspect_journal_records(backend.journal().as_ref()).unwrap();
        assert!(matches!(
            m1.status,
            StoreCapabilityStatus::MigrationInProgress { .. }
        ));
        assert!(m1.cursor.journal_record_count > 0);
        assert_eq!(m1.cursor.event_count, 0);
        assert_eq!(
            m1.minimum_reader_profile.as_deref(),
            Some("review_change_revision_v1")
        );
        assert_ne!(
            l0.cursor.journal_record_set_hash,
            m1.cursor.journal_record_set_hash
        );
        assert_eq!(l0.cursor.event_set_hash, m1.cursor.event_set_hash);
        assert_ne!(l0.cursor.capability_set_hash, m1.cursor.capability_set_hash);
    }

    #[test]
    fn complete_fixture_routes_as_l2_and_exposes_only_semantic_events() {
        let root = tempfile::tempdir().unwrap();
        let backend = StoreBackend::Local(root.path().to_path_buf());
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();

        let inspection = inspect_journal_records(backend.journal().as_ref()).unwrap();
        assert!(matches!(
            inspection.status,
            StoreCapabilityStatus::Ready { .. }
        ));
        assert_eq!(
            inspection.minimum_reader_profile.as_deref(),
            Some("review_change_revision_v1")
        );
        assert!(inspection.cursor.journal_record_count > inspection.cursor.event_count);
        assert!(!inspection.event_entries.is_empty());
    }

    #[test]
    fn minimum_reader_profile_is_capability_authority_not_product_semver() {
        let root = tempfile::tempdir().unwrap();
        let backend = StoreBackend::Local(root.path().to_path_buf());
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::M1)
            .unwrap();

        let inspection = inspect_journal_records(backend.journal().as_ref()).unwrap();
        let value = serde_json::to_value(StoreCapabilityInspection {
            status: inspection.status,
            cursor: inspection.cursor,
            minimum_reader_profile: inspection.minimum_reader_profile,
        })
        .unwrap();

        assert_eq!(
            value.get("minimumReaderProfile"),
            Some(&serde_json::json!("review_change_revision_v1"))
        );
        assert!(value.get("minimumProductVersion").is_none());
    }

    #[test]
    fn activation_publication_is_idempotent_and_never_overwrites_divergent_bytes() {
        let root = tempfile::tempdir().unwrap();
        let backend = StoreBackend::Local(root.path().to_path_buf());
        let journal = backend.journal();

        write_capability_fixture_for_test(journal.as_ref(), CapabilityFixtureState::M1).unwrap();
        let first = inspect_journal_records(journal.as_ref()).unwrap();
        let activation_bytes = journal
            .read_event_bytes(ROOT_ACTIVATION_LOGICAL_KEY_V1)
            .unwrap()
            .unwrap();
        assert_eq!(
            journal
                .create_record_once(ROOT_ACTIVATION_LOGICAL_KEY_V1, &activation_bytes)
                .unwrap(),
            crate::storage::CreateOutcome::AlreadyExists
        );
        let duplicate = inspect_journal_records(journal.as_ref()).unwrap();
        assert_eq!(first.cursor, duplicate.cursor);

        assert_eq!(
            journal
                .create_record_once(ROOT_ACTIVATION_LOGICAL_KEY_V1, br#"{"divergent":true}"#)
                .unwrap(),
            crate::storage::CreateOutcome::AlreadyExists
        );
        assert_eq!(
            first.cursor,
            inspect_journal_records(journal.as_ref()).unwrap().cursor
        );
    }

    #[test]
    fn future_capability_successors_must_be_monotonic_supersets() {
        assert!(
            validate_monotonic_successor(&["a".to_owned()], &["a".to_owned(), "b".to_owned()])
                .is_ok()
        );
        assert!(
            validate_monotonic_successor(&["a".to_owned(), "b".to_owned()], &["b".to_owned()])
                .is_err()
        );
        assert!(validate_monotonic_successor(&["a".to_owned()], &["a".to_owned()]).is_err());
    }

    #[test]
    fn malformed_unknown_and_divergent_control_records_fail_closed() {
        for corruption in [
            CapabilityFixtureCorruption::Malformed,
            CapabilityFixtureCorruption::UnknownSchema,
            CapabilityFixtureCorruption::UnsupportedMinimumReaderProfile,
            CapabilityFixtureCorruption::DivergentLogicalKey,
            CapabilityFixtureCorruption::CompletionWithoutCoverage,
        ] {
            let root = tempfile::tempdir().unwrap();
            let backend = StoreBackend::Local(root.path().to_path_buf());
            write_corrupt_capability_fixture_for_test(backend.journal().as_ref(), corruption)
                .unwrap();
            assert!(inspect_journal_records(backend.journal().as_ref()).is_err());
        }
    }

    #[test]
    fn production_source_has_no_public_activation_route() {
        let source = include_str!("capabilities.rs");
        assert!(!source.contains(&["pub fn ", "activate"].concat()));
        assert!(!source.contains(&["pub(crate) fn ", "activate"].concat()));
        assert!(!source.contains(&["Command::", "Activate"].concat()));
    }

    /// Operator-only fixture producer for the native old-reader falsifier. It
    /// is ignored by every ordinary suite, accepts only an explicit L0 store
    /// directory, and emits no private data.
    #[test]
    #[ignore = "qualification operator only"]
    fn materialize_native_capability_fixture() {
        let store_dir = std::env::var_os("POINTBREAK_CAPABILITY_FIXTURE_STORE")
            .map(std::path::PathBuf::from)
            .expect("POINTBREAK_CAPABILITY_FIXTURE_STORE is required");
        let state = match std::env::var("POINTBREAK_CAPABILITY_FIXTURE_STATE").as_deref() {
            Ok("m1") => CapabilityFixtureState::M1,
            Ok("l2") => CapabilityFixtureState::L2,
            other => panic!("POINTBREAK_CAPABILITY_FIXTURE_STATE must be m1 or l2, got {other:?}"),
        };
        std::fs::create_dir_all(&store_dir).expect("create explicit fixture store");
        let journal = crate::session::store::backend::LocalJournal::new(&store_dir);
        assert!(
            matches!(
                inspect_journal_records(&journal)
                    .expect("explicit fixture source must be a valid L0 store")
                    .status,
                StoreCapabilityStatus::MigrationRequired
            ),
            "explicit fixture source must not already be activated"
        );
        write_capability_fixture_for_test(&journal, state).expect("materialize fixture");
        let inspection = inspect_journal_records(&journal).expect("positive-control fixture");
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "schema": "pointbreak.qualification-capability-fixture-receipt",
                "version": 1,
                "state": inspection.status,
                "cursor": inspection.cursor,
                "minimumReaderProfile": inspection.minimum_reader_profile,
                "storePathClass": "explicit_disposable",
            }))
            .expect("fixture receipt JSON")
        );
    }

    /// Read-only positive control over a retained native fixture root.
    #[test]
    #[ignore = "qualification operator only"]
    fn inspect_native_capability_fixture() {
        let store_dir = std::env::var_os("POINTBREAK_CAPABILITY_FIXTURE_STORE")
            .map(std::path::PathBuf::from)
            .expect("POINTBREAK_CAPABILITY_FIXTURE_STORE is required");
        let journal = crate::session::store::backend::LocalJournal::new(&store_dir);
        let inspection = inspect_journal_records(&journal).expect("positive-control fixture");
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "schema": "pointbreak.qualification-capability-positive-control",
                "version": 1,
                "state": inspection.status,
                "cursor": inspection.cursor,
                "minimumReaderProfile": inspection.minimum_reader_profile,
            }))
            .expect("positive-control JSON")
        );
    }
}
