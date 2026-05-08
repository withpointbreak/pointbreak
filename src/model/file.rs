use serde::{Deserialize, Serialize};

use super::{FileId, ReviewHunk};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiffFile {
    pub id: FileId,
    pub status: FileStatus,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub hunks: Vec<ReviewHunk>,
}
