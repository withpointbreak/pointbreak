//! Shared interaction policy for optional derived read acceleration.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static HINTED_STORES: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

/// Explain one command-local fallback without exposing the disposable
/// implementation. The process emits at most one hint for an exact resolved
/// store even when an embedded caller invokes more than one command.
pub(super) fn emit_authoritative_fallback_hint(repo: &Path) {
    let store =
        pointbreak::session::store_dir_for_repo(repo).unwrap_or_else(|_| repo.to_path_buf());
    if !HINTED_STORES
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .expect("derived read hint lock poisoned")
        .insert(store)
    {
        return;
    }
    eprintln!(
        "hint: derived access is unavailable; using authoritative journal data; \
         run `pointbreak store derived status` or `pointbreak store derived build`"
    );
}
