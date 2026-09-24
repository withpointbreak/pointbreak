//! Test-only authority-check verdicts, queued per store and check site.
//!
//! Outside Windows a native authority check reports only `Stable` or
//! `Changed`, and a truth append can force `Changed` anywhere. An unproven
//! `Indeterminate` verdict comes only from an exhausted NTFS journal budget, so
//! tests queue the check a site reports instead of provoking one.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use crate::session::store::backend::{
    JournalChangeCheck, JournalChangeStamp, JournalChangeVerdict,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum AuthorityCheckSite {
    PrePublication,
    BootstrapPopulation,
}

type QueuedChecks = HashMap<(PathBuf, AuthorityCheckSite), VecDeque<JournalChangeCheck>>;

static QUEUED_CHECKS: OnceLock<Mutex<QueuedChecks>> = OnceLock::new();

/// Queue `check` for the next authority check at `site` in `store_root`.
/// Checks are used once each, in the order they were queued.
pub(crate) fn queue_authority_check(
    store_root: &Path,
    site: AuthorityCheckSite,
    check: JournalChangeCheck,
) {
    QUEUED_CHECKS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry((canonical_store_root(store_root), site))
        .or_default()
        .push_back(check);
}

/// The oldest check queued for `site` in `store_root`, removing it.
pub(crate) fn take_queued_authority_check(
    store_root: &Path,
    site: AuthorityCheckSite,
) -> Option<JournalChangeCheck> {
    let mut queued = QUEUED_CHECKS
        .get()?
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let key = (canonical_store_root(store_root), site);
    let checks = queued.get_mut(&key)?;
    let check = checks.pop_front();
    if checks.is_empty() {
        queued.remove(&key);
    }
    check
}

/// A check that could not prove authority stable within its budget.
pub(crate) fn unproven_check(
    mechanism: &str,
    native_bytes_examined: u64,
    native_records_examined: u64,
) -> JournalChangeCheck {
    JournalChangeCheck {
        after: JournalChangeStamp::Absent,
        verdict: JournalChangeVerdict::Indeterminate,
        native_bytes_examined,
        native_records_examined,
        relevant_file_references: Vec::new(),
        mechanism: mechanism.to_owned(),
    }
}

/// A check that proved authoritative truth moved.
pub(crate) fn changed_check(mechanism: &str) -> JournalChangeCheck {
    JournalChangeCheck {
        after: JournalChangeStamp::Absent,
        verdict: JournalChangeVerdict::Changed,
        native_bytes_examined: 4096,
        native_records_examined: 1,
        relevant_file_references: Vec::new(),
        mechanism: mechanism.to_owned(),
    }
}

fn canonical_store_root(store_root: &Path) -> PathBuf {
    std::fs::canonicalize(store_root).unwrap_or_else(|_| store_root.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_checks_are_used_once_in_order_per_store_and_site() {
        let store = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        queue_authority_check(
            store.path(),
            AuthorityCheckSite::PrePublication,
            unproven_check("first", 1, 1),
        );
        queue_authority_check(
            store.path(),
            AuthorityCheckSite::PrePublication,
            changed_check("second"),
        );

        for (root, site) in [
            (other.path(), AuthorityCheckSite::PrePublication),
            (store.path(), AuthorityCheckSite::BootstrapPopulation),
        ] {
            assert_eq!(take_queued_authority_check(root, site), None);
        }
        // The same directory through a different spelling is the same store.
        let respelled = store.path().join(".");
        let taken = [
            take_queued_authority_check(&respelled, AuthorityCheckSite::PrePublication),
            take_queued_authority_check(store.path(), AuthorityCheckSite::PrePublication),
            take_queued_authority_check(store.path(), AuthorityCheckSite::PrePublication),
        ];
        let mechanisms = taken
            .iter()
            .map(|check| check.as_ref().map(|check| check.mechanism.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(mechanisms, [Some("first"), Some("second"), None]);
    }
}
