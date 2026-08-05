mod change;
mod cursor;
mod file;
mod hunk;
pub(crate) mod id_prefix;
mod ids;
mod review;
mod review_note;
mod revision;
mod row;
mod validation;
mod work_object;

pub fn decode_json<T>(json: &str) -> crate::error::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    Ok(serde_json::from_str(json)?)
}

pub use change::{ChangeIdentityDescriptorV1, derive_change_id};
pub(crate) use change::{
    current_revisions, derive_membership_claim_id, lowercase_hex, replacement_heads_diverge,
    revision_graph_has_cycle,
};
pub use cursor::CursorState;
pub use file::{DiffFile, FileStatus};
pub use hunk::ReviewHunk;
pub use ids::{
    ActorId, AssessmentId, ChangeDeclarationClaimId, ChangeId, ChangeLinkClaimId,
    ChangeMembershipClaimId, ChangeMembershipWithdrawalId, ChangeRevisionRelationClaimId,
    ChangeRevisionRelationWithdrawalId, CheckpointId, CommitAssociationId, CommitWithdrawalId,
    EngagementId, EventId, FileId, HunkId, InputRequestId, InputRequestResponseId, JournalId,
    ObjectId, ObservationId, RefAssociationId, RefWithdrawalId, ReviewFactPortId, ReviewId,
    RevisionId, RevisionRelationAttestationId, RowId, TrackId, ValidationCheckId, WorkObjectId,
};
pub use review::DiffSnapshot;
pub use review_note::Side;
pub use revision::{
    CommitRangeCaptureMode, ReviewEndpoint, ReviewTargetRef, RevisionRefV1, RevisionSource,
    RootCommitCaptureMode, StagedCaptureMode, UnstagedCaptureMode, WorktreeCaptureMode,
};
pub use row::{DiffRow, DiffRowKind, FileMetadataKind, FileMetadataRow};
pub use validation::{ValidationStatus, ValidationTarget, ValidationTrigger};
pub use work_object::{
    EngagementType, TargetRef, TaskTargetRef, WorkObjectType, engagement_type_of_subject,
    subject_revision_id, work_object_type_of_subject,
};
