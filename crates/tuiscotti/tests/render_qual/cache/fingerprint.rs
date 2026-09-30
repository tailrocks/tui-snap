//! F09 fingerprint soundness: one-field mutations each move the key.

use super::super::*;
use super::helpers::{
    ProfileParts, cell_variants, placeholder_rp, screen_at_origin, screen_of, screen_with_cursor,
    styled_lead,
};
use tuiscotti::profile::VENDORED_FALLBACK_FACES;
use tuiscotti::render::{RenderCache, screen_content_hash};
use tuiscotti::{CursorStyle, Rgb};

#[test]
fn one_field_screen_mutations_each_move_the_key() {
    let rp = placeholder_rp();
    let base = screen_of(styled_lead()).expect("screen_of succeeds");
    let base_key = RenderCache::key_for(&base, &rp);
    let base_hash = screen_content_hash(&base);

    let mut variants: Vec<(&str, Screen)> = Vec::new();
    for (label, cell) in cell_variants() {
        variants.push((label, screen_of(cell).expect("screen_of succeeds")));
    }
    // Dims.
    variants.push((
        "cols",
        screen_from_leads(5, 2, vec![styled_lead()]).expect("screen_from_leads succeeds"),
    ));
    variants.push((
        "rows",
        screen_from_leads(4, 3, vec![styled_lead()]).expect("screen_from_leads succeeds"),
    ));

    for (label, screen) in &variants {
        assert_ne!(
            screen_content_hash(screen),
            base_hash,
            "screen hash must move for {label}"
        );
        assert_ne!(
            RenderCache::key_for(screen, &rp),
            base_key,
            "cache key must move for {label}"
        );
    }
}

#[test]
fn one_field_cursor_mutations_each_move_the_key() {
    let rp = placeholder_rp();
    // Cursor fields, one at a time (validated cursor on a 4x2 grid).
    let cursor_base = Cursor::default();
    let cursor_variants = [
        (
            "cursor-x",
            Cursor {
                x: 1,
                ..cursor_base
            },
        ),
        (
            "cursor-y",
            Cursor {
                y: 1,
                ..cursor_base
            },
        ),
        (
            "cursor-visible",
            Cursor {
                visible: !cursor_base.visible,
                ..cursor_base
            },
        ),
        (
            "cursor-underline",
            Cursor {
                style: CursorStyle::Underline,
                ..cursor_base
            },
        ),
        (
            "cursor-bar",
            Cursor {
                style: CursorStyle::Bar,
                ..cursor_base
            },
        ),
        (
            "cursor-blinking",
            Cursor {
                blinking: !cursor_base.blinking,
                ..cursor_base
            },
        ),
    ];
    let base_cursor_key = RenderCache::key_for(
        &screen_with_cursor(cursor_base).expect("screen_with_cursor succeeds"),
        &rp,
    );
    for (label, cursor) in cursor_variants {
        assert_ne!(
            RenderCache::key_for(
                &screen_with_cursor(cursor).expect("screen_with_cursor succeeds"),
                &rp
            ),
            base_cursor_key,
            "cache key must move for {label}"
        );
    }

    // Origin is NOT rendering-relevant (the screen→frame adaptation drops
    // it): same pixels, same key.
    assert_eq!(
        RenderCache::key_for(
            &screen_at_origin(0, 0).expect("screen_at_origin succeeds"),
            &rp
        ),
        RenderCache::key_for(
            &screen_at_origin(7, -3).expect("screen_at_origin succeeds"),
            &rp
        ),
        "grid origin must not affect the key"
    );
}

#[test]
fn one_field_profile_mutations_each_move_the_key() {
    let screen = screen_of(styled_lead()).expect("screen_of succeeds");
    let base_key = RenderCache::key_for(
        &screen,
        &ProfileParts::base().build().expect("profile builds"),
    );

    let mut named: Vec<(&str, RenderProfile<'static>)> = Vec::new();
    let mut p = ProfileParts::base();
    p.name = "other".to_string();
    named.push(("name", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.font_px = 20.0;
    named.push(("font_px", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.cell_w = 12;
    named.push(("cell_w", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.cell_h = 22;
    named.push(("cell_h", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.pad = 0;
    named.push(("pad", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.scale = 1;
    named.push(("scale", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.palette.default_fg = Rgb::new(1, 1, 1);
    named.push(("palette-fg", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.palette.default_bg = Rgb::new(2, 2, 2);
    named.push(("palette-bg", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.cursor = CursorPolicy::Hide;
    named.push(("cursor", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.blink = BlinkPhase::Off;
    named.push(("blink", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.missing = MissingGlyphPolicy::Strict;
    named.push(("missing", p.build().expect("profile builds")));
    // Fallback chain: order AND per-face identity participate.
    let mut p = ProfileParts::base();
    p.fallbacks.reverse();
    named.push(("fallback-order", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.fallbacks[0].desc = "renamed-face";
    named.push(("fallback-desc", p.build().expect("profile builds")));

    for (label, rp) in &named {
        assert_ne!(
            RenderCache::key_for(&screen, rp),
            base_key,
            "cache key must move for profile field {label}"
        );
    }

    // Face pins cannot vary on a VALID profile: strict construction refuses
    // a pin that does not match the bytes, so no second key exists there.
    let err = RenderProfile::strict(
        "qual".to_string(),
        VENDORED_FACES,
        [
            "00",
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
        MissingGlyphPolicy::Placeholder,
        RENDERER_VERSION,
    )
    .expect_err("wrong face pin must refuse the profile");
    assert!(err.to_string().contains("sha256 mismatch"), "{err}");
}
