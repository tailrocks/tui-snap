//! F09 fingerprint soundness: one-field mutations each move the key.

use super::super::*;
use super::helpers::{
    ProfileParts, cell_variants, placeholder_rp, screen_at_origin, screen_of, screen_with_cursor,
    styled_lead,
};
use tuiscotti::profile::{VENDORED_FALLBACK_FACES, font_sha256};
use tuiscotti::render::{RenderCache, screen_content_hash};
use tuiscotti::{CursorStyle, FallbackFace, FontFaces, Rgb};

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

    // Face pins cannot LIE on a valid profile: strict construction refuses
    // a pin that does not match the bytes (positive key-move for TRUE pin
    // changes lives in face_pin_moves_the_key_on_valid_profiles).
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

/// Second font fixture: `DejaVuSansM` Nerd Font Mono, vendored for reference
/// (distinct bytes from every styled face of the default family).
static SECOND_FACE: &[u8] =
    include_bytes!("../../../../../assets/fonts/DejaVuSansMNerdFontMono-Regular.ttf");

#[test]
fn face_pin_moves_the_key_on_valid_profiles() {
    let screen = screen_of(styled_lead()).expect("screen_of succeeds");
    let base = RenderCache::key_for(
        &screen,
        &ProfileParts::base().build().expect("profile builds"),
    );
    let second_sha = font_sha256(SECOND_FACE);
    // Sanity: the fixture really is a second face, not a copy.
    for pin in [
        VENDORED_FONT_SHA256,
        VENDORED_FONT_BOLD_SHA256,
        VENDORED_FONT_ITALIC_SHA256,
        VENDORED_FONT_BOLD_ITALIC_SHA256,
    ] {
        assert_ne!(second_sha, pin);
    }
    // Each styled slot swapped to the second face WITH its true pin: the
    // profile stays valid (strict construction accepts it) and the key moves.
    for slot in 0..4 {
        let mut p = ProfileParts::base();
        let mut faces = VENDORED_FACES;
        match slot {
            0 => faces.regular = SECOND_FACE,
            1 => faces.bold = SECOND_FACE,
            2 => faces.italic = SECOND_FACE,
            _ => faces.bold_italic = SECOND_FACE,
        }
        p.faces = faces;
        p.pins[slot] = second_sha.clone();
        let rp = p.build().expect("second-face profile builds");
        assert_ne!(
            RenderCache::key_for(&screen, &rp),
            base,
            "face slot {slot} must move the key"
        );
    }
    // Validity is preserved: the second face under a WRONG pin still refuses.
    let mut p = ProfileParts::base();
    p.faces = FontFaces {
        regular: SECOND_FACE,
        ..VENDORED_FACES
    };
    p.pins[0] = VENDORED_FONT_SHA256.to_string();
    let err = p
        .build()
        .expect_err("wrong pin for second face must refuse");
    assert!(err.contains("sha256 mismatch"), "{err}");
}

#[test]
fn fallback_face_sha_moves_the_key() {
    let screen = screen_of(styled_lead()).expect("screen_of succeeds");
    let base = RenderCache::key_for(
        &screen,
        &ProfileParts::base().build().expect("profile builds"),
    );
    let second_sha = font_sha256(SECOND_FACE);
    // Same chain slot, same desc, different bytes + true pin: the sha alone
    // moves the key.
    let mut p = ProfileParts::base();
    let desc = p.fallbacks[0].desc;
    p.fallbacks[0] = FallbackFace {
        bytes: SECOND_FACE,
        sha256: &second_sha,
        desc,
    };
    let rp = p.build().expect("second-fallback profile builds");
    assert_ne!(
        RenderCache::key_for(&screen, &rp),
        base,
        "fallback sha must move the key"
    );
    // Dropping a chain face moves the key too (chain length participates).
    let mut p = ProfileParts::base();
    p.fallbacks.pop();
    let rp = p.build().expect("shorter chain builds");
    assert_ne!(
        RenderCache::key_for(&screen, &rp),
        base,
        "fallback count must move the key"
    );
}

#[test]
fn cache_key_hex_is_pinned() {
    // Both version inputs (CACHE_FINGERPRINT_VERSION, renderer_version) are
    // compile-time pins: no two VALID profiles can differ in version (a wrong
    // version refuses strict construction), so version participation is pinned
    // by this golden instead — dropping the version bytes from the pre-image
    // moves it. Key encodings are explicit bytes (never Debug), so this is
    // stable across platforms and toolchains.
    let screen = screen_of(styled_lead()).expect("screen_of succeeds");
    let rp = ProfileParts::base().build().expect("profile builds");
    assert_eq!(
        RenderCache::key_for(&screen, &rp).hex(),
        "43a9644309db75ebd01db2baebbb6af1ee63266adc19d583d67041840d983d59"
    );
}
