//! M1 validated screen/observation model (backlog M01, M02, M04, M07-regions, M08).
//!
//! - [`Screen`]: immutable validated grid with dimensions AND origin.
//! - [`Observation`]: one atomic capture (owned screen, revision, reason,
//!   terminal state, informational provenance). Equality/hash cover only the
//!   approval-relevant subset: provenance (timestamps/PIDs/paths) is excluded.
//! - [`Region`]: a cropped screen with geometry/origin preserved and the
//!   [`RegionPolicy`] recorded. Crops that would split a wide grapheme fail.
//!
//! Cell content reuses `crate::frame` types (`Cell`, `Color`, `Mods`, `Cursor`,
//! `Rgb`); they are not duplicated here. This module adds `Hash` impls for
//! those types so observations hash deterministically.

mod canonical;
mod error;
mod grid;
mod hash;
mod observation;
mod region;
mod validate;

pub use canonical::{canonical_string, canonical_value};
pub use error::{MAX_DIM, ScreenError};
pub use grid::Screen;
pub use observation::{CaptureProvenance, CaptureReason, Maybe, Observation, TermState};
pub use region::{Region, RegionPolicy};
