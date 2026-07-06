use serde::{Deserialize, Serialize};

use super::Side;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffRowKind {
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiffRow {
    pub kind: DiffRowKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub text: String,
}

impl DiffRow {
    pub fn line_on_side(&self, side: Side) -> Option<u32> {
        match side {
            Side::Old => self.old_line,
            Side::New => self.new_line,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileMetadataKind {
    BinarySummary,
    ModeChange,
    RenameSummary,
    SubmoduleSummary,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileMetadataRow {
    pub kind: FileMetadataKind,
    pub text: String,
}
