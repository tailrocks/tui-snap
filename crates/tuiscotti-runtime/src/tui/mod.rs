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

mod builder;
mod capture;
mod encode;
mod encode_key;
mod error;
mod exit;
mod frame;
mod input_types;
mod limits;
mod profile;
mod session;
mod session_input;
mod session_teardown;
mod shared;
#[cfg(test)]
mod tests;
mod worker;
mod worker_ctx;

pub use builder::Tui;
pub use error::{CancelToken, TuiError, WaitError};
pub use exit::{ExitStatus, ExitWait, process_exists};
pub use input_types::{
    Key, KeyEventKind, KeyExtMods, KeyMods, MouseButton, MouseMods, Signal, Wheel, parse_chord,
};
pub use limits::{DEFAULT_STABLE_QUIET, MAX_COLS, MAX_ROWS, MIN_COLS, MIN_ROWS};
pub use profile::{MouseProfile, TerminalProfile, TrackedModes};
pub use session::Session;
