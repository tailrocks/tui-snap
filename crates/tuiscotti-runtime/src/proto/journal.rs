use std::path::Path;

use super::{OpError, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Bounded recording (A04/A06 partial)
// ---------------------------------------------------------------------------

/// One journal event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalEvent {
    /// Monotonic event number within the journal.
    pub seq: u64,
    /// Event kind string.
    pub kind: String,
    /// Human-readable event detail.
    pub detail: String,
}

/// Append-only JSONL recorder with hard bounds. Exceeding a bound is an
/// error, never silent truncation.
#[derive(Debug)]
pub struct Recorder {
    file: std::fs::File,
    seq: u64,
    bytes: u64,
    max_events: u64,
    max_bytes: u64,
}

impl Recorder {
    /// Create a recorder appending to `path`, creating parent dirs as needed.
    ///
    /// # Errors
    ///
    /// Returns [`OpError`] for zero bounds or when the file cannot be created.
    pub fn create(path: &Path, max_events: u64, max_bytes: u64) -> Result<Self, OpError> {
        if max_events == 0 || max_bytes == 0 {
            return Err(OpError::new(
                "invalid-input",
                "record bounds must be nonzero",
            ));
        }
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| OpError::new("io", format!("mkdir {}: {e}", parent.display())))?;
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

    /// Append one event; exceeding a bound is an error, never truncation.
    ///
    /// # Errors
    ///
    /// Returns [`OpError`] when a bound is exceeded or the append fails.
    pub fn record(&mut self, kind: &str, detail: &str) -> Result<(), OpError> {
        use std::io::Write;
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
        self.file
            .write_all(&line)
            .map_err(|e| OpError::new("io", format!("append journal: {e}")))?;
        self.seq += 1;
        self.bytes += line.len() as u64;
        Ok(())
    }

    /// Number of events recorded so far.
    #[must_use]
    pub fn events(&self) -> u64 {
        self.seq
    }
}

/// Default event cap for [`read_journal`] / [`iter_journal`]: a million-event
/// file is corrupt or hostile (explicit `_bounded` variants when needed).
pub const MAX_JOURNAL_READ_EVENTS: u64 = 1_000_000;

/// Default byte cap for [`read_journal`] / [`iter_journal`] (raw line bytes
/// including newlines). See [`MAX_JOURNAL_READ_EVENTS`].
pub const MAX_JOURNAL_READ_BYTES: u64 = 256 * 1024 * 1024;

/// Bounded journal read (offline; used by `inspect`): `bound-exceeded` past
/// the default caps, never silent truncation; small journals decode as before.
///
/// # Errors
///
/// Returns [`OpError`] on unreadable files, bad events, or exceeded bounds.
pub fn read_journal(path: &Path) -> Result<Vec<JournalEvent>, OpError> {
    read_journal_bounded(path, MAX_JOURNAL_READ_EVENTS, MAX_JOURNAL_READ_BYTES)
}

/// [`read_journal`] with explicit bounds (zero bounds reject everything).
///
/// # Errors
///
/// Same as [`read_journal`].
pub fn read_journal_bounded(
    path: &Path,
    max_events: u64,
    max_bytes: u64,
) -> Result<Vec<JournalEvent>, OpError> {
    iter_journal_bounded(path, max_events, max_bytes)?.collect()
}

/// Stream a journal event-by-event (offline; used by `trace`). Same bounds
/// and errors as [`read_journal`], but constant-memory: `trace` never
/// materializes the whole file.
///
/// # Errors
///
/// Returns [`OpError`] when the file cannot be opened. Decode, bound, and
/// I/O failures surface as `Err` items (iteration stops after the first).
pub fn iter_journal(path: &Path) -> Result<JournalReader, OpError> {
    iter_journal_bounded(path, MAX_JOURNAL_READ_EVENTS, MAX_JOURNAL_READ_BYTES)
}

/// [`iter_journal`] with explicit bounds (zero bounds reject everything).
///
/// # Errors
///
/// Same as [`iter_journal`].
pub fn iter_journal_bounded(
    path: &Path,
    max_events: u64,
    max_bytes: u64,
) -> Result<JournalReader, OpError> {
    use std::io::BufRead;
    let file = std::fs::File::open(path)
        .map_err(|e| OpError::new("io", format!("open {}: {e}", path.display())))?;
    Ok(JournalReader {
        path: path.to_path_buf(),
        lines: std::io::BufReader::new(file).lines(),
        line_no: 0,
        events: 0,
        bytes: 0,
        max_events,
        max_bytes,
        failed: false,
    })
}

/// Streaming decoder from [`iter_journal`]: same line-numbered errors and
/// bounds as [`read_journal`]; `None` forever after the first `Err` or EOF.
#[derive(Debug)]
pub struct JournalReader {
    path: std::path::PathBuf,
    lines: std::io::Lines<std::io::BufReader<std::fs::File>>,
    line_no: u64,
    events: u64,
    bytes: u64,
    max_events: u64,
    max_bytes: u64,
    failed: bool,
}

impl JournalReader {
    /// `path line N` context shared by every decode/bound error.
    fn at_line(&self) -> String {
        format!("{} line {}", self.path.display(), self.line_no)
    }

    /// Poison the iterator with `err` (nothing yields after the first error).
    fn fail(&mut self, err: OpError) -> Result<JournalEvent, OpError> {
        self.failed = true;
        Err(err)
    }

    fn decode_one(&mut self, line: &str) -> Result<Option<JournalEvent>, OpError> {
        if line.trim().is_empty() {
            return Ok(None);
        }
        if self.events >= self.max_events {
            let at = self.at_line();
            let cap = self.max_events;
            return Err(OpError::new(
                "bound-exceeded",
                format!("{at}: event cap {cap} reached"),
            ));
        }
        let at = self.at_line();
        let ev: JournalEvent = serde_json::from_str(line)
            .map_err(|e| OpError::new("invalid-input", format!("{at}: bad event: {e}")))?;
        self.events += 1;
        Ok(Some(ev))
    }
}

impl Iterator for JournalReader {
    type Item = Result<JournalEvent, OpError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        loop {
            let line = match self.lines.next()? {
                Ok(line) => line,
                Err(e) => {
                    let msg = format!("read {}: {e}", self.path.display());
                    return Some(self.fail(OpError::new("io", msg)));
                }
            };
            self.line_no += 1;
            // `lines()` strips the terminator; count one byte back (overcounting is safe).
            self.bytes += line.len() as u64 + 1;
            if self.bytes > self.max_bytes {
                let msg = format!("{}: byte cap {} reached", self.at_line(), self.max_bytes);
                return Some(self.fail(OpError::new("bound-exceeded", msg)));
            }
            match self.decode_one(&line) {
                Ok(None) => {} // Blank: next iteration.
                Ok(Some(ev)) => return Some(Ok(ev)),
                Err(e) => return Some(self.fail(e)),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Offline review/report
// ---------------------------------------------------------------------------

/// One offline verdict file (`<name>.verdict.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Verdict {
    /// Verdict name (file stem).
    pub name: String,
    /// Verdict status (`pass` or `fail`).
    pub status: String,
    /// Human-readable verdict detail.
    #[serde(default)]
    pub detail: String,
}

impl Verdict {
    /// True when the status is `pass`.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.status == "pass"
    }
}

/// Read all `*.verdict.json` files in `dir` (sorted by name). Non-verdict
/// files are ignored; a malformed verdict file is an error.
///
/// # Errors
///
/// Returns [`OpError`] when the dir cannot be read or a verdict is malformed.
pub fn read_verdicts(dir: &Path) -> Result<Vec<Verdict>, OpError> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| OpError::new("io", format!("read {}: {e}", dir.display())))?
        .collect::<Result<_, _>>()
        .map_err(|e| OpError::new("io", format!("read {}: {e}", dir.display())))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let mut out = Vec::new();
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".verdict.json") {
            continue;
        }
        let bytes = std::fs::read(entry.path())
            .map_err(|e| OpError::new("io", format!("read {name}: {e}")))?;
        let v: Verdict = serde_json::from_slice(&bytes)
            .map_err(|e| OpError::new("invalid-input", format!("{name}: bad verdict: {e}")))?;
        out.push(v);
    }
    Ok(out)
}

/// Write a standalone offline HTML report from `verdicts`. Pure rendering over
/// the given verdicts; reads nothing else.
#[must_use]
pub fn write_html_report(verdicts: &[Verdict], title: &str) -> String {
    use std::fmt::Write as _;
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
        writeln!(
            rows,
            "<tr class=\"{cls}\"><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(&v.name),
            esc(&v.status),
            esc(&v.detail)
        )
        .ok();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(suffix: &str) -> std::path::PathBuf {
        let name = format!("tuiscotti-proto-journal-{suffix}-{}", std::process::id());
        std::env::temp_dir().join(name)
    }

    #[test]
    fn bounded_read_matches_stream_and_trips_caps() {
        let dir = tmp("bounded");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("mkdir succeeds");
        let path = dir.join("j.jsonl");
        std::fs::write(&path, "{\"seq\":0,\"kind\":\"s\",\"detail\":\"a\"}\n{\"seq\":1,\"kind\":\"e\",\"detail\":\"b\"}\n")
            .expect("fixture write succeeds");
        let all = read_journal(&path).expect("small journal reads");
        assert_eq!(all.len(), 2);
        let streamed: Vec<JournalEvent> = iter_journal(&path)
            .expect("stream opens")
            .collect::<Result<_, _>>()
            .expect("stream decodes");
        assert_eq!(streamed, all, "stream yields the same events");
        for (events, bytes) in [(1, u64::MAX), (u64::MAX, 10)] {
            let err = read_journal_bounded(&path, events, bytes).expect_err("cap trips");
            assert!(err.to_string().contains("bound-exceeded"), "{err}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bad_events_keep_line_numbers() {
        let dir = tmp("badline");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("mkdir succeeds");
        let path = dir.join("j.jsonl");
        std::fs::write(
            &path,
            "{\"seq\":0,\"kind\":\"s\",\"detail\":\"a\"}\nnot-json\n",
        )
        .expect("fixture write succeeds");
        let err = read_journal(&path).expect_err("bad event fails");
        assert!(err.to_string().contains("line 2"), "{err}");
        let mut reader = iter_journal(&path).expect("stream opens");
        assert!(reader.next().expect("first event").is_ok());
        let err = reader
            .next()
            .expect("second item")
            .expect_err("stream fails too");
        assert!(err.to_string().contains("line 2"), "{err}");
        assert!(reader.next().is_none(), "stream ends after the error");
        std::fs::remove_dir_all(&dir).ok();
    }
}
