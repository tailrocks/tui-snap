//! Format contracts: the six distinct projections on pure views.
//!
//! ASCII (7-bit diagnostic) vs TXT (plain Unicode) vs ANSI (normalized SGR)
//! vs PNG (opaque RGB pixels) vs HTML (static offline, no JavaScript) vs
//! canonical JSON (versioned state + provenance). Includes the negative
//! battery: loss/truncation reporting, ANSI-only changes, whitespace,
//! hidden data, HTML injection, and mixed generations.

#[path = "common/mod.rs"]
mod common;

use common::{menu_frame, renderer};
use tuiscotti_fixtures::driver::Scenario;
use tuiscotti_fixtures::views::Theme;
use tuiscotti_render::formats::capture_all;

/// Capture every format of the menu demo in one bundle.
fn menu_bundle() -> tuiscotti_render::formats::CaptureBundle {
    let frame = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    capture_all(&mut renderer(), &frame, "menu demo").expect("capture")
}

#[path = "format_contracts/text.rs"]
mod text;

#[path = "format_contracts/pixels.rs"]
mod pixels;

#[path = "format_contracts/state.rs"]
mod state;
