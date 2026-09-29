//! Menu fixture view: settings list with filter, toggles, and errors.
//!
//! [`render`] is the single rendering function shared by pure-view tests and
//! the `menu_fixture` binary. [`step`] is the separate key controller.
//!
//! Deterministic models: [`Model::demo`] (populated list incl. a CJK row and
//! a disabled row), [`Model::empty`] (empty state), [`Model::with_error`]
//! (error popup). Selection markers are `[x]`/`[ ]` text so PTY journeys can
//! pin them with text locators.

use super::Theme;
use ratatui::Frame as RFrame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

/// Which pane has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    /// The item list.
    #[default]
    List,
    /// The filter input.
    Filter,
}

/// One menu row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Stable identity (also the locator text).
    pub name: String,
    /// Toggle state, rendered as `[x]` / `[ ]`.
    pub toggled: bool,
    /// Disabled rows render dimmed and cannot toggle.
    pub disabled: bool,
}

/// Menu model: everything [`render`] reads.
#[derive(Debug, Clone)]
pub struct Model {
    /// All rows (unfiltered).
    pub items: Vec<Item>,
    /// Index into the *visible* (filtered) rows.
    pub selected: usize,
    /// Filter substring; empty matches everything.
    pub filter: String,
    /// Focused pane.
    pub focus: Focus,
    /// Theme.
    pub theme: Theme,
    /// Error popup text; `None` hides the popup.
    pub error: Option<String>,
}

impl Model {
    /// Populated deterministic model: three ASCII rows, one CJK row, one
    /// disabled row. Selection starts on row 0.
    #[must_use]
    pub fn demo(theme: Theme) -> Self {
        Self {
            items: vec![
                Item {
                    name: "autosave".to_string(),
                    toggled: false,
                    disabled: false,
                },
                Item {
                    name: "line_numbers".to_string(),
                    toggled: false,
                    disabled: false,
                },
                Item {
                    name: "word_wrap".to_string(),
                    toggled: false,
                    disabled: false,
                },
                Item {
                    name: "日本語モード".to_string(),
                    toggled: false,
                    disabled: false,
                },
                Item {
                    name: "legacy_mode".to_string(),
                    toggled: false,
                    disabled: true,
                },
            ],
            selected: 0,
            filter: String::new(),
            focus: Focus::List,
            theme,
            error: None,
        }
    }

    /// Empty deterministic model: no rows at all.
    #[must_use]
    pub fn empty(theme: Theme) -> Self {
        Self {
            items: Vec::new(),
            selected: 0,
            filter: String::new(),
            focus: Focus::List,
            theme,
            error: None,
        }
    }

    /// Demo model with an error popup showing `message`.
    #[must_use]
    pub fn with_error(theme: Theme, message: &str) -> Self {
        let mut model = Self::demo(theme);
        model.error = Some(message.to_string());
        model
    }

    /// Rows matching [`Model::filter`], with their source indices.
    #[must_use]
    pub fn visible(&self) -> Vec<(usize, &Item)> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| self.filter.is_empty() || item.name.contains(&self.filter))
            .collect()
    }

    /// Clamp [`Model::selected`] into the visible rows.
    pub fn clamp_selection(&mut self) {
        let len = self.visible().len();
        if len == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(len - 1);
        }
    }
}

/// Controller input (crossterm mapping lives in the binary driver).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuKey {
    /// Move selection up.
    Up,
    /// Move selection down.
    Down,
    /// Toggle the selected row (no-op on disabled rows; rings an error
    /// when the list is empty).
    Toggle,
    /// Activate the selected row (same as toggle here).
    Enter,
    /// Focus the filter input.
    FocusFilter,
    /// Focus the list.
    FocusList,
    /// Append a character to the filter (filter focus only).
    Char(char),
    /// Delete the last filter character (filter focus only).
    Backspace,
    /// Dismiss the error popup.
    Escape,
    /// Quit the application.
    Quit,
}

/// Apply one controller key. Returns true when the app should quit.
pub fn step(model: &mut Model, key: &MenuKey) -> bool {
    match key {
        MenuKey::Quit => return true,
        MenuKey::Escape => {
            model.error = None;
        }
        MenuKey::FocusFilter => model.focus = Focus::Filter,
        MenuKey::FocusList => model.focus = Focus::List,
        MenuKey::Up => {
            model.selected = model.selected.saturating_sub(1);
        }
        MenuKey::Down => {
            model.selected = model.selected.saturating_add(1);
            model.clamp_selection();
        }
        MenuKey::Char(c) => {
            if model.focus == Focus::Filter {
                model.filter.push(*c);
                model.selected = 0;
            }
        }
        MenuKey::Backspace => {
            if model.focus == Focus::Filter {
                model.filter.pop();
                model.selected = 0;
            }
        }
        MenuKey::Toggle | MenuKey::Enter => toggle_selected(model),
    }
    model.clamp_selection();
    false
}

/// Toggle the selected visible row, ringing an error when impossible.
fn toggle_selected(model: &mut Model) {
    let selected = model.selected;
    let source = model.visible().get(selected).map(|(s, _)| *s);
    let Some(source) = source else {
        model.error = Some("nothing to toggle: list is empty".to_string());
        return;
    };
    let item = &mut model.items[source];
    if item.disabled {
        model.error = Some(format!("row {:?} is disabled", item.name));
    } else {
        item.toggled = !item.toggled;
    }
}

/// Render the menu. Shared by pure-view tests and the live binary.
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
    render_title(frame, model, rows[0]);
    render_filter(frame, model, rows[1]);
    render_list(frame, model, rows[2]);
    render_status(frame, model, rows[3]);
    if let Some(error) = &model.error {
        render_error(frame, area, error);
    }
}

/// Render the title bar with the toggled count.
fn render_title(frame: &mut RFrame<'_>, model: &Model, area: Rect) {
    let toggled = model.items.iter().filter(|i| i.toggled).count();
    let title = Paragraph::new(Line::from(vec![
        Span::styled("Settings ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!("(space toggles, q quits, {toggled} on)")),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Menu"));
    frame.render_widget(title, area);
}

/// Render the filter input with a hardware cursor when focused.
fn render_filter(frame: &mut RFrame<'_>, model: &Model, area: Rect) {
    let border = if model.focus == Focus::Filter {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let input = Paragraph::new(model.filter.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Filter (/ to focus)")
            .border_style(border),
    );
    frame.render_widget(input, area);
    if model.focus == Focus::Filter {
        // Degenerate viewports (fewer rows than panes) can push the input
        // line outside the grid: place the cursor only when it lands inside.
        let grid = frame.area();
        let filter_cols = u16::try_from(model.filter.len()).unwrap_or(u16::MAX);
        let x = (area.x + 1 + filter_cols).min(area.right().saturating_sub(1));
        let y = area.y + 1;
        if x >= grid.x && x < grid.right() && y >= grid.y && y < grid.bottom() {
            frame.set_cursor_position((x, y));
        }
    }
}

/// Render the visible rows with selection, markers, and disabled styling.
fn render_list(frame: &mut RFrame<'_>, model: &Model, area: Rect) {
    let visible = model.visible();
    if visible.is_empty() {
        let empty = Paragraph::new("No rows match.")
            .style(Style::default().add_modifier(Modifier::DIM))
            .block(Block::default().borders(Borders::ALL).title("Rows"));
        frame.render_widget(empty, area);
        return;
    }
    let items: Vec<ListItem<'_>> = visible
        .iter()
        .enumerate()
        .map(|(i, (_, item))| {
            let marker = if item.toggled { "[x]" } else { "[ ]" };
            let mut style = Style::default();
            if item.disabled {
                style = style.add_modifier(Modifier::DIM);
            }
            if i == model.selected {
                style = style
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD);
            }
            ListItem::new(format!("{marker} {}", item.name)).style(style)
        })
        .collect();
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Rows"));
    frame.render_widget(list, area);
}

/// Render the one-line status bar with focus and selection state.
fn render_status(frame: &mut RFrame<'_>, model: &Model, area: Rect) {
    let focus = match model.focus {
        Focus::List => "list",
        Focus::Filter => "filter",
    };
    let status = Paragraph::new(format!(
        "focus={focus} selected={} filter={:?}",
        model.selected, model.filter
    ))
    .style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_widget(status, area);
}

/// Render the centered error popup over the list.
fn render_error(frame: &mut RFrame<'_>, area: Rect, error: &str) {
    let popup = centered(area, 44, 7);
    frame.render_widget(Clear, popup);
    let text = Paragraph::new(vec![
        Line::from(Span::styled(
            "Error",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(error),
        Line::from(Span::styled(
            "Esc dismisses",
            Style::default().add_modifier(Modifier::DIM),
        )),
    ])
    .block(Block::default().borders(Borders::ALL).title("!"));
    frame.render_widget(text, popup);
}

/// Center a `w`×`h` rect inside `area`, clipped to `area`.
fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    Rect::new(x, y, w.min(area.width), h.min(area.height))
}
