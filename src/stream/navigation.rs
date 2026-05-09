use serde::{Deserialize, Serialize};

use crate::model::{CursorState, HunkId, ReviewRowKind, ReviewStream, RowId};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationCommand {
    NextHunk,
    PreviousHunk,
    NextNoteHunk,
    PreviousNoteHunk,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NavigationResult {
    pub cursor: CursorState,
    pub reveal: Option<RevealTarget>,
    pub clamped: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevealTarget {
    Row { row_id: RowId },
}

fn navigate_review_stream(
    stream: &ReviewStream,
    cursor: &CursorState,
    command: NavigationCommand,
) -> NavigationResult {
    match command {
        NavigationCommand::NextHunk => navigate_hunk(stream, cursor, Direction::Next),
        NavigationCommand::PreviousHunk => navigate_hunk(stream, cursor, Direction::Previous),
        NavigationCommand::NextNoteHunk => navigate_note_hunk(stream, cursor, Direction::Next),
        NavigationCommand::PreviousNoteHunk => {
            navigate_note_hunk(stream, cursor, Direction::Previous)
        }
    }
}

impl ReviewStream {
    pub fn navigate(&self, cursor: &CursorState, command: NavigationCommand) -> NavigationResult {
        navigate_review_stream(self, cursor, command)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Direction {
    Next,
    Previous,
}

fn navigate_hunk(
    stream: &ReviewStream,
    cursor: &CursorState,
    direction: Direction,
) -> NavigationResult {
    let Some(selected_index) = selected_row_index(stream, cursor) else {
        return unresolved_result();
    };
    let hunk_indices = stream
        .rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            matches!(row.kind, ReviewRowKind::HunkHeader { .. }).then_some(index)
        })
        .collect::<Vec<_>>();

    if hunk_indices.is_empty() {
        return result_for_index(stream, selected_index, true);
    }

    let current_position = hunk_indices
        .iter()
        .rposition(|hunk_index| *hunk_index <= selected_index);
    let (target_index, clamped) = match (direction, current_position) {
        (Direction::Next, Some(position)) if position + 1 < hunk_indices.len() => {
            (hunk_indices[position + 1], false)
        }
        (Direction::Next, Some(position)) => (hunk_indices[position], true),
        (Direction::Next, None) => (hunk_indices[0], false),
        (Direction::Previous, Some(position)) if position > 0 => {
            (hunk_indices[position - 1], false)
        }
        (Direction::Previous, Some(position)) => (hunk_indices[position], true),
        (Direction::Previous, None) => (hunk_indices[0], true),
    };

    result_for_index(stream, target_index, clamped)
}

fn navigate_note_hunk(
    stream: &ReviewStream,
    cursor: &CursorState,
    direction: Direction,
) -> NavigationResult {
    let Some(selected_index) = selected_row_index(stream, cursor) else {
        return unresolved_result();
    };
    let targets = note_hunk_targets(stream);
    if targets.is_empty() {
        return result_for_index(stream, selected_index, true);
    }

    let current_hunk_index =
        current_hunk_header_index(stream, selected_index).unwrap_or(selected_index);
    let target = match direction {
        Direction::Next => targets
            .iter()
            .find(|target| target.hunk_header_index > current_hunk_index)
            .map(|target| (*target, false))
            .unwrap_or_else(|| (*targets.last().expect("targets exist"), true)),
        Direction::Previous => targets
            .iter()
            .rev()
            .find(|target| target.hunk_header_index < current_hunk_index)
            .map(|target| (*target, false))
            .unwrap_or_else(|| (targets[0], true)),
    };

    result_for_index(stream, target.0.note_row_index, target.1)
}

fn selected_row_index(stream: &ReviewStream, cursor: &CursorState) -> Option<usize> {
    match cursor.row_id.as_ref() {
        Some(row_id) => stream
            .rows
            .iter()
            .position(|row| &row.id == row_id)
            .or((!stream.rows.is_empty()).then_some(0)),
        None => (!stream.rows.is_empty()).then_some(0),
    }
}

fn current_hunk_header_index(stream: &ReviewStream, selected_index: usize) -> Option<usize> {
    stream.rows[..=selected_index]
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, row)| {
            matches!(row.kind, ReviewRowKind::HunkHeader { .. }).then_some(index)
        })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NoteHunkTarget {
    hunk_header_index: usize,
    note_row_index: usize,
}

fn note_hunk_targets(stream: &ReviewStream) -> Vec<NoteHunkTarget> {
    let mut targets = Vec::new();
    let mut seen_hunks = Vec::<HunkId>::new();

    for (note_row_index, row) in stream.rows.iter().enumerate() {
        if !matches!(row.kind, ReviewRowKind::Note { .. }) {
            continue;
        }
        let Some(hunk_id) = row.hunk_id.as_ref() else {
            continue;
        };
        if seen_hunks.contains(hunk_id) {
            continue;
        }
        let Some(hunk_header_index) = hunk_header_index(stream, hunk_id) else {
            continue;
        };
        seen_hunks.push(hunk_id.clone());
        targets.push(NoteHunkTarget {
            hunk_header_index,
            note_row_index,
        });
    }

    targets
}

fn hunk_header_index(stream: &ReviewStream, hunk_id: &HunkId) -> Option<usize> {
    stream.rows.iter().enumerate().find_map(|(index, row)| {
        (row.hunk_id.as_ref() == Some(hunk_id)
            && matches!(row.kind, ReviewRowKind::HunkHeader { .. }))
        .then_some(index)
    })
}

fn result_for_index(stream: &ReviewStream, row_index: usize, clamped: bool) -> NavigationResult {
    let row_id = stream.rows[row_index].id.clone();
    NavigationResult {
        cursor: CursorState::at_row(row_id.clone()),
        reveal: Some(RevealTarget::Row { row_id }),
        clamped,
    }
}

fn unresolved_result() -> NavigationResult {
    NavigationResult {
        cursor: CursorState::empty(),
        reveal: None,
        clamped: true,
    }
}
