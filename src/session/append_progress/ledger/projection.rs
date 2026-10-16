//! Lexical native scopes for the two admitted projection targets.
use super::*;
use crate::session::append_progress::{ProgressReadFence, ReadContext};
use crate::session::derived_access::generation::{
    GenerationDescriptor, GenerationLayout, GenerationPublication, GenerationReadLease,
    RebuildLease,
};

/// All fixed cursor/locator/semantic/binding/request queries share this snapshot.
/// The outer lifecycle retains the actual generation lease through the response.
pub(in crate::session) fn with_published_projection<T>(
    context: &ReadContext,
    layout: &GenerationLayout,
    publication: &GenerationPublication,
    descriptor: &GenerationDescriptor,
    reader_receipt: &crate::session::derived_access::semantic::change::ChangeReaderProfileReceiptV3,
    lease: &GenerationReadLease,
    query: impl for<'db> FnOnce(&'db Connection, &ProgressReadFence) -> Result<T>,
) -> Result<T> {
    with_projection_phase(
        context,
        layout,
        publication,
        descriptor,
        reader_receipt,
        lease,
        None,
        query,
    )
}

/// Actual request-wide file handles remain in N for every native scope.
pub(in crate::session) struct ProjectionRequestWitnesses<'a>(RetainedEvidence<'a>);
impl<'a> ProjectionRequestWitnesses<'a> {
    pub(in crate::session) fn capture(
        context: &'a ReadContext,
        layout: &GenerationLayout,
        publication: &GenerationPublication,
        descriptor: &GenerationDescriptor,
        receipt: &crate::session::derived_access::semantic::change::ChangeReaderProfileReceiptV3,
        lease: &'a GenerationReadLease,
    ) -> Result<Self> {
        Ok(Self(RetainedEvidence::capture_projection_read(
            context,
            layout,
            publication,
            descriptor,
            receipt,
            lease,
        )?))
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "explicit borrowed evidence and phase callback"
)]
pub(in crate::session) fn with_projection_phase<T>(
    context: &ReadContext,
    layout: &GenerationLayout,
    publication: &GenerationPublication,
    descriptor: &GenerationDescriptor,
    reader_receipt: &crate::session::derived_access::semantic::change::ChangeReaderProfileReceiptV3,
    lease: &GenerationReadLease,
    witnesses: Option<&ProjectionRequestWitnesses<'_>>,
    query: impl for<'db> FnOnce(&'db Connection, &ProgressReadFence) -> Result<T>,
) -> Result<T> {
    let mut evidence = RetainedEvidence::capture_projection_read(
        context,
        layout,
        publication,
        descriptor,
        reader_receipt,
        lease,
    )?;
    if let Some(witnesses) = witnesses {
        evidence.retain_scope_witnesses(&witnesses.0)?;
    }
    let before = context.observe_with_witnesses(&evidence)?;
    let reserve = super::super::audit::reserve_retirement(evidence.retirement_read_bytes()?)?;
    let permit = LiveSqlPermit(super::super::owner::SqlPermit::enter()?);
    let mut owner = NativeSqlOwner::prepare(evidence, ScopeProfile::OrdinaryRead);
    let result = owner.open_native(&permit).and_then(|()| {
        owner.with_connection(&permit, |db, native| {
            native.gate(db, before.native_stamp())?;
            let value = query(db, &before)?;
            native.gate(db, before.native_stamp())?;
            Ok(value)
        })
    });
    let finished = owner.finish();
    drop(permit);
    drop(reserve);
    let finished = finished?;
    finished.evidence().validate_live()?;
    accept_check(
        before.native_stamp(),
        &finished.evidence().journal_check(before.native_stamp())?,
    )?;
    let value = result?;
    if context.observe_with_witnesses(finished.evidence())? != before {
        return Err(refuse("progress changed before projection response"));
    }
    if layout
        .current_publication()
        .map_err(|e| refuse(&e.to_string()))?
        .as_ref()
        != Some(publication)
        || layout
            .descriptor(publication)
            .map_err(|e| refuse(&e.to_string()))?
            != *descriptor
    {
        return Err(refuse("generation changed before response"));
    }
    finished.evidence().validate_live()?;
    Ok(value)
}

/// One CREATE_INIT scope owns the complete fresh staging population. Neither
/// this API nor its private target can reopen a failed or published generation.
pub(in crate::session) fn with_staging_projection<'a, T>(
    context: &'a ReadContext,
    layout: &GenerationLayout,
    generation_id: &str,
    authority: &'a StoreAuthorityLock,
    rebuild: &'a RebuildLease,
    populate: impl for<'db> FnOnce(&'db Connection, &ProgressReadFence) -> Result<T>,
) -> Result<(T, StagedProjection<'a>)> {
    let evidence = RetainedEvidence::prepare_projection_staging(
        context,
        layout,
        generation_id,
        authority,
        rebuild,
    )?;
    // Preparation may add the fresh staging directory. Observe only after all
    // admitted provisioning; no reader is asked to create or normalize it.
    let before = context.observe_with_witnesses(&evidence)?;
    evidence.validate_live()?;
    let reserve = super::super::audit::reserve_retirement(evidence.retirement_read_bytes()?)?;
    let permit = LiveSqlPermit(super::super::owner::SqlPermit::enter()?);
    let mut owner = NativeSqlOwner::prepare(evidence, ScopeProfile::CreateInit);
    let result = owner.open_native(&permit).and_then(|()| {
        owner.with_connection(&permit, |db, native| {
            native.gate(db, before.native_stamp())?;
            let value = populate(db, &before)?;
            if !db.is_autocommit() {
                return Err(refuse("staging transaction remains open"));
            }
            let (busy, log, done): (i64, i64, i64) = db
                .query_row("PRAGMA wal_checkpoint(FULL)", [], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .map_err(sql)?;
            if busy != 0 || log < 0 || done != log {
                return Err(refuse("staging checkpoint incomplete"));
            }
            // SAFETY: the lexical native owner retains every Q role until finish.
            if unsafe { ffi::sqlite3_pb_writer_barrier_v1(db.handle(), 0) } != ffi::SQLITE_OK {
                return Err(refuse("staging native barrier failed"));
            }
            native.evidence.sync_directories()?;
            native.gate(db, before.native_stamp())?;
            Ok(value)
        })
    });
    let finished = owner.finish();
    drop(permit);
    drop(reserve);
    let finished = finished?;
    finished.evidence().validate_live()?;
    authority.validate_physical_root(context.store_dir())?;
    let value = result?;
    if context.observe_with_witnesses(finished.evidence())? != before {
        return Err(refuse("progress changed during staging population"));
    }
    finished.evidence().validate_live()?;
    Ok((
        value,
        StagedProjection {
            finished,
            fence: before,
        },
    ))
}

/// Retains the actual finished writer's N through metadata, promotion, final
/// progress observation and publication. It never reopens a native SQL writer.
pub(in crate::session) struct StagedProjection<'a> {
    finished: FinishedScope<'a>,
    fence: ProgressReadFence,
}
impl StagedProjection<'_> {
    pub(in crate::session) fn publish(
        mut self,
        context: &ReadContext,
        layout: &GenerationLayout,
        descriptor: &GenerationDescriptor,
        receipt: &[u8],
    ) -> Result<GenerationPublication> {
        if context.observe_with_witnesses(self.finished.evidence())? != self.fence {
            return Err(refuse("progress changed before metadata"));
        }
        let hash = context.write_rebuild_metadata(
            layout,
            descriptor,
            receipt,
            self.finished.evidence().native_parent()?,
        )?;
        self.finished.evidence().validate_live()?;
        if context.observe_with_witnesses(self.finished.evidence())? != self.fence {
            return Err(refuse("progress changed before promotion"));
        }
        context.promote_rebuild(
            layout,
            &descriptor.generation_id,
            self.finished.evidence().native_parent()?,
        )?;
        self.finished
            .context
            .evidence
            .after_projection_promotion(layout, &descriptor.generation_id)?;
        if context.observe_with_witnesses(self.finished.evidence())? != self.fence {
            return Err(refuse("progress changed before publication"));
        }
        let publication =
            GenerationPublication::new(descriptor.epoch, &descriptor.generation_id, hash);
        context.publish_rebuild(layout, &publication)?;
        self.finished.evidence().validate_live()?;
        if layout
            .current_publication()
            .map_err(|error| refuse(&error.to_string()))?
            .as_ref()
            != Some(&publication)
        {
            return Err(refuse("publication changed before rebuild response"));
        }
        accept_check(
            self.fence.native_stamp(),
            &self
                .finished
                .evidence()
                .journal_check(self.fence.native_stamp())?,
        )?;
        Ok(publication)
    }
}
