//! Streams fixture view: scrollable log with mixed colors and glyphs.
//!
//! [`render`] is the single rendering function shared by pure-view tests and
//! the `streams_fixture` binary. [`step`] is the separate key controller.
//!
//! The deterministic [`Model::demo`] covers RGB, indexed, and default
//! colors, independent modifiers, styled spaces, wide continuations, CJK,
//! icons, and combining characters — the glyph matrix in one screen.

use super::Theme;
use ratatui::Frame as RFrame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// Log severity: selects the line color source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Default terminal color.
    Trace,
    /// Indexed palette color.
    Info,
    /// Direct RGB color.
    Warn,
    /// Bold + reversed indexed color.
    Error,
}

/// One log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// Severity.
    pub level: Level,
    /// Message text (may carry wide/combining glyphs).
    pub text: String,
}

/// Streams model: everything [`render`] reads.
#[derive(Debug, Clone)]
pub struct Model {
    /// All log lines, oldest first.
    pub lines: Vec<LogLine>,
    /// First visible line index (ignored while autoscroll holds).
    pub scroll: usize,
    /// Follow the tail instead of honoring [`Model::scroll`].
    pub autoscroll: bool,
    /// Highlighted line (absolute index), if any.
    pub selected: Option<usize>,
    /// Theme.
    pub theme: Theme,
}

impl Model {
    /// Deterministic demo model: every color source, every modifier family,
    /// styled spaces, wide CJK, icons, and combining characters.
    #[must_use]
    pub fn demo(theme: Theme) -> Self {
        Self {
            lines: vec![
                LogLine {
                    level: Level::Trace,
                    text: "trace: plain default color".to_string(),
                },
                LogLine {
                    level: Level::Info,
                    text: "info: indexed palette line".to_string(),
                },
                LogLine {
                    level: Level::Warn,
                    text: "warn: direct RGB line".to_string(),
                },
                LogLine {
                    level: Level::Error,
                    text: "error: bold reversed alert".to_string(),
                },
                LogLine {
                    level: Level::Info,
                    text: "wide: 日本語 한국어 中文".to_string(),
                },
                LogLine {
                    level: Level::Info,
                    text: "icons: → ✓ ✗ ★ ⠋".to_string(),
                },
                LogLine {
                    level: Level::Trace,
                    text: "combining: é ä õ".to_string(),
                },
                LogLine {
                    level: Level::Warn,
                    text: "spaced: padded   cells   here".to_string(),
                },
                LogLine {
                    level: Level::Info,
                    text: "box: ╔═╗║╚═╝ ┌─┐│└─┘".to_string(),
                },
                LogLine {
                    level: Level::Trace,
                    text: "tail: end of deterministic log".to_string(),
                },
            ],
            scroll: 0,
            autoscroll: true,
            selected: Some(3),
            theme,
        }
    }

    /// Empty deterministic model: no lines at all.
    #[must_use]
    pub fn empty(theme: Theme) -> Self {
        Self {
            lines: Vec::new(),
            scroll: 0,
            autoscroll: true,
            selected: None,
            theme,
        }
    }

    /// Count lines at `level`.
    #[must_use]
    pub fn count(&self, level: Level) -> usize {
        self.lines.iter().filter(|l| l.level == level).count()
    }
}

/// Controller input (crossterm mapping lives in the binary driver).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamsKey {
    /// Scroll one line up (disables autoscroll).
    Up,
    /// Scroll one line down.
    Down,
    /// Scroll one page up.
    PageUp,
    /// Scroll one page down.
    PageDown,
    /// Jump to the first line.
    Home,
    /// Jump to the tail (enables autoscroll).
    End,
    /// Toggle autoscroll.
    ToggleFollow,
    /// Quit the application.
    Quit,
}

/// Apply one controller key against a viewport of `view_rows` text rows.
/// Returns true when the app should quit.
pub fn step(model: &mut Model, key: &StreamsKey, view_rows: usize) -> bool {
    let max_scroll = model.lines.len().saturating_sub(view_rows.max(1));
    match key {
        StreamsKey::Quit => return true,
        StreamsKey::Up => {
            model.autoscroll = false;
            model.scroll = model.scroll.saturating_sub(1);
        }
        StreamsKey::Down => {
            model.scroll = model.scroll.saturating_add(1).min(max_scroll);
        }
        StreamsKey::PageUp => {
            model.autoscroll = false;
            model.scroll = model.scroll.saturating_sub(view_rows.max(1));
        }
        StreamsKey::PageDown => {
            model.scroll = model.scroll.saturating_add(view_rows.max(1)).min(max_scroll);
        }
        StreamsKey::Home => {
            model.autoscroll = false;
            model.scroll = 0;
        }
        StreamsKey::End => {
            model.autoscroll = true;
            model.scroll = max_scroll;
        }
        StreamsKey::ToggleFollow => {
            model.autoscroll = !model.autoscroll;
            if model.autoscroll {
                model.scroll = max_scroll;
            }
        }
    }
    model.scroll = model.scroll.min(max_scroll);
    false
}

/// Style for one log level: each level uses a different color source.
fn level_style(level: Level) -> Style {
    match level {
        Level::Trace => Style::default(),
        Level::Info => Style::default().fg(Color::Indexed(33)),
        Level::Warn => Style::default().fg(Color::Rgb(255, 170, 0)),
        Level::Error => Style::default()
            .fg(Color::Indexed(15))
            .bg(Color::Indexed(9))
            .add_modifier(Modifier::BOLD | Modifier::REVERSED),
    }
}

/// Render the streams view. Shared by pure-view tests and the live binary.
pub fn render(frame: &mut RFrame, model: &Model) {
    let area = frame.area();
    let root = Block::default().style(Style::default().bg(model.theme.bg()));
    frame.render_widget(root, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);
    render_header(frame, model, rows[0]);
    render_log(frame, model, rows[1]);
    render_footer(frame, model, rows[2]);
}

/// Render the header with per-level counts.
fn render_header(frame: &mut RFrame, model: &Model, area: Rect) {
    let header = Paragraph::new(Line::from(vec![
        Span::styled("Streams ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!(
            "trace={} info={} warn={} error={}",
            model.count(Level::Trace),
            model.count(Level::Info),
            model.count(Level::Warn),
            model.count(Level::Error),
        )),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Log"));
    frame.render_widget(header, area);
}

/// Render the visible log window with level styles and selection.
fn render_log(frame: &mut RFrame, model: &Model, area: Rect) {
    let inner_rows = usize::from(area.height.saturating_sub(2));
    if model.lines.is_empty() || inner_rows == 0 {
        let empty = Paragraph::new("No log lines.")
            .style(Style::default().add_modifier(Modifier::DIM))
            .block(Block::default().borders(Borders::ALL).title("Lines"));
        frame.render_widget(empty, area);
        return;
    }
    let max_scroll = model.lines.len().saturating_sub(inner_rows.max(1));
    let first = if model.autoscroll {
        max_scroll
    } else {
        model.scroll.min(max_scroll)
    };
    let lines: Vec<Line> = model
        .lines
        .iter()
        .enumerate()
        .skip(first)
        .take(inner_rows)
        .map(|(i, line)| style_line(model, i, line))
        .collect();
    let log = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Lines"));
    frame.render_widget(log, area);
}

/// Style one log line: level color plus selection/underline markers.
///
/// The warn line's trailing spaces are explicitly styled (background wash)
/// so styled-space projections have deterministic content.
fn style_line(model: &Model, index: usize, line: &LogLine) -> Line<'static> {
    let mut style = level_style(line.level);
    if model.selected == Some(index) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    let mut spans = vec![Span::styled(line.text.clone(), style)];
    if line.level == Level::Warn {
        spans.push(Span::styled("   ", style.bg(Color::Indexed(236))));
    }
    Line::from(spans)
}

/// Render the footer with scroll/follow state.
fn render_footer(frame: &mut RFrame, model: &Model, area: Rect) {
    let footer = Paragraph::new(format!(
        "scroll={} follow={} selected={:?} (End follows, q quits)",
        model.scroll, model.autoscroll, model.selected
    ))
    .style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_widget(footer, area);
}
