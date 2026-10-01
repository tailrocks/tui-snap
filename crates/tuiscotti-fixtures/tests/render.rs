//! Renderer matrix: real glyphs, geometry, Unicode, themes, sizes, formats.
//!
//! Covers: A→B pixel change, deterministic reruns (byte-identical PNG),
//! box/Braille/icons non-blank, CJK 2-cell geometry, clipping/wrapping,
//! themes and sizes, geometry-pin failure, SVG/ANSI outputs.

use ratatui::widgets::Paragraph;
use tuiscotti::{Profile, Provenance, VENDORED_FACES};

fn prov() -> Provenance {
    Provenance {
        tool: "tuiscotti".into(),
        tool_version: "test".into(),
        profile: "tuiscotti-default".into(),
        source: "test".into(),
        argv: vec![],
        created_unix: 0,
    }
}

fn profile() -> Profile {
    Profile::default_profile()
}

fn widget_png(text: &str, cols: u16, rows: u16) -> anyhow::Result<Vec<u8>> {
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new(text), cols, rows, prov());
    Ok(tuiscotti::render::render_png(
        &frame,
        &profile(),
        &VENDORED_FACES,
    )?)
}
fn frame_with_mods(symbol: &str, mods: tuiscotti::Mods) -> tuiscotti::Frame {
    let mut f = tuiscotti::ratatui::widget_frame(Paragraph::new(symbol), 10, 3, prov());
    // Apply mods to the non-blank lead cells only.
    for cell in &mut f.cells {
        if !cell.continuation && !cell.symbol.trim().is_empty() {
            cell.mods = mods;
        }
    }
    f
}

#[path = "render/matrix.rs"]
mod matrix;

#[path = "render/styles.rs"]
mod styles;

#[path = "render/fallback.rs"]
mod fallback;
