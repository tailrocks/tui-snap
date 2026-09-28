//! Decoded-pixel comparison: approved PNG vs actual PNG.
//!
//! Kept separate from [`Frame::diff_cells`] on purpose: a renderer upgrade
//! can change pixels without changing application cells, and that must read
//! as a renderer event — not an app regression. Gates compare **decoded**
//! pixels, never compressed PNG bytes (re-encoding the same image must not
//! fail a gate).
//!
//! Two contracts, deliberately different types:
//! - **Strict** ([`compare_png`] / [`compare_png_with_alpha`]): equal
//!   dimensions plus exact decoded-pixel identity under an explicit
//!   [`AlphaPolicy`]. Takes no threshold. The verdict bit is
//!   [`PixelVerdict::pixels_equal`] — never the score.
//! - **Perceptual** ([`perceptual_score`] + [`PerceptualPolicy`]): a hybrid
//!   structural+chroma similarity diagnostic. It can inform review; it can
//!   never establish strict equality, not even at 1.0 (alpha-only
//!   differences are invisible to it, and rounding can hide small changes).
//!   Thresholds exist ONLY on [`PerceptualPolicy::new`], which validates its
//!   range.
//!
//! Cell equality never implies pixel equality: there is no bypass from the
//! cell gate into this engine (C01). [`compare_png_with_flags`] keeps its
//! legacy signature for source compatibility, but the flag is ignored and
//! the full decoded-pixel comparison always runs.
//!
//! [`Frame::diff_cells`]: crate::frame::Frame::diff_cells

use image::{RgbImage, RgbaImage};

/// Pixel-gate failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffError(pub String);

impl std::fmt::Display for DiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pixel diff error: {}", self.0)
    }
}

impl std::error::Error for DiffError {}

/// How the strict gate treats the alpha channel. Explicit by construction:
/// alpha is never silently dropped (C03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlphaPolicy {
    /// Exact RGBA byte identity: dimensions plus every R, G, B **and A**
    /// channel must match. Default. RGB inputs decode with uniform A=255,
    /// so RGB-vs-RGB behaves as RGB-exact.
    #[default]
    StraightRgba,
    /// Both images must be fully opaque (every alpha exactly 255) **and**
    /// RGB-identical. A non-255 alpha anywhere fails the gate: callers that
    /// know their pipeline is opaque assert that here instead of ignoring
    /// the channel.
    Opaque,
}

/// Outcome of one PNG-vs-PNG comparison.
#[derive(Debug)]
pub struct PixelVerdict {
    pub dims_equal: bool,
    pub expected_dims: (u32, u32),
    pub actual_dims: (u32, u32),
    /// THE strict verdict: dimensions equal AND decoded pixels identical
    /// under [`Self::alpha_policy`]. This bit — never [`Self::score`] —
    /// establishes strict equality.
    pub pixels_equal: bool,
    /// The alpha policy this verdict was computed under.
    pub alpha_policy: AlphaPolicy,
    /// 1.0 exactly when [`Self::pixels_equal`]; a perceptual diagnostic
    /// capped strictly below 1.0 otherwise (0.0 when dimensions differ).
    /// Invariant: `score >= 1.0` implies decoded-pixel identity, so legacy
    /// `score < threshold` gates stay sound — but new code must branch on
    /// [`Self::pixels_equal`].
    pub score: f64,
    /// Red-overlay diff image (empty when pixels are equal or dimensions
    /// differ).
    pub diff_png: Vec<u8>,
}

/// Explicit perceptual gate: a similarity threshold in `[0.0, 1.0]`,
/// validated at construction. This is the ONLY place a float threshold
/// exists — the strict gate takes none. Meeting the threshold means
/// "similar enough for review", never "pixel-equal": a perceptual policy
/// cannot report exact equality (C04).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerceptualPolicy {
    threshold: f64,
}

impl PerceptualPolicy {
    /// Validated constructor: rejects NaN, infinities, and anything outside
    /// `[0.0, 1.0]`. An invalid tolerance is an error, never silently
    /// applied (a NaN threshold would make every `score < threshold`
    /// comparison false and fake a match).
    pub fn new(threshold: f64) -> Result<Self, DiffError> {
        if !threshold.is_finite() {
            return Err(DiffError(format!(
                "invalid pixel_threshold {threshold}: must be finite"
            )));
        }
        if !(0.0..=1.0).contains(&threshold) {
            return Err(DiffError(format!(
                "invalid pixel_threshold {threshold}: must be in [0.0, 1.0]"
            )));
        }
        Ok(Self { threshold })
    }

    #[must_use]
    pub fn threshold(self) -> f64 {
        self.threshold
    }

    /// Diagnostic gate only: does this similarity meet the review bar?
    /// Never a claim of pixel equality.
    #[must_use]
    pub fn allows(self, score: f64) -> bool {
        score >= self.threshold
    }
}

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

/// Legacy signature, kept for source compatibility. The `ansi_matched` flag
/// is IGNORED: same cells and dimensions never imply equal pixels (C01), so
/// the full decoded-pixel comparison always runs. New code should call
/// [`compare_png`] or [`compare_png_with_alpha`] directly, and
/// [`perceptual_score`] for review diagnostics.
pub fn compare_png_with_flags(
    expected_png: &[u8],
    actual_png: &[u8],
    _ansi_matched: bool,
) -> Result<PixelVerdict, DiffError> {
    compare_png(expected_png, actual_png)
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;

    fn gradient_rgba() -> RgbaImage {
        let mut img = RgbaImage::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                img.put_pixel(
                    x,
                    y,
                    image::Rgba([(x * 16) as u8, (y * 16) as u8, 128, 255]),
                );
            }
        }
        img
    }

    fn encode(img: &RgbaImage, c: CompressionType, f: FilterType) -> Vec<u8> {
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

    #[test]
    fn strict_recompression_passes_single_channel_fails() {
        let img = gradient_rgba();
        let a = encode(&img, CompressionType::Default, FilterType::Adaptive);
        let b = encode(&img, CompressionType::Best, FilterType::NoFilter);
        assert_ne!(a, b, "setup: encodings must differ");
        let v = compare_png(&a, &b).unwrap();
        assert!(v.pixels_equal);
        assert_eq!(v.score, 1.0);
        assert!(v.diff_png.is_empty());

        let mut one = img.clone();
        let p = *one.get_pixel(7, 7);
        one.put_pixel(7, 7, image::Rgba([p[0].wrapping_add(1), p[1], p[2], p[3]]));
        let v = compare_png(
            &a,
            &encode(&one, CompressionType::Default, FilterType::Adaptive),
        )
        .unwrap();
        assert!(!v.pixels_equal);
        assert!(v.score < 1.0);
        assert!(!v.diff_png.is_empty());
    }

    #[test]
    fn legacy_flag_cannot_skip_pixel_reads() {
        let mut a = gradient_rgba();
        let mut b = gradient_rgba();
        b.put_pixel(3, 5, image::Rgba([255, 0, 0, 255]));
        a.put_pixel(3, 5, image::Rgba([0, 0, 255, 255]));
        let pa = encode(&a, CompressionType::Default, FilterType::Adaptive);
        let pb = encode(&b, CompressionType::Default, FilterType::Adaptive);
        for flag in [false, true] {
            let v = compare_png_with_flags(&pa, &pb, flag).unwrap();
            assert!(!v.pixels_equal, "flag={flag} must not bypass pixels");
            assert!(v.score < 1.0, "flag={flag} score={}", v.score);
        }
    }

    #[test]
    fn alpha_semantics_are_explicit() {
        let mut opaque = RgbaImage::new(8, 8);
        let mut clear = RgbaImage::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                opaque.put_pixel(x, y, image::Rgba([200, 100, 50, 255]));
                clear.put_pixel(x, y, image::Rgba([200, 100, 50, 0]));
            }
        }
        let po = encode(&opaque, CompressionType::Default, FilterType::Adaptive);
        let pc = encode(&clear, CompressionType::Default, FilterType::Adaptive);
        // Straight: alpha bytes differ -> not equal.
        let v = compare_png_with_alpha(&po, &pc, AlphaPolicy::StraightRgba).unwrap();
        assert!(!v.pixels_equal);
        assert!(v.score < 1.0);
        // Opaque: non-255 alpha fails the gate instead of being ignored.
        let v = compare_png_with_alpha(&po, &pc, AlphaPolicy::Opaque).unwrap();
        assert!(!v.pixels_equal);
        // Opaque-vs-opaque RGB-identical passes under both policies.
        for policy in [AlphaPolicy::StraightRgba, AlphaPolicy::Opaque] {
            let v = compare_png_with_alpha(&po, &po, policy).unwrap();
            assert!(v.pixels_equal, "{policy:?} must pass identical opaque");
        }
        // Same semi-transparent pixels: StraightRgba equal, Opaque fails.
        let semi = encode(&clear, CompressionType::Best, FilterType::NoFilter);
        assert!(
            compare_png_with_alpha(&pc, &semi, AlphaPolicy::StraightRgba)
                .unwrap()
                .pixels_equal
        );
        assert!(
            !compare_png_with_alpha(&pc, &semi, AlphaPolicy::Opaque)
                .unwrap()
                .pixels_equal
        );
    }

    #[test]
    fn perceptual_policy_validates_threshold() {
        assert!(PerceptualPolicy::new(0.0).is_ok());
        assert!(PerceptualPolicy::new(1.0).is_ok());
        assert!(PerceptualPolicy::new(0.99).is_ok());
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1, 1.1, 2.0] {
            assert!(
                PerceptualPolicy::new(bad).is_err(),
                "threshold {bad} must be rejected"
            );
        }
        let p = PerceptualPolicy::new(0.99).unwrap();
        assert!(p.allows(1.0));
        assert!(!p.allows(0.5));
    }

    #[test]
    fn perceptual_score_never_proves_equality() {
        // RGB-identical but alpha-differing: perceptual diagnostic is blind
        // to the difference (RGB hybrid 1.0) while strict fails.
        let mut opaque = RgbaImage::new(8, 8);
        let mut clear = RgbaImage::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                opaque.put_pixel(x, y, image::Rgba([10, 20, 30, 255]));
                clear.put_pixel(x, y, image::Rgba([10, 20, 30, 0]));
            }
        }
        let po = encode(&opaque, CompressionType::Default, FilterType::Adaptive);
        let pc = encode(&clear, CompressionType::Default, FilterType::Adaptive);
        let diag = perceptual_score(&po, &pc).unwrap();
        assert_eq!(diag, 1.0, "setup: RGB-hybrid is alpha-blind");
        assert!(!compare_png(&po, &pc).unwrap().pixels_equal);
        // Dimension mismatch has no correspondence: 0.0.
        let tiny = RgbaImage::new(4, 4);
        let pt = encode(&tiny, CompressionType::Default, FilterType::Adaptive);
        assert_eq!(perceptual_score(&po, &pt).unwrap(), 0.0);
    }
}
