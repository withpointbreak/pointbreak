//! Validated content access over a byte [`ContentStore`].
//!
//! This is the one place that owns "validate then store" and "fetch then
//! validate" for both object artifacts and note bodies — the content-hash
//! validation half that used to be split across the object and body modules and
//! the raw `std::fs` reads under them. The content-hash math itself stays in
//! those modules; this wrapper owns the flow, so one place sits above the byte
//! store and a backend can never re-encode a blob and shift the bytes a digest
//! is validated over.

use std::path::Path;

use super::backend::{ContentStore, LocalContentStore, StoreBackend};
use super::body_artifact::parse_note_body_artifact;
use super::object_artifact::{ObjectArtifact, decode_and_validate_object_artifact};
use crate::error::{Result, ShoreError};
use crate::model::ObjectId;
use crate::storage::{CreateFileOutcome, RemoveOutcome};

/// Validated put/get for content-addressed artifacts over a byte
/// [`ContentStore`]. Built per operation today; a later change injects the
/// backend handle directly.
pub(crate) struct ContentArtifacts {
    store: Box<dyn ContentStore>,
}

impl ContentArtifacts {
    /// Build over the file content store rooted at `store_dir`.
    pub(crate) fn local(store_dir: &Path) -> Self {
        Self {
            store: Box::new(LocalContentStore::new(store_dir)),
        }
    }

    /// Build over the content store a resolved backend yields. The constructor
    /// production consumers use, so the resolved backend flows through; `local`
    /// stays for `store_dir`-keyed callers and direct file-store access.
    pub(crate) fn from_backend(backend: &StoreBackend) -> Self {
        Self {
            store: backend.content_store(),
        }
    }

    // --- object artifacts ---

    /// Store an object artifact's bytes at `content_ref`, deduping on a
    /// snapshot-content match: a byte-identical artifact already present returns
    /// the existing one; a different artifact under the same locator is a loud
    /// conflict.
    pub(crate) fn put_object(
        &self,
        content_ref: &str,
        bytes: &[u8],
        artifact: ObjectArtifact,
    ) -> Result<ObjectArtifact> {
        match self.store.put_once(content_ref, bytes)? {
            CreateFileOutcome::Created => Ok(artifact),
            CreateFileOutcome::AlreadyExists => {
                // The locator already holds a blob, so this read expects it
                // present — an absent blob here is a write race, not the
                // "import the referenced artifacts" case the read surfaces map.
                let existing_bytes = self.store.get(content_ref)?;
                let existing = decode_and_validate_object_artifact(&existing_bytes)?;
                if existing.snapshot == artifact.snapshot {
                    Ok(existing)
                } else {
                    Err(ShoreError::Message(format!(
                        "object artifact conflict for {}",
                        artifact.snapshot.object_id.as_str()
                    )))
                }
            }
        }
    }

    /// Fetch an object artifact's stored bytes, mapping an absent blob to the
    /// canonical "import referenced artifacts" guidance.
    pub(crate) fn read_object_bytes(
        &self,
        content_ref: &str,
        object_id: &ObjectId,
    ) -> Result<Vec<u8>> {
        match self.store.get_if_exists(content_ref)? {
            Some(bytes) => Ok(bytes),
            None => Err(missing_object_artifact(object_id)),
        }
    }

    /// Fetch an object artifact's stored bytes, or `None` when absent (the
    /// resolver tries the next store on a miss).
    pub(crate) fn read_object_bytes_if_exists(&self, content_ref: &str) -> Result<Option<Vec<u8>>> {
        self.store.get_if_exists(content_ref)
    }

    // --- note bodies ---

    /// Fetch and parse a note body artifact, mapping an absent blob to the
    /// canonical "import referenced artifacts" guidance.
    pub(crate) fn read_note_body(&self, content_ref: &str) -> Result<String> {
        match self.store.get_if_exists(content_ref)? {
            Some(bytes) => Ok(parse_note_body_artifact(&bytes)?.body),
            None => Err(ShoreError::Message(format!(
                "missing artifact {content_ref}; import referenced artifacts before reading"
            ))),
        }
    }

    // --- content-addressed maintenance (the compact sweep) ---

    /// Every stored locator under `prefix`, in deterministic order.
    pub(crate) fn list_refs(&self, prefix: &str) -> Result<Vec<String>> {
        self.store.list(prefix)
    }

    /// The stored bytes for `content_ref`, or `None` when absent.
    pub(crate) fn get_if_exists(&self, content_ref: &str) -> Result<Option<Vec<u8>>> {
        self.store.get_if_exists(content_ref)
    }

    /// Plain unlink of the blob at `content_ref`. The re-hash-before-erase floor
    /// is the caller's, above this.
    pub(crate) fn remove(&self, content_ref: &str) -> Result<RemoveOutcome> {
        self.store.remove(content_ref)
    }
}

fn missing_object_artifact(object_id: &ObjectId) -> ShoreError {
    ShoreError::Message(format!(
        "missing artifact for snapshot {}; import referenced artifacts before reading",
        object_id.as_str()
    ))
}
