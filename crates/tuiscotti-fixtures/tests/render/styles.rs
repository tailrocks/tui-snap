//! Modifiers, themes, decorations, cache, cursor (split from `render.rs`; shared helpers live in the root).

use super::{frame_with_mods, profile, prov};
use ratatui::widgets::Paragraph;
use tuiscotti::VENDORED_FACES;

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
