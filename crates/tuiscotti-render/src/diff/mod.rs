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
//! cell gate into this engine (C01) — the full decoded-pixel comparison
//! always runs.
//!
//! [`Frame::diff_cells`]: tuiscotti_core::frame::Frame::diff_cells

pub mod compare;
pub mod types;

pub use compare::{compare_png, compare_png_with_alpha, perceptual_score};
pub use types::{AlphaPolicy, DiffError, PerceptualPolicy, PixelVerdict};

#[cfg(test)]
mod tests {
    use crate::diff::*;
    use image::ImageEncoder;
    use image::RgbaImage;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};

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

    fn encode(img: &RgbaImage, c: CompressionType, f: FilterType) -> Result<Vec<u8>, DiffError> {
        let mut buf = Vec::new();
        PngEncoder::new_with_quality(&mut buf, c, f)
            .write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| DiffError(format!("test setup: PNG encode failed: {e}")))?;
        Ok(buf)
    }

    #[test]
    fn strict_recompression_passes_single_channel_fails() -> Result<(), DiffError> {
        let img = gradient_rgba();
        let a = encode(&img, CompressionType::Default, FilterType::Adaptive)?;
        let b = encode(&img, CompressionType::Best, FilterType::NoFilter)?;
        assert_ne!(a, b, "setup: encodings must differ");
        let v = compare_png(&a, &b)?;
        assert!(v.pixels_equal);
        assert_eq!(v.score, 1.0);
        assert!(v.diff_png.is_empty());

        let mut one = img.clone();
        let p = *one.get_pixel(7, 7);
        one.put_pixel(7, 7, image::Rgba([p[0].wrapping_add(1), p[1], p[2], p[3]]));
        let one_png = encode(&one, CompressionType::Default, FilterType::Adaptive)?;
        let v = compare_png(&a, &one_png)?;
        assert!(!v.pixels_equal);
        assert!(v.score < 1.0);
        assert!(!v.diff_png.is_empty());
        Ok(())
    }

    #[test]
    fn differing_pixels_always_fail() -> Result<(), DiffError> {
        let mut a = gradient_rgba();
        let mut b = gradient_rgba();
        b.put_pixel(3, 5, image::Rgba([255, 0, 0, 255]));
        a.put_pixel(3, 5, image::Rgba([0, 0, 255, 255]));
        let pa = encode(&a, CompressionType::Default, FilterType::Adaptive)?;
        let pb = encode(&b, CompressionType::Default, FilterType::Adaptive)?;
        let v = compare_png(&pa, &pb)?;
        assert!(!v.pixels_equal);
        assert!(v.score < 1.0, "score={}", v.score);
        Ok(())
    }

    #[test]
    fn alpha_semantics_are_explicit() -> Result<(), DiffError> {
        let mut opaque = RgbaImage::new(8, 8);
        let mut clear = RgbaImage::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                opaque.put_pixel(x, y, image::Rgba([200, 100, 50, 255]));
                clear.put_pixel(x, y, image::Rgba([200, 100, 50, 0]));
            }
        }
        let po = encode(&opaque, CompressionType::Default, FilterType::Adaptive)?;
        let pc = encode(&clear, CompressionType::Default, FilterType::Adaptive)?;
        // Straight: alpha bytes differ -> not equal.
        let v = compare_png_with_alpha(&po, &pc, AlphaPolicy::StraightRgba)?;
        assert!(!v.pixels_equal);
        assert!(v.score < 1.0);
        // Opaque: non-255 alpha fails the gate instead of being ignored.
        let v = compare_png_with_alpha(&po, &pc, AlphaPolicy::Opaque)?;
        assert!(!v.pixels_equal);
        // Opaque-vs-opaque RGB-identical passes under both policies.
        for policy in [AlphaPolicy::StraightRgba, AlphaPolicy::Opaque] {
            let v = compare_png_with_alpha(&po, &po, policy)?;
            assert!(v.pixels_equal, "{policy:?} must pass identical opaque");
        }
        // Same semi-transparent pixels: StraightRgba equal, Opaque fails.
        let semi = encode(&clear, CompressionType::Best, FilterType::NoFilter)?;
        assert!(compare_png_with_alpha(&pc, &semi, AlphaPolicy::StraightRgba)?.pixels_equal);
        assert!(!compare_png_with_alpha(&pc, &semi, AlphaPolicy::Opaque)?.pixels_equal);
        Ok(())
    }

    #[test]
    fn perceptual_policy_validates_threshold() -> Result<(), DiffError> {
        assert!(PerceptualPolicy::new(0.0).is_ok());
        assert!(PerceptualPolicy::new(1.0).is_ok());
        assert!(PerceptualPolicy::new(0.99).is_ok());
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1, 1.1, 2.0] {
            assert!(
                PerceptualPolicy::new(bad).is_err(),
                "threshold {bad} must be rejected"
            );
        }
        let p = PerceptualPolicy::new(0.99)?;
        assert!(p.allows(1.0));
        assert!(!p.allows(0.5));
        Ok(())
    }

    #[test]
    fn perceptual_score_never_proves_equality() -> Result<(), DiffError> {
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
        let po = encode(&opaque, CompressionType::Default, FilterType::Adaptive)?;
        let pc = encode(&clear, CompressionType::Default, FilterType::Adaptive)?;
        let diag = perceptual_score(&po, &pc)?;
        assert_eq!(diag, 1.0, "setup: RGB-hybrid is alpha-blind");
        assert!(!compare_png(&po, &pc)?.pixels_equal);
        // Dimension mismatch has no correspondence: 0.0.
        let tiny = RgbaImage::new(4, 4);
        let pt = encode(&tiny, CompressionType::Default, FilterType::Adaptive)?;
        assert_eq!(perceptual_score(&po, &pt)?, 0.0);
        Ok(())
    }
}
