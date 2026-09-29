//! tuiscotti: Rust TUI visual-regression toolkit (facade).
//!
//! Two capture paths share one canonical [`Frame`]:
//! - **Pure view tests** ([`ratatui`]): fixture model + view state +
//!   viewport + theme → the actual production Ratatui view → frame. No
//!   business logic, network, database, or PTY.
//! - **Interactive tests** ([`tui`], feature `pty`): the real executable in a
//!   real PTY (owned runtime), keyboard/mouse/resize, readiness waits that
//!   fail on timeout → frame.
//!
//! Both produce full approved frames + readable PNGs and portable HTML
//! expected/actual/diff reports ([`snapshot`]). A changed snapshot requires
//! explicit review ([`snapshot::Store::accept`]); CI never auto-blesses.
//! Equality only validates the fixtures covered — not every app state.
//!
//! Suites that prefer nested scenario names and committed text/HTML
//! artifacts can use [`grouped::GroupedStore`]: four artifacts per scenario
//! (`.ansi` / `.txt` / `.png` / `.html`), recursive accept and report, same
//! statuses and renderer.

pub use tuiscotti_core::{frame, locate, names, ratatui, screen, semant};
pub use tuiscotti_insta::{assert, assert_screenshot, assert_snapshot, insta_proto};
pub use tuiscotti_render::{diff, export, profile, render};
pub use tuiscotti_runtime::{
    command, grouped, import_compat, mcp, observe, proto, runner, snapshot,
};

#[cfg(feature = "pty")]
pub use tuiscotti_runtime::{tui, tui_shell};

pub use tuiscotti_core::frame::{
    Cell, Color, Cursor, CursorStyle, Frame, FrameError, Mods, Provenance, Rgb, UnderlineStyle,
};
pub use tuiscotti_core::screen::{
    CaptureProvenance, CaptureReason, Maybe, Observation, Region, RegionPolicy, Screen,
    ScreenError, TermState,
};
pub use tuiscotti_render::profile::{
    FallbackFace, FontFaces, Profile, VENDORED_CJK_FONT, VENDORED_CJK_FONT_SHA256, VENDORED_FACES,
    VENDORED_FALLBACK_FACES, VENDORED_FONT, VENDORED_FONT_BOLD, VENDORED_FONT_BOLD_ITALIC,
    VENDORED_FONT_ITALIC, VENDORED_SYMBOLS2_FONT, VENDORED_SYMBOLS2_FONT_SHA256,
    VENDORED_SYMBOLS_FONT, VENDORED_SYMBOLS_FONT_SHA256,
};
pub use tuiscotti_render::render::Renderer;
pub use tuiscotti_runtime::grouped::{ArtifactPaths, GroupedOutcome, GroupedStore, InvalidName};
