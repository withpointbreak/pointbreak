pub mod artifact_removal;
pub(crate) mod commit_oid_grouping;
pub(crate) mod commit_range;
pub(crate) mod cosignature;
pub(crate) mod engagement;
pub(crate) mod freshness;
pub(crate) mod liveness;
mod read;
pub(crate) mod revisions_by_base;
pub mod state;
pub(crate) mod supersession;
pub(crate) mod task;
#[cfg(test)]
pub(crate) mod test_support;

pub use artifact_removal::ArtifactRemovalProjection;
pub use commit_oid_grouping::CommitOidGroupingProjection;
pub use commit_range::{
    CommitEdgeSource, CurrentCommitAssociation, CurrentRefAssociation,
    RevisionCommitRangeProjection, RevisionCommitRangeView, WithdrawnCommitAssociation,
    WithdrawnRefAssociation,
};
pub use engagement::{EngagementGrouping, EngagementLifecycle, EngagementView};
pub use liveness::{LivenessScope, LivenessToken};
pub use read::{load_durable_notes_for_repo, read_events, rebuild_state};
pub use revisions_by_base::RevisionsByBase;
pub use state::{ProjectionDiagnostic, SessionState};
pub use supersession::SupersessionView;
