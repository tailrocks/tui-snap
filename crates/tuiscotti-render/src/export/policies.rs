//! Pinned export knobs: per-format policies plus [`ExportPolicies`](super::ExportPolicies).

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
