//! Consent-gated, non-destructive fold of a worktree-local `.pointbreak/data` store
//! into the common-dir store (`<git-common-dir>/shore`).
//!
//! This is the user's path across the shared-default flip: it copies events and
//! artifacts forward via `import_store_bundle` (content-addressed, idempotent,
//! source untouched) so a worktree's prior captures are reachable from the common
//! dir. It never deletes the source BY DEFAULT — the opt-in retire-source
//! completion deletes only the source files an independent re-verification
//! proves present in the shared store. It NEVER registers anything
//! (registration is retired) and NEVER runs on a hot path — only the
//! `pointbreak store migrate` subcommand / `just migrate-store-common-dir` driver
//! invoke it. It REFUSES an ephemeral or scanned-sensitive worktree unless the
//! caller passes an explicit override, so sensitive throwaway bytes are never
//! silently fanned into the shared store.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Result, ShoreError};
use crate::session::derived_access::product_contract::DerivedAccessProfile;
use crate::session::store::bundle::{
    ImportBundleResult, import_store_bundle_into_with_verification,
};
use crate::session::store::resolution::{clone_local_store_dir, event_store_for_explicit_target};
use crate::session::store::sensitivity::scan_worktree_sensitivity;
use crate::session::store::source_retirement::{
    SOURCE_BUSY_PREFIX, SourceRetirementAdmission, SourceRetirementGuard, SourceRetirementOutcome,
    admit_source_retirement, retire_verified_source,
};
use crate::session::store::store_config::{StoreMode, resolve_store_mode};
use crate::session::store::store_init::RepositoryPaths;
use crate::session::{
    EventVerificationPolicy, ProjectionDiagnostic, TrustSet, WriteAcknowledgementV1,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrateToCommonDirOptions {
    repo: PathBuf,
    include_ephemeral: bool,
    retire_source: bool,
}

impl MigrateToCommonDirOptions {
    pub fn new(repo: impl AsRef<Path>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            include_ephemeral: false,
            retire_source: false,
        }
    }

    /// Opt in to migrating an ephemeral / scanned-sensitive worktree. Off by
    /// default: the migration refuses such a worktree without this override
    /// (no silent fan-in of sensitive bytes into the shared store).
    pub fn with_include_ephemeral(mut self, include_ephemeral: bool) -> Self {
        self.include_ephemeral = include_ephemeral;
        self
    }

    /// Opt in to retiring the worktree-local `.pointbreak/data` after the fold is
    /// independently verified (every source event and artifact file present in
    /// the shared store; see `verify_source_subset_of_target`), so reads
    /// resolve in one command. Retirement deletes only the verified files and
    /// keeps the directory with its authority lock file. Off by default: the
    /// source is never discarded before the migration is confirmed.
    pub fn with_retire_source(mut self, retire_source: bool) -> Self {
        self.retire_source = retire_source;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateToCommonDirResult {
    pub acknowledgement: WriteAcknowledgementV1,
    pub diagnostics: Vec<ProjectionDiagnostic>,
    pub events_created: usize,
    pub events_existing: usize,
    pub artifacts_created: usize,
    pub artifacts_existing: usize,
    /// True when the source had nothing to migrate (no worktree-local store).
    /// Only reported once the consent gate has passed: an ephemeral/sensitive
    /// worktree is refused first, even when its source store is empty, so a
    /// refusal is never silently downgraded to a `sourceEmpty` no-op.
    pub source_empty: bool,
    /// True when `--retire-source` removed every verified record and disposable
    /// entry from the worktree-local `.pointbreak/data` (after a verified fold, or
    /// as a no-durable-files husk) and nothing but the store directory and its
    /// authority lock file remains. Those two are kept on purpose so a writer
    /// waiting on the lock never races a newly created one.
    pub source_retired: bool,
    /// Files the retire verification confirmed in the shared store; zero when
    /// the retire was not requested or nothing needed verifying.
    pub verified_events: usize,
    pub verified_artifacts: usize,
    /// Referenced artifacts absent from the source with no removal claim (old
    /// snapshots GC'd/migrated away). The migrate carried the referencing events
    /// without their content; the CLI discloses this when > 0.
    pub absent_artifact_count: usize,
    /// Unique paths the consent-gate sensitivity scan skipped via the
    /// configured exclude globs — `Some` whenever the gate scan ran (audit for
    /// the targeted opt-out), `None` when `--include-ephemeral` skipped the
    /// scan (absent, not zero: zero would claim a scan ran).
    pub sensitivity_excluded_path_count: Option<usize>,
}

/// The sentinel `scan_worktree_sensitivity` emits for a worktree that must not be
/// fanned into the shared store without an explicit override.
const SENSITIVITY_BLOCK: &str = "block";

pub fn migrate_store_to_common_dir(
    options: MigrateToCommonDirOptions,
) -> Result<MigrateToCommonDirResult> {
    let paths = RepositoryPaths::resolve(&options.repo)?;
    let worktree_root = paths.worktree_root().to_path_buf();
    let source = paths.worktree_store().to_path_buf(); // worktree-local .pointbreak/data

    // Consent gate: refuse an ephemeral or scanned-sensitive worktree unless the
    // caller explicitly opted in. Checked BEFORE any write to the common dir, and
    // deliberately before the missing-source no-op below: a refusal is uniform for
    // an ephemeral worktree and is never downgraded to a `source_empty` success.
    let mut sensitivity_excluded_path_count = None;
    if !options.include_ephemeral {
        if resolve_store_mode(&worktree_root)? == StoreMode::Ephemeral {
            return Err(ShoreError::Message(
                "refusing to migrate an ephemeral worktree into the shared store; \
                 re-run with the include-ephemeral override to fan it in"
                    .to_owned(),
            ));
        }
        let scan = scan_worktree_sensitivity(&worktree_root)?;
        if scan.policy_outcome == SENSITIVITY_BLOCK {
            return Err(ShoreError::Message(
                "refusing to migrate a worktree flagged sensitive into the shared store; run \
                 `pointbreak store status --show-paths` to see which files matched, then add \
                 known-safe paths to .pointbreak/sensitivity.json excludeGlobs for a targeted \
                 exclude, or re-run with the include-ephemeral override to fan it in wholesale"
                    .to_owned(),
            ));
        }
        sensitivity_excluded_path_count = Some(scan.excluded_path_count);
    }

    // The retire path classifies the source by PHYSICAL FILE COUNTS, never
    // directory existence: the writer pre-creates empty events/ and
    // artifacts/… dirs, so a directory-existence test would misroute an
    // empty-events-plus-orphan-artifacts source down the populated path and
    // delete unverified bytes.
    if options.retire_source {
        match classify_retire_source(&source)? {
            RetireSourceShape::Populated => {} // fold, verify, then delete below
            RetireSourceShape::Husk => {
                let mut result = MigrateToCommonDirResult {
                    acknowledgement: WriteAcknowledgementV1::unchanged(),
                    diagnostics: Vec::new(),
                    events_created: 0,
                    events_existing: 0,
                    artifacts_created: 0,
                    artifacts_existing: 0,
                    source_empty: true,
                    source_retired: false,
                    verified_events: 0,
                    verified_artifacts: 0,
                    absent_artifact_count: 0,
                    sensitivity_excluded_path_count,
                };
                // Nothing to fold, but retirement still deletes only what it
                // recognizes: an entry it does not understand refuses. An
                // absent husk stays absent (admission would create it).
                if source.exists() {
                    let target = clone_local_store_dir(&worktree_root)?;
                    let guard = admit_worktree_store_retirement(&source, &target)?;
                    result.record_retirement(
                        &source,
                        retire_verified_source(guard, &source, &target)?,
                    );
                }
                return Ok(result);
            }
            RetireSourceShape::ArtifactsWithoutEvents => {
                return Err(ShoreError::Message(format!(
                    "refusing to retire {}: it holds artifact files but no event files, so the \
                     fold cannot verify them; inspect the artifacts manually before deleting \
                     anything",
                    source.display()
                )));
            }
        }
    }

    // Nothing to migrate if the worktree has no local store yet.
    if !source.join("events").exists() {
        return Ok(MigrateToCommonDirResult {
            acknowledgement: WriteAcknowledgementV1::unchanged(),
            diagnostics: Vec::new(),
            events_created: 0,
            events_existing: 0,
            artifacts_created: 0,
            artifacts_existing: 0,
            source_empty: true,
            source_retired: false,
            verified_events: 0,
            verified_artifacts: 0,
            absent_artifact_count: 0,
            sensitivity_excluded_path_count,
        });
    }

    // Source is resolved via the raw `RepositoryPaths::resolve` and the target via
    // `clone_local_store_dir` (= `<git-common-dir>/shore`); both are reused, neither
    // recomputed. `import_store_bundle` only reads the source — by default this fn
    // performs no `remove`/`remove_dir` on it; the opt-in retire below deletes only
    // the source files an independent re-verification proves present in the
    // target. (The in-place flat-store relocation is a different migration and
    // must not be conflated.)
    let target = clone_local_store_dir(&worktree_root)?;
    // Retirement takes the source store's authority lock before the fold reads
    // it and holds it through deletion, so no Pointbreak writer can land a record
    // between the fold and the verification.
    let retirement = options
        .retire_source
        .then(|| admit_worktree_store_retirement(&source, &target))
        .transpose()?;
    let profile = DerivedAccessProfile::from_environment()
        .map_err(|error| ShoreError::Message(error.to_string()))?;
    let target_event_store = event_store_for_explicit_target(&target, profile)?;
    let imported = import_store_bundle_into_with_verification(
        &source,
        &target,
        &target_event_store,
        EventVerificationPolicy::advisory(),
        TrustSet::default(),
    )?;
    let mut result = MigrateToCommonDirResult::from_import(imported);
    result.sensitivity_excluded_path_count = sensitivity_excluded_path_count;
    if let Some(guard) = retirement {
        result.record_retirement(&source, retire_verified_source(guard, &source, &target)?);
    }
    Ok(result)
}

/// Take the worktree-local store's authority lock for retirement without
/// waiting, or refuse while another Pointbreak writer holds it.
fn admit_worktree_store_retirement(source: &Path, target: &Path) -> Result<SourceRetirementGuard> {
    match admit_source_retirement(source, target)? {
        SourceRetirementAdmission::Admitted(guard) => Ok(guard),
        SourceRetirementAdmission::Busy => Err(ShoreError::Message(format!(
            "{SOURCE_BUSY_PREFIX} another Pointbreak writer holds the worktree-local store {}; \
             nothing was folded or deleted — retry once it finishes",
            source.display()
        ))),
    }
}

/// The retire-path classification of a worktree-local source store, by durable
/// file counts under `events/` and `artifacts/` (excluding in-flight `*.tmp`
/// files; a leftover store-root `state.json` is a sibling of those trees
/// and is never counted — a nested file merely NAMED `state.json` is durable).
enum RetireSourceShape {
    /// Event files present: fold, verify, then delete.
    Populated,
    /// No durable files at all (absent dir, or only state.json/temp/empty
    /// dirs): removable without a fold.
    Husk,
    /// Artifact files but no event files: never silently deletable.
    ArtifactsWithoutEvents,
}

fn classify_retire_source(source: &Path) -> Result<RetireSourceShape> {
    let events = count_durable_files(&source.join("events"))?;
    let artifacts = count_durable_files(&source.join("artifacts"))?;
    Ok(match (events, artifacts) {
        (0, 0) => RetireSourceShape::Husk,
        (0, _) => RetireSourceShape::ArtifactsWithoutEvents,
        _ => RetireSourceShape::Populated,
    })
}

/// Count durable files under `dir` recursively; a missing dir counts zero.
/// Skips only in-flight `*.tmp` files, mirroring `verify_source_subset_of_target`.
fn count_durable_files(dir: &Path) -> Result<usize> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(ShoreError::Message(format!(
                "read store directory {} for retire classification: {error}",
                dir.display()
            )));
        }
    };
    let mut count = 0;
    for entry in entries {
        let entry = entry.map_err(|error| {
            ShoreError::Message(format!(
                "read store directory entry under {} for retire classification: {error}",
                dir.display()
            ))
        })?;
        if entry.path().is_dir() {
            count += count_durable_files(&entry.path())?;
        } else if !entry.file_name().to_string_lossy().ends_with(".tmp") {
            count += 1;
        }
    }
    Ok(count)
}

impl MigrateToCommonDirResult {
    fn from_import(imported: ImportBundleResult) -> Self {
        Self {
            acknowledgement: imported.acknowledgement,
            diagnostics: imported.diagnostics,
            events_created: imported.events_created,
            events_existing: imported.events_existing,
            artifacts_created: imported.artifacts_created,
            artifacts_existing: imported.artifacts_existing,
            source_empty: false,
            source_retired: false,
            verified_events: 0,
            verified_artifacts: 0,
            absent_artifact_count: imported.absent_artifact_count,
            sensitivity_excluded_path_count: None,
        }
    }

    /// Copy a retirement outcome onto the result. Files retirement kept
    /// become one diagnostic asking for a rerun.
    fn record_retirement(&mut self, source: &Path, retired: SourceRetirementOutcome) {
        self.source_retired = retired.source_retired;
        self.verified_events = retired.verified_events;
        self.verified_artifacts = retired.verified_artifacts;
        if retired.residue_entries > 0 {
            self.diagnostics.push(ProjectionDiagnostic {
                code: "source_retirement_residue".to_owned(),
                message: format!(
                    "the worktree-local store {} was not retired: {} file(s) appeared after \
                     verification or could not be removed and were kept; rerun the migrate with \
                     --retire-source to fold and retire them",
                    source.display(),
                    retired.residue_entries
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{MigrateToCommonDirOptions, migrate_store_to_common_dir};
    use crate::git::git_common_dir;
    use crate::session::store::authority_lock::{STORE_AUTHORITY_LOCK_FILE, StoreAuthorityLock};
    use crate::session::store::resolution::{clone_local_store_dir, resolve_store};
    use crate::session::store::source_retirement::install_after_verify_hook;
    use crate::session::store::store_config::{StoreMode, write_store_config};
    use crate::session::store::store_init::worktree_local_store_is_populated;
    use crate::session::{CaptureOptions, EventStore, capture_worktree_review};

    struct TestRepo {
        root: tempfile::TempDir,
    }

    impl TestRepo {
        fn new() -> Self {
            let root = tempfile::tempdir().expect("create temp git repository directory");
            let repo = Self { root };
            repo.git(["init"]);
            repo.git(["config", "user.name", "Shore Tests"]);
            repo.git(["config", "user.email", "shore-tests@example.com"]);
            repo.git(["config", "commit.gpgsign", "false"]);
            repo
        }

        fn path(&self) -> &Path {
            self.root.path()
        }

        fn write(&self, path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
            let path = self.root.path().join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent directories");
            }
            fs::write(path, contents).expect("write test repository file");
        }

        fn commit_all(&self, message: &str) {
            self.git(["add", "--all"]);
            self.git(["commit", "-m", message]);
        }

        fn git<I, S>(&self, args: I)
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let args = args
                .into_iter()
                .map(|arg| arg.as_ref().to_owned())
                .collect::<Vec<_>>();
            let output = Command::new("git")
                .args(&args)
                .current_dir(self.root.path())
                .output()
                .unwrap_or_else(|error| panic!("run git {args:?}: {error}"));
            assert!(
                output.status.success(),
                "git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
    }

    fn modified_repo() -> TestRepo {
        let repo = TestRepo::new();
        repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
        repo.commit_all("base");
        repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
        repo
    }

    /// Seed a pre-shared-default capture: a populated worktree-local `.pointbreak/data`
    /// store, which is exactly the source `pointbreak store migrate` folds forward. We
    /// capture under ephemeral mode (so the write lands in `.pointbreak/data`), then
    /// restore the default Shared mode so the migration runs against a
    /// non-ephemeral worktree carrying a legacy worktree-local store.
    fn seed_worktree_local_capture(repo: &TestRepo) {
        write_store_config(repo.path(), StoreMode::Ephemeral).unwrap();
        capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        write_store_config(repo.path(), StoreMode::Shared).unwrap();
        assert!(
            repo.path().join(".pointbreak/data/events").is_dir(),
            "the seed lands a worktree-local store to migrate"
        );
    }

    /// Record one more observation into the worktree-local store, the way a
    /// writer that still resolves that store would. Runs on the calling thread,
    /// so it can land while this thread holds the store's authority lock.
    fn record_worktree_local_observation(repo: &Path) {
        write_store_config(repo, StoreMode::Ephemeral).unwrap();
        crate::session::record_observation(
            crate::session::ObservationAddOptions::new(repo)
                .with_track("agent:late-writer")
                .with_title("Late observation")
                .with_body("written after verification"),
        )
        .unwrap();
        write_store_config(repo, StoreMode::Shared).unwrap();
    }

    /// Retirement keeps the store directory and its authority lock file, so the
    /// lock's identity never changes under a waiting writer; nothing else stays.
    fn assert_only_the_lock_remains(store: &Path) {
        let remaining = fs::read_dir(store)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(
            remaining,
            vec![std::ffi::OsString::from(STORE_AUTHORITY_LOCK_FILE)],
            "only the authority lock file is left after a retire"
        );
    }

    /// Every file under `root` with its bytes, keyed by root-relative path. The
    /// store-root authority lock file is left out: it holds no data, and on
    /// Windows it cannot be read while another handle holds its lock.
    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                let relative = path.strip_prefix(root).unwrap().to_path_buf();
                if path.is_dir() {
                    walk(&path, root, out);
                } else if relative != Path::new(STORE_AUTHORITY_LOCK_FILE) {
                    out.insert(relative, fs::read(&path).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(root, root, &mut out);
        out
    }

    #[test]
    fn folds_worktree_local_store_into_common_dir_non_destructively() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);
        let local = repo.path().join(".pointbreak/data");
        let common = git_common_dir(repo.path()).unwrap().join("pointbreak");
        assert!(
            !common.join("events").exists(),
            "common-dir store has no events before migration"
        );

        let result =
            migrate_store_to_common_dir(MigrateToCommonDirOptions::new(repo.path())).unwrap();

        // Events + the object artifact landed in the common dir.
        assert!(result.events_created >= 1);
        assert!(result.artifacts_created >= 1);
        assert!(common.join("events").is_dir());
        assert!(common.join("artifacts/objects").is_dir());
        assert!(!common.join("state.json").exists());
        // Source is NEVER deleted (non-destructive).
        assert!(local.join("events").is_dir());
        let source_events = EventStore::open(&local).list_events().unwrap();
        assert!(!source_events.is_empty());
    }

    #[test]
    fn re_run_is_idempotent_and_reports_existing() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);

        let first =
            migrate_store_to_common_dir(MigrateToCommonDirOptions::new(repo.path())).unwrap();
        let second =
            migrate_store_to_common_dir(MigrateToCommonDirOptions::new(repo.path())).unwrap();

        assert!(first.events_created >= 1);
        assert_eq!(second.events_created, 0, "nothing new on re-run");
        assert!(
            second.events_existing >= 1,
            "re-run reports the already-present events"
        );
        assert_eq!(second.artifacts_created, 0);
        assert!(second.artifacts_existing >= 1);
    }

    #[test]
    fn refuses_an_ephemeral_worktree_without_include_ephemeral() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);
        // Mark the worktree ephemeral via the store-config writer.
        write_store_config(repo.path(), StoreMode::Ephemeral).unwrap();

        let error = migrate_store_to_common_dir(MigrateToCommonDirOptions::new(repo.path()))
            .expect_err("an ephemeral worktree must refuse without an explicit override");

        assert!(
            error.to_string().contains("ephemeral"),
            "the refusal names the ephemeral opt-out: {error}"
        );
        // Refused before any write to the common dir.
        let common = git_common_dir(repo.path()).unwrap().join("pointbreak");
        assert!(
            !common.join("events").exists(),
            "no fan-in happened on a refused ephemeral migration"
        );
    }

    #[test]
    fn include_ephemeral_override_migrates_an_ephemeral_worktree() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);
        write_store_config(repo.path(), StoreMode::Ephemeral).unwrap();

        let result = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_include_ephemeral(true),
        )
        .unwrap();

        assert!(result.events_created >= 1);
    }

    #[test]
    fn ephemeral_empty_worktree_refuses_before_reporting_source_empty() {
        // An ephemeral worktree with no local store yet is refused (the consent gate
        // runs before the missing-source no-op), so the refusal is uniform and never
        // downgraded to a `source_empty` success. The override then reports the empty
        // source honestly.
        let repo = TestRepo::new();
        repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
        repo.commit_all("base");
        write_store_config(repo.path(), StoreMode::Ephemeral).unwrap();
        assert!(!repo.path().join(".pointbreak/data/events").exists());

        let error = migrate_store_to_common_dir(MigrateToCommonDirOptions::new(repo.path()))
            .expect_err("an ephemeral worktree refuses even with an empty source store");
        assert!(error.to_string().contains("ephemeral"));

        let overridden = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_include_ephemeral(true),
        )
        .unwrap();
        assert!(
            overridden.source_empty,
            "an empty source reports sourceEmpty once consent passes"
        );
        assert_eq!(overridden.events_created, 0);
    }

    #[test]
    fn retire_source_deletes_the_source_after_a_verified_fold() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);

        let result = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .unwrap();

        assert!(result.source_retired);
        assert!(result.verified_events >= 1);
        assert!(result.verified_artifacts >= 1);
        assert_only_the_lock_remains(&repo.path().join(".pointbreak/data"));
        // The committed config siblings under .pointbreak/ survive.
        assert!(repo.path().join(".pointbreak/store.json").is_file());
    }

    #[test]
    fn migrate_acknowledgement_leaves_a_leftover_target_state_json_inert() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);
        let target = git_common_dir(repo.path()).unwrap().join("pointbreak");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("state.json"), "leftover").unwrap();
        let result = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .unwrap();
        assert!(result.source_retired);
        assert!(result.verified_events > 0 && result.verified_artifacts > 0);
        assert_eq!(
            result.acknowledgement.legacy_projection_state,
            crate::session::LegacyProjectionStateV1::NotAttempted
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.code != "legacy_state_projection_refresh_failed")
        );
        // The fold neither refreshes, reads nor removes the target's leftover file.
        assert_eq!(fs::read(target.join("state.json")).unwrap(), b"leftover");
    }

    #[test]
    fn default_migrate_still_never_deletes_the_source() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);

        let result =
            migrate_store_to_common_dir(MigrateToCommonDirOptions::new(repo.path())).unwrap();

        assert!(!result.source_retired);
        assert_eq!(result.verified_events, 0);
        assert_eq!(result.verified_artifacts, 0);
        assert!(repo.path().join(".pointbreak/data/events").is_dir());
    }

    #[test]
    fn retire_source_refuses_and_preserves_source_when_verification_fails() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);
        // Poison the fold with an ENVELOPE-only divergent copy of one source
        // event pre-planted in the target (same idempotency key and payload,
        // different occurredAt). Payload divergence would fail at import
        // preflight instead; envelope divergence dedups to the first-stored
        // target record, so only the verification can catch it.
        let source = repo.path().join(".pointbreak/data");
        let common = git_common_dir(repo.path()).unwrap().join("pointbreak");
        let name = EventStore::open(&source)
            .list_event_file_names()
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(source.join("events").join(&name)).unwrap()).unwrap();
        value["occurredAt"] = serde_json::Value::String("2020-01-01T00:00:00Z".to_owned());
        fs::create_dir_all(common.join("events")).unwrap();
        fs::write(
            common.join("events").join(&name),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();

        fs::create_dir_all(common.join("state.json")).unwrap();
        let error = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .expect_err("a divergent target must fail verification and refuse the retire");

        assert!(
            error.to_string().contains("not deleted") || error.to_string().contains("left"),
            "the error says the source survives: {error}"
        );
        assert!(
            repo.path().join(".pointbreak/data/events").is_dir(),
            "source untouched"
        );
    }

    #[test]
    fn retire_source_removes_a_husk_source_with_no_durable_files() {
        let repo = modified_repo();
        // A .pointbreak/data holding only a stale state.json plus the EMPTY dirs the
        // writer pre-creates trips the populated guard but has nothing durable:
        // state.json is an inert leftover, the dirs are empty.
        // Classification is by FILE COUNT, so the empty events/ dir must not
        // route this down the populated path.
        fs::create_dir_all(repo.path().join(".pointbreak/data/events")).unwrap();
        fs::create_dir_all(repo.path().join(".pointbreak/data/artifacts/objects")).unwrap();
        fs::write(repo.path().join(".pointbreak/data/state.json"), "{}").unwrap();

        let result = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .unwrap();

        assert!(result.source_retired);
        assert_only_the_lock_remains(&repo.path().join(".pointbreak/data"));
    }

    #[test]
    fn retire_source_refuses_orphan_artifacts_under_an_empty_events_dir() {
        // The regression the plan review caught: the writer pre-creates events/,
        // so a directory-existence husk test would send this source down the
        // populated path — zero events means the fold verifies nothing — and
        // then delete the orphan artifact bytes unverified.
        let repo = modified_repo();
        fs::create_dir_all(repo.path().join(".pointbreak/data/events")).unwrap();
        fs::create_dir_all(repo.path().join(".pointbreak/data/artifacts/objects")).unwrap();
        fs::write(
            repo.path()
                .join(".pointbreak/data/artifacts/objects/orphan.json"),
            "{}",
        )
        .unwrap();

        let error = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .expect_err("artifact files without event files must never be silently deleted");
        assert!(error.to_string().contains("artifact"));
        assert!(
            repo.path()
                .join(".pointbreak/data/artifacts/objects/orphan.json")
                .is_file(),
            "the unverified bytes survive"
        );
    }

    #[test]
    fn retire_source_refuses_a_nested_file_named_state_json_under_artifacts() {
        // Only the STORE-ROOT state.json is an inert leftover; a nested file merely
        // NAMED state.json is durable bytes — it must classify as an artifact
        // file (refusal), never be skipped as a husk and deleted unverified.
        let repo = modified_repo();
        fs::create_dir_all(repo.path().join(".pointbreak/data/events")).unwrap();
        fs::create_dir_all(repo.path().join(".pointbreak/data/artifacts/objects")).unwrap();
        fs::write(
            repo.path()
                .join(".pointbreak/data/artifacts/objects/state.json"),
            "{}",
        )
        .unwrap();

        let error = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .expect_err("a nested state.json is durable bytes, never silently deletable");
        assert!(error.to_string().contains("artifact"));
        assert!(
            repo.path()
                .join(".pointbreak/data/artifacts/objects/state.json")
                .is_file(),
            "the unverified bytes survive"
        );
    }

    #[test]
    fn retire_source_refuses_a_source_with_artifacts_but_no_events_dir() {
        let repo = modified_repo();
        fs::create_dir_all(repo.path().join(".pointbreak/data/artifacts/objects")).unwrap();
        fs::write(
            repo.path()
                .join(".pointbreak/data/artifacts/objects/orphan.json"),
            "{}",
        )
        .unwrap();

        let error = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .expect_err("artifacts without events must never be silently deleted");
        assert!(error.to_string().contains("artifact"));
        assert!(repo.path().join(".pointbreak/data/artifacts").is_dir());
    }

    #[test]
    fn retire_source_refuses_a_populated_source_carrying_an_orphan_artifact() {
        // A populated source whose artifacts/ holds a file no event references:
        // the fold does not carry it, so the physical-walk verification finds it
        // missing in the target and the retire refuses rather than deleting the
        // only copy.
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);
        fs::write(
            repo.path()
                .join(".pointbreak/data/artifacts/objects/orphan.json"),
            "{}",
        )
        .unwrap();

        let error = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .expect_err("an orphan artifact must fail verification and block the retire");
        assert!(
            error.to_string().contains("orphan.json"),
            "names the file: {error}"
        );
        assert!(
            repo.path().join(".pointbreak/data/events").is_dir(),
            "source untouched on refusal"
        );
    }

    #[test]
    fn retire_source_still_respects_the_ephemeral_refusal() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);
        write_store_config(repo.path(), StoreMode::Ephemeral).unwrap();

        let error = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .expect_err("the consent gate fires before any fold or retire");
        assert!(error.to_string().contains("ephemeral"));
        assert!(repo.path().join(".pointbreak/data/events").is_dir());
    }

    #[test]
    fn source_shore_data_is_never_deleted_by_migration() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);
        let local = repo.path().join(".pointbreak/data");
        let before = EventStore::open(&local).list_event_file_names().unwrap();

        migrate_store_to_common_dir(MigrateToCommonDirOptions::new(repo.path())).unwrap();

        let after = EventStore::open(&local).list_event_file_names().unwrap();
        assert_eq!(before, after, "the source store is byte-for-byte preserved");
    }

    #[test]
    fn retire_source_refuses_a_busy_worktree_store() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);
        let source = repo.path().join(".pointbreak/data");
        let common = git_common_dir(repo.path()).unwrap().join("pointbreak");
        let (held_tx, held_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let holder_source = source.clone();
        let holder = std::thread::spawn(move || {
            let _lock = StoreAuthorityLock::acquire(&holder_source).unwrap();
            held_tx.send(()).unwrap();
            let _ = release_rx.recv();
        });
        held_rx.recv().unwrap();
        let before = snapshot(&source);

        let result = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        );
        release_tx.send(()).unwrap();
        holder.join().unwrap();

        let message = result
            .expect_err("a busy worktree-local store must refuse")
            .to_string();
        assert!(message.starts_with("source_busy;"), "{message}");
        assert_eq!(snapshot(&source), before, "the source is untouched");
        assert!(
            !common.join("events").exists(),
            "nothing was folded into the shared store"
        );
    }

    #[test]
    fn retire_reports_residue_without_error() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);
        let source = repo.path().join(".pointbreak/data");
        let before = snapshot(&source);
        let repo_path = repo.path().to_path_buf();
        let _hook = install_after_verify_hook(move |_| {
            record_worktree_local_observation(&repo_path);
        });

        let result = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .unwrap();

        assert!(!result.source_retired);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "source_retirement_residue"),
            "{:?}",
            result.diagnostics
        );
        let after = snapshot(&source);
        for (path, bytes) in &before {
            assert_eq!(after.get(path), Some(bytes), "{} was kept", path.display());
        }
        let late_events = after
            .keys()
            .filter(|path| path.starts_with("events") && !before.contains_key(*path))
            .count();
        assert_eq!(late_events, 1, "the late event is kept");

        let rerun = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .unwrap();
        assert!(rerun.source_retired, "the rerun folds the late event first");
        assert_only_the_lock_remains(&source);
    }

    #[test]
    fn husk_retire_never_recurses() {
        let repo = modified_repo();
        let source = repo.path().join(".pointbreak/data");
        fs::create_dir_all(source.join("events")).unwrap();
        fs::create_dir_all(source.join("operations")).unwrap();
        fs::write(source.join("operations/op.json"), b"{}").unwrap();

        let error = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .expect_err("an entry retirement does not understand must refuse");

        assert!(error.to_string().contains("operations/op.json"), "{error}");
        assert_eq!(fs::read(source.join("operations/op.json")).unwrap(), b"{}");
    }

    #[test]
    fn husk_with_only_derived_entries_retires() {
        let repo = modified_repo();
        let source = repo.path().join(".pointbreak/data");
        fs::create_dir_all(source.join("derived/generation")).unwrap();
        fs::write(
            source.join("derived/generation/index.sqlite"),
            b"rebuildable",
        )
        .unwrap();
        fs::write(source.join("derived.writer.lock"), b"").unwrap();
        let options = || MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true);

        let result = migrate_store_to_common_dir(options()).unwrap();

        assert!(result.source_retired);
        assert!(result.source_empty);
        assert_only_the_lock_remains(&source);

        let rerun = migrate_store_to_common_dir(options()).unwrap();
        assert!(rerun.source_retired, "a lock-only store is a retired husk");
        assert!(rerun.diagnostics.is_empty(), "{:?}", rerun.diagnostics);
        assert_only_the_lock_remains(&source);
        let resolution = resolve_store(repo.path()).unwrap();
        assert_eq!(
            resolution.store_dir(),
            clone_local_store_dir(repo.path()).unwrap(),
            "reads resolve the shared store"
        );
    }

    #[test]
    fn retired_worktree_store_does_not_read_as_populated() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);
        let source = repo.path().join(".pointbreak/data");

        let result = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .unwrap();

        assert!(result.source_retired);
        assert!(!worktree_local_store_is_populated(&source));
        let resolution = resolve_store(repo.path()).unwrap();
        assert_eq!(
            resolution.store_dir(),
            clone_local_store_dir(repo.path()).unwrap()
        );
    }

    #[test]
    fn residue_keeps_the_worktree_store_populated_and_loud() {
        let repo = modified_repo();
        seed_worktree_local_capture(&repo);
        let source = repo.path().join(".pointbreak/data");
        let _hook = install_after_verify_hook(|source| {
            fs::write(source.join("artifacts/objects/late"), b"late content").unwrap();
        });

        let result = migrate_store_to_common_dir(
            MigrateToCommonDirOptions::new(repo.path()).with_retire_source(true),
        )
        .unwrap();

        assert!(!result.source_retired);
        assert!(worktree_local_store_is_populated(&source));
        let message = resolve_store(repo.path())
            .expect_err("a store left populated keeps the transfer refusal")
            .to_string();
        assert!(
            message.contains("change_store_transfer_unavailable"),
            "{message}"
        );
    }
}
