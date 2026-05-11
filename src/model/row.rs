use serde::{Deserialize, Serialize};

use super::{FileId, FileStatus, HunkId, LineRange, ResolutionStatus, ReviewNoteId, RowId, Side};

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
    StaleNote {
        note_id: ReviewNoteId,
        title: String,
        resolution_status: ResolutionStatus,
        target_path: String,
        target_line_range: LineRange,
    },
    EmptyState {
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LineRange, ResolutionStatus, ReviewNoteId};

    #[test]
    fn stale_note_variant_round_trips_through_serde() {
        let kind = ReviewRowKind::StaleNote {
            note_id: ReviewNoteId::new("note:stale"),
            title: "Stale review note".to_owned(),
            resolution_status: ResolutionStatus::Stale,
            target_path: "src/lib.rs".to_owned(),
            target_line_range: LineRange::new(42, 42),
        };

        let json = serde_json::to_value(&kind).expect("serializes");
        let stale_note = &json["stale_note"];

        assert_eq!(stale_note["note_id"], "note:stale");
        assert_eq!(stale_note["title"], "Stale review note");
        assert_eq!(stale_note["resolution_status"], "stale");
        assert_eq!(stale_note["target_path"], "src/lib.rs");
        assert_eq!(stale_note["target_line_range"]["start"], 42);
        assert_eq!(stale_note["target_line_range"]["end"], 42);

        let decoded: ReviewRowKind = serde_json::from_value(json).expect("deserializes");
        assert_eq!(decoded, kind);
    }

    #[test]
    fn stale_note_serializes_orphaned_resolution_status() {
        let kind = ReviewRowKind::StaleNote {
            note_id: ReviewNoteId::new("note:orphan"),
            title: "Orphaned review note".to_owned(),
            resolution_status: ResolutionStatus::Orphaned,
            target_path: "src/gone.rs".to_owned(),
            target_line_range: LineRange::new(1, 3),
        };

        let json = serde_json::to_value(&kind).expect("serializes");
        let stale_note = &json["stale_note"];

        assert_eq!(stale_note["resolution_status"], "orphaned");
        assert_eq!(stale_note["target_line_range"]["start"], 1);
        assert_eq!(stale_note["target_line_range"]["end"], 3);
    }
}
