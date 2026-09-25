//! Process-local deferred derived receipts.
//!
//! A governed append that finds the derived writer lock busy for longer than
//! its admission budget still publishes authoritative truth, but it proves
//! the same single-carrier transition the admitted path proves and retains
//! the resulting receipt here instead of opening an authority gap. The next
//! holder of the writer lock in this process (the next governed append, the
//! existing maintenance worker, or the coordinator at drop) settles the
//! retained receipts for the exact generation before it proves authority.
//!
//! Receipts never outlive the process. If it exits before settlement the
//! generation shows the same authority gap a loose publication always did and
//! requires an explicit rebuild.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use super::cursor::DeferredCursorReceipt;

/// Receipts retained per exact store beyond this bound break the chain: the
/// coordinator publishes loose and degrades, as it did before deferral.
pub(crate) const MAX_DEFERRED_RECEIPTS_PER_STORE: usize = 64;

#[derive(Clone, Debug, Default)]
pub(crate) struct DeferredReceipts {
    pub(crate) generation_id: String,
    pub(crate) receipts: Vec<DeferredCursorReceipt>,
}

type Registry = HashMap<PathBuf, Arc<Mutex<Option<DeferredReceipts>>>>;

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

/// The retained receipts for one exact store root. Holders serialize deferred
/// publication against settlement, so a receipt chain is observed and settled
/// in order even when the writer and the maintenance worker race.
pub(crate) fn pending_for(store_root: &Path) -> Arc<Mutex<Option<DeferredReceipts>>> {
    let key = store_root
        .canonicalize()
        .unwrap_or_else(|_| store_root.to_path_buf());
    let mut registry = REGISTRY
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    Arc::clone(registry.entry(key).or_default())
}

pub(crate) fn lock_pending(
    pending: &Mutex<Option<DeferredReceipts>>,
) -> MutexGuard<'_, Option<DeferredReceipts>> {
    pending.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether this process retains unsettled receipts for `store_root`.
pub(crate) fn has_pending(store_root: &Path) -> bool {
    let pending = pending_for(store_root);
    let guard = lock_pending(&pending);
    guard
        .as_ref()
        .is_some_and(|entry| !entry.receipts.is_empty())
}
