//! Owned file/directory barriers. Callers must hold and validate authority
//! around progress mutation. These primitives do not checkpoint SQLite, publish
//! enrollment, or qualify power-loss behavior.
use std::path::Path;

use crate::error::{Result, ShoreError};

#[cfg(any(target_os = "macos", windows))]
pub(super) mod namespace;

#[derive(Debug)]
pub(super) struct NativeRootHandle {
    #[cfg(any(target_os = "macos", windows))]
    directory: std::fs::File,
    #[cfg(any(target_os = "macos", windows))]
    ancestors: Vec<(std::path::PathBuf, std::fs::File)>,
    #[cfg(any(target_os = "macos", windows))]
    canonical: std::path::PathBuf,
    #[cfg(any(target_os = "macos", windows))]
    supplied: std::path::PathBuf,
}

/// Borrow the existing owner; no open, duplication, validation or admission.
pub(super) fn retained_root_file(root: &NativeRootHandle) -> Result<&std::fs::File> {
    #[cfg(any(target_os = "macos", windows))]
    {
        Ok(&root.directory)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = root;
        Err(unsupported("append-progress durability is unavailable"))
    }
}

/// Detached values only; the retained root and authority guard remain required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RootSnapshot {
    pub(super) volume: u64,
    pub(super) file: u128,
    pub(super) canonical: std::path::PathBuf,
}

pub(super) fn root_snapshot(root: &NativeRootHandle) -> Result<RootSnapshot> {
    #[cfg(any(target_os = "macos", windows))]
    {
        platform::validate_root(root)?;
        let (volume, file) =
            crate::session::store::authority_lock::file_identity(&root.directory)?.parts();
        let snapshot = RootSnapshot {
            volume,
            file,
            canonical: root.canonical.clone(),
        };
        platform::validate_root(root)?;
        Ok(snapshot)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = root;
        Err(unsupported("append-progress durability is unavailable"))
    }
}

pub(super) fn physical_root(root: &Path) -> Result<NativeRootHandle> {
    super::owner::require_quiescent()?;
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd as _;
        let supplied = std::path::absolute(root).map_err(native_error)?;
        let canonical = root.canonicalize().map_err(native_error)?;
        let directory = platform::open(&canonical, true)?;
        let mut stat = std::mem::MaybeUninit::<libc::statfs>::zeroed();
        // SAFETY: fstatfs receives a live descriptor and a correctly sized output.
        if unsafe { libc::fstatfs(directory.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
            return Err(native_error(std::io::Error::last_os_error()));
        }
        // SAFETY: successful fstatfs initialized the structure.
        let stat = unsafe { stat.assume_init() };
        let filesystem: Vec<u8> = stat
            .f_fstypename
            .iter()
            .take_while(|&&v| v != 0)
            .map(|&v| v as u8)
            .collect();
        if filesystem != b"apfs" {
            return Err(unsupported("append progress requires a native APFS root"));
        }
        // Unlike the existing Windows no-op helper, admission exercises the
        // actual directory barrier before returning a usable handle.
        directory.sync_all().map_err(native_error)?;
        let ancestors = retain_ancestors(&supplied, &canonical)?;
        let handle = NativeRootHandle {
            ancestors,
            directory,
            canonical,
            supplied,
        };
        platform::validate_root(&handle)?;
        Ok(handle)
    }
    #[cfg(windows)]
    {
        ntfs_candidate_root(root)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = root;
        Err(unsupported("append-progress durability is unavailable"))
    }
}

pub(super) fn sync_progress_files(root: &NativeRootHandle, database: &Path) -> Result<()> {
    #[cfg(any(target_os = "macos", windows))]
    {
        platform::sync_files(root, database, None, true)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (root, database);
        Err(unsupported("append-progress durability is unavailable"))
    }
}

/// Re-establish durability for one native logical Journal key and its exact
/// stored-byte witness. This does not validate event semantics or settle a ledger.
pub(super) fn sync_exact_carrier(
    root: &NativeRootHandle,
    key: &str,
    witness: &str,
    length: u64,
) -> Result<()> {
    if key.is_empty()
        || witness.len() != 64
        || !witness
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ShoreError::Message(
            "invalid native carrier key or SHA-256 witness".into(),
        ));
    }
    #[cfg(any(target_os = "macos", windows))]
    {
        // Exactly LocalJournal::event_path/event_filename_stem: logical keys
        // are opaque, never path components or already-hashed filenames.
        let stem = crate::canonical_hash::sha256_bytes_hex(key.as_bytes());
        let path = root.canonical.join("events").join(format!("{stem}.json"));
        platform::sync_files(
            root,
            &path,
            Some(platform::Expected::Witness { witness, length }),
            false,
        )
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (root, length);
        Err(unsupported("append-progress durability is unavailable"))
    }
}

fn unsupported(detail: &str) -> ShoreError {
    ShoreError::Message(format!("unsupported platform: {detail}"))
}

fn native_error(error: std::io::Error) -> ShoreError {
    ShoreError::Message(format!("append-progress native durability: {error}"))
}

/// Detached observations, not an admission receipt or a retained native owner.
/// Callers still owe the complete identity/security gate around their operation.
#[cfg(any(target_os = "macos", windows))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BorrowedMetadata {
    pub(super) identity: crate::session::store::authority_lock::PhysicalFileIdentity,
    pub(super) directory: bool,
    #[cfg(target_os = "macos")]
    pub(super) mode: u32,
}

/// Synchronously observe this fd without opening, duplicating or closing it.
#[cfg(target_os = "macos")]
pub(super) fn borrowed_metadata(file: std::os::fd::BorrowedFd<'_>) -> Result<BorrowedMetadata> {
    use crate::session::store::authority_lock::{borrowed_file_stat, identity_from_stat};
    let stat = borrowed_file_stat(file).map_err(native_error)?;
    let directory = match stat.st_mode & libc::S_IFMT {
        libc::S_IFDIR => true,
        libc::S_IFREG => false,
        _ => return Err(platform::refused()),
    };
    Ok(BorrowedMetadata {
        identity: identity_from_stat(&stat),
        directory,
        // Match the existing progress permission predicate, without chmod.
        mode: u32::from(stat.st_mode) & 0o777,
    })
}

#[cfg(target_os = "macos")]
pub(super) fn borrowed_private_mode(
    file: std::os::fd::BorrowedFd<'_>,
    directory: bool,
) -> Result<BorrowedMetadata> {
    borrowed_metadata(file)?.require_private_mode(directory)
}

#[cfg(target_os = "macos")]
impl BorrowedMetadata {
    pub(super) fn require_private_mode(self, directory: bool) -> Result<Self> {
        if self.directory != directory || self.mode != if directory { 0o700 } else { 0o600 } {
            return Err(platform::refused());
        }
        Ok(self)
    }
}

/// Synchronously query the actual HANDLE; no path or ownership wrapper is used.
#[cfg(windows)]
pub(super) fn borrowed_metadata(
    file: std::os::windows::io::BorrowedHandle<'_>,
) -> Result<BorrowedMetadata> {
    use std::os::windows::io::AsRawHandle as _;
    #[repr(C)]
    struct AttributeTagInfo {
        attributes: u32,
        reparse_tag: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileType(file: *mut std::ffi::c_void) -> u32;
        fn GetFileInformationByHandleEx(
            file: *mut std::ffi::c_void,
            class: i32,
            info: *mut std::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    // SAFETY: the caller keeps this borrowed HANDLE live for the query.
    match unsafe { GetFileType(file.as_raw_handle()) } {
        1 => {}
        0 => {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error().is_some_and(|code| code != 0) {
                return Err(native_error(error));
            }
            return Err(platform::refused());
        }
        _ => return Err(platform::refused()),
    }
    let mut info = std::mem::MaybeUninit::<AttributeTagInfo>::zeroed();
    // SAFETY: FileAttributeTagInfo (9) writes this exact two-DWORD structure.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            9,
            info.as_mut_ptr().cast(),
            std::mem::size_of::<AttributeTagInfo>() as u32,
        )
    } == 0
    {
        return Err(native_error(std::io::Error::last_os_error()));
    }
    // SAFETY: the successful query initialized the complete structure.
    let info = unsafe { info.assume_init() };
    if info.attributes & 0x400 != 0 {
        return Err(platform::refused());
    }
    Ok(BorrowedMetadata {
        identity: crate::session::store::authority_lock::borrowed_file_identity(file)?,
        directory: info.attributes & 0x10 != 0,
    })
}

/// Closed descriptor subset, not a general Windows effective-access evaluator.
#[cfg(any(windows, test))]
pub(super) mod private_acl {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Kind {
        Root,
        Directory,
        File,
    }

    #[derive(Debug, PartialEq, Eq)]
    pub(crate) struct Descriptor {
        owner: Vec<u8>,
        protected: bool,
        grants: Vec<(Vec<u8>, u8)>,
    }

    pub(crate) const SYSTEM: &[u8] = &[1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];

    #[derive(Debug, PartialEq, Eq)]
    pub(crate) enum TokenFailure {
        Impersonation,
        Query(i32),
    }
    pub(crate) fn absent_thread_token(opened: bool, error: i32) -> Result<(), TokenFailure> {
        match (opened, error) {
            (true, _) => Err(TokenFailure::Impersonation),
            (false, 1008) => Ok(()),
            (false, code) => Err(TokenFailure::Query(code)),
        }
    }
    /// Inspect an owned TokenUser buffer without following its embedded pointer.
    pub(crate) fn token_user(bytes: &[u8], base: usize) -> Option<&[u8]> {
        let word = std::mem::size_of::<usize>();
        let address = usize::from_ne_bytes(bytes.get(..word)?.try_into().ok()?);
        let offset = address.checked_sub(base)?;
        if offset < 2 * word || offset % 4 != 0 {
            return None;
        }
        sid(bytes.get(offset..)?).filter(|value| *value != SYSTEM)
    }

    fn word(bytes: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(
            bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
        ))
    }
    fn dword(bytes: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(
            bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
        ))
    }
    pub(crate) fn sid(bytes: &[u8]) -> Option<&[u8]> {
        if bytes.first() != Some(&1) || *bytes.get(1)? > 15 {
            return None;
        }
        bytes.get(..8 + 4 * usize::from(bytes[1]))
    }

    /// Parse only self-relative bytes. All offsets, ACE sizes and SID lengths
    /// are checked before reads; no fixture-controlled pointer is dereferenced.
    pub(crate) fn parse(bytes: &[u8], user: &[u8], kind: Kind) -> Option<Descriptor> {
        if sid(user)? != user || user == SYSTEM || bytes.len() < 20 || bytes[0] != 1 {
            return None;
        }
        let control = word(bytes, 2)?;
        if control & 0x8004 != 0x8004 || (kind == Kind::Root && control & 0x1000 == 0) {
            return None;
        }
        let owner_at = usize::try_from(dword(bytes, 4)?).ok()?;
        let acl_at = usize::try_from(dword(bytes, 16)?).ok()?;
        if owner_at < 20 || acl_at < 20 || owner_at % 4 != 0 || acl_at % 4 != 0 {
            return None;
        }
        let owner = sid(bytes.get(owner_at..)?)?;
        if owner != user {
            return None;
        }
        let size = usize::from(word(bytes, acl_at.checked_add(2)?)?);
        let acl_end = acl_at.checked_add(size)?;
        let owner_end = owner_at.checked_add(owner.len())?;
        if owner_at < acl_end && acl_at < owner_end {
            return None;
        }
        let acl = bytes.get(acl_at..acl_end)?;
        if acl.len() < 8 || acl[0] != 2 {
            return None;
        }
        let count = word(acl, 4)?;
        if !(1..=2).contains(&count) {
            return None;
        }
        let mut at = 8usize;
        let mut grants = Vec::new();
        for _ in 0..count {
            let size = usize::from(word(acl, at.checked_add(2)?)?);
            if size < 16 || size % 4 != 0 {
                return None;
            }
            let end = at.checked_add(size)?;
            let ace = acl.get(at..end)?;
            if ace[0] != 0 || dword(ace, 4)? != 0x001f_01ff {
                return None;
            }
            let flags = ace[1];
            let flags_ok = match kind {
                Kind::Root => flags == 3,
                Kind::Directory => matches!(flags, 3 | 19),
                Kind::File => matches!(flags, 0 | 16),
            };
            let trustee = sid(ace.get(8..)?)?;
            if !flags_ok
                || trustee.len() + 8 != ace.len()
                || (trustee != user && trustee != SYSTEM)
                || grants
                    .iter()
                    .any(|(seen, _): &(Vec<u8>, u8)| seen == trustee)
            {
                return None;
            }
            grants.push((trustee.to_vec(), flags));
            at = end;
        }
        if !grants.iter().any(|(sid, _)| sid == user) {
            return None;
        }
        grants.sort();
        Some(Descriptor {
            owner: owner.to_vec(),
            protected: control & 0x1000 != 0,
            grants,
        })
    }
    #[cfg(test)]
    pub(crate) mod fault {
        use std::cell::RefCell;
        use std::path::Path;
        pub(crate) enum Fault {
            Query(i32),
            Descriptor(Vec<u8>),
        }
        type Hook = Box<dyn FnMut(&Path) -> Option<Fault>>;
        thread_local! { static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) }; }
        pub(crate) fn with<T>(
            hook: impl FnMut(&Path) -> Option<Fault> + 'static,
            run: impl FnOnce() -> T,
        ) -> T {
            struct Restore(Option<Hook>);
            impl Drop for Restore {
                fn drop(&mut self) {
                    HOOK.with(|slot| *slot.borrow_mut() = self.0.take());
                }
            }
            let _restore = Restore(HOOK.with(|slot| slot.replace(Some(Box::new(hook)))));
            run()
        }
        pub(crate) fn take(path: &Path) -> Option<Fault> {
            HOOK.with(|slot| slot.borrow_mut().as_mut().and_then(|hook| hook(path)))
        }
    }
}

#[cfg(windows)]
pub(super) mod windows_acl {
    use std::ffi::c_void;
    use std::fs::{File, OpenOptions};
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    use std::os::windows::io::{AsHandle as _, AsRawHandle as _, BorrowedHandle};
    use std::path::{Component, Path, PathBuf};
    use std::ptr;

    use super::private_acl::{self, Descriptor, Kind};
    use super::{NativeRootHandle, Result, ShoreError, platform};
    use crate::session::store::authority_lock::{StoreAuthorityLock, file_identity};

    type Handle = *mut c_void;
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenThreadToken(thread: Handle, access: u32, as_self: i32, token: *mut Handle) -> i32;
        fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
        fn GetTokenInformation(
            token: Handle,
            class: u32,
            data: Handle,
            length: u32,
            needed: *mut u32,
        ) -> i32;
        fn GetSecurityInfo(
            object: Handle,
            kind: u32,
            flags: u32,
            owner: *mut Handle,
            group: *mut Handle,
            dacl: *mut Handle,
            sacl: *mut Handle,
            descriptor: *mut Handle,
        ) -> u32;
        fn IsValidSecurityDescriptor(descriptor: Handle) -> i32;
        fn GetSecurityDescriptorControl(
            descriptor: Handle,
            control: *mut u16,
            revision: *mut u32,
        ) -> i32;
        fn GetSecurityDescriptorLength(descriptor: Handle) -> u32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> Handle;
        fn GetCurrentProcess() -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn LocalFree(memory: Handle) -> Handle;
    }
    struct Token(Handle);
    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: this wrapper owns one successful token-open result.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    struct Security(Handle);
    impl Drop for Security {
        fn drop(&mut self) {
            // SAFETY: GetSecurityInfo allocations, including null, use LocalFree.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    fn refused(class: &str, role: &Path, detail: impl std::fmt::Display) -> ShoreError {
        ShoreError::Message(format!(
            "append-progress ACL {class} [{}]: {detail}",
            role.display()
        ))
    }
    fn query(role: &Path, error: std::io::Error) -> ShoreError {
        let class = if error.kind() == std::io::ErrorKind::PermissionDenied {
            "permission/identity change"
        } else {
            "query error"
        };
        refused(class, role, error)
    }
    fn user() -> Result<Vec<u8>> {
        let role = Path::new("process token");
        let mut handle = ptr::null_mut();
        // SAFETY: pseudo-handle and writable output; request only TOKEN_QUERY.
        let opened = unsafe { OpenThreadToken(GetCurrentThread(), 8, 1, &mut handle) } != 0;
        let error = std::io::Error::last_os_error();
        let _thread = opened.then(|| Token(handle));
        match private_acl::absent_thread_token(opened, error.raw_os_error().unwrap_or(0)) {
            Ok(()) => {}
            Err(private_acl::TokenFailure::Impersonation) => {
                return Err(refused("unsupported token", role, "thread impersonation"));
            }
            Err(private_acl::TokenFailure::Query(_)) => return Err(query(role, error)),
        }
        // SAFETY: process pseudo-handle and writable token output.
        if unsafe { OpenProcessToken(GetCurrentProcess(), 8, &mut handle) } == 0 {
            return Err(query(role, std::io::Error::last_os_error()));
        }
        let token = Token(handle);
        let mut needed = 0;
        // SAFETY: null/zero size requests the required TOKEN_USER buffer length.
        let sized = unsafe { GetTokenInformation(token.0, 1, ptr::null_mut(), 0, &mut needed) };
        let error = std::io::Error::last_os_error();
        if sized == 0 && error.raw_os_error() != Some(122) {
            return Err(query(role, error));
        }
        if sized != 0 || !(16..=65536).contains(&needed) {
            return Err(refused(
                "query error",
                role,
                "invalid TokenUser size response",
            ));
        }
        let capacity = needed;
        let mut words = vec![0u64; (capacity as usize).div_ceil(8)];
        // SAFETY: aligned, allocated capacity bytes; token remains live.
        if unsafe {
            GetTokenInformation(token.0, 1, words.as_mut_ptr().cast(), capacity, &mut needed)
        } == 0
        {
            return Err(query(role, std::io::Error::last_os_error()));
        }
        if needed > capacity || needed < std::mem::size_of::<usize>() as u32 {
            return Err(refused(
                "query error",
                role,
                "invalid TokenUser result length",
            ));
        }
        // SAFETY: successful API initialized at most capacity bytes in words.
        let bytes =
            unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), needed as usize) };
        let sid = private_acl::token_user(bytes, words.as_ptr() as usize).ok_or_else(|| {
            refused(
                "unsupported token",
                role,
                "invalid or LocalSystem TokenUser",
            )
        })?;
        Ok(sid.to_vec())
    }
    fn descriptor(file: &File, role: &Path, user: &[u8], kind: Kind) -> Result<Descriptor> {
        borrowed_descriptor(file.as_handle(), role, user, kind)
    }
    /// The role is only an error/fault label; it is never opened by this query.
    fn borrowed_descriptor(
        file: BorrowedHandle<'_>,
        role: &Path,
        user: &[u8],
        kind: Kind,
    ) -> Result<Descriptor> {
        #[cfg(test)]
        if let Some(fault) = private_acl::fault::take(role) {
            return match fault {
                private_acl::fault::Fault::Query(code) => {
                    Err(query(role, std::io::Error::from_raw_os_error(code)))
                }
                private_acl::fault::Fault::Descriptor(bytes) => {
                    private_acl::parse(&bytes, user, kind).ok_or_else(|| {
                        refused(
                            "unsupported descriptor",
                            role,
                            "injected closed owner/DACL predicate",
                        )
                    })
                }
            };
        }
        let mut allocation = ptr::null_mut();
        // SAFETY: live borrowed HANDLE; only OWNER and DACL requested.
        // Missing READ_CONTROL is an OS query error, not borrowed ownership.
        let code = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                1,
                5,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut allocation,
            )
        };
        let allocation = Security(allocation);
        if code != 0 {
            return Err(query(role, std::io::Error::from_raw_os_error(code as i32)));
        }
        let mut control = 0;
        let mut revision = 0;
        // SAFETY: only the OS-returned allocation enters these native validators.
        if allocation.0.is_null() || unsafe { IsValidSecurityDescriptor(allocation.0) } == 0 {
            return Err(refused(
                "unsupported descriptor",
                role,
                "invalid self-relative descriptor",
            ));
        }
        // SAFETY: the OS allocation passed its native structural validator.
        if unsafe { GetSecurityDescriptorControl(allocation.0, &mut control, &mut revision) } == 0 {
            return Err(query(role, std::io::Error::last_os_error()));
        }
        if revision != 1 || control & 0x8000 == 0 {
            return Err(refused(
                "unsupported descriptor",
                role,
                "absolute descriptor",
            ));
        }
        // SAFETY: valid self-relative descriptor; associated structures are in this allocation.
        let length = unsafe { GetSecurityDescriptorLength(allocation.0) } as usize;
        if !(20..=131072).contains(&length) {
            return Err(refused("unsupported descriptor", role, "descriptor size"));
        }
        // SAFETY: validated OS allocation is alive for the entire bounded parse.
        let bytes = unsafe { std::slice::from_raw_parts(allocation.0.cast::<u8>(), length) };
        private_acl::parse(bytes, user, kind).ok_or_else(|| {
            refused(
                "unsupported descriptor",
                role,
                "closed owner/DACL predicate",
            )
        })
    }
    /// Validate the current non-impersonating token and closed owner/DACL subset
    /// on a borrowed object. Returned values do not retain or close its HANDLE.
    /// This does not prove namespace binding or replace a surrounding live gate.
    pub(in crate::session::append_progress) fn borrowed_private_descriptor(
        file: BorrowedHandle<'_>,
        role: &Path,
        kind: Kind,
    ) -> Result<(super::BorrowedMetadata, Descriptor)> {
        let observed = super::borrowed_metadata(file)?;
        if observed.directory != (kind != Kind::File) {
            return Err(refused("permission/identity change", role, "object type"));
        }
        let expected_user = user()?;
        let descriptor = borrowed_descriptor(file, role, &expected_user, kind)?;
        if user()? != expected_user {
            return Err(refused("unsupported token", role, "TokenUser changed"));
        }
        Ok((observed, descriptor))
    }
    fn open(path: &Path, kind: Kind) -> Result<File> {
        let file = OpenOptions::new()
            .access_mode(0x0002_0080)
            .share_mode(7)
            .custom_flags(0x0020_0000 | if kind != Kind::File { 0x0200_0000 } else { 0 })
            .open(path)
            .map_err(|error| query(path, error))?;
        let meta = file.metadata().map_err(|error| query(path, error))?;
        if meta.file_attributes() & 0x400 != 0
            || (kind != Kind::File && !meta.is_dir())
            || (kind == Kind::File && !meta.is_file())
        {
            return Err(refused(
                "permission/identity change",
                path,
                "object type/reparse",
            ));
        }
        Ok(file)
    }
    struct Object {
        path: PathBuf,
        file: File,
        kind: Kind,
        descriptor: Descriptor,
    }
    #[must_use = "finish the ACL gate before admitting the bounded operation"]
    pub(in crate::session::append_progress) struct Gate<'a> {
        root: &'a NativeRootHandle,
        guard: &'a StoreAuthorityLock,
        user: Vec<u8>,
        objects: Vec<Object>,
    }
    fn binding(file: &File, path: &Path, kind: Kind) -> Result<()> {
        if file_identity(file)? != file_identity(&open(path, kind)?)? {
            return Err(refused(
                "permission/identity change",
                path,
                "retained handle differs",
            ));
        }
        Ok(())
    }
    impl Gate<'_> {
        /// Consume the gate after the caller's bounded operation. No retry or
        /// descriptor epoch is persisted; failed gates confer no admission.
        pub(in crate::session::append_progress) fn finish(self) -> Result<()> {
            self.guard.validate_physical_root(&self.root.supplied)?;
            platform::validate_root(self.root)?;
            if user()? != self.user {
                return Err(refused(
                    "unsupported token",
                    &self.root.supplied,
                    "TokenUser changed",
                ));
            }
            for object in &self.objects {
                binding(&object.file, &object.path, object.kind)?;
                if descriptor(&object.file, &object.path, &self.user, object.kind)?
                    != object.descriptor
                {
                    return Err(refused(
                        "permission/identity change",
                        &object.path,
                        "descriptor changed",
                    ));
                }
                binding(&object.file, &object.path, object.kind)?;
            }
            self.guard.validate_physical_root(&self.root.supplied)?;
            platform::validate_root(self.root)?;
            if user()? != self.user {
                return Err(refused(
                    "unsupported token",
                    &self.root.supplied,
                    "TokenUser changed",
                ));
            }
            Ok(())
        }
    }
    /// All supplied objects must already exist. The caller retains responsibility
    /// for the stage-specific complete set and absent WAL/SHM endpoint checks.
    /// Root, stable lock and every containing component are always inspected.
    pub(in crate::session::append_progress) fn begin<'a>(
        root: &'a NativeRootHandle,
        guard: &'a StoreAuthorityLock,
        objects: &[(&Path, &File, Kind)],
    ) -> Result<Gate<'a>> {
        guard.validate_physical_root(&root.supplied)?;
        platform::validate_root(root)?;
        let user = user()?;
        let mut paths = std::collections::BTreeMap::new();
        let mut expected = std::collections::BTreeMap::new();
        expected.insert(root.canonical.clone(), file_identity(&root.directory)?);
        paths.insert(root.canonical.clone(), Kind::Root);
        paths.insert(root.canonical.join("authority.writer.lock"), Kind::File);
        for (path, retained, kind) in objects {
            if *kind == Kind::Root || path.components().any(|c| matches!(c, Component::ParentDir)) {
                return Err(refused(
                    "permission/identity change",
                    path,
                    "invalid descendant",
                ));
            }
            let absolute = std::path::absolute(path).map_err(|e| query(path, e))?;
            let relative = absolute
                .strip_prefix(&root.supplied)
                .or_else(|_| absolute.strip_prefix(&root.canonical))
                .map_err(|_| refused("permission/identity change", path, "outside root"))?;
            if relative.as_os_str().is_empty()
                || relative
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
            {
                return Err(refused(
                    "permission/identity change",
                    path,
                    "invalid relative path",
                ));
            }
            let canonical = root.canonical.join(relative);
            binding(retained, &canonical, *kind)?;
            let identity = file_identity(retained)?;
            if expected
                .insert(canonical.clone(), identity)
                .is_some_and(|old| old != identity)
            {
                return Err(refused(
                    "permission/identity change",
                    path,
                    "conflicting retained handles",
                ));
            }
            let mut parent = canonical.parent();
            while let Some(path) =
                parent.filter(|p| p.starts_with(&root.canonical) && *p != root.canonical)
            {
                if paths
                    .insert(path.to_path_buf(), Kind::Directory)
                    .is_some_and(|old| old != Kind::Directory)
                {
                    return Err(refused(
                        "permission/identity change",
                        path,
                        "conflicting type",
                    ));
                }
                parent = path.parent();
            }
            if paths
                .insert(canonical, *kind)
                .is_some_and(|old| old != *kind)
            {
                return Err(refused(
                    "permission/identity change",
                    path,
                    "conflicting type",
                ));
            }
        }
        let mut held = Vec::new();
        for (path, kind) in paths {
            let file = open(&path, kind)?;
            if let Some(identity) = expected.get(&path) {
                if file_identity(&file)? != *identity {
                    return Err(refused(
                        "permission/identity change",
                        &path,
                        "query handle differs",
                    ));
                }
            }
            binding(&file, &path, kind)?;
            let descriptor = descriptor(&file, &path, &user, kind)?;
            binding(&file, &path, kind)?;
            held.push(Object {
                path,
                file,
                kind,
                descriptor,
            });
        }
        let gate = Gate {
            root,
            guard,
            user,
            objects: held,
        };
        // Recheck guard/token and all retained query identities before returning.
        gate.guard.validate_physical_root(&root.supplied)?;
        platform::validate_root(root)?;
        if self::user()? != gate.user {
            return Err(refused(
                "unsupported token",
                &root.supplied,
                "TokenUser changed",
            ));
        }
        for (path, file, kind) in objects {
            binding(file, path, *kind)?;
        }
        Ok(gate)
    }
}

#[cfg(any(target_os = "macos", windows))]
pub(super) fn sync_carrier(root: &NativeRootHandle, path: &Path, expected: &[u8]) -> Result<()> {
    platform::sync_files(root, path, Some(platform::Expected::Bytes(expected)), false)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(super) fn sync_carrier(root: &NativeRootHandle, path: &Path, expected: &[u8]) -> Result<()> {
    let _ = (root, path, expected);
    Err(unsupported("append-progress durability is unavailable"))
}

#[cfg(any(target_os = "macos", windows))]
mod platform {
    use std::fs::{File, OpenOptions};
    use std::io::{Read as _, Seek as _};
    #[cfg(target_os = "macos")]
    use std::os::fd::AsRawFd as _;
    #[cfg(target_os = "macos")]
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
    use std::path::{Component, Path};

    use super::{NativeRootHandle, Result, ShoreError, native_error};

    pub(super) fn refused() -> ShoreError {
        ShoreError::Message("append-progress physical root or file binding changed".to_owned())
    }

    pub(super) fn open(path: &Path, directory: bool) -> Result<File> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(target_os = "macos")]
        options
            .write(!directory)
            .custom_flags(libc::O_NOFOLLOW | if directory { libc::O_DIRECTORY } else { 0 });
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt as _;
            options
                .write(true)
                .custom_flags(0x0020_0000 | if directory { 0x0200_0000 } else { 0 });
        }
        let file = options.open(path).map_err(native_error)?;
        let metadata = file.metadata().map_err(native_error)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt as _;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(refused());
            }
        }
        if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
            return Err(refused());
        }
        Ok(file)
    }

    fn same_file(left: &File, right: &File) -> Result<bool> {
        #[cfg(target_os = "macos")]
        {
            let left = left.metadata().map_err(native_error)?;
            let right = right.metadata().map_err(native_error)?;
            Ok((left.dev(), left.ino()) == (right.dev(), right.ino()))
        }
        #[cfg(windows)]
        {
            use crate::session::store::authority_lock::file_identity;
            Ok(file_identity(left)? == file_identity(right)?)
        }
    }

    pub(super) fn validate_root(root: &NativeRootHandle) -> Result<()> {
        root.validate_live()
    }

    #[derive(Clone, Copy)]
    pub(super) enum Expected<'a> {
        Bytes(&'a [u8]),
        Witness { witness: &'a str, length: u64 },
    }

    fn check_bytes(file: &mut File, expected: Option<Expected<'_>>) -> Result<()> {
        let Some(expected) = expected else {
            return Ok(());
        };
        if super::super::audit::active() {
            let length = match expected {
                Expected::Bytes(bytes) => bytes.len() as u64,
                Expected::Witness { length, .. } => length,
            };
            let bytes = super::super::audit::read(file, length)?;
            let matches = match expected {
                Expected::Bytes(expected) => bytes == expected,
                Expected::Witness { witness, .. } => {
                    crate::canonical_hash::sha256_bytes_hex(&bytes) == witness
                }
            };
            return if matches { Ok(()) } else { Err(refused()) };
        }
        file.seek(std::io::SeekFrom::Start(0))
            .map_err(native_error)?;
        match expected {
            Expected::Bytes(expected) => {
                let mut bytes = Vec::new();
                file.take(expected.len() as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(native_error)?;
                if bytes != expected {
                    return Err(refused());
                }
            }
            Expected::Witness { witness, length } => {
                use sha2::{Digest as _, Sha256};
                if file.metadata().map_err(native_error)?.len() != length {
                    return Err(refused());
                }
                let mut remaining = length;
                let mut hasher = Sha256::new();
                let mut buffer = [0u8; 8192];
                while remaining > 0 {
                    let limit = remaining.min(buffer.len() as u64) as usize;
                    let count = file.read(&mut buffer[..limit]).map_err(native_error)?;
                    if count == 0 {
                        return Err(refused());
                    }
                    hasher.update(&buffer[..count]);
                    remaining -= count as u64;
                }
                // Detect growth without length+1 overflow or unbounded allocation.
                if file.read(&mut buffer[..1]).map_err(native_error)? != 0
                    || format!("{:x}", hasher.finalize()) != witness
                {
                    return Err(refused());
                }
            }
        }
        Ok(())
    }

    fn sync_file(path: &Path, expected: Option<Expected<'_>>) -> Result<File> {
        let mut file = open(path, false)?;
        check_bytes(&mut file, expected)?;
        #[cfg(test)]
        super::run_hook(path, super::Barrier::File)?;
        file.sync_all().map_err(native_error)?;
        #[cfg(target_os = "macos")]
        // SAFETY: F_FULLFSYNC has no variadic argument and file remains open.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } == -1 {
            return Err(native_error(std::io::Error::last_os_error()));
        }
        if !same_file(&file, &open(path, false)?)? {
            return Err(refused());
        }
        #[cfg(test)]
        super::run_hook(path, super::Barrier::AfterFile)?;
        Ok(file)
    }

    pub(super) fn sync_files(
        root: &NativeRootHandle,
        database: &Path,
        expected: Option<Expected<'_>>,
        with_wal: bool,
    ) -> Result<()> {
        validate_root(root)?;
        // Windows absolute-path conversion normalizes ParentDir; refuse it first.
        if database
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(refused());
        }
        let database = std::path::absolute(database).map_err(native_error)?;
        let relative = database
            .strip_prefix(&root.supplied)
            .or_else(|_| database.strip_prefix(&root.canonical))
            .map_err(|_| refused())?;
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(refused());
        }
        let name = relative.file_name().ok_or_else(refused)?;
        let mut parent = root.canonical.clone();
        let mut directories = vec![(parent.clone(), open(&parent, true)?)];
        for part in relative.parent().ok_or_else(refused)?.components() {
            parent.push(part);
            directories.push((parent.clone(), open(&parent, true)?));
        }
        let database = parent.join(name);
        let file = sync_file(&database, expected)?;
        let mut wal_name = database.as_os_str().to_os_string();
        wal_name.push("-wal");
        let wal = std::path::PathBuf::from(wal_name);
        let wal_file = if with_wal {
            match std::fs::symlink_metadata(&wal) {
                Ok(_) => Some(sync_file(&wal, None)?),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(native_error(error)),
            }
        } else {
            None
        };
        // Sync every newly created parent chain back through the admitted root.
        // Never follow a descendant directory symlink or accept a replaced parent.
        for (path, directory) in directories.iter().rev() {
            #[cfg(test)]
            super::run_hook(path, super::Barrier::Parent)?;
            if !same_file(directory, &open(path, true)?)? {
                return Err(refused());
            }
            directory.sync_all().map_err(native_error)?;
            if !same_file(directory, &open(path, true)?)? {
                return Err(refused());
            }
        }
        let mut reopened = open(&database, false)?;
        if !same_file(&file, &reopened)? {
            return Err(refused());
        }
        check_bytes(&mut reopened, expected)?;
        if let Some(file) = wal_file
            && !same_file(&file, &open(&wal, false)?)?
        {
            return Err(refused());
        }
        validate_root(root)
    }
}

// This candidate is deliberately separate from held Windows admission.
#[cfg(windows)]
pub(super) fn ntfs_candidate_root(root: &Path) -> Result<NativeRootHandle> {
    super::owner::require_quiescent()?;
    use std::os::windows::io::AsRawHandle as _;
    use std::path::{Component, Prefix};
    let supplied = std::path::absolute(root).map_err(native_error)?;
    let canonical = root.canonicalize().map_err(native_error)?;
    if !matches!(canonical.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
    {
        return Err(unsupported("native local disk root required"));
    }
    let directory = platform::open(&supplied, true)?;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetVolumeInformationByHandleW(
            handle: *mut std::ffi::c_void,
            name: *mut u16,
            name_len: u32,
            serial: *mut u32,
            component_len: *mut u32,
            flags: *mut u32,
            filesystem: *mut u16,
            filesystem_len: u32,
        ) -> i32;
    }
    let mut filesystem = [0u16; 32];
    // SAFETY: live directory handle, valid output buffer, unused outputs null.
    if unsafe {
        GetVolumeInformationByHandleW(
            directory.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        )
    } == 0
    {
        return Err(native_error(std::io::Error::last_os_error()));
    }
    let len = filesystem
        .iter()
        .position(|v| *v == 0)
        .unwrap_or(filesystem.len());
    if String::from_utf16_lossy(&filesystem[..len]) != "NTFS" {
        return Err(unsupported("native NTFS root required"));
    }
    directory.sync_all().map_err(native_error)?;
    let ancestors = retain_ancestors(&supplied, &canonical)?;
    let handle = NativeRootHandle {
        ancestors,
        directory,
        canonical,
        supplied,
    };
    platform::validate_root(&handle)?;
    Ok(handle)
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Barrier {
    File,
    AfterFile,
    Parent,
}
#[cfg(test)]
type Hook = Box<dyn FnMut(&Path, Barrier) -> Result<()>>;
#[cfg(test)]
thread_local! { static HOOK: std::cell::RefCell<Option<Hook>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(super) fn with_hook<T>(
    hook: impl FnMut(&Path, Barrier) -> Result<()> + 'static,
    action: impl FnOnce() -> T,
) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            HOOK.with(|h| *h.borrow_mut() = None);
        }
    }
    HOOK.with(|h| *h.borrow_mut() = Some(Box::new(hook)));
    let _reset = Reset;
    action()
}
#[cfg(test)]
pub(super) fn run_hook(path: &Path, stage: Barrier) -> Result<()> {
    HOOK.with(|h| match h.borrow_mut().as_mut() {
        Some(hook) => hook(path, stage),
        None => Ok(()),
    })
}

#[cfg(any(target_os = "macos", windows))]
fn retain_ancestors(
    supplied: &Path,
    canonical: &Path,
) -> Result<Vec<(std::path::PathBuf, std::fs::File)>> {
    // Private foundation accepts canonical absolute roots only. Aliased caller
    // paths require the later repository resolver and are not silently trusted.
    #[cfg(windows)]
    let same = supplied
        .to_str()
        .map(|p| p.strip_prefix(r"\\?\").unwrap_or(p))
        == canonical
            .to_str()
            .map(|p| p.strip_prefix(r"\\?\").unwrap_or(p));
    #[cfg(not(windows))]
    let same = supplied == canonical;
    if !same {
        return Err(unsupported(
            "owned root must be an exact canonical absolute path",
        ));
    }
    let mut paths: Vec<_> = canonical.ancestors().skip(1).collect();
    paths.reverse();
    paths
        .into_iter()
        .map(|path| {
            Ok((
                path.to_owned(),
                crate::session::store::authority_lock::open_identity_path(path, true, false)?,
            ))
        })
        .collect()
}
#[cfg(any(target_os = "macos", windows))]
impl NativeRootHandle {
    pub(super) fn validate_live(&self) -> Result<()> {
        use crate::session::store::authority_lock::file_identity;
        super::owner::require_owned()?;
        for pair in self.ancestors.windows(2) {
            self.edge(&pair[0].1, &pair[1].0, &pair[1].1)?;
        }
        let (_, parent) = self.ancestors.last().ok_or_else(platform::refused)?;
        self.edge(parent, &self.canonical, &self.directory)?;
        // Query the retained root; neither this nor edge opens a Q descriptor.
        let _ = file_identity(&self.directory)?;
        Ok(())
    }
    fn edge(&self, parent: &std::fs::File, path: &Path, child: &std::fs::File) -> Result<()> {
        use crate::session::store::authority_lock::file_identity;
        let name = path.file_name().ok_or_else(platform::refused)?;
        // SAFETY: the owner permit is live and all ancestor handles remain held.
        let found =
            unsafe { namespace::observe_private_worker(parent, name, namespace::Role::Ancestor) }?;
        if !found.is_some_and(|v| v.directory && Some(v.identity) == file_identity(child).ok()) {
            return Err(platform::refused());
        }
        Ok(())
    }
    pub(super) fn conflicts_with_ancestor(
        &self,
        id: crate::session::store::authority_lock::PhysicalFileIdentity,
    ) -> Result<bool> {
        for (_, file) in &self.ancestors {
            if crate::session::store::authority_lock::file_identity(file)? == id {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Exact existing macOS JournalChangeStamp encoding, queried through retained N.
#[cfg(target_os = "macos")]
pub(super) fn retained_journal_stamp(
    file: &std::fs::File,
    path: &Path,
) -> Result<crate::session::store::backend::JournalChangeStamp> {
    use std::os::fd::AsRawFd as _;
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::MetadataExt as _;
    let metadata = file.metadata().map_err(native_error)?;
    let mut attributes = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: 0,
        dirattr: libc::ATTR_DIR_ENTRYCOUNT,
        fileattr: 0,
        forkattr: 0,
    };
    let mut output = [0u32; 2];
    // SAFETY: retained directory fd, exact attrlist and sized writable output.
    if unsafe {
        libc::fgetattrlist(
            file.as_raw_fd(),
            (&raw mut attributes).cast(),
            output.as_mut_ptr().cast(),
            std::mem::size_of_val(&output),
            0,
        )
    } != 0
    {
        return Err(native_error(std::io::Error::last_os_error()));
    }
    if output[0] as usize != std::mem::size_of_val(&output) {
        return Err(platform::refused());
    }
    let count = u64::from(output[1]);
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
    change.extend_from_slice(&count.to_le_bytes());
    Ok(
        crate::session::store::backend::JournalChangeStamp::observed_with_entry_count(
            &identity, &change, count,
        ),
    )
}
