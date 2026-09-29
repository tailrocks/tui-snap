//! 07: advanced profiles — strict RenderProfile + missing-glyph policies.
//!
//! Run: `cargo run --example 07-advanced-profiles`
//!
//! `RenderProfile::strict` verifies every face hash and pin, substituting
//! nothing. `MissingGlyphPolicy::Strict` fails on uncovered glyphs;
//! `Placeholder` draws deterministic tofu AND reports it in `Fidelity`
//! (a bad pin is rejected at construction, never rendered).

use tuiscotti::profile::{
    BlinkPhase, CursorPolicy, MissingGlyphPolicy, PalettePolicy, RenderProfile, RENDERER_VERSION,
    VENDORED_FACES, VENDORED_FALLBACK_FACES, VENDORED_FONT_BOLD_ITALIC_SHA256,
    VENDORED_FONT_BOLD_SHA256, VENDORED_FONT_ITALIC_SHA256, VENDORED_FONT_SHA256,
};
use tuiscotti::ratatui::{render_screen, EdgePolicy};
use tuiscotti::render::Renderer;

fn strict(missing: MissingGlyphPolicy) -> RenderProfile<'static> {
    RenderProfile::strict(
        "learn".to_string(),
        VENDORED_FACES,
        [
            VENDORED_FONT_SHA256,
            VENDORED_FONT_BOLD_SHA256,
            VENDORED_FONT_ITALIC_SHA256,
            VENDORED_FONT_BOLD_ITALIC_SHA256,
        ],
        VENDORED_FALLBACK_FACES.to_vec(),
        16.0,
        10,
        21,
        12,
        2,
        PalettePolicy::xterm(),
        CursorPolicy::Show,
        BlinkPhase::On,
        missing,
        RENDERER_VERSION,
    )
    .unwrap()
}

fn main() {
    let screen = render_screen(
        16,
        3,
        |f| {
            f.render_widget(ratatui::widgets::Paragraph::new("plain ascii"), f.area());
        },
        EdgePolicy::default(),
    )
    .unwrap()
    .into_screen();

    // Strict policy: plain-ASCII frame renders, fidelity is exact.
    let strict_rp = strict(MissingGlyphPolicy::Strict);
    let mut r = Renderer::for_render_profile(&strict_rp).unwrap();
    let rendered = r.render_screen(&screen).unwrap();
    assert!(!rendered.png.is_empty());
    assert!(!rendered.fidelity.approximate);
    assert!(rendered.fidelity.missing.is_empty());

    // Placeholder policy: same frame, same engine, explicit opt-in.
    let placeholder_rp = strict(MissingGlyphPolicy::Placeholder);
    let mut r2 = Renderer::for_render_profile(&placeholder_rp).unwrap();
    let rendered2 = r2.render_screen(&screen).unwrap();
    assert_eq!(rendered.png.len(), rendered2.png.len());

    // A wrong pin fails construction — never a quiet fallback.
    let zero = "0".repeat(64);
    let bad = RenderProfile::strict(
        "bad".to_string(),
        VENDORED_FACES,
        [
            zero.as_str(),
            VENDORED_FONT_BOLD_SHA256,
            VENDORED_FONT_ITALIC_SHA256,
            VENDORED_FONT_BOLD_ITALIC_SHA256,
        ],
        Vec::new(),
        16.0,
        10,
        21,
        12,
        2,
        PalettePolicy::xterm(),
        CursorPolicy::Show,
        BlinkPhase::On,
        MissingGlyphPolicy::Strict,
        RENDERER_VERSION,
    );
    assert!(bad.is_err());

    println!(
        "EXAMPLE-07-OK hash={} png_bytes={}",
        &strict_rp.hash()[..12],
        rendered.png.len()
    );
}
