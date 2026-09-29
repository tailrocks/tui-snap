//! Shared fixture-binary driver: argument parsing plus the interactive loop.
//!
//! The `*_fixture` binaries are thin shells over this driver: parse
//! [`CommonArgs`], build the view's deterministic model, then [`drive`] the
//! view's `render` + `step` pair. Pure-view tests call the same `render`
//! function headlessly, so both paths exercise identical view code.

use crate::views::Theme;
use anyhow::Context;
use crossterm::event::{self, Event};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::Frame as RFrame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::time::Duration;

/// Deterministic model scenario selected by `--scenario`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scenario {
    /// Populated deterministic model.
    #[default]
    Demo,
    /// Empty-state model.
    Empty,
    /// Error-state model (views without one fall back to demo).
    Error,
}

impl Scenario {
    /// Parse a `--scenario` argument. Unknown values are an explicit error.
    ///
    /// # Errors
    ///
    /// Returns an error naming the unknown value.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "demo" => Ok(Scenario::Demo),
            "empty" => Ok(Scenario::Empty),
            "error" => Ok(Scenario::Error),
            _ => Err(format!("unknown scenario {s:?} (want demo|empty|error)")),
        }
    }
}

/// Arguments shared by every `*_fixture` binary.
#[derive(Debug, Clone)]
pub struct CommonArgs {
    /// View theme.
    pub theme: Theme,
    /// Deterministic scenario.
    pub scenario: Scenario,
    /// Exit after this many drawn frames (deterministic runs).
    pub frames: Option<u64>,
    /// Print the deterministic text summary to stdout and exit (no TUI).
    pub print_only: bool,
}

/// Parse `argv` (without argv0). Unknown flags and missing values fail
/// explicitly; binaries exit 2 with the message on stderr.
///
/// # Errors
///
/// Returns an error for unknown flags, missing values, or invalid values.
pub fn parse_common(argv: &[String]) -> Result<CommonArgs, String> {
    let mut theme = Theme::Dark;
    let mut scenario = Scenario::Demo;
    let mut frames = None;
    let mut print_only = false;
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--theme" => {
                let v = value_of(argv, &mut i, "--theme")?;
                theme = Theme::parse(&v)?;
            }
            "--scenario" => {
                let v = value_of(argv, &mut i, "--scenario")?;
                scenario = Scenario::parse(&v)?;
            }
            "--frames" => {
                let v = value_of(argv, &mut i, "--frames")?;
                frames =
                    Some(v.parse::<u64>().map_err(|_| {
                        format!("--frames wants a non-negative integer, got {v:?}")
                    })?);
            }
            "--print" => print_only = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
        i += 1;
    }
    Ok(CommonArgs {
        theme,
        scenario,
        frames,
        print_only,
    })
}

/// Read the value following a flag at `argv[*i]`; advances `*i` past it.
fn value_of(argv: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    *i += 1;
    argv.get(*i)
        .cloned()
        .ok_or_else(|| format!("{flag} wants a value"))
}

/// Interactive-loop options.
#[derive(Debug, Clone)]
pub struct DriveOpts {
    /// Input poll quantum.
    pub tick: Duration,
    /// Exit after this many drawn frames (`None` runs until quit).
    pub frames: Option<u64>,
    /// Enable bracketed paste + focus tracking in the terminal.
    pub protocol_modes: bool,
}

/// Drive one fixture app: draw `render`, map input events with `map`, apply
/// them with `on_key` until it reports quit or the frame budget runs out.
/// The terminal (raw mode, alternate screen, protocol modes) is always
/// restored before returning.
///
/// # Errors
///
/// Returns an error when terminal setup, input polling, drawing, or terminal
/// restore fails.
pub fn drive<M, K>(
    mut model: M,
    opts: &DriveOpts,
    render: impl Fn(&mut RFrame<'_>, &M),
    on_key: impl Fn(&mut M, &K) -> bool,
    map: impl Fn(Event) -> Option<K>,
) -> anyhow::Result<()> {
    enable_raw_mode().context("enable raw mode")?;
    let result = drive_inner(&mut model, opts, &render, &on_key, &map);
    // Raw mode is always restored; the interactive-loop error wins when both
    // fail (matches `drive_inner`, which surfaces its own restore errors).
    let restore = disable_raw_mode().context("disable raw mode");
    match result {
        Ok(()) => restore,
        err => err,
    }
}

/// [`drive`] with the terminal already in raw mode.
fn drive_inner<M, K>(
    model: &mut M,
    opts: &DriveOpts,
    render: &impl Fn(&mut RFrame<'_>, &M),
    on_key: &impl Fn(&mut M, &K) -> bool,
    map: &impl Fn(Event) -> Option<K>,
) -> anyhow::Result<()> {
    use crossterm::event::{DisableBracketedPaste, DisableFocusChange};
    use crossterm::event::{EnableBracketedPaste, EnableFocusChange};
    use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    if opts.protocol_modes {
        crossterm::execute!(stdout, EnableBracketedPaste, EnableFocusChange)
            .context("enable protocol modes")?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend).context("open terminal")?;
    // Initial paint, then repaint only when an event may have changed state:
    // idle tick repaints would churn revisions and defeat stability waits.
    // Scripted `--frames` runs repaint on idle ticks instead, so the budget
    // advances without input.
    let mut drawn: u64 = 0;
    term.draw(|f| render(f, model)).context("draw frame")?;
    drawn += 1;
    loop {
        if opts.frames.is_some_and(|n| drawn >= n) {
            break;
        }
        if event::poll(opts.tick).context("poll input")? {
            let ev = event::read().context("read input")?;
            let mut quit = false;
            if let Some(key) = map(ev) {
                quit = on_key(model, &key);
            }
            term.draw(|f| render(f, model)).context("draw frame")?;
            drawn += 1;
            if quit {
                break;
            }
        } else if opts.frames.is_some() {
            term.draw(|f| render(f, model)).context("draw frame")?;
            drawn += 1;
        }
    }
    if opts.protocol_modes {
        crossterm::execute!(
            term.backend_mut(),
            DisableBracketedPaste,
            DisableFocusChange
        )
        .context("disable protocol modes")?;
    }
    crossterm::execute!(term.backend_mut(), LeaveAlternateScreen).context("leave screen")?;
    term.show_cursor().context("show cursor")?;
    Ok(())
}

/// Usage line shared by every `*_fixture --help` failure path.
#[must_use]
pub fn usage(name: &str) -> String {
    format!(
        "usage: {name} [--theme dark|light] [--scenario demo|empty|error] [--frames N] [--print]"
    )
}
