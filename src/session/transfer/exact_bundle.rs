#![cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "exact-transfer source assembly and interruption hooks remain evidence-only"
    )
)]

#[cfg(any(test, feature = "bench"))]
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use super::import_receipt::ImportReceiptV1;
use crate::canonical_hash::{sha256_bytes_hex, sha256_json_prefixed};
use crate::error::ShoreError;
#[cfg(any(test, feature = "bench"))]
use crate::model::RevisionId;
use crate::model::{ChangeId, RevisionRefV1};
#[cfg(any(test, feature = "bench"))]
use crate::session::event::{
    ChangeMembershipAssertedPayload, ChangeMembershipWithdrawnPayload,
    ChangeRevisionRelationAssertedPayload, ChangeRevisionRelationWithdrawnPayload, EventType,
    ShoreEvent,
};
use crate::session::store::authority_lock::StoreAuthorityLock;
use crate::session::store::resolution::resolve_change_read_store;
use crate::session::{AuthorityCursorV2, StoreCapabilityStatus};
use crate::storage::{CreateOutcome, Durability, LocalStorage};

pub const EXACT_BUNDLE_SCHEMA_V2: &str = "pointbreak.exact-bundle.v2";

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(
    tag = "scope",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ExactBundleSelectionV2 {
    ExactRevision {
        change_id: ChangeId,
        revision: RevisionRefV1,
    },
    CompleteChange {
        change_id: ChangeId,
    },
}

impl ExactBundleSelectionV2 {
    fn change_id(&self) -> &ChangeId {
        match self {
            Self::ExactRevision { change_id, .. } | Self::CompleteChange { change_id } => change_id,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExactBundleRecordKindV2 {
    CapabilityActivation,
    BulkAdoptionCompletion,
    Event,
    BodyArtifact,
    ObjectArtifact,
    RelationProof,
    AuxiliaryDocument,
}

impl ExactBundleRecordKindV2 {
    fn is_content(self) -> bool {
        matches!(
            self,
            Self::BodyArtifact
                | Self::ObjectArtifact
                | Self::RelationProof
                | Self::AuxiliaryDocument
        )
    }

    fn is_control(self) -> bool {
        matches!(
            self,
            Self::CapabilityActivation | Self::BulkAdoptionCompletion
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExactBundleRecordV2 {
    pub logical_key: String,
    pub record_kind: ExactBundleRecordKindV2,
    pub decoded_bytes: Vec<u8>,
    pub decoded_sha256: String,
}

impl ExactBundleRecordV2 {
    pub fn new(
        logical_key: impl Into<String>,
        record_kind: ExactBundleRecordKindV2,
        decoded_bytes: Vec<u8>,
    ) -> Result<Self, ExactTransferError> {
        let record = Self {
            logical_key: logical_key.into(),
            record_kind,
            decoded_sha256: format!("sha256:{}", sha256_bytes_hex(&decoded_bytes)),
            decoded_bytes,
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> Result<(), ExactTransferError> {
        if self.logical_key.trim().is_empty()
            || self.decoded_sha256 != format!("sha256:{}", sha256_bytes_hex(&self.decoded_bytes))
        {
            return Err(ExactTransferError::Contract(
                "exact record has an invalid logical key or decoded hash".to_owned(),
            ));
        }
        match self.record_kind {
            ExactBundleRecordKindV2::Event => {
                let event = crate::session::EventStore::decode_qualification_entry(
                    sha256_bytes_hex(self.logical_key.as_bytes()),
                    self.decoded_bytes.clone(),
                )?;
                if event.idempotency_key != self.logical_key {
                    return Err(ExactTransferError::Contract(format!(
                        "event {} does not match its logical key",
                        self.logical_key
                    )));
                }
            }
            ExactBundleRecordKindV2::CapabilityActivation => {
                let control = crate::session::store::capabilities::transfer_control_record(
                    &self.decoded_bytes,
                )?;
                if control.kind
                    != crate::session::store::capabilities::TransferControlRecordKind::CapabilityActivation
                    || control.logical_key != self.logical_key
                {
                    return Err(ExactTransferError::Contract(
                        "activation record logical identity mismatch".to_owned(),
                    ));
                }
            }
            ExactBundleRecordKindV2::BulkAdoptionCompletion => {
                let control = crate::session::store::capabilities::transfer_control_record(
                    &self.decoded_bytes,
                )?;
                if control.kind
                    != crate::session::store::capabilities::TransferControlRecordKind::BulkAdoptionCompletion
                    || control.logical_key != self.logical_key
                {
                    return Err(ExactTransferError::Contract(
                        "completion record logical identity mismatch".to_owned(),
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExactBundleCapabilityV2 {
    pub minimum_reader_profile: String,
    pub required: Vec<String>,
}

impl ExactBundleCapabilityV2 {
    fn validate(&self) -> Result<(), ExactTransferError> {
        if self.minimum_reader_profile.trim().is_empty()
            || self.required.is_empty()
            || self.required.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(ExactTransferError::Contract(
                "exact bundle capability set is not canonical".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExactBundleClosureV2 {
    pub event_logical_key: String,
    pub required_content_keys: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExactBundleManifestV2 {
    pub schema: String,
    pub selection: ExactBundleSelectionV2,
    pub source_authority_cursor: AuthorityCursorV2,
    pub source_manifest_sha256: String,
    pub required_capabilities: ExactBundleCapabilityV2,
    pub events: Vec<ExactBundleRecordV2>,
    pub content: Vec<ExactBundleRecordV2>,
    pub closure: Vec<ExactBundleClosureV2>,
    pub source_change_projection_sha256: String,
    pub event_set_sha256: String,
    pub bundle_sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EventSetPreimage<'a> {
    schema: &'static str,
    events: &'a [ExactBundleRecordV2],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BundlePreimage<'a> {
    schema: &'a str,
    selection: &'a ExactBundleSelectionV2,
    source_authority_cursor: &'a AuthorityCursorV2,
    source_manifest_sha256: &'a str,
    required_capabilities: &'a ExactBundleCapabilityV2,
    events: &'a [ExactBundleRecordV2],
    content: &'a [ExactBundleRecordV2],
    closure: &'a [ExactBundleClosureV2],
    source_change_projection_sha256: &'a str,
    event_set_sha256: &'a str,
}

impl ExactBundleManifestV2 {
    pub fn validate(&self) -> Result<(), ExactTransferError> {
        if self.schema != EXACT_BUNDLE_SCHEMA_V2 {
            return Err(ExactTransferError::Contract(
                "unsupported exact bundle schema".to_owned(),
            ));
        }
        self.required_capabilities.validate()?;
        validate_authority_cursor(&self.source_authority_cursor)?;
        if !is_prefixed_sha256(&self.source_manifest_sha256)
            || !is_prefixed_sha256(&self.source_change_projection_sha256)
            || !is_prefixed_sha256(&self.event_set_sha256)
            || !is_prefixed_sha256(&self.bundle_sha256)
        {
            return Err(ExactTransferError::Contract(
                "exact bundle carries an invalid hash".to_owned(),
            ));
        }
        validate_records(&self.events, |kind| {
            kind == ExactBundleRecordKindV2::Event || kind.is_control()
        })?;
        validate_records(&self.content, ExactBundleRecordKindV2::is_content)?;
        validate_unique_keys(self.events.iter().chain(&self.content))?;
        validate_control_closure(
            &self.events,
            &self.required_capabilities,
            &self.source_manifest_sha256,
        )?;
        validate_closure(self)?;
        if self.event_set_sha256 != event_set_sha256(&self.events)?
            || self.bundle_sha256 != bundle_sha256(self)?
        {
            return Err(ExactTransferError::Contract(
                "exact bundle digest mismatch".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ExactTransferError {
    #[error("exact transfer contract failed: {0}")]
    Contract(String),
    #[error("destination is missing required capability {capability}")]
    MissingDestinationCapability { capability: String },
    #[error("logical key {logical_key} already exists with different decoded bytes")]
    HardConflict { logical_key: String },
    #[error("event {event_logical_key} requires absent content {content_logical_key}")]
    MissingRequiredContent {
        event_logical_key: String,
        content_logical_key: String,
    },
    #[error("qualification publication interrupted after {phase}")]
    Interrupted { phase: &'static str },
}

impl From<serde_json::Error> for ExactTransferError {
    fn from(error: serde_json::Error) -> Self {
        Self::Contract(error.to_string())
    }
}

impl From<ShoreError> for ExactTransferError {
    fn from(error: ShoreError) -> Self {
        Self::Contract(error.to_string())
    }
}

/// Reconcile one exact logical bundle into an already-complete Change store.
/// L0 and M1 destinations fail before any content or event is published. Source
/// capability records remain transfer proof and are never installed as a second
/// destination authority root.
pub fn import_exact_bundle_v2(
    repo: &Path,
    manifest: &ExactBundleManifestV2,
    local_import_context: &str,
) -> Result<ImportReceiptV1, ExactTransferError> {
    manifest.validate()?;
    let (resolved, _) = resolve_change_read_store(repo)?;
    let store_root = resolved.store_dir().to_path_buf();
    let _authority = StoreAuthorityLock::acquire(&store_root)?;
    let (resolved, inspection) = resolve_change_read_store(repo)?;
    if !matches!(inspection.status, StoreCapabilityStatus::Ready { .. })
        || inspection.minimum_reader_profile.as_deref()
            != Some(
                manifest
                    .required_capabilities
                    .minimum_reader_profile
                    .as_str(),
            )
    {
        return Err(ExactTransferError::MissingDestinationCapability {
            capability: manifest
                .required_capabilities
                .minimum_reader_profile
                .clone(),
        });
    }
    let journal = resolved.backend().journal();
    let content_store = resolved.backend().content_store();
    for record in &manifest.content {
        validate_content_locator(record)?;
        if let Some(existing) = content_store.get_if_exists(&record.logical_key)?
            && existing != record.decoded_bytes
        {
            return Err(ExactTransferError::HardConflict {
                logical_key: record.logical_key.clone(),
            });
        }
    }
    for record in manifest
        .events
        .iter()
        .filter(|record| record.record_kind == ExactBundleRecordKindV2::Event)
    {
        if let Some(existing) = journal.read_event_bytes(&record.logical_key)?
            && existing != record.decoded_bytes
        {
            return Err(ExactTransferError::HardConflict {
                logical_key: record.logical_key.clone(),
            });
        }
    }
    let receipt = ImportReceiptV1::new(manifest, local_import_context)?;
    for record in &manifest.content {
        content_store.put_once(&record.logical_key, &record.decoded_bytes)?;
    }
    for record in manifest
        .events
        .iter()
        .filter(|record| record.record_kind == ExactBundleRecordKindV2::Event)
    {
        journal.create_event_once(&record.logical_key, &record.decoded_bytes)?;
    }
    let after = crate::session::store::capabilities::inspect_journal_records(journal.as_ref())?;
    if !matches!(after.status, StoreCapabilityStatus::Ready { .. }) {
        return Err(ExactTransferError::Contract(
            "exact import did not preserve complete destination authority".to_owned(),
        ));
    }
    let receipt_hex = receipt
        .receipt_sha256
        .strip_prefix("sha256:")
        .ok_or_else(|| ExactTransferError::Contract("invalid import receipt digest".to_owned()))?;
    let receipt_path = Path::new("operations")
        .join("imports")
        .join(format!("{receipt_hex}.json"));
    let bytes = crate::canonical_hash::canonical_json_bytes(&serde_json::to_value(&receipt)?)?;
    let storage = LocalStorage::new(&store_root);
    match storage.create_file_exclusive(&receipt_path, &bytes, Durability::Durable)? {
        CreateOutcome::Created => {}
        CreateOutcome::AlreadyExists => {
            if storage.read_bytes(&receipt_path)? != bytes {
                return Err(ExactTransferError::HardConflict {
                    logical_key: receipt.receipt_sha256.clone(),
                });
            }
        }
    }
    Ok(receipt)
}

fn validate_content_locator(record: &ExactBundleRecordV2) -> Result<(), ExactTransferError> {
    let path = Path::new(&record.logical_key);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || !path.starts_with("artifacts")
    {
        return Err(ExactTransferError::Contract(format!(
            "exact content record has an unsafe destination locator: {}",
            record.logical_key
        )));
    }
    Ok(())
}

#[cfg(any(test, feature = "bench"))]
#[derive(Clone, Debug)]
pub(crate) struct ExactBundleSourceV2 {
    pub(crate) selection: ExactBundleSelectionV2,
    pub(crate) source_authority_cursor: AuthorityCursorV2,
    pub(crate) source_manifest_sha256: String,
    pub(crate) required_capabilities: ExactBundleCapabilityV2,
    pub(crate) records: Vec<ExactBundleRecordV2>,
    pub(crate) closure: Vec<ExactBundleClosureV2>,
}

#[cfg(any(test, feature = "bench"))]
impl ExactBundleSourceV2 {
    pub(crate) fn from_inspection(
        selection: ExactBundleSelectionV2,
        inspection: &crate::session::store::capabilities::JournalInspection,
        content: Vec<ExactBundleRecordV2>,
        closure: Vec<ExactBundleClosureV2>,
    ) -> Result<Self, ExactTransferError> {
        let crate::session::StoreCapabilityStatus::Ready { manifest_hash, .. } = &inspection.status
        else {
            return Err(ExactTransferError::Contract(
                "exact activated export requires an L2 source".to_owned(),
            ));
        };
        let mut records = content;
        let mut required_capabilities = None;
        for entry in &inspection.record_entries {
            if inspection
                .event_entries
                .iter()
                .any(|event| event.key_digest == entry.key_digest)
            {
                let event: ShoreEvent = serde_json::from_slice(&entry.bytes)?;
                records.push(ExactBundleRecordV2::new(
                    event.idempotency_key,
                    ExactBundleRecordKindV2::Event,
                    entry.bytes.clone(),
                )?);
                continue;
            }
            let control =
                crate::session::store::capabilities::transfer_control_record(&entry.bytes)?;
            let kind = match control.kind {
                crate::session::store::capabilities::TransferControlRecordKind::CapabilityActivation => {
                    required_capabilities = Some(ExactBundleCapabilityV2 {
                        minimum_reader_profile: control.minimum_reader_profile.ok_or_else(|| {
                            ExactTransferError::Contract(
                                "activation has no minimum reader profile".to_owned(),
                            )
                        })?,
                        required: control.required_capabilities,
                    });
                    ExactBundleRecordKindV2::CapabilityActivation
                }
                crate::session::store::capabilities::TransferControlRecordKind::BulkAdoptionCompletion => {
                    ExactBundleRecordKindV2::BulkAdoptionCompletion
                }
            };
            records.push(ExactBundleRecordV2::new(
                control.logical_key,
                kind,
                entry.bytes.clone(),
            )?);
        }
        Ok(Self {
            selection,
            source_authority_cursor: inspection.cursor.clone(),
            source_manifest_sha256: manifest_hash.clone(),
            required_capabilities: required_capabilities.ok_or_else(|| {
                ExactTransferError::Contract("source activation is absent".to_owned())
            })?,
            records,
            closure,
        })
    }

    pub(crate) fn build(self) -> Result<ExactBundleManifestV2, ExactTransferError> {
        let mut controls = self
            .records
            .iter()
            .filter(|record| record.record_kind.is_control())
            .cloned()
            .collect::<Vec<_>>();
        let all_events = self
            .records
            .iter()
            .filter(|record| record.record_kind == ExactBundleRecordKindV2::Event)
            .map(|record| {
                let event = serde_json::from_slice::<ShoreEvent>(&record.decoded_bytes)?;
                Ok((record, event))
            })
            .collect::<Result<Vec<_>, ExactTransferError>>()?;
        let strict_events = all_events
            .iter()
            .map(|(_, event)| event.clone())
            .collect::<Vec<_>>();
        let strict_projection = crate::session::project_changes(&strict_events)?;
        let change = strict_projection
            .changes
            .get(self.selection.change_id())
            .ok_or_else(|| ExactTransferError::Contract("selected Change is absent".to_owned()))?;
        let source_change_projection_sha256 = sha256_json_prefixed(&serde_json::to_value(change)?)?;

        let selected_keys = selected_event_keys(&self.selection, &all_events)?;
        let mut events = all_events
            .iter()
            .filter(|(record, _)| selected_keys.contains(&record.logical_key))
            .map(|(record, _)| (*record).clone())
            .collect::<Vec<_>>();
        if events.is_empty() {
            return Err(ExactTransferError::Contract(
                "selection produced no exact events".to_owned(),
            ));
        }
        let closure_by_event = self
            .closure
            .into_iter()
            .map(|item| (item.event_logical_key.clone(), item))
            .collect::<BTreeMap<_, _>>();
        let mut closure = Vec::with_capacity(events.len());
        let mut content_keys = BTreeSet::new();
        for event in &events {
            let item =
                closure_by_event
                    .get(&event.logical_key)
                    .cloned()
                    .unwrap_or(ExactBundleClosureV2 {
                        event_logical_key: event.logical_key.clone(),
                        required_content_keys: Vec::new(),
                    });
            content_keys.extend(item.required_content_keys.iter().cloned());
            closure.push(item);
        }
        let mut content = self
            .records
            .iter()
            .filter(|record| {
                record.record_kind.is_content() && content_keys.contains(&record.logical_key)
            })
            .cloned()
            .collect::<Vec<_>>();
        let selected_content_hashes = content
            .iter()
            .map(|record| record.decoded_sha256.as_str())
            .collect::<BTreeSet<_>>();
        for (record, event) in &all_events {
            if event.event_type == EventType::ArtifactRemoved
                && event
                    .payload
                    .get("contentHash")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|hash| selected_content_hashes.contains(hash))
                && !events
                    .iter()
                    .any(|selected| selected.logical_key == record.logical_key)
            {
                events.push((*record).clone());
                closure.push(ExactBundleClosureV2 {
                    event_logical_key: record.logical_key.clone(),
                    required_content_keys: Vec::new(),
                });
            }
        }
        events.append(&mut controls);
        sort_records(&mut events);
        sort_records(&mut content);
        for item in &mut closure {
            item.required_content_keys.sort();
            item.required_content_keys.dedup();
        }
        closure.extend(
            events
                .iter()
                .filter(|record| record.record_kind.is_control())
                .map(|record| ExactBundleClosureV2 {
                    event_logical_key: record.logical_key.clone(),
                    required_content_keys: Vec::new(),
                }),
        );
        closure.sort_by(|left, right| left.event_logical_key.cmp(&right.event_logical_key));
        let event_set_sha256 = event_set_sha256(&events)?;
        let mut manifest = ExactBundleManifestV2 {
            schema: EXACT_BUNDLE_SCHEMA_V2.to_owned(),
            selection: self.selection,
            source_authority_cursor: self.source_authority_cursor,
            source_manifest_sha256: self.source_manifest_sha256,
            required_capabilities: self.required_capabilities,
            events,
            content,
            closure,
            source_change_projection_sha256,
            event_set_sha256,
            bundle_sha256: String::new(),
        };
        manifest.bundle_sha256 = bundle_sha256(&manifest)?;
        manifest.validate()?;
        Ok(manifest)
    }
}

#[cfg(any(test, feature = "bench"))]
fn selected_event_keys(
    selection: &ExactBundleSelectionV2,
    events: &[(&ExactBundleRecordV2, ShoreEvent)],
) -> Result<BTreeSet<String>, ExactTransferError> {
    let change_id = selection.change_id();
    let mut fact_revisions = BTreeSet::<RevisionId>::new();
    if let ExactBundleSelectionV2::ExactRevision { revision, .. } = selection {
        fact_revisions.insert(revision.revision_id.clone());
    }
    let mut closure_revisions = fact_revisions.clone();
    let mut relation_claims = BTreeSet::new();
    for (_, event) in events {
        if event.event_type == EventType::ChangeRevisionRelationAsserted {
            let payload: ChangeRevisionRelationAssertedPayload =
                serde_json::from_value(event.payload.clone())?;
            let selected = &payload.change_id == change_id
                && match selection {
                    ExactBundleSelectionV2::ExactRevision { revision, .. } => {
                        payload.successor == *revision || payload.predecessor == *revision
                    }
                    ExactBundleSelectionV2::CompleteChange { .. } => true,
                };
            if selected {
                if matches!(selection, ExactBundleSelectionV2::CompleteChange { .. }) {
                    closure_revisions.insert(payload.successor.revision_id);
                    closure_revisions.insert(payload.predecessor.revision_id);
                }
                relation_claims.insert(payload.relation_claim_id);
            }
        }
    }
    let mut membership_claims = BTreeSet::new();
    for (_, event) in events {
        if event.event_type == EventType::ChangeMembershipAsserted {
            let payload: ChangeMembershipAssertedPayload =
                serde_json::from_value(event.payload.clone())?;
            if &payload.change_id == change_id
                && (matches!(selection, ExactBundleSelectionV2::CompleteChange { .. })
                    || closure_revisions.contains(&payload.revision_id))
            {
                if matches!(selection, ExactBundleSelectionV2::CompleteChange { .. }) {
                    fact_revisions.insert(payload.revision_id.clone());
                }
                closure_revisions.insert(payload.revision_id);
                membership_claims.insert(payload.membership_claim_id);
            }
        }
    }
    if let ExactBundleSelectionV2::ExactRevision { revision, .. } = selection {
        let mut bindings = BTreeSet::new();
        for (_, event) in events {
            if let Some(crate::session::projection::change::ChangeProjectionFact::Revision {
                revision_id,
                object_artifact_content_hash,
            }) = crate::session::projection::change::extract_change_projection_fact(event)?
                && revision_id == revision.revision_id
            {
                bindings.insert(object_artifact_content_hash);
            }
        }
        if bindings.len() != 1 || !bindings.contains(&revision.object_artifact_content_hash) {
            return Err(ExactTransferError::Contract(
                "selected exact Revision has an absent or divergent artifact binding".to_owned(),
            ));
        }
    }

    let mut selected = BTreeSet::new();
    let mut selected_event_ids = BTreeSet::new();
    for (record, event) in events {
        let include = match event.event_type {
            EventType::ChangeDeclared => {
                let fact =
                    crate::session::projection::change::extract_change_projection_fact(event)?;
                matches!(fact, Some(crate::session::projection::change::ChangeProjectionFact::Declaration { change_id: ref declared, .. }) if declared == change_id)
            }
            EventType::ChangeMembershipAsserted => {
                let payload: ChangeMembershipAssertedPayload =
                    serde_json::from_value(event.payload.clone())?;
                membership_claims.contains(&payload.membership_claim_id)
            }
            EventType::ChangeMembershipWithdrawn => {
                let payload: ChangeMembershipWithdrawnPayload =
                    serde_json::from_value(event.payload.clone())?;
                membership_claims.contains(&payload.membership_claim_id)
            }
            EventType::ChangeRevisionRelationAsserted => {
                let payload: ChangeRevisionRelationAssertedPayload =
                    serde_json::from_value(event.payload.clone())?;
                relation_claims.contains(&payload.relation_claim_id)
            }
            EventType::ChangeRevisionRelationWithdrawn => {
                let payload: ChangeRevisionRelationWithdrawnPayload =
                    serde_json::from_value(event.payload.clone())?;
                relation_claims.contains(&payload.relation_claim_id)
            }
            EventType::ChangeLinkAsserted => {
                matches!(selection, ExactBundleSelectionV2::CompleteChange { .. })
                    && event
                        .payload
                        .get("leftChangeId")
                        .and_then(serde_json::Value::as_str)
                        == Some(change_id.as_str())
                    || matches!(selection, ExactBundleSelectionV2::CompleteChange { .. })
                        && event
                            .payload
                            .get("rightChangeId")
                            .and_then(serde_json::Value::as_str)
                            == Some(change_id.as_str())
            }
            EventType::EventSignatureRecorded | EventType::ArtifactRemoved => false,
            EventType::WorkObjectProposed => event
                .subject_revision_id()?
                .is_some_and(|revision_id| closure_revisions.contains(&revision_id)),
            _ => event
                .subject_revision_id()?
                .is_some_and(|revision_id| fact_revisions.contains(&revision_id)),
        };
        if include {
            selected.insert(record.logical_key.clone());
            selected_event_ids.insert(event.event_id.clone());
        }
    }
    // Detached signatures follow a selected target event. Content-removal
    // claims follow selected closure through their content logical key instead
    // of being inferred here.
    for (record, event) in events {
        if event.event_type == EventType::EventSignatureRecorded
            && event
                .payload
                .get("targetEventId")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|id| {
                    selected_event_ids
                        .iter()
                        .any(|event_id| event_id.as_str() == id)
                })
        {
            selected.insert(record.logical_key.clone());
        }
    }
    Ok(selected)
}

#[cfg(any(test, feature = "bench"))]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum PublicationRecordClassV2 {
    Content,
    Capability,
    Cohort,
    Completion,
    Receipt,
}

#[cfg(any(test, feature = "bench"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExactBundleFailurePointV2 {
    None,
    AfterContent,
    AfterCapability,
    AfterCohort,
    AfterCompletion,
}

#[cfg(any(test, feature = "bench"))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ExactBundlePublicationReportV2 {
    pub(crate) created: usize,
    pub(crate) existing: usize,
    pub(crate) receipt_created: bool,
}

#[cfg(any(test, feature = "bench"))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DisposableExactDestinationV2 {
    capabilities: ExactBundleCapabilityV2,
    records: BTreeMap<String, ExactBundleRecordV2>,
    receipts: BTreeMap<String, ImportReceiptV1>,
    #[serde(skip)]
    publication_log: Vec<(PublicationRecordClassV2, String)>,
}

#[cfg(any(test, feature = "bench"))]
impl DisposableExactDestinationV2 {
    pub(crate) fn new(capabilities: ExactBundleCapabilityV2) -> Self {
        Self {
            capabilities,
            records: BTreeMap::new(),
            receipts: BTreeMap::new(),
            publication_log: Vec::new(),
        }
    }

    pub(crate) fn backup(&self) -> Result<Vec<u8>, ExactTransferError> {
        crate::canonical_hash::canonical_json_bytes(&serde_json::to_value(self)?)
            .map_err(ExactTransferError::from)
    }

    pub(crate) fn restore(bytes: &[u8]) -> Result<Self, ExactTransferError> {
        let restored: Self = serde_json::from_slice(bytes)?;
        for record in restored.records.values() {
            record.validate()?;
        }
        for receipt in restored.receipts.values() {
            receipt.validate()?;
        }
        Ok(restored)
    }

    pub(crate) fn publication_log(&self) -> &[(PublicationRecordClassV2, String)] {
        &self.publication_log
    }

    #[cfg(test)]
    fn seed_record(&mut self, record: ExactBundleRecordV2) {
        self.records.insert(record.logical_key.clone(), record);
    }

    #[cfg(test)]
    fn remove_record(&mut self, logical_key: &str) {
        self.records.remove(logical_key);
    }

    #[cfg(test)]
    fn record_count(&self) -> usize {
        self.records.len()
    }
}

#[cfg(any(test, feature = "bench"))]
pub(crate) fn publish_exact_bundle_for_qualification(
    destination: &mut DisposableExactDestinationV2,
    manifest: &ExactBundleManifestV2,
    local_import_context: &str,
    failure: ExactBundleFailurePointV2,
) -> Result<ExactBundlePublicationReportV2, ExactTransferError> {
    preflight(destination, manifest)?;
    let receipt = ImportReceiptV1::new(manifest, local_import_context)?;
    let mut report = ExactBundlePublicationReportV2::default();
    publish_records(
        destination,
        &manifest.content,
        PublicationRecordClassV2::Content,
        &mut report,
    );
    fail_after(failure, ExactBundleFailurePointV2::AfterContent, "content")?;
    publish_records(
        destination,
        manifest
            .events
            .iter()
            .filter(|record| record.record_kind == ExactBundleRecordKindV2::CapabilityActivation),
        PublicationRecordClassV2::Capability,
        &mut report,
    );
    fail_after(
        failure,
        ExactBundleFailurePointV2::AfterCapability,
        "capability",
    )?;
    publish_records(
        destination,
        manifest
            .events
            .iter()
            .filter(|record| record.record_kind == ExactBundleRecordKindV2::Event),
        PublicationRecordClassV2::Cohort,
        &mut report,
    );
    fail_after(failure, ExactBundleFailurePointV2::AfterCohort, "cohort")?;
    publish_records(
        destination,
        manifest
            .events
            .iter()
            .filter(|record| record.record_kind == ExactBundleRecordKindV2::BulkAdoptionCompletion),
        PublicationRecordClassV2::Completion,
        &mut report,
    );
    fail_after(
        failure,
        ExactBundleFailurePointV2::AfterCompletion,
        "completion",
    )?;
    if !destination.receipts.contains_key(&receipt.receipt_sha256) {
        destination.publication_log.push((
            PublicationRecordClassV2::Receipt,
            receipt.receipt_sha256.clone(),
        ));
        destination
            .receipts
            .insert(receipt.receipt_sha256.clone(), receipt);
        report.receipt_created = true;
    }
    Ok(report)
}

#[cfg(any(test, feature = "bench"))]
fn preflight(
    destination: &DisposableExactDestinationV2,
    manifest: &ExactBundleManifestV2,
) -> Result<(), ExactTransferError> {
    manifest.validate()?;
    if destination.capabilities.minimum_reader_profile
        != manifest.required_capabilities.minimum_reader_profile
    {
        return Err(ExactTransferError::MissingDestinationCapability {
            capability: manifest
                .required_capabilities
                .minimum_reader_profile
                .clone(),
        });
    }
    for capability in &manifest.required_capabilities.required {
        if !destination.capabilities.required.contains(capability) {
            return Err(ExactTransferError::MissingDestinationCapability {
                capability: capability.clone(),
            });
        }
    }
    for record in manifest.content.iter().chain(&manifest.events) {
        if let Some(existing) = destination.records.get(&record.logical_key)
            && existing != record
        {
            return Err(ExactTransferError::HardConflict {
                logical_key: record.logical_key.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(any(test, feature = "bench"))]
fn publish_records<'a>(
    destination: &mut DisposableExactDestinationV2,
    records: impl IntoIterator<Item = &'a ExactBundleRecordV2>,
    class: PublicationRecordClassV2,
    report: &mut ExactBundlePublicationReportV2,
) {
    for record in records {
        if destination.records.contains_key(&record.logical_key) {
            report.existing += 1;
        } else {
            destination
                .records
                .insert(record.logical_key.clone(), record.clone());
            destination
                .publication_log
                .push((class, record.logical_key.clone()));
            report.created += 1;
        }
    }
}

#[cfg(any(test, feature = "bench"))]
fn fail_after(
    actual: ExactBundleFailurePointV2,
    expected: ExactBundleFailurePointV2,
    phase: &'static str,
) -> Result<(), ExactTransferError> {
    if actual == expected {
        Err(ExactTransferError::Interrupted { phase })
    } else {
        Ok(())
    }
}

fn validate_records(
    records: &[ExactBundleRecordV2],
    expected: impl Fn(ExactBundleRecordKindV2) -> bool,
) -> Result<(), ExactTransferError> {
    let mut previous = None;
    for record in records {
        record.validate()?;
        if !expected(record.record_kind)
            || previous.is_some_and(|key: &str| record.logical_key.as_str() <= key)
        {
            return Err(ExactTransferError::Contract(
                "exact records have a wrong class or non-canonical order".to_owned(),
            ));
        }
        previous = Some(record.logical_key.as_str());
    }
    Ok(())
}

fn validate_unique_keys<'a>(
    records: impl Iterator<Item = &'a ExactBundleRecordV2>,
) -> Result<(), ExactTransferError> {
    let mut keys = BTreeSet::new();
    for record in records {
        if !keys.insert(&record.logical_key) {
            return Err(ExactTransferError::Contract(format!(
                "logical key {} appears more than once",
                record.logical_key
            )));
        }
    }
    Ok(())
}

fn validate_control_closure(
    records: &[ExactBundleRecordV2],
    capability: &ExactBundleCapabilityV2,
    source_manifest_sha256: &str,
) -> Result<(), ExactTransferError> {
    let activations = records
        .iter()
        .filter(|record| record.record_kind == ExactBundleRecordKindV2::CapabilityActivation)
        .count();
    let completions = records
        .iter()
        .filter(|record| record.record_kind == ExactBundleRecordKindV2::BulkAdoptionCompletion)
        .count();
    if activations != 1 || completions != 1 {
        return Err(ExactTransferError::Contract(
            "an activated exact bundle requires one activation and one completion".to_owned(),
        ));
    }
    let activation = records
        .iter()
        .find(|record| record.record_kind == ExactBundleRecordKindV2::CapabilityActivation)
        .expect("count proved one activation");
    let value: serde_json::Value = serde_json::from_slice(&activation.decoded_bytes)?;
    if value
        .get("minimumReaderProfile")
        .and_then(serde_json::Value::as_str)
        != Some(capability.minimum_reader_profile.as_str())
        || value
            .get("requiredCapabilities")
            .and_then(serde_json::Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            != Some(capability.required.clone())
    {
        return Err(ExactTransferError::Contract(
            "activation does not bind the advertised reader capability".to_owned(),
        ));
    }
    let completion = records
        .iter()
        .find(|record| record.record_kind == ExactBundleRecordKindV2::BulkAdoptionCompletion)
        .expect("count proved one completion");
    let completion_value: serde_json::Value = serde_json::from_slice(&completion.decoded_bytes)?;
    if completion_value.get("activationId") != value.get("activationId")
        || completion_value.get("bulkAdoptionManifestHash") != value.get("bulkAdoptionManifestHash")
        || value
            .get("bulkAdoptionManifestHash")
            .and_then(serde_json::Value::as_str)
            != Some(source_manifest_sha256)
    {
        return Err(ExactTransferError::Contract(
            "completion does not close the advertised activation manifest".to_owned(),
        ));
    }
    Ok(())
}

fn validate_authority_cursor(cursor: &AuthorityCursorV2) -> Result<(), ExactTransferError> {
    if cursor.schema != "pointbreak.authority-cursor.v2"
        || cursor.event_count > cursor.journal_record_count
        || !is_prefixed_sha256(&cursor.journal_record_set_hash)
        || !is_prefixed_sha256(&cursor.event_set_hash)
        || !is_prefixed_sha256(&cursor.capability_set_hash)
    {
        return Err(ExactTransferError::Contract(
            "source authority cursor is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn validate_closure(manifest: &ExactBundleManifestV2) -> Result<(), ExactTransferError> {
    let events = manifest
        .events
        .iter()
        .map(|record| record.logical_key.as_str())
        .collect::<BTreeSet<_>>();
    let content = manifest
        .content
        .iter()
        .map(|record| record.logical_key.as_str())
        .collect::<BTreeSet<_>>();
    if manifest.closure.len() != manifest.events.len() {
        return Err(ExactTransferError::Contract(
            "every selected event requires one closure row".to_owned(),
        ));
    }
    let mut previous = None;
    for item in &manifest.closure {
        if !events.contains(item.event_logical_key.as_str())
            || previous.is_some_and(|key: &str| item.event_logical_key.as_str() <= key)
            || item
                .required_content_keys
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(ExactTransferError::Contract(
                "exact closure is incomplete or non-canonical".to_owned(),
            ));
        }
        for key in &item.required_content_keys {
            if !content.contains(key.as_str()) {
                return Err(ExactTransferError::MissingRequiredContent {
                    event_logical_key: item.event_logical_key.clone(),
                    content_logical_key: key.clone(),
                });
            }
        }
        previous = Some(item.event_logical_key.as_str());
    }
    Ok(())
}

fn event_set_sha256(events: &[ExactBundleRecordV2]) -> Result<String, ExactTransferError> {
    sha256_json_prefixed(&serde_json::to_value(EventSetPreimage {
        schema: "pointbreak.exact-bundle-event-set.v2",
        events,
    })?)
    .map_err(ExactTransferError::from)
}

fn bundle_sha256(manifest: &ExactBundleManifestV2) -> Result<String, ExactTransferError> {
    sha256_json_prefixed(&serde_json::to_value(BundlePreimage {
        schema: &manifest.schema,
        selection: &manifest.selection,
        source_authority_cursor: &manifest.source_authority_cursor,
        source_manifest_sha256: &manifest.source_manifest_sha256,
        required_capabilities: &manifest.required_capabilities,
        events: &manifest.events,
        content: &manifest.content,
        closure: &manifest.closure,
        source_change_projection_sha256: &manifest.source_change_projection_sha256,
        event_set_sha256: &manifest.event_set_sha256,
    })?)
    .map_err(ExactTransferError::from)
}

#[cfg(any(test, feature = "bench"))]
fn sort_records(records: &mut [ExactBundleRecordV2]) {
    records.sort_by(|left, right| left.logical_key.cmp(&right.logical_key));
}

fn is_prefixed_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ChangeIdentityDescriptorV1, EngagementId, JournalId, ObjectId, RevisionId};
    use crate::session::event::{
        ArtifactRemovedPayload, EventPayload, EventTarget, Revision, WorkObjectProposal,
        WorkObjectProposedPayload, Writer,
    };
    use crate::session::store::backend::{Journal, StoreBackend};
    use crate::session::store::capabilities::{
        CapabilityFixtureState, inspect_journal_records, write_capability_fixture_for_test,
    };

    const HASH_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn change_id() -> ChangeId {
        crate::model::derive_change_id(&ChangeIdentityDescriptorV1::opaque_nonce([1; 32])).unwrap()
    }

    fn revision_ref(label: &str, hash: &str) -> RevisionRefV1 {
        RevisionRefV1::new(
            RevisionId::new(format!("rev:sha256:qualification-{label}")),
            hash,
        )
        .unwrap()
    }

    fn append_revision(
        journal: &dyn Journal,
        logical_key: &str,
        revision: &RevisionRefV1,
    ) -> ShoreEvent {
        let payload = WorkObjectProposedPayload {
            engagement_id: EngagementId::new(format!("engagement:sha256:{logical_key}")),
            work_object: WorkObjectProposal::Revision {
                revision: Revision {
                    id: revision.revision_id.clone(),
                    object_id: ObjectId::new(format!("obj:sha256:{logical_key}")),
                    git_provenance: None,
                },
                summary: None,
                object_artifact_content_hash: revision.object_artifact_content_hash.clone(),
                supersedes: Vec::new(),
            },
        };
        let event = ShoreEvent::new(
            payload.event_type(),
            logical_key,
            EventTarget::for_revision(
                JournalId::new("journal:qualification"),
                revision.revision_id.clone(),
                None,
            )
            .unwrap(),
            Writer::shore_local("qualification"),
            payload,
            "2026-08-04T00:00:00Z",
        )
        .unwrap();
        journal
            .create_event_once(logical_key, &serde_json::to_vec(&event).unwrap())
            .unwrap();
        event
    }

    fn source(
        selection: ExactBundleSelectionV2,
    ) -> Result<(ExactBundleManifestV2, ExactBundleCapabilityV2), ExactTransferError> {
        let backend = StoreBackend::memory();
        let journal = backend.journal();
        let revision_a = revision_ref("a", HASH_A);
        let revision_b = revision_ref("b", HASH_B);
        let event_a = append_revision(journal.as_ref(), "legacy:revision:a", &revision_a);
        let event_b = append_revision(journal.as_ref(), "legacy:revision:b", &revision_b);
        let removed_hash = format!("sha256:{}", sha256_bytes_hex(b"object-a"));
        let removal = ArtifactRemovedPayload {
            content_hash: removed_hash.clone(),
        };
        let removal_event = ShoreEvent::new(
            removal.event_type(),
            ArtifactRemovedPayload::idempotency_key(&removed_hash),
            EventTarget::for_journal(JournalId::new("journal:qualification")),
            Writer::shore_local("qualification"),
            removal,
            "2026-08-04T00:00:01Z",
        )
        .unwrap();
        journal
            .create_event_once(
                &removal_event.idempotency_key,
                &serde_json::to_vec(&removal_event).unwrap(),
            )
            .unwrap();
        write_capability_fixture_for_test(journal.as_ref(), CapabilityFixtureState::L2).unwrap();
        let inspection = inspect_journal_records(journal.as_ref()).unwrap();
        let content_a_key = format!("artifacts/objects/{}.json", sha256_bytes_hex(b"object-a"));
        let content_b_key = format!("artifacts/objects/{}.json", sha256_bytes_hex(b"object-b"));
        let content_a = ExactBundleRecordV2::new(
            &content_a_key,
            ExactBundleRecordKindV2::ObjectArtifact,
            b"object-a".to_vec(),
        )?;
        let content_b = ExactBundleRecordV2::new(
            &content_b_key,
            ExactBundleRecordKindV2::ObjectArtifact,
            b"object-b".to_vec(),
        )?;
        let source = ExactBundleSourceV2::from_inspection(
            selection,
            &inspection,
            vec![content_a, content_b],
            vec![
                ExactBundleClosureV2 {
                    event_logical_key: event_a.idempotency_key,
                    required_content_keys: vec![content_a_key],
                },
                ExactBundleClosureV2 {
                    event_logical_key: event_b.idempotency_key,
                    required_content_keys: vec![content_b_key],
                },
            ],
        )?;
        let capability = source.required_capabilities.clone();
        Ok((source.build()?, capability))
    }

    #[test]
    fn exact_revision_and_complete_change_select_distinct_closed_sets() {
        let (exact, _) = source(ExactBundleSelectionV2::ExactRevision {
            change_id: change_id(),
            revision: revision_ref("a", HASH_A),
        })
        .unwrap();
        let (complete, _) = source(ExactBundleSelectionV2::CompleteChange {
            change_id: change_id(),
        })
        .unwrap();

        assert!(exact.events.len() < complete.events.len());
        assert_eq!(
            exact
                .events
                .iter()
                .filter(|record| record.record_kind.is_control())
                .count(),
            2
        );
        assert_eq!(
            complete
                .events
                .iter()
                .filter(|record| record.record_kind.is_control())
                .count(),
            2
        );
        assert!(exact.events.iter().any(|record| {
            if record.record_kind != ExactBundleRecordKindV2::Event {
                return false;
            }
            serde_json::from_slice::<ShoreEvent>(&record.decoded_bytes)
                .unwrap()
                .event_type
                == EventType::ChangeRevisionRelationAsserted
        }));
        assert_eq!(
            exact.content.len(),
            1,
            "an exact Revision keeps the neighbouring relation endpoint hash-only"
        );
        assert!(exact.events.iter().any(|record| {
            if record.record_kind != ExactBundleRecordKindV2::Event {
                return false;
            }
            serde_json::from_slice::<ShoreEvent>(&record.decoded_bytes)
                .unwrap()
                .event_type
                == EventType::ArtifactRemoved
        }));
        assert!(!exact.events.iter().any(|record| {
            record.record_kind == ExactBundleRecordKindV2::Event
                && serde_json::from_slice::<ShoreEvent>(&record.decoded_bytes)
                    .is_ok_and(|event| event.event_type == EventType::RevisionRelationAttested)
        }));
        assert_eq!(
            exact.source_authority_cursor,
            complete.source_authority_cursor
        );
        assert_eq!(
            exact.source_change_projection_sha256,
            complete.source_change_projection_sha256
        );

        let document = serde_json::to_value(&complete).unwrap();
        let object = document.as_object().expect("manifest document");
        assert!(!object.contains_key("version"));
        assert!(!object.contains_key("controlRecords"));
        assert!(object.contains_key("selection"));
        assert!(
            complete.events.iter().any(|record| {
                record.record_kind == ExactBundleRecordKindV2::CapabilityActivation
            })
        );
        assert!(complete.events.iter().any(|record| {
            record.record_kind == ExactBundleRecordKindV2::BulkAdoptionCompletion
        }));
    }

    #[test]
    fn exact_revision_refuses_a_divergent_artifact_binding() {
        let error = source(ExactBundleSelectionV2::ExactRevision {
            change_id: change_id(),
            revision: revision_ref(
                "a",
                "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            ),
        })
        .unwrap_err();
        assert!(error.to_string().contains("artifact binding"));
    }

    #[test]
    fn preflight_is_zero_write_and_publication_is_phased_idempotent_and_receipt_last() {
        let (manifest, capability) = source(ExactBundleSelectionV2::CompleteChange {
            change_id: change_id(),
        })
        .unwrap();
        let mut incomplete = capability.clone();
        incomplete.required.pop();
        let mut rejected = DisposableExactDestinationV2::new(incomplete);
        assert!(matches!(
            publish_exact_bundle_for_qualification(
                &mut rejected,
                &manifest,
                "local:test",
                ExactBundleFailurePointV2::None,
            ),
            Err(ExactTransferError::MissingDestinationCapability { .. })
        ));
        assert_eq!(rejected.record_count(), 0);
        assert!(rejected.publication_log().is_empty());

        let mut missing_activation = manifest.clone();
        missing_activation
            .events
            .retain(|record| record.record_kind != ExactBundleRecordKindV2::CapabilityActivation);
        assert!(missing_activation.validate().is_err());
        let mut corrupt_activation = manifest.clone();
        corrupt_activation
            .events
            .iter_mut()
            .find(|record| record.record_kind == ExactBundleRecordKindV2::CapabilityActivation)
            .expect("activation")
            .decoded_bytes = b"{}".to_vec();
        assert!(corrupt_activation.validate().is_err());

        let mut destination = DisposableExactDestinationV2::new(capability);
        let report = publish_exact_bundle_for_qualification(
            &mut destination,
            &manifest,
            "local:test",
            ExactBundleFailurePointV2::None,
        )
        .unwrap();
        assert!(report.receipt_created);
        let receipt_document = serde_json::to_value(
            destination
                .receipts
                .values()
                .next()
                .expect("destination receipt"),
        )
        .unwrap();
        assert!(
            !receipt_document
                .as_object()
                .unwrap()
                .contains_key("version")
        );
        let classes = destination
            .publication_log()
            .iter()
            .map(|(class, _)| *class)
            .collect::<Vec<_>>();
        assert!(classes.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(classes.last(), Some(&PublicationRecordClassV2::Receipt));

        let record_count = destination.record_count();
        let second = publish_exact_bundle_for_qualification(
            &mut destination,
            &manifest,
            "local:test",
            ExactBundleFailurePointV2::None,
        )
        .unwrap();
        assert_eq!(second.created, 0);
        assert!(!second.receipt_created);
        assert_eq!(destination.record_count(), record_count);
    }

    #[test]
    fn every_interruption_retries_without_overwrite_or_delete_by_omission() {
        let (manifest, capability) = source(ExactBundleSelectionV2::CompleteChange {
            change_id: change_id(),
        })
        .unwrap();
        for failure in [
            ExactBundleFailurePointV2::AfterContent,
            ExactBundleFailurePointV2::AfterCapability,
            ExactBundleFailurePointV2::AfterCohort,
            ExactBundleFailurePointV2::AfterCompletion,
        ] {
            let mut destination = DisposableExactDestinationV2::new(capability.clone());
            let unrelated = ExactBundleRecordV2::new(
                format!("unrelated:{failure:?}"),
                ExactBundleRecordKindV2::ObjectArtifact,
                b"unrelated".to_vec(),
            )
            .unwrap();
            destination.seed_record(unrelated.clone());
            assert!(matches!(
                publish_exact_bundle_for_qualification(
                    &mut destination,
                    &manifest,
                    "local:retry",
                    failure,
                ),
                Err(ExactTransferError::Interrupted { .. })
            ));
            publish_exact_bundle_for_qualification(
                &mut destination,
                &manifest,
                "local:retry",
                ExactBundleFailurePointV2::None,
            )
            .unwrap();
            assert_eq!(
                destination.records.get(&unrelated.logical_key),
                Some(&unrelated)
            );
        }
    }

    #[test]
    fn divergence_never_overwrites_and_backup_restore_repair_preserves_receipts() {
        let (manifest, capability) = source(ExactBundleSelectionV2::CompleteChange {
            change_id: change_id(),
        })
        .unwrap();
        let mut conflicted = DisposableExactDestinationV2::new(capability.clone());
        let mut divergent = manifest.events[0].clone();
        divergent.decoded_bytes.push(b' ');
        divergent.decoded_sha256 = format!("sha256:{}", sha256_bytes_hex(&divergent.decoded_bytes));
        conflicted.seed_record(divergent);
        let before = conflicted.records.clone();
        assert!(matches!(
            publish_exact_bundle_for_qualification(
                &mut conflicted,
                &manifest,
                "local:conflict",
                ExactBundleFailurePointV2::None,
            ),
            Err(ExactTransferError::HardConflict { .. })
        ));
        assert_eq!(conflicted.records, before);

        let mut destination = DisposableExactDestinationV2::new(capability);
        publish_exact_bundle_for_qualification(
            &mut destination,
            &manifest,
            "local:backup",
            ExactBundleFailurePointV2::None,
        )
        .unwrap();
        let backup = destination.backup().unwrap();
        let mut restored = DisposableExactDestinationV2::restore(&backup).unwrap();
        assert_eq!(restored.receipts, destination.receipts);
        let repair_key = manifest.events[0].logical_key.clone();
        restored.remove_record(&repair_key);
        publish_exact_bundle_for_qualification(
            &mut restored,
            &manifest,
            "local:backup",
            ExactBundleFailurePointV2::None,
        )
        .unwrap();
        assert_eq!(restored.records.get(&repair_key), Some(&manifest.events[0]));

        let mut corrupt = backup;
        corrupt.push(b'!');
        assert!(DisposableExactDestinationV2::restore(&corrupt).is_err());
    }

    #[test]
    fn production_import_is_gated_by_l2_and_reconciles_exact_bytes_idempotently() {
        let (manifest, _) = source(ExactBundleSelectionV2::CompleteChange {
            change_id: change_id(),
        })
        .unwrap();
        let repo = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(repo.path())
                .output()
                .unwrap();
            assert!(output.status.success());
        };
        git(&["init", "--quiet"]);
        git(&["config", "user.name", "Pointbreak Test"]);
        git(&["config", "user.email", "pointbreak@example.test"]);
        let (store, _) = resolve_change_read_store(repo.path()).unwrap();
        assert!(matches!(
            import_exact_bundle_v2(repo.path(), &manifest, "local:product").unwrap_err(),
            ExactTransferError::MissingDestinationCapability { .. }
        ));
        crate::session::store::capabilities::write_capability_fixture_for_test(
            store.backend().journal().as_ref(),
            crate::session::store::capabilities::CapabilityFixtureState::EmptyL2,
        )
        .unwrap();
        let receipt = import_exact_bundle_v2(repo.path(), &manifest, "local:product").unwrap();
        let repeated = import_exact_bundle_v2(repo.path(), &manifest, "local:product").unwrap();
        assert_eq!(receipt, repeated);
        let receipt_hex = receipt.receipt_sha256.strip_prefix("sha256:").unwrap();
        assert!(
            store
                .store_dir()
                .join("operations")
                .join("imports")
                .join(format!("{receipt_hex}.json"))
                .is_file()
        );
        for record in &manifest.content {
            assert_eq!(
                store
                    .backend()
                    .content_store()
                    .get(&record.logical_key)
                    .unwrap(),
                record.decoded_bytes
            );
        }
        let (_, inspection) = resolve_change_read_store(repo.path()).unwrap();
        assert!(matches!(
            inspection.status,
            StoreCapabilityStatus::Ready { .. }
        ));
    }
}
