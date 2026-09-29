//! M2 vertical slice, part 1: pure settings view + piped CLI error.
//!
//! Item 1 (`settings_view`): a small production-style settings model loaded
//! from `tests/fixtures/slice/settings.json`, rendered by a real draw closure
//! (header `Paragraph`, stateful `Table` with a selected row, footer hint,
//! explicit cursor) through the production adapter
//! (`tuiscotti::ratatui::render_screen`), gated by `assert_snapshot!` and
//! `assert_screenshot!`.
//!
//! Item 2 (`cli_error`): the real `tuisnap` binary run with a bad flag through
//! the piped adapter (`tuiscotti::command::Command::cargo_bin`), asserting the
//! exit code, usage text on stderr, and an insta snapshot of a documented
//! stdout/stderr/exit projection.
//!
//! `INSTA_UPDATE` stays ambient (read-only): Insta exposes no `Settings`
//! switch for the update behavior, and `set_var` is an `unsafe fn` in edition
//! 2024 that cannot be used under the workspace lints. Committed snapshots
//! match, so green-path assertions hold under every mode; run with
//! `INSTA_UPDATE=no` for fail-clean (never auto-bless) or
//! `INSTA_UPDATE=always` to regenerate approvals.
//!
//! Both tests run inside a [`tuiscotti::runner::TestContext`] (attempt-qualified
//! scratch isolation, child-only env) and finish with a journal completion
//! marker; completion is asserted, not assumed.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell as TCell, Paragraph, Row, Table, TableState},
};
use tuiscotti::command::{Command, Termination};
use tuiscotti::ratatui::{EdgePolicy, render_screen};
use tuiscotti::runner::{Journal, JournalStatus, TestContext};

/// Explicit snapshot dirs: the committed `tests/snapshots` (absolute: the
/// facade's caller-derived default is a *relative* path, which Insta resolves
/// against the facade crate instead of this test). The old
/// `TUISNAP_SNAPSHOT_DIR` defaulting is now an explicit
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
    fn load() -> Self {
        let text =
            std::fs::read_to_string(fixture_path("settings.json")).expect("read settings fixture");
        serde_json::from_str(&text).expect("parse settings fixture")
    }
}

/// Real draw closure for [`Settings`]: styled header, stateful table with a
/// highlighted selected row, dim footer hint, and an explicit cursor parked on
/// the selected row's value cell.
fn draw_settings(frame: &mut ratatui::Frame, model: &Settings) {
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
        .block(Block::default().borders(Borders::ALL).title(" tuisnap "));
    frame.render_widget(title, header);

    let header_row = Row::new(vec![
        TCell::from("key"),
        TCell::from("value"),
        TCell::from("status"),
    ])
    .style(Style::default().add_modifier(Modifier::UNDERLINED));
    let rows: Vec<Row> = model
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
    let cursor_y = table_rect.y + 1 + 1 + model.selected as u16;
    frame.set_cursor_position((cursor_x, cursor_y));
}

/// Plain-text grid of a screen (symbols only, row-major) for content probes.
/// Style/cursor pinning belongs to the insta snapshots, not to this helper.
fn screen_text(screen: &tuiscotti::Screen) -> String {
    let mut out = String::new();
    for y in 0..screen.rows() {
        for x in 0..screen.cols() {
            match screen.get(x, y) {
                Some(c) if !c.continuation => out.push_str(&c.symbol),
                _ => {}
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

    let model = Settings::load();
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

// ---------------------------------------------------------------------------
// Item 2: piped CLI error
// ---------------------------------------------------------------------------

/// Documented projection of a piped run for snapshot review: exit code (or
/// non-exit termination), per-stream byte lengths, then lossy stream bodies.
/// No environment, path, or timing data enters the projection, so it is stable
/// across machines and runners (the binary under test prints no paths for a
/// flag-parse error).
fn cli_projection(argv: &[&str], out: &tuiscotti::command::ProcessOutput) -> String {
    format!(
        "argv: tuisnap {}\ntermination: {:?}\nexit_code: {}\ntruncated: {}\n\
         --- stdout ({} bytes) ---\n{}\n--- stderr ({} bytes) ---\n{}",
        argv.join(" "),
        out.status,
        out.code().map_or("none".to_string(), |c| c.to_string()),
        out.truncated,
        out.stdout.len(),
        out.stdout_lossy(),
        out.stderr.len(),
        out.stderr_lossy(),
    )
}

#[test]
fn cli_error() {
    let ctx = TestContext::current("cli-error").expect("test context");
    let mut journal = Journal::open(&ctx.journal_path()).expect("open journal");
    journal.append("start", "cli-error").expect("journal start");

    let argv = ["--bad-flag"];
    let out = Command::cargo_bin("tuisnap").arg(argv[0]).run();
    journal
        .append("ran", &format!("status={:?}", out.status))
        .expect("journal");

    // Clap parse errors exit 2; the child is reaped (Exit, never a kill or
    // spawn failure) with complete output.
    assert_eq!(
        out.status,
        Termination::Exit(2),
        "bad flag must exit 2, got {:?} (error: {:?})",
        out.status,
        out.error
    );
    assert!(!out.truncated, "error output must be complete");
    assert!(out.stdout.is_empty(), "no stdout on parse error");
    let stderr = out.stderr_lossy();
    assert!(
        stderr.contains("Usage:"),
        "stderr carries usage text:\n{stderr}"
    );
    assert!(
        stderr.contains("--bad-flag"),
        "stderr names the offending flag:\n{stderr}"
    );

    // Piped run is fully reaped inside `run` (Exit status observed, pipes
    // drained): no leaked children by construction, and the runner touched no
    // global state (child env was never applied to this process).
    insta::assert_snapshot!("cli_error", cli_projection(&argv, &out));

    journal.complete("pass").expect("journal complete");
    match Journal::status(ctx.scratch_dir()) {
        JournalStatus::Complete { status } => assert_eq!(status, "pass"),
        JournalStatus::Incomplete { reason } => panic!("journal incomplete: {reason}"),
    }
}
