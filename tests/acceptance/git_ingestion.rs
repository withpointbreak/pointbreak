use shore::git::ingest_tracked_diff;
use shore::model::{DiffFile, DiffRowKind, FileStatus};

use crate::support::git_repo::GitRepo;
use crate::support::snapshots::normalize_path;

#[test]
fn scratch_repo_can_create_commit_modify_and_report_status() {
    let repo = GitRepo::new();

    repo.write("src/lib.rs", "pub fn original() {}\n");
    repo.commit_all("initial commit");
    repo.write("src/lib.rs", "pub fn changed() {}\n");
    repo.write("obsolete.rs", "remove me\n");
    repo.remove("obsolete.rs");

    let status = repo.git(["status", "--porcelain=v2"]);

    assert!(repo.path().join("src/lib.rs").exists());
    assert!(!repo.path().join("obsolete.rs").exists());
    assert!(!normalize_path(repo.path()).is_empty());
    assert!(
        status.stderr.is_empty(),
        "status command should not write stderr:\n{}",
        status.stderr
    );
    assert!(
        status.stdout.contains("src/lib.rs"),
        "status output should mention modified file:\n{}",
        status.stdout
    );
}

#[test]
fn tracked_text_diff_ingests_modified_added_and_deleted_files() {
    let repo = GitRepo::new();
    repo.write(
        "src/modified.rs",
        "fn keep() {}\nfn old_name() {}\nfn tail() {}\n",
    );
    repo.write("src/deleted.rs", "fn gone() {}\nfn deleted_tail() {}\n");
    repo.commit_all("initial commit");

    repo.write(
        "src/modified.rs",
        "fn keep() {}\nfn new_name() {}\nfn tail() {}\n",
    );
    repo.write("src/added.rs", "fn added() {}\nfn added_tail() {}\n");
    repo.remove("src/deleted.rs");
    repo.git(["add", "--all"]);

    let snapshot = ingest_tracked_diff(repo.path()).expect("tracked diff ingests");

    assert_eq!(snapshot.files.len(), 3);
    assert!(!snapshot.snapshot_id.as_str().is_empty());

    let added = file_by_path(&snapshot.files, "src/added.rs");
    assert_eq!(added.status, FileStatus::Added);
    assert_eq!(added.old_path, None);
    assert_eq!(added.new_path.as_deref(), Some("src/added.rs"));
    assert_eq!(added.hunks.len(), 1);
    assert_eq!(added.hunks[0].old_start, 0);
    assert_eq!(added.hunks[0].old_lines, 0);
    assert_eq!(added.hunks[0].new_start, 1);
    assert_eq!(added.hunks[0].new_lines, 2);
    assert_eq!(added.hunks[0].rows.len(), 2);
    assert_eq!(added.hunks[0].rows[0].kind, DiffRowKind::Added);
    assert_eq!(added.hunks[0].rows[0].old_line, None);
    assert_eq!(added.hunks[0].rows[0].new_line, Some(1));
    assert_eq!(added.hunks[0].rows[1].new_line, Some(2));

    let deleted = file_by_path(&snapshot.files, "src/deleted.rs");
    assert_eq!(deleted.status, FileStatus::Deleted);
    assert_eq!(deleted.old_path.as_deref(), Some("src/deleted.rs"));
    assert_eq!(deleted.new_path, None);
    assert_eq!(deleted.hunks.len(), 1);
    assert_eq!(deleted.hunks[0].old_start, 1);
    assert_eq!(deleted.hunks[0].old_lines, 2);
    assert_eq!(deleted.hunks[0].new_start, 0);
    assert_eq!(deleted.hunks[0].new_lines, 0);
    assert_eq!(deleted.hunks[0].rows[0].kind, DiffRowKind::Removed);
    assert_eq!(deleted.hunks[0].rows[0].old_line, Some(1));
    assert_eq!(deleted.hunks[0].rows[0].new_line, None);
    assert_eq!(deleted.hunks[0].rows[1].old_line, Some(2));

    let modified = file_by_path(&snapshot.files, "src/modified.rs");
    assert_eq!(modified.status, FileStatus::Modified);
    assert_eq!(modified.old_path.as_deref(), Some("src/modified.rs"));
    assert_eq!(modified.new_path.as_deref(), Some("src/modified.rs"));
    assert_eq!(modified.hunks.len(), 1);
    assert_eq!(modified.hunks[0].old_start, 1);
    assert_eq!(modified.hunks[0].old_lines, 3);
    assert_eq!(modified.hunks[0].new_start, 1);
    assert_eq!(modified.hunks[0].new_lines, 3);

    let rows = &modified.hunks[0].rows;
    assert_eq!(rows[0].kind, DiffRowKind::Context);
    assert_eq!(rows[0].old_line, Some(1));
    assert_eq!(rows[0].new_line, Some(1));
    assert_eq!(rows[1].kind, DiffRowKind::Removed);
    assert_eq!(rows[1].old_line, Some(2));
    assert_eq!(rows[1].new_line, None);
    assert_eq!(rows[2].kind, DiffRowKind::Added);
    assert_eq!(rows[2].old_line, None);
    assert_eq!(rows[2].new_line, Some(2));
    assert_eq!(rows[3].kind, DiffRowKind::Context);
    assert_eq!(rows[3].old_line, Some(3));
    assert_eq!(rows[3].new_line, Some(3));
}

fn file_by_path<'a>(files: &'a [DiffFile], path: &str) -> &'a DiffFile {
    files
        .iter()
        .find(|file| {
            file.old_path.as_deref() == Some(path) || file.new_path.as_deref() == Some(path)
        })
        .unwrap_or_else(|| panic!("missing diff file for {path}; files: {files:#?}"))
}
