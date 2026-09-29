//! tuiscotti: Rust TUI visual-regression toolkit (facade).
//!
//! Two capture paths share one canonical screen:
//! - **Pure view tests** ([`ratatui::render`]): production Ratatui view +
//!   viewport → [`Screen`]. No PTY, no subprocess.
//! - **Interactive tests** ([`Tui`], feature `pty`): the real executable in a
//!   real PTY — [`Session`] drives input, locators, waits, [`Screen`] capture.
//! - **Piped tests** ([`Command`]): piped stdio runs → truthful
//!   [`ProcessOutput`] (exit/signal/timeout/output-limit stay distinct).
//!
//! All three assert through [`assert_snapshot!`] (canonical state) and
//! [`assert_screenshot!`] (canonical + PNG as one sample) with native Insta
//! review, caller-fixed metadata, and evidence before failure.
//!
//! Ordinary tests use only the top-level names. Advanced primitives live in
//! the purposeful modules below (`tui`, `command`, `locate`, `proto`, ...),
//! which also preserve every pre-G6 path used out of crate.

pub mod error;

pub use error::{Error, Result};

// Daily facade: launch, capture, query, assert.
pub use tuiscotti_core::locate::Locator;
pub use tuiscotti_core::screen::{CaptureProvenance, CaptureReason, Maybe, Observation, Region};
pub use tuiscotti_core::screen::{RegionPolicy, Screen, ScreenError, TermState};
pub use tuiscotti_insta::assert::{FrozenError, Policy};
pub use tuiscotti_insta::{assert_screenshot, assert_snapshot};
pub use tuiscotti_render::profile::Profile;
pub use tuiscotti_render::render::Renderer;
#[cfg(feature = "pty")]
pub use tuiscotti_runtime::bound_locator::{ActionError, BoundLocator};
pub use tuiscotti_runtime::command::{
    Command, ProcessOutput, SpawnError, SpawnErrorKind, Termination,
};
#[cfg(feature = "pty")]
pub use tuiscotti_runtime::keys::KeyChord;
#[cfg(feature = "pty")]
pub use tuiscotti_runtime::tui::{
    CancelToken, ExitStatus, ExitWait, Key, KeyEventKind, KeyMods, MouseButton, MouseMods, Session,
    Signal, Tui, TuiError, WaitError,
};

// Purposeful advanced modules (also the pre-G6 compatibility surface).
pub use tuiscotti_core::{frame, locate, names, ratatui, screen, semant};
pub use tuiscotti_insta::{assert, insta_proto};
pub use tuiscotti_render::{diff, export, profile, render};
#[cfg(feature = "pty")]
pub use tuiscotti_runtime::{bound_locator, keys, tui, tui_shell, waits};
pub use tuiscotti_runtime::{
    command, grouped, import_compat, mcp, observe, proto, runner, snapshot,
};

// Pre-G6 top-level compatibility re-exports (kept: used out of crate).
pub use tuiscotti_core::frame::{
    Cell, Color, Cursor, CursorStyle, Frame, FrameError, Mods, Provenance, Rgb, UnderlineStyle,
};
pub use tuiscotti_render::profile::{
    FallbackFace, FontFaces, VENDORED_CJK_FONT, VENDORED_CJK_FONT_SHA256, VENDORED_FACES,
    VENDORED_FALLBACK_FACES, VENDORED_FONT, VENDORED_FONT_BOLD, VENDORED_FONT_BOLD_ITALIC,
    VENDORED_FONT_ITALIC, VENDORED_SYMBOLS_FONT, VENDORED_SYMBOLS_FONT_SHA256,
    VENDORED_SYMBOLS2_FONT, VENDORED_SYMBOLS2_FONT_SHA256,
};
pub use tuiscotti_runtime::grouped::{ArtifactPaths, GroupedOutcome, GroupedStore, InvalidName};
