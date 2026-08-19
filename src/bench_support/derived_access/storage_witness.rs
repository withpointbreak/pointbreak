//! Read-only, path-free evidence for one selected derived generation.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension as _};
use serde::{Deserialize, Serialize};

use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
use crate::session::derived_access::generation::{
    GenerationDescriptor, GenerationLayout, GenerationPublication, GenerationReadLease,
};
use crate::session::derived_access::semantic::change::{
    CHANGE_READER_PROFILE_RESOURCE_V3, ReaderProjectionCheckpointV1,
    reader_projection_checkpoint_sha256_v1,
};
use crate::session::derived_access::sqlite::{
    sqlite_companion_exists, sqlite_immutable_read_only_uri,
};

pub const QUALIFICATION_DERIVED_STORAGE_WITNESS_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-storage-witness.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedStorageCarrierRoleV1 {
    Database,
    Wal,
    SharedMemory,
    Temporary,
    Descriptor,
    ReaderReceipt,
    Publication,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedStorageForbiddenProbeKindV1 {
    ProposalSummary,
    Prose,
    PayloadDocument,
    FixturePrivatePath,
    StoreRootPath,
}

impl QualificationDerivedStorageForbiddenProbeKindV1 {
    const ALL: [Self; 5] = [
        Self::ProposalSummary,
        Self::Prose,
        Self::PayloadDocument,
        Self::FixturePrivatePath,
        Self::StoreRootPath,
    ];
}

/// Runtime-only fixture sentinels. These bytes never serialize into evidence.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageForbiddenProbeInputV1 {
    pub proposal_summary: String,
    pub prose: String,
    pub payload_document: String,
    pub private_path: String,
}

impl QualificationDerivedStorageForbiddenProbeInputV1 {
    pub fn new(
        proposal_summary: impl Into<String>,
        prose: impl Into<String>,
        payload_document: impl Into<String>,
        private_path: impl Into<String>,
    ) -> Result<Self, String> {
        let input = Self {
            proposal_summary: proposal_summary.into(),
            prose: prose.into(),
            payload_document: payload_document.into(),
            private_path: private_path.into(),
        };
        input.validate()?;
        Ok(input)
    }

    pub fn validate(&self) -> Result<(), String> {
        for value in [
            &self.proposal_summary,
            &self.prose,
            &self.payload_document,
            &self.private_path,
        ] {
            if value.is_empty() {
                return Err("derived storage forbidden probe is empty".to_owned());
            }
        }
        Ok(())
    }

    fn values(&self) -> [(QualificationDerivedStorageForbiddenProbeKindV1, &[u8]); 4] {
        [
            (
                QualificationDerivedStorageForbiddenProbeKindV1::ProposalSummary,
                self.proposal_summary.as_bytes(),
            ),
            (
                QualificationDerivedStorageForbiddenProbeKindV1::Prose,
                self.prose.as_bytes(),
            ),
            (
                QualificationDerivedStorageForbiddenProbeKindV1::PayloadDocument,
                self.payload_document.as_bytes(),
            ),
            (
                QualificationDerivedStorageForbiddenProbeKindV1::FixturePrivatePath,
                self.private_path.as_bytes(),
            ),
        ]
    }

    pub fn canonical_hashes(&self) -> QualificationDerivedStorageForbiddenProbeHashesV1 {
        QualificationDerivedStorageForbiddenProbeHashesV1 {
            proposal_summary_sha256: sha256_bytes_hex(self.proposal_summary.as_bytes()),
            prose_sha256: sha256_bytes_hex(self.prose.as_bytes()),
            payload_document_sha256: sha256_bytes_hex(self.payload_document.as_bytes()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageForbiddenProbeHashesV1 {
    pub proposal_summary_sha256: String,
    pub prose_sha256: String,
    pub payload_document_sha256: String,
}

impl QualificationDerivedStorageForbiddenProbeHashesV1 {
    pub fn validate(&self) -> Result<(), String> {
        if [
            &self.proposal_summary_sha256,
            &self.prose_sha256,
            &self.payload_document_sha256,
        ]
        .into_iter()
        .any(|value| !is_sha256(value))
        {
            return Err("derived storage fixture probe hashes are invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStoragePublicationV1 {
    pub sequence: u64,
    pub generation_id_sha256: String,
    pub descriptor_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageDescriptorV1 {
    pub schema: String,
    pub profile: String,
    pub epoch: u64,
    pub head_sequence: u64,
    pub store_id_sha256: String,
    pub semantic_receipt_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageReaderReceiptV1 {
    pub schema: String,
    pub version: u32,
    pub receipt_sha256: String,
    pub content_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageLiveCheckpointV1 {
    pub schema: String,
    pub version: u32,
    pub checkpoint_sha256: String,
    pub reader_receipt_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageColumnV1 {
    pub ordinal: u32,
    pub name: String,
    pub declared_type: String,
    pub not_null: bool,
    pub default_sql: Option<String>,
    pub primary_key_ordinal: u32,
    pub hidden: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageIndexColumnV1 {
    pub ordinal: u32,
    pub table_column_ordinal: i32,
    pub name: Option<String>,
    pub descending: bool,
    pub collation: String,
    pub key: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageIndexV1 {
    pub ordinal: u32,
    pub name: String,
    pub unique: bool,
    pub origin: String,
    pub partial: bool,
    pub columns: Vec<QualificationDerivedStorageIndexColumnV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageCatalogEntryV1 {
    pub schema: String,
    pub name: String,
    pub kind: String,
    pub declared_column_count: u32,
    pub strict: bool,
    pub without_rowid: bool,
    pub columns: Vec<QualificationDerivedStorageColumnV1>,
    pub indexes: Vec<QualificationDerivedStorageIndexV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageCatalogV1 {
    pub entries: Vec<QualificationDerivedStorageCatalogEntryV1>,
    pub catalog_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageCarrierV1 {
    pub role: QualificationDerivedStorageCarrierRoleV1,
    pub relative_path_sha256: String,
    pub byte_count: u64,
    pub content_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageBytesV1 {
    pub database: u64,
    pub wal: u64,
    pub shared_memory: u64,
    pub temporary: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageForbiddenProbeV1 {
    pub kind: QualificationDerivedStorageForbiddenProbeKindV1,
    pub sentinel_sha256: String,
    pub found: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedStorageWitnessV1 {
    pub schema: String,
    pub publication: QualificationDerivedStoragePublicationV1,
    pub descriptor: QualificationDerivedStorageDescriptorV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reader_receipt: Option<QualificationDerivedStorageReaderReceiptV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_checkpoint: Option<QualificationDerivedStorageLiveCheckpointV1>,
    pub sqlite_catalog: QualificationDerivedStorageCatalogV1,
    pub carriers: Vec<QualificationDerivedStorageCarrierV1>,
    pub bytes: QualificationDerivedStorageBytesV1,
    pub forbidden_probes: Vec<QualificationDerivedStorageForbiddenProbeV1>,
    pub witness_sha256: String,
}

impl QualificationDerivedStorageWitnessV1 {
    pub fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.witness_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn refresh_sha256(&mut self) -> Result<(), String> {
        self.witness_sha256 = self.canonical_sha256()?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_STORAGE_WITNESS_SCHEMA_V1
            || !is_sha256(&self.publication.generation_id_sha256)
            || !is_sha256(&self.publication.descriptor_sha256)
            || !is_sha256(&self.descriptor.store_id_sha256)
            || !is_sha256(&self.descriptor.semantic_receipt_sha256)
            || !is_sha256(&self.sqlite_catalog.catalog_sha256)
            || !is_sha256(&self.witness_sha256)
            || self
                .forbidden_probes
                .iter()
                .any(|probe| !is_sha256(&probe.sentinel_sha256))
        {
            return Err("derived storage witness carries a malformed identity".to_owned());
        }
        if self.witness_sha256 != self.canonical_sha256()? {
            return Err("derived storage witness self-hash drifted".to_owned());
        }
        if self.sqlite_catalog.catalog_sha256 != canonical_sha256(&self.sqlite_catalog.entries)? {
            return Err("derived storage witness catalog hash drifted".to_owned());
        }
        if self.carriers.is_empty() {
            return Err("derived storage witness selected no carriers".to_owned());
        }
        if self.forbidden_probes.len() != QualificationDerivedStorageForbiddenProbeKindV1::ALL.len()
        {
            return Err("derived storage witness omitted a forbidden probe".to_owned());
        }
        let found = self
            .forbidden_probes
            .iter()
            .filter(|probe| probe.found)
            .map(|probe| format!("{:?}", probe.kind))
            .collect::<Vec<_>>();
        if !found.is_empty() {
            return Err(format!(
                "derived storage witness found forbidden fixture bytes: {}",
                found.join(", ")
            ));
        }
        if self.reader_receipt.as_ref().is_some_and(|receipt| {
            !is_sha256(&receipt.receipt_sha256) || !is_sha256(&receipt.content_sha256)
        }) || self.live_checkpoint.as_ref().is_some_and(|checkpoint| {
            !is_sha256(&checkpoint.checkpoint_sha256)
                || !is_sha256(&checkpoint.reader_receipt_sha256)
                || self.reader_receipt.as_ref().is_none_or(|receipt| {
                    receipt.receipt_sha256 != checkpoint.reader_receipt_sha256
                })
        }) {
            return Err("derived storage witness has an invalid publication identifier".to_owned());
        }
        let probe_kinds = self
            .forbidden_probes
            .iter()
            .map(|probe| probe.kind)
            .collect::<std::collections::BTreeSet<_>>();
        if probe_kinds.len() != QualificationDerivedStorageForbiddenProbeKindV1::ALL.len() {
            return Err("derived storage witness has duplicate forbidden probes".to_owned());
        }
        let mut previous = None;
        let mut observed_bytes = QualificationDerivedStorageBytesV1 {
            database: 0,
            wal: 0,
            shared_memory: 0,
            temporary: 0,
        };
        for carrier in &self.carriers {
            if !is_sha256(&carrier.relative_path_sha256) || !is_sha256(&carrier.content_sha256) {
                return Err("derived storage witness has an invalid carrier hash".to_owned());
            }
            let key = (carrier.role, carrier.relative_path_sha256.as_str());
            if previous.is_some_and(|last| last >= key) {
                return Err("derived storage carriers are not deterministically ordered".to_owned());
            }
            previous = Some(key);
            match carrier.role {
                QualificationDerivedStorageCarrierRoleV1::Database => {
                    observed_bytes.database += carrier.byte_count
                }
                QualificationDerivedStorageCarrierRoleV1::Wal => {
                    observed_bytes.wal += carrier.byte_count
                }
                QualificationDerivedStorageCarrierRoleV1::SharedMemory => {
                    observed_bytes.shared_memory += carrier.byte_count
                }
                QualificationDerivedStorageCarrierRoleV1::Temporary => {
                    observed_bytes.temporary += carrier.byte_count
                }
                _ => {}
            }
        }
        if self.bytes != observed_bytes || self.bytes.database == 0 {
            return Err("derived storage witness byte inventory is inconsistent".to_owned());
        }
        Ok(())
    }
}

/// Capture one immutable published generation without opening it for write.
pub fn capture_qualification_derived_storage_witness_v1(
    store_root: &Path,
    forbidden: &QualificationDerivedStorageForbiddenProbeInputV1,
) -> Result<QualificationDerivedStorageWitnessV1, String> {
    let mut last_attempt_error = None;
    for _ in 0..3 {
        let selected = match stable_selected_generation(store_root) {
            Ok(selected) => selected,
            Err(error) => {
                last_attempt_error = Some(error);
                continue;
            }
        };
        let snapshot = match stable_storage_snapshot(store_root, &selected, forbidden) {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => {
                // Name the churn instead of clearing the accumulator, so an
                // earlier attributable structural failure is never erased
                // into the anonymous exhaustion fallback.
                last_attempt_error = Some(format!(
                    "derived storage snapshot was unstable at generation sequence {}",
                    selected.publication.sequence
                ));
                continue;
            }
            Err(error) => {
                last_attempt_error = Some(error);
                continue;
            }
        };
        // A forbidden probe that survived the triple-read stability window is
        // a genuine finding: fail closed immediately and name every probe and
        // carrier so the failure is attributable without another invocation.
        if !snapshot.probe_hits.is_empty() {
            let hits = snapshot
                .probe_hits
                .iter()
                .map(|(kind, relative_path)| format!("{kind:?} in {relative_path}"))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "derived storage witness found forbidden fixture bytes at generation \
                 sequence {}: {hits}",
                selected.publication.sequence
            ));
        }
        let descriptor_value =
            serde_json::to_value(&selected.descriptor).map_err(|error| error.to_string())?;
        let descriptor_schema = descriptor_value
            .get("schema")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "derived generation descriptor omitted schema".to_owned())?;

        let mut witness = QualificationDerivedStorageWitnessV1 {
            schema: QUALIFICATION_DERIVED_STORAGE_WITNESS_SCHEMA_V1.to_owned(),
            publication: QualificationDerivedStoragePublicationV1 {
                sequence: selected.publication.sequence,
                generation_id_sha256: sha256_bytes_hex(
                    selected.publication.generation_id.as_bytes(),
                ),
                descriptor_sha256: selected.publication.descriptor_sha256.clone(),
            },
            descriptor: QualificationDerivedStorageDescriptorV1 {
                schema: descriptor_schema.to_owned(),
                profile: selected.descriptor.profile.as_str().to_owned(),
                epoch: selected.descriptor.epoch,
                head_sequence: selected.descriptor.head_sequence,
                store_id_sha256: sha256_bytes_hex(selected.descriptor.store_id.as_bytes()),
                semantic_receipt_sha256: selected.descriptor.semantic_receipt.clone(),
            },
            reader_receipt: snapshot.reader_receipt,
            live_checkpoint: snapshot.live_checkpoint,
            sqlite_catalog: snapshot.catalog,
            carriers: snapshot.carriers,
            bytes: snapshot.bytes,
            forbidden_probes: snapshot.forbidden_probes,
            witness_sha256: String::new(),
        };
        witness.refresh_sha256()?;
        // With probe hits already excluded above, remaining validation
        // failures are structural (empty carriers, catalog or hash drift) and
        // are treated as capture instability: retry, then fail with the
        // attributable condition rather than silently converging.
        match witness.validate() {
            Ok(()) => return Ok(witness),
            Err(error) => {
                last_attempt_error = Some(format!(
                    "unstable derived storage witness at generation sequence {}: {error}",
                    selected.publication.sequence
                ));
                continue;
            }
        }
    }
    Err(last_attempt_error
        .unwrap_or_else(|| "derived storage changed throughout witness capture".to_owned()))
}

struct StableStorageSnapshot {
    reader_receipt: Option<QualificationDerivedStorageReaderReceiptV1>,
    live_checkpoint: Option<QualificationDerivedStorageLiveCheckpointV1>,
    catalog: QualificationDerivedStorageCatalogV1,
    carriers: Vec<QualificationDerivedStorageCarrierV1>,
    bytes: QualificationDerivedStorageBytesV1,
    forbidden_probes: Vec<QualificationDerivedStorageForbiddenProbeV1>,
    probe_hits: Vec<(QualificationDerivedStorageForbiddenProbeKindV1, String)>,
}

fn stable_storage_snapshot(
    store_root: &Path,
    selected: &SelectedGeneration,
    forbidden: &QualificationDerivedStorageForbiddenProbeInputV1,
) -> Result<Option<StableStorageSnapshot>, String> {
    stable_storage_snapshot_with_hook(store_root, selected, forbidden, |_| Ok(()))
}

fn stable_storage_snapshot_with_hook(
    store_root: &Path,
    selected: &SelectedGeneration,
    forbidden: &QualificationDerivedStorageForbiddenProbeInputV1,
    mut between_snapshot_reads: impl FnMut(&Path) -> Result<(), String>,
) -> Result<Option<StableStorageSnapshot>, String> {
    let before = collect_carriers(
        store_root,
        &selected.generation_root,
        selected.publication_path.as_deref(),
        forbidden,
    )?;
    between_snapshot_reads(&selected.generation_root)?;
    let reader_receipt = read_reader_receipt(&selected.generation_root)?;
    let database = selected.generation_root.join("cursor.sqlite3");
    let (mut connection, immutable_without_companions) = storage_read_connection(&database)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let live_checkpoint = read_live_checkpoint(&transaction)?;
    let catalog = sqlite_catalog(&transaction)?;
    let during = collect_carriers(
        store_root,
        &selected.generation_root,
        selected.publication_path.as_deref(),
        forbidden,
    )?;
    between_snapshot_reads(&selected.generation_root)?;
    drop(transaction);
    drop(connection);
    let after = collect_carriers(
        store_root,
        &selected.generation_root,
        selected.publication_path.as_deref(),
        forbidden,
    )?;
    if !carrier_snapshots_are_stable(&before, &during, &after)
        || immutable_without_companions && sqlite_companion_exists(&database)
        || ensure_publication_is_current(store_root, &selected.publication).is_err()
    {
        return Ok(None);
    }
    let CollectedCarriersV1 {
        carriers,
        bytes,
        forbidden_probes,
        probe_hits,
    } = after;
    Ok(Some(StableStorageSnapshot {
        reader_receipt,
        live_checkpoint,
        catalog,
        carriers,
        bytes,
        forbidden_probes,
        probe_hits,
    }))
}

fn carrier_snapshots_are_stable<T: Eq>(before: &T, during: &T, after: &T) -> bool {
    before == during && during == after
}

struct SelectedGeneration {
    publication: GenerationPublication,
    descriptor: GenerationDescriptor,
    generation_root: PathBuf,
    publication_path: Option<PathBuf>,
    _lease: GenerationReadLease,
}

fn stable_selected_generation(store_root: &Path) -> Result<SelectedGeneration, String> {
    let layout = GenerationLayout::new(store_root).map_err(|error| error.to_string())?;
    validate_publication_directory(&layout)?;
    for _ in 0..3 {
        let publication = layout
            .current_publication()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "derived storage witness has no current publication".to_owned())?;
        let lease = layout
            .acquire_read_lease(&publication.generation_id)
            .map_err(|error| error.to_string())?;
        let reread = layout
            .current_publication()
            .map_err(|error| error.to_string())?;
        if reread.as_ref() != Some(&publication) {
            drop(lease);
            continue;
        }
        let generation_root = layout.generation(&publication.generation_id);
        validate_regular_tree(&generation_root)?;
        let descriptor = layout
            .descriptor(&publication)
            .map_err(|error| error.to_string())?;
        let publication_path = publication_file_path(&layout, &publication)?;
        return Ok(SelectedGeneration {
            generation_root,
            publication,
            descriptor,
            publication_path,
            _lease: lease,
        });
    }
    Err("derived storage publication changed while acquiring its read lease".to_owned())
}

fn validate_publication_directory(layout: &GenerationLayout) -> Result<(), String> {
    let root = layout.root().join("publications");
    let metadata = std::fs::symlink_metadata(&root).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("derived storage witness rejects symbolic links".to_owned());
    }
    if !metadata.is_dir() {
        return Err("derived storage witness rejects non-directory publications".to_owned());
    }
    for entry in std::fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            return Err("derived storage witness rejects symbolic links".to_owned());
        }
        if !file_type.is_file() {
            return Err("derived storage witness rejects non-file publication carriers".to_owned());
        }
    }
    Ok(())
}

fn publication_file_path(
    layout: &GenerationLayout,
    publication: &GenerationPublication,
) -> Result<Option<PathBuf>, String> {
    let root = layout.root().join("publications");
    if !root.exists() {
        return Ok(None);
    }
    for entry in std::fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            return Err("derived storage witness rejects symbolic links".to_owned());
        }
        if !file_type.is_file() {
            continue;
        }
        let candidate: GenerationPublication = serde_json::from_slice(
            &std::fs::read(entry.path()).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        if candidate == *publication {
            return Ok(Some(entry.path()));
        }
    }
    Err("derived storage current publication carrier is absent".to_owned())
}

fn ensure_publication_is_current(
    store_root: &Path,
    expected: &GenerationPublication,
) -> Result<(), String> {
    let layout = GenerationLayout::new(store_root).map_err(|error| error.to_string())?;
    if layout
        .current_publication()
        .map_err(|error| error.to_string())?
        .as_ref()
        != Some(expected)
    {
        return Err("derived storage publication changed during witness capture".to_owned());
    }
    Ok(())
}

fn read_reader_receipt(
    generation_root: &Path,
) -> Result<Option<QualificationDerivedStorageReaderReceiptV1>, String> {
    let path = generation_root.join(CHANGE_READER_PROFILE_RESOURCE_V3);
    let bytes = match read_regular_file(&path)? {
        Some(bytes) => bytes,
        None => return Ok(None),
    };
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let schema = value
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "derived Change reader receipt omitted schema".to_owned())?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "derived Change reader receipt omitted version".to_owned())?;
    Ok(Some(QualificationDerivedStorageReaderReceiptV1 {
        schema: schema.to_owned(),
        version: u32::try_from(version)
            .map_err(|_| "derived Change reader receipt version overflowed".to_owned())?,
        receipt_sha256: value
            .get("receiptSha256")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "derived Change reader receipt omitted receipt hash".to_owned())?
            .to_owned(),
        content_sha256: sha256_bytes_hex(&bytes),
    }))
}

fn storage_read_connection(database: &Path) -> Result<(Connection, bool), String> {
    let immutable_without_companions = !sqlite_companion_exists(database);
    let connection = if immutable_without_companions {
        Connection::open_with_flags(
            sqlite_immutable_read_only_uri(database),
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI,
        )
    } else {
        Connection::open_with_flags(
            database,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
    }
    .map_err(|error| error.to_string())?;
    Ok((connection, immutable_without_companions))
}

fn read_live_checkpoint(
    connection: &Connection,
) -> Result<Option<QualificationDerivedStorageLiveCheckpointV1>, String> {
    let table_exists = connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_schema
                 WHERE type = 'table' AND name = 'reader_projection_checkpoint'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| error.to_string())?;
    if !table_exists {
        return Ok(None);
    }
    let checkpoint_json = connection
        .query_row(
            "SELECT checkpoint_json
             FROM reader_projection_checkpoint WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some(checkpoint_json) = checkpoint_json else {
        return Ok(None);
    };
    let checkpoint: ReaderProjectionCheckpointV1 =
        serde_json::from_str(&checkpoint_json).map_err(|error| error.to_string())?;
    if checkpoint.checkpoint_sha256
        != reader_projection_checkpoint_sha256_v1(&checkpoint).map_err(|error| error.to_string())?
    {
        return Err("derived storage live checkpoint self-hash drifted".to_owned());
    }
    Ok(Some(QualificationDerivedStorageLiveCheckpointV1 {
        schema: checkpoint.schema,
        version: checkpoint.version,
        checkpoint_sha256: checkpoint.checkpoint_sha256,
        reader_receipt_sha256: checkpoint.reader_receipt_sha256,
    }))
}

fn sqlite_catalog(connection: &Connection) -> Result<QualificationDerivedStorageCatalogV1, String> {
    let mut statement = connection
        .prepare("PRAGMA table_list")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut entries = Vec::new();
    for (schema, name, kind, declared_column_count, without_rowid, strict) in rows {
        if name.starts_with("sqlite_") {
            continue;
        }
        entries.push(QualificationDerivedStorageCatalogEntryV1 {
            schema,
            columns: table_columns(connection, &name)?,
            indexes: table_indexes(connection, &name)?,
            name,
            kind,
            declared_column_count: u32::try_from(declared_column_count)
                .map_err(|_| "SQLite table_list column count overflowed".to_owned())?,
            strict: strict != 0,
            without_rowid: without_rowid != 0,
        });
    }
    entries.sort_by(|left, right| {
        (left.schema.as_str(), left.kind.as_str(), left.name.as_str()).cmp(&(
            right.schema.as_str(),
            right.kind.as_str(),
            right.name.as_str(),
        ))
    });
    let catalog_sha256 = canonical_sha256(&entries)?;
    Ok(QualificationDerivedStorageCatalogV1 {
        entries,
        catalog_sha256,
    })
}

fn table_columns(
    connection: &Connection,
    table: &str,
) -> Result<Vec<QualificationDerivedStorageColumnV1>, String> {
    let pragma = format!("PRAGMA table_xinfo({})", quote_identifier(table));
    let mut statement = connection
        .prepare(&pragma)
        .map_err(|error| error.to_string())?;
    let mut columns = statement
        .query_map([], |row| {
            Ok(QualificationDerivedStorageColumnV1 {
                ordinal: u32::try_from(row.get::<_, i64>(0)?)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, 0))?,
                name: row.get(1)?,
                declared_type: row.get(2)?,
                not_null: row.get::<_, i64>(3)? != 0,
                default_sql: row.get(4)?,
                primary_key_ordinal: u32::try_from(row.get::<_, i64>(5)?)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(5, 0))?,
                hidden: u32::try_from(row.get::<_, i64>(6)?)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(6, 0))?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    columns.sort_by_key(|column| column.ordinal);
    Ok(columns)
}

fn table_indexes(
    connection: &Connection,
    table: &str,
) -> Result<Vec<QualificationDerivedStorageIndexV1>, String> {
    let pragma = format!("PRAGMA index_list({})", quote_identifier(table));
    let mut statement = connection
        .prepare(&pragma)
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut indexes = Vec::new();
    for (ordinal, name, unique, origin, partial) in rows {
        let pragma = format!("PRAGMA index_xinfo({})", quote_identifier(&name));
        let mut columns_statement = connection
            .prepare(&pragma)
            .map_err(|error| error.to_string())?;
        let mut columns = columns_statement
            .query_map([], |row| {
                Ok(QualificationDerivedStorageIndexColumnV1 {
                    ordinal: u32::try_from(row.get::<_, i64>(0)?)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, 0))?,
                    table_column_ordinal: i32::try_from(row.get::<_, i64>(1)?)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, 0))?,
                    name: row.get(2)?,
                    descending: row.get::<_, i64>(3)? != 0,
                    collation: row.get(4)?,
                    key: row.get::<_, i64>(5)? != 0,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        columns.sort_by_key(|column| column.ordinal);
        indexes.push(QualificationDerivedStorageIndexV1 {
            ordinal: u32::try_from(ordinal)
                .map_err(|_| "SQLite index_list ordinal overflowed".to_owned())?,
            name,
            unique: unique != 0,
            origin,
            partial: partial != 0,
            columns,
        });
    }
    indexes.sort_by(|left, right| {
        (left.ordinal, left.name.as_str()).cmp(&(right.ordinal, right.name.as_str()))
    });
    Ok(indexes)
}

#[derive(Eq, PartialEq)]
struct CollectedCarriersV1 {
    carriers: Vec<QualificationDerivedStorageCarrierV1>,
    bytes: QualificationDerivedStorageBytesV1,
    forbidden_probes: Vec<QualificationDerivedStorageForbiddenProbeV1>,
    probe_hits: Vec<(QualificationDerivedStorageForbiddenProbeKindV1, String)>,
}

fn collect_carriers(
    store_root: &Path,
    generation_root: &Path,
    publication_path: Option<&Path>,
    forbidden: &QualificationDerivedStorageForbiddenProbeInputV1,
) -> Result<CollectedCarriersV1, String> {
    let mut files = Vec::new();
    collect_regular_files(generation_root, generation_root, &mut files)?;
    if let Some(publication) = publication_path {
        files.push((publication.to_path_buf(), "publication".to_owned()));
    }
    files.sort_by(|left, right| left.1.cmp(&right.1));
    let mut probes = forbidden
        .values()
        .into_iter()
        .map(|(kind, value)| (kind, value.to_vec()))
        .collect::<Vec<_>>();
    probes.push((
        QualificationDerivedStorageForbiddenProbeKindV1::StoreRootPath,
        store_root.as_os_str().as_encoded_bytes().to_vec(),
    ));
    let mut found = vec![false; probes.len()];
    let mut probe_hits = Vec::new();
    let mut carriers = Vec::new();
    let mut bytes = QualificationDerivedStorageBytesV1 {
        database: 0,
        wal: 0,
        shared_memory: 0,
        temporary: 0,
    };
    for (path, relative) in files {
        let content = read_regular_file(&path)?.ok_or_else(|| {
            "derived storage selected carrier disappeared while reading".to_owned()
        })?;
        for (index, (kind, sentinel)) in probes.iter().enumerate() {
            if contains_bytes(&content, sentinel) {
                found[index] = true;
                probe_hits.push((*kind, relative.clone()));
            }
        }
        let role = carrier_role(&relative);
        let byte_count = content.len() as u64;
        match role {
            QualificationDerivedStorageCarrierRoleV1::Database => bytes.database += byte_count,
            QualificationDerivedStorageCarrierRoleV1::Wal => bytes.wal += byte_count,
            QualificationDerivedStorageCarrierRoleV1::SharedMemory => {
                bytes.shared_memory += byte_count
            }
            QualificationDerivedStorageCarrierRoleV1::Temporary => bytes.temporary += byte_count,
            _ => {}
        }
        carriers.push(QualificationDerivedStorageCarrierV1 {
            role,
            relative_path_sha256: sha256_bytes_hex(relative.as_bytes()),
            byte_count,
            content_sha256: sha256_bytes_hex(&content),
        });
    }
    carriers.sort_by(|left, right| {
        (left.role, left.relative_path_sha256.as_str())
            .cmp(&(right.role, right.relative_path_sha256.as_str()))
    });
    let forbidden_probes = probes
        .iter()
        .enumerate()
        .map(
            |(index, (kind, value))| QualificationDerivedStorageForbiddenProbeV1 {
                kind: *kind,
                sentinel_sha256: sha256_bytes_hex(value),
                found: found[index],
            },
        )
        .collect();
    probe_hits.sort();
    Ok(CollectedCarriersV1 {
        carriers,
        bytes,
        forbidden_probes,
        probe_hits,
    })
}

fn collect_regular_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(PathBuf, String)>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("derived storage witness rejects symbolic links".to_owned());
        }
        if metadata.is_dir() {
            collect_regular_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            files.push((path, relative));
        } else {
            return Err("derived storage witness rejects non-file carriers".to_owned());
        }
    }
    Ok(())
}

fn validate_regular_tree(root: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("derived storage witness rejects symbolic links".to_owned());
    }
    if !metadata.is_dir() {
        return Err("derived storage witness rejects non-directory generations".to_owned());
    }
    collect_regular_files(root, root, &mut Vec::new())
}

fn read_regular_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() {
        return Err("derived storage witness rejects symbolic links".to_owned());
    }
    if !metadata.is_file() {
        return Err("derived storage witness rejects non-file carriers".to_owned());
    }
    std::fs::read(path)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn carrier_role(relative: &str) -> QualificationDerivedStorageCarrierRoleV1 {
    match relative {
        "cursor.sqlite3" => QualificationDerivedStorageCarrierRoleV1::Database,
        "cursor.sqlite3-wal" => QualificationDerivedStorageCarrierRoleV1::Wal,
        "cursor.sqlite3-shm" => QualificationDerivedStorageCarrierRoleV1::SharedMemory,
        "generation.json" => QualificationDerivedStorageCarrierRoleV1::Descriptor,
        CHANGE_READER_PROFILE_RESOURCE_V3 => {
            QualificationDerivedStorageCarrierRoleV1::ReaderReceipt
        }
        "publication" => QualificationDerivedStorageCarrierRoleV1::Publication,
        _ if relative.ends_with(".tmp")
            || relative
                .rsplit('/')
                .next()
                .is_some_and(|name| name.starts_with('.')) =>
        {
            QualificationDerivedStorageCarrierRoleV1::Temporary
        }
        _ => QualificationDerivedStorageCarrierRoleV1::Other,
    }
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn canonical_sha256<T: Serialize>(value: &T) -> Result<String, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    canonical_json_bytes(&value)
        .map(|bytes| sha256_bytes_hex(&bytes))
        .map_err(|error| error.to_string())
}

fn is_sha256(value: &str) -> bool {
    let value = value.strip_prefix("sha256:").unwrap_or(value);
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
impl QualificationDerivedStorageWitnessV1 {
    pub(crate) fn test_fixture(checkpoint: &str) -> Self {
        let entries = Vec::new();
        let mut witness = Self {
            schema: QUALIFICATION_DERIVED_STORAGE_WITNESS_SCHEMA_V1.to_owned(),
            publication: QualificationDerivedStoragePublicationV1 {
                sequence: 1,
                generation_id_sha256: sha256_bytes_hex(b"generation"),
                descriptor_sha256: sha256_bytes_hex(b"descriptor"),
            },
            descriptor: QualificationDerivedStorageDescriptorV1 {
                schema: "pointbreak.derived-generation.v1".to_owned(),
                profile: "sqlite-wal-bodyless-v1".to_owned(),
                epoch: 1,
                head_sequence: 128,
                store_id_sha256: sha256_bytes_hex(b"store"),
                semantic_receipt_sha256: sha256_bytes_hex(b"semantic receipt"),
            },
            reader_receipt: Some(QualificationDerivedStorageReaderReceiptV1 {
                schema: "pointbreak.change-reader-profile-receipt.v3".to_owned(),
                version: 3,
                receipt_sha256: sha256_bytes_hex(b"reader receipt"),
                content_sha256: sha256_bytes_hex(b"reader receipt content"),
            }),
            live_checkpoint: Some(QualificationDerivedStorageLiveCheckpointV1 {
                schema: "pointbreak.reader-projection-checkpoint.v1".to_owned(),
                version: 1,
                checkpoint_sha256: sha256_bytes_hex(checkpoint.as_bytes()),
                reader_receipt_sha256: sha256_bytes_hex(b"reader receipt"),
            }),
            sqlite_catalog: QualificationDerivedStorageCatalogV1 {
                catalog_sha256: canonical_sha256(&entries).expect("catalog hash"),
                entries,
            },
            carriers: vec![QualificationDerivedStorageCarrierV1 {
                role: QualificationDerivedStorageCarrierRoleV1::Database,
                relative_path_sha256: sha256_bytes_hex(b"cursor.sqlite3"),
                byte_count: 1,
                content_sha256: sha256_bytes_hex(b"database bytes"),
            }],
            bytes: QualificationDerivedStorageBytesV1 {
                database: 1,
                wal: 0,
                shared_memory: 0,
                temporary: 0,
            },
            forbidden_probes: QualificationDerivedStorageForbiddenProbeKindV1::ALL
                .into_iter()
                .map(|kind| QualificationDerivedStorageForbiddenProbeV1 {
                    kind,
                    sentinel_sha256: sha256_bytes_hex(format!("{kind:?}").as_bytes()),
                    found: false,
                })
                .collect(),
            witness_sha256: String::new(),
        };
        witness.refresh_sha256().expect("storage witness hash");
        witness
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::derived_access::lifecycle::{DerivedAccessLifecycle, LifecycleControl};
    use crate::session::derived_access::product_contract::DerivedAccessProfile;

    fn probe_input() -> QualificationDerivedStorageForbiddenProbeInputV1 {
        QualificationDerivedStorageForbiddenProbeInputV1::new(
            "PRIVATE SUMMARY SENTINEL",
            "PRIVATE PROSE SENTINEL",
            "PRIVATE PAYLOAD DOCUMENT SENTINEL",
            "/private/fixture/path",
        )
        .expect("probe input")
    }

    fn published_empty_generation() -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("root");
        DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            root.path(),
            "store:storage-witness-test",
        )
        .expect("lifecycle")
        .rebuild(|_| LifecycleControl::Continue)
        .expect("publish empty generation");
        root
    }

    fn published_carrier_fingerprints(store_root: &Path) -> Vec<(String, String)> {
        let layout = GenerationLayout::new(store_root).expect("layout");
        let publication = layout
            .current_publication()
            .expect("read publication")
            .expect("published generation");
        let mut files = Vec::new();
        collect_regular_files(
            layout.root(),
            &layout.generation(&publication.generation_id),
            &mut files,
        )
        .expect("inventory generation");
        collect_regular_files(
            layout.root(),
            &layout.root().join("publications"),
            &mut files,
        )
        .expect("inventory publications");
        let mut fingerprints = files
            .into_iter()
            .map(|(path, relative)| {
                (
                    relative,
                    sha256_bytes_hex(&std::fs::read(path).expect("read carrier")),
                )
            })
            .collect::<Vec<_>>();
        fingerprints.sort();
        fingerprints
    }

    #[test]
    fn published_generation_witness_is_hash_only_and_deterministic() {
        let root = published_empty_generation();
        let before = published_carrier_fingerprints(root.path());
        let first = capture_qualification_derived_storage_witness_v1(root.path(), &probe_input())
            .expect("capture first witness");
        let second = capture_qualification_derived_storage_witness_v1(root.path(), &probe_input())
            .expect("capture second witness");
        let after = published_carrier_fingerprints(root.path());

        first.validate().expect("validate witness");
        assert_eq!(first, second);
        assert_eq!(before, after, "witness capture mutated published carriers");
        assert!(first.bytes.database > 0);
        assert!(
            first
                .sqlite_catalog
                .entries
                .iter()
                .any(|entry| entry.name == "cursor_meta")
        );
        assert!(
            first
                .sqlite_catalog
                .entries
                .iter()
                .any(|entry| entry.name == "semantic_event_fact")
        );
        let json = serde_json::to_string(&first).expect("serialize witness");
        assert!(!json.contains(root.path().to_str().expect("utf8 root")));
        assert!(!json.contains("PRIVATE SUMMARY SENTINEL"));
        assert!(!json.contains("/private/fixture/path"));
    }

    #[test]
    fn forbidden_probe_detection_marks_a_selected_carrier() {
        let root = tempfile::tempdir().expect("root");
        let carrier = root.path().join("carrier");
        std::fs::write(
            &carrier,
            format!(
                "before PRIVATE PROSE SENTINEL {} after",
                root.path().display()
            ),
        )
        .expect("write carrier");
        let collected = collect_carriers(root.path(), root.path(), None, &probe_input())
            .expect("collect carriers");
        let (probes, probe_hits) = (collected.forbidden_probes, collected.probe_hits);
        let prose = probes
            .iter()
            .find(|probe| probe.kind == QualificationDerivedStorageForbiddenProbeKindV1::Prose)
            .expect("prose probe");
        assert!(prose.found);
        let store_root = probes
            .iter()
            .find(|probe| {
                probe.kind == QualificationDerivedStorageForbiddenProbeKindV1::StoreRootPath
            })
            .expect("store-root probe");
        assert!(store_root.found);
        assert!(probe_hits.contains(&(
            QualificationDerivedStorageForbiddenProbeKindV1::Prose,
            "carrier".to_owned()
        )));
        assert!(probe_hits.contains(&(
            QualificationDerivedStorageForbiddenProbeKindV1::StoreRootPath,
            "carrier".to_owned()
        )));
    }

    #[test]
    fn witness_validation_names_each_failing_condition() {
        let mut witness = QualificationDerivedStorageWitnessV1 {
            schema: QUALIFICATION_DERIVED_STORAGE_WITNESS_SCHEMA_V1.to_owned(),
            publication: QualificationDerivedStoragePublicationV1 {
                sequence: 1,
                generation_id_sha256: "a".repeat(64),
                descriptor_sha256: "b".repeat(64),
            },
            descriptor: QualificationDerivedStorageDescriptorV1 {
                schema: "pointbreak.derived-generation-descriptor.v2".to_owned(),
                profile: "sqlite-wal-bodyless-v1".to_owned(),
                epoch: 1,
                head_sequence: 1,
                store_id_sha256: "c".repeat(64),
                semantic_receipt_sha256: "d".repeat(64),
            },
            reader_receipt: None,
            live_checkpoint: None,
            sqlite_catalog: QualificationDerivedStorageCatalogV1 {
                entries: Vec::new(),
                catalog_sha256: canonical_sha256(
                    &Vec::<QualificationDerivedStorageCatalogEntryV1>::new(),
                )
                .expect("empty catalog hash"),
            },
            carriers: vec![QualificationDerivedStorageCarrierV1 {
                role: QualificationDerivedStorageCarrierRoleV1::Database,
                relative_path_sha256: "e".repeat(64),
                byte_count: 1,
                content_sha256: "f".repeat(64),
            }],
            bytes: QualificationDerivedStorageBytesV1 {
                database: 1,
                wal: 0,
                shared_memory: 0,
                temporary: 0,
            },
            forbidden_probes: QualificationDerivedStorageForbiddenProbeKindV1::ALL
                .iter()
                .map(|kind| QualificationDerivedStorageForbiddenProbeV1 {
                    kind: *kind,
                    sentinel_sha256: "1".repeat(64),
                    found: false,
                })
                .collect(),
            witness_sha256: String::new(),
        };
        witness.refresh_sha256().expect("witness hash");
        witness.validate().expect("well-formed witness validates");

        let mut found_probe = witness.clone();
        found_probe.forbidden_probes[0].found = true;
        found_probe.refresh_sha256().expect("witness hash");
        let error = found_probe.validate().expect_err("found probe rejects");
        assert!(
            error.contains("found forbidden fixture bytes")
                && error.contains(&format!("{:?}", witness.forbidden_probes[0].kind)),
            "unexpected error: {error}"
        );

        let mut empty_carriers = witness.clone();
        empty_carriers.carriers.clear();
        empty_carriers.refresh_sha256().expect("witness hash");
        let error = empty_carriers
            .validate()
            .expect_err("empty carriers reject");
        assert!(
            error.contains("selected no carriers"),
            "unexpected error: {error}"
        );

        let mut stale_hash = witness.clone();
        stale_hash.bytes.database = 2;
        let error = stale_hash.validate().expect_err("stale self-hash rejects");
        assert!(
            error.contains("self-hash drifted"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn publication_recheck_rejects_a_newer_selected_generation() {
        let root = published_empty_generation();
        let layout = GenerationLayout::new(root.path()).expect("layout");
        let expected = layout
            .current_publication()
            .expect("read publication")
            .expect("published generation");

        DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            root.path(),
            "store:storage-witness-test",
        )
        .expect("lifecycle")
        .rebuild(|_| LifecycleControl::Continue)
        .expect("publish newer generation");

        assert!(ensure_publication_is_current(root.path(), &expected).is_err());
    }

    #[test]
    fn carrier_snapshot_requires_identical_before_during_and_after_reads() {
        let stable = vec![("cursor.sqlite3", "sha256:one")];
        let advanced = vec![("cursor.sqlite3", "sha256:two")];
        assert!(carrier_snapshots_are_stable(&stable, &stable, &stable));
        assert!(!carrier_snapshots_are_stable(&stable, &advanced, &advanced));
        assert!(!carrier_snapshots_are_stable(&stable, &stable, &advanced));
    }

    #[test]
    fn stable_snapshot_refuses_a_carrier_created_between_reads() {
        let root = published_empty_generation();
        let selected = stable_selected_generation(root.path()).expect("selected generation");
        let mut hook_count = 0;
        let snapshot = stable_storage_snapshot_with_hook(
            root.path(),
            &selected,
            &probe_input(),
            |generation_root| {
                if hook_count == 0 {
                    std::fs::write(generation_root.join("transient-carrier"), b"changed")
                        .map_err(|error| error.to_string())?;
                }
                hook_count += 1;
                Ok(())
            },
        )
        .expect("capture changed generation");
        assert!(snapshot.is_none());
    }

    #[test]
    fn catalog_captures_full_index_shape_and_rejects_links() {
        let root = tempfile::tempdir().expect("root");
        let database = root.path().join("cursor.sqlite3");
        let connection = Connection::open(&database).expect("database");
        connection
            .execute_batch(
                "CREATE TABLE shape (id INTEGER PRIMARY KEY, value TEXT NOT NULL UNIQUE) STRICT;
                 CREATE INDEX shape_value_desc ON shape(value DESC);",
            )
            .expect("schema");
        let catalog = sqlite_catalog(&connection).expect("catalog");
        drop(connection);
        let shape = catalog
            .entries
            .iter()
            .find(|entry| entry.name == "shape")
            .expect("shape table");
        assert!(shape.strict);
        assert!(
            shape
                .indexes
                .iter()
                .any(|index| index.name == "shape_value_desc" && index.columns[0].descending)
        );

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&database, root.path().join("link")).expect("make link");
            assert!(collect_regular_files(root.path(), root.path(), &mut Vec::new()).is_err());
        }
    }
}
