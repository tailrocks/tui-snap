//! Rendering qualification (backlog V01, V03–V10; V02 frozen).
//!
//! Independent of capture paths: every grid here is HAND-AUTHORED (no
//! Ratatui, no PTY), so expectations cannot share the renderer's
//! assumptions. Source widths in the test data control layout; fallback
//! faces must never shift the grid (V04).

use tuiscotti::frame::Frame;
use tuiscotti::profile::{
    BlinkPhase, CursorPolicy, MissingGlyphPolicy, PalettePolicy, RENDERER_VERSION, RenderProfile,
    VENDORED_FACES, VENDORED_FONT_BOLD_ITALIC_SHA256, VENDORED_FONT_BOLD_SHA256,
    VENDORED_FONT_ITALIC_SHA256, VENDORED_FONT_SHA256,
};
use tuiscotti::render::render_screen;
use tuiscotti::{Cell, Color, Cursor, Mods, Provenance, Screen};

#[path = "render_qual/cache.rs"]
mod cache;
#[path = "render_qual/engine.rs"]
mod engine;
#[path = "render_qual/export.rs"]
mod export;
#[path = "render_qual/faces.rs"]
mod faces;
#[path = "render_qual/styles.rs"]
mod styles;

// ---------------------------------------------------------------------------
// Hand-authored grid helpers (independent of capture/model adapters).
// ---------------------------------------------------------------------------

fn prov() -> Provenance {
    Provenance {
        tool: "tuisnap".into(),
        tool_version: "test".into(),
        profile: "tuisnap-default".into(),
        source: "test".into(),
        argv: vec![],
        created_unix: 0,
    }
}

fn cell(x: u16, y: u16, symbol: &str, width: u8) -> Cell {
    Cell {
        x,
        y,
        symbol: symbol.to_string(),
        width,
        continuation: false,
        fg: Color::Default,
        bg: Color::Default,
        mods: Mods::default(),
        underline_color: Color::Default,
    }
}

fn cont(x: u16, y: u16) -> Cell {
    Cell {
        x,
        y,
        symbol: String::new(),
        width: 0,
        continuation: true,
        fg: Color::Default,
        bg: Color::Default,
        mods: Mods::default(),
        underline_color: Color::Default,
    }
}

/// Blank frame with `leads` overlaid by (x, y). Caller supplies wide-cell
/// continuations explicitly — nothing is inferred.
fn frame_from_leads(
    cols: u16,
    rows: u16,
    leads: Vec<Cell>,
) -> Result<Frame, Box<dyn std::error::Error>> {
    let mut f = Frame::blank(cols, rows, prov());
    for c in leads {
        f.set(c);
    }
    f.validate()?;
    Ok(f)
}

fn screen_from_leads(
    cols: u16,
    rows: u16,
    leads: Vec<Cell>,
) -> Result<Screen, Box<dyn std::error::Error>> {
    let mut cells = Vec::with_capacity(cols as usize * rows as usize);
    for y in 0..rows {
        for x in 0..cols {
            cells.push(Cell::blank(x, y));
        }
    }
    for c in leads {
        let i = c.y as usize * cols as usize + c.x as usize;
        cells[i] = c;
    }
    Ok(Screen::validate(
        cols,
        rows,
        0,
        0,
        cells,
        Cursor::default(),
    )?)
}

fn strict_placeholder(
    fallbacks: Vec<tuiscotti::FallbackFace<'_>>,
) -> Result<RenderProfile<'_>, Box<dyn std::error::Error>> {
    RenderProfile::strict(
        "qual".to_string(),
        VENDORED_FACES,
        [
            VENDORED_FONT_SHA256,
            VENDORED_FONT_BOLD_SHA256,
            VENDORED_FONT_ITALIC_SHA256,
            VENDORED_FONT_BOLD_ITALIC_SHA256,
        ],
        fallbacks,
        16.0,
        10,
        21,
        12,
        2,
        PalettePolicy::xterm(),
        CursorPolicy::Show,
        BlinkPhase::On,
        MissingGlyphPolicy::Placeholder,
        RENDERER_VERSION,
    )
    .map_err(Box::<dyn std::error::Error>::from)
}

fn decode(png: &[u8]) -> Result<image::RgbImage, Box<dyn std::error::Error>> {
    Ok(image::load_from_memory(png)?.to_rgb8())
}

// ---------------------------------------------------------------------------
// V01: one engine for screens and frames.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// V03/V04: grapheme qualification corpus — hand-authored expected grids.
// Each case states its expected (symbol, width, continuation) cells; the
// test asserts the adapted grid matches EXACTLY and the render keeps the
// source geometry (dims from source widths, never from fallback metrics).
// ---------------------------------------------------------------------------

/// (case name, lead symbol, expected width, covered-by-pinned-chain?)
const CORPUS: &[(&str, &str, u8, bool)] = &[
    ("combining", "e\u{301}", 1, true), // e + U+0301 overlays at one origin
    ("cjk", "東", 2, true),             // vendored CJK fallback face
    ("nerd", "\u{f015}", 1, true),      // U+F015 primary Nerd coverage
    ("box", "╔", 1, true),
    ("box-h", "═", 1, true),
    ("braille", "⠋", 1, true),
    ("block-full", "█", 1, true),
    ("block-shade", "▓", 1, true),
    ("symbol-star", "★", 1, true),  // vendored Symbols2 fallback face
    ("coffee", "☕", 2, true),      // wide, vendored fallback face
    ("emoji-crab", "🦀", 2, false), // POLICY: wide; uncovered → strict fails
];

// ---------------------------------------------------------------------------
// V08: content-addressed cache.
// ---------------------------------------------------------------------------

static CACHE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn cache_png() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    Ok(render_screen(&screen_from_leads(4, 2, vec![cell(0, 0, "Q", 1)])?, &rp)?.png)
}
