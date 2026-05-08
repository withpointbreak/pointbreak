use serde::{Deserialize, Serialize};

use super::{DiffFile, ReviewId, SnapshotId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub id: ReviewId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiffSnapshot {
    pub review_id: ReviewId,
    pub snapshot_id: SnapshotId,
    pub files: Vec<DiffFile>,
}

impl DiffSnapshot {
    pub fn empty(review_id: ReviewId) -> Self {
        Self {
            review_id,
            snapshot_id: SnapshotId::new("empty"),
            files: Vec::new(),
        }
    }

    pub fn new(review_id: ReviewId, snapshot_id: SnapshotId, files: Vec<DiffFile>) -> Self {
        Self {
            review_id,
            snapshot_id,
            files,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReviewStream {
    pub review_id: ReviewId,
}

impl ReviewStream {
    pub fn empty(review_id: ReviewId) -> Self {
        Self { review_id }
    }
}
