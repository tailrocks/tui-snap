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

use tuiscotti_core::frame::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, UnderlineStyle};
use tuiscotti_core::screen::{Maybe, Observation, Screen};
use crate::tui::{CancelToken, ExitWait, Session, Tui, TuiError, WaitError};
use super::*;


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
            // Prompt-silence: an interactive shell prints PS1 before reading
            // each line. Under load that prompt can land on an attestation
            // row (observed: `# __TUISNAP_SETUP_OK__`), breaking the exact
            // protocol match. Empty prompts remove the interleaving
            // structurally; boundaries stay strict.
            .env("PS1", "")
            .env("PS2", "")
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
