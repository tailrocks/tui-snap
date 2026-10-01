//! Canonical frame schema (v3): the single artifact both capture paths share.
//!
//! ```text
//! fixture model + view state + viewport + theme ──▶ production Ratatui view ──▶ Frame
//! real executable ──▶ PTY + terminal-state engine ──▶ Frame
//! ```
//!
//! A [`Frame`] preserves grapheme content, cell positions and widths
//! (including wide-cell continuations), default/indexed/RGB colors, the
//! supported modifier set, cursor state, and provenance. It deliberately does
//! NOT preserve terminal-protocol details that do not affect the visible
//! grid (hyperlink targets, kitty image payloads, blink phase): those belong
//! in additional assertions, not in a screenshot contract.
//!
//! Import is strict: [`Frame::validate`] rejects malformed frames with an
//! explicit error instead of guessing.

mod cell;
mod color;
mod error;
mod model;
mod mods;
mod provenance;
mod validate;

pub use cell::{Cell, Cursor, CursorStyle};
pub use color::{Color, Rgb};
pub use error::FrameError;
pub use model::{FRAME_VERSION, Frame, MAX_DIM};
pub use mods::{Mods, UnderlineStyle};
pub use provenance::Provenance;
