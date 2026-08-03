//! Stable and legacy filesystem vocabulary for disposable derived access.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

const STABLE_ROOT: &str = "derived";
const STABLE_WRITER_LOCK: &str = "derived.writer.lock";
const STABLE_REBUILD_LOCK: &str = "derived.rebuild.lock";
const STABLE_GENERATION_LEASE_PREFIX: &str = "derived.generation-lease-";
const STABLE_QUARANTINE_PREFIX: &str = "derived.quarantine-";
const STABLE_RETIRED_PREFIX: &str = "derived.retired-";

const LEGACY_ROOT: &str = ".pointbreak-derived";
const LEGACY_WRITER_LOCK: &str = ".pointbreak-derived.writer.lock";
const LEGACY_REBUILD_LOCK: &str = ".pointbreak-derived.rebuild.lock";
const LEGACY_GENERATION_LEASE_PREFIX: &str = ".pointbreak-derived.generation-lease-";
const LEGACY_QUARANTINE_PREFIX: &str = ".pointbreak-derived.quarantine-";
const LEGACY_RETIRED_PREFIX: &str = ".pointbreak-derived.retired-";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DerivedStorageNamespace {
    Stable,
    Legacy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DerivedStorageLayout {
    store_root: PathBuf,
    namespace: DerivedStorageNamespace,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DerivedStorageDiscovery {
    Selected(DerivedStorageLayout),
    Conflict {
        stable: DerivedStorageLayout,
        legacy: DerivedStorageLayout,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DerivedStorageTransition {
    NotNeeded,
    Deferred,
    Moved,
    Conflict,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DerivedStorageTransitionReceipt {
    pub(crate) disposition: DerivedStorageTransition,
    pub(crate) from_namespace: Option<DerivedStorageNamespace>,
    pub(crate) to_namespace: DerivedStorageNamespace,
    pub(crate) moved_artifacts: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DerivedStorageTransitionError {
    #[error("derived-storage transition I/O failed at {path}: {message}")]
    Io { path: PathBuf, message: String },
}

#[derive(Debug)]
struct TransitionMove {
    source: PathBuf,
    destination: PathBuf,
}

#[derive(Debug)]
struct TransitionFileLock {
    file: File,
}

#[derive(Debug)]
enum TransitionPreflight {
    Ready(Vec<TransitionMove>),
    Conflict,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error(
    "both stable and legacy derived-access roots exist at {stable} and {legacy}; derived state is disposable, so set POINTBREAK_DERIVED_ACCESS=off or move one root aside before retrying"
)]
pub(crate) struct DerivedStorageConflict {
    pub(crate) stable: PathBuf,
    pub(crate) legacy: PathBuf,
}

impl DerivedStorageLayout {
    pub(crate) fn discover(store_root: &Path) -> DerivedStorageDiscovery {
        let stable = Self::for_namespace(store_root, DerivedStorageNamespace::Stable);
        let legacy = Self::for_namespace(store_root, DerivedStorageNamespace::Legacy);
        match (stable.root().exists(), legacy.root().exists()) {
            (true, true) => DerivedStorageDiscovery::Conflict { stable, legacy },
            (false, true) => DerivedStorageDiscovery::Selected(legacy),
            (true, false) | (false, false) => DerivedStorageDiscovery::Selected(stable),
        }
    }

    pub(crate) fn resolve(store_root: &Path) -> Result<Self, DerivedStorageConflict> {
        match Self::discover(store_root) {
            DerivedStorageDiscovery::Selected(layout) => Ok(layout),
            DerivedStorageDiscovery::Conflict { stable, legacy } => Err(DerivedStorageConflict {
                stable: stable.root().to_path_buf(),
                legacy: legacy.root().to_path_buf(),
            }),
        }
    }

    /// Moves compatible disposable state from the legacy namespace without
    /// reading authoritative truth or replaying a generation.
    ///
    /// The transition first preflights every destination, then holds both
    /// namespace writer/rebuild locks and exclusive generation leases. Retired
    /// and quarantined siblings move first; the current root moves last as the
    /// namespace publication marker. A process interruption before that final
    /// rename still selects legacy, while an interruption after it has already
    /// published the stable root. Re-entry derives its answer from those exact
    /// paths, so no ambient marker needs repair.
    pub(crate) fn transition_legacy(
        store_root: &Path,
    ) -> Result<DerivedStorageTransitionReceipt, DerivedStorageTransitionError> {
        Self::transition_legacy_with_hook(store_root, |_| {})
    }

    fn transition_legacy_with_hook(
        store_root: &Path,
        mut after_move: impl FnMut(&Path),
    ) -> Result<DerivedStorageTransitionReceipt, DerivedStorageTransitionError> {
        let initial = transition_preflight(store_root)?;
        let TransitionPreflight::Ready(initial_moves) = initial else {
            return Ok(transition_receipt(DerivedStorageTransition::Conflict));
        };
        if initial_moves.is_empty() {
            return Ok(DerivedStorageTransitionReceipt {
                disposition: DerivedStorageTransition::NotNeeded,
                from_namespace: current_namespace(store_root)?,
                to_namespace: DerivedStorageNamespace::Stable,
                moved_artifacts: Vec::new(),
            });
        }

        let Some(_coordination_locks) = try_lock_paths(&coordination_lock_paths(store_root))?
        else {
            return Ok(transition_receipt(DerivedStorageTransition::Deferred));
        };

        // Lock acquisition is non-blocking. Re-read exact filesystem state
        // only after both namespace lock pairs are held; a contender that
        // moved first becomes an idempotent no-op, never an overwrite.
        let locked = transition_preflight(store_root)?;
        let TransitionPreflight::Ready(moves) = locked else {
            return Ok(transition_receipt(DerivedStorageTransition::Conflict));
        };
        if moves.is_empty() {
            return Ok(DerivedStorageTransitionReceipt {
                disposition: DerivedStorageTransition::NotNeeded,
                from_namespace: current_namespace(store_root)?,
                to_namespace: DerivedStorageNamespace::Stable,
                moved_artifacts: Vec::new(),
            });
        }

        let generation_ids = transition_generation_ids(store_root)?;
        let lease_paths = generation_ids
            .iter()
            .flat_map(|generation_id| {
                [
                    Self::for_namespace(store_root, DerivedStorageNamespace::Legacy)
                        .generation_lease(generation_id),
                    Self::for_namespace(store_root, DerivedStorageNamespace::Stable)
                        .generation_lease(generation_id),
                ]
            })
            .collect::<Vec<_>>();
        let Some(_generation_leases) = try_lock_paths(&lease_paths)? else {
            return Ok(transition_receipt(DerivedStorageTransition::Deferred));
        };

        let mut moved_artifacts = Vec::with_capacity(moves.len());
        for movement in moves {
            std::fs::rename(&movement.source, &movement.destination)
                .map_err(|error| transition_io_error(&movement.source, error))?;
            moved_artifacts.push(
                movement
                    .destination
                    .file_name()
                    .expect("transition destinations are store-root children")
                    .to_string_lossy()
                    .into_owned(),
            );
            after_move(&movement.destination);
        }

        Ok(DerivedStorageTransitionReceipt {
            disposition: DerivedStorageTransition::Moved,
            from_namespace: Some(DerivedStorageNamespace::Legacy),
            to_namespace: DerivedStorageNamespace::Stable,
            moved_artifacts,
        })
    }

    pub(crate) fn for_namespace(store_root: &Path, namespace: DerivedStorageNamespace) -> Self {
        Self {
            store_root: store_root.to_path_buf(),
            namespace,
        }
    }

    pub(crate) const fn namespace(&self) -> DerivedStorageNamespace {
        self.namespace
    }

    pub(crate) fn store_root(&self) -> &Path {
        &self.store_root
    }

    pub(crate) fn root(&self) -> PathBuf {
        self.store_root.join(self.root_name())
    }

    pub(crate) fn writer_lock(&self) -> PathBuf {
        self.store_root.join(self.writer_lock_name())
    }

    pub(crate) fn rebuild_lock(&self) -> PathBuf {
        self.store_root.join(self.rebuild_lock_name())
    }

    pub(crate) fn generation_lease(&self, generation_id: &str) -> PathBuf {
        self.store_root.join(format!(
            "{}{generation_id}.lock",
            self.generation_lease_prefix()
        ))
    }

    pub(crate) fn quarantine(&self, suffix: &str) -> PathBuf {
        self.store_root
            .join(format!("{}{suffix}", self.quarantine_prefix()))
    }

    pub(crate) fn retired(&self, suffix: &str) -> PathBuf {
        self.store_root
            .join(format!("{}{suffix}", self.retired_prefix()))
    }

    pub(crate) fn generation_lease_id<'a>(&self, name: &'a str) -> Option<&'a str> {
        Self::namespaces().find_map(|namespace| {
            let layout = Self::for_namespace(Path::new(""), namespace);
            name.strip_prefix(layout.generation_lease_prefix())?
                .strip_suffix(".lock")
                .filter(|id| !id.is_empty())
        })
    }

    pub(crate) fn is_disposable_root(&self, path: &Path) -> bool {
        self.layout_for_disposable_root(path).is_some()
    }

    pub(crate) fn layout_for_disposable_root(&self, path: &Path) -> Option<Self> {
        if path.parent() != Some(self.store_root()) {
            return None;
        }
        let name = path.file_name()?.to_str()?;
        Self::namespaces().find_map(|namespace| {
            let layout = Self::for_namespace(self.store_root(), namespace);
            name.strip_prefix(layout.quarantine_prefix())
                .or_else(|| name.strip_prefix(layout.retired_prefix()))
                .filter(|suffix| is_well_formed_disposable_suffix(suffix))
                .map(|_| layout)
        })
    }

    pub(crate) fn is_governed_store_entry(name: &str, is_directory: bool, is_file: bool) -> bool {
        Self::namespaces().any(|namespace| {
            let layout = Self::for_namespace(Path::new(""), namespace);
            (is_directory && name == layout.root_name())
                || (is_file && name == layout.writer_lock_name())
                || (is_file && name == layout.rebuild_lock_name())
                || (is_file
                    && name
                        .strip_prefix(layout.generation_lease_prefix())
                        .and_then(|suffix| suffix.strip_suffix(".lock"))
                        .is_some_and(|id| !id.is_empty()))
                || (is_directory
                    && name
                        .strip_prefix(layout.quarantine_prefix())
                        .is_some_and(is_well_formed_disposable_suffix))
                || (is_directory
                    && name
                        .strip_prefix(layout.retired_prefix())
                        .is_some_and(is_well_formed_disposable_suffix))
        })
    }

    fn namespaces() -> impl Iterator<Item = DerivedStorageNamespace> {
        [
            DerivedStorageNamespace::Stable,
            DerivedStorageNamespace::Legacy,
        ]
        .into_iter()
    }

    const fn root_name(&self) -> &'static str {
        match self.namespace {
            DerivedStorageNamespace::Stable => STABLE_ROOT,
            DerivedStorageNamespace::Legacy => LEGACY_ROOT,
        }
    }

    const fn writer_lock_name(&self) -> &'static str {
        match self.namespace {
            DerivedStorageNamespace::Stable => STABLE_WRITER_LOCK,
            DerivedStorageNamespace::Legacy => LEGACY_WRITER_LOCK,
        }
    }

    const fn rebuild_lock_name(&self) -> &'static str {
        match self.namespace {
            DerivedStorageNamespace::Stable => STABLE_REBUILD_LOCK,
            DerivedStorageNamespace::Legacy => LEGACY_REBUILD_LOCK,
        }
    }

    const fn generation_lease_prefix(&self) -> &'static str {
        match self.namespace {
            DerivedStorageNamespace::Stable => STABLE_GENERATION_LEASE_PREFIX,
            DerivedStorageNamespace::Legacy => LEGACY_GENERATION_LEASE_PREFIX,
        }
    }

    const fn quarantine_prefix(&self) -> &'static str {
        match self.namespace {
            DerivedStorageNamespace::Stable => STABLE_QUARANTINE_PREFIX,
            DerivedStorageNamespace::Legacy => LEGACY_QUARANTINE_PREFIX,
        }
    }

    const fn retired_prefix(&self) -> &'static str {
        match self.namespace {
            DerivedStorageNamespace::Stable => STABLE_RETIRED_PREFIX,
            DerivedStorageNamespace::Legacy => LEGACY_RETIRED_PREFIX,
        }
    }
}

impl Drop for TransitionFileLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn transition_preflight(
    store_root: &Path,
) -> Result<TransitionPreflight, DerivedStorageTransitionError> {
    let legacy = DerivedStorageLayout::for_namespace(store_root, DerivedStorageNamespace::Legacy);
    let stable = DerivedStorageLayout::for_namespace(store_root, DerivedStorageNamespace::Stable);
    let legacy_root = path_kind(&legacy.root())?;
    let stable_root = path_kind(&stable.root())?;
    if matches!(legacy_root, Some(false))
        || matches!(stable_root, Some(false))
        || (legacy_root.is_some() && stable_root.is_some())
    {
        return Ok(TransitionPreflight::Conflict);
    }

    let mut movements = Vec::new();
    for entry in read_store_entries(store_root)? {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let destination = if let Some(suffix) = name
            .strip_prefix(legacy.quarantine_prefix())
            .filter(|suffix| is_well_formed_disposable_suffix(suffix))
        {
            Some(stable.quarantine(suffix))
        } else {
            name.strip_prefix(legacy.retired_prefix())
                .filter(|suffix| is_well_formed_disposable_suffix(suffix))
                .map(|suffix| stable.retired(suffix))
        };
        let Some(destination) = destination else {
            continue;
        };
        if !entry
            .file_type()
            .map_err(|error| transition_io_error(&entry.path(), error))?
            .is_dir()
            || path_kind(&destination)?.is_some()
        {
            return Ok(TransitionPreflight::Conflict);
        }
        movements.push(TransitionMove {
            source: entry.path(),
            destination,
        });
    }
    movements.sort_by(|left, right| left.source.cmp(&right.source));

    // The current root is the namespace publication marker. Moving it last
    // means every earlier interruption still selects legacy, while a crash
    // after this final rename is already a complete stable publication.
    if legacy_root.is_some() {
        movements.push(TransitionMove {
            source: legacy.root(),
            destination: stable.root(),
        });
    }
    Ok(TransitionPreflight::Ready(movements))
}

fn current_namespace(
    store_root: &Path,
) -> Result<Option<DerivedStorageNamespace>, DerivedStorageTransitionError> {
    let stable = DerivedStorageLayout::for_namespace(store_root, DerivedStorageNamespace::Stable);
    if path_kind(&stable.root())?.is_some() {
        return Ok(Some(DerivedStorageNamespace::Stable));
    }
    let legacy = DerivedStorageLayout::for_namespace(store_root, DerivedStorageNamespace::Legacy);
    if path_kind(&legacy.root())?.is_some() {
        return Ok(Some(DerivedStorageNamespace::Legacy));
    }
    Ok(None)
}

fn coordination_lock_paths(store_root: &Path) -> Vec<PathBuf> {
    [
        DerivedStorageNamespace::Legacy,
        DerivedStorageNamespace::Stable,
    ]
    .into_iter()
    .flat_map(|namespace| {
        let layout = DerivedStorageLayout::for_namespace(store_root, namespace);
        [layout.writer_lock(), layout.rebuild_lock()]
    })
    .collect()
}

fn transition_generation_ids(
    store_root: &Path,
) -> Result<BTreeSet<String>, DerivedStorageTransitionError> {
    let reference =
        DerivedStorageLayout::for_namespace(store_root, DerivedStorageNamespace::Stable);
    let mut generation_roots = Vec::new();
    let mut generation_ids = BTreeSet::new();
    for entry in read_store_entries(store_root)? {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| transition_io_error(&path, error))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if file_type.is_file()
            && let Some(generation_id) = reference.generation_lease_id(name)
            && is_transition_generation_id(generation_id)
        {
            generation_ids.insert(generation_id.to_owned());
        }
        if file_type.is_dir()
            && (name == STABLE_ROOT
                || name == LEGACY_ROOT
                || reference.layout_for_disposable_root(&path).is_some())
        {
            generation_roots.push(path);
        }
    }

    for root in generation_roots {
        let generations = root.join("generations");
        for entry in read_directory_if_present(&generations)? {
            let path = entry.path();
            if !entry
                .file_type()
                .map_err(|error| transition_io_error(&path, error))?
                .is_dir()
            {
                continue;
            }
            let generation_id = entry.file_name().to_string_lossy().into_owned();
            if is_transition_generation_id(&generation_id) {
                generation_ids.insert(generation_id);
            }
        }
    }
    Ok(generation_ids)
}

fn try_lock_paths(
    paths: &[PathBuf],
) -> Result<Option<Vec<TransitionFileLock>>, DerivedStorageTransitionError> {
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    let mut locks = Vec::with_capacity(paths.len());
    for path in paths {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| transition_io_error(&path, error))?;
        match file.try_lock() {
            Ok(()) => locks.push(TransitionFileLock { file }),
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(std::fs::TryLockError::Error(error)) => {
                return Err(transition_io_error(&path, error));
            }
        }
    }
    Ok(Some(locks))
}

fn read_store_entries(
    store_root: &Path,
) -> Result<Vec<std::fs::DirEntry>, DerivedStorageTransitionError> {
    read_directory_if_present(store_root)
}

fn read_directory_if_present(
    path: &Path,
) -> Result<Vec<std::fs::DirEntry>, DerivedStorageTransitionError> {
    match std::fs::read_dir(path) {
        Ok(entries) => entries
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| transition_io_error(path, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(transition_io_error(path, error)),
    }
}

fn path_kind(path: &Path) -> Result<Option<bool>, DerivedStorageTransitionError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata.is_dir())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(transition_io_error(path, error)),
    }
}

fn is_transition_generation_id(generation_id: &str) -> bool {
    !generation_id.is_empty()
        && generation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn transition_receipt(disposition: DerivedStorageTransition) -> DerivedStorageTransitionReceipt {
    DerivedStorageTransitionReceipt {
        disposition,
        from_namespace: Some(DerivedStorageNamespace::Legacy),
        to_namespace: DerivedStorageNamespace::Stable,
        moved_artifacts: Vec::new(),
    }
}

fn transition_io_error(path: &Path, error: std::io::Error) -> DerivedStorageTransitionError {
    DerivedStorageTransitionError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

fn is_well_formed_disposable_suffix(suffix: &str) -> bool {
    let mut components = suffix.split('-');
    let Some(process_id) = components.next() else {
        return false;
    };
    let Some(sequence) = components.next() else {
        return false;
    };
    components.next().is_none()
        && !process_id.is_empty()
        && process_id.bytes().all(|byte| byte.is_ascii_digit())
        && !sequence.is_empty()
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs::{self, OpenOptions};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};

    use super::{
        DerivedStorageDiscovery, DerivedStorageLayout, DerivedStorageNamespace,
        DerivedStorageTransition,
    };

    #[test]
    fn absent_store_selects_stable_paths_without_creating_them() {
        let root = tempfile::tempdir().unwrap();

        let discovery = DerivedStorageLayout::discover(root.path());

        let DerivedStorageDiscovery::Selected(layout) = discovery else {
            panic!("an absent namespace must select the stable layout");
        };
        assert_eq!(layout.namespace(), DerivedStorageNamespace::Stable);
        assert_eq!(layout.root(), root.path().join("derived"));
        assert_eq!(
            layout.writer_lock(),
            root.path().join("derived.writer.lock")
        );
        assert_eq!(
            layout.rebuild_lock(),
            root.path().join("derived.rebuild.lock")
        );
        assert!(!layout.root().exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn discovery_preserves_legacy_only_and_selects_stable_only() {
        let legacy = tempfile::tempdir().unwrap();
        fs::create_dir(legacy.path().join(".pointbreak-derived")).unwrap();
        let DerivedStorageDiscovery::Selected(layout) =
            DerivedStorageLayout::discover(legacy.path())
        else {
            panic!("legacy-only namespace must remain usable");
        };
        assert_eq!(layout.namespace(), DerivedStorageNamespace::Legacy);
        assert_eq!(layout.root(), legacy.path().join(".pointbreak-derived"));

        let stable = tempfile::tempdir().unwrap();
        fs::create_dir(stable.path().join("derived")).unwrap();
        let DerivedStorageDiscovery::Selected(layout) =
            DerivedStorageLayout::discover(stable.path())
        else {
            panic!("stable-only namespace must be selected");
        };
        assert_eq!(layout.namespace(), DerivedStorageNamespace::Stable);
        assert_eq!(layout.root(), stable.path().join("derived"));
    }

    #[test]
    fn both_roots_are_conflict_and_governed_names_cover_every_artifact_family() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".pointbreak-derived")).unwrap();
        fs::create_dir(root.path().join("derived")).unwrap();

        assert!(matches!(
            DerivedStorageLayout::discover(root.path()),
            DerivedStorageDiscovery::Conflict { .. }
        ));
        let conflict = DerivedStorageLayout::resolve(root.path())
            .expect_err("both namespaces must not be selected")
            .to_string();
        assert!(conflict.contains("POINTBREAK_DERIVED_ACCESS=off"));
        assert!(conflict.contains("move one root aside"));

        for namespace in [
            DerivedStorageNamespace::Stable,
            DerivedStorageNamespace::Legacy,
        ] {
            let layout = DerivedStorageLayout::for_namespace(root.path(), namespace);
            for path in [
                layout.root(),
                layout.writer_lock(),
                layout.rebuild_lock(),
                layout.generation_lease("g-1"),
                layout.quarantine("42-7"),
                layout.retired("42-7"),
            ] {
                let name = path.file_name().unwrap().to_str().unwrap();
                let is_directory = path == layout.root() || layout.is_disposable_root(&path);
                assert!(DerivedStorageLayout::is_governed_store_entry(
                    name,
                    is_directory,
                    !is_directory,
                ));
            }
            for path in [layout.quarantine("pid-7"), layout.retired("42")] {
                assert!(!DerivedStorageLayout::is_governed_store_entry(
                    path.file_name().unwrap().to_str().unwrap(),
                    true,
                    false,
                ));
            }
        }
        let stable =
            DerivedStorageLayout::for_namespace(root.path(), DerivedStorageNamespace::Stable);
        let legacy =
            DerivedStorageLayout::for_namespace(root.path(), DerivedStorageNamespace::Legacy);
        assert!(stable.is_disposable_root(&legacy.quarantine("42-7")));
        let legacy_lease = legacy.generation_lease("g-1");
        assert_eq!(
            stable.generation_lease_id(legacy_lease.file_name().unwrap().to_str().unwrap()),
            Some("g-1"),
        );
        assert!(!DerivedStorageLayout::is_governed_store_entry(
            "derived-notes",
            true,
            false,
        ));
        assert!(!DerivedStorageLayout::is_governed_store_entry(
            ".pointbreak-derived-notes",
            true,
            false,
        ));
    }

    #[test]
    fn compatible_legacy_tree_moves_byte_identically_and_repeated_transition_is_a_noop() {
        let root = tempfile::tempdir().unwrap();
        let (legacy, stable) = seed_legacy_tree(root.path());
        let before = tree_snapshot(&legacy.root());

        let receipt = DerivedStorageLayout::transition_legacy(root.path()).unwrap();

        assert_eq!(receipt.disposition, DerivedStorageTransition::Moved);
        assert_eq!(
            receipt.from_namespace,
            Some(DerivedStorageNamespace::Legacy)
        );
        assert_eq!(receipt.to_namespace, DerivedStorageNamespace::Stable);
        assert!(!legacy.root().exists());
        assert_eq!(tree_snapshot(&stable.root()), before);
        assert!(stable.quarantine("42-7").exists());
        assert!(stable.retired("42-8").exists());
        assert!(!legacy.quarantine("42-7").exists());
        assert!(!legacy.retired("42-8").exists());

        let repeated = DerivedStorageLayout::transition_legacy(root.path()).unwrap();
        assert_eq!(repeated.disposition, DerivedStorageTransition::NotNeeded);
        assert!(repeated.moved_artifacts.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn transition_never_opens_authoritative_carriers_or_rebuilds_the_generation() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let (legacy, stable) = seed_legacy_tree(root.path());
        let before = tree_snapshot(&legacy.root());
        let events = root.path().join("events");
        fs::create_dir(&events).unwrap();
        fs::write(events.join("authoritative.json"), b"truth").unwrap();
        fs::set_permissions(&events, fs::Permissions::from_mode(0o000)).unwrap();

        let receipt = DerivedStorageLayout::transition_legacy(root.path()).unwrap();

        fs::set_permissions(&events, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(receipt.disposition, DerivedStorageTransition::Moved);
        assert_eq!(tree_snapshot(&stable.root()), before);
        assert_eq!(
            fs::read(events.join("authoritative.json")).unwrap(),
            b"truth"
        );
    }

    #[test]
    fn conflicts_preserve_both_roots_and_colliding_artifacts() {
        let roots = tempfile::tempdir().unwrap();
        let (legacy, stable) = seed_legacy_tree(roots.path());
        fs::create_dir_all(stable.root()).unwrap();
        fs::write(stable.root().join("stable-only"), b"stable").unwrap();

        let receipt = DerivedStorageLayout::transition_legacy(roots.path()).unwrap();

        assert_eq!(receipt.disposition, DerivedStorageTransition::Conflict);
        assert_eq!(fs::read(legacy.root().join("sentinel")).unwrap(), b"legacy");
        assert_eq!(
            fs::read(stable.root().join("stable-only")).unwrap(),
            b"stable"
        );

        let artifacts = tempfile::tempdir().unwrap();
        let (legacy, stable) = seed_legacy_tree(artifacts.path());
        fs::create_dir_all(stable.quarantine("42-7")).unwrap();
        fs::write(stable.quarantine("42-7").join("stable"), b"stable").unwrap();

        let receipt = DerivedStorageLayout::transition_legacy(artifacts.path()).unwrap();

        assert_eq!(receipt.disposition, DerivedStorageTransition::Conflict);
        assert!(legacy.root().exists());
        assert!(legacy.quarantine("42-7").exists());
        assert_eq!(
            fs::read(stable.quarantine("42-7").join("stable")).unwrap(),
            b"stable"
        );
    }

    #[test]
    fn non_directory_root_and_retired_collision_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let stable =
            DerivedStorageLayout::for_namespace(root.path(), DerivedStorageNamespace::Stable);
        fs::write(stable.root(), b"not a directory").unwrap();

        let receipt = DerivedStorageLayout::transition_legacy(root.path()).unwrap();

        assert_eq!(receipt.disposition, DerivedStorageTransition::Conflict);
        assert_eq!(fs::read(stable.root()).unwrap(), b"not a directory");

        let artifacts = tempfile::tempdir().unwrap();
        let stable =
            DerivedStorageLayout::for_namespace(artifacts.path(), DerivedStorageNamespace::Stable);
        let legacy =
            DerivedStorageLayout::for_namespace(artifacts.path(), DerivedStorageNamespace::Legacy);
        fs::create_dir_all(legacy.retired("7-9")).unwrap();
        fs::create_dir_all(stable.retired("7-9")).unwrap();
        fs::write(legacy.retired("7-9").join("legacy"), b"legacy").unwrap();
        fs::write(stable.retired("7-9").join("stable"), b"stable").unwrap();

        let receipt = DerivedStorageLayout::transition_legacy(artifacts.path()).unwrap();

        assert_eq!(receipt.disposition, DerivedStorageTransition::Conflict);
        assert_eq!(
            fs::read(legacy.retired("7-9").join("legacy")).unwrap(),
            b"legacy"
        );
        assert_eq!(
            fs::read(stable.retired("7-9").join("stable")).unwrap(),
            b"stable"
        );
    }

    #[test]
    fn stable_side_generation_lease_defers_a_legacy_transition() {
        let root = tempfile::tempdir().unwrap();
        let legacy =
            DerivedStorageLayout::for_namespace(root.path(), DerivedStorageNamespace::Legacy);
        let stable =
            DerivedStorageLayout::for_namespace(root.path(), DerivedStorageNamespace::Stable);
        fs::create_dir_all(legacy.root().join("generations/g-test")).unwrap();
        let stable_lease = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(stable.generation_lease("g-test"))
            .unwrap();
        stable_lease.lock_shared().unwrap();

        let receipt = DerivedStorageLayout::transition_legacy(root.path()).unwrap();

        assert_eq!(receipt.disposition, DerivedStorageTransition::Deferred);
        assert!(legacy.root().is_dir());
        assert!(!stable.root().exists());
    }

    #[test]
    fn live_reader_writer_and_rebuilder_each_defer_without_moving_data() {
        for mode in ["reader", "writer", "rebuild"] {
            let root = tempfile::tempdir().unwrap();
            let signals = tempfile::tempdir().unwrap();
            let (legacy, stable) = seed_legacy_tree(root.path());
            seed_coordination_files(&legacy, &stable);
            let before = tree_snapshot(&legacy.root());
            let ready = signals.path().join("ready");
            let release = signals.path().join("release");
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "session::derived_access::layout::tests::transition_lock_child",
                ])
                .env("POINTBREAK_TRANSITION_LOCK_ROOT", root.path())
                .env("POINTBREAK_TRANSITION_LOCK_MODE", mode)
                .env("POINTBREAK_TRANSITION_LOCK_READY", &ready)
                .env("POINTBREAK_TRANSITION_LOCK_RELEASE", &release)
                .spawn()
                .unwrap();
            wait_for_path(&ready);

            let receipt = DerivedStorageLayout::transition_legacy(root.path()).unwrap();

            assert_eq!(
                receipt.disposition,
                DerivedStorageTransition::Deferred,
                "{mode}"
            );
            assert_eq!(tree_snapshot(&legacy.root()), before, "{mode}");
            assert!(!stable.root().exists(), "{mode}");
            fs::write(&release, b"release").unwrap();
            assert!(child.wait().unwrap().success(), "{mode}");
        }
    }

    #[test]
    fn every_filesystem_boundary_resumes_after_process_interruption() {
        for crash_after in 1..=3 {
            let root = tempfile::tempdir().unwrap();
            let (legacy, stable) = seed_legacy_tree(root.path());
            let before = tree_snapshot(&legacy.root());
            let result = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "session::derived_access::layout::tests::transition_crash_child",
                ])
                .env("POINTBREAK_TRANSITION_CRASH_ROOT", root.path())
                .env("POINTBREAK_TRANSITION_CRASH_AFTER", crash_after.to_string())
                .status()
                .unwrap();
            assert_eq!(result.code(), Some(91), "boundary {crash_after}");

            let resumed = DerivedStorageLayout::transition_legacy(root.path()).unwrap();

            let expected = if crash_after == 3 {
                DerivedStorageTransition::NotNeeded
            } else {
                DerivedStorageTransition::Moved
            };
            assert_eq!(resumed.disposition, expected, "boundary {crash_after}");
            assert!(!legacy.root().exists());
            assert_eq!(tree_snapshot(&stable.root()), before);
            assert!(stable.quarantine("42-7").exists());
            assert!(stable.retired("42-8").exists());
        }
    }

    #[test]
    #[ignore = "spawned by live_reader_writer_and_rebuilder_each_defer_without_moving_data"]
    fn transition_lock_child() {
        let root = PathBuf::from(std::env::var_os("POINTBREAK_TRANSITION_LOCK_ROOT").unwrap());
        let mode = std::env::var("POINTBREAK_TRANSITION_LOCK_MODE").unwrap();
        let ready = PathBuf::from(std::env::var_os("POINTBREAK_TRANSITION_LOCK_READY").unwrap());
        let release =
            PathBuf::from(std::env::var_os("POINTBREAK_TRANSITION_LOCK_RELEASE").unwrap());
        let legacy = DerivedStorageLayout::for_namespace(&root, DerivedStorageNamespace::Legacy);
        let path = match mode.as_str() {
            "reader" => legacy.generation_lease("g-test"),
            "writer" => legacy.writer_lock(),
            "rebuild" => legacy.rebuild_lock(),
            _ => panic!("unknown transition-lock mode"),
        };
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .unwrap();
        if mode == "reader" {
            file.lock_shared().unwrap();
        } else {
            file.lock().unwrap();
        }
        fs::write(&ready, b"ready").unwrap();
        wait_for_path(&release);
        file.unlock().unwrap();
    }

    #[test]
    #[ignore = "spawned by every_filesystem_boundary_resumes_after_process_interruption"]
    fn transition_crash_child() {
        let root = PathBuf::from(std::env::var_os("POINTBREAK_TRANSITION_CRASH_ROOT").unwrap());
        let crash_after = std::env::var("POINTBREAK_TRANSITION_CRASH_AFTER")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let mut observed = 0;
        DerivedStorageLayout::transition_legacy_with_hook(&root, |_| {
            observed += 1;
            if observed == crash_after {
                std::process::exit(91);
            }
        })
        .unwrap();
        panic!("transition did not reach requested crash boundary");
    }

    fn seed_legacy_tree(root: &Path) -> (DerivedStorageLayout, DerivedStorageLayout) {
        let legacy = DerivedStorageLayout::for_namespace(root, DerivedStorageNamespace::Legacy);
        let stable = DerivedStorageLayout::for_namespace(root, DerivedStorageNamespace::Stable);
        fs::create_dir_all(legacy.root().join("generations/g-test")).unwrap();
        fs::write(legacy.root().join("sentinel"), b"legacy").unwrap();
        fs::write(
            legacy.root().join("generations/g-test/payload"),
            b"generation",
        )
        .unwrap();
        fs::create_dir_all(legacy.quarantine("42-7").join("generations/g-old")).unwrap();
        fs::write(legacy.quarantine("42-7").join("quarantine"), b"quarantine").unwrap();
        fs::create_dir_all(legacy.retired("42-8").join("generations/g-retired")).unwrap();
        fs::write(legacy.retired("42-8").join("retired"), b"retired").unwrap();
        (legacy, stable)
    }

    fn seed_coordination_files(legacy: &DerivedStorageLayout, stable: &DerivedStorageLayout) {
        for path in [
            legacy.writer_lock(),
            legacy.rebuild_lock(),
            stable.writer_lock(),
            stable.rebuild_lock(),
        ] {
            fs::write(path, b"").unwrap();
        }
    }

    fn tree_snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        fn visit(root: &Path, path: &Path, snapshot: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            if path.is_dir() {
                snapshot.insert(relative, None);
                let mut entries = fs::read_dir(path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .collect::<Vec<_>>();
                entries.sort();
                for entry in entries {
                    visit(root, &entry, snapshot);
                }
            } else {
                snapshot.insert(relative, Some(fs::read(path).unwrap()));
            }
        }

        let mut snapshot = BTreeMap::new();
        visit(root, root, &mut snapshot);
        snapshot
    }

    fn wait_for_path(path: &Path) {
        let started = Instant::now();
        while !path.exists() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "timed out waiting for {}",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
