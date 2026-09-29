use super::*;
use tuiscotti::profile::{BlinkPhase, MissingGlyphPolicy, RenderProfile};
use tuiscotti::render::{frame_from_screen, render_screen};
use tuiscotti::{Cell, Color, Mods};

#[test]
fn clipped_styles_draw_across_trailing_spaces() {
    // Hand-authored: underline+strike on blank cells must ink pixels (real
    // terminals decorate whitespace); image geometry is unchanged.
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let deco = Mods {
        underline: true,
        strikethrough: true,
        ..Mods::default()
    };
    let mut blanks: Vec<Cell> = (0..4).map(|x| cell(x, 0, " ", 1)).collect();
    for c in &mut blanks {
        c.mods = deco;
    }
    let decorated = screen_from_leads(8, 2, blanks).expect("screen_from_leads succeeds");
    let plain = screen_from_leads(8, 2, vec![]).expect("screen_from_leads succeeds");
    let a = decode(
        &render_screen(&decorated, &rp)
            .expect("render_screen(&decorated, &rp) succeeds")
            .png,
    )
    .expect("decode succeeds");
    let b = decode(
        &render_screen(&plain, &rp)
            .expect("render_screen(&plain, &rp) succeeds")
            .png,
    )
    .expect("decode succeeds");
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let changed = a.pixels().zip(b.pixels()).filter(|(x, y)| x != y).count();
    assert!(
        changed > 50,
        "decorations across blanks changed {changed} px"
    );
}

#[test]
fn styled_space_paints_exact_palette_color() {
    // Independent pixel expectation: an Indexed(1) space centers on
    // xterm red (205,49,49) — no glyph ink involved.
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let mut c = cell(2, 1, " ", 1);
    c.bg = Color::Indexed(1);
    let screen = screen_from_leads(6, 3, vec![c]).expect("screen_from_leads succeeds");
    let img = decode(
        &render_screen(&screen, &rp)
            .expect("render_screen(&screen, &rp) succeeds")
            .png,
    )
    .expect("decode succeeds");
    let (cw, ch, pad, u) = (10u32, 21u32, 12u32, 2u32);
    let (px, py) = ((pad + 2 * cw) * u + 10, (pad + ch) * u + 21);
    assert_eq!(img.get_pixel(px, py), &image::Rgb([205, 49, 49]));
}

// ---------------------------------------------------------------------------
// V05: strict missing-glyph policy.
// ---------------------------------------------------------------------------
#[test]
fn strict_missing_glyph_fails_with_exact_diagnostics() {
    let rp = RenderProfile::vendored(); // Strict by default.
    let screen = screen_from_leads(8, 2, vec![cell(3, 0, "🦀", 2), cont(4, 0)])
        .expect("screen_from_leads succeeds");
    let err = render_screen(&screen, &rp).expect_err("strict must fail on uncovered glyph");
    let msg = err.to_string();
    assert!(msg.contains("strict missing-glyph"), "{msg}");
    assert!(msg.contains("U+1F980"), "{msg}");
    assert!(msg.contains("(3,0)"), "{msg}");
    // Explicit Placeholder renders + reports, never claims faithful.
    let rp = rp.with_missing(MissingGlyphPolicy::Placeholder);
    let r = render_screen(&screen, &rp).expect("render_screen(&screen, &rp) succeeds");
    assert!(r.fidelity.approximate);
    assert_eq!(r.fidelity.missing.len(), 1);
    assert_eq!(
        r.fidelity.missing[0].codepoints,
        vec!["U+1F980".to_string()]
    );
    assert!(!r.fidelity.to_json().contains("faithful"));
}

// ---------------------------------------------------------------------------
// V07: blink intent preserved, stills sample a declared phase.
// ---------------------------------------------------------------------------
#[test]
fn blink_intent_preserved_and_phases_sampled() {
    let mut c = cell(1, 0, "X", 1);
    c.mods.blink = true;
    let screen = screen_from_leads(6, 2, vec![c]).expect("screen_from_leads succeeds");
    // Intent survives adaptation to canonical state.
    let frame = frame_from_screen(&screen, "qual");
    assert!(frame.get(1, 0).expect("frame.get(1, 0) is some").mods.blink);
    assert!(frame.to_json().contains("\"blink\":true"));

    let on = RenderProfile::vendored()
        .with_missing(MissingGlyphPolicy::Placeholder)
        .with_phase(BlinkPhase::On);
    let off = on.with_phase(BlinkPhase::Off);
    let a = render_screen(&screen, &on)
        .expect("render_screen(&screen, &on) succeeds")
        .png;
    let b = render_screen(&screen, &off)
        .expect("render_screen(&screen, &off) succeeds")
        .png;
    assert_ne!(a, b, "declared phases must sample different stills");
    // Off-phase cell region is pure background (ink + decorations gone).
    let img = decode(&b).expect("decode succeeds");
    let (cw, ch, pad, u) = (10u32, 21u32, 12u32, 2u32);
    let (x0, y0) = ((pad + cw) * u, pad * u);
    for y in y0..y0 + ch * u {
        for x in x0..x0 + cw * u {
            assert_eq!(img.get_pixel(x, y), &image::Rgb([0, 0, 0]), "({x},{y})");
        }
    }
    // SVG samples the same declared phase.
    let svg_on = tuiscotti::render::render_svg_phased(&frame, &on.to_profile(), BlinkPhase::On);
    let svg_off = tuiscotti::render::render_svg_phased(&frame, &off.to_profile(), BlinkPhase::Off);
    assert!(svg_on.contains('X'));
    assert!(!svg_off.contains('X'));
}
