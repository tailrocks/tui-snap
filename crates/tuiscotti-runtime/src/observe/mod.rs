//! Live observation + replay-vs-rerun (backlog A03, A05).
//!
//! - **A03 [`Watcher`]**: subscribes to a live [`Session`](crate::tui::Session)'s
//!   [`Observation`](tuiscotti_core::screen::Observation) stream over a bounded channel
//!   (latest-wins + dropped counter, never blocks the session) and injects
//!   input through the same owned session — no tmux/multiplexer. The watcher
//!   sees exactly what assertions see: the same `Observation` type.
//! - **A05 [`Replay`] vs [`Rerun`]**: [`Replay`] feeds recorded output bytes to
//!   a fresh emulator deterministically (no process spawn, by construction);
//!   [`Rerun`] re-spawns the recorded command (documented nondeterministic).
//!   [`compare_replay_vs_rerun`] reports same/different with revision maps.
//!
//! The CLI `session attach` (see `src/main.rs`) is a best-effort human view:
//! it tails the named session's log as text frames. Assertions remain on
//! `Observation`s, never on attach output.

mod pty;
mod text;

pub use pty::*;
pub use text::*;
