//! Decoded-pixel comparison engine (strict + perceptual diagnostic).

use super::{AlphaPolicy, DiffError, PixelVerdict};
use image::{RgbImage, RgbaImage};

fn decode_png(bytes: &[u8], label: &str) -> Result<RgbaImage, DiffError> {
    image::load_from_memory(bytes)
        .map_err(|e| DiffError(format!("cannot decode {label} PNG: {e}")))
        .map(|d| d.to_rgba8())
}

fn rgba_to_rgb(img: &RgbaImage) -> RgbImage {
    let mut out = RgbImage::new(img.width(), img.height());
    for (x, y, p) in img.enumerate_pixels() {
        out.put_pixel(x, y, image::Rgb([p[0], p[1], p[2]]));
    }
    out
}

fn pixels_equal_under(expected: &RgbaImage, actual: &RgbaImage, policy: AlphaPolicy) -> bool {
    if expected.dimensions() != actual.dimensions() {
        return false;
    }
    match policy {
        AlphaPolicy::StraightRgba => expected.as_raw() == actual.as_raw(),
        AlphaPolicy::Opaque => {
            expected.pixels().all(|p| p[3] == 255)
                && actual.pixels().all(|p| p[3] == 255)
                && expected
                    .pixels()
                    .zip(actual.pixels())
                    .all(|(e, a)| e[0] == a[0] && e[1] == a[1] && e[2] == a[2])
        }
    }
}

/// Hybrid structural+chroma similarity plus a red-overlay diff PNG over the
/// RGB channels. Diagnostic material only: the score must never establish
/// strict equality — callers cap it below 1.0 for non-identical pixels.
fn hybrid_diagnostic(
    expected: &RgbaImage,
    actual: &RgbaImage,
) -> Result<(f64, Vec<u8>), DiffError> {
    let expected_rgb = rgba_to_rgb(expected);
    let actual_rgb = rgba_to_rgb(actual);
    let result = image_compare::rgb_hybrid_compare(&expected_rgb, &actual_rgb)
        .map_err(|e| DiffError(format!("comparison failed: {e}")))?;
    let mut diff_png = Vec::new();
    image::DynamicImage::ImageRgb8(result.image.to_color_map().to_rgb8())
        .write_to(
            &mut std::io::Cursor::new(&mut diff_png),
            image::ImageFormat::Png,
        )
        .map_err(|e| DiffError(format!("diff PNG encode: {e}")))?;
    Ok((result.score, diff_png))
}

/// Strict comparison over decoded pixels with the default
/// [`AlphaPolicy::StraightRgba`]. Never compares compressed bytes:
/// re-encoding identical pixels passes. Takes no threshold.
pub fn compare_png(expected_png: &[u8], actual_png: &[u8]) -> Result<PixelVerdict, DiffError> {
    compare_png_with_alpha(expected_png, actual_png, AlphaPolicy::default())
}

/// Strict comparison over decoded pixels with an explicit [`AlphaPolicy`].
/// Equality is equal dimensions plus exact decoded-pixel identity — no
/// threshold, no perceptual rounding.
pub fn compare_png_with_alpha(
    expected_png: &[u8],
    actual_png: &[u8],
    alpha_policy: AlphaPolicy,
) -> Result<PixelVerdict, DiffError> {
    // Sound fast path: identical compressed bytes decode to identical
    // pixels. Still decodes once so corrupt-but-identical bytes error
    // instead of passing.
    if expected_png == actual_png {
        let decoded = decode_png(expected_png, "expected")?;
        let dims = (decoded.width(), decoded.height());
        return Ok(PixelVerdict {
            dims_equal: true,
            expected_dims: dims,
            actual_dims: dims,
            pixels_equal: true,
            alpha_policy,
            score: 1.0,
            diff_png: Vec::new(),
        });
    }
    let expected = decode_png(expected_png, "expected")?;
    let actual = decode_png(actual_png, "actual")?;
    let expected_dims = (expected.width(), expected.height());
    let actual_dims = (actual.width(), actual.height());
    if expected_dims != actual_dims {
        return Ok(PixelVerdict {
            dims_equal: false,
            expected_dims,
            actual_dims,
            pixels_equal: false,
            alpha_policy,
            score: 0.0,
            diff_png: Vec::new(),
        });
    }
    if pixels_equal_under(&expected, &actual, alpha_policy) {
        return Ok(PixelVerdict {
            dims_equal: true,
            expected_dims,
            actual_dims,
            pixels_equal: true,
            alpha_policy,
            score: 1.0,
            diff_png: Vec::new(),
        });
    }
    let (raw_score, diff_png) = hybrid_diagnostic(&expected, &actual)?;
    // A perceptual value must never read as equality (C04): identical RGB
    // with differing alpha scores a hybrid 1.0, so clamp below 1.0 here.
    // `score >= 1.0` therefore implies decoded-pixel identity.
    let score = raw_score.min(1.0 - f64::EPSILON);
    Ok(PixelVerdict {
        dims_equal: true,
        expected_dims,
        actual_dims,
        pixels_equal: false,
        alpha_policy,
        score,
        diff_png,
    })
}

/// Diagnostic-only perceptual similarity in `[0.0, 1.0]` (hybrid
/// structural+chroma over RGB). NEVER establishes strict equality — not
/// even at 1.0: alpha-only differences are invisible to it, and rounding
/// can hide small changes. Only [`PixelVerdict::pixels_equal`] proves pixel
/// identity. Returns 0.0 when dimensions differ (no pixel correspondence
/// exists).
pub fn perceptual_score(expected_png: &[u8], actual_png: &[u8]) -> Result<f64, DiffError> {
    let expected = decode_png(expected_png, "expected")?;
    let actual = decode_png(actual_png, "actual")?;
    if expected.dimensions() != actual.dimensions() {
        return Ok(0.0);
    }
    if expected.as_raw() == actual.as_raw() {
        return Ok(1.0);
    }
    let (score, _) = hybrid_diagnostic(&expected, &actual)?;
    Ok(score)
}
