//! Typed op protocol (A01) + named sessions (A02) + trace journal (A04).
//!
//! One JSON-serializable [`Op`]/[`OpResult`]/[`OpError`] vocabulary shared by
//! the Rust library entry ([`execute`]) and the CLI machine mode (`tuisnap
//! --machine`: JSON lines in on stdin, JSON envelopes out on stdout).
//!
//! PTY-backed ops (`spawn`, `stdin`, `observe`, `snapshot`, `screenshot`,
//! `wait`, `exit`) run against an in-process session registry and require the
//! `pty` feature; without it they fail with code `unsupported`. Everything
//! else (version, capabilities, assert, render, diff, named sessions, record,
//! review, report) is feature-independent.

mod types_a;
mod types_b;
mod execute;
mod registry;
mod sessions;
mod session_ops;
mod journal;

pub use types_a::*;
pub use types_b::*;
pub use execute::*;
pub use registry::*;
pub use sessions::*;
pub use session_ops::*;
pub use journal::*;
