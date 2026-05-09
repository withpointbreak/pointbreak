use std::path::Path;

use serde::Serialize;

use crate::canonical_hash::sha256_json_hex;
use crate::error::Result;
use crate::git::{capture_worktree_diff_files, git_head_oid, git_worktree_root};
use crate::model::{DiffFile, RevisionId, SnapshotId};

const FINGERPRINT_SCHEMA: &str = "shore.worktree-fingerprint";
const FINGERPRINT_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeFingerprint {
    pub revision_id: RevisionId,
    pub snapshot_id: SnapshotId,
}

pub fn capture_worktree_fingerprint(repo: &Path) -> Result<WorktreeFingerprint> {
    let files = capture_worktree_diff_files(repo)?;
    worktree_fingerprint_for_files(repo, &files)
}

pub(crate) fn worktree_fingerprint_for_files(
    repo: &Path,
    files: &[DiffFile],
) -> Result<WorktreeFingerprint> {
    let descriptor = WorktreeFingerprintDescriptor {
        schema: FINGERPRINT_SCHEMA,
        version: FINGERPRINT_VERSION,
        worktree_root: normalized_worktree_root(repo)?,
        base_head: git_head_oid(repo)?,
        files,
    };
    let hash = sha256_json_hex(&descriptor)?;

    Ok(WorktreeFingerprint {
        revision_id: RevisionId::new(format!("rev:worktree:sha256:{hash}")),
        snapshot_id: SnapshotId::new(format!("snap:git:sha256:{hash}")),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorktreeFingerprintDescriptor<'a> {
    schema: &'static str,
    version: u32,
    worktree_root: String,
    base_head: String,
    files: &'a [DiffFile],
}

fn normalized_worktree_root(repo: &Path) -> Result<String> {
    let root = git_worktree_root(repo)?;
    let root = root.canonicalize().unwrap_or(root);
    Ok(root.to_string_lossy().into_owned())
}
