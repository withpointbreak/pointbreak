//! Stable and legacy filesystem vocabulary for disposable derived access.

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
    use std::fs;

    use super::{DerivedStorageDiscovery, DerivedStorageLayout, DerivedStorageNamespace};

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
}
