//! Canonical state projections (F12): deterministic [`Screen`] text for
//! snapshot gates plus its structured twin.
//!
//! These are pure functions over the screen model — no Insta, no rendering,
//! no I/O — so they live in core next to the type they project. The text
//! projection is the canonical state every gate binds (compound screenshot
//! gates tag the PNG with its generation); the JSON projection carries the
//! same state for structured review.

use super::Screen;
use crate::frame::{Color, CursorStyle};
use std::fmt::Write as _;

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
fn mods_token(m: crate::frame::Mods) -> String {
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

/// Deterministic styled-state text for snapshot gates.
///
/// Covers geometry (cols/rows/origin), every cell in row-major order (symbol,
/// width, continuation flag, colors, all modifiers — styled blanks included),
/// and cursor intent (position, visibility, style, blink). No maps, pointers,
/// or timestamps are involved, so the output is stable across runs.
#[must_use]
pub fn canonical_string(screen: &Screen) -> String {
    let mut out = String::from("tuiscotti screen snapshot v1\n");
    let (ox, oy) = screen.origin();
    writeln!(
        out,
        "geometry cols={} rows={} ox={} oy={}",
        screen.cols(),
        screen.rows(),
        ox,
        oy
    )
    .unwrap_or_default();
    let c = screen.cursor();
    writeln!(
        out,
        "cursor x={} y={} visible={} style={} blinking={}",
        c.x,
        c.y,
        c.visible,
        cursor_style_token(c.style),
        c.blinking
    )
    .unwrap_or_default();
    for cell in screen.cells() {
        write!(
            out,
            "cell {},{} sym={:?} w={} cont={} fg={} bg={} mods={}",
            cell.x,
            cell.y,
            cell.symbol,
            cell.width,
            cell.continuation,
            color_token(cell.fg),
            color_token(cell.bg),
            mods_token(cell.mods)
        )
        .unwrap_or_default();
        // Sparse: default underline color adds nothing, so default snapshots
        // keep their exact shape.
        if !cell.underline_color.is_default() {
            write!(out, " uc={}", color_token(cell.underline_color)).unwrap_or_default();
        }
        out.push('\n');
    }
    out
}

/// Structured projection of the same state for JSON snapshot gates.
///
/// Same coverage as [`canonical_string`] (geometry, all cells, cursor); colors use
/// the same tokens, modifiers are explicit booleans. `"underline"` stays a
/// bool (any style) for shape stability; the exact style and underline color
/// appear as sparse keys (`"underline_style"` / `"underline_color"`) only
/// when non-default, so default snapshots keep their exact shape.
#[must_use]
pub fn canonical_value(screen: &Screen) -> serde_json::Value {
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
