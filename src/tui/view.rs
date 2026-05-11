use shore::model::{
    DiffRow, DiffRowKind, FileStatus, LineRange, ResolutionStatus, ReviewRow, ReviewRowKind, RowId,
};
use shore::stream::ORPHAN_SECTION_PATH;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DisplayRowKind {
    FileHeader,
    HunkHeader,
    Added,
    Removed,
    Context,
    Metadata,
    Note,
    StaleNote,
    Empty,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DisplayRow {
    pub row_id: RowId,
    pub kind: DisplayRowKind,
    pub prefix: String,
    pub text: String,
}

impl DisplayRow {
    pub(crate) fn from_review_row(row: &ReviewRow) -> Self {
        let (kind, prefix, text) = match &row.kind {
            ReviewRowKind::FileHeader { path, status } => (
                DisplayRowKind::FileHeader,
                "",
                if path == ORPHAN_SECTION_PATH {
                    path.clone()
                } else {
                    format!("{path} ({})", display_file_status(status))
                },
            ),
            ReviewRowKind::HunkHeader { header } => {
                (DisplayRowKind::HunkHeader, "@@", header.clone())
            }
            ReviewRowKind::Diff { row } => display_diff_row(row),
            ReviewRowKind::Metadata { metadata } => {
                (DisplayRowKind::Metadata, "!", metadata.text.clone())
            }
            ReviewRowKind::Note { title, .. } => (DisplayRowKind::Note, "note", title.clone()),
            ReviewRowKind::StaleNote {
                title,
                resolution_status,
                target_path,
                target_line_range,
                ..
            } => (
                DisplayRowKind::StaleNote,
                "stale",
                format!(
                    "{title} — {target_path}:{} ({})",
                    display_line_range(target_line_range),
                    display_resolution_status(resolution_status),
                ),
            ),
            ReviewRowKind::EmptyState { message } => (DisplayRowKind::Empty, "", message.clone()),
        };

        Self {
            row_id: row.id.clone(),
            kind,
            prefix: prefix.to_owned(),
            text,
        }
    }
}

fn display_diff_row(row: &DiffRow) -> (DisplayRowKind, &'static str, String) {
    let (kind, prefix) = match row.kind {
        DiffRowKind::Added => (DisplayRowKind::Added, "+"),
        DiffRowKind::Removed => (DisplayRowKind::Removed, "-"),
        DiffRowKind::Context => (DisplayRowKind::Context, " "),
    };
    let old_line = display_line_number(row.old_line);
    let new_line = display_line_number(row.new_line);
    (kind, prefix, format!("{old_line} {new_line} {}", row.text))
}

fn display_line_number(line: Option<u32>) -> String {
    line.map(|line| line.to_string())
        .unwrap_or_else(|| "-".to_owned())
}

fn display_file_status(status: &FileStatus) -> &'static str {
    match status {
        FileStatus::Modified => "modified",
        FileStatus::Added => "added",
        FileStatus::Deleted => "deleted",
        FileStatus::Renamed => "renamed",
        FileStatus::Copied => "copied",
    }
}

fn display_line_range(range: &LineRange) -> String {
    if range.start == range.end {
        range.start.to_string()
    } else {
        format!("{}-{}", range.start, range.end)
    }
}

fn display_resolution_status(status: &ResolutionStatus) -> &'static str {
    match status {
        ResolutionStatus::Stale => "stale",
        ResolutionStatus::Orphaned => "orphaned",
        ResolutionStatus::Exact => "exact",
        ResolutionStatus::Relocated => "relocated",
        ResolutionStatus::FileLevel => "file-level",
        ResolutionStatus::Unresolved => "unresolved",
    }
}

#[cfg(test)]
mod tests {
    use shore::model::{
        DiffRow, DiffRowKind, FileMetadataKind, FileMetadataRow, FileStatus, LineRange,
        ResolutionStatus, ReviewNoteId, ReviewRow, ReviewRowKind, RowId,
    };
    use shore::stream::ORPHAN_SECTION_PATH;

    use super::{DisplayRow, DisplayRowKind};

    #[test]
    fn display_row_formats_file_headers_with_path_and_status() {
        let row = review_row(ReviewRowKind::FileHeader {
            path: "src/lib.rs".to_owned(),
            status: FileStatus::Modified,
        });

        let display = DisplayRow::from_review_row(&row);

        assert_eq!(display.row_id, RowId::new("row:test"));
        assert_eq!(display.kind, DisplayRowKind::FileHeader);
        assert_eq!(display.prefix, "");
        assert!(display.text.contains("src/lib.rs"));
        assert!(display.text.contains("modified"));
    }

    #[test]
    fn display_row_formats_hunk_headers() {
        let row = review_row(ReviewRowKind::HunkHeader {
            header: "@@ -1,2 +1,3 @@".to_owned(),
        });

        let display = DisplayRow::from_review_row(&row);

        assert_eq!(display.kind, DisplayRowKind::HunkHeader);
        assert_eq!(display.prefix, "@@");
        assert!(display.text.contains("@@ -1,2 +1,3 @@"));
    }

    #[test]
    fn display_row_formats_diff_rows_with_line_numbers_and_prefixes() {
        let added = DisplayRow::from_review_row(&review_row_for_diff(
            DiffRowKind::Added,
            None,
            Some(12),
            "let value = 1;",
        ));
        let removed = DisplayRow::from_review_row(&review_row_for_diff(
            DiffRowKind::Removed,
            Some(7),
            None,
            "let old = 1;",
        ));
        let context = DisplayRow::from_review_row(&review_row_for_diff(
            DiffRowKind::Context,
            Some(3),
            Some(3),
            "let same = true;",
        ));

        assert_eq!(added.kind, DisplayRowKind::Added);
        assert_eq!(added.prefix, "+");
        assert!(added.text.contains("12"));
        assert!(added.text.contains("let value = 1;"));

        assert_eq!(removed.kind, DisplayRowKind::Removed);
        assert_eq!(removed.prefix, "-");
        assert!(removed.text.contains("7"));
        assert!(removed.text.contains("let old = 1;"));

        assert_eq!(context.kind, DisplayRowKind::Context);
        assert_eq!(context.prefix, " ");
        assert!(context.text.contains("3"));
        assert!(context.text.contains("let same = true;"));
    }

    #[test]
    fn display_row_formats_metadata_note_and_empty_rows() {
        let metadata = DisplayRow::from_review_row(&review_row(ReviewRowKind::Metadata {
            metadata: FileMetadataRow {
                kind: FileMetadataKind::ModeChange,
                text: "mode changed from 100644 to 100755".to_owned(),
            },
        }));
        let note = DisplayRow::from_review_row(&review_row(ReviewRowKind::Note {
            note_id: ReviewNoteId::new("note:test"),
            target_row_id: RowId::new("row:target"),
            title: "Important review note".to_owned(),
        }));
        let empty = DisplayRow::from_review_row(&review_row(ReviewRowKind::EmptyState {
            message: "no changes".to_owned(),
        }));

        assert_eq!(metadata.kind, DisplayRowKind::Metadata);
        assert_eq!(metadata.prefix, "!");
        assert!(metadata.text.contains("mode changed"));

        assert_eq!(note.kind, DisplayRowKind::Note);
        assert_eq!(note.prefix, "note");
        assert!(note.text.contains("Important review note"));

        assert_eq!(empty.kind, DisplayRowKind::Empty);
        assert_eq!(empty.prefix, "");
        assert!(empty.text.contains("no changes"));
    }

    #[test]
    fn display_row_formats_stale_note_with_target_and_status() {
        let row = review_row(ReviewRowKind::StaleNote {
            note_id: ReviewNoteId::new("note:stale"),
            title: "Anchor drifted".to_owned(),
            resolution_status: ResolutionStatus::Stale,
            target_path: "src/lib.rs".to_owned(),
            target_line_range: LineRange::new(42, 42),
        });

        let display = DisplayRow::from_review_row(&row);

        assert_eq!(display.kind, DisplayRowKind::StaleNote);
        assert_eq!(display.prefix, "stale");
        assert!(
            display.text.contains("Anchor drifted"),
            "title missing from text: {:?}",
            display.text,
        );
        assert!(
            display.text.contains("src/lib.rs:42"),
            "target path/line missing: {:?}",
            display.text,
        );
        assert!(
            display.text.contains("stale"),
            "status missing: {:?}",
            display.text,
        );
    }

    #[test]
    fn display_row_formats_orphaned_note_with_status_word() {
        let row = review_row(ReviewRowKind::StaleNote {
            note_id: ReviewNoteId::new("note:orphan"),
            title: "File removed".to_owned(),
            resolution_status: ResolutionStatus::Orphaned,
            target_path: "src/gone.rs".to_owned(),
            target_line_range: LineRange::new(1, 3),
        });

        let display = DisplayRow::from_review_row(&row);

        assert_eq!(display.kind, DisplayRowKind::StaleNote);
        assert!(
            display.text.contains("src/gone.rs:1-3"),
            "expected line range 1-3 in text: {:?}",
            display.text,
        );
        assert!(
            display.text.contains("orphaned"),
            "expected orphaned status word in text: {:?}",
            display.text,
        );
    }

    #[test]
    fn display_row_skips_status_suffix_for_orphan_section_file_header() {
        let row = review_row(ReviewRowKind::FileHeader {
            path: ORPHAN_SECTION_PATH.to_owned(),
            status: FileStatus::Modified,
        });

        let display = DisplayRow::from_review_row(&row);

        assert_eq!(display.kind, DisplayRowKind::FileHeader);
        assert_eq!(display.text, ORPHAN_SECTION_PATH);
        assert!(
            !display.text.contains('('),
            "orphan section header should not carry a status suffix: {:?}",
            display.text,
        );
    }

    fn review_row(kind: ReviewRowKind) -> ReviewRow {
        ReviewRow {
            id: RowId::new("row:test"),
            ordinal: 0,
            file_id: None,
            hunk_id: None,
            kind,
        }
    }

    fn review_row_for_diff(
        kind: DiffRowKind,
        old_line: Option<u32>,
        new_line: Option<u32>,
        text: &str,
    ) -> ReviewRow {
        review_row(ReviewRowKind::Diff {
            row: DiffRow {
                kind,
                old_line,
                new_line,
                text: text.to_owned(),
            },
        })
    }
}
