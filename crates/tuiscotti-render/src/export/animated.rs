//! Animated image exports: GIF plus hand-assembled APNG.

use super::{ApngPolicy, ExportError, GifPolicy};
use std::path::Path;

// ---------------------------------------------------------------------------
// Shared animated-image input handling (A06)
// ---------------------------------------------------------------------------

/// PNG magic bytes; inputs claiming to be PNG frames must carry them.
const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Decode + validate PNG frames: nonempty, delay per frame, PNG magic,
/// decodable, identical nonzero dimensions.
pub(crate) fn decode_png_frames(
    frames_png: &[Vec<u8>],
    delays_ms: &[u32],
    what: &str,
) -> Result<Vec<image::RgbaImage>, ExportError> {
    if frames_png.is_empty() {
        return Err(ExportError::InvalidInput(format!(
            "{what}: need at least one frame"
        )));
    }
    if frames_png.len() != delays_ms.len() {
        return Err(ExportError::InvalidInput(format!(
            "{what}: {} frames but {} delays",
            frames_png.len(),
            delays_ms.len()
        )));
    }
    let mut frames = Vec::with_capacity(frames_png.len());
    let mut dims = None;
    for (i, bytes) in frames_png.iter().enumerate() {
        if bytes.len() < PNG_MAGIC.len() || bytes[..8] != PNG_MAGIC {
            return Err(ExportError::InvalidInput(format!(
                "{what}: frame {i} lacks PNG magic bytes"
            )));
        }
        let img = image::load_from_memory(bytes)
            .map_err(|e| {
                ExportError::InvalidInput(format!("{what}: frame {i} PNG decode failed: {e}"))
            })?
            .to_rgba8();
        if img.width() == 0 || img.height() == 0 {
            return Err(ExportError::InvalidInput(format!(
                "{what}: frame {i} has zero dimensions"
            )));
        }
        match dims {
            None => dims = Some((img.width(), img.height())),
            Some(d) if d == (img.width(), img.height()) => {}
            Some((w, h)) => {
                return Err(ExportError::InvalidInput(format!(
                    "{what}: frame {i} is {}x{} but frame 0 is {w}x{h}",
                    img.width(),
                    img.height()
                )));
            }
        }
        frames.push(img);
    }
    Ok(frames)
}

// ---------------------------------------------------------------------------
// GIF (A06)
// ---------------------------------------------------------------------------

/// Encode PNG frames as an animated GIF.
///
/// Policy (see [`GifPolicy`]): `image` GIF encoder at pinned speed 1 (best
/// quantization quality), infinite repeat, per-frame delays clamped to
/// `[min_delay_ms, 655350]` ms and quantized by the format to 10 ms units.
/// Same PNGs + delays → byte-identical file (qualified by test).
pub fn gif(frames_png: &[Vec<u8>], delays_ms: &[u32], path: &Path) -> Result<(), ExportError> {
    gif_with(frames_png, delays_ms, path, &GifPolicy::default())
}

/// [`gif`] with an explicit policy.
pub fn gif_with(
    frames_png: &[Vec<u8>],
    delays_ms: &[u32],
    path: &Path,
    policy: &GifPolicy,
) -> Result<(), ExportError> {
    if !(1..=30).contains(&policy.speed) {
        return Err(ExportError::InvalidInput(format!(
            "gif speed must be 1..=30, got {}",
            policy.speed
        )));
    }
    let frames = decode_png_frames(frames_png, delays_ms, "gif")?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = image::codecs::gif::GifEncoder::new_with_speed(file, policy.speed);
    if policy.repeat_infinite {
        enc.set_repeat(image::codecs::gif::Repeat::Infinite)
            .map_err(|e| ExportError::Encode(format!("gif repeat extension failed: {e}")))?;
    }
    for (img, delay) in frames.into_iter().zip(delays_ms.iter()) {
        let ms = (*delay).clamp(policy.min_delay_ms.max(1), 655350);
        let frame = image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(ms, 1));
        enc.encode_frame(frame)
            .map_err(|e| ExportError::Encode(format!("gif frame encode failed: {e}")))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// APNG (A06)
// ---------------------------------------------------------------------------

/// Encode PNG frames as an animated PNG.
///
/// `image` 0.25 ships no APNG encoder, so this assembles the file directly:
/// each frame is re-encoded with pinned `PngEncoder(Best, Adaptive)` and the
/// `IDAT` payloads are reframed as `IDAT` (frame 0) / `fdAT` (rest) under one
/// `IHDR` + `acTL(plays)` + per-frame `fcTL` + `IEND`. Frames are full-canvas
/// opaque renders, so `dispose_op` is NONE (0) and `blend_op` is SOURCE (1).
/// Same PNGs + delays → byte-identical file (qualified by test).
pub fn apng(frames_png: &[Vec<u8>], delays_ms: &[u32], path: &Path) -> Result<(), ExportError> {
    apng_with(frames_png, delays_ms, path, &ApngPolicy::default())
}

/// [`apng`] with an explicit policy.
pub fn apng_with(
    frames_png: &[Vec<u8>],
    delays_ms: &[u32],
    path: &Path,
    policy: &ApngPolicy,
) -> Result<(), ExportError> {
    if policy.delay_den == 0 {
        return Err(ExportError::InvalidInput(
            "apng delay denominator must be nonzero".to_string(),
        ));
    }
    let frames = decode_png_frames(frames_png, delays_ms, "apng")?;
    let (w, h) = (frames[0].width(), frames[0].height());
    let mut idats = Vec::with_capacity(frames.len());
    let mut ihdr: Option<[u8; 13]> = None;
    for (i, img) in frames.iter().enumerate() {
        let png = encode_png_rgba(img)?;
        let split = split_png(&png, i)?;
        match ihdr {
            None => ihdr = Some(split.ihdr),
            Some(prev) if prev == split.ihdr => {}
            Some(_) => {
                return Err(ExportError::Encode(format!(
                    "apng: frame {i} IHDR differs from frame 0 (same dims required)"
                )));
            }
        }
        idats.push(split.idat);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&PNG_MAGIC);
    let Some(ihdr) = ihdr else {
        return Err(ExportError::Encode(
            "apng: no frames after decode".to_string(),
        ));
    };
    emit_chunk(b"IHDR", &ihdr, &mut out);
    let mut actl = Vec::with_capacity(8);
    actl.extend_from_slice(&(frames.len() as u32).to_be_bytes());
    actl.extend_from_slice(&policy.plays.to_be_bytes());
    emit_chunk(b"acTL", &actl, &mut out);
    let mut seq: u32 = 0;
    for (i, (idat, delay)) in idats.iter().zip(delays_ms.iter()).enumerate() {
        let num = (*delay).max(1).min(u32::from(u16::MAX)) as u16;
        let mut fctl = Vec::with_capacity(26);
        fctl.extend_from_slice(&seq.to_be_bytes());
        seq = seq.wrapping_add(1);
        fctl.extend_from_slice(&w.to_be_bytes());
        fctl.extend_from_slice(&h.to_be_bytes());
        fctl.extend_from_slice(&0u32.to_be_bytes());
        fctl.extend_from_slice(&0u32.to_be_bytes());
        fctl.extend_from_slice(&num.to_be_bytes());
        fctl.extend_from_slice(&policy.delay_den.to_be_bytes());
        fctl.push(0); // dispose_op: APNG_DISPOSE_OP_NONE
        fctl.push(1); // blend_op: APNG_BLEND_OP_SOURCE
        emit_chunk(b"fcTL", &fctl, &mut out);
        if i == 0 {
            emit_chunk(b"IDAT", idat, &mut out);
        } else {
            let mut fdat = Vec::with_capacity(4 + idat.len());
            fdat.extend_from_slice(&seq.to_be_bytes());
            seq = seq.wrapping_add(1);
            fdat.extend_from_slice(idat);
            emit_chunk(b"fdAT", &fdat, &mut out);
        }
    }
    emit_chunk(b"IEND", &[], &mut out);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, out)?;
    Ok(())
}

/// Re-encode one RGBA frame with the pinned PNG settings.
fn encode_png_rgba(img: &image::RgbaImage) -> Result<Vec<u8>, ExportError> {
    use image::ImageEncoder as _;
    let mut buf = Vec::new();
    let enc = image::codecs::png::PngEncoder::new_with_quality(
        &mut buf,
        image::codecs::png::CompressionType::Best,
        image::codecs::png::FilterType::Adaptive,
    );
    enc.write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| ExportError::Encode(format!("apng frame PNG encode failed: {e}")))?;
    Ok(buf)
}

struct SplitPng {
    ihdr: [u8; 13],
    idat: Vec<u8>,
}

/// Split an encoder-produced PNG into its IHDR bytes and concatenated IDAT
/// payload. Ancillary chunks are dropped (deterministic minimal layout).
fn split_png(png: &[u8], frame: usize) -> Result<SplitPng, ExportError> {
    let bad = |m: String| ExportError::Encode(format!("apng frame {frame} PNG split: {m}"));
    if png.len() < 8 || png[..8] != PNG_MAGIC {
        return Err(bad("missing PNG magic".to_string()));
    }
    let mut ihdr: Option<[u8; 13]> = None;
    let mut idat = Vec::new();
    let mut pos = 8;
    loop {
        if pos + 8 > png.len() {
            return Err(bad("truncated chunk header".to_string()));
        }
        let len = u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
        let tag = &png[pos + 4..pos + 8];
        let data_start = pos + 8;
        let data_end = data_start
            .checked_add(len)
            .ok_or_else(|| bad("length overflow".to_string()))?;
        if data_end + 4 > png.len() {
            return Err(bad(format!(
                "truncated {} chunk",
                String::from_utf8_lossy(tag)
            )));
        }
        match tag {
            b"IHDR" => {
                if len != 13 {
                    return Err(bad(format!("IHDR length {len} != 13")));
                }
                let mut arr = [0u8; 13];
                arr.copy_from_slice(&png[data_start..data_end]);
                ihdr = Some(arr);
            }
            b"IDAT" => idat.extend_from_slice(&png[data_start..data_end]),
            b"IEND" => break,
            _ => {}
        }
        pos = data_end + 4;
    }
    match ihdr {
        Some(ihdr) if !idat.is_empty() => Ok(SplitPng { ihdr, idat }),
        Some(_) => Err(bad("no IDAT payload".to_string())),
        None => Err(bad("no IHDR chunk".to_string())),
    }
}

fn emit_chunk(tag: &[u8; 4], data: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(tag);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32_ieee(&crc_input).to_be_bytes());
}

/// CRC-32 (IEEE 802.3, polynomial 0xEDB88320), bitwise. No `crc` dependency;
/// evidence frames are small enough that table-less speed is irrelevant.
fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
