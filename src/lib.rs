//! tuisnap: Rust TUI visual-regression toolkit.
//!
//! Two capture paths share one canonical [`Frame`]:
//! - **Pure view tests** ([`ratatui`]): fixture model + view state +
//!   viewport + theme → the actual production Ratatui view → frame. No
//!   business logic, network, database, or PTY.
//! - **Interactive tests** ([`pty`], feature `pty`): the real executable in a
//!   real PTY (termlens engine), keyboard/mouse/resize, readiness waits that
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

pub mod assert;
pub mod command;
pub mod diff;
pub mod frame;
pub mod grouped;
pub mod insta_proto;
pub mod profile;
pub mod ratatui;
pub mod render;
pub mod runner;
pub mod screen;
pub mod snapshot;

#[cfg(feature = "pty")]
pub mod ansi;
#[cfg(feature = "pty")]
pub mod pty;

/// The pinned PTY engine, re-exported for callers constructing screens for
/// [`pty::frame_from_screen`]. Git/path consumers need no Cargo patches.
#[cfg(feature = "pty")]
pub use termlens;

pub use frame::{Cell, Color, Cursor, CursorStyle, Frame, FrameError, Mods, Provenance, Rgb};
pub use grouped::{ArtifactPaths, GroupedCheckOptions, GroupedOutcome, GroupedStore, InvalidName};
pub use profile::{
    FallbackFace, FontFaces, Profile, VENDORED_CJK_FONT, VENDORED_CJK_FONT_SHA256, VENDORED_FACES,
    VENDORED_FALLBACK_FACES, VENDORED_FONT, VENDORED_FONT_BOLD, VENDORED_FONT_BOLD_ITALIC,
    VENDORED_FONT_ITALIC, VENDORED_SYMBOLS2_FONT, VENDORED_SYMBOLS2_FONT_SHA256,
    VENDORED_SYMBOLS_FONT, VENDORED_SYMBOLS_FONT_SHA256,
};
pub use render::Renderer;
pub use screen::{
    CaptureProvenance, CaptureReason, Maybe, Observation, Region, RegionPolicy, Screen,
    ScreenError, TermState,
};
