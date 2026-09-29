use std::path::Path;
use super::*;


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
