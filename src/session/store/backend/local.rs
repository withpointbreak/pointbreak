//! File-backed implementations of the durable backend traits over
//! [`LocalStorage`]. These preserve today's on-disk layout, hash-sorted listing
//! order, and atomic create-if-absent semantics, so a backend swap is invisible
//! to every stored byte.

use std::path::{Path, PathBuf};

use super::{
    ContentStore, Journal, JournalChangeCheck, JournalChangeStamp, JournalCreatedTransition,
    JournalCreatedTransitionVerdict, JournalEntry,
};
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

#[derive(Debug)]
pub(crate) struct JournalCreateObservation {
    #[cfg(not(target_os = "linux"))]
    before: JournalChangeStamp,
    #[cfg(target_os = "linux")]
    watcher: LinuxCreateWatcher,
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

    fn begin_created_transition(
        &self,
        _before: &JournalChangeStamp,
        _idempotency_key: &str,
    ) -> Result<JournalCreateObservation> {
        #[cfg(target_os = "linux")]
        let watcher = LinuxCreateWatcher::new(
            &self.events_dir(),
            format!("{}.json", event_filename_stem(_idempotency_key)),
        )?;
        Ok(JournalCreateObservation {
            #[cfg(not(target_os = "linux"))]
            before: _before.clone(),
            #[cfg(target_os = "linux")]
            watcher,
        })
    }

    fn finish_created_transition(
        &self,
        observation: JournalCreateObservation,
    ) -> Result<JournalCreatedTransition> {
        #[cfg(target_os = "linux")]
        {
            let verdict = observation.watcher.finish()?;
            Ok(JournalCreatedTransition {
                after: self.change_stamp()?,
                verdict,
                mechanism: "inotify interval around one governed carrier publication".to_owned(),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.created_transition(&observation.before)
        }
    }

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

    fn created_transition(&self, before: &JournalChangeStamp) -> Result<JournalCreatedTransition> {
        #[cfg(windows)]
        {
            super::ntfs_journal::created_transition(&self.events_dir(), before)
        }
        #[cfg(target_os = "macos")]
        {
            let after = self.change_stamp()?;
            let expected_count = before
                .entry_count()
                .map(|count| count.checked_add(1))
                .unwrap_or(Some(1));
            let accepted = expected_count == after.entry_count() && before != &after;
            Ok(JournalCreatedTransition {
                after,
                verdict: if accepted {
                    JournalCreatedTransitionVerdict::Accepted
                } else {
                    JournalCreatedTransitionVerdict::Contended
                },
                mechanism: "APFS directory entry count must advance by exactly one".to_owned(),
            })
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let after = self.change_stamp()?;
            Ok(JournalCreatedTransition {
                after,
                verdict: JournalCreatedTransitionVerdict::Indeterminate,
                mechanism: "platform has no qualified single-create transition proof".to_owned(),
            })
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct LinuxCreateWatcher {
    descriptor: std::os::fd::OwnedFd,
    expected_filename: String,
}

#[cfg(target_os = "linux")]
impl LinuxCreateWatcher {
    fn new(events_dir: &Path, expected_filename: String) -> Result<Self> {
        use std::os::fd::FromRawFd as _;
        use std::os::unix::ffi::OsStrExt as _;

        let path = std::ffi::CString::new(events_dir.as_os_str().as_bytes()).map_err(|_| {
            ShoreError::Message(format!(
                "journal events directory contains an interior NUL: {}",
                events_dir.display()
            ))
        })?;
        // SAFETY: no borrowed pointer crosses the call; the returned descriptor
        // is transferred immediately to `OwnedFd` on success.
        let raw = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if raw < 0 {
            return Err(ShoreError::Message(format!(
                "could not initialize journal publication watch: {}",
                std::io::Error::last_os_error()
            )));
        }
        // SAFETY: `raw` is a fresh owned descriptor from `inotify_init1`.
        let descriptor = unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) };
        // SAFETY: `path` is NUL-terminated and the descriptor remains owned.
        let watch = unsafe {
            libc::inotify_add_watch(
                std::os::fd::AsRawFd::as_raw_fd(&descriptor),
                path.as_ptr(),
                libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_Q_OVERFLOW,
            )
        };
        if watch < 0 {
            return Err(ShoreError::Message(format!(
                "could not watch journal publication directory {}: {}",
                events_dir.display(),
                std::io::Error::last_os_error()
            )));
        }
        Ok(Self {
            descriptor,
            expected_filename,
        })
    }

    fn finish(self) -> Result<JournalCreatedTransitionVerdict> {
        use std::os::fd::AsRawFd as _;

        let mut observed_expected = false;
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            // SAFETY: the descriptor is valid and `buffer` is writable for its
            // full declared length.
            let read = unsafe {
                libc::read(
                    self.descriptor.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            };
            if read < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    break;
                }
                return Err(ShoreError::Message(format!(
                    "could not read journal publication watch: {error}"
                )));
            }
            if read == 0 {
                break;
            }
            let mut offset = 0_usize;
            let read = read as usize;
            while offset < read {
                let header_size = std::mem::size_of::<libc::inotify_event>();
                if read - offset < header_size {
                    return Ok(JournalCreatedTransitionVerdict::Indeterminate);
                }
                // SAFETY: the complete header is present and an unaligned read
                // produces an owned value.
                let event = unsafe {
                    std::ptr::read_unaligned(
                        buffer.as_ptr().add(offset).cast::<libc::inotify_event>(),
                    )
                };
                let event_len = header_size.checked_add(event.len as usize).ok_or_else(|| {
                    ShoreError::Message("inotify event length overflowed".to_owned())
                })?;
                if event_len > read - offset {
                    return Ok(JournalCreatedTransitionVerdict::Indeterminate);
                }
                if event.mask & libc::IN_Q_OVERFLOW != 0 {
                    return Ok(JournalCreatedTransitionVerdict::Indeterminate);
                }
                let name_bytes = &buffer[offset + header_size..offset + event_len];
                let name_end = name_bytes
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(name_bytes.len());
                let name = std::str::from_utf8(&name_bytes[..name_end]).map_err(|_| {
                    ShoreError::Message(
                        "journal publication watch returned a non-UTF-8 name".to_owned(),
                    )
                })?;
                if name == self.expected_filename {
                    observed_expected = true;
                } else if !is_temp_file_path(Path::new(name)) {
                    return Ok(JournalCreatedTransitionVerdict::Contended);
                }
                offset += event_len;
            }
        }
        Ok(if observed_expected {
            JournalCreatedTransitionVerdict::Accepted
        } else {
            JournalCreatedTransitionVerdict::Indeterminate
        })
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

    fn change_stamp(&self) -> Result<JournalChangeStamp> {
        let events_dir = self.events_dir();
        let metadata = match std::fs::metadata(&events_dir) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(JournalChangeStamp::Absent);
            }
            Err(error) => {
                return Err(ShoreError::Message(format!(
                    "could not inspect journal events directory {}: {error}",
                    events_dir.display()
                )));
            }
        };
        if !metadata.is_dir() {
            return Err(ShoreError::Message(format!(
                "journal events path is not a directory: {}",
                events_dir.display()
            )));
        }
        let canonical = std::fs::canonicalize(&events_dir).map_err(|error| {
            ShoreError::Message(format!(
                "could not canonicalize journal events directory {}: {error}",
                events_dir.display()
            ))
        })?;
        local_directory_stamp(&canonical, &metadata)
    }

    fn changes_since(&self, before: &JournalChangeStamp) -> Result<JournalChangeCheck> {
        if matches!(before, JournalChangeStamp::Absent) {
            return Ok(JournalChangeStamp::compared(before, self.change_stamp()?));
        }
        #[cfg(windows)]
        {
            super::ntfs_journal::changes_since(&self.events_dir(), before)
        }
        #[cfg(not(windows))]
        {
            Ok(JournalChangeStamp::compared(before, self.change_stamp()?))
        }
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

#[cfg(all(unix, not(target_os = "macos")))]
fn local_directory_stamp(path: &Path, metadata: &std::fs::Metadata) -> Result<JournalChangeStamp> {
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::MetadataExt as _;

    let mut identity = b"unix-directory-identity-v1\0".to_vec();
    identity.extend_from_slice(path.as_os_str().as_bytes());
    identity.extend_from_slice(&metadata.dev().to_le_bytes());
    identity.extend_from_slice(&metadata.ino().to_le_bytes());
    let mut change = b"unix-directory-change-v1\0".to_vec();
    change.extend_from_slice(&metadata.mtime().to_le_bytes());
    change.extend_from_slice(&metadata.mtime_nsec().to_le_bytes());
    change.extend_from_slice(&metadata.ctime().to_le_bytes());
    change.extend_from_slice(&metadata.ctime_nsec().to_le_bytes());
    change.extend_from_slice(&metadata.len().to_le_bytes());
    Ok(JournalChangeStamp::observed(&identity, &change))
}

#[cfg(target_os = "macos")]
fn local_directory_stamp(path: &Path, metadata: &std::fs::Metadata) -> Result<JournalChangeStamp> {
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::MetadataExt as _;

    let entry_count = macos_directory_entry_count(path)?;
    let mut identity = b"unix-directory-identity-v1\0".to_vec();
    identity.extend_from_slice(path.as_os_str().as_bytes());
    identity.extend_from_slice(&metadata.dev().to_le_bytes());
    identity.extend_from_slice(&metadata.ino().to_le_bytes());
    let mut change = b"unix-directory-change-v1\0".to_vec();
    change.extend_from_slice(&metadata.mtime().to_le_bytes());
    change.extend_from_slice(&metadata.mtime_nsec().to_le_bytes());
    change.extend_from_slice(&metadata.ctime().to_le_bytes());
    change.extend_from_slice(&metadata.ctime_nsec().to_le_bytes());
    change.extend_from_slice(&metadata.len().to_le_bytes());
    change.extend_from_slice(&entry_count.to_le_bytes());
    Ok(JournalChangeStamp::observed_with_entry_count(
        &identity,
        &change,
        entry_count,
    ))
}

#[cfg(target_os = "macos")]
fn macos_directory_entry_count(path: &Path) -> Result<u64> {
    use std::os::unix::ffi::OsStrExt as _;

    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        ShoreError::Message(format!(
            "journal events directory contains an interior NUL: {}",
            path.display()
        ))
    })?;
    let mut attributes = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: 0,
        dirattr: libc::ATTR_DIR_ENTRYCOUNT,
        fileattr: 0,
        forkattr: 0,
    };
    let mut output = [0_u32; 2];
    // SAFETY: `path` is NUL-terminated, the attribute list requests one u32
    // directory field, and `output` is writable for the declared size.
    let status = unsafe {
        libc::getattrlist(
            path.as_ptr(),
            (&raw mut attributes).cast(),
            output.as_mut_ptr().cast(),
            std::mem::size_of_val(&output),
            0,
        )
    };
    if status != 0 {
        return Err(ShoreError::Message(format!(
            "could not query journal directory entry count: {}",
            std::io::Error::last_os_error()
        )));
    }
    if output[0] as usize != std::mem::size_of_val(&output) {
        return Err(ShoreError::Message(format!(
            "journal directory entry-count query returned {} bytes",
            output[0]
        )));
    }
    Ok(u64::from(output[1]))
}

#[cfg(windows)]
fn local_directory_stamp(path: &Path, _metadata: &std::fs::Metadata) -> Result<JournalChangeStamp> {
    super::ntfs_journal::capture(path)
}

#[cfg(not(any(unix, windows)))]
fn local_directory_stamp(path: &Path, metadata: &std::fs::Metadata) -> Result<JournalChangeStamp> {
    Ok(JournalChangeStamp::observed(
        path.to_string_lossy().as_bytes(),
        format!("{:?}:{}", metadata.modified(), metadata.len()).as_bytes(),
    ))
}

/// Derived-access adapter over the existing durable loose-event primitives.
///
/// The dormant cursor implementation holds its own store-scoped publication
/// lock around these calls. This adapter deliberately adds no product route and
/// performs no directory listing.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug)]
pub(crate) struct QualificationLocalJournal {
    journal: LocalJournal,
    store_dir: PathBuf,
}

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

    pub(crate) fn ensure_authority_directory(&self) -> Result<()> {
        std::fs::create_dir_all(self.store_dir.join("events")).map_err(|error| {
            ShoreError::Message(format!(
                "could not create journal events directory {}: {error}",
                self.store_dir.join("events").display()
            ))
        })
    }

    /// Capture the backend-specific authority cursor without enumerating event
    /// entries. The result is persisted only in disposable derived metadata.
    pub(crate) fn change_stamp(&self) -> Result<JournalChangeStamp> {
        self.journal.change_stamp()
    }

    /// Continue from a previously persisted authority cursor. Any inability to
    /// prove one continuous interval is returned as `Indeterminate`, never as
    /// a false `Stable` verdict.
    pub(crate) fn changes_since(&self, before: &JournalChangeStamp) -> Result<JournalChangeCheck> {
        self.journal.changes_since(before)
    }

    pub(crate) fn created_transition(
        &self,
        before: &JournalChangeStamp,
    ) -> Result<JournalCreatedTransition> {
        self.journal.created_transition(before)
    }

    pub(crate) fn begin_created_transition(
        &self,
        before: &JournalChangeStamp,
        idempotency_key: &str,
    ) -> Result<JournalCreateObservation> {
        self.journal
            .begin_created_transition(before, idempotency_key)
    }

    pub(crate) fn finish_created_transition(
        &self,
        observation: JournalCreateObservation,
    ) -> Result<JournalCreatedTransition> {
        self.journal.finish_created_transition(observation)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn head_marker(&self) -> Result<u64> {
        self.journal.head_marker()
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
