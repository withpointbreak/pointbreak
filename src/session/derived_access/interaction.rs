//! Process-local interaction policy for unavailable derived generations.
//!
//! Reads and writes share this exact-store cadence so one unavailable
//! generation cannot produce a stream of equivalent recovery hints from
//! different product surfaces.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub(super) const RECOVERY_ACTION: &str =
    "run `pointbreak store derived status` or `pointbreak store derived build`";
pub(super) const AUTHORITATIVE_FALLBACK_HINT: &str = "hint: derived access is unavailable; using authoritative journal data; run `pointbreak store derived status` or `pointbreak store derived build`";

static HINTED_STORES: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

pub(super) fn claim_unavailable_hint(store_root: &Path) -> bool {
    HINTED_STORES
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .expect("derived hint store lock poisoned")
        .insert(store_root.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_store_claim_is_shared_and_process_local() {
        let store = tempfile::tempdir().expect("hint store tempdir");

        assert!(claim_unavailable_hint(store.path()));
        assert!(!claim_unavailable_hint(store.path()));
    }
}
