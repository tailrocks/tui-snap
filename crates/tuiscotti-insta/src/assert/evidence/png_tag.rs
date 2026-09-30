//! PNG generation tagging (`tEXt` chunk) + snapshot description binding.
//!
//! Moved out of `evidence.rs` so both files stay under the repo line gate;
//! behavior is unchanged.

use std::path::Path;

use super::super::{GEN_DESC_PREFIX, PNG_GEN_KEYWORD};

const PNG_SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = if crc & 1 == 1 { 0xEDB8_8320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

/// Insert a `tEXt` generation chunk before `IEND`. Decoders ignore it (the pixel
/// verdict is unaffected); [`check_consistent`](super::super::check_consistent) reads it back.
///
/// # Panics
///
/// Panics on malformed PNG input or a bad keyword (caller bug: renderer output
/// is always well-formed and the keyword is fixed).
#[must_use]
pub fn png_tag_generation(png: &[u8], generation: &str) -> Vec<u8> {
    assert!(png.starts_with(&PNG_SIG), "png_tag_generation: not a PNG");
    assert!(
        !PNG_GEN_KEYWORD.contains('\0') && PNG_GEN_KEYWORD.len() <= 79,
        "png_tag_generation: bad keyword"
    );
    assert!(
        !generation.contains('\0'),
        "png_tag_generation: generation contains NUL"
    );
    let mut data = Vec::new();
    data.extend_from_slice(PNG_GEN_KEYWORD.as_bytes());
    data.push(0);
    data.extend_from_slice(generation.as_bytes());
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
    chunk.extend_from_slice(b"tEXt");
    chunk.extend_from_slice(&data);
    let mut crc_input = b"tEXt".to_vec();
    crc_input.extend_from_slice(&data);
    chunk.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    assert!(
        png.len() > 12 && &png[png.len() - 8..png.len() - 4] == b"IEND",
        "png_tag_generation: PNG missing IEND"
    );
    let mut out = Vec::with_capacity(png.len() + chunk.len());
    out.extend_from_slice(&png[..png.len() - 12]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[png.len() - 12..]);
    out
}

/// Read back the generation tag, if any. `None` on malformed input or no tag
/// (legacy/foreign PNGs) — never panics.
#[must_use]
pub fn png_generation(png: &[u8]) -> Option<String> {
    if !png.starts_with(&PNG_SIG) || png.len() < 12 {
        return None;
    }
    let mut i = 8;
    while i + 8 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().ok()?) as usize;
        let typ = &png[i + 4..i + 8];
        if i + 8 + len + 4 > png.len() {
            return None;
        }
        if typ == b"tEXt" {
            let data = &png[i + 8..i + 8 + len];
            if let Some(z) = data.iter().position(|&b| b == 0)
                && &data[..z] == PNG_GEN_KEYWORD.as_bytes()
            {
                return Some(String::from_utf8_lossy(&data[z + 1..]).into_owned());
            }
        }
        if typ == b"IEND" {
            break;
        }
        i += 8 + len + 4;
    }
    None
}

/// Generation binding parsed from a `.snap` description (`None` when absent).
pub(crate) fn snap_generation(snap_path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(snap_path).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(v) = line.trim().strip_prefix("description:") {
            let v = v.trim().trim_matches('"');
            let generation = v.strip_prefix(GEN_DESC_PREFIX)?;
            return generation.split_whitespace().next().map(str::to_string);
        }
    }
    None
}
