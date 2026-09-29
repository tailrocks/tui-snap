//! Renderer matrix: glyphs, geometry, Unicode, faces, fidelity (split from `render.rs`; shared helpers live in the root).

use super::{frame_with_mods, profile, prov, widget_png};
use ratatui::widgets::Paragraph;
use tuiscotti::{VENDORED_FACES, VENDORED_FONT};

#[test]
fn glyph_change_changes_pixels() {
    // THE regression test the old block renderer failed by construction.
    let a = widget_png("A", 20, 5);
    let b = widget_png("B", 20, 5);
    assert_ne!(a, b, "A vs B must differ at the pixel level");
}

#[test]
fn deterministic_reruns_are_byte_identical() {
    let a = widget_png("hello determinism ╔═╗ ⠋", 30, 6);
    let b = widget_png("hello determinism ╔═╗ ⠋", 30, 6);
    assert_eq!(a, b);
}

#[test]
fn box_braille_icons_render_ink() {
    let png = widget_png("╔═╗ █ ⠋ \u{f015}", 20, 5);
    let img = image::load_from_memory(&png).unwrap().to_rgb8();
    let bg = image::Rgb([0u8, 0, 0]);
    let ink = img.pixels().filter(|p| **p != bg).count();
    assert!(ink > 200, "expected real glyph ink, got {ink} pixels");
}

#[test]
fn cjk_keeps_two_cell_geometry() {
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("日本"), 20, 5, prov());
    frame.validate().unwrap();
    let lead = frame.get(0, 0).unwrap();
    assert_eq!(lead.width, 2, "CJK lead must be width 2");
    let cont = frame.get(1, 0).unwrap();
    assert!(cont.continuation && cont.width == 0);
    // Renders without error regardless of font coverage: 日本 is served by
    // the vendored CJK fallback face; uncovered codepoints would draw tofu.
    let png = tuiscotti::render::render_png(&frame, &profile(), &VENDORED_FACES).unwrap();
    assert!(!png.is_empty());
}

#[test]
fn clipping_and_wrapping_match_terminal() {
    // Ratatui clips overlong lines; the adapter must preserve the clip.
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("0123456789ABCDEF"), 10, 3, prov());
    assert_eq!(frame.get(9, 0).unwrap().symbol, "9");
    assert!(frame.get(10, 0).is_none());
}

#[test]
fn themes_change_pixels_and_sizes_change_dims() {
    let dark = widget_png("theme", 20, 5);
    assert!(!dark.is_empty());
    let small = widget_png("theme", 20, 5);
    let wide = widget_png("theme", 40, 5);
    assert_ne!(small.len(), wide.len());
    let (w1, _) = profile().image_size(20, 5);
    let img = image::load_from_memory(&wide).unwrap().to_rgb8();
    assert_eq!(img.width(), (40 * 10 + 24) * 2);
    assert_eq!(w1, (20 * 10 + 24) * 2);
}

#[test]
fn geometry_pin_fails_loudly() {
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("x"), 10, 3, prov());
    let mut bad = profile();
    bad.cell_w = 11;
    let err = tuiscotti::render::render_png(&frame, &bad, &VENDORED_FACES).unwrap_err();
    assert!(err.to_string().contains("geometry pin broken"), "{err}");
}

#[test]
fn svg_and_ansi_outputs_carry_content() {
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("hi svg"), 20, 5, prov());
    let svg = tuiscotti::render::render_svg(&frame, &profile());
    assert!(svg.contains("<svg") && svg.contains("hi svg"));
    let ansi = tuiscotti::render::ansi_dump(&frame);
    assert!(ansi.contains("hi svg"));
    assert_eq!(frame.text().trim(), "hi svg");
}

#[test]
fn font_hash_pinned_and_documented() {
    let p = profile();
    assert_eq!(p.font_sha256.len(), 64);
    // JetBrainsMonoNerdFontMono-Regular.ttf (SIL OFL 1.1, see FONTS.md).
    assert_eq!(
        p.font_sha256,
        "f2a5ea6cfab397445ffab00c0370927b66d61e560a05db5db271b42006381c1a"
    );
}

#[test]
fn bold_and_italic_use_real_faces_not_faux() {
    use tuiscotti::Mods;
    let bold = Mods {
        bold: true,
        ..Default::default()
    };
    let italic = Mods {
        italic: true,
        ..Default::default()
    };
    let real =
        tuiscotti::render::render_png(&frame_with_mods("real", bold), &profile(), &VENDORED_FACES)
            .unwrap();
    // Single-face chain: bold falls back to the faux double-strike, which
    // must differ from the real Bold face.
    let faux = tuiscotti::render::render_png(
        &frame_with_mods("real", bold),
        &profile(),
        &tuiscotti::FontFaces::single(VENDORED_FONT),
    )
    .unwrap();
    assert_ne!(real, faux, "real Bold face must differ from faux bold");
    let real_it = tuiscotti::render::render_png(
        &frame_with_mods("real", italic),
        &profile(),
        &VENDORED_FACES,
    )
    .unwrap();
    let faux_it = tuiscotti::render::render_png(
        &frame_with_mods("real", italic),
        &profile(),
        &tuiscotti::FontFaces::single(VENDORED_FONT),
    )
    .unwrap();
    assert_ne!(real_it, faux_it, "real Italic face must differ from faux");
}

#[test]
fn fidelity_reports_missing_glyphs_exactly() {
    // 🦀 (U+1F980) is not covered by the vendored family (see FONTS.md).
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("ok 🦀"), 20, 5, prov());
    let r = tuiscotti::render::render_png_report(&frame, &profile(), &VENDORED_FACES).unwrap();
    assert!(
        r.fidelity.approximate,
        "uncovered glyph must mark approximate"
    );
    assert_eq!(r.fidelity.missing.len(), 1);
    let m = &r.fidelity.missing[0];
    assert_eq!(m.symbol, "🦀");
    assert_eq!(m.codepoints, vec!["U+1F980".to_string()]);
    assert_eq!((m.x, m.y), (3, 0));
    assert!(r.fidelity.to_json().contains("U+1F980"));
    // Fully covered text: exact, nothing missing, no fallback faces engaged.
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("plain ╔═╗ ⠋"), 20, 5, prov());
    let r = tuiscotti::render::render_png_report(&frame, &profile(), &VENDORED_FACES).unwrap();
    assert!(!r.fidelity.approximate);
    assert!(r.fidelity.missing.is_empty());
    assert!(r.fidelity.faces_fell_back.is_empty());
    assert!(r.fidelity.fallback_glyphs.is_empty());
}

#[test]
fn broken_styled_face_falls_back_and_is_recorded() {
    let faces = tuiscotti::FontFaces {
        regular: VENDORED_FONT,
        bold: b"not a font",
        italic: VENDORED_FONT,
        bold_italic: VENDORED_FONT,
    };
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("fallback"), 20, 5, prov());
    let r = tuiscotti::render::render_png_report(&frame, &profile(), &faces).unwrap();
    assert_eq!(r.fidelity.faces_fell_back, vec!["bold".to_string()]);
    assert!(r.fidelity.approximate);
}
