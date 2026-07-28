//! File-backed implementations of the durable backend traits over
//! [`LocalStorage`]. These preserve today's on-disk layout, hash-sorted listing
//! order, and atomic create-if-absent semantics, so a backend swap is invisible
//! to every stored byte.

use std::path::{Path, PathBuf};

use super::{ContentStore, Journal, JournalEntry};
use crate::error::{Result, ShoreError};
use crate::session::store::event_store::{event_filename_stem, is_event_file};
use crate::storage::{CreateOutcome, Durability, LocalStorage, RemoveOutcome, is_temp_file_path};

/// The file-backed [`Journal`]: events live at
/// `events/<sha256(idempotency_key)>.json` under the store dir.
#[derive(Debug)]
pub(crate) struct LocalJournal {
    storage: LocalStorage,
    store_dir: PathBuf,
}

impl LocalJournal {
    pub(crate) fn new(store_dir: impl AsRef<Path>) -> Self {
        let store_dir = store_dir.as_ref().to_path_buf();
        Self {
            storage: LocalStorage::new(&store_dir),
            store_dir,
        }
    }

    fn events_dir(&self) -> PathBuf {
        self.store_dir.join("events")
    }

    fn event_path(&self, idempotency_key: &str) -> PathBuf {
        self.events_dir()
            .join(format!("{}.json", event_filename_stem(idempotency_key)))
    }

    #[cfg(any(test, feature = "bench"))]
    fn read_event_bytes_by_key_digest(&self, key_digest: &str) -> Result<Option<Vec<u8>>> {
        if key_digest.len() != 64
            || !key_digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ShoreError::Message(
                "event key digest must be 64 lowercase hexadecimal characters".to_owned(),
            ));
        }
        #[cfg(any(test, feature = "longitudinal-counting"))]
        crate::bench_support::longitudinal::record_carrier_open();
        let path = self.events_dir().join(format!("{key_digest}.json"));
        let bytes = self.storage.read_bytes_if_exists(&path)?;
        #[cfg(any(test, feature = "longitudinal-counting"))]
        if let Some(bytes) = &bytes {
            crate::bench_support::longitudinal::record_carrier_bytes(bytes.len());
        }
        Ok(bytes)
    }
}

impl Journal for LocalJournal {
    fn create_event_once(&self, idempotency_key: &str, bytes: &[u8]) -> Result<CreateOutcome> {
        self.storage.create_file_exclusive(
            &self.event_path(idempotency_key),
            bytes,
            Durability::Durable,
        )
    }

    fn read_event_bytes(&self, idempotency_key: &str) -> Result<Option<Vec<u8>>> {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        crate::bench_support::longitudinal::record_carrier_open();
        let bytes = self
            .storage
            .read_bytes_if_exists(&self.event_path(idempotency_key))?;
        #[cfg(any(test, feature = "longitudinal-counting"))]
        if let Some(bytes) = &bytes {
            crate::bench_support::longitudinal::record_carrier_bytes(bytes.len());
        }
        Ok(bytes)
    }

    fn event_exists(&self, idempotency_key: &str) -> Result<bool> {
        Ok(self.event_path(idempotency_key).exists())
    }

    fn list_event_entries(&self) -> Result<Vec<JournalEntry>> {
        // `list_dir` already sorts, so this preserves today's hash-sorted order;
        // the same event-file filter keeps temp files and stray names out. Each
        // entry's digest is the file name stem (the sha256 of the logical key it
        // was stored under), which the wrapper checks against the decoded event.
        self.storage
            .list_dir(&self.events_dir())?
            .into_iter()
            .filter(|path| is_event_file(path))
            .map(|path| {
                let key_digest = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .ok_or_else(|| {
                        ShoreError::Message(format!(
                            "event file has no readable name: {}",
                            path.display()
                        ))
                    })?
                    .to_owned();
                #[cfg(any(test, feature = "longitudinal-counting"))]
                crate::bench_support::longitudinal::record_carrier_open();
                let bytes = self.storage.read_bytes(&path)?;
                #[cfg(any(test, feature = "longitudinal-counting"))]
                crate::bench_support::longitudinal::record_carrier_bytes(bytes.len());
                Ok(JournalEntry { key_digest, bytes })
            })
            .collect()
    }

    fn head_marker(&self) -> Result<u64> {
        // A dirent scan of `events/`, filtered to event files, then counted —
        // never opening a file. Distinct from `list_event_entries`, which reads
        // every file's bytes; the marker pays only the directory listing. The same
        // `is_event_file` filter keeps temp files and stray names out, so the count
        // matches the listed-entry count without the reads.
        Ok(self
            .storage
            .list_dir(&self.events_dir())?
            .into_iter()
            .filter(|path| is_event_file(path))
            .count() as u64)
    }

    #[cfg(test)]
    fn insert_raw(&self, idempotency_key: &str, bytes: &[u8]) -> Result<()> {
        // A plain atomic write at the key's content-addressed path, overwriting
        // any existing file — the create-if-absent dedup is deliberately skipped.
        self.storage.write_bytes_atomic(
            &self.event_path(idempotency_key),
            bytes,
            Durability::Durable,
        )
    }
}

/// Qualification-only access to the existing durable loose-event primitives.
///
/// The cursor falsifier holds its own store-scoped publication lock around
/// these calls. This adapter deliberately adds no production route and performs
/// no directory listing.
#[cfg(any(test, feature = "bench"))]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug)]
pub(crate) struct QualificationLocalJournal {
    journal: LocalJournal,
    store_dir: PathBuf,
}

#[cfg(any(test, feature = "bench"))]
#[cfg_attr(not(test), allow(dead_code))]
impl QualificationLocalJournal {
    pub(crate) fn new(store_dir: impl AsRef<Path>) -> Self {
        let store_dir = store_dir.as_ref().to_path_buf();
        Self {
            journal: LocalJournal::new(&store_dir),
            store_dir,
        }
    }

    pub(crate) fn record_event_once(
        &self,
        event: &crate::session::event::ShoreEvent,
    ) -> Result<crate::session::EventWriteOutcome> {
        crate::session::EventStore::open(&self.store_dir).record_event_once(event)
    }

    pub(crate) fn read_event_bytes(&self, logical_reread_key: &str) -> Result<Option<Vec<u8>>> {
        self.journal.read_event_bytes(logical_reread_key)
    }

    pub(crate) fn read_event_bytes_by_key_digest(
        &self,
        key_digest: &str,
    ) -> Result<Option<Vec<u8>>> {
        self.journal.read_event_bytes_by_key_digest(key_digest)
    }
}

/// The file-backed [`ContentStore`]: blobs live at their store-relative
/// `content_ref` under the store dir.
#[derive(Debug)]
pub(crate) struct LocalContentStore {
    storage: LocalStorage,
}

impl LocalContentStore {
    pub(crate) fn new(store_dir: impl AsRef<Path>) -> Self {
        Self {
            storage: LocalStorage::new(store_dir),
        }
    }
}

impl ContentStore for LocalContentStore {
    fn put_once(&self, content_ref: &str, bytes: &[u8]) -> Result<CreateOutcome> {
        self.storage
            .create_file_exclusive(Path::new(content_ref), bytes, Durability::Durable)
    }

    fn get(&self, content_ref: &str) -> Result<Vec<u8>> {
        self.storage.read_bytes(Path::new(content_ref))
    }

    fn get_if_exists(&self, content_ref: &str) -> Result<Option<Vec<u8>>> {
        self.storage.read_bytes_if_exists(Path::new(content_ref))
    }

    fn remove(&self, content_ref: &str) -> Result<RemoveOutcome> {
        self.storage.remove_file(content_ref)
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        // `list_dir` returns sorted, store-resolved paths; surface each as a
        // store-relative ref under `prefix`, dropping any non-UTF-8 name.
        Ok(self
            .storage
            .list_dir(Path::new(prefix))?
            .into_iter()
            .filter(|path| !is_temp_file_path(path))
            .filter_map(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| format!("{prefix}/{name}"))
            })
            .collect())
    }

    #[cfg(test)]
    fn put_raw(&self, content_ref: &str, bytes: &[u8]) -> Result<()> {
        // A plain atomic write at the locator, overwriting any existing blob —
        // the create-side validation the wrapper performs is deliberately skipped.
        self.storage
            .write_bytes_atomic(Path::new(content_ref), bytes, Durability::Durable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench_support::longitudinal::LongitudinalCountingScopeV1;

    fn counting_scope(byte: char) -> LongitudinalCountingScopeV1 {
        LongitudinalCountingScopeV1::new(std::iter::repeat_n(byte, 64).collect::<String>())
            .expect("valid counting scope")
    }

    #[test]
    fn content_list_ignores_in_flight_temp_files() {
        let root = tempfile::tempdir().unwrap();
        let store = LocalContentStore::new(root.path());
        let objects_dir = root.path().join("artifacts/objects");
        std::fs::create_dir_all(&objects_dir).unwrap();
        std::fs::write(objects_dir.join(".shore-write.inflight.tmp"), b"partial").unwrap();

        store.put_once("artifacts/objects/a.json", b"a").unwrap();

        assert_eq!(
            store.list("artifacts/objects").unwrap(),
            vec!["artifacts/objects/a.json".to_owned()]
        );
    }

    #[test]
    fn counting_calibrates_directory_point_list_and_head_boundaries() {
        let root = tempfile::tempdir().unwrap();
        let journal = LocalJournal::new(root.path());
        journal.create_event_once("one", b"one").unwrap();
        journal.create_event_once("two", b"second").unwrap();
        std::fs::write(journal.events_dir().join("stray.txt"), b"stray").unwrap();

        let point = counting_scope('1');
        {
            let _guard = point.enter();
            assert_eq!(
                journal.read_event_bytes("one").unwrap(),
                Some(b"one".to_vec())
            );
        }
        let point = point.snapshot().counters;
        assert_eq!(point.directory_entries_walked, 0);
        assert_eq!(point.carrier_opens, 1);
        assert_eq!(point.carrier_bytes_read, 3);

        let list = counting_scope('2');
        let entries = {
            let _guard = list.enter();
            journal.list_event_entries().unwrap()
        };
        assert_eq!(entries.len(), 2);
        let list = list.snapshot().counters;
        assert_eq!(list.directory_entries_walked, 3);
        assert_eq!(list.carrier_opens, 2);
        assert_eq!(list.carrier_bytes_read, 9);

        let head = counting_scope('3');
        let marker = {
            let _guard = head.enter();
            journal.head_marker().unwrap()
        };
        assert_eq!(marker, 2);
        let head = head.snapshot().counters;
        assert_eq!(head.directory_entries_walked, 3);
        assert_eq!(head.carrier_opens, 0);
        assert_eq!(head.carrier_bytes_read, 0);
    }
}
