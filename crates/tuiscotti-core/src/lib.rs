//! tuiscotti-core: pure models, queries, and the Ratatui view adapter.
//!
//! No rendering, no PTY, no filesystem beyond parsing: canonical [`Frame`]
//! data ([`frame`]), the validated screen/observation model ([`screen`]),
//! Playwright-style queries ([`locate`]), semantic providers ([`semant`]),
//! scenario-name validation ([`names`]), and the production-view adapter
//! ([`ratatui`]).

pub mod frame;
pub mod locate;
pub mod names;
pub mod ratatui;
pub mod screen;
pub mod semant;

pub use frame::{
    Cell, Color, Cursor, CursorStyle, Frame, FrameError, Mods, Provenance, Rgb, UnderlineStyle,
};
pub use screen::{
    CaptureProvenance, CaptureReason, Maybe, Observation, Region, RegionPolicy, Screen,
    ScreenError, TermState,
};
