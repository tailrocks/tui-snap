//! Read-only compat importers (backlog A10).
//!
//! - [`import_cast`]: asciinema v2 `.cast` (also what our own
//!   [`export::cast_v2`](tuiscotti_render::export::cast_v2) writes, and what
//!   `microsoft/tui-test` emits — see finding below).
//! - [`import_termctrl`]: `anomalyco/terminal-control` versioned `.termctrl`
//!   JSON Lines recordings (schema v1 + v2).
//!
//! ## Competitor finding (2026-09-28, inspected in `/tmp` only)
//!
//! - `microsoft/tui-test` @ `7afb14b` has NO own trace format. Its
//!   `RecordingFormat` is `{Apng, Gif, Mp4, Cast}` (`crates/tui-test/src/api.rs`)
//!   and the `Cast` writer emits standard asciinema v2, already covered by
//!   [`import_cast`]. Nothing else to import; no format invented here.
//! - `anomalyco/terminal-control` @ `c1d4f95` HAS a versioned, schema-documented
//!   trace: `.termctrl` JSON Lines with `schemas/recording-entry-v{1,2}.schema.json`
//!   and `FORMAT_VERSION = 2` (`src/recording.rs`). Imported by
//!   [`import_termctrl`] with an explicit [`LossReport`].
//!
//! ## Guarantees
//!
//! - READ-ONLY: importers only `read` the source. They never write beside it
//!   and never execute anything recorded in it (commands, markers, titles are
//!   data; input bytes are marked non-executable and are never fed as output).
//! - OUTPUT-ONLY direction: [`CastTrace::screens_via`] /
//!   [`TermctrlTrace::screens_via`] feed caller replay functions with terminal
//!   OUTPUT bytes only. No emulator coupling lives here; the caller supplies
//!   replay.
//! - BOUNDED: [`ImportLimits`] caps line length, event count, and total bytes.
//!   Violations fail with [`CompatError::TooLarge`], never truncation.
//! - Errors carry byte offsets: header problems are
//!   [`CompatError::Version`], bad event lines [`CompatError::Content`].

use std::path::Path;

// ---------------------------------------------------------------------------
// Errors + limits
// ---------------------------------------------------------------------------

/// Compat import failure: explicit, never silent. Offsets are byte offsets of
/// the offending line's first byte in the source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatError {
    /// Filesystem failure (message carries path + cause).
    Io(String),
    /// Header/version failure (missing header, wrong version, bad dimensions).
    Version {
        /// Byte offset of the header line (always 0: header is line 1).
        offset: u64,
        /// What was wrong.
        msg: String,
    },
    /// Malformed event line.
    Content {
        /// Byte offset of the bad line's first byte.
        offset: u64,
        /// What was wrong.
        msg: String,
    },
    /// A bound from [`ImportLimits`] was exceeded. Fails; never truncates.
    TooLarge {
        /// Which bound (`line`, `events`, `bytes`).
        what: &'static str,
        /// The configured limit.
        limit: u64,
    },
}

impl std::fmt::Display for CompatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompatError::Io(e) => write!(f, "compat import I/O error: {e}"),
            CompatError::Version { offset, msg } => {
                write!(f, "compat import version error at byte {offset}: {msg}")
            }
            CompatError::Content { offset, msg } => {
                write!(f, "compat import content error at byte {offset}: {msg}")
            }
            CompatError::TooLarge { what, limit } => {
                write!(f, "compat import too large: {what} exceeds limit {limit}")
            }
        }
    }
}

impl std::error::Error for CompatError {}

/// Bounds for every importer in this module. Defaults are generous for real
/// traces but finite so hostile files fail fast instead of exhausting memory.
#[derive(Debug, Clone)]
pub struct ImportLimits {
    /// Max events (non-header lines carrying an entry) per file.
    pub max_events: usize,
    /// Max total source bytes read.
    pub max_total_bytes: u64,
    /// Max bytes per line (excluding the `\n`).
    pub max_line_bytes: usize,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_events: 100_000,
            max_total_bytes: 64 << 20,
            max_line_bytes: 4 << 20,
        }
    }
}

/// Read a source file with the total-bytes bound applied. Read-only: a single
/// `read`, no writes, no command execution anywhere in this module.
fn read_bounded(path: &Path, lim: &ImportLimits) -> Result<String, CompatError> {
    let len = std::fs::metadata(path)
        .map_err(|e| CompatError::Io(format!("stat {}: {e}", path.display())))?
        .len();
    if len > lim.max_total_bytes {
        return Err(CompatError::TooLarge {
            what: "bytes",
            limit: lim.max_total_bytes,
        });
    }
    let bytes = std::fs::read(path)
        .map_err(|e| CompatError::Io(format!("read {}: {e}", path.display())))?;
    if bytes.len() as u64 > lim.max_total_bytes {
        return Err(CompatError::TooLarge {
            what: "bytes",
            limit: lim.max_total_bytes,
        });
    }
    String::from_utf8(bytes).map_err(|e| CompatError::Content {
        offset: e.utf8_error().valid_up_to() as u64,
        msg: format!("source is not UTF-8: {e}"),
    })
}

/// Split into `(byte_offset, line)` pairs, skipping the empty segment after a
/// trailing newline. Offsets count the stripped `\n` (and `\r` of CRLF).
fn lines_with_offsets(text: &str) -> Vec<(u64, &str)> {
    let mut out = Vec::new();
    let mut off: u64 = 0;
    let mut rest = text;
    while !rest.is_empty() {
        let (line, adv) = match rest.find('\n') {
            Some(i) => (&rest[..i], i + 1),
            None => (rest, rest.len()),
        };
        let line = line.strip_suffix('\r').unwrap_or(line);
        if !(line.is_empty() && adv == 0) {
            out.push((off, line));
        }
        off += adv as u64;
        rest = &rest[adv..];
    }
    out
}

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

// ---------------------------------------------------------------------------
// terminal-control .termctrl recordings
// ---------------------------------------------------------------------------

/// Explicit loss report for competitor-trace import (backlog A10): nothing is
/// silently dropped or normalized away.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LossReport {
    /// Unknown fields seen on otherwise-valid entries, each with line + byte
    /// offset (e.g. `line 4 (byte 120): output.extra`). Non-fatal.
    pub unsupported_fields: Vec<String>,
    /// Entries skipped deliberately with reasons (unknown entry types,
    /// version-gated entries such as v2 `mouse` in a v1 file), each with
    /// line + byte offset. Non-fatal.
    pub dropped_events: Vec<String>,
}

impl LossReport {
    /// True when nothing was lost: no unknown fields, no dropped entries.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.unsupported_fields.is_empty() && self.dropped_events.is_empty()
    }
}

/// One `.termctrl` entry. `Input`/`Mouse` carry delivered CLIENT input bytes:
/// non-executable, excluded from [`TermctrlTrace::output_deltas`] and
/// [`TermctrlTrace::screens_via`], never fed as output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermctrlEvent {
    /// Terminal output at `at_ms` milliseconds since recording start.
    Output {
        /// Milliseconds since recording start.
        at_ms: u64,
        /// Raw output bytes.
        bytes: Vec<u8>,
    },
    /// Typed input written to the app (NON-EXECUTABLE, never replayed).
    Input {
        /// Milliseconds since recording start.
        at_ms: u64,
        /// `client` or `host` origin from the source.
        origin: String,
        /// Raw input bytes.
        bytes: Vec<u8>,
    },
    /// Delivered mouse input (NON-EXECUTABLE, never replayed). v2 only.
    Mouse {
        /// Milliseconds since recording start.
        at_ms: u64,
        /// Raw encoded mouse bytes.
        bytes: Vec<u8>,
    },
    /// Terminal resize.
    Resize {
        /// Milliseconds since recording start.
        at_ms: u64,
        /// New width in columns.
        cols: u16,
        /// New height in rows.
        rows: u16,
    },
    /// Named marker (data only — never executed).
    Marker {
        /// Milliseconds since recording start.
        at_ms: u64,
        /// Marker name.
        name: String,
    },
}

impl TermctrlEvent {
    #[must_use]
    pub fn at_ms(&self) -> u64 {
        match self {
            TermctrlEvent::Output { at_ms, .. }
            | TermctrlEvent::Input { at_ms, .. }
            | TermctrlEvent::Mouse { at_ms, .. }
            | TermctrlEvent::Resize { at_ms, .. }
            | TermctrlEvent::Marker { at_ms, .. } => *at_ms,
        }
    }
}

/// An imported `.termctrl` trace: validated header dims + entries in file
/// order + the explicit [`LossReport`].
#[derive(Debug, Clone)]
pub struct TermctrlTrace {
    /// Recording schema version (1 or 2).
    pub version: u8,
    /// Header terminal width in columns.
    pub cols: u16,
    /// Header terminal height in rows.
    pub rows: u16,
    /// Entries in file order (dropped entries excluded; see `loss`).
    pub events: Vec<TermctrlEvent>,
    /// Explicit loss report: unknown fields + dropped entries.
    pub loss: LossReport,
}

impl TermctrlTrace {
    /// Output entries as `(dt, bytes)` with `dt` = seconds since the previous
    /// OUTPUT entry (first output relative to start). Input/mouse/resize/marker
    /// entries never appear here.
    #[must_use]
    pub fn output_deltas(&self) -> Vec<(f64, Vec<u8>)> {
        let mut out = Vec::new();
        let mut prev = 0u64;
        for e in &self.events {
            if let TermctrlEvent::Output { at_ms, bytes } = e {
                out.push((at_ms.saturating_sub(prev) as f64 / 1000.0, bytes.clone()));
                prev = *at_ms;
            }
        }
        out
    }

    /// Feed OUTPUT bytes through caller-supplied `replay`, collecting the
    /// state after each output entry. Input/mouse bytes are never passed
    /// (output-only direction); resize/marker entries are skipped. No emulator
    /// coupling: `replay` is the caller's.
    pub fn screens_via<S: Clone>(
        &self,
        initial: S,
        mut replay: impl FnMut(S, &[u8]) -> S,
    ) -> Vec<S> {
        let mut state = initial;
        let mut out = Vec::new();
        for e in &self.events {
            if let TermctrlEvent::Output { bytes, .. } = e {
                state = replay(state, bytes);
                out.push(state.clone());
            }
        }
        out
    }
}

/// Read-only `.termctrl` import with default limits. See [`import_termctrl_with`].
pub fn import_termctrl(path: &Path) -> Result<TermctrlTrace, CompatError> {
    import_termctrl_with(path, &ImportLimits::default())
}

/// Read-only `terminal-control` `.termctrl` JSON Lines import (schema v1 + v2).
///
/// - Line 1 must be `{"type":"header","version":1|2,...}` with nonzero
///   `cols`/`rows`; anything else is [`CompatError::Version`].
/// - Entry lines must be JSON objects with a known `type`
///   (`output`/`input`/`mouse`/`resize`/`marker`) and schema-shaped fields;
///   malformed lines are [`CompatError::Content`] with byte offsets.
/// - Forward-compatible losses are REPORTED, never silent: unknown entry
///   types and v2-only `mouse` entries in v1 files go to
///   [`LossReport::dropped_events`]; unknown fields on known entries go to
///   [`LossReport::unsupported_fields`].
/// - Bounds from `lim` fail with [`CompatError::TooLarge`], never truncate.
/// - Reads only; the source is untouched and nothing recorded is executed.
pub fn import_termctrl_with(path: &Path, lim: &ImportLimits) -> Result<TermctrlTrace, CompatError> {
    let text = read_bounded(path, lim)?;
    let lines = lines_with_offsets(&text);
    let (_, header_line) = lines.first().copied().ok_or(CompatError::Version {
        offset: 0,
        msg: "empty file: missing termctrl header".to_string(),
    })?;
    let (version, cols, rows) = parse_termctrl_header(header_line)?;
    let mut events = Vec::new();
    let mut loss = LossReport::default();
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
        let lineno = i + 1;
        match parse_termctrl_entry(line, *off, version)? {
            TermctrlParse::Event(e) => events.push(e),
            TermctrlParse::Dropped(reason) => loss
                .dropped_events
                .push(format!("line {lineno} (byte {off}): {reason}")),
            TermctrlParse::FieldNotes(mut notes, e) => {
                for n in notes.drain(..) {
                    loss.unsupported_fields
                        .push(format!("line {lineno} (byte {off}): {n}"));
                }
                events.push(e);
            }
        }
    }
    Ok(TermctrlTrace {
        version,
        cols,
        rows,
        events,
        loss,
    })
}

fn parse_termctrl_header(line: &str) -> Result<(u8, u16, u16), CompatError> {
    let verr = |m: String| CompatError::Version { offset: 0, msg: m };
    let v: serde_json::Value =
        serde_json::from_str(line).map_err(|e| verr(format!("header is not JSON: {e}")))?;
    let obj = v
        .as_object()
        .ok_or_else(|| verr("header must be a JSON object".to_string()))?;
    if obj.get("type").and_then(|v| v.as_str()) != Some("header") {
        return Err(verr("first line must be a \"header\" entry".to_string()));
    }
    let version = obj
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| verr("header lacks numeric \"version\"".to_string()))?;
    if version != 1 && version != 2 {
        return Err(verr(format!(
            "unsupported termctrl version {version} (want 1 or 2)"
        )));
    }
    let dim = |key: &str| -> Result<u16, CompatError> {
        let n = obj
            .get(key)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| verr(format!("header lacks numeric {key:?}")))?;
        if n == 0 || n > u64::from(u16::MAX) {
            return Err(verr(format!("header {key} out of range: {n}")));
        }
        Ok(n as u16)
    };
    Ok((version as u8, dim("cols")?, dim("rows")?))
}

enum TermctrlParse {
    Event(TermctrlEvent),
    Dropped(String),
    FieldNotes(Vec<String>, TermctrlEvent),
}

fn parse_termctrl_entry(line: &str, off: u64, version: u8) -> Result<TermctrlParse, CompatError> {
    let bad = |m: String| CompatError::Content {
        offset: off,
        msg: m,
    };
    let v: serde_json::Value =
        serde_json::from_str(line).map_err(|e| bad(format!("not JSON: {e}")))?;
    let obj = v
        .as_object()
        .ok_or_else(|| bad("entry must be a JSON object".to_string()))?;
    let ty = obj
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad("entry lacks string \"type\"".to_string()))?;
    let at_ms = |obj: &serde_json::Map<String, serde_json::Value>| -> Result<u64, CompatError> {
        obj.get("at_ms")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| bad("entry lacks numeric \"at_ms\"".to_string()))
    };
    let byte_array =
        |obj: &serde_json::Map<String, serde_json::Value>| -> Result<Vec<u8>, CompatError> {
            let arr = obj
                .get("bytes")
                .and_then(|v| v.as_array())
                .ok_or_else(|| bad("entry lacks \"bytes\" array".to_string()))?;
            let mut out = Vec::with_capacity(arr.len());
            for b in arr {
                let n = b
                    .as_u64()
                    .ok_or_else(|| bad("bytes must be integers".to_string()))?;
                if n > 255 {
                    return Err(bad(format!("byte out of range: {n}")));
                }
                out.push(n as u8);
            }
            Ok(out)
        };
    let known: &[&str] = match ty {
        "output" => &["type", "at_ms", "bytes"],
        "input" => &["type", "at_ms", "origin", "bytes"],
        "mouse" => &["type", "at_ms", "event", "bytes"],
        "resize" => &["type", "at_ms", "cols", "rows", "cell_width", "cell_height"],
        "marker" => &["type", "at_ms", "name"],
        _ => return Ok(TermctrlParse::Dropped(format!("unknown entry type {ty:?}"))),
    };
    if ty == "mouse" && version < 2 {
        return Ok(TermctrlParse::Dropped(
            "mouse entry requires recording version 2".to_string(),
        ));
    }
    let mut notes = Vec::new();
    for k in obj.keys() {
        if !known.contains(&k.as_str()) {
            notes.push(format!("{ty}.{k}"));
        }
    }
    let event = match ty {
        "output" => TermctrlEvent::Output {
            at_ms: at_ms(obj)?,
            bytes: byte_array(obj)?,
        },
        "input" => {
            let origin = obj
                .get("origin")
                .and_then(|v| v.as_str())
                .ok_or_else(|| bad("input lacks string \"origin\"".to_string()))?;
            if origin != "client" && origin != "host" {
                return Err(bad(format!("bad input origin {origin:?}")));
            }
            TermctrlEvent::Input {
                at_ms: at_ms(obj)?,
                origin: origin.to_string(),
                bytes: byte_array(obj)?,
            }
        }
        "mouse" => TermctrlEvent::Mouse {
            at_ms: at_ms(obj)?,
            bytes: byte_array(obj)?,
        },
        "resize" => {
            let dim = |key: &str| -> Result<u16, CompatError> {
                let n = obj
                    .get(key)
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| bad(format!("resize lacks numeric {key:?}")))?;
                if n == 0 || n > u64::from(u16::MAX) {
                    return Err(bad(format!("resize {key} out of range: {n}")));
                }
                Ok(n as u16)
            };
            TermctrlEvent::Resize {
                at_ms: at_ms(obj)?,
                cols: dim("cols")?,
                rows: dim("rows")?,
            }
        }
        "marker" => {
            let name = obj
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| bad("marker lacks string \"name\"".to_string()))?;
            if name.is_empty() {
                return Err(bad("marker name must be nonempty".to_string()));
            }
            TermctrlEvent::Marker {
                at_ms: at_ms(obj)?,
                name: name.to_string(),
            }
        }
        _ => unreachable!("type gated above"),
    };
    if notes.is_empty() {
        Ok(TermctrlParse::Event(event))
    } else {
        Ok(TermctrlParse::FieldNotes(notes, event))
    }
}
