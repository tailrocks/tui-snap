//! Bounded graphics decode: Kitty payloads plus shared raster helpers.

use super::{
    DecodedImage, GraphicsDecodeError, GraphicsKind, GraphicsPayload, GraphicsPolicy, decode_sixel,
};

/// Parse a `key1/key2` u32 pair: `Some` when at least one key is present and
/// every present key parses; missing side reads 0.
pub(crate) fn num_pair(params: &[(String, String)], k1: &str, k2: &str) -> Option<(u32, u32)> {
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
pub(crate) fn parse_sixel_raster(data: &[u8]) -> Option<(u32, u32)> {
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

pub(crate) fn base64_decode(b64: &[u8]) -> Result<Vec<u8>, String> {
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
    ///
    /// # Errors
    ///
    /// Returns `GraphicsDecodeError` on truncated/oversize/unparseable payloads.
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
        if let Some(id) = self.references
            && self.data.is_empty()
        {
            return Err(GraphicsDecodeError::UnknownReference(id));
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
pub(crate) fn check_dims(
    w: u32,
    h: u32,
    policy: &GraphicsPolicy,
) -> Result<(), GraphicsDecodeError> {
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
    if u64::from(w) * u64::from(h) > policy.max_pixels {
        return Err(GraphicsDecodeError::TooLarge {
            w,
            h,
            max: policy.max_dim,
        });
    }
    Ok(())
}
