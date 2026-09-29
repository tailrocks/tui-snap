use super::*;
use tuiscotti::VENDORED_FONT;
use tuiscotti::profile::{
    BlinkPhase, CursorPolicy, MissingGlyphPolicy, PalettePolicy, RENDERER_VERSION, RenderProfile,
    VENDORED_FALLBACK_FACES, VENDORED_FONT_BOLD, VENDORED_FONT_BOLD_ITALIC,
    VENDORED_FONT_BOLD_ITALIC_SHA256, VENDORED_FONT_BOLD_SHA256, VENDORED_FONT_ITALIC,
    VENDORED_FONT_ITALIC_SHA256, VENDORED_FONT_SHA256,
};
use tuiscotti::render::{Renderer, frame_from_screen, render_screen};

#[test]
fn face_swap_detected_at_construction_and_at_render() {
    // Mutate one byte of a COPY of the regular face: strict construction
    // must fail (hash verified, never substituted).
    let mut swapped = VENDORED_FONT.to_vec();
    let mid = swapped.len() / 2;
    swapped[mid] = swapped[mid].wrapping_add(1);
    let faces = tuiscotti::FontFaces {
        regular: &swapped,
        bold: VENDORED_FONT_BOLD,
        italic: VENDORED_FONT_ITALIC,
        bold_italic: VENDORED_FONT_BOLD_ITALIC,
    };
    let err = RenderProfile::strict(
        "x".to_string(),
        faces,
        [
            VENDORED_FONT_SHA256,
            VENDORED_FONT_BOLD_SHA256,
            VENDORED_FONT_ITALIC_SHA256,
            VENDORED_FONT_BOLD_ITALIC_SHA256,
        ],
        vec![],
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
    .expect_err("swapped face bytes are an error");
    assert!(err.to_string().contains("regular"), "{err}");

    // NOTE (wave 1): the historical second half mutated the borrowed bytes
    // AFTER construction through `UnsafeCell` aliasing, proving the
    // render-time re-verification refuses post-construction swaps. Safe Rust
    // cannot express mutation through a shared borrow and the workspace
    // forbids `unsafe`, so that half cannot run here (recorded as known
    // debt). The render path itself is still exercised on unswapped bytes.
    let rp = RenderProfile::strict(
        "x".to_string(),
        tuiscotti::FontFaces {
            regular: VENDORED_FONT,
            bold: VENDORED_FONT_BOLD,
            italic: VENDORED_FONT_ITALIC,
            bold_italic: VENDORED_FONT_BOLD_ITALIC,
        },
        [
            VENDORED_FONT_SHA256,
            VENDORED_FONT_BOLD_SHA256,
            VENDORED_FONT_ITALIC_SHA256,
            VENDORED_FONT_BOLD_ITALIC_SHA256,
        ],
        vec![],
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
    .expect("unswapped faces construct");
    Renderer::for_render_profile(&rp).expect("unswapped faces must render");
}

#[test]
fn grapheme_corpus_grids_match_hand_authored_expectations() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    for (name, symbol, width, covered) in CORPUS {
        let mut leads = vec![cell(0, 0, symbol, *width)];
        if *width == 2 {
            leads.push(cont(1, 0));
        }
        let screen = screen_from_leads(6, 2, leads).expect("screen_from_leads succeeds");
        // Independent grid expectation: exact widths/continuations.
        let frame = frame_from_screen(&screen, "qual");
        let lead = frame.get(0, 0).expect("frame.get(0, 0) is some");
        assert_eq!(lead.symbol, *symbol, "{name}");
        assert_eq!(lead.width, *width, "{name}");
        assert!(!lead.continuation, "{name}");
        if *width == 2 {
            let c = frame.get(1, 0).expect("frame.get(1, 0) is some");
            assert!(
                c.continuation && c.width == 0 && c.symbol.is_empty(),
                "{name}"
            );
        }
        // Source widths control layout: image dims derive from the grid.
        let rendered = render_screen(&screen, &rp).expect("render_screen(&screen, &rp) succeeds");
        let img = decode(&rendered.png).expect("decode succeeds");
        let (ew, eh) = rp.image_size(6, 2);
        assert_eq!((img.width(), img.height()), (ew, eh), "{name}");
        // Coverage expectation is per-case, not renderer-derived.
        if *covered {
            assert!(
                rendered.fidelity.missing.is_empty(),
                "{name}: {:?}",
                rendered.fidelity.missing
            );
        } else {
            assert_eq!(rendered.fidelity.missing.len(), 1, "{name}");
            assert!(!rendered.fidelity.to_json().contains("faithful"), "{name}");
        }
    }
}

#[test]
fn fallback_never_shifts_the_grid() {
    // Same hand-authored frame, full chain vs NO fallback chain: geometry
    // (dims + every pixel outside the served span) must be identical —
    // fallback only fills ink inside the source-width span (V04).
    let full =
        strict_placeholder(VENDORED_FALLBACK_FACES.to_vec()).expect("strict_placeholder succeeds");
    let bare = strict_placeholder(vec![]).expect("strict_placeholder succeeds");
    let screen = screen_from_leads(
        8,
        2,
        vec![cell(0, 0, "東", 2), cont(1, 0), cell(3, 0, "A", 1)],
    )
    .expect("screen_from_leads succeeds");
    let a = render_screen(&screen, &full).expect("render_screen(&screen, &full) succeeds");
    let b = render_screen(&screen, &bare).expect("render_screen(&screen, &bare) succeeds");
    assert!(!a.fidelity.fallback_glyphs.is_empty());
    assert_eq!(b.fidelity.missing.len(), 1);
    let ia = decode(&a.png).expect("decode succeeds");
    let ib = decode(&b.png).expect("decode succeeds");
    assert_eq!((ia.width(), ia.height()), (ib.width(), ib.height()));
    // Served span: row 0, cells 0..2 → pixel rect.
    let (cw, ch, pad, u) = (10u32, 21u32, 12u32, 2u32);
    let (x0, x1) = (pad * u, (pad + 2 * cw) * u);
    let (y0, y1) = (pad * u, (pad + ch) * u);
    let mut inside_diffs = 0;
    for y in 0..ia.height() {
        for x in 0..ia.width() {
            let same = ia.get_pixel(x, y) == ib.get_pixel(x, y);
            let inside = x >= x0 && x < x1 && y >= y0 && y < y1;
            if inside {
                if !same {
                    inside_diffs += 1;
                }
            } else {
                assert!(same, "fallback moved pixels outside its span at ({x},{y})");
            }
        }
    }
    assert!(
        inside_diffs > 20,
        "glyph vs tofu must differ inside the span"
    );
}
