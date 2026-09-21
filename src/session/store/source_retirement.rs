//! Retiring a source store after its records were folded into a destination.
//!
//! Retirement is the only code path that deletes a source store. It holds the
//! source store's existing authority lock for the whole fold → verify → delete
//! sequence (taken without blocking; a held lock refuses the retire), and it
//! deletes only the individual files that subset verification proved present in
//! the destination — never a recursive delete of the store. The store directory
//! and its authority lock file are left behind on purpose.

use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::error::{Result, ShoreError};
use crate::session::derived_access::layout::DerivedStorageLayout;
use crate::session::store::authority_lock::{STORE_AUTHORITY_LOCK_FILE, StoreAuthorityLock};
use crate::session::store::bundle::verify_source_subset_of_target;

/// Every refusal caused by another writer holding the source store starts with
/// this prefix, so library and CLI callers can match it without an enum.
pub(in crate::session) const SOURCE_BUSY_PREFIX: &str = "source_busy;";

/// Held for the whole retire: fold, verify, delete.
#[derive(Debug)]
pub(in crate::session) struct SourceRetirementGuard {
    _lock: StoreAuthorityLock,
    canonical_source: PathBuf,
}

#[derive(Debug)]
pub(in crate::session) enum SourceRetirementAdmission {
    Admitted(SourceRetirementGuard),
    Busy,
}

/// Take the source store's authority lock without blocking, before the fold
/// reads the source. `source` must already exist: acquiring the lock creates
/// the store root, and an absent source must stay absent. Refuses when source
/// and target are the same store. A lock this thread already holds is reported
/// as `Busy`, because a re-entrant hold excludes nobody.
pub(in crate::session) fn admit_source_retirement(
    source: &Path,
    target: &Path,
) -> Result<SourceRetirementAdmission> {
    let exists = source.try_exists().map_err(|error| {
        ShoreError::Message(format!(
            "could not inspect source store {}: {error}",
            source.display()
        ))
    })?;
    if !exists {
        return Err(ShoreError::Message(format!(
            "source store {} does not exist; nothing to retire",
            source.display()
        )));
    }
    let canonical_source = canonicalize(source)?;
    let target_exists = target.try_exists().map_err(|error| {
        ShoreError::Message(format!(
            "could not inspect destination store {}: {error}",
            target.display()
        ))
    })?;
    if target_exists && canonicalize(target)? == canonical_source {
        return Err(ShoreError::Message(format!(
            "source store {} and destination store {} are the same store; refusing to retire it",
            source.display(),
            target.display()
        )));
    }
    match StoreAuthorityLock::try_acquire(source)? {
        None => Ok(SourceRetirementAdmission::Busy),
        Some(lock) if lock.is_reentrant() => Ok(SourceRetirementAdmission::Busy),
        Some(lock) => Ok(SourceRetirementAdmission::Admitted(SourceRetirementGuard {
            _lock: lock,
            canonical_source,
        })),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::session) struct SourceRetirementOutcome {
    pub source_retired: bool,
    pub verified_events: usize,
    pub verified_artifacts: usize,
    pub residue_entries: usize,
}

/// Verify the source against the target, then delete only the verified paths.
///
/// Consumes the admission guard and releases it after the outcome is observed.
/// The store directory and its authority lock file are never removed, so the
/// lock's identity is stable for any writer waiting on it; the source counts as
/// retired when nothing else remains. Refuses, deleting nothing, when verification
/// fails, when any symlink exists under the source, or when the store root
/// holds an entry retirement does not understand. A file under `events/` or
/// `artifacts/` that verification did not cover (it appeared after the
/// verification walk) is residue: nothing is deleted and the result reports the
/// source as not retired, so a rerun folds it first. Deletion order leaves
/// event files for last, so any interrupted retire still looks populated.
pub(in crate::session) fn retire_verified_source(
    guard: SourceRetirementGuard,
    source: &Path,
    target: &Path,
) -> Result<SourceRetirementOutcome> {
    if canonicalize(source)? != guard.canonical_source {
        return Err(ShoreError::Message(format!(
            "source store {} is not the store admitted for retirement",
            source.display()
        )));
    }
    let verification = verify_source_subset_of_target(source, target)?;
    let verified_events = verification.verified_events;
    let verified_artifacts = verification.verified_artifacts;
    let verified: HashSet<&Path> = verification
        .verified_relative_paths
        .iter()
        .map(PathBuf::as_path)
        .collect();

    #[cfg(test)]
    run_retirement_hook(RetirementHookPoint::AfterVerify, source);

    let plan = scan_source(source, &verified)?;
    if plan.residue_entries > 0 {
        return Ok(SourceRetirementOutcome {
            source_retired: false,
            verified_events,
            verified_artifacts,
            residue_entries: plan.residue_entries,
        });
    }

    #[cfg(test)]
    run_retirement_hook(RetirementHookPoint::AfterScan, source);

    for relative in &plan.disposable_files {
        remove_file_if_present(&source.join(relative))?;
    }
    for relative in &plan.derived_directories {
        empty_directory_tree(&source.join(relative))?;
    }
    for top in ["artifacts", "events"] {
        for relative in &verification.verified_relative_paths {
            if relative.starts_with(top) {
                remove_file_if_present(&source.join(relative))?;
            }
        }
    }
    remove_empty_directories_below(source)?;

    // The store directory and its authority lock file stay. Unlinking the lock
    // file would let a writer that already opened it and a newcomer that creates
    // a fresh one at the same path both hold "the" lock. So the source counts as
    // retired when nothing but that lock file remains.
    let residue_entries = count_files_other_than_the_lock(source)?;
    let source_retired = residue_entries == 0;
    drop(guard);

    Ok(SourceRetirementOutcome {
        source_retired,
        verified_events,
        verified_artifacts,
        residue_entries,
    })
}

/// What the pre-deletion scan found. Paths are relative to the source root.
#[derive(Debug, Default)]
struct RetirementPlan {
    /// Store-root and in-tree files that hold nothing durable (`state.json`,
    /// `*.tmp`, governed derived-access lock files). Never the authority lock.
    disposable_files: Vec<PathBuf>,
    /// Governed derived-access directories at the store root: rebuild-only data.
    derived_directories: Vec<PathBuf>,
    /// Files under `events/` or `artifacts/` that verification did not cover.
    residue_entries: usize,
}

/// Classify every entry under the source without following a symlink. Any
/// symlink, and any store-root entry outside the allowlist, is an error; both
/// are reported before anything is deleted.
fn scan_source(source: &Path, verified: &HashSet<&Path>) -> Result<RetirementPlan> {
    if let Some(link) = first_symlink_below(source, Path::new(""))? {
        return Err(ShoreError::Message(format!(
            "source store {} contains a symbolic link at {}; retirement never follows links, \
             so nothing was deleted — the source store is left untouched",
            source.display(),
            store_relative_display(&link)
        )));
    }

    let mut plan = RetirementPlan::default();
    let mut unknown = Vec::new();
    for entry in read_directory(source)? {
        let name = entry.file_name();
        let relative = PathBuf::from(&name);
        let kind = entry_kind(&source.join(&name))?;
        let name = name.to_string_lossy();
        match name.as_ref() {
            "events" | "artifacts" if kind.is_dir() => {
                classify_record_tree(source, &relative, verified, &mut plan)?;
            }
            _ if kind.is_file() && name == STORE_AUTHORITY_LOCK_FILE => {}
            _ if kind.is_file() && (name == "state.json" || name.ends_with(".tmp")) => {
                plan.disposable_files.push(relative);
            }
            _ if DerivedStorageLayout::is_governed_store_entry(
                &name,
                kind.is_dir(),
                kind.is_file(),
            ) =>
            {
                if kind.is_dir() {
                    plan.derived_directories.push(relative);
                } else {
                    plan.disposable_files.push(relative);
                }
            }
            _ if kind.is_dir() && count_remaining_files(&source.join(&relative))? == 0 => {}
            _ => unknown.push(relative),
        }
    }
    if unknown.is_empty() {
        return Ok(plan);
    }

    let mut named = Vec::new();
    for relative in &unknown {
        let mut files = Vec::new();
        collect_files(source, relative, &mut files)?;
        if files.is_empty() {
            files.push(relative.clone());
        }
        named.extend(files);
    }
    named.sort();
    let shown = named
        .iter()
        .take(3)
        .map(|path| store_relative_display(path))
        .collect::<Vec<_>>();
    Err(ShoreError::Message(format!(
        "source store {} holds {} file(s) outside the verified record trees ({}); retirement \
         does not verify them, so nothing was deleted — the source store is left untouched",
        source.display(),
        named.len(),
        shown.join(", ")
    )))
}

/// Walk `events/` or `artifacts/`: verified files are deleted later from the
/// verification list, `*.tmp` files are disposable, anything else is residue.
fn classify_record_tree(
    source: &Path,
    relative: &Path,
    verified: &HashSet<&Path>,
    plan: &mut RetirementPlan,
) -> Result<()> {
    for entry in read_directory(&source.join(relative))? {
        let child = relative.join(entry.file_name());
        let kind = entry_kind(&source.join(&child))?;
        if kind.is_dir() {
            classify_record_tree(source, &child, verified, plan)?;
        } else if verified.contains(child.as_path()) {
            // Deleted later, from the verification list itself.
        } else if entry.file_name().to_string_lossy().ends_with(".tmp") {
            plan.disposable_files.push(child);
        } else {
            plan.residue_entries += 1;
        }
    }
    Ok(())
}

fn first_symlink_below(root: &Path, relative: &Path) -> Result<Option<PathBuf>> {
    for entry in read_directory(&root.join(relative))? {
        let child = relative.join(entry.file_name());
        let kind = entry_kind(&root.join(&child))?;
        if kind.is_symlink() {
            return Ok(Some(child));
        }
        if kind.is_dir()
            && let Some(link) = first_symlink_below(root, &child)?
        {
            return Ok(Some(link));
        }
    }
    Ok(None)
}

/// Every non-directory entry below `root.join(relative)`, relative to `root`.
fn collect_files(root: &Path, relative: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in read_directory(&root.join(relative))? {
        let child = relative.join(entry.file_name());
        if entry_kind(&root.join(&child))?.is_dir() {
            collect_files(root, &child, out)?;
        } else {
            out.push(child);
        }
    }
    Ok(())
}

/// Files left under the source other than its store-root authority lock file,
/// plus any other store-root entry that is not a directory emptied below.
fn count_files_other_than_the_lock(source: &Path) -> Result<usize> {
    let mut files = Vec::new();
    collect_files(source, Path::new(""), &mut files)?;
    let remaining = files
        .iter()
        .filter(|path| path.as_path() != Path::new(STORE_AUTHORITY_LOCK_FILE))
        .count();
    if remaining > 0 {
        return Ok(remaining);
    }
    // A directory that appeared after the directory pass holds no files but is
    // still something retirement did not account for.
    let stray_directories = read_directory(source)?
        .iter()
        .filter(|entry| entry.file_name() != STORE_AUTHORITY_LOCK_FILE)
        .count();
    Ok(stray_directories)
}

fn count_remaining_files(dir: &Path) -> Result<usize> {
    let mut files = Vec::new();
    collect_files(dir, Path::new(""), &mut files)?;
    Ok(files.len())
}

/// Delete everything inside a governed derived directory, deepest first,
/// without following links, then the directory itself when it is empty.
fn empty_directory_tree(dir: &Path) -> Result<()> {
    for entry in read_directory(dir)? {
        let path = dir.join(entry.file_name());
        if entry_kind(&path)?.is_dir() {
            empty_directory_tree(&path)?;
        } else {
            remove_file_if_present(&path)?;
        }
    }
    remove_directory_if_empty(dir)
}

/// Remove every empty directory below `dir`, deepest first. A directory that
/// still holds something is left in place.
fn remove_empty_directories_below(dir: &Path) -> Result<()> {
    for entry in read_directory(dir)? {
        let path = dir.join(entry.file_name());
        if entry_kind(&path)?.is_dir() {
            remove_empty_directories_below(&path)?;
            remove_directory_if_empty(&path)?;
        }
    }
    Ok(())
}

fn remove_file_if_present(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ShoreError::Message(format!(
            "retire source store: could not delete {}: {error}; files deleted so far were \
             verified in the destination — rerun the retire to finish",
            path.display()
        ))),
    }
}

fn remove_directory_if_empty(path: &Path) -> Result<()> {
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(ShoreError::Message(format!(
            "retire source store: could not remove directory {}: {error}",
            path.display()
        ))),
    }
}

/// Directory entries; a directory that vanished reads as empty.
fn read_directory(dir: &Path) -> Result<Vec<std::fs::DirEntry>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(ShoreError::Message(format!(
                "retire source store: could not read directory {}: {error}",
                dir.display()
            )));
        }
    };
    entries
        .map(|entry| {
            entry.map_err(|error| {
                ShoreError::Message(format!(
                    "retire source store: could not read an entry under {}: {error}",
                    dir.display()
                ))
            })
        })
        .collect()
}

/// The entry's own type; a symlink is reported as a symlink, never followed.
fn entry_kind(path: &Path) -> Result<std::fs::FileType> {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type())
        .map_err(|error| {
            ShoreError::Message(format!(
                "retire source store: could not inspect {}: {error}",
                path.display()
            ))
        })
}

fn store_relative_display(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Where a test hook runs inside `retire_verified_source`.
#[cfg(test)]
#[derive(Clone, Copy)]
enum RetirementHookPoint {
    /// After subset verification, before the pre-deletion scan.
    AfterVerify,
    /// After a residue-free scan, before the first unlink.
    AfterScan,
}

#[cfg(test)]
type RetirementHook = Box<dyn FnOnce(&Path)>;

#[cfg(test)]
thread_local! {
    static RETIREMENT_HOOKS: std::cell::RefCell<[Option<RetirementHook>; 2]> =
        const { std::cell::RefCell::new([None, None]) };
}

/// Run `hook` once, on this thread, inside the next retirement: after subset
/// verification and before the pre-deletion scan. Dropping the returned guard
/// clears a hook that never ran.
#[cfg(test)]
pub(in crate::session) fn install_after_verify_hook(
    hook: impl FnOnce(&Path) + 'static,
) -> RetirementHookGuard {
    install_retirement_hook(RetirementHookPoint::AfterVerify, Box::new(hook))
}

/// Run `hook` once, on this thread, inside the next retirement: after a
/// residue-free scan and before the first file is unlinked. Dropping the
/// returned guard clears a hook that never ran.
#[cfg(test)]
pub(in crate::session) fn install_after_scan_hook(
    hook: impl FnOnce(&Path) + 'static,
) -> RetirementHookGuard {
    install_retirement_hook(RetirementHookPoint::AfterScan, Box::new(hook))
}

#[cfg(test)]
fn install_retirement_hook(
    point: RetirementHookPoint,
    hook: RetirementHook,
) -> RetirementHookGuard {
    RETIREMENT_HOOKS.with(|slots| slots.borrow_mut()[point as usize] = Some(hook));
    RetirementHookGuard {
        point,
        _not_send: std::marker::PhantomData,
    }
}

#[cfg(test)]
pub(in crate::session) struct RetirementHookGuard {
    point: RetirementHookPoint,
    _not_send: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(test)]
impl Drop for RetirementHookGuard {
    fn drop(&mut self) {
        RETIREMENT_HOOKS.with(|slots| slots.borrow_mut()[self.point as usize].take());
    }
}

#[cfg(test)]
fn run_retirement_hook(point: RetirementHookPoint, source: &Path) {
    if let Some(hook) = RETIREMENT_HOOKS.with(|slots| slots.borrow_mut()[point as usize].take()) {
        hook(source);
    }
}

fn canonicalize(path: &Path) -> Result<PathBuf> {
    path.canonicalize().map_err(|error| {
        ShoreError::Message(format!(
            "could not resolve store directory {}: {error}",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::mpsc;

    use super::*;

    /// Every file under `root` with its bytes, keyed by root-relative path.
    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    walk(&path, root, out);
                } else {
                    out.insert(
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        fs::read(&path).unwrap(),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(root, root, &mut out);
        out
    }

    fn seeded_store(parent: &Path, name: &str) -> PathBuf {
        let store = parent.join(name);
        fs::create_dir_all(store.join("events")).unwrap();
        fs::write(store.join("events/a.json"), b"{}").unwrap();
        store
    }

    /// Hold `store`'s authority lock on another thread until the returned
    /// sender is dropped or sent to.
    fn hold_lock_on_another_thread(
        store: &Path,
    ) -> (mpsc::Sender<()>, std::thread::JoinHandle<()>) {
        let (held_tx, held_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let store = store.to_path_buf();
        let handle = std::thread::spawn(move || {
            let _lock = StoreAuthorityLock::acquire(&store).unwrap();
            held_tx.send(()).unwrap();
            let _ = release_rx.recv();
        });
        held_rx.recv().unwrap();
        (release_tx, handle)
    }

    #[test]
    fn busy_source_is_refused_and_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let source = seeded_store(temp.path(), "source");
        let target = temp.path().join("target");
        let (release, holder) = hold_lock_on_another_thread(&source);
        let before = snapshot(&source);

        let admission = admit_source_retirement(&source, &target).unwrap();

        assert!(matches!(admission, SourceRetirementAdmission::Busy));
        assert_eq!(snapshot(&source), before);
        release.send(()).unwrap();
        holder.join().unwrap();
    }

    #[test]
    fn idle_source_is_admitted_and_excludes_a_writer() {
        let temp = tempfile::tempdir().unwrap();
        let source = seeded_store(temp.path(), "source");
        let target = temp.path().join("target");

        let admission = admit_source_retirement(&source, &target).unwrap();
        let SourceRetirementAdmission::Admitted(guard) = admission else {
            panic!("an idle source must be admitted");
        };

        let probe = |source: PathBuf| {
            std::thread::spawn(move || StoreAuthorityLock::try_acquire(&source).unwrap().is_some())
                .join()
                .unwrap()
        };
        assert!(
            !probe(source.clone()),
            "the guard must exclude another writer"
        );
        drop(guard);
        assert!(
            probe(source.clone()),
            "dropping the guard must release the lock"
        );
    }

    #[test]
    fn absent_source_is_never_created() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        let target = temp.path().join("target");

        assert!(admit_source_retirement(&missing, &target).is_err());
        assert!(!missing.exists());
    }

    #[test]
    fn same_store_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let source = seeded_store(temp.path(), "source");
        let alias = source.join("..").join("source");

        assert!(admit_source_retirement(&source, &alias).is_err());
        assert!(!source.join(STORE_AUTHORITY_LOCK_FILE).exists());
    }

    #[test]
    fn reentrant_holder_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let source = seeded_store(temp.path(), "source");
        let target = temp.path().join("target");
        let _held = StoreAuthorityLock::acquire(&source).unwrap();

        let admission = admit_source_retirement(&source, &target).unwrap();

        assert!(matches!(admission, SourceRetirementAdmission::Busy));
    }

    // ---- retirement ----------------------------------------------------------

    struct Repo {
        root: tempfile::TempDir,
    }

    impl Repo {
        fn path(&self) -> &Path {
            self.root.path()
        }

        fn git(&self, args: &[&str]) {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(self.path())
                .output()
                .unwrap();
            assert!(output.status.success(), "git {args:?}: {output:?}");
        }

        /// The store captures in this repository land in.
        fn store(&self) -> PathBuf {
            crate::git::git_common_dir(self.path())
                .unwrap()
                .join("pointbreak")
        }
    }

    /// A repository with one captured review: a real source store holding
    /// events, object artifacts and whatever store-root entries a capture
    /// writes.
    fn captured_repo() -> Repo {
        let repo = Repo {
            root: tempfile::tempdir().unwrap(),
        };
        repo.git(&["init", "-q"]);
        repo.git(&["config", "user.name", "Retirement Tests"]);
        repo.git(&["config", "user.email", "retirement-tests@example.com"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        fs::write(repo.path().join("lib.rs"), "pub fn value() -> u32 { 1 }\n").unwrap();
        repo.git(&["add", "--all"]);
        repo.git(&["commit", "-q", "-m", "base"]);
        fs::write(repo.path().join("lib.rs"), "pub fn value() -> u32 { 2 }\n").unwrap();
        crate::session::capture_worktree_review(crate::session::CaptureOptions::new(repo.path()))
            .unwrap();
        repo
    }

    /// Fold `source` into `target` the way the link and migrate workflows do.
    fn fold(source: &Path, target: &Path) {
        crate::session::store::bundle::import_store_bundle(source, target).unwrap();
    }

    fn admit(source: &Path, target: &Path) -> SourceRetirementGuard {
        match admit_source_retirement(source, target).unwrap() {
            SourceRetirementAdmission::Admitted(guard) => guard,
            SourceRetirementAdmission::Busy => panic!("source unexpectedly busy"),
        }
    }

    fn fold_and_retire(source: &Path, target: &Path) -> Result<SourceRetirementOutcome> {
        let guard = admit(source, target);
        fold(source, target);
        retire_verified_source(guard, source, target)
    }

    /// A folded source and its target, with the source's lock file already
    /// present so snapshots taken now match snapshots taken after admission.
    fn folded_pair() -> (Repo, PathBuf, tempfile::TempDir, PathBuf) {
        let repo = captured_repo();
        let source = repo.store();
        let target_root = tempfile::tempdir().unwrap();
        let target = target_root.path().join("store");
        fold(&source, &target);
        drop(admit(&source, &target));
        (repo, source, target_root, target)
    }

    /// Retirement keeps the store directory and its authority lock file so the
    /// lock's identity never changes under a waiting writer; nothing else stays.
    fn assert_only_the_lock_remains(source: &Path) {
        let remaining = fs::read_dir(source)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(
            remaining,
            vec![std::ffi::OsString::from(STORE_AUTHORITY_LOCK_FILE)]
        );
    }

    #[test]
    fn retires_a_fully_verified_source() {
        let (_repo, source, _target_root, target) = folded_pair();
        let files = snapshot(&source);
        let events = files
            .keys()
            .filter(|path| path.starts_with("events"))
            .count();
        let artifacts = files
            .keys()
            .filter(|path| {
                path.starts_with("artifacts") && !path.to_string_lossy().ends_with(".tmp")
            })
            .count();

        let outcome = retire_verified_source(admit(&source, &target), &source, &target).unwrap();

        assert!(outcome.source_retired);
        assert_only_the_lock_remains(&source);
        assert_eq!(outcome.verified_events, events);
        assert_eq!(outcome.verified_artifacts, artifacts);
        assert!(outcome.verified_events >= 1 && outcome.verified_artifacts >= 1);
        assert_eq!(outcome.residue_entries, 0);
    }

    #[test]
    fn unlisted_store_root_entry_refuses_before_any_unlink() {
        let (_repo, source, _target_root, target) = folded_pair();
        fs::create_dir_all(source.join("operations")).unwrap();
        fs::write(source.join("operations/op.json"), b"{}").unwrap();
        let before = snapshot(&source);

        let error = retire_verified_source(admit(&source, &target), &source, &target)
            .expect_err("an unverified store-root entry must refuse");

        assert!(error.to_string().contains("operations/op.json"), "{error}");
        assert_eq!(snapshot(&source), before);
    }

    #[test]
    fn disposables_are_removed() {
        let (_repo, source, _target_root, target) = folded_pair();
        fs::write(source.join("state.json"), b"{}").unwrap();
        fs::write(source.join("events/x.tmp"), b"in flight").unwrap();
        fs::create_dir_all(source.join("artifacts/notes")).unwrap();

        let outcome = retire_verified_source(admit(&source, &target), &source, &target).unwrap();

        assert!(outcome.source_retired);
        assert_only_the_lock_remains(&source);
    }

    #[test]
    fn divergent_target_deletes_nothing() {
        let (_repo, source, _target_root, target) = folded_pair();
        let event = fs::read_dir(target.join("events"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::remove_file(event).unwrap();
        let before = snapshot(&source);

        let error = retire_verified_source(admit(&source, &target), &source, &target)
            .expect_err("a divergent target must refuse");

        assert!(error.to_string().contains("nothing was deleted"), "{error}");
        assert_eq!(snapshot(&source), before);
    }

    #[test]
    fn nested_state_json_is_not_disposable() {
        let (_repo, source, _target_root, target) = folded_pair();
        fs::write(source.join("artifacts/objects/state.json"), b"{}").unwrap();
        let before = snapshot(&source);

        assert!(retire_verified_source(admit(&source, &target), &source, &target).is_err());
        assert_eq!(snapshot(&source), before);
    }

    #[test]
    fn ordinary_store_with_derived_entries_retires() {
        use crate::session::derived_access::lifecycle::{DerivedAccessLifecycle, LifecycleControl};
        use crate::session::derived_access::product_contract::DerivedAccessProfile;
        use crate::session::store::resolution::opaque_path_identity;

        let (_repo, source, _target_root, target) = folded_pair();
        let lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            &source,
            opaque_path_identity("store", &source).unwrap(),
        )
        .unwrap();
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        drop(lifecycle);
        fs::write(source.join("derived.writer.lock"), b"").unwrap();
        fs::write(source.join("derived.rebuild.lock"), b"").unwrap();
        let derived_files = snapshot(&source)
            .into_keys()
            .filter(|path| path.starts_with("derived"))
            .count();
        assert!(derived_files > 2, "the derived tree must be populated");

        let outcome = retire_verified_source(admit(&source, &target), &source, &target).unwrap();

        assert!(outcome.source_retired);
        assert_only_the_lock_remains(&source);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_into_target_refuses_and_target_survives() {
        let (_repo, source, _target_root, target) = folded_pair();
        std::os::unix::fs::symlink(
            target.join("artifacts/objects"),
            source.join("artifacts/objects/link"),
        )
        .unwrap();
        crate::session::store::bundle::verify_source_subset_of_target(&source, &target)
            .expect("linked artifacts verify by content");
        let target_before = snapshot(&target);

        assert!(retire_verified_source(admit(&source, &target), &source, &target).is_err());
        for (path, bytes) in &target_before {
            if path.starts_with("events") || path.starts_with("artifacts") {
                assert_eq!(
                    &fs::read(target.join(path)).unwrap(),
                    bytes,
                    "{}",
                    path.display()
                );
            }
        }
        assert!(source.join("artifacts/objects/link").exists());
    }

    /// Restores a directory's permissions when dropped, so a failing test
    /// cannot leave an undeletable temp directory behind.
    #[cfg(unix)]
    struct ReadOnlyDir(pub PathBuf);

    #[cfg(unix)]
    impl ReadOnlyDir {
        fn new(path: PathBuf) -> Self {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o555)).unwrap();
            Self(path)
        }
    }

    #[cfg(unix)]
    impl Drop for ReadOnlyDir {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }

    #[cfg(unix)]
    #[test]
    fn unlink_error_is_an_error_not_residue() {
        let (_repo, source, _target_root, target) = folded_pair();
        let _read_only = ReadOnlyDir::new(source.join("artifacts/objects"));

        assert!(retire_verified_source(admit(&source, &target), &source, &target).is_err());
    }

    // ---- interleavings -------------------------------------------------------

    fn record_straggler_observation(repo: &Path, body: String) {
        crate::session::record_observation(
            crate::session::ObservationAddOptions::new(repo)
                .with_track("agent:straggler")
                .with_title("Straggler")
                .with_body(body),
        )
        .unwrap();
    }

    fn new_paths(
        before: &BTreeMap<PathBuf, Vec<u8>>,
        after: &BTreeMap<PathBuf, Vec<u8>>,
        top: &str,
    ) -> Vec<PathBuf> {
        after
            .keys()
            .filter(|path| path.starts_with(top) && !before.contains_key(*path))
            .cloned()
            .collect()
    }

    fn assert_still_present(source: &Path, files: &BTreeMap<PathBuf, Vec<u8>>, top: &[&str]) {
        for (path, bytes) in files {
            if top.iter().any(|top| path.starts_with(top)) {
                assert_eq!(
                    &fs::read(source.join(path)).unwrap(),
                    bytes,
                    "{}",
                    path.display()
                );
            }
        }
    }

    /// An event written after verification's walk is not in the deletion list,
    /// so it survives, and because the scan finds it nothing else is deleted
    /// either. A recursive delete of the store would have lost it.
    #[test]
    fn append_after_verification_survives_retirement() {
        let (repo, source, _target_root, target) = folded_pair();
        let before = snapshot(&source);
        let repo_path = repo.path().to_path_buf();
        let _hook = install_after_verify_hook(move |_| {
            record_straggler_observation(&repo_path, "late".to_owned());
        });

        let outcome = retire_verified_source(admit(&source, &target), &source, &target).unwrap();

        assert!(!outcome.source_retired);
        assert!(outcome.residue_entries >= 1);
        let after = snapshot(&source);
        let stragglers = new_paths(&before, &after, "events");
        assert_eq!(stragglers.len(), 1, "one late event");
        assert_still_present(&source, &before, &["events", "artifacts"]);

        let outcome = fold_and_retire(&source, &target).unwrap();
        assert!(outcome.source_retired);
        assert_only_the_lock_remains(&source);
        assert!(target.join(&stragglers[0]).exists());
    }

    /// Content no source event refers to is never folded, so a rerun keeps
    /// refusing — and keeps deleting nothing.
    #[test]
    fn orphan_content_after_verification_survives_and_rerun_refuses() {
        let (_repo, source, _target_root, target) = folded_pair();
        let before = snapshot(&source);
        let _hook = install_after_verify_hook(|source| {
            fs::write(
                source.join("artifacts/objects/orphan"),
                b"no event names me",
            )
            .unwrap();
        });

        let outcome = retire_verified_source(admit(&source, &target), &source, &target).unwrap();

        assert!(!outcome.source_retired);
        assert!(outcome.residue_entries >= 1);
        assert_still_present(&source, &before, &["events", "artifacts"]);
        let with_orphan = snapshot(&source);

        assert!(fold_and_retire(&source, &target).is_err());
        assert_still_present(&source, &with_orphan, &["events", "artifacts"]);
        assert!(source.join("artifacts/objects/orphan").exists());
    }

    #[test]
    fn referenced_content_after_verification_converges() {
        let (repo, source, _target_root, target) = folded_pair();
        let before = snapshot(&source);
        let repo_path = repo.path().to_path_buf();
        let body = "x".repeat(crate::session::body_artifact::BODY_INLINE_LIMIT + 1);
        let _hook = install_after_verify_hook(move |_| {
            record_straggler_observation(&repo_path, body);
        });

        let outcome = retire_verified_source(admit(&source, &target), &source, &target).unwrap();

        assert!(!outcome.source_retired);
        assert!(outcome.residue_entries >= 2);
        let after = snapshot(&source);
        assert_eq!(
            new_paths(&before, &after, "artifacts").len(),
            1,
            "one late body"
        );
        assert_still_present(&source, &before, &["events", "artifacts"]);

        let outcome = fold_and_retire(&source, &target).unwrap();
        assert!(outcome.source_retired);
        assert_only_the_lock_remains(&source);
    }

    /// An interruption after the artifacts are gone still leaves event files,
    /// so the rerun sees a populated store and finishes.
    #[cfg(unix)]
    #[test]
    fn interrupted_after_artifacts_converges_on_rerun() {
        let (_repo, source, _target_root, target) = folded_pair();
        let events_before = snapshot(&source.join("events"));
        assert!(!events_before.is_empty());

        let read_only = ReadOnlyDir::new(source.join("events"));
        assert!(retire_verified_source(admit(&source, &target), &source, &target).is_err());
        drop(read_only);

        assert_eq!(snapshot(&source.join("events")), events_before);
        assert!(snapshot(&source.join("artifacts")).is_empty());

        let outcome = fold_and_retire(&source, &target).unwrap();
        assert!(outcome.source_retired);
        assert_only_the_lock_remains(&source);
    }

    /// A writer that blocks on the source lock during retirement proceeds once
    /// the lock is released and writes into the retained store directory; its
    /// record is kept on disk. Only preservation is asserted, not visibility.
    #[test]
    fn writer_blocked_during_retirement_does_not_lose_its_event() {
        let (_repo, source, _target_root, target) = folded_pair();
        let writer_source = source.clone();
        let writer: std::rc::Rc<std::cell::Cell<Option<std::thread::JoinHandle<()>>>> =
            std::rc::Rc::default();
        let slot = writer.clone();
        let _hook = install_after_verify_hook(move |_| {
            let (started_tx, started_rx) = mpsc::channel();
            let handle = std::thread::spawn(move || {
                started_tx.send(()).unwrap();
                let _lock = StoreAuthorityLock::acquire(&writer_source).unwrap();
                fs::create_dir_all(writer_source.join("events")).unwrap();
                fs::write(
                    writer_source.join("events/late-writer.json"),
                    b"{\"late\":true}",
                )
                .unwrap();
            });
            started_rx.recv().unwrap();
            slot.set(Some(handle));
        });

        retire_verified_source(admit(&source, &target), &source, &target).unwrap();
        writer.take().unwrap().join().unwrap();

        assert_eq!(
            fs::read(source.join("events/late-writer.json")).unwrap(),
            b"{\"late\":true}"
        );
    }

    /// Retirement must not unlink the lock file: a writer that opened the
    /// original lock before retirement finished would then hold one inode while
    /// a newcomer creates and locks another at the same path.
    #[test]
    fn retirement_keeps_the_authority_lock_exclusive() {
        let (_repo, source, _target_root, target) = folded_pair();
        let lock_path = source.join(STORE_AUTHORITY_LOCK_FILE);
        let (locked_tx, locked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let waiter: std::rc::Rc<std::cell::Cell<Option<std::thread::JoinHandle<()>>>> =
            std::rc::Rc::default();
        let slot = waiter.clone();
        let _hook = install_after_verify_hook(move |_| {
            let (opened_tx, opened_rx) = mpsc::channel();
            let handle = std::thread::spawn(move || {
                let file = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&lock_path)
                    .unwrap();
                opened_tx.send(()).unwrap();
                file.lock().unwrap();
                locked_tx.send(()).unwrap();
                let _ = release_rx.recv();
                drop(file);
            });
            opened_rx.recv().unwrap();
            slot.set(Some(handle));
        });

        let outcome = retire_verified_source(admit(&source, &target), &source, &target).unwrap();
        locked_rx.recv().unwrap();

        assert!(outcome.source_retired);
        let contender = source.clone();
        let contender_acquired = std::thread::spawn(move || {
            StoreAuthorityLock::try_acquire(&contender)
                .unwrap()
                .is_some()
        })
        .join()
        .unwrap();
        assert!(
            !contender_acquired,
            "a newcomer must not acquire while the waiter holds the original lock"
        );
        assert_only_the_lock_remains(&source);
        release_tx.send(()).unwrap();
        waiter.take().unwrap().join().unwrap();
    }

    /// Files created after the scan are not in the deletion list, so they
    /// survive the deletion passes while every verified path goes.
    #[test]
    fn late_files_during_deletion_survive() {
        let (_repo, source, _target_root, target) = folded_pair();
        let before = snapshot(&source);
        let _hook = install_after_scan_hook(|source| {
            fs::write(source.join("events/late-event.json"), b"{\"late\":1}").unwrap();
            fs::write(source.join("artifacts/objects/late-object"), b"late").unwrap();
        });

        let outcome = retire_verified_source(admit(&source, &target), &source, &target).unwrap();

        assert!(!outcome.source_retired);
        assert!(outcome.residue_entries >= 2);
        assert_eq!(
            fs::read(source.join("events/late-event.json")).unwrap(),
            b"{\"late\":1}"
        );
        assert_eq!(
            fs::read(source.join("artifacts/objects/late-object")).unwrap(),
            b"late"
        );
        for path in before.keys() {
            if path.starts_with("events") || path.starts_with("artifacts") {
                assert!(
                    !source.join(path).exists(),
                    "{} was verified",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn only_the_lock_file_counts_as_retired() {
        let (_repo, source, _target_root, target) = folded_pair();
        let _hook = install_after_scan_hook(|source| {
            fs::write(source.join("stray"), b"late").unwrap();
        });

        let outcome = retire_verified_source(admit(&source, &target), &source, &target).unwrap();

        assert!(!outcome.source_retired);
        assert_eq!(outcome.residue_entries, 1);
        assert!(source.join("stray").exists());
    }
}
