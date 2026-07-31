//! Bounded NTFS change-journal observation for derived-access authority.
//! The parser and continuity rules are platform-neutral so their fail-closed
//! behavior is testable off Windows; native I/O stays behind `cfg(windows)`.

const USN_V2_MINIMUM_RECORD_LENGTH: usize = 60;
#[cfg(windows)]
const DEFAULT_MAX_BYTES: u64 = 1024 * 1024;
#[cfg(windows)]
const DEFAULT_MAX_RECORDS: u64 = 8192;

#[derive(Debug, Eq, PartialEq)]
struct ParsedPage {
    next_usn: i64,
    records_examined: u64,
    relevant_file_references: Vec<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContinuityFailure {
    SourceIdentityChanged,
    JournalIdentityChanged,
    CursorOutsideRetainedInterval,
}

fn validate_continuity(
    before: &super::JournalNativeCursor,
    current: &super::JournalNativeCursor,
    retained_start_usn: i64,
) -> Result<(), ContinuityFailure> {
    if current.volume_serial_number != before.volume_serial_number
        || current.directory_file_reference != before.directory_file_reference
    {
        return Err(ContinuityFailure::SourceIdentityChanged);
    }
    if current.journal_id != before.journal_id {
        return Err(ContinuityFailure::JournalIdentityChanged);
    }
    if before.next_usn < retained_start_usn || before.next_usn > current.next_usn {
        return Err(ContinuityFailure::CursorOutsideRetainedInterval);
    }
    Ok(())
}

fn parse_page(
    bytes: &[u8],
    target_usn: i64,
    directory_file_reference: u64,
    remaining_records: u64,
) -> Result<ParsedPage, String> {
    if bytes.len() < 8 {
        return Err("NTFS journal page omitted its continuation USN".to_owned());
    }
    let next_usn = read_i64(bytes, 0)?;
    let mut offset = 8_usize;
    let mut records_examined = 0_u64;
    let mut relevant_file_references = Vec::new();
    while offset < bytes.len() {
        if bytes.len() - offset < 8 {
            return Err("NTFS journal page ended inside a record header".to_owned());
        }
        let record_length = read_u32(bytes, offset)? as usize;
        let record_end = offset
            .checked_add(record_length)
            .ok_or_else(|| "NTFS journal record length overflowed".to_owned())?;
        if record_length < USN_V2_MINIMUM_RECORD_LENGTH || record_end > bytes.len() {
            return Err("NTFS journal record length is invalid".to_owned());
        }
        let major_version = read_u16(bytes, offset + 4)?;
        if major_version != 2 {
            return Err(format!(
                "NTFS journal returned unsupported record version {major_version}"
            ));
        }
        let record_usn = read_i64(bytes, offset + 24)?;
        if record_usn >= target_usn {
            break;
        }
        records_examined = records_examined
            .checked_add(1)
            .ok_or_else(|| "NTFS journal record count overflowed".to_owned())?;
        if records_examined > remaining_records {
            return Err("NTFS journal record work cap was exhausted".to_owned());
        }
        let parent_reference = read_u64(bytes, offset + 16)?;
        let file_reference = read_u64(bytes, offset + 8)?;
        let name_length = read_u16(bytes, offset + 56)? as usize;
        let name_offset = read_u16(bytes, offset + 58)? as usize;
        let name_end = name_offset
            .checked_add(name_length)
            .ok_or_else(|| "NTFS journal file-name range overflowed".to_owned())?;
        if !name_length.is_multiple_of(2)
            || name_offset < USN_V2_MINIMUM_RECORD_LENGTH
            || name_end > record_length
        {
            return Err("NTFS journal file-name range is invalid".to_owned());
        }
        let name_bytes = &bytes[offset + name_offset..offset + name_end];
        let _name = decode_utf16(name_bytes)?;
        // The unprivileged control code strips file names but retains the
        // parent file reference. Conservatively invalidate on any direct child
        // record under `events/`; unrelated/temp children may trigger an audit,
        // while a supported carrier publication cannot hide.
        if parent_reference == directory_file_reference {
            if !relevant_file_references.contains(&file_reference) {
                relevant_file_references.push(file_reference);
            }
        }
        offset = record_end;
    }
    Ok(ParsedPage {
        next_usn,
        records_examined,
        relevant_file_references,
    })
}

fn decode_utf16(bytes: &[u8]) -> Result<String, String> {
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .map_err(|_| "NTFS journal file name is not valid UTF-16".to_owned())
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    bytes
        .get(offset..offset + 2)
        .and_then(|value| value.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| "NTFS journal record is truncated".to_owned())
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| "NTFS journal record is truncated".to_owned())
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, String> {
    bytes
        .get(offset..offset + 8)
        .and_then(|value| value.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| "NTFS journal record is truncated".to_owned())
}

fn read_i64(bytes: &[u8], offset: usize) -> Result<i64, String> {
    bytes
        .get(offset..offset + 8)
        .and_then(|value| value.try_into().ok())
        .map(i64::from_le_bytes)
        .ok_or_else(|| "NTFS journal record is truncated".to_owned())
}

#[cfg(windows)]
mod native {
    use std::ffi::{OsStr, OsString, c_void};
    use std::fs::{File, OpenOptions};
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use std::path::{Path, PathBuf};

    use super::super::{
        JournalChangeCheck, JournalChangeStamp, JournalChangeVerdict, JournalCreatedTransition,
        JournalCreatedTransitionVerdict, JournalNativeCursor,
    };
    use super::{
        ContinuityFailure, DEFAULT_MAX_BYTES, DEFAULT_MAX_RECORDS, parse_page, validate_continuity,
    };
    use crate::error::{Result, ShoreError};

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_ID_INFO_CLASS: i32 = 18;
    const FSCTL_QUERY_USN_JOURNAL: u32 = 0x0009_00f4;
    const FSCTL_READ_UNPRIVILEGED_USN_JOURNAL: u32 = 0x0009_03ab;
    const JOURNAL_READ_BUFFER_BYTES: usize = 64 * 1024;

    #[repr(C)]
    struct FileIdInfo {
        volume_serial_number: u64,
        file_id: [u8; 16],
    }

    #[repr(C)]
    struct FileTime {
        low_date_time: u32,
        high_date_time: u32,
    }

    #[repr(C)]
    struct ByHandleFileInformation {
        file_attributes: u32,
        creation_time: FileTime,
        last_access_time: FileTime,
        last_write_time: FileTime,
        volume_serial_number: u32,
        file_size_high: u32,
        file_size_low: u32,
        number_of_links: u32,
        file_index_high: u32,
        file_index_low: u32,
    }

    #[repr(C)]
    struct ReadUsnJournalDataV1 {
        start_usn: i64,
        reason_mask: u32,
        return_only_on_close: u32,
        timeout: u64,
        bytes_to_wait_for: u64,
        usn_journal_id: u64,
        min_major_version: u16,
        max_major_version: u16,
    }

    #[derive(Debug)]
    struct CurrentObservation {
        stamp: JournalChangeStamp,
        cursor: JournalNativeCursor,
        retained_start_usn: i64,
        volume: File,
    }

    unsafe extern "system" {
        fn GetFileInformationByHandleEx(
            file: *mut c_void,
            info_class: i32,
            info: *mut c_void,
            info_size: u32,
        ) -> i32;
        fn GetFileInformationByHandle(file: *mut c_void, info: *mut ByHandleFileInformation)
        -> i32;
        fn GetVolumePathNameW(
            file_name: *const u16,
            volume_path_name: *mut u16,
            buffer_length: u32,
        ) -> i32;
        fn GetVolumeNameForVolumeMountPointW(
            volume_mount_point: *const u16,
            volume_name: *mut u16,
            buffer_length: u32,
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

    pub(crate) fn capture(events_dir: &Path) -> Result<JournalChangeStamp> {
        Ok(observe_current(events_dir)?.stamp)
    }

    pub(crate) fn changes_since(
        events_dir: &Path,
        before: &JournalChangeStamp,
    ) -> Result<JournalChangeCheck> {
        let Some(before_cursor) = before.native_cursor() else {
            return Ok(JournalChangeStamp::compared(before, capture(events_dir)?));
        };
        let current = match observe_current(events_dir) {
            Ok(current) => current,
            Err(error) => {
                return Ok(indeterminate(
                    before.clone(),
                    0,
                    0,
                    format!("could not establish current NTFS journal head: {error}"),
                ));
            }
        };
        if let Err(failure) =
            validate_continuity(before_cursor, &current.cursor, current.retained_start_usn)
        {
            let (verdict, mechanism) = match failure {
                ContinuityFailure::SourceIdentityChanged => (
                    JournalChangeVerdict::Changed,
                    "NTFS volume or events-directory identity changed",
                ),
                ContinuityFailure::JournalIdentityChanged => (
                    JournalChangeVerdict::Indeterminate,
                    "NTFS journal identity changed",
                ),
                ContinuityFailure::CursorOutsideRetainedInterval => (
                    JournalChangeVerdict::Indeterminate,
                    "saved NTFS journal cursor is outside the retained interval",
                ),
            };
            return Ok(JournalChangeCheck {
                after: current.stamp,
                verdict,
                native_bytes_examined: 0,
                native_records_examined: 0,
                relevant_file_references: Vec::new(),
                mechanism: mechanism.to_owned(),
            });
        }
        read_interval(
            current,
            before_cursor.next_usn,
            DEFAULT_MAX_BYTES,
            DEFAULT_MAX_RECORDS,
        )
    }

    pub(crate) fn created_transition(
        events_dir: &Path,
        before: &JournalChangeStamp,
    ) -> Result<JournalCreatedTransition> {
        let check = changes_since(events_dir, before)?;
        let verdict = match check.verdict {
            JournalChangeVerdict::Changed if check.relevant_file_references.len() == 1 => {
                JournalCreatedTransitionVerdict::Accepted
            }
            JournalChangeVerdict::Changed => JournalCreatedTransitionVerdict::Contended,
            JournalChangeVerdict::Stable | JournalChangeVerdict::Indeterminate => {
                JournalCreatedTransitionVerdict::Indeterminate
            }
        };
        Ok(JournalCreatedTransition {
            after: check.after,
            verdict,
            mechanism: format!(
                "{}; observed {} distinct event-directory file reference(s)",
                check.mechanism,
                check.relevant_file_references.len()
            ),
        })
    }

    fn observe_current(events_dir: &Path) -> Result<CurrentObservation> {
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(events_dir)
            .map_err(|error| native_error("open journal events directory", events_dir, error))?;
        let mut id = std::mem::MaybeUninit::<FileIdInfo>::zeroed();
        let mut legacy = std::mem::MaybeUninit::<ByHandleFileInformation>::zeroed();
        // SAFETY: each output points to a correctly sized writable structure,
        // and `directory` remains open for both synchronous queries.
        let id_ok = unsafe {
            GetFileInformationByHandleEx(
                directory.as_raw_handle(),
                FILE_ID_INFO_CLASS,
                id.as_mut_ptr().cast(),
                std::mem::size_of::<FileIdInfo>() as u32,
            )
        };
        // SAFETY: the output points to a correctly sized writable structure,
        // and the directory handle remains open for the synchronous query.
        let legacy_ok =
            unsafe { GetFileInformationByHandle(directory.as_raw_handle(), legacy.as_mut_ptr()) };
        if id_ok == 0 || legacy_ok == 0 {
            return Err(last_native_error("query journal events-directory identity"));
        }
        // SAFETY: successful calls initialized the complete structures.
        let id = unsafe { id.assume_init() };
        // SAFETY: successful calls initialized the complete structures.
        let legacy = unsafe { legacy.assume_init() };
        let volume_path = volume_guid_path(events_dir)?;
        let volume = OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(&volume_path)
            .map_err(|error| native_error("open NTFS volume root", &volume_path, error))?;
        let journal = query_journal(&volume)?;
        let cursor = JournalNativeCursor {
            journal_id: journal.journal_id,
            next_usn: journal.next_usn,
            directory_file_reference: (u64::from(legacy.file_index_high) << 32)
                | u64::from(legacy.file_index_low),
            volume_serial_number: id.volume_serial_number,
        };
        let mut identity = b"windows-usn-authority-identity-v1\0".to_vec();
        identity.extend_from_slice(&cursor.volume_serial_number.to_le_bytes());
        identity.extend_from_slice(&cursor.directory_file_reference.to_le_bytes());
        let mut change = b"windows-usn-authority-cursor-v1\0".to_vec();
        change.extend_from_slice(&cursor.journal_id.to_le_bytes());
        change.extend_from_slice(&cursor.next_usn.to_le_bytes());
        let stamp =
            JournalChangeStamp::observed_with_native_cursor(&identity, &change, cursor.clone());
        Ok(CurrentObservation {
            stamp,
            cursor,
            retained_start_usn: journal.first_usn.max(journal.lowest_valid_usn),
            volume,
        })
    }

    #[derive(Clone, Copy, Debug)]
    struct JournalData {
        journal_id: u64,
        first_usn: i64,
        next_usn: i64,
        lowest_valid_usn: i64,
    }

    fn query_journal(volume: &File) -> Result<JournalData> {
        let mut words = [0_u64; 16];
        let mut returned = 0_u32;
        // SAFETY: `words` is aligned writable storage and the volume-root handle
        // remains open for this synchronous query.
        let ok = unsafe {
            DeviceIoControl(
                volume.as_raw_handle(),
                FSCTL_QUERY_USN_JOURNAL,
                std::ptr::null_mut(),
                0,
                words.as_mut_ptr().cast(),
                std::mem::size_of_val(&words) as u32,
                &raw mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(last_native_error("query NTFS USN journal"));
        }
        if returned < 32 {
            return Err(ShoreError::Message(
                "NTFS USN journal query returned a truncated header".to_owned(),
            ));
        }
        let bytes = bytemuck_words(&words);
        Ok(JournalData {
            journal_id: super::read_u64(bytes, 0).map_err(ShoreError::Message)?,
            first_usn: super::read_i64(bytes, 8).map_err(ShoreError::Message)?,
            next_usn: super::read_i64(bytes, 16).map_err(ShoreError::Message)?,
            lowest_valid_usn: super::read_i64(bytes, 24).map_err(ShoreError::Message)?,
        })
    }

    #[cfg(test)]
    pub(super) fn changes_since_with_limits(
        events_dir: &Path,
        before: &JournalChangeStamp,
        max_bytes: u64,
        max_records: u64,
    ) -> Result<JournalChangeCheck> {
        let Some(before_cursor) = before.native_cursor() else {
            return Ok(JournalChangeStamp::compared(before, capture(events_dir)?));
        };
        let current = match observe_current(events_dir) {
            Ok(current) => current,
            Err(error) => {
                return Ok(indeterminate(
                    before.clone(),
                    0,
                    0,
                    format!("could not establish current NTFS journal head: {error}"),
                ));
            }
        };
        if validate_continuity(before_cursor, &current.cursor, current.retained_start_usn).is_err()
        {
            return changes_since(events_dir, before);
        }
        read_interval(current, before_cursor.next_usn, max_bytes, max_records)
    }

    fn read_interval(
        current: CurrentObservation,
        mut start_usn: i64,
        max_bytes: u64,
        max_records: u64,
    ) -> Result<JournalChangeCheck> {
        let target_usn = current.cursor.next_usn;
        let mut bytes_examined = 0_u64;
        let mut records_examined = 0_u64;
        let mut relevant_file_references = Vec::new();
        let mut storage = vec![0_u64; JOURNAL_READ_BUFFER_BYTES / 8];
        while start_usn < target_usn {
            if bytes_examined + JOURNAL_READ_BUFFER_BYTES as u64 > max_bytes {
                return Ok(indeterminate(
                    current.stamp,
                    bytes_examined,
                    records_examined,
                    "NTFS journal byte work cap was exhausted".to_owned(),
                ));
            }
            let mut request = ReadUsnJournalDataV1 {
                start_usn,
                reason_mask: u32::MAX,
                return_only_on_close: 0,
                timeout: 0,
                bytes_to_wait_for: 0,
                usn_journal_id: current.cursor.journal_id,
                min_major_version: 2,
                max_major_version: 2,
            };
            let mut returned = 0_u32;
            // SAFETY: request and aligned output storage remain valid for this
            // synchronous unprivileged journal read.
            let ok = unsafe {
                DeviceIoControl(
                    current.volume.as_raw_handle(),
                    FSCTL_READ_UNPRIVILEGED_USN_JOURNAL,
                    (&raw mut request).cast(),
                    std::mem::size_of::<ReadUsnJournalDataV1>() as u32,
                    storage.as_mut_ptr().cast(),
                    (storage.len() * 8) as u32,
                    &raw mut returned,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Ok(indeterminate(
                    current.stamp,
                    bytes_examined,
                    records_examined,
                    format!(
                        "unprivileged NTFS journal read failed: {}",
                        std::io::Error::last_os_error()
                    ),
                ));
            }
            let returned = returned as usize;
            if returned < 8 || returned > storage.len() * 8 {
                return Ok(indeterminate(
                    current.stamp,
                    bytes_examined,
                    records_examined,
                    "unprivileged NTFS journal read returned an invalid byte count".to_owned(),
                ));
            }
            bytes_examined += returned as u64;
            let page = match parse_page(
                &bytemuck_words(&storage)[..returned],
                target_usn,
                current.cursor.directory_file_reference,
                max_records.saturating_sub(records_examined),
            ) {
                Ok(page) => page,
                Err(reason) => {
                    return Ok(indeterminate(
                        current.stamp,
                        bytes_examined,
                        records_examined,
                        reason,
                    ));
                }
            };
            records_examined += page.records_examined;
            for file_reference in page.relevant_file_references {
                if !relevant_file_references.contains(&file_reference) {
                    relevant_file_references.push(file_reference);
                }
            }
            if page.next_usn <= start_usn {
                return Ok(indeterminate(
                    current.stamp,
                    bytes_examined,
                    records_examined,
                    "unprivileged NTFS journal cursor did not advance".to_owned(),
                ));
            }
            start_usn = page.next_usn.min(target_usn);
        }
        let relevant_change = !relevant_file_references.is_empty();
        Ok(JournalChangeCheck {
            after: current.stamp,
            verdict: if relevant_change {
                JournalChangeVerdict::Changed
            } else {
                JournalChangeVerdict::Stable
            },
            native_bytes_examined: bytes_examined,
            native_records_examined: records_examined,
            relevant_file_references,
            mechanism: if relevant_change {
                "continuous NTFS journal interval contains an event-carrier change".to_owned()
            } else {
                "continuous bounded NTFS journal interval contains no event-carrier change"
                    .to_owned()
            },
        })
    }

    fn volume_guid_path(path: &Path) -> Result<PathBuf> {
        let input = wide_null(path.as_os_str());
        let mut mount = vec![0_u16; 32_768];
        // SAFETY: input is NUL-terminated and output is writable for the stated
        // number of UTF-16 units.
        let mount_ok =
            unsafe { GetVolumePathNameW(input.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) };
        if mount_ok == 0 {
            return Err(last_native_error("resolve NTFS volume mount point"));
        }
        let mount_len = mount.iter().position(|unit| *unit == 0).ok_or_else(|| {
            ShoreError::Message("NTFS volume mount point was not terminated".to_owned())
        })?;
        mount.truncate(mount_len + 1);
        let mut volume = vec![0_u16; 1024];
        // SAFETY: mount is NUL-terminated and output is writable for the stated
        // number of UTF-16 units.
        let volume_ok = unsafe {
            GetVolumeNameForVolumeMountPointW(
                mount.as_ptr(),
                volume.as_mut_ptr(),
                volume.len() as u32,
            )
        };
        if volume_ok == 0 {
            return Err(last_native_error("resolve NTFS volume GUID path"));
        }
        let volume_len = volume.iter().position(|unit| *unit == 0).ok_or_else(|| {
            ShoreError::Message("NTFS volume GUID path was not terminated".to_owned())
        })?;
        Ok(PathBuf::from(OsString::from_wide(&volume[..volume_len])))
    }

    fn wide_null(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn bytemuck_words(words: &[u64]) -> &[u8] {
        // SAFETY: `u64` storage is contiguous, and viewing initialized bytes for
        // parsing does not outlive the slice.
        unsafe { std::slice::from_raw_parts(words.as_ptr().cast(), std::mem::size_of_val(words)) }
    }

    fn indeterminate(
        after: JournalChangeStamp,
        bytes: u64,
        records: u64,
        mechanism: String,
    ) -> JournalChangeCheck {
        JournalChangeCheck {
            after,
            verdict: JournalChangeVerdict::Indeterminate,
            native_bytes_examined: bytes,
            native_records_examined: records,
            relevant_file_references: Vec::new(),
            mechanism,
        }
    }

    fn last_native_error(action: &str) -> ShoreError {
        ShoreError::Message(format!(
            "could not {action}: {}",
            std::io::Error::last_os_error()
        ))
    }

    fn native_error(action: &str, path: &Path, error: std::io::Error) -> ShoreError {
        ShoreError::Message(format!("could not {action} {}: {error}", path.display()))
    }
}

#[cfg(windows)]
pub(super) use native::{capture, changes_since, created_transition};

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor(journal_id: u64, next_usn: i64) -> super::super::JournalNativeCursor {
        super::super::JournalNativeCursor {
            journal_id,
            next_usn,
            directory_file_reference: 41,
            volume_serial_number: 73,
        }
    }

    #[test]
    fn continuity_accepts_only_the_retained_interval_on_the_same_source() {
        let before = cursor(11, 50);
        let current = cursor(11, 80);
        assert_eq!(validate_continuity(&before, &current, 40), Ok(()));

        let mut replaced_source = current.clone();
        replaced_source.directory_file_reference += 1;
        assert_eq!(
            validate_continuity(&before, &replaced_source, 40),
            Err(ContinuityFailure::SourceIdentityChanged)
        );

        assert_eq!(
            validate_continuity(&before, &cursor(12, 80), 40),
            Err(ContinuityFailure::JournalIdentityChanged)
        );
        assert_eq!(
            validate_continuity(&before, &current, 51),
            Err(ContinuityFailure::CursorOutsideRetainedInterval)
        );
        assert_eq!(
            validate_continuity(&cursor(11, 81), &current, 40),
            Err(ContinuityFailure::CursorOutsideRetainedInterval)
        );
    }

    #[test]
    fn parser_finds_direct_event_carrier_before_target() {
        let page = page(
            90,
            &[record(2, 41, 7, &format!("{}.json", "ab".repeat(32)))],
        );
        let parsed = parse_page(&page, 100, 41, 8).expect("page parses");
        assert_eq!(
            parsed,
            ParsedPage {
                next_usn: 90,
                records_examined: 1,
                relevant_file_references: vec![0],
            }
        );
    }

    #[test]
    fn parser_conservatively_invalidates_any_direct_child_record() {
        let page = page(90, &[record(2, 41, 7, "")]);
        assert!(
            !parse_page(&page, 100, 41, 8)
                .expect("page parses")
                .relevant_file_references
                .is_empty()
        );
    }

    #[test]
    fn parser_retains_distinct_file_references_for_created_transition_proof() {
        let mut first = record(2, 41, 7, "");
        first[8..16].copy_from_slice(&101_u64.to_le_bytes());
        let mut second = record(2, 41, 8, "");
        second[8..16].copy_from_slice(&202_u64.to_le_bytes());
        let parsed = parse_page(&page(90, &[first, second]), 100, 41, 8).expect("page parses");

        assert_eq!(parsed.relevant_file_references, vec![101, 202]);
    }

    #[test]
    fn parser_ignores_other_parents_and_records_at_target() {
        let page = page(
            110,
            &[
                record(2, 40, 7, &format!("{}.json", "ab".repeat(32))),
                record(2, 41, 100, &format!("{}.json", "cd".repeat(32))),
            ],
        );
        let parsed = parse_page(&page, 100, 41, 8).expect("page parses");
        assert_eq!(parsed.records_examined, 1);
        assert!(parsed.relevant_file_references.is_empty());
    }

    #[test]
    fn parser_fails_closed_on_unsupported_or_malformed_records() {
        let unsupported = page(90, &[record(3, 41, 7, "name")]);
        assert!(parse_page(&unsupported, 100, 41, 8).is_err());

        let mut malformed = page(90, &[record(2, 41, 7, "name")]);
        malformed[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_page(&malformed, 100, 41, 8).is_err());
    }

    #[test]
    fn parser_fails_closed_when_record_cap_is_exhausted() {
        let page = page(90, &[record(2, 40, 7, "unrelated")]);
        assert_eq!(
            parse_page(&page, 100, 41, 0).unwrap_err(),
            "NTFS journal record work cap was exhausted"
        );
    }

    #[cfg(windows)]
    #[test]
    fn unprivileged_native_interval_detects_direct_event_create() {
        let root = tempfile::tempdir().expect("temporary root");
        let events = root.path().join("events");
        std::fs::create_dir(&events).expect("events directory");
        let first = capture(&events).expect("capture initial journal cursor");
        let unchanged = changes_since(&events, &first).expect("check unchanged interval");
        assert_eq!(
            unchanged.verdict,
            super::super::JournalChangeVerdict::Stable
        );

        let before_create = unchanged.after;
        std::fs::write(events.join(format!("{}.json", "ab".repeat(32))), b"event")
            .expect("write direct event carrier");
        let changed = changes_since(&events, &before_create).expect("check changed interval");
        assert_eq!(changed.verdict, super::super::JournalChangeVerdict::Changed);
        assert!(changed.native_bytes_examined > 0);
        assert!(changed.native_records_examined > 0);
    }

    #[cfg(windows)]
    #[test]
    fn native_created_transition_accepts_one_file_identity_and_rejects_two() {
        use super::super::JournalCreatedTransitionVerdict;

        let root = tempfile::tempdir().expect("temporary root");
        let events = root.path().join("events");
        std::fs::create_dir(&events).expect("events directory");
        let before = capture(&events).expect("capture before one create");
        std::fs::write(events.join(format!("{}.json", "11".repeat(32))), b"one")
            .expect("write one carrier");
        assert_eq!(
            created_transition(&events, &before)
                .expect("classify one create")
                .verdict,
            JournalCreatedTransitionVerdict::Accepted
        );

        let before = capture(&events).expect("capture before two creates");
        std::fs::write(events.join(format!("{}.json", "22".repeat(32))), b"two")
            .expect("write first raced carrier");
        std::fs::write(events.join(format!("{}.json", "33".repeat(32))), b"three")
            .expect("write second raced carrier");
        assert_eq!(
            created_transition(&events, &before)
                .expect("classify raced creates")
                .verdict,
            JournalCreatedTransitionVerdict::Contended
        );
    }

    #[cfg(windows)]
    #[test]
    fn unprivileged_native_interval_works_under_restricted_token() {
        use std::ffi::c_void;

        const DISABLE_MAX_PRIVILEGE: u32 = 0x1;
        const LUA_TOKEN: u32 = 0x4;
        const TOKEN_DUPLICATE: u32 = 0x0002;
        const TOKEN_IMPERSONATE: u32 = 0x0004;
        const TOKEN_QUERY: u32 = 0x0008;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut c_void;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn OpenProcessToken(
                process: *mut c_void,
                desired_access: u32,
                token: *mut *mut c_void,
            ) -> i32;
            fn CreateRestrictedToken(
                existing_token: *mut c_void,
                flags: u32,
                disable_sid_count: u32,
                sids_to_disable: *mut c_void,
                delete_privilege_count: u32,
                privileges_to_delete: *mut c_void,
                restricted_sid_count: u32,
                sids_to_restrict: *mut c_void,
                new_token: *mut *mut c_void,
            ) -> i32;
            fn ImpersonateLoggedOnUser(token: *mut c_void) -> i32;
            fn RevertToSelf() -> i32;
        }

        struct RestrictedTokenGuard(*mut c_void, *mut c_void);

        impl Drop for RestrictedTokenGuard {
            fn drop(&mut self) {
                // SAFETY: both handles were returned by the token APIs, and
                // reverting before closing ends this thread's impersonation.
                unsafe {
                    RevertToSelf();
                    CloseHandle(self.1);
                    CloseHandle(self.0);
                }
            }
        }

        let root = tempfile::tempdir().expect("temporary root");
        let events = root.path().join("events");
        std::fs::create_dir(&events).expect("events directory");
        let mut process_token = std::ptr::null_mut();
        // SAFETY: output pointers are writable and the pseudo-process handle is
        // valid for the current process.
        let opened = unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_IMPERSONATE | TOKEN_QUERY,
                &raw mut process_token,
            )
        };
        assert_ne!(opened, 0, "open current process token");
        let mut restricted_token = std::ptr::null_mut();
        // SAFETY: the source token is valid and every optional list has a zero
        // count with a null pointer.
        let restricted = unsafe {
            CreateRestrictedToken(
                process_token,
                DISABLE_MAX_PRIVILEGE | LUA_TOKEN,
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &raw mut restricted_token,
            )
        };
        assert_ne!(restricted, 0, "create restricted token");
        // SAFETY: the new token remains open through the guard's lifetime.
        let impersonated = unsafe { ImpersonateLoggedOnUser(restricted_token) };
        assert_ne!(impersonated, 0, "impersonate restricted token");
        let _guard = RestrictedTokenGuard(process_token, restricted_token);

        let first = capture(&events).expect("capture under restricted token");
        std::fs::write(events.join(format!("{}.json", "cd".repeat(32))), b"event")
            .expect("write direct event carrier under restricted token");
        let changed = changes_since(&events, &first).expect("check under restricted token");
        assert_eq!(changed.verdict, super::super::JournalChangeVerdict::Changed);
    }

    #[cfg(windows)]
    #[test]
    fn native_fast_publication_shapes_never_return_stable() {
        use super::super::JournalChangeVerdict;

        let root = tempfile::tempdir().expect("temporary root");
        let events = root.path().join("events");
        std::fs::create_dir(&events).expect("events directory");

        for index in 0..100_u64 {
            let before = capture(&events).expect("capture before direct create");
            std::fs::write(events.join(format!("{index:064x}.json")), b"direct")
                .expect("direct create");
            assert_ne!(
                changes_since(&events, &before)
                    .expect("check direct create")
                    .verdict,
                JournalChangeVerdict::Stable,
                "direct create repetition {index}"
            );

            let before = capture(&events).expect("capture before temp rename");
            let temp = events.join(format!(".{index:064x}.tmp"));
            let carrier = events.join(format!("{:064x}.json", index + 100));
            std::fs::write(&temp, b"rename").expect("temporary create");
            std::fs::rename(temp, carrier).expect("publish rename");
            assert_ne!(
                changes_since(&events, &before)
                    .expect("check temp rename")
                    .verdict,
                JournalChangeVerdict::Stable,
                "temp rename repetition {index}"
            );

            let before = capture(&events).expect("capture before concurrent create");
            let concurrent = events.join(format!("{:064x}.json", index + 200));
            std::thread::spawn(move || std::fs::write(concurrent, b"concurrent"))
                .join()
                .expect("concurrent writer thread")
                .expect("concurrent create");
            assert_ne!(
                changes_since(&events, &before)
                    .expect("check concurrent create")
                    .verdict,
                JournalChangeVerdict::Stable,
                "concurrent create repetition {index}"
            );

            let aliased = events.join("..").join("events");
            let before = capture(&aliased).expect("capture through canonical alias");
            for burst in 0..8_u64 {
                std::fs::write(
                    events.join(format!("{:064x}.json", 1_000 + index * 8 + burst)),
                    b"rapid",
                )
                .expect("rapid create");
            }
            assert_ne!(
                changes_since(&events, &before)
                    .expect("check rapid alias create")
                    .verdict,
                JournalChangeVerdict::Stable,
                "rapid alias repetition {index}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn native_volume_churn_is_counted_and_caps_fail_closed() {
        use super::super::JournalChangeVerdict;

        let root = tempfile::tempdir().expect("temporary root");
        let events = root.path().join("events");
        let churn = root.path().join("unrelated");
        std::fs::create_dir(&events).expect("events directory");
        std::fs::create_dir(&churn).expect("churn directory");

        let before = capture(&events).expect("capture before unrelated volume churn");
        for index in 0..64_u64 {
            std::fs::write(churn.join(format!("noise-{index}")), b"noise")
                .expect("unrelated volume write");
        }
        let observed = changes_since(&events, &before).expect("bounded churn observation");
        assert_eq!(observed.verdict, JournalChangeVerdict::Stable);
        assert!(observed.native_bytes_examined > 8);
        assert!(observed.native_records_examined > 0);
        assert!(observed.native_bytes_examined <= DEFAULT_MAX_BYTES);
        assert!(observed.native_records_examined <= DEFAULT_MAX_RECORDS);

        let before = capture(&events).expect("capture before byte-cap probe");
        std::fs::write(churn.join("byte-cap"), b"noise").expect("byte-cap churn");
        let byte_capped =
            native::changes_since_with_limits(&events, &before, 0, DEFAULT_MAX_RECORDS)
                .expect("byte-cap observation");
        assert_eq!(byte_capped.verdict, JournalChangeVerdict::Indeterminate);
        assert_eq!(byte_capped.native_bytes_examined, 0);

        let before = capture(&events).expect("capture before record-cap probe");
        std::fs::write(churn.join("record-cap"), b"noise").expect("record-cap churn");
        let record_capped =
            native::changes_since_with_limits(&events, &before, DEFAULT_MAX_BYTES, 0)
                .expect("record-cap observation");
        assert_eq!(record_capped.verdict, JournalChangeVerdict::Indeterminate);
    }

    fn page(next_usn: i64, records: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = next_usn.to_le_bytes().to_vec();
        for record in records {
            bytes.extend_from_slice(record);
        }
        bytes
    }

    fn record(major: u16, parent: u64, usn: i64, name: &str) -> Vec<u8> {
        let name = name.encode_utf16().collect::<Vec<_>>();
        let unaligned = USN_V2_MINIMUM_RECORD_LENGTH + name.len() * 2;
        let length = (unaligned + 7) & !7;
        let mut bytes = vec![0_u8; length];
        bytes[0..4].copy_from_slice(&(length as u32).to_le_bytes());
        bytes[4..6].copy_from_slice(&major.to_le_bytes());
        bytes[16..24].copy_from_slice(&parent.to_le_bytes());
        bytes[24..32].copy_from_slice(&usn.to_le_bytes());
        bytes[56..58].copy_from_slice(&((name.len() * 2) as u16).to_le_bytes());
        bytes[58..60].copy_from_slice(&(USN_V2_MINIMUM_RECORD_LENGTH as u16).to_le_bytes());
        for (index, unit) in name.into_iter().enumerate() {
            let offset = USN_V2_MINIMUM_RECORD_LENGTH + index * 2;
            bytes[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes
    }
}
