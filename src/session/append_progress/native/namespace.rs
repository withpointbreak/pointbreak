//! Sampled generated-leaf observations inside an already retained parent.
//! No root resolution, admission, privacy proof, or continuous namespace fence.
use std::ffi::OsStr;
use std::fs::File;

#[cfg(target_os = "macos")]
use super::native_error;
use super::{BorrowedMetadata, Result, ShoreError};

#[derive(Clone, Copy, Debug)]
pub(in crate::session::append_progress) enum Role {
    Ancestor,
    CloneLock,
    CloneBinding,
    RepositoryControls,
    StoreConfiguration,
    DerivedRoot,
    Generations,
    Staging,
    Publications,
    Generation,
    GenerationDescriptor,
    GenerationReaderReceipt,
    GenerationPublication,
    GenerationLease,
    RebuildLease,
    ProjectionMain,
    ProjectionWal,
    ProjectionShm,
    ProjectionJournal,
    Authority,
    Progress,
    Enrollment,
    Incarnations,
    Events,
    Artifacts,
    Notes,
    Objects,
    Active,
    Main,
    Wal,
    Shm,
    Journal,
    Incarnation,
    Request,
    Carrier,
}

impl Role {
    fn directory(self) -> bool {
        matches!(
            self,
            Self::Ancestor
                | Self::RepositoryControls
                | Self::DerivedRoot
                | Self::Generations
                | Self::Staging
                | Self::Publications
                | Self::Generation
                | Self::Progress
                | Self::Enrollment
                | Self::Incarnations
                | Self::Events
                | Self::Artifacts
                | Self::Notes
                | Self::Objects
                | Self::Incarnation
        )
    }

    fn leaf(self, raw: &OsStr) -> Result<&str> {
        // Lossless conversion, before Path normalization; the exact ASCII
        // allowlist also rejects separators, NUL, streams and DOS device names.
        let invalid = || ShoreError::Message("invalid generated namespace leaf".into());
        let name = raw.to_str().ok_or_else(invalid)?;
        let fixed = match self {
            Self::Ancestor => {
                if name.is_empty()
                    || name == "."
                    || name == ".."
                    || name.contains(['/', '\\', ':', '\0'])
                    || name.len() > 32766
                {
                    return Err(invalid());
                }
                return Ok(name);
            }
            Self::CloneLock => "pointbreak.link.lock",
            Self::CloneBinding => "pointbreak.link.json",
            Self::RepositoryControls => ".pointbreak",
            Self::StoreConfiguration => {
                return if matches!(name, "store.json" | "store.local.json") {
                    Ok(name)
                } else {
                    Err(invalid())
                };
            }
            Self::DerivedRoot => {
                return if matches!(name, "derived" | ".pointbreak-derived") {
                    Ok(name)
                } else {
                    Err(invalid())
                };
            }
            Self::Generations => "generations",
            Self::Staging => "staging",
            Self::Publications => "publications",
            Self::Generation => {
                return if !name.is_empty()
                    && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    Ok(name)
                } else {
                    Err(invalid())
                };
            }
            Self::GenerationDescriptor => "generation.json",
            Self::GenerationReaderReceipt => "change-reader-profile.json",
            Self::GenerationPublication => {
                let token = name.strip_suffix(".json").ok_or_else(invalid)?;
                let (sequence, generation) = token.split_once('-').ok_or_else(invalid)?;
                return if sequence.len() == 20
                    && sequence.bytes().all(|b| b.is_ascii_digit())
                    && !generation.is_empty()
                    && generation
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    Ok(name)
                } else {
                    Err(invalid())
                };
            }
            Self::GenerationLease => {
                let token = name
                    .strip_prefix("derived.generation-lease-")
                    .or_else(|| name.strip_prefix(".pointbreak-derived.generation-lease-"))
                    .and_then(|n| n.strip_suffix(".lock"))
                    .ok_or_else(invalid)?;
                return if !token.is_empty()
                    && token
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    Ok(name)
                } else {
                    Err(invalid())
                };
            }
            Self::RebuildLease => {
                return if matches!(
                    name,
                    "derived.rebuild.lock" | ".pointbreak-derived.rebuild.lock"
                ) {
                    Ok(name)
                } else {
                    Err(invalid())
                };
            }
            Self::ProjectionMain => "cursor.sqlite3",
            Self::ProjectionWal => "cursor.sqlite3-wal",
            Self::ProjectionShm => "cursor.sqlite3-shm",
            Self::ProjectionJournal => "cursor.sqlite3-journal",
            Self::Authority => "authority.writer.lock",
            Self::Progress => "append-progress",
            Self::Enrollment => "enrollment",
            Self::Incarnations => "incarnations",
            Self::Events => "events",
            Self::Artifacts => "artifacts",
            Self::Notes => "notes",
            Self::Objects => "objects",
            Self::Active => "active.json",
            Self::Main => "ledger.sqlite",
            Self::Wal => "ledger.sqlite-wal",
            Self::Shm => "ledger.sqlite-shm",
            Self::Journal => "ledger.sqlite-journal",
            Self::Incarnation | Self::Request | Self::Carrier => {
                let token = if matches!(self, Self::Incarnation) {
                    name
                } else {
                    name.strip_suffix(".json").ok_or_else(invalid)?
                };
                if token.len() != 64
                    || !token
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(invalid());
                }
                return Ok(name);
            }
        };
        if name != fixed {
            return Err(invalid());
        }
        Ok(name)
    }
}

fn require_kind(value: BorrowedMetadata, role: Role) -> Result<BorrowedMetadata> {
    if value.directory != role.directory() {
        return Err(super::platform::refused());
    }
    Ok(value)
}

/// Observe only this generated edge, with no handle escaping the call.
///
/// # Safety
/// Windows callers must be in a dedicated disposable private worker whose
/// unknown acquisition/cleanup boundary permits immediate exit(90), never in
/// shared Inspector/library execution. No overlapping query or SQL reentry is
/// allowed. The caller retains the valid parent directory and all actual native
/// owners through this call, and independently proves parent/root currentness
/// before treating absence as permission. This is not a gate constructor.
pub(in crate::session::append_progress) unsafe fn observe_private_worker(
    parent: &File,
    raw: &OsStr,
    role: Role,
) -> Result<Option<BorrowedMetadata>> {
    let leaf = role.leaf(raw)?;
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd as _;
        let name = std::ffi::CString::new(leaf).expect("validated ASCII leaf");
        let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
        // SAFETY: retained parent, validated single component and sized output;
        // no child descriptor is opened, duplicated, or closed.
        if unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            let error = std::io::Error::last_os_error();
            return if error.raw_os_error() == Some(libc::ENOENT) {
                Ok(None)
            } else {
                Err(native_error(error))
            };
        }
        // SAFETY: successful fstatat initialized stat.
        let stat = unsafe { stat.assume_init() };
        let directory = match stat.st_mode & libc::S_IFMT {
            libc::S_IFDIR => true,
            libc::S_IFREG => false,
            _ => return Err(super::platform::refused()),
        };
        require_kind(
            BorrowedMetadata {
                identity: crate::session::store::authority_lock::identity_from_stat(&stat),
                directory,
                mode: u32::from(stat.st_mode) & 0o777,
            },
            role,
        )
        .map(Some)
    }
    #[cfg(windows)]
    {
        windows::observe(parent, leaf, role)
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::os::windows::io::{AsRawHandle, BorrowedHandle};
    use std::ptr;

    use super::*;

    type Handle = *mut c_void;
    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *mut u16,
    }
    #[repr(C)]
    struct ObjectAttributes {
        length: u32,
        root: Handle,
        name: *mut UnicodeString,
        attributes: u32,
        security: *mut c_void,
        qos: *mut c_void,
    }
    #[repr(C)]
    union StatusOrPointer {
        status: i32,
        pointer: *mut c_void,
    }
    #[repr(C)]
    struct IoStatusBlock {
        value: StatusOrPointer,
        information: usize,
    }
    // Original declarations matching the Microsoft winternl/ntdef ABI.
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtCreateFile(
            file: *mut Handle,
            access: u32,
            attributes: *mut ObjectAttributes,
            status: *mut IoStatusBlock,
            allocation: *mut i64,
            file_attributes: u32,
            share: u32,
            disposition: u32,
            options: u32,
            ea: *mut c_void,
            ea_length: u32,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CloseHandle(file: Handle) -> i32;
    }

    fn terminal(first: Option<&ShoreError>, cleanup: impl std::fmt::Display) -> ! {
        // Best-effort diagnostics retain the first functional failure. Never
        // unwind/drop owners, retry an uncertain handle, or return success.
        struct ExitOnUnwind;
        impl Drop for ExitOnUnwind {
            fn drop(&mut self) {
                // SAFETY: same private-worker boundary, even if diagnostics panic.
                crate::session::append_progress::owner::terminal()
            }
        }
        let _exit_on_unwind = ExitOnUnwind;
        use std::io::Write as _;
        let _ = writeln!(
            std::io::stderr().lock(),
            "namespace cleanup unknown: {cleanup}; first: {first:?}"
        );
        // SAFETY: reachable only under observe_private_worker's worker contract.
        crate::session::append_progress::owner::terminal()
    }

    struct Query {
        raw: Option<Handle>,
        close: unsafe extern "system" fn(Handle) -> i32,
    }
    impl Query {
        fn close_once(&mut self, first: Option<&ShoreError>) {
            if let Some(raw) = self.raw {
                // SAFETY: this guard alone owns the successful fresh open.
                if unsafe { (self.close)(raw) } == 0 {
                    terminal(first, std::io::Error::last_os_error());
                }
                // Clear only after confirmed close; Drop cannot close it twice.
                self.raw = None;
            }
        }

        fn finish(mut self, first: Result<BorrowedMetadata>) -> Result<Option<BorrowedMetadata>> {
            self.close_once(first.as_ref().err());
            first.map(Some)
        }
    }
    impl Drop for Query {
        fn drop(&mut self) {
            // Unwind has no usable result; cleanup failure is still terminal.
            self.close_once(None);
        }
    }

    fn acquired(status: i32, raw: Handle) -> Result<Option<Query>> {
        if status != 0 {
            // Pending/informational or ambiguous ownership cannot unwind the
            // call buffers or close a possibly unrelated numeric handle.
            if status >= 0 || !raw.is_null() {
                terminal(None, "unexpected acquisition state");
            }
            return if status as u32 == 0xc0000034 {
                Ok(None) // STATUS_OBJECT_NAME_NOT_FOUND, never generic NotFound
            } else {
                Err(ShoreError::Message(format!(
                    "namespace NtCreateFile status {status:#010x}"
                )))
            };
        }
        if raw.is_null() || raw as isize == -1 {
            terminal(None, "successful open without a valid handle");
        }
        Ok(Some(Query {
            raw: Some(raw),
            close: CloseHandle,
        }))
    }

    pub(super) fn observe(
        parent: &File,
        leaf: &str,
        role: Role,
    ) -> Result<Option<BorrowedMetadata>> {
        let mut encoded: Vec<u16> = leaf.encode_utf16().collect();
        let mut name = UnicodeString {
            length: (encoded.len() * 2) as u16,
            maximum_length: (encoded.len() * 2) as u16,
            buffer: encoded.as_mut_ptr(),
        };
        let mut attributes = ObjectAttributes {
            length: std::mem::size_of::<ObjectAttributes>() as u32,
            root: parent.as_raw_handle(),
            name: &mut name,
            attributes: 0x40, // OBJ_CASE_INSENSITIVE, no inheritance
            security: ptr::null_mut(),
            qos: ptr::null_mut(),
        };
        let mut iosb = IoStatusBlock {
            value: StatusOrPointer {
                pointer: ptr::null_mut(),
            },
            information: 0,
        };
        let mut raw = ptr::null_mut();
        // SAFETY: live borrowed parent; counted generated leaf and C outputs
        // remain live through the synchronous call. No create/data/delete access.
        let status = unsafe {
            NtCreateFile(
                &mut raw,
                0x00100080, // SYNCHRONIZE | FILE_READ_ATTRIBUTES
                &mut attributes,
                &mut iosb,
                ptr::null_mut(),
                0,
                7,          // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
                1,          // FILE_OPEN
                0x00200020, // FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT
                ptr::null_mut(),
                0,
            )
        };
        let Some(query) = acquired(status, raw)? else {
            return Ok(None);
        };
        let first = if iosb.information != 1 {
            // FILE_OPENED
            Err(super::super::platform::refused())
        } else {
            // SAFETY: guard owns raw until finish; no owning conversion/dup.
            super::super::borrowed_metadata(unsafe { BorrowedHandle::borrow_raw(raw) })
                .and_then(|value| require_kind(value, role))
        };
        query.finish(first)
    }

    #[cfg(test)]
    mod tests;
}

#[cfg(test)]
mod tests;
