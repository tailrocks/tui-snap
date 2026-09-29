//! Insta integration prototype (spike for backlog I01–I05; de-risks M2).
//!
//! This module is an experiment, not the final M2 API. It answers: can public
//! Insta APIs carry a compound canonical-plus-PNG snapshot lifecycle?
//!
//! - [`insta_string`] / [`insta_value`]: deterministic [`Screen`] projections
//!   for `assert_snapshot!` / `assert_json_snapshot!` (I01).
//! - [`PngPixelComparator`]: custom [`insta::Comparator`] doing decoded-pixel
//!   equality via [`tuiscotti_render::diff`] for for `assert_binary_snapshot!` (I02, I03).
//!
//! Qualified against insta **1.48.0** (see `Cargo.lock`), trait signature
//! verified against the compiled registry source
//! (`insta-1.48.0/src/comparator.rs`):
//!
//! ```text
//! pub trait Comparator: Send + Sync + 'static {
//!     fn matches(&self, reference: &Snapshot, test: &Snapshot) -> bool;
//!     fn matches_fully(&self, reference: &Snapshot, test: &Snapshot) -> bool { ... }
//!     fn dyn_clone(&self) -> Box<dyn Comparator>;
//! }
//! ```
//!
//! Public-API notes (see also the header of `tests/insta_spike.rs`):
//! - [`insta::Comparator`], [`insta::DefaultComparator`], [`insta::Settings`]
//!   and [`insta::Snapshot`] are root-public. [`insta::Snapshot::contents`]
//!   exposes the payload, but the [`insta::internals::SnapshotContents`] enum
//!   lives under `insta::internals` and there is no public
//!   `Snapshot::as_binary()` accessor — that is the one gap found (I05).
//! - `MetaData::snapshot_kind` (binary extension) is `pub(crate)`, so an
//!   external comparator cannot re-check extension equality the way
//!   `DefaultComparator` does. For decoded-pixel equality this is the correct
//!   behavior anyway: bytes that do not decode as PNG never match.

use tuiscotti_render::diff::AlphaPolicy;
use tuiscotti_core::frame::{Color, CursorStyle};
use tuiscotti_core::screen::Screen;

/// Compact lossless color token shared by both projections.
fn color_token(c: Color) -> String {
    match c {
        Color::Default => "default".to_string(),
        Color::Indexed(n) => format!("index={n}"),
        Color::Rgb(rgb) => format!("#{:02x}{:02x}{:02x}", rgb.r, rgb.g, rgb.b),
    }
}

/// Modifier flags in struct-declaration order, or `-` when no flag is set.
/// Every flag (including `hidden`/`blink`) is always represented (M02).
fn mods_token(m: tuiscotti_core::frame::Mods) -> String {
    let mut out = Vec::new();
    if m.hidden {
        out.push("hidden");
    }
    if m.blink {
        out.push("blink");
    }
    if m.bold {
        out.push("bold");
    }
    if m.dim {
        out.push("dim");
    }
    if m.italic {
        out.push("italic");
    }
    let ul = m.effective_underline_style();
    if ul.is_some() {
        out.push(ul.token());
    }
    if m.strikethrough {
        out.push("strikethrough");
    }
    if m.reverse {
        out.push("reverse");
    }
    if out.is_empty() {
        "-".to_string()
    } else {
        out.join("+")
    }
}

fn cursor_style_token(s: CursorStyle) -> &'static str {
    match s {
        CursorStyle::Block => "block",
        CursorStyle::Underline => "underline",
        CursorStyle::Bar => "bar",
    }
}

/// Deterministic styled-state text for `insta::assert_snapshot!` (I01).
///
/// Covers geometry (cols/rows/origin), every cell in row-major order (symbol,
/// width, continuation flag, colors, all modifiers — styled blanks included),
/// and cursor intent (position, visibility, style, blink). No maps, pointers,
/// or timestamps are involved, so the output is stable across runs.
#[must_use]
pub fn insta_string(screen: &Screen) -> String {
    let mut out = String::from("tuisnap screen snapshot v1\n");
    let (ox, oy) = screen.origin();
    out.push_str(&format!(
        "geometry cols={} rows={} ox={} oy={}\n",
        screen.cols(),
        screen.rows(),
        ox,
        oy
    ));
    let c = screen.cursor();
    out.push_str(&format!(
        "cursor x={} y={} visible={} style={} blinking={}\n",
        c.x,
        c.y,
        c.visible,
        cursor_style_token(c.style),
        c.blinking
    ));
    for cell in screen.cells() {
        out.push_str(&format!(
            "cell {},{} sym={:?} w={} cont={} fg={} bg={} mods={}",
            cell.x,
            cell.y,
            cell.symbol,
            cell.width,
            cell.continuation,
            color_token(cell.fg),
            color_token(cell.bg),
            mods_token(cell.mods)
        ));
        // Sparse: default underline color adds nothing, so default snapshots
        // keep their exact shape.
        if !cell.underline_color.is_default() {
            out.push_str(&format!(" uc={}", color_token(cell.underline_color)));
        }
        out.push('\n');
    }
    out
}

/// Structured projection of the same state for `assert_json_snapshot!` (I01).
///
/// Same coverage as [`insta_string`] (geometry, all cells, cursor); colors use
/// the same tokens, modifiers are explicit booleans. `"underline"` stays a
/// bool (any style) for shape stability; the exact style and underline color
/// appear as sparse keys (`"underline_style"` / `"underline_color"`) only
/// when non-default, so default snapshots keep their exact shape.
#[must_use]
pub fn insta_value(screen: &Screen) -> serde_json::Value {
    let (ox, oy) = screen.origin();
    let c = screen.cursor();
    let cells: Vec<serde_json::Value> = screen
        .cells()
        .iter()
        .map(|cell| {
            let mut mods = serde_json::json!({
                "hidden": cell.mods.hidden,
                "blink": cell.mods.blink,
                "bold": cell.mods.bold,
                "dim": cell.mods.dim,
                "italic": cell.mods.italic,
                "underline": cell.mods.underline,
                "strikethrough": cell.mods.strikethrough,
                "reverse": cell.mods.reverse,
            });
            if cell.mods.underline_style.is_some() {
                mods["underline_style"] = serde_json::json!(cell.mods.underline_style.token());
            }
            let mut obj = serde_json::json!({
                "x": cell.x,
                "y": cell.y,
                "symbol": cell.symbol,
                "width": cell.width,
                "continuation": cell.continuation,
                "fg": color_token(cell.fg),
                "bg": color_token(cell.bg),
                "mods": mods,
            });
            if !cell.underline_color.is_default() {
                obj["underline_color"] = serde_json::json!(color_token(cell.underline_color));
            }
            obj
        })
        .collect();
    serde_json::json!({
        "cols": screen.cols(),
        "rows": screen.rows(),
        "ox": ox,
        "oy": oy,
        "cursor": {
            "x": c.x,
            "y": c.y,
            "visible": c.visible,
            "style": cursor_style_token(c.style),
            "blinking": c.blinking,
        },
        "cells": cells,
    })
}

/// Custom Insta comparator (I03): binary snapshots compare by **decoded**
/// pixels under an explicit [`AlphaPolicy`]; text snapshots delegate to
/// [`insta::DefaultComparator`] so canonical assertions keep exact stock
/// semantics (including legacy-format acceptance).
///
/// Never matches: undecodable/corrupt PNG on either side, a missing binary
/// sidecar (`Binary(None)`), text/binary kind mixes, or any decode error.
/// There is no threshold and no perceptual fallback — the verdict is
/// [`tuiscotti_render::diff::PixelVerdict::pixels_equal`].
#[derive(Debug, Clone, Copy, Default)]
pub struct PngPixelComparator {
    alpha_policy: AlphaPolicy,
}

impl PngPixelComparator {
    /// Comparator with an explicit alpha policy (`StraightRgba` default).
    #[must_use]
    pub fn new(alpha_policy: AlphaPolicy) -> Self {
        Self { alpha_policy }
    }

    #[must_use]
    pub fn alpha_policy(self) -> AlphaPolicy {
        self.alpha_policy
    }
}

impl insta::Comparator for PngPixelComparator {
    fn matches(&self, reference: &insta::Snapshot, test: &insta::Snapshot) -> bool {
        use insta::internals::SnapshotContents;
        match (reference.contents(), test.contents()) {
            (SnapshotContents::Binary(Some(a)), SnapshotContents::Binary(Some(b))) => {
                tuiscotti_render::diff::compare_png_with_alpha(a, b, self.alpha_policy)
                    .map(|v| v.pixels_equal)
                    .unwrap_or(false)
            }
            // Stock text semantics, untouched: no canonical field is ignored
            // or reinterpreted here.
            (SnapshotContents::Text(_), SnapshotContents::Text(_)) => {
                insta::DefaultComparator.matches(reference, test)
            }
            // Absent sidecar or kind mix: never a match.
            _ => false,
        }
    }

    fn dyn_clone(&self) -> Box<dyn insta::Comparator> {
        Box::new(*self)
    }
}
