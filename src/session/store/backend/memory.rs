//! In-memory implementations of the durable backend traits.
//!
//! These satisfy the same [`Journal`] + [`ContentStore`] traits and the same
//! wrapper validation as the file backend, over plain maps behind a lock — the
//! honesty test for the seam. They drop only crash durability and cross-process
//! visibility: create-if-absent is atomic **within the process** (one lock, one
//! `HashMap::entry`), the single-process weakening the file backend's
//! `O_CREAT|O_EXCL` does not need. So this backend is **injection-only** — it is
//! never a `SHORE_BACKEND` value, only constructed directly in-process — and a
//! spawned child can never inherit an empty, lost-on-exit store.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::{ContentStore, Journal, JournalEntry};
use crate::error::{Result, ShoreError};
use crate::session::store::event_store::event_filename_stem;
use crate::storage::{CreateOutcome, RemoveOutcome};

/// The shared state behind the injection-only memory backend: an event map keyed
/// by the logical idempotency key and a blob map keyed by the path-shaped
/// `content_ref`. A single `Arc<InMemoryStore>` hands out journal and content
/// handles that all read and write these same maps, so a test can hold one
/// handle to inject raw bytes while the wrapper reads through another.
#[derive(Debug, Default)]
pub(crate) struct InMemoryStore {
    events: Mutex<HashMap<String, Vec<u8>>>,
    blobs: Mutex<HashMap<String, Vec<u8>>>,
    /// How many times any journal handle over this store has listed the event log.
    /// Lets a test prove the overview batch reads the log once for the whole batch
    /// (not `revision_count + 1` times — the N+1 it replaces).
    list_event_entries_calls: AtomicUsize,
    /// How many times any journal handle over this store has read an event's
    /// bytes. Lets a test prove `head_marker` reads no event bytes.
    read_event_bytes_calls: AtomicUsize,
}

impl InMemoryStore {
    /// A fresh, empty in-memory store.
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Test observability for the single-read invariant: the running count of
    /// `list_event_entries` calls served by every journal handle over this store.
    pub(crate) fn list_event_entries_call_count(&self) -> usize {
        self.list_event_entries_calls.load(Ordering::Relaxed)
    }

    /// Test observability for the no-bytes invariant: the running count of
    /// `read_event_bytes` calls served by every journal handle over this store.
    pub(crate) fn read_event_bytes_call_count(&self) -> usize {
        self.read_event_bytes_calls.load(Ordering::Relaxed)
    }

    /// A journal handle sharing this store's event map.
    pub(crate) fn journal(self: &Arc<Self>) -> InMemoryJournal {
        InMemoryJournal {
            store: Arc::clone(self),
        }
    }

    /// A content-store handle sharing this store's blob map.
    pub(crate) fn content_store(self: &Arc<Self>) -> InMemoryContentStore {
        InMemoryContentStore {
            store: Arc::clone(self),
        }
    }
}

/// The in-memory [`Journal`]: events live in a `HashMap` keyed by the logical
/// idempotency key.
#[derive(Debug)]
pub(crate) struct InMemoryJournal {
    store: Arc<InMemoryStore>,
}

impl Journal for InMemoryJournal {
    fn create_event_once(&self, idempotency_key: &str, bytes: &[u8]) -> Result<CreateOutcome> {
        // One lock around the vacancy check and the insert makes create-if-absent
        // atomic within the process; an existing entry is never overwritten.
        match lock(&self.store.events).entry(idempotency_key.to_owned()) {
            Entry::Vacant(slot) => {
                slot.insert(bytes.to_vec());
                Ok(CreateOutcome::Created)
            }
            Entry::Occupied(_) => Ok(CreateOutcome::AlreadyExists),
        }
    }

    fn read_event_bytes(&self, idempotency_key: &str) -> Result<Option<Vec<u8>>> {
        // Counted so a test can assert `head_marker` reads no event bytes.
        self.store
            .read_event_bytes_calls
            .fetch_add(1, Ordering::Relaxed);
        Ok(lock(&self.store.events).get(idempotency_key).cloned())
    }

    fn event_exists(&self, idempotency_key: &str) -> Result<bool> {
        Ok(lock(&self.store.events).contains_key(idempotency_key))
    }

    fn list_event_entries(&self) -> Result<Vec<JournalEntry>> {
        // Count every listing so a test can assert the overview batch reads the
        // log once for the whole batch, not once per revision.
        self.store
            .list_event_entries_calls
            .fetch_add(1, Ordering::Relaxed);
        // Each entry carries `sha256(idempotency_key)` as its content-address
        // digest — the same stem the file backend names files by — and the list
        // is ordered by that digest, matching the file backend's hash-sorted
        // listing (the projection folds events in this order, so it must agree
        // across backends).
        let mut entries: Vec<JournalEntry> = lock(&self.store.events)
            .iter()
            .map(|(key, bytes)| JournalEntry {
                key_digest: event_filename_stem(key),
                bytes: bytes.clone(),
            })
            .collect();
        entries.sort_by(|a, b| a.key_digest.cmp(&b.key_digest));
        Ok(entries)
    }

    fn head_marker(&self) -> Result<u64> {
        // The event map's length — the count of stored events, no bytes read.
        Ok(lock(&self.store.events).len() as u64)
    }

    #[cfg(test)]
    fn insert_raw(&self, idempotency_key: &str, bytes: &[u8]) -> Result<()> {
        // A raw map insert, overwriting any existing entry — the create-if-absent
        // dedup is deliberately skipped.
        lock(&self.store.events).insert(idempotency_key.to_owned(), bytes.to_vec());
        Ok(())
    }
}

/// The in-memory [`ContentStore`]: blobs live in a `HashMap` keyed by their
/// path-shaped `content_ref`.
#[derive(Debug)]
pub(crate) struct InMemoryContentStore {
    store: Arc<InMemoryStore>,
}

impl ContentStore for InMemoryContentStore {
    fn put_once(&self, content_ref: &str, bytes: &[u8]) -> Result<CreateOutcome> {
        match lock(&self.store.blobs).entry(content_ref.to_owned()) {
            Entry::Vacant(slot) => {
                slot.insert(bytes.to_vec());
                Ok(CreateOutcome::Created)
            }
            Entry::Occupied(_) => Ok(CreateOutcome::AlreadyExists),
        }
    }

    fn get(&self, content_ref: &str) -> Result<Vec<u8>> {
        lock(&self.store.blobs)
            .get(content_ref)
            .cloned()
            .ok_or_else(|| ShoreError::Message(format!("missing in-memory blob {content_ref}")))
    }

    fn get_if_exists(&self, content_ref: &str) -> Result<Option<Vec<u8>>> {
        Ok(lock(&self.store.blobs).get(content_ref).cloned())
    }

    fn remove(&self, content_ref: &str) -> Result<RemoveOutcome> {
        Ok(match lock(&self.store.blobs).remove(content_ref) {
            Some(_) => RemoveOutcome::Removed,
            None => RemoveOutcome::Missing,
        })
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        // Match the file backend's one-level, sorted listing: the immediate
        // children under `prefix`, in lexicographic order.
        let mut refs: Vec<String> = lock(&self.store.blobs)
            .keys()
            .filter(|key| is_immediate_child(key, prefix))
            .cloned()
            .collect();
        refs.sort();
        Ok(refs)
    }

    #[cfg(test)]
    fn put_raw(&self, content_ref: &str, bytes: &[u8]) -> Result<()> {
        // A raw map insert, overwriting any existing blob — the create-side
        // validation the wrapper performs is deliberately skipped.
        lock(&self.store.blobs).insert(content_ref.to_owned(), bytes.to_vec());
        Ok(())
    }
}

/// Lock a backend mutex, recovering the guard if a prior holder panicked. The
/// stored bytes are still intact after a poison, so there is nothing to abort
/// for; a test that panics mid-operation should surface its own panic, not a
/// secondary poison error here.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether `key` is an immediate child of `prefix` (`<prefix>/<name>` with no
/// further `/`), matching `LocalContentStore::list`'s one-level directory walk.
fn is_immediate_child(key: &str, prefix: &str) -> bool {
    key.strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|name| !name.is_empty() && !name.contains('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_marker_reads_no_event_bytes() {
        let store = InMemoryStore::new();
        let journal = store.journal();
        for key in ["k:a", "k:b", "k:c"] {
            journal.create_event_once(key, key.as_bytes()).unwrap();
        }

        let before = store.read_event_bytes_call_count();
        assert_eq!(journal.head_marker().unwrap(), 3);
        assert_eq!(
            store.read_event_bytes_call_count(),
            before,
            "the marker is a count, not a read — it touches no event bytes"
        );
    }
}
