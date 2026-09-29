use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Bounded recording (A04/A06 partial)
// ---------------------------------------------------------------------------

/// One journal event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalEvent {
    pub seq: u64,
    pub kind: String,
    pub detail: String,
}

/// Append-only JSONL recorder with hard bounds. Exceeding a bound is an
/// error, never silent truncation.
pub struct Recorder {
    file: std::fs::File,
    seq: u64,
    bytes: u64,
    max_events: u64,
    max_bytes: u64,
}

impl Recorder {
    pub fn create(path: &Path, max_events: u64, max_bytes: u64) -> Result<Self, OpError> {
        if max_events == 0 || max_bytes == 0 {
            return Err(OpError::new(
                "invalid-input",
                "record bounds must be nonzero",
            ));
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| OpError::new("io", format!("mkdir {}: {e}", parent.display())))?;
            }
        }
        let file = std::fs::File::create(path)
            .map_err(|e| OpError::new("io", format!("create {}: {e}", path.display())))?;
        Ok(Self {
            file,
            seq: 0,
            bytes: 0,
            max_events,
            max_bytes,
        })
    }

    pub fn record(&mut self, kind: &str, detail: &str) -> Result<(), OpError> {
        if self.seq >= self.max_events {
            return Err(OpError::new(
                "bound-exceeded",
                format!("event cap {} reached", self.max_events),
            ));
        }
        let ev = JournalEvent {
            seq: self.seq,
            kind: kind.to_string(),
            detail: detail.to_string(),
        };
        let mut line = serde_json::to_vec(&ev)
            .map_err(|e| OpError::new("io", format!("encode event: {e}")))?;
        line.push(b'\n');
        if self.bytes + line.len() as u64 > self.max_bytes {
            return Err(OpError::new(
                "bound-exceeded",
                format!("byte cap {} reached", self.max_bytes),
            ));
        }
        use std::io::Write;
        self.file
            .write_all(&line)
            .map_err(|e| OpError::new("io", format!("append journal: {e}")))?;
        self.seq += 1;
        self.bytes += line.len() as u64;
        Ok(())
    }

    #[must_use]
    pub fn events(&self) -> u64 {
        self.seq
    }
}

/// Read a journal back (offline; used by `trace`).
pub fn read_journal(path: &Path) -> Result<Vec<JournalEvent>, OpError> {
    use std::io::BufRead;
    let file = std::fs::File::open(path)
        .map_err(|e| OpError::new("io", format!("open {}: {e}", path.display())))?;
    let mut out = Vec::new();
    for (n, line) in std::io::BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|e| OpError::new("io", format!("read {}: {e}", path.display())))?;
        if line.trim().is_empty() {
            continue;
        }
        let ev: JournalEvent = serde_json::from_str(&line).map_err(|e| {
            OpError::new(
                "invalid-input",
                format!("{} line {}: bad event: {e}", path.display(), n + 1),
            )
        })?;
        out.push(ev);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Offline review/report
// ---------------------------------------------------------------------------

/// One offline verdict file (`<name>.verdict.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Verdict {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub detail: String,
}

impl Verdict {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.status == "pass"
    }
}

/// Read all `*.verdict.json` files in `dir` (sorted by name). Non-verdict
/// files are ignored; a malformed verdict file is an error.
pub fn read_verdicts(dir: &Path) -> Result<Vec<Verdict>, OpError> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| OpError::new("io", format!("read {}: {e}", dir.display())))?
        .collect::<Result<_, _>>()
        .map_err(|e| OpError::new("io", format!("read {}: {e}", dir.display())))?;
    entries.sort_by_key(|e| e.file_name());
    let mut out = Vec::new();
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".verdict.json") {
            continue;
        }
        let bytes = std::fs::read(entry.path())
            .map_err(|e| OpError::new("io", format!("read {}: {e}", name)))?;
        let v: Verdict = serde_json::from_slice(&bytes)
            .map_err(|e| OpError::new("invalid-input", format!("{name}: bad verdict: {e}")))?;
        out.push(v);
    }
    Ok(out)
}

/// Write a standalone offline HTML report from `verdicts`. Pure rendering over
/// the given verdicts; reads nothing else.
pub fn write_html_report(verdicts: &[Verdict], title: &str) -> String {
    fn esc(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    let passed = verdicts.iter().filter(|v| v.passed()).count();
    let failed = verdicts.len() - passed;
    let mut rows = String::new();
    for v in verdicts {
        let cls = if v.passed() { "pass" } else { "fail" };
        rows.push_str(&format!(
            "<tr class=\"{cls}\"><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            esc(&v.name),
            esc(&v.status),
            esc(&v.detail)
        ));
    }
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{t}</title>\
<style>body{{font-family:sans-serif}}table{{border-collapse:collapse}}\
td,th{{border:1px solid #ccc;padding:4px 8px}}.pass td{{background:#e6f4ea}}\
.fail td{{background:#fce8e6}}</style></head><body><h1>{t}</h1>\
<p>{} passed, {} failed (protocol v{})</p>\
<table><tr><th>name</th><th>status</th><th>detail</th></tr>\n{rows}</table></body></html>\n",
        passed,
        failed,
        PROTOCOL_VERSION,
        t = esc(title),
    )
}
