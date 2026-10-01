//! Per-cell and cursor painting for [`Renderer`](super::Renderer).
//!
//! One impl block moved out of `renderer.rs` so both files stay under the
//! repo line gate; behavior is unchanged.

use tuiscotti_core::frame::Frame;

use super::super::{CellSinks, FallbackGlyph, MissingGlyph, blend, draw_symbol, fill_rect};
use super::Renderer;
use crate::profile::BlinkPhase;

impl Renderer {
    /// Paint one lead cell: background, sampled-blink/hidden gating, glyph,
    /// and text decorations. Continuation cells are skipped (the lead cell
    /// spans them).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "pixel geometry; saturation intended"
    )]
    pub(super) fn draw_cell(
        &mut self,
        img: &mut image::RgbImage,
        frame: &Frame,
        x: u16,
        y: u16,
        missing: &mut Vec<MissingGlyph>,
        fallback_glyphs: &mut Vec<FallbackGlyph>,
    ) {
        let Some(cell) = frame.get(x, y) else {
            return;
        };
        if cell.continuation {
            return;
        }
        let (u, cell_w, cell_h, pad) = self.geom();
        let u_i = u.cast_signed();
        {
            let (fg, cbg) =
                Frame::resolve_cell(cell, self.profile.default_fg, self.profile.default_bg);
            let span = u32::from(cell.width.max(1)) * cell_w;
            let cx = pad + u32::from(x) * cell_w;
            let cy = pad + u32::from(y) * cell_h;
            if cbg != self.profile.default_bg {
                fill_rect(img, cx, cy, span, cell_h, cbg);
            }
            // V07: a still samples one declared blink phase. Off-phase
            // blinking cells keep their background but draw no ink and no
            // text decorations — what a real terminal shows mid-blink.
            // Blink intent stays in canonical state; only the still is
            // sampled. Legacy renderers pin phase On (frozen-visible).
            if cell.mods.hidden || (cell.mods.blink && self.blink_phase == BlinkPhase::Off) {
                return;
            }
            let baseline = cy.cast_signed() + self.set.regular.ascent.round() as i32;
            // Whitespace cells carry no glyph, but real terminals still
            // draw underline/strikethrough across them (the background is
            // already painted above) — decorations are not part of the
            // skipped glyph draw.
            if !cell.symbol.trim().is_empty() {
                draw_symbol(
                    img,
                    &self.set,
                    &mut self.glyphs,
                    &cell.symbol,
                    cx.cast_signed(),
                    baseline,
                    span,
                    cy.cast_signed(),
                    cell_h,
                    fg,
                    cell.mods.bold,
                    cell.mods.italic,
                    u_i,
                    Some(CellSinks {
                        x,
                        y,
                        missing,
                        fallback: fallback_glyphs,
                    }),
                );
            }
            if cell.mods.underline {
                // All styles render as one rule; only the color varies
                // (SGR 58) — double/curly/dotted/dashed geometry is a
                // known renderer limitation, tracked in canonical state.
                let ul = match cell.underline_color {
                    tuiscotti_core::frame::Color::Default => fg,
                    tuiscotti_core::frame::Color::Indexed(i) => {
                        tuiscotti_core::frame::Rgb::from_indexed(i)
                    }
                    tuiscotti_core::frame::Color::Rgb(r) => r,
                };
                let uy = (baseline + 2 * u_i).min((cy + cell_h - 1).cast_signed());
                let th = if cell.mods.bold { 2 * u } else { u };
                for t in 0..th {
                    let row = (uy + t.cast_signed()).cast_unsigned();
                    for dx in 0..span {
                        blend(img, cx + dx, row, ul, 255);
                    }
                }
            }
            if cell.mods.strikethrough {
                let sy = baseline - (self.set.regular.ascent * 0.35) as i32;
                for t in 0..u {
                    let row = (sy + t.cast_signed()).max(0).cast_unsigned();
                    for dx in 0..span {
                        blend(img, cx + dx, row, fg, 255);
                    }
                }
            }
        }
    }

    /// Block cursor: fill cell with fg, redraw glyph in bg (classic
    /// terminal). Underline/bar cursors draw rules instead.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "pixel geometry; saturation intended"
    )]
    pub(super) fn draw_cursor(&mut self, img: &mut image::RgbImage, frame: &Frame) {
        let (u, cell_w, cell_h, pad) = self.geom();
        let u_i = u.cast_signed();
        if !(frame.cursor.visible && self.profile.cursor_visible) {
            return;
        }
        let mut cx = frame.cursor.x;
        if frame
            .get(cx, frame.cursor.y)
            .is_some_and(|c| c.continuation)
        {
            cx = cx.saturating_sub(1);
        }
        if let Some(cell) = frame.get(cx, frame.cursor.y) {
            let (fg, cbg) =
                Frame::resolve_cell(cell, self.profile.default_fg, self.profile.default_bg);
            let span = u32::from(cell.width.max(1)) * cell_w;
            let px = pad + u32::from(cx) * cell_w;
            let py = pad + u32::from(frame.cursor.y) * cell_h;
            let style = frame.cursor.style;
            match style {
                tuiscotti_core::frame::CursorStyle::Block => {
                    fill_rect(img, px, py, span, cell_h, fg);
                    if !cell.mods.hidden && !cell.symbol.trim().is_empty() {
                        let baseline = py.cast_signed() + self.set.regular.ascent.round() as i32;
                        draw_symbol(
                            img,
                            &self.set,
                            &mut self.glyphs,
                            &cell.symbol,
                            px.cast_signed(),
                            baseline,
                            span,
                            py.cast_signed(),
                            cell_h,
                            cbg,
                            false,
                            false,
                            u_i,
                            None,
                        );
                    }
                }
                tuiscotti_core::frame::CursorStyle::Underline => {
                    let uy = (py + cell_h - 2 * u).cast_signed();
                    for t in 0..2 * u {
                        let row = (uy + t.cast_signed()).cast_unsigned();
                        for dx in 0..span {
                            blend(img, px + dx, row, fg, 255);
                        }
                    }
                }
                tuiscotti_core::frame::CursorStyle::Bar => {
                    for dx in 0..2 * u {
                        for dy in 0..cell_h {
                            blend(img, px + dx, py + dy, fg, 255);
                        }
                    }
                }
            }
        }
    }
}
