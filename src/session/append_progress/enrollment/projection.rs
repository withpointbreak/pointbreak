//! Closed projection targets. All N and exclusive preparation precede SQL.
use super::*;
use crate::session::derived_access::generation::{
    GenerationDescriptor, GenerationLayout, GenerationPublication, GenerationReadLease,
    RebuildLease,
};
use crate::session::store::authority_lock::file_identity;

fn generation_id(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(refuse("invalid projection generation identifier"));
    }
    Ok(())
}

impl<'a> RetainedEvidence<'a> {
    /// Confirm the caller's completed rename against the same retained staging
    /// directory. Only its expected path changes; its physical witness does not.
    pub(in crate::session::append_progress) fn after_projection_promotion(
        &mut self,
        layout: &GenerationLayout,
        id: &str,
    ) -> Result<()> {
        super::super::owner::require_quiescent()?;
        let member = self
            .members
            .iter_mut()
            .find(|m| m.role == EvidenceRole::ProjectionGeneration)
            .ok_or_else(|| refuse("missing retained staging generation"))?;
        if member.path != layout.staging(id) {
            return Err(refuse("unexpected staging promotion"));
        }
        member.path = layout.generation(id);
        self.validate_live()
    }
    pub(in crate::session::append_progress) fn capture_rebuild_read(
        context: &'a super::super::read::ReadContext,
        layout: &GenerationLayout,
        authority: &'a StoreAuthorityLock,
        rebuild: &'a RebuildLease,
    ) -> Result<Self> {
        Self::projection_layout(context, layout)?;
        authority.validate_physical_root(context.store_dir())?;
        let mut evidence = Self::capture_read(context)?;
        evidence._authority = Some(authority);
        evidence.members.push(RetainedMember::present(
            EvidenceRole::Authority,
            context.store_dir().join("authority.writer.lock"),
            EvidenceHandle::Borrowed(authority.retained_file()),
        )?);
        evidence.members.push(RetainedMember::present(
            EvidenceRole::RebuildLease,
            layout.rebuild_lock_path(),
            EvidenceHandle::Borrowed(rebuild.retained_file()),
        )?);
        evidence.validate_live()?;
        Ok(evidence)
    }
    fn projection_layout(
        context: &super::super::read::ReadContext,
        layout: &GenerationLayout,
    ) -> Result<()> {
        if layout.root().parent() != Some(context.store_dir())
            || !matches!(
                layout.root().file_name().and_then(|n| n.to_str()),
                Some("derived" | ".pointbreak-derived")
            )
        {
            return Err(refuse(
                "projection layout is outside the retained authority",
            ));
        }
        Ok(())
    }

    fn retain_projection_directories(
        &mut self,
        layout: &GenerationLayout,
        generation: &Path,
        published: bool,
    ) -> Result<()> {
        for (role, path) in [
            (EvidenceRole::DerivedRoot, layout.root().to_path_buf()),
            (
                if published {
                    EvidenceRole::Generations
                } else {
                    EvidenceRole::Staging
                },
                generation
                    .parent()
                    .ok_or_else(|| refuse("missing generation parent"))?
                    .to_path_buf(),
            ),
            (EvidenceRole::ProjectionGeneration, generation.to_path_buf()),
        ] {
            self.members
                .push(RetainedMember::capture(role, path, Some(true))?);
        }
        Ok(())
    }

    /// The actual shared lease is borrowed through confirmed SQL finish.
    pub(in crate::session::append_progress) fn capture_projection_read(
        context: &'a super::super::read::ReadContext,
        layout: &GenerationLayout,
        publication: &GenerationPublication,
        descriptor: &GenerationDescriptor,
        reader_receipt: &crate::session::derived_access::semantic::change::ChangeReaderProfileReceiptV3,
        lease: &'a GenerationReadLease,
    ) -> Result<Self> {
        super::super::owner::require_quiescent()?;
        Self::projection_layout(context, layout)?;
        generation_id(&publication.generation_id)?;
        if publication.generation_id != descriptor.generation_id
            || layout.current_publication().map_err(refuse)?.as_ref() != Some(publication)
            || layout.descriptor(publication).map_err(refuse)? != *descriptor
        {
            return Err(refuse("projection publication/descriptor changed"));
        }
        let generation = layout.generation(&publication.generation_id);
        let mut evidence = Self::capture_read(context)?;
        evidence.retain_projection_directories(layout, &generation, true)?;
        evidence.members.push(RetainedMember::present(
            EvidenceRole::GenerationLease,
            layout.generation_lease_path(&publication.generation_id),
            EvidenceHandle::Borrowed(lease.retained_file()),
        )?);
        let publications = layout.root().join("publications");
        evidence.members.push(RetainedMember::capture(
            EvidenceRole::Publications,
            publications.clone(),
            Some(true),
        )?);
        for (role, path, expected) in [
            (
                EvidenceRole::GenerationReaderReceipt,
                generation.join("change-reader-profile.json"),
                canonical_json_bytes(&serde_json::to_value(reader_receipt)?)?,
            ),
            (
                EvidenceRole::GenerationDescriptor,
                generation.join("generation.json"),
                canonical_json_bytes(&serde_json::to_value(descriptor)?)?,
            ),
            (
                EvidenceRole::GenerationPublication,
                publications.join(format!(
                    "{:020}-{}.json",
                    publication.sequence, publication.generation_id
                )),
                canonical_json_bytes(&serde_json::to_value(publication)?)?,
            ),
        ] {
            let mut member = RetainedMember::capture(role, path, Some(true))?;
            member.expected = Some(expected);
            member.read_actual()?;
            evidence.members.push(member);
        }
        evidence.target = SqlTarget::PublishedProjection(generation.join("cursor.sqlite3"));
        evidence.validate_live()?;
        if layout.current_publication().map_err(refuse)?.as_ref() != Some(publication) {
            return Err(refuse("projection publication changed during capture"));
        }
        evidence.validate_live()?;
        Ok(evidence)
    }

    /// One fresh unique staging target; this constructor cannot reopen or resume.
    pub(in crate::session::append_progress) fn prepare_projection_staging(
        context: &'a super::super::read::ReadContext,
        layout: &GenerationLayout,
        id: &str,
        authority: &'a StoreAuthorityLock,
        rebuild: &'a RebuildLease,
    ) -> Result<Self> {
        use std::os::fd::{AsRawFd as _, FromRawFd as _};

        use super::super::native::namespace::{Role, observe_private_worker};
        super::super::owner::require_quiescent()?;
        Self::projection_layout(context, layout)?;
        generation_id(id)?;
        authority.validate_physical_root(context.store_dir())?;
        // Capture the parent chain before any creation. Only descriptor-relative
        // operations below may create the fresh generation/main, never a prefix.
        let derived = RetainedMember::capture(
            EvidenceRole::DerivedRoot,
            layout.root().to_path_buf(),
            Some(true),
        )?;
        let staging = RetainedMember::capture(
            EvidenceRole::Staging,
            layout.root().join("staging"),
            Some(true),
        )?;
        let parent = staging
            .file()
            .ok_or_else(|| refuse("missing staging parent"))?;
        let root_file = native::retained_root_file(&context.root)?;
        let rebuild_named = unsafe {
            observe_private_worker(
                root_file,
                layout
                    .rebuild_lock_path()
                    .file_name()
                    .ok_or_else(|| refuse("rebuild lock leaf"))?,
                Role::RebuildLease,
            )
        }?;
        if rebuild_named.map(|v| v.identity) != Some(file_identity(rebuild.retained_file())?) {
            return Err(refuse("wrong rebuild lease before staging creation"));
        }
        // SAFETY: all arguments are retained N; this is a quiescent owned scope.
        let derived_named = unsafe {
            observe_private_worker(
                root_file,
                layout
                    .root()
                    .file_name()
                    .ok_or_else(|| refuse("derived leaf"))?,
                Role::DerivedRoot,
            )
        }?;
        let staging_named = unsafe {
            observe_private_worker(
                derived.file().ok_or_else(|| refuse("derived parent"))?,
                "staging".as_ref(),
                Role::Staging,
            )
        }?;
        if derived_named.map(|v| v.identity) != derived.identity()
            || staging_named.map(|v| v.identity) != staging.identity()
        {
            return Err(refuse("staging parent changed before creation"));
        }
        let name = std::ffi::CString::new(id).map_err(refuse)?;
        // SAFETY: retained parent and closed single-component name. EEXIST is
        // refusal: neither incomplete nor published generations can be resumed.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            return Err(io(std::io::Error::last_os_error()));
        }
        let directory_fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if directory_fd < 0 {
            return Err(io(std::io::Error::last_os_error()));
        }
        // SAFETY: openat just returned one newly owned descriptor, before SQL.
        let directory = unsafe { fs::File::from_raw_fd(directory_fd) };
        let directory_identity = file_identity(&directory)?;
        let named = unsafe { observe_private_worker(parent, id.as_ref(), Role::Generation) }?;
        if named.map(|v| v.identity) != Some(directory_identity) {
            return Err(refuse("fresh generation namespace changed"));
        }
        let main_fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                c"cursor.sqlite3".as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if main_fd < 0 {
            return Err(io(std::io::Error::last_os_error()));
        }
        // SAFETY: this exclusively created descriptor is owned only here; it is
        // explicitly closed before native SQLite can acquire Q.
        let main = unsafe { fs::File::from_raw_fd(main_fd) };
        let main_identity = file_identity(&main)?;
        let named = unsafe {
            observe_private_worker(&directory, "cursor.sqlite3".as_ref(), Role::ProjectionMain)
        }?;
        if named.map(|v| v.identity) != Some(main_identity) {
            return Err(refuse("fresh projection main changed"));
        }
        main.sync_all().map_err(io)?;
        directory.sync_all().map_err(io)?;
        parent.sync_all().map_err(io)?;
        drop(main);
        drop(directory);
        authority.validate_physical_root(context.store_dir())?;
        let mut evidence = Self::capture_read(context)?;
        evidence._authority = Some(authority);
        evidence.retain_projection_directories(layout, &layout.staging(id), false)?;
        evidence.members.push(RetainedMember::capture(
            EvidenceRole::Generations,
            layout.root().join("generations"),
            Some(true),
        )?);
        evidence.members.push(RetainedMember::present(
            EvidenceRole::Authority,
            context.store_dir().join("authority.writer.lock"),
            EvidenceHandle::Borrowed(authority.retained_file()),
        )?);
        evidence.members.push(RetainedMember::present(
            EvidenceRole::RebuildLease,
            layout.rebuild_lock_path(),
            EvidenceHandle::Borrowed(rebuild.retained_file()),
        )?);
        evidence.target = SqlTarget::StagingProjection {
            path: layout.staging(id).join("cursor.sqlite3"),
            main: main_identity,
        };
        evidence.validate_live()?;
        Ok(evidence)
    }
}
