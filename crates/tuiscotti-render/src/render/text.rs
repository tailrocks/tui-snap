//! One-shot PNG conveniences and the normalized ANSI dump.

use super::{RenderError, Rendered, Renderer};
use crate::profile::FontFaces;
use crate::profile::Profile;
use tuiscotti_core::frame::Frame;

/// Render a validated frame to PNG bytes under `profile`.
///
/// One-shot convenience: constructs a fresh [`Renderer`] per call (8 font
/// parses, cold glyph cache). Bulk gates should keep a `Renderer` instead.
///
/// # Errors
///
/// Returns `RenderError` when the frame is invalid or PNG encoding fails.
pub fn render_png(
    frame: &Frame,
    profile: &Profile,
    faces: &FontFaces<'_>,
) -> Result<Vec<u8>, RenderError> {
    Renderer::new(profile, faces)?.render_png(frame)
}

/// Render plus exact coverage accounting (see [`Fidelity`](super::Fidelity)).
///
/// One-shot convenience: constructs a fresh [`Renderer`] per call (8 font
/// parses, cold glyph cache). Bulk gates should keep a `Renderer` instead.
///
/// # Errors
///
/// Returns `RenderError` when the frame is invalid or PNG encoding fails.
pub fn render_png_report(
    frame: &Frame,
    profile: &Profile,
    faces: &FontFaces<'_>,
) -> Result<Rendered, RenderError> {
    Renderer::new(profile, faces)?.render(frame)
}

/// Normalized ANSI dump (SGR runs from canonical state — for debugging, not
/// for replay; replay raw streams with `crate::ansi::replay_raw`).
#[must_use]
pub fn ansi_dump(frame: &Frame) -> String {
    let mut out = String::new();
    for y in 0..frame.rows {
        let mut cur = String::new();
        for x in 0..frame.cols {
            let Some(c) = frame.get(x, y) else { continue };
            if c.continuation {
                continue;
            }
            let sgr = sgr_for(c);
            if sgr != cur {
                out.push_str("\x1b[0m");
                if !sgr.is_empty() {
                    out.push_str("\x1b[");
                    out.push_str(&sgr);
                    out.push('m');
                }
                cur = sgr;
            }
            out.push_str(&c.symbol);
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

fn sgr_for(c: &tuiscotti_core::frame::Cell) -> String {
    let mut p: Vec<String> = Vec::new();
    if c.mods.hidden {
        p.push("8".into());
    }
    if c.mods.blink {
        p.push("5".into());
    }
    if c.mods.bold {
        p.push("1".into());
    }
    if c.mods.dim {
        p.push("2".into());
    }
    if c.mods.italic {
        p.push("3".into());
    }
    match c.mods.effective_underline_style() {
        tuiscotti_core::frame::UnderlineStyle::None => {}
        tuiscotti_core::frame::UnderlineStyle::Single => p.push("4".into()),
        tuiscotti_core::frame::UnderlineStyle::Double => p.push("4:2".into()),
        tuiscotti_core::frame::UnderlineStyle::Curly => p.push("4:3".into()),
        tuiscotti_core::frame::UnderlineStyle::Dotted => p.push("4:4".into()),
        tuiscotti_core::frame::UnderlineStyle::Dashed => p.push("4:5".into()),
    }
    if c.mods.strikethrough {
        p.push("9".into());
    }
    if c.mods.reverse {
        p.push("7".into());
    }
    let push_color = |p: &mut Vec<String>, code: u8, c: tuiscotti_core::frame::Color| match c {
        tuiscotti_core::frame::Color::Default => {}
        tuiscotti_core::frame::Color::Indexed(i) => p.push(format!("{code};5;{i}")),
        tuiscotti_core::frame::Color::Rgb(r) => {
            p.push(format!("{code};2;{};{};{}", r.r, r.g, r.b));
        }
    };
    push_color(&mut p, 38, c.fg);
    push_color(&mut p, 48, c.bg);
    push_color(&mut p, 58, c.underline_color);
    p.join(";")
}
