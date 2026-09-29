//! Graphics inspection types: Sixel/Kitty payloads, scans, bounded decode entry.

use super::{GraphicsPolicy, Scanner};

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
        image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone()).unwrap_or_else(
            || {
                unreachable!(
                    "decoded image length is width*height*4 by construction ({}x{} vs {} bytes)",
                    self.width,
                    self.height,
                    self.rgba.len()
                )
            },
        )
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
