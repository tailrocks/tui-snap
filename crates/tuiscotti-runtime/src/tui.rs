//! Owned PTY session runtime (backlog R06, R07, R08, R10, R11-core).
//!
//! Backend: `portable-pty` 0.9 (PTY owner) + `alacritty_terminal` 0.26
//! (emulator), per `docs/PTY-BACKENDS.md`. No vendored engine, no git-only
//! crates, no fallback backend.
//!
//! ## Thread model (R06, R07)
//!
//! Each [`Session`] owns exactly two threads:
//!
//! - a **reader thread** that blocks on the PTY master and forwards byte
//!   batches to the worker over the op channel;
//! - a **worker thread** that owns the `alacritty_terminal::Term`, the PTY
//!   writer, and the child handle. ALL `Term` access happens on this thread.
//!   The session handle only sends ops and receives replies over channels.
//!
//! The worker publishes every new [`Observation`](tuiscotti_core::screen::Observation)
//! (grid + cursor + palette + modes captured together at one revision) into
//! shared state under a short critical section plus a `Condvar`. Waits block
//! on that condvar — never on a lock the worker needs — so a long wait cannot
//! block cancellation, observation, or unrelated sessions (R07). No `unsafe`
//! `Send`/`Sync` anywhere: confinement is structural.
//!
//! ## Waits (R10)
//!
//! [`Session::wait_predicate`], [`Session::wait_stable`],
//! [`Session::wait_frame`], and [`Session::wait_exit`] are distinct
//! operations. Timeouts and cancellation yield the latest evidence snapshot;
//! they never report success. [`Session::wait_frame`] is kitty-sync-gated:
//! this backend does not track DEC 2026, so it always fails closed with
//! [`WaitError::Unsupported`].
//!
//! ## Input (R11-core)
//!
//! Text, typed chords ([`parse_chord`] + [`Key`]), raw bytes,
//! press/down/repeat/up ([`KeyEventKind`]), negotiated bracketed paste,
//! mouse click/hover/drag/wheel, focus, resize, and signals. Encodings are
//! derived from the live `TermMode`: mouse/focus input is refused when the
//! application has not enabled the corresponding mode, and releases need the
//! kitty keyboard protocol. Shell sessions and paste edge cases belong to a
//! later agent.
//!
//! ## Cleanup (R08)
//!
//! [`Session::finish`] (graceful: EOF stdin, wait, reap) and
//! [`Session::close`] (forceful, idempotent) return teardown errors.
//! `Drop` reaps children and joins threads without double-panicking.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions as GridDims;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::{Config as TermConfig, TermMode};
use alacritty_terminal::vte::ansi::{
    Color as VteColor, CursorShape, NamedColor, Processor, Rgb as VteRgb,
};
use portable_pty::{native_pty_system, Child as PtyChild, CommandBuilder, MasterPty, PtySize};

use tuiscotti_core::frame::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, UnderlineStyle};
use tuiscotti_core::screen::{CaptureProvenance, CaptureReason, Maybe, Observation, Screen, TermState};

// ---------------------------------------------------------------------------
// Process-global PTY lifecycle guard (macOS `revoke()` race)
// ---------------------------------------------------------------------------

/// Serializes PTY open/spawn against kill/reap process-wide. This guards a
/// kernel race, not an emulator bug; every `portable-pty` consumer needs it.
static PTY_LIFECYCLE: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// Backend limits
// ---------------------------------------------------------------------------

/// Backend grid limits (alacritty minimum columns = 2; generous maximum).
pub const MIN_COLS: u16 = 2;
pub const MIN_ROWS: u16 = 1;
pub const MAX_COLS: u16 = 1000;
pub const MAX_ROWS: u16 = 1000;

/// How long after child exit the worker still accepts trailing reader bytes.
const DRAIN_GRACE: Duration = Duration::from_millis(500);
/// Worker tick: child-exit polling cadence.
const WORKER_TICK: Duration = Duration::from_millis(25);
/// Wait polling slice: cancel/deadline responsiveness (R07).
const WAIT_SLICE: Duration = Duration::from_millis(25);
/// Default quiet period for [`Session::wait_stable`].
pub const DEFAULT_STABLE_QUIET: Duration = Duration::from_millis(200);
/// Grace for SIGKILL-triggered reap during teardown.
const KILL_GRACE: Duration = Duration::from_secs(2);
/// Bound for joining one session thread during teardown. Must exceed the
/// worker's worst case (`KILL_GRACE` + tick) so a healthy-but-slow kill is
/// never misreported as stuck. The reader has no unblock handle (it owns
/// the only PTY reader), so a child that survives kill would block `read()`
/// forever — past this grace the thread is detached, never joined forever.
const JOIN_GRACE: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// TerminalProfile
// ---------------------------------------------------------------------------

/// Advertised terminal/protocol behavior for a session.
///
/// `spawn()` rejects any profile claiming a capability the backend cannot
/// implement — capabilities are never silently normalized away.
#[derive(Debug, Clone)]
pub struct TerminalProfile {
    /// `TERM` value exported to the child (child-only).
    pub term: String,
    /// Application may use SGR mouse (1006). Backend tracks it.
    pub mouse_sgr: bool,
    /// Application may use UTF-8 mouse (1005). Backend tracks it.
    pub mouse_utf8: bool,
    /// Application may use legacy X10 mouse (1000/1002/1003). Tracked.
    pub mouse_legacy: bool,
    /// Parse kitty progressive-enhancement flags (enables them in the
    /// emulator config so applications can negotiate them).
    pub kitty_keyboard: bool,
    /// Application may use bracketed paste (2004). Tracked.
    pub bracketed_paste: bool,
    /// Application may use focus tracking (1004). Tracked.
    pub focus_tracking: bool,
    /// Application may use the alternate screen (1049). Tracked.
    pub alt_screen: bool,
    /// Synchronized output (DEC 2026). **Backend lacks it**: `spawn()`
    /// fails when this is true, and `wait_frame` is unsupported.
    pub synchronized_output: bool,
    /// Per-cell blink (SGR 5/6). **Backend drops it**: `spawn()` fails
    /// when this is true rather than passing on a lossy grid.
    pub cell_blink: bool,
}

impl Default for TerminalProfile {
    fn default() -> Self {
        Self {
            term: "xterm-256color".to_string(),
            mouse_sgr: true,
            mouse_utf8: true,
            mouse_legacy: true,
            kitty_keyboard: true,
            bracketed_paste: true,
            focus_tracking: true,
            alt_screen: true,
            synchronized_output: false,
            cell_blink: false,
        }
    }
}

impl TerminalProfile {
    /// Reject profiles advertising what the backend lacks.
    fn check(&self) -> Result<(), TuiError> {
        if self.synchronized_output {
            return Err(TuiError::Unsupported(
                "profile advertises synchronized-output (DEC 2026): backend cannot track it",
            ));
        }
        if self.cell_blink {
            return Err(TuiError::Unsupported(
                "profile advertises per-cell blink: backend drops SGR 5/6",
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// A key independent of modifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// A Unicode character (letters, digits, punctuation, space, ...).
    Char(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// Function key 1..=12.
    F(u8),
}

/// Modifier set for [`Key`] input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyMods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Super/Cmd/Win. Legacy encodings cannot carry it: with super held and
    /// no kitty keyboard active, key input fails closed with `Unsupported`.
    pub sup: bool,
}

impl KeyMods {
    pub const NONE: KeyMods = KeyMods {
        ctrl: false,
        alt: false,
        shift: false,
        sup: false,
    };
    pub const CTRL: KeyMods = KeyMods {
        ctrl: true,
        alt: false,
        shift: false,
        sup: false,
    };
    pub const ALT: KeyMods = KeyMods {
        ctrl: false,
        alt: true,
        shift: false,
        sup: false,
    };
    pub const SHIFT: KeyMods = KeyMods {
        ctrl: false,
        alt: false,
        shift: true,
        sup: false,
    };

    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.ctrl && !self.alt && !self.shift && !self.sup
    }
}

/// Key event kind. Without the kitty keyboard protocol, `Up` (release)
/// emits no bytes — legacy encodings cannot represent releases — while
/// `Press`, `Down`, and `Repeat` all emit the key's legacy bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventKind {
    /// A complete press (down immediately followed by up).
    Press,
    /// Key down (no automatic release).
    Down,
    /// Auto-repeat tick while held.
    Repeat,
    /// Key release.
    Up,
}

/// Parse a typed chord such as `"Ctrl+P"`, `"Alt+Enter"`, `"Shift+F5"`,
/// `"F1"`, or a bare `"a"`.
///
/// Modifiers (case-insensitive, any order): `Ctrl`/`Control`/`Ctl`,
/// `Alt`/`Opt`/`Meta`, `Shift`, `Super`/`Cmd`/`Win`/`Command`. The final
/// segment names the key: `Enter`/`Return`, `Tab`, `Backspace`/`BS`,
/// `Esc`/`Escape`, `Space`, arrows, `Home`/`End`, `PageUp`/`PgUp`,
/// `PageDown`/`PgDn`, `Insert`/`Ins`, `Delete`/`Del`, `F1`..`F12`, or any
/// single character.
pub fn parse_chord(text: &str) -> Result<(Key, KeyMods), TuiError> {
    let bad = |m: String| TuiError::Chord(m);
    let mut parts: Vec<&str> = text.split('+').collect();
    if parts.is_empty() {
        return Err(bad("empty chord".to_string()));
    }
    let name = parts.pop().unwrap_or_default();
    if name.is_empty() {
        return Err(bad(format!("empty key in chord {text:?}")));
    }
    let mut mods = KeyMods::NONE;
    for m in parts {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "ctl" => mods.ctrl = true,
            "alt" | "opt" | "meta" => mods.alt = true,
            "shift" => mods.shift = true,
            "super" | "cmd" | "win" | "windows" | "command" => mods.sup = true,
            "" => return Err(bad(format!("empty modifier in chord {text:?}"))),
            other => return Err(bad(format!("unknown modifier {other:?} in chord {text:?}"))),
        }
    }
    let key = match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Key::Enter,
        "tab" => Key::Tab,
        "backspace" | "bs" => Key::Backspace,
        "esc" | "escape" => Key::Escape,
        "space" => Key::Char(' '),
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        "insert" | "ins" => Key::Insert,
        "delete" | "del" => Key::Delete,
        _ => {
            if let Some(rest) = name.strip_prefix('F').or_else(|| name.strip_prefix('f')) {
                match rest.parse::<u8>() {
                    Ok(n) if (1..=12).contains(&n) => Key::F(n),
                    _ => return Err(bad(format!("bad function key {name:?}"))),
                }
            } else if name.chars().count() == 1 {
                Key::Char(name.chars().next().unwrap_or('?'))
            } else {
                return Err(bad(format!("unknown key {name:?}")));
            }
        }
    };
    Ok((key, mods))
}

// ---------------------------------------------------------------------------
// Mouse / focus / signals
// ---------------------------------------------------------------------------

/// Mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// Wheel direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wheel {
    Up,
    Down,
    Left,
    Right,
}

/// Modifier set for mouse input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseMods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl MouseMods {
    pub const NONE: MouseMods = MouseMods {
        shift: false,
        alt: false,
        ctrl: false,
    };
}

/// Process signal for [`Session::signal`]. Unix only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Int,
    Term,
    Kill,
    Quit,
    Hup,
    Custom(i32),
}

#[cfg(unix)]
impl Signal {
    fn number(self) -> i32 {
        match self {
            Signal::Int => libc::SIGINT,
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
            Signal::Quit => libc::SIGQUIT,
            Signal::Hup => libc::SIGHUP,
            Signal::Custom(n) => n,
        }
    }
}

// ---------------------------------------------------------------------------
// Exit status
// ---------------------------------------------------------------------------

/// Termination status of the session's direct child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitStatus {
    code: u32,
    signal: Option<String>,
}

impl ExitStatus {
    #[must_use]
    pub fn success(&self) -> bool {
        self.signal.is_none() && self.code == 0
    }

    #[must_use]
    pub fn code(&self) -> u32 {
        self.code
    }

    #[must_use]
    pub fn signal(&self) -> Option<&str> {
        self.signal.as_deref()
    }
}

impl std::fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.signal {
            Some(sig) => write!(f, "terminated by {sig}"),
            None => write!(f, "exited with code {}", self.code),
        }
    }
}

impl From<portable_pty::ExitStatus> for ExitStatus {
    fn from(s: portable_pty::ExitStatus) -> Self {
        Self {
            code: s.exit_code(),
            signal: s.signal().map(str::to_string),
        }
    }
}

/// A reaped exit plus the final evidence observation.
#[derive(Debug, Clone)]
pub struct ExitWait {
    pub status: ExitStatus,
    pub observation: Observation,
}

impl ExitWait {
    /// Assert successful termination, yielding the final observation.
    pub fn success(self) -> Result<Observation, TuiError> {
        if self.status.success() {
            Ok(self.observation)
        } else {
            Err(TuiError::Assertion(format!(
                "expected successful exit, got {} (revision {})",
                self.status, self.observation.revision
            )))
        }
    }

    /// Assert an exact exit code, yielding the final observation.
    pub fn code(self, expected: u32) -> Result<Observation, TuiError> {
        if self.status.signal().is_none() && self.status.code() == expected {
            Ok(self.observation)
        } else {
            Err(TuiError::Assertion(format!(
                "expected exit code {expected}, got {} (revision {})",
                self.status, self.observation.revision
            )))
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Session failure.
#[derive(Debug, Clone)]
pub enum TuiError {
    /// Spawn failed (unresolved binary, PTY open, validation, ...).
    Spawn(String),
    /// Invalid argument (bad size, coordinates, empty input, ...).
    InvalidInput(String),
    /// Unparseable chord.
    Chord(String),
    /// Capability the backend cannot provide. Fails closed, never emulated.
    Unsupported(&'static str),
    /// Input refused: the application has not enabled the mode (mouse
    /// reporting, focus tracking, ...) that would make it meaningful.
    ModeNotEnabled(&'static str),
    /// Paste rejected: content contains the bracketed-paste delimiters.
    PasteRejected(String),
    /// PTY I/O failure.
    Io(String),
    /// Deadlines: wait expired / worker unresponsive.
    Timeout(String),
    /// Input or observation refused: the child already exited.
    ChildExited(String),
    /// Session is closed (or its worker is gone).
    Closed(String),
    /// Teardown itself failed (kill/reap/join errors from finish/close).
    Teardown(String),
    /// Signal delivery failed.
    Signal(String),
    /// `ExitWait::success` / `code` assertion failed.
    Assertion(String),
}

impl std::fmt::Display for TuiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(m) => write!(f, "spawn failed: {m}"),
            Self::InvalidInput(m) => write!(f, "invalid input: {m}"),
            Self::Chord(m) => write!(f, "bad chord: {m}"),
            Self::Unsupported(m) => write!(f, "unsupported: {m}"),
            Self::ModeNotEnabled(m) => write!(f, "mode not enabled: {m}"),
            Self::PasteRejected(m) => write!(f, "paste rejected: {m}"),
            Self::Io(m) => write!(f, "pty io: {m}"),
            Self::Timeout(m) => write!(f, "timeout: {m}"),
            Self::ChildExited(m) => write!(f, "child exited: {m}"),
            Self::Closed(m) => write!(f, "session closed: {m}"),
            Self::Teardown(m) => write!(f, "teardown failed: {m}"),
            Self::Signal(m) => write!(f, "signal failed: {m}"),
            Self::Assertion(m) => write!(f, "exit assertion failed: {m}"),
        }
    }
}

impl std::error::Error for TuiError {}

/// Wait failure. Every variant carries the latest evidence snapshot; a wait
/// never reports success without its condition holding. Evidence is boxed:
/// snapshots are large and only travel on failure paths.
#[derive(Debug, Clone)]
pub enum WaitError {
    Timeout {
        waited: Duration,
        evidence: Box<Observation>,
    },
    Cancelled {
        evidence: Box<Observation>,
    },
    /// wait_frame only: the backend cannot track synchronized frames.
    Unsupported {
        capability: &'static str,
        evidence: Box<Observation>,
    },
    /// The session closed before the condition held.
    Closed {
        evidence: Option<Box<Observation>>,
    },
}

impl WaitError {
    #[must_use]
    pub fn evidence(&self) -> Option<&Observation> {
        match self {
            Self::Timeout { evidence, .. }
            | Self::Cancelled { evidence }
            | Self::Unsupported { evidence, .. } => Some(evidence.as_ref()),
            Self::Closed { evidence } => evidence.as_deref(),
        }
    }
}

impl std::fmt::Display for WaitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout { waited, evidence } => write!(
                f,
                "wait timed out after {waited:?} (evidence at revision {})",
                evidence.revision
            ),
            Self::Cancelled { evidence } => write!(
                f,
                "wait cancelled (evidence at revision {})",
                evidence.revision
            ),
            Self::Unsupported {
                capability,
                evidence,
            } => write!(
                f,
                "wait unsupported ({capability}; evidence at revision {})",
                evidence.revision
            ),
            Self::Closed { .. } => write!(f, "wait failed: session closed"),
        }
    }
}

impl std::error::Error for WaitError {}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

/// Cooperative cancellation token shared across threads. Every blocking wait
/// takes one; cancelling unblocks the wait with [`WaitError::Cancelled`].
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------
// Tui builder
// ---------------------------------------------------------------------------

enum Program {
    Argv(Vec<String>),
    CargoBin(String),
}

/// PTY session builder. Environment and working directory apply to the child
/// only; the parent process is never mutated.
pub struct Tui {
    program: Program,
    extra_args: Vec<String>,
    size: (u16, u16),
    env: Vec<(String, String)>,
    cwd: Option<PathBuf>,
    profile: TerminalProfile,
}

impl Tui {
    /// Launch `argv[0]` with `argv[1..]` as arguments.
    pub fn new<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            program: Program::Argv(argv.into_iter().map(Into::into).collect()),
            extra_args: Vec::new(),
            size: (80, 24),
            env: Vec::new(),
            cwd: None,
            profile: TerminalProfile::default(),
        }
    }

    /// Launch a cargo-built binary of this package by name. Resolved at
    /// `spawn()` time: `CARGO_BIN_EXE_<name>` when set, else the binary next
    /// to the current test executable's directory (`target/debug/<name>`).
    /// Resolution failure surfaces from `spawn()` listing every path tried.
    pub fn cargo_bin(name: &str) -> Self {
        Self {
            program: Program::CargoBin(name.to_string()),
            extra_args: Vec::new(),
            size: (80, 24),
            env: Vec::new(),
            cwd: None,
            profile: TerminalProfile::default(),
        }
    }

    #[must_use]
    pub fn arg(mut self, arg: &str) -> Self {
        self.extra_args.push(arg.to_string());
        self
    }

    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.extra_args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Initial PTY/emulator size. Backend limits: 2..=1000 columns,
    /// 1..=1000 rows (static 1x1 screens stay valid outside the PTY path).
    #[must_use]
    pub fn size(mut self, cols: u16, rows: u16) -> Self {
        self.size = (cols, rows);
        self
    }

    /// Child-only environment entry. Never touches the parent environment.
    #[must_use]
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_string(), value.to_string()));
        self
    }

    /// Child working directory.
    #[must_use]
    pub fn cwd(mut self, dir: PathBuf) -> Self {
        self.cwd = Some(dir);
        self
    }

    /// Advertised terminal behavior. Profiles claiming backend-unsupported
    /// capabilities are rejected by `spawn()`.
    #[must_use]
    pub fn profile(mut self, profile: TerminalProfile) -> Self {
        self.profile = profile;
        self
    }

    /// Spawn the child in a new PTY and start the session threads.
    pub fn spawn(self) -> Result<Session, TuiError> {
        self.profile.check()?;
        let (cols, rows) = self.size;
        if !(MIN_COLS..=MAX_COLS).contains(&cols) {
            return Err(TuiError::Spawn(format!(
                "cols {cols} outside backend range {MIN_COLS}..={MAX_COLS}"
            )));
        }
        if !(MIN_ROWS..=MAX_ROWS).contains(&rows) {
            return Err(TuiError::Spawn(format!(
                "rows {rows} outside backend range {MIN_ROWS}..={MAX_ROWS}"
            )));
        }
        let argv = self.resolve_argv()?;
        if argv.is_empty() {
            return Err(TuiError::Spawn("empty argv".to_string()));
        }

        let mut cmd = CommandBuilder::new(&argv[0]);
        for a in argv.iter().skip(1).chain(self.extra_args.iter()) {
            cmd.arg(a);
        }
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        if cmd.get_env("TERM").is_none() {
            cmd.env("TERM", self.profile.term.clone());
        }
        if let Some(cwd) = &self.cwd {
            cmd.cwd(cwd);
        }

        let _guard = PTY_LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| TuiError::Spawn(format!("openpty failed: {e:#}")))?;
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| TuiError::Spawn(format!("spawn failed: {e:#}")))?;
        // Drain discipline: take I/O handles before any wait can run.
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| TuiError::Spawn(format!("pty reader failed: {e:#}")))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| TuiError::Spawn(format!("pty writer failed: {e:#}")))?;
        child
            .try_wait()
            .map_err(|e| TuiError::Spawn(format!("child poll failed: {e:#}")))?;
        let pid = child.process_id();
        drop(_guard);

        let (op_tx, op_rx) = mpsc::channel::<Op>();
        let shared = Arc::new(Shared::new());
        let term_config = TermConfig {
            kitty_keyboard: self.profile.kitty_keyboard,
            ..TermConfig::default()
        };

        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("tuisnap-tui-worker".to_string())
            .spawn(move || {
                run_worker(
                    pair.master,
                    child,
                    writer,
                    term_config,
                    cols,
                    rows,
                    pid,
                    op_rx,
                    worker_shared,
                );
            })
            .map_err(|e| TuiError::Spawn(format!("worker spawn failed: {e}")))?;

        let feed_tx = op_tx.clone();
        let reader_thread = std::thread::Builder::new()
            .name("tuisnap-tui-reader".to_string())
            .spawn(move || run_reader(reader, feed_tx))
            .map_err(|e| TuiError::Spawn(format!("reader spawn failed: {e}")))?;

        let session = Session {
            op_tx: Some(op_tx),
            shared,
            worker: Mutex::new(Some(worker)),
            reader: Mutex::new(Some(reader_thread)),
            closed: AtomicBool::new(false),
            pid,
        };
        // The session is usable only once the worker published revision 0.
        session.await_initial()?;
        Ok(session)
    }

    fn resolve_argv(&self) -> Result<Vec<String>, TuiError> {
        match &self.program {
            Program::Argv(argv) => Ok(argv.clone()),
            Program::CargoBin(name) => {
                let var = format!(
                    "CARGO_BIN_EXE_{}",
                    name.replace('-', "_").to_ascii_uppercase()
                );
                let mut tried = Vec::new();
                if let Ok(p) = std::env::var(&var) {
                    return Ok(vec![p]);
                }
                tried.push(format!("env {var} (unset)"));
                if let Ok(exe) = std::env::current_exe() {
                    if let Some(deps) = exe.parent() {
                        let dir = if deps.file_name().is_some_and(|n| n == "deps") {
                            deps.parent().unwrap_or(deps).to_path_buf()
                        } else {
                            deps.to_path_buf()
                        };
                        #[cfg(windows)]
                        let candidate = dir.join(format!("{name}.exe"));
                        #[cfg(not(windows))]
                        let candidate = dir.join(name);
                        tried.push(candidate.display().to_string());
                        if candidate.is_file() {
                            return Ok(vec![candidate.to_string_lossy().into_owned()]);
                        }
                    }
                }
                Err(TuiError::Spawn(format!(
                    "binary {name:?} not found; tried: {}",
                    tried.join(", ")
                )))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// Owned PTY session: the child, its emulator, and both I/O threads.
/// `Send + Sync`; concurrent sessions are fully independent (R07).
pub struct Session {
    op_tx: Option<mpsc::Sender<Op>>,
    shared: Arc<Shared>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    reader: Mutex<Option<std::thread::JoinHandle<()>>>,
    closed: AtomicBool,
    pid: Option<u32>,
}

// No unsafe impls: every field is Send + Sync by construction, while the
// `Term` itself never leaves the worker thread.

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("pid", &self.pid)
            .field("revision", &self.revision())
            .field("closed", &self.closed.load(Ordering::SeqCst))
            .field("exited", &self.poll_exit().is_some())
            .finish()
    }
}

impl Session {
    /// Direct child PID, when the platform reports one.
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Latest published revision (a short critical section; never blocks on
    /// the worker).
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.shared.revision()
    }

    /// Non-blocking exit poll. `Some` once the worker reaped the child.
    #[must_use]
    pub fn poll_exit(&self) -> Option<ExitStatus> {
        self.shared.exit()
    }

    // -- observation ------------------------------------------------------

    /// Fresh atomic capture at the worker's current revision (R06).
    pub fn observe_now(&self) -> Result<Observation, TuiError> {
        let (tx, rx) = mpsc::channel();
        self.send(Op::Observe { reply: tx })?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| TuiError::Timeout("observe_now: worker unresponsive".to_string()))?
    }

    /// Fresh grid snapshot.
    pub fn snapshot(&self) -> Result<Screen, TuiError> {
        Ok(self.observe_now()?.screen)
    }

    /// Wait until `predicate` holds, the deadline passes, or `cancel` fires.
    /// Timeout/cancel yield evidence; they never report success.
    pub fn wait_predicate<F>(
        &self,
        predicate: F,
        deadline: Instant,
        cancel: &CancelToken,
    ) -> Result<Observation, WaitError>
    where
        F: Fn(&Observation) -> bool,
    {
        self.wait_loop(deadline, cancel, |latest| {
            latest.filter(|o| predicate(o)).cloned()
        })
    }

    /// Wait until no new revision arrives for `quiet` (default
    /// [`DEFAULT_STABLE_QUIET`]): output settled, not business completion.
    pub fn wait_stable(
        &self,
        deadline: Instant,
        cancel: &CancelToken,
    ) -> Result<Observation, WaitError> {
        self.wait_stable_quiet(deadline, DEFAULT_STABLE_QUIET, cancel)
    }

    /// [`Session::wait_stable`] with an explicit quiet period.
    pub fn wait_stable_quiet(
        &self,
        deadline: Instant,
        quiet: Duration,
        cancel: &CancelToken,
    ) -> Result<Observation, WaitError> {
        let start = Instant::now();
        // Seed from the latest revision; a settled session returns quickly.
        let mut seen = self.latest_or_closed()?.revision;
        let mut quiet_since = Instant::now();
        loop {
            if cancel.is_cancelled() {
                return Err(WaitError::Cancelled {
                    evidence: Box::new(self.latest_or_closed()?),
                });
            }
            if Instant::now() >= deadline {
                return Err(WaitError::Timeout {
                    waited: start.elapsed(),
                    evidence: Box::new(self.latest_or_closed()?),
                });
            }
            match self.shared.wait_for_newer_than(seen, deadline, cancel) {
                Some(obs) => {
                    seen = obs.revision;
                    quiet_since = Instant::now();
                }
                None => {
                    if cancel.is_cancelled() {
                        return Err(WaitError::Cancelled {
                            evidence: Box::new(self.latest_or_closed()?),
                        });
                    }
                    if Instant::now() >= deadline {
                        return Err(WaitError::Timeout {
                            waited: start.elapsed(),
                            evidence: Box::new(self.latest_or_closed()?),
                        });
                    }
                    // No newer revision within the slice: check quiet.
                    if quiet_since.elapsed() >= quiet {
                        return self.latest_or_closed();
                    }
                }
            }
        }
    }

    /// Wait for the next synchronized frame (DEC 2026). The backend does
    /// not track synchronized output, so this always fails closed with
    /// [`WaitError::Unsupported`] plus an evidence snapshot (R10).
    pub fn wait_frame(
        &self,
        _deadline: Instant,
        cancel: &CancelToken,
    ) -> Result<Observation, WaitError> {
        let evidence = Box::new(self.latest_or_closed()?);
        if cancel.is_cancelled() {
            return Err(WaitError::Cancelled { evidence });
        }
        Err(WaitError::Unsupported {
            capability: "synchronized-output (DEC 2026)",
            evidence,
        })
    }

    /// Wait until the direct child exits and is reaped.
    pub fn wait_exit(
        &self,
        deadline: Instant,
        cancel: &CancelToken,
    ) -> Result<ExitWait, WaitError> {
        let start = Instant::now();
        loop {
            if cancel.is_cancelled() {
                return Err(WaitError::Cancelled {
                    evidence: Box::new(self.latest_or_closed()?),
                });
            }
            if let Some(status) = self.shared.exit() {
                return Ok(ExitWait {
                    status,
                    observation: self.latest_or_closed()?,
                });
            }
            if Instant::now() >= deadline {
                return Err(WaitError::Timeout {
                    waited: start.elapsed(),
                    evidence: Box::new(self.latest_or_closed()?),
                });
            }
            self.shared.wait_changed(deadline, cancel);
        }
    }

    /// [`Session::wait_exit`] as an assertion entry point; the returned
    /// [`ExitWait`] offers `.success()` / `.code(n)`.
    pub fn expect_exit(
        &self,
        deadline: Instant,
        cancel: &CancelToken,
    ) -> Result<ExitWait, WaitError> {
        self.wait_exit(deadline, cancel)
    }

    // -- input ------------------------------------------------------------

    /// Send literal text (UTF-8 bytes, no chord interpretation).
    pub fn send_text(&self, text: &str) -> Result<(), TuiError> {
        self.send_input(Input::Bytes(text.as_bytes().to_vec()))
    }

    /// Send raw bytes verbatim.
    pub fn send_bytes(&self, bytes: &[u8]) -> Result<(), TuiError> {
        self.send_input(Input::Bytes(bytes.to_vec()))
    }

    /// Negotiated paste: wrapped in `ESC[200~...ESC[201~` when the
    /// application enabled bracketed paste (2004), sent plain otherwise.
    /// Content containing either delimiter is rejected outright.
    pub fn paste(&self, text: &str) -> Result<(), TuiError> {
        self.send_input(Input::Paste(text.to_string()))
    }

    /// Parse `chord` ([`parse_chord`]) and send it as a complete press.
    pub fn press(&self, chord: &str) -> Result<(), TuiError> {
        let (key, mods) = parse_chord(chord)?;
        self.press_key(key, mods)
    }

    /// Send a complete key press.
    pub fn press_key(&self, key: Key, mods: KeyMods) -> Result<(), TuiError> {
        self.key_event(key, mods, KeyEventKind::Press)
    }

    /// Send a key-down event (no automatic release).
    pub fn key_down(&self, key: Key, mods: KeyMods) -> Result<(), TuiError> {
        self.key_event(key, mods, KeyEventKind::Down)
    }

    /// Send a key-repeat tick.
    pub fn key_repeat(&self, key: Key, mods: KeyMods) -> Result<(), TuiError> {
        self.key_event(key, mods, KeyEventKind::Repeat)
    }

    /// Send a key-release event. Releases emit bytes only with the kitty
    /// keyboard protocol active; otherwise this is a successful no-op.
    pub fn key_up(&self, key: Key, mods: KeyMods) -> Result<(), TuiError> {
        self.key_event(key, mods, KeyEventKind::Up)
    }

    /// Send a key event of any kind.
    pub fn key_event(&self, key: Key, mods: KeyMods, kind: KeyEventKind) -> Result<(), TuiError> {
        if matches!(key, Key::F(n) if !(1..=12).contains(&n)) {
            return Err(TuiError::InvalidInput(format!(
                "function key out of range: {key:?}"
            )));
        }
        self.send_input(Input::Key { key, mods, kind })
    }

    /// Click: button down immediately followed by button up at `(x, y)`.
    pub fn click(
        &self,
        button: MouseButton,
        x: u16,
        y: u16,
        mods: MouseMods,
    ) -> Result<(), TuiError> {
        self.mouse_down(button, x, y, mods)?;
        self.mouse_up(button, x, y, mods)
    }

    /// Button press at `(x, y)`.
    pub fn mouse_down(
        &self,
        button: MouseButton,
        x: u16,
        y: u16,
        mods: MouseMods,
    ) -> Result<(), TuiError> {
        self.send_input(Input::Mouse {
            action: MouseAction::Press(button),
            x,
            y,
            mods,
        })
    }

    /// Button release at `(x, y)`.
    pub fn mouse_up(
        &self,
        button: MouseButton,
        x: u16,
        y: u16,
        mods: MouseMods,
    ) -> Result<(), TuiError> {
        let _ = button;
        self.send_input(Input::Mouse {
            action: MouseAction::Release,
            x,
            y,
            mods,
        })
    }

    /// Hover (motion with no button held) to `(x, y)`.
    pub fn mouse_move(&self, x: u16, y: u16, mods: MouseMods) -> Result<(), TuiError> {
        self.send_input(Input::Mouse {
            action: MouseAction::Move { held: None },
            x,
            y,
            mods,
        })
    }

    /// Drag step: motion with `button` held, to `(x, y)`.
    pub fn mouse_drag(
        &self,
        button: MouseButton,
        x: u16,
        y: u16,
        mods: MouseMods,
    ) -> Result<(), TuiError> {
        self.send_input(Input::Mouse {
            action: MouseAction::Move { held: Some(button) },
            x,
            y,
            mods,
        })
    }

    /// Wheel event at `(x, y)`.
    pub fn mouse_wheel(
        &self,
        wheel: Wheel,
        x: u16,
        y: u16,
        mods: MouseMods,
    ) -> Result<(), TuiError> {
        self.send_input(Input::Mouse {
            action: MouseAction::Wheel(wheel),
            x,
            y,
            mods,
        })
    }

    /// Focus-in (`CSI I`). Refused unless the application enabled 1004.
    pub fn focus_in(&self) -> Result<(), TuiError> {
        self.send_input(Input::Focus(true))
    }

    /// Focus-out (`CSI O`). Refused unless the application enabled 1004.
    pub fn focus_out(&self) -> Result<(), TuiError> {
        self.send_input(Input::Focus(false))
    }

    /// Resize the PTY and the emulator together (atomic from the test's
    /// view: one revision, reason `Resize`).
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), TuiError> {
        if !(MIN_COLS..=MAX_COLS).contains(&cols) {
            return Err(TuiError::InvalidInput(format!(
                "cols {cols} outside backend range {MIN_COLS}..={MAX_COLS}"
            )));
        }
        if !(MIN_ROWS..=MAX_ROWS).contains(&rows) {
            return Err(TuiError::InvalidInput(format!(
                "rows {rows} outside backend range {MIN_ROWS}..={MAX_ROWS}"
            )));
        }
        let (tx, rx) = mpsc::channel();
        self.send(Op::Resize {
            cols,
            rows,
            reply: tx,
        })?;
        recv_reply(rx, "resize")
    }

    /// Deliver a signal to the direct child (Unix only).
    #[cfg(unix)]
    pub fn signal(&self, signal: Signal) -> Result<(), TuiError> {
        let pid = self
            .pid
            .ok_or_else(|| TuiError::Signal("child PID unknown on this platform".to_string()))?;
        if self.shared.exit().is_some() {
            return Err(TuiError::ChildExited("child already exited".to_string()));
        }
        // No libc: `kill(1)` exit status follows the `pid_alive` convention
        // (the workspace forbids `unsafe`). A failed signal against a dead
        // pid still maps to `ChildExited`, matching the old ESRCH branch.
        let delivered = std::process::Command::new("kill")
            .arg(format!("-{}", signal.number()))
            .arg(pid.to_string())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if delivered {
            return Ok(());
        }
        if !process_exists(pid) {
            return Err(TuiError::ChildExited(format!(
                "child {pid} no longer exists"
            )));
        }
        Err(TuiError::Signal(format!("kill({pid}) failed")))
    }

    /// Non-Unix stub: signals are unsupported.
    #[cfg(not(unix))]
    pub fn signal(&self, _signal: Signal) -> Result<(), TuiError> {
        Err(TuiError::Unsupported("signals require a Unix platform"))
    }

    // -- teardown (R08) ----------------------------------------------------

    /// Graceful shutdown: EOF stdin, wait for natural exit until `deadline`,
    /// reap. On timeout the child is killed and a timeout error (with the
    /// final evidence revision noted) is returned; teardown still completes.
    pub fn finish(mut self, deadline: Instant) -> Result<ExitStatus, TuiError> {
        let cancel = CancelToken::new();
        match self.close_input() {
            Ok(()) => {}
            Err(TuiError::ChildExited(_)) => {}
            Err(e) => return Err(e),
        }
        match self.wait_exit(deadline, &cancel) {
            Ok(w) => {
                self.close()?;
                Ok(w.status)
            }
            Err(WaitError::Timeout { evidence, waited }) => {
                let _ = self.close();
                Err(TuiError::Timeout(format!(
                    "finish: child still alive after {waited:?}; killed during teardown (evidence at revision {})",
                    evidence.revision
                )))
            }
            Err(WaitError::Cancelled { .. }) => {
                let _ = self.close();
                Err(TuiError::Timeout(
                    "finish: wait cancelled via internal token (unreachable)".to_string(),
                ))
            }
            Err(WaitError::Unsupported { .. }) => {
                let _ = self.close();
                Err(TuiError::Timeout(
                    "finish: unsupported wait (unreachable)".to_string(),
                ))
            }
            Err(WaitError::Closed { .. }) => {
                let _ = self.close();
                Err(TuiError::Closed("finish: session closed".to_string()))
            }
        }
    }

    /// Forceful idempotent teardown: kill a living child (bounded grace),
    /// reap, join threads. Returns the first teardown error, if any.
    pub fn close(&mut self) -> Result<(), TuiError> {
        self.teardown();
        if let Some(msg) = self.shared.teardown_error() {
            return Err(TuiError::Teardown(msg));
        }
        Ok(())
    }

    // -- internals ---------------------------------------------------------

    fn send(&self, op: Op) -> Result<(), TuiError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(TuiError::Closed("session is closed".to_string()));
        }
        match &self.op_tx {
            Some(tx) => tx
                .send(op)
                .map_err(|_| TuiError::Closed("worker is gone".to_string())),
            None => Err(TuiError::Closed("session is closed".to_string())),
        }
    }

    fn send_input(&self, input: Input) -> Result<(), TuiError> {
        let (tx, rx) = mpsc::channel();
        self.send(Op::Input { input, reply: tx })?;
        recv_reply(rx, "input")
    }

    fn close_input(&self) -> Result<(), TuiError> {
        let (tx, rx) = mpsc::channel();
        self.send(Op::CloseInput { reply: tx })?;
        recv_reply(rx, "close stdin")
    }

    fn await_initial(&self) -> Result<(), TuiError> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let cancel = CancelToken::new();
        loop {
            if self.shared.latest().is_some() {
                return Ok(());
            }
            if self.shared.is_closed() {
                return Err(TuiError::Spawn(
                    "worker exited before publishing revision 0".to_string(),
                ));
            }
            if Instant::now() >= deadline {
                return Err(TuiError::Spawn(
                    "worker did not publish revision 0 in time".to_string(),
                ));
            }
            self.shared.wait_changed(deadline, &cancel);
        }
    }

    fn latest_or_closed(&self) -> Result<Observation, WaitError> {
        match self.shared.latest() {
            Some(o) => Ok(o),
            None if self.shared.is_closed() => Err(WaitError::Closed { evidence: None }),
            None => Err(WaitError::Closed { evidence: None }),
        }
    }

    fn wait_loop<F>(
        &self,
        deadline: Instant,
        cancel: &CancelToken,
        mut done: F,
    ) -> Result<Observation, WaitError>
    where
        F: FnMut(Option<&Observation>) -> Option<Observation>,
    {
        let start = Instant::now();
        loop {
            if cancel.is_cancelled() {
                return Err(WaitError::Cancelled {
                    evidence: Box::new(self.latest_or_closed()?),
                });
            }
            let latest = self.shared.latest();
            if let Some(obs) = done(latest.as_ref()) {
                return Ok(obs);
            }
            if Instant::now() >= deadline {
                return Err(WaitError::Timeout {
                    waited: start.elapsed(),
                    evidence: Box::new(self.latest_or_closed()?),
                });
            }
            self.shared.wait_changed(deadline, cancel);
        }
    }

    /// Run teardown exactly once; never panics (safe from `Drop`).
    fn teardown(&mut self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            // A previous close/drop already shut down; still join in case a
            // concurrent teardown is in flight.
            self.join_threads();
            return;
        }
        if let Some(tx) = self.op_tx.take() {
            let _ = tx.send(Op::Shutdown);
        }
        self.join_threads();
    }

    fn join_threads(&mut self) {
        let worker = self.worker.lock().map(|mut g| g.take()).unwrap_or(None);
        let reader = self.reader.lock().map(|mut g| g.take()).unwrap_or(None);
        if let Some(h) = worker {
            join_one(h, &self.shared, "worker", JOIN_GRACE);
        }
        if let Some(h) = reader {
            join_one(h, &self.shared, "reader", JOIN_GRACE);
        }
        self.shared.mark_closed();
    }
}

/// Bounded join of one session thread; never blocks past `grace`, never
/// panics. On timeout the handle is detached (the waiter thread owns it
/// and reaps the thread if it ever exits) and a teardown diagnostic is
/// recorded, so `Drop` can never hang on a reader blocked in `read()`
/// after a failed child kill. On waiter-spawn failure the handle drops
/// here, which also detaches rather than hangs.
fn join_one(h: std::thread::JoinHandle<()>, shared: &Shared, name: &str, grace: Duration) {
    let (tx, rx) = mpsc::channel::<bool>();
    let waiter = std::thread::Builder::new()
        .name(format!("tuisnap-tui-join-{name}"))
        .spawn(move || {
            let panicked = h.join().is_err();
            let _ = tx.send(panicked);
        });
    match waiter {
        Ok(_waiter) => match rx.recv_timeout(grace) {
            Ok(true) => shared.record_teardown(&format!("{name} thread panicked")),
            Ok(false) => {}
            Err(_) => shared.record_teardown(&format!(
                "{name} thread did not exit within {grace:?}; detached"
            )),
        },
        Err(e) => shared.record_teardown(&format!("join waiter spawn failed for {name}: {e}")),
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Never panic from Drop: teardown paths only record errors.
        self.teardown();
    }
}

fn recv_reply(rx: mpsc::Receiver<Result<(), TuiError>>, what: &str) -> Result<(), TuiError> {
    rx.recv_timeout(Duration::from_secs(10))
        .map_err(|_| TuiError::Timeout(format!("{what}: worker unresponsive")))?
}

/// True when a process with `pid` exists (Unix: `kill -0` exit status).
/// EPERM targets (a live process owned by another user) read as absent;
/// callers only probe owned children, where EPERM cannot occur.
/// Used to assert teardown reaped the child.
#[cfg(unix)]
#[must_use]
pub fn process_exists(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // No libc: `kill -0` (the workspace forbids `unsafe`).
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Non-Unix stub.
#[cfg(not(unix))]
#[must_use]
pub fn process_exists(_pid: u32) -> bool {
    true
}

// The handle must stay shareable without any unsafe impl (R07).
const _: fn() = || {
    fn share<T: Send + Sync>() {}
    share::<Session>();
    share::<CancelToken>();
};

// ---------------------------------------------------------------------------
// Shared published state (R06, R07)
// ---------------------------------------------------------------------------

struct SharedState {
    latest: Option<Observation>,
    exit: Option<ExitStatus>,
    closed: bool,
    teardown_error: Option<String>,
}

struct Shared {
    state: Mutex<SharedState>,
    changed: Condvar,
}

impl Shared {
    fn new() -> Self {
        Self {
            state: Mutex::new(SharedState {
                latest: None,
                exit: None,
                closed: false,
                teardown_error: None,
            }),
            changed: Condvar::new(),
        }
    }

    fn publish(&self, obs: Observation, exit: Option<ExitStatus>) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if exit.is_some() {
            s.exit = exit;
        }
        s.latest = Some(obs);
        drop(s);
        self.changed.notify_all();
    }

    fn publish_exit(&self, status: ExitStatus, obs: Observation) {
        self.publish(obs, Some(status));
    }

    fn latest(&self) -> Option<Observation> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .latest
            .clone()
    }

    fn revision(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .latest
            .as_ref()
            .map_or(0, |o| o.revision)
    }

    fn exit(&self) -> Option<ExitStatus> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .exit
            .clone()
    }

    fn is_closed(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).closed
    }

    fn mark_closed(&self) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.closed = true;
        drop(s);
        self.changed.notify_all();
    }

    fn record_teardown(&self, msg: &str) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.teardown_error.is_none() {
            s.teardown_error = Some(msg.to_string());
        }
    }

    fn teardown_error(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .teardown_error
            .clone()
    }

    /// Wait (bounded by `deadline`, `cancel`, and [`WAIT_SLICE`]) for any
    /// publication. Returns immediately on cancel/deadline.
    fn wait_changed(&self, deadline: Instant, cancel: &CancelToken) {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if cancel.is_cancelled() || now >= deadline {
            return;
        }
        let slice = (deadline - now).min(WAIT_SLICE);
        let _ = self.changed.wait_timeout(s, slice);
    }

    /// Wait for a revision newer than `seen`; `None` on slice expiry (the
    /// caller re-checks cancel/deadline/quiet).
    fn wait_for_newer_than(
        &self,
        seen: u64,
        deadline: Instant,
        cancel: &CancelToken,
    ) -> Option<Observation> {
        // One slice per call: the caller owns quiet/deadline accounting.
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(o) = s.latest.clone() {
            if o.revision > seen {
                return Some(o);
            }
        }
        if s.closed || cancel.is_cancelled() {
            return None;
        }
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        let slice = (deadline - now).min(WAIT_SLICE);
        s = match self.changed.wait_timeout(s, slice) {
            Ok((guard, _)) => guard,
            Err(e) => e.into_inner().0,
        };
        if let Some(o) = s.latest.clone() {
            if o.revision > seen {
                return Some(o);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Worker ops
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum MouseAction {
    Press(MouseButton),
    // Release carries no button: every encoding reports release as 3.
    Release,
    Move { held: Option<MouseButton> },
    Wheel(Wheel),
}

#[derive(Debug)]
enum Input {
    Bytes(Vec<u8>),
    Paste(String),
    Key {
        key: Key,
        mods: KeyMods,
        kind: KeyEventKind,
    },
    Mouse {
        action: MouseAction,
        x: u16,
        y: u16,
        mods: MouseMods,
    },
    Focus(bool),
}

enum Op {
    Feed(Vec<u8>),
    Eof(Option<String>),
    Observe {
        reply: mpsc::Sender<Result<Observation, TuiError>>,
    },
    Input {
        input: Input,
        reply: mpsc::Sender<Result<(), TuiError>>,
    },
    Resize {
        cols: u16,
        rows: u16,
        reply: mpsc::Sender<Result<(), TuiError>>,
    },
    CloseInput {
        reply: mpsc::Sender<Result<(), TuiError>>,
    },
    Shutdown,
}

// ---------------------------------------------------------------------------
// Worker thread: sole owner of Term, writer, and child
// ---------------------------------------------------------------------------

struct WorkerDims {
    cols: usize,
    rows: usize,
}

impl GridDims for WorkerDims {
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
struct QueueListener {
    tx: mpsc::Sender<Event>,
}

impl EventListener for QueueListener {
    fn send_event(&self, event: Event) {
        let _ = self.tx.send(event);
    }
}

struct WorkerEventState {
    title: Option<String>,
    bells: u64,
}

impl WorkerEventState {
    fn new() -> Self {
        Self {
            title: None,
            bells: 0,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_worker(
    master: Box<dyn MasterPty + Send>,
    mut child: Box<dyn PtyChild + Send + Sync>,
    writer: Box<dyn std::io::Write + Send>,
    term_config: TermConfig,
    cols: u16,
    rows: u16,
    pid: Option<u32>,
    op_rx: mpsc::Receiver<Op>,
    shared: Arc<Shared>,
) {
    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let dims = WorkerDims {
        cols: cols as usize,
        rows: rows as usize,
    };
    let mut term = Term::new(term_config, &dims, QueueListener { tx: event_tx });
    let mut processor: Processor = Processor::new();
    let mut writer: Option<Box<dyn std::io::Write + Send>> = Some(writer);
    let mut events = WorkerEventState::new();
    let mut revision: u64 = 0;
    let mut eof = false;
    let mut exited: Option<ExitStatus> = None;
    let mut exit_seen_at: Option<Instant> = None;
    let mut finalized = false;

    // Revision 0: the Initial observation spawn() blocks on.
    publish_current(
        &mut term,
        &mut events,
        &event_rx,
        writer.as_deref_mut(),
        &shared,
        revision,
        CaptureReason::Initial,
        pid,
        cols,
        rows,
    );

    loop {
        match op_rx.recv_timeout(WORKER_TICK) {
            Ok(Op::Feed(bytes)) => {
                processor.advance(&mut term, &bytes);
                drain_term_events(&mut term, &event_rx, &mut events, writer.as_deref_mut());
                revision += 1;
                let (c, r) = (cols_of(&term), rows_of(&term));
                publish_current(
                    &mut term,
                    &mut events,
                    &event_rx,
                    writer.as_deref_mut(),
                    &shared,
                    revision,
                    CaptureReason::Poll,
                    pid,
                    c,
                    r,
                );
            }
            Ok(Op::Eof(read_err)) => {
                eof = true;
                if let Some(msg) = read_err {
                    // A read error at EOF (e.g. Linux EIO after child death)
                    // is informational; the exit status is authoritative.
                    let _ = msg;
                }
            }
            Ok(Op::Observe { reply }) => {
                drain_term_events(&mut term, &event_rx, &mut events, writer.as_deref_mut());
                let (c, r) = (cols_of(&term), rows_of(&term));
                let obs =
                    build_observation(&term, &events, revision, CaptureReason::Manual, pid, c, r);
                match obs {
                    Ok(obs) => {
                        shared.publish(obs.clone(), None);
                        let _ = reply.send(Ok(obs));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Ok(Op::Input { input, reply }) => {
                let r = apply_input(&mut term, &input, writer.as_deref_mut(), exited.is_some());
                drain_term_events(&mut term, &event_rx, &mut events, writer.as_deref_mut());
                if let Input::Focus(focused) = input {
                    // Focus is emulator state too: record it and publish so
                    // waits can observe the round-trip.
                    term.is_focused = focused;
                    revision += 1;
                    let (c, r) = (cols_of(&term), rows_of(&term));
                    publish_current(
                        &mut term,
                        &mut events,
                        &event_rx,
                        writer.as_deref_mut(),
                        &shared,
                        revision,
                        CaptureReason::Input,
                        pid,
                        c,
                        r,
                    );
                }
                let _ = reply.send(r);
            }
            Ok(Op::Resize { cols, rows, reply }) => {
                let r = apply_resize(master.as_ref(), &mut term, cols, rows);
                if r.is_ok() {
                    revision += 1;
                    publish_current(
                        &mut term,
                        &mut events,
                        &event_rx,
                        writer.as_deref_mut(),
                        &shared,
                        revision,
                        CaptureReason::Resize,
                        pid,
                        cols,
                        rows,
                    );
                }
                let _ = reply.send(r);
            }
            Ok(Op::CloseInput { reply }) => {
                if exited.is_some() {
                    let _ = reply.send(Err(TuiError::ChildExited(
                        "child already exited".to_string(),
                    )));
                } else if writer.take().is_some() {
                    let _ = reply.send(Ok(()));
                } else {
                    let _ = reply.send(Err(TuiError::Closed("stdin already closed".to_string())));
                }
            }
            Ok(Op::Shutdown) => {
                shutdown_child(&mut child, &shared);
                exited = poll_child(&mut child).map(ExitStatus::from).or(exited);
                if !finalized {
                    revision += 1;
                    let status = exited.clone().unwrap_or(ExitStatus {
                        code: 1,
                        signal: Some("unknown".to_string()),
                    });
                    publish_exit(
                        &mut term,
                        &mut events,
                        &event_rx,
                        &shared,
                        revision,
                        pid,
                        status,
                    );
                }
                shared.mark_closed();
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Handle and reader both gone: reap and go away quietly.
                shutdown_child(&mut child, &shared);
                shared.mark_closed();
                return;
            }
        }

        // Exit polling: reap promptly, but give trailing output DRAIN_GRACE
        // after the child dies before publishing the final revision.
        if !finalized {
            if exited.is_none() {
                if let Some(status) = poll_child(&mut child) {
                    exited = Some(ExitStatus::from(status));
                    exit_seen_at = Some(Instant::now());
                }
            }
            let drained = eof || exit_seen_at.is_some_and(|t| t.elapsed() >= DRAIN_GRACE);
            if exited.is_some() && drained {
                finalized = true;
                revision += 1;
                publish_exit(
                    &mut term,
                    &mut events,
                    &event_rx,
                    &shared,
                    revision,
                    pid,
                    exited.clone().unwrap_or(ExitStatus {
                        code: 0,
                        signal: None,
                    }),
                );
            }
        }
    }
}

fn cols_of<T: EventListener>(term: &Term<T>) -> u16 {
    term.columns().min(u16::MAX as usize) as u16
}

fn rows_of<T: EventListener>(term: &Term<T>) -> u16 {
    term.screen_lines().min(u16::MAX as usize) as u16
}

/// Best-effort child poll under the lifecycle guard.
fn poll_child(child: &mut Box<dyn PtyChild + Send + Sync>) -> Option<portable_pty::ExitStatus> {
    let _guard = PTY_LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
    child.try_wait().unwrap_or(None)
}

/// Bounded kill + reap. Each child operation runs under the lifecycle
/// guard; the guard is released while sleeping so teardown never blocks an
/// unrelated session's spawn. Records teardown errors instead of failing.
fn shutdown_child(child: &mut Box<dyn PtyChild + Send + Sync>, shared: &Shared) {
    {
        let _guard = PTY_LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(e) => {
                shared.record_teardown(&format!("child poll during teardown failed: {e}"));
            }
        }
        if let Err(e) = child.kill() {
            shared.record_teardown(&format!("child kill during teardown failed: {e}"));
        }
    }
    let deadline = Instant::now() + KILL_GRACE;
    loop {
        {
            let _guard = PTY_LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {}
                Err(e) => {
                    shared.record_teardown(&format!("child reap during teardown failed: {e}"));
                    return;
                }
            }
        }
        if Instant::now() >= deadline {
            shared.record_teardown("child still alive after kill grace");
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

// ---------------------------------------------------------------------------
// Input encoding (worker side: derived from live TermMode)
// ---------------------------------------------------------------------------

fn apply_input<T: EventListener>(
    term: &mut Term<T>,
    input: &Input,
    writer: Option<&mut (dyn std::io::Write + Send + 'static)>,
    exited: bool,
) -> Result<(), TuiError> {
    if exited {
        return Err(TuiError::ChildExited("child already exited".to_string()));
    }
    let writer = writer.ok_or_else(|| TuiError::Closed("stdin is closed".to_string()))?;
    let mode = *term.mode();
    let bytes: Option<Vec<u8>> = match input {
        Input::Bytes(b) => Some(b.clone()),
        Input::Paste(text) => Some(encode_paste(text, &mode)?),
        Input::Key { key, mods, kind } => encode_key(key, mods, *kind, &mode)?,
        Input::Mouse { action, x, y, mods } => {
            let (cols, rows) = (term.columns(), term.screen_lines());
            Some(encode_mouse(action, *x, *y, *mods, &mode, cols, rows)?)
        }
        Input::Focus(focused) => Some(encode_focus(*focused, &mode)?),
    };
    match bytes {
        Some(b) if !b.is_empty() => writer
            .write_all(&b)
            .map_err(|e| TuiError::Io(format!("pty write failed: {e}"))),
        _ => Ok(()),
    }
}

fn apply_resize<T: EventListener>(
    master: &dyn MasterPty,
    term: &mut Term<T>,
    cols: u16,
    rows: u16,
) -> Result<(), TuiError> {
    // PTY first: if the kernel refuses, the emulator stays consistent.
    master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| TuiError::Io(format!("pty resize failed: {e}")))?;
    term.resize(WorkerDims {
        cols: cols as usize,
        rows: rows as usize,
    });
    Ok(())
}

// -- paste ---------------------------------------------------------------

const PASTE_START: &str = "\x1b[200~";
const PASTE_END: &str = "\x1b[201~";

fn encode_paste(text: &str, mode: &TermMode) -> Result<Vec<u8>, TuiError> {
    if text.contains(PASTE_START) || text.contains(PASTE_END) {
        return Err(TuiError::PasteRejected(
            "content contains bracketed-paste delimiters".to_string(),
        ));
    }
    if mode.contains(TermMode::BRACKETED_PASTE) {
        Ok(format!("{PASTE_START}{text}{PASTE_END}").into_bytes())
    } else {
        Ok(text.as_bytes().to_vec())
    }
}

// -- focus ---------------------------------------------------------------

fn encode_focus(focused: bool, mode: &TermMode) -> Result<Vec<u8>, TuiError> {
    if !mode.contains(TermMode::FOCUS_IN_OUT) {
        return Err(TuiError::ModeNotEnabled(
            "focus tracking (DEC 1004) not enabled by the application",
        ));
    }
    Ok(if focused {
        b"\x1b[I".to_vec()
    } else {
        b"\x1b[O".to_vec()
    })
}

// -- keys ----------------------------------------------------------------

/// xterm modifier parameter: 1 + shift*1 + alt*2 + ctrl*4 (+ super*8 kitty).
fn mods_param(mods: &KeyMods, kitty: bool) -> Result<u8, TuiError> {
    if mods.sup && !kitty {
        return Err(TuiError::Unsupported(
            "super modifier needs the kitty keyboard protocol",
        ));
    }
    Ok(1 + u8::from(mods.shift)
        + 2 * u8::from(mods.alt)
        + 4 * u8::from(mods.ctrl)
        + if kitty { 8 * u8::from(mods.sup) } else { 0 })
}

fn encode_key(
    key: &Key,
    mods: &KeyMods,
    kind: KeyEventKind,
    mode: &TermMode,
) -> Result<Option<Vec<u8>>, TuiError> {
    let kitty = mode.intersects(TermMode::KITTY_KEYBOARD_PROTOCOL);
    if kitty {
        if let Some(bytes) = encode_key_kitty(key, mods, kind)? {
            return Ok(Some(bytes));
        }
        // Kitty cannot express this key (functional table deferred to the
        // shell/paste agent): fall through to legacy encoding.
    }
    if kind == KeyEventKind::Up {
        // Legacy encodings cannot represent releases: successful no-op.
        return Ok(None);
    }
    Ok(Some(encode_key_legacy(key, mods, mode)?))
}

/// Kitty `CSI u` encoding for text keys (`Enter`/`Tab`/`Backspace`/`Escape`
/// by codepoint). Returns `None` for functional keys, which keep legacy
/// encoding until the full kitty functional table lands.
fn encode_key_kitty(
    key: &Key,
    mods: &KeyMods,
    kind: KeyEventKind,
) -> Result<Option<Vec<u8>>, TuiError> {
    let codepoint: u32 = match key {
        Key::Char(c) => (*c).into(),
        Key::Enter => 13,
        Key::Tab => 9,
        Key::Backspace => 127,
        Key::Escape => 27,
        _ => return Ok(None),
    };
    let m = mods_param(mods, true)?;
    let event = match kind {
        KeyEventKind::Press => String::new(),
        KeyEventKind::Down => ":1".to_string(),
        KeyEventKind::Repeat => ":2".to_string(),
        KeyEventKind::Up => ":3".to_string(),
    };
    Ok(Some(format!("\x1b[{codepoint};{m}u{event}").into_bytes()))
}

fn encode_key_legacy(key: &Key, mods: &KeyMods, mode: &TermMode) -> Result<Vec<u8>, TuiError> {
    // Alt-only chords prefix ESC; richer modifier mixes use CSI params or
    // CSI-u, which cannot combine with a bare ESC prefix.
    let alt_only = mods.alt && !mods.ctrl && !mods.shift && !mods.sup;
    if mods.sup {
        return Err(TuiError::Unsupported(
            "super modifier needs the kitty keyboard protocol",
        ));
    }
    let app_cursor = mode.contains(TermMode::APP_CURSOR);

    // CSI-u fallback for control keys with modifiers (modifyOtherKeys style).
    let csi_u = |code: u32| -> Result<Vec<u8>, TuiError> {
        Ok(format!("\x1b[{code};{}u", mods_param(mods, false)?).into_bytes())
    };

    match key {
        Key::Char(c) => {
            if mods.ctrl && !mods.alt {
                return Ok(vec![ctrl_byte(*c)?]);
            }
            if mods.ctrl {
                // Ctrl+Alt and Ctrl+Shift mixes have no legacy form.
                return Err(TuiError::Unsupported(
                    "ctrl+alt/shift character chords need the kitty keyboard protocol",
                ));
            }
            if mods.alt && !alt_only {
                return Err(TuiError::Unsupported(
                    "alt+shift character chords need the kitty keyboard protocol",
                ));
            }
            // Shift on a character is the caller's case choice; no bytes.
            let mut text = String::new();
            if alt_only {
                text.push('\x1b');
            }
            text.push(*c);
            Ok(text.into_bytes())
        }
        Key::Enter => {
            if mods.is_empty() {
                Ok(vec![b'\r'])
            } else if alt_only {
                Ok(vec![0x1b, b'\r'])
            } else {
                csi_u(13)
            }
        }
        Key::Tab => {
            if mods.is_empty() {
                Ok(vec![b'\t'])
            } else if *mods == KeyMods::SHIFT {
                Ok(b"\x1b[Z".to_vec())
            } else if alt_only {
                Ok(vec![0x1b, b'\t'])
            } else {
                csi_u(9)
            }
        }
        Key::Backspace => {
            if mods.is_empty() {
                Ok(vec![0x7f])
            } else if alt_only {
                Ok(vec![0x1b, 0x7f])
            } else {
                csi_u(127)
            }
        }
        Key::Escape => {
            if mods.is_empty() {
                Ok(vec![0x1b])
            } else {
                csi_u(27)
            }
        }
        Key::Up | Key::Down | Key::Right | Key::Left => {
            let letter = match key {
                Key::Up => 'A',
                Key::Down => 'B',
                Key::Right => 'C',
                _ => 'D',
            };
            if mods.is_empty() {
                return Ok(if app_cursor {
                    format!("\x1bO{letter}").into_bytes()
                } else {
                    format!("\x1b[{letter}").into_bytes()
                });
            }
            if alt_only {
                let base = if app_cursor {
                    format!("\x1bO{letter}")
                } else {
                    format!("\x1b[{letter}")
                };
                return Ok(format!("\x1b{base}").into_bytes());
            }
            Ok(format!("\x1b[1;{}{letter}", mods_param(mods, false)?).into_bytes())
        }
        Key::Home | Key::End => {
            let letter = if matches!(key, Key::Home) { 'H' } else { 'F' };
            if mods.is_empty() {
                return Ok(format!("\x1b[{letter}").into_bytes());
            }
            if alt_only {
                return Ok(format!("\x1b\x1b[{letter}").into_bytes());
            }
            Ok(format!("\x1b[1;{}{letter}", mods_param(mods, false)?).into_bytes())
        }
        Key::Insert | Key::Delete | Key::PageUp | Key::PageDown => {
            let n = match key {
                Key::Insert => 2,
                Key::Delete => 3,
                Key::PageUp => 5,
                _ => 6,
            };
            if mods.is_empty() {
                return Ok(format!("\x1b[{n}~").into_bytes());
            }
            if alt_only {
                return Ok(format!("\x1b\x1b[{n}~").into_bytes());
            }
            Ok(format!("\x1b[{n};{}~", mods_param(mods, false)?).into_bytes())
        }
        Key::F(n) => {
            debug_assert!((1..=12).contains(n));
            if *n <= 4 {
                let letter = ['P', 'Q', 'R', 'S'][(n - 1) as usize];
                if mods.is_empty() {
                    return Ok(format!("\x1bO{letter}").into_bytes());
                }
                if alt_only {
                    return Ok(format!("\x1b\x1bO{letter}").into_bytes());
                }
                return Ok(format!("\x1b[1;{}{letter}", mods_param(mods, false)?).into_bytes());
            }
            let tilde = [15, 17, 18, 19, 20, 21, 23, 24][(n - 5) as usize];
            if mods.is_empty() {
                return Ok(format!("\x1b[{tilde}~").into_bytes());
            }
            if alt_only {
                return Ok(format!("\x1b\x1b[{tilde}~").into_bytes());
            }
            Ok(format!("\x1b[{tilde};{}~", mods_param(mods, false)?).into_bytes())
        }
    }
}

fn ctrl_byte(c: char) -> Result<u8, TuiError> {
    if c == ' ' {
        return Ok(0);
    }
    let upper = c.to_ascii_uppercase();
    if upper.is_ascii_alphabetic() {
        return Ok((upper as u8) & 0x1f);
    }
    if matches!(c, '@' | '[' | '\\' | ']' | '^' | '_' | '?') {
        return Ok((c as u8) & 0x1f);
    }
    Err(TuiError::Unsupported(
        "this ctrl+character chord needs the kitty keyboard protocol",
    ))
}

// -- mouse ---------------------------------------------------------------

fn button_code(b: MouseButton) -> u8 {
    match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

fn encode_mouse(
    action: &MouseAction,
    x: u16,
    y: u16,
    mods: MouseMods,
    mode: &TermMode,
    cols: usize,
    rows: usize,
) -> Result<Vec<u8>, TuiError> {
    if x as usize >= cols || y as usize >= rows {
        return Err(TuiError::InvalidInput(format!(
            "mouse ({x},{y}) outside {cols}x{rows} grid"
        )));
    }
    let any = TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION;
    let required: TermMode = match action {
        MouseAction::Press(_) | MouseAction::Release | MouseAction::Wheel(_) => any,
        MouseAction::Move { held: None } => TermMode::MOUSE_MOTION,
        MouseAction::Move { held: Some(_) } => TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION,
    };
    let what = match action {
        MouseAction::Press(_) | MouseAction::Release => {
            "mouse reporting (DEC 1000/1002/1003) not enabled by the application"
        }
        MouseAction::Move { held: None } => {
            "mouse motion reporting (DEC 1003) not enabled by the application"
        }
        MouseAction::Move { held: Some(_) } => {
            "mouse drag reporting (DEC 1002/1003) not enabled by the application"
        }
        MouseAction::Wheel(_) => {
            "mouse reporting (DEC 1000/1002/1003) not enabled by the application"
        }
    };
    if !mode.intersects(required) {
        return Err(TuiError::ModeNotEnabled(what));
    }

    let mut cb: u32 = match action {
        MouseAction::Press(b) => u32::from(button_code(*b)),
        MouseAction::Release => 3,
        MouseAction::Move { held: None } => 3 + 32,
        MouseAction::Move { held: Some(b) } => u32::from(button_code(*b)) + 32,
        MouseAction::Wheel(Wheel::Up) => 64,
        MouseAction::Wheel(Wheel::Down) => 65,
        MouseAction::Wheel(Wheel::Left) => 66,
        MouseAction::Wheel(Wheel::Right) => 67,
    };
    if mods.shift {
        cb += 4;
    }
    if mods.alt {
        cb += 8;
    }
    if mods.ctrl {
        cb += 16;
    }
    let cx = u32::from(x) + 1;
    let cy = u32::from(y) + 1;

    if mode.contains(TermMode::SGR_MOUSE) {
        let marker = if matches!(action, MouseAction::Release) {
            'm'
        } else {
            'M'
        };
        return Ok(format!("\x1b[<{cb};{cx};{cy}{marker}").into_bytes());
    }
    if mode.contains(TermMode::UTF8_MOUSE) {
        let mut out = b"\x1b[M".to_vec();
        for v in [cb + 32, cx + 32, cy + 32] {
            let ch = char::from_u32(v).ok_or_else(|| {
                TuiError::InvalidInput(format!("mouse coordinate {v} unencodable"))
            })?;
            let mut tmp = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
        }
        return Ok(out);
    }
    // Legacy X10: single bytes, coordinates must fit.
    for (v, name) in [(cb + 32, "button"), (cx + 32, "x"), (cy + 32, "y")] {
        if v > 255 {
            return Err(TuiError::InvalidInput(format!(
                "mouse {name} {v} exceeds legacy X10 encoding"
            )));
        }
    }
    Ok(vec![
        0x1b,
        b'[',
        b'M',
        (cb + 32) as u8,
        (cx + 32) as u8,
        (cy + 32) as u8,
    ])
}

// ---------------------------------------------------------------------------
// Reader thread: blocking PTY reads forwarded as ops
// ---------------------------------------------------------------------------

fn run_reader(mut reader: Box<dyn std::io::Read + Send>, tx: mpsc::Sender<Op>) {
    let mut buf = vec![0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                let _ = tx.send(Op::Eof(None));
                return;
            }
            Ok(n) => {
                if tx.send(Op::Feed(buf[..n].to_vec())).is_err() {
                    return;
                }
            }
            Err(e) => {
                let _ = tx.send(Op::Eof(Some(e.to_string())));
                return;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Terminal events: query replies go back to the PTY; title/bells recorded
// ---------------------------------------------------------------------------

fn drain_term_events<T: EventListener>(
    term: &mut Term<T>,
    event_rx: &mpsc::Receiver<Event>,
    events: &mut WorkerEventState,
    mut writer: Option<&mut (dyn std::io::Write + Send + 'static)>,
) {
    while let Ok(event) = event_rx.try_recv() {
        match event {
            Event::Title(t) => events.title = Some(t),
            Event::ResetTitle => events.title = None,
            Event::Bell => events.bells += 1,
            Event::PtyWrite(text) => {
                if let Some(w) = writer.as_deref_mut() {
                    let _ = w.write_all(text.as_bytes());
                }
            }
            Event::ClipboardLoad(_, respond) => {
                // The harness holds no clipboard: answer honestly empty.
                if let Some(w) = writer.as_deref_mut() {
                    let _ = w.write_all(respond("").as_bytes());
                }
            }
            Event::ColorRequest(index, respond) => {
                let rgb = resolve_color(term, index);
                if let Some(w) = writer.as_deref_mut() {
                    let _ = w.write_all(respond(rgb).as_bytes());
                }
            }
            Event::TextAreaSizeRequest(respond) => {
                // Headless: grid geometry is exact; cell pixels are nominal.
                let size = WindowSize {
                    num_lines: rows_of(term),
                    num_cols: cols_of(term),
                    cell_width: 8,
                    cell_height: 16,
                };
                if let Some(w) = writer.as_deref_mut() {
                    let _ = w.write_all(respond(size).as_bytes());
                }
            }
            Event::ClipboardStore(_, _)
            | Event::MouseCursorDirty
            | Event::Wakeup
            | Event::Exit
            | Event::ChildExit(_)
            | Event::CursorBlinkingChange => {}
        }
    }
}

/// Resolve a palette slot for `OSC 4;n;?` replies: live override, else the
/// xterm default table (documented nominal for fg/bg/cursor).
fn resolve_color<T: EventListener>(term: &Term<T>, index: usize) -> VteRgb {
    if let Some(rgb) = term.colors()[index.min(268)] {
        return rgb;
    }
    let def = |n: u8| {
        let c = Rgb::from_indexed(n);
        VteRgb {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    };
    match index {
        0..=255 => def(index as u8),
        256 | 258 => def(7),
        _ => def(0),
    }
}

// ---------------------------------------------------------------------------
// Atomic observation builder (R06)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn publish_current<T: EventListener>(
    term: &mut Term<T>,
    events: &mut WorkerEventState,
    event_rx: &mpsc::Receiver<Event>,
    writer: Option<&mut (dyn std::io::Write + Send + 'static)>,
    shared: &Shared,
    revision: u64,
    reason: CaptureReason,
    pid: Option<u32>,
    cols: u16,
    rows: u16,
) {
    drain_term_events(term, event_rx, events, writer);
    match build_observation(term, events, revision, reason, pid, cols, rows) {
        Ok(obs) => shared.publish(obs, None),
        Err(e) => shared.record_teardown(&format!("observation build failed: {e}")),
    }
}

fn publish_exit<T: EventListener>(
    term: &mut Term<T>,
    events: &mut WorkerEventState,
    event_rx: &mpsc::Receiver<Event>,
    shared: &Shared,
    revision: u64,
    pid: Option<u32>,
    status: ExitStatus,
) {
    // Drain without the writer: replies have nowhere to go, but title and
    // bell state still belong in the final observation.
    while let Ok(event) = event_rx.try_recv() {
        match event {
            Event::Title(t) => events.title = Some(t),
            Event::ResetTitle => events.title = None,
            Event::Bell => events.bells += 1,
            _ => {}
        }
    }
    let cols = cols_of(term);
    let rows = rows_of(term);
    match build_observation(term, events, revision, CaptureReason::Exit, pid, cols, rows) {
        Ok(obs) => shared.publish_exit(status, obs),
        Err(e) => shared.record_teardown(&format!("exit observation build failed: {e}")),
    }
}

/// Build one atomic observation: grid + cursor + palette + modes at the
/// worker's current state. Runs only on the worker thread.
fn build_observation<T: EventListener>(
    term: &Term<T>,
    events: &WorkerEventState,
    revision: u64,
    reason: CaptureReason,
    pid: Option<u32>,
    cols: u16,
    rows: u16,
) -> Result<Observation, TuiError> {
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
                cells.push(frame_cell(x, y, cell, 2, false));
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
            } else if flags.contains(CellFlags::WIDE_CHAR_SPACER)
                && x > 0
                && cells.last().is_some_and(|lead: &Cell| {
                    lead.width == 2 && !lead.continuation && lead.y == y && lead.x + 1 == x
                })
            {
                // Spacer already emitted with its lead above; unreachable in
                // practice, kept as the documented pairing rule.
                cells.push(Cell {
                    x,
                    y,
                    symbol: String::new(),
                    width: 0,
                    continuation: true,
                    fg: Color::Default,
                    bg: Color::Default,
                    mods: Mods::default(),
                    underline_color: Color::Default,
                });
                x += 1;
            } else if flags
                .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER)
            {
                // Orphan spacer (wrapped lead lives on another row): render
                // as a blank so the grid stays valid.
                cells.push(Cell::blank(x, y));
                x += 1;
            } else {
                // Narrow cell, or a wide lead with no in-row follower
                // (clamped to width 1: validity over width fidelity).
                cells.push(frame_cell(x, y, cell, 1, false));
                x += 1;
            }
        }
    }

    let point = grid.cursor.point;
    let cursor = frame_cursor(term, point.line.0, point.column.0, cols, rows);
    let screen = Screen::validate(cols, rows, 0, 0, cells, cursor)
        .map_err(|e| TuiError::Teardown(format!("built an invalid screen: {e}")))?;

    let mut modes = Vec::new();
    push_modes(term.mode(), &mut modes);
    let palette: Vec<(u8, Rgb)> = (0..256u16)
        .filter_map(|i| {
            term.colors()[i as usize].map(|c| {
                (
                    i as u8,
                    Rgb {
                        r: c.r,
                        g: c.g,
                        b: c.b,
                    },
                )
            })
        })
        .collect();
    let state = TermState {
        modes: Maybe::Known(modes),
        palette: Maybe::Known(palette),
        title: match &events.title {
            Some(t) => Maybe::Known(t.clone()),
            None => Maybe::Unknown,
        },
        bells: Maybe::Known(events.bells),
    };
    let provenance = CaptureProvenance::new(now_ms(), pid, None, 0);
    Ok(Observation::new(
        screen, revision, reason, state, provenance,
    ))
}

fn frame_cell(
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
        fg: frame_color(cell.fg),
        bg: frame_color(cell.bg),
        mods: Mods {
            // Backend gap, honest: alacritty drops per-cell blink.
            hidden: flags.contains(CellFlags::HIDDEN),
            blink: false,
            bold: flags.contains(CellFlags::BOLD),
            dim: flags.contains(CellFlags::DIM),
            italic: flags.contains(CellFlags::ITALIC),
            underline: flags.intersects(CellFlags::ALL_UNDERLINES),
            underline_style: frame_underline_style(flags),
            strikethrough: flags.contains(CellFlags::STRIKEOUT),
            reverse: flags.contains(CellFlags::INVERSE),
        },
        underline_color: cell
            .underline_color()
            .map(frame_color)
            .unwrap_or(Color::Default),
    }
}

/// Map alacritty underline flags (set from SGR 4 / 4:0..4:5 / 24) to the
/// canonical style. The emulator holds at most one underline flag per cell
/// (each SGR 4:x clears the rest); the order below is defensive only.
fn frame_underline_style(flags: CellFlags) -> UnderlineStyle {
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

fn frame_color(c: VteColor) -> Color {
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

fn frame_cursor<T: EventListener>(
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

/// Map live `TermMode` bits to DEC/private mode numbers.
fn push_modes(mode: &TermMode, out: &mut Vec<u16>) {
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
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_one_returns_for_clean_thread() {
        let shared = Shared::new();
        let h = std::thread::spawn(|| {});
        join_one(h, &shared, "worker", Duration::from_secs(5));
        assert_eq!(shared.teardown_error(), None);
    }

    #[test]
    fn join_one_records_panic() {
        let shared = Shared::new();
        let h = std::thread::spawn(|| panic!("boom"));
        join_one(h, &shared, "reader", Duration::from_secs(5));
        assert_eq!(
            shared.teardown_error().as_deref(),
            Some("reader thread panicked")
        );
    }

    /// F5: a thread stuck forever (kill-failure stand-in for a reader
    /// blocked in `read()`) must not hang teardown: bounded wait, then
    /// detach with a diagnostic.
    #[test]
    fn join_one_detaches_stuck_thread() {
        let shared = Shared::new();
        let h = std::thread::Builder::new()
            .name("stuck-stand-in".to_string())
            .spawn(std::thread::park)
            .unwrap();
        let start = Instant::now();
        join_one(h, &shared, "reader", Duration::from_millis(50));
        assert!(start.elapsed() < Duration::from_secs(5), "join hung");
        let err = shared.teardown_error().expect("diagnostic recorded");
        assert!(err.contains("did not exit"), "{err}");
        assert!(err.contains("detached"), "{err}");
    }
}
