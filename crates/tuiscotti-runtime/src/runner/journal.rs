use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Default event cap per journal: a million-event journal is a runaway
/// writer, never a real attempt. See [`Journal::open_with_limits`].
pub const MAX_JOURNAL_EVENTS: u64 = 1_000_000;

/// Default byte cap per journal on disk (existing + appended).
pub const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;

/// Cap on one encoded event line (JSON + newline): no single `append` blows
/// past the byte cap, and every line [`Journal::status`] reads fits its window.
pub const MAX_EVENT_LINE_BYTES: usize = 64 * 1024;

/// [`Journal::status`] reads at most this many tail bytes (completion is a
/// last-line property). A multiple of [`MAX_EVENT_LINE_BYTES`], so any
/// API-produced final line is fully visible.
pub const STATUS_TAIL_BYTES: u64 = 128 * 1024;

/// Append-only JSONL event journal (N07).
///
/// Every [`Journal::append`] flushes, so a killed or timed-out attempt leaves
/// its last flushed events inspectable. Completion requires an explicit
/// [`Journal::complete`]; a journal without the completion marker is
/// [`JournalStatus::Incomplete`], never a pass.
///
/// Bounded: past the event, byte, or per-line caps, `append` fails with an
/// explicit `QuotaExceeded` error — never silent truncation.
#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
    file: File,
    seq: u64,
    bytes: u64,
    max_events: u64,
    max_bytes: u64,
}

/// Completion marker filename written beside `journal.jsonl`.
pub const COMPLETE_MARKER: &str = "COMPLETE";

impl Journal {
    /// Open (or resume) the journal at `path`, creating parent directories.
    /// The sequence counter resumes after the existing line count.
    /// Default bounds ([`MAX_JOURNAL_EVENTS`] / [`MAX_JOURNAL_BYTES`]) apply.
    /// # Errors
    ///
    /// Returns an I/O error when directories or the journal cannot be opened.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        Self::open_with_limits(path, MAX_JOURNAL_EVENTS, MAX_JOURNAL_BYTES)
    }

    /// [`Journal::open`] with explicit bounds (zero bounds are rejected).
    /// # Errors
    ///
    /// Returns an I/O error when a bound is zero, or when directories or the
    /// journal cannot be opened.
    pub fn open_with_limits(path: &Path, max_events: u64, max_bytes: u64) -> std::io::Result<Self> {
        if max_events == 0 || max_bytes == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "journal bounds must be nonzero",
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let (seq, bytes) = resume_counters(path, max_events, max_bytes);
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            seq,
            bytes,
            max_events,
            max_bytes,
        })
    }

    /// Journal path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one event and flush (minimal std-only JSON escaping). Fails
    /// explicitly past the event, byte, or per-line caps.
    /// # Errors
    ///
    /// Returns an I/O error when a bound is exceeded, or when the event
    /// cannot be written or flushed.
    pub fn append(&mut self, event: &str, detail: &str) -> std::io::Result<()> {
        if self.seq >= self.max_events {
            return Err(quota_exceeded(format!(
                "journal event cap {} reached",
                self.max_events
            )));
        }
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let ev = json_escape(event);
        let de = json_escape(detail);
        let line = format!(
            "{{\"seq\":{},\"unix_ms\":{ms},\"event\":\"{ev}\",\"detail\":\"{de}\"}}\n",
            self.seq
        );
        if line.len() > MAX_EVENT_LINE_BYTES {
            let msg = format!(
                "journal event line {} bytes exceeds {MAX_EVENT_LINE_BYTES}",
                line.len()
            );
            return Err(quota_exceeded(msg));
        }
        if self.bytes + line.len() as u64 > self.max_bytes {
            return Err(quota_exceeded(format!(
                "journal byte cap {} reached",
                self.max_bytes
            )));
        }
        self.file.write_all(line.as_bytes())?;
        self.file.flush()?;
        self.seq += 1;
        self.bytes += line.len() as u64;
        Ok(())
    }

    /// Mark the attempt complete with a terminal `status`, then write the
    /// `COMPLETE` marker. Fail-closed: anything killed before this stays incomplete.
    /// # Errors
    ///
    /// Returns an I/O error when the event or marker cannot be written.
    pub fn complete(&mut self, status: &str) -> std::io::Result<()> {
        self.append("complete", status)?;
        if let Some(parent) = self.path.parent() {
            fs::write(parent.join(COMPLETE_MARKER), format!("{status}\n"))?;
        }
        Ok(())
    }

    /// Completion status of a journal directory: complete only when the
    /// `COMPLETE` marker exists **and** the last event is `complete`.
    /// Bounded to [`STATUS_TAIL_BYTES`] of tail read; small journals
    /// verdict exactly as before.
    #[must_use]
    pub fn status(dir: &Path) -> JournalStatus {
        let marker = dir.join(COMPLETE_MARKER);
        if !marker.is_file() {
            return incomplete(format!(
                "missing {} marker",
                dir.join(COMPLETE_MARKER).display()
            ));
        }
        let tail = match read_last_line(&dir.join("journal.jsonl")) {
            Ok(t) => t,
            Err(e) => return incomplete(format!("cannot read journal.jsonl: {e}")),
        };
        match tail {
            TailLine::Line(last) if is_complete_event(&last) => JournalStatus::Complete {
                status: extract_field(&last, "detail").unwrap_or_default(),
            },
            TailLine::Line(_) | TailLine::Empty => {
                incomplete("journal tail is not a complete event".into())
            }
            TailLine::BeyondWindow => incomplete(format!(
                "journal tail is not fully inside the {STATUS_TAIL_BYTES}-byte status window"
            )),
        }
    }
}

fn incomplete(reason: String) -> JournalStatus {
    JournalStatus::Incomplete { reason }
}

/// Bound-exceeded I/O error (explicit saturation, never silent truncation).
fn quota_exceeded(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::QuotaExceeded, message)
}

/// Resume counters for an existing journal. The scan stops at the bounds: a
/// pre-existing file past them resumes saturated (next `append` fails).
fn resume_counters(path: &Path, max_events: u64, max_bytes: u64) -> (u64, u64) {
    let Ok(file) = fs::File::open(path) else {
        return (0, 0);
    };
    let bytes = file.metadata().map_or(0, |m| m.len()).min(max_bytes);
    let mut seq: u64 = 0;
    for result in BufReader::new(file).lines() {
        if result.is_err() || seq >= max_events {
            break;
        }
        seq += 1;
    }
    (seq.min(max_events), bytes)
}

/// The journal's last line as [`Journal::status`] sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TailLine {
    /// Last non-empty complete line.
    Line(String),
    /// No non-empty line (missing, empty, or all-blank journal).
    Empty,
    /// Tail content starts before the window (hand-planted files only).
    BeyondWindow,
}

/// Last non-empty line of `path` from the tail window. Files within it read
/// exactly; larger files skip the leading partial line (none after it means
/// the tail content starts before the window).
fn read_last_line(path: &Path) -> std::io::Result<TailLine> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    let take = len.min(STATUS_TAIL_BYTES);
    if take == 0 {
        return Ok(TailLine::Empty);
    }
    file.seek(SeekFrom::End(-i64::try_from(take).unwrap_or(i64::MAX)))?;
    let mut buf = vec![0u8; usize::try_from(take).unwrap_or(usize::MAX)];
    file.read_exact(&mut buf)?;
    let windowed = len > take;
    let mut slice: &[u8] = &buf;
    if windowed {
        // The window cut the file: skip the leading partial line (it
        // extends before the window) so only complete lines remain.
        match buf.iter().position(|&b| b == b'\n') {
            Some(i) => slice = &buf[i + 1..],
            None => return Ok(TailLine::BeyondWindow),
        }
    }
    let text = std::str::from_utf8(slice).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("journal is not UTF-8: {e}"),
        )
    })?;
    match text.lines().rfind(|l| !l.trim().is_empty()) {
        Some(last) => Ok(TailLine::Line(last.to_string())),
        // Cut file, no complete line: the tail content starts before the window.
        None if windowed => Ok(TailLine::BeyondWindow),
        None => Ok(TailLine::Empty),
    }
}

/// Journal completion state (N07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalStatus {
    /// Explicitly completed with this terminal status string.
    Complete {
        /// Terminal status string.
        status: String,
    },
    /// Killed, timed out, or never finished: not a pass.
    Incomplete {
        /// Why the journal counts as incomplete.
        reason: String,
    },
}

impl JournalStatus {
    /// True only for [`JournalStatus::Complete`].
    #[must_use]
    pub fn is_complete(&self) -> bool {
        matches!(self, JournalStatus::Complete { .. })
    }
}

pub(crate) fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                write!(out, "\\u{:04x}", c as u32).ok();
            }
            c => out.push(c),
        }
    }
    out
}

fn is_complete_event(line: &str) -> bool {
    extract_field(line, "event").as_deref() == Some("complete")
}

/// Extract a top-level string field from a flat JSON object line (handles escapes).
pub(crate) fn extract_field(line: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\"");
    let mut rest = line.split_once(&key)?.1.trim_start();
    rest = rest.strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let n = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(n)?);
                }
                _ => return None,
            },
            '"' => return Some(out),
            c => out.push(c),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(suffix: &str) -> PathBuf {
        let name = format!("tuiscotti-runner-journal-{suffix}-{}", std::process::id());
        std::env::temp_dir().join(name)
    }

    #[test]
    fn caps_fail_explicitly() {
        let dir = tmp("caps");
        fs::remove_dir_all(&dir).ok();
        // Event cap: two appends fit, the third refuses.
        let mut j =
            Journal::open_with_limits(&dir.join("e.jsonl"), 2, u64::MAX).expect("open succeeds");
        j.append("a", "1").expect("first appends");
        j.append("b", "2").expect("second appends");
        let err = j.append("c", "3").expect_err("third exceeds the event cap");
        assert_eq!(err.kind(), std::io::ErrorKind::QuotaExceeded);
        // Byte cap: one 200-byte detail blows a 100-byte journal.
        let mut j =
            Journal::open_with_limits(&dir.join("b.jsonl"), u64::MAX, 100).expect("open succeeds");
        let err = j.append("a", &"x".repeat(200)).expect_err("byte cap trips");
        assert_eq!(err.kind(), std::io::ErrorKind::QuotaExceeded);
        // Line cap: one oversize event refuses under default bounds.
        let mut j = Journal::open(&dir.join("l.jsonl")).expect("open succeeds");
        let err = j
            .append("big", &"x".repeat(MAX_EVENT_LINE_BYTES))
            .expect_err("line cap trips");
        assert!(err.to_string().contains("exceeds"), "{err}");
        // Zero bounds are rejected at open, never silently empty.
        assert!(Journal::open_with_limits(&dir.join("z.jsonl"), 0, 100).is_err());
        assert!(Journal::open_with_limits(&dir.join("z.jsonl"), 100, 0).is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn status_reads_large_journal_from_the_tail() {
        let dir = tmp("tail");
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("mkdir succeeds");
        let mut j = Journal::open(&dir.join("journal.jsonl")).expect("open succeeds");
        // ~200KB of events: past the window, so the verdict must come from
        // the tail path (partial-line skip included).
        for i in 0..2000 {
            j.append("tick", &format!("{i:06}"))
                .expect("append succeeds");
        }
        j.complete("pass").expect("complete succeeds");
        drop(j);
        let len = fs::metadata(dir.join("journal.jsonl")).expect("stat").len();
        assert!(len > STATUS_TAIL_BYTES, "fixture must exceed the window");
        let st = Journal::status(&dir);
        assert!(matches!(st, JournalStatus::Complete { status } if status == "pass"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn status_names_a_beyond_window_tail() {
        let dir = tmp("beyond");
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("mkdir succeeds");
        // Hand-planted 200KB line: unproducible via append (line cap).
        let line = format!(
            "{{\"seq\":0,\"event\":\"x\",\"detail\":\"{}\"}}\n",
            "y".repeat(200_000)
        );
        fs::write(dir.join("journal.jsonl"), &line).expect("plant succeeds");
        fs::write(dir.join(COMPLETE_MARKER), "pass\n").expect("marker succeeds");
        let JournalStatus::Incomplete { reason } = Journal::status(&dir) else {
            panic!("expected Incomplete");
        };
        assert!(reason.contains("status window"), "{reason}");
        fs::remove_dir_all(&dir).ok();
    }
}
