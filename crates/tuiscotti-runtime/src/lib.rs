//! tuiscotti-runtime: execution backends and artifact stores.
//!
//! The owned PTY session runtime ([`tui`], [`tui_shell`], feature `pty`),
//! piped child processes ([`command`]), the runner-neutral context
//! ([`runner`]), live observation ([`observe`]), the typed op protocol plus
//! named sessions ([`proto`]), the MCP stdio bridge ([`mcp`]), and the
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
pub mod tui;
#[cfg(feature = "pty")]
pub mod tui_shell;

pub use grouped::{ArtifactPaths, GroupedOutcome, GroupedStore, InvalidName};
