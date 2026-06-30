//! Best-effort, read-time syntax highlighting for diff views.
//!
//! A surface-neutral tokenizer core: it turns diff rows into [`TokenSpan`]s keyed by a token
//! [`TokenKind`], which the inspector (web) and the TUI each render in their own way. Tokens are a
//! view-only projection — they are never stored on the diff model and never affect the
//! content-addressed snapshot artifact.

// Scaffolding for the tokenizer core: detection and the line tokenizer land before their consumer
// (the file highlighter), so their items are briefly unused outside tests. The allows are removed
// once `highlight_file` wires them in.
#[allow(dead_code)]
mod syntax;
#[allow(dead_code)]
mod tokenize;

/// Surface-neutral classification of a token.
///
/// The lowercase [`TokenKind::as_str`] string is the single spelling shared across surfaces: the
/// web inspector's CSS class family and the TUI's style map both consume it. `Plain` denotes
/// unclassified text and is never emitted in a span (gaps between spans are implicitly `Plain`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenKind {
    Keyword,
    String,
    Comment,
    Number,
    Type,
    Function,
    Constant,
    Operator,
    Punctuation,
    Variable,
    Plain,
}

impl TokenKind {
    /// The stable lowercase wire spelling for this kind, shared verbatim by every surface.
    pub fn as_str(self) -> &'static str {
        match self {
            TokenKind::Keyword => "keyword",
            TokenKind::String => "string",
            TokenKind::Comment => "comment",
            TokenKind::Number => "number",
            TokenKind::Type => "type",
            TokenKind::Function => "function",
            TokenKind::Constant => "constant",
            TokenKind::Operator => "operator",
            TokenKind::Punctuation => "punctuation",
            TokenKind::Variable => "variable",
            TokenKind::Plain => "plain",
        }
    }
}

/// A classified, contiguous range of a single raw diff-row text.
///
/// `start`/`end` are **byte offsets into the raw `DiffRow.text`** (UTF-8). Emitted spans are
/// sorted, non-overlapping, and **non-`Plain` only** — any byte range not covered by a span is
/// implicitly [`TokenKind::Plain`]. `Copy` so callers can collect and sort spans cheaply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenSpan {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_kind_renders_lowercase_wire_str() {
        assert_eq!(TokenKind::Keyword.as_str(), "keyword");
        assert_eq!(TokenKind::Punctuation.as_str(), "punctuation");
        assert_eq!(TokenKind::Plain.as_str(), "plain");
    }

    #[test]
    fn token_span_holds_byte_range_and_kind() {
        let s = TokenSpan {
            start: 0,
            end: 3,
            kind: TokenKind::Keyword,
        };
        assert_eq!((s.start, s.end), (0, 3));
        assert_eq!(s.kind, TokenKind::Keyword);
    }
}
