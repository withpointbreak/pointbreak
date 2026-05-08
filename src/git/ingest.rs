use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Result, ShoreError};
use crate::git::command::run_git;
use crate::git::patch::{PatchFile, parse_patch};
use crate::git::raw::parse_raw;
use crate::model::{DiffFile, DiffSnapshot, FileId, ReviewId, SnapshotId};

pub fn ingest_tracked_diff(repo: impl AsRef<Path>) -> Result<DiffSnapshot> {
    let repo = repo.as_ref();
    let raw_output = run_git(
        repo,
        [
            "diff",
            "--raw",
            "-z",
            "--no-ext-diff",
            "--no-color",
            "--full-index",
            "-M",
            "-C",
            "--submodule=short",
            "HEAD",
            "--",
        ],
    )?;
    let patch_output = run_git(
        repo,
        [
            "diff",
            "--patch",
            "--no-ext-diff",
            "--no-color",
            "--full-index",
            "-M",
            "-C",
            "--submodule=short",
            "HEAD",
            "--",
        ],
    )?;

    let raw_files = parse_raw(&raw_output.stdout)?;
    let patch_files = parse_patch(&String::from_utf8_lossy(&patch_output.stdout))?
        .into_iter()
        .map(|file| (file.key(), file))
        .collect::<BTreeMap<_, _>>();

    let files = raw_files
        .into_iter()
        .map(|raw_file| {
            let key = raw_file.key();
            let patch_file = patch_files
                .get(&key)
                .ok_or_else(|| ShoreError::Message(format!("missing patch entry for {key}")))?;
            diff_file(raw_file, patch_file)
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(DiffSnapshot::new(
        ReviewId::new("working-tree"),
        SnapshotId::new("git-diff-head"),
        files,
    ))
}

fn diff_file(raw_file: crate::git::raw::RawFile, patch_file: &PatchFile) -> Result<DiffFile> {
    Ok(DiffFile {
        id: FileId::new(raw_file.key()),
        status: raw_file.status,
        old_path: raw_file.old_path,
        new_path: raw_file.new_path,
        hunks: patch_file.hunks.clone(),
    })
}
