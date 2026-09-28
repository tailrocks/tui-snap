//! M2 vertical slice, part 1: pure settings view + piped CLI error.
//!
//! Item 1 (`settings_view`): a small production-style settings model loaded
//! from `tests/fixtures/slice/settings.json`, rendered by a real draw closure
//! (header `Paragraph`, stateful `Table` with a selected row, footer hint,
//! explicit cursor) through the production adapter
//! (`tuisnap::ratatui::render_screen`), gated by `assert_snapshot!` and
//! `assert_screenshot!`.
//!
//! Item 2 (`cli_error`): the real `tuisnap` binary run with a bad flag through
//! the piped adapter (`tuisnap::command::Command::cargo_bin`), asserting the
//! exit code, usage text on stderr, and an insta snapshot of a documented
//! stdout/stderr/exit projection.
//!
//! `INSTA_UPDATE=no` semantics: Insta exposes no `Settings` switch for the
//! update behavior (see `src/assert.rs` docs), so the freeze is the env var,
//! defaulted in-process before the first Insta call of each test (Insta
//! memoizes tool config per binary). An explicit external value (e.g. the one
//! `INSTA_UPDATE=always` generation run) is honored, never overridden — CI
//! with an unset variable can never auto-bless.
//!
//! Both tests run inside a [`tuisnap::runner::TestContext`] (attempt-qualified
//! scratch isolation, child-only env) and finish with a journal completion
//! marker; completion is asserted, not assumed.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell as TCell, Paragraph, Row, Table, TableState},
};
use std::sync::OnceLock;
use tuisnap::command::{Command, Termination};
use tuisnap::ratatui::{render_screen, EdgePolicy};
use tuisnap::runner::{Journal, JournalStatus, TestContext};

/// Insta environment, fixed exactly once per test binary (env is
/// process-global and tests run in parallel threads, so per-test mutation
/// would race; the values are identical for every caller, making this benign).
/// Externally set values always win (generation runs, custom dirs).
///
/// Two settings:
/// - `INSTA_UPDATE=no` unless already set: Insta exposes no `Settings` switch
///   for the update behavior, so the freeze is the env var, defaulted before
///   the first Insta call (Insta memoizes tool config per binary). CI with an
///   unset variable can never auto-bless.
/// - `TUISNAP_SNAPSHOT_DIR=<absolute tests/snapshots>` unless already set:
///   the facade's caller-derived default is a *relative* path, which Insta
///   resolves against `src/assert.rs` (where the facade's `insta!` calls
///   textually expand), landing snapshots in `src/tests/snapshots`. An
///   absolute path resolves unambiguously to the committed directory.
fn freeze_insta_updates() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        if std::env::var_os("INSTA_UPDATE").is_none() {
            std::env::set_var("INSTA_UPDATE", "no");
        }
        if std::env::var_os(tuisnap::assert::SNAPSHOT_DIR_ENV).is_none() {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
            std::env::set_var(tuisnap::assert::SNAPSHOT_DIR_ENV, &dir);
        }
    });
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
fn screen_text(screen: &tuisnap::Screen) -> String {
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
    freeze_insta_updates();
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

    tuisnap::assert_snapshot!("vertical_slice__settings_view", &screen);
    tuisnap::assert_screenshot!("vertical_slice__settings_view_shot", &screen);

    // This test spawns no children: nothing to leak, no global state beyond
    // the INSTA_UPDATE default above. The completion marker proves the run
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
fn cli_projection(argv: &[&str], out: &tuisnap::command::ProcessOutput) -> String {
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
    freeze_insta_updates();
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
