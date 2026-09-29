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

pub mod animated;
pub mod cast;
pub mod decode;
pub mod error;
pub mod graphics;
pub mod kitty;
pub mod mp4;
pub mod policies;
pub mod scanner;
pub mod sixel;

pub(crate) use animated::decode_png_frames;
pub use animated::{apng, apng_with, gif, gif_with};
pub(crate) use cast::json_string;
pub use cast::{CAST_FILE_NAME, cast_v2, cast_v2_with};
pub(crate) use decode::{base64_decode, check_dims, num_pair, parse_sixel_raster};
pub use error::ExportError;
pub use graphics::{
    DecodedImage, GraphicsDecodeError, GraphicsDiagKind, GraphicsDiagnostic, GraphicsKind,
    GraphicsPayload, GraphicsScan, Placement, scan_graphics, scan_graphics_default,
};
pub use mp4::{FFMPEG_INSTALL, Mp4Sidecar, ffmpeg_version, mp4, mp4_with};
pub use policies::{ApngPolicy, CastPolicy, ExportPolicies, GifPolicy, GraphicsPolicy, Mp4Policy};
pub(crate) use scanner::{PendingKitty, Scanner};
pub(crate) use sixel::{bound_bytes, decode_sixel};
