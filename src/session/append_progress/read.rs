//! Detached completed-state fences. Only a binary-owned caller may observe SQL.
use std::path::PathBuf;

use super::enrollment::{EvidenceRole, RetainedEvidence};
use super::native;
#[cfg(target_os = "macos")]
use super::{ledger, owner};
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
    #[cfg(target_os = "macos")]
    pub(crate) fn selection_sha256(&self) -> &str {
        &self.selection_sha256
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn baseline_sha256(&self) -> &str {
        &self.baseline_sha256
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn capability_pair_sha256(&self) -> &str {
        &self.capability_pair_sha256
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn progress_epoch(&self) -> &str {
        &self.progress_epoch
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn head(&self) -> u64 {
        self.head
    }
    #[cfg(target_os = "macos")]
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
    pub(in crate::session) fn selected_event_bytes(&self, digest: &str) -> Result<Vec<u8>> {
        super::require_quiescent_data()?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ShoreError::Message(
                "invalid selected event key digest".into(),
            ));
        }
        self.clone.validate(&self.common)?;
        let path = self.store.join("events").join(format!("{digest}.json"));
        let file = crate::session::store::authority_lock::open_identity_path(&path, false, false)?;
        let length = file
            .metadata()
            .map_err(|error| ShoreError::Message(error.to_string()))?
            .len();
        if length > 16 * 1024 * 1024 {
            return Err(ShoreError::Message("selected carrier byte limit".into()));
        }
        #[cfg(any(test, feature = "longitudinal-counting"))]
        crate::bench_support::longitudinal::record_carrier_open();
        let bytes = super::audit::read(&file, length)?;
        #[cfg(any(test, feature = "longitudinal-counting"))]
        crate::bench_support::longitudinal::record_carrier_bytes(bytes.len());
        Ok(bytes)
    }
    pub(in crate::session) fn identity_document(&self) -> Result<serde_json::Value> {
        super::require_quiescent_data()?;
        let before = self.observe()?;
        let repository = native::root_snapshot(&self.repository_root)?.canonical;
        let basename = |path: &std::path::Path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        };
        let current = basename(&repository).unwrap_or_else(|| "repository".into());
        let label = self
            .common
            .parent()
            .and_then(basename)
            .unwrap_or_else(|| current.clone());
        let identity = crate::session::StoreIdentity {
            store_identity: crate::session::store::resolution::opaque_path_identity(
                "store",
                &self.store,
            )?,
            context_identity: crate::session::store::resolution::opaque_path_identity(
                "context",
                &repository,
            )?,
            repository: label.clone(),
            worktree: (current != label).then_some(current),
            placement: crate::session::StorePlacement {
                tier: "clone",
                label: "clone store",
            },
            family: None,
        };
        let mut document = serde_json::to_value(identity)?;
        document["schema"] = "pointbreak.inspect-identity".into();
        if self.observe()? != before {
            return Err(ShoreError::Message(
                "owned Inspector identity changed".into(),
            ));
        }
        Ok(document)
    }
    pub(in crate::session) fn trust_set(&self) -> Result<crate::session::TrustSet> {
        super::require_quiescent_data()?;
        let repository = native::root_snapshot(&self.repository_root)?.canonical;
        let path = crate::paths::RepositoryPaths::from_worktree_root(repository).allowed_signers();
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(crate::session::TrustSet::default());
            }
            Err(error) => return Err(ShoreError::Message(error.to_string())),
            Ok(_) => {}
        }
        let bytes = super::data_file_bytes(&path, 1_048_576)?;
        match serde_json::from_slice(&bytes)
            .map_err(ShoreError::from)
            .and_then(crate::session::signing::event_signature_trust_set)
        {
            Ok(trust) => Ok(trust),
            Err(error) => {
                eprintln!("warning: ignoring {}: {error}", path.display());
                Ok(crate::session::TrustSet::default())
            }
        }
    }
    pub(in crate::session) fn backend(
        &self,
    ) -> Result<crate::session::store::backend::StoreBackend> {
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
        Ok(resolved.backend().clone())
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
