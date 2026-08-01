//! Immutable generation paths and atomic current-generation publication.
#![cfg_attr(not(test), allow(dead_code))]

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::product_contract::DerivedAccessProfile;
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
use crate::session::store::backend::JournalChangeStamp;

const GENERATION_DESCRIPTOR: &str = "generation.json";
const GENERATION_SCHEMA: &str = "pointbreak.derived-access-generation.v2";
const LEGACY_GENERATION_SCHEMA: &str = "pointbreak.derived-access-generation.v1";
const PUBLICATION_SCHEMA: &str = "pointbreak.derived-access-publication.v1";
const PROGRESS_SCHEMA: &str = "pointbreak.derived-access-generation-progress.v2";
const PROGRESS_INTERVAL: usize = 256;
const REBUILD_LOCK_FILE: &str = ".pointbreak-derived.rebuild.lock";
const GENERATION_LEASE_PREFIX: &str = ".pointbreak-derived.generation-lease-";
static UNIQUE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub(crate) struct GenerationLayout {
    store_root: PathBuf,
    root: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GenerationDescriptor {
    schema: String,
    pub(crate) generation_id: String,
    pub(crate) store_id: String,
    pub(crate) profile: DerivedAccessProfile,
    pub(crate) epoch: u64,
    pub(crate) head_sequence: u64,
    pub(crate) authority_stamp: JournalChangeStamp,
    pub(crate) semantic_receipt: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GenerationPublication {
    schema: String,
    pub(crate) sequence: u64,
    pub(crate) generation_id: String,
    pub(crate) descriptor_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GenerationProgressPhase {
    CursorPopulation,
    ProjectionPopulation,
    StrictVerification,
    Finalizing,
}

impl GenerationProgressPhase {
    const fn order(self) -> u8 {
        match self {
            Self::CursorPopulation => 1,
            Self::ProjectionPopulation => 2,
            Self::StrictVerification => 3,
            Self::Finalizing => 4,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GenerationProgress {
    schema: String,
    pub(crate) phase: GenerationProgressPhase,
    pub(crate) completed: usize,
    pub(crate) total: usize,
    pub(crate) bytes_processed: u64,
    pub(crate) elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) estimated_remaining_ms: Option<u64>,
}

impl GenerationProgress {
    pub(crate) fn new(
        phase: GenerationProgressPhase,
        completed: usize,
        total: usize,
        bytes_processed: u64,
        elapsed_ms: u64,
        estimated_remaining_ms: Option<u64>,
    ) -> Self {
        Self {
            schema: PROGRESS_SCHEMA.to_owned(),
            phase,
            completed,
            total,
            bytes_processed,
            elapsed_ms,
            estimated_remaining_ms,
        }
    }
}

#[derive(Debug)]
pub(crate) struct GenerationReadLease {
    file: File,
}

#[derive(Debug)]
pub(crate) struct RebuildLease {
    file: File,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GenerationReclaim {
    pub(crate) reclaimed: Vec<String>,
    pub(crate) retained_by_readers: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum GenerationError {
    #[error("invalid derived generation identifier {0:?}")]
    InvalidGenerationId(String),
    #[error("derived generation I/O failed at {path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error("derived generation metadata at {path} is invalid: {message}")]
    Metadata { path: PathBuf, message: String },
    #[error("derived generation metadata at {path} uses legacy schema {schema}")]
    LegacyDescriptor { path: PathBuf, schema: String },
    #[error("derived generation publication sequence overflow")]
    SequenceOverflow,
    #[error("another derived generation rebuild is already running")]
    RebuildBusy,
    #[error("a derived generation is still in use")]
    GenerationInUse,
    #[error("the current derived generation changed repeatedly while opening")]
    PublicationUnstable,
}

impl GenerationLayout {
    pub(crate) fn new(store_root: &Path) -> Self {
        Self {
            store_root: store_root.to_path_buf(),
            root: store_root.join(super::sqlite::DERIVED_SIDECAR_DIRECTORY),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn staging(&self, generation_id: &str) -> PathBuf {
        self.root.join("staging").join(generation_id)
    }

    pub(crate) fn generation(&self, generation_id: &str) -> PathBuf {
        self.root.join("generations").join(generation_id)
    }

    pub(crate) fn ensure_scaffold(&self) -> Result<(), GenerationError> {
        for directory in [
            self.root.join("staging"),
            self.root.join("generations"),
            self.root.join("publications"),
            self.root.join("publication-staging"),
        ] {
            std::fs::create_dir_all(&directory).map_err(|error| io_error(&directory, error))?;
        }
        Ok(())
    }

    pub(crate) fn try_rebuild_lease(&self) -> Result<RebuildLease, GenerationError> {
        std::fs::create_dir_all(&self.store_root)
            .map_err(|error| io_error(&self.store_root, error))?;
        let path = self.store_root.join(REBUILD_LOCK_FILE);
        let file = open_lock_file(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(RebuildLease { file }),
            Err(std::fs::TryLockError::WouldBlock) => Err(GenerationError::RebuildBusy),
            Err(std::fs::TryLockError::Error(error)) => Err(io_error(&path, error)),
        }
    }

    pub(crate) fn acquire_read_lease(
        &self,
        generation_id: &str,
    ) -> Result<GenerationReadLease, GenerationError> {
        validate_generation_id(generation_id)?;
        let path = self.generation_lease_path(generation_id);
        let file = open_lock_file(&path)?;
        file.lock_shared().map_err(|error| io_error(&path, error))?;
        Ok(GenerationReadLease { file })
    }

    pub(crate) fn next_generation(&self) -> Result<(u64, String), GenerationError> {
        let next = self.current_publication()?.map_or(Ok(1), |publication| {
            publication
                .sequence
                .checked_add(1)
                .ok_or(GenerationError::SequenceOverflow)
        })?;
        let unique = UNIQUE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Ok((
            next,
            format!(
                "g-{next:020}-{}-{timestamp:032x}-{unique}",
                std::process::id()
            ),
        ))
    }

    pub(crate) fn record_progress(
        &self,
        generation_id: &str,
        progress: GenerationProgress,
    ) -> Result<(), GenerationError> {
        self.record_progress_with_hook(generation_id, progress, |_| {})
    }

    fn record_progress_with_hook(
        &self,
        generation_id: &str,
        progress: GenerationProgress,
        mut before_publish: impl FnMut(&Path),
    ) -> Result<(), GenerationError> {
        if progress.completed != 0
            && progress.completed != progress.total
            && !progress.completed.is_multiple_of(PROGRESS_INTERVAL)
        {
            return Ok(());
        }
        let directory = self.staging(generation_id).join("progress");
        std::fs::create_dir_all(&directory).map_err(|error| io_error(&directory, error))?;
        let phase_order = progress.phase.order();
        let completed = progress.completed;
        let value = serde_json::to_value(progress)
            .map_err(|error| metadata_error(&directory, error.to_string()))?;
        let bytes = canonical_json_bytes(&value)
            .map_err(|error| metadata_error(&directory, error.to_string()))?;
        let published = directory.join(format!("{phase_order:02}-{completed:020}.json"));
        let temporary = directory.join(format!(
            ".{phase_order:02}-{completed:020}.{}-{}.tmp",
            std::process::id(),
            UNIQUE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        // The final `.json` name is the progress state machine's publication
        // marker. Opening that name before writing made a concurrent status
        // reader interpret an empty/partial file as corrupt generation state.
        // Readers ignore `.tmp`; publish only after the bytes and file metadata
        // are durable. The store-scoped rebuild lease guarantees one writer for
        // a generation, so this rename cannot replace a competing progress row.
        write_new_synced(&temporary, &bytes)?;
        before_publish(&temporary);
        if let Err(error) = std::fs::rename(&temporary, &published) {
            let _ = std::fs::remove_file(&temporary);
            return Err(io_error(&temporary, error));
        }
        Ok(())
    }

    pub(crate) fn staging_progress(&self) -> Result<Option<GenerationProgress>, GenerationError> {
        self.staging_progress_with_hook(|_| {})
    }

    fn staging_progress_with_hook(
        &self,
        mut before_progress_read: impl FnMut(&Path),
    ) -> Result<Option<GenerationProgress>, GenerationError> {
        let staging = self.root.join("staging");
        let Some(generations) = read_directory_if_present(&staging)? else {
            return Ok(None);
        };
        let mut latest: Option<GenerationProgress> = None;
        for generation in generations {
            let progress_root = generation.path().join("progress");
            let Some(entries) = read_directory_if_present(&progress_root)? else {
                continue;
            };
            for entry in entries {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                before_progress_read(&path);
                let Some(bytes) = read_file_if_present(&path)? else {
                    continue;
                };
                let progress: GenerationProgress = serde_json::from_slice(&bytes)
                    .map_err(|error| metadata_error(&path, error.to_string()))?;
                if progress.schema != PROGRESS_SCHEMA || progress.completed > progress.total {
                    return Err(metadata_error(
                        &path,
                        "invalid generation progress".to_owned(),
                    ));
                }
                if latest.as_ref().is_none_or(|observed| {
                    (progress.phase, progress.completed) > (observed.phase, observed.completed)
                }) {
                    latest = Some(progress);
                }
            }
        }
        Ok(latest)
    }

    pub(crate) fn clear_progress(&self, generation_id: &str) -> Result<(), GenerationError> {
        let directory = self.staging(generation_id).join("progress");
        if directory.exists() {
            std::fs::remove_dir_all(&directory).map_err(|error| io_error(&directory, error))?;
        }
        Ok(())
    }

    pub(crate) fn write_descriptor(
        &self,
        staging_root: &Path,
        descriptor: &GenerationDescriptor,
    ) -> Result<String, GenerationError> {
        validate_generation_id(&descriptor.generation_id)?;
        let value = serde_json::to_value(descriptor)
            .map_err(|error| metadata_error(staging_root, error.to_string()))?;
        let bytes = canonical_json_bytes(&value)
            .map_err(|error| metadata_error(staging_root, error.to_string()))?;
        let path = staging_root.join(GENERATION_DESCRIPTOR);
        write_new_synced(&path, &bytes)?;
        Ok(sha256_bytes_hex(&bytes))
    }

    pub(crate) fn promote_staging(&self, generation_id: &str) -> Result<PathBuf, GenerationError> {
        validate_generation_id(generation_id)?;
        let staging = self.staging(generation_id);
        let generation = self.generation(generation_id);
        std::fs::rename(&staging, &generation).map_err(|error| io_error(&staging, error))?;
        Ok(generation)
    }

    pub(crate) fn publish(
        &self,
        publication: &GenerationPublication,
    ) -> Result<PathBuf, GenerationError> {
        validate_generation_id(&publication.generation_id)?;
        let value = serde_json::to_value(publication)
            .map_err(|error| metadata_error(&self.root, error.to_string()))?;
        let bytes = canonical_json_bytes(&value)
            .map_err(|error| metadata_error(&self.root, error.to_string()))?;
        let name = publication_file_name(publication.sequence, &publication.generation_id);
        let temporary = self.root.join("publication-staging").join(format!(
            "{name}.{}-{}.tmp",
            std::process::id(),
            UNIQUE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let published = self.root.join("publications").join(name);
        write_new_synced(&temporary, &bytes)?;
        std::fs::rename(&temporary, &published).map_err(|error| io_error(&temporary, error))?;
        Ok(published)
    }

    pub(crate) fn retire_prior_publications(
        &self,
        current_sequence: u64,
    ) -> Result<(), GenerationError> {
        let directory = self.root.join("publications");
        for entry in read_directory(&directory)? {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let publication = read_publication(&path)?;
            if publication.sequence < current_sequence {
                std::fs::remove_file(&path).map_err(|error| io_error(&path, error))?;
            }
        }
        Ok(())
    }

    pub(crate) fn reclaim_inactive_generations(
        &self,
        current_generation_id: &str,
    ) -> Result<GenerationReclaim, GenerationError> {
        validate_generation_id(current_generation_id)?;
        // A delayed reader can keep an already-reclaimed generation's lease
        // inode alive until it observes the newer publication and retries.
        // Sweep before and after reclaim so those store-root lock files are
        // eventually collected without ever unlinking a lease that protects a
        // live generation.
        self.sweep_orphaned_generation_leases()?;
        let directory = self.root.join("generations");
        let mut receipt = GenerationReclaim::default();
        for entry in read_directory(&directory)? {
            if !entry
                .file_type()
                .map_err(|error| io_error(&entry.path(), error))?
                .is_dir()
            {
                continue;
            }
            let generation_id = entry.file_name().to_string_lossy().into_owned();
            validate_generation_id(&generation_id)?;
            if generation_id == current_generation_id {
                continue;
            }
            let lease_path = self.generation_lease_path(&generation_id);
            let lease = open_lock_file(&lease_path)?;
            match lease.try_lock() {
                Ok(()) => {
                    let generation = entry.path();
                    std::fs::remove_dir_all(&generation)
                        .map_err(|error| io_error(&generation, error))?;
                    // Keep the lease path stable. A reader may already have
                    // opened this inode while waiting for the exclusive lock;
                    // after it acquires the lock, the lifecycle publication
                    // recheck redirects it to the new generation. The orphan
                    // sweep removes the empty lock file only after the
                    // generation directory is absent and no reader holds it.
                    receipt.reclaimed.push(generation_id);
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    receipt.retained_by_readers.push(generation_id);
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    return Err(io_error(&lease_path, error));
                }
            }
        }
        self.sweep_orphaned_generation_leases()?;
        receipt.reclaimed.sort();
        receipt.retained_by_readers.sort();
        Ok(receipt)
    }

    fn sweep_orphaned_generation_leases(&self) -> Result<(), GenerationError> {
        let Some(entries) = read_directory_if_present(&self.store_root)? else {
            return Ok(());
        };
        for entry in entries {
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(generation_id) = name
                .strip_prefix(GENERATION_LEASE_PREFIX)
                .and_then(|name| name.strip_suffix(".lock"))
            else {
                continue;
            };
            if validate_generation_id(generation_id).is_err()
                || self.generation(generation_id).exists()
            {
                continue;
            }
            let lease = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .map_err(|error| io_error(&path, error))?;
            match lease.try_lock() {
                Ok(()) => {
                    let _ = lease.unlock();
                    drop(lease);
                    match std::fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(io_error(&path, error)),
                    }
                }
                Err(std::fs::TryLockError::WouldBlock) => {}
                Err(std::fs::TryLockError::Error(error)) => {
                    return Err(io_error(&path, error));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn current_publication(
        &self,
    ) -> Result<Option<GenerationPublication>, GenerationError> {
        let directory = self.root.join("publications");
        if !directory.exists() {
            return Ok(None);
        }
        let mut paths = Vec::new();
        for entry in read_directory(&directory)? {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("json") {
                paths.push(path);
            }
        }
        paths.sort();
        let Some(current_path) = paths.pop() else {
            return Ok(None);
        };
        let current_sequence = publication_sequence_from_path(&current_path)?;
        let prior_sequence = paths
            .last()
            .map(|path| publication_sequence_from_path(path))
            .transpose()?;
        if prior_sequence == Some(current_sequence) {
            return Err(metadata_error(
                &directory,
                format!("multiple publications claim sequence {current_sequence}"),
            ));
        }
        read_publication(&current_path).map(Some)
    }

    pub(crate) fn descriptor(
        &self,
        publication: &GenerationPublication,
    ) -> Result<GenerationDescriptor, GenerationError> {
        let generation = self.generation(&publication.generation_id);
        let path = generation.join(GENERATION_DESCRIPTOR);
        let bytes = std::fs::read(&path).map_err(|error| io_error(&path, error))?;
        if sha256_bytes_hex(&bytes) != publication.descriptor_sha256 {
            return Err(metadata_error(
                &path,
                "descriptor hash does not match publication".to_owned(),
            ));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| metadata_error(&path, error.to_string()))?;
        let schema = value
            .get("schema")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| metadata_error(&path, "descriptor schema is absent".to_owned()))?;
        if schema == LEGACY_GENERATION_SCHEMA {
            return Err(GenerationError::LegacyDescriptor {
                path,
                schema: schema.to_owned(),
            });
        }
        let descriptor: GenerationDescriptor = serde_json::from_value(value)
            .map_err(|error| metadata_error(&path, error.to_string()))?;
        if descriptor.schema != GENERATION_SCHEMA
            || descriptor.generation_id != publication.generation_id
        {
            return Err(metadata_error(
                &path,
                "descriptor schema or generation identity mismatch".to_owned(),
            ));
        }
        Ok(descriptor)
    }

    pub(crate) fn discard_staging(&self, generation_id: &str) -> Result<(), GenerationError> {
        let path = self.staging(generation_id);
        if path.exists() {
            std::fs::remove_dir_all(&path).map_err(|error| io_error(&path, error))?;
        }
        Ok(())
    }

    pub(crate) fn discard_all_staging(&self) -> Result<(), GenerationError> {
        for path in [
            self.root.join("staging"),
            self.root.join("publication-staging"),
        ] {
            if path.exists() {
                std::fs::remove_dir_all(&path).map_err(|error| io_error(&path, error))?;
            }
            std::fs::create_dir_all(&path).map_err(|error| io_error(&path, error))?;
        }
        Ok(())
    }

    pub(crate) fn quarantine(&self, reason: &str) -> Result<PathBuf, GenerationError> {
        let reason_path = self.root.join("quarantine-reason.txt");
        if self.root.exists() {
            let _ = std::fs::write(&reason_path, reason.as_bytes());
        }
        let destination = self.store_root.join(format!(
            "{}{}-{}",
            super::sqlite::DERIVED_QUARANTINE_PREFIX,
            std::process::id(),
            UNIQUE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::rename(&self.root, &destination).map_err(|error| io_error(&self.root, error))?;
        Ok(destination)
    }

    pub(crate) fn retire(&self) -> Result<Option<PathBuf>, GenerationError> {
        if !self.root.exists() {
            return Ok(None);
        }
        let destination = self.store_root.join(format!(
            ".pointbreak-derived.retired-{}-{}",
            std::process::id(),
            UNIQUE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::rename(&self.root, &destination).map_err(|error| io_error(&self.root, error))?;
        Ok(Some(destination))
    }

    pub(crate) fn delete(&self) -> Result<(), GenerationError> {
        self.remove_root_and_generation_leases(&self.root)
    }

    pub(crate) fn purge_disposable_root(&self, path: &Path) -> Result<(), GenerationError> {
        let parent = path.parent();
        let name = path.file_name().and_then(|value| value.to_str());
        let allowed = parent == Some(self.store_root.as_path())
            && name.is_some_and(|name| {
                name.starts_with(super::sqlite::DERIVED_QUARANTINE_PREFIX)
                    || name.starts_with(".pointbreak-derived.retired-")
            });
        if !allowed {
            return Err(GenerationError::Metadata {
                path: path.to_path_buf(),
                message: "refused to purge a path outside the exact disposable-root namespace"
                    .to_owned(),
            });
        }

        self.remove_root_and_generation_leases(path)
    }

    fn remove_root_and_generation_leases(&self, path: &Path) -> Result<(), GenerationError> {
        let lease_paths = self.generation_lease_paths_under(path)?;
        let mut leases = Vec::with_capacity(lease_paths.len());
        for lease_path in &lease_paths {
            let lease = open_lock_file(lease_path)?;
            match lease.try_lock() {
                Ok(()) => leases.push(lease),
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(GenerationError::GenerationInUse);
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    return Err(io_error(lease_path, error));
                }
            }
        }
        if path.exists() {
            std::fs::remove_dir_all(path).map_err(|error| io_error(path, error))?;
        }
        for (lease, lease_path) in leases.into_iter().zip(lease_paths) {
            let _ = lease.unlock();
            drop(lease);
            let _ = std::fs::remove_file(lease_path);
        }
        Ok(())
    }

    fn generation_lease_paths_under(&self, root: &Path) -> Result<Vec<PathBuf>, GenerationError> {
        let generations = root.join("generations");
        if !generations.exists() {
            return Ok(Vec::new());
        }
        let mut paths = Vec::new();
        for entry in read_directory(&generations)? {
            if !entry
                .file_type()
                .map_err(|error| io_error(&entry.path(), error))?
                .is_dir()
            {
                continue;
            }
            let generation_id = entry.file_name().to_string_lossy().into_owned();
            if validate_generation_id(&generation_id).is_ok() {
                paths.push(self.generation_lease_path(&generation_id));
            }
        }
        paths.sort();
        Ok(paths)
    }

    pub(crate) fn generation_lease_path(&self, generation_id: &str) -> PathBuf {
        self.store_root
            .join(format!("{GENERATION_LEASE_PREFIX}{generation_id}.lock"))
    }
}

impl Drop for GenerationReadLease {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

impl Drop for RebuildLease {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

impl GenerationDescriptor {
    pub(crate) fn new(
        generation_id: impl Into<String>,
        store_id: impl Into<String>,
        profile: DerivedAccessProfile,
        epoch: u64,
        head_sequence: u64,
        authority_stamp: JournalChangeStamp,
        semantic_receipt: impl Into<String>,
    ) -> Self {
        Self {
            schema: GENERATION_SCHEMA.to_owned(),
            generation_id: generation_id.into(),
            store_id: store_id.into(),
            profile,
            epoch,
            head_sequence,
            authority_stamp,
            semantic_receipt: semantic_receipt.into(),
        }
    }
}

impl GenerationPublication {
    pub(crate) fn new(
        sequence: u64,
        generation_id: impl Into<String>,
        descriptor_sha256: impl Into<String>,
    ) -> Self {
        Self {
            schema: PUBLICATION_SCHEMA.to_owned(),
            sequence,
            generation_id: generation_id.into(),
            descriptor_sha256: descriptor_sha256.into(),
        }
    }
}

fn read_publication(path: &Path) -> Result<GenerationPublication, GenerationError> {
    let bytes = std::fs::read(path).map_err(|error| io_error(path, error))?;
    let publication: GenerationPublication =
        serde_json::from_slice(&bytes).map_err(|error| metadata_error(path, error.to_string()))?;
    if publication.schema != PUBLICATION_SCHEMA {
        return Err(metadata_error(
            path,
            "publication schema mismatch".to_owned(),
        ));
    }
    validate_generation_id(&publication.generation_id)?;
    if path.file_name().and_then(|value| value.to_str())
        != Some(&publication_file_name(
            publication.sequence,
            &publication.generation_id,
        ))
    {
        return Err(metadata_error(
            path,
            "publication file name does not match its body".to_owned(),
        ));
    }
    Ok(publication)
}

fn publication_file_name(sequence: u64, generation_id: &str) -> String {
    format!("{sequence:020}-{generation_id}.json")
}

fn publication_sequence_from_path(path: &Path) -> Result<u64, GenerationError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| metadata_error(path, "publication name is not Unicode".to_owned()))?;
    let sequence = name
        .get(..20)
        .ok_or_else(|| metadata_error(path, "publication sequence is absent".to_owned()))?;
    sequence
        .parse()
        .map_err(|error| metadata_error(path, format!("invalid publication sequence: {error}")))
}

fn validate_generation_id(generation_id: &str) -> Result<(), GenerationError> {
    if generation_id.is_empty()
        || !generation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(GenerationError::InvalidGenerationId(
            generation_id.to_owned(),
        ));
    }
    Ok(())
}

fn read_directory(path: &Path) -> Result<Vec<std::fs::DirEntry>, GenerationError> {
    std::fs::read_dir(path)
        .map_err(|error| io_error(path, error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| io_error(path, error))
}

fn read_directory_if_present(
    path: &Path,
) -> Result<Option<Vec<std::fs::DirEntry>>, GenerationError> {
    match std::fs::read_dir(path) {
        Ok(entries) => entries
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
            .map_err(|error| io_error(path, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(path, error)),
    }
}

fn read_file_if_present(path: &Path) -> Result<Option<Vec<u8>>, GenerationError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(path, error)),
    }
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), GenerationError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| io_error(path, error))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| io_error(path, error))
}

fn open_lock_file(path: &Path) -> Result<File, GenerationError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    }
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|error| io_error(path, error))
}

fn io_error(path: &Path, error: std::io::Error) -> GenerationError {
    GenerationError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

fn metadata_error(path: &Path, message: String) -> GenerationError {
    GenerationError::Metadata {
        path: path.to_path_buf(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn staging_progress_treats_concurrent_cleanup_as_absence() {
        let temp = TempDir::new().unwrap();
        let layout = GenerationLayout::new(temp.path());
        layout.ensure_scaffold().unwrap();
        layout
            .record_progress(
                "g-test",
                GenerationProgress::new(
                    GenerationProgressPhase::CursorPopulation,
                    0,
                    1,
                    0,
                    0,
                    None,
                ),
            )
            .unwrap();
        let mut removed = false;

        let progress = layout
            .staging_progress_with_hook(|path| {
                if !removed {
                    std::fs::remove_file(path).unwrap();
                    removed = true;
                }
            })
            .unwrap();

        assert!(removed);
        assert_eq!(progress, None);
    }

    #[test]
    fn staging_progress_never_observes_an_unpublished_progress_file() {
        let temp = TempDir::new().unwrap();
        let layout = GenerationLayout::new(temp.path());
        layout.ensure_scaffold().unwrap();
        let mut observed_before_publish = None;

        layout
            .record_progress_with_hook(
                "g-test",
                GenerationProgress::new(
                    GenerationProgressPhase::CursorPopulation,
                    0,
                    1,
                    0,
                    0,
                    None,
                ),
                |temporary| {
                    assert_eq!(
                        temporary.extension().and_then(|value| value.to_str()),
                        Some("tmp")
                    );
                    observed_before_publish = Some(layout.staging_progress().unwrap());
                },
            )
            .unwrap();

        assert_eq!(observed_before_publish, Some(None));
        assert_eq!(
            layout.staging_progress().unwrap(),
            Some(GenerationProgress::new(
                GenerationProgressPhase::CursorPopulation,
                0,
                1,
                0,
                0,
                None,
            ))
        );
    }
}
