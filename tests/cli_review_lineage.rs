mod support;

use support::{dump_repo, shore};

/// `shore review lineage` was removed. A stale invocation must exit non-zero and
/// point at the supersession replacement (`--supersedes` on capture, read with
/// `shore review revisions`).
#[test]
fn review_lineage_command_is_removed_with_a_supersedes_hint() {
    let repo = dump_repo();
    let repo_path = repo.path().to_str().unwrap();

    let output = shore([
        "review",
        "lineage",
        "attach",
        "--repo",
        repo_path,
        "--lineage",
        "review-unit-lineage:random:test",
    ]);

    assert!(
        !output.status.success(),
        "stale `review lineage` must exit non-zero"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("is removed"),
        "stderr should announce the command is removed:\n{stderr}"
    );
    assert!(
        stderr.contains("--supersedes"),
        "stderr should reference --supersedes:\n{stderr}"
    );
}
