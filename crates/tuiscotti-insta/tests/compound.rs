//! G6 compound approval tests: same-sample canonical+PNG binding.
//!
//! - Canonical-identical/render-different repro: one [`Screen`] rendered
//!   under two profiles shares its canonical text but yields different PNG
//!   bytes, and the decoded-pixel comparison fails (pixel equality is never
//!   inferred from cells).
//! - Partial acceptance: a canonical approval without its PNG partner (or
//!   with a mismatched generation) fails the strict [`check_consistent`]
//!   gate instead of passing half-blind.
//! - Evidence precedes failure: [`assert_screenshot!`] writes all four
//!   artifacts before the Insta assertions can fail.
//! - Render identity: snapshot descriptions record the profile, renderer
//!   version, and alpha policy the PNG verdict depends on.

use std::fs;
use std::path::Path;

use tuiscotti_core::screen::{Screen, ScreenError};
use tuiscotti_insta::assert::{
    Location, Policy, check_consistent, frame_from_screen, generation_id, png_tag_generation,
    render_sample,
};
use tuiscotti_insta::insta_proto::insta_string;
use tuiscotti_render::diff::{AlphaPolicy, compare_png_with_alpha};
use tuiscotti_render::profile::{Profile, VENDORED_FACES};
use tuiscotti_render::render::Renderer;

fn styled_screen() -> Result<Screen, ScreenError> {
    use tuiscotti_core::frame::{Cell, Color, Cursor, CursorStyle, Mods};
    let cells = vec![
        Cell {
            x: 0,
            y: 0,
            symbol: "A".to_string(),
            width: 1,
            continuation: false,
            fg: Color::Indexed(1),
            bg: Color::Default,
            mods: Mods {
                bold: true,
                ..Mods::default()
            },
            underline_color: Color::Default,
        },
        Cell {
            x: 1,
            y: 0,
            symbol: "b".to_string(),
            width: 1,
            continuation: false,
            fg: Color::Default,
            bg: Color::Default,
            mods: Mods::default(),
            underline_color: Color::Default,
        },
    ];
    Screen::validate(
        2,
        1,
        0,
        0,
        cells,
        Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: CursorStyle::Block,
            blinking: false,
        },
    )
}

#[test]
fn canonical_identical_render_different_repro() {
    let screen = styled_screen().expect("valid test screen");
    let canonical = insta_string(&screen);

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
    let rendered = renderer.render(&frame).expect("render frame");

    // Canonical text is identical (same screen) ...
    assert_eq!(canonical, insta_string(&screen));
    // ... but the pixels differ, and the decoded-pixel comparison — the same
    // function the PNG comparator delegates to — fails loudly.
    assert_ne!(sample.png, rendered.png);
    let verdict = compare_png_with_alpha(&sample.png, &rendered.png, AlphaPolicy::StraightRgba)
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

fn write_text_snap(dir: &Path, name: &str, generation: &str, body: &str) -> std::io::Result<()> {
    let content = format!(
        "---\nsource: tests/compound.rs\ndescription: tuisnap generation {generation}\nexpression: canonical\n---\n{body}"
    );
    fs::write(dir.join(format!("{name}.snap")), content)
}

fn write_binary_snap(
    dir: &Path,
    name: &str,
    generation: &str,
    sidecar: &[u8],
) -> std::io::Result<()> {
    let meta = format!(
        "---\nsource: tests/compound.rs\ndescription: tuisnap generation {generation}\nexpression: png_bytes\nextension: png\nsnapshot_kind: binary\n---\n"
    );
    fs::write(dir.join(format!("{name}.snap")), meta)?;
    fs::write(dir.join(format!("{name}.snap.png")), sidecar)
}

#[test]
fn partial_acceptance_fails_the_strict_gate() {
    let screen = styled_screen().expect("valid test screen");
    let canonical = insta_string(&screen);
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
        description.contains("tuisnap generation abc123"),
        "{description}"
    );
    assert!(description.contains("tuisnap-default"), "{description}");
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

/// Whether Insta would bless in place (then the failure-ordering test has no
/// failure to order). Mirrors `tuiscotti/tests/common` logic.
fn insta_updates_in_place() -> bool {
    matches!(
        std::env::var("INSTA_UPDATE").ok().as_deref(),
        Some("always") | Some("1") | Some("unseen") | Some("force")
    )
}

#[test]
fn evidence_is_written_before_failure() {
    if insta_updates_in_place() {
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let snaps = tmp.path().join("snaps");
    let evidence = tmp.path().join("evidence");
    fs::create_dir(&snaps).expect("create snaps dir");
    fs::create_dir(&evidence).expect("create evidence dir");
    let policy = Policy::EvolvingIn {
        snapshots: snaps.clone(),
        evidence: evidence.clone(),
    };
    // No approvals exist: the assertion MUST fail in every non-blessing mode.
    let screen = styled_screen().expect("valid test screen");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tuiscotti_insta::assert_screenshot!("g6_evidence_first", &screen, &policy);
    }));
    assert!(outcome.is_err(), "unapproved snapshot must fail");
    // ... and every artifact was already on disk before the failure.
    for ext in ["png", "ansi", "txt", "html"] {
        let path = evidence.join(format!("g6_evidence_first.{ext}"));
        assert!(path.is_file(), "missing evidence {}", path.display());
    }
}
