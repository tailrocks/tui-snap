//! Strict bounded Sixel rasterizer.

use super::{DecodedImage, GraphicsDecodeError, GraphicsPolicy, check_dims};
use std::collections::HashMap;

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
pub(crate) fn decode_sixel(
    data: &[u8],
    policy: &GraphicsPolicy,
) -> Result<DecodedImage, GraphicsDecodeError> {
    let mut plotter = SixelPlotter {
        policy,
        regs: HashMap::new(),
        current: 0,
        current_set: false,
        plotted: HashMap::new(),
        x: 0,
        y: 0,
        max_x: None,
        max_y: None,
        canvas: None,
        raster_seen: false,
    };
    let mut i = 0;
    while i < data.len() {
        let c = data[i];
        match c {
            b'\r' | b'\n' => i += 1,
            b'"' => i = plotter.on_raster(data, i)?,
            b'#' => i = plotter.on_define(data, i)?,
            b'!' => i = plotter.on_repeat(data, i)?,
            b'$' => {
                plotter.x = 0;
                i += 1;
            }
            b'-' => {
                plotter.x = 0;
                plotter.y += 6;
                i += 1;
            }
            0x3F..=0x7E => {
                let rgb = plotter.current_rgb()?;
                plotter.plot(c - 0x3F, rgb)?;
                plotter.x += 1;
                i += 1;
            }
            other => {
                return Err(GraphicsDecodeError::InvalidData(format!(
                    "unexpected sixel byte 0x{other:02X} at offset {i}"
                )));
            }
        }
    }
    plotter.finish()
}

/// Incremental sixel rasterizer state for one [`decode_sixel`] call.
struct SixelPlotter<'a> {
    policy: &'a GraphicsPolicy,
    regs: HashMap<u16, [u8; 3]>,
    current: u16,
    current_set: bool,
    plotted: HashMap<(u32, u32), [u8; 3]>,
    x: u32,
    y: u32,
    max_x: Option<u32>,
    max_y: Option<u32>,
    canvas: Option<(u32, u32)>,
    raster_seen: bool,
}

impl SixelPlotter<'_> {
    fn plot(&mut self, v: u8, rgb: [u8; 3]) -> Result<(), GraphicsDecodeError> {
        let (x, y) = (self.x, self.y);
        for bit in 0..6u32 {
            if v >> bit & 1 == 1 {
                let py = y + bit;
                if x >= self.policy.max_dim || py >= self.policy.max_dim {
                    return Err(GraphicsDecodeError::TooLarge {
                        w: x + 1,
                        h: py + 1,
                        max: self.policy.max_dim,
                    });
                }
                if self.plotted.len() as u64 >= self.policy.max_pixels
                    && !self.plotted.contains_key(&(x, py))
                {
                    return Err(GraphicsDecodeError::TooLarge {
                        w: x + 1,
                        h: py + 1,
                        max: self.policy.max_dim,
                    });
                }
                self.plotted.insert((x, py), rgb);
                self.max_x.replace(self.max_x.map_or(x, |m| m.max(x)));
                self.max_y.replace(self.max_y.map_or(py, |m| m.max(py)));
            }
        }
        Ok(())
    }

    fn current_rgb(&self) -> Result<[u8; 3], GraphicsDecodeError> {
        sixel_current(&self.regs, self.current, self.current_set)
    }

    /// `"` raster attributes (only the FIRST well-formed set sticks).
    /// Returns the resume offset.
    fn on_raster(&mut self, data: &[u8], i: usize) -> Result<usize, GraphicsDecodeError> {
        if self.raster_seen {
            return Err(GraphicsDecodeError::InvalidData(
                "second sixel raster attribute".to_string(),
            ));
        }
        self.raster_seen = true;
        let (args, next) = sixel_ints(data, i + 1, 4)?;
        if args.len() != 4 {
            return Err(GraphicsDecodeError::InvalidData(format!(
                "raster attribute needs Pan;Pad;Ph;Pv, got {} values",
                args.len()
            )));
        }
        let (ph, pv) = (args[2], args[3]);
        check_dims(ph, pv, self.policy).map_err(|_| GraphicsDecodeError::TooLarge {
            w: ph,
            h: pv,
            max: self.policy.max_dim,
        })?;
        self.canvas = Some((ph, pv));
        Ok(next)
    }

    /// `#n` select plus optional `#n;type;a;b;c` color define.
    /// Returns the resume offset.
    fn on_define(&mut self, data: &[u8], i: usize) -> Result<usize, GraphicsDecodeError> {
        let bad = |m: String| GraphicsDecodeError::InvalidData(m);
        let (reg, mut next) =
            sixel_uint(data, i + 1).map_err(|()| bad(format!("bad color register at byte {i}")))?;
        let reg16 =
            u16::try_from(reg).map_err(|_| bad(format!("color register {reg} out of range")))?;
        if next < data.len() && data[next] == b';' {
            let (args, after) = sixel_ints(data, next + 1, 4)?;
            next = after;
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
                    )));
                }
            };
            self.regs.insert(reg16, rgb);
        }
        self.current = reg16;
        self.current_set = true;
        Ok(next)
    }

    /// `!n<c>` repeat. Returns the resume offset.
    fn on_repeat(&mut self, data: &[u8], i: usize) -> Result<usize, GraphicsDecodeError> {
        let bad = |m: String| GraphicsDecodeError::InvalidData(m);
        let (n, next) =
            sixel_uint(data, i + 1).map_err(|()| bad(format!("bad repeat count at byte {i}")))?;
        if n == 0 || n > self.policy.max_dim {
            return Err(bad(format!("repeat count {n} out of range")));
        }
        if next >= data.len() || !(0x3F..=0x7E).contains(&data[next]) {
            return Err(bad(format!("repeat at byte {i} not followed by a sixel")));
        }
        let rgb = self.current_rgb()?;
        let v = data[next] - 0x3F;
        for _ in 0..n {
            self.plot(v, rgb)?;
            self.x += 1;
        }
        Ok(next + 1)
    }

    /// Canvas dimensions plus RGBA8 assembly (unplotted pixels transparent).
    fn finish(self) -> Result<DecodedImage, GraphicsDecodeError> {
        let (w, h) = match (self.canvas, self.max_x, self.max_y) {
            (Some((ph, pv)), _, _) => (ph, pv),
            (None, Some(mx), Some(my)) => (mx + 1, my + 1),
            _ => {
                return Err(GraphicsDecodeError::InvalidData(
                    "empty sixel image (no pixels, no raster)".to_string(),
                ));
            }
        };
        check_dims(w, h, self.policy)?;
        let mut rgba = vec![0u8; w as usize * h as usize * 4];
        for ((px, py), rgb) in &self.plotted {
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
    // Bound: v <= 100, so (v*255+50)/100 <= 255 — always succeeds.
    Ok(u8::try_from((v * 255 + 50) / 100).unwrap_or(u8::MAX))
}

/// HLS (`hue_deg` 0-360, `light`/`sat` 0-100%) → RGB. Standard single-hexcone
/// conversion.
#[expect(
    clippy::cast_possible_truncation,
    reason = "channels are clamped to 0..=255 before rounding, so the casts are in range"
)]
#[expect(
    clippy::cast_sign_loss,
    reason = "channels are clamped to 0..=255 before rounding, so the casts are in range"
)]
fn hls_to_rgb(hue_deg: u32, light: u32, sat: u32) -> Result<[u8; 3], GraphicsDecodeError> {
    let bad = |m: String| GraphicsDecodeError::InvalidData(m);
    if hue_deg > 360 {
        return Err(bad(format!("sixel HLS h={hue_deg} out of 0-360 range")));
    }
    if light > 100 || sat > 100 {
        return Err(bad(format!(
            "sixel HLS l={light} s={sat} out of 0-100 range"
        )));
    }
    let hue_n = f64::from(hue_deg) / 360.0;
    let light_n = f64::from(light) / 100.0;
    let sat_n = f64::from(sat) / 100.0;
    let (red, green, blue) = if sat_n == 0.0 {
        (light_n, light_n, light_n)
    } else {
        let temp_q = if light_n < 0.5 {
            light_n * (1.0 + sat_n)
        } else {
            light_n + sat_n - light_n * sat_n
        };
        let temp_p = 2.0 * light_n - temp_q;
        (
            hue(temp_p, temp_q, hue_n + 1.0 / 3.0),
            hue(temp_p, temp_q, hue_n),
            hue(temp_p, temp_q, hue_n - 1.0 / 3.0),
        )
    };
    Ok([
        (red.clamp(0.0, 1.0) * 255.0).round() as u8,
        (green.clamp(0.0, 1.0) * 255.0).round() as u8,
        (blue.clamp(0.0, 1.0) * 255.0).round() as u8,
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

pub(crate) fn bound_bytes(data: &[u8], max: usize) -> (Vec<u8>, bool) {
    if data.len() > max {
        (data[..max].to_vec(), true)
    } else {
        (data.to_vec(), false)
    }
}
