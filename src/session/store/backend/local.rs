//! File-backed implementations of the durable backend traits over
//! [`LocalStorage`]. These preserve today's on-disk layout, hash-sorted listing
//! order, and atomic create-if-absent semantics, so a backend swap is invisible
//! to every stored byte.

use std::path::{Path, PathBuf};

#[cfg(any(test, feature = "bench"))]
use super::JournalChangeStamp;
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

    #[cfg(any(test, feature = "bench"))]
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
        let (identity, change) = local_directory_observation(&canonical, &metadata)?;
        Ok(JournalChangeStamp::observed(&identity, &change))
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

#[cfg(all(any(test, feature = "bench"), unix))]
fn local_directory_observation(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(Vec<u8>, Vec<u8>)> {
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
    Ok((identity, change))
}

#[cfg(all(any(test, feature = "bench"), windows))]
fn local_directory_observation(
    path: &Path,
    _metadata: &std::fs::Metadata,
) -> Result<(Vec<u8>, Vec<u8>)> {
    use std::ffi::c_void;
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_BASIC_INFO_CLASS: i32 = 0;
    const FILE_ID_INFO_CLASS: i32 = 18;
    // CTL_CODE(FILE_DEVICE_FILE_SYSTEM, 58, METHOD_NEITHER, FILE_ANY_ACCESS).
    // Include the directory's NTFS USN so the native falsifier can test a
    // stronger candidate than directory timestamps alone.
    const FSCTL_READ_FILE_USN_DATA: u32 = 0x0009_00eb;

    #[repr(C)]
    struct FileBasicInfo {
        creation_time: i64,
        last_access_time: i64,
        last_write_time: i64,
        change_time: i64,
        file_attributes: u32,
    }
    #[repr(C)]
    struct FileIdInfo {
        volume_serial_number: u64,
        file_id: [u8; 16],
    }
    #[repr(C)]
    struct ReadFileUsnData {
        minimum_major_version: u16,
        maximum_major_version: u16,
    }
    #[repr(C, align(8))]
    struct UsnRecordBuffer([u8; 80]);
    unsafe extern "system" {
        fn GetFileInformationByHandleEx(
            file: *mut c_void,
            info_class: i32,
            info: *mut c_void,
            info_size: u32,
        ) -> i32;
        fn DeviceIoControl(
            device: *mut c_void,
            control_code: u32,
            input: *mut c_void,
            input_size: u32,
            output: *mut c_void,
            output_size: u32,
            bytes_returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
    }

    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|error| {
            ShoreError::Message(format!(
                "could not open journal events directory {}: {error}",
                path.display()
            ))
        })?;
    let mut basic = std::mem::MaybeUninit::<FileBasicInfo>::zeroed();
    let mut id = std::mem::MaybeUninit::<FileIdInfo>::zeroed();
    let mut usn_request = ReadFileUsnData {
        minimum_major_version: 2,
        maximum_major_version: 2,
    };
    let mut usn_record = UsnRecordBuffer([0_u8; 80]);
    let mut usn_bytes = 0_u32;
    // SAFETY: both output pointers name correctly sized writable structures and
    // the directory handle remains open for both synchronous metadata queries.
    let basic_ok = unsafe {
        GetFileInformationByHandleEx(
            directory.as_raw_handle(),
            FILE_BASIC_INFO_CLASS,
            basic.as_mut_ptr().cast(),
            std::mem::size_of::<FileBasicInfo>() as u32,
        )
    };
    // SAFETY: same contract as the basic-info query above.
    let id_ok = unsafe {
        GetFileInformationByHandleEx(
            directory.as_raw_handle(),
            FILE_ID_INFO_CLASS,
            id.as_mut_ptr().cast(),
            std::mem::size_of::<FileIdInfo>() as u32,
        )
    };
    // SAFETY: the directory handle is synchronous, the input names the
    // documented READ_FILE_USN_DATA v2 range, and the aligned byte buffer is
    // writable for the duration of the call.
    let usn_ok = unsafe {
        DeviceIoControl(
            directory.as_raw_handle(),
            FSCTL_READ_FILE_USN_DATA,
            (&raw mut usn_request).cast(),
            std::mem::size_of::<ReadFileUsnData>() as u32,
            usn_record.0.as_mut_ptr().cast(),
            usn_record.0.len() as u32,
            &raw mut usn_bytes,
            std::ptr::null_mut(),
        )
    };
    if basic_ok == 0 || id_ok == 0 || usn_ok == 0 {
        return Err(ShoreError::Message(format!(
            "could not query journal events directory {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    // SAFETY: successful calls initialized the complete output structures.
    let basic = unsafe { basic.assume_init() };
    // SAFETY: successful calls initialized the complete output structures.
    let id = unsafe { id.assume_init() };
    if usn_bytes < 32 || u16::from_le_bytes([usn_record.0[4], usn_record.0[5]]) != 2 {
        return Err(ShoreError::Message(format!(
            "journal events directory {} returned an unsupported NTFS USN record",
            path.display()
        )));
    }
    let directory_usn = i64::from_le_bytes(
        usn_record.0[24..32]
            .try_into()
            .expect("USN v2 byte range is fixed"),
    );

    let mut identity = b"windows-directory-identity-v1\0".to_vec();
    identity.extend_from_slice(&id.volume_serial_number.to_le_bytes());
    identity.extend_from_slice(&id.file_id);
    let mut change = b"windows-directory-change-v2\0".to_vec();
    change.extend_from_slice(&basic.last_write_time.to_le_bytes());
    change.extend_from_slice(&basic.change_time.to_le_bytes());
    change.extend_from_slice(&directory_usn.to_le_bytes());
    Ok((identity, change))
}

#[cfg(all(any(test, feature = "bench"), not(any(unix, windows))))]
fn local_directory_observation(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(Vec<u8>, Vec<u8>)> {
    Ok((
        path.to_string_lossy().as_bytes().to_vec(),
        format!("{:?}:{}", metadata.modified(), metadata.len()).into_bytes(),
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
