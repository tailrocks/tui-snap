use super::*;
use std::fs;
use std::path::Path;
use tuiscotti::assert::{
    FrozenError, assert_frozen_screenshot, assert_frozen_snapshot, check_frozen_screenshot,
    check_frozen_screenshot_with_cache, check_frozen_snapshot, frozen_accept, generation_id,
    png_tag_generation, render_identity, render_sample, sample_binding,
};
use tuiscotti::render::{CacheOptions, RenderCache, render_cache_disabled};
use tuiscotti::screen::canonical_string;

#[test]
fn frozen_missing_fails() {
    let root = frozen_dir().expect("frozen_dir succeeds");
    let screen = fixture().expect("fixture succeeds");
    let err = check_frozen_snapshot(root.path(), "shot", &screen)
        .expect_err("check_frozen_snapshot(root.path(), \"shot\", &screen) is an error");
    assert!(matches!(err, FrozenError::Missing { .. }), "{err}");
    let err = check_frozen_screenshot(root.path(), "shot", &screen)
        .expect_err("check_frozen_screenshot(root.path(), \"shot\", &screen) is an error");
    assert!(matches!(err, FrozenError::Missing { .. }), "{err}");
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_frozen_snapshot(root.path(), "shot", &screen);
        }))
        .is_err()
    );
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_frozen_screenshot(root.path(), "shot", &screen);
        }))
        .is_err()
    );
}

#[test]
fn frozen_corrupt_fails_and_never_heals() {
    let root = frozen_dir().expect("frozen_dir succeeds");
    let screen = fixture().expect("fixture succeeds");
    write_frozen(root.path(), "shot", &screen, false).expect("write_frozen succeeds");
    fs::write(root.path().join("shot.png"), b"not a png")
        .expect("fs::write(root.path().join(\"shot.png\"), b\"not a png\") succeeds");
    let before = list_files(root.path()).expect("list_files succeeds");
    let err = check_frozen_screenshot(root.path(), "shot", &screen)
        .expect_err("check_frozen_screenshot(root.path(), \"shot\", &screen) is an error");
    assert!(matches!(err, FrozenError::Corrupt { .. }), "{err}");
    assert_eq!(
        list_files(root.path()).expect("list_files succeeds"),
        before,
        "frozen failure must not write"
    );
    // Non-UTF-8 canonical is corrupt too.
    fs::write(root.path().join("shot.canonical.txt"), b"\xff\xfe invalid").expect(
        "fs::write(root.path().join(\"shot.canonical.txt\"), b\"\\xff\\xfe invalid\") succeeds",
    );
    let err = check_frozen_snapshot(root.path(), "shot", &screen)
        .expect_err("check_frozen_snapshot(root.path(), \"shot\", &screen) is an error");
    assert!(matches!(err, FrozenError::Corrupt { .. }), "{err}");
    assert_eq!(
        list_files(root.path()).expect("list_files succeeds"),
        before,
        "frozen failure must not write"
    );
}

#[test]
fn frozen_accept_always_errors() {
    let root = frozen_dir().expect("frozen_dir succeeds");
    write_frozen(
        root.path(),
        "shot",
        &fixture().expect("fixture succeeds"),
        true,
    )
    .expect("write_frozen succeeds");
    // Even with valid state present...
    let err = frozen_accept(root.path(), "shot")
        .expect_err("frozen_accept(root.path(), \"shot\") is an error");
    assert!(matches!(err, FrozenError::AcceptRejected { .. }), "{err}");
    // ...and on an empty root.
    let empty = frozen_dir().expect("frozen_dir succeeds");
    assert!(matches!(
        frozen_accept(empty.path(), "x"),
        Err(FrozenError::AcceptRejected { .. })
    ));
}

#[test]
fn frozen_passes_when_matching() {
    let root = frozen_dir().expect("frozen_dir succeeds");
    let screen = fixture().expect("fixture succeeds");
    // Untagged legacy PNG: pixel verdict stands.
    write_frozen(root.path(), "plain", &screen, false).expect("write_frozen succeeds");
    check_frozen_snapshot(root.path(), "plain", &screen)
        .expect("check_frozen_snapshot(root.path(), \"plain\", &screen) succeeds");
    check_frozen_screenshot(root.path(), "plain", &screen)
        .expect("check_frozen_screenshot(root.path(), \"plain\", &screen) succeeds");
    // Tagged PNG with the right v2 binding: full gate green.
    write_frozen(root.path(), "tagged", &screen, true).expect("write_frozen succeeds");
    check_frozen_screenshot(root.path(), "tagged", &screen)
        .expect("check_frozen_screenshot(root.path(), \"tagged\", &screen) succeeds");
    // Tagged PNG with a WRONG binding: pixels match, binding fails.
    let sample = render_sample(&screen).expect("render_sample(&screen) succeeds");
    fs::write(
        root.path().join("mistag.canonical.txt"),
        canonical_string(&screen),
    )
    .expect(
        "fs::write( root.path().join(\"mistag.canonical.txt\"), canonical_string(&screen), ) succeeds",
    );
    fs::write(
        root.path().join("mistag.png"),
        png_tag_generation(&sample.png, "gen-b"),
    )
    .expect("fs::write( root.path().join(\"mistag.png\"), png_tag_generation(&sample.png, \"gen-b\"), ) succeeds");
    let err = check_frozen_screenshot(root.path(), "mistag", &screen)
        .expect_err("check_frozen_screenshot(root.path(), \"mistag\", &screen) is an error");
    assert!(matches!(err, FrozenError::Mismatch { .. }), "{err}");
    assert!(err.to_string().contains("sample binding mismatch"), "{err}");
}

#[test]
fn frozen_v1_generation_tag_is_rejected() {
    // Tags must be v2 sample bindings: a v1 canonical-only generation tag no
    // longer satisfies the gate even though the pixels match.
    let root = frozen_dir().expect("frozen_dir succeeds");
    let screen = fixture().expect("fixture succeeds");
    let sample = render_sample(&screen).expect("render_sample succeeds");
    fs::write(
        root.path().join("v1.canonical.txt"),
        canonical_string(&screen),
    )
    .expect("write canonical");
    fs::write(
        root.path().join("v1.png"),
        png_tag_generation(&sample.png, &generation_id(&sample.canonical)),
    )
    .expect("write v1-tagged png");
    let err = check_frozen_screenshot(root.path(), "v1", &screen)
        .expect_err("v1 generation tag must fail");
    assert!(matches!(err, FrozenError::Mismatch { .. }), "{err}");
    assert!(err.to_string().contains("sample binding mismatch"), "{err}");
}

#[test]
fn frozen_tag_from_other_render_is_rejected_with_cause() {
    // Same canonical text and same pixels, but the tag binds a DIFFERENT
    // render identity (other profile/pins): the gate fails and names the
    // render cause (our own identity, for the reviewer to compare).
    let root = frozen_dir().expect("frozen_dir succeeds");
    let screen = fixture().expect("fixture succeeds");
    let sample = render_sample(&screen).expect("render_sample succeeds");
    fs::write(
        root.path().join("other.canonical.txt"),
        canonical_string(&screen),
    )
    .expect("write canonical");
    let foreign = sample_binding(
        &sample.canonical,
        "other-profile/rv9/straight-rgba/profile-deadbeef",
        &sample.png,
    );
    fs::write(
        root.path().join("other.png"),
        png_tag_generation(&sample.png, &foreign),
    )
    .expect("write foreign-tagged png");
    let err = check_frozen_screenshot(root.path(), "other", &screen)
        .expect_err("foreign render tag must fail");
    assert!(matches!(err, FrozenError::Mismatch { .. }), "{err}");
    let msg = err.to_string();
    assert!(msg.contains("sample binding mismatch"), "{msg}");
    assert!(
        msg.contains(&format!("(render {}", render_identity())),
        "{msg}"
    );
}

// ---------------------------------------------------------------------------
// Cached frozen gate: verdict agreement (match/mismatch/corrupt).
// ---------------------------------------------------------------------------

/// Same grid as [`fixture`] with one symbol changed (a canonical mismatch).
fn fixture_variant() -> Result<Screen, Box<dyn std::error::Error>> {
    let base = fixture()?;
    let mut cells = base.cells().to_vec();
    cells[0].symbol = "Q".to_string();
    Ok(Screen::validate(
        base.cols(),
        base.rows(),
        0,
        0,
        cells,
        *base.cursor(),
    )?)
}

/// One scenario through all four gate paths: uncached, cold cache, warm
/// cache, and per-cache no-cache mode. Verdicts (exact [`FrozenError`]
/// equality, or all-pass) must agree.
fn assert_cached_verdicts_agree(root: &Path, name: &str, screen: &Screen) -> Result<(), String> {
    let plain = check_frozen_screenshot(root, name, screen);
    let dir = tempfile::tempdir().map_err(|e| format!("tempdir: {e}"))?;
    let mut cache = RenderCache::open(dir.path(), &[]).map_err(|e| e.to_string())?;
    let cold = check_frozen_screenshot_with_cache(root, name, screen, &mut cache);
    let warm = check_frozen_screenshot_with_cache(root, name, screen, &mut cache);
    let mut disabled =
        RenderCache::open_with_options(dir.path(), &[], CacheOptions { no_cache: true })
            .map_err(|e| e.to_string())?;
    let off = check_frozen_screenshot_with_cache(root, name, screen, &mut disabled);
    assert_eq!(plain, cold, "{name}: cold-cache verdict must agree");
    assert_eq!(plain, warm, "{name}: warm-cache verdict must agree");
    assert_eq!(plain, off, "{name}: no-cache verdict must agree");
    Ok(())
}

#[test]
fn cached_and_uncached_frozen_verdicts_agree() {
    let root = frozen_dir().expect("frozen_dir succeeds");
    let screen = fixture().expect("fixture succeeds");
    let other = fixture_variant().expect("fixture variant");
    // Match (tagged): all four paths pass.
    write_frozen(root.path(), "agree", &screen, true).expect("write_frozen succeeds");
    assert_cached_verdicts_agree(root.path(), "agree", &screen).expect("verdicts agree");
    // Pixels mismatch (approved canonical, foreign valid PNG): all fail alike.
    fs::write(
        root.path().join("pix.canonical.txt"),
        canonical_string(&screen),
    )
    .expect("write canonical");
    let foreign_png = render_sample(&other).expect("render other").png;
    fs::write(root.path().join("pix.png"), &foreign_png).expect("write foreign png");
    assert_cached_verdicts_agree(root.path(), "pix", &screen).expect("verdicts agree");
    // Canonical mismatch: all fail alike (no render happens on any path).
    assert_cached_verdicts_agree(root.path(), "agree", &other).expect("verdicts agree");
    // Corrupt approved PNG: all fail alike.
    fs::write(
        root.path().join("badpng.canonical.txt"),
        canonical_string(&screen),
    )
    .expect("write canonical");
    fs::write(root.path().join("badpng.png"), b"not a png").expect("write junk png");
    assert_cached_verdicts_agree(root.path(), "badpng", &screen).expect("verdicts agree");
    // Corrupt approved canonical: all fail alike.
    fs::write(
        root.path().join("badcanon.canonical.txt"),
        b"\xff\xfe invalid",
    )
    .expect("write bad canonical");
    fs::write(
        root.path().join("badcanon.png"),
        render_sample(&screen).expect("render sample").png,
    )
    .expect("write png");
    assert_cached_verdicts_agree(root.path(), "badcanon", &screen).expect("verdicts agree");
}

#[test]
fn frozen_gate_cache_hit_serves_without_rerender() {
    // A stats proof the warm path above really hits (the agreement test
    // proves verdicts; this proves the hit happened). Unobservable when the
    // ambient environment already forces no-cache mode.
    if render_cache_disabled() {
        return;
    }
    let root = frozen_dir().expect("frozen_dir succeeds");
    let screen = fixture().expect("fixture succeeds");
    write_frozen(root.path(), "stats", &screen, true).expect("write_frozen succeeds");
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cache = RenderCache::open(dir.path(), &[]).expect("open cache");
    check_frozen_screenshot_with_cache(root.path(), "stats", &screen, &mut cache)
        .expect("cold gate passes");
    assert_eq!((cache.stores(), cache.hits()), (1, 0));
    check_frozen_screenshot_with_cache(root.path(), "stats", &screen, &mut cache)
        .expect("warm gate passes");
    assert_eq!(cache.hits(), 1);
}

/// Subprocess handoff for the `RENDER_NO_CACHE` path test (`set_var` is
/// `unsafe` under edition 2024, so the env-carrying process is spawned).
const E_CACHE_DIR_ENV: &str = "TUISCOTTI_TEST_E_CACHE_DIR";
const E_FROZEN_DIR_ENV: &str = "TUISCOTTI_TEST_E_FROZEN_DIR";

#[test]
fn render_no_cache_env_disables_gate_cache() {
    // The toggle is untestable when the ambient environment already forces
    // no-cache mode (nothing to turn off).
    if render_cache_disabled() {
        return;
    }
    let cache_tmp = tempfile::tempdir().expect("tempdir");
    let frozen_tmp = frozen_dir().expect("frozen_dir succeeds");
    let screen = fixture().expect("fixture succeeds");
    write_frozen(frozen_tmp.path(), "env", &screen, true).expect("write_frozen succeeds");
    // Control (this process, no env): populate and prove the entry servable.
    let mut cache = RenderCache::open(cache_tmp.path(), &[]).expect("open cache");
    check_frozen_screenshot_with_cache(frozen_tmp.path(), "env", &screen, &mut cache)
        .expect("control gate passes");
    check_frozen_screenshot_with_cache(frozen_tmp.path(), "env", &screen, &mut cache)
        .expect("control gate passes warm");
    assert_eq!(cache.hits(), 1, "control: warm cache hits");
    // Helper run with RENDER_NO_CACHE=1: the valid entry is bypassed.
    let exe = std::env::current_exe().expect("current exe");
    let out = std::process::Command::new(exe)
        .args([
            "frozen::render_no_cache_env_helper_bypasses_gate_cache",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env("RENDER_NO_CACHE", "1")
        .env(E_CACHE_DIR_ENV, cache_tmp.path())
        .env(E_FROZEN_DIR_ENV, frozen_tmp.path())
        .output()
        .expect("spawn helper");
    assert!(
        out.status.success(),
        "helper failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The filter must have matched (a zero-match run also exits 0).
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("1 passed"),
        "helper ran nothing: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// Subprocess-only helper for [`render_no_cache_env_disables_gate_cache`]:
/// runs with `RENDER_NO_CACHE=1` against the spawner's populated cache dir.
#[test]
#[ignore = "subprocess-only: run via render_no_cache_env_disables_gate_cache"]
fn render_no_cache_env_helper_bypasses_gate_cache() {
    assert!(render_cache_disabled(), "helper requires RENDER_NO_CACHE=1");
    let cache_dir = std::env::var(E_CACHE_DIR_ENV).expect("cache dir env");
    let frozen_dir = std::env::var(E_FROZEN_DIR_ENV).expect("frozen dir env");
    let screen = fixture().expect("fixture succeeds");
    let mut cache = RenderCache::open(Path::new(&cache_dir), &[]).expect("open cache");
    // A valid entry sits on disk (the spawner control proved it servable),
    // yet every access misses while the verdict stays correct.
    check_frozen_screenshot_with_cache(Path::new(&frozen_dir), "env", &screen, &mut cache)
        .expect("gate verdict stays correct under RENDER_NO_CACHE");
    assert_eq!(cache.hits(), 0, "get must bypass under RENDER_NO_CACHE");
    assert_eq!(cache.stores(), 0, "put must drop under RENDER_NO_CACHE");
}
