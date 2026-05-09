pub mod event;
mod fingerprint;
pub mod state;
mod store_init;

pub use event::{
    EventPayload, EventTarget, EventType, ReviewInitializedPayload, RevisionPublishedPayload,
    ShoreEvent, SidecarObservedPayload, SidecarSource, SnapshotObservedPayload, Writer, WriterRole,
    WriterTool,
};
pub(crate) use fingerprint::worktree_fingerprint_for_files;
pub use fingerprint::{WorktreeFingerprint, capture_worktree_fingerprint};
pub use state::{ProjectionDiagnostic, SessionState};
pub use store_init::{ensure_shore_ignored, shore_dir_for_repo};
