mod command;
mod ingest;
mod patch;
mod raw;

pub(crate) use command::git_head_oid;
pub use command::git_worktree_root;
pub(crate) use ingest::capture_worktree_diff_files;
pub use ingest::ingest_tracked_diff;
