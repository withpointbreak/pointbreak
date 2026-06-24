//! The durable storage layer's byte-oriented backend traits.
//!
//! Two narrow, object-safe traits sit below the event and content wrappers.
//! [`Journal`] is append-with-dedup over opaque event bytes, keyed by a logical
//! idempotency key; [`ContentStore`] is content-addressed access over opaque
//! blobs shared by object artifacts and note bodies. Both deal only in `&[u8]`,
//! never typed records, so a backend can never re-serialize a record and shift
//! the bytes a digest is validated over — the co-signature classification and the
//! content-hash validation live entirely in the wrappers above.

mod local;

use std::fmt::Debug;
use std::path::PathBuf;

pub(crate) use local::{LocalContentStore, LocalJournal};

use crate::error::Result;
use crate::storage::{CreateFileOutcome, RemoveOutcome};

/// The closed set of durable-storage backends, and the one place that dispatches
/// to a concrete impl. A resolution carries a `StoreBackend` handle and the
/// event/content wrappers are built from it, so the selection made once at the
/// resolve choke point flows to every consumer without threading a path.
///
/// `Local` is the only variant today; a future file-shaped backend adds a
/// variant here and every consumer follows with no change. Deliberately **not**
/// `Eq`/`PartialEq`: no resolution is ever compared whole, and a later
/// injection-only in-memory variant would not be comparable.
#[derive(Clone, Debug)]
pub(crate) enum StoreBackend {
    /// The default file backend, wrapping the resolved store directory.
    Local(PathBuf),
}

impl StoreBackend {
    /// A fresh journal handle for this backend.
    pub(crate) fn journal(&self) -> Box<dyn Journal> {
        match self {
            StoreBackend::Local(store_dir) => Box::new(LocalJournal::new(store_dir)),
        }
    }

    /// A fresh content-store handle for this backend.
    pub(crate) fn content_store(&self) -> Box<dyn ContentStore> {
        match self {
            StoreBackend::Local(store_dir) => Box::new(LocalContentStore::new(store_dir)),
        }
    }
}

/// One listed event: its opaque bytes plus the backend's content-address digest
/// for it (the `sha256` of the logical idempotency key — the file backend's
/// filename stem). The digest lets the wrapper confirm the decoded event's key
/// hashes to where the backend stored it, catching a blob that was relocated or
/// renamed away from its content-addressed home.
#[derive(Clone, Debug)]
pub(crate) struct JournalEntry {
    pub(crate) key_digest: String,
    pub(crate) bytes: Vec<u8>,
}

/// Append-with-dedup over opaque event bytes, keyed by the logical idempotency
/// key. Append-only — there is no remove (content removal targets the content
/// store, never the journal). The `Debug` supertrait lets a wrapper hold the
/// trait object in a `#[derive(Debug)]` struct.
pub(crate) trait Journal: Debug {
    /// Store the event for `idempotency_key` only if absent, atomically and safe
    /// against a concurrent writer. Reports whether the bytes were written or an
    /// entry already existed; an existing entry is never overwritten.
    fn create_event_once(&self, idempotency_key: &str, bytes: &[u8]) -> Result<CreateFileOutcome>;

    /// The stored bytes for `idempotency_key`, or `None` when absent.
    fn read_event_bytes(&self, idempotency_key: &str) -> Result<Option<Vec<u8>>>;

    /// Whether an event is stored for `idempotency_key`.
    fn event_exists(&self, idempotency_key: &str) -> Result<bool>;

    /// Every stored event, paired with its content-address digest, in a
    /// deterministic order. The order is part of the contract: the projection
    /// folds events in this order, so it must be stable across backends. The
    /// per-entry digest lets the wrapper verify each blob still sits at its
    /// content-addressed home.
    fn list_event_entries(&self) -> Result<Vec<JournalEntry>>;
}

/// Content-addressed access over opaque blobs, shared by object artifacts and
/// note bodies. A `content_ref` is a store-relative locator
/// (`artifacts/objects/<hash>.json`, `artifacts/notes/<hash>.json`).
pub(crate) trait ContentStore: Debug {
    /// Store `bytes` at `content_ref` only if absent, atomically. Reports whether
    /// the bytes were written or a blob already existed.
    fn put_once(&self, content_ref: &str, bytes: &[u8]) -> Result<CreateFileOutcome>;

    /// The stored bytes for `content_ref`; errors if absent.
    fn get(&self, content_ref: &str) -> Result<Vec<u8>>;

    /// The stored bytes for `content_ref`, or `None` when absent.
    fn get_if_exists(&self, content_ref: &str) -> Result<Option<Vec<u8>>>;

    /// Remove the blob at `content_ref`. A plain unlink: any re-hash-before-erase
    /// floor is the caller's, above this. Reports removed vs already-absent.
    fn remove(&self, content_ref: &str) -> Result<RemoveOutcome>;

    /// Every stored locator under `prefix`, in a deterministic order.
    fn list(&self, prefix: &str) -> Result<Vec<String>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_backends() -> (tempfile::TempDir, LocalJournal, LocalContentStore) {
        let root = tempfile::tempdir().unwrap();
        let store_dir = root.path().join(".shore/data");
        let journal = LocalJournal::new(&store_dir);
        let content = LocalContentStore::new(&store_dir);
        (root, journal, content)
    }

    #[test]
    fn create_event_once_is_create_then_already_exists_without_overwriting() {
        let (_root, journal, _content) = local_backends();
        let key = "review_initialized:journal:default:work:default";

        assert_eq!(
            journal.create_event_once(key, b"first").unwrap(),
            CreateFileOutcome::Created
        );
        assert_eq!(
            journal.create_event_once(key, b"second").unwrap(),
            CreateFileOutcome::AlreadyExists
        );
        assert_eq!(
            journal.read_event_bytes(key).unwrap(),
            Some(b"first".to_vec())
        );
    }

    #[test]
    fn journal_read_and_exists_resolve_by_logical_key() {
        let (_root, journal, _content) = local_backends();
        let key = "some:idempotency:key";

        assert!(!journal.event_exists(key).unwrap());
        assert_eq!(journal.read_event_bytes(key).unwrap(), None);

        journal.create_event_once(key, b"bytes").unwrap();

        assert!(journal.event_exists(key).unwrap());
        assert_eq!(
            journal.read_event_bytes(key).unwrap(),
            Some(b"bytes".to_vec())
        );
        assert!(!journal.event_exists("absent:key").unwrap());
    }

    #[test]
    fn list_event_entries_is_complete_stably_ordered_and_digest_addressed() {
        use crate::canonical_hash::sha256_bytes_hex;

        let (_root, journal, _content) = local_backends();
        let keys = ["k:a", "k:b", "k:c"];
        for key in keys {
            journal.create_event_once(key, key.as_bytes()).unwrap();
        }

        let first = journal.list_event_entries().unwrap();
        let second = journal.list_event_entries().unwrap();
        assert_eq!(first.len(), 3);
        let first_pairs: Vec<(&str, &[u8])> = first
            .iter()
            .map(|e| (e.key_digest.as_str(), e.bytes.as_slice()))
            .collect();
        let second_pairs: Vec<(&str, &[u8])> = second
            .iter()
            .map(|e| (e.key_digest.as_str(), e.bytes.as_slice()))
            .collect();
        assert_eq!(
            first_pairs, second_pairs,
            "the listing is stable across calls"
        );

        // Each entry carries the sha256 of its logical key (the content-address
        // digest), and the order is that digest's sort order.
        let mut expected_digests: Vec<String> = keys
            .iter()
            .map(|k| sha256_bytes_hex(k.as_bytes()))
            .collect();
        expected_digests.sort();
        let listed_digests: Vec<String> = first.iter().map(|e| e.key_digest.clone()).collect();
        assert_eq!(listed_digests, expected_digests);
    }

    #[test]
    fn content_store_round_trips_bytes_and_dedups_a_second_put() {
        let (_root, _journal, content) = local_backends();
        let content_ref = "artifacts/objects/abc.json";

        assert_eq!(
            content.put_once(content_ref, b"blob").unwrap(),
            CreateFileOutcome::Created
        );
        assert_eq!(
            content.put_once(content_ref, b"other").unwrap(),
            CreateFileOutcome::AlreadyExists
        );
        assert_eq!(content.get(content_ref).unwrap(), b"blob");
        assert_eq!(
            content.get_if_exists(content_ref).unwrap(),
            Some(b"blob".to_vec())
        );
        assert_eq!(
            content
                .get_if_exists("artifacts/objects/missing.json")
                .unwrap(),
            None
        );
    }

    #[test]
    fn content_store_remove_is_removed_then_missing() {
        let (_root, _journal, content) = local_backends();
        let content_ref = "artifacts/notes/def.json";
        content.put_once(content_ref, b"body").unwrap();

        assert_eq!(content.remove(content_ref).unwrap(), RemoveOutcome::Removed);
        assert_eq!(content.remove(content_ref).unwrap(), RemoveOutcome::Missing);
    }

    #[test]
    fn content_store_list_returns_store_relative_refs_in_order() {
        let (_root, _journal, content) = local_backends();
        content.put_once("artifacts/objects/b.json", b"x").unwrap();
        content.put_once("artifacts/objects/a.json", b"y").unwrap();

        assert_eq!(
            content.list("artifacts/objects").unwrap(),
            vec![
                "artifacts/objects/a.json".to_owned(),
                "artifacts/objects/b.json".to_owned(),
            ]
        );
        assert_eq!(
            content.list("artifacts/notes").unwrap(),
            Vec::<String>::new(),
            "a missing prefix lists as empty"
        );
    }
}
