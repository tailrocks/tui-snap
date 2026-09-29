//! Shell sessions, terminal-state assertions, raw replay, and the scoped
//! guardian (backlog R09, R12, R13, R14).
//!
//! This module only *uses* [`Session`](crate::tui::Session); it never reaches
//! into its worker. Everything unobservable through
//! [`Observation`](tuiscotti_core::screen::Observation) is reported as
//! [`Maybe::Unknown`](tuiscotti_core::screen::Maybe)/`Unsupported` and fails closed.
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
//!   both live [`Observation`](tuiscotti_core::screen::Observation) (via
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

mod state;
mod replay_api;
mod replay_screen;
mod replay_state;
mod shell;
mod guardian;
mod unix;

pub use state::*;
pub use replay_api::*;
pub use replay_screen::*;
pub use replay_state::*;
pub use shell::*;
pub use guardian::*;
pub use unix::*;
