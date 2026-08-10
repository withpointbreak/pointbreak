//! Capability-bound, bodyless Change semantic generation.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::canonical_hash::{sha256_bytes_hex, sha256_json_prefixed};
use crate::error::{Result, ShoreError};
use crate::model::{ActorId, EventId, TrackId};
use crate::session::derived_access::cursor::TruthCursor;
use crate::session::derived_access::generation::{
    GenerationDescriptor, GenerationLayout, GenerationPublication,
};
use crate::session::derived_access::product_contract::DerivedAccessProfile;
use crate::session::derived_access::semantic::{SemanticFact, SemanticSnapshot};
use crate::session::event::ShoreEvent;
use crate::session::projection::change::{
    ChangeDocumentProjectionFact, ChangeProjectionFact, extract_change_projection_fact,
    project_change_documents_from_facts, project_changes_from_facts,
};
use crate::session::store::EventStore;
use crate::session::store::backend::JournalChangeStamp;
use crate::session::store::capabilities::{
    AUTHORITY_CURSOR_SCHEMA_V2, JournalInspection, REVIEW_CHANGE_REVISION_COHORT_V1,
    StoreCapabilityStatus, reader_profile_versions_v1,
};
use crate::session::{AuthorityCursorV2, ChangeDocumentProjectionV1, ChangeProjection};

pub(crate) const CHANGE_SEMANTIC_GENERATION_SCHEMA_V2: &str =
    "pointbreak.derived-change-semantic-generation.v2";
pub(crate) const CHANGE_READER_PROFILE_RECEIPT_SCHEMA_V2: &str =
    "pointbreak.derived-change-reader-profile-receipt.v2";
pub(crate) const CHANGE_READER_PROFILE_RECEIPT_SCHEMA_V3: &str =
    "pointbreak.derived-change-reader-profile-receipt.v3";
pub(crate) const READER_PROJECTION_CHECKPOINT_SCHEMA_V1: &str =
    "pointbreak.derived-change-reader-projection-checkpoint.v1";
pub(crate) const CHANGE_GENERATION_STAMP_SCHEMA_V1: &str =
    "pointbreak.derived-change-generation-stamp.v1";
const CHANGE_SEMANTIC_RESOURCE: &str = "change-semantic.json";
pub(crate) const CHANGE_READER_PROFILE_RESOURCE_V3: &str = "change-reader-profile.json";
const CURSOR_PROFILE_ID_V1: &str = "pointbreak.sqlite-derived-access-cursor.v1";
const CURSOR_SCHEMA_VERSION_V1: u32 = 4;
const LOCATOR_PROFILE_ID_V1: &str = "pointbreak.sqlite-derived-access-locator.v1";
const LOCATOR_SCHEMA_VERSION_V1: u32 = 3;
const SEMANTIC_PROFILE_ID_V1: &str = "pointbreak.sqlite-derived-access-semantic.v1";
const SEMANTIC_SCHEMA_VERSION_V1: u32 = 8;
const PRODUCT_HISTORY_PROFILE_ID_V1: &str = "pointbreak.sqlite-derived-access-history.v1";
const PRODUCT_HISTORY_SCHEMA_VERSION_V1: u32 = 3;
const READER_PROJECTOR_VERSIONS_V1: &[(&str, u32)] =
    &[("change-document", 1), ("change-semantic", 2)];

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChangeFactAvailabilityV1 {
    pub(crate) family: String,
    pub(crate) count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChangeResourceAvailabilityV1 {
    pub(crate) content_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReaderDocumentVersionV1 {
    pub(crate) schema: String,
    pub(crate) version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CapabilityCarrierBindingV1 {
    pub(crate) logical_key: String,
    pub(crate) key_digest: String,
    pub(crate) record_sha256: String,
}

impl CapabilityCarrierBindingV1 {
    pub(crate) fn new(logical_key: impl Into<String>, record_sha256: impl Into<String>) -> Self {
        let logical_key = logical_key.into();
        Self {
            key_digest: sha256_bytes_hex(logical_key.as_bytes()),
            logical_key,
            record_sha256: record_sha256.into(),
        }
    }

    fn validate(&self, name: &str) -> ChangeReaderContractResult<()> {
        if self.logical_key.is_empty()
            || !is_lower_sha256(&self.key_digest)
            || self.key_digest != sha256_bytes_hex(self.logical_key.as_bytes())
        {
            return Err(ChangeReaderContractError::invalid(format!(
                "{name} carrier locator does not match its logical key"
            )));
        }
        validate_prefixed_sha256(&format!("{name} carrier witness"), &self.record_sha256)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReaderSchemaIdentityV1 {
    pub(crate) profile_id: String,
    pub(crate) schema_version: u32,
}

impl ReaderSchemaIdentityV1 {
    fn validate(&self, name: &str) -> ChangeReaderContractResult<()> {
        if self.profile_id.is_empty() || self.schema_version == 0 {
            return Err(ChangeReaderContractError::invalid(format!(
                "{name} schema identity is incomplete"
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReaderProjectorVersionV1 {
    pub(crate) projector: String,
    pub(crate) version: u32,
}

fn reader_document_versions_v1() -> Vec<ReaderDocumentVersionV1> {
    reader_profile_versions_v1()
        .iter()
        .map(|reservation| ReaderDocumentVersionV1 {
            schema: reservation.schema.to_owned(),
            version: reservation.version,
        })
        .collect()
}

fn reader_projector_versions_v1() -> Vec<ReaderProjectorVersionV1> {
    READER_PROJECTOR_VERSIONS_V1
        .iter()
        .map(|(projector, version)| ReaderProjectorVersionV1 {
            projector: (*projector).to_owned(),
            version: *version,
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AppliedProjectionCheckpointV1 {
    pub(crate) identity: ReaderSchemaIdentityV1,
    #[serde(with = "truth_cursor_wire")]
    pub(crate) applied_cursor: TruthCursor,
    pub(crate) receipt_sha256: String,
}

impl AppliedProjectionCheckpointV1 {
    fn validate(&self, name: &str) -> ChangeReaderContractResult<()> {
        self.identity.validate(name)?;
        validate_truth_cursor(name, self.applied_cursor)?;
        validate_prefixed_sha256(&format!("{name} checkpoint receipt"), &self.receipt_sha256)?;
        if self.receipt_sha256 != applied_projection_checkpoint_sha256_v1(self)? {
            return Err(ChangeReaderContractError::invalid(format!(
                "{name} checkpoint receipt mismatch"
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LocatorProjectionCheckpointV1 {
    pub(crate) identity: ReaderSchemaIdentityV1,
    #[serde(with = "truth_cursor_wire")]
    pub(crate) applied_cursor: TruthCursor,
    #[serde(with = "truth_cursor_wire")]
    pub(crate) observed_cursor: TruthCursor,
    pub(crate) receipt_sha256: String,
}

impl LocatorProjectionCheckpointV1 {
    fn validate(&self) -> ChangeReaderContractResult<()> {
        self.identity.validate("locator")?;
        validate_truth_cursor("locator applied", self.applied_cursor)?;
        validate_truth_cursor("locator observed", self.observed_cursor)?;
        if self.observed_cursor.epoch != self.applied_cursor.epoch
            || self.observed_cursor.sequence < self.applied_cursor.sequence
        {
            return Err(ChangeReaderContractError::invalid(
                "locator observed checkpoint is behind its applied checkpoint",
            ));
        }
        validate_prefixed_sha256("locator checkpoint receipt", &self.receipt_sha256)?;
        if self.receipt_sha256 != locator_projection_checkpoint_sha256_v1(self)? {
            return Err(ChangeReaderContractError::invalid(
                "locator checkpoint receipt mismatch",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChangeReaderProfileReceiptV3 {
    pub(crate) schema: String,
    pub(crate) version: u32,
    pub(crate) minimum_reader_profile: String,
    pub(crate) publication_generation_id: String,
    pub(crate) publication_store_id: String,
    pub(crate) publication_profile: DerivedAccessProfile,
    pub(crate) publication_activation_id: String,
    pub(crate) publication_manifest_hash: String,
    pub(crate) publication_completion_id: String,
    pub(crate) publication_activation_carrier: CapabilityCarrierBindingV1,
    pub(crate) publication_completion_carrier: CapabilityCarrierBindingV1,
    pub(crate) publication_authority_cursor: AuthorityCursorV2,
    pub(crate) publication_platform_stamp: JournalChangeStamp,
    #[serde(with = "truth_cursor_wire")]
    pub(crate) publication_truth_cursor: TruthCursor,
    pub(crate) publication_cursor_identity: ReaderSchemaIdentityV1,
    pub(crate) publication_locator_checkpoint: LocatorProjectionCheckpointV1,
    pub(crate) publication_semantic_checkpoint: AppliedProjectionCheckpointV1,
    pub(crate) publication_product_checkpoint: AppliedProjectionCheckpointV1,
    pub(crate) projector_versions: Vec<ReaderProjectorVersionV1>,
    pub(crate) document_versions: Vec<ReaderDocumentVersionV1>,
    pub(crate) fact_availability: Vec<ChangeFactAvailabilityV1>,
    pub(crate) resource_availability: Vec<ChangeResourceAvailabilityV1>,
    pub(crate) projection_sha256: String,
    pub(crate) document_projection_sha256: String,
    pub(crate) semantic_receipt: String,
    pub(crate) reader_document_registry_sha256: String,
    pub(crate) receipt_sha256: String,
}

impl ChangeReaderProfileReceiptV3 {
    pub(crate) fn validate(&self) -> ChangeReaderContractResult<()> {
        if self.schema != CHANGE_READER_PROFILE_RECEIPT_SCHEMA_V3 || self.version != 3 {
            return Err(ChangeReaderContractError::invalid(
                "incompatible Change reader-profile receipt schema",
            ));
        }
        if self.minimum_reader_profile != REVIEW_CHANGE_REVISION_COHORT_V1 {
            return Err(ChangeReaderContractError::invalid(
                "Change reader-profile receipt has the wrong minimum reader profile",
            ));
        }
        if self.publication_generation_id.is_empty() || self.publication_store_id.is_empty() {
            return Err(ChangeReaderContractError::invalid(
                "Change reader-profile publication identity is incomplete",
            ));
        }
        if self.publication_profile != DerivedAccessProfile::SqliteWalBodylessV1 {
            return Err(ChangeReaderContractError::invalid(
                "Change reader-profile publication has the wrong derived profile",
            ));
        }
        validate_named_sha256(
            "publication activation ID",
            &self.publication_activation_id,
            "capability-activation:",
        )?;
        validate_prefixed_sha256("publication manifest", &self.publication_manifest_hash)?;
        validate_named_sha256(
            "publication completion ID",
            &self.publication_completion_id,
            "bulk-adoption-completion:",
        )?;
        self.publication_activation_carrier
            .validate("publication activation")?;
        self.publication_completion_carrier
            .validate("publication completion")?;
        if self.publication_activation_carrier.logical_key
            != "store_capability_activation:review_change_revision_v1:root"
            || self.publication_completion_carrier.logical_key
                != format!(
                    "bulk_adoption_completion:{}",
                    self.publication_completion_id
                )
        {
            return Err(ChangeReaderContractError::invalid(
                "publication capability carrier locator mismatch",
            ));
        }
        validate_authority_cursor(&self.publication_authority_cursor)?;
        validate_platform_stamp(&self.publication_platform_stamp)?;
        validate_truth_cursor("publication truth", self.publication_truth_cursor)?;
        validate_schema_identities(
            &self.publication_cursor_identity,
            &self.publication_locator_checkpoint.identity,
            &self.publication_semantic_checkpoint.identity,
            &self.publication_product_checkpoint.identity,
        )?;
        self.publication_locator_checkpoint.validate()?;
        self.publication_semantic_checkpoint.validate("semantic")?;
        self.publication_product_checkpoint.validate("product")?;
        validate_projector_versions(&self.projector_versions)?;

        let expected_document_versions = reader_document_versions_v1();
        if self.document_versions != expected_document_versions {
            return Err(ChangeReaderContractError::invalid(
                "Change reader-profile document versions mismatch",
            ));
        }
        if self
            .fact_availability
            .windows(2)
            .any(|pair| pair[0].family >= pair[1].family)
            || self
                .fact_availability
                .iter()
                .any(|fact| fact.family.is_empty())
        {
            return Err(ChangeReaderContractError::invalid(
                "Change fact availability is not canonical",
            ));
        }
        for resource in &self.resource_availability {
            validate_prefixed_sha256("Change resource availability", &resource.content_hash)?;
        }
        if self
            .resource_availability
            .windows(2)
            .any(|pair| pair[0].content_hash >= pair[1].content_hash)
        {
            return Err(ChangeReaderContractError::invalid(
                "Change resource availability is not canonical",
            ));
        }
        validate_prefixed_sha256("Change projection", &self.projection_sha256)?;
        validate_prefixed_sha256(
            "Change document projection",
            &self.document_projection_sha256,
        )?;
        validate_prefixed_sha256("Change semantic receipt", &self.semantic_receipt)?;
        validate_prefixed_sha256(
            "reader-document registry",
            &self.reader_document_registry_sha256,
        )?;
        if self.reader_document_registry_sha256 != reader_document_registry_sha256_v1()? {
            return Err(ChangeReaderContractError::invalid(
                "reader-document registry hash mismatch",
            ));
        }
        validate_exact_projection_sequence(
            &self.publication_authority_cursor,
            self.publication_truth_cursor,
            &self.publication_locator_checkpoint,
            &self.publication_semantic_checkpoint,
            &self.publication_product_checkpoint,
        )?;
        validate_prefixed_sha256("Change reader-profile receipt", &self.receipt_sha256)?;
        if self.receipt_sha256 != change_reader_profile_receipt_sha256_v3(self)? {
            return Err(ChangeReaderContractError::invalid(
                "Change reader-profile receipt self-hash mismatch",
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_for_descriptor(
        &self,
        descriptor: &GenerationDescriptor,
    ) -> ChangeReaderContractResult<()> {
        self.validate()?;
        if descriptor.generation_id != self.publication_generation_id
            || descriptor.store_id != self.publication_store_id
            || descriptor.profile != self.publication_profile
            || descriptor.epoch != self.publication_truth_cursor.epoch
            || descriptor.head_sequence != self.publication_truth_cursor.sequence
            || descriptor.authority_stamp != self.publication_platform_stamp
            || descriptor.semantic_receipt != self.receipt_sha256
        {
            return Err(ChangeReaderContractError::invalid(
                "generation descriptor does not bind the Change reader-profile publication",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReaderProjectionCheckpointV1 {
    pub(crate) schema: String,
    pub(crate) version: u32,
    pub(crate) store_id: String,
    pub(crate) reader_receipt_sha256: String,
    pub(crate) authority_cursor: AuthorityCursorV2,
    #[serde(with = "truth_cursor_wire")]
    pub(crate) truth_cursor: TruthCursor,
    pub(crate) cursor_identity: ReaderSchemaIdentityV1,
    pub(crate) locator_checkpoint: LocatorProjectionCheckpointV1,
    pub(crate) semantic_checkpoint: AppliedProjectionCheckpointV1,
    pub(crate) product_checkpoint: AppliedProjectionCheckpointV1,
    pub(crate) projector_versions: Vec<ReaderProjectorVersionV1>,
    pub(crate) checkpoint_sha256: String,
}

impl ReaderProjectionCheckpointV1 {
    pub(crate) fn validate_for_receipt(
        &self,
        receipt: &ChangeReaderProfileReceiptV3,
    ) -> ChangeReaderContractResult<()> {
        receipt.validate()?;
        if self.schema != READER_PROJECTION_CHECKPOINT_SCHEMA_V1 || self.version != 1 {
            return Err(ChangeReaderContractError::rebuild(
                "unsupported reader projection checkpoint schema",
            ));
        }
        if self.reader_receipt_sha256 != receipt.receipt_sha256 {
            return Err(ChangeReaderContractError::rebuild(
                "reader projection checkpoint has the wrong publication anchor",
            ));
        }
        if self.store_id != receipt.publication_store_id
            || self.authority_cursor.capability_set_hash
                != receipt.publication_authority_cursor.capability_set_hash
            || self.truth_cursor.epoch != receipt.publication_truth_cursor.epoch
            || self.cursor_identity != receipt.publication_cursor_identity
            || self.projector_versions != receipt.projector_versions
        {
            return Err(ChangeReaderContractError::rebuild(
                "reader projection checkpoint is incompatible with its publication",
            ));
        }
        if self.authority_cursor.event_count < receipt.publication_authority_cursor.event_count
            || self.authority_cursor.journal_record_count
                < receipt.publication_authority_cursor.journal_record_count
            || self.truth_cursor.sequence < receipt.publication_truth_cursor.sequence
        {
            return Err(ChangeReaderContractError::invalid(
                "reader projection checkpoint regresses behind its publication",
            ));
        }
        self.validate_self()
    }

    fn validate_self(&self) -> ChangeReaderContractResult<()> {
        if self.store_id.is_empty() {
            return Err(ChangeReaderContractError::invalid(
                "reader projection checkpoint store identity is empty",
            ));
        }
        validate_prefixed_sha256("reader receipt anchor", &self.reader_receipt_sha256)?;
        validate_authority_cursor(&self.authority_cursor)?;
        validate_truth_cursor("live truth", self.truth_cursor)?;
        validate_schema_identities(
            &self.cursor_identity,
            &self.locator_checkpoint.identity,
            &self.semantic_checkpoint.identity,
            &self.product_checkpoint.identity,
        )?;
        self.locator_checkpoint.validate()?;
        self.semantic_checkpoint.validate("semantic")?;
        self.product_checkpoint.validate("product")?;
        validate_projector_versions(&self.projector_versions)?;
        validate_exact_projection_sequence(
            &self.authority_cursor,
            self.truth_cursor,
            &self.locator_checkpoint,
            &self.semantic_checkpoint,
            &self.product_checkpoint,
        )?;
        validate_prefixed_sha256("reader projection checkpoint", &self.checkpoint_sha256)?;
        if self.checkpoint_sha256 != reader_projection_checkpoint_sha256_v1(self)? {
            return Err(ChangeReaderContractError::invalid(
                "reader projection checkpoint self-hash mismatch",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RuntimeTrustIdentityV1 {
    NotApplicable,
    Bound { trust_set_sha256: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChangeGenerationStampPreimageV1 {
    pub(crate) schema: String,
    pub(crate) version: u32,
    pub(crate) reader_receipt_sha256: String,
    pub(crate) reader_checkpoint: ReaderProjectionCheckpointV1,
    pub(crate) semantic_projection_sha256: String,
    pub(crate) document_projection_sha256: String,
    pub(crate) presentation_identity_sha256: String,
    pub(crate) presentation_event_set_hash: String,
    pub(crate) internal_schema_sha256: String,
    pub(crate) runtime_trust_identity: RuntimeTrustIdentityV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ChangeReaderProfileReceiptProbeV1 {
    Current(Box<ChangeReaderProfileReceiptV3>),
    RebuildRequired {
        schema: Option<String>,
        version: Option<u64>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum ChangeReaderContractError {
    #[error("Change reader contract requires rebuild: {0}")]
    RebuildRequired(String),
    #[error("Change reader contract is invalid: {0}")]
    Invalid(String),
}

impl ChangeReaderContractError {
    fn rebuild(message: impl Into<String>) -> Self {
        Self::RebuildRequired(message.into())
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    pub(crate) const fn is_rebuild_required(&self) -> bool {
        matches!(self, Self::RebuildRequired(_))
    }
}

type ChangeReaderContractResult<T> = std::result::Result<T, ChangeReaderContractError>;

pub(crate) fn applied_projection_checkpoint_sha256_v1(
    checkpoint: &AppliedProjectionCheckpointV1,
) -> ChangeReaderContractResult<String> {
    sha256_json_without_field(checkpoint, "receiptSha256")
}

pub(crate) fn locator_projection_checkpoint_sha256_v1(
    checkpoint: &LocatorProjectionCheckpointV1,
) -> ChangeReaderContractResult<String> {
    sha256_json_without_field(checkpoint, "receiptSha256")
}

pub(crate) fn change_reader_profile_receipt_sha256_v3(
    receipt: &ChangeReaderProfileReceiptV3,
) -> ChangeReaderContractResult<String> {
    sha256_json_without_field(receipt, "receiptSha256")
}

pub(crate) fn reader_projection_checkpoint_sha256_v1(
    checkpoint: &ReaderProjectionCheckpointV1,
) -> ChangeReaderContractResult<String> {
    sha256_json_without_field(checkpoint, "checkpointSha256")
}

pub(crate) fn reader_document_registry_sha256_v1() -> ChangeReaderContractResult<String> {
    contract_sha256(&crate::documents::change_revision_document_registry())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_change_reader_profile_receipt_v3(
    generation_id: &str,
    store_id: &str,
    inspection: &JournalInspection,
    activation_record_sha256: String,
    completion_record_sha256: String,
    platform_stamp: JournalChangeStamp,
    truth_cursor: TruthCursor,
    events: &[ShoreEvent],
    semantic_snapshot: &SemanticSnapshot,
    compact_document_projection: &ChangeDocumentProjectionV1,
) -> Result<ChangeReaderProfileReceiptV3> {
    let StoreCapabilityStatus::Ready {
        activation_id,
        manifest_hash,
        completion_id,
    } = &inspection.status
    else {
        return Err(ShoreError::Message(
            "Change reader publication requires completed capability authority".to_owned(),
        ));
    };
    if inspection.minimum_reader_profile.as_deref() != Some(REVIEW_CHANGE_REVISION_COHORT_V1) {
        return Err(ShoreError::Message(
            "completed Change authority has the wrong minimum reader profile".to_owned(),
        ));
    }
    if inspection.cursor.event_count != truth_cursor.sequence {
        return Err(ShoreError::Message(format!(
            "Change authority event count {} differs from truth cursor {}",
            inspection.cursor.event_count, truth_cursor.sequence
        )));
    }
    let strict_projection = crate::session::project_changes(events)?;
    if semantic_snapshot.changes != strict_projection {
        return Err(ShoreError::Message(
            "compact Change semantic projection diverges from strict replay".to_owned(),
        ));
    }
    let strict_document_projection = crate::session::project_change_documents(events)?;
    if compact_document_projection != &strict_document_projection {
        return Err(ShoreError::Message(
            "compact Change document projection diverges from strict replay".to_owned(),
        ));
    }
    crate::documents::ChangeDocumentFacadeV1::new(
        semantic_snapshot.changes.clone(),
        compact_document_projection.clone(),
    )?;

    let locator_checkpoint = publication_locator_checkpoint(truth_cursor)
        .map_err(|error| ShoreError::Message(error.to_string()))?;
    let semantic_checkpoint = publication_applied_checkpoint(
        SEMANTIC_PROFILE_ID_V1,
        SEMANTIC_SCHEMA_VERSION_V1,
        truth_cursor,
    )
    .map_err(|error| ShoreError::Message(error.to_string()))?;
    let product_checkpoint = publication_applied_checkpoint(
        PRODUCT_HISTORY_PROFILE_ID_V1,
        PRODUCT_HISTORY_SCHEMA_VERSION_V1,
        truth_cursor,
    )
    .map_err(|error| ShoreError::Message(error.to_string()))?;
    let mut receipt = ChangeReaderProfileReceiptV3 {
        schema: CHANGE_READER_PROFILE_RECEIPT_SCHEMA_V3.to_owned(),
        version: 3,
        minimum_reader_profile: REVIEW_CHANGE_REVISION_COHORT_V1.to_owned(),
        publication_generation_id: generation_id.to_owned(),
        publication_store_id: store_id.to_owned(),
        publication_profile: DerivedAccessProfile::SqliteWalBodylessV1,
        publication_activation_id: activation_id.clone(),
        publication_manifest_hash: manifest_hash.clone(),
        publication_completion_id: completion_id.clone(),
        publication_activation_carrier: CapabilityCarrierBindingV1::new(
            "store_capability_activation:review_change_revision_v1:root",
            activation_record_sha256,
        ),
        publication_completion_carrier: CapabilityCarrierBindingV1::new(
            format!("bulk_adoption_completion:{completion_id}"),
            completion_record_sha256,
        ),
        publication_authority_cursor: inspection.cursor.clone(),
        publication_platform_stamp: platform_stamp,
        publication_truth_cursor: truth_cursor,
        publication_cursor_identity: ReaderSchemaIdentityV1 {
            profile_id: CURSOR_PROFILE_ID_V1.to_owned(),
            schema_version: CURSOR_SCHEMA_VERSION_V1,
        },
        publication_locator_checkpoint: locator_checkpoint,
        publication_semantic_checkpoint: semantic_checkpoint,
        publication_product_checkpoint: product_checkpoint,
        projector_versions: reader_projector_versions_v1(),
        document_versions: reader_document_versions_v1(),
        fact_availability: fact_availability(events),
        resource_availability: resource_availability(events)?,
        projection_sha256: sha256_json_prefixed(&serde_json::to_value(
            &semantic_snapshot.changes,
        )?)?,
        document_projection_sha256: sha256_json_prefixed(&serde_json::to_value(
            compact_document_projection,
        )?)?,
        semantic_receipt: semantic_snapshot.semantic_receipt.clone(),
        reader_document_registry_sha256: reader_document_registry_sha256_v1()
            .map_err(|error| ShoreError::Message(error.to_string()))?,
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = change_reader_profile_receipt_sha256_v3(&receipt)
        .map_err(|error| ShoreError::Message(error.to_string()))?;
    receipt
        .validate()
        .map_err(|error| ShoreError::Message(error.to_string()))?;
    Ok(receipt)
}

pub(crate) fn initial_reader_projection_checkpoint_v1(
    receipt: &ChangeReaderProfileReceiptV3,
) -> ChangeReaderContractResult<ReaderProjectionCheckpointV1> {
    receipt.validate()?;
    let mut checkpoint = ReaderProjectionCheckpointV1 {
        schema: READER_PROJECTION_CHECKPOINT_SCHEMA_V1.to_owned(),
        version: 1,
        store_id: receipt.publication_store_id.clone(),
        reader_receipt_sha256: receipt.receipt_sha256.clone(),
        authority_cursor: receipt.publication_authority_cursor.clone(),
        truth_cursor: receipt.publication_truth_cursor,
        cursor_identity: receipt.publication_cursor_identity.clone(),
        locator_checkpoint: receipt.publication_locator_checkpoint.clone(),
        semantic_checkpoint: receipt.publication_semantic_checkpoint.clone(),
        product_checkpoint: receipt.publication_product_checkpoint.clone(),
        projector_versions: receipt.projector_versions.clone(),
        checkpoint_sha256: String::new(),
    };
    checkpoint.checkpoint_sha256 = reader_projection_checkpoint_sha256_v1(&checkpoint)?;
    checkpoint.validate_for_receipt(receipt)?;
    Ok(checkpoint)
}

pub(crate) fn advance_reader_projection_checkpoint_v1(
    checkpoint: &ReaderProjectionCheckpointV1,
    authority_cursor: AuthorityCursorV2,
    truth_cursor: TruthCursor,
) -> ChangeReaderContractResult<ReaderProjectionCheckpointV1> {
    checkpoint.validate_self()?;
    let mut advanced = checkpoint.clone();
    advanced.authority_cursor = authority_cursor;
    advanced.truth_cursor = truth_cursor;
    advanced.locator_checkpoint.applied_cursor = truth_cursor;
    advanced.locator_checkpoint.observed_cursor = truth_cursor;
    advanced.locator_checkpoint.receipt_sha256 =
        locator_projection_checkpoint_sha256_v1(&advanced.locator_checkpoint)?;
    advanced.semantic_checkpoint.applied_cursor = truth_cursor;
    advanced.semantic_checkpoint.receipt_sha256 =
        applied_projection_checkpoint_sha256_v1(&advanced.semantic_checkpoint)?;
    advanced.product_checkpoint.applied_cursor = truth_cursor;
    advanced.product_checkpoint.receipt_sha256 =
        applied_projection_checkpoint_sha256_v1(&advanced.product_checkpoint)?;
    advanced.checkpoint_sha256 = reader_projection_checkpoint_sha256_v1(&advanced)?;
    advanced.validate_self()?;
    Ok(advanced)
}

fn publication_applied_checkpoint(
    profile_id: &str,
    schema_version: u32,
    cursor: TruthCursor,
) -> ChangeReaderContractResult<AppliedProjectionCheckpointV1> {
    let mut checkpoint = AppliedProjectionCheckpointV1 {
        identity: ReaderSchemaIdentityV1 {
            profile_id: profile_id.to_owned(),
            schema_version,
        },
        applied_cursor: cursor,
        receipt_sha256: String::new(),
    };
    checkpoint.receipt_sha256 = applied_projection_checkpoint_sha256_v1(&checkpoint)?;
    Ok(checkpoint)
}

fn publication_locator_checkpoint(
    cursor: TruthCursor,
) -> ChangeReaderContractResult<LocatorProjectionCheckpointV1> {
    let mut checkpoint = LocatorProjectionCheckpointV1 {
        identity: ReaderSchemaIdentityV1 {
            profile_id: LOCATOR_PROFILE_ID_V1.to_owned(),
            schema_version: LOCATOR_SCHEMA_VERSION_V1,
        },
        applied_cursor: cursor,
        observed_cursor: cursor,
        receipt_sha256: String::new(),
    };
    checkpoint.receipt_sha256 = locator_projection_checkpoint_sha256_v1(&checkpoint)?;
    Ok(checkpoint)
}

pub(crate) fn probe_change_reader_profile_receipt(
    bytes: &[u8],
) -> ChangeReaderContractResult<ChangeReaderProfileReceiptProbeV1> {
    let value = serde_json::from_slice::<serde_json::Value>(bytes)
        .map_err(|error| ChangeReaderContractError::invalid(error.to_string()))?;
    let object = value.as_object().ok_or_else(|| {
        ChangeReaderContractError::invalid("Change reader-profile receipt is not an object")
    })?;
    let schema = object
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let version = object.get("version").and_then(serde_json::Value::as_u64);
    if schema.as_deref() != Some(CHANGE_READER_PROFILE_RECEIPT_SCHEMA_V3) || version != Some(3) {
        return Ok(ChangeReaderProfileReceiptProbeV1::RebuildRequired { schema, version });
    }
    let receipt = serde_json::from_value::<ChangeReaderProfileReceiptV3>(value)
        .map_err(|error| ChangeReaderContractError::invalid(error.to_string()))?;
    receipt.validate()?;
    Ok(ChangeReaderProfileReceiptProbeV1::Current(Box::new(
        receipt,
    )))
}

pub(crate) fn change_generation_stamp_sha256_v1(
    preimage: &ChangeGenerationStampPreimageV1,
) -> ChangeReaderContractResult<String> {
    if preimage.schema != CHANGE_GENERATION_STAMP_SCHEMA_V1 || preimage.version != 1 {
        return Err(ChangeReaderContractError::invalid(
            "incompatible Change generation-stamp preimage schema",
        ));
    }
    validate_prefixed_sha256(
        "generation-stamp reader receipt",
        &preimage.reader_receipt_sha256,
    )?;
    preimage.reader_checkpoint.validate_self()?;
    if preimage.reader_receipt_sha256 != preimage.reader_checkpoint.reader_receipt_sha256 {
        return Err(ChangeReaderContractError::invalid(
            "generation-stamp receipt anchor mismatch",
        ));
    }
    if preimage.presentation_event_set_hash
        != preimage.reader_checkpoint.authority_cursor.event_set_hash
    {
        return Err(ChangeReaderContractError::invalid(
            "generation-stamp presentation event set does not match current authority",
        ));
    }
    for (name, value) in [
        (
            "generation-stamp semantic projection",
            &preimage.semantic_projection_sha256,
        ),
        (
            "generation-stamp document projection",
            &preimage.document_projection_sha256,
        ),
        (
            "generation-stamp presentation identity",
            &preimage.presentation_identity_sha256,
        ),
        (
            "generation-stamp presentation event set",
            &preimage.presentation_event_set_hash,
        ),
        (
            "generation-stamp internal schema",
            &preimage.internal_schema_sha256,
        ),
    ] {
        validate_prefixed_sha256(name, value)?;
    }
    if let RuntimeTrustIdentityV1::Bound { trust_set_sha256 } = &preimage.runtime_trust_identity {
        validate_prefixed_sha256("generation-stamp runtime trust set", trust_set_sha256)?;
    }
    contract_sha256(preimage)
}

pub(crate) fn strict_change_generation_stamp_preimage_v1(
    receipt: &ChangeReaderProfileReceiptV3,
    checkpoint: &ReaderProjectionCheckpointV1,
    authority_cursor: &AuthorityCursorV2,
    semantic_projection: &ChangeProjection,
    document_projection: &ChangeDocumentProjectionV1,
    runtime_trust_identity: RuntimeTrustIdentityV1,
) -> ChangeReaderContractResult<ChangeGenerationStampPreimageV1> {
    if authority_cursor != &checkpoint.authority_cursor {
        return Err(ChangeReaderContractError::invalid(
            "strict authority cursor does not match the reader checkpoint",
        ));
    }
    assemble_change_generation_stamp_preimage_v1(
        receipt,
        checkpoint,
        contract_sha256(semantic_projection)?,
        contract_sha256(document_projection)?,
        document_projection.projection_stamp.clone(),
        authority_cursor.event_set_hash.clone(),
        frozen_reader_schema_set_sha256_v1()?,
        runtime_trust_identity,
    )
}

pub(crate) fn derived_change_generation_stamp_preimage_v1(
    receipt: &ChangeReaderProfileReceiptV3,
    checkpoint: &ReaderProjectionCheckpointV1,
    semantic_projection_sha256: impl Into<String>,
    document_projection_sha256: impl Into<String>,
    presentation_identity_sha256: impl Into<String>,
    runtime_trust_identity: RuntimeTrustIdentityV1,
) -> ChangeReaderContractResult<ChangeGenerationStampPreimageV1> {
    assemble_change_generation_stamp_preimage_v1(
        receipt,
        checkpoint,
        semantic_projection_sha256.into(),
        document_projection_sha256.into(),
        presentation_identity_sha256.into(),
        checkpoint.authority_cursor.event_set_hash.clone(),
        checkpoint_reader_schema_set_sha256_v1(checkpoint)?,
        runtime_trust_identity,
    )
}

#[allow(clippy::too_many_arguments)]
fn assemble_change_generation_stamp_preimage_v1(
    receipt: &ChangeReaderProfileReceiptV3,
    checkpoint: &ReaderProjectionCheckpointV1,
    semantic_projection_sha256: String,
    document_projection_sha256: String,
    presentation_identity_sha256: String,
    presentation_event_set_hash: String,
    internal_schema_sha256: String,
    runtime_trust_identity: RuntimeTrustIdentityV1,
) -> ChangeReaderContractResult<ChangeGenerationStampPreimageV1> {
    receipt.validate()?;
    checkpoint.validate_for_receipt(receipt)?;
    let preimage = ChangeGenerationStampPreimageV1 {
        schema: CHANGE_GENERATION_STAMP_SCHEMA_V1.to_owned(),
        version: 1,
        reader_receipt_sha256: receipt.receipt_sha256.clone(),
        reader_checkpoint: checkpoint.clone(),
        semantic_projection_sha256,
        document_projection_sha256,
        presentation_identity_sha256,
        presentation_event_set_hash,
        internal_schema_sha256,
        runtime_trust_identity,
    };
    change_generation_stamp_sha256_v1(&preimage)?;
    Ok(preimage)
}

fn frozen_reader_schema_set_sha256_v1() -> ChangeReaderContractResult<String> {
    reader_schema_set_sha256_v1(
        &ReaderSchemaIdentityV1 {
            profile_id: CURSOR_PROFILE_ID_V1.to_owned(),
            schema_version: CURSOR_SCHEMA_VERSION_V1,
        },
        &ReaderSchemaIdentityV1 {
            profile_id: LOCATOR_PROFILE_ID_V1.to_owned(),
            schema_version: LOCATOR_SCHEMA_VERSION_V1,
        },
        &ReaderSchemaIdentityV1 {
            profile_id: SEMANTIC_PROFILE_ID_V1.to_owned(),
            schema_version: SEMANTIC_SCHEMA_VERSION_V1,
        },
        &ReaderSchemaIdentityV1 {
            profile_id: PRODUCT_HISTORY_PROFILE_ID_V1.to_owned(),
            schema_version: PRODUCT_HISTORY_SCHEMA_VERSION_V1,
        },
        &reader_projector_versions_v1(),
    )
}

fn checkpoint_reader_schema_set_sha256_v1(
    checkpoint: &ReaderProjectionCheckpointV1,
) -> ChangeReaderContractResult<String> {
    reader_schema_set_sha256_v1(
        &checkpoint.cursor_identity,
        &checkpoint.locator_checkpoint.identity,
        &checkpoint.semantic_checkpoint.identity,
        &checkpoint.product_checkpoint.identity,
        &checkpoint.projector_versions,
    )
}

fn reader_schema_set_sha256_v1(
    cursor: &ReaderSchemaIdentityV1,
    locator: &ReaderSchemaIdentityV1,
    semantic: &ReaderSchemaIdentityV1,
    product: &ReaderSchemaIdentityV1,
    projector_versions: &[ReaderProjectorVersionV1],
) -> ChangeReaderContractResult<String> {
    contract_sha256(&serde_json::json!({
        "cursor": cursor,
        "locator": locator,
        "semantic": semantic,
        "product": product,
        "projectorVersions": projector_versions,
    }))
}

fn validate_exact_projection_sequence(
    authority: &AuthorityCursorV2,
    truth: TruthCursor,
    locator: &LocatorProjectionCheckpointV1,
    semantic: &AppliedProjectionCheckpointV1,
    product: &AppliedProjectionCheckpointV1,
) -> ChangeReaderContractResult<()> {
    let sequences = [
        authority.event_count,
        truth.sequence,
        locator.applied_cursor.sequence,
        locator.observed_cursor.sequence,
        semantic.applied_cursor.sequence,
        product.applied_cursor.sequence,
    ];
    if sequences.iter().any(|sequence| *sequence != sequences[0]) {
        return Err(ChangeReaderContractError::invalid(
            "authority event count and projection checkpoint sequences differ",
        ));
    }
    let epochs = [
        truth.epoch,
        locator.applied_cursor.epoch,
        locator.observed_cursor.epoch,
        semantic.applied_cursor.epoch,
        product.applied_cursor.epoch,
    ];
    if epochs.iter().any(|epoch| *epoch != epochs[0]) {
        return Err(ChangeReaderContractError::invalid(
            "projection checkpoint epochs differ",
        ));
    }
    Ok(())
}

fn validate_projector_versions(
    versions: &[ReaderProjectorVersionV1],
) -> ChangeReaderContractResult<()> {
    let expected = reader_projector_versions_v1();
    if versions != expected {
        return Err(ChangeReaderContractError::invalid(
            "reader projector versions do not match the frozen projection contract",
        ));
    }
    Ok(())
}

fn validate_schema_identities(
    cursor: &ReaderSchemaIdentityV1,
    locator: &ReaderSchemaIdentityV1,
    semantic: &ReaderSchemaIdentityV1,
    product: &ReaderSchemaIdentityV1,
) -> ChangeReaderContractResult<()> {
    let observed = [
        (cursor, CURSOR_PROFILE_ID_V1, CURSOR_SCHEMA_VERSION_V1),
        (locator, LOCATOR_PROFILE_ID_V1, LOCATOR_SCHEMA_VERSION_V1),
        (semantic, SEMANTIC_PROFILE_ID_V1, SEMANTIC_SCHEMA_VERSION_V1),
        (
            product,
            PRODUCT_HISTORY_PROFILE_ID_V1,
            PRODUCT_HISTORY_SCHEMA_VERSION_V1,
        ),
    ];
    if observed.iter().any(|(identity, profile_id, version)| {
        identity.profile_id != *profile_id || identity.schema_version != *version
    }) {
        return Err(ChangeReaderContractError::invalid(
            "reader schema identities do not match the frozen derived schemas",
        ));
    }
    Ok(())
}

fn validate_authority_cursor(cursor: &AuthorityCursorV2) -> ChangeReaderContractResult<()> {
    if cursor.schema != AUTHORITY_CURSOR_SCHEMA_V2
        || cursor.event_count > cursor.journal_record_count
    {
        return Err(ChangeReaderContractError::invalid(
            "authority cursor shape mismatch",
        ));
    }
    for (name, value) in [
        (
            "authority Journal-record set",
            &cursor.journal_record_set_hash,
        ),
        ("authority event set", &cursor.event_set_hash),
        ("authority capability set", &cursor.capability_set_hash),
    ] {
        validate_prefixed_sha256(name, value)?;
    }
    Ok(())
}

fn validate_platform_stamp(stamp: &JournalChangeStamp) -> ChangeReaderContractResult<()> {
    if let JournalChangeStamp::Observed {
        identity_sha256,
        change_sha256,
        ..
    } = stamp
    {
        validate_raw_sha256("platform stamp identity", identity_sha256)?;
        validate_raw_sha256("platform stamp change", change_sha256)?;
    }
    Ok(())
}

fn validate_truth_cursor(name: &str, cursor: TruthCursor) -> ChangeReaderContractResult<()> {
    if cursor.epoch == 0 {
        return Err(ChangeReaderContractError::invalid(format!(
            "{name} cursor epoch is zero"
        )));
    }
    Ok(())
}

fn validate_named_sha256(name: &str, value: &str, prefix: &str) -> ChangeReaderContractResult<()> {
    let digest = value.strip_prefix(prefix).ok_or_else(|| {
        ChangeReaderContractError::invalid(format!("{name} has the wrong identity prefix"))
    })?;
    validate_prefixed_sha256(name, digest)
}

fn validate_prefixed_sha256(name: &str, value: &str) -> ChangeReaderContractResult<()> {
    let valid = value.strip_prefix("sha256:").is_some_and(is_lower_sha256);
    if !valid {
        return Err(ChangeReaderContractError::invalid(format!(
            "{name} is not a canonical SHA-256 identity"
        )));
    }
    Ok(())
}

fn validate_raw_sha256(name: &str, value: &str) -> ChangeReaderContractResult<()> {
    if !is_lower_sha256(value) {
        return Err(ChangeReaderContractError::invalid(format!(
            "{name} is not a canonical raw SHA-256 digest"
        )));
    }
    Ok(())
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_json_without_field<T: Serialize>(
    value: &T,
    field: &str,
) -> ChangeReaderContractResult<String> {
    let mut material = serde_json::to_value(value)
        .map_err(|error| ChangeReaderContractError::invalid(error.to_string()))?;
    let object = material
        .as_object_mut()
        .ok_or_else(|| ChangeReaderContractError::invalid("self-hash material is not an object"))?;
    if object.remove(field).is_none() {
        return Err(ChangeReaderContractError::invalid(format!(
            "self-hash material does not contain {field}"
        )));
    }
    sha256_json_prefixed(&material)
        .map_err(|error| ChangeReaderContractError::invalid(error.to_string()))
}

fn contract_sha256<T: Serialize>(value: &T) -> ChangeReaderContractResult<String> {
    let material = serde_json::to_value(value)
        .map_err(|error| ChangeReaderContractError::invalid(error.to_string()))?;
    sha256_json_prefixed(&material)
        .map_err(|error| ChangeReaderContractError::invalid(error.to_string()))
}

mod truth_cursor_wire {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::TruthCursor;

    #[derive(Deserialize, Serialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct TruthCursorWire {
        epoch: u64,
        sequence: u64,
    }

    pub(super) fn serialize<S>(cursor: &TruthCursor, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        TruthCursorWire {
            epoch: cursor.epoch,
            sequence: cursor.sequence,
        }
        .serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<TruthCursor, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cursor = TruthCursorWire::deserialize(deserializer)?;
        Ok(TruthCursor::new(cursor.epoch, cursor.sequence))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChangeReaderProfileReceiptV2 {
    pub(crate) schema: String,
    pub(crate) version: u32,
    pub(crate) minimum_reader_profile: String,
    pub(crate) authority_cursor: AuthorityCursorV2,
    pub(crate) document_versions: Vec<ReaderDocumentVersionV1>,
    pub(crate) fact_availability: Vec<ChangeFactAvailabilityV1>,
    pub(crate) resource_availability: Vec<ChangeResourceAvailabilityV1>,
    pub(crate) projection_sha256: String,
    pub(crate) document_projection_sha256: String,
    pub(crate) semantic_receipt: String,
    pub(crate) receipt_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// V2 binds the claim-provenance document projection in addition to the
/// effective semantic projection. V1 readers therefore fail closed instead of
/// serving Change documents that would require reconstructing actor support.
pub(crate) struct ChangeSemanticGenerationV2 {
    pub(crate) schema: String,
    pub(crate) version: u32,
    pub(crate) facts: Vec<ChangeProjectionFact>,
    pub(crate) projection: ChangeProjection,
    pub(crate) document_projection: ChangeDocumentProjectionV1,
    pub(crate) reader_profile: ChangeReaderProfileReceiptV2,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReaderReceiptPreimage<'a> {
    schema: &'a str,
    version: u32,
    minimum_reader_profile: &'a str,
    authority_cursor: &'a AuthorityCursorV2,
    document_versions: &'a [ReaderDocumentVersionV1],
    fact_availability: &'a [ChangeFactAvailabilityV1],
    resource_availability: &'a [ChangeResourceAvailabilityV1],
    projection_sha256: &'a str,
    document_projection_sha256: &'a str,
    semantic_receipt: &'a str,
}

fn compact_change_document_facts(facts: &[SemanticFact]) -> Vec<ChangeDocumentProjectionFact> {
    facts
        .iter()
        .filter_map(|fact| {
            fact.change.clone().map(|change| {
                ChangeDocumentProjectionFact::new(
                    change,
                    EventId::new(fact.event_id.clone()),
                    ActorId::new(fact.actor_id.clone()),
                    fact.track_id.clone().map(TrackId::new),
                )
            })
        })
        .collect()
}

pub(crate) fn build_change_semantic_generation(
    inspection: &JournalInspection,
) -> Result<ChangeSemanticGenerationV2> {
    match inspection.status {
        StoreCapabilityStatus::MigrationRequired => {
            return Err(ShoreError::Message(
                "migration_required; Change semantic generation requires completed adoption"
                    .to_owned(),
            ));
        }
        StoreCapabilityStatus::MigrationInProgress { .. } => {
            return Err(ShoreError::Message(
                "migration_in_progress; refusing a partial Change semantic generation".to_owned(),
            ));
        }
        StoreCapabilityStatus::Ready { .. } => {}
    }
    if inspection.minimum_reader_profile.as_deref() != Some(REVIEW_CHANGE_REVISION_COHORT_V1) {
        return Err(ShoreError::Message(
            "completed Change store has a mismatched minimum reader profile".to_owned(),
        ));
    }

    let mut events = Vec::with_capacity(inspection.event_entries.len());
    for entry in &inspection.event_entries {
        events.push(EventStore::decode_qualification_entry(
            entry.key_digest.clone(),
            entry.bytes.clone(),
        )?);
    }
    let facts = events
        .iter()
        .map(extract_change_projection_fact)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let projection = crate::session::project_changes(&events)?;
    let document_projection = crate::session::project_change_documents(&events)?;
    if project_changes_from_facts(&facts)? != projection {
        return Err(ShoreError::Message(
            "bodyless Change facts diverge from strict replay".to_owned(),
        ));
    }

    // The existing semantic receipt remains the cross-family strict-replay
    // witness. The Change reader receipt additionally binds the capability
    // cursor and the frozen public document versions.
    let cursor = TruthCursor::new(1, inspection.cursor.event_count.max(1));
    let semantic_snapshot = SemanticSnapshot::from_events(cursor, &events)
        .map_err(|error| ShoreError::Message(error.to_string()))?;
    let rebuilt_facts = events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            SemanticFact::from_event(
                TruthCursor::new(
                    1,
                    u64::try_from(index.saturating_add(1)).unwrap_or(u64::MAX),
                ),
                event,
                crate::canonical_hash::sha256_bytes_hex(&inspection.event_entries[index].bytes),
            )
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| ShoreError::Message(error.to_string()))?;
    let rebuilt_snapshot = SemanticSnapshot::audit_from_facts(cursor, &rebuilt_facts)
        .map_err(|error| ShoreError::Message(error.to_string()))?;
    let compact_projection = rebuilt_snapshot.changes;
    if compact_projection != projection {
        return Err(ShoreError::Message(
            "rebuilt derived Change facts diverge from strict replay".to_owned(),
        ));
    }
    let document_facts = compact_change_document_facts(&rebuilt_facts);
    let compact_document_projection = project_change_documents_from_facts(&document_facts)?;
    if compact_document_projection != document_projection {
        return Err(ShoreError::Message(
            "rebuilt derived Change document facts diverge from strict replay".to_owned(),
        ));
    }

    let fact_availability = fact_availability(&events);
    let resource_availability = resource_availability(&events)?;
    let document_versions = reader_document_versions_v1();
    let projection_sha256 = sha256_json_prefixed(&serde_json::to_value(&compact_projection)?)?;
    let mut reader_profile = ChangeReaderProfileReceiptV2 {
        schema: CHANGE_READER_PROFILE_RECEIPT_SCHEMA_V2.to_owned(),
        version: 2,
        minimum_reader_profile: REVIEW_CHANGE_REVISION_COHORT_V1.to_owned(),
        authority_cursor: inspection.cursor.clone(),
        document_versions,
        fact_availability,
        resource_availability,
        projection_sha256,
        document_projection_sha256: sha256_json_prefixed(&serde_json::to_value(
            &compact_document_projection,
        )?)?,
        semantic_receipt: semantic_snapshot.semantic_receipt,
        receipt_sha256: String::new(),
    };
    reader_profile.receipt_sha256 = reader_receipt_sha256(&reader_profile)?;

    Ok(ChangeSemanticGenerationV2 {
        schema: CHANGE_SEMANTIC_GENERATION_SCHEMA_V2.to_owned(),
        version: 2,
        facts,
        projection: compact_projection,
        document_projection: compact_document_projection,
        reader_profile,
    })
}

impl ChangeSemanticGenerationV2 {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema != CHANGE_SEMANTIC_GENERATION_SCHEMA_V2 || self.version != 2 {
            return Err(ShoreError::Message(
                "incompatible Change semantic generation schema".to_owned(),
            ));
        }
        if project_changes_from_facts(&self.facts)? != self.projection {
            return Err(ShoreError::Message(
                "Change semantic generation projection mismatch".to_owned(),
            ));
        }
        if self.reader_profile.minimum_reader_profile != REVIEW_CHANGE_REVISION_COHORT_V1
            || self.reader_profile.schema != CHANGE_READER_PROFILE_RECEIPT_SCHEMA_V2
            || self.reader_profile.version != 2
            || self.reader_profile.document_versions != reader_document_versions_v1()
            || self.reader_profile.projection_sha256
                != sha256_json_prefixed(&serde_json::to_value(&self.projection)?)?
            || self.reader_profile.document_projection_sha256
                != sha256_json_prefixed(&serde_json::to_value(&self.document_projection)?)?
            || self.document_projection.projection_stamp
                != crate::session::change_document_projection_stamp(
                    &self.projection,
                    &self.document_projection,
                )?
            || self.reader_profile.receipt_sha256 != reader_receipt_sha256(&self.reader_profile)?
            || self
                .reader_profile
                .fact_availability
                .windows(2)
                .any(|pair| pair[0].family >= pair[1].family)
            || self
                .reader_profile
                .resource_availability
                .windows(2)
                .any(|pair| pair[0].content_hash >= pair[1].content_hash)
        {
            return Err(ShoreError::Message(
                "Change reader-profile receipt mismatch".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChangeGenerationFailurePointV1 {
    None,
    AfterStaging,
    AfterPromotion,
}

#[cfg(any(test, feature = "bench"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChangeSemanticRouteV1 {
    Current,
    LooseFallback,
    ExplicitOff,
}

#[cfg(any(test, feature = "bench"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChangeSemanticReadV1 {
    pub(crate) route: ChangeSemanticRouteV1,
    pub(crate) projection: ChangeProjection,
    pub(crate) document_projection: ChangeDocumentProjectionV1,
}

fn publish_change_semantic_generation_with_failure(
    store_root: &Path,
    inspection: &JournalInspection,
    authority_now: &AuthorityCursorV2,
    enabled: bool,
    failure: ChangeGenerationFailurePointV1,
) -> Result<String> {
    if !enabled {
        return Err(ShoreError::Message(
            "derived Change semantics are explicitly off".to_owned(),
        ));
    }
    let generation = build_change_semantic_generation(inspection)?;
    generation.validate()?;
    let layout = GenerationLayout::new(store_root).map_err(generation_error)?;
    let _lease = layout.try_rebuild_lease().map_err(generation_error)?;
    layout.ensure_scaffold().map_err(generation_error)?;
    layout.discard_all_staging().map_err(generation_error)?;
    let (sequence, generation_id) = layout.next_generation().map_err(generation_error)?;
    let staging = layout.staging(&generation_id);
    std::fs::create_dir_all(&staging).map_err(|error| {
        ShoreError::Message(format!("create Change generation staging: {error}"))
    })?;
    let bytes = crate::canonical_hash::canonical_json_bytes(&serde_json::to_value(&generation)?)?;
    layout
        .write_resource(&staging, CHANGE_SEMANTIC_RESOURCE, &bytes)
        .map_err(generation_error)?;
    if failure == ChangeGenerationFailurePointV1::AfterStaging {
        return Err(ShoreError::Message(
            "interrupted after Change generation staging".to_owned(),
        ));
    }
    if inspection.cursor != *authority_now {
        layout
            .discard_staging(&generation_id)
            .map_err(generation_error)?;
        return Err(ShoreError::Message(
            "Change authority moved while the derived generation was staging".to_owned(),
        ));
    }
    let store_id = crate::session::store::resolution::opaque_path_identity("store", store_root)?;
    let descriptor = GenerationDescriptor::new(
        &generation_id,
        store_id,
        DerivedAccessProfile::SqliteWalBodylessV1,
        sequence,
        inspection.cursor.event_count,
        JournalChangeStamp::Absent,
        &generation.reader_profile.receipt_sha256,
    );
    let descriptor_sha256 = layout
        .write_descriptor(&staging, &descriptor)
        .map_err(generation_error)?;
    layout
        .promote_staging(&generation_id)
        .map_err(generation_error)?;
    if failure == ChangeGenerationFailurePointV1::AfterPromotion {
        return Err(ShoreError::Message(
            "interrupted after Change generation promotion".to_owned(),
        ));
    }
    layout
        .publish(&GenerationPublication::new(
            sequence,
            &generation_id,
            descriptor_sha256,
        ))
        .map_err(generation_error)?;
    layout
        .retire_prior_publications(sequence)
        .map_err(generation_error)?;
    let _ = layout.reclaim_inactive_generations(&generation_id);
    Ok(generation_id)
}

#[cfg(any(test, feature = "bench"))]
pub(crate) fn read_change_semantics_for_qualification(
    store_root: &Path,
    inspection: &JournalInspection,
    enabled: bool,
) -> Result<ChangeSemanticReadV1> {
    // Capability routing wins before any sidecar read. An old generation can
    // never make L0 or M1 appear semantically usable.
    match inspection.status {
        StoreCapabilityStatus::MigrationRequired => {
            return Err(ShoreError::Message("migration_required".to_owned()));
        }
        StoreCapabilityStatus::MigrationInProgress { .. } => {
            return Err(ShoreError::Message("migration_in_progress".to_owned()));
        }
        StoreCapabilityStatus::Ready { .. } => {}
    }
    let (strict, strict_documents) = strict_projections(inspection)?;
    if !enabled {
        return Ok(ChangeSemanticReadV1 {
            route: ChangeSemanticRouteV1::ExplicitOff,
            projection: strict,
            document_projection: strict_documents,
        });
    }
    let derived = (|| {
        let layout = GenerationLayout::new(store_root).map_err(generation_error)?;
        let publication = layout
            .current_publication()
            .map_err(generation_error)?
            .ok_or_else(|| {
                ShoreError::Message("Change semantic generation is absent".to_owned())
            })?;
        let descriptor = layout.descriptor(&publication).map_err(generation_error)?;
        let path = layout
            .generation(&publication.generation_id)
            .join(CHANGE_SEMANTIC_RESOURCE);
        let bytes = std::fs::read(&path).map_err(|error| {
            ShoreError::Message(format!("read Change semantic generation: {error}"))
        })?;
        let generation: ChangeSemanticGenerationV2 = serde_json::from_slice(&bytes)?;
        generation.validate()?;
        if generation.reader_profile.authority_cursor != inspection.cursor
            || descriptor.semantic_receipt != generation.reader_profile.receipt_sha256
            || generation.projection != strict
            || generation.document_projection != strict_documents
        {
            return Err(ShoreError::Message(
                "Change semantic generation is stale or divergent".to_owned(),
            ));
        }
        Ok((generation.projection, generation.document_projection))
    })();
    Ok(match derived {
        Ok((projection, document_projection)) => ChangeSemanticReadV1 {
            route: ChangeSemanticRouteV1::Current,
            projection,
            document_projection,
        },
        Err(_) => ChangeSemanticReadV1 {
            route: ChangeSemanticRouteV1::LooseFallback,
            projection: strict,
            document_projection: strict_documents,
        },
    })
}

#[cfg(any(test, feature = "bench"))]
fn strict_projections(
    inspection: &JournalInspection,
) -> Result<(ChangeProjection, ChangeDocumentProjectionV1)> {
    let events = inspection
        .event_entries
        .iter()
        .map(|entry| {
            EventStore::decode_qualification_entry(entry.key_digest.clone(), entry.bytes.clone())
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((
        crate::session::project_changes(&events)?,
        crate::session::project_change_documents(&events)?,
    ))
}

fn generation_error(error: impl std::fmt::Display) -> ShoreError {
    ShoreError::Message(format!(
        "Change semantic generation lifecycle failed: {error}"
    ))
}

fn reader_receipt_sha256(receipt: &ChangeReaderProfileReceiptV2) -> Result<String> {
    sha256_json_prefixed(&serde_json::to_value(ReaderReceiptPreimage {
        schema: &receipt.schema,
        version: receipt.version,
        minimum_reader_profile: &receipt.minimum_reader_profile,
        authority_cursor: &receipt.authority_cursor,
        document_versions: &receipt.document_versions,
        fact_availability: &receipt.fact_availability,
        resource_availability: &receipt.resource_availability,
        projection_sha256: &receipt.projection_sha256,
        document_projection_sha256: &receipt.document_projection_sha256,
        semantic_receipt: &receipt.semantic_receipt,
    })?)
}

fn fact_availability(events: &[ShoreEvent]) -> Vec<ChangeFactAvailabilityV1> {
    let mut counts = BTreeMap::<String, u64>::new();
    for event in events {
        *counts
            .entry(event.event_type.as_str().to_owned())
            .or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(family, count)| ChangeFactAvailabilityV1 { family, count })
        .collect()
}

fn resource_availability(events: &[ShoreEvent]) -> Result<Vec<ChangeResourceAvailabilityV1>> {
    Ok(
        crate::session::workflow::selected_support_content_hashes(events)?
            .into_iter()
            .map(|content_hash| ChangeResourceAvailabilityV1 { content_hash })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EngagementId, JournalId, ObjectId, RevisionId};
    use crate::session::event::{
        EventTarget, Revision, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload, Writer,
    };
    use crate::session::store::backend::StoreBackend;
    use crate::session::store::capabilities::{
        AUTHORITY_CURSOR_SCHEMA_V2, CapabilityFixtureState, inspect_journal_records,
        write_capability_fixture_for_test,
    };

    type Mutation<T> = Box<dyn Fn(&mut T)>;

    fn contract_hash(character: char) -> String {
        format!("sha256:{}", character.to_string().repeat(64))
    }

    fn contract_raw_hash(character: char) -> String {
        character.to_string().repeat(64)
    }

    fn contract_identity(profile_id: &str, schema_version: u32) -> ReaderSchemaIdentityV1 {
        ReaderSchemaIdentityV1 {
            profile_id: profile_id.to_owned(),
            schema_version,
        }
    }

    fn contract_applied_checkpoint(
        profile_id: &str,
        schema_version: u32,
        cursor: TruthCursor,
    ) -> AppliedProjectionCheckpointV1 {
        let mut checkpoint = AppliedProjectionCheckpointV1 {
            identity: contract_identity(profile_id, schema_version),
            applied_cursor: cursor,
            receipt_sha256: String::new(),
        };
        checkpoint.receipt_sha256 = applied_projection_checkpoint_sha256_v1(&checkpoint).unwrap();
        checkpoint
    }

    fn contract_locator_checkpoint(cursor: TruthCursor) -> LocatorProjectionCheckpointV1 {
        let mut checkpoint = LocatorProjectionCheckpointV1 {
            identity: contract_identity(LOCATOR_PROFILE_ID_V1, LOCATOR_SCHEMA_VERSION_V1),
            applied_cursor: cursor,
            observed_cursor: cursor,
            receipt_sha256: String::new(),
        };
        checkpoint.receipt_sha256 = locator_projection_checkpoint_sha256_v1(&checkpoint).unwrap();
        checkpoint
    }

    fn contract_authority_cursor(sequence: u64) -> AuthorityCursorV2 {
        AuthorityCursorV2 {
            schema: AUTHORITY_CURSOR_SCHEMA_V2.to_owned(),
            journal_record_count: sequence.saturating_add(2),
            event_count: sequence,
            journal_record_set_hash: contract_hash('1'),
            event_set_hash: contract_hash('2'),
            capability_set_hash: contract_hash('3'),
        }
    }

    fn contract_receipt(sequence: u64) -> ChangeReaderProfileReceiptV3 {
        let cursor = TruthCursor::new(1, sequence);
        let publication_completion_id = format!("bulk-adoption-completion:{}", contract_hash('6'));
        let mut receipt = ChangeReaderProfileReceiptV3 {
            schema: CHANGE_READER_PROFILE_RECEIPT_SCHEMA_V3.to_owned(),
            version: 3,
            minimum_reader_profile: REVIEW_CHANGE_REVISION_COHORT_V1.to_owned(),
            publication_generation_id: "generation:test".to_owned(),
            publication_store_id: "store:test".to_owned(),
            publication_profile: DerivedAccessProfile::SqliteWalBodylessV1,
            publication_activation_id: format!("capability-activation:{}", contract_hash('4')),
            publication_manifest_hash: contract_hash('5'),
            publication_completion_id: publication_completion_id.clone(),
            publication_activation_carrier: CapabilityCarrierBindingV1::new(
                "store_capability_activation:review_change_revision_v1:root",
                contract_hash('7'),
            ),
            publication_completion_carrier: CapabilityCarrierBindingV1::new(
                format!("bulk_adoption_completion:{publication_completion_id}"),
                contract_hash('8'),
            ),
            publication_authority_cursor: contract_authority_cursor(sequence),
            publication_platform_stamp: JournalChangeStamp::Observed {
                identity_sha256: contract_raw_hash('9'),
                change_sha256: contract_raw_hash('a'),
                entry_count: Some(sequence.saturating_add(2)),
                native_cursor: None,
            },
            publication_truth_cursor: cursor,
            publication_cursor_identity: contract_identity(
                CURSOR_PROFILE_ID_V1,
                CURSOR_SCHEMA_VERSION_V1,
            ),
            publication_locator_checkpoint: contract_locator_checkpoint(cursor),
            publication_semantic_checkpoint: contract_applied_checkpoint(
                SEMANTIC_PROFILE_ID_V1,
                SEMANTIC_SCHEMA_VERSION_V1,
                cursor,
            ),
            publication_product_checkpoint: contract_applied_checkpoint(
                PRODUCT_HISTORY_PROFILE_ID_V1,
                PRODUCT_HISTORY_SCHEMA_VERSION_V1,
                cursor,
            ),
            projector_versions: reader_projector_versions_v1(),
            document_versions: reader_document_versions_v1(),
            fact_availability: vec![ChangeFactAvailabilityV1 {
                family: "work_object_proposed".to_owned(),
                count: sequence,
            }],
            resource_availability: vec![ChangeResourceAvailabilityV1 {
                content_hash: contract_hash('b'),
            }],
            projection_sha256: contract_hash('c'),
            document_projection_sha256: contract_hash('d'),
            semantic_receipt: contract_hash('e'),
            reader_document_registry_sha256: reader_document_registry_sha256_v1().unwrap(),
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = change_reader_profile_receipt_sha256_v3(&receipt).unwrap();
        receipt
    }

    fn contract_capability_binding(
        inspection: &JournalInspection,
        logical_key: impl Into<String>,
    ) -> CapabilityCarrierBindingV1 {
        let logical_key = logical_key.into();
        let key_digest = sha256_bytes_hex(logical_key.as_bytes());
        let entry = inspection
            .record_entries
            .iter()
            .find(|entry| entry.key_digest == key_digest)
            .expect("capability fixture contains the bound control record");
        CapabilityCarrierBindingV1::new(
            logical_key,
            format!("sha256:{}", sha256_bytes_hex(&entry.bytes)),
        )
    }

    fn contract_receipt_for_generation(
        inspection: &JournalInspection,
        generation: &ChangeSemanticGenerationV2,
    ) -> ChangeReaderProfileReceiptV3 {
        let StoreCapabilityStatus::Ready {
            activation_id,
            manifest_hash,
            completion_id,
        } = &inspection.status
        else {
            panic!("contract receipt requires completed capability authority");
        };
        let mut receipt = contract_receipt(inspection.cursor.event_count);
        receipt.publication_activation_id = activation_id.clone();
        receipt.publication_manifest_hash = manifest_hash.clone();
        receipt.publication_completion_id = completion_id.clone();
        receipt.publication_activation_carrier = contract_capability_binding(
            inspection,
            "store_capability_activation:review_change_revision_v1:root",
        );
        receipt.publication_completion_carrier = contract_capability_binding(
            inspection,
            format!("bulk_adoption_completion:{completion_id}"),
        );
        receipt.publication_authority_cursor = inspection.cursor.clone();
        receipt.publication_platform_stamp = JournalChangeStamp::Absent;
        receipt.document_versions = generation.reader_profile.document_versions.clone();
        receipt.fact_availability = generation.reader_profile.fact_availability.clone();
        receipt.resource_availability = generation.reader_profile.resource_availability.clone();
        receipt.projection_sha256 = generation.reader_profile.projection_sha256.clone();
        receipt.document_projection_sha256 =
            generation.reader_profile.document_projection_sha256.clone();
        receipt.semantic_receipt = generation.reader_profile.semantic_receipt.clone();
        receipt.receipt_sha256 = change_reader_profile_receipt_sha256_v3(&receipt).unwrap();
        receipt
    }

    fn contract_live_checkpoint(
        receipt: &ChangeReaderProfileReceiptV3,
    ) -> ReaderProjectionCheckpointV1 {
        contract_live_checkpoint_at(receipt, receipt.publication_authority_cursor.clone())
    }

    fn contract_live_checkpoint_at(
        receipt: &ChangeReaderProfileReceiptV3,
        authority_cursor: AuthorityCursorV2,
    ) -> ReaderProjectionCheckpointV1 {
        let cursor = TruthCursor::new(
            receipt.publication_truth_cursor.epoch,
            authority_cursor.event_count,
        );
        let mut checkpoint = ReaderProjectionCheckpointV1 {
            schema: READER_PROJECTION_CHECKPOINT_SCHEMA_V1.to_owned(),
            version: 1,
            store_id: receipt.publication_store_id.clone(),
            reader_receipt_sha256: receipt.receipt_sha256.clone(),
            authority_cursor,
            truth_cursor: cursor,
            cursor_identity: receipt.publication_cursor_identity.clone(),
            locator_checkpoint: contract_locator_checkpoint(cursor),
            semantic_checkpoint: contract_applied_checkpoint(
                SEMANTIC_PROFILE_ID_V1,
                SEMANTIC_SCHEMA_VERSION_V1,
                cursor,
            ),
            product_checkpoint: contract_applied_checkpoint(
                PRODUCT_HISTORY_PROFILE_ID_V1,
                PRODUCT_HISTORY_SCHEMA_VERSION_V1,
                cursor,
            ),
            projector_versions: receipt.projector_versions.clone(),
            checkpoint_sha256: String::new(),
        };
        checkpoint.checkpoint_sha256 = reader_projection_checkpoint_sha256_v1(&checkpoint).unwrap();
        checkpoint
    }

    fn assert_receipt_field_is_hash_bound(
        receipt: &ChangeReaderProfileReceiptV3,
        mutate: impl FnOnce(&mut ChangeReaderProfileReceiptV3),
    ) {
        let expected = change_reader_profile_receipt_sha256_v3(receipt).unwrap();
        let mut mutated = receipt.clone();
        mutate(&mut mutated);
        assert_ne!(
            change_reader_profile_receipt_sha256_v3(&mutated).unwrap(),
            expected
        );
        assert!(mutated.validate().is_err());
    }

    #[test]
    fn change_reader_contract_accepts_exact_zero_and_nonzero_event_checkpoints() {
        for sequence in [0, 7] {
            let receipt = contract_receipt(sequence);
            receipt.validate().unwrap();
            let checkpoint = contract_live_checkpoint(&receipt);
            checkpoint.validate_for_receipt(&receipt).unwrap();

            assert_eq!(receipt.publication_authority_cursor.event_count, sequence);
            assert_eq!(receipt.publication_truth_cursor.sequence, sequence);
            assert_eq!(
                receipt
                    .publication_locator_checkpoint
                    .applied_cursor
                    .sequence,
                sequence
            );
            assert_eq!(
                receipt
                    .publication_semantic_checkpoint
                    .applied_cursor
                    .sequence,
                sequence
            );
            assert_eq!(
                receipt
                    .publication_product_checkpoint
                    .applied_cursor
                    .sequence,
                sequence
            );
            assert_eq!(checkpoint.truth_cursor.sequence, sequence);
            assert_ne!(
                receipt.publication_authority_cursor.journal_record_count,
                sequence
            );
        }
    }

    #[test]
    fn completed_capability_fixtures_preserve_their_exact_event_sequences() {
        for fixture in [CapabilityFixtureState::EmptyL2, CapabilityFixtureState::L2] {
            let backend = StoreBackend::memory();
            write_capability_fixture_for_test(backend.journal().as_ref(), fixture).unwrap();
            let inspection = inspect_journal_records(backend.journal().as_ref()).unwrap();
            let generation = build_change_semantic_generation(&inspection).unwrap();
            generation.validate().unwrap();

            let receipt = contract_receipt_for_generation(&inspection, &generation);
            receipt.validate().unwrap();
            contract_live_checkpoint(&receipt)
                .validate_for_receipt(&receipt)
                .unwrap();

            assert_eq!(
                receipt.publication_truth_cursor.sequence,
                inspection.cursor.event_count
            );
            assert!(inspection.cursor.journal_record_count >= inspection.cursor.event_count);
        }
    }

    #[test]
    fn v3_receipt_requires_strict_and_compact_change_document_parity() {
        let backend = StoreBackend::memory();
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();
        let inspection = inspect_journal_records(backend.journal().as_ref()).unwrap();
        let events = inspection
            .event_entries
            .iter()
            .map(|entry| {
                EventStore::decode_qualification_entry(
                    entry.key_digest.clone(),
                    entry.bytes.clone(),
                )
            })
            .collect::<Result<Vec<_>>>()
            .unwrap();
        let truth_cursor = TruthCursor::new(1, inspection.cursor.event_count);
        let semantic_facts = events
            .iter()
            .enumerate()
            .map(|(index, event)| {
                SemanticFact::from_event(
                    TruthCursor::new(1, u64::try_from(index + 1).unwrap()),
                    event,
                    contract_raw_hash('f'),
                )
            })
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let compact_snapshot = SemanticSnapshot::audit_from_facts(truth_cursor, &semantic_facts)
            .expect("compact semantic projection");
        let compact_facts = compact_change_document_facts(&semantic_facts);
        let compact_documents = project_change_documents_from_facts(&compact_facts)
            .expect("compact Change document projection");

        let StoreCapabilityStatus::Ready { completion_id, .. } = &inspection.status else {
            panic!("fixture must provide completed Change authority");
        };
        let activation = contract_capability_binding(
            &inspection,
            "store_capability_activation:review_change_revision_v1:root",
        );
        let completion = contract_capability_binding(
            &inspection,
            format!("bulk_adoption_completion:{completion_id}"),
        );
        let build = |snapshot: &SemanticSnapshot, documents: &ChangeDocumentProjectionV1| {
            build_change_reader_profile_receipt_v3(
                "generation:compact-parity",
                "store:compact-parity",
                &inspection,
                activation.record_sha256.clone(),
                completion.record_sha256.clone(),
                JournalChangeStamp::Absent,
                truth_cursor,
                &events,
                snapshot,
                documents,
            )
        };

        build(&compact_snapshot, &compact_documents)
            .expect("matching compact projection may publish V3");

        let mut changed_semantic = compact_snapshot.clone();
        changed_semantic.changes = ChangeProjection::default();
        assert!(
            build(&changed_semantic, &compact_documents).is_err(),
            "V3 publication must reject changed compact semantic state"
        );

        let mut changed = compact_documents;
        changed
            .diagnostics
            .push("changed_compact_provenance".to_owned());
        changed.projection_stamp =
            crate::session::change_document_projection_stamp(&compact_snapshot.changes, &changed)
                .unwrap();
        assert!(
            build(&compact_snapshot, &changed).is_err(),
            "V3 publication must reject changed compact document provenance"
        );
    }

    #[test]
    fn change_reader_contract_receipt_is_bound_to_its_generation_descriptor() {
        let receipt = contract_receipt(7);
        let descriptor = GenerationDescriptor::new(
            receipt.publication_generation_id.clone(),
            receipt.publication_store_id.clone(),
            receipt.publication_profile,
            receipt.publication_truth_cursor.epoch,
            receipt.publication_truth_cursor.sequence,
            receipt.publication_platform_stamp.clone(),
            receipt.receipt_sha256.clone(),
        );
        receipt.validate_for_descriptor(&descriptor).unwrap();

        let mutations: Vec<Mutation<GenerationDescriptor>> = vec![
            Box::new(|value| value.generation_id.push('x')),
            Box::new(|value| value.store_id.push('x')),
            Box::new(|value| value.profile = DerivedAccessProfile::Off),
            Box::new(|value| value.epoch += 1),
            Box::new(|value| value.head_sequence += 1),
            Box::new(|value| value.authority_stamp = JournalChangeStamp::Absent),
            Box::new(|value| value.semantic_receipt.push('x')),
        ];
        for mutate in mutations {
            let mut mutated = descriptor.clone();
            mutate(&mut mutated);
            assert!(receipt.validate_for_descriptor(&mutated).is_err());
        }
    }

    #[test]
    fn change_reader_contract_accepts_the_production_platform_stamp_shape() {
        let mut receipt = contract_receipt(7);
        receipt.publication_platform_stamp = JournalChangeStamp::Observed {
            identity_sha256: sha256_bytes_hex(b"platform identity"),
            change_sha256: sha256_bytes_hex(b"platform change"),
            entry_count: Some(9),
            native_cursor: None,
        };
        receipt.receipt_sha256 = change_reader_profile_receipt_sha256_v3(&receipt).unwrap();
        receipt.validate().unwrap();

        let JournalChangeStamp::Observed {
            identity_sha256,
            change_sha256,
            ..
        } = &receipt.publication_platform_stamp
        else {
            panic!("test stamp must be observed");
        };
        assert!(is_lower_sha256(identity_sha256));
        assert!(is_lower_sha256(change_sha256));
        assert!(!identity_sha256.starts_with("sha256:"));
        assert!(!change_sha256.starts_with("sha256:"));

        let mut prefixed = receipt;
        if let JournalChangeStamp::Observed {
            identity_sha256, ..
        } = &mut prefixed.publication_platform_stamp
        {
            *identity_sha256 = format!("sha256:{identity_sha256}");
        }
        prefixed.receipt_sha256 = change_reader_profile_receipt_sha256_v3(&prefixed).unwrap();
        assert!(prefixed.validate().is_err());
    }

    #[test]
    fn change_reader_contract_receipt_hash_binds_every_publication_field() {
        let receipt = contract_receipt(7);
        receipt.validate().unwrap();

        assert_receipt_field_is_hash_bound(&receipt, |value| value.schema.push('x'));
        assert_receipt_field_is_hash_bound(&receipt, |value| value.version += 1);
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.minimum_reader_profile.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_generation_id.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| value.publication_store_id.push('x'));
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_profile = DerivedAccessProfile::Off
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_activation_id.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_manifest_hash = contract_hash('f')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_completion_id.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_activation_carrier.logical_key.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_activation_carrier.key_digest.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_activation_carrier.record_sha256 = contract_hash('f')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_completion_carrier.logical_key.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_completion_carrier.key_digest.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_completion_carrier.record_sha256 = contract_hash('f')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_authority_cursor.schema.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_authority_cursor.journal_record_count += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_authority_cursor.event_count += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_authority_cursor.journal_record_set_hash = contract_hash('f')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_authority_cursor.event_set_hash = contract_hash('f')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_authority_cursor.capability_set_hash = contract_hash('f')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_platform_stamp = JournalChangeStamp::Absent
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            if let JournalChangeStamp::Observed {
                identity_sha256, ..
            } = &mut value.publication_platform_stamp
            {
                *identity_sha256 = contract_raw_hash('f');
            }
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            if let JournalChangeStamp::Observed { change_sha256, .. } =
                &mut value.publication_platform_stamp
            {
                *change_sha256 = contract_raw_hash('f');
            }
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            if let JournalChangeStamp::Observed { entry_count, .. } =
                &mut value.publication_platform_stamp
            {
                *entry_count = Some(99);
            }
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            let mut stamp = serde_json::to_value(&value.publication_platform_stamp).unwrap();
            stamp["native_cursor"] = serde_json::json!({
                "journalId": 1,
                "nextUsn": 2,
                "directoryFileReference": 3,
                "volumeSerialNumber": 4,
            });
            value.publication_platform_stamp = serde_json::from_value(stamp).unwrap();
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_truth_cursor.epoch += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_truth_cursor.sequence += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_cursor_identity.profile_id.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_cursor_identity.schema_version += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value
                .publication_locator_checkpoint
                .identity
                .profile_id
                .push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_locator_checkpoint.identity.schema_version += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_locator_checkpoint.applied_cursor.epoch += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_locator_checkpoint.applied_cursor.sequence += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_locator_checkpoint.observed_cursor.epoch += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value
                .publication_locator_checkpoint
                .observed_cursor
                .sequence += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_locator_checkpoint.receipt_sha256 = contract_hash('f')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value
                .publication_semantic_checkpoint
                .identity
                .profile_id
                .push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value
                .publication_semantic_checkpoint
                .identity
                .schema_version += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_semantic_checkpoint.applied_cursor.epoch += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value
                .publication_semantic_checkpoint
                .applied_cursor
                .sequence += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_semantic_checkpoint.receipt_sha256 = contract_hash('f')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value
                .publication_product_checkpoint
                .identity
                .profile_id
                .push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_product_checkpoint.identity.schema_version += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_product_checkpoint.applied_cursor.epoch += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_product_checkpoint.applied_cursor.sequence += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.publication_product_checkpoint.receipt_sha256 = contract_hash('f')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.projector_versions[0].projector.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.projector_versions[0].version += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.projector_versions[1].projector.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.projector_versions[1].version += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.document_versions[0].schema.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.document_versions[0].version += 1
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| value.fact_availability[0].count += 1);
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.fact_availability[0].family.push('x')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.resource_availability[0].content_hash = contract_hash('f')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.projection_sha256 = contract_hash('f')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.document_projection_sha256 = contract_hash('f')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.semantic_receipt = contract_hash('f')
        });
        assert_receipt_field_is_hash_bound(&receipt, |value| {
            value.reader_document_registry_sha256 = contract_hash('f')
        });

        let mut self_hash_changed = receipt;
        self_hash_changed.receipt_sha256 = contract_hash('f');
        assert!(self_hash_changed.validate().is_err());
    }

    #[test]
    fn change_reader_contract_live_checkpoint_rejects_every_bound_field_mutation() {
        let receipt = contract_receipt(7);
        let checkpoint = contract_live_checkpoint(&receipt);
        checkpoint.validate_for_receipt(&receipt).unwrap();

        let mutations: Vec<Mutation<ReaderProjectionCheckpointV1>> = vec![
            Box::new(|value| value.schema.push('x')),
            Box::new(|value| value.version += 1),
            Box::new(|value| value.store_id.push('x')),
            Box::new(|value| value.reader_receipt_sha256.push('x')),
            Box::new(|value| value.authority_cursor.schema.push('x')),
            Box::new(|value| value.authority_cursor.journal_record_count += 1),
            Box::new(|value| value.authority_cursor.event_count += 1),
            Box::new(|value| value.authority_cursor.journal_record_set_hash.push('x')),
            Box::new(|value| value.authority_cursor.event_set_hash.push('x')),
            Box::new(|value| value.authority_cursor.capability_set_hash.push('x')),
            Box::new(|value| value.truth_cursor.epoch += 1),
            Box::new(|value| value.truth_cursor.sequence += 1),
            Box::new(|value| value.cursor_identity.profile_id.push('x')),
            Box::new(|value| value.cursor_identity.schema_version += 1),
            Box::new(|value| value.locator_checkpoint.identity.profile_id.push('x')),
            Box::new(|value| value.locator_checkpoint.identity.schema_version += 1),
            Box::new(|value| value.locator_checkpoint.applied_cursor.epoch += 1),
            Box::new(|value| value.locator_checkpoint.applied_cursor.sequence += 1),
            Box::new(|value| value.locator_checkpoint.observed_cursor.epoch += 1),
            Box::new(|value| value.locator_checkpoint.observed_cursor.sequence += 1),
            Box::new(|value| value.locator_checkpoint.receipt_sha256.push('x')),
            Box::new(|value| value.semantic_checkpoint.identity.profile_id.push('x')),
            Box::new(|value| value.semantic_checkpoint.identity.schema_version += 1),
            Box::new(|value| value.semantic_checkpoint.applied_cursor.epoch += 1),
            Box::new(|value| value.semantic_checkpoint.applied_cursor.sequence += 1),
            Box::new(|value| value.semantic_checkpoint.receipt_sha256.push('x')),
            Box::new(|value| value.product_checkpoint.identity.profile_id.push('x')),
            Box::new(|value| value.product_checkpoint.identity.schema_version += 1),
            Box::new(|value| value.product_checkpoint.applied_cursor.epoch += 1),
            Box::new(|value| value.product_checkpoint.applied_cursor.sequence += 1),
            Box::new(|value| value.product_checkpoint.receipt_sha256.push('x')),
            Box::new(|value| value.projector_versions[0].projector.push('x')),
            Box::new(|value| value.projector_versions[0].version += 1),
            Box::new(|value| value.projector_versions[1].projector.push('x')),
            Box::new(|value| value.projector_versions[1].version += 1),
            Box::new(|value| value.checkpoint_sha256.push('x')),
        ];
        for mutate in mutations {
            let mut mutated = checkpoint.clone();
            mutate(&mut mutated);
            assert!(mutated.validate_for_receipt(&receipt).is_err());
        }
    }

    #[test]
    fn change_reader_contract_classifies_legacy_unknown_and_wrong_anchor_as_rebuild() {
        for value in [
            serde_json::json!({
                "schema": CHANGE_READER_PROFILE_RECEIPT_SCHEMA_V2,
                "version": 2,
            }),
            serde_json::json!({
                "schema": "pointbreak.derived-change-reader-profile-receipt.v4",
                "version": 4,
            }),
            serde_json::json!({
                "schema": CHANGE_READER_PROFILE_RECEIPT_SCHEMA_V3,
                "version": 4,
            }),
        ] {
            assert!(matches!(
                probe_change_reader_profile_receipt(&serde_json::to_vec(&value).unwrap()).unwrap(),
                ChangeReaderProfileReceiptProbeV1::RebuildRequired { .. }
            ));
        }

        let receipt = contract_receipt(7);
        assert!(matches!(
            probe_change_reader_profile_receipt(&serde_json::to_vec(&receipt).unwrap()).unwrap(),
            ChangeReaderProfileReceiptProbeV1::Current(_)
        ));
        let mut checkpoint = contract_live_checkpoint(&receipt);
        checkpoint.reader_receipt_sha256 = contract_hash('f');
        let error = checkpoint.validate_for_receipt(&receipt).unwrap_err();
        assert!(error.is_rebuild_required());
    }

    #[test]
    fn change_reader_contract_generation_stamp_uses_one_complete_preimage() {
        let backend = StoreBackend::memory();
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();
        let inspection = inspect_journal_records(backend.journal().as_ref()).unwrap();
        let generation = build_change_semantic_generation(&inspection).unwrap();
        let receipt = contract_receipt_for_generation(&inspection, &generation);
        let ChangeReaderProfileReceiptProbeV1::Current(receipt) =
            probe_change_reader_profile_receipt(&serde_json::to_vec(&receipt).unwrap()).unwrap()
        else {
            panic!("serialized V3 receipt must probe as current");
        };
        let receipt = *receipt;
        let checkpoint = contract_live_checkpoint(&receipt);
        checkpoint.validate_for_receipt(&receipt).unwrap();
        let trust = RuntimeTrustIdentityV1::Bound {
            trust_set_sha256: contract_hash('3'),
        };
        let strict_preimage = strict_change_generation_stamp_preimage_v1(
            &receipt,
            &checkpoint,
            &inspection.cursor,
            &generation.projection,
            &generation.document_projection,
            trust.clone(),
        )
        .unwrap();
        let derived_preimage = derived_change_generation_stamp_preimage_v1(
            &receipt,
            &checkpoint,
            contract_sha256(&generation.projection).unwrap(),
            contract_sha256(&generation.document_projection).unwrap(),
            generation.document_projection.projection_stamp.clone(),
            trust.clone(),
        )
        .unwrap();

        let strict = change_generation_stamp_sha256_v1(&strict_preimage).unwrap();
        let derived = change_generation_stamp_sha256_v1(&derived_preimage).unwrap();
        assert_eq!(strict_preimage, derived_preimage);
        assert_eq!(strict, derived);

        let mut changed_strict_document = generation.document_projection.clone();
        changed_strict_document
            .diagnostics
            .push("strict-input-change".to_owned());
        changed_strict_document.projection_stamp =
            crate::session::change_document_projection_stamp(
                &generation.projection,
                &changed_strict_document,
            )
            .unwrap();
        let changed_strict = strict_change_generation_stamp_preimage_v1(
            &receipt,
            &checkpoint,
            &inspection.cursor,
            &generation.projection,
            &changed_strict_document,
            trust.clone(),
        )
        .unwrap();
        assert_ne!(
            change_generation_stamp_sha256_v1(&changed_strict).unwrap(),
            strict
        );

        let appended_revision_id = RevisionId::new(format!("rev:sha256:{}", "14".repeat(32)));
        let appended = ShoreEvent::new(
            crate::session::event::EventType::WorkObjectProposed,
            format!("work_object_proposed:{}", appended_revision_id.as_str()),
            EventTarget::for_revision(
                JournalId::new("journal:contract-stamp"),
                appended_revision_id.clone(),
                None,
            )
            .unwrap(),
            Writer::shore_local("test"),
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new(format!("engagement:sha256:{}", "15".repeat(32))),
                work_object: WorkObjectProposal::Revision {
                    revision: Revision {
                        id: appended_revision_id,
                        object_id: ObjectId::new(format!("obj:sha256:{}", "16".repeat(32))),
                        git_provenance: None,
                    },
                    summary: None,
                    object_artifact_content_hash: contract_hash('7'),
                    supersedes: Vec::new(),
                },
            },
            "2026-08-09T00:00:00Z",
        )
        .unwrap();
        assert_eq!(
            EventStore::from_backend(&backend)
                .record_change_event_once(&appended)
                .unwrap(),
            crate::session::store::EventWriteOutcome::Created
        );
        let advanced_inspection = inspect_journal_records(backend.journal().as_ref()).unwrap();
        let (advanced_projection, advanced_document_projection) =
            strict_projections(&advanced_inspection).unwrap();
        let advanced_checkpoint =
            contract_live_checkpoint_at(&receipt, advanced_inspection.cursor.clone());
        advanced_checkpoint.validate_for_receipt(&receipt).unwrap();
        assert_eq!(
            advanced_checkpoint.reader_receipt_sha256,
            checkpoint.reader_receipt_sha256
        );
        assert!(advanced_checkpoint.truth_cursor.sequence > checkpoint.truth_cursor.sequence);

        let advanced_strict = strict_change_generation_stamp_preimage_v1(
            &receipt,
            &advanced_checkpoint,
            &advanced_inspection.cursor,
            &advanced_projection,
            &advanced_document_projection,
            trust.clone(),
        )
        .unwrap();
        let advanced_derived = derived_change_generation_stamp_preimage_v1(
            &receipt,
            &advanced_checkpoint,
            contract_sha256(&advanced_projection).unwrap(),
            contract_sha256(&advanced_document_projection).unwrap(),
            advanced_document_projection.projection_stamp.clone(),
            trust,
        )
        .unwrap();
        assert_eq!(advanced_strict, advanced_derived);
        let advanced_stamp = change_generation_stamp_sha256_v1(&advanced_derived).unwrap();
        assert_eq!(
            change_generation_stamp_sha256_v1(&advanced_strict).unwrap(),
            advanced_stamp
        );
        assert_ne!(advanced_stamp, derived);

        let mut event_set_mismatch = strict_preimage.clone();
        event_set_mismatch.presentation_event_set_hash = contract_hash('4');
        assert!(change_generation_stamp_sha256_v1(&event_set_mismatch).is_err());

        for mutate in [
            |value: &mut ChangeGenerationStampPreimageV1| {
                value.reader_receipt_sha256 = contract_hash('4');
                value.reader_checkpoint.reader_receipt_sha256 = contract_hash('4');
                value.reader_checkpoint.checkpoint_sha256 =
                    reader_projection_checkpoint_sha256_v1(&value.reader_checkpoint).unwrap();
            },
            |value: &mut ChangeGenerationStampPreimageV1| {
                value.semantic_projection_sha256 = contract_hash('4')
            },
            |value: &mut ChangeGenerationStampPreimageV1| {
                value.document_projection_sha256 = contract_hash('4')
            },
            |value: &mut ChangeGenerationStampPreimageV1| {
                value.presentation_identity_sha256 = contract_hash('4')
            },
            |value: &mut ChangeGenerationStampPreimageV1| {
                value.presentation_event_set_hash = contract_hash('4');
                value.reader_checkpoint.authority_cursor.event_set_hash = contract_hash('4');
                value.reader_checkpoint.checkpoint_sha256 =
                    reader_projection_checkpoint_sha256_v1(&value.reader_checkpoint).unwrap();
            },
            |value: &mut ChangeGenerationStampPreimageV1| {
                value.internal_schema_sha256 = contract_hash('4')
            },
            |value: &mut ChangeGenerationStampPreimageV1| {
                value.runtime_trust_identity = RuntimeTrustIdentityV1::NotApplicable
            },
        ] {
            let mut mutated = strict_preimage.clone();
            mutate(&mut mutated);
            assert_ne!(change_generation_stamp_sha256_v1(&mutated).unwrap(), strict);
        }
    }

    #[test]
    fn only_completed_capability_state_can_build_change_semantics() {
        let l0_backend = StoreBackend::memory();
        let l0 = inspect_journal_records(l0_backend.journal().as_ref()).unwrap();
        assert!(
            build_change_semantic_generation(&l0)
                .unwrap_err()
                .to_string()
                .contains("migration_required")
        );

        let m1_backend = StoreBackend::memory();
        write_capability_fixture_for_test(
            m1_backend.journal().as_ref(),
            CapabilityFixtureState::M1,
        )
        .unwrap();
        let m1 = inspect_journal_records(m1_backend.journal().as_ref()).unwrap();
        assert_eq!(m1.cursor.event_count, 0);
        assert!(
            build_change_semantic_generation(&m1)
                .unwrap_err()
                .to_string()
                .contains("migration_in_progress")
        );

        let l2_backend = StoreBackend::memory();
        write_capability_fixture_for_test(
            l2_backend.journal().as_ref(),
            CapabilityFixtureState::L2,
        )
        .unwrap();
        let l2 = inspect_journal_records(l2_backend.journal().as_ref()).unwrap();
        let generation = build_change_semantic_generation(&l2).unwrap();
        generation.validate().unwrap();
        assert_eq!(
            generation.reader_profile.document_versions,
            reader_document_versions_v1()
        );
        assert!(!generation.reader_profile.receipt_sha256.is_empty());
        assert!(
            !serde_json::to_string(&generation)
                .unwrap()
                .contains("legacy_derived")
        );
    }

    #[test]
    fn old_or_corrupt_change_generation_never_validates() {
        let (_backend, l2) = l2_inspection();
        let generation = build_change_semantic_generation(&l2).unwrap();
        generation.validate().unwrap();

        let mut old = generation.clone();
        old.schema = "pointbreak.derived-access-generation.v2".to_owned();
        assert!(
            old.validate()
                .unwrap_err()
                .to_string()
                .contains("incompatible Change semantic generation schema")
        );

        let mut unsupported = generation;
        unsupported.version = 3;
        assert!(unsupported.validate().is_err());
        assert!(serde_json::from_slice::<ChangeSemanticGenerationV2>(b"not-json").is_err());
    }

    fn l2_inspection() -> (StoreBackend, JournalInspection) {
        let backend = StoreBackend::memory();
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();
        let inspection = inspect_journal_records(backend.journal().as_ref()).unwrap();
        (backend, inspection)
    }

    #[test]
    fn immutable_change_generation_publishes_only_at_one_l2_authority() {
        let root = tempfile::tempdir().unwrap();
        let (_backend, l2) = l2_inspection();
        let generation_id = publish_change_semantic_generation_with_failure(
            root.path(),
            &l2,
            &l2.cursor,
            true,
            ChangeGenerationFailurePointV1::None,
        )
        .unwrap();
        let read = read_change_semantics_for_qualification(root.path(), &l2, true).unwrap();
        assert_eq!(read.route, ChangeSemanticRouteV1::Current);
        let (strict, strict_documents) = strict_projections(&l2).unwrap();
        assert_eq!(read.projection, strict);
        assert_eq!(read.document_projection, strict_documents);
        assert!(
            GenerationLayout::new(root.path())
                .unwrap()
                .generation(&generation_id)
                .join(CHANGE_SEMANTIC_RESOURCE)
                .is_file()
        );

        let mut moved = l2.cursor.clone();
        moved.journal_record_count += 1;
        moved.journal_record_set_hash = format!("sha256:{}", "f".repeat(64));
        assert!(
            publish_change_semantic_generation_with_failure(
                tempfile::tempdir().unwrap().path(),
                &l2,
                &moved,
                true,
                ChangeGenerationFailurePointV1::None,
            )
            .unwrap_err()
            .to_string()
            .contains("authority moved")
        );
    }

    #[test]
    fn interrupted_staging_and_promotion_retry_to_one_current_generation() {
        let (_backend, l2) = l2_inspection();
        for failure in [
            ChangeGenerationFailurePointV1::AfterStaging,
            ChangeGenerationFailurePointV1::AfterPromotion,
        ] {
            let root = tempfile::tempdir().unwrap();
            assert!(
                publish_change_semantic_generation_with_failure(
                    root.path(),
                    &l2,
                    &l2.cursor,
                    true,
                    failure,
                )
                .is_err()
            );
            assert!(
                GenerationLayout::new(root.path())
                    .unwrap()
                    .current_publication()
                    .unwrap()
                    .is_none()
            );
            publish_change_semantic_generation_with_failure(
                root.path(),
                &l2,
                &l2.cursor,
                true,
                ChangeGenerationFailurePointV1::None,
            )
            .unwrap();
            assert_eq!(
                read_change_semantics_for_qualification(root.path(), &l2, true)
                    .unwrap()
                    .route,
                ChangeSemanticRouteV1::Current
            );
        }
    }

    #[test]
    fn off_missing_corrupt_and_pre_l2_routes_never_serve_derived_authority() {
        let root = tempfile::tempdir().unwrap();
        let (_backend, l2) = l2_inspection();
        let (strict, strict_documents) = strict_projections(&l2).unwrap();
        let off = read_change_semantics_for_qualification(root.path(), &l2, false).unwrap();
        assert_eq!(off.route, ChangeSemanticRouteV1::ExplicitOff);
        assert_eq!(off.projection, strict);
        assert_eq!(off.document_projection, strict_documents);
        assert!(!root.path().join("derived").exists());

        let missing = read_change_semantics_for_qualification(root.path(), &l2, true).unwrap();
        assert_eq!(missing.route, ChangeSemanticRouteV1::LooseFallback);
        publish_change_semantic_generation_with_failure(
            root.path(),
            &l2,
            &l2.cursor,
            true,
            ChangeGenerationFailurePointV1::None,
        )
        .unwrap();
        let layout = GenerationLayout::new(root.path()).unwrap();
        let publication = layout.current_publication().unwrap().unwrap();
        std::fs::write(
            layout
                .generation(&publication.generation_id)
                .join(CHANGE_SEMANTIC_RESOURCE),
            b"corrupt",
        )
        .unwrap();
        let corrupt = read_change_semantics_for_qualification(root.path(), &l2, true).unwrap();
        assert_eq!(corrupt.route, ChangeSemanticRouteV1::LooseFallback);
        assert_eq!(corrupt.projection, strict);
        assert_eq!(corrupt.document_projection, strict_documents);

        let l0_backend = StoreBackend::memory();
        let l0 = inspect_journal_records(l0_backend.journal().as_ref()).unwrap();
        assert!(
            read_change_semantics_for_qualification(root.path(), &l0, true)
                .unwrap_err()
                .to_string()
                .contains("migration_required")
        );
        let m1_backend = StoreBackend::memory();
        write_capability_fixture_for_test(
            m1_backend.journal().as_ref(),
            CapabilityFixtureState::M1,
        )
        .unwrap();
        let m1 = inspect_journal_records(m1_backend.journal().as_ref()).unwrap();
        assert!(
            read_change_semantics_for_qualification(root.path(), &m1, true)
                .unwrap_err()
                .to_string()
                .contains("migration_in_progress")
        );
    }

    #[test]
    fn legacy_revision_refs_do_not_abort_off_or_loose_fallback_routes() {
        let backend = StoreBackend::memory();
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();
        let payload = WorkObjectProposedPayload {
            engagement_id: EngagementId::new("engagement:sha256:legacy"),
            work_object: WorkObjectProposal::Revision {
                revision: Revision {
                    id: RevisionId::new("review-unit:sha256:legacy"),
                    object_id: ObjectId::new("obj:sha256:legacy"),
                    git_provenance: None,
                },
                summary: None,
                object_artifact_content_hash: "legacy-artifact-hash".to_owned(),
                supersedes: Vec::new(),
            },
        };
        let event = ShoreEvent::new(
            crate::session::event::EventType::WorkObjectProposed,
            "revision:legacy",
            EventTarget::for_journal(JournalId::new("journal:test")),
            Writer::shore_local("test"),
            payload,
            "2026-08-05T00:00:00Z",
        )
        .unwrap();
        backend
            .journal()
            .insert_raw(&event.idempotency_key, &serde_json::to_vec(&event).unwrap())
            .unwrap();
        let inspection = inspect_journal_records(backend.journal().as_ref()).unwrap();
        let root = tempfile::tempdir().unwrap();

        let off = read_change_semantics_for_qualification(root.path(), &inspection, false).unwrap();
        assert_eq!(off.route, ChangeSemanticRouteV1::ExplicitOff);
        assert_eq!(off.document_projection.unavailable_revision_refs.len(), 1);
        let fallback =
            read_change_semantics_for_qualification(root.path(), &inspection, true).unwrap();
        assert_eq!(fallback.route, ChangeSemanticRouteV1::LooseFallback);
        assert_eq!(
            fallback.document_projection.unavailable_revision_refs.len(),
            1
        );
    }
}
