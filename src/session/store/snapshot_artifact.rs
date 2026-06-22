use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::canonical_hash::sha256_json_prefixed;
use crate::error::{Result, ShoreError};
use crate::model::{DiffSnapshot, ObjectId};
use crate::session::store::resolution::resolve_read_store;
use crate::session::{RevisionFingerprint, ShoreStorePaths};
use crate::storage::{CreateFileOutcome, Durability, LocalStorage};

const SNAPSHOT_ARTIFACT_SCHEMA: &str = "shore.snapshot";
const SNAPSHOT_ARTIFACT_VERSION: u32 = 2;

/// The snapshot-scoped v2 artifact body (#146). It carries only namespace-
/// independent content, so two worktrees capturing the same `snapshot_id`
/// produce **byte-identical** artifacts that dedup. Revision identity and
/// endpoints (`revision_id`/`source`/`base`/`target`) live in the
/// `WorkObjectProposed` event/projection, never here (INV-1/INV-3).
///
/// All writes and reads are v2. The legacy dual-read that also accepted
/// identity-bearing v1 bodies has been removed: [`decode_and_validate_snapshot_artifact`]
/// now rejects any non-v2 body, and the one-shot store migrator re-emits every
/// artifact as a clean v2 body, so no v1 artifact survives to be read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotArtifact {
    pub schema: String,
    pub version: u32,
    pub snapshot: DiffSnapshot,
    pub content_hash: String,
}

/// Write a snapshot artifact to an explicit store dir (the resolved write store).
/// Capture resolves the write store once for the whole landing (artifact → event
/// → `state.json` all target the same dir). The content-addressed
/// exclusive-create write is idempotent: a byte-identical artifact already
/// present returns `Ok` (INV-2/INV-3); a different artifact under the same path is
/// a loud conflict.
pub(crate) fn write_snapshot_artifact_to(
    store_dir: &Path,
    fingerprint: &RevisionFingerprint,
    snapshot: DiffSnapshot,
) -> Result<SnapshotArtifact> {
    if snapshot.snapshot_id != fingerprint.snapshot_id {
        return Err(ShoreError::Message(format!(
            "snapshot id {} does not match review unit fingerprint {}",
            snapshot.snapshot_id.as_str(),
            fingerprint.snapshot_id.as_str()
        )));
    }

    let artifact = build_snapshot_artifact_v2(snapshot)?;

    let storage = LocalStorage::new(store_dir);
    let path = snapshot_artifact_path(store_dir, &artifact.snapshot.snapshot_id);
    let bytes = serde_json::to_vec(&artifact)?;

    match storage.create_file_exclusive(&path, &bytes, Durability::Durable)? {
        CreateFileOutcome::Created => Ok(artifact),
        CreateFileOutcome::AlreadyExists => {
            // Dedup on snapshot-content match (INV-7): the path is keyed by
            // `snapshot_id`, so an existing valid artifact whose `snapshot` equals
            // ours holds the same content. Two fresh worktrees write byte-identical
            // v2 artifacts and dedup against each other — #146 is fixed. We return
            // the existing artifact, so the capture event binds to the hash on disk.
            let existing_bytes = std::fs::read(&path).map_err(|error| {
                missing_artifact_or_io(error, &artifact.snapshot.snapshot_id, &path)
            })?;
            let existing = decode_and_validate_snapshot_artifact(&existing_bytes)?;
            if existing.snapshot == artifact.snapshot {
                Ok(existing)
            } else {
                Err(ShoreError::Message(format!(
                    "snapshot artifact conflict for {}",
                    artifact.snapshot.snapshot_id.as_str()
                )))
            }
        }
    }
}

/// Build a v2 snapshot-scoped artifact with its content hash filled in. The
/// single place that assembles a [`SnapshotArtifact`] for writing; reuse it so
/// every native v2 capture of the same snapshot produces byte-identical bytes.
pub(crate) fn build_snapshot_artifact_v2(snapshot: DiffSnapshot) -> Result<SnapshotArtifact> {
    let mut artifact = SnapshotArtifact {
        schema: SNAPSHOT_ARTIFACT_SCHEMA.to_owned(),
        version: SNAPSHOT_ARTIFACT_VERSION,
        content_hash: String::new(),
        snapshot,
    };
    artifact.content_hash = snapshot_artifact_content_hash(&artifact)?;
    Ok(artifact)
}

/// Read and hash-validate a stored snapshot artifact.
///
/// Reads resolve through the worktree's resolved store — the shared common-dir
/// store by default, or the worktree-local `.shore/data` store when the worktree
/// is ephemeral.
pub fn read_snapshot_artifact(
    repo: impl AsRef<Path>,
    snapshot_id: &ObjectId,
) -> Result<SnapshotArtifact> {
    let bytes = read_snapshot_artifact_bytes(repo, snapshot_id)?;
    decode_and_validate_snapshot_artifact(&bytes)
}

pub(crate) fn read_snapshot_artifact_bytes(
    repo: impl AsRef<Path>,
    snapshot_id: &ObjectId,
) -> Result<Vec<u8>> {
    let read_store = resolve_read_store(repo.as_ref())?;
    let path = snapshot_artifact_path(read_store.store_dir(), snapshot_id);
    std::fs::read(&path).map_err(|error| missing_artifact_or_io(error, snapshot_id, &path))
}

/// Read a snapshot artifact for WRITE-PATH target validation. Resolves the
/// worktree's store first (matching read surfaces), then falls back to the
/// worktree-local `.shore/data/` when the artifact lives only there — an
/// ephemeral or pre-migration capture the resolved common-dir store does not
/// hold. Both sources are content-addressed and the hash is validated, so the
/// choice is invisible to the caller. This closes a split-brain where a unit's
/// events validate (write-path unit validation reads the resolved store) but its
/// file target could not resolve its artifact from a different store.
pub(crate) fn read_snapshot_artifact_for_write_validation(
    repo: impl AsRef<Path>,
    snapshot_id: &ObjectId,
) -> Result<SnapshotArtifact> {
    let bytes = read_snapshot_artifact_bytes_with_local_fallback(repo, snapshot_id)?;
    decode_and_validate_snapshot_artifact(&bytes)
}

fn read_snapshot_artifact_bytes_with_local_fallback(
    repo: impl AsRef<Path>,
    snapshot_id: &ObjectId,
) -> Result<Vec<u8>> {
    let read_store = resolve_read_store(repo.as_ref())?;
    let resolved_path = snapshot_artifact_path(read_store.store_dir(), snapshot_id);
    match std::fs::read(&resolved_path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Fall back to the worktree-local store: the case where the unit's
            // content-addressed artifact lives only in `.shore/data` (an
            // ephemeral or pre-migration capture) and not in the resolved store.
            let local = ShoreStorePaths::resolve(repo.as_ref())?;
            let local_path = snapshot_artifact_path(local.store_dir(), snapshot_id);
            std::fs::read(&local_path)
                .map_err(|error| missing_artifact_or_io(error, snapshot_id, &local_path))
        }
        Err(error) => Err(missing_artifact_or_io(error, snapshot_id, &resolved_path)),
    }
}

/// Shared error mapping for snapshot-artifact byte reads: a missing file yields
/// the canonical "import referenced artifacts" message; any other I/O error is
/// reported with its path. The read surface and the write-validation fallback
/// differ only in whether a `NotFound` triggers the local fallback before this.
fn missing_artifact_or_io(
    error: std::io::Error,
    snapshot_id: &ObjectId,
    path: &Path,
) -> ShoreError {
    if error.kind() == std::io::ErrorKind::NotFound {
        return ShoreError::Message(format!(
            "missing artifact for snapshot {}; import referenced artifacts before reading",
            snapshot_id.as_str()
        ));
    }
    ShoreError::Message(format!("read file {}: {error}", path.display()))
}

/// The one decode path for stored snapshot-artifact bytes. Strict v2-only:
/// rejects any `version` other than v2 (the snapshot-scoped body), then validates
/// the `contentHash` over the typed v2 struct. The legacy dual-read that also
/// accepted identity-bearing v1 bodies is gone — the one-shot migrator re-emits
/// every artifact as v2, so a v1 body in a migrated store is a stray and is
/// loudly rejected rather than silently accepted.
pub(crate) fn decode_and_validate_snapshot_artifact(bytes: &[u8]) -> Result<SnapshotArtifact> {
    let artifact: SnapshotArtifact = serde_json::from_slice(bytes)?;
    if artifact.version != SNAPSHOT_ARTIFACT_VERSION {
        return Err(ShoreError::Message(format!(
            "unsupported snapshot artifact version {}; only v{SNAPSHOT_ARTIFACT_VERSION} (snapshot-scoped) is supported",
            artifact.version
        )));
    }
    let expected = snapshot_artifact_content_hash(&artifact)?;
    if artifact.content_hash != expected {
        return Err(ShoreError::Message(format!(
            "snapshot artifact content hash mismatch for {}",
            artifact.snapshot.snapshot_id.as_str()
        )));
    }
    Ok(artifact)
}

/// Hash a v2 artifact's body minus `contentHash` (the value [`build_snapshot_artifact_v2`]
/// stamps in). With the snapshot-scoped struct the hashed material is
/// `{schema, version, snapshot}` — namespace-independent (INV-2).
fn snapshot_artifact_content_hash(artifact: &SnapshotArtifact) -> Result<String> {
    let mut material = serde_json::to_value(artifact)?;
    let Some(object) = material.as_object_mut() else {
        return Err(ShoreError::Message(
            "snapshot artifact hash material must be an object".to_owned(),
        ));
    };
    if object.remove("contentHash").is_none() {
        return Err(ShoreError::Message(
            "snapshot artifact hash material is missing contentHash".to_owned(),
        ));
    }

    sha256_json_prefixed(&material)
}

pub(crate) fn snapshot_artifact_path(store_dir: &Path, snapshot_id: &ObjectId) -> PathBuf {
    store_dir
        .join("artifacts/snapshots")
        .join(format!("{}.json", artifact_file_stem(snapshot_id.as_str())))
}

fn artifact_file_stem(id: &str) -> String {
    // Snapshot IDs include a colon-bearing prefix; hashing keeps artifact
    // filenames portable while the artifact body preserves the readable ID.
    crate::canonical_hash::sha256_bytes_hex(id.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use super::*;
    use crate::canonical_hash::sha256_json_prefixed;
    use crate::git::capture_worktree_diff_files;
    use crate::model::{DiffSnapshot, ObjectId, ReviewId};
    use crate::session::store::resolution::resolve_store;
    use crate::session::{
        CaptureOptions, CommitRangeSpec, capture_review, compute_revision_fingerprint,
        read_snapshot_artifact,
    };

    #[test]
    fn snapshot_artifact_schema_is_pinned_at_shore_snapshot_v2() {
        // Native writes are v2. Any future elision-aware artifact must bump one of
        // these constants (see docs/adr/adr-0002-large-snapshot-artifact-policy.md).
        assert_eq!(super::SNAPSHOT_ARTIFACT_SCHEMA, "shore.snapshot");
        assert_eq!(super::SNAPSHOT_ARTIFACT_VERSION, 2);
    }

    #[test]
    fn snapshot_artifact_body_uses_object_id_wire_key() {
        // The stored artifact body finishes Snapshot->Object on the wire: the
        // content-only id serializes under `object_id` (value already `obj:`),
        // not the legacy `snapshot_id` field name.
        let snapshot = DiffSnapshot::new(
            ReviewId::new("review:default"),
            ObjectId::new("obj:sha256:abc"),
            Vec::new(),
        );
        let artifact = build_snapshot_artifact_v2(snapshot).unwrap();

        let json = serde_json::to_value(&artifact.snapshot).unwrap();
        assert!(
            json.get("object_id").is_some(),
            "artifact snapshot body must serialize the object id under `object_id`"
        );
        assert!(
            json.get("snapshot_id").is_none(),
            "the legacy `snapshot_id` wire key must be gone"
        );
        assert!(
            json["object_id"].as_str().unwrap().starts_with("obj:"),
            "the object id value is unchanged (already `obj:`)"
        );
    }

    #[test]
    fn snapshot_artifact_body_is_snapshot_scoped_v2() {
        let repo = modified_repo();
        let artifact = write_current_snapshot_artifact(&repo);

        let json = serde_json::to_value(&artifact).unwrap();
        let keys = json
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys,
            ["contentHash", "schema", "snapshot", "version"]
                .iter()
                .map(|key| key.to_string())
                .collect::<std::collections::BTreeSet<_>>(),
            "v2 body carries no reviewUnitId/source/base/target"
        );
        assert_eq!(artifact.version, 2);
    }

    #[test]
    fn same_range_in_two_repos_produces_byte_identical_artifacts() {
        let repo_a = committed_repo();
        let repo_b = clone_repo(&repo_a); // real clone preserves commit/tree oids

        let a = capture_range(&repo_a, "HEAD~1");
        let b = capture_range(&repo_b, "HEAD~1");

        assert_eq!(a.object_id, b.object_id);
        // A commit-range revision keys off the content object plus the commit/tree
        // provenance, both of which a real clone preserves, so the revision id
        // converges across the two repos (the local repo path never enters it).
        assert_eq!(a.revision_id, b.revision_id);
        assert_eq!(
            a.snapshot_artifact_content_hash,
            b.snapshot_artifact_content_hash
        );

        let bytes_a = fs::read(snapshot_artifact_path(
            &resolved_store_dir(repo_a.path()),
            &a.object_id,
        ))
        .unwrap();
        let bytes_b = fs::read(snapshot_artifact_path(
            &resolved_store_dir(repo_b.path()),
            &b.object_id,
        ))
        .unwrap();
        assert_eq!(
            bytes_a, bytes_b,
            "snapshot-scoped artifacts must be byte-identical"
        );
    }

    #[test]
    fn v1_snapshot_body_is_rejected_after_the_break() {
        // The dual-read is gone: an identity-bearing v1 body no longer decodes;
        // only the snapshot-scoped v2 body does. A clean store carries only v2
        // artifacts (the one-shot migrator re-emits them), so a stray v1 body is a
        // loud rejection, not a silently-accepted legacy shape.
        let repo = modified_repo();
        let artifact = write_current_snapshot_artifact(&repo);
        let path = snapshot_artifact_path(
            &resolved_store_dir(repo.path()),
            &artifact.snapshot.snapshot_id,
        );
        let v1_bytes = rewrite_as_v1(&fs::read(&path).unwrap());

        let error = decode_and_validate_snapshot_artifact(&v1_bytes)
            .expect_err("a v1 body must be rejected after the break");
        assert!(
            error.to_string().contains("v2"),
            "rejection must name the supported v2 shape, got: {error}"
        );
    }

    #[test]
    fn v2_snapshot_body_decodes_cleanly() {
        let repo = modified_repo();
        let artifact = write_current_snapshot_artifact(&repo);
        let path = snapshot_artifact_path(
            &resolved_store_dir(repo.path()),
            &artifact.snapshot.snapshot_id,
        );
        let v2_bytes = fs::read(&path).unwrap();

        let decoded = decode_and_validate_snapshot_artifact(&v2_bytes).unwrap();
        assert_eq!(decoded.version, 2);
        assert_eq!(decoded, artifact);
    }

    #[test]
    fn decode_rejects_a_tampered_v2_artifact() {
        let repo = modified_repo();
        let artifact = write_current_snapshot_artifact(&repo);
        let path = snapshot_artifact_path(
            &resolved_store_dir(repo.path()),
            &artifact.snapshot.snapshot_id,
        );
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        // Mutate a body field without re-stamping the hash.
        value["snapshot"]["files"][0]["new_path"] = serde_json::json!("/evil");

        let error = decode_and_validate_snapshot_artifact(&serde_json::to_vec(&value).unwrap())
            .expect_err("tampered v2 artifact is rejected");
        assert!(error.to_string().contains("content hash"));
    }

    #[test]
    fn write_dedups_against_a_pre_existing_v2_artifact() {
        // Within a store, a snapshot path already holding a v2 artifact accepts a
        // second write of the same snapshot without conflict and returns the
        // existing artifact, never rewriting it.
        let repo = modified_repo();
        let files = capture_worktree_diff_files(repo.path()).unwrap();
        let fingerprint = compute_revision_fingerprint(repo.path()).unwrap();
        let snapshot = DiffSnapshot::new(
            ReviewId::new("review:default"),
            fingerprint.snapshot_id.clone(),
            files,
        );
        let store_dir = ShoreStorePaths::resolve(repo.path())
            .unwrap()
            .store_dir()
            .to_path_buf();

        let first = write_snapshot_artifact_to(&store_dir, &fingerprint, snapshot.clone()).unwrap();
        assert_eq!(first.version, 2);
        let path = snapshot_artifact_path(&store_dir, &fingerprint.snapshot_id);
        let on_disk = fs::read(&path).unwrap();

        let deduped = write_snapshot_artifact_to(&store_dir, &fingerprint, snapshot).unwrap();
        assert_eq!(deduped, first, "dedup returns the existing v2 artifact");
        assert_eq!(fs::read(&path).unwrap(), on_disk, "artifact left untouched");
    }

    #[test]
    fn captured_text_rows_remain_inline_in_snapshot_artifact() {
        let repo = TestRepo::new();
        repo.write("README.md", "base\n");
        repo.commit_all("base");

        let added = (1..=25).map(|n| format!("line {n}\n")).collect::<String>();
        repo.write("docs/example.md", added);

        let files = capture_worktree_diff_files(repo.path()).unwrap();
        let fingerprint = compute_revision_fingerprint(repo.path()).unwrap();
        let snapshot = DiffSnapshot::new(
            ReviewId::new("review:default"),
            fingerprint.snapshot_id.clone(),
            files,
        );
        let artifact = write_snapshot_artifact(repo.path(), &fingerprint, snapshot).unwrap();

        let stored = read_snapshot_artifact(repo.path(), &artifact.snapshot.snapshot_id).unwrap();
        let added_file = stored
            .snapshot
            .files
            .iter()
            .find(|f| f.new_path.as_deref() == Some("docs/example.md"))
            .expect("captured added file");

        // V1: every captured row stays inline in the artifact JSON; no elision.
        assert_eq!(added_file.hunks.len(), 1);
        assert_eq!(added_file.hunks[0].rows.len(), 25);
        assert!(added_file.metadata_rows.is_empty());
    }

    #[test]
    fn write_snapshot_artifact_stores_full_snapshot() {
        let repo = modified_repo();
        let artifact = write_current_snapshot_artifact(&repo);

        let stored = read_snapshot_artifact(repo.path(), &artifact.snapshot.snapshot_id).unwrap();

        assert_eq!(stored.schema, "shore.snapshot");
        assert_eq!(stored.version, 2);
        assert_eq!(stored.snapshot.snapshot_id, artifact.snapshot.snapshot_id);
        assert_eq!(stored.snapshot.files.len(), 1);
        assert_eq!(
            stored.snapshot.files[0].new_path.as_deref(),
            Some("src/lib.rs")
        );
        assert!(!stored.snapshot.files[0].hunks.is_empty());
    }

    #[test]
    fn stored_snapshot_artifact_survives_worktree_drift() {
        let repo = modified_repo();
        let artifact = write_current_snapshot_artifact(&repo);

        repo.write("src/lib.rs", "pub fn value() -> u32 { 99 }\n");
        let stored = read_snapshot_artifact(repo.path(), &artifact.snapshot.snapshot_id).unwrap();

        assert_eq!(
            stored.snapshot.files[0].new_path.as_deref(),
            Some("src/lib.rs")
        );
        assert!(format!("{:?}", stored.snapshot).contains("2"));
        assert!(!format!("{:?}", stored.snapshot).contains("99"));
    }

    #[test]
    fn read_snapshot_artifact_rejects_tampered_content() {
        let repo = modified_repo();
        let artifact = write_current_snapshot_artifact(&repo);
        let path = snapshot_artifact_path(
            &resolved_store_dir(repo.path()),
            &artifact.snapshot.snapshot_id,
        );

        let mut json: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        // Tamper a field inside the v2 content hash. `DiffFile` is snake_case,
        // unlike the camelCase `SnapshotArtifact` wrapper.
        json["snapshot"]["files"][0]["new_path"] = serde_json::json!("/evil");
        fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();

        let error = read_snapshot_artifact(repo.path(), &artifact.snapshot.snapshot_id)
            .expect_err("tampered artifact should be rejected");

        assert!(error.to_string().contains("content hash"));
    }

    #[test]
    fn snapshot_artifact_hash_covers_snapshot_rows() {
        let repo = modified_repo();
        let artifact = write_current_snapshot_artifact(&repo);
        let mut changed = artifact.clone();
        changed.snapshot.files.clear();

        assert_ne!(
            snapshot_artifact_content_hash(&artifact).unwrap(),
            snapshot_artifact_content_hash(&changed).unwrap()
        );
    }

    #[test]
    fn snapshot_artifact_hash_is_stable_across_json_round_trip() {
        let repo = modified_repo();
        let artifact = write_current_snapshot_artifact(&repo);
        let stored = read_snapshot_artifact(repo.path(), &artifact.snapshot.snapshot_id).unwrap();
        let reparsed: SnapshotArtifact =
            serde_json::from_str(&serde_json::to_string_pretty(&stored).unwrap()).unwrap();

        assert_eq!(
            stored.content_hash,
            snapshot_artifact_content_hash(&stored).unwrap()
        );
        assert_eq!(
            stored.content_hash,
            snapshot_artifact_content_hash(&reparsed).unwrap()
        );
    }

    #[test]
    fn snapshot_artifact_helpers_resolve_shore_dir_from_subdirectory() {
        let repo = modified_repo();
        fs::create_dir_all(repo.path().join("src")).unwrap();
        let files = capture_worktree_diff_files(repo.path()).unwrap();
        let fingerprint = compute_revision_fingerprint(repo.path()).unwrap();
        let snapshot = DiffSnapshot::new(
            ReviewId::new("review:default"),
            fingerprint.snapshot_id.clone(),
            files,
        );

        let artifact =
            write_snapshot_artifact(repo.path().join("src"), &fingerprint, snapshot).unwrap();
        let read = read_snapshot_artifact(repo.path().join("src"), &artifact.snapshot.snapshot_id)
            .unwrap();

        assert_eq!(read, artifact);
    }

    #[test]
    fn write_validation_artifact_read_prefers_resolved_store_when_present() {
        let repo = modified_repo();
        // The artifact lands in the resolved (shared common-dir) store, exactly
        // where capture writes it, so the read resolves it without any fallback.
        let artifact = write_current_snapshot_artifact(&repo);

        let read = read_snapshot_artifact_for_write_validation(
            repo.path(),
            &artifact.snapshot.snapshot_id,
        )
        .unwrap();

        assert_eq!(read, artifact);
    }

    // The worktree-local read fallback's old premise — a non-ephemeral worktree
    // that captured locally and had not yet copied its artifact into a separate
    // linked store — no longer exists: with one shared store by default, a
    // populated worktree-local `.shore/data` is a pre-default store that the
    // legacy guard routes to `shore store migrate` rather than reading through.
    // The dedicated fallback test is retired; `..._prefers_resolved_store...` and
    // `..._missing_everywhere...` cover the surviving write-validation reads.

    #[test]
    fn write_validation_artifact_read_missing_everywhere_errors_clearly() {
        let repo = modified_repo();
        let fingerprint = compute_revision_fingerprint(repo.path()).unwrap();

        let error =
            read_snapshot_artifact_for_write_validation(repo.path(), &fingerprint.snapshot_id)
                .expect_err("an artifact absent from both stores errors");

        assert!(
            error.to_string().contains("missing artifact for snapshot"),
            "got: {error}"
        );
        assert!(
            error
                .to_string()
                .contains("import referenced artifacts before reading"),
            "got: {error}"
        );
    }

    /// Test convenience: write a snapshot artifact to the worktree's resolved
    /// write store — the shared common-dir store by default, exactly where
    /// capture lands it. Production has a single snapshot writer
    /// (`write_snapshot_artifact_to`); capture resolves the write store once and
    /// calls it directly. Resolving the store here keeps these reads and the
    /// production read surface on the same store.
    fn write_snapshot_artifact(
        repo: impl AsRef<Path>,
        fingerprint: &RevisionFingerprint,
        snapshot: DiffSnapshot,
    ) -> Result<SnapshotArtifact> {
        let store_dir = resolve_store(repo.as_ref())?.store_dir().to_path_buf();
        write_snapshot_artifact_to(&store_dir, fingerprint, snapshot)
    }

    fn write_current_snapshot_artifact(repo: &TestRepo) -> SnapshotArtifact {
        let files = capture_worktree_diff_files(repo.path()).unwrap();
        let fingerprint = compute_revision_fingerprint(repo.path()).unwrap();
        let snapshot = DiffSnapshot::new(
            ReviewId::new("review:default"),
            fingerprint.snapshot_id.clone(),
            files,
        );

        write_snapshot_artifact(repo.path(), &fingerprint, snapshot).unwrap()
    }

    fn modified_repo() -> TestRepo {
        let repo = TestRepo::new();
        repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
        repo.commit_all("base");
        repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
        repo
    }

    /// The store a capture/write actually lands in for `repo` — the shared
    /// common-dir store by default.
    fn resolved_store_dir(repo: &Path) -> PathBuf {
        resolve_store(repo).unwrap().store_dir().to_path_buf()
    }

    fn committed_repo() -> TestRepo {
        let repo = TestRepo::new();
        repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
        repo.commit_all("base");
        repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
        repo.commit_all("change");
        repo
    }

    /// Real `git clone` of `source` into a fresh temp dir. Cloning preserves the
    /// commit/tree OIDs, so the same `--base HEAD~1` range captures the same
    /// `snapshot_id` while the differing canonical worktree root mints a distinct
    /// `revision_id` — exactly the two-worktree shape of #146.
    fn clone_repo(source: &TestRepo) -> TestRepo {
        let root = tempfile::tempdir().expect("create clone temp directory");
        let status = Command::new("git")
            .args(["clone", "--quiet"])
            .arg(source.path())
            .arg(root.path())
            .status()
            .expect("run git clone");
        assert!(status.success(), "git clone failed");
        let clone = TestRepo { root };
        clone.git(["config", "user.name", "Shore Tests"]);
        clone.git(["config", "user.email", "shore-tests@example.com"]);
        clone.git(["config", "commit.gpgsign", "false"]);
        clone
    }

    fn capture_range(repo: &TestRepo, base_rev: &str) -> crate::session::CaptureResult {
        capture_review(
            CaptureOptions::new(repo.path()).with_commit_range(CommitRangeSpec::new(base_rev)),
        )
        .unwrap()
    }

    /// Rewrite a v2 artifact's bytes into a faithful **v1** artifact: re-add the
    /// identity/endpoint fields the v1 body carried, set `version: 1`, and
    /// recompute `contentHash` over the full v1 body, so it passes the
    /// version-agnostic validation the dual-read decode applies.
    fn rewrite_as_v1(bytes: &[u8]) -> Vec<u8> {
        let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        let object = value.as_object_mut().unwrap();
        object.insert("version".to_owned(), serde_json::json!(1));
        object.insert(
            "reviewUnitId".to_owned(),
            serde_json::json!("review-unit:sha256:legacy"),
        );
        object.insert(
            "source".to_owned(),
            serde_json::json!({ "kind": "git_working_tree" }),
        );
        object.insert(
            "base".to_owned(),
            serde_json::json!({ "kind": "git_working_tree", "worktreeRoot": "/legacy" }),
        );
        object.insert(
            "target".to_owned(),
            serde_json::json!({ "kind": "git_working_tree", "worktreeRoot": "/legacy" }),
        );
        object.remove("contentHash");
        let hash = sha256_json_prefixed(&value).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("contentHash".to_owned(), serde_json::json!(hash));
        serde_json::to_vec(&value).unwrap()
    }

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
                .unwrap_or_else(|error| panic!("run git {:?}: {error}", args));

            assert!(
                output.status.success(),
                "git {:?} failed\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
                args,
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
