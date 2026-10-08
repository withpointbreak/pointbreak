//! Detached completed-state fences. Only a binary-owned caller may observe SQL.
use std::path::PathBuf;

use super::enrollment::{EvidenceRole, RetainedEvidence};
use super::{ledger, native, owner};
use crate::canonical_hash::sha256_json_hex;
use crate::error::{Result, ShoreError};
use crate::session::store::authority_lock::PhysicalFileIdentity;
use crate::session::store::backend::JournalChangeStamp;

#[cfg(target_os = "macos")]
mod rebuild;

/// Each field comes from retained N and one completed native SQL snapshot.
/// Detached identity is comparison evidence, never permission to reopen a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProgressReadFence {
    root: native::RootSnapshot,
    members: Vec<(
        EvidenceRole,
        PathBuf,
        Option<PhysicalFileIdentity>,
        Option<String>,
    )>,
    selection_sha256: String,
    baseline_sha256: String,
    capability_pair_sha256: String,
    progress_epoch: String,
    head: u64,
    prefix_sha256: String,
    native: JournalChangeStamp,
    completed_state_sha256: String,
}
impl ProgressReadFence {
    pub(super) fn completed(
        evidence: &RetainedEvidence<'_>,
        head: i64,
        prefix_sha256: String,
        native: JournalChangeStamp,
        completed_state_sha256: String,
    ) -> Result<Self> {
        let binding = evidence.inputs().bindings()?;
        let progress_epoch = binding.selection["progressEpoch"]
            .as_str()
            .ok_or_else(|| ShoreError::Message("missing progress epoch".into()))?
            .to_owned();
        let mut members = Vec::new();
        for member in evidence.members().iter().filter(|m| m.progress_identity()) {
            let identity = (
                member.role(),
                member.path().to_path_buf(),
                member.identity(),
                member.content_sha256(),
            );
            if !members.contains(&identity) {
                members.push(identity);
            }
        }
        Ok(Self {
            root: super::native::root_snapshot(evidence.root())?,
            members,
            selection_sha256: sha256_json_hex(&binding.selection)?,
            baseline_sha256: sha256_json_hex(&binding.baseline)?,
            capability_pair_sha256: sha256_json_hex(&binding.pair)?,
            progress_epoch,
            head: u64::try_from(head)
                .map_err(|_| ShoreError::Message("negative progress head".into()))?,
            prefix_sha256,
            native,
            completed_state_sha256,
        })
    }
    pub(crate) fn selection_sha256(&self) -> &str {
        &self.selection_sha256
    }
    pub(crate) fn baseline_sha256(&self) -> &str {
        &self.baseline_sha256
    }
    pub(crate) fn capability_pair_sha256(&self) -> &str {
        &self.capability_pair_sha256
    }
    pub(crate) fn progress_epoch(&self) -> &str {
        &self.progress_epoch
    }
    pub(crate) fn head(&self) -> u64 {
        self.head
    }
    pub(crate) fn prefix_sha256(&self) -> &str {
        &self.prefix_sha256
    }
    pub(crate) fn native_stamp(&self) -> &JournalChangeStamp {
        &self.native
    }
}

#[cfg(target_os = "macos")]
pub(super) fn observe(context: &ReadContext) -> Result<ProgressReadFence> {
    owner::require_quiescent()?;
    ledger::c4::observe_completed(RetainedEvidence::capture_read(context)?)
}

/// Actual clone admission and physical roots live across pre/query/post scopes.
/// This context is never constructed by a stock/shared-host service.
#[cfg(target_os = "macos")]
pub(in crate::session) struct ReadContext {
    pub(super) root: native::NativeRootHandle,
    pub(super) repository_root: native::NativeRootHandle,
    pub(super) clone: crate::session::store::family_topology::CloneBindingGuard,
    pub(super) common: PathBuf,
    pub(super) request: String,
    pub(super) actor: String,
    store: PathBuf,
    profile: crate::session::derived_access::product_contract::DerivedAccessProfile,
}
#[cfg(target_os = "macos")]
impl ReadContext {
    pub(in crate::session) fn begin(
        repo: &std::path::Path,
        request: &str,
        actor: &str,
    ) -> Result<Self> {
        owner::require_quiescent()?;
        super::enrollment::validate_request(request, true, "review_change_revision_progress_v1")?;
        let common = crate::paths::CommonDirPaths::resolve(repo)?;
        let repository = crate::paths::RepositoryPaths::resolve(repo)?;
        let clone =
            crate::session::store::family_topology::CloneBindingGuard::try_acquire_existing(
                common.common_dir(),
            )?
            .ok_or_else(|| {
                ShoreError::Message("clone_binding_busy; retry original operation".into())
            })?;
        let resolved = crate::session::store::resolution::resolve_owned_data_store(
            repo,
            &clone,
            common.common_dir(),
        )?;
        if resolved.store_dir() != common.store_dir() {
            return Err(ShoreError::Message(
                "private data entry requires a disposable clone-local store".into(),
            ));
        }
        let repository_root = native::read_root(repository.worktree_root())?;
        let root = native::read_root(resolved.store_dir())?;
        let store = native::root_snapshot(&root)?.canonical;
        clone.validate(common.common_dir())?;
        let value = Self {
            root,
            repository_root,
            clone,
            common: common.common_dir().to_path_buf(),
            request: request.into(),
            actor: actor.into(),
            store,
            profile: resolved.derived_access_profile(),
        };
        // Complete bounded admission before a derived reader can use this context.
        value.observe()?;
        Ok(value)
    }
    pub(in crate::session) fn store_dir(&self) -> &std::path::Path {
        &self.store
    }
    pub(in crate::session) fn profile(
        &self,
    ) -> crate::session::derived_access::product_contract::DerivedAccessProfile {
        self.profile
    }
    pub(super) fn validate_resolution(&self) -> Result<()> {
        owner::require_quiescent()?;
        let repository = native::root_snapshot(&self.repository_root)?.canonical;
        let resolved = crate::session::store::resolution::resolve_owned_data_store(
            &repository,
            &self.clone,
            &self.common,
        )?;
        if resolved.store_dir() != self.store || resolved.derived_access_profile() != self.profile {
            return Err(ShoreError::Message(
                "owned data store resolution changed".into(),
            ));
        }
        Ok(())
    }
    pub(in crate::session) fn observe(&self) -> Result<ProgressReadFence> {
        observe(self)
    }
    pub(super) fn observe_with_witnesses(
        &self,
        retained: &RetainedEvidence<'_>,
    ) -> Result<ProgressReadFence> {
        owner::require_quiescent()?;
        let mut evidence = RetainedEvidence::capture_read(self)?;
        evidence.retain_scope_witnesses(retained)?;
        ledger::c4::observe_completed(evidence)
    }
}
