use crate::session::state::ProjectionDiagnostic;
use crate::session::store::resolution::{StoreResolutionMode, WriteValidationStore};

pub(crate) const CLONE_LOCAL_FACT_BATCH_ONLY_CODE: &str = "clone_local_fact_batch_only";

/// Linked-mode fact writes land worktree-local and are invisible to reads until
/// `shore store link`. This diagnostic tells the writer at write time that the
/// fact it just recorded needs syncing, mirroring `clone_local_capture_batch_only`
/// for captures and the read-side `clone_local_unsynced_local_events`.
///
/// Empty in worktree-local mode, so it is purely additive: the unlinked write
/// path is byte-identical.
pub(crate) fn fact_batch_only_diagnostics(
    store: &WriteValidationStore,
) -> Vec<ProjectionDiagnostic> {
    if store.read_store().resolution.mode != StoreResolutionMode::CloneLocal {
        return Vec::new();
    }
    vec![ProjectionDiagnostic {
        code: CLONE_LOCAL_FACT_BATCH_ONLY_CODE.to_owned(),
        message:
            "this fact was written to the worktree-local store; run shore store link to copy it to the linked clone-local store"
                .to_owned(),
    }]
}
