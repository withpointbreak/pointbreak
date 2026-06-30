use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};
use shoreline::dump::{DumpDocument, DumpInputSource};
use shoreline::highlight::{TokenKind, TokenSpan};
use shoreline::model::{LineRange, ResolutionStatus, ReviewNoteId, ReviewRow, ReviewRowKind};
use shoreline::sidecar::ReviewNotesDiagnosticCode;

use crate::tui::app::TuiApp;
use crate::tui::view::{DisplayRow, DisplayRowKind};

pub(crate) fn render(frame: &mut Frame<'_>, app: &TuiApp) {
    let area = frame.area();
    let shell = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(header(app.document()), shell[0]);
    render_body(frame, app, shell[1]);
    frame.render_widget(footer(app), shell[2]);
}

fn render_body(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    if area.width < 80 {
        render_stream(frame, app, area);
    } else {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(area);
        render_stream(frame, app, panes[0]);
        render_detail(frame, app, panes[1]);
    }
}

fn header(document: &DumpDocument) -> Paragraph<'static> {
    Paragraph::new(format!(
        "shore show | {} | files {} hunks {} rows {} notes {} diagnostics {}",
        input_source(&document.input.source),
        document.summary.file_count,
        document.summary.hunk_count,
        document.summary.row_count,
        document.summary.note_count,
        document.summary.diagnostic_count
    ))
}

fn footer(app: &TuiApp) -> Paragraph<'static> {
    if let Some(error) = app.last_reload_error() {
        Paragraph::new(error.to_owned())
    } else {
        Paragraph::new("q/Esc quit | j/k rows | [/] hunks | {/} note hunks | r reload")
    }
}

fn render_stream(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let body_height = area.height.saturating_sub(2) as usize;
    let lines = app
        .document()
        .stream
        .rows
        .iter()
        .skip(app.scroll_top())
        .take(body_height)
        .map(|row| render_diff_line(app, row))
        .collect::<Vec<_>>();

    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title("Review"));
    frame.render_widget(paragraph, area);
}

/// Build one stream row's rendered line. A selected row uses the single selection style (tokens
/// suppressed). An unhighlighted row (non-diff, unknown language, or over the size cap) renders
/// exactly as before. A highlighted diff row becomes a multi-span line: the add/remove signal
/// moves to a background tint while the code carries syntax foreground.
fn render_diff_line(app: &TuiApp, row: &ReviewRow) -> Line<'static> {
    let display = DisplayRow::from_review_row(row);
    if app.cursor().row_id.as_ref() == Some(&row.id) {
        return Line::styled(display_text(&display), selected_row_style());
    }
    let spans = app.highlights_for(&row.id);
    if spans.is_empty() {
        return Line::styled(display_text(&display), row_style(display.kind));
    }
    build_highlighted_line(&display, spans, color_depth())
}

/// Terminal color capability, used to choose between truecolor and named-ANSI palettes.
#[derive(Clone, Copy)]
enum ColorDepth {
    Truecolor,
    Named,
}

/// Truecolor only when the terminal advertises it; otherwise the named-ANSI palette (which also
/// renders acceptably on 16-color terminals). No new dependency — just the `COLORTERM` convention.
fn color_depth() -> ColorDepth {
    match std::env::var("COLORTERM").ok().as_deref() {
        Some("truecolor") | Some("24bit") => ColorDepth::Truecolor,
        _ => ColorDepth::Named,
    }
}

/// Build a highlighted diff row as a prefix span, a gutter span (both carrying the add/remove
/// signal), and one code span per attributed segment. The segment sweep splits the raw text at the
/// span boundaries so a later intraline channel can add another attribute without changing this.
fn build_highlighted_line(
    display: &DisplayRow,
    spans: &[TokenSpan],
    depth: ColorDepth,
) -> Line<'static> {
    let base = match diff_bg_tint(display.kind, depth) {
        Some(bg) => Style::default().bg(bg),
        None => Style::default(),
    };
    let signal_style = match diff_signal_fg(display.kind) {
        Some(fg) => base.fg(fg),
        None => base,
    };
    let mut out: Vec<Span<'static>> = Vec::new();
    out.push(Span::styled(
        format!("{:<4} ", display.prefix),
        signal_style,
    ));
    out.push(Span::styled(format!("{} ", display.gutter), signal_style));
    append_code_segments(&mut out, &display.text, spans, base, depth);
    Line::from(out)
}

/// Append one span per code segment: each token span carries its syntax foreground over `base`, and
/// the plain gaps between spans carry `base` alone. A malformed span (reversed, out of range, or off
/// a char boundary) is skipped so the row still renders.
fn append_code_segments(
    out: &mut Vec<Span<'static>>,
    text: &str,
    spans: &[TokenSpan],
    base: Style,
    depth: ColorDepth,
) {
    let mut cursor = 0usize;
    for span in spans {
        if span.start < cursor
            || span.end < span.start
            || span.end > text.len()
            || !text.is_char_boundary(span.start)
            || !text.is_char_boundary(span.end)
        {
            continue;
        }
        if span.start > cursor {
            out.push(Span::styled(text[cursor..span.start].to_owned(), base));
        }
        out.push(Span::styled(
            text[span.start..span.end].to_owned(),
            base.fg(token_fg(span.kind, depth)),
        ));
        cursor = span.end;
    }
    if cursor < text.len() {
        out.push(Span::styled(text[cursor..].to_owned(), base));
    }
}

/// The add/remove signal foreground kept on the prefix and gutter of a highlighted diff row.
fn diff_signal_fg(kind: DisplayRowKind) -> Option<Color> {
    match kind {
        DisplayRowKind::Added => Some(Color::Green),
        DisplayRowKind::Removed => Some(Color::Red),
        _ => None,
    }
}

/// The subtle background tint for a changed row (truecolor only), freeing the code foreground for
/// syntax. Named/16-color terminals keep the plain background and rely on the prefix/gutter signal.
fn diff_bg_tint(kind: DisplayRowKind, depth: ColorDepth) -> Option<Color> {
    if !matches!(depth, ColorDepth::Truecolor) {
        return None;
    }
    match kind {
        DisplayRowKind::Added => Some(Color::Rgb(18, 38, 24)),
        DisplayRowKind::Removed => Some(Color::Rgb(46, 22, 22)),
        _ => None,
    }
}

/// The per-kind syntax foreground. Truecolor echoes the inspector's `--tok-*` hues; the named
/// palette respects the user's terminal theme and degrades cleanly to 16 colors.
fn token_fg(kind: TokenKind, depth: ColorDepth) -> Color {
    match depth {
        ColorDepth::Truecolor => match kind {
            TokenKind::Keyword => Color::Rgb(179, 136, 255),
            TokenKind::String => Color::Rgb(109, 210, 138),
            TokenKind::Comment => Color::Rgb(154, 165, 179),
            TokenKind::Number => Color::Rgb(79, 208, 192),
            TokenKind::Type => Color::Rgb(138, 180, 248),
            TokenKind::Function => Color::Rgb(90, 169, 230),
            TokenKind::Constant => Color::Rgb(240, 183, 90),
            TokenKind::Operator => Color::Rgb(215, 221, 229),
            TokenKind::Punctuation => Color::Rgb(154, 165, 179),
            TokenKind::Variable => Color::Rgb(215, 221, 229),
            TokenKind::Plain => Color::Reset,
        },
        ColorDepth::Named => match kind {
            TokenKind::Keyword => Color::Magenta,
            TokenKind::String => Color::Green,
            TokenKind::Comment => Color::DarkGray,
            TokenKind::Number => Color::Cyan,
            TokenKind::Type => Color::Yellow,
            TokenKind::Function => Color::Blue,
            TokenKind::Constant => Color::LightYellow,
            TokenKind::Operator => Color::White,
            TokenKind::Punctuation => Color::Gray,
            TokenKind::Variable => Color::White,
            TokenKind::Plain => Color::Reset,
        },
    }
}

fn render_detail(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let (title, lines) = if let Some(detail) = selected_note_detail(app) {
        let mut lines = vec![Line::styled(
            detail.title,
            Style::default().add_modifier(Modifier::BOLD),
        )];
        lines.push(Line::from(""));
        if let Some(status_line) = detail.status_line {
            lines.push(Line::styled(
                status_line,
                Style::default().fg(Color::Yellow),
            ));
            lines.push(Line::from(""));
        }
        lines.push(Line::from(
            detail.body.unwrap_or_else(|| "No note body".to_owned()),
        ));
        ("Note", lines)
    } else if !app.document().diagnostics.is_empty() {
        (
            "Diagnostics",
            app.document()
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    Line::from(format!(
                        "{}: {}",
                        diagnostic_code(&diagnostic.code),
                        diagnostic.message
                    ))
                })
                .collect::<Vec<_>>(),
        )
    } else {
        (
            "Summary",
            vec![
                Line::from(format!("files: {}", app.document().summary.file_count)),
                Line::from(format!("hunks: {}", app.document().summary.hunk_count)),
                Line::from(format!("rows: {}", app.document().summary.row_count)),
                Line::from(format!("notes: {}", app.document().summary.note_count)),
            ],
        )
    };

    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(paragraph, area);
}

struct NoteDetail {
    title: String,
    status_line: Option<String>,
    body: Option<String>,
}

fn selected_note_detail(app: &TuiApp) -> Option<NoteDetail> {
    let selected_row = selected_row(app)?;
    match &selected_row.kind {
        ReviewRowKind::Note { note_id, title, .. } => Some(NoteDetail {
            title: title.clone(),
            status_line: None,
            body: note_body(app, note_id),
        }),
        ReviewRowKind::StaleNote {
            note_id,
            title,
            resolution_status,
            target_path,
            target_line_range,
        } => Some(NoteDetail {
            title: title.clone(),
            status_line: Some(format!(
                "status: {} at {}:{}",
                resolution_status_word(resolution_status),
                target_path,
                display_line_range(target_line_range),
            )),
            body: note_body(app, note_id),
        }),
        _ => None,
    }
}

fn selected_row(app: &TuiApp) -> Option<&ReviewRow> {
    let row_id = app.cursor().row_id.as_ref()?;
    app.document()
        .stream
        .rows
        .iter()
        .find(|row| &row.id == row_id)
}

fn note_body(app: &TuiApp, note_id: &ReviewNoteId) -> Option<String> {
    app.document()
        .notes
        .iter()
        .find(|note| &note.id == note_id)
        .and_then(|note| note.body.clone())
}

fn display_text(row: &DisplayRow) -> String {
    if row.prefix.is_empty() {
        row.text.clone()
    } else if row.gutter.is_empty() {
        format!("{:<4} {}", row.prefix, row.text)
    } else {
        format!("{:<4} {} {}", row.prefix, row.gutter, row.text)
    }
}

fn row_style(kind: DisplayRowKind) -> Style {
    match kind {
        DisplayRowKind::FileHeader => Style::default().fg(Color::Cyan),
        DisplayRowKind::HunkHeader => Style::default().fg(Color::Yellow),
        DisplayRowKind::Added => Style::default().fg(Color::Green),
        DisplayRowKind::Removed => Style::default().fg(Color::Red),
        DisplayRowKind::Context => Style::default(),
        DisplayRowKind::Metadata => Style::default().fg(Color::Magenta),
        DisplayRowKind::Note => Style::default().fg(Color::LightBlue),
        DisplayRowKind::StaleNote => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::ITALIC),
        DisplayRowKind::Empty => Style::default().fg(Color::DarkGray),
    }
}

fn resolution_status_word(status: &ResolutionStatus) -> &'static str {
    match status {
        ResolutionStatus::Stale => "stale",
        ResolutionStatus::Orphaned => "orphaned",
        ResolutionStatus::Exact => "exact",
        ResolutionStatus::Relocated => "relocated",
        ResolutionStatus::FileLevel => "file-level",
        ResolutionStatus::Unresolved => "unresolved",
    }
}

fn display_line_range(range: &LineRange) -> String {
    if range.start == range.end {
        range.start.to_string()
    } else {
        format!("{}-{}", range.start, range.end)
    }
}

fn selected_row_style() -> Style {
    Style::default()
        .fg(Color::White)
        .bg(Color::Blue)
        .add_modifier(Modifier::BOLD)
}

fn input_source(source: &DumpInputSource) -> &'static str {
    match source {
        DumpInputSource::None => "no notes",
        DumpInputSource::ReviewNotes => "review notes",
        DumpInputSource::Durable => "durable",
    }
}

fn diagnostic_code(code: &ReviewNotesDiagnosticCode) -> &'static str {
    match code {
        ReviewNotesDiagnosticCode::InvalidSchema => "invalid_schema",
        ReviewNotesDiagnosticCode::InvalidRange => "invalid_range",
        ReviewNotesDiagnosticCode::MissingFilePath => "missing_file_path",
        ReviewNotesDiagnosticCode::MissingNoteTarget => "missing_note_target",
        ReviewNotesDiagnosticCode::MissingNoteTitle => "missing_note_title",
        ReviewNotesDiagnosticCode::MissingNotes => "missing_notes",
        ReviewNotesDiagnosticCode::MissingVersion => "missing_version",
        ReviewNotesDiagnosticCode::StaleFilePath => "stale_file_path",
        ReviewNotesDiagnosticCode::UnresolvedNote => "unresolved_note",
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::{Color, Style};
    use ratatui::text::Span;
    use shoreline::dump::{DumpDocument, DumpInputSource, DumpInputSummary};
    use shoreline::highlight::{TokenKind, TokenSpan};
    use shoreline::model::{
        Anchor, DiffFile, DiffRow, DiffRowKind, DiffSnapshot, FileId, FileStatus, HunkId,
        LineRange, ObjectId, ResolutionStatus, ReviewHunk, ReviewId, ReviewNote, ReviewNoteId,
        ReviewNoteSource, ReviewRow, ReviewRowKind, ReviewStream, RowId, Side,
    };
    use shoreline::sidecar::{DiagnosticLevel, ReviewNotesDiagnostic, ReviewNotesDiagnosticCode};
    use shoreline::stream::ViewportSpec;

    use super::{
        ColorDepth, append_code_segments, render, render_diff_line, selected_row_style, token_fg,
    };
    use crate::tui::app::{TuiAction, TuiApp};

    #[test]
    fn render_frame_shows_diff_stream_and_note_detail() {
        let mut app = app_with_note(ViewportSpec::new(100, 20));
        app.handle_action(TuiAction::NextNoteHunk);

        let buffer = render_to_buffer(&app, 100, 20);

        assert!(buffer_contains(&buffer, "src/lib.rs"));
        assert!(buffer_contains(&buffer, "@@"));
        assert!(buffer_contains(
            &buffer,
            "decode_json keeps the error boundary explicit"
        ));
        assert!(buffer_contains(&buffer, "Full review note body"));
        assert!(buffer_contains(&buffer, "q"));
    }

    #[test]
    fn render_frame_shows_diagnostics_when_no_note_is_selected() {
        let app = app_with_diagnostic(ViewportSpec::new(100, 20));

        let buffer = render_to_buffer(&app, 100, 20);

        assert!(buffer_contains(&buffer, "Diagnostics"));
        assert!(buffer_contains(&buffer, "missing_note_title"));
        assert!(buffer_contains(&buffer, "missing title"));
    }

    #[test]
    fn render_frame_marks_selected_row() {
        let app = app_with_note(ViewportSpec::new(100, 20));

        let buffer = render_to_buffer(&app, 100, 20);

        assert_eq!(buffer[(1, 2)].style().bg, Some(Color::Blue));
    }

    #[test]
    fn render_frame_omits_detail_pane_below_eighty_columns() {
        let mut app = app_with_note(ViewportSpec::new(60, 12));
        app.handle_action(TuiAction::NextNoteHunk);

        let buffer = render_to_buffer(&app, 60, 12);

        assert!(buffer_contains(&buffer, "src/lib.rs"));
        assert!(!buffer_contains(&buffer, "Full review note body"));
    }

    #[test]
    fn render_frame_handles_tiny_terminals_without_panic() {
        let app = app_with_note(ViewportSpec::new(20, 4));

        let buffer = render_to_buffer(&app, 20, 4);

        assert_eq!(buffer.area.width, 20);
        assert_eq!(buffer.area.height, 4);
    }

    #[test]
    fn render_frame_shows_stale_note_row_in_body() {
        let app = TuiApp::new(
            document_with_stale_note(Vec::new()),
            ViewportSpec::new(100, 20),
        );

        let buffer = render_to_buffer(&app, 100, 20);
        let text = buffer_text(&buffer);

        assert!(
            text.contains("Stale anchor"),
            "stale note title missing; got:\n{text}",
        );
        assert!(
            text.contains("src/lib.rs:99"),
            "target path/line missing; got:\n{text}",
        );
        assert!(
            text.contains("(stale)"),
            "status marker missing; got:\n{text}",
        );
    }

    #[test]
    fn render_frame_shows_stale_note_detail_when_selected() {
        let mut app = TuiApp::new(
            document_with_stale_note(Vec::new()),
            ViewportSpec::new(100, 20),
        );
        let last_row_id = app
            .document()
            .stream
            .rows
            .last()
            .map(|row| row.id.clone())
            .expect("stream non-empty");
        while app.cursor().row_id.as_ref() != Some(&last_row_id) {
            app.handle_action(TuiAction::RowDown);
        }

        let buffer = render_to_buffer(&app, 100, 20);
        let text = buffer_text(&buffer);

        assert!(
            text.contains("Stale anchor"),
            "title missing in detail pane"
        );
        assert!(
            text.contains("status: stale at src/lib.rs:99"),
            "status line missing in detail pane; got:\n{text}",
        );
        assert!(
            text.contains("This anchor no longer matches"),
            "body missing in detail pane; got:\n{text}",
        );
    }

    #[test]
    fn render_frame_shows_reload_hint_in_footer() {
        let app = app_with_note(ViewportSpec::new(100, 20));

        let buffer = render_to_buffer(&app, 100, 20);
        let text = buffer_text(&buffer);

        assert!(
            text.contains("r reload"),
            "reload hint missing; got:\n{text}"
        );
    }

    #[test]
    fn render_frame_shows_reload_error_in_footer_when_present() {
        let mut app = app_with_note(ViewportSpec::new(100, 20));
        app.set_last_reload_error("reload failed: boom");

        let buffer = render_to_buffer(&app, 100, 20);
        let text = buffer_text(&buffer);

        assert!(
            text.contains("reload failed: boom"),
            "reload error missing; got:\n{text}"
        );
    }

    #[test]
    fn highlighted_row_builds_multiple_styled_spans() {
        // Cursor defaults to the first row (file header), so the .rs code row is unselected.
        let app = app_with_note(ViewportSpec::new(100, 20));
        let row = app
            .document()
            .stream
            .rows
            .iter()
            .find(|row| row.id == RowId::new("row:0002"))
            .expect("code row present");

        let line = render_diff_line(&app, row);

        assert!(line.spans.len() > 1);
        // a code segment carries a syntax foreground, distinct from the gutter/prefix span.
        assert!(line.spans.iter().any(|span| span.style.fg.is_some()));
    }

    #[test]
    fn selected_row_suppresses_tokens() {
        let mut app = app_with_note(ViewportSpec::new(100, 20));
        // Move the cursor onto the highlighted code row.
        while app.cursor().row_id.as_ref() != Some(&RowId::new("row:0002")) {
            app.handle_action(TuiAction::RowDown);
        }
        let row = app
            .document()
            .stream
            .rows
            .iter()
            .find(|row| row.id == RowId::new("row:0002"))
            .expect("code row present");

        let line = render_diff_line(&app, row);

        // The selected row renders as a single selection style with no per-token spans.
        assert_eq!(line.style, selected_row_style());
        assert!(line.spans.iter().all(|span| span.style == Style::default()));
    }

    #[test]
    fn token_fg_distinguishes_kinds() {
        assert_eq!(
            token_fg(TokenKind::Keyword, ColorDepth::Named),
            Color::Magenta
        );
        assert_eq!(token_fg(TokenKind::String, ColorDepth::Named), Color::Green);
        assert_ne!(
            token_fg(TokenKind::Keyword, ColorDepth::Named),
            token_fg(TokenKind::Number, ColorDepth::Named),
        );
    }

    #[test]
    fn append_code_segments_splits_on_adjacent_kinds() {
        let mut out: Vec<Span<'static>> = Vec::new();
        append_code_segments(
            &mut out,
            "ab",
            &[
                TokenSpan {
                    start: 0,
                    end: 1,
                    kind: TokenKind::Keyword,
                },
                TokenSpan {
                    start: 1,
                    end: 2,
                    kind: TokenKind::Number,
                },
            ],
            Style::default(),
            ColorDepth::Named,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].content, "a");
        assert_eq!(out[1].content, "b");
        assert_eq!(
            out[0].style.fg,
            Some(token_fg(TokenKind::Keyword, ColorDepth::Named))
        );
        assert_eq!(
            out[1].style.fg,
            Some(token_fg(TokenKind::Number, ColorDepth::Named))
        );
    }

    #[test]
    fn append_code_segments_leaves_gaps_plain() {
        let mut out: Vec<Span<'static>> = Vec::new();
        append_code_segments(
            &mut out,
            "a b",
            &[TokenSpan {
                start: 0,
                end: 1,
                kind: TokenKind::Keyword,
            }],
            Style::default(),
            ColorDepth::Named,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].content, " b");
        assert_eq!(out[1].style.fg, None); // the gap is plain
    }

    fn render_to_buffer(app: &TuiApp, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| render(frame, app))
            .expect("draw frame");
        terminal.backend().buffer().clone()
    }

    fn buffer_contains(buffer: &Buffer, needle: &str) -> bool {
        buffer_text(buffer).contains(needle)
    }

    fn buffer_text(buffer: &Buffer) -> String {
        let mut text = String::new();
        for row in 0..buffer.area.height {
            let line = (0..buffer.area.width)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>();
            text.push_str(line.trim_end());
            text.push('\n');
        }
        text
    }

    fn app_with_note(viewport: ViewportSpec) -> TuiApp {
        TuiApp::new(document_with_note(Vec::new()), viewport)
    }

    fn app_with_diagnostic(viewport: ViewportSpec) -> TuiApp {
        TuiApp::new(
            document_with_note(vec![ReviewNotesDiagnostic {
                level: DiagnosticLevel::Warning,
                code: ReviewNotesDiagnosticCode::MissingNoteTitle,
                path: "files[0].notes[0].title".to_owned(),
                message: "missing title".to_owned(),
            }]),
            viewport,
        )
    }

    fn document_with_note(diagnostics: Vec<ReviewNotesDiagnostic>) -> DumpDocument {
        let review_id = ReviewId::new("review:test");
        let object_id = ObjectId::new("snapshot:test");
        let file_id = FileId::new("src/lib.rs");
        let hunk_id = HunkId::new("hunk:0000");
        let note_id = ReviewNoteId::new("note:test");
        let diff_row = DiffRow {
            kind: DiffRowKind::Added,
            old_line: None,
            new_line: Some(9),
            text: "pub fn decode_json() {}".to_owned(),
        };
        let hunk = ReviewHunk {
            id: hunk_id.clone(),
            header: "@@ -8,0 +9,1 @@".to_owned(),
            old_start: 8,
            old_lines: 0,
            new_start: 9,
            new_lines: 1,
            rows: vec![diff_row.clone()],
        };
        let snapshot = DiffSnapshot::new(
            review_id.clone(),
            object_id.clone(),
            vec![DiffFile {
                id: file_id.clone(),
                status: FileStatus::Modified,
                old_path: Some("src/lib.rs".to_owned()),
                new_path: Some("src/lib.rs".to_owned()),
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
                hunks: vec![hunk.clone()],
            }],
        );
        let note = ReviewNote {
            id: note_id.clone(),
            anchor: Anchor {
                file_id: file_id.clone(),
                side: Side::New,
                line_range: LineRange::new(9, 9),
                hunk_signature: hunk.signature(),
                target_text_hash: "sha256:test".to_owned(),
                status: ResolutionStatus::Exact,
            },
            source: ReviewNoteSource::Sidecar,
            title: "decode_json keeps the error boundary explicit".to_owned(),
            body: Some("Full review note body in markdown.".to_owned()),
            tags: Vec::new(),
            confidence: None,
            external_source: None,
            author: Some("reviewer".to_owned()),
            created_at: None,
        };
        let stream = ReviewStream {
            review_id,
            object_id,
            rows: vec![
                ReviewRow {
                    id: RowId::new("row:0000"),
                    ordinal: 0,
                    file_id: Some(file_id.clone()),
                    hunk_id: None,
                    kind: ReviewRowKind::FileHeader {
                        path: "src/lib.rs".to_owned(),
                        status: FileStatus::Modified,
                    },
                },
                ReviewRow {
                    id: RowId::new("row:0001"),
                    ordinal: 1,
                    file_id: Some(file_id.clone()),
                    hunk_id: Some(hunk_id.clone()),
                    kind: ReviewRowKind::HunkHeader {
                        header: hunk.header,
                    },
                },
                ReviewRow {
                    id: RowId::new("row:0002"),
                    ordinal: 2,
                    file_id: Some(file_id.clone()),
                    hunk_id: Some(hunk_id.clone()),
                    kind: ReviewRowKind::Diff { row: diff_row },
                },
                ReviewRow {
                    id: RowId::new("row:0003"),
                    ordinal: 3,
                    file_id: Some(file_id),
                    hunk_id: Some(hunk_id),
                    kind: ReviewRowKind::Note {
                        note_id: note_id.clone(),
                        target_row_id: RowId::new("row:0002"),
                        title: note.title.clone(),
                    },
                },
            ],
        };

        DumpDocument::new(
            DumpInputSummary {
                source: DumpInputSource::ReviewNotes,
            },
            snapshot,
            vec![note],
            stream,
            diagnostics,
        )
    }

    fn document_with_stale_note(diagnostics: Vec<ReviewNotesDiagnostic>) -> DumpDocument {
        let mut document = document_with_note(diagnostics);
        let file_id = FileId::new("src/lib.rs");
        let note_id = ReviewNoteId::new("note:stale");

        let stale_note = ReviewNote {
            id: note_id.clone(),
            anchor: Anchor {
                file_id: file_id.clone(),
                side: Side::New,
                line_range: LineRange::new(99, 99),
                hunk_signature: "hunk:stale".to_owned(),
                target_text_hash: String::new(),
                status: ResolutionStatus::Stale,
            },
            source: ReviewNoteSource::Sidecar,
            title: "Stale anchor".to_owned(),
            body: Some("This anchor no longer matches.".to_owned()),
            tags: Vec::new(),
            confidence: None,
            external_source: None,
            author: None,
            created_at: None,
        };
        document.notes.push(stale_note);

        let next_ordinal = document.stream.rows.len();
        document.stream.rows.push(ReviewRow {
            id: RowId::new(format!("row:{next_ordinal:04}")),
            ordinal: next_ordinal,
            file_id: Some(file_id),
            hunk_id: None,
            kind: ReviewRowKind::StaleNote {
                note_id,
                title: "Stale anchor".to_owned(),
                resolution_status: ResolutionStatus::Stale,
                target_path: "src/lib.rs".to_owned(),
                target_line_range: LineRange::new(99, 99),
            },
        });
        document
    }
}
