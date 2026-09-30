//! Render identity: real profile inputs + primary-face verification.
//!
//! [`render_identity`] covers the strict content hash (face pins, fallback
//! chain, geometry, scale, palette, policies, version): scale-2-vs-1 and
//! every other sampled leg move the string. The Insta sample path verifies
//! the primary faces against the vendored pins before rendering.

use super::helpers::styled_screen;
use tuiscotti_core::frame::Rgb;
use tuiscotti_insta::assert::{
    render_identity, render_identity_for, render_sample, render_sample_with_faces,
};
use tuiscotti_render::profile::{
    BlinkPhase, CursorPolicy, FallbackFace, FontFaces, MissingGlyphPolicy, PalettePolicy,
    RENDERER_VERSION, RenderProfile, VENDORED_FACES, VENDORED_FALLBACK_FACES,
    VENDORED_FONT_BOLD_ITALIC_SHA256, VENDORED_FONT_BOLD_SHA256, VENDORED_FONT_ITALIC_SHA256,
    VENDORED_FONT_SHA256, font_sha256,
};

/// Second font fixture: `DejaVuSansM` Nerd Font Mono, vendored for reference.
static SECOND_FACE: &[u8] =
    include_bytes!("../../../../assets/fonts/DejaVuSansMNerdFontMono-Regular.ttf");

/// Strict-profile parts for identity tests (vendored defaults, one field
/// varied per case).
struct IdParts<'a> {
    scale: u32,
    cell_w: u32,
    palette_fg: Rgb,
    faces: FontFaces<'a>,
    pins: [&'a str; 4],
    fallbacks: Vec<FallbackFace<'a>>,
}

impl IdParts<'static> {
    fn base() -> Self {
        Self {
            scale: 2,
            cell_w: 10,
            palette_fg: Rgb::new(0xd0, 0xd0, 0xd0),
            faces: VENDORED_FACES,
            pins: [
                VENDORED_FONT_SHA256,
                VENDORED_FONT_BOLD_SHA256,
                VENDORED_FONT_ITALIC_SHA256,
                VENDORED_FONT_BOLD_ITALIC_SHA256,
            ],
            fallbacks: VENDORED_FALLBACK_FACES.to_vec(),
        }
    }
}

impl<'a> IdParts<'a> {
    fn build(self) -> Result<RenderProfile<'a>, String> {
        RenderProfile::strict(
            "id".to_string(),
            self.faces,
            self.pins,
            self.fallbacks,
            16.0,
            self.cell_w,
            21,
            12,
            self.scale,
            PalettePolicy {
                default_fg: self.palette_fg,
                ..PalettePolicy::xterm()
            },
            CursorPolicy::Show,
            BlinkPhase::On,
            MissingGlyphPolicy::Placeholder,
            RENDERER_VERSION,
        )
        .map_err(|e| e.to_string())
    }
}

#[test]
fn default_render_identity_names_profile_version_and_pins() {
    let id = render_identity();
    assert!(id.starts_with("tuiscotti-default/rv"), "{id}");
    assert!(id.contains("/straight-rgba/profile-"), "{id}");
    let hash = id.rsplit("profile-").next().expect("profile hash");
    assert_eq!(hash.len(), 64, "{id}");
    assert!(
        hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "profile hash must be hex: {id}"
    );
}

#[test]
fn render_identity_represents_real_profile_inputs() {
    let base = render_identity_for(&IdParts::base().build().expect("base builds"));
    // Scale 2 vs 1: same screen, different strings.
    let scale1 = IdParts {
        scale: 1,
        ..IdParts::base()
    }
    .build()
    .expect("scale-1 builds");
    assert_ne!(
        render_identity_for(&scale1),
        base,
        "scale must move the identity"
    );
    // Face hash: second face + true pin (valid profile, moved identity).
    let second_sha = font_sha256(SECOND_FACE);
    let swapped = IdParts {
        faces: FontFaces {
            regular: SECOND_FACE,
            ..VENDORED_FACES
        },
        pins: [
            &second_sha,
            VENDORED_FONT_BOLD_SHA256,
            VENDORED_FONT_ITALIC_SHA256,
            VENDORED_FONT_BOLD_ITALIC_SHA256,
        ],
        ..IdParts::base()
    }
    .build()
    .expect("second-face profile builds");
    assert_ne!(
        render_identity_for(&swapped),
        base,
        "face hash must move the identity"
    );
    // Geometry.
    let wide = IdParts {
        cell_w: 12,
        ..IdParts::base()
    }
    .build()
    .expect("wide profile builds");
    assert_ne!(
        render_identity_for(&wide),
        base,
        "geometry must move the identity"
    );
    // Palette.
    let tinted = IdParts {
        palette_fg: Rgb::new(1, 2, 3),
        ..IdParts::base()
    }
    .build()
    .expect("tinted profile builds");
    assert_ne!(
        render_identity_for(&tinted),
        base,
        "palette must move the identity"
    );
    // Fallback chain face: same slot, different bytes + true pin.
    let mut chain = VENDORED_FALLBACK_FACES.to_vec();
    chain[0] = FallbackFace {
        bytes: SECOND_FACE,
        sha256: &second_sha,
        desc: chain[0].desc,
    };
    let refallback = IdParts {
        fallbacks: chain,
        ..IdParts::base()
    }
    .build()
    .expect("refallback profile builds");
    assert_ne!(
        render_identity_for(&refallback),
        base,
        "fallback sha must move the identity"
    );
}

#[test]
fn insta_path_verifies_primary_face_pins() {
    let screen = styled_screen().expect("valid test screen");
    render_sample(&screen).expect("vendored faces render");
    let tampered = FontFaces {
        regular: SECOND_FACE,
        ..VENDORED_FACES
    };
    let err =
        render_sample_with_faces(&screen, &tampered).expect_err("swapped primary must refuse");
    assert!(err.to_string().contains("sha256 mismatch"), "{err}");
}
