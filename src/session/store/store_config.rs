//! Store-mode configuration: the worktree-local opt-out for the sensitive,
//! throwaway case. A committed `.pointbreak/store.json` and a git-excluded
//! `.pointbreak/store.local.json` override compose git-config style — the exact
//! `delegates.json` / `delegates.local.json` precedent ([`with_local_override`]).
//!
//! The merge **precedence** mirrors delegates (local wins; both absent →
//! `StoreMode::default()`), but the failure posture deliberately diverges: a
//! malformed or unsupported-version config is a **hard error**, never the
//! advisory warn-and-ignore the delegates merge uses. The mode decides *where*
//! sensitive bytes land, so a silent fallback to the shared default would be a
//! privacy regression. Every such error is actionable — it names the offending
//! file, the valid modes, and the command that rewrites it.
//!
//! [`with_local_override`]: crate::session::identity::DelegationMap::with_local_override

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, ShoreError};
use crate::paths::{CommonDirPaths, RepositoryPaths};

const STORE_CONFIG_SCHEMA: &str = "shore.store-config";
const STORE_CONFIG_VERSION: u32 = 1;

/// The common-dir family-binding document schema/version and filename. This binding
/// lives in the git common dir (`<git-common-dir>/pointbreak.link.json`), which is shared
/// by every worktree of one physical clone and sits inside `.git/`, so it is never
/// tracked and never pushed — the opt-in can never arrive via a pulled commit.
const STORE_LINK_SCHEMA: &str = "shore.store-link";
const STORE_LINK_VERSION: u32 = 1;

/// Where the resolved review store for a worktree lives. The opt-out the topology
/// collapse keeps for the sensitive-throwaway case: `Ephemeral` pins a worktree's
/// data to the discardable worktree-local `.pointbreak/data`; `Shared` (the default)
/// lets the resolver place the store per its normal policy. This is a single bit
/// consulted by the resolver — it carries no store identity.
// `pub` (not `pub(crate)`): the binary/CLI crate names this type — it appears in
// the public `..._for_repo` wrapper signatures re-exported from `session::mod`,
// and a crate-internal type cannot be re-exported publicly.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StoreMode {
    /// The resolver places the store per its normal policy (the default).
    #[default]
    Shared,
    /// Pin the store worktree-local and discardable.
    Ephemeral,
}

/// The persisted store-config document. Modeled on `StoreManifest` (schema +
/// version + body) so an unsupported schema/version is a loud error, never a
/// silent misread.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoreConfig {
    schema: String,
    version: u32,
    mode: StoreMode,
    /// Frozen schema fields retained so stale binding-bearing config is rejected
    /// explicitly. Runtime binding authority lives only in the Git common dir.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    family_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clone_ref: Option<String>,
}

impl StoreConfig {
    fn new(mode: StoreMode) -> Self {
        Self {
            schema: STORE_CONFIG_SCHEMA.to_owned(),
            version: STORE_CONFIG_VERSION,
            mode,
            family_ref: None,
            clone_ref: None,
        }
    }

    fn validate_schema_version(&self, path: &Path) -> Result<()> {
        if self.schema == STORE_CONFIG_SCHEMA && self.version == STORE_CONFIG_VERSION {
            return Ok(());
        }
        // Name the offending file, like the malformed branch: with both a
        // committed and a local config possible, the user must know which file to
        // rewrite (the actionable-error contract — never a path-free message).
        Err(ShoreError::Message(format!(
            "store config {} has unsupported schema/version {} v{} (expected {} v{}); \
             rewrite it with `pointbreak store mode shared` or `pointbreak store mode ephemeral`",
            path.display(),
            self.schema,
            self.version,
            STORE_CONFIG_SCHEMA,
            STORE_CONFIG_VERSION
        )))
    }
}

/// Resolve the effective store mode under `<worktree-root>/.pointbreak/`. Two files
/// compose, git-config style: the committed `.pointbreak/store.json` and a
/// locally-excluded `.pointbreak/store.local.json` override; the local file's `mode`
/// fully replaces the committed `mode` (mirroring
/// `DelegationMap::with_local_override`). When **neither** file exists, returns
/// `StoreMode::default()` (`Shared`) — zero-setup stores see zero change. A
/// malformed or unsupported-version file is a hard error (unlike the advisory
/// delegates merge: the mode gates where bytes land, so a misread must never
/// silently fall back).
pub(crate) fn resolve_store_mode(worktree_root: &Path) -> Result<StoreMode> {
    let paths = RepositoryPaths::from_worktree_root(worktree_root);
    let committed = load_store_config(&paths.store_config())?;
    let local = load_store_config(&paths.store_config_local())?;
    // Local wins; otherwise committed; otherwise the default.
    Ok(local
        .or(committed)
        .map(|config| config.mode)
        .unwrap_or_default())
}

/// A resolved local-only family binding: this clone is promoted to the user-level
/// family tier. Read from the common-dir `pointbreak.link.json`, shared by every
/// worktree of one physical clone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FamilyBinding {
    pub family_ref: String,
    pub clone_ref: String,
}

/// The persisted common-dir family binding. Distinct from `StoreConfig`: it carries
/// no `mode` — the mode stays per-worktree in `.pointbreak/store.local.json`, the binding
/// is per-physical-clone here. `familyRef`/`cloneRef` are `Option` so a half-written
/// document is caught explicitly (serde has no `deny_unknown_fields`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommonDirBinding {
    schema: String,
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    family_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clone_ref: Option<String>,
}

/// The common-dir binding file path: `<common-dir>/pointbreak.link.json`. The single site
/// that composes this path.
pub(crate) fn common_dir_binding_path(common_dir: &Path) -> PathBuf {
    CommonDirPaths::from_common_dir(common_dir).binding()
}

/// Read `<common-dir>/pointbreak.link.json`. Absent → `None`; malformed, an unsupported
/// schema/version, or a half-binding (one of `familyRef`/`cloneRef` without the
/// other) → a hard, actionable error naming the file.
pub(crate) fn read_common_dir_binding(common_dir: &Path) -> Result<Option<FamilyBinding>> {
    let path = common_dir_binding_path(common_dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ShoreError::Message(format!(
                "read common-dir store binding {}: {error}",
                path.display()
            )));
        }
    };
    let doc: CommonDirBinding = serde_json::from_slice(&bytes).map_err(|error| {
        ShoreError::Message(format!(
            "common-dir store binding {} is malformed: {error}",
            path.display()
        ))
    })?;
    if doc.schema != STORE_LINK_SCHEMA || doc.version != STORE_LINK_VERSION {
        return Err(ShoreError::Message(format!(
            "common-dir store binding {} has unsupported schema/version {} v{} (expected {} v{}); \
             re-run `pointbreak store link <slug>` to rewrite it, or `pointbreak store unlink` to clear it",
            path.display(),
            doc.schema,
            doc.version,
            STORE_LINK_SCHEMA,
            STORE_LINK_VERSION
        )));
    }
    match (doc.family_ref, doc.clone_ref) {
        (Some(family_ref), Some(clone_ref)) => Ok(Some(FamilyBinding {
            family_ref,
            clone_ref,
        })),
        (None, None) => Ok(None),
        _ => Err(ShoreError::Message(format!(
            "common-dir store binding {} carries only one of familyRef/cloneRef; a family binding \
             needs both. Re-run `pointbreak store link <slug>`, or `pointbreak store unlink` to clear it",
            path.display(),
        ))),
    }
}

/// Write `<common-dir>/pointbreak.link.json`, pretty-printed with a trailing newline. The
/// common dir is always `.git`, which always exists, so no `create_dir_all` is needed.
pub(crate) fn write_common_dir_binding(
    common_dir: &Path,
    family_ref: &str,
    clone_ref: &str,
) -> Result<()> {
    let doc = CommonDirBinding {
        schema: STORE_LINK_SCHEMA.to_owned(),
        version: STORE_LINK_VERSION,
        family_ref: Some(family_ref.to_owned()),
        clone_ref: Some(clone_ref.to_owned()),
    };
    let mut bytes = serde_json::to_vec_pretty(&doc)?;
    bytes.push(b'\n');
    let path = common_dir_binding_path(common_dir);
    std::fs::write(&path, &bytes)
        .map_err(|error| ShoreError::Message(format!("write {}: {error}", path.display())))
}

/// Remove `<common-dir>/pointbreak.link.json`. Absent → a clean no-op.
pub(crate) fn remove_common_dir_binding(common_dir: &Path) -> Result<()> {
    let path = common_dir_binding_path(common_dir);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ShoreError::Message(format!(
            "remove {}: {error}",
            path.display()
        ))),
    }
}

/// Resolve the family binding for `worktree_root`. `Ok(None)` when no binding is
/// present. The common-dir `pointbreak.link.json`, shared by every worktree of one
/// physical clone, is the only binding authority. Neither committed nor local
/// store config may carry binding fields.
pub(crate) fn resolve_family_binding(worktree_root: &Path) -> Result<Option<FamilyBinding>> {
    // The committed document may never carry a binding. serde cannot reject it
    // (no deny_unknown_fields), so check the loaded config explicitly. This guard
    // fires before any git access, so it also holds on a non-git path.
    let paths = RepositoryPaths::from_worktree_root(worktree_root);
    let committed_path = paths.store_config();
    if let Some(committed) = load_store_config(&committed_path)?
        && (committed.family_ref.is_some() || committed.clone_ref.is_some())
    {
        return Err(ShoreError::Message(format!(
            "committed store config {} carries a family binding (familyRef/cloneRef), but the \
             user-level family tier is opt-in per clone and must never be committed. Remove those \
             fields and run `pointbreak store link <slug>` locally instead.",
            committed_path.display(),
        )));
    }

    let local_path = paths.store_config_local();
    if let Some(local) = load_store_config(&local_path)?
        && (local.family_ref.is_some() || local.clone_ref.is_some())
    {
        return Err(ShoreError::Message(format!(
            "local store config {} carries familyRef/cloneRef, but clone bindings are stored only \
             in the Git common directory. Remove those fields and run `pointbreak store link <slug>`.",
            local_path.display(),
        )));
    }

    let common = CommonDirPaths::resolve(worktree_root)?;
    read_common_dir_binding(common.common_dir())
}

/// Load and validate a store-config file if present; absent → `None`.
fn load_store_config(path: &Path) -> Result<Option<StoreConfig>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ShoreError::Message(format!(
                "read store config {}: {error}",
                path.display()
            )));
        }
    };
    // A malformed config is a HARD error (privacy: never silently fall back to
    // Shared and land bytes in the shared store). The message must be ACTIONABLE
    // — name the file, the parse problem, the valid `mode` values, and the
    // command that rewrites it.
    let config: StoreConfig = serde_json::from_slice(&bytes).map_err(|error| {
        ShoreError::Message(format!(
            "store config {} is malformed: {error}; \
             expected a JSON document with a \"mode\" of \"shared\" or \"ephemeral\" \
             (e.g. run `pointbreak store mode shared` to rewrite it)",
            path.display()
        ))
    })?;
    config.validate_schema_version(path)?;
    Ok(Some(config))
}

/// Persist the committed `.pointbreak/store.json` for `worktree_root` with `mode`.
/// Pretty-printed with a trailing newline, like `write_delegates`, so a committed
/// config diffs cleanly. The CLI is the only caller; resolution never writes.
pub(crate) fn write_store_config(worktree_root: &Path, mode: StoreMode) -> Result<()> {
    let paths = RepositoryPaths::from_worktree_root(worktree_root);
    write_store_config_document(&paths.store_config(), &StoreConfig::new(mode))
}

/// Persist a store-config document to `path`, pretty-printed with a trailing
/// newline (so a committed config diffs cleanly), creating the parent as needed.
fn write_store_config_document(path: &Path, config: &StoreConfig) -> Result<()> {
    crate::paths::require_plain_repository_control_write(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            ShoreError::Message(format!("create {}: {error}", parent.display()))
        })?;
    }
    let mut bytes = serde_json::to_vec_pretty(config)?;
    bytes.push(b'\n');
    std::fs::write(path, &bytes)
        .map_err(|error| ShoreError::Message(format!("write {}: {error}", path.display())))
}

/// Which layer the effective store mode was sourced from, so a reporting command
/// can explain *why* a worktree resolves the mode it does without leaking a path.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StoreModeSource {
    /// Neither config file is present; the built-in default applies.
    Default,
    /// The committed `.pointbreak/store.json` supplied the mode.
    Committed,
    /// The git-excluded `.pointbreak/store.local.json` override supplied the mode.
    Local,
}

/// The effective store mode for a worktree together with the layer it came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoreModeOutcome {
    pub mode: StoreMode,
    pub source: StoreModeSource,
}

/// Resolve the effective store mode for `repo` (the worktree root or any path
/// inside it) and report which layer it came from. The library entry point the
/// `store mode show` CLI consumes — it keeps the worktree-root resolution and
/// source classification on the library side of the boundary so the binary crate
/// never names the crate-internal config helpers.
pub fn resolve_store_mode_for_repo(repo: &Path) -> Result<StoreModeOutcome> {
    let paths = RepositoryPaths::resolve(repo)?;
    // Validate + resolve first, so a malformed/unsupported file errors before we
    // attribute a source; then classify by presence using the same precedence as
    // `resolve_store_mode` (local wins, else committed, else default).
    let mode = resolve_store_mode(paths.worktree_root())?;
    let source = if paths.store_config_local().exists() {
        StoreModeSource::Local
    } else if paths.store_config().exists() {
        StoreModeSource::Committed
    } else {
        StoreModeSource::Default
    };
    Ok(StoreModeOutcome { mode, source })
}

/// Persist `mode` to the committed `.pointbreak/store.json` for `repo` (the worktree
/// root or any path inside it). The library entry point the `store mode
/// shared|ephemeral` CLI consumes. Opting into `Ephemeral` also ensures the
/// committed `.pointbreak/.gitignore`, so the soon-to-exist worktree-local
/// `.pointbreak/data/` store is covered before its first write; the committed
/// `store.json` itself is tracked and never excluded.
pub fn set_store_mode_for_repo(repo: &Path, mode: StoreMode) -> Result<()> {
    let paths = RepositoryPaths::resolve(repo)?;
    if mode == StoreMode::Ephemeral {
        crate::session::store::store_init::ensure_pointbreak_gitignore(paths.worktree_root())?;
    }
    write_store_config(paths.worktree_root(), mode)
}

/// Promote `repo`'s clone into the user-level family tier by writing the
/// `familyRef`/`cloneRef` binding into the git common dir (`<common-dir>/pointbreak.link.json`)
/// — shared by every worktree of this physical clone, and inside `.git/` so it is
/// never tracked or pulled. The slug is validated first. The committed
/// `.pointbreak/store.json` is never touched. Called by the `link` workflow.
///
/// A local `mode: ephemeral` pin is neutralized to the shared default so it does not
/// shadow the binding. A family binding and an ephemeral pin are contradictory
/// resolution outcomes: `resolve_store` gives Ephemeral precedence over the user-level
/// arm, so a preserved pin would leave the binding inert and `store status` still
/// reporting ephemeral after a successful link. The ephemeral gate already forced an
/// explicit `--include-ephemeral` override to reach this write. When the effective
/// mode is already shared, no local file is written.
pub(crate) fn set_family_binding_for_repo(repo: &Path, slug: &str, clone_ref: &str) -> Result<()> {
    crate::session::store::user_level::validate_family_slug(slug)?;
    let common = CommonDirPaths::resolve(repo)?;
    write_common_dir_binding(common.common_dir(), slug, clone_ref)?;

    let paths = RepositoryPaths::resolve(repo)?;
    if resolve_store_mode(paths.worktree_root())? == StoreMode::Ephemeral {
        // The local file (covered by the `*.local.json` gitignore spec) needs the
        // committed `.pointbreak/.gitignore` before its first write — mirroring
        // `set_store_mode_for_repo`'s gitignore step.
        crate::session::store::store_init::ensure_pointbreak_gitignore(paths.worktree_root())?;
        write_store_config_document(
            &paths.store_config_local(),
            &StoreConfig::new(StoreMode::default()),
        )?;
    }
    Ok(())
}

/// Detach `repo`'s clone from the family tier. Removes the common-dir binding (the
/// current write target) and also clears a legacy per-worktree binding if one predates
/// the heal: a default-mode local file carrying only the binding is removed; a
/// non-default `mode` is preserved (the file stays, binding-free). A no-op when nothing
/// is bound. Called by the `unlink` workflow.
pub(crate) fn clear_family_binding_for_repo(repo: &Path) -> Result<()> {
    let common = CommonDirPaths::resolve(repo)?;
    remove_common_dir_binding(common.common_dir())?;

    let paths = RepositoryPaths::resolve(repo)?;
    let local_path = paths.store_config_local();
    let Some(existing) = load_store_config(&local_path)? else {
        return Ok(());
    };
    if existing.mode == StoreMode::default() {
        std::fs::remove_file(&local_path).map_err(|error| {
            ShoreError::Message(format!("remove {}: {error}", local_path.display()))
        })?;
        return Ok(());
    }
    write_store_config_document(&local_path, &StoreConfig::new(existing.mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &std::path::Path, rel: &str, contents: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn git_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().expect("create temp git repository directory");
        let output = std::process::Command::new("git")
            .arg("init")
            .current_dir(repo.path())
            .output()
            .expect("run git init");
        assert!(output.status.success(), "git init failed");
        repo
    }

    #[test]
    fn store_mode_defaults_to_shared() {
        assert_eq!(StoreMode::default(), StoreMode::Shared);
    }

    #[test]
    fn absent_both_files_resolves_to_shared() {
        // Zero-setup stores see zero change: no config means the default mode.
        let root = tempfile::tempdir().unwrap();
        assert_eq!(resolve_store_mode(root.path()).unwrap(), StoreMode::Shared);
    }

    #[test]
    fn committed_config_round_trips_through_the_reader() {
        // A persisted `.pointbreak/store.json` reads back to the mode it stored.
        let root = tempfile::tempdir().unwrap();
        write_store_config(root.path(), StoreMode::Ephemeral).unwrap();
        assert!(root.path().join(".pointbreak/store.json").is_file());
        assert_eq!(
            resolve_store_mode(root.path()).unwrap(),
            StoreMode::Ephemeral
        );
    }

    #[test]
    fn camel_case_mode_strings_are_used_on_the_wire() {
        // The serialized document spells the variants in camelCase.
        let root = tempfile::tempdir().unwrap();
        write_store_config(root.path(), StoreMode::Ephemeral).unwrap();
        let raw = std::fs::read_to_string(root.path().join(".pointbreak/store.json")).unwrap();
        assert!(raw.contains("\"mode\": \"ephemeral\""), "got: {raw}");
        assert!(
            raw.contains("\"schema\": \"shore.store-config\""),
            "got: {raw}"
        );
    }

    #[test]
    fn local_override_wins_over_committed() {
        // committed = shared, local = ephemeral -> effective ephemeral (local wins,
        // mirroring DelegationMap::with_local_override).
        let root = tempfile::tempdir().unwrap();
        write(root.path(), ".pointbreak/store.json", SHARED_DOC);
        write(root.path(), ".pointbreak/store.local.json", EPHEMERAL_DOC);
        assert_eq!(
            resolve_store_mode(root.path()).unwrap(),
            StoreMode::Ephemeral
        );
    }

    #[test]
    fn local_alone_is_used_when_committed_absent() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), ".pointbreak/store.local.json", EPHEMERAL_DOC);
        assert_eq!(
            resolve_store_mode(root.path()).unwrap(),
            StoreMode::Ephemeral
        );
    }

    #[test]
    fn committed_alone_is_used_when_local_absent() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), ".pointbreak/store.json", EPHEMERAL_DOC);
        assert_eq!(
            resolve_store_mode(root.path()).unwrap(),
            StoreMode::Ephemeral
        );
    }

    #[test]
    fn unsupported_schema_version_is_rejected_with_actionable_message() {
        // Mirror manifest.rs validate_schema_version: a wrong schema/version errors.
        // The hard error MUST be actionable so users know how to fix it — naming
        // the offending file (with a committed and a local config both possible,
        // a path-free message can't say which to rewrite) and the fix command.
        let root = tempfile::tempdir().unwrap();
        write(
            root.path(),
            ".pointbreak/store.local.json",
            r#"{"schema":"shore.store-config","version":999,"mode":"shared"}"#,
        );
        let err = resolve_store_mode(root.path()).unwrap_err().to_string();
        assert!(err.contains("store mode"), "names the fix command: {err}");
        assert!(
            err.contains("store.local.json"),
            "names the offending file: {err}"
        );
    }

    #[test]
    fn malformed_config_is_rejected_with_actionable_message() {
        // Not valid JSON / wrong shape → hard error naming the file + the fix
        // (never a silent fallback to Shared — privacy).
        let root = tempfile::tempdir().unwrap();
        write(root.path(), ".pointbreak/store.json", "{ not json");
        let err = resolve_store_mode(root.path()).unwrap_err().to_string();
        assert!(err.contains("store.json"), "names the file: {err}");
        assert!(
            err.contains("shared") && err.contains("ephemeral"),
            "names the valid modes: {err}"
        );
    }

    const LOCAL_BINDING_DOC: &str = r#"{"schema":"shore.store-config","version":1,"mode":"shared","familyRef":"acme-web","cloneRef":"0123abcd4567ef89"}"#;

    #[test]
    fn a_full_local_binding_is_rejected() {
        let root = git_repo();
        write(
            root.path(),
            ".pointbreak/store.local.json",
            LOCAL_BINDING_DOC,
        );
        let error = resolve_family_binding(root.path())
            .expect_err("the common-dir document is the only binding authority");
        assert!(error.to_string().contains("Git common directory"));
    }

    #[test]
    fn a_committed_binding_is_a_hard_error() {
        // INV-1: the committed store.json must never carry a binding — a pulled
        // commit could otherwise silently promote every clone.
        let root = tempfile::tempdir().unwrap();
        write(root.path(), ".pointbreak/store.json", LOCAL_BINDING_DOC);
        let err = resolve_family_binding(root.path())
            .expect_err("a committed binding is rejected")
            .to_string();
        assert!(
            err.contains("store.json"),
            "names the committed file: {err}"
        );
        assert!(
            err.contains("store link") || err.contains("opt-in"),
            "explains the local-only opt-in: {err}"
        );
    }

    #[test]
    fn a_half_binding_in_the_local_file_is_a_hard_error() {
        let root = git_repo();
        write(
            root.path(),
            ".pointbreak/store.local.json",
            r#"{"schema":"shore.store-config","version":1,"mode":"shared","familyRef":"acme-web"}"#,
        );
        let err = resolve_family_binding(root.path())
            .expect_err("familyRef without cloneRef is rejected")
            .to_string();
        assert!(
            err.contains("store.local.json"),
            "names the local file: {err}"
        );
        assert!(
            err.contains("both") || err.contains("cloneRef"),
            "explains both fields are required: {err}"
        );
    }

    #[test]
    fn no_binding_resolves_to_none() {
        // Absent local file, or a mode-only local file, is not a binding.
        let root = git_repo();
        assert!(resolve_family_binding(root.path()).unwrap().is_none());
        write(root.path(), ".pointbreak/store.local.json", EPHEMERAL_DOC);
        assert!(resolve_family_binding(root.path()).unwrap().is_none());
    }

    #[test]
    fn a_mode_only_local_file_still_resolves_its_mode_with_a_binding_present_field_absent() {
        // Regression: adding the optional binding fields must not change how a
        // mode-only local file resolves its mode.
        let root = git_repo();
        write(root.path(), ".pointbreak/store.local.json", EPHEMERAL_DOC);
        assert_eq!(
            resolve_store_mode(root.path()).unwrap(),
            StoreMode::Ephemeral
        );
        assert!(resolve_family_binding(root.path()).unwrap().is_none());
    }

    #[test]
    fn common_dir_binding_round_trips() {
        let common = tempfile::tempdir().unwrap();
        assert!(read_common_dir_binding(common.path()).unwrap().is_none());

        write_common_dir_binding(common.path(), "fam", "abcdef0123456789").unwrap();
        assert!(common.path().join("pointbreak.link.json").is_file());

        let binding = read_common_dir_binding(common.path())
            .unwrap()
            .expect("bound");
        assert_eq!(binding.family_ref, "fam");
        assert_eq!(binding.clone_ref, "abcdef0123456789");
    }

    #[test]
    fn common_dir_binding_document_is_camel_case_store_link_schema() {
        let common = tempfile::tempdir().unwrap();
        write_common_dir_binding(common.path(), "fam", "abcdef0123456789").unwrap();
        let raw = std::fs::read_to_string(common.path().join("pointbreak.link.json")).unwrap();
        assert!(
            raw.contains("\"schema\": \"shore.store-link\""),
            "got: {raw}"
        );
        assert!(raw.contains("\"familyRef\": \"fam\""), "got: {raw}");
        assert!(
            raw.contains("\"cloneRef\": \"abcdef0123456789\""),
            "got: {raw}"
        );
        assert!(
            raw.ends_with('\n'),
            "trailing newline like the other writers"
        );
    }

    #[test]
    fn common_dir_binding_half_document_is_a_hard_error() {
        let common = tempfile::tempdir().unwrap();
        std::fs::write(
            common.path().join("pointbreak.link.json"),
            r#"{"schema":"shore.store-link","version":1,"familyRef":"fam"}"#,
        )
        .unwrap();
        let error =
            read_common_dir_binding(common.path()).expect_err("a half binding is a hard error");
        assert!(
            error.to_string().contains("pointbreak.link.json"),
            "names the file"
        );
    }

    #[test]
    fn common_dir_binding_wrong_schema_version_is_a_hard_error() {
        let common = tempfile::tempdir().unwrap();
        std::fs::write(
            common.path().join("pointbreak.link.json"),
            r#"{"schema":"shore.store-link","version":999,"familyRef":"f","cloneRef":"c"}"#,
        )
        .unwrap();
        assert!(read_common_dir_binding(common.path()).is_err());
    }

    #[test]
    fn remove_common_dir_binding_is_a_clean_no_op_when_absent() {
        let common = tempfile::tempdir().unwrap();
        remove_common_dir_binding(common.path()).unwrap();
        write_common_dir_binding(common.path(), "fam", "abcdef0123456789").unwrap();
        remove_common_dir_binding(common.path()).unwrap();
        assert!(read_common_dir_binding(common.path()).unwrap().is_none());
    }

    #[test]
    fn a_common_dir_binding_resolves() {
        let repo = git_repo();
        let common = crate::git::git_common_dir(repo.path()).unwrap();
        write_common_dir_binding(&common, "fam", "abcdef0123456789").unwrap();

        let binding = resolve_family_binding(repo.path()).unwrap().expect("bound");
        assert_eq!(binding.family_ref, "fam");
        assert_eq!(binding.clone_ref, "abcdef0123456789");
    }

    #[test]
    fn per_worktree_binding_does_not_redirect_resolution() {
        let repo = git_repo();
        write(
            repo.path(),
            ".pointbreak/store.local.json",
            LOCAL_BINDING_DOC,
        );

        assert!(resolve_family_binding(repo.path()).is_err());
    }

    #[test]
    fn per_worktree_binding_fields_are_rejected_even_when_common_binding_exists() {
        let repo = git_repo();
        let common = crate::git::git_common_dir(repo.path()).unwrap();
        write_common_dir_binding(&common, "new", "1111111111111111").unwrap();
        write(
            repo.path(),
            ".pointbreak/store.local.json",
            LOCAL_BINDING_DOC,
        );

        assert!(resolve_family_binding(repo.path()).is_err());
    }

    #[test]
    fn set_family_binding_clears_a_local_ephemeral_pin_so_the_binding_resolves() {
        let repo = git_repo();
        // A local ephemeral pin must NOT survive the binding write: an ephemeral pin
        // and a family binding are contradictory (resolve_store gives ephemeral
        // precedence over the user-level arm), so a preserved pin would leave the
        // link inert.
        write(repo.path(), ".pointbreak/store.local.json", EPHEMERAL_DOC);
        set_family_binding_for_repo(repo.path(), "acme-web", "0123abcd4567ef89").unwrap();

        let binding = resolve_family_binding(repo.path())
            .unwrap()
            .expect("binding written");
        assert_eq!(binding.family_ref, "acme-web");
        assert_eq!(binding.clone_ref, "0123abcd4567ef89");
        // The ephemeral pin is cleared to the shared default so the binding takes
        // effect (the clone will resolve the family store, not `.pointbreak/data`).
        assert_eq!(resolve_store_mode(repo.path()).unwrap(), StoreMode::Shared);
    }

    #[test]
    fn set_family_binding_leaves_the_committed_file_untouched() {
        let repo = git_repo();
        write(repo.path(), ".pointbreak/store.json", SHARED_DOC);
        set_family_binding_for_repo(repo.path(), "acme-web", "0123abcd4567ef89").unwrap();
        // The committed document is byte-for-byte unchanged (no binding leaks in).
        let committed =
            std::fs::read_to_string(repo.path().join(".pointbreak/store.json")).unwrap();
        assert_eq!(committed, SHARED_DOC);
        assert!(resolve_family_binding(repo.path()).unwrap().is_some());
    }

    #[test]
    fn clear_family_binding_removes_the_binding_and_preserves_a_non_default_mode() {
        let repo = git_repo();
        // Seed a local file carrying BOTH an ephemeral pin and a binding directly
        // (not via `set`, which now clears the pin), to prove `clear` drops only the
        // binding and preserves a non-default mode.
        write(
            repo.path(),
            ".pointbreak/store.local.json",
            r#"{"schema":"shore.store-config","version":1,"mode":"ephemeral","familyRef":"acme-web","cloneRef":"0123abcd4567ef89"}"#,
        );
        clear_family_binding_for_repo(repo.path()).unwrap();

        assert!(resolve_family_binding(repo.path()).unwrap().is_none());
        // The ephemeral mode survives; the local file stays, binding-free.
        assert_eq!(
            resolve_store_mode(repo.path()).unwrap(),
            StoreMode::Ephemeral
        );
        assert!(repo.path().join(".pointbreak/store.local.json").is_file());
    }

    #[test]
    fn set_family_binding_writes_the_common_dir_doc_not_the_local_file() {
        let repo = git_repo();
        set_family_binding_for_repo(repo.path(), "fam", "abcdef0123456789").unwrap();

        // The binding lives in the common dir, shared by every worktree.
        let common = crate::git::git_common_dir(repo.path()).unwrap();
        assert!(
            common.join("pointbreak.link.json").is_file(),
            "binding in the common dir"
        );
        // A fresh (non-ephemeral) link writes NO per-worktree local file.
        assert!(
            !repo.path().join(".pointbreak/store.local.json").exists(),
            "no spurious per-worktree binding file"
        );
        assert_eq!(
            resolve_family_binding(repo.path())
                .unwrap()
                .unwrap()
                .family_ref,
            "fam"
        );
    }

    #[test]
    fn clear_family_binding_removes_the_common_dir_doc() {
        let repo = git_repo();
        set_family_binding_for_repo(repo.path(), "fam", "abcdef0123456789").unwrap();
        let common = crate::git::git_common_dir(repo.path()).unwrap();
        assert!(common.join("pointbreak.link.json").is_file());

        clear_family_binding_for_repo(repo.path()).unwrap();

        assert!(
            !common.join("pointbreak.link.json").exists(),
            "common-dir binding removed"
        );
        assert!(resolve_family_binding(repo.path()).unwrap().is_none());
    }

    #[test]
    fn clear_family_binding_also_clears_a_legacy_per_worktree_binding() {
        // A binding that predates the heal lives in the local file; unlink must clear it
        // (this exercises the default-mode-local-file removal branch of `clear`).
        let repo = git_repo();
        write(
            repo.path(),
            ".pointbreak/store.local.json",
            LOCAL_BINDING_DOC,
        );

        clear_family_binding_for_repo(repo.path()).unwrap();

        assert!(resolve_family_binding(repo.path()).unwrap().is_none());
    }

    #[test]
    fn set_family_binding_validates_the_slug_before_writing() {
        let repo = git_repo();
        let err = set_family_binding_for_repo(repo.path(), "Bad Slug", "0123abcd4567ef89")
            .expect_err("an invalid slug is rejected before any write");
        assert!(
            err.to_string().contains("slug"),
            "names the slug problem: {err}"
        );
        assert!(
            !repo.path().join(".pointbreak/store.local.json").exists(),
            "no local file is written when the slug is invalid"
        );
    }

    const SHARED_DOC: &str = r#"{"schema":"shore.store-config","version":1,"mode":"shared"}"#;
    const EPHEMERAL_DOC: &str = r#"{"schema":"shore.store-config","version":1,"mode":"ephemeral"}"#;

    #[test]
    fn naming_cutover_store_config_and_binding_bytes_are_frozen() {
        let repo = git_repo();
        write_store_config(repo.path(), StoreMode::Ephemeral).unwrap();
        assert_eq!(
            std::fs::read(
                RepositoryPaths::resolve(repo.path())
                    .unwrap()
                    .store_config()
            )
            .unwrap(),
            crate::test_fixtures::naming_cutover_bytes("topology/repo/.shore/store.json")
        );

        let common = tempfile::tempdir().unwrap();
        write_common_dir_binding(common.path(), "acme-web", "0123abcd4567ef89").unwrap();
        assert_eq!(
            std::fs::read(CommonDirPaths::from_common_dir(common.path()).binding()).unwrap(),
            crate::test_fixtures::naming_cutover_bytes("topology/git-common/shore.link.json")
        );
    }
    #[cfg(unix)]
    #[test]
    fn control_writer_family_set_reports_published_binding_when_local_link_is_refused() {
        let repo = git_repo();
        let paths = RepositoryPaths::resolve(repo.path()).unwrap();
        let common = CommonDirPaths::resolve(repo.path()).unwrap();
        let external = tempfile::tempdir().unwrap();
        let sentinel = external.path().join("local.json");
        std::fs::write(&sentinel, EPHEMERAL_DOC).unwrap();
        std::fs::create_dir(paths.config_dir()).unwrap();
        std::os::unix::fs::symlink(&sentinel, paths.store_config_local()).unwrap();
        let result = set_family_binding_for_repo(repo.path(), "acme-web", "0123abcd4567ef89");
        let binding = load_common_dir_binding(common.common_dir())
            .unwrap()
            .unwrap();
        assert_eq!(binding.family_ref, "acme-web");
        assert_eq!(binding.clone_ref, "0123abcd4567ef89");
        assert_eq!(std::fs::read(&sentinel).unwrap(), EPHEMERAL_DOC.as_bytes());
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("common-dir binding was already written; local store config refused:"),
            "{error}"
        );
        assert!(error.contains("link or reparse point"), "{error}");
        assert!(
            error.contains(&paths.store_config_local().display().to_string()),
            "{error}"
        );
        assert!(
            std::fs::symlink_metadata(paths.store_config_local())
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn control_writer_family_clear_reports_completed_removal_and_preserves_local_links() {
        // Ancestor deletion, leaf rewrite, leaf deletion, and an already-absent binding.
        for (ancestor, body, bound) in [
            (true, SHARED_DOC, true),
            (false, EPHEMERAL_DOC, true),
            (false, SHARED_DOC, true),
            (false, SHARED_DOC, false),
        ] {
            let repo = git_repo();
            let paths = RepositoryPaths::resolve(repo.path()).unwrap();
            let common = CommonDirPaths::resolve(repo.path()).unwrap();
            if bound {
                write_common_dir_binding(common.common_dir(), "acme-web", "0123abcd4567ef89")
                    .unwrap();
            }
            let external = tempfile::tempdir().unwrap();
            let sentinel = external.path().join("store.local.json");
            std::fs::write(&sentinel, body).unwrap();
            let link = if ancestor {
                std::os::unix::fs::symlink(external.path(), paths.config_dir()).unwrap();
                paths.config_dir().to_path_buf()
            } else {
                std::fs::create_dir(paths.config_dir()).unwrap();
                std::os::unix::fs::symlink(&sentinel, paths.store_config_local()).unwrap();
                paths.store_config_local()
            };
            let result = clear_family_binding_for_repo(repo.path());
            assert!(!common.binding().exists(), "common-dir removal completed");
            assert_eq!(
                std::fs::read(&sentinel).ok().as_deref(),
                Some(body.as_bytes())
            );
            let error = result.unwrap_err().to_string();
            assert!(
                error.contains("common-dir binding removal completed; local store config refused:"),
                "{error}"
            );
            assert!(error.contains("link or reparse point"), "{error}");
            assert!(error.contains(&link.display().to_string()), "{error}");
            assert!(
                std::fs::symlink_metadata(link)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
    }
}
