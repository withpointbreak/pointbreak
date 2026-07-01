//! Best-effort, read-time intraline emphasis for diff views.
//!
//! A second, view-only channel that marks the changed sub-spans within a paired removed/added line.
//! Like [`super::highlight_file`], the emphasis it produces is a projection: it is never stored on the
//! diff model and never affects the content-addressed snapshot artifact.

use std::collections::HashMap;

use similar::{Algorithm, DiffOp, capture_diff_slices};
use unicode_width::UnicodeWidthStr;

use super::RowKey;
use crate::model::DiffFile;

/// delta's `--max-line-distance` default: a (removed, added) pair whose width-ratio distance exceeds
/// this is treated as a wholesale replacement and left un-emphasized (a full-line rewrite lights up
/// nothing; only genuinely similar lines emphasize their changed words).
const MAX_LINE_DISTANCE: f64 = 0.6;

/// A changed sub-span within a diff row, as **byte offsets into the raw `DiffRow.text`** (mirrors
/// [`super::TokenSpan`] offsets). Emphasis is a boolean channel — there is no `kind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmphSpan {
    pub start: usize,
    pub end: usize,
}

/// delta's `\w` proxy: a word character is alphanumeric or `_`.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// delta-style tokenization (`src/edits.rs::tokenize`): a run of `\w` characters is one token; every
/// other character is its own token. Returns `(byte_start, byte_end)` ranges covering `line` in
/// order, with no gaps or overlaps, so callers can map `similar` edit ops back onto `DiffRow.text`
/// byte offsets. Char-based (not grapheme-based) — a deliberate simplification for this best-effort
/// channel.
fn tokenize(line: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut word_start: Option<usize> = None;
    for (i, c) in line.char_indices() {
        if is_word(c) {
            word_start.get_or_insert(i);
        } else {
            if let Some(ws) = word_start.take() {
                out.push((ws, i));
            }
            out.push((i, i + c.len_utf8()));
        }
    }
    if let Some(ws) = word_start.take() {
        out.push((ws, line.len()));
    }
    out
}

/// delta's exact Unicode display width (matching delta's `UnicodeWidthStr::width`): wide
/// (CJK/fullwidth) chars count 2, so width-ratio distances line up with delta's.
fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Display width of a byte-sliced section, with leading/trailing whitespace trimmed. Slicing is safe
/// because token ranges land on char boundaries by construction (see [`tokenize`]); trimming matches
/// delta's `annotate`, so internal spaces still count but a whitespace-only section scores width 0.
fn section_width(line: &str, start: usize, end: usize) -> usize {
    display_width(line[start..end].trim())
}

/// Merge sorted/overlapping spans into a minimal set of non-overlapping spans.
fn coalesce(mut spans: Vec<EmphSpan>) -> Vec<EmphSpan> {
    spans.sort_unstable_by_key(|s| s.start);
    let mut out: Vec<EmphSpan> = Vec::new();
    for s in spans {
        match out.last_mut() {
            Some(last) if s.start <= last.end => last.end = last.end.max(s.end),
            _ => out.push(s),
        }
    }
    out
}

/// Diff a `(minus, plus)` line pair. Returns the changed byte-spans on each side and delta's
/// width-ratio distance: `numer = Σ changed widths`, `denom = 2·Σ equal widths + Σ changed widths`,
/// `distance = numer/denom` (`0` when `denom == 0`). Width is per **section** (the coalesced op run),
/// trimmed (mirrors delta `edits.rs::annotate`/`compute_distance`), so a whitespace-only changed
/// section contributes `0` width and emits no span. Does **not** apply the [`MAX_LINE_DISTANCE`]
/// guard itself — the caller uses `distance` for both pairing and gating.
fn diff_pair(minus: &str, plus: &str) -> (Vec<EmphSpan>, Vec<EmphSpan>, f64) {
    let mt = tokenize(minus);
    let pt = tokenize(plus);
    let ms: Vec<&str> = mt.iter().map(|&(s, e)| &minus[s..e]).collect();
    let ps: Vec<&str> = pt.iter().map(|&(s, e)| &plus[s..e]).collect();

    let mut del = Vec::new();
    let mut ins = Vec::new();
    let (mut numer, mut denom) = (0usize, 0usize);

    // Byte range covering tokens `[i, i + len)` on one side.
    let span = |ranges: &[(usize, usize)], i: usize, len: usize| -> (usize, usize) {
        (ranges[i].0, ranges[i + len - 1].1)
    };

    for op in capture_diff_slices(Algorithm::Myers, &ms, &ps) {
        match op {
            DiffOp::Equal { old_index, len, .. } => {
                let (s, e) = span(&mt, old_index, len);
                denom += 2 * section_width(minus, s, e);
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                let (s, e) = span(&mt, old_index, old_len);
                let w = section_width(minus, s, e);
                numer += w;
                denom += w;
                if w > 0 {
                    del.push(EmphSpan { start: s, end: e });
                }
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                let (s, e) = span(&pt, new_index, new_len);
                let w = section_width(plus, s, e);
                numer += w;
                denom += w;
                if w > 0 {
                    ins.push(EmphSpan { start: s, end: e });
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                let (ms_, me_) = span(&mt, old_index, old_len);
                let wo = section_width(minus, ms_, me_);
                numer += wo;
                denom += wo;
                if wo > 0 {
                    del.push(EmphSpan {
                        start: ms_,
                        end: me_,
                    });
                }
                let (ps_, pe_) = span(&pt, new_index, new_len);
                let wn = section_width(plus, ps_, pe_);
                numer += wn;
                denom += wn;
                if wn > 0 {
                    ins.push(EmphSpan {
                        start: ps_,
                        end: pe_,
                    });
                }
            }
        }
    }

    let distance = if denom > 0 {
        numer as f64 / denom as f64
    } else {
        0.0
    };
    (coalesce(del), coalesce(ins), distance)
}

/// Intraline emphasis for a whole diff file, keyed by the same [`RowKey`] as
/// [`super::highlight_file`].
///
/// Stub: the real block-buffering/greedy-pairing algorithm arrives in a later task; until then any
/// file yields an empty map (render everything plain).
pub fn emphasis_file(_file: &DiffFile) -> HashMap<RowKey, Vec<EmphSpan>> {
    HashMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DiffFile, DiffRow, DiffRowKind, FileId, FileStatus, HunkId, ReviewHunk};

    fn row(kind: DiffRowKind, text: &str) -> DiffRow {
        DiffRow {
            kind,
            old_line: None,
            new_line: None,
            text: text.to_owned(),
        }
    }

    fn file_with(new_path: Option<&str>, rows: Vec<DiffRow>) -> DiffFile {
        DiffFile {
            id: FileId::new("file:a"),
            status: FileStatus::Modified,
            old_path: new_path.map(str::to_owned),
            new_path: new_path.map(str::to_owned),
            old_mode: None,
            new_mode: None,
            old_oid: None,
            new_oid: None,
            similarity: None,
            is_binary: false,
            is_submodule: false,
            is_mode_only: false,
            synthetic: false,
            metadata_rows: Vec::new(),
            hunks: vec![ReviewHunk {
                id: HunkId::new("hunk:1"),
                header: "@@ -1,4 +1,4 @@".to_owned(),
                old_start: 1,
                old_lines: 4,
                new_start: 1,
                new_lines: 4,
                rows,
            }],
        }
    }

    #[test]
    fn emphspan_is_copy_two_field() {
        let s = EmphSpan { start: 0, end: 3 };
        let t = s; // Copy, not move
        assert_eq!((s.start, s.end), (0, 3));
        assert_eq!((t.start, t.end), (0, 3));
    }

    #[test]
    fn emphasis_file_stub_is_empty() {
        // Any DiffFile → empty map for now (algorithm arrives in a later task).
        let file = file_with(
            Some("a.rs"),
            vec![
                row(DiffRowKind::Removed, "let b = 2;"),
                row(DiffRowKind::Added, "let b = 3;"),
            ],
        );
        assert!(emphasis_file(&file).is_empty());
    }

    #[test]
    fn tokenize_words_and_separators() {
        // "a.b cd" → word "a", '.', word "b", ' ', word "cd"
        assert_eq!(
            tokenize("a.b cd"),
            vec![(0, 1), (1, 2), (2, 3), (3, 4), (4, 6)]
        );
    }

    #[test]
    fn tokenize_empty_is_empty() {
        assert_eq!(tokenize(""), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn tokenize_concatenation_reconstructs_line() {
        let line = "let x = f(a, b);";
        let joined: String = tokenize(line).iter().map(|&(s, e)| &line[s..e]).collect();
        assert_eq!(joined, line);
    }

    #[test]
    fn tokenize_multibyte_word_run_is_one_token() {
        // A `\w+` run of multibyte chars stays one token; concatenation still reconstructs.
        let line = "café x";
        assert_eq!(tokenize(line), vec![(0, 5), (5, 6), (6, 7)]);
        let joined: String = tokenize(line).iter().map(|&(s, e)| &line[s..e]).collect();
        assert_eq!(joined, line);
    }

    #[test]
    fn display_width_is_unicode_display_width() {
        assert_eq!(display_width("café"), 4); // narrow chars: width == char count
        assert_eq!(display_width("한"), 2); // wide char: display width 2 (char count 1, 3 bytes)
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn diff_pair_single_word_substitution_emphasizes_only_that_word() {
        let (del, ins, dist) = diff_pair("let x = 1;", "let y = 1;");
        assert!(dist < 0.6, "similar lines are homologous, dist={dist}");
        assert_eq!(del, vec![EmphSpan { start: 4, end: 5 }]); // "x"
        assert_eq!(ins, vec![EmphSpan { start: 4, end: 5 }]); // "y"
    }

    #[test]
    fn diff_pair_wholesale_replacement_exceeds_guard() {
        let (_, _, dist) = diff_pair("alpha beta", "gamma delta");
        assert!(
            dist > 0.6,
            "fully-different words are a wholesale replacement, dist={dist}"
        );
    }

    #[test]
    fn diff_pair_trailing_whitespace_is_distance_zero_and_no_span() {
        let (del, ins, dist) = diff_pair("let x", "let x  ");
        assert_eq!(dist, 0.0);
        assert!(del.is_empty());
        assert!(ins.is_empty(), "whitespace-only insertion is suppressed");
    }
}
