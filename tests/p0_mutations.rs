//! P0 verification-gap mutation tests (backlog C01–C10, docs/REDESIGN-BACKLOG.md).
//!
//! Each test pins a CURRENT gap: it FAILS on today's code and must PASS only
//! after the corresponding fix lands. Deterministic, offline, no network.
//!
//! Owned files: this file only (plus `tests/fixtures/p0/` if needed — none
//! needed; all PNGs are generated in memory).

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ImageEncoder, RgbImage, RgbaImage};
use ratatui::widgets::Paragraph;
use tuisnap::diff::compare_png_with_flags;
use tuisnap::grouped::GroupedStore;
use tuisnap::snapshot::{Status, Store};
use tuisnap::{Profile, Provenance, VENDORED_FACES};

// ---------------------------------------------------------------- helpers

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

fn frame_with(text: &str) -> tuisnap::Frame {
    tuisnap::ratatui::widget_frame(Paragraph::new(text), 30, 6, prov())
}

fn tmp_classic(tag: &str) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let st = Store::new(&dir.path().join(tag));
    (dir, st)
}

fn tmp_grouped(tag: &str) -> (tempfile::TempDir, GroupedStore) {
    let dir = tempfile::tempdir().unwrap();
    let st = GroupedStore::new(&dir.path().join(tag));
    (dir, st)
}

/// 16x16 deterministic gradient (non-trivial bytes so encoder settings matter).
fn gradient_rgb() -> RgbImage {
    let mut img = RgbImage::new(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(x, y, image::Rgb([(x * 16) as u8, (y * 16) as u8, 128]));
        }
    }
    img
}

fn encode_rgb(img: &RgbImage, c: CompressionType, f: FilterType) -> Vec<u8> {
    let mut buf = Vec::new();
    PngEncoder::new_with_quality(&mut buf, c, f)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    buf
}

fn encode_rgba(img: &RgbaImage, c: CompressionType, f: FilterType) -> Vec<u8> {
    let mut buf = Vec::new();
    PngEncoder::new_with_quality(&mut buf, c, f)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    buf
}

fn decode_rgb(png: &[u8]) -> RgbImage {
    image::load_from_memory(png).unwrap().to_rgb8()
}

// ------------------------------------------------- C01: cell-equality bypass

#[test]
fn c01_same_cells_dims_but_different_pixels_must_fail_strict_check() {
    // Two PNGs, identical dimensions, different decoded pixels.
    let mut a = gradient_rgb();
    let mut b = gradient_rgb();
    b.put_pixel(3, 5, image::Rgb([255, 0, 0]));
    a.put_pixel(3, 5, image::Rgb([0, 0, 255]));
    let png_a = encode_rgb(&a, CompressionType::Default, FilterType::Adaptive);
    let png_b = encode_rgb(&b, CompressionType::Default, FilterType::Adaptive);
    assert_ne!(png_a, png_b, "setup: encodings must differ");
    assert_ne!(
        decode_rgb(&png_a).as_raw(),
        decode_rgb(&png_b).as_raw(),
        "setup: decoded pixels must differ"
    );

    // Guard: the real pixel gate (ansi_matched=false) catches the difference.
    let honest = compare_png_with_flags(&png_a, &png_b, false).unwrap();
    assert!(honest.dims_equal);
    assert!(
        honest.score < 1.0,
        "guard: hybrid gate must see pixel difference, got {}",
        honest.score
    );

    // Gap (src/diff.rs `compare_png_with_flags`, `ansi_matched` branch):
    // same cells+dims currently SKIP the pixel metric and report score 1.0.
    let bypassed = compare_png_with_flags(&png_a, &png_b, true).unwrap();
    assert!(
        bypassed.score < 1.0,
        "C01 gap: ansi_matched=true reports score={} for differing decoded pixels; \
         strict check must fail",
        bypassed.score
    );
}

// --------------------------------- C03: exact RGBA / opaque-policy comparison

#[test]
fn c03_exact_decoded_rgba_comparison_with_explicit_alpha_policy() {
    let img = gradient_rgb();
    let enc_default = encode_rgb(&img, CompressionType::Default, FilterType::Adaptive);
    let enc_best = encode_rgb(&img, CompressionType::Best, FilterType::NoFilter);
    assert_ne!(
        enc_default, enc_best,
        "setup: different encoder settings must give different bytes for same pixels"
    );

    // Re-encoding identical pixels must pass.
    let re = compare_png_with_flags(&enc_default, &enc_best, false).unwrap();
    assert!(
        !(re.score < 1.0),
        "re-encoded identical pixels must pass strict gate, got score={}",
        re.score
    );

    // One relevant channel difference must fail.
    let mut one = img.clone();
    let p = *one.get_pixel(7, 7);
    one.put_pixel(7, 7, image::Rgb([p[0].wrapping_add(1), p[1], p[2]]));
    let enc_one = encode_rgb(&one, CompressionType::Default, FilterType::Adaptive);
    let v_one = compare_png_with_flags(&enc_default, &enc_one, false).unwrap();
    assert!(
        v_one.score < 1.0,
        "one-channel pixel difference must fail strict gate, got score={}",
        v_one.score
    );

    // Gap (src/diff.rs `decode_png` uses `.to_rgb8()`, discarding alpha):
    // alpha-only differences are invisible to the gate; alpha semantics
    // must be explicit (opaque-policy compare or alpha-mismatch failure).
    let mut opaque = RgbaImage::new(16, 16);
    let mut clear = RgbaImage::new(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            opaque.put_pixel(x, y, image::Rgba([200, 100, 50, 255]));
            clear.put_pixel(x, y, image::Rgba([200, 100, 50, 0]));
        }
    }
    let png_opaque = encode_rgba(&opaque, CompressionType::Default, FilterType::Adaptive);
    let png_clear = encode_rgba(&clear, CompressionType::Default, FilterType::Adaptive);
    let v_alpha = compare_png_with_flags(&png_opaque, &png_clear, false).unwrap();
    assert!(
        v_alpha.score < 1.0,
        "C03 gap: fully-transparent vs fully-opaque (same RGB) scores {}; \
         alpha semantics must be explicit, not silently dropped",
        v_alpha.score
    );
}

// -------------------- C04: perceptual diagnostics vs exact verdicts

#[test]
fn c04_similarity_score_must_not_establish_strict_equality() {
    // Part 1: score >= 1.0 must imply decoded-pixel identity. The
    // ansi_matched bypass breaks this: score 1.0 with differing pixels.
    let mut a = gradient_rgb();
    let mut b = gradient_rgb();
    b.put_pixel(0, 0, image::Rgb([1, 2, 3]));
    a.put_pixel(0, 0, image::Rgb([3, 2, 1]));
    let png_a = encode_rgb(&a, CompressionType::Default, FilterType::Adaptive);
    let png_b = encode_rgb(&b, CompressionType::Default, FilterType::Adaptive);
    let v = compare_png_with_flags(&png_a, &png_b, true).unwrap();
    let decoded_equal = decode_rgb(&png_a).as_raw() == decode_rgb(&png_b).as_raw();
    assert!(
        !(v.score >= 1.0) || decoded_equal,
        "C04 gap: score={} establishes 'equality' for decoded-different pixels; \
         a rounded/perceptual score cannot prove strict equality",
        v.score
    );

    // Part 2: invalid tolerances must be rejected, not silently applied.
    // Today `score < pixel_threshold` with NaN is always false → Matched,
    // and threshold 2.0 fails even identical images (no validation anywhere
    // on the check/report path).
    let (_dir, st) = tmp_classic("c04");
    let frame = frame_with("tolerance");
    st.check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept("home").unwrap();
    let nan = st.check("home", &frame, &profile(), &VENDORED_FACES, f64::NAN);
    assert!(
        nan.is_err(),
        "C04 gap: NaN pixel_threshold must be rejected, got {nan:?}"
    );
    let huge = st.check("home", &frame, &profile(), &VENDORED_FACES, 2.0);
    assert!(
        huge.is_err(),
        "C04 gap: pixel_threshold=2.0 must be rejected, got {huge:?}"
    );
}

// ----------------- C06: missing/corrupt reference artifacts must fail

#[test]
fn c06_missing_approved_png_must_fail_not_regenerate_in_memory() {
    // Classic store gap (src/snapshot.rs `check_with`: missing approved PNG
    // is rendered to MEMORY and the gate can still return Matched).
    let (_dir, st) = tmp_classic("c06");
    let frame = frame_with("frozen pixels");
    st.check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept("home").unwrap();
    let matched = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(matched.status, Status::Matched, "setup: must match first");

    std::fs::remove_file(st.root().join("approved").join("home.png")).unwrap();
    let gap = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(
        gap.status,
        Status::MissingApproval,
        "C06 gap: missing approved PNG must fail closed, got {:?} \
         (approved_png_regenerated={})",
        gap.status,
        gap.approved_png_regenerated
    );
    assert!(
        !gap.approved_png_regenerated,
        "C06 gap: frozen visual mode must not regenerate expected images in memory"
    );

    // Guard: the grouped store already fails closed here — keep it that way.
    let (_gdir, gst) = tmp_grouped("c06g");
    gst.check("s", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    gst.accept("s").unwrap();
    std::fs::remove_file(gst.approved_root().join("s.png")).unwrap();
    let grouped = gst
        .check("s", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(
        grouped.status(),
        Status::MissingApproval,
        "guard: grouped store must stay fail-closed on missing approved PNG"
    );
}

// ----------------------- C05: report/test verdict unification

#[test]
fn c05_report_verdict_must_equal_test_verdict_on_same_inputs() {
    // Gap: `GroupedStore::report_with` IGNORES pixel_threshold
    // (`_pixel_threshold`, src/grouped.rs) and byte-compares compressed PNGs
    // (`disk_status`), while `check` compares DECODED pixels. Re-encoding the
    // same pixels with different bytes splits the two verdicts.
    let (_dir, gst) = tmp_grouped("c05");
    let name = "verdict/parity";
    let frame = frame_with("same verdict everywhere");
    gst.check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    gst.accept(name).unwrap();
    let checked = gst
        .check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(checked.status(), Status::Matched, "setup: check must match");

    // Same decoded pixels, different compressed bytes, written as the actual.
    let actual_png = checked.actual.png.clone();
    let approved_bytes = std::fs::read(gst.approved_root().join(format!("{name}.png"))).unwrap();
    let decoded = decode_rgb(&approved_bytes);
    let reencoded = encode_rgb(&decoded, CompressionType::Best, FilterType::NoFilter);
    assert_ne!(
        approved_bytes, reencoded,
        "setup: re-encode must change bytes while keeping pixels"
    );
    assert_eq!(
        decode_rgb(&reencoded).as_raw(),
        decoded.as_raw(),
        "setup: re-encode must keep decoded pixels"
    );
    std::fs::write(&actual_png, &reencoded).unwrap();

    let mut renderer = tuisnap::render::Renderer::new(&profile(), &VENDORED_FACES).unwrap();
    let report = gst.report_with(&mut renderer, 1.0, "c05").unwrap();
    let reported = report
        .outcomes
        .iter()
        .find(|o| o.name == name)
        .expect("report must cover the scenario");
    assert_eq!(
        reported.status,
        checked.status(),
        "C05 gap: report says {:?} but check said {:?} for identical decoded \
         pixels (report byte-compares PNGs, check compares decoded pixels)",
        reported.status,
        checked.status()
    );
}

// ------------------------------- C07: assertions hard to ignore

#[test]
fn c07_dropped_mismatch_outcome_must_not_silently_pass() {
    let (_dir, st) = tmp_classic("c07");
    st.check(
        "home",
        &frame_with("before"),
        &profile(),
        &VENDORED_FACES,
        1.0,
    )
    .unwrap();
    st.accept("home").unwrap();
    let mismatch = st
        .check(
            "home",
            &frame_with("after"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .unwrap();
    assert_eq!(mismatch.status, Status::CellsDiffer, "setup: must mismatch");

    // `CompareOutcome`/`Status` are #[must_use] (src/snapshot.rs), so a
    // caller that forgets `ensure_matched()` gets an `unused_must_use`
    // warning; `drop` here documents the previously silent scenario.
    drop(mismatch);

    // Runtime half: `ensure_matched` fails loudly on mismatch.
    let fresh = st
        .check(
            "home",
            &frame_with("after"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .unwrap();
    assert!(
        fresh.ensure_matched().is_err(),
        "ensure_matched must error on mismatch"
    );
}

// ------ C08: transactional candidate generation / consistent approval

#[test]
fn c08_interrupted_candidate_must_report_incomplete_not_match() {
    // Case A (classic store): report RE-RENDERS from the actual frame
    // (`Store::report_with` → `check_with`), silently healing an interrupted
    // candidate (frame present, PNG write lost) back to Matched.
    let (_dir, st) = tmp_classic("c08");
    let frame = frame_with("atomic candidate");
    st.check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept("home").unwrap();
    let matched = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(matched.status, Status::Matched, "setup: must match first");

    // Simulate interruption: actual frame survived, actual PNG did not.
    let actual_png = matched.actual_png.clone();
    std::fs::remove_file(&actual_png).unwrap();
    let sidecar = actual_png.with_extension("png.fidelity.json");
    let _ = std::fs::remove_file(&sidecar);
    assert!(!actual_png.exists(), "setup: actual PNG must be gone");

    let mut renderer = tuisnap::render::Renderer::new(&profile(), &VENDORED_FACES).unwrap();
    let report = st.report_with(&mut renderer, 1.0, "c08").unwrap();
    let reported = &report.outcomes[0];
    assert!(
        !reported.status.matched(),
        "C08 gap (classic): interrupted candidate (actual frame without PNG) \
         reports {:?}; an absent completion must be incomplete, never pass",
        reported.status
    );

    // Case B (grouped store): a missing actual PNG is misattributed as a
    // pixel difference (`disk_status` byte-compares), not flagged as
    // incomplete/missing evidence.
    let (_gdir, gst) = tmp_grouped("c08g");
    gst.check("s", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    gst.accept("s").unwrap();
    let gmatched = gst
        .check("s", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(
        gmatched.status(),
        Status::Matched,
        "setup: must match first"
    );
    std::fs::remove_file(&gmatched.actual.png).unwrap();
    let mut grenderer = tuisnap::render::Renderer::new(&profile(), &VENDORED_FACES).unwrap();
    let greport = gst.report_with(&mut grenderer, 1.0, "c08g").unwrap();
    let greported = &greport.outcomes[0];
    assert_eq!(
        greported.status,
        Status::MissingApproval,
        "C08 gap (grouped): interrupted candidate (actual frame without PNG) \
         reports {:?} instead of missing/incomplete evidence",
        greported.status
    );
}

// ------ C02: actual evidence from candidate, never approved

#[test]
fn c02_actual_evidence_comes_from_candidate_never_approved() {
    // Gap (src/grouped.rs tiered fast path): when the ansi/txt cell gates
    // passed, check COPIED approved PNG/HTML bytes into actual/ instead of
    // rendering the candidate — fabricating html_match=true and pixel
    // score 1.0 without rendering, and masking approved-side tamper.
    let (_dir, gst) = tmp_grouped("c02");
    let name = "c02/evidence";
    let frame = frame_with("candidate evidence");
    gst.check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    gst.accept(name).unwrap();

    // Sabotage approved RENDER bytes only (cells unchanged): re-encode the
    // approved PNG (same decoded pixels, different bytes) and perturb the
    // approved HTML. Any approved→actual byte copy is then detectable.
    let approved_png_path = gst.approved_root().join(format!("{name}.png"));
    let approved_html_path = gst.approved_root().join(format!("{name}.html"));
    let approved_png_before = std::fs::read(&approved_png_path).unwrap();
    let decoded_rgba = image::load_from_memory(&approved_png_before)
        .unwrap()
        .to_rgba8();
    let reencoded = encode_rgba(&decoded_rgba, CompressionType::Best, FilterType::NoFilter);
    assert_ne!(
        approved_png_before, reencoded,
        "setup: re-encode must change bytes while keeping pixels"
    );
    assert_eq!(
        image::load_from_memory(&reencoded)
            .unwrap()
            .to_rgba8()
            .as_raw(),
        decoded_rgba.as_raw(),
        "setup: re-encode must keep decoded pixels"
    );
    std::fs::write(&approved_png_path, &reencoded).unwrap();
    let approved_html_before = std::fs::read(&approved_html_path).unwrap();
    let mut tampered_html = approved_html_before.clone();
    tampered_html.extend_from_slice(b"\n<!-- c02 tamper -->\n");
    std::fs::write(&approved_html_path, &tampered_html).unwrap();

    // Fresh render of the CANDIDATE frame, straight from the renderer.
    let mut renderer = profile().renderer(&VENDORED_FACES).unwrap();
    let fresh = renderer.render_artifacts(&frame, name).unwrap();

    // Both tiers — the default check and the explicit tiered flag that used
    // to skip rendering — must write fresh candidate renders as evidence.
    // There is no skip-render path anymore, so there is no copied-bytes
    // verdict to mark not-checked: every tier renders (C02).
    for full_render in [false, true] {
        let mut r = profile().renderer(&VENDORED_FACES).unwrap();
        let opts = tuisnap::grouped::GroupedCheckOptions { full_render };
        let outcome = gst
            .check_with_options(&mut r, name, &frame, 1.0, &opts)
            .unwrap();
        assert_eq!(outcome.ansi_match, Some(true), "setup: cells unchanged");
        assert_eq!(outcome.txt_match, Some(true), "setup: cells unchanged");
        let actual_png = std::fs::read(&outcome.actual.png).unwrap();
        let actual_html = std::fs::read(&outcome.actual.html).unwrap();
        assert_eq!(
            actual_png, fresh.png,
            "C02 gap (full_render={full_render}): actual PNG must be a fresh \
             render of the candidate frame, not approved bytes"
        );
        assert_eq!(
            actual_html,
            fresh.html.as_bytes(),
            "C02 gap (full_render={full_render}): actual HTML must be a fresh \
             render of the candidate frame, not approved bytes"
        );
        assert_ne!(
            actual_png, reencoded,
            "C02 gap: actual PNG copies sabotaged approved bytes"
        );
        assert_ne!(
            actual_html, tampered_html,
            "C02 gap: actual HTML copies sabotaged approved bytes"
        );
        // The render-level gate now sees the tamper (PNG pixels still match,
        // so only HTML falls).
        assert_eq!(
            outcome.status(),
            Status::PixelsDiffer,
            "tampered approved HTML must fail the render-level gate"
        );
        assert_eq!(outcome.html_match, Some(false));
    }
}
