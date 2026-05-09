mod annotation;
mod cursor;
mod file;
mod hunk;
mod ids;
mod review;
mod row;

pub fn decode_json<T>(json: &str) -> crate::error::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    Ok(serde_json::from_str(json)?)
}

pub use annotation::{
    Anchor, AnchorResolution, AnchorResolutionReason, Annotation, AnnotationSource, LineRange,
    ResolutionStatus, Side, re_resolve_annotations,
};
pub(crate) use annotation::{hash_normalized_lines, rows_for_line_range};
pub use cursor::CursorState;
pub use file::{DiffFile, FileStatus};
pub use hunk::ReviewHunk;
pub use ids::{AnnotationId, FileId, HunkId, ReviewId, RowId, SnapshotId};
pub use review::{DiffSnapshot, Review, ReviewStream};
pub use row::{DiffRow, DiffRowKind, FileMetadataKind, FileMetadataRow, ReviewRow, ReviewRowKind};
