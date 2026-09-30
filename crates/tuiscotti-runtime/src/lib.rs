//! tuiscotti-runtime: execution backends and artifact stores.
//!
//! The owned PTY session runtime (`tui`, feature `pty`) with typed key
//! chords (`keys`), Duration-based waits (`waits`), session-bound
//! locators (`bound_locator`), and shell/state helpers (`tui_shell`);
//! piped child processes ([`command`]); the runner-neutral context
//! ([`runner`]); live observation ([`observe`]); the typed op protocol plus
//! named sessions ([`proto`]); the MCP stdio bridge ([`mcp`]); and the
//! approved stores ([`snapshot`], [`grouped`]) with read-only compat
//! importers ([`import_compat`]).

pub mod command;
pub mod grouped;
pub mod import_compat;
pub mod mcp;
pub mod observe;
pub mod proto;
pub mod runner;
pub mod snapshot;

#[cfg(feature = "pty")]
pub mod bound_locator;
#[cfg(feature = "pty")]
pub mod keys;
#[cfg(feature = "pty")]
pub mod tui;
#[cfg(feature = "pty")]
pub mod tui_shell;
#[cfg(feature = "pty")]
pub mod waits;

pub use grouped::{ArtifactPaths, GroupedOutcome, GroupedStore, InvalidName};
#[cfg(feature = "pty")]
pub use keys::KeyChord;
