//! tuiscotti-render: pinned-profile rendering and evidence artifacts.
//!
//! Canonical [`tuiscotti_core::frame::Frame`] data becomes real-glyph PNGs,
//! SVG, ANSI, and text ([`render`]) under a pinned [`profile`]; decoded-pixel
//! comparison ([`diff`]) and evidence exports ([`export`]) build on the same
//! renderer. The Ratatui view adapter lives in
//! [`tuiscotti_core::ratatui`] and is re-exported here for path stability.

pub mod diff;
pub mod export;
pub mod formats;
pub mod profile;
pub mod render;

pub use tuiscotti_core::ratatui;

pub use profile::{
    FallbackFace, FontFaces, Profile, VENDORED_CJK_FONT, VENDORED_CJK_FONT_SHA256, VENDORED_FACES,
    VENDORED_FALLBACK_FACES, VENDORED_FONT, VENDORED_FONT_BOLD, VENDORED_FONT_BOLD_ITALIC,
    VENDORED_FONT_ITALIC, VENDORED_SYMBOLS_FONT, VENDORED_SYMBOLS_FONT_SHA256,
    VENDORED_SYMBOLS2_FONT, VENDORED_SYMBOLS2_FONT_SHA256,
};
pub use render::Renderer;

/// Lowercase hex rendering of bytes (digest/key display shared by the cache,
/// format, and profile modules).
pub(crate) fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 15)]));
    }
    out
}
