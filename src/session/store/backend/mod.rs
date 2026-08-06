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
#[cfg(test)]
mod memory;
#[cfg(any(test, windows))]
mod ntfs_journal;

use std::fmt::Debug;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Arc;

pub(crate) use local::{LocalContentStore, LocalJournal, QualificationLocalJournal};
#[cfg(test)]
pub(crate) use memory::InMemoryStore;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::session::derived_access::cursor::{CursorDelta, TruthCursor, TruthHead};
use crate::storage::{CreateOutcome, RemoveOutcome};

/// Opaque, comparable observation of the local journal directory.
///
/// The value is deliberately not an event count or event-set proof. Consumers
/// may only compare two observations: equality means the native observable did
/// not move; inequality means an exact audit is required. Platform-specific
/// metadata stays inside the local backend. Equality proves only that the
/// observable stayed equal; it is not itself a no-change guarantee.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum JournalChangeStamp {
    Absent,
    Observed {
        identity_sha256: String,
        change_sha256: String,
        entry_count: Option<u64>,
        native_cursor: Option<JournalNativeCursor>,
    },
}

/// Native state needed to continue an exact change observation without
/// exposing platform layouts to consumers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct JournalNativeCursor {
    pub(super) journal_id: u64,
    pub(super) next_usn: i64,
    pub(super) directory_file_reference: u64,
    pub(super) volume_serial_number: u64,
}

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JournalChangeVerdict {
    Stable,
    Changed,
    Indeterminate,
}

/// Result of continuing a native change observation from a saved stamp.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct JournalChangeCheck {
    pub(crate) after: JournalChangeStamp,
    pub(crate) verdict: JournalChangeVerdict,
    pub(crate) native_bytes_examined: u64,
    pub(crate) native_records_examined: u64,
    pub(crate) relevant_file_references: Vec<u64>,
    pub(crate) mechanism: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) enum JournalCreatedTransitionVerdict {
    Accepted,
    Contended,
    Indeterminate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct JournalCreatedTransition {
    pub(crate) after: JournalChangeStamp,
    pub(crate) verdict: JournalCreatedTransitionVerdict,
    pub(crate) mechanism: String,
}

impl JournalChangeStamp {
    #[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
    pub(super) fn observed(identity: &[u8], change: &[u8]) -> Self {
        Self::Observed {
            identity_sha256: crate::canonical_hash::sha256_bytes_hex(identity),
            change_sha256: crate::canonical_hash::sha256_bytes_hex(change),
            entry_count: None,
            native_cursor: None,
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn observed_with_entry_count(
        identity: &[u8],
        change: &[u8],
        entry_count: u64,
    ) -> Self {
        Self::Observed {
            identity_sha256: crate::canonical_hash::sha256_bytes_hex(identity),
            change_sha256: crate::canonical_hash::sha256_bytes_hex(change),
            entry_count: Some(entry_count),
            native_cursor: None,
        }
    }

    #[cfg(windows)]
    pub(super) fn observed_with_native_cursor(
        identity: &[u8],
        change: &[u8],
        native_cursor: JournalNativeCursor,
    ) -> Self {
        Self::Observed {
            identity_sha256: crate::canonical_hash::sha256_bytes_hex(identity),
            change_sha256: crate::canonical_hash::sha256_bytes_hex(change),
            entry_count: None,
            native_cursor: Some(native_cursor),
        }
    }

    #[cfg(windows)]
    pub(super) fn native_cursor(&self) -> Option<&JournalNativeCursor> {
        match self {
            Self::Absent => None,
            Self::Observed { native_cursor, .. } => native_cursor.as_ref(),
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn entry_count(&self) -> Option<u64> {
        match self {
            Self::Absent => None,
            Self::Observed { entry_count, .. } => *entry_count,
        }
    }

    #[cfg(any(test, feature = "bench"))]
    pub(crate) fn continuation_token(&self) -> Option<String> {
        let Self::Observed {
            identity_sha256,
            change_sha256,
            entry_count: _,
            native_cursor: Some(cursor),
        } = self
        else {
            return None;
        };
        Some(format!(
            "ntfs-v1,{identity_sha256},{change_sha256},{},{},{},{}",
            cursor.journal_id,
            cursor.next_usn,
            cursor.directory_file_reference,
            cursor.volume_serial_number
        ))
    }

    #[cfg(any(test, feature = "bench"))]
    pub(crate) fn from_continuation_token(token: &str) -> Option<Self> {
        let mut parts = token.split(',');
        if parts.next()? != "ntfs-v1" {
            return None;
        }
        let identity_sha256 = parts.next()?.to_owned();
        let change_sha256 = parts.next()?.to_owned();
        let cursor = JournalNativeCursor {
            journal_id: parts.next()?.parse().ok()?,
            next_usn: parts.next()?.parse().ok()?,
            directory_file_reference: parts.next()?.parse().ok()?,
            volume_serial_number: parts.next()?.parse().ok()?,
        };
        if parts.next().is_some()
            || !is_lower_sha256(&identity_sha256)
            || !is_lower_sha256(&change_sha256)
        {
            return None;
        }
        Some(Self::Observed {
            identity_sha256,
            change_sha256,
            entry_count: None,
            native_cursor: Some(cursor),
        })
    }

    #[cfg(any(test, feature = "bench"))]
    pub(crate) fn opaque_sha256(&self) -> String {
        match self {
            Self::Absent => crate::canonical_hash::sha256_bytes_hex(b"journal-change-stamp:absent"),
            Self::Observed {
                identity_sha256,
                change_sha256,
                ..
            } => crate::canonical_hash::sha256_bytes_hex(
                format!("journal-change-stamp:v1:{identity_sha256}:{change_sha256}").as_bytes(),
            ),
        }
    }

    pub(super) fn compared(before: &Self, after: Self) -> JournalChangeCheck {
        let verdict = if before == &after {
            JournalChangeVerdict::Stable
        } else {
            JournalChangeVerdict::Changed
        };
        JournalChangeCheck {
            after,
            verdict,
            native_bytes_examined: 0,
            native_records_examined: 0,
            relevant_file_references: Vec::new(),
            mechanism: "compare native directory observations".to_owned(),
        }
    }
}

#[cfg(any(test, feature = "bench"))]
fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The closed set of durable-storage backends, and the one place that dispatches
/// to a concrete impl. A resolution carries a `StoreBackend` handle and the
/// event/content wrappers are built from it, so the selection made once at the
/// resolve choke point flows to every consumer without threading a path.
///
/// `Local` is the production backend `resolve_store` selects; `Memory` is an
/// injection-only backend constructed directly in-process (never a
/// `POINTBREAK_BACKEND` value) for tests and experiments, so it is compiled only
/// under `cfg(test)`. Deliberately **not** `Eq`/`PartialEq`: no resolution is
/// ever compared whole, and `Memory`'s shared map is not comparable.
#[derive(Clone, Debug)]
pub(crate) enum StoreBackend {
    /// The default file backend, wrapping the resolved store directory.
    Local(PathBuf),
    /// The injection-only in-memory backend, sharing one set of maps.
    #[cfg(test)]
    Memory(Arc<InMemoryStore>),
}

impl StoreBackend {
    /// A fresh in-memory backend over empty maps. Injection-only: there is no
    /// `POINTBREAK_BACKEND` value that resolves here (the selector rejects `memory`).
    #[cfg(test)]
    pub(crate) fn memory() -> Self {
        StoreBackend::Memory(InMemoryStore::new())
    }

    /// A fresh journal handle for this backend.
    pub(crate) fn journal(&self) -> Box<dyn Journal> {
        match self {
            StoreBackend::Local(store_dir) => Box::new(LocalJournal::new(store_dir)),
            #[cfg(test)]
            StoreBackend::Memory(store) => Box::new(store.journal()),
        }
    }

    /// A fresh content-store handle for this backend.
    pub(crate) fn content_store(&self) -> Box<dyn ContentStore> {
        match self {
            StoreBackend::Local(store_dir) => Box::new(LocalContentStore::new(store_dir)),
            #[cfg(test)]
            StoreBackend::Memory(store) => Box::new(store.content_store()),
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
    /// Store one opaque Journal record under its logical key. Event-only callers
    /// keep using `create_event_once`; capability records use this deliberately
    /// generic spelling so they never masquerade as an event in typed code.
    fn create_record_once(&self, logical_key: &str, bytes: &[u8]) -> Result<CreateOutcome> {
        self.create_event_once(logical_key, bytes)
    }

    /// Store the event for `idempotency_key` only if absent, atomically and safe
    /// against a concurrent writer. Reports whether the bytes were written or an
    /// entry already existed; an existing entry is never overwritten.
    fn create_event_once(&self, idempotency_key: &str, bytes: &[u8]) -> Result<CreateOutcome>;

    /// The stored bytes for `idempotency_key`, or `None` when absent.
    fn read_event_bytes(&self, idempotency_key: &str) -> Result<Option<Vec<u8>>>;

    /// Whether an event is stored for `idempotency_key`.
    fn event_exists(&self, idempotency_key: &str) -> Result<bool>;

    fn record_exists(&self, logical_key: &str) -> Result<bool> {
        self.event_exists(logical_key)
    }

    /// Every stored event, paired with its content-address digest, in a
    /// deterministic order. The order is part of the contract: the projection
    /// folds events in this order, so it must be stable across backends. The
    /// per-entry digest lets the wrapper verify each blob still sits at its
    /// content-addressed home.
    fn list_event_entries(&self) -> Result<Vec<JournalEntry>>;

    /// Every opaque record in the Journal namespace. A capable router must call
    /// this before attempting `ShoreEvent` decoding.
    fn list_record_entries(&self) -> Result<Vec<JournalEntry>> {
        self.list_event_entries()
    }

    /// A cheap, monotonic staleness signal — the count of stored events —
    /// computed **without reading any event bytes** (no decode, no hash). It is
    /// the per-read freshness *detector*: the append-only journal contract (there
    /// is no remove; removals are appended events; `compact` never touches the
    /// event log) makes the count rise on every real append and never fall, so a
    /// changed count means the log moved. Its blind spot is exactly the event-set
    /// hash's — an out-of-band edit to an existing event's bytes that adds no
    /// entry — so the hash stays the authoritative confirm stamp on the full-read
    /// surfaces.
    fn head_marker(&self) -> Result<u64>;

    /// Capture the backend's O(1) local change observable without enumerating
    /// directory entries or opening a carrier. This is a conservative drift
    /// signal only; a changed value never proves which event changed or that any
    /// event is valid.
    fn change_stamp(&self) -> Result<JournalChangeStamp>;

    fn changes_since(&self, before: &JournalChangeStamp) -> Result<JournalChangeCheck> {
        Ok(JournalChangeStamp::compared(before, self.change_stamp()?))
    }

    /// Test-only tamper hook: write `bytes` at the slot this backend would use
    /// for `idempotency_key`, bypassing create-side validation and dedup
    /// (overwriting any existing entry). It lets a test inject bytes that skip
    /// the wrapper's write-side checks so the wrapper's **read-side** validation
    /// can be exercised against either backend identically.
    #[cfg(test)]
    fn insert_raw(&self, idempotency_key: &str, bytes: &[u8]) -> Result<()>;
}

/// Bounded cursor view for the dormant derived-access implementation. Existing
/// `Journal` implementations and product resolution do not implement or select
/// this interface; a derived ledger supplies it beside unchanged authoritative
/// truth.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the first physical cursor ledger is introduced in a later source slice"
    )
)]
pub(crate) trait QualificationJournalCursor: Debug {
    type Error;

    fn qualification_truth_head(&self) -> std::result::Result<TruthHead, Self::Error>;

    fn qualification_events_after(
        &self,
        after: TruthCursor,
        limit: usize,
    ) -> std::result::Result<CursorDelta, Self::Error>;
}

/// Content-addressed access over opaque blobs, shared by object artifacts and
/// note bodies. A `content_ref` is a store-relative locator
/// (`artifacts/objects/<hash>.json`, `artifacts/notes/<hash>.json`).
pub(crate) trait ContentStore: Debug {
    /// Store `bytes` at `content_ref` only if absent, atomically. Reports whether
    /// the bytes were written or a blob already existed.
    fn put_once(&self, content_ref: &str, bytes: &[u8]) -> Result<CreateOutcome>;

    /// The stored bytes for `content_ref`; errors if absent.
    fn get(&self, content_ref: &str) -> Result<Vec<u8>>;

    /// The stored bytes for `content_ref`, or `None` when absent.
    fn get_if_exists(&self, content_ref: &str) -> Result<Option<Vec<u8>>>;

    /// Remove the blob at `content_ref`. A plain unlink: any re-hash-before-erase
    /// floor is the caller's, above this. Reports removed vs already-absent.
    fn remove(&self, content_ref: &str) -> Result<RemoveOutcome>;

    /// Every stored locator under `prefix`, in a deterministic order.
    fn list(&self, prefix: &str) -> Result<Vec<String>>;

    /// Test-only tamper hook: write `bytes` at `content_ref`, bypassing
    /// create-side validation (overwriting any existing blob). It lets a test
    /// inject bytes that skip the wrapper's write-side checks so the wrapper's
    /// **read-side** content-hash validation can be exercised against either
    /// backend identically.
    #[cfg(test)]
    fn put_raw(&self, content_ref: &str, bytes: &[u8]) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical_hash::sha256_bytes_hex;

    /// Every trait-contract assertion runs over each backend in turn: the file
    /// backend (rooted at a temp dir the returned guard keeps alive) and the
    /// injection-only in-memory backend. A backend that diverged on any of these
    /// would not be a faithful drop-in below the wrappers — this is the honesty
    /// test at the trait level.
    fn each_backend() -> Vec<(Option<tempfile::TempDir>, StoreBackend)> {
        let root = tempfile::tempdir().unwrap();
        let store_dir = root.path().join(".pointbreak/data");
        vec![
            (Some(root), StoreBackend::Local(store_dir)),
            (None, StoreBackend::memory()),
        ]
    }

    #[test]
    fn create_event_once_is_create_then_already_exists_without_overwriting() {
        for (_guard, backend) in each_backend() {
            let journal = backend.journal();
            let key = "review_initialized:journal:default:work:default";

            assert_eq!(
                journal.create_event_once(key, b"first").unwrap(),
                CreateOutcome::Created
            );
            assert_eq!(
                journal.create_event_once(key, b"second").unwrap(),
                CreateOutcome::AlreadyExists
            );
            assert_eq!(
                journal.read_event_bytes(key).unwrap(),
                Some(b"first".to_vec())
            );
        }
    }

    #[test]
    fn journal_read_and_exists_resolve_by_logical_key() {
        for (_guard, backend) in each_backend() {
            let journal = backend.journal();
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
    }

    #[test]
    fn list_event_entries_is_complete_stably_ordered_and_digest_addressed() {
        for (_guard, backend) in each_backend() {
            let journal = backend.journal();
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

            // Each entry carries the sha256 of its logical key (the
            // content-address digest), and the order is that digest's sort order —
            // identically for both backends, so the projection folds the same way.
            let mut expected_digests: Vec<String> = keys
                .iter()
                .map(|k| sha256_bytes_hex(k.as_bytes()))
                .collect();
            expected_digests.sort();
            let listed_digests: Vec<String> = first.iter().map(|e| e.key_digest.clone()).collect();
            assert_eq!(listed_digests, expected_digests);
        }
    }

    #[test]
    fn head_marker_counts_written_events_and_is_monotonic_on_append() {
        for (_guard, backend) in each_backend() {
            let journal = backend.journal();
            assert_eq!(journal.head_marker().unwrap(), 0, "an empty log marks zero");

            for key in ["k:a", "k:b", "k:c"] {
                journal.create_event_once(key, key.as_bytes()).unwrap();
            }
            assert_eq!(
                journal.head_marker().unwrap(),
                3,
                "the marker is the count of stored events"
            );

            journal.create_event_once("k:d", b"d").unwrap();
            assert_eq!(
                journal.head_marker().unwrap(),
                4,
                "an append bumps the marker by one"
            );
            // A deduped re-create writes nothing, so the marker is unchanged.
            journal.create_event_once("k:d", b"again").unwrap();
            assert_eq!(journal.head_marker().unwrap(), 4);
        }
    }

    #[test]
    fn head_marker_unchanged_by_an_in_place_envelope_edit() {
        for (_guard, backend) in each_backend() {
            let journal = backend.journal();
            journal.create_event_once("k:a", b"original").unwrap();
            journal.create_event_once("k:b", b"original").unwrap();
            let before = journal.head_marker().unwrap();

            // An out-of-band edit to an existing event's bytes overwrites in place;
            // it adds no entry, so the count is unchanged — the same blind spot the
            // event-set hash carries for an envelope-only edit.
            journal.insert_raw("k:a", b"edited-in-place").unwrap();

            assert_eq!(journal.head_marker().unwrap(), before);
        }
    }

    #[test]
    fn journal_change_stamp_detects_created_carriers_without_proving_their_truth() {
        for (_guard, backend) in each_backend() {
            let journal = backend.journal();
            let absent = journal.change_stamp().unwrap();

            journal.create_event_once("k:a", b"first").unwrap();
            let created = journal.change_stamp().unwrap();
            assert_eq!(
                journal.changes_since(&absent).unwrap().verdict,
                JournalChangeVerdict::Changed,
                "a created carrier changes the bounded observation"
            );

            journal.create_event_once("k:a", b"different").unwrap();
            assert_eq!(
                journal.changes_since(&created).unwrap().verdict,
                JournalChangeVerdict::Stable,
                "a duplicate attempt that creates no carrier leaves the bounded observation stable"
            );

            journal.create_event_once("k:b", b"second").unwrap();
            assert_eq!(
                journal.changes_since(&created).unwrap().verdict,
                JournalChangeVerdict::Changed,
                "a second created carrier changes the bounded observation again"
            );
        }
    }

    #[test]
    fn journal_change_stamp_has_an_explicit_existing_carrier_overwrite_non_claim() {
        for (_guard, backend) in each_backend() {
            let journal = backend.journal();
            journal.create_event_once("k:a", b"original").unwrap();
            let before = journal.change_stamp().unwrap();

            journal.insert_raw("k:a", b"tampered").unwrap();

            let after = journal.change_stamp().unwrap();
            let _observed_stamp_change = before != after;
            assert_eq!(
                journal.read_event_bytes("k:a").unwrap(),
                Some(b"tampered".to_vec()),
                "the selected-carrier read still exposes bytes for validation"
            );
        }
    }

    #[test]
    fn content_store_round_trips_bytes_and_dedups_a_second_put() {
        for (_guard, backend) in each_backend() {
            let content = backend.content_store();
            let content_ref = "artifacts/objects/abc.json";

            assert_eq!(
                content.put_once(content_ref, b"blob").unwrap(),
                CreateOutcome::Created
            );
            assert_eq!(
                content.put_once(content_ref, b"other").unwrap(),
                CreateOutcome::AlreadyExists
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
    }

    #[test]
    fn content_store_remove_is_removed_then_missing() {
        for (_guard, backend) in each_backend() {
            let content = backend.content_store();
            let content_ref = "artifacts/notes/def.json";
            content.put_once(content_ref, b"body").unwrap();

            assert_eq!(content.remove(content_ref).unwrap(), RemoveOutcome::Removed);
            assert_eq!(content.remove(content_ref).unwrap(), RemoveOutcome::Missing);
        }
    }

    #[test]
    fn content_store_list_returns_store_relative_refs_in_order() {
        for (_guard, backend) in each_backend() {
            let content = backend.content_store();
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
}
