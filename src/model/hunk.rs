use serde::{Deserialize, Serialize};

use super::review_note::{push_normalized_line, sha256_prefixed};
use super::{DiffRow, DiffRowKind, HunkId};

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

impl ReviewHunk {
    pub fn signature(&self) -> String {
        hunk_signature(self)
    }
}

fn hunk_signature(hunk: &ReviewHunk) -> String {
    let mut payload = String::new();
    push_normalized_line(&mut payload, &hunk_header_range(&hunk.header));
    for row in &hunk.rows {
        let marker = match row.kind {
            DiffRowKind::Added => '+',
            DiffRowKind::Removed => '-',
            DiffRowKind::Context => continue,
        };
        let mut line = String::new();
        line.push(marker);
        line.push_str(&row.text);
        push_normalized_line(&mut payload, &line);
    }
    sha256_prefixed(&payload)
}

fn hunk_header_range(header: &str) -> String {
    let mut parts = header.splitn(3, "@@");
    if parts.next() == Some("")
        && let Some(range) = parts.next()
    {
        return format!("@@{range}@@");
    }
    header.to_owned()
}
