use serde::{Deserialize, Serialize};

use super::{FileId, FileStatus, HunkId, ReviewNoteId, RowId, Side};

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReviewRow {
    pub id: RowId,
    pub ordinal: usize,
    pub file_id: Option<FileId>,
    pub hunk_id: Option<HunkId>,
    pub kind: ReviewRowKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRowKind {
    FileHeader {
        path: String,
        status: FileStatus,
    },
    HunkHeader {
        header: String,
    },
    Diff {
        row: DiffRow,
    },
    Metadata {
        metadata: FileMetadataRow,
    },
    Note {
        note_id: ReviewNoteId,
        target_row_id: RowId,
        title: String,
    },
    EmptyState {
        message: String,
    },
}
