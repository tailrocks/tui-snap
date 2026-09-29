//! Matrix fixture screens: one deterministic render per screen.
//!
//! [`Screen`] × theme × size is the visual-gate matrix; each screen owns one
//! public render function below, sharing [`Model`]. Headless tests call the
//! render directly through `draw_frame`; there is no subprocess path.

use ratatui::Frame as RFrame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Row, Table};

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
        if self.dark { Color::Gray } else { Color::Black }
    }
}

/// Render one matrix screen. The [`Model::screen`] selects the screen; every
/// screen paints the theme background first.
pub fn render(f: &mut RFrame, model: &Model) {
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

/// Home screen: title bar plus a selectable row list.
pub fn render_home(f: &mut RFrame, model: &Model, area: Rect) {
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

/// Table screen: id/value grid with one reversed selection row.
pub fn render_table(f: &mut RFrame, model: &Model, area: Rect) {
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

/// Dialog screen: home beneath a centered confirmation popup.
pub fn render_dialog(f: &mut RFrame, model: &Model, area: Rect) {
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

/// Glyphs screen: Unicode coverage sampler plus a hardware cursor.
pub fn render_glyphs(f: &mut RFrame, model: &Model, area: Rect) {
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
