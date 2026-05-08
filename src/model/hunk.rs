use serde::{Deserialize, Serialize};

use super::{DiffRow, HunkId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReviewHunk {
    pub id: HunkId,
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub rows: Vec<DiffRow>,
}
