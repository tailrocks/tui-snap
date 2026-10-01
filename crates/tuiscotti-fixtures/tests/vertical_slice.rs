//! M2 vertical slice, part 1: pure settings view.
//!
//! `settings_view`: a small production-style settings model loaded
//! from `tests/fixtures/slice/settings.json`, rendered by a real draw closure
//! (header `Paragraph`, stateful `Table` with a selected row, footer hint,
//! explicit cursor) through the production adapter
//! (`tuiscotti::ratatui::render_screen`), gated by `assert_snapshot!` and
//! `assert_screenshot!`.
//!
//! Item 2 (`cli_error`, the piped-CLI-error projection) lives in
//! `tuiscotti-cli/tests/vertical_slice_cli_error.rs`: stable cargo cannot
//! express a cross-package binary dependency, so the binary test runs where
//! `CARGO_BIN_EXE_tuiscotti` is set.
//!
//! `INSTA_UPDATE` stays ambient (read-only): Insta exposes no `Settings`
//! switch for the update behavior, and `set_var` is an `unsafe fn` in edition
//! 2024 that cannot be used under the workspace lints. Committed snapshots
//! match, so green-path assertions hold under every mode; run with
//! `INSTA_UPDATE=no` for fail-clean (never auto-bless) or
//! `INSTA_UPDATE=always` to regenerate approvals.
//!
//! The test runs inside a [`tuiscotti::runner::TestContext`] (attempt-qualified
//! scratch isolation, child-only env) and finishes with a journal completion
//! marker; completion is asserted, not assumed.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell as TCell, Paragraph, Row, Table, TableState},
};
use tuiscotti::ratatui::{EdgePolicy, render_screen};
use tuiscotti::runner::{Journal, JournalStatus, TestContext};

/// Explicit snapshot dirs: the committed `tests/snapshots` (absolute: the
/// facade's caller-derived default is a *relative* path, which Insta resolves
/// against the facade crate instead of this test). Ambient
/// `TUISCOTTI_SNAPSHOT_DIR` defaulting is replaced by an explicit
/// `tuiscotti::assert::Policy::EvolvingIn`, since `set_var` is unavailable;
/// evidence keeps the default `tuiscotti::assert::evidence_dir`.
fn policy() -> tuiscotti::assert::Policy {
    tuiscotti::assert::Policy::EvolvingIn {
        snapshots: std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
        evidence: tuiscotti::assert::evidence_dir(),
    }
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/slice")
        .join(name)
}

// ---------------------------------------------------------------------------
// Item 1: pure settings view
// ---------------------------------------------------------------------------

/// Production-style settings model: owned data + selection state, loaded from
/// a committed fixture. The view below is the real render path for this model.
#[derive(Debug, serde::Deserialize)]
struct Settings {
    title: String,
    selected: usize,
    rows: Vec<SettingRow>,
    hint: String,
}

#[derive(Debug, serde::Deserialize)]
struct SettingRow {
    key: String,
    value: String,
    enabled: bool,
}

impl Settings {
    fn load() -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(fixture_path("settings.json"))?;
        Ok(serde_json::from_str(&text)?)
    }
}

/// Real draw closure for [`Settings`]: styled header, stateful table with a
/// highlighted selected row, dim footer hint, and an explicit cursor parked on
/// the selected row's value cell.
fn draw_settings(frame: &mut ratatui::Frame<'_>, model: &Settings) {
    let area = frame.area();
    let header = Rect::new(0, 0, area.width, 3);
    let table_rect = Rect::new(1, 4, area.width.saturating_sub(2), 11);
    let footer = Rect::new(0, area.height.saturating_sub(2), area.width, 2);

    let title = Paragraph::new(model.title.as_str())
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL).title(" tuiscotti "));
    frame.render_widget(title, header);

    let header_row = Row::new(vec![
        TCell::from("key"),
        TCell::from("value"),
        TCell::from("status"),
    ])
    .style(Style::default().add_modifier(Modifier::UNDERLINED));
    let rows: Vec<Row<'_>> = model
        .rows
        .iter()
        .map(|r| {
            Row::new(vec![
                TCell::from(r.key.as_str()),
                TCell::from(r.value.as_str()),
                TCell::from(if r.enabled { "enabled" } else { "disabled" }),
            ])
            .style(Style::default().fg(if r.enabled {
                Color::Green
            } else {
                Color::DarkGray
            }))
        })
        .collect();
    let widths = [
        ratatui::layout::Constraint::Length(16),
        ratatui::layout::Constraint::Length(20),
        ratatui::layout::Constraint::Min(8),
    ];
    let table = Table::new(rows, widths)
        .header(header_row)
        .block(Block::default().borders(Borders::ALL).title(" settings "))
        .row_highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = TableState::default();
    state.select(Some(model.selected));
    frame.render_stateful_widget(table, table_rect, &mut state);

    let hint = Paragraph::new(model.hint.as_str()).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(hint, footer);

    // Cursor on the selected row's value cell: table x + left border (1) +
    // key column (16) + column spacing (1); y = table y + top border (1) +
    // header row (1) + selected index.
    let cursor_x = table_rect.x + 1 + 16 + 1;
    let selected = u16::try_from(model.selected).unwrap_or(u16::MAX);
    let cursor_y = table_rect.y + 1 + 1 + selected;
    frame.set_cursor_position((cursor_x, cursor_y));
}

/// Plain-text grid of a screen (symbols only, row-major) for content probes.
/// Style/cursor pinning belongs to the insta snapshots, not to this helper.
fn screen_text(screen: &tuiscotti::Screen) -> String {
    let mut out = String::new();
    for y in 0..screen.rows() {
        for x in 0..screen.cols() {
            if let Some(cell) = screen.get(x, y)
                && !cell.continuation
            {
                out.push_str(&cell.symbol);
            }
        }
        out.push('\n');
    }
    out
}

#[test]
fn settings_view() {
    let policy = policy();
    let ctx = TestContext::current("settings-view").expect("test context");
    let mut journal = Journal::open(&ctx.journal_path()).expect("open journal");
    journal
        .append("start", "settings-view")
        .expect("journal start");

    let model = Settings::load().expect("load settings fixture");
    assert_eq!(model.rows.len(), 4, "fixture row count");
    assert!(
        model.selected < model.rows.len(),
        "fixture selection in range"
    );

    let capture = render_screen(64, 18, |f| draw_settings(f, &model), EdgePolicy::default())
        .expect("render settings screen");
    assert!(
        !capture.has_clips(),
        "no wide-glyph clips expected: {:?}",
        capture.clipped
    );
    assert!(
        capture.notes.is_empty(),
        "no capture notes: {:?}",
        capture.notes
    );
    let screen = capture.into_screen();
    assert_eq!((screen.cols(), screen.rows()), (64, 18));
    assert!(screen.cursor().visible, "explicit cursor must survive");
    assert_eq!(
        (screen.cursor().x, screen.cursor().y),
        (19, 7),
        "cursor parked on the selected value cell"
    );
    let text = screen_text(&screen);
    for needle in ["Settings", "autosave", "tab_width", "disabled", "q quit"] {
        assert!(text.contains(needle), "view shows {needle:?}:\n{text}");
    }
    journal
        .append("rendered", "64x18 settings screen")
        .expect("journal");

    tuiscotti::assert_snapshot!("vertical_slice__settings_view", &screen, &policy);
    tuiscotti::assert_screenshot!("vertical_slice__settings_view_shot", &screen, &policy);

    // This test spawns no children: nothing to leak, no global state beyond
    // ambient INSTA_UPDATE. The completion marker proves the run
    // reached its end; the status read-back proves the marker + tail agree.
    journal.complete("pass").expect("journal complete");
    match Journal::status(ctx.scratch_dir()) {
        JournalStatus::Complete { status } => assert_eq!(status, "pass"),
        JournalStatus::Incomplete { reason } => panic!("journal incomplete: {reason}"),
    }
}
