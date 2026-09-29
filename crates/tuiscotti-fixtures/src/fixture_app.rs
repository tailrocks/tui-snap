//! Fixture application: the executable target for PTY tests AND the
//! model/view source for headless tests.
//!
//! ```text
//! cargo build --example fixture_app                     # PTY target
//! ./target/debug/examples/fixture_app --screen home     # run it
//! ```
//!
//! Headless tests reuse [`Model`] + [`render_model`] directly (no subprocess):
//! ```rust,no_run
//! use ratatui::{backend::TestBackend, Terminal};
//! // in tests: #[path = "../examples/fixture_app.rs"] mod fixture_app;
//! ```
//!
//! Keys (PTY): `Up/Down` move selection, `Enter` increments, `q` quits.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Row, Table};
use ratatui::Frame as RFrame;

/// Which screen the app shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Table,
    Dialog,
    Glyphs,
}

/// Fixture model: everything the view reads. Tests construct this directly.
#[derive(Debug, Clone)]
pub struct Model {
    pub screen: Screen,
    pub count: i32,
    pub selected: usize,
    pub dark: bool,
    pub input: String,
    pub cursor_at_end: bool,
}

impl Model {
    #[must_use]
    pub fn new(screen: Screen, dark: bool) -> Self {
        Self {
            screen,
            count: 0,
            selected: 1,
            dark,
            input: String::new(),
            cursor_at_end: true,
        }
    }

    pub fn bg(&self) -> Color {
        if self.dark {
            Color::Black
        } else {
            Color::White
        }
    }

    pub fn fg(&self) -> Color {
        if self.dark {
            Color::Gray
        } else {
            Color::Black
        }
    }
}

/// The actual production view function. Both paths exercise THIS code.
pub fn render_model(f: &mut RFrame, model: &Model) {
    let area = f.area();
    let bg_block = Block::default().style(Style::default().bg(model.bg()).fg(model.fg()));
    f.render_widget(bg_block, area);
    match model.screen {
        Screen::Home => render_home(f, model, area),
        Screen::Table => render_table(f, model, area),
        Screen::Dialog => render_dialog(f, model, area),
        Screen::Glyphs => render_glyphs(f, model, area),
    }
}

fn render_home(f: &mut RFrame, model: &Model, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "tuisnap fixture ",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("count={}", model.count)),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Home"));
    f.render_widget(title, rows[0]);
    let items: Vec<ListItem> = (0..5)
        .map(|i| {
            let style = if i == model.selected {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(format!("row {i}")).style(style)
        })
        .collect();
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Rows"));
    f.render_widget(list, rows[1]);
}

fn render_table(f: &mut RFrame, model: &Model, area: Rect) {
    let rows: Vec<Row> = (0..8)
        .map(|i| {
            let mut row = Row::new(vec![format!("id-{i}"), format!("value {}", i * 7)]);
            if i == model.selected {
                row = row.style(
                    Style::default()
                        .bg(Color::Green)
                        .fg(Color::Black)
                        .add_modifier(Modifier::REVERSED),
                );
            }
            row
        })
        .collect();
    let table = Table::new(rows, [Constraint::Length(8), Constraint::Min(0)])
        .block(Block::default().borders(Borders::ALL).title("Table"))
        .header(
            Row::new(vec!["id", "value"])
                .style(Style::default().add_modifier(Modifier::UNDERLINED)),
        );
    f.render_widget(table, area);
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    Rect::new(x, y, w.min(area.width), h.min(area.height))
}

fn render_dialog(f: &mut RFrame, model: &Model, area: Rect) {
    render_home(f, model, area);
    let popup = centered(area, 40, 8);
    f.render_widget(Clear, popup);
    let text = Paragraph::new(vec![
        Line::from("Confirm the thing?"),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "[ Yes ]",
                Style::default().bg(Color::Green).fg(Color::Black),
            ),
            Span::raw("  "),
            Span::styled("[ No ]", Style::default().add_modifier(Modifier::DIM)),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).title("Dialog"));
    f.render_widget(text, popup);
}

fn render_glyphs(f: &mut RFrame, model: &Model, area: Rect) {
    let lines = vec![
        Line::from("Box: ╔═╗║╚═╝ ┌─┐│└─┘ ├┤┬┴┼"),
        Line::from("Blocks: █▓▒░ ▀▄■□▪▫"),
        Line::from("Braille: ⠋⠙⠹⠸⢰⣰⣹⣿"),
        Line::from("Icons: \u{f015} \u{e615} \u{f120} \u{2764} → ✓ ✗ ★"),
        Line::from("CJK: 日本語 한국어 中文"),
        Line::from("Emoji: 🦀 🎉 (tofu fallback documented)"),
        Line::from("Combining: e\u{301} a\u{308} o\u{303}"),
        Line::from(format!(
            "Input: {}{}",
            model.input,
            if model.cursor_at_end { "▌" } else { "" }
        )),
    ];
    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Glyphs"))
        .style(Style::default().bg(model.bg()).fg(model.fg()));
    f.render_widget(p, area);
    if model.screen == Screen::Glyphs {
        // Real hardware cursor at end of the input line (last row, col 8+len).
        let cx = 8u16.saturating_add(model.input.len() as u16);
        let cy = area.y + 8;
        f.set_cursor_position((cx.min(area.right().saturating_sub(1)), cy));
    }
}

// Binary-only CLI parsing (unused when tests include this file as a module).
#[allow(dead_code)]
fn parse_screen(s: &str) -> Screen {
    match s {
        "table" => Screen::Table,
        "dialog" => Screen::Dialog,
        "glyphs" => Screen::Glyphs,
        _ => Screen::Home,
    }
}

// Entry point when built as an example binary (unused when tests include
// this file as a module).
#[allow(dead_code)]
fn main() -> anyhow::Result<()> {
    use crossterm::event::{self, Event, KeyCode};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
    use ratatui::backend::CrosstermBackend;
    use ratatui::Terminal;
    use std::time::Duration;

    let mut screen = Screen::Home;
    let mut dark = true;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screen" => {
                if let Some(s) = args.next() {
                    screen = parse_screen(&s);
                }
            }
            "--theme" => {
                if let Some(t) = args.next() {
                    dark = t != "light";
                }
            }
            _ => {}
        }
    }
    let mut model = Model::new(screen, dark);
    enable_raw_mode()?;
    let mut term = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    loop {
        term.draw(|f| render_model(f, &model))?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(k) = event::read()? {
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Up => model.selected = model.selected.saturating_sub(1),
                    KeyCode::Down => model.selected = model.selected.saturating_add(1),
                    KeyCode::Enter => model.count += 1,
                    KeyCode::Char(c) => model.input.push(c),
                    KeyCode::Backspace => {
                        model.input.pop();
                    }
                    _ => {}
                }
            }
        }
    }
    disable_raw_mode()?;
    Ok(())
}
