//! Shared fixtures for the render-cache qualification tests.

use super::super::*;
use tuiscotti::profile::{
    BlinkPhase, MissingGlyphPolicy, RenderProfile, VENDORED_FACES, VENDORED_FALLBACK_FACES,
    VENDORED_FONT_BOLD_ITALIC_SHA256, VENDORED_FONT_BOLD_SHA256, VENDORED_FONT_ITALIC_SHA256,
    VENDORED_FONT_SHA256,
};
use tuiscotti::{FallbackFace, FontFaces, Rgb, UnderlineStyle};

pub(super) fn styled_lead() -> Cell {
    Cell {
        x: 0,
        y: 0,
        symbol: "A".to_string(),
        width: 1,
        continuation: false,
        fg: Color::Indexed(1),
        bg: Color::Rgb(Rgb::new(10, 20, 30)),
        mods: Mods {
            bold: true,
            italic: true,
            underline: true,
            underline_style: UnderlineStyle::Single,
            ..Mods::default()
        },
        underline_color: Color::Indexed(5),
    }
}

pub(super) fn screen_of(lead: Cell) -> Result<Screen, String> {
    screen_from_leads(4, 2, vec![lead]).map_err(|e| e.to_string())
}

pub(super) fn screen_with_cursor(cursor: Cursor) -> Result<Screen, String> {
    let mut cells = Vec::with_capacity(8);
    for y in 0..2 {
        for x in 0..4 {
            cells.push(Cell::blank(x, y));
        }
    }
    cells[0] = styled_lead();
    Screen::validate(4, 2, 0, 0, cells, cursor).map_err(|e| e.to_string())
}

pub(super) fn screen_at_origin(ox: i32, oy: i32) -> Result<Screen, String> {
    let mut cells = Vec::with_capacity(8);
    for y in 0..2 {
        for x in 0..4 {
            cells.push(Cell::blank(x, y));
        }
    }
    cells[0] = styled_lead();
    Screen::validate(4, 2, ox, oy, cells, Cursor::default()).map_err(|e| e.to_string())
}

pub(super) fn placeholder_rp() -> RenderProfile<'static> {
    RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder)
}

/// One-field cell mutations of [`styled_lead`]: (label, mutated cell).
pub(super) fn cell_variants() -> Vec<(&'static str, Cell)> {
    let mut variants: Vec<(&str, Cell)> = Vec::new();
    // Symbol + position.
    let mut c = styled_lead();
    c.symbol = "B".to_string();
    variants.push(("symbol", c));
    let mut c = styled_lead();
    c.x = 1;
    variants.push(("x", c));
    // Foreground across all three color shapes.
    let mut c = styled_lead();
    c.fg = Color::Rgb(Rgb::new(1, 2, 3));
    variants.push(("fg-rgb", c));
    let mut c = styled_lead();
    c.fg = Color::Default;
    variants.push(("fg-default", c));
    let mut c = styled_lead();
    c.fg = Color::Indexed(2);
    variants.push(("fg-index", c));
    // Background across all three color shapes.
    let mut c = styled_lead();
    c.bg = Color::Indexed(4);
    variants.push(("bg-index", c));
    let mut c = styled_lead();
    c.bg = Color::Default;
    variants.push(("bg-default", c));
    // Every modifier bit, one at a time.
    for label in ["hidden", "blink", "dim", "strikethrough", "reverse"] {
        let mut cell = styled_lead();
        match label {
            "hidden" => cell.mods.hidden = true,
            "blink" => cell.mods.blink = true,
            "dim" => cell.mods.dim = true,
            "strikethrough" => cell.mods.strikethrough = true,
            _ => cell.mods.reverse = true,
        }
        variants.push((label, cell));
    }
    for label in ["bold", "italic", "underline"] {
        let mut cell = styled_lead();
        match label {
            "bold" => cell.mods.bold = false,
            "italic" => cell.mods.italic = false,
            _ => {
                cell.mods.underline = false;
                cell.mods.underline_style = UnderlineStyle::None;
            }
        }
        variants.push((label, cell));
    }
    // Underline style refinement alone (F09: the renderer draws styles).
    for style in [
        UnderlineStyle::Double,
        UnderlineStyle::Curly,
        UnderlineStyle::Dotted,
        UnderlineStyle::Dashed,
    ] {
        let mut cell = styled_lead();
        cell.mods.underline_style = style;
        variants.push(("underline-style", cell));
    }
    // Underline color alone (F09: the headline omission).
    let mut c = styled_lead();
    c.underline_color = Color::Indexed(6);
    variants.push(("underline-color-index", c));
    let mut c = styled_lead();
    c.underline_color = Color::Rgb(Rgb::new(9, 9, 9));
    variants.push(("underline-color-rgb", c));
    let mut c = styled_lead();
    c.underline_color = Color::Default;
    variants.push(("underline-color-default", c));
    variants
}

pub(super) struct ProfileParts<'a> {
    pub(super) name: String,
    pub(super) faces: FontFaces<'a>,
    pub(super) pins: [String; 4],
    pub(super) font_px: f32,
    pub(super) cell_w: u32,
    pub(super) cell_h: u32,
    pub(super) pad: u32,
    pub(super) scale: u32,
    pub(super) palette: PalettePolicy,
    pub(super) cursor: CursorPolicy,
    pub(super) blink: BlinkPhase,
    pub(super) missing: MissingGlyphPolicy,
    pub(super) fallbacks: Vec<FallbackFace<'a>>,
}

impl ProfileParts<'static> {
    pub(super) fn base() -> Self {
        Self {
            name: "qual".to_string(),
            faces: VENDORED_FACES,
            pins: [
                VENDORED_FONT_SHA256.to_string(),
                VENDORED_FONT_BOLD_SHA256.to_string(),
                VENDORED_FONT_ITALIC_SHA256.to_string(),
                VENDORED_FONT_BOLD_ITALIC_SHA256.to_string(),
            ],
            font_px: 16.0,
            cell_w: 10,
            cell_h: 21,
            pad: 12,
            scale: 2,
            palette: PalettePolicy::xterm(),
            cursor: CursorPolicy::Show,
            blink: BlinkPhase::On,
            missing: MissingGlyphPolicy::Placeholder,
            fallbacks: VENDORED_FALLBACK_FACES.to_vec(),
        }
    }
}

impl<'a> ProfileParts<'a> {
    pub(super) fn build(self) -> Result<RenderProfile<'a>, String> {
        let pins = std::array::from_fn(|i| self.pins[i].as_str());
        RenderProfile::strict(
            self.name,
            self.faces,
            pins,
            self.fallbacks,
            self.font_px,
            self.cell_w,
            self.cell_h,
            self.pad,
            self.scale,
            self.palette,
            self.cursor,
            self.blink,
            self.missing,
            RENDERER_VERSION,
        )
        .map_err(|e| e.to_string())
    }
}
