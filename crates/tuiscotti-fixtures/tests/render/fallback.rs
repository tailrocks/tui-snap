//! Fallback chain + approved-PNG spot checks (split from `render.rs`; shared helpers live in the root).

use super::{profile, prov};
use ratatui::widgets::Paragraph;
use tuiscotti::VENDORED_FACES;

// ---------------------------------------------------------------------------
// Per-glyph fallback chain (vendored Noto subsets): coverage, geometry pins,
// determinism, and the zero-drift contract for primary-covered frames.
// ---------------------------------------------------------------------------

/// `(interior ink, unique_colors_in_full_cell)`. Hollow tofu is 2 colors
/// (bg + solid outline); a real antialiased glyph is dozens.
fn cell_stats(png: &[u8], x: u16, span_cells: u32) -> Result<(usize, usize), image::ImageError> {
    let img = image::load_from_memory(png)?.to_rgb8();
    let bg = image::Rgb([0u8, 0, 0]);
    let (cw, ch, pad, u) = (10u32, 21u32, 12u32, 2u32);
    let pen = (pad + u32::from(x) * cw) * u;
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
    Ok((n, colors.len()))
}

#[test]
fn fallback_faces_render_the_previously_missing_set() {
    // The exact codepoints the consumer audit found rasterizing as tofu:
    // 東 京 ☕ ⚷ ◐ ★ (U+6771 U+4EAC U+2615 U+26B7 U+25D0 U+2605).
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("東京 ☕ ⚷ ◐ ★"), 30, 4, prov());
    let r =
        tuiscotti::render::render_png_report(&frame, &profile(), &VENDORED_FACES).expect("render");
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
        let (ink, ncolors) = cell_stats(&r.png, x, span).expect("decode png");
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
    let mut bare = tuiscotti::render::Renderer::with_fallbacks(&profile(), &VENDORED_FACES, &[])
        .expect("renderer");
    let tofu = bare.render(&frame).expect("render");
    assert_eq!(tofu.fidelity.missing.len(), 6);
    assert!(tofu.fidelity.approximate);
    assert!(tofu.fidelity.fallback_glyphs.is_empty());
    assert_ne!(tofu.png, r.png);
    for (x, span) in [(0u16, 2u32), (2, 2), (5, 2), (8, 1), (10, 1), (12, 1)] {
        let (ink, ncolors) = cell_stats(&tofu.png, x, span).expect("decode png");
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
    let frame =
        tuiscotti::ratatui::widget_frame(Paragraph::new("東京 ★ ☕\u{fe0f}"), 20, 3, prov());
    let r =
        tuiscotti::render::render_png_report(&frame, &profile(), &VENDORED_FACES).expect("render");
    assert!(
        r.fidelity.missing.is_empty(),
        "VS16 must not tofu: {:?}",
        r.fidelity.missing
    );
    for (x, span, label) in [(0u16, 2u32, "東"), (2, 2, "京"), (5, 1, "★"), (7, 2, "☕")] {
        let (ink, ncolors) = cell_stats(&r.png, x, span).expect("decode png");
        assert!(ink > 20, "{label} cell {x} near-empty ink={ink}");
        assert!(ncolors > 8, "{label} cell {x} {ncolors}-color tofu-like");
    }
    let html = tuiscotti::render::Renderer::new(&profile(), &VENDORED_FACES)
        .expect("renderer")
        .render_html(&frame, "glyphs")
        .expect("render html");
    let body = html.split("<body>").nth(1).expect("body");
    let img = body.find("<img ").expect("primary img");
    let details = body.find("<details");
    assert!(
        details.is_none() || img < details.expect("details index"),
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
        tuiscotti::render::render_png(&frame, &profile(), &VENDORED_FACES).expect("render")
    };
    assert_eq!(
        render(),
        render(),
        "same frame, fresh renderers: same bytes"
    );
    let mut r = tuiscotti::render::Renderer::new(&profile(), &VENDORED_FACES).expect("renderer");
    let frame =
        tuiscotti::ratatui::widget_frame(Paragraph::new("東京 ☕ ⚷ ◐ ★ ❤ ●"), 30, 4, prov());
    let a = r.render(&frame).expect("render").png;
    let b = r.render(&frame).expect("render").png;
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
        .expect_err("a hash mismatch must fail at construction");
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
        .expect_err("an unparsable fallback face must fail at construction");
    assert!(err.to_string().contains("fallback face 'junk'"), "{err}");
}

#[test]
fn consumer_registered_fallback_face_serves_glyphs() {
    // DejaVuSansMNerdFontMono (vendored for reference) covers U+25D0 ◐, the
    // primary family does not: a consumer-registered chain serves it.
    static DEJAVU: &[u8] =
        include_bytes!("../../../../assets/fonts/DejaVuSansMNerdFontMono-Regular.ttf");
    let sha = tuiscotti::profile::font_sha256(DEJAVU);
    let face = tuiscotti::FallbackFace {
        bytes: DEJAVU,
        sha256: &sha,
        desc: "test DejaVuSansM Nerd Font Mono",
    };
    let mut r = tuiscotti::render::Renderer::with_fallbacks(&profile(), &VENDORED_FACES, &[face])
        .expect("renderer");
    let frame = tuiscotti::ratatui::widget_frame(Paragraph::new("◐"), 10, 3, prov());
    let rendered = r.render(&frame).expect("render");
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
    let mut with = tuiscotti::render::Renderer::new(&profile(), &VENDORED_FACES).expect("renderer");
    let mut without = tuiscotti::render::Renderer::with_fallbacks(&profile(), &VENDORED_FACES, &[])
        .expect("renderer");
    let a = with.render(&frame).expect("render");
    let b = without.render(&frame).expect("render");
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
fn approved_pngs_are_exactly_what_the_current_renderer_emits() {
    // The stale `tests/fixtures/render-baseline` copies were deleted: the
    // visual approvals are the single source of truth. Spot-check that fresh
    // renders of approved frames reproduce the approved PNG bytes exactly.
    // Every glyph in these frames is covered by the primary JetBrainsMono
    // family, so the fallback chain must stay out (zero-drift contract,
    // constraint: PRIMARY GEOMETRY UNCHANGED).
    let approved = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/visual/approved");
    for name in [
        "home-dark-80x24",
        "home-light-160x50",
        "table-dark-120x40",
        "dialog-light-80x24",
    ] {
        let text = std::fs::read_to_string(approved.join(format!("{name}.frame.json")))
            .expect("approved frame");
        let frame = tuiscotti::Frame::from_json(&text).expect("parse frame");
        let r = tuiscotti::render::render_png_report(&frame, &profile(), &VENDORED_FACES)
            .expect("render");
        let committed = std::fs::read(approved.join(format!("{name}.png"))).expect("approved png");
        assert_eq!(
            r.png, committed,
            "{name}: fresh render drifted from the approved PNG bytes"
        );
        assert!(
            r.fidelity.missing.is_empty(),
            "{name}: {:?}",
            r.fidelity.missing
        );
        assert!(r.fidelity.fallback_glyphs.is_empty(), "{name}");
    }
}
