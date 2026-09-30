//! Entry format + bounded full-decode validation.
//!
//! Moved out of `cache.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use sha2::{Digest, Sha256};

use super::key::CacheKey;

// ---------------------------------------------------------------------------
// Entry format + bounded full-decode validation.
// ---------------------------------------------------------------------------

/// Entry header magic (v2 format; v1 entries are rejected outright).
const ENTRY_MAGIC: &[u8; 9] = b"TSCACHE02";
/// `magic(9) + key(32) + png_len u64le(8) + sha256(32)`.
const HEADER_LEN: usize = 9 + 32 + 8 + 32;

/// Largest PNG side the cache will decode (pixels). Real renders are
/// `cols*cell_w*scale`-sized (tens of megapixels at most); anything larger
/// is a corrupt or hostile entry, rejected BEFORE the full decode allocates.
const MAX_PNG_DIM: u32 = 16_384;
/// Largest PNG pixel count the cache will decode.
const MAX_PNG_PIXELS: u64 = 1 << 26;

const PNG_SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Full bounded PNG validation: IHDR dims are bounds-checked BEFORE the
/// decode allocates, then the whole image is decoded and the decoded dims
/// must match the header. Magic + length alone never validates.
pub(super) fn fully_valid_png(png: &[u8]) -> bool {
    if png.len() < 33 || png[0..8] != PNG_SIG || png[12..16] != *b"IHDR" {
        return false;
    }
    let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    if w == 0 || h == 0 || w > MAX_PNG_DIM || h > MAX_PNG_DIM {
        return false;
    }
    if u64::from(w) * u64::from(h) > MAX_PNG_PIXELS {
        return false;
    }
    match image::load_from_memory(png) {
        Ok(img) => {
            use image::GenericImageView;
            img.dimensions() == (w, h)
        }
        Err(_) => false,
    }
}

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub(super) fn encode_entry(key: &CacheKey, png: &[u8]) -> Vec<u8> {
    let mut entry = Vec::with_capacity(HEADER_LEN + png.len());
    entry.extend_from_slice(ENTRY_MAGIC);
    entry.extend_from_slice(key.raw());
    entry.extend_from_slice(&u64::try_from(png.len()).unwrap_or(u64::MAX).to_le_bytes());
    entry.extend_from_slice(&sha256_bytes(png));
    entry.extend_from_slice(png);
    entry
}

/// Decode one entry, bound to the expected key. `None` on ANY defect:
/// bad magic, key mismatch (wrong-entry payload), length/checksum mismatch,
/// truncation, or a PNG that does not fully decode within bounds.
pub(super) fn decode_entry(key: &CacheKey, bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < HEADER_LEN || bytes[0..9] != *ENTRY_MAGIC {
        return None;
    }
    if bytes[9..41] != *key.raw() {
        return None;
    }
    let len = usize::try_from(u64::from_le_bytes(bytes[41..49].try_into().ok()?)).ok()?;
    if bytes.len() != HEADER_LEN + len {
        return None;
    }
    let png = &bytes[HEADER_LEN..];
    if bytes[49..81] != sha256_bytes(png) {
        return None;
    }
    if !fully_valid_png(png) {
        return None;
    }
    Some(png.to_vec())
}
