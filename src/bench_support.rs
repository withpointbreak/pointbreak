//! Developer-run measurement support for the durable event store's file backend.
//!
//! This module establishes the file backend's baseline for the three metrics a
//! future log-structured backend would aim to improve — whole-log read latency,
//! single append latency, and on-disk amplification — over synthetic event
//! changesets the caller populates. It exists so the `store_backend` benchmark
//! crate (a separate compilation unit that only sees the public API) and an
//! in-crate smoke test can share one harness; the event store it drives is
//! crate-internal, so the harness lives here rather than in the benchmark file.
//!
//! It is gated behind `cfg(test)` (so the smoke test runs under the normal test
//! runner) and the `bench` cargo feature (so the benchmark crate can reach it),
//! and is never compiled into a release build — it stays out of the published
//! crate's surface.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::model::JournalId;
use crate::session::EventStore;
use crate::session::event::{EventTarget, EventType, ReviewInitializedPayload, ShoreEvent, Writer};

/// On-disk versus logical byte accounting for a store directory.
pub struct ByteUsage {
    /// Sum of file content lengths — the logical bytes the events occupy.
    pub logical: u64,
    /// Sum of the bytes actually allocated on disk. On unix this is the block
    /// count, so a small event file costs a whole filesystem block; where
    /// allocation info is unavailable it equals `logical`. The `physical /
    /// logical` ratio is the amplification a many-small-files layout pays.
    pub physical: u64,
}

/// A file-backed event store under a caller-owned directory, populated with
/// synthetic events for measurement. The caller owns the directory (a temp dir,
/// or a captured fixture) so this stays free of any test-only dependency.
pub struct StoreBenchHarness {
    store_dir: PathBuf,
    store: EventStore,
    next_index: AtomicUsize,
}

impl StoreBenchHarness {
    /// Open a file-backed event store rooted at `store_dir`.
    pub fn open(store_dir: impl AsRef<Path>) -> Self {
        let store_dir = store_dir.as_ref().to_path_buf();
        Self {
            store: EventStore::open(&store_dir),
            store_dir,
            next_index: AtomicUsize::new(0),
        }
    }

    /// Write `count` distinct synthetic events into the store, advancing the
    /// append cursor so a later `append_one` writes a fresh key.
    pub fn populate(&self, count: usize) {
        for _ in 0..count {
            self.write_next();
        }
    }

    /// Append one fresh synthetic event into the (warm) store. Each call uses a
    /// new key, so the write is a genuine create rather than an idempotent
    /// no-op.
    pub fn append_one(&self) {
        self.write_next();
    }

    /// Read the whole event log back, returning the event count. Panics on a
    /// decode failure — a synthetic store this harness wrote always lists.
    pub fn read_all(&self) -> usize {
        self.try_read_all()
            .expect("a synthetic store lists cleanly")
    }

    /// Read the whole event log, surfacing a failure as `Err` rather than
    /// panicking so a fixture on a schema this build no longer decodes can be
    /// skipped instead of aborting the run.
    pub fn try_read_all(&self) -> Result<usize, String> {
        self.store
            .list_events()
            .map(|events| events.len())
            .map_err(|error| error.to_string())
    }

    /// On-disk versus logical byte usage for this store directory.
    pub fn byte_usage(&self) -> ByteUsage {
        dir_byte_usage(&self.store_dir)
    }

    /// Record the next synthetic event, keyed by a monotonically increasing
    /// index so every write lands a distinct event file.
    fn write_next(&self) {
        let index = self.next_index.fetch_add(1, Ordering::Relaxed);
        self.store
            .record_event_once(&synthetic_event(index))
            .expect("a synthetic event records");
    }
}

/// A valid, self-consistent `ReviewInitialized` event keyed by `index`. The
/// constructor derives the event id and payload hash, so the write- and
/// read-side validation both accept it; varying the index by one varies the
/// idempotency key, so each event occupies its own content-addressed file.
fn synthetic_event(index: usize) -> ShoreEvent {
    let session = format!("session:bench-{index}");
    ShoreEvent::new(
        EventType::ReviewInitialized,
        format!("review_initialized:{session}:work:default"),
        EventTarget::for_journal(JournalId::new(session.as_str())),
        Writer::shore_local("0.1.0"),
        ReviewInitializedPayload {},
        "2026-05-10T00:00:00Z",
    )
    .expect("a synthetic event builds")
}

/// Recursively sum the logical (content) and physical (allocated) bytes of every
/// file under `dir`. A missing directory contributes nothing.
fn dir_byte_usage(dir: &Path) -> ByteUsage {
    let mut usage = ByteUsage {
        logical: 0,
        physical: 0,
    };
    accumulate_byte_usage(dir, &mut usage);
    usage
}

fn accumulate_byte_usage(dir: &Path, usage: &mut ByteUsage) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            accumulate_byte_usage(&entry.path(), usage);
        } else if metadata.is_file() {
            usage.logical += metadata.len();
            usage.physical += physical_bytes(&metadata);
        }
    }
}

#[cfg(unix)]
fn physical_bytes(metadata: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    // `blocks()` counts the 512-byte units actually allocated — this is what
    // makes a small event file cost a whole filesystem block, the amplification
    // we measure. (Filesystems that pack tiny files inline can report fewer.)
    metadata.blocks() * 512
}

#[cfg(not(unix))]
fn physical_bytes(metadata: &std::fs::Metadata) -> u64 {
    // No portable allocation query; fall back to the logical length so the ratio
    // reads as 1.0 rather than misreporting.
    metadata.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_measures_a_synthetic_store_without_panicking() {
        let root = tempfile::tempdir().expect("a temp dir");
        let harness = StoreBenchHarness::open(root.path().join(".shore/data"));

        harness.populate(50);
        assert_eq!(harness.read_all(), 50, "every populated event reads back");

        harness.append_one();
        assert_eq!(
            harness.read_all(),
            51,
            "the appended event is visible on the next read"
        );
        assert_eq!(harness.try_read_all().unwrap(), 51);

        let usage = harness.byte_usage();
        assert!(
            usage.logical > 0,
            "the synthetic store occupies logical bytes"
        );
        assert!(
            usage.physical > 0,
            "the synthetic store occupies on-disk bytes"
        );
    }
}
