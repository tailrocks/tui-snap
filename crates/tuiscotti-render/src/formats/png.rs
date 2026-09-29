//! PNG: independently rendered pixels with explicit alpha/profile behavior.
//!
//! PNG evidence is produced by [`Renderer`](crate::render::Renderer) from
//! canonical state — it is an independent re-render, never a re-encode of
//! application bytes. Explicit contract:
//!
//! - color type 2 (truecolor RGB), bit depth 8, **no alpha channel**:
//!   terminal grids are fully opaque, so every pixel's alpha would be 255
//!   and carrying a channel would only hide encoder differences;
//! - deterministic bytes for identical frame + profile (pinned fonts,
//!   pinned geometry, no timestamps or ancillary chunks);
//! - same profile required on both sides of any comparison: the
//!   [`Generation`](crate::formats::Generation) binds the profile name.
//!
//! Pixel equality is decided on *decoded* pixels
//! ([`changed_pixels`], [`crate::diff::compare_png`]), never by assuming
//! equal bytes. Byte-identity of our own deterministic encoder is asserted
//! separately, so an encoder change reads as a renderer change.

use crate::render::RenderError;

/// PNG magic bytes.
const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Parsed PNG header fields relevant to the evidence contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PngInfo {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Bits per channel; evidence requires 8.
    pub bit_depth: u8,
    /// PNG color type; evidence requires 2 (truecolor RGB, no alpha).
    pub color_type: u8,
}

/// Parse the `IHDR` of `png` without decoding pixels.
pub fn png_info(png: &[u8]) -> Result<PngInfo, RenderError> {
    let bad = |m: &str| RenderError(format!("bad PNG header: {m}"));
    if png.len() < 8 || png[..8] != PNG_MAGIC {
        return Err(bad("missing PNG magic bytes"));
    }
    if png.len() < 33 {
        return Err(bad("truncated before IHDR"));
    }
    let len = u32::from_be_bytes([png[8], png[9], png[10], png[11]]);
    if &png[12..16] != b"IHDR" || len != 13 {
        return Err(bad("first chunk is not a 13-byte IHDR"));
    }
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    Ok(PngInfo {
        width,
        height,
        bit_depth: png[24],
        color_type: png[25],
    })
}

/// Fail unless `png` is opaque RGB evidence: color type 2, bit depth 8,
/// nonzero dimensions.
pub fn assert_opaque_rgb(png: &[u8]) -> Result<PngInfo, RenderError> {
    let info = png_info(png)?;
    if info.color_type != 2 {
        return Err(RenderError(format!(
            "PNG color type {} is not opaque RGB (want 2)",
            info.color_type
        )));
    }
    if info.bit_depth != 8 {
        return Err(RenderError(format!(
            "PNG bit depth {} is not 8",
            info.bit_depth
        )));
    }
    if info.width == 0 || info.height == 0 {
        return Err(RenderError("PNG has zero dimensions".to_string()));
    }
    Ok(info)
}

/// Decode `png` to an RGB image (any input color type accepted for
/// *comparison* inputs; evidence *outputs* still go through
/// [`assert_opaque_rgb`]).
pub fn decode_rgb(png: &[u8]) -> Result<image::RgbImage, RenderError> {
    image::load_from_memory(png)
        .map_err(|e| RenderError(format!("PNG decode failed: {e}")))
        .map(|d| d.to_rgb8())
}

/// Decoded-pixel difference: positions where `a` and `b` differ after
/// decoding. Dimensions must match; a dimension mismatch is an error, not
/// a diff. Empty output means pixel-identical.
pub fn changed_pixels(a: &[u8], b: &[u8]) -> Result<Vec<(u32, u32)>, RenderError> {
    let ia = decode_rgb(a)?;
    let ib = decode_rgb(b)?;
    if ia.dimensions() != ib.dimensions() {
        return Err(RenderError(format!(
            "PNG dimension mismatch: {:?} vs {:?}",
            ia.dimensions(),
            ib.dimensions()
        )));
    }
    let mut out = Vec::new();
    for (x, y, pa) in ia.enumerate_pixels() {
        if pa != ib.get_pixel(x, y) {
            out.push((x, y));
        }
    }
    Ok(out)
}
