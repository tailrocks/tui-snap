use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use super::*;


/// Append-only JSONL event journal (N07).
///
/// Every [`Journal::append`] flushes, so a killed or timed-out attempt leaves
/// its last flushed events inspectable. Completion requires an explicit
/// [`Journal::complete`]; a journal without the completion marker is
/// [`JournalStatus::Incomplete`], never a pass.
#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
    file: File,
    seq: u64,
}


/// Completion marker filename written beside `journal.jsonl`.
pub const COMPLETE_MARKER: &str = "COMPLETE";


impl Journal {
    /// Open (or resume) the journal at `path`, creating parent directories.
    /// The sequence counter resumes after the existing line count.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let seq = fs::File::open(path)
            .map(|f| BufReader::new(f).lines().count() as u64)
            .unwrap_or(0);
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            seq,
        })
    }

    /// Journal path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one event and flush. Minimal std-only JSON escaping applies.
    pub fn append(&mut self, event: &str, detail: &str) -> std::io::Result<()> {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        writeln!(
            self.file,
            "{{\"seq\":{},\"unix_ms\":{ms},\"event\":\"{}\",\"detail\":\"{}\"}}",
            self.seq,
            json_escape(event),
            json_escape(detail),
        )?;
        self.file.flush()?;
        self.seq += 1;
        Ok(())
    }

    /// Mark the attempt complete with a terminal `status`, then write the
    /// `COMPLETE` marker. Fail-closed: anything killed before this stays incomplete.
    pub fn complete(&mut self, status: &str) -> std::io::Result<()> {
        self.append("complete", status)?;
        if let Some(parent) = self.path.parent() {
            fs::write(parent.join(COMPLETE_MARKER), format!("{status}\n"))?;
        }
        Ok(())
    }

    /// Read the completion status of a journal directory: complete only when the
    /// `COMPLETE` marker exists **and** the journal's last event is `complete`.
    /// Missing journal, missing marker, or any other tail → incomplete.
    pub fn status(dir: &Path) -> JournalStatus {
        let marker = dir.join(COMPLETE_MARKER);
        if !marker.is_file() {
            return JournalStatus::Incomplete {
                reason: format!("missing {} marker", dir.join(COMPLETE_MARKER).display()),
            };
        }
        let text = match fs::read_to_string(dir.join("journal.jsonl")) {
            Ok(t) => t,
            Err(e) => {
                return JournalStatus::Incomplete {
                    reason: format!("cannot read journal.jsonl: {e}"),
                };
            }
        };
        match text.lines().rfind(|l| !l.trim().is_empty()) {
            Some(last) if is_complete_event(last) => JournalStatus::Complete {
                status: extract_field(last, "detail").unwrap_or_default(),
            },
            _ => JournalStatus::Incomplete {
                reason: "journal tail is not a complete event".to_string(),
            },
        }
    }
}


/// Journal completion state (N07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalStatus {
    /// Explicitly completed with this terminal status string.
    Complete { status: String },
    /// Killed, timed out, or never finished: not a pass.
    Incomplete { reason: String },
}


impl JournalStatus {
    /// True only for [`JournalStatus::Complete`].
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
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
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
