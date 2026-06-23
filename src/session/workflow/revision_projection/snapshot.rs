use std::path::Path;

use super::identity::{RevisionProjectionIdentity, SnapshotContentState};
use crate::error::{Result, ShoreError};
use crate::model::{DiffSnapshot, ObjectId};
use crate::session::object_artifact::{object_artifact_path, read_object_artifact};
use crate::session::projection::ArtifactRemovalProjection;
use crate::session::store::resolution::resolve_read_store;

/// The resolved read state of a content-addressed snapshot. The join layer
/// returns this instead of erroring when the bound content hash has an
/// `ArtifactRemoved` fact, distinguishing a removal whose bytes are still stored
/// (`SuppressedPresent`) from one whose bytes have been swept
/// (`PhysicallyRemoved`) so the read surface never overstates what has happened.
pub(crate) enum SnapshotContent {
    Present(DiffSnapshot),
    /// A removal is recorded for the bound content, but its blob is still on disk
    /// (the suppression is reversible until a `compact` reclaims the bytes).
    SuppressedPresent {
        content_hash: String,
    },
    /// A removal is recorded and the bound blob has been swept from the store.
    PhysicallyRemoved {
        content_hash: String,
    },
}

impl From<&SnapshotContent> for SnapshotContentState {
    fn from(content: &SnapshotContent) -> Self {
        match content {
            SnapshotContent::Present(_) => SnapshotContentState::Present,
            SnapshotContent::SuppressedPresent { .. } => SnapshotContentState::SuppressedPresent,
            SnapshotContent::PhysicallyRemoved { .. } => SnapshotContentState::PhysicallyRemoved,
        }
    }
}

/// Resolve the bound snapshot, splitting a recorded removal of its content hash
/// into `SuppressedPresent` (bytes still on disk) vs `PhysicallyRemoved` (bytes
/// swept) by an on-disk presence check. The removed-vs-missing decision lives
/// here, at the layer that holds the event set — the storage byte readers stay
/// event-unaware. A still-missing, *not*-removed artifact falls through to the
/// reader's hard error (removed != not-yet-synced).
pub(super) fn resolve_snapshot_content(
    repo: &Path,
    revision: &RevisionProjectionIdentity,
    removal: &ArtifactRemovalProjection,
) -> Result<SnapshotContent> {
    if removal.is_removed(&revision.object_artifact_content_hash) {
        let content_hash = revision.object_artifact_content_hash.clone();
        return Ok(if bound_blob_present(repo, &revision.object_id)? {
            SnapshotContent::SuppressedPresent { content_hash }
        } else {
            SnapshotContent::PhysicallyRemoved { content_hash }
        });
    }
    Ok(SnapshotContent::Present(load_bound_object_artifact(
        repo, revision,
    )?))
}

/// Cheap read-path presence check: does the bound object artifact file exist? A
/// stat, never a decode — the removed-vs-swept split must not pay a full read of
/// every still-present blob.
fn bound_blob_present(repo: &Path, object_id: &ObjectId) -> Result<bool> {
    let store_dir = resolve_read_store(repo)?.store_dir().to_path_buf();
    Ok(object_artifact_path(&store_dir, object_id).exists())
}

pub(super) fn load_bound_object_artifact(
    repo: &Path,
    revision: &RevisionProjectionIdentity,
) -> Result<DiffSnapshot> {
    let artifact = read_object_artifact(repo, &revision.object_id)?;
    // Bind via the namespace-independent object_id + content_hash only. Identity
    // (revision_id/source/base/target) lives in the capture event/projection,
    // never the content-addressed artifact body.
    if artifact.snapshot.object_id != revision.object_id {
        return Err(ShoreError::Message(format!(
            "object artifact metadata mismatch for {}",
            revision.id.as_str()
        )));
    }
    if artifact.content_hash != revision.object_artifact_content_hash {
        return Err(ShoreError::Message(format!(
            "object artifact content hash mismatch for {}",
            revision.id.as_str()
        )));
    }

    Ok(artifact.snapshot)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use super::*;
    use crate::model::{ReviewEndpoint, RevisionId};
    use crate::session::event::EventType;
    use crate::session::{
        CaptureOptions, CaptureResult, CommitRangeSpec, EventStore, capture_review,
    };

    #[test]
    fn binds_by_snapshot_id_and_content_hash_ignoring_endpoint_identity() {
        // Capture a range so a real artifact + capture event exist in .shore/data.
        let repo = committed_repo();
        let captured = capture_range(&repo);

        // The authentic projection identity binds.
        let authentic = identity_from(&captured, repo.path());
        load_bound_object_artifact(repo.path(), &authentic).unwrap();

        // A second identity over the SAME snapshot + content hash but a DIFFERENT
        // revision_id and a different worktree target also binds — identity is
        // not read from the artifact body (INV-3).
        let other = RevisionProjectionIdentity {
            id: RevisionId::new("review-unit:sha256:other-worktree"),
            target: ReviewEndpoint::GitWorkingTree {
                worktree_root: "/some/other/worktree".to_owned(),
            },
            ..authentic.clone()
        };
        let snapshot = load_bound_object_artifact(repo.path(), &other).unwrap();
        assert_eq!(snapshot.object_id, captured.object_id);
    }

    #[test]
    fn rejects_content_hash_mismatch() {
        let repo = committed_repo();
        let captured = capture_range(&repo);
        let mut tampered = identity_from(&captured, repo.path());
        tampered.object_artifact_content_hash = "sha256:not-the-real-hash".to_owned();
        let err = load_bound_object_artifact(repo.path(), &tampered).unwrap_err();
        assert!(err.to_string().contains("content hash"));
    }

    #[test]
    fn removed_but_unswept_content_is_suppressed_present() {
        // Remove the bound content (an ArtifactRemoved fact exists) but do NOT
        // compact: the blob is still on disk. The join must report
        // SuppressedPresent, not an unconditional removed/swept state.
        let repo = committed_repo();
        let captured = capture_range(&repo);
        let identity = identity_from(&captured, repo.path());
        let removal = removal_for(&captured.object_artifact_content_hash);

        let content = resolve_snapshot_content(repo.path(), &identity, &removal).unwrap();

        assert!(matches!(
            content,
            SnapshotContent::SuppressedPresent { content_hash }
                if content_hash == captured.object_artifact_content_hash
        ));
    }

    #[test]
    fn removed_and_swept_content_is_physically_removed() {
        // Remove the bound content AND sweep its blob from disk: the join must
        // report PhysicallyRemoved.
        let repo = committed_repo();
        let captured = capture_range(&repo);
        let identity = identity_from(&captured, repo.path());
        let removal = removal_for(&captured.object_artifact_content_hash);
        delete_bound_blob(repo.path(), &captured.object_id);

        let content = resolve_snapshot_content(repo.path(), &identity, &removal).unwrap();

        assert!(matches!(
            content,
            SnapshotContent::PhysicallyRemoved { content_hash }
                if content_hash == captured.object_artifact_content_hash
        ));
    }

    /// A removal projection carrying a single `ArtifactRemoved` fact for `hash`.
    fn removal_for(content_hash: &str) -> ArtifactRemovalProjection {
        use crate::model::JournalId;
        use crate::session::event::{ArtifactRemovedPayload, EventTarget, ShoreEvent, Writer};
        let event = ShoreEvent::new(
            EventType::ArtifactRemoved,
            ArtifactRemovedPayload::idempotency_key(content_hash),
            EventTarget::for_journal(JournalId::new("journal:fixture")),
            Writer::shore_local("test"),
            ArtifactRemovedPayload {
                content_hash: content_hash.to_owned(),
            },
            "2026-06-19T00:00:00Z",
        )
        .unwrap();
        ArtifactRemovalProjection::from_events(&[event]).unwrap()
    }

    fn delete_bound_blob(repo: &Path, object_id: &crate::model::ObjectId) {
        let store_dir = resolved_store_dir(repo);
        let path = crate::session::object_artifact::object_artifact_path(&store_dir, object_id);
        fs::remove_file(path).expect("delete bound object artifact blob");
    }

    fn capture_range(repo: &TestRepo) -> CaptureResult {
        capture_review(
            CaptureOptions::new(repo.path()).with_commit_range(CommitRangeSpec::new("HEAD~1")),
        )
        .unwrap()
    }

    /// Build a `RevisionProjectionIdentity` from a `CaptureResult` the way the
    /// projection's `selected_revision_capture` would — sourcing every field
    /// from the capture event/result, never from the artifact body.
    fn identity_from(captured: &CaptureResult, repo: &Path) -> RevisionProjectionIdentity {
        let events = EventStore::open(resolved_store_dir(repo))
            .list_events()
            .unwrap();
        let event = events
            .iter()
            .find(|event| {
                event.event_type == EventType::WorkObjectProposed
                    && event.payload["workObject"]["revision"]["id"]
                        == captured.revision_id.as_str()
            })
            .expect("capture event");
        RevisionProjectionIdentity {
            id: captured.revision_id.clone(),
            journal_id: captured.journal_id.clone(),
            source: captured.source.clone(),
            base: captured.base.clone(),
            target: captured.target.clone(),
            revision_id: captured.revision_id.clone(),
            object_id: captured.object_id.clone(),
            object_artifact_content_hash: captured.object_artifact_content_hash.clone(),
            capture_event_id: event.event_id.clone(),
        }
    }

    /// The store a workflow actually lands in for `repo` — the shared common-dir
    /// store by default. Reads that follow a workflow resolve here, not the raw
    /// worktree-local `.shore/data`.
    fn resolved_store_dir(repo: &Path) -> std::path::PathBuf {
        crate::git::git_common_dir(repo).unwrap().join("shore")
    }

    fn committed_repo() -> TestRepo {
        let repo = TestRepo::new();
        repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
        repo.commit_all("base");
        repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
        repo.commit_all("change");
        repo
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
