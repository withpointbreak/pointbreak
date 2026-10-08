//! Quiescent, descriptor-relative provisioning and publication for stable layout.
//! No handle here survives into a native SQL scope.
use std::fs::File;
use std::io::Write as _;
use std::os::fd::{AsRawFd as _, FromRawFd as _};
use std::os::unix::fs::MetadataExt as _;

use super::*;
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
use crate::session::derived_access::generation::{
    GenerationDescriptor, GenerationLayout, GenerationPublication,
};
use crate::session::store::authority_lock::file_identity;

fn fail(error: impl std::fmt::Display) -> ShoreError {
    ShoreError::Message(format!("owned rebuild namespace: {error}"))
}
fn leaf(name: &str) -> Result<std::ffi::CString> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(fail("invalid leaf"));
    }
    std::ffi::CString::new(name).map_err(fail)
}
fn open_at(parent: &File, name: &str, directory: bool, create: bool) -> Result<File> {
    super::super::audit::visit()?;
    let name = leaf(name)?;
    if directory && create {
        // SAFETY: live retained parent and a checked single-component name.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0
            && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
        {
            return Err(fail(std::io::Error::last_os_error()));
        }
    }
    let flags = libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | if directory {
            libc::O_RDONLY | libc::O_DIRECTORY
        } else if create {
            libc::O_RDWR | libc::O_CREAT
        } else {
            libc::O_RDONLY
        };
    // SAFETY: parent remains live and name is a closed leaf.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags, 0o600) };
    if fd < 0 {
        return Err(fail(std::io::Error::last_os_error()));
    }
    // SAFETY: exactly one newly owned descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata().map_err(fail)?;
    if meta.is_dir() != directory
        || (!directory && (!meta.is_file() || meta.nlink() != 1))
        || meta.mode() & 0o077 != 0
        || meta.uid() != unsafe { libc::geteuid() }
    {
        return Err(fail("nonprivate or invalid provisioned object"));
    }
    Ok(file)
}
fn validate_at(parent: &File, name: &str, file: &File, directory: bool) -> Result<()> {
    let named = open_at(parent, name, directory, false)?;
    if file_identity(&named)? != file_identity(file)? {
        return Err(fail("namespace changed"));
    }
    Ok(())
}
fn write_at(parent: &File, name: &str, bytes: &[u8]) -> Result<()> {
    super::super::audit::check()?;
    let raw_name = leaf(name)?;
    // SAFETY: retained parent and exclusive no-follow leaf. Existing evidence is
    // never overwritten, truncated or automatically removed on error.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            raw_name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(fail(std::io::Error::last_os_error()));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes).map_err(fail)?;
    file.sync_all().map_err(fail)?;
    validate_at(parent, name, &file, false)
}
fn rename_at(from: &File, old: &str, to: &File, new: &str) -> Result<()> {
    super::super::audit::check()?;
    let old = leaf(old)?;
    let new = leaf(new)?;
    // SAFETY: both retained parents remain live. RENAME_EXCL also refuses any
    // preexisting target; failed generations/publications are never replaced.
    if unsafe {
        libc::renameatx_np(
            from.as_raw_fd(),
            old.as_ptr(),
            to.as_raw_fd(),
            new.as_ptr(),
            libc::RENAME_EXCL,
        )
    } != 0
    {
        return Err(fail(std::io::Error::last_os_error()));
    }
    from.sync_all().map_err(fail)?;
    to.sync_all().map_err(fail)
}
struct Directories<'a> {
    root: &'a File,
    derived: File,
    staging: File,
    generations: File,
    publications: File,
    publication_staging: File,
}
impl<'a> Directories<'a> {
    fn open(root: &'a File, create: bool) -> Result<Self> {
        let derived = open_at(root, "derived", true, create)?;
        let value = Self {
            staging: open_at(&derived, "staging", true, create)?,
            generations: open_at(&derived, "generations", true, create)?,
            publications: open_at(&derived, "publications", true, create)?,
            publication_staging: open_at(&derived, "publication-staging", true, create)?,
            derived,
            root,
        };
        value.validate()?;
        if create {
            value.sync()?;
        }
        Ok(value)
    }
    fn validate(&self) -> Result<()> {
        validate_at(self.root, "derived", &self.derived, true)?;
        for (name, file) in [
            ("staging", &self.staging),
            ("generations", &self.generations),
            ("publications", &self.publications),
            ("publication-staging", &self.publication_staging),
        ] {
            validate_at(&self.derived, name, file, true)?;
        }
        Ok(())
    }
    fn sync(&self) -> Result<()> {
        self.validate()?;
        for file in [
            &self.staging,
            &self.generations,
            &self.publications,
            &self.publication_staging,
            &self.derived,
            self.root,
        ] {
            file.sync_all().map_err(fail)?;
        }
        self.validate()
    }
}
impl ReadContext {
    pub(in crate::session) fn require_stable_layout(
        &self,
        layout: &GenerationLayout,
    ) -> Result<()> {
        owner::require_quiescent()?;
        native::root_snapshot(&self.root)?;
        let legacy_present =
            match std::fs::symlink_metadata(self.store_dir().join(".pointbreak-derived")) {
                Ok(_) => true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => return Err(fail(error)),
            };
        if layout.root() != self.store_dir().join("derived") || legacy_present {
            return Err(fail(
                "legacy derived layout unsupported by owned data entry",
            ));
        }
        Ok(())
    }
    pub(in crate::session) fn provision_rebuild_layout(
        &self,
        layout: &GenerationLayout,
    ) -> Result<()> {
        self.require_stable_layout(layout)?;
        let root = native::retained_root_file(&self.root)?;
        let dirs = Directories::open(root, true)?;
        let lock = open_at(root, "derived.rebuild.lock", false, true)?;
        validate_at(root, "derived.rebuild.lock", &lock, false)?;
        lock.sync_all().map_err(fail)?;
        dirs.sync()?;
        native::root_snapshot(&self.root)?;
        Ok(())
    }
    pub(in crate::session) fn provision_generation_lease(
        &self,
        layout: &GenerationLayout,
        id: &str,
    ) -> Result<()> {
        self.require_stable_layout(layout)?;
        let path = layout.generation_lease_path(id);
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| fail("lease leaf"))?;
        let root = native::retained_root_file(&self.root)?;
        let file = open_at(root, name, false, true)?;
        validate_at(root, name, &file, false)?;
        file.sync_all().map_err(fail)?;
        root.sync_all().map_err(fail)?;
        native::root_snapshot(&self.root)?;
        Ok(())
    }
    pub(in crate::session) fn write_rebuild_metadata(
        &self,
        layout: &GenerationLayout,
        descriptor: &GenerationDescriptor,
        receipt: &[u8],
        retained_generation: &File,
    ) -> Result<String> {
        self.require_stable_layout(layout)?;
        let dirs = Directories::open(native::retained_root_file(&self.root)?, false)?;
        let generation = open_at(&dirs.staging, &descriptor.generation_id, true, false)?;
        if file_identity(&generation)? != file_identity(retained_generation)? {
            return Err(fail("staging generation replaced before metadata"));
        }
        write_at(&generation, "change-reader-profile.json", receipt)?;
        let bytes = canonical_json_bytes(&serde_json::to_value(descriptor)?)?;
        write_at(&generation, "generation.json", &bytes)?;
        generation.sync_all().map_err(fail)?;
        validate_at(&dirs.staging, &descriptor.generation_id, &generation, true)?;
        dirs.sync()?;
        native::root_snapshot(&self.root)?;
        Ok(sha256_bytes_hex(&bytes))
    }
    pub(in crate::session) fn promote_rebuild(
        &self,
        layout: &GenerationLayout,
        id: &str,
        retained_generation: &File,
    ) -> Result<()> {
        self.require_stable_layout(layout)?;
        let dirs = Directories::open(native::retained_root_file(&self.root)?, false)?;
        let generation = open_at(&dirs.staging, id, true, false)?;
        if file_identity(&generation)? != file_identity(retained_generation)? {
            return Err(fail("staging generation replaced before promotion"));
        }
        generation.sync_all().map_err(fail)?;
        dirs.validate()?;
        rename_at(&dirs.staging, id, &dirs.generations, id)?;
        validate_at(&dirs.generations, id, &generation, true)?;
        dirs.sync()?;
        native::root_snapshot(&self.root)?;
        Ok(())
    }
    pub(in crate::session) fn publish_rebuild(
        &self,
        layout: &GenerationLayout,
        publication: &GenerationPublication,
    ) -> Result<()> {
        self.require_stable_layout(layout)?;
        let dirs = Directories::open(native::retained_root_file(&self.root)?, false)?;
        let name = format!(
            "{:020}-{}.json",
            publication.sequence, publication.generation_id
        );
        let bytes = canonical_json_bytes(&serde_json::to_value(publication)?)?;
        write_at(&dirs.publication_staging, &name, &bytes)?;
        dirs.validate()?;
        rename_at(&dirs.publication_staging, &name, &dirs.publications, &name)?;
        dirs.sync()?;
        native::root_snapshot(&self.root)?;
        Ok(())
    }
}
