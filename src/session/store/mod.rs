pub(crate) mod body_artifact;
mod event_store;
pub(in crate::session) mod fingerprint;
pub(crate) mod snapshot_artifact;
pub(crate) mod store_init;

pub(crate) use event_store::{EventStore, EventWriteOutcome};
pub(crate) use fingerprint::worktree_fingerprint_for_files;
pub use fingerprint::{
    ReviewUnitFingerprint, WorktreeFingerprint, capture_worktree_fingerprint,
    compute_review_unit_fingerprint,
};
pub use snapshot_artifact::{SnapshotArtifact, read_snapshot_artifact, write_snapshot_artifact};
pub(crate) use store_init::{ShoreStorePaths, prepare_shore_writer, sweep_stale_temp_files};
pub use store_init::{ensure_shore_ignored, shore_dir_for_repo};
