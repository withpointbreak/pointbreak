use std::error::Error;
use std::io::{self, stdout};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use shore::stream::ViewportSpec;

use crate::tui::app::{TuiAction, TuiApp};
use crate::tui::render::render;

type TerminalResult<T> = Result<T, Box<dyn Error>>;

pub(crate) fn run(mut app: TuiApp) -> TerminalResult<()> {
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    while !app.should_quit() {
        terminal.draw(|frame| render(frame, &app))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => {
                if let Some(action) = action_for_key(key) {
                    app.handle_action(action);
                }
            }
            Event::Resize(width, height) => {
                app.handle_action(TuiAction::Resize(ViewportSpec::new(
                    width as usize,
                    height as usize,
                )));
            }
            _ => {}
        }
    }

    Ok(())
}

pub(crate) fn action_for_key(key: KeyEvent) -> Option<TuiAction> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    match key.code {
        KeyCode::Esc => Some(TuiAction::Quit),
        KeyCode::Up => Some(TuiAction::RowUp),
        KeyCode::Down => Some(TuiAction::RowDown),
        KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => Some(TuiAction::Quit),
        KeyCode::Char('q') if is_unmodified(key.modifiers) => Some(TuiAction::Quit),
        KeyCode::Char('j') if is_unmodified(key.modifiers) => Some(TuiAction::RowDown),
        KeyCode::Char('k') if is_unmodified(key.modifiers) => Some(TuiAction::RowUp),
        KeyCode::Char(']') if allows_shift(key.modifiers) => Some(TuiAction::NextHunk),
        KeyCode::Char('[') if allows_shift(key.modifiers) => Some(TuiAction::PreviousHunk),
        KeyCode::Char('}') if allows_shift(key.modifiers) => Some(TuiAction::NextNoteHunk),
        KeyCode::Char('{') if allows_shift(key.modifiers) => Some(TuiAction::PreviousNoteHunk),
        _ => None,
    }
}

fn is_unmodified(modifiers: KeyModifiers) -> bool {
    modifiers.is_empty()
}

fn allows_shift(modifiers: KeyModifiers) -> bool {
    modifiers.is_empty() || modifiers == KeyModifiers::SHIFT
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> TerminalResult<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(stdout(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(Box::new(error));
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{TerminalResult, action_for_key, run};
    use crate::tui::app::TuiAction;

    #[test]
    fn key_events_map_to_quit_actions() {
        assert_eq!(action_for_key(key_char('q')), Some(TuiAction::Quit));
        assert_eq!(
            action_for_key(key_code(KeyCode::Esc)),
            Some(TuiAction::Quit)
        );
        assert_eq!(
            action_for_key(key_code_with_modifiers(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL
            )),
            Some(TuiAction::Quit)
        );
    }

    #[test]
    fn key_events_map_to_row_actions() {
        assert_eq!(action_for_key(key_char('j')), Some(TuiAction::RowDown));
        assert_eq!(
            action_for_key(key_code(KeyCode::Down)),
            Some(TuiAction::RowDown)
        );
        assert_eq!(action_for_key(key_char('k')), Some(TuiAction::RowUp));
        assert_eq!(
            action_for_key(key_code(KeyCode::Up)),
            Some(TuiAction::RowUp)
        );
    }

    #[test]
    fn key_events_map_to_hunk_actions() {
        assert_eq!(action_for_key(key_char(']')), Some(TuiAction::NextHunk));
        assert_eq!(action_for_key(key_char('[')), Some(TuiAction::PreviousHunk));
        assert_eq!(action_for_key(key_char('}')), Some(TuiAction::NextNoteHunk));
        assert_eq!(
            action_for_key(key_char('{')),
            Some(TuiAction::PreviousNoteHunk)
        );
    }

    #[test]
    fn unrelated_key_events_are_ignored() {
        assert_eq!(action_for_key(key_char('x')), None);
        assert_eq!(
            action_for_key(key_code_with_modifiers(
                KeyCode::Char('q'),
                KeyModifiers::CONTROL
            )),
            None
        );
    }

    #[test]
    fn run_has_terminal_result_signature() {
        let _run: fn(crate::tui::app::TuiApp) -> TerminalResult<()> = run;
    }

    fn key_char(ch: char) -> KeyEvent {
        key_code(KeyCode::Char(ch))
    }

    fn key_code(code: KeyCode) -> KeyEvent {
        key_code_with_modifiers(code, KeyModifiers::NONE)
    }

    fn key_code_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }
}
