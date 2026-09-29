use super::*;
use tuiscotti::frame::FRAME_VERSION;
use tuiscotti::profile::{
    BlinkPhase, CursorPolicy, MissingGlyphPolicy, PalettePolicy, RENDERER_VERSION, RenderProfile,
    VENDORED_FACES, VENDORED_FALLBACK_FACES, VENDORED_FONT_BOLD, VENDORED_FONT_BOLD_ITALIC,
    VENDORED_FONT_BOLD_ITALIC_SHA256, VENDORED_FONT_BOLD_SHA256, VENDORED_FONT_ITALIC,
    VENDORED_FONT_ITALIC_SHA256, VENDORED_FONT_SHA256, font_sha256,
};
use tuiscotti::render::{frame_from_screen, render_frame_strict, render_screen, render_screen_png};
use tuiscotti::{Color, Mods, Screen, VENDORED_FONT};

#[test]
fn screen_and_frame_share_one_engine() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let mods = Mods {
        bold: true,
        underline: true,
        ..Mods::default()
    };
    let mut lead = cell(0, 0, "東", 2);
    lead.mods = mods;
    lead.fg = Color::Indexed(2);
    let frame = frame_from_leads(6, 3, vec![lead, cont(1, 0), cell(2, 0, "★", 1)])
        .expect("frame_from_leads succeeds");
    let screen = Screen::from_frame(&frame).expect("Screen::from_frame(&frame) succeeds");
    let via_screen = render_screen(&screen, &rp).expect("render_screen(&screen, &rp) succeeds");
    let via_frame =
        render_frame_strict(&frame, &rp).expect("render_frame_strict(&frame, &rp) succeeds");
    assert_eq!(via_screen.png, via_frame.png);
    assert_eq!(via_screen.fidelity.to_json(), via_frame.fidelity.to_json());
    assert_eq!(
        render_screen_png(&screen, &rp).expect("render_screen_png(&screen, &rp) succeeds"),
        via_screen.png
    );
}

#[test]
fn screen_adaptation_is_lossless_and_origin_free() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let mut lead = cell(1, 1, "京", 2);
    lead.mods.italic = true;
    lead.bg = Color::Indexed(4);
    let screen = screen_from_leads(6, 3, vec![lead.clone(), cont(2, 1)])
        .expect("screen_from_leads succeeds");
    let frame = frame_from_screen(&screen, "qual");
    assert_eq!(frame.version, FRAME_VERSION);
    assert_eq!((frame.cols, frame.rows), (6, 3));
    assert_eq!(frame.cells, screen.cells().to_vec());
    assert_eq!(frame.cursor, *screen.cursor());
    frame.validate().expect("frame.validate() succeeds");
    // Same grid at a nonzero origin renders identical pixels: origin is
    // positional metadata, not render input.
    let moved = Screen::validate(6, 3, 5, 7, screen.cells().to_vec(), *screen.cursor())
        .expect("Screen::validate(6, 3, 5, 7, screen.cells().to_vec(), *screen.cursor()) succeeds");
    assert_eq!(
        render_screen(&screen, &rp)
            .expect("render_screen(&screen, &rp) succeeds")
            .png,
        render_screen(&moved, &rp)
            .expect("render_screen(&moved, &rp) succeeds")
            .png
    );
    assert_eq!(lead.symbol, "京");
}

// ---------------------------------------------------------------------------
// Strict profile: pins, hashes, face-swap detection.
// ---------------------------------------------------------------------------
#[test]
fn vendored_pins_match_vendored_bytes() {
    assert_eq!(font_sha256(VENDORED_FONT), VENDORED_FONT_SHA256);
    assert_eq!(font_sha256(VENDORED_FONT_BOLD), VENDORED_FONT_BOLD_SHA256);
    assert_eq!(
        font_sha256(VENDORED_FONT_ITALIC),
        VENDORED_FONT_ITALIC_SHA256
    );
    assert_eq!(
        font_sha256(VENDORED_FONT_BOLD_ITALIC),
        VENDORED_FONT_BOLD_ITALIC_SHA256
    );
    let rp = RenderProfile::vendored();
    assert_eq!(
        rp.face_hashes(),
        &[
            VENDORED_FONT_SHA256.to_string(),
            VENDORED_FONT_BOLD_SHA256.to_string(),
            VENDORED_FONT_ITALIC_SHA256.to_string(),
            VENDORED_FONT_BOLD_ITALIC_SHA256.to_string(),
        ]
    );
    assert_eq!(rp.fallback_order().len(), VENDORED_FALLBACK_FACES.len());
    assert_eq!(rp.renderer_version(), RENDERER_VERSION);
    assert_eq!(rp.missing(), MissingGlyphPolicy::Strict);
    // Profile hash is stable and covers every pin.
    assert_eq!(rp.hash(), rp.hash());
    assert_eq!(rp.hash().len(), 64);
}

#[test]
fn strict_constructor_validates_every_pin() {
    let rp = RenderProfile::vendored();
    assert_eq!(rp.hash(), RenderProfile::vendored().hash());
    // Reordered fallback chain = different identity.
    let mut rev = VENDORED_FALLBACK_FACES.to_vec();
    rev.reverse();
    let reordered = strict_placeholder(rev)
        .expect("strict_placeholder succeeds")
        .hash();
    assert_ne!(
        reordered,
        strict_placeholder(VENDORED_FALLBACK_FACES.to_vec())
            .expect("strict_placeholder succeeds")
            .hash()
    );
    // Other phase / policy / cursor = different identity (no shared keys).
    assert_ne!(
        rp.with_phase(BlinkPhase::Off).hash(),
        rp.with_phase(BlinkPhase::On).hash()
    );
    assert_ne!(
        rp.with_missing(MissingGlyphPolicy::Placeholder).hash(),
        rp.hash()
    );
    assert_ne!(rp.with_cursor(CursorPolicy::Hide).hash(), rp.hash());
}

#[test]
fn strict_constructor_rejects_bad_pins_and_geometry() {
    let good = [
        VENDORED_FONT_SHA256,
        VENDORED_FONT_BOLD_SHA256,
        VENDORED_FONT_ITALIC_SHA256,
        VENDORED_FONT_BOLD_ITALIC_SHA256,
    ];
    let build = |hashes: [&str; 4]| {
        RenderProfile::strict(
            "x".to_string(),
            VENDORED_FACES,
            hashes,
            VENDORED_FALLBACK_FACES.to_vec(),
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
        )
    };
    // Swapped pin: no substitution, an error naming the face.
    let mut bad = good;
    bad[1] = VENDORED_FONT_SHA256;
    let err = build(bad).expect_err("build(bad) is an error");
    assert!(err.to_string().contains("bold"), "{err}");
    // Wrong renderer version.
    let err = RenderProfile::strict(
        "x".to_string(),
        VENDORED_FACES,
        good,
        VENDORED_FALLBACK_FACES.to_vec(),
        16.0,
        10,
        21,
        12,
        2,
        PalettePolicy::xterm(),
        CursorPolicy::Show,
        BlinkPhase::On,
        MissingGlyphPolicy::Strict,
        RENDERER_VERSION + 1,
    )
    .expect_err("wrong renderer version is an error");
    assert!(err.to_string().contains("renderer version"), "{err}");
    // Degenerate geometry.
    let err = RenderProfile::strict(
        "x".to_string(),
        VENDORED_FACES,
        good,
        vec![],
        16.0,
        0,
        21,
        12,
        2,
        PalettePolicy::xterm(),
        CursorPolicy::Show,
        BlinkPhase::On,
        MissingGlyphPolicy::Strict,
        RENDERER_VERSION,
    )
    .expect_err("degenerate geometry is an error");
    assert!(err.to_string().contains("geometry"), "{err}");
}
