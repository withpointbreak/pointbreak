//! Enrolled reads use lexical native ownership for every SQL operation.
//! Legacy lifecycle constructors remain closed to this profile.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::Connection;

use super::cursor::TruthCursor;
use super::generation::{GenerationDescriptor, GenerationLayout, GenerationPublication};
use super::locator::LocatorRead;
use super::product_contract::DerivedAccessProfile;
use super::progress::ProgressProjectionBindingV1;
use super::semantic::change::{
    CHANGE_READER_PROFILE_RESOURCE_V3, ChangeReaderProfileReceiptV3, ReaderProjectionCheckpointV1,
    build_change_reader_profile_receipt_v3, initial_reader_projection_checkpoint_v1,
};
use super::service::DerivedAccessService;
use super::sqlite::{
    CursorLedgerIdentity, SqliteCursorLedger, SqliteLocator, SqliteSemantic,
    initialize_enrolled_cursor_on, recompute_authority_cursor_from_identities,
    seed_authority_record_identities,
};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
use crate::error::{Result, ShoreError};
use crate::session::append_progress::{
    ProgressReadFence, ProjectionRequestWitnesses, ReadContext, completed_rebuild_inputs,
    with_projection_phase, with_published_projection, with_staging_projection,
};
use crate::session::store::authority_lock::StoreAuthorityLock;
use crate::session::store::capabilities::StoreCapabilityStatus;

fn fail(error: impl std::fmt::Display) -> ShoreError {
    ShoreError::Message(format!("enrolled derived access: {error}"))
}

/// One explicit O(H) rebuild. Failure preserves staging and never retries.
pub(in crate::session) fn rebuild(
    context: &ReadContext,
    cancelled: Arc<AtomicBool>,
) -> Result<GenerationPublication> {
    if context.profile() != DerivedAccessProfile::SqliteWalBodylessV1 {
        return Err(fail("derived access disabled"));
    }
    crate::session::append_progress::data_work_checkpoint()?;
    let paths = GenerationLayout::new(context.store_dir()).map_err(fail)?;
    context.require_stable_layout(&paths)?;
    let store_id =
        crate::session::store::resolution::opaque_path_identity("store", context.store_dir())?;
    let authority = StoreAuthorityLock::try_acquire_existing(context.store_dir())?
        .ok_or_else(|| fail("authority busy"))?;
    authority.validate_physical_root(context.store_dir())?;
    context.provision_rebuild_layout(&paths)?;
    let rebuild = paths.try_existing_rebuild_lease().map_err(fail)?;
    let (epoch, generation_id) = paths.next_generation().map_err(fail)?;
    // Provision the stable reader leaf before the first authority observation.
    context.provision_generation_lease(&paths, &generation_id)?;
    let inputs = completed_rebuild_inputs(context, &paths, &authority, &rebuild)?;
    if cancelled.load(Ordering::Acquire) {
        return Err(fail("cancelled"));
    }
    let truth = TruthCursor::new(epoch, inputs.fence.head());
    let identity = CursorLedgerIdentity::new(store_id.clone());
    let (receipt, staged) = with_staging_projection(
        context,
        &paths,
        &generation_id,
        &authority,
        &rebuild,
        |db, fence| {
            if fence != &inputs.fence {
                return Err(fail("progress changed before population"));
            }
            let entries = initialize_enrolled_cursor_on(
                db,
                &identity,
                epoch,
                fence.native_stamp(),
                inputs.events,
            )
            .map_err(fail)?;
            SqliteSemantic::populate_enrolled_on(db, truth, &entries).map_err(fail)?;
            SqliteLocator::validate_enrolled_rows_on(db, &entries).map_err(fail)?;
            let snapshot =
                SqliteSemantic::validate_enrolled_materialization_on(db, truth, &entries)
                    .map_err(fail)?;
            seed_authority_record_identities(db, &inputs.inspection.authority_record_identities)
                .map_err(fail)?;
            let cursor = recompute_authority_cursor_from_identities(
                db,
                &inputs.inspection.cursor.capability_set_hash,
            )
            .map_err(fail)?;
            if cursor != inputs.inspection.cursor {
                return Err(fail("full authority cursor mismatch"));
            }
            let StoreCapabilityStatus::Ready { completion_id, .. } = &inputs.inspection.status
            else {
                return Err(fail("capability is incomplete"));
            };
            let witness = |key: &str| -> Result<String> {
                let digest = sha256_bytes_hex(key.as_bytes());
                inputs
                    .inspection
                    .authority_record_identities
                    .iter()
                    .find(|v| v.key_digest == digest)
                    .map(|v| v.record_sha256.clone())
                    .ok_or_else(|| fail("capability witness missing"))
            };
            let events = entries
                .iter()
                .map(|entry| entry.event.clone())
                .collect::<Vec<_>>();
            let LocatorRead::Ready(changes) =
                SqliteSemantic::materialized_change_projection_on(db, truth).map_err(fail)?
            else {
                return Err(fail("Change projection behind"));
            };
            let receipt = build_change_reader_profile_receipt_v3(
                &generation_id,
                &store_id,
                &inputs.inspection,
                witness("store_capability_activation:review_change_revision_v1:root")?,
                witness(&format!("bulk_adoption_completion:{completion_id}"))?,
                fence.native_stamp().clone(),
                truth,
                &events,
                &snapshot,
                &changes.document_projection,
            )?;
            let checkpoint = initial_reader_projection_checkpoint_v1(&receipt).map_err(fail)?;
            let binding = ProgressProjectionBindingV1::from_observation(
                fence,
                &generation_id,
                epoch,
                &cursor,
            )?;
            SqliteSemantic::seed_enrolled_reader_checkpoint_on(db, &checkpoint, &binding)
                .map_err(fail)?;
            validate_snapshot(db, &identity, &receipt, fence)?;
            if cancelled.load(Ordering::Acquire) {
                return Err(fail("cancelled"));
            }
            Ok(receipt)
        },
    )?;
    // The writer and all native handles are fully finished before metadata
    // publication or any fresh readonly operation.
    let descriptor = GenerationDescriptor::new(
        &generation_id,
        &store_id,
        context.profile(),
        epoch,
        truth.sequence,
        inputs.fence.native_stamp().clone(),
        &receipt.receipt_sha256,
    );
    if cancelled.load(Ordering::Acquire) {
        return Err(fail("cancelled"));
    }
    staged.publish(
        context,
        &paths,
        &descriptor,
        &canonical_json_bytes(&serde_json::to_value(&receipt)?)?,
    )
}

fn validate_snapshot(
    db: &Connection,
    identity: &CursorLedgerIdentity,
    receipt: &ChangeReaderProfileReceiptV3,
    fence: &ProgressReadFence,
) -> Result<ReaderProjectionCheckpointV1> {
    let raw = SqliteCursorLedger::enrolled_publication_snapshot_on(db, identity).map_err(fail)?;
    let snapshot =
        DerivedAccessService::decode_publication_validation_snapshot(raw).map_err(fail)?;
    let checkpoint = snapshot
        .reader_projection_checkpoint
        .ok_or_else(|| fail("reader checkpoint missing"))?;
    checkpoint.validate_for_receipt(receipt).map_err(fail)?;
    SqliteLocator::validate_enrolled_checkpoint_on(db).map_err(fail)?;
    SqliteSemantic::validate_enrolled_checkpoint_on(db, checkpoint.truth_cursor).map_err(fail)?;
    if snapshot.authority.head.cursor != checkpoint.truth_cursor
        || snapshot.locator_applied != checkpoint.truth_cursor
        || checkpoint.truth_cursor != receipt.publication_truth_cursor
        || checkpoint.authority_cursor != receipt.publication_authority_cursor
        || snapshot.authority.change_stamp != *fence.native_stamp()
        || checkpoint.truth_cursor.sequence != fence.head()
    {
        return Err(fail("generation is stale; explicit rebuild required"));
    }
    snapshot
        .progress_binding
        .ok_or_else(|| fail("progress binding missing"))?
        .validate(
            fence,
            &receipt.publication_generation_id,
            checkpoint.truth_cursor.epoch,
            &checkpoint.authority_cursor,
        )?;
    Ok(checkpoint)
}

/// The request's entire derived query runs in one native snapshot. Results must
/// be owned values; postflight and explicit native finish precede their return.
pub(in crate::session) fn read<T>(
    context: &ReadContext,
    expected: Option<&GenerationPublication>,
    query: impl for<'db> FnOnce(
        &'db Connection,
        &ReaderProjectionCheckpointV1,
        &ChangeReaderProfileReceiptV3,
    ) -> Result<T>,
) -> Result<T> {
    if context.profile() != DerivedAccessProfile::SqliteWalBodylessV1 {
        return Err(fail("derived access disabled"));
    }
    let paths = GenerationLayout::new(context.store_dir()).map_err(fail)?;
    context.require_stable_layout(&paths)?;
    let store_id =
        crate::session::store::resolution::opaque_path_identity("store", context.store_dir())?;
    let publication = paths
        .current_publication()
        .map_err(fail)?
        .ok_or_else(|| fail("projection absent; explicit rebuild required"))?;
    if expected.is_some_and(|old| old != &publication) {
        return Err(fail("cached generation replaced"));
    }
    let lease = paths
        .acquire_existing_read_lease(&publication.generation_id)
        .map_err(fail)?;
    let descriptor = paths.descriptor(&publication).map_err(fail)?;
    let path = paths
        .generation(&publication.generation_id)
        .join(CHANGE_READER_PROFILE_RESOURCE_V3);
    use std::io::Read as _;
    let mut bytes = Vec::new();
    crate::session::store::authority_lock::open_identity_path(&path, false, false)?
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(fail)?;
    if bytes.len() > 1_048_576 {
        return Err(fail("reader receipt exceeds fixed limit"));
    }
    let receipt: ChangeReaderProfileReceiptV3 = serde_json::from_slice(&bytes)?;
    receipt.validate().map_err(fail)?;
    if receipt.minimum_reader_profile != "review_change_revision_progress_v1"
        || receipt.publication_store_id != store_id
        || descriptor.store_id != store_id
        || receipt.publication_generation_id != publication.generation_id
        || receipt.publication_truth_cursor
            != TruthCursor::new(descriptor.epoch, descriptor.head_sequence)
        || receipt.receipt_sha256 != descriptor.semantic_receipt
        || receipt.publication_platform_stamp != descriptor.authority_stamp
        || descriptor.profile != context.profile()
    {
        return Err(fail("publication identity mismatch"));
    }
    let identity = CursorLedgerIdentity::new(store_id);
    with_published_projection(
        context,
        &paths,
        &publication,
        &descriptor,
        &receipt,
        &lease,
        |db, fence| {
            let checkpoint = validate_snapshot(db, &identity, &receipt, fence)?;
            query(db, &checkpoint, &receipt)
        },
    )
}

/// A lexical response pin: native scopes never outlive a phase, while all
/// request witnesses and the actual generation lease survive hydration.
pub(in crate::session) struct RequestPin<'a> {
    context: &'a ReadContext,
    paths: GenerationLayout,
    publication: GenerationPublication,
    descriptor: GenerationDescriptor,
    receipt: ChangeReaderProfileReceiptV3,
    lease: &'a super::generation::GenerationReadLease,
    witnesses: ProjectionRequestWitnesses<'a>,
    identity: CursorLedgerIdentity,
    checkpoint: ReaderProjectionCheckpointV1,
    fence: ProgressReadFence,
    phases: usize,
}
pub(in crate::session) fn with_request_pin<T>(
    context: &ReadContext,
    compose: impl for<'pin> FnOnce(RequestPin<'pin>) -> Result<T>,
) -> Result<T> {
    if context.profile() != DerivedAccessProfile::SqliteWalBodylessV1 {
        return Err(fail("derived access disabled"));
    }
    let paths = GenerationLayout::new(context.store_dir()).map_err(fail)?;
    context.require_stable_layout(&paths)?;
    let store_id =
        crate::session::store::resolution::opaque_path_identity("store", context.store_dir())?;
    let publication = paths
        .current_publication()
        .map_err(fail)?
        .ok_or_else(|| fail("projection absent; explicit rebuild required"))?;
    let lease = paths
        .acquire_existing_read_lease(&publication.generation_id)
        .map_err(fail)?;
    let descriptor = paths.descriptor(&publication).map_err(fail)?;
    let path = paths
        .generation(&publication.generation_id)
        .join(CHANGE_READER_PROFILE_RESOURCE_V3);
    let bytes = crate::session::append_progress::data_file_bytes(&path, 1_048_576)?;
    let receipt: ChangeReaderProfileReceiptV3 = serde_json::from_slice(&bytes)?;
    receipt.validate().map_err(fail)?;
    if receipt.minimum_reader_profile != "review_change_revision_progress_v1"
        || receipt.publication_store_id != store_id
        || descriptor.store_id != store_id
        || receipt.publication_generation_id != publication.generation_id
        || receipt.publication_truth_cursor
            != TruthCursor::new(descriptor.epoch, descriptor.head_sequence)
        || receipt.receipt_sha256 != descriptor.semantic_receipt
        || receipt.publication_platform_stamp != descriptor.authority_stamp
        || descriptor.profile != context.profile()
    {
        return Err(fail("publication identity mismatch"));
    }
    let identity = CursorLedgerIdentity::new(store_id);

    let witnesses = ProjectionRequestWitnesses::capture(
        context,
        &paths,
        &publication,
        &descriptor,
        &receipt,
        &lease,
    )?;
    let (checkpoint, fence) = with_projection_phase(
        context,
        &paths,
        &publication,
        &descriptor,
        &receipt,
        &lease,
        Some(&witnesses),
        |db, fence| {
            Ok((
                validate_snapshot(db, &identity, &receipt, fence)?,
                fence.clone(),
            ))
        },
    )?;
    compose(RequestPin {
        context,
        paths,
        publication,
        descriptor,
        receipt,
        lease: &lease,
        witnesses,
        identity,
        checkpoint,
        fence,
        phases: 1,
    })
}
impl RequestPin<'_> {
    pub(in crate::session) fn checkpoint(&self) -> &ReaderProjectionCheckpointV1 {
        &self.checkpoint
    }
    pub(in crate::session) fn receipt(&self) -> &ChangeReaderProfileReceiptV3 {
        &self.receipt
    }
    pub(in crate::session) fn context(&self) -> &ReadContext {
        self.context
    }
    pub(in crate::session) fn phase<T>(
        &mut self,
        query: impl for<'db> FnOnce(
            &'db Connection,
            &ReaderProjectionCheckpointV1,
            &ChangeReaderProfileReceiptV3,
        ) -> Result<T>,
    ) -> Result<T> {
        crate::session::append_progress::data_work_checkpoint()?;
        self.phases += 1;
        if self.phases > 32 {
            return Err(fail("request SQL phase limit"));
        }
        with_projection_phase(
            self.context,
            &self.paths,
            &self.publication,
            &self.descriptor,
            &self.receipt,
            self.lease,
            Some(&self.witnesses),
            |db, fence| {
                let checkpoint = validate_snapshot(db, &self.identity, &self.receipt, fence)?;
                if fence != &self.fence || checkpoint != self.checkpoint {
                    return Err(fail("request pin changed between phases"));
                }
                query(db, &checkpoint, &self.receipt)
            },
        )
    }
    pub(in crate::session) fn finish(mut self) -> Result<()> {
        // Runs after every filesystem/resource/trust/cache effect. The native
        // producer revalidates actual N, namespace, journal and publication.
        self.phase(|_, _, _| Ok(()))
    }
}

#[cfg(test)]
mod pin_controls;
