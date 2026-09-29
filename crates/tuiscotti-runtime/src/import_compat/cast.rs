use super::*;
use std::path::Path;

// ---------------------------------------------------------------------------
// asciinema v2 cast
// ---------------------------------------------------------------------------

/// Parsed asciinema header (`{"version":2,"width":..,"height":..,...}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastHeader {
    /// Always 2 after successful validation.
    pub version: u64,
    /// Terminal width in columns.
    pub width: u16,
    /// Terminal height in rows.
    pub height: u16,
    /// Optional `title` (data only — never executed).
    pub title: Option<String>,
    /// Optional `env.TERM`.
    pub term: Option<String>,
    /// Optional `timestamp`.
    pub timestamp: Option<u64>,
}

/// One asciinema event. `Input` is terminal INPUT (keystrokes): marked
/// non-executable, excluded from [`CastTrace::output_deltas`] and
/// [`CastTrace::screens_via`], and never fed as output.
#[derive(Debug, Clone, PartialEq)]
pub enum CastEvent {
    /// `["o"]` terminal output at absolute time `t` (seconds).
    Output {
        /// Absolute event time in seconds.
        t: f64,
        /// Raw output bytes.
        bytes: Vec<u8>,
    },
    /// `["i"]` terminal input (NON-EXECUTABLE, never replayed as output).
    Input {
        /// Absolute event time in seconds.
        t: f64,
        /// Raw input bytes.
        bytes: Vec<u8>,
    },
    /// `["m"]` marker annotation.
    Marker {
        /// Absolute event time in seconds.
        t: f64,
        /// Marker text.
        text: String,
    },
}

impl CastEvent {
    #[must_use]
    pub fn t(&self) -> f64 {
        match self {
            CastEvent::Output { t, .. }
            | CastEvent::Input { t, .. }
            | CastEvent::Marker { t, .. } => *t,
        }
    }
}

/// An imported asciinema v2 trace: validated header + all events in file order.
#[derive(Debug, Clone)]
pub struct CastTrace {
    /// Validated header.
    pub header: CastHeader,
    /// Every event in file order (output + non-executable input + markers).
    pub events: Vec<CastEvent>,
    /// Non-fatal notes (unknown event codes), each with line + byte offset.
    pub unsupported: Vec<String>,
}

impl CastTrace {
    /// Output events as `(dt, bytes)` with `dt` = seconds since the previous
    /// OUTPUT event (first output is relative to trace start `t=0`). Input and
    /// marker events never appear here.
    #[must_use]
    pub fn output_deltas(&self) -> Vec<(f64, Vec<u8>)> {
        let mut out = Vec::new();
        let mut prev = 0.0f64;
        for e in &self.events {
            if let CastEvent::Output { t, bytes } = e {
                out.push(((t - prev).max(0.0), bytes.clone()));
                prev = *t;
            }
        }
        out
    }

    /// Feed OUTPUT bytes through caller-supplied `replay`, collecting the
    /// state after each output event. Input events are never passed (output-only
    /// direction); markers are skipped. No emulator coupling: `replay` is the
    /// caller's (e.g. their emulator's `feed` + snapshot).
    pub fn screens_via<S: Clone>(
        &self,
        initial: S,
        mut replay: impl FnMut(S, &[u8]) -> S,
    ) -> Vec<S> {
        let mut state = initial;
        let mut out = Vec::new();
        for e in &self.events {
            if let CastEvent::Output { bytes, .. } = e {
                state = replay(state, bytes);
                out.push(state.clone());
            }
        }
        out
    }
}

/// Read-only asciinema v2 import with default limits. See [`import_cast_with`].
pub fn import_cast(path: &Path) -> Result<CastTrace, CompatError> {
    import_cast_with(path, &ImportLimits::default())
}

/// Read-only asciinema v2 `.cast` import.
///
/// - Line 1 must be a JSON object with `"version": 2` and nonzero
///   `"width"`/`"height"`; anything else is [`CompatError::Version`].
/// - Event lines must be `[time, code, data]` with finite non-negative `time`,
///   `code` in `{"o","i","m"}`, string `data`; anything else is
///   [`CompatError::Content`] with the line's byte offset. Unknown codes are
///   non-fatal (reported in `unsupported`).
/// - Bounds from `lim` fail with [`CompatError::TooLarge`], never truncate.
/// - Reads only; the source is untouched and nothing recorded is executed.
pub fn import_cast_with(path: &Path, lim: &ImportLimits) -> Result<CastTrace, CompatError> {
    let text = read_bounded(path, lim)?;
    let lines = lines_with_offsets(&text);
    let (hoff, header_line) = lines.first().copied().ok_or(CompatError::Version {
        offset: 0,
        msg: "empty file: missing asciinema header".to_string(),
    })?;
    debug_assert_eq!(hoff, 0);
    let header = parse_cast_header(header_line)?;
    let mut events = Vec::new();
    let mut unsupported = Vec::new();
    let mut count = 0usize;
    for (i, (off, line)) in lines.iter().enumerate().skip(1) {
        if line.is_empty() {
            continue;
        }
        if line.len() > lim.max_line_bytes {
            return Err(CompatError::TooLarge {
                what: "line",
                limit: lim.max_line_bytes as u64,
            });
        }
        count += 1;
        if count > lim.max_events {
            return Err(CompatError::TooLarge {
                what: "events",
                limit: lim.max_events as u64,
            });
        }
        match parse_cast_event(line, *off)? {
            Some(e) => events.push(e),
            None => {
                let code = line_code_hint(line);
                unsupported.push(format!(
                    "line {} (byte {off}): unknown event code {code}",
                    i + 1
                ));
            }
        }
    }
    Ok(CastTrace {
        header,
        events,
        unsupported,
    })
}

fn parse_cast_header(line: &str) -> Result<CastHeader, CompatError> {
    let v: serde_json::Value = serde_json::from_str(line).map_err(|e| CompatError::Version {
        offset: 0,
        msg: format!("header is not JSON: {e}"),
    })?;
    let obj = v.as_object().ok_or_else(|| CompatError::Version {
        offset: 0,
        msg: "header must be a JSON object".to_string(),
    })?;
    let version =
        obj.get("version")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| CompatError::Version {
                offset: 0,
                msg: "header lacks numeric \"version\"".to_string(),
            })?;
    if version != 2 {
        return Err(CompatError::Version {
            offset: 0,
            msg: format!("unsupported asciinema version {version} (want 2)"),
        });
    }
    let dim = |key: &str| -> Result<u16, CompatError> {
        let n = obj
            .get(key)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| CompatError::Version {
                offset: 0,
                msg: format!("header lacks numeric {key:?}"),
            })?;
        if n == 0 || n > u64::from(u16::MAX) {
            return Err(CompatError::Version {
                offset: 0,
                msg: format!("header {key} out of range: {n}"),
            });
        }
        Ok(n as u16)
    };
    Ok(CastHeader {
        version,
        width: dim("width")?,
        height: dim("height")?,
        title: obj
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        term: obj
            .get("env")
            .and_then(|e| e.get("TERM"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        timestamp: obj.get("timestamp").and_then(|v| v.as_u64()),
    })
}

/// Parse one event line. `Ok(None)` = well-formed but unknown event code
/// (caller reports it as unsupported, non-fatal).
fn parse_cast_event(line: &str, off: u64) -> Result<Option<CastEvent>, CompatError> {
    let bad = |m: String| CompatError::Content {
        offset: off,
        msg: m,
    };
    let v: serde_json::Value =
        serde_json::from_str(line).map_err(|e| bad(format!("not JSON: {e}")))?;
    let arr = v
        .as_array()
        .ok_or_else(|| bad("event must be a JSON array".to_string()))?;
    if arr.len() != 3 {
        return Err(bad(format!(
            "event must have 3 elements, got {}",
            arr.len()
        )));
    }
    let t = arr[0]
        .as_f64()
        .ok_or_else(|| bad("event time must be a number".to_string()))?;
    if !t.is_finite() || t < 0.0 {
        return Err(bad(format!("event time must be finite and >= 0, got {t}")));
    }
    let code = arr[1]
        .as_str()
        .ok_or_else(|| bad("event code must be a string".to_string()))?;
    let data = arr[2]
        .as_str()
        .ok_or_else(|| bad("event data must be a string".to_string()))?;
    match code {
        "o" => Ok(Some(CastEvent::Output {
            t,
            bytes: data.as_bytes().to_vec(),
        })),
        "i" => Ok(Some(CastEvent::Input {
            t,
            bytes: data.as_bytes().to_vec(),
        })),
        "m" => Ok(Some(CastEvent::Marker {
            t,
            text: data.to_string(),
        })),
        _ => Ok(None),
    }
}

fn line_code_hint(line: &str) -> String {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|v| v.get(1).cloned())
        .map(|c| c.to_string())
        .unwrap_or_else(|| "<unparseable>".to_string())
}
