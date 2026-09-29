//! Semantic provider + deterministic event harness (backlog Q06, Q07, Q09).
//!
//! - [`SemanticProvider`]: explicit role/id/label/focused/disabled/hit-region
//!   data. Semantics NEVER come from appearance: nothing here inspects pixels,
//!   glyphs, styles, or [`Screen`](crate::screen::Screen) cells.
//! - Locators ([`by_role`], [`by_id`], [`by_label`]) resolve provider nodes to
//!   hit-region-center screen coordinates for REAL input. They return coords
//!   only and never call application controllers (Q07).
//! - [`RatatuiTestAdapter`]: example provider fed by test code alongside a
//!   draw closure; tests map widget areas to nodes manually.
//! - [`Harness`]: deterministic `update`/`render` + manual clock harness for
//!   runtime tests (Q09). No live clock, threads, or services.

mod harness;
mod model;

pub use harness::{Harness, HarnessEvent};
pub use model::{
    HitRegion, RatatuiTestAdapter, Role, SemNode, SemanticError, SemanticProvider, by_id, by_label,
    by_role,
};
