use std::path::{Path, PathBuf};

use crate::error::{Result, ShoreError};
use crate::git::git_worktree_root;
use crate::session::store::backend::{
    JournalChangeCheck, JournalChangeStamp, JournalChangeVerdict,
};
use crate::session::store::capabilities::{
    BoundedChangeCapabilityPairStateV1, BoundedChangeCapabilityPairV1,
    bounded_change_capability_pair_state_v1,
};
use crate::session::store::resolution::{ReadStore, resolve_public_read_context_store_v1};

/// One invocation's repository-bound proof for a catalog-qualified public read.
///
/// The value intentionally implements neither `Clone` nor Serde traits. It must
/// be consumed by one workflow and closed with its postflight before output.
///
/// ```compile_fail
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<pointbreak::session::PublicReadCommandContextV1>();
/// ```
///
/// ```compile_fail
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<pointbreak::session::PublicReadCommandContextV1>();
/// ```
#[doc(hidden)]
pub struct PublicReadCommandContextV1 {
    canonical_repository: PathBuf,
    canonical_store: PathBuf,
    read_store: ReadStore,
    #[allow(dead_code, reason = "the context retains the bounded authority proof")]
    capability_pair: BoundedChangeCapabilityPairV1,
    after_pair: JournalChangeStamp,
}

impl PublicReadCommandContextV1 {
    pub(crate) fn require_repository(&self, repo: &Path) -> Result<()> {
        if git_worktree_root(repo)? != self.canonical_repository {
            return Err(ShoreError::WorkflowInputInvalid {
                reason: "public read command context belongs to a different repository".to_owned(),
            });
        }
        let current_store = self
            .read_store
            .store_dir()
            .canonicalize()
            .map_err(|error| {
                ShoreError::Message(format!(
                    "could not canonicalize bound public read store {}: {error}",
                    self.read_store.store_dir().display()
                ))
            })?;
        if current_store != self.canonical_store {
            return Err(ShoreError::WorkflowInputInvalid {
                reason: "public read command context store binding changed".to_owned(),
            });
        }
        Ok(())
    }

    pub(crate) fn read_store(&self) -> &ReadStore {
        &self.read_store
    }

    #[cfg(test)]
    pub(crate) fn with_derived_access_profile_for_test(
        mut self,
        profile: crate::session::derived_access::product_contract::DerivedAccessProfile,
    ) -> Self {
        self.read_store = self
            .read_store
            .with_derived_access_profile_for_test(profile);
        self
    }

    pub(crate) fn postflight(self) -> Result<()> {
        let check = self
            .read_store
            .backend()
            .journal()
            .changes_since(&self.after_pair)?;
        require_stable_change_check(check, "public read postflight")?;
        Ok(())
    }
}

fn require_stable_change_check(
    check: JournalChangeCheck,
    phase: &'static str,
) -> Result<JournalChangeStamp> {
    match check.verdict {
        JournalChangeVerdict::Stable => Ok(check.after),
        JournalChangeVerdict::Changed => Err(ShoreError::Message(format!(
            "public read command authority changed during {phase}; refusing output"
        ))),
        JournalChangeVerdict::Indeterminate => Err(ShoreError::Message(format!(
            "public read command authority was indeterminate during {phase}; refusing output"
        ))),
    }
}

#[doc(hidden)]
pub fn prepare_public_read_command_context_v1(repo: &Path) -> Result<PublicReadCommandContextV1> {
    let canonical_repository = git_worktree_root(repo)?;
    let read_store = resolve_public_read_context_store_v1(&canonical_repository)?;
    let journal = read_store.backend().journal();
    let before_pair = journal.change_stamp()?;
    let pair = match bounded_change_capability_pair_state_v1(journal.as_ref())? {
        BoundedChangeCapabilityPairStateV1::MigrationRequired => {
            return Err(ShoreError::Message(
                "migration_required; this command requires an explicit completed store migration"
                    .to_owned(),
            ));
        }
        BoundedChangeCapabilityPairStateV1::MigrationInProgress { .. } => {
            return Err(ShoreError::Message(
                "migration_in_progress; this command refuses partial Change authority".to_owned(),
            ));
        }
        BoundedChangeCapabilityPairStateV1::Ready(pair) => pair,
    };
    let after_pair = require_stable_change_check(
        journal.changes_since(&before_pair)?,
        "bounded capability preparation",
    )?;
    let canonical_store = read_store.store_dir().canonicalize().map_err(|error| {
        ShoreError::Message(format!(
            "could not canonicalize public read store {}: {error}",
            read_store.store_dir().display()
        ))
    })?;

    Ok(PublicReadCommandContextV1 {
        canonical_repository,
        canonical_store,
        read_store,
        capability_pair: pair,
        after_pair,
    })
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::bench_support::longitudinal::LongitudinalCountingScopeV1;
    use crate::session::store::capabilities::{
        CapabilityFixtureState, StoreCapabilityStatus, inspect_journal_records,
        write_capability_fixture_for_test,
    };
    use crate::session::store::resolution::resolve_change_read_backend;

    fn repository() -> TempDir {
        let repo = TempDir::new().expect("create repository");
        let output = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repo.path())
            .output()
            .expect("launch git init");
        assert!(output.status.success(), "git init failed: {output:?}");
        repo
    }

    fn capability_store(repo: &Path, state: CapabilityFixtureState) -> ReadStore {
        let store = resolve_change_read_backend(repo).expect("resolve fixture store");
        write_capability_fixture_for_test(store.backend().journal().as_ref(), state)
            .expect("write capability fixture");
        store
    }

    #[test]
    fn l2_factory_builds_one_bounded_repository_context_without_history_work() {
        let repo = repository();
        let store = capability_store(repo.path(), CapabilityFixtureState::L2);
        let scope = LongitudinalCountingScopeV1::new("2".repeat(64)).unwrap();
        let context = {
            let _guard = scope.enter();
            prepare_public_read_command_context_v1(repo.path()).unwrap()
        };

        assert_eq!(
            context.canonical_repository,
            git_worktree_root(repo.path()).unwrap()
        );
        assert_eq!(
            context.canonical_store,
            store.store_dir().canonicalize().unwrap()
        );
        assert_eq!(
            context.read_store().store_dir().canonicalize().unwrap(),
            store.store_dir().canonicalize().unwrap()
        );
        let counters = scope.snapshot().counters;
        assert_eq!(counters.directory_entries_walked, 0);
        assert_eq!(counters.carrier_opens, 2);
        assert_eq!(counters.change_capability_carriers_opened, 2);
        assert_eq!(counters.event_decodes, 0);
        assert_eq!(counters.event_folds, 0);
        assert_eq!(counters.projection_rebuilds, 0);
        assert_eq!(counters.state_rebuilds, 0);
        context.postflight().unwrap();
    }

    #[test]
    fn factory_preserves_l0_and_m1_refusal_precedence() {
        let l0 = repository();
        let l0_error = prepare_public_read_command_context_v1(l0.path())
            .err()
            .expect("L0 must refuse")
            .to_string();
        assert!(l0_error.contains("migration_required"), "{l0_error}");

        let m1 = repository();
        capability_store(m1.path(), CapabilityFixtureState::M1);
        let m1_error = prepare_public_read_command_context_v1(m1.path())
            .err()
            .expect("M1 must refuse")
            .to_string();
        assert!(m1_error.contains("migration_in_progress"), "{m1_error}");
    }

    #[test]
    fn factory_rejects_unknown_or_mismatched_bounded_carriers() {
        for (carrier, field) in [
            ("activation", "schema"),
            ("activation", "activationId"),
            ("completion", "activationId"),
        ] {
            let repo = repository();
            let store = capability_store(repo.path(), CapabilityFixtureState::L2);
            let journal = store.backend().journal();
            let root_key = "store_capability_activation:review_change_revision_v1:root";
            let logical_key = if carrier == "activation" {
                root_key.to_owned()
            } else {
                let StoreCapabilityStatus::Ready { completion_id, .. } =
                    inspect_journal_records(journal.as_ref()).unwrap().status
                else {
                    panic!("fixture must be ready");
                };
                format!("bulk_adoption_completion:{completion_id}")
            };
            let bytes = journal.read_event_bytes(&logical_key).unwrap().unwrap();
            let mut record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            record[field] = serde_json::json!(format!("corrupt-{field}"));
            journal
                .insert_raw(&logical_key, &serde_json::to_vec(&record).unwrap())
                .unwrap();

            assert!(prepare_public_read_command_context_v1(repo.path()).is_err());
        }
    }

    #[test]
    fn context_rejects_a_different_repository() {
        let first = repository();
        capability_store(first.path(), CapabilityFixtureState::L2);
        let second = repository();
        let context = prepare_public_read_command_context_v1(first.path()).unwrap();

        let error = context
            .require_repository(second.path())
            .unwrap_err()
            .to_string();
        assert!(error.contains("different repository"), "{error}");
    }

    #[test]
    fn equal_head_marker_cannot_hide_movement_and_a_new_invocation_reproves() {
        let repo = repository();
        let store = capability_store(repo.path(), CapabilityFixtureState::L2);
        let context = prepare_public_read_command_context_v1(repo.path()).unwrap();
        let journal = store.backend().journal();
        let root_key = "store_capability_activation:review_change_revision_v1:root";
        let before_head = journal.head_marker().unwrap();
        let bytes = journal.read_event_bytes(root_key).unwrap().unwrap();
        journal.insert_raw(root_key, &bytes).unwrap();

        assert_eq!(journal.head_marker().unwrap(), before_head);
        let error = context.postflight().unwrap_err().to_string();
        assert!(error.contains("changed"), "{error}");

        prepare_public_read_command_context_v1(repo.path())
            .unwrap()
            .postflight()
            .unwrap();
    }

    #[test]
    fn changed_and_indeterminate_native_checks_both_fail_closed() {
        for verdict in [
            JournalChangeVerdict::Changed,
            JournalChangeVerdict::Indeterminate,
        ] {
            let check = JournalChangeCheck {
                after: JournalChangeStamp::Absent,
                verdict,
                native_bytes_examined: 0,
                native_records_examined: 0,
                relevant_file_references: Vec::new(),
                mechanism: "injected test verdict".to_owned(),
            };
            assert!(require_stable_change_check(check, "test").is_err());
        }
    }

    #[test]
    fn bounded_resolver_has_one_factory_call_site_and_no_public_export() {
        let resolution = include_str!("resolution.rs");
        let context = include_str!("read_context.rs");
        let store_module = include_str!("mod.rs");
        let factory_call = concat!(
            "resolve_public_read_context_",
            "store_v1(&canonical_repository)"
        );
        assert_eq!(
            resolution
                .matches("fn resolve_public_read_context_store_v1(")
                .count(),
            1
        );
        assert_eq!(context.matches(factory_call).count(), 1);
        assert!(!store_module.contains("resolve_public_read_context_store_v1,"));
    }
}
