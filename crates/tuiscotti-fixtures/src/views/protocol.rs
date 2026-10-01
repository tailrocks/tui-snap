//! Protocol fixture view: modes, input echo, resizes, and focus.
//!
//! [`render`] is the single rendering function shared by pure-view tests and
//! the `protocol_fixture` binary. [`step`] is the separate input controller.
//!
//! The view shows negotiated terminal modes (bracketed paste, focus
//! tracking), an echo area with an explicit cursor state, the last resize,
//! and a bounded event log — the resize/paste/input/focus matrix in one
//! screen. [`Model::demo`] is the deterministic populated model;
//! [`Model::empty`] shows every empty state at once.

use super::Theme;
use ratatui::Frame as RFrame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// Negotiated terminal modes the app believes are active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modes {
    /// Bracketed paste (DEC 2004) enabled.
    pub bracketed_paste: bool,
    /// Focus tracking (DEC 1004) enabled.
    pub focus_tracking: bool,
}

/// Cursor presentation of the echo area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorState {
    /// Visible block cursor at the end of the echo text.
    #[default]
    Block,
    /// Visible underline cursor.
    Underline,
    /// Visible bar cursor.
    Bar,
    /// Hidden cursor (content still renders).
    Hidden,
}

/// Protocol model: everything [`render`] reads.
#[derive(Debug, Clone)]
pub struct Model {
    /// Negotiated modes.
    pub modes: Modes,
    /// Echoed input text.
    pub echo: String,
    /// Echo-area cursor presentation.
    pub cursor: CursorState,
    /// Currently focused (true) or blurred (false).
    pub focused: bool,
    /// Last observed viewport size.
    pub size: (u16, u16),
    /// Bounded event log (newest last, capped at 64 entries).
    pub log: Vec<String>,
    /// Error banner text; `None` hides the banner.
    pub error: Option<String>,
    /// Theme.
    pub theme: Theme,
}

impl Model {
    /// Deterministic populated model at `size`.
    #[must_use]
    pub fn demo(theme: Theme, size: (u16, u16)) -> Self {
        Self {
            modes: Modes {
                bracketed_paste: true,
                focus_tracking: true,
            },
            echo: "ready".to_string(),
            cursor: CursorState::Block,
            focused: true,
            size,
            log: vec![
                "spawn: modes negotiated".to_string(),
                "input: \"ready\" echoed".to_string(),
            ],
            error: None,
            theme,
        }
    }

    /// Empty deterministic model: no echo, no log, modes off, blurred.
    #[must_use]
    pub fn empty(theme: Theme, size: (u16, u16)) -> Self {
        Self {
            modes: Modes::default(),
            echo: String::new(),
            cursor: CursorState::Hidden,
            focused: false,
            size,
            log: Vec::new(),
            error: None,
            theme,
        }
    }

    /// Append a log entry, keeping the newest 64.
    pub fn push_log(&mut self, entry: String) {
        self.log.push(entry);
        while self.log.len() > 64 {
            self.log.remove(0);
        }
    }
}

/// Controller input (crossterm mapping lives in the binary driver).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolKey {
    /// Append a character to the echo area.
    Char(char),
    /// Append pasted text (bracketed when negotiated).
    Paste(String),
    /// Delete the last echo character.
    Backspace,
    /// Record a viewport resize.
    Resize(u16, u16),
    /// Record a focus-in event.
    FocusIn,
    /// Record a focus-out event.
    FocusOut,
    /// Cycle the cursor presentation.
    CycleCursor,
    /// Toggle the bracketed-paste mode flag.
    TogglePaste,
    /// Quit the application.
    Quit,
}

/// Apply one controller input. Returns true when the app should quit.
pub fn step(model: &mut Model, key: &ProtocolKey) -> bool {
    match key {
        ProtocolKey::Quit => return true,
        ProtocolKey::Char(c) => {
            model.echo.push(*c);
            model.push_log(format!("input: {c:?} echoed"));
        }
        ProtocolKey::Paste(text) => {
            model.echo.push_str(text);
            let mode = if model.modes.bracketed_paste {
                "bracketed"
            } else {
                "plain"
            };
            model.push_log(format!("paste({mode}): {text:?}"));
        }
        ProtocolKey::Backspace => {
            model.echo.pop();
        }
        ProtocolKey::Resize(cols, rows) => {
            model.size = (*cols, *rows);
            model.push_log(format!("resize: {cols}x{rows}"));
        }
        ProtocolKey::FocusIn => {
            model.focused = true;
            model.push_log("focus: in".to_string());
        }
        ProtocolKey::FocusOut => {
            model.focused = false;
            model.push_log("focus: out".to_string());
        }
        ProtocolKey::CycleCursor => {
            model.cursor = match model.cursor {
                CursorState::Block => CursorState::Underline,
                CursorState::Underline => CursorState::Bar,
                CursorState::Bar => CursorState::Hidden,
                CursorState::Hidden => CursorState::Block,
            };
            model.push_log(format!("cursor: {:?}", model.cursor));
        }
        ProtocolKey::TogglePaste => {
            model.modes.bracketed_paste = !model.modes.bracketed_paste;
            model.push_log(format!("paste: {}", model.modes.bracketed_paste));
        }
    }
    model.error = None;
    false
}

/// Render the protocol view. Shared by pure-view tests and the live binary.
pub fn render(frame: &mut RFrame<'_>, model: &Model) {
    let area = frame.area();
    let root = Block::default().style(Style::default().bg(model.theme.bg()));
    frame.render_widget(root, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);
    render_modes(frame, model, rows[0]);
    render_echo(frame, model, rows[1]);
    render_log(frame, model, rows[2]);
    render_footer(frame, model, rows[3]);
}

/// Render the negotiated-modes row.
fn render_modes(frame: &mut RFrame<'_>, model: &Model, area: Rect) {
    let flag = |on: bool| {
        if on {
            Span::styled(
                "on",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled("off", Style::default().add_modifier(Modifier::DIM))
        }
    };
    let focus = if model.focused {
        Span::styled(
            "focused",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("blurred", Style::default().add_modifier(Modifier::DIM))
    };
    let modes_widget = Paragraph::new(Line::from(vec![
        Span::raw("paste="),
        flag(model.modes.bracketed_paste),
        Span::raw(" focus-track="),
        flag(model.modes.focus_tracking),
        Span::raw(" state="),
        focus,
    ]))
    .block(Block::default().borders(Borders::ALL).title("Modes"));
    frame.render_widget(modes_widget, area);
}

/// Render the echo area with its cursor presentation.
fn render_echo(frame: &mut RFrame<'_>, model: &Model, area: Rect) {
    let cursor_glyph = match model.cursor {
        CursorState::Block => "▌",
        CursorState::Underline => "▁",
        CursorState::Bar => "▏",
        CursorState::Hidden => "",
    };
    let echo = Paragraph::new(format!("{}{}", model.echo, cursor_glyph)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Echo ({:?})", model.cursor)),
    );
    frame.render_widget(echo, area);
    if model.cursor != CursorState::Hidden {
        // Degenerate viewports can push the echo line outside the grid:
        // place the cursor only when it lands inside.
        let grid = frame.area();
        let echo_cols = u16::try_from(model.echo.len()).unwrap_or(u16::MAX);
        let x = (area.x + 1 + echo_cols).min(area.right().saturating_sub(1));
        let y = area.y + 1;
        if x >= grid.x && x < grid.right() && y >= grid.y && y < grid.bottom() {
            frame.set_cursor_position((x, y));
        }
    }
}

/// Render the bounded event log (newest visible at the bottom).
fn render_log(frame: &mut RFrame<'_>, model: &Model, area: Rect) {
    if model.log.is_empty() {
        let empty = Paragraph::new("No events yet.")
            .style(Style::default().add_modifier(Modifier::DIM))
            .block(Block::default().borders(Borders::ALL).title("Events"));
        frame.render_widget(empty, area);
        return;
    }
    let inner = usize::from(area.height.saturating_sub(2));
    let lines: Vec<Line<'_>> = model
        .log
        .iter()
        .rev()
        .take(inner.max(1))
        .rev()
        .map(String::as_str)
        .map(Line::from)
        .collect();
    let log = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Events"));
    frame.render_widget(log, area);
}

/// Render the footer with size and error state.
fn render_footer(frame: &mut RFrame<'_>, model: &Model, area: Rect) {
    let (cols, rows) = model.size;
    let text = match &model.error {
        Some(error) => format!("ERROR: {error}"),
        None => format!("size={cols}x{rows} log={} (q quits)", model.log.len()),
    };
    let footer = Paragraph::new(text).style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_widget(footer, area);
}
