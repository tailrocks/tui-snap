//! Evidence exports + graphics inspection (backlog A06, A07).
//!
//! - [`cast_v2`]: asciinema v2 `.cast` from rendered-text frames.
//! - [`gif`] / [`apng`]: animated image exports from PNG frames via the
//!   `image` crate (GIF) plus hand-assembled APNG chunk surgery (`image` 0.25
//!   ships an APNG *decoder* but no APNG encoder).
//! - [`mp4`]: MP4 via an EXTERNAL `ffmpeg` binary only (never vendored).
//! - [`scan_graphics`]: Sixel + Kitty payload inspection with bounded decode.
//!
//! ## Determinism table
//!
//! | Format | Byte-deterministic | Notes |
//! |---|---|---|
//! | Cast (`.cast`) | YES | pinned header (`timestamp` 0), fixed `{:.6}` event times, fixed `session.cast` name |
//! | GIF | YES (qualified by `tests/export.rs`) | pinned speed 1, repeat infinite; delays quantized to 10 ms units (GIF format limit), clamped to [10, 655350] ms |
//! | APNG | YES | frames encoded with pinned `PngEncoder(Best, Adaptive)`, single IHDR/acTL/fcTL/IDAT-fdAT/IEND layout, CRC32; delays exact milliseconds (`delay_den` 1000), clamped to [1, 65535] ms |
//! | MP4 | NO | external encoder; flags pinned but output depends on the ffmpeg build; version recorded in the sidecar |
//! | `placement_equality` | YES (exact compare) | kind + params + bytes + placement; stream offsets excluded like provenance |
//! | composited-image equality | UNQUALIFIED — NOT IMPLEMENTED | no compositor exists; never claim coverage |
//!
//! [`ExportPolicies`] pins every knob above; each `*_with` constructor takes a
//! policy so pins are explicit at the call site.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Export failure: explicit, never silent.
#[derive(Debug)]
pub enum ExportError {
    /// Filesystem failure.
    Io(std::io::Error),
    /// Encoder failure (internal encoder or external `ffmpeg`).
    Encode(String),
    /// Caller input rejected (empty frames, length mismatch, bad dims, ...).
    InvalidInput(String),
    /// The external encoder binary is not installed.
    EncoderMissing {
        /// Binary name (`ffmpeg`).
        tool: &'static str,
        /// Where/how to install it.
        install: &'static str,
    },
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Io(e) => write!(f, "export I/O error: {e}"),
            ExportError::Encode(e) => write!(f, "export encode error: {e}"),
            ExportError::InvalidInput(e) => write!(f, "export invalid input: {e}"),
            ExportError::EncoderMissing { tool, install } => {
                write!(f, "export encoder missing: `{tool}` not found ({install})")
            }
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExportError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ExportError {
    fn from(e: std::io::Error) -> Self {
        ExportError::Io(e)
    }
}

impl ExportError {
    /// True only for the missing-external-encoder path.
    #[must_use]
    pub fn is_encoder_missing(&self) -> bool {
        matches!(self, ExportError::EncoderMissing { .. })
    }
}

// ---------------------------------------------------------------------------
// Policies (all pins in one place)
// ---------------------------------------------------------------------------

/// Pinned asciinema header fields.
#[derive(Debug, Clone)]
pub struct CastPolicy {
    /// `title` header value.
    pub title: String,
    /// `env.TERM` header value.
    pub term: String,
    /// `timestamp` header value. Pinned 0: wall-clock time must never enter
    /// evidence bytes.
    pub timestamp: u64,
}

impl Default for CastPolicy {
    fn default() -> Self {
        Self {
            title: "tuisnap".to_string(),
            term: "tuisnap".to_string(),
            timestamp: 0,
        }
    }
}

/// Pinned GIF encoder policy.
#[derive(Debug, Clone)]
pub struct GifPolicy {
    /// `gif` crate quantization speed (1..=30). Pinned 1 = best quality.
    pub speed: i32,
    /// Emit an infinite-repeat extension. Pinned true.
    pub repeat_infinite: bool,
    /// Delays below this clamp up (GIF stores 10 ms units; 0 reads as
    /// "unspecified" in players). Pinned 10.
    pub min_delay_ms: u32,
}

impl Default for GifPolicy {
    fn default() -> Self {
        Self {
            speed: 1,
            repeat_infinite: true,
            min_delay_ms: 10,
        }
    }
}

/// Pinned APNG assembly policy.
#[derive(Debug, Clone)]
pub struct ApngPolicy {
    /// `acTL` play count (0 = infinite). Pinned 0.
    pub plays: u32,
    /// `fcTL` delay denominator (delays are exact milliseconds). Must be
    /// nonzero. Pinned 1000.
    pub delay_den: u16,
}

impl Default for ApngPolicy {
    fn default() -> Self {
        Self {
            plays: 0,
            delay_den: 1000,
        }
    }
}

/// Pinned external-`ffmpeg` invocation policy.
#[derive(Debug, Clone)]
pub struct Mp4Policy {
    /// x264 constant-rate factor. Pinned 23.
    pub crf: u8,
    /// x264 preset. Pinned `medium`.
    pub preset: String,
    /// Output pixel format. Pinned `yuv420p` (widest player support).
    pub pix_fmt: String,
}

impl Default for Mp4Policy {
    fn default() -> Self {
        Self {
            crf: 23,
            preset: "medium".to_string(),
            pix_fmt: "yuv420p".to_string(),
        }
    }
}

/// Bounds for graphics inspection and decode. Everything that crosses an
/// untrusted byte stream is capped; truncation is explicit, never silent.
#[derive(Debug, Clone)]
pub struct GraphicsPolicy {
    /// Max payload bytes kept per image (decoded bytes for Kitty, sixel
    /// source bytes for Sixel). Pinned 1 MiB.
    pub max_payload_bytes: usize,
    /// Max decoded image dimension (either axis). Pinned 4096.
    pub max_dim: u32,
    /// Max decoded pixels (`width * height`). Pinned 2^24.
    pub max_pixels: u64,
    /// Max payloads per scan; the scan stops with a diagnostic past this.
    pub max_payloads: usize,
    /// Max transmitted-image table entries (`a=t` + `i=<id>`). Past this,
    /// images are still inspected but not retained for `a=p` references.
    pub max_images: usize,
}

impl Default for GraphicsPolicy {
    fn default() -> Self {
        Self {
            max_payload_bytes: 1 << 20,
            max_dim: 4096,
            max_pixels: 1 << 24,
            max_payloads: 1024,
            max_images: 64,
        }
    }
}

/// All export pins. `Default` is the qualified gate configuration.
#[derive(Debug, Clone, Default)]
pub struct ExportPolicies {
    pub cast: CastPolicy,
    pub gif: GifPolicy,
    pub apng: ApngPolicy,
    pub mp4: Mp4Policy,
    pub graphics: GraphicsPolicy,
}

// ---------------------------------------------------------------------------
// JSON string escaping (hand-rolled: fixed output, no dependency surface)
// ---------------------------------------------------------------------------

fn json_escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    json_escape(s, &mut out);
    out
}

// ---------------------------------------------------------------------------
// asciinema v2 cast (A06)
// ---------------------------------------------------------------------------

/// Fixed output file name inside the target directory.
pub const CAST_FILE_NAME: &str = "session.cast";

/// Write an asciinema v2 `.cast` document: `frames` are
/// `(Screen-rendered text, seconds since the previous frame)` pairs and
/// `cols`/`rows` are the terminal dimensions for the pinned header.
///
/// Event times are cumulative (`dt` sums) printed with fixed `{:.6}`
/// precision; the header timestamp is pinned 0 (see [`CastPolicy`]). Same
/// input → byte-identical file. Creates `dir` when missing.
pub fn cast_v2(
    frames: &[(String, f64)],
    cols: u16,
    rows: u16,
    dir: &Path,
) -> Result<PathBuf, ExportError> {
    cast_v2_with(frames, cols, rows, dir, &CastPolicy::default())
}

/// [`cast_v2`] with an explicit header policy.
pub fn cast_v2_with(
    frames: &[(String, f64)],
    cols: u16,
    rows: u16,
    dir: &Path,
    policy: &CastPolicy,
) -> Result<PathBuf, ExportError> {
    if cols == 0 || rows == 0 {
        return Err(ExportError::InvalidInput(format!(
            "cast dimensions must be nonzero, got {cols}x{rows}"
        )));
    }
    for (i, (_, dt)) in frames.iter().enumerate() {
        if !dt.is_finite() || *dt < 0.0 {
            return Err(ExportError::InvalidInput(format!(
                "cast frame {i} has non-finite or negative dt ({dt})"
            )));
        }
    }
    let mut doc = String::new();
    doc.push_str(&format!(
        "{{\"version\":2,\"width\":{cols},\"height\":{rows},\"timestamp\":{},\"title\":{},\"env\":{{\"TERM\":{}}}}}\n",
        policy.timestamp,
        json_string(&policy.title),
        json_string(&policy.term),
    ));
    let mut t = 0.0f64;
    for (text, dt) in frames {
        t += dt;
        doc.push_str(&format!("[{t:.6},\"o\",{}]\n", json_string(text)));
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join(CAST_FILE_NAME);
    std::fs::write(&path, doc)?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Shared animated-image input handling (A06)
// ---------------------------------------------------------------------------

/// PNG magic bytes; inputs claiming to be PNG frames must carry them.
const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Decode + validate PNG frames: nonempty, delay per frame, PNG magic,
/// decodable, identical nonzero dimensions.
fn decode_png_frames(
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
                )))
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
                )))
            }
        }
        idats.push(split.idat);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&PNG_MAGIC);
    emit_chunk(b"IHDR", &ihdr.expect("at least one frame"), &mut out);
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

// ---------------------------------------------------------------------------
// MP4 via external ffmpeg (A06)
// ---------------------------------------------------------------------------

/// Install hint carried by [`ExportError::EncoderMissing`] for `ffmpeg`.
pub const FFMPEG_INSTALL: &str = "install ffmpeg: https://ffmpeg.org/download.html (Debian/Ubuntu: `apt install ffmpeg`; macOS: `brew install ffmpeg`)";

/// Sidecar record of one MP4 encode: output paths plus the exact encoder
/// identity (MP4 bytes are NOT deterministic across ffmpeg builds).
#[derive(Debug, Clone)]
pub struct Mp4Sidecar {
    /// The encoded video.
    pub mp4: PathBuf,
    /// JSON sidecar next to it (`<name>.ffmpeg.json`).
    pub sidecar: PathBuf,
    /// First line of `ffmpeg -version` output.
    pub ffmpeg_version: String,
}

/// Probe for an external `ffmpeg`: run `ffmpeg -version`, return its first
/// output line. Missing binary → [`ExportError::EncoderMissing`].
pub fn ffmpeg_version() -> Result<String, ExportError> {
    match std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
    {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ExportError::EncoderMissing {
            tool: "ffmpeg",
            install: FFMPEG_INSTALL,
        }),
        Err(e) => Err(ExportError::Encode(format!("ffmpeg probe failed: {e}"))),
        Ok(out) if !out.status.success() => Err(ExportError::Encode(format!(
            "ffmpeg -version exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ))),
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            Ok(stdout
                .lines()
                .next()
                .unwrap_or("ffmpeg (empty version output)")
                .to_string())
        }
    }
}

/// Encode PNG frames as MP4 via an EXTERNAL `ffmpeg` binary.
///
/// Pipeline: PNGs are staged verbatim plus a concat-demuxer playlist into a
/// sibling `<name>.mp4frames/` directory, then `ffmpeg` runs with pinned
/// flags (`-c:v libx264 -pix_fmt <pix_fmt> -crf <crf> -preset <preset>
/// `-movflags +faststart`). The staging directory is removed on success and
/// KEPT on failure (named in the error) for diagnosis. A `<name>.ffmpeg.json`
/// sidecar records the ffmpeg version line, argv, frames, and dims.
///
/// Output bytes are explicitly NOT deterministic: they depend on the ffmpeg
/// build (encoder version, platform SIMD). Only the sidecar pins identity.
pub fn mp4(
    frames_png: &[Vec<u8>],
    delays_ms: &[u32],
    path: &Path,
) -> Result<Mp4Sidecar, ExportError> {
    mp4_with(frames_png, delays_ms, path, &Mp4Policy::default())
}

/// [`mp4`] with an explicit policy.
pub fn mp4_with(
    frames_png: &[Vec<u8>],
    delays_ms: &[u32],
    path: &Path,
    policy: &Mp4Policy,
) -> Result<Mp4Sidecar, ExportError> {
    if policy.crf > 51 {
        return Err(ExportError::InvalidInput(format!(
            "mp4 crf must be 0..=51, got {}",
            policy.crf
        )));
    }
    if policy.preset.is_empty() || policy.pix_fmt.is_empty() {
        return Err(ExportError::InvalidInput(
            "mp4 preset and pix_fmt must be nonempty".to_string(),
        ));
    }
    let version = ffmpeg_version()?;
    let frames = decode_png_frames(frames_png, delays_ms, "mp4")?;
    let (w, h) = (frames[0].width(), frames[0].height());
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let staging = path.with_extension("mp4frames");
    std::fs::create_dir_all(&staging)?;
    for (i, png) in frames_png.iter().enumerate() {
        std::fs::write(staging.join(format!("f{i:06}.png")), png)?;
    }
    // Concat demuxer playlist: each file followed by its display duration;
    // the final file is repeated so the last duration takes effect.
    let mut list = String::new();
    for (i, delay) in delays_ms.iter().enumerate() {
        list.push_str(&format!("file 'f{i:06}.png'\n"));
        list.push_str(&format!("duration {}\n", format_secs(*delay)));
    }
    list.push_str(&format!("file 'f{:06}.png'\n", frames_png.len() - 1));
    std::fs::write(staging.join("list.txt"), &list)?;
    // Absolute output: the child runs with cwd=staging.
    let out_abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|c| c.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let args = [
        "-y".to_string(),
        "-f".to_string(),
        "concat".to_string(),
        "-safe".to_string(),
        "0".to_string(),
        "-i".to_string(),
        "list.txt".to_string(),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-pix_fmt".to_string(),
        policy.pix_fmt.clone(),
        "-crf".to_string(),
        policy.crf.to_string(),
        "-preset".to_string(),
        policy.preset.clone(),
        "-movflags".to_string(),
        "+faststart".to_string(),
        out_abs.to_string_lossy().into_owned(),
    ];
    let run = std::process::Command::new("ffmpeg")
        .args(&args)
        .current_dir(&staging)
        .output()
        .map_err(|e| ExportError::Encode(format!("ffmpeg encode spawn failed: {e}")))?;
    if !run.status.success() {
        let mut stderr = String::from_utf8_lossy(&run.stderr).into_owned();
        if stderr.len() > 2048 {
            stderr = format!("...<truncated>...{}", &stderr[stderr.len() - 2048..]);
        }
        return Err(ExportError::Encode(format!(
            "ffmpeg exited {} (staging kept at {}): {stderr}",
            run.status,
            staging.display()
        )));
    }
    let sidecar_path = path.with_extension("ffmpeg.json");
    let mut sidecar = String::from("{\n");
    sidecar.push_str("  \"tool\": \"ffmpeg\",\n");
    sidecar.push_str(&format!("  \"version\": {},\n", json_string(&version)));
    sidecar.push_str("  \"args\": [");
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            sidecar.push_str(", ");
        }
        sidecar.push_str(&json_string(a));
    }
    sidecar.push_str(&format!(
        "],\n  \"frames\": {},\n  \"width\": {w},\n  \"height\": {h},\n  \"deterministic\": false\n}}\n",
        frames.len()
    ));
    std::fs::write(&sidecar_path, &sidecar)?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(Mp4Sidecar {
        mp4: path.to_path_buf(),
        sidecar: sidecar_path,
        ffmpeg_version: version,
    })
}

/// `delays_ms` → concat-demuxer seconds with exact millisecond precision.
fn format_secs(ms: u32) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

// ---------------------------------------------------------------------------
// Graphics protocol inspection (A07)
// ---------------------------------------------------------------------------

/// Inspected graphics protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsKind {
    /// Sixel (`DCS ... q <data> ST`).
    Sixel,
    /// Kitty graphics (`APC G ... ST`, possibly chunked with `m=1`).
    Kitty,
}

/// Where/how an image is placed. Semantics follow the Kitty graphics
/// protocol ([spec](https://sw.kovidgoyal.net/kitty/graphics-protocol/)):
/// display starts at the cursor cell plus `X`/`Y` pixel offsets; `c`/`r`
/// size the display area in cells; `w`/`h` in pixels; `z` stacks. Sixel has
/// no placement keys (cursor-relative, no z), so its cell/z fields are `None`
/// and only raster dims (when the `"Ph;Pv` attributes are present) survive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Placement {
    /// 0-based placement cell. `None` = cursor-relative (Kitty display and
    /// all Sixel: the byte stream carries no cursor tracking).
    pub col: Option<u32>,
    /// 0-based placement row. See [`Placement::col`].
    pub row: Option<u32>,
    /// Kitty `X` pixel offset within the cell (display actions only).
    pub dx_px: u32,
    /// Kitty `Y` pixel offset within the cell (display actions only).
    pub dy_px: u32,
    /// Kitty `z` stacking order. `None` when the action carries no stacking
    /// meaning (animation `f`/`a`/`c` use `z` as frame gap, `d`/`q` are
    /// commands) and always for Sixel.
    pub z: Option<i32>,
    /// Image pixel dims: Kitty `s`/`v`, Sixel raster `Ph`/`Pv`.
    pub image_px: Option<(u32, u32)>,
    /// Kitty `c`/`r` display area in cells (display actions only).
    pub display_cells: Option<(u32, u32)>,
    /// Kitty `w`/`h` display area in pixels (display actions only).
    pub display_px: Option<(u32, u32)>,
}

/// One inspected graphics image/command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsPayload {
    /// Which protocol produced this.
    pub kind: GraphicsKind,
    /// Raw parameters in stream order. Kitty: first-chunk header pairs
    /// (minus the `m` chunking marker). Sixel: the DCS parameter string as
    /// `("P", <raw>)` plus `("Ph", ..)` / `("Pv", ..)` when raster
    /// attributes parsed.
    pub params: Vec<(String, String)>,
    /// Payload bytes (bounded): Kitty = reassembled base64-decoded bytes
    /// across `m=1` chunks (or the referenced transmitted image for `a=p`
    /// by id); Sixel = raw sixel source bytes.
    pub data: Vec<u8>,
    /// True when `data` was cut at `max_payload_bytes` (a diagnostic is
    /// always recorded alongside; truncated payloads never decode).
    pub truncated: bool,
    /// Kitty `a=p` display of a previously transmitted image id with no
    /// inline data. `data` holds the referenced bytes when the id resolved,
    /// else is empty with an `UnknownReference` diagnostic.
    pub references: Option<u32>,
    /// Preserved placement (cell, z, sizes).
    pub placement: Placement,
    /// Byte offset of the introducer (first chunk for chunked Kitty).
    /// Positional metadata: excluded from [`GraphicsPayload::placement_equality`].
    pub stream_offset: usize,
}

impl GraphicsPayload {
    /// Kitty action key (`a`, default `t`); `None` for Sixel.
    #[must_use]
    pub fn action(&self) -> Option<&str> {
        if self.kind != GraphicsKind::Kitty {
            return None;
        }
        self.params
            .iter()
            .find(|(k, _)| k == "a")
            .map(|(_, v)| v.as_str())
    }

    /// Raw parameter lookup (first match); `None` when absent.
    #[must_use]
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Byte/placement-level equality: kind, params, data, truncation flag,
    /// references, and placement must match exactly. Stream offsets are
    /// excluded (positional metadata, like provenance).
    ///
    /// This is NOT composited-image equality: it says nothing about what the
    /// image looks like blended over terminal content at some z-order — that
    /// comparison is UNQUALIFIED (no compositor exists) and deliberately has
    /// no constructor here.
    #[must_use]
    pub fn placement_equality(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.params == other.params
            && self.data == other.data
            && self.truncated == other.truncated
            && self.references == other.references
            && self.placement == other.placement
    }
}

/// Decoded image pixels: always RGBA8, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

impl DecodedImage {
    /// Borrow as an `image` buffer (panics only if the bytes miscount —
    /// constructors guarantee the length).
    #[must_use]
    pub fn image(&self) -> image::RgbaImage {
        image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone())
            .expect("decoded image length is width*height*4 by construction")
    }
}

/// Bounded-decode failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphicsDecodeError {
    /// Payload was truncated at inspection; refusing to decode partial data.
    Truncated,
    /// Format variant outside the decoder (e.g. Kitty `f=` other than
    /// 24/32/100, Sixel HLS/RGB type beyond 1/2).
    UnsupportedFormat(String),
    /// Kitty non-direct transmission medium (`t=f`/`t=t`/`t=s`): the bytes
    /// live in a file or shm object this inspector never touches.
    UnsupportedMedium(String),
    /// Required dimensions missing (Kitty `f=24`/`f=32` without `s`/`v`).
    MissingDims,
    /// Decoded dims exceed the policy (`TooLarge { w, h, max }` carries the
    /// claimed dims and `max_dim`; pixel-count overflow names `max_pixels`).
    TooLarge { w: u32, h: u32, max: u32 },
    /// Bytes do not parse (bad PNG, short raw buffer, bad sixel operator...).
    InvalidData(String),
    /// Sixel plotted with a color register that was never defined (strict:
    /// no assumed default palette).
    UndefinedColor(u16),
    /// Kitty `a=p` references an image id never transmitted in this scan.
    UnknownReference(u32),
}

impl std::fmt::Display for GraphicsDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphicsDecodeError::Truncated => {
                write!(f, "graphics payload truncated; refusing partial decode")
            }
            GraphicsDecodeError::UnsupportedFormat(m) => {
                write!(f, "unsupported graphics format: {m}")
            }
            GraphicsDecodeError::UnsupportedMedium(m) => {
                write!(f, "unsupported graphics medium: {m}")
            }
            GraphicsDecodeError::MissingDims => {
                write!(f, "graphics decode needs dimensions the payload lacks")
            }
            GraphicsDecodeError::TooLarge { w, h, max } => {
                write!(
                    f,
                    "graphics image {w}x{h} exceeds max dim/pixels (max {max})"
                )
            }
            GraphicsDecodeError::InvalidData(m) => write!(f, "invalid graphics data: {m}"),
            GraphicsDecodeError::UndefinedColor(r) => {
                write!(f, "sixel uses undefined color register {r}")
            }
            GraphicsDecodeError::UnknownReference(id) => {
                write!(f, "kitty display references untransmitted image id {id}")
            }
        }
    }
}

impl std::error::Error for GraphicsDecodeError {}

/// Inspection finding that is not a payload. Unsupported sequences always
/// land here — inspection is never silently lossy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsDiagKind {
    /// Recognized-but-uninspected graphics (non-sixel DCS, non-`G` APC,
    /// iTerm2 OSC 1337, non-direct Kitty media is decode-time instead).
    Unsupported,
    /// A bound cut content (payload bytes, payload/image-table counts).
    Truncated,
    /// Bytes violate the protocol (unterminated introducer, bad chunking,
    /// bad base64, unparseable keys...).
    Malformed,
    /// Kitty `a=p` names an image id with no transmitted bytes in this scan.
    UnknownReference,
}

/// One inspection diagnostic: byte offset + kind + human message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsDiagnostic {
    pub offset: usize,
    pub kind: GraphicsDiagKind,
    pub message: String,
}

/// Full scan result: payloads plus every diagnostic. Non-graphics bytes
/// (plain text, SGR, non-1337 OSC such as titles) are out of scope and
/// intentionally produce neither.
#[derive(Debug, Clone, Default)]
pub struct GraphicsScan {
    pub payloads: Vec<GraphicsPayload>,
    pub diagnostics: Vec<GraphicsDiagnostic>,
}

impl GraphicsScan {
    /// True when nothing was cut or rejected: no diagnostics at all.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Scan a byte stream for Sixel + Kitty graphics with default bounds.
#[must_use]
pub fn scan_graphics_default(stream: &[u8]) -> GraphicsScan {
    scan_graphics(stream, &GraphicsPolicy::default())
}

/// Scan a byte stream for Sixel + Kitty graphics.
///
/// Recognized: Sixel `DCS <params> q <data> ST`, Kitty `APC G <params> ;
/// <base64> ST` (7-bit `ESC P`/`ESC _` and C1 `0x90`/`0x9F` introducers;
/// `ESC \` and `0x9C` terminators), including `m=1` chunk reassembly and the
/// `a=t,i=<id>` → `a=p,i=<id>` transmit/display table. Everything else
/// graphics-shaped becomes a diagnostic: other DCS/APC finals, iTerm2
/// `OSC 1337`, unterminated introducers, broken chains, bad base64.
/// Parsing is lenient (a payload is still produced when salvageable) but
/// loud (every anomaly is a diagnostic).
pub fn scan_graphics(stream: &[u8], policy: &GraphicsPolicy) -> GraphicsScan {
    Scanner::new(stream, policy).run()
}

struct PendingKitty {
    offset: usize,
    params: Vec<(String, String)>,
    b64: Vec<u8>,
}

struct Scanner<'a> {
    stream: &'a [u8],
    policy: &'a GraphicsPolicy,
    scan: GraphicsScan,
    pending: Option<PendingKitty>,
    /// Transmitted image bytes by Kitty image id (`a=t/T/f`, `i=<id> != 0`).
    images: HashMap<u32, Vec<u8>>,
    images_full: bool,
}

impl<'a> Scanner<'a> {
    fn new(stream: &'a [u8], policy: &'a GraphicsPolicy) -> Self {
        Self {
            stream,
            policy,
            scan: GraphicsScan::default(),
            pending: None,
            images: HashMap::new(),
            images_full: false,
        }
    }

    fn diag(&mut self, offset: usize, kind: GraphicsDiagKind, message: String) {
        self.scan.diagnostics.push(GraphicsDiagnostic {
            offset,
            kind,
            message,
        });
    }

    fn push_payload(&mut self, payload: GraphicsPayload) {
        if self.scan.payloads.len() >= self.policy.max_payloads {
            self.diag(
                payload.stream_offset,
                GraphicsDiagKind::Truncated,
                format!(
                    "payload limit {} reached; remainder of stream uninspected",
                    self.policy.max_payloads
                ),
            );
            return;
        }
        self.scan.payloads.push(payload);
    }

    fn run(mut self) -> GraphicsScan {
        let mut i = 0;
        while i < self.stream.len() {
            let b = self.stream[i];
            if b == 0x1B && i + 1 < self.stream.len() {
                let n = self.stream[i + 1];
                match n {
                    b'P' => {
                        i = self.on_dcs(i, i + 2);
                        continue;
                    }
                    b'_' => {
                        i = self.on_apc(i, i + 2);
                        continue;
                    }
                    b']' => {
                        i = self.on_osc(i, i + 2);
                        continue;
                    }
                    _ => {}
                }
                i += 1;
            } else if b == 0x90 {
                i = self.on_dcs(i, i + 1);
            } else if b == 0x9F {
                i = self.on_apc(i, i + 1);
            } else if b == 0x9D {
                i = self.on_osc(i, i + 1);
            } else {
                i += 1;
            }
            if self.scan.payloads.len() >= self.policy.max_payloads {
                break;
            }
        }
        if let Some(pending) = self.pending.take() {
            self.diag(
                pending.offset,
                GraphicsDiagKind::Malformed,
                "kitty chunk chain ends with m=1 (missing final chunk)".to_string(),
            );
        }
        self.scan
    }

    /// Find `ESC \` or `0x9C` from `start`; returns (content, resume_at).
    fn st_content(&self, start: usize) -> Option<(&'a [u8], usize)> {
        let s = self.stream;
        let mut j = start;
        while j < s.len() {
            if s[j] == 0x1B && j + 1 < s.len() && s[j + 1] == b'\\' {
                return Some((&s[start..j], j + 2));
            }
            if s[j] == 0x9C {
                return Some((&s[start..j], j + 1));
            }
            j += 1;
        }
        None
    }

    fn on_dcs(&mut self, offset: usize, start: usize) -> usize {
        let Some((content, resume)) = self.st_content(start) else {
            self.diag(
                offset,
                GraphicsDiagKind::Malformed,
                "unterminated DCS (no ST)".to_string(),
            );
            return start;
        };
        // First byte >= 0x40 ends params/intermediates: that is the final.
        let mut fin = None;
        for (k, &c) in content.iter().enumerate() {
            if c >= 0x40 {
                fin = Some(k);
                break;
            }
        }
        let Some(f) = fin else {
            self.diag(
                offset,
                GraphicsDiagKind::Malformed,
                "DCS with no final byte".to_string(),
            );
            return resume;
        };
        if content[f] != b'q' {
            self.diag(
                offset,
                GraphicsDiagKind::Unsupported,
                format!(
                    "non-sixel DCS (final '{}', {} param bytes): not inspected",
                    content[f] as char, f
                ),
            );
            return resume;
        }
        let params_raw = String::from_utf8_lossy(&content[..f]).into_owned();
        let data = &content[f + 1..];
        let (kept, truncated) = bound_bytes(data, self.policy.max_payload_bytes);
        if truncated {
            self.diag(
                offset,
                GraphicsDiagKind::Truncated,
                format!(
                    "sixel payload cut from {} to {} bytes",
                    data.len(),
                    kept.len()
                ),
            );
        }
        let mut params = vec![("P".to_string(), params_raw)];
        let mut placement = Placement::default();
        // Raster attributes `"Pan;Pad;Ph;Pv`: only Ph/Pv (pixel dims) survive.
        if let Some(raster) = parse_sixel_raster(data) {
            params.push(("Ph".to_string(), raster.0.to_string()));
            params.push(("Pv".to_string(), raster.1.to_string()));
            placement.image_px = Some(raster);
        }
        self.push_payload(GraphicsPayload {
            kind: GraphicsKind::Sixel,
            params,
            data: kept,
            truncated,
            references: None,
            placement,
            stream_offset: offset,
        });
        resume
    }

    fn on_apc(&mut self, offset: usize, start: usize) -> usize {
        let Some((content, resume)) = self.st_content(start) else {
            self.diag(
                offset,
                GraphicsDiagKind::Malformed,
                "unterminated APC (no ST)".to_string(),
            );
            return start;
        };
        if content.first() != Some(&b'G') {
            let first = content.first().copied().unwrap_or(b'?');
            self.diag(
                offset,
                GraphicsDiagKind::Unsupported,
                format!(
                    "non-kitty APC (first byte '{}'): not inspected",
                    first as char
                ),
            );
            return resume;
        }
        let body = &content[1..];
        let Some(semi) = body.iter().position(|&c| c == b';') else {
            self.diag(
                offset,
                GraphicsDiagKind::Malformed,
                "kitty command without ';' header/payload separator".to_string(),
            );
            return resume;
        };
        let (header, payload_b64) = (&body[..semi], &body[semi + 1..]);
        let mut params = Vec::new();
        for pair in header.split(|&c| c == b',') {
            if pair.is_empty() {
                continue;
            }
            match pair.iter().position(|&c| c == b'=') {
                Some(eq) => params.push((
                    String::from_utf8_lossy(&pair[..eq]).into_owned(),
                    String::from_utf8_lossy(&pair[eq + 1..]).into_owned(),
                )),
                None => {
                    self.diag(
                        offset,
                        GraphicsDiagKind::Malformed,
                        format!("kitty key without '=': {:?}", String::from_utf8_lossy(pair)),
                    );
                    params.push((String::from_utf8_lossy(pair).into_owned(), String::new()));
                }
            }
        }
        let more = params.iter().any(|(k, v)| k == "m" && v == "1");
        if more {
            self.on_kitty_chunk(offset, params, payload_b64);
            return resume;
        }
        self.on_kitty_final(offset, params, payload_b64, resume);
        resume
    }

    /// A non-final (`m=1`) Kitty chunk: accumulate, enforcing the spec rule
    /// that continuation chunks carry only `m` (+`q`, +`a=f` for animation).
    fn on_kitty_chunk(&mut self, offset: usize, params: Vec<(String, String)>, b64: &[u8]) {
        if self.pending.is_none() {
            self.pending = Some(PendingKitty {
                offset,
                params,
                b64: b64.to_vec(),
            });
            return;
        }
        let action_f = params.iter().any(|(k, v)| k == "a" && v == "f");
        for (k, v) in &params {
            let allowed = k == "m" || k == "q" || (action_f && k == "a" && v == "f");
            if !allowed {
                self.diag(
                    offset,
                    GraphicsDiagKind::Malformed,
                    format!("kitty continuation chunk repeats key '{k}={v}' (only m/q allowed)"),
                );
            }
        }
        if let Some(pending) = self.pending.as_mut() {
            pending.b64.extend_from_slice(b64);
        }
    }

    /// A final (`m=0`/absent) Kitty command: close any pending chain,
    /// base64-decode, resolve `a=p` references, extract placement.
    fn on_kitty_final(
        &mut self,
        offset: usize,
        params: Vec<(String, String)>,
        b64: &[u8],
        _resume: usize,
    ) {
        let (first_offset, mut first_params, mut all_b64) = match self.pending.take() {
            None => (offset, params, Vec::new()),
            Some(pending) => {
                for (k, v) in &params {
                    let allowed = k == "m" || k == "q" || k == "a" && v == "f";
                    if !allowed && Self::param_lookup(&pending.params, k).is_none() {
                        // Final-chunk keys the first chunk lacked are kept
                        // (lenient) but flagged (loud).
                        self.diag(
                            offset,
                            GraphicsDiagKind::Malformed,
                            format!(
                                "kitty final chunk adds key '{k}={v}' missing from first chunk"
                            ),
                        );
                    }
                }
                let mut merged = pending.params;
                for (k, v) in params {
                    if k != "m" && Self::param_lookup(&merged, &k).is_none() {
                        merged.push((k, v));
                    }
                }
                (pending.offset, merged, pending.b64)
            }
        };
        // Strip the chunking marker: equality must not see transport.
        first_params.retain(|(k, _)| k != "m");
        all_b64.extend_from_slice(b64);
        let action = Self::param_lookup(&first_params, "a").unwrap_or("t");
        let action = action.to_string();
        let mut references = None;
        let mut data = match base64_decode(&all_b64) {
            Ok(d) => d,
            Err(e) => {
                self.diag(
                    first_offset,
                    GraphicsDiagKind::Malformed,
                    format!("kitty base64 payload invalid: {e}"),
                );
                Vec::new()
            }
        };
        // `a=p` with no inline bytes displays a transmitted id.
        if action == "p" && data.is_empty() {
            if let Some(id) =
                Self::param_lookup(&first_params, "i").and_then(|s| s.parse::<u32>().ok())
            {
                if id != 0 {
                    references = Some(id);
                    match self.images.get(&id) {
                        Some(bytes) => data = bytes.clone(),
                        None => self.diag(
                            first_offset,
                            GraphicsDiagKind::UnknownReference,
                            format!("kitty a=p references untransmitted image id {id}"),
                        ),
                    }
                }
            }
        }
        // Retain transmitted bytes for later `a=p` (bounded table).
        if (action == "t" || action == "T" || action == "f") && !data.is_empty() {
            if let Some(id) =
                Self::param_lookup(&first_params, "i").and_then(|s| s.parse::<u32>().ok())
            {
                if id != 0 && !self.images.contains_key(&id) {
                    if self.images.len() >= self.policy.max_images {
                        if !self.images_full {
                            self.images_full = true;
                            self.diag(
                                first_offset,
                                GraphicsDiagKind::Truncated,
                                format!(
                                    "transmitted-image table full ({}); id {id} inspected but not retained",
                                    self.policy.max_images
                                ),
                            );
                        }
                    } else {
                        self.images.insert(id, data.clone());
                    }
                }
            }
        }
        let (data, truncated) = if data.len() > self.policy.max_payload_bytes {
            self.diag(
                first_offset,
                GraphicsDiagKind::Truncated,
                format!(
                    "kitty payload cut from {} to {} bytes",
                    data.len(),
                    self.policy.max_payload_bytes
                ),
            );
            (data[..self.policy.max_payload_bytes].to_vec(), true)
        } else {
            (data, false)
        };
        let placement = self.kitty_placement(first_offset, &action, &first_params);
        self.push_payload(GraphicsPayload {
            kind: GraphicsKind::Kitty,
            params: first_params,
            data,
            truncated,
            references,
            placement,
            stream_offset: first_offset,
        });
    }

    fn param_lookup<'p>(params: &'p [(String, String)], key: &str) -> Option<&'p str> {
        params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Best-effort placement extraction. Display actions (`t`/`T`/`p`) read
    /// `X`/`Y`/`z`/`c`/`r`/`w`/`h`; animation actions (`f`/`a`/`c`) reuse
    /// those letters for frames/gaps/rectangles, so only `s`/`v` image dims
    /// cross over. Unparseable numbers diagnose + fall back (loud, lenient).
    fn kitty_placement(
        &mut self,
        offset: usize,
        action: &str,
        params: &[(String, String)],
    ) -> Placement {
        let mut p = Placement::default();
        match (num_pair(params, "s", "v"), action) {
            (Some(dims), _) => p.image_px = Some(dims),
            (None, _) => {
                if Self::param_lookup(params, "s").is_some()
                    || Self::param_lookup(params, "v").is_some()
                {
                    self.diag(
                        offset,
                        GraphicsDiagKind::Malformed,
                        "kitty s/v dims unparseable; image size unknown".to_string(),
                    );
                }
            }
        }
        if !matches!(action, "t" | "T" | "p") {
            return p;
        }
        if let Some(x) = Self::param_lookup(params, "X") {
            match x.parse::<u32>() {
                Ok(v) => p.dx_px = v,
                Err(_) => self.diag(
                    offset,
                    GraphicsDiagKind::Malformed,
                    format!("kitty X offset unparseable ({x:?}); using 0"),
                ),
            }
        }
        if let Some(y) = Self::param_lookup(params, "Y") {
            match y.parse::<u32>() {
                Ok(v) => p.dy_px = v,
                Err(_) => self.diag(
                    offset,
                    GraphicsDiagKind::Malformed,
                    format!("kitty Y offset unparseable ({y:?}); using 0"),
                ),
            }
        }
        match Self::param_lookup(params, "z") {
            None => p.z = Some(0),
            Some(z) => match z.parse::<i32>() {
                Ok(v) => p.z = Some(v),
                Err(_) => {
                    self.diag(
                        offset,
                        GraphicsDiagKind::Malformed,
                        format!("kitty z-index unparseable ({z:?}); using 0"),
                    );
                    p.z = Some(0);
                }
            },
        }
        p.display_cells = num_pair(params, "c", "r");
        p.display_px = num_pair(params, "w", "h");
        p
    }

    fn on_osc(&mut self, offset: usize, start: usize) -> usize {
        // OSC terminators: ST (ESC \ / 0x9C) or BEL.
        let s = self.stream;
        let mut j = start;
        let mut end = None;
        while j < s.len() {
            if s[j] == 0x1B && j + 1 < s.len() && s[j + 1] == b'\\' {
                end = Some((j, j + 2));
                break;
            }
            if s[j] == 0x9C || s[j] == 0x07 {
                end = Some((j, j + 1));
                break;
            }
            j += 1;
        }
        let Some((term, resume)) = end else {
            self.diag(
                offset,
                GraphicsDiagKind::Malformed,
                "unterminated OSC (no ST/BEL)".to_string(),
            );
            return start;
        };
        let content = &s[start..term];
        if content.starts_with(b"1337;") {
            self.diag(
                offset,
                GraphicsDiagKind::Unsupported,
                "iTerm2 inline image (OSC 1337): recognized, not inspected".to_string(),
            );
        }
        resume
    }
}

/// Parse a `key1/key2` u32 pair: `Some` when at least one key is present and
/// every present key parses; missing side reads 0.
fn num_pair(params: &[(String, String)], k1: &str, k2: &str) -> Option<(u32, u32)> {
    let v1 = params
        .iter()
        .find(|(k, _)| k == k1)
        .map(|(_, v)| v.as_str());
    let v2 = params
        .iter()
        .find(|(k, _)| k == k2)
        .map(|(_, v)| v.as_str());
    match (v1, v2) {
        (None, None) => None,
        (a, b) => {
            let x = match a {
                None => 0,
                Some(s) => s.parse::<u32>().ok()?,
            };
            let y = match b {
                None => 0,
                Some(s) => s.parse::<u32>().ok()?,
            };
            Some((x, y))
        }
    }
}

/// First `"Pan;Pad;Ph;Pv` raster attribute in sixel data, returning
/// `(Ph, Pv)` pixel dims. Malformed attributes are ignored here (the strict
/// decoder reports them); absence is `None`.
fn parse_sixel_raster(data: &[u8]) -> Option<(u32, u32)> {
    let q = data.iter().position(|&c| c == b'"')?;
    let rest = &data[q + 1..];
    let end = rest
        .iter()
        .position(|&c| !(c.is_ascii_digit() || c == b';'))
        .unwrap_or(rest.len());
    let mut parts = rest[..end].split(|&c| c == b';');
    let _pan = parts.next()?.to_vec();
    let _pad = parts.next()?.to_vec();
    let ph = std::str::from_utf8(parts.next()?)
        .ok()?
        .parse::<u32>()
        .ok()?;
    let pv = std::str::from_utf8(parts.next()?)
        .ok()?
        .parse::<u32>()
        .ok()?;
    Some((ph, pv))
}

fn base64_decode(b64: &[u8]) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    // Kitty payloads are base64 text; reject non-ASCII loudly instead of
    // letting the engine report a bare offset.
    if let Some(&bad) = b64
        .iter()
        .find(|&&c| c > 0x7E || (c < 0x20 && c != b'\r' && c != b'\n'))
    {
        return Err(format!("non-base64 byte 0x{bad:02X} in payload"));
    }
    let mut compact = Vec::with_capacity(b64.len());
    compact.extend(b64.iter().filter(|&&c| c != b'\r' && c != b'\n'));
    base64::engine::general_purpose::STANDARD
        .decode(&compact)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Bounded decode to RGBA (A07)
// ---------------------------------------------------------------------------

impl GraphicsPayload {
    /// Decode this payload to RGBA8 within `policy` bounds.
    ///
    /// - Kitty `f=100`: PNG bytes via the `image` crate (dims from the PNG
    ///   header, bound-checked).
    /// - Kitty `f=32`/`f=24`: raw RGBA/RGB (`s`/`v` required; RGB gains
    ///   opaque alpha).
    /// - Kitty `t=f`/`t=t`/`t=s`: refused — the bytes live outside the
    ///   stream and this inspector never opens files or shm objects.
    /// - Sixel: strict bounded rasterizer (RGB `#...;2;..` + HLS `#...;1;..`
    ///   defines, `!` repeats, `$`/`-`, `"` raster); unplotted pixels are
    ///   transparent so the missing compositor stays visible.
    ///
    /// Truncated payloads never decode; oversize claims fail before any
    /// allocation proportional to them.
    pub fn decode_bounded(
        &self,
        policy: &GraphicsPolicy,
    ) -> Result<DecodedImage, GraphicsDecodeError> {
        if self.truncated {
            return Err(GraphicsDecodeError::Truncated);
        }
        match self.kind {
            GraphicsKind::Kitty => self.decode_kitty(policy),
            GraphicsKind::Sixel => decode_sixel(&self.data, policy),
        }
    }

    fn decode_kitty(&self, policy: &GraphicsPolicy) -> Result<DecodedImage, GraphicsDecodeError> {
        let medium = self.param("t").unwrap_or("d");
        if medium != "d" {
            return Err(GraphicsDecodeError::UnsupportedMedium(format!(
                "kitty t={medium}: only direct (t=d) data decodes; file/temp/shm media never touched"
            )));
        }
        if let Some(id) = self.references {
            if self.data.is_empty() {
                return Err(GraphicsDecodeError::UnknownReference(id));
            }
        }
        let format = self.param("f").unwrap_or("32");
        match format {
            "100" => {
                let img = image::load_from_memory(&self.data).map_err(|e| {
                    GraphicsDecodeError::InvalidData(format!("kitty PNG payload: {e}"))
                })?;
                let rgba = img.to_rgba8();
                check_dims(rgba.width(), rgba.height(), policy)?;
                Ok(DecodedImage {
                    width: rgba.width(),
                    height: rgba.height(),
                    rgba: rgba.into_raw(),
                })
            }
            "32" | "24" => {
                let (w, h) = self.raw_dims()?;
                check_dims(w, h, policy)?;
                let bpp = if format == "32" { 4 } else { 3 };
                let want = w as usize * h as usize * bpp;
                if self.data.len() != want {
                    return Err(GraphicsDecodeError::InvalidData(format!(
                        "kitty f={format} {w}x{h} needs {want} bytes, payload has {}",
                        self.data.len()
                    )));
                }
                let rgba = if format == "32" {
                    self.data.clone()
                } else {
                    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
                    for px in self.data.as_chunks::<3>().0 {
                        out.extend_from_slice(&[px[0], px[1], px[2], 0xFF]);
                    }
                    out
                };
                Ok(DecodedImage {
                    width: w,
                    height: h,
                    rgba,
                })
            }
            other => Err(GraphicsDecodeError::UnsupportedFormat(format!(
                "kitty f={other}: only f=24 (RGB), f=32 (RGBA), f=100 (PNG) decode"
            ))),
        }
    }

    fn raw_dims(&self) -> Result<(u32, u32), GraphicsDecodeError> {
        let w = self
            .param("s")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let h = self
            .param("v")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        if w == 0 || h == 0 {
            return Err(GraphicsDecodeError::MissingDims);
        }
        Ok((w, h))
    }
}

/// Reject dims before allocating anything proportional to them.
fn check_dims(w: u32, h: u32, policy: &GraphicsPolicy) -> Result<(), GraphicsDecodeError> {
    if w == 0 || h == 0 {
        return Err(GraphicsDecodeError::InvalidData(format!(
            "zero image dimension {w}x{h}"
        )));
    }
    if w > policy.max_dim || h > policy.max_dim {
        return Err(GraphicsDecodeError::TooLarge {
            w,
            h,
            max: policy.max_dim,
        });
    }
    if w as u64 * h as u64 > policy.max_pixels {
        return Err(GraphicsDecodeError::TooLarge {
            w,
            h,
            max: policy.max_dim,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Strict bounded Sixel rasterizer
// ---------------------------------------------------------------------------

/// Rasterize sixel source bytes to RGBA8. Supported: `"` raster attributes
/// (only the FIRST well-formed set; later ones error), `#n` select,
/// `#n;2;r;g;b` RGB defines (0-100%), `#n;1;h;l;s` HLS defines, `!n<c>`
/// repeats, sixel columns `?`..`~`, `$` (x=0), `-` (x=0, y+=6). `\r`/`\n`
/// (transport wrapping) are skipped; every other byte errors explicitly.
/// Plotting with an undefined register errors (no assumed palette).
/// Unplotted pixels are transparent; plotted pixels are opaque.
fn decode_sixel(data: &[u8], policy: &GraphicsPolicy) -> Result<DecodedImage, GraphicsDecodeError> {
    let bad = |m: String| GraphicsDecodeError::InvalidData(m);
    let mut regs: HashMap<u16, [u8; 3]> = HashMap::new();
    let mut current: u16 = 0;
    let mut current_set = false;
    let mut plotted: HashMap<(u32, u32), [u8; 3]> = HashMap::new();
    let mut x: u32 = 0;
    let mut y: u32 = 0;
    let mut max_x: Option<u32> = None;
    let mut max_y: Option<u32> = None;
    let mut canvas: Option<(u32, u32)> = None;
    let mut raster_seen = false;

    let plot = |x: u32,
                y: u32,
                v: u8,
                rgb: [u8; 3],
                plotted: &mut HashMap<(u32, u32), [u8; 3]>,
                max_x: &mut Option<u32>,
                max_y: &mut Option<u32>|
     -> Result<(), GraphicsDecodeError> {
        for bit in 0..6u32 {
            if v >> bit & 1 == 1 {
                let py = y + bit;
                if x >= policy.max_dim || py >= policy.max_dim {
                    return Err(GraphicsDecodeError::TooLarge {
                        w: x + 1,
                        h: py + 1,
                        max: policy.max_dim,
                    });
                }
                if plotted.len() as u64 >= policy.max_pixels && !plotted.contains_key(&(x, py)) {
                    return Err(GraphicsDecodeError::TooLarge {
                        w: x + 1,
                        h: py + 1,
                        max: policy.max_dim,
                    });
                }
                plotted.insert((x, py), rgb);
                max_x.replace(max_x.map_or(x, |m| m.max(x)));
                max_y.replace(max_y.map_or(py, |m| m.max(py)));
            }
        }
        Ok(())
    };

    let mut i = 0;
    while i < data.len() {
        let c = data[i];
        match c {
            b'\r' | b'\n' => i += 1,
            b'"' => {
                if raster_seen {
                    return Err(bad("second sixel raster attribute".to_string()));
                }
                raster_seen = true;
                let (args, next) = sixel_ints(data, i + 1, 4)?;
                i = next;
                if args.len() != 4 {
                    return Err(bad(format!(
                        "raster attribute needs Pan;Pad;Ph;Pv, got {} values",
                        args.len()
                    )));
                }
                let (ph, pv) = (args[2], args[3]);
                check_dims(ph, pv, policy).map_err(|_| GraphicsDecodeError::TooLarge {
                    w: ph,
                    h: pv,
                    max: policy.max_dim,
                })?;
                canvas = Some((ph, pv));
            }
            b'#' => {
                let (reg, next) = sixel_uint(data, i + 1)
                    .map_err(|_| bad(format!("bad color register at byte {i}")))?;
                i = next;
                let reg16 = u16::try_from(reg)
                    .map_err(|_| bad(format!("color register {reg} out of range")))?;
                if i < data.len() && data[i] == b';' {
                    let (args, next) = sixel_ints(data, i + 1, 4)?;
                    i = next;
                    if args.len() != 4 {
                        return Err(bad(format!(
                            "color define needs type;a;b;c, got {} values",
                            args.len()
                        )));
                    }
                    let rgb = match args[0] {
                        2 => [
                            sixel_pct(args[1], "r")?,
                            sixel_pct(args[2], "g")?,
                            sixel_pct(args[3], "b")?,
                        ],
                        1 => hls_to_rgb(args[1], args[2], args[3])?,
                        t => {
                            return Err(GraphicsDecodeError::UnsupportedFormat(format!(
                                "sixel color type {t} (only 1=HLS, 2=RGB)"
                            )))
                        }
                    };
                    regs.insert(reg16, rgb);
                }
                current = reg16;
                current_set = true;
            }
            b'!' => {
                let (n, next) = sixel_uint(data, i + 1)
                    .map_err(|_| bad(format!("bad repeat count at byte {i}")))?;
                if n == 0 || n > policy.max_dim {
                    return Err(bad(format!("repeat count {n} out of range")));
                }
                if next >= data.len() || !(0x3F..=0x7E).contains(&data[next]) {
                    return Err(bad(format!("repeat at byte {i} not followed by a sixel")));
                }
                let rgb = sixel_current(&regs, current, current_set)?;
                let v = data[next] - 0x3F;
                for _ in 0..n {
                    plot(x, y, v, rgb, &mut plotted, &mut max_x, &mut max_y)?;
                    x += 1;
                }
                i = next + 1;
            }
            b'$' => {
                x = 0;
                i += 1;
            }
            b'-' => {
                x = 0;
                y += 6;
                i += 1;
            }
            0x3F..=0x7E => {
                let rgb = sixel_current(&regs, current, current_set)?;
                plot(x, y, c - 0x3F, rgb, &mut plotted, &mut max_x, &mut max_y)?;
                x += 1;
                i += 1;
            }
            other => {
                return Err(bad(format!(
                    "unexpected sixel byte 0x{other:02X} at offset {i}"
                )));
            }
        }
    }
    let (w, h) = match (canvas, max_x, max_y) {
        (Some((ph, pv)), _, _) => (ph, pv),
        (None, Some(mx), Some(my)) => (mx + 1, my + 1),
        _ => return Err(bad("empty sixel image (no pixels, no raster)".to_string())),
    };
    check_dims(w, h, policy)?;
    let mut rgba = vec![0u8; w as usize * h as usize * 4];
    for ((px, py), rgb) in &plotted {
        if *px < w && *py < h {
            let o = (*py as usize * w as usize + *px as usize) * 4;
            rgba[o..o + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 0xFF]);
        }
    }
    Ok(DecodedImage {
        width: w,
        height: h,
        rgba,
    })
}

fn sixel_current(
    regs: &HashMap<u16, [u8; 3]>,
    current: u16,
    set: bool,
) -> Result<[u8; 3], GraphicsDecodeError> {
    if !set {
        return Err(GraphicsDecodeError::UndefinedColor(current));
    }
    regs.get(&current)
        .copied()
        .ok_or(GraphicsDecodeError::UndefinedColor(current))
}

/// Parse `!`-style unsigned int at `start` (must have ≥1 digit).
fn sixel_uint(data: &[u8], start: usize) -> Result<(u32, usize), ()> {
    let mut j = start;
    while j < data.len() && data[j].is_ascii_digit() {
        j += 1;
    }
    if j == start {
        return Err(());
    }
    std::str::from_utf8(&data[start..j])
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .map(|n| (n, j))
        .ok_or(())
}

/// Parse up to `max` `;`-separated unsigned ints at `start`.
fn sixel_ints(
    data: &[u8],
    start: usize,
    max: usize,
) -> Result<(Vec<u32>, usize), GraphicsDecodeError> {
    let bad = |m: String| GraphicsDecodeError::InvalidData(m);
    let mut vals = Vec::new();
    let mut j = start;
    loop {
        let mut k = j;
        while k < data.len() && data[k].is_ascii_digit() {
            k += 1;
        }
        if k == j {
            return Err(bad(format!("expected number at sixel byte {j}")));
        }
        vals.push(
            std::str::from_utf8(&data[j..k])
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| bad(format!("number out of range at sixel byte {j}")))?,
        );
        j = k;
        if vals.len() == max || j >= data.len() || data[j] != b';' {
            return Ok((vals, j));
        }
        j += 1;
    }
}

/// Sixel percent (0-100) → u8. Out-of-range errors (strict, explicit).
fn sixel_pct(v: u32, which: &str) -> Result<u8, GraphicsDecodeError> {
    if v > 100 {
        return Err(GraphicsDecodeError::InvalidData(format!(
            "sixel RGB {which}={v} out of 0-100 range"
        )));
    }
    Ok(((v * 255 + 50) / 100) as u8)
}

/// HLS (h 0-360, l/s 0-100%) → RGB. Standard single-hexcone conversion.
fn hls_to_rgb(h: u32, l: u32, s: u32) -> Result<[u8; 3], GraphicsDecodeError> {
    let bad = |m: String| GraphicsDecodeError::InvalidData(m);
    if h > 360 {
        return Err(bad(format!("sixel HLS h={h} out of 0-360 range")));
    }
    if l > 100 || s > 100 {
        return Err(bad(format!("sixel HLS l={l} s={s} out of 0-100 range")));
    }
    let h = h as f64 / 360.0;
    let l = l as f64 / 100.0;
    let s = s as f64 / 100.0;
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        (
            hue(p, q, h + 1.0 / 3.0),
            hue(p, q, h),
            hue(p, q, h - 1.0 / 3.0),
        )
    };
    Ok([
        (r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (b.clamp(0.0, 1.0) * 255.0).round() as u8,
    ])
}

fn hue(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

fn bound_bytes(data: &[u8], max: usize) -> (Vec<u8>, bool) {
    if data.len() > max {
        (data[..max].to_vec(), true)
    } else {
        (data.to_vec(), false)
    }
}
