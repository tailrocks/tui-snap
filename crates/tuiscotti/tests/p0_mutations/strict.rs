use super::*;
use image::RgbaImage;
use image::codecs::png::{CompressionType, FilterType};
use tuiscotti::VENDORED_FACES;
use tuiscotti::diff::compare_png;

#[test]
fn c01_same_cells_dims_but_different_pixels_must_fail_strict_check() {
    // Two PNGs, identical dimensions, different decoded pixels.
    let mut a = gradient_rgb();
    let mut b = gradient_rgb();
    b.put_pixel(3, 5, image::Rgb([255, 0, 0]));
    a.put_pixel(3, 5, image::Rgb([0, 0, 255]));
    let png_a = encode_rgb(&a, CompressionType::Default, FilterType::Adaptive)
        .expect("encode_rgb succeeds");
    let png_b = encode_rgb(&b, CompressionType::Default, FilterType::Adaptive)
        .expect("encode_rgb succeeds");
    assert_ne!(png_a, png_b, "setup: encodings must differ");
    assert_ne!(
        decode_rgb(&png_a).expect("decode_rgb succeeds").as_raw(),
        decode_rgb(&png_b).expect("decode_rgb succeeds").as_raw(),
        "setup: decoded pixels must differ"
    );

    // Guard: the real pixel gate catches the difference.
    let honest = compare_png(&png_a, &png_b).expect("compare_png(&png_a, &png_b) succeeds");
    assert!(honest.dims_equal);
    assert!(
        honest.score < 1.0,
        "guard: hybrid gate must see pixel difference, got {}",
        honest.score
    );

    // The strict gate must catch the same difference unconditionally:
    // same cells+dims never SKIP the pixel metric (C01).
    let bypassed = compare_png(&png_a, &png_b).expect("compare_png(&png_a, &png_b) succeeds");
    assert!(
        bypassed.score < 1.0,
        "C01 gap: score={} for differing decoded pixels; strict check must fail",
        bypassed.score
    );
}

// --------------------------------- C03: exact RGBA / opaque-policy comparison
#[test]
fn c03_exact_decoded_rgba_comparison_with_explicit_alpha_policy() {
    let img = gradient_rgb();
    let enc_default = encode_rgb(&img, CompressionType::Default, FilterType::Adaptive)
        .expect("encode_rgb succeeds");
    let enc_best =
        encode_rgb(&img, CompressionType::Best, FilterType::NoFilter).expect("encode_rgb succeeds");
    assert_ne!(
        enc_default, enc_best,
        "setup: different encoder settings must give different bytes for same pixels"
    );

    // Re-encoding identical pixels must pass.
    let re = compare_png(&enc_default, &enc_best)
        .expect("compare_png(&enc_default, &enc_best) succeeds");
    assert!(
        re.score >= 1.0,
        "re-encoded identical pixels must pass strict gate, got score={}",
        re.score
    );

    // One relevant channel difference must fail.
    let mut one = img.clone();
    let p = *one.get_pixel(7, 7);
    one.put_pixel(7, 7, image::Rgb([p[0].wrapping_add(1), p[1], p[2]]));
    let enc_one = encode_rgb(&one, CompressionType::Default, FilterType::Adaptive)
        .expect("encode_rgb succeeds");
    let v_one =
        compare_png(&enc_default, &enc_one).expect("compare_png(&enc_default, &enc_one) succeeds");
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
    let png_opaque = encode_rgba(&opaque, CompressionType::Default, FilterType::Adaptive)
        .expect("encode_rgba succeeds");
    let png_clear = encode_rgba(&clear, CompressionType::Default, FilterType::Adaptive)
        .expect("encode_rgba succeeds");
    let v_alpha = compare_png(&png_opaque, &png_clear)
        .expect("compare_png(&png_opaque, &png_clear) succeeds");
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
    // Part 1: score >= 1.0 must imply decoded-pixel identity — never
    // score 1.0 with differing pixels.
    let mut a = gradient_rgb();
    let mut b = gradient_rgb();
    b.put_pixel(0, 0, image::Rgb([1, 2, 3]));
    a.put_pixel(0, 0, image::Rgb([3, 2, 1]));
    let png_a = encode_rgb(&a, CompressionType::Default, FilterType::Adaptive)
        .expect("encode_rgb succeeds");
    let png_b = encode_rgb(&b, CompressionType::Default, FilterType::Adaptive)
        .expect("encode_rgb succeeds");
    let v = compare_png(&png_a, &png_b).expect("compare_png(&png_a, &png_b) succeeds");
    let decoded_equal = decode_rgb(&png_a).expect("decode_rgb succeeds").as_raw()
        == decode_rgb(&png_b).expect("decode_rgb succeeds").as_raw();
    assert!(
        v.score < 1.0 || decoded_equal,
        "C04 gap: score={} establishes 'equality' for decoded-different pixels; \
         a rounded/perceptual score cannot prove strict equality",
        v.score
    );

    // Part 2: invalid tolerances must be rejected, not silently applied.
    // Today `score < pixel_threshold` with NaN is always false → Matched,
    // and threshold 2.0 fails even identical images (no validation anywhere
    // on the check/report path).
    let (_dir, st) = tmp_classic("c04").expect("tmp_classic succeeds");
    let frame = frame_with("tolerance");
    drop(
        st.check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
            .expect("st .check(\"home\", &frame, &profile(), &VENDORED_FACES, 1.0) succeeds"),
    );
    st.accept("home").expect("st.accept(\"home\") succeeds");
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
