mod command;
mod ingest;
mod patch;
mod raw;

pub use command::git_worktree_root;
pub use ingest::ingest_tracked_diff;
