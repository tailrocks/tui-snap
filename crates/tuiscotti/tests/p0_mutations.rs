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
use tuiscotti::grouped::GroupedStore;
use tuiscotti::snapshot::Store;
use tuiscotti::{Profile, Provenance};

#[path = "p0_mutations/evidence.rs"]
mod evidence;
#[path = "p0_mutations/integrity.rs"]
mod integrity;
#[path = "p0_mutations/strict.rs"]
mod strict;

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

fn frame_with(text: &str) -> tuiscotti::Frame {
    tuiscotti::ratatui::widget_frame(Paragraph::new(text), 30, 6, prov())
}

fn tmp_classic(tag: &str) -> Result<(tempfile::TempDir, Store), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let st = Store::new(&dir.path().join(tag));
    Ok((dir, st))
}

fn tmp_grouped(tag: &str) -> Result<(tempfile::TempDir, GroupedStore), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let st = GroupedStore::new(&dir.path().join(tag));
    Ok((dir, st))
}

/// 16x16 deterministic gradient (non-trivial bytes so encoder settings matter).
fn gradient_rgb() -> RgbImage {
    let mut img = RgbImage::new(16, 16);
    for y in 0..16u8 {
        for x in 0..16u8 {
            img.put_pixel(
                u32::from(x),
                u32::from(y),
                image::Rgb([x * 16, y * 16, 128]),
            );
        }
    }
    img
}

fn encode_rgb(
    img: &RgbImage,
    c: CompressionType,
    f: FilterType,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut buf = Vec::new();
    PngEncoder::new_with_quality(&mut buf, c, f).write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(buf)
}

fn encode_rgba(
    img: &RgbaImage,
    c: CompressionType,
    f: FilterType,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut buf = Vec::new();
    PngEncoder::new_with_quality(&mut buf, c, f).write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(buf)
}

fn decode_rgb(png: &[u8]) -> Result<RgbImage, Box<dyn std::error::Error>> {
    Ok(image::load_from_memory(png)?.to_rgb8())
}

// ------------------------------------------------- C01: cell-equality bypass
