//! Surface-neutral attributed-segment sweep over one diff row's two span channels.
//!
//! [`attributed_segments`] merges the syntax-token channel ([`TokenSpan`]) and the intraline-emphasis
//! channel ([`EmphSpan`]) of a single row into a flat, gap-free list of minimal segments, each tagged
//! with its covering token kind and emphasis flag. It is the shared, surface-neutral form of the
//! per-row sweep each emit surface needs: the TUI's private `append_code_segments`
//! (`src/tui/render.rs`) is the ratatui-typed sibling and is a candidate follow-up to migrate onto
//! this helper.

use super::{EmphSpan, TokenKind, TokenSpan};

/// One minimal, contiguous piece of a diff row's text, tagged with its attributes.
///
/// `start`/`end` are byte offsets into the row text (UTF-8, always on char boundaries). Segments
/// returned by [`attributed_segments`] tile the whole string with no gaps or overlaps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttributedSegment {
    pub start: usize,
    pub end: usize,
    /// The covering syntax token kind, or `None` when no token covers this segment (plain text).
    pub kind: Option<TokenKind>,
    /// Whether intraline word-diff emphasis covers this segment.
    pub emphasized: bool,
}

/// Split `text` at the union of the (validity-filtered) token and emphasis span boundaries; tag each
/// minimal segment with its covering token kind and emphasis flag.
///
/// Best-effort: a span that is reversed, out of range, or not on a UTF-8 char boundary is dropped
/// from *its* channel independently, so a malformed channel degrades to plain rather than corrupting
/// the sweep — it never panics. Because every boundary is a validated char boundary, each
/// `text[a..b]` an emit surface slices from a returned segment is safe. Mirrors the TUI's
/// `append_code_segments` (`src/tui/render.rs`) but surface-neutral: no ratatui, no ANSI — the emit
/// surface maps segments to its own styling.
pub fn attributed_segments(
    text: &str,
    tokens: &[TokenSpan],
    emphasis: &[EmphSpan],
) -> Vec<AttributedSegment> {
    let len = text.len();
    let valid = |start: usize, end: usize| {
        start < end && end <= len && text.is_char_boundary(start) && text.is_char_boundary(end)
    };
    let toks: Vec<&TokenSpan> = tokens.iter().filter(|t| valid(t.start, t.end)).collect();
    let emph: Vec<&EmphSpan> = emphasis.iter().filter(|e| valid(e.start, e.end)).collect();

    // Union of both channels' boundaries plus the row endpoints, deduped and sorted. Every boundary
    // is a validated char boundary of `text`, so each `text[a..b]` slice is safe.
    let mut points: Vec<usize> = Vec::with_capacity(2 + 2 * (toks.len() + emph.len()));
    points.push(0);
    points.push(len);
    for t in &toks {
        points.push(t.start);
        points.push(t.end);
    }
    for e in &emph {
        points.push(e.start);
        points.push(e.end);
    }
    points.sort_unstable();
    points.dedup();

    let mut out = Vec::with_capacity(points.len().saturating_sub(1));
    for window in points.windows(2) {
        let (a, b) = (window[0], window[1]);
        if a >= b {
            continue;
        }
        let kind = toks
            .iter()
            .find(|t| t.start <= a && a < t.end)
            .map(|t| t.kind);
        let emphasized = emph.iter().any(|e| e.start <= a && a < e.end);
        out.push(AttributedSegment {
            start: a,
            end: b,
            kind,
            emphasized,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::{EmphSpan, TokenKind, TokenSpan};

    fn tok(start: usize, end: usize, kind: TokenKind) -> TokenSpan {
        TokenSpan { start, end, kind }
    }
    fn emph(start: usize, end: usize) -> EmphSpan {
        EmphSpan { start, end }
    }

    #[test]
    fn plain_text_is_one_unattributed_segment() {
        let segs = attributed_segments("hello", &[], &[]);
        assert_eq!(
            segs,
            vec![AttributedSegment {
                start: 0,
                end: 5,
                kind: None,
                emphasized: false
            }]
        );
    }

    #[test]
    fn splits_at_the_union_of_token_and_emphasis_boundaries() {
        // "let x = 1"  tokens: [0,3)=Keyword ; emphasis: [4,5) on `x`
        let segs =
            attributed_segments("let x = 1", &[tok(0, 3, TokenKind::Keyword)], &[emph(4, 5)]);
        // Expect: [0,3) keyword ; [3,4) plain ; [4,5) plain+emph ; [5,9) plain
        assert!(segs.iter().any(|s| s.start == 0
            && s.end == 3
            && s.kind == Some(TokenKind::Keyword)
            && !s.emphasized));
        assert!(
            segs.iter()
                .any(|s| s.start == 4 && s.end == 5 && s.kind.is_none() && s.emphasized)
        );
        // Segments tile the whole string with no gaps/overlaps.
        assert_eq!(segs.first().unwrap().start, 0);
        assert_eq!(segs.last().unwrap().end, "let x = 1".len());
        for w in segs.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
    }

    #[test]
    fn overlapping_token_and_emphasis_produce_a_segment_carrying_both() {
        // token [0,5)=String fully covers emphasis [1,3)
        let segs = attributed_segments("abcde", &[tok(0, 5, TokenKind::String)], &[emph(1, 3)]);
        assert!(segs.iter().any(|s| s.start == 1
            && s.end == 3
            && s.kind == Some(TokenKind::String)
            && s.emphasized));
    }

    #[test]
    fn drops_a_malformed_channel_without_panicking() {
        // out-of-range / reversed / non-char-boundary spans are dropped per channel, never panic.
        let text = "héllo"; // 'é' is 2 bytes -> the whole string is 6 bytes.
        let bad_token = tok(0, 999, TokenKind::Keyword); // end out of range
        let segs = attributed_segments(text, &[bad_token], &[]);
        // The bad token channel is ignored; result is the whole plain string.
        assert_eq!(
            segs,
            vec![AttributedSegment {
                start: 0,
                end: text.len(),
                kind: None,
                emphasized: false
            }]
        );
    }
}
