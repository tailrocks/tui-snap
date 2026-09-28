//! Shell sessions, terminal-state assertions, raw replay, and the scoped
//! guardian (backlog R09, R12, R13, R14).
//!
//! This module only *uses* [`Session`](crate::tui::Session); it never reaches
//! into its worker. Everything unobservable through
//! [`Observation`](crate::screen::Observation) is reported as
//! [`Maybe::Unknown`](crate::screen::Maybe)/`Unsupported` and fails closed.
//!
//! - **R12 [`Shell`]**: explicit `/bin/sh` sessions. Each [`Shell::run`]
//!   wraps the command in a shell integration that emits real OSC 133
//!   `C` (command start) / `D;code` (command end) boundaries plus an
//!   in-band textual attestation. Boundaries and exit codes come from the
//!   protocol, never from prompt-text guessing. [`Markers::Unavailable`]
//!   means the integration was never established: [`Shell::run`] then
//!   refuses with [`ShellError::NoIntegration`] instead of fabricating a
//!   span. A shell-command exit is unrelated to the direct-child exit.
//! - **R13 terminal state**: [`TermSnapshot`] + explicit `assert_*` fns over
//!   both live [`Observation`](crate::screen::Observation) (via
//!   [`TermSnapshot::from_observation`], partial: title/bells/modes/palette)
//!   and [`Replayed`] state (full: + defaults/clipboard/hyperlinks/
//!   scrollback). Clipboard capture is a [`SandboxClipboard`]: process
//!   memory only, the host clipboard is never touched.
//! - **R14 replay**: [`Recording`] tags every event as output or input at
//!   record time; [`replay_recording`] feeds *only* output bytes through a
//!   fresh emulator, so recorded input can never be mistaken for terminal
//!   output. Replay and recording are byte-capped ([`MAX_REPLAY_BYTES`]).
//! - **R09 [`Guardian`]**: owns a [`Session`](crate::tui::Session) and, on
//!   `finish`/`drop`, kills the child's whole process group with
//!   PID-reuse guards (session-id + start-time checks, per-pid reverify,
//!   never pid 0/1/self, never a foreign group). Descendants that called
//!   `setsid`/`setpgid` leave the group and are NOT contained; that escape
//!   boundary is documented on [`GuardianReport::escape_boundary_note`].
//! - Final state after exit is preserved (see
//!   [`Shell::wait_shell_exit`]); [`ShellResult::truncated`] flags spans
//!   whose start scrolled out of the viewport.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions as GridDims;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{ClipboardType, Config as TermConfig, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Color as VteColor, CursorShape, NamedColor, Processor, Rgb as VteRgb,
};

use crate::frame::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, UnderlineStyle};
use crate::screen::{Maybe, Observation, Screen};
use crate::tui::{CancelToken, ExitWait, Session, Tui, TuiError, WaitError};

// ---------------------------------------------------------------------------
// R13: terminal-state snapshot + explicit assertions
// ---------------------------------------------------------------------------

/// Which clipboard an OSC 52 store targeted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipboardTarget {
    Clipboard,
    Selection,
}

/// One captured OSC 52 store: decoded text plus its target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClipboardItem {
    pub target: ClipboardTarget,
    pub text: String,
}

/// Sandboxed clipboard capture: process memory only.
///
/// Bytes come from the replayed stream's OSC 52 sequences (base64-decoded,
/// UTF-8 only, like the backend). This type has no host-clipboard API to
/// call: capture can never read or modify the real clipboard.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct SandboxClipboard {
    items: Vec<ClipboardItem>,
}

impl SandboxClipboard {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn store(&mut self, item: ClipboardItem) {
        self.items.push(item);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn latest(&self) -> Option<&ClipboardItem> {
        self.items.last()
    }

    #[must_use]
    pub fn items(&self) -> &[ClipboardItem] {
        &self.items
    }
}

/// Default (non-indexed) foreground/background colors. `None` = terminal
/// default, i.e. no OSC 10/11 override observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DefaultColors {
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
}

/// One OSC 8 hyperlink URI observed on the grid (order of first appearance).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hyperlink {
    pub uri: String,
}

/// Full terminal state: everything assertions may target.
///
/// [`TermSnapshot::from_observation`] fills only what a live
/// [`Observation`] carries (title/bells/modes/palette); the rest is
/// [`Maybe::Unsupported`] because the live path drops it. Replay
/// ([`Replayed::state`]) fills everything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermSnapshot {
    pub title: Maybe<String>,
    pub bells: Maybe<u64>,
    pub modes: Maybe<Vec<u16>>,
    pub palette: Maybe<Vec<(u8, Rgb)>>,
    pub defaults: Maybe<DefaultColors>,
    pub clipboard: Maybe<SandboxClipboard>,
    pub hyperlinks: Maybe<Vec<Hyperlink>>,
    /// Scrollback lines, oldest first, viewport excluded.
    pub scrollback: Maybe<Vec<String>>,
}

impl TermSnapshot {
    /// Project a live observation onto the assertion surface. Fields the
    /// live path cannot provide are `Unsupported`, never fabricated.
    #[must_use]
    pub fn from_observation(obs: &Observation) -> Self {
        Self {
            title: obs.state.title.clone(),
            bells: obs.state.bells.clone(),
            modes: obs.state.modes.clone(),
            palette: obs.state.palette.clone(),
            defaults: Maybe::Unsupported,
            clipboard: Maybe::Unsupported,
            hyperlinks: Maybe::Unsupported,
            scrollback: Maybe::Unsupported,
        }
    }
}

/// Terminal-state assertion failure (mismatch, unknown, or unsupported).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateError(pub String);

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "terminal-state assertion failed: {}", self.0)
    }
}

impl std::error::Error for StateError {}

/// Assert the window/icon title equals `expected`.
pub fn assert_title_eq(state: &TermSnapshot, expected: &str) -> Result<(), StateError> {
    match &state.title {
        Maybe::Known(t) if t == expected => Ok(()),
        Maybe::Known(t) => Err(StateError(format!(
            "title mismatch: expected {expected:?}, observed {t:?}"
        ))),
        Maybe::Unknown => Err(StateError("title is unknown (never set)".to_string())),
        Maybe::Unsupported => Err(StateError(
            "title is unsupported by this capture path".to_string(),
        )),
    }
}

/// Assert the bell count since session start equals `expected`.
pub fn assert_bells_eq(state: &TermSnapshot, expected: u64) -> Result<(), StateError> {
    match &state.bells {
        Maybe::Known(n) if *n == expected => Ok(()),
        Maybe::Known(n) => Err(StateError(format!(
            "bell count mismatch: expected {expected}, observed {n}"
        ))),
        Maybe::Unknown => Err(StateError("bell count is unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "bell count is unsupported by this capture path".to_string(),
        )),
    }
}

/// Assert DEC/private mode `mode` is currently set.
pub fn assert_mode_set(state: &TermSnapshot, mode: u16) -> Result<(), StateError> {
    match &state.modes {
        Maybe::Known(m) if m.contains(&mode) => Ok(()),
        Maybe::Known(m) => Err(StateError(format!(
            "mode {mode} is not set (observed: {m:?})"
        ))),
        Maybe::Unknown => Err(StateError("modes are unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "modes are unsupported by this capture path".to_string(),
        )),
    }
}

/// Assert DEC/private mode `mode` is currently unset.
pub fn assert_mode_unset(state: &TermSnapshot, mode: u16) -> Result<(), StateError> {
    match &state.modes {
        Maybe::Known(m) if !m.contains(&mode) => Ok(()),
        Maybe::Known(_) => Err(StateError(format!("mode {mode} is set"))),
        Maybe::Unknown => Err(StateError("modes are unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "modes are unsupported by this capture path".to_string(),
        )),
    }
}

/// Assert palette entry `index` resolves to `expected`: a live OSC 4
/// override when present, else the documented nominal xterm default.
pub fn assert_palette_entry(
    state: &TermSnapshot,
    index: u8,
    expected: Rgb,
) -> Result<(), StateError> {
    match &state.palette {
        Maybe::Known(list) => {
            let observed = list
                .iter()
                .find(|(i, _)| *i == index)
                .map(|(_, c)| *c)
                .unwrap_or_else(|| Rgb::from_indexed(index));
            if observed == expected {
                Ok(())
            } else {
                Err(StateError(format!(
                    "palette {index} mismatch: expected {expected:?}, observed {observed:?}"
                )))
            }
        }
        Maybe::Unknown => Err(StateError("palette is unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "palette is unsupported by this capture path".to_string(),
        )),
    }
}

/// Assert the default fg/bg (OSC 10/11 overrides; `None` = terminal default).
pub fn assert_default_colors(
    state: &TermSnapshot,
    fg: Option<Rgb>,
    bg: Option<Rgb>,
) -> Result<(), StateError> {
    match &state.defaults {
        Maybe::Known(d) if d.fg == fg && d.bg == bg => Ok(()),
        Maybe::Known(d) => Err(StateError(format!(
            "default colors mismatch: expected fg={fg:?} bg={bg:?}, observed fg={:?} bg={:?}",
            d.fg, d.bg
        ))),
        Maybe::Unknown => Err(StateError("default colors are unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "default colors are unsupported by this capture path (live sessions drop OSC 10/11)"
                .to_string(),
        )),
    }
}

/// Assert the latest sandboxed clipboard store equals `expected`.
pub fn assert_clipboard_latest_eq(state: &TermSnapshot, expected: &str) -> Result<(), StateError> {
    match &state.clipboard {
        Maybe::Known(sb) => match sb.latest() {
            Some(item) if item.text == expected => Ok(()),
            Some(item) => Err(StateError(format!(
                "clipboard mismatch: expected {expected:?}, observed {:?}",
                item.text
            ))),
            None => Err(StateError("no clipboard stores captured".to_string())),
        },
        Maybe::Unknown => Err(StateError("clipboard is unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "clipboard is unsupported by this capture path (live sessions drop OSC 52)".to_string(),
        )),
    }
}

/// Assert no clipboard stores were captured.
pub fn assert_clipboard_empty(state: &TermSnapshot) -> Result<(), StateError> {
    match &state.clipboard {
        Maybe::Known(sb) if sb.is_empty() => Ok(()),
        Maybe::Known(sb) => Err(StateError(format!(
            "expected no clipboard stores, observed {}",
            sb.len()
        ))),
        Maybe::Unknown => Err(StateError("clipboard is unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "clipboard is unsupported by this capture path (live sessions drop OSC 52)".to_string(),
        )),
    }
}

/// Assert a hyperlink with exactly `uri` is present on the grid.
pub fn assert_hyperlink_present(state: &TermSnapshot, uri: &str) -> Result<(), StateError> {
    match &state.hyperlinks {
        Maybe::Known(links) if links.iter().any(|l| l.uri == uri) => Ok(()),
        Maybe::Known(links) => Err(StateError(format!(
            "hyperlink {uri:?} not present ({} links observed)",
            links.len()
        ))),
        Maybe::Unknown => Err(StateError("hyperlinks are unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "hyperlinks are unsupported by this capture path".to_string(),
        )),
    }
}

/// Assert scrollback (oldest-first, viewport excluded) contains `needle`.
pub fn assert_scrollback_contains(state: &TermSnapshot, needle: &str) -> Result<(), StateError> {
    match &state.scrollback {
        Maybe::Known(lines) if lines.iter().any(|l| l.contains(needle)) => Ok(()),
        Maybe::Known(lines) => Err(StateError(format!(
            "scrollback ({} lines) does not contain {needle:?}",
            lines.len()
        ))),
        Maybe::Unknown => Err(StateError("scrollback is unknown".to_string())),
        Maybe::Unsupported => Err(StateError(
            "scrollback is unsupported by this capture path (live sessions capture the viewport only)"
                .to_string(),
        )),
    }
}

// ---------------------------------------------------------------------------
// R14: bounded raw replay (direction-tagged recordings, fresh emulator)
// ---------------------------------------------------------------------------

/// Byte cap for one [`Recording`] and for one replay call.
pub const MAX_REPLAY_BYTES: usize = 1 << 20;
/// Scrollback lines retained by the replay emulator.
const REPLAY_HISTORY: usize = 1000;
/// Cap on hyperlinks collected from one replay.
const MAX_REPLAY_LINKS: usize = 1024;

/// One recorded event. The direction tag is structural: [`replay_recording`]
/// only feeds [`RecEvent::Output`], so recorded input can never be mistaken
/// for terminal output, by construction rather than by caller discipline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecEvent {
    /// PTY output bytes (terminal input): fed to the emulator on replay.
    Output(Vec<u8>),
    /// Bytes the test sent to the PTY: never fed on replay.
    Input(Vec<u8>),
}

/// A bounded, direction-tagged recording of a PTY conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    cols: u16,
    rows: u16,
    events: Vec<RecEvent>,
    bytes: usize,
}

/// Replay failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayError {
    TooLarge { bytes: usize, max: usize },
    InvalidSize(String),
    InvalidChunks(String),
    ScreenBuild(String),
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { bytes, max } => {
                write!(f, "recording too large: {bytes} bytes exceed cap {max}")
            }
            Self::InvalidSize(m) => write!(f, "invalid replay size: {m}"),
            Self::InvalidChunks(m) => write!(f, "invalid chunking: {m}"),
            Self::ScreenBuild(m) => write!(f, "replay screen build failed: {m}"),
        }
    }
}

impl std::error::Error for ReplayError {}

impl Recording {
    #[must_use]
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols,
            rows,
            events: Vec::new(),
            bytes: 0,
        }
    }

    /// Record PTY output bytes. Refused past [`MAX_REPLAY_BYTES`].
    pub fn push_output(&mut self, bytes: &[u8]) -> Result<(), ReplayError> {
        self.push(RecEvent::Output(bytes.to_vec()))
    }

    /// Record input bytes (never replayed as output). Counts toward the cap.
    pub fn push_input(&mut self, bytes: &[u8]) -> Result<(), ReplayError> {
        self.push(RecEvent::Input(bytes.to_vec()))
    }

    fn push(&mut self, event: RecEvent) -> Result<(), ReplayError> {
        let n = match &event {
            RecEvent::Output(b) | RecEvent::Input(b) => b.len(),
        };
        if self.bytes + n > MAX_REPLAY_BYTES {
            return Err(ReplayError::TooLarge {
                bytes: self.bytes + n,
                max: MAX_REPLAY_BYTES,
            });
        }
        self.bytes += n;
        self.events.push(event);
        Ok(())
    }

    #[must_use]
    pub fn cols(&self) -> u16 {
        self.cols
    }

    #[must_use]
    pub fn rows(&self) -> u16 {
        self.rows
    }

    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.bytes
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Concatenated output bytes in record order (input excluded).
    #[must_use]
    pub fn output_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for e in &self.events {
            if let RecEvent::Output(b) = e {
                out.extend_from_slice(b);
            }
        }
        out
    }
}

/// The result of one replay: final screen + full terminal state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replayed {
    pub screen: Screen,
    pub state: TermSnapshot,
    pub bytes_fed: usize,
    pub chunks: usize,
}

/// Replay raw output bytes through a fresh emulator, fed as one chunk.
pub fn replay_bytes(output: &[u8], cols: u16, rows: u16) -> Result<Replayed, ReplayError> {
    replay_chunks([output], cols, rows)
}

/// Replay raw output bytes through a fresh emulator, fed in the given
/// chunks. Splits may fall anywhere, including mid-UTF-8 and mid-escape:
/// the streaming parser makes chunking unobservable in the result.
pub fn replay_chunks<'a>(
    chunks: impl IntoIterator<Item = &'a [u8]>,
    cols: u16,
    rows: u16,
) -> Result<Replayed, ReplayError> {
    if !(1..=1000).contains(&cols) || !(1..=1000).contains(&rows) {
        return Err(ReplayError::InvalidSize(format!(
            "replay size {cols}x{rows} outside 1..=1000"
        )));
    }
    let chunks: Vec<&[u8]> = chunks.into_iter().collect();
    let total: usize = chunks.iter().map(|c| c.len()).sum();
    if total > MAX_REPLAY_BYTES {
        return Err(ReplayError::TooLarge {
            bytes: total,
            max: MAX_REPLAY_BYTES,
        });
    }

    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let dims = ReplayDims {
        cols: cols as usize,
        rows: rows as usize,
    };
    let config = TermConfig {
        scrolling_history: REPLAY_HISTORY,
        kitty_keyboard: true,
        ..TermConfig::default()
    };
    let mut term = Term::new(config, &dims, ReplayListener { tx: event_tx });
    let mut processor: Processor = Processor::new();
    for chunk in &chunks {
        processor.advance(&mut term, chunk);
    }

    let mut replayed = ReplayEvents::default();
    drain_replay_events(&mut term, &event_rx, &mut replayed);
    let screen = build_replay_screen(&term, cols, rows)?;
    let state = build_replay_state(&term, &replayed, cols);
    Ok(Replayed {
        screen,
        state,
        bytes_fed: total,
        chunks: chunks.len(),
    })
}

/// Replay a recording through a fresh emulator: only [`RecEvent::Output`]
/// bytes are fed, in record order. `chunk_len` re-splits the output stream
/// (`None` = one chunk); recorded input is always skipped.
pub fn replay_recording(
    recording: &Recording,
    chunk_len: Option<usize>,
) -> Result<Replayed, ReplayError> {
    let output = recording.output_bytes();
    match chunk_len {
        None => replay_chunks([output.as_slice()], recording.cols, recording.rows),
        Some(0) => Err(ReplayError::InvalidChunks("chunk length 0".to_string())),
        Some(n) => {
            let chunks: Vec<&[u8]> = output.chunks(n).collect();
            replay_chunks(chunks, recording.cols, recording.rows)
        }
    }
}

struct ReplayDims {
    cols: usize,
    rows: usize,
}

impl GridDims for ReplayDims {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

#[derive(Clone)]
struct ReplayListener {
    tx: mpsc::Sender<Event>,
}

impl EventListener for ReplayListener {
    fn send_event(&self, event: Event) {
        let _ = self.tx.send(event);
    }
}

#[derive(Default)]
struct ReplayEvents {
    title: Option<String>,
    bells: u64,
    clipboard: SandboxClipboard,
}

fn drain_replay_events<T: EventListener>(
    term: &mut Term<T>,
    event_rx: &mpsc::Receiver<Event>,
    events: &mut ReplayEvents,
) {
    let _ = term;
    while let Ok(event) = event_rx.try_recv() {
        match event {
            Event::Title(t) => events.title = Some(t),
            Event::ResetTitle => events.title = None,
            Event::Bell => events.bells += 1,
            Event::ClipboardStore(ty, text) => {
                let target = match ty {
                    ClipboardType::Clipboard => ClipboardTarget::Clipboard,
                    ClipboardType::Selection => ClipboardTarget::Selection,
                };
                events.clipboard.store(ClipboardItem { target, text });
            }
            // No PTY to answer: query replies are dropped, like the live
            // worker drops them once the writer is gone.
            _ => {}
        }
    }
}

/// Viewport grid + cursor, mirroring the live observation builder's mapping
/// rules (wide-char pairing, orphan spacers, color/cursor mapping).
fn build_replay_screen<T: EventListener>(
    term: &Term<T>,
    cols: u16,
    rows: u16,
) -> Result<Screen, ReplayError> {
    let grid = term.grid();
    let mut cells = Vec::with_capacity(cols as usize * rows as usize);
    for y in 0..rows {
        let line = Line(y as i32);
        let mut x: u16 = 0;
        while x < cols {
            let cell = &grid[line][Column(x as usize)];
            let flags = cell.flags;
            if flags.contains(CellFlags::WIDE_CHAR)
                && x + 1 < cols
                && grid[line][Column(x as usize + 1)]
                    .flags
                    .contains(CellFlags::WIDE_CHAR_SPACER)
            {
                cells.push(replay_cell(x, y, cell, 2, false));
                cells.push(Cell {
                    x: x + 1,
                    y,
                    symbol: String::new(),
                    width: 0,
                    continuation: true,
                    fg: Color::Default,
                    bg: Color::Default,
                    mods: Mods::default(),
                    underline_color: Color::Default,
                });
                x += 2;
            } else if flags
                .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER)
            {
                cells.push(Cell::blank(x, y));
                x += 1;
            } else {
                cells.push(replay_cell(x, y, cell, 1, false));
                x += 1;
            }
        }
    }
    let point = grid.cursor.point;
    let cursor = replay_cursor(term, point.line.0, point.column.0, cols, rows);
    Screen::validate(cols, rows, 0, 0, cells, cursor)
        .map_err(|e| ReplayError::ScreenBuild(e.to_string()))
}

fn replay_cell(
    x: u16,
    y: u16,
    cell: &alacritty_terminal::term::cell::Cell,
    width: u8,
    cont: bool,
) -> Cell {
    let mut symbol = String::new();
    symbol.push(cell.c);
    if let Some(extra) = cell.zerowidth() {
        symbol.extend(extra.iter());
    }
    let flags = cell.flags;
    Cell {
        x,
        y,
        symbol,
        width,
        continuation: cont,
        fg: replay_color(cell.fg),
        bg: replay_color(cell.bg),
        mods: Mods {
            hidden: flags.contains(CellFlags::HIDDEN),
            blink: false,
            bold: flags.contains(CellFlags::BOLD),
            dim: flags.contains(CellFlags::DIM),
            italic: flags.contains(CellFlags::ITALIC),
            underline: flags.intersects(CellFlags::ALL_UNDERLINES),
            underline_style: replay_underline_style(flags),
            strikethrough: flags.contains(CellFlags::STRIKEOUT),
            reverse: flags.contains(CellFlags::INVERSE),
        },
        underline_color: cell
            .underline_color()
            .map(replay_color)
            .unwrap_or(Color::Default),
    }
}

/// Map alacritty underline flags (set from SGR 4 / 4:0..4:5 / 24) to the
/// canonical style. The emulator holds at most one underline flag per cell
/// (each SGR 4:x clears the rest); the order below is defensive only.
fn replay_underline_style(flags: CellFlags) -> UnderlineStyle {
    if flags.contains(CellFlags::DOUBLE_UNDERLINE) {
        UnderlineStyle::Double
    } else if flags.contains(CellFlags::UNDERCURL) {
        UnderlineStyle::Curly
    } else if flags.contains(CellFlags::DOTTED_UNDERLINE) {
        UnderlineStyle::Dotted
    } else if flags.contains(CellFlags::DASHED_UNDERLINE) {
        UnderlineStyle::Dashed
    } else if flags.contains(CellFlags::UNDERLINE) {
        UnderlineStyle::Single
    } else {
        UnderlineStyle::None
    }
}

fn replay_color(c: VteColor) -> Color {
    match c {
        VteColor::Named(n) => match n {
            NamedColor::Black => Color::Indexed(0),
            NamedColor::Red => Color::Indexed(1),
            NamedColor::Green => Color::Indexed(2),
            NamedColor::Yellow => Color::Indexed(3),
            NamedColor::Blue => Color::Indexed(4),
            NamedColor::Magenta => Color::Indexed(5),
            NamedColor::Cyan => Color::Indexed(6),
            NamedColor::White => Color::Indexed(7),
            NamedColor::BrightBlack => Color::Indexed(8),
            NamedColor::BrightRed => Color::Indexed(9),
            NamedColor::BrightGreen => Color::Indexed(10),
            NamedColor::BrightYellow => Color::Indexed(11),
            NamedColor::BrightBlue => Color::Indexed(12),
            NamedColor::BrightMagenta => Color::Indexed(13),
            NamedColor::BrightCyan => Color::Indexed(14),
            NamedColor::BrightWhite => Color::Indexed(15),
            NamedColor::DimBlack => Color::Indexed(0),
            NamedColor::DimRed => Color::Indexed(1),
            NamedColor::DimGreen => Color::Indexed(2),
            NamedColor::DimYellow => Color::Indexed(3),
            NamedColor::DimBlue => Color::Indexed(4),
            NamedColor::DimMagenta => Color::Indexed(5),
            NamedColor::DimCyan => Color::Indexed(6),
            NamedColor::DimWhite => Color::Indexed(7),
            NamedColor::Foreground
            | NamedColor::Background
            | NamedColor::Cursor
            | NamedColor::BrightForeground
            | NamedColor::DimForeground => Color::Default,
        },
        VteColor::Spec(rgb) => Color::Rgb(Rgb {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        }),
        VteColor::Indexed(i) => Color::Indexed(i),
    }
}

fn replay_cursor<T: EventListener>(
    term: &Term<T>,
    line: i32,
    column: usize,
    cols: u16,
    rows: u16,
) -> Cursor {
    let style = term.cursor_style();
    let shape = match style.shape {
        CursorShape::Block | CursorShape::HollowBlock | CursorShape::Hidden => CursorStyle::Block,
        CursorShape::Underline => CursorStyle::Underline,
        CursorShape::Beam => CursorStyle::Bar,
    };
    let visible = term.mode().contains(TermMode::SHOW_CURSOR)
        && !matches!(style.shape, CursorShape::Hidden)
        && line >= 0
        && (line as u32) < rows as u32
        && column < cols as usize;
    if !visible {
        return Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: shape,
            blinking: style.blinking,
        };
    }
    Cursor {
        x: column as u16,
        y: line as u16,
        visible: true,
        style: shape,
        blinking: style.blinking,
    }
}

fn replay_modes(mode: &TermMode) -> Vec<u16> {
    let mut out = Vec::new();
    let mut push = |flag: TermMode, n: u16| {
        if mode.contains(flag) {
            out.push(n);
        }
    };
    push(TermMode::APP_CURSOR, 1);
    push(TermMode::INSERT, 4);
    push(TermMode::ORIGIN, 6);
    push(TermMode::LINE_WRAP, 7);
    push(TermMode::LINE_FEED_NEW_LINE, 20);
    push(TermMode::APP_KEYPAD, 66);
    push(TermMode::MOUSE_REPORT_CLICK, 1000);
    push(TermMode::MOUSE_DRAG, 1002);
    push(TermMode::MOUSE_MOTION, 1003);
    push(TermMode::FOCUS_IN_OUT, 1004);
    push(TermMode::UTF8_MOUSE, 1005);
    push(TermMode::SGR_MOUSE, 1006);
    push(TermMode::ALT_SCREEN, 1049);
    push(TermMode::BRACKETED_PASTE, 2004);
    if mode.intersects(TermMode::KITTY_KEYBOARD_PROTOCOL) {
        out.push(57399);
    }
    out
}

fn vte_to_rgb(c: VteRgb) -> Rgb {
    Rgb {
        r: c.r,
        g: c.g,
        b: c.b,
    }
}

/// Full terminal state from a replayed emulator: everything is `Known`.
fn build_replay_state<T: EventListener>(
    term: &Term<T>,
    events: &ReplayEvents,
    cols: u16,
) -> TermSnapshot {
    let palette: Vec<(u8, Rgb)> = (0..256u16)
        .filter_map(|i| term.colors()[i as usize].map(|c| (i as u8, vte_to_rgb(c))))
        .collect();
    let defaults = DefaultColors {
        fg: term.colors()[NamedColor::Foreground].map(vte_to_rgb),
        bg: term.colors()[NamedColor::Background].map(vte_to_rgb),
    };
    let history = term
        .total_lines()
        .saturating_sub(term.screen_lines())
        .min(REPLAY_HISTORY);
    let grid = term.grid();
    let mut scrollback = Vec::with_capacity(history);
    for h in (0..history).rev() {
        scrollback.push(replay_line_text(grid, Line(-(h as i32) - 1), cols));
    }
    let mut seen = HashSet::new();
    let mut hyperlinks = Vec::new();
    for h in (0..history).rev() {
        collect_links(
            grid,
            Line(-(h as i32) - 1),
            cols,
            &mut seen,
            &mut hyperlinks,
        );
    }
    for y in 0..term.screen_lines() {
        collect_links(grid, Line(y as i32), cols, &mut seen, &mut hyperlinks);
    }
    TermSnapshot {
        title: match &events.title {
            Some(t) => Maybe::Known(t.clone()),
            None => Maybe::Unknown,
        },
        bells: Maybe::Known(events.bells),
        modes: Maybe::Known(replay_modes(term.mode())),
        palette: Maybe::Known(palette),
        defaults: Maybe::Known(defaults),
        clipboard: Maybe::Known(events.clipboard.clone()),
        hyperlinks: Maybe::Known(hyperlinks),
        scrollback: Maybe::Known(scrollback),
    }
}

fn replay_line_text(
    grid: &alacritty_terminal::grid::Grid<alacritty_terminal::term::cell::Cell>,
    line: Line,
    cols: u16,
) -> String {
    let mut s = String::new();
    for x in 0..cols {
        let cell = &grid[line][Column(x as usize)];
        if cell
            .flags
            .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        s.push(cell.c);
        if let Some(extra) = cell.zerowidth() {
            s.extend(extra.iter());
        }
    }
    s.trim_end().to_string()
}

fn collect_links(
    grid: &alacritty_terminal::grid::Grid<alacritty_terminal::term::cell::Cell>,
    line: Line,
    cols: u16,
    seen: &mut HashSet<String>,
    out: &mut Vec<Hyperlink>,
) {
    if out.len() >= MAX_REPLAY_LINKS {
        return;
    }
    for x in 0..cols {
        if out.len() >= MAX_REPLAY_LINKS {
            return;
        }
        let cell = &grid[line][Column(x as usize)];
        if let Some(link) = cell.hyperlink() {
            let uri = link.uri().to_string();
            if seen.insert(uri.clone()) {
                out.push(Hyperlink { uri });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// R12: explicit shell sessions with command-boundary integration
// ---------------------------------------------------------------------------

/// Shell integration setup handshake timeout for [`Shell::sh`].
const SHELL_SETUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Whether the shell speaks the command-boundary protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Markers {
    /// Integration handshake verified: every [`Shell::run`] is delimited by
    /// OSC 133 `C`/`D;code` boundaries plus an in-band attestation, and the
    /// result comes from that protocol.
    Available,
    /// No integration: [`Shell::run`] refuses rather than guessing spans
    /// from prompt text.
    Unavailable,
}

/// One shell-command result. `exit_code` is the *shell command's* exit, not
/// the direct child's; the shell usually keeps running afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellResult {
    pub exit_code: i32,
    /// Command output lines between the start/end attestations (exclusive).
    pub output_span: Vec<String>,
    pub markers: Markers,
    /// True when the start attestation scrolled out of the viewport: the
    /// span is a truncated tail, not the full output.
    pub truncated: bool,
}

/// Shell failure.
#[derive(Debug)]
pub enum ShellError {
    Tui(TuiError),
    Wait(WaitError),
    NoIntegration(&'static str),
    BadCommand(String),
    Protocol(String),
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tui(e) => write!(f, "shell session: {e}"),
            Self::Wait(e) => write!(f, "shell wait: {e}"),
            Self::NoIntegration(m) => write!(f, "no shell integration: {m}"),
            Self::BadCommand(m) => write!(f, "bad shell command: {m}"),
            Self::Protocol(m) => write!(f, "shell protocol: {m}"),
        }
    }
}

impl std::error::Error for ShellError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Tui(e) => Some(e),
            Self::Wait(e) => Some(e),
            _ => None,
        }
    }
}

impl From<TuiError> for ShellError {
    fn from(e: TuiError) -> Self {
        Self::Tui(e)
    }
}

impl From<WaitError> for ShellError {
    fn from(e: WaitError) -> Self {
        Self::Wait(e)
    }
}

/// An explicit `/bin/sh` session with command-boundary integration.
///
/// Each [`Shell::run`] sends one wrapped line; the wrapper emits OSC 133
/// `C` at command start and `D;code` at command end, plus textual
/// attestations the harness waits on. Exit codes and spans come from the
/// protocol only.
pub struct Shell {
    session: Session,
    markers: Markers,
    runs: AtomicU64,
    tag: String,
}

impl std::fmt::Debug for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shell")
            .field("pid", &self.session.pid())
            .field("markers", &self.markers)
            .field("runs", &self.runs.load(Ordering::SeqCst))
            .finish()
    }
}

impl Shell {
    /// Spawn `/bin/sh` (80x24) and establish the integration handshake.
    pub fn sh() -> Result<Self, ShellError> {
        Self::sh_sized(80, 24)
    }

    /// Spawn `/bin/sh` at `cols`x`rows` and establish the handshake.
    pub fn sh_sized(cols: u16, rows: u16) -> Result<Self, ShellError> {
        let session = Tui::new(["/bin/sh"])
            .size(cols, rows)
            .env("ENV", "/dev/null")
            .spawn()?;
        let mut shell = Self::wrap(session);
        shell.setup(Instant::now() + SHELL_SETUP_TIMEOUT)?;
        Ok(shell)
    }

    /// Adopt an existing session without assuming any integration.
    /// Markers start [`Markers::Unavailable`]; call [`Shell::setup`] to run
    /// the handshake (succeeds only on a POSIX shell prompt).
    #[must_use]
    pub fn wrap(session: Session) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self {
            session,
            markers: Markers::Unavailable,
            runs: AtomicU64::new(0),
            tag: format!("TS{}-{nanos}", std::process::id()),
        }
    }

    #[must_use]
    pub fn markers(&self) -> Markers {
        self.markers
    }

    #[must_use]
    pub fn session(&self) -> &Session {
        &self.session
    }

    #[must_use]
    pub fn into_session(self) -> Session {
        self.session
    }

    /// Run the integration handshake: install the wrapper functions and wait
    /// for their confirmation. On success markers become available.
    pub fn setup(&mut self, deadline: Instant) -> Result<(), ShellError> {
        const SETUP: &str = "__tuisnap_c(){ printf '__TUISNAP_C__ %s\\n' \"$1\"; printf '\\033]133;C\\a'; }; __tuisnap_d(){ printf '\\033]133;D;%s\\a' \"$2\"; printf '__TUISNAP_D__ %s %s\\n' \"$1\" \"$2\"; }; echo __TUISNAP_SETUP_OK__";
        self.session.send_text(&format!("{SETUP}\n"))?;
        let cancel = CancelToken::new();
        self.session.wait_predicate(
            |o| {
                screen_rows(&o.screen)
                    .iter()
                    .any(|r| r == "__TUISNAP_SETUP_OK__")
            },
            deadline,
            &cancel,
        )?;
        self.markers = Markers::Available;
        Ok(())
    }

    /// Run one single-line shell command, delimited by the protocol.
    /// Fails without integration; never infers spans from prompt text.
    pub fn run(&self, cmd: &str, deadline: Instant) -> Result<ShellResult, ShellError> {
        if self.markers != Markers::Available {
            return Err(ShellError::NoIntegration(
                "shell integration unavailable; refusing to guess command boundaries",
            ));
        }
        if cmd.is_empty() || cmd.trim().is_empty() {
            return Err(ShellError::BadCommand("command is empty".to_string()));
        }
        if cmd.contains('\n') || cmd.contains('\r') {
            return Err(ShellError::BadCommand(
                "command must be a single line".to_string(),
            ));
        }
        let n = self.runs.fetch_add(1, Ordering::SeqCst);
        let token = format!("{}-{n}", self.tag);
        let line = format!(
            "__tuisnap_c {token}; {cmd}; __tuisnap_code=$?; __tuisnap_d {token} \"$__tuisnap_code\"\n"
        );
        self.session.send_text(&line)?;
        let start_marker = format!("__TUISNAP_C__ {token}");
        let end_prefix = format!("__TUISNAP_D__ {token} ");
        let cancel = CancelToken::new();
        let obs = self.session.wait_predicate(
            |o| {
                screen_rows(&o.screen)
                    .iter()
                    .any(|r| r.starts_with(&end_prefix))
            },
            deadline,
            &cancel,
        )?;
        let rows = screen_rows(&obs.screen);
        let end_idx = rows
            .iter()
            .position(|r| r.starts_with(&end_prefix))
            .ok_or_else(|| {
                ShellError::Protocol("end attestation vanished after wait".to_string())
            })?;
        let exit_code: i32 = rows[end_idx][end_prefix.len()..]
            .trim()
            .parse()
            .map_err(|_| {
                ShellError::Protocol(format!("unparseable exit code in {:?}", rows[end_idx]))
            })?;
        let (output_span, truncated) =
            match rows[..end_idx].iter().rposition(|r| *r == start_marker) {
                Some(start_idx) => (rows[start_idx + 1..end_idx].to_vec(), false),
                None => (rows[..end_idx].to_vec(), true),
            };
        Ok(ShellResult {
            exit_code,
            output_span,
            markers: Markers::Available,
            truncated,
        })
    }

    /// Wait for the shell itself to exit, preserving the final observation
    /// (final grid + terminal state survive the child).
    pub fn wait_shell_exit(
        &self,
        deadline: Instant,
        cancel: &CancelToken,
    ) -> Result<ExitWait, WaitError> {
        self.session.wait_exit(deadline, cancel)
    }
}

/// Plain-text viewport rows (trailing blanks trimmed per row).
fn screen_rows(screen: &Screen) -> Vec<String> {
    let mut out = Vec::with_capacity(screen.rows() as usize);
    for y in 0..screen.rows() {
        let mut s = String::new();
        for x in 0..screen.cols() {
            if let Some(c) = screen.get(x, y) {
                if !c.continuation {
                    s.push_str(&c.symbol);
                }
            }
        }
        out.push(s.trim_end().to_string());
    }
    out
}

// ---------------------------------------------------------------------------
// R09: scoped guardian (process-group containment with PID-reuse guards)
// ---------------------------------------------------------------------------

/// Cap on pids signalled during one sweep (bounded containment).
const MAX_SWEEP_TARGETS: usize = 256;
/// Cap on survivors listed in a report.
const MAX_SURVIVORS: usize = 64;
/// Cap on `ps` snapshot lines parsed.
const MAX_PS_LINES: usize = 131_072;
/// Post-kill settle polling budget per sweep.
const SWEEP_SETTLE: std::time::Duration = std::time::Duration::from_millis(500);

/// How completely the guardian contained the child's process group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Containment {
    /// Group verified empty after the sweep (or was already empty).
    Full,
    /// Some group members survived (listed in
    /// [`GuardianReport::survivors`]).
    Partial,
    /// The sweep refused to signal: killing would have risked unrelated
    /// pids (foreign/reused group id, unresolvable identity, ...).
    Refused { reason: String },
    /// Identity or enumeration failed; nothing was signalled.
    Unknown { reason: String },
    /// Platform cannot enumerate process groups.
    Unsupported,
}

/// What a guardian teardown did, pid by pid. Escaping descendants (setsid/
/// setpgid) are invisible to the group scan by nature; see
/// [`GuardianReport::escape_boundary_note`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardianReport {
    pub child_pid: Option<u32>,
    pub pgid: Option<i32>,
    /// Every signalled pid was re-verified in the expected session.
    pub sid_verified: bool,
    /// The direct child's start time still matched at sweep time
    /// (`None` = start time unavailable on this platform/run).
    pub start_verified: Option<bool>,
    /// Pids sent SIGKILL (bounded to [`MAX_SWEEP_TARGETS`]).
    pub signalled: Vec<u32>,
    /// Group members still alive after the sweep (bounded).
    pub survivors: Vec<u32>,
    pub containment: Containment,
    pub teardown_error: Option<String>,
}

impl GuardianReport {
    /// The documented escape boundary: descendants that called
    /// `setsid(2)`/`setpgid(2)` leave the child's process group (and
    /// possibly its session), so the group sweep cannot see or contain
    /// them. Containment is process-group-scoped, not a sandbox.
    #[must_use]
    pub fn escape_boundary_note() -> &'static str {
        "escape boundary: descendants that called setsid(2)/setpgid(2) leave the \
         child's process group and are NOT contained by the group sweep; use an \
         OS sandbox for untrusted code"
    }
}

/// Owns a [`Session`] and contains its whole process group on teardown.
///
/// `finish`/`drop` close the session (reaping the direct child) and then
/// sweep the child's process group with PID-reuse guards. Nothing is ever
/// signalled blindly: see the guards in the sweep implementation.
pub struct Guardian {
    session: Option<Session>,
    child: Option<ChildIds>,
    swept: bool,
}

#[derive(Debug, Clone)]
struct ChildIds {
    pid: u32,
    pgid: i32,
    sid: i32,
    start: Option<String>,
}

impl std::fmt::Debug for Guardian {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Guardian")
            .field("child", &self.child)
            .field("swept", &self.swept)
            .finish()
    }
}

impl Guardian {
    /// Adopt a session, recording the child's pid/group/session/start-time
    /// for the teardown guards. Never fails; unresolvable identity degrades
    /// to a sweep that refuses to signal (reported, never blind).
    #[must_use]
    pub fn wrap(session: Session) -> Self {
        let child = session.pid().and_then(ChildIds::capture);
        Self {
            session: Some(session),
            child,
            swept: false,
        }
    }

    #[must_use]
    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// Close the session and sweep the child's process group (bounded by
    /// `deadline`), returning the per-pid report.
    pub fn finish(mut self, deadline: Instant) -> Result<GuardianReport, TuiError> {
        let mut teardown_error = None;
        if let Some(mut s) = self.session.take() {
            if let Err(e) = s.close() {
                teardown_error = Some(e.to_string());
            }
        }
        self.swept = true;
        Ok(sweep_group(&self.child, Some(deadline), teardown_error))
    }
}

impl Drop for Guardian {
    fn drop(&mut self) {
        if let Some(mut s) = self.session.take() {
            let _ = s.close();
        }
        if !self.swept {
            self.swept = true;
            let _ = sweep_group(&self.child, None, None);
        }
    }
}

#[cfg(unix)]
mod guardian_unix {
    use super::{Containment, GuardianReport, MAX_PS_LINES, MAX_SURVIVORS, MAX_SWEEP_TARGETS};
    use std::time::{Duration, Instant};

    #[derive(Debug, Clone)]
    pub(super) struct ProcRow {
        pub(super) pid: u32,
        pub(super) pgid: i32,
        pub(super) sid: i32,
        pub(super) lstart: String,
    }

    pub(super) fn capture_ids(pid: u32) -> Option<super::ChildIds> {
        if pid == 0 {
            return None;
        }
        let pid_t = pid as libc::pid_t;
        // SAFETY: getpgid/getsid/getpgrp with a pid only read kernel state.
        let (pgid, sid, own) =
            unsafe { (libc::getpgid(pid_t), libc::getsid(pid_t), libc::getpgrp()) };
        if pgid <= 1 || sid < 0 {
            return None;
        }
        if pgid == own {
            // The child shares OUR group: a group sweep would suicide.
            return None;
        }
        Some(super::ChildIds {
            pid,
            pgid,
            sid,
            start: lstart_of(pid),
        })
    }

    pub(super) fn lstart_of(pid: u32) -> Option<String> {
        let out = std::process::Command::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if line.is_empty() {
            None
        } else {
            Some(line)
        }
    }

    /// One full process-table snapshot, filtered by the caller.
    pub(super) fn snapshot() -> Option<Vec<ProcRow>> {
        let out = std::process::Command::new("ps")
            .args(["-ax", "-o", "pid=,pgid=,sess=,lstart="])
            .output()
            .ok()?;
        if !out.status.success() || out.stdout.len() > 8 << 20 {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut rows = Vec::new();
        for line in text.lines().take(MAX_PS_LINES) {
            let mut parts = line.split_whitespace();
            let (Some(pid), Some(pgid), Some(sid)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let (Ok(pid), Ok(pgid), Ok(sid)) =
                (pid.parse::<u32>(), pgid.parse::<i32>(), sid.parse::<i32>())
            else {
                continue;
            };
            let lstart: String = parts.collect::<Vec<_>>().join(" ");
            rows.push(ProcRow {
                pid,
                pgid,
                sid,
                lstart,
            });
        }
        Some(rows)
    }

    /// Fresh single-pid identity check, used to re-verify each target
    /// immediately before signalling (closes the scan/kill TOCTOU).
    pub(super) fn reverify(pid: u32, pgid: i32, sid: i32) -> bool {
        let out = match std::process::Command::new("ps")
            .args(["-o", "pgid=,sess=", "-p", &pid.to_string()])
            .output()
        {
            Ok(o) => o,
            Err(_) => return false,
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut parts = text.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some(g), Some(s)) => g.parse::<i32>() == Ok(pgid) && s.parse::<i32>() == Ok(sid),
            _ => false,
        }
    }

    /// Guards (any failure refuses, never kills blindly):
    /// 1. no identity -> refuse; 2. any member with a foreign sid -> abort
    ///    (group id reused); 3. never pid 0/1/self; 4. the direct child pid
    ///    only when its start time still matches; 5. every other target
    ///    re-verified (pgid+sid) immediately before the signal.
    pub(super) fn sweep(
        child: &Option<super::ChildIds>,
        deadline: Option<Instant>,
        teardown_error: Option<String>,
    ) -> GuardianReport {
        let mk = |child_pid, pgid, containment| GuardianReport {
            child_pid,
            pgid,
            sid_verified: false,
            start_verified: None,
            signalled: Vec::new(),
            survivors: Vec::new(),
            containment,
            teardown_error: teardown_error.clone(),
        };
        let Some(child) = child.as_ref() else {
            return mk(
                None,
                None,
                Containment::Unknown {
                    reason: "child identity unavailable (no pid, exited early, or shared group)"
                        .to_string(),
                },
            );
        };
        // SAFETY: getpid only reads kernel state.
        let own = unsafe { libc::getpid() } as u32;
        let Some(rows) = snapshot() else {
            return mk(
                Some(child.pid),
                Some(child.pgid),
                Containment::Unknown {
                    reason: "process-table snapshot failed".to_string(),
                },
            );
        };
        let members: Vec<&ProcRow> = rows.iter().filter(|r| r.pgid == child.pgid).collect();
        if members.iter().any(|m| m.sid != child.sid) {
            return mk(
                Some(child.pid),
                Some(child.pgid),
                Containment::Refused {
                    reason: format!(
                        "group {} contains foreign-session members; id may be reused",
                        child.pgid
                    ),
                },
            );
        }
        let mut signalled = Vec::new();
        let mut sid_verified = true;
        let mut start_verified = None;
        for m in members.iter().take(MAX_SWEEP_TARGETS) {
            if m.pid <= 1 || m.pid == own {
                continue;
            }
            if m.pid == child.pid {
                match (&child.start, &m.lstart) {
                    (Some(a), b) if a == b => start_verified = Some(true),
                    (Some(_), _) => continue, // pid reused: refuse this pid
                    (None, _) => start_verified = None,
                }
            }
            if !reverify(m.pid, child.pgid, child.sid) {
                sid_verified = false;
                continue;
            }
            // SAFETY: kill(2) with a verified pid + SIGKILL has no memory effects.
            if unsafe { libc::kill(m.pid as libc::pid_t, libc::SIGKILL) } == 0 {
                signalled.push(m.pid);
            }
        }
        // Settle: bounded rescan for survivors.
        let settle = deadline
            .map(|d| {
                d.saturating_duration_since(Instant::now())
                    .min(super::SWEEP_SETTLE)
            })
            .unwrap_or(super::SWEEP_SETTLE)
            .min(Duration::from_secs(5));
        let start = Instant::now();
        let mut survivors = Vec::new();
        loop {
            let alive: Vec<u32> = snapshot()
                .unwrap_or_default()
                .iter()
                .filter(|r| r.pgid == child.pgid && r.sid == child.sid && r.pid > 1 && r.pid != own)
                .map(|r| r.pid)
                .collect();
            if alive.is_empty() {
                survivors.clear();
                break;
            }
            survivors = alive;
            if start.elapsed() >= settle {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        survivors.truncate(MAX_SURVIVORS);
        let containment = if survivors.is_empty() {
            Containment::Full
        } else {
            Containment::Partial
        };
        GuardianReport {
            child_pid: Some(child.pid),
            pgid: Some(child.pgid),
            sid_verified,
            start_verified,
            signalled,
            survivors,
            containment,
            teardown_error,
        }
    }
}

#[cfg(unix)]
impl ChildIds {
    fn capture(pid: u32) -> Option<Self> {
        guardian_unix::capture_ids(pid)
    }
}

#[cfg(unix)]
fn sweep_group(
    child: &Option<ChildIds>,
    deadline: Option<Instant>,
    teardown_error: Option<String>,
) -> GuardianReport {
    guardian_unix::sweep(child, deadline, teardown_error)
}

#[cfg(not(unix))]
impl ChildIds {
    fn capture(_pid: u32) -> Option<Self> {
        None
    }
}

#[cfg(not(unix))]
fn sweep_group(
    child: &Option<ChildIds>,
    _deadline: Option<Instant>,
    teardown_error: Option<String>,
) -> GuardianReport {
    GuardianReport {
        child_pid: child.as_ref().map(|c| c.pid),
        pgid: None,
        sid_verified: false,
        start_verified: None,
        signalled: Vec::new(),
        survivors: Vec::new(),
        containment: Containment::Unsupported,
        teardown_error,
    }
}

// The new handles stay shareable without any unsafe impl.
const _: fn() = || {
    fn share<T: Send + Sync>() {}
    share::<Shell>();
    share::<Guardian>();
};
