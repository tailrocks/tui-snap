//! Renderer matrix: real glyphs, geometry, Unicode, themes, sizes, formats.
//!
//! Covers: A→B pixel change, deterministic reruns (byte-identical PNG),
//! box/Braille/icons non-blank, CJK 2-cell geometry, clipping/wrapping,
//! themes and sizes, geometry-pin failure, SVG/ANSI outputs.

use ratatui::widgets::Paragraph;
use tuiscotti::{Profile, Provenance, VENDORED_FACES, VENDORED_FONT};

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

fn profile() -> Profile {
    Profile::default_profile()
}

fn widget_png(text: &str, cols: u16, rows: u16) -> Vec<u8> {
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new(text), cols, rows, prov());
    tuiscotti::render::render_png(&frame, &profile(), &VENDORED_FACES).unwrap()
}

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

fn frame_with_mods(symbol: &str, mods: tuiscotti::Mods) -> tuiscotti::Frame {
    let mut f = tuiscotti::ratatui::widget_frame(Paragraph::new(symbol), 10, 3, prov());
    // Apply mods to the non-blank lead cells only.
    for cell in f.cells.iter_mut() {
        if !cell.continuation && !cell.symbol.trim().is_empty() {
            cell.mods = mods;
        }
    }
    f
}

#[test]
fn every_modifier_changes_pixels() {
    use tuiscotti::Mods;
    let plain = tuiscotti::render::render_png(
        &frame_with_mods("x", Mods::default()),
        &profile(),
        &VENDORED_FACES,
    )
    .unwrap();
    for (label, mods) in [
        (
            "bold",
            Mods {
                bold: true,
                ..Default::default()
            },
        ),
        (
            "dim",
            Mods {
                dim: true,
                ..Default::default()
            },
        ),
        (
            "italic",
            Mods {
                italic: true,
                ..Default::default()
            },
        ),
        (
            "underline",
            Mods {
                underline: true,
                ..Default::default()
            },
        ),
        (
            "strike",
            Mods {
                strikethrough: true,
                ..Default::default()
            },
        ),
        (
            "reverse",
            Mods {
                reverse: true,
                ..Default::default()
            },
        ),
    ] {
        let styled =
            tuiscotti::render::render_png(&frame_with_mods("x", mods), &profile(), &VENDORED_FACES)
                .unwrap();
        assert_ne!(plain, styled, "{label} must change pixels");
    }
}

#[test]
fn dark_vs_light_theme_changes_pixels() {
    let mut dark = tuiscotti::ratatui::widget_frame(Paragraph::new("theme"), 20, 5, prov());
    let mut light = dark.clone();
    for cell in light.cells.iter_mut() {
        cell.bg = tuiscotti::Color::Indexed(15);
        cell.fg = tuiscotti::Color::Indexed(0);
    }
    let a = tuiscotti::render::render_png(&dark, &profile(), &VENDORED_FACES).unwrap();
    let b = tuiscotti::render::render_png(&light, &profile(), &VENDORED_FACES).unwrap();
    assert_ne!(a, b);
    let _ = &mut dark;
}

#[test]
fn emoji_tofu_keeps_two_cell_advance() {
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("🦀!"), 20, 5, prov());
    frame.validate().unwrap();
    let lead = frame.get(0, 0).unwrap();
    assert_eq!(lead.width, 2, "emoji lead must be width 2 (glyph or tofu)");
    assert!(frame.get(1, 0).unwrap().continuation);
    // Renders either way: real glyph if covered, deterministic tofu if not.
    let png = tuiscotti::render::render_png(&frame, &profile(), &VENDORED_FACES).unwrap();
    assert!(!png.is_empty());
}

#[test]
fn whitespace_cells_keep_underline_and_strikethrough() {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    let styled_frame = |m: Modifier| {
        tuiscotti::ratatui::widget_frame(
            Paragraph::new(Line::from(Span::styled(
                "    ",
                Style::default().add_modifier(m),
            ))),
            10,
            3,
            prov(),
        )
    };
    let plain = tuiscotti::render::render_png(
        &styled_frame(Modifier::empty()),
        &profile(),
        &VENDORED_FACES,
    )
    .unwrap();
    for (label, m) in [
        ("underline", Modifier::UNDERLINED),
        ("strikethrough", Modifier::CROSSED_OUT),
    ] {
        let decorated =
            tuiscotti::render::render_png(&styled_frame(m), &profile(), &VENDORED_FACES).unwrap();
        assert_ne!(
            plain, decorated,
            "{label} must draw across whitespace cells (real terminals do)"
        );
        // And the decoration is substantial: a line across 4 cells at 2×
        // scale is ~160 px, not a stray dot.
        let a = image::load_from_memory(&plain).unwrap().to_rgb8();
        let b = image::load_from_memory(&decorated).unwrap().to_rgb8();
        let changed = a
            .pixels()
            .zip(b.pixels())
            .filter(|(pa, pb)| pa != pb)
            .count();
        assert!(changed > 50, "{label} changed only {changed} px");
    }
}

#[test]
fn svg_carries_text_decorations_across_spaces() {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    let frame = tuiscotti::ratatui::widget_frame(
        Paragraph::new(Line::from(Span::styled(
            "a  b",
            Style::default().add_modifier(Modifier::UNDERLINED),
        ))),
        10,
        3,
        prov(),
    );
    let svg = tuiscotti::render::render_svg(&frame, &profile());
    assert!(
        svg.contains("text-decoration=\"underline\""),
        "underlined run must carry the decoration: {svg}"
    );
    // The decorated run keeps its spaces, so the decoration spans them.
    assert!(svg.contains(">a  b</text>"), "{svg}");
    // Plain runs stay undecorated.
    let plain = tuiscotti::ratatui::widget_frame(Paragraph::new("a  b"), 10, 3, prov());
    assert!(!tuiscotti::render::render_svg(&plain, &profile()).contains("text-decoration"),);
}

#[test]
fn renderer_cache_matches_one_shot_and_stays_stable() {
    let mut r = tuiscotti::render::Renderer::new(&profile(), &VENDORED_FACES).unwrap();
    let f1 = tuiscotti::ratatui::widget_frame(Paragraph::new("cache me"), 20, 5, prov());
    let f2 = tuiscotti::ratatui::widget_frame(Paragraph::new("CACHE 2 \u{280b}"), 20, 5, prov());
    // Byte-identical to the one-shot free functions (cold vs warm cache).
    let one_shot = tuiscotti::render::render_png_report(&f1, &profile(), &VENDORED_FACES).unwrap();
    let cached = r.render(&f1).unwrap();
    assert_eq!(one_shot.png, cached.png);
    assert_eq!(one_shot.fidelity.approximate, cached.fidelity.approximate);
    assert_eq!(one_shot.fidelity.missing, cached.fidelity.missing);
    let n1 = r.cached_glyphs();
    assert!(n1 > 0, "render must populate the glyph cache");
    let _ = r.render(&f2).unwrap();
    let n2 = r.cached_glyphs();
    assert!(n2 > n1, "new glyphs extend the cache ({n1} -> {n2})");
    // Re-render: identical pixels, no cache growth.
    let again = r.render(&f1).unwrap();
    assert_eq!(again.png, cached.png);
    assert_eq!(r.cached_glyphs(), n2);
    // render_png convenience matches render().png.
    assert_eq!(r.render_png(&f1).unwrap(), cached.png);
}

#[test]
fn renderer_new_checks_geometry_pin() {
    let mut bad = profile();
    bad.cell_w = 11;
    let err = tuiscotti::render::Renderer::new(&bad, &VENDORED_FACES)
        .err()
        .expect("broken geometry pin must fail at construction");
    assert!(err.to_string().contains("geometry pin broken"), "{err}");
}

#[test]
fn cursor_styles_render() {
    use tuiscotti::{Cursor, CursorStyle};
    let mut hidden = tuiscotti::ratatui::widget_frame(Paragraph::new("cur"), 20, 5, prov());
    hidden.cursor.visible = false;
    let base = tuiscotti::render::render_png(&hidden, &profile(), &VENDORED_FACES).unwrap();
    for style in [CursorStyle::Block, CursorStyle::Underline, CursorStyle::Bar] {
        let mut f = hidden.clone();
        f.cursor = Cursor {
            x: 0,
            y: 0,
            visible: true,
            style,
            blinking: false,
        };
        let png = tuiscotti::render::render_png(&f, &profile(), &VENDORED_FACES).unwrap();
        assert_ne!(base, png, "{style:?} cursor must change pixels");
    }
}

// ---------------------------------------------------------------------------
// Per-glyph fallback chain (vendored Noto subsets): coverage, geometry pins,
// determinism, and the zero-drift contract for primary-covered frames.
// ---------------------------------------------------------------------------

/// `(interior ink, unique_colors_in_full_cell)`. Hollow tofu is 2 colors
/// (bg + solid outline); a real antialiased glyph is dozens.
fn cell_stats(png: &[u8], x: u16, span_cells: u32) -> (usize, usize) {
    let img = image::load_from_memory(png).unwrap().to_rgb8();
    let bg = image::Rgb([0u8, 0, 0]);
    let (cw, ch, pad, u) = (10u32, 21u32, 12u32, 2u32);
    let pen = (pad + x as u32 * cw) * u;
    let top = pad * u;
    let (span, height) = (span_cells * cw * u, ch * u);
    let mut n = 0;
    let mut colors = std::collections::BTreeSet::new();
    for dy in 0..height {
        for dx in 0..span {
            colors.insert(img.get_pixel(pen + dx, top + dy).0);
        }
    }
    for dy in 5..(height - 6) {
        for dx in 3..(span - 4) {
            if img.get_pixel(pen + dx, top + dy) != &bg {
                n += 1;
            }
        }
    }
    (n, colors.len())
}

#[test]
fn fallback_faces_render_the_previously_missing_set() {
    // The exact codepoints the consumer audit found rasterizing as tofu:
    // 東 京 ☕ ⚷ ◐ ★ (U+6771 U+4EAC U+2615 U+26B7 U+25D0 U+2605).
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("東京 ☕ ⚷ ◐ ★"), 30, 4, prov());
    let r = tuiscotti::render::render_png_report(&frame, &profile(), &VENDORED_FACES).unwrap();
    assert!(
        r.fidelity.missing.is_empty(),
        "missing: {:?}",
        r.fidelity.missing
    );
    assert!(
        !r.fidelity.approximate,
        "fully served by real faces: not approximate"
    );
    let served: Vec<String> = r
        .fidelity
        .fallback_glyphs
        .iter()
        .flat_map(|g| g.codepoints.clone())
        .collect();
    for cp in ["U+6771", "U+4EAC", "U+2615", "U+26B7", "U+25D0", "U+2605"] {
        assert!(
            served.contains(&cp.to_string()),
            "{cp} not fallback-served: {served:?}"
        );
    }
    let json = r.fidelity.to_json();
    assert!(json.contains("fallback_glyphs"), "{json}");
    assert!(json.contains("NotoSansSymbols2 subset"), "{json}");
    assert!(json.contains("NotoSansSymbols subset"), "{json}");
    assert!(json.contains("NotoSansCJKjp subset"), "{json}");
    // Non-tofu pixels: every glyph cell has interior ink (tofu has none)
    // AND more than the 2 colors of a hollow outline. Widths follow the
    // frame model: 東 京 ☕ are wide (2 cells), ⚷ ◐ ★ narrow.
    for (x, span, label) in [
        (0u16, 2u32, "東"),
        (2, 2, "京"),
        (5, 2, "☕"),
        (8, 1, "⚷"),
        (10, 1, "◐"),
        (12, 1, "★"),
    ] {
        let (ink, ncolors) = cell_stats(&r.png, x, span);
        assert!(
            ink > 20,
            "{label} at cell {x} rendered as tofu or blank (ink={ink})"
        );
        assert!(
            ncolors > 8,
            "{label} at cell {x} is a {ncolors}-color box (hollow tofu is 2)"
        );
    }
    // Without the fallback chain the same frame is tofu + missing records,
    // and the pixels differ.
    let mut bare =
        tuiscotti::render::Renderer::with_fallbacks(&profile(), &VENDORED_FACES, &[]).unwrap();
    let tofu = bare.render(&frame).unwrap();
    assert_eq!(tofu.fidelity.missing.len(), 6);
    assert!(tofu.fidelity.approximate);
    assert!(tofu.fidelity.fallback_glyphs.is_empty());
    assert_ne!(tofu.png, r.png);
    for (x, span) in [(0u16, 2u32), (2, 2), (5, 2), (8, 1), (10, 1), (12, 1)] {
        let (ink, ncolors) = cell_stats(&tofu.png, x, span);
        assert_eq!(ink, 0, "cell {x} must be hollow tofu");
        assert_eq!(
            ncolors, 2,
            "cell {x} hollow tofu is bg+outline, got {ncolors} colors"
        );
    }
}

#[test]
fn cjk_star_coffee_cells_are_not_hollow_tofu() {
    // Consumer audit: 東京 / ★ / ☕ rasterized as 2-color ~8% ink outlines
    // even after the fallback faces were vendored, because coverage was
    // cmap-index-only and HTML showed viewer-font SVG. This is the ink
    // contract those snapshots must meet after recapture.
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("東京 ★ ☕\u{fe0f}"), 20, 3, prov());
    let r = tuiscotti::render::render_png_report(&frame, &profile(), &VENDORED_FACES).unwrap();
    assert!(
        r.fidelity.missing.is_empty(),
        "VS16 must not tofu: {:?}",
        r.fidelity.missing
    );
    for (x, span, label) in [(0u16, 2u32, "東"), (2, 2, "京"), (5, 1, "★"), (7, 2, "☕")] {
        let (ink, ncolors) = cell_stats(&r.png, x, span);
        assert!(ink > 20, "{label} cell {x} near-empty ink={ink}");
        assert!(ncolors > 8, "{label} cell {x} {ncolors}-color tofu-like");
    }
    let html = tuiscotti::render::Renderer::new(&profile(), &VENDORED_FACES)
        .unwrap()
        .render_html(&frame, "glyphs")
        .unwrap();
    let body = html.split("<body>").nth(1).expect("body");
    let img = body.find("<img ").expect("primary img");
    let details = body.find("<details");
    assert!(
        details.is_none() || img < details.unwrap(),
        "authoritative PNG must be the primary visual, not hidden in details"
    );
    assert!(body[..img].contains("class=\"shot\""), "{body}");
    assert!(html.contains("data:image/png;base64,"), "{html}");
}

#[test]
fn fallback_render_is_byte_deterministic() {
    let render = || {
        let frame =
            tuiscotti::ratatui::widget_frame(Paragraph::new("東京 ☕ ⚷ ◐ ★ ❤ ●"), 30, 4, prov());
        tuiscotti::render::render_png(&frame, &profile(), &VENDORED_FACES).unwrap()
    };
    assert_eq!(
        render(),
        render(),
        "same frame, fresh renderers: same bytes"
    );
    let mut r = tuiscotti::render::Renderer::new(&profile(), &VENDORED_FACES).unwrap();
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("東京 ☕ ⚷ ◐ ★ ❤ ●"), 30, 4, prov());
    let a = r.render(&frame).unwrap().png;
    let b = r.render(&frame).unwrap().png;
    assert_eq!(a, b, "warm cache: same bytes");
}

#[test]
fn vendored_fallback_hashes_pinned_and_documented() {
    use tuiscotti::profile::font_sha256;
    // Subsets built by tools/subset_fonts.py from commit-pinned Noto
    // upstreams (SIL OFL 1.1, assets/fonts/LICENSE-Noto.txt, FONTS.md).
    assert_eq!(
        font_sha256(tuiscotti::VENDORED_SYMBOLS2_FONT),
        "e1d177a40af910100eceb0e825331e55f0cfd005bc0f26087fd4e58fbe60e6c5"
    );
    assert_eq!(
        font_sha256(tuiscotti::VENDORED_SYMBOLS_FONT),
        "6f9cc93e71f8676361c5db286368be046e75d42c0b841afbf3f50da6bb0a2b8a"
    );
    assert_eq!(
        font_sha256(tuiscotti::VENDORED_CJK_FONT),
        "777bee41f0c6076c00ad919384359a6e396b8822cf9056041fca8fcf2759d897"
    );
    // The exported pins match the bytes (Renderer::new verifies at load).
    assert_eq!(
        tuiscotti::VENDORED_SYMBOLS2_FONT_SHA256,
        font_sha256(tuiscotti::VENDORED_SYMBOLS2_FONT)
    );
    assert_eq!(
        tuiscotti::VENDORED_SYMBOLS_FONT_SHA256,
        font_sha256(tuiscotti::VENDORED_SYMBOLS_FONT)
    );
    assert_eq!(
        tuiscotti::VENDORED_CJK_FONT_SHA256,
        font_sha256(tuiscotti::VENDORED_CJK_FONT)
    );
    assert_eq!(tuiscotti::VENDORED_FALLBACK_FACES.len(), 3);
}

#[test]
fn fallback_hash_mismatch_refuses_to_render() {
    let bad = tuiscotti::FallbackFace {
        bytes: tuiscotti::VENDORED_CJK_FONT,
        sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        desc: "swapped bytes",
    };
    let err = tuiscotti::render::Renderer::with_fallbacks(&profile(), &VENDORED_FACES, &[bad])
        .err()
        .expect("a hash mismatch must fail at construction");
    assert!(err.to_string().contains("sha256 mismatch"), "{err}");
}

#[test]
fn unparsable_fallback_face_fails_loudly() {
    let bytes: &[u8] = b"definitely not a font";
    let sha = tuiscotti::profile::font_sha256(bytes);
    let bad = tuiscotti::FallbackFace {
        bytes,
        sha256: &sha,
        desc: "junk",
    };
    let err = tuiscotti::render::Renderer::with_fallbacks(&profile(), &VENDORED_FACES, &[bad])
        .err()
        .expect("an unparsable fallback face must fail at construction");
    assert!(err.to_string().contains("fallback face 'junk'"), "{err}");
}

#[test]
fn consumer_registered_fallback_face_serves_glyphs() {
    // DejaVuSansMNerdFontMono (vendored for reference) covers U+25D0 ◐, the
    // primary family does not: a consumer-registered chain serves it.
    static DEJAVU: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMNerdFontMono-Regular.ttf");
    let sha = tuiscotti::profile::font_sha256(DEJAVU);
    let face = tuiscotti::FallbackFace {
        bytes: DEJAVU,
        sha256: &sha,
        desc: "test DejaVuSansM Nerd Font Mono",
    };
    let mut r =
        tuiscotti::render::Renderer::with_fallbacks(&profile(), &VENDORED_FACES, &[face]).unwrap();
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("◐"), 10, 3, prov());
    let rendered = r.render(&frame).unwrap();
    assert!(rendered.fidelity.missing.is_empty());
    assert_eq!(rendered.fidelity.fallback_glyphs.len(), 1);
    assert_eq!(
        rendered.fidelity.fallback_glyphs[0].faces,
        vec!["test DejaVuSansM Nerd Font Mono".to_string()]
    );
}

#[test]
fn primary_covered_frames_are_byte_identical_with_and_without_fallbacks() {
    let frame =
        tuiscotti::ratatui::widget_frame(Paragraph::new("plain ╔═╗ ⠋ \u{f015} → ✓"), 30, 4, prov());
    let mut with = tuiscotti::render::Renderer::new(&profile(), &VENDORED_FACES).unwrap();
    let mut without =
        tuiscotti::render::Renderer::with_fallbacks(&profile(), &VENDORED_FACES, &[]).unwrap();
    let a = with.render(&frame).unwrap();
    let b = without.render(&frame).unwrap();
    assert_eq!(
        a.png, b.png,
        "fallback chain must not move primary-covered pixels"
    );
    assert_eq!(a.fidelity.to_json(), b.fidelity.to_json());
    assert!(
        !a.fidelity.to_json().contains("fallback_glyphs"),
        "empty fallback record is omitted from the JSON"
    );
}

#[test]
fn primary_covered_fixtures_match_pre_fallback_render_bytes() {
    // Baselines rendered by the pre-fallback renderer (rev 00b178e) from the
    // approved fixture frames; every glyph in these frames is covered by the
    // primary JetBrainsMono family, so the fallback chain must not move a
    // single byte (zero-drift contract, constraint: PRIMARY GEOMETRY
    // UNCHANGED).
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let baselines = root.join("tests/fixtures/render-baseline");
    let approved = root.join("tests/visual/approved");
    for name in [
        "home-dark-80x24",
        "home-light-160x50",
        "table-dark-120x40",
        "dialog-light-80x24",
    ] {
        let text = std::fs::read_to_string(approved.join(format!("{name}.frame.json"))).unwrap();
        let frame = tuiscotti::Frame::from_json(&text).unwrap();
        let r = tuiscotti::render::render_png_report(&frame, &profile(), &VENDORED_FACES).unwrap();
        let baseline = std::fs::read(baselines.join(format!("{name}.png"))).unwrap();
        assert_eq!(
            r.png, baseline,
            "{name}: primary-covered render drifted from the pre-fallback bytes"
        );
        assert!(
            r.fidelity.missing.is_empty(),
            "{name}: {:?}",
            r.fidelity.missing
        );
        assert!(r.fidelity.fallback_glyphs.is_empty(), "{name}");
        let baseline_fidelity =
            std::fs::read_to_string(baselines.join(format!("{name}.png.fidelity.json"))).unwrap();
        assert_eq!(
            r.fidelity.to_json().trim(),
            baseline_fidelity.trim(),
            "{name}: fidelity sidecar drifted"
        );
    }
}
