//! Same-sample canonical+PNG binding: repro, determinism, strict gate.

use std::path::Path;

use super::helpers::{styled_screen, write_binary_snap, write_text_snap};
use tuiscotti_core::screen::canonical_string;
use tuiscotti_insta::assert::{
    Location, check_consistent, frame_from_screen, generation_id, png_tag_generation, render_sample,
};
use tuiscotti_render::diff::{AlphaPolicy, compare_png_with_alpha};
use tuiscotti_render::profile::{Profile, VENDORED_FACES};
use tuiscotti_render::render::Renderer;

#[test]
fn canonical_identical_render_different_repro() {
    let screen = styled_screen().expect("valid test screen");
    let canonical = canonical_string(&screen);

    // Same sample through the pinned pipeline.
    let sample = render_sample(&screen).expect("render sample");

    // Same screen through a DIFFERENT profile (half the raster scale).
    let frame = frame_from_screen(&screen);
    let other = Profile {
        scale: 1,
        ..Profile::default_profile()
    };
    let mut renderer =
        Renderer::new(&other, &VENDORED_FACES).expect("renderer with vendored faces");
    let alt_image = renderer.render(&frame).expect("render frame");

    // Canonical text is identical (same screen) ...
    assert_eq!(canonical, canonical_string(&screen));
    // ... but the pixels differ, and the decoded-pixel comparison — the same
    // function the PNG comparator delegates to — fails loudly.
    assert_ne!(sample.png, alt_image.png);
    let verdict = compare_png_with_alpha(&sample.png, &alt_image.png, AlphaPolicy::StraightRgba)
        .expect("compare pngs");
    assert!(
        !verdict.pixels_equal,
        "different renders must not compare equal"
    );
}

#[test]
fn same_sample_renders_deterministically() {
    let screen = styled_screen().expect("valid test screen");
    let first = render_sample(&screen).expect("render sample");
    let second = render_sample(&screen).expect("render sample");
    assert_eq!(first.canonical, second.canonical);
    assert_eq!(first.png, second.png);
    assert_eq!(first.ansi, second.ansi);
    assert_eq!(first.txt, second.txt);
    assert_eq!(first.html, second.html);
}

#[test]
fn partial_acceptance_fails_the_strict_gate() {
    let screen = styled_screen().expect("valid test screen");
    let canonical = canonical_string(&screen);
    let generation = generation_id(&canonical);
    let sample = render_sample(&screen).expect("render sample");
    let tagged = png_tag_generation(&sample.png, &generation);

    // Complete compound baseline: passes.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "shot", &generation, &canonical).expect("write text snap");
    write_binary_snap(tmp.path(), "shot-img", &generation, &tagged).expect("write binary snap");
    check_consistent(tmp.path(), "shot", "shot-img").expect("consistent compound baseline passes");

    // Canonical accepted, PNG never approved: strict gate fails (no
    // half-blind pass).
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "shot", &generation, &canonical).expect("write text snap");
    let err = check_consistent(tmp.path(), "shot", "shot-img")
        .expect_err("missing PNG sidecar must fail");
    assert!(err.to_string().contains("missing"), "{err}");

    // Canonical re-approved alone after a change (mixed generations): strict
    // gate names the mismatch instead of comparing across samples.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "shot", "aaa", &canonical).expect("write text snap");
    write_binary_snap(tmp.path(), "shot-img", "bbb", &tagged).expect("write binary snap");
    let err =
        check_consistent(tmp.path(), "shot", "shot-img").expect_err("mixed generations must fail");
    assert!(err.to_string().contains("mixed compound baseline"), "{err}");
}

#[test]
fn render_identity_is_recorded_in_descriptions() {
    let settings = tuiscotti_insta::assert::snapshot_settings(
        Path::new("snaps"),
        Location {
            file: "tests/compound.rs",
            line: 1,
        },
        "abc123",
    );
    let description = settings.description().unwrap_or_default();
    assert!(
        description.contains("tuiscotti generation abc123"),
        "{description}"
    );
    assert!(description.contains("tuiscotti-default"), "{description}");
    assert!(
        description.contains(&format!(
            "rv{}",
            tuiscotti_render::profile::RENDERER_VERSION
        )),
        "{description}"
    );
    assert!(description.contains("straight-rgba"), "{description}");
    assert!(description.contains("tests/compound.rs"), "{description}");
}
