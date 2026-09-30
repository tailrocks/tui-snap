//! Bounded incremental log tail (F12): follow a growing log file without
//! ever re-reading it whole.
//!
//! [`LogTail`] opens the file once and reads only NEW bytes per
//! [`LogTail::poll`], so following a session log costs O(new bytes) per
//! poll instead of O(file size). Both the per-poll read and the lifetime
//! total are capped: past the total cap the tail skips to end-of-file,
//! reports [`LogTail::truncated`], and keeps following (returning nothing
//! further). Memory stays bounded no matter how large the log grows.
//!
//! The on-disk session log itself is intentionally unbounded: the session
//! child holds the file descriptor directly, so truncating or rotating
//! under it would lose bytes. Boundedness lives in the READERS (this
//! type), honestly reported via the truncated flag.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::OpError;

/// Default per-poll read cap (64 KiB): one poll never returns more.
pub const DEFAULT_MAX_POLL_BYTES: usize = 64 * 1024;
/// Default lifetime total cap (16 MiB): past this the tail skips to EOF
/// and reports truncated.
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;

/// Incremental bounded reader over a growing log file.
#[derive(Debug)]
pub struct LogTail {
    path: PathBuf,
    file: File,
    offset: u64,
    total: u64,
    truncated: bool,
    max_poll: usize,
    max_total: u64,
}

impl LogTail {
    /// Open `path` for incremental tailing from the start with default
    /// caps ([`DEFAULT_MAX_POLL_BYTES`]/[`DEFAULT_MAX_TOTAL_BYTES`]).
    ///
    /// # Errors
    ///
    /// Returns [`OpError`] when the file cannot be opened for reading.
    pub fn open(path: &Path) -> Result<Self, OpError> {
        Self::open_with_caps(path, DEFAULT_MAX_POLL_BYTES, DEFAULT_MAX_TOTAL_BYTES)
    }

    /// [`LogTail::open`] with explicit caps: at most `max_poll` bytes per
    /// [`LogTail::poll`], at most `max_total` bytes over the tail's life.
    ///
    /// # Errors
    ///
    /// Returns [`OpError`] for zero caps or when the file cannot be opened.
    pub fn open_with_caps(path: &Path, max_poll: usize, max_total: u64) -> Result<Self, OpError> {
        if max_poll == 0 || max_total == 0 {
            return Err(OpError::new(
                "invalid-input",
                "log tail caps must be nonzero",
            ));
        }
        let file = File::open(path)
            .map_err(|e| OpError::new("io", format!("open {}: {e}", path.display())))?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            offset: 0,
            total: 0,
            truncated: false,
            max_poll,
            max_total,
        })
    }

    /// Read up to the per-poll cap of NEW bytes (since the previous poll).
    /// Returns an empty vec when nothing new arrived, or when the lifetime
    /// cap already tripped (see [`LogTail::truncated`]).
    ///
    /// A shrunk file (replaced under the open handle) reopens from the
    /// start and reports truncated: bytes were skipped, never silently.
    ///
    /// # Errors
    ///
    /// Returns [`OpError`] when the file cannot be read or re-opened.
    pub fn poll(&mut self) -> Result<Vec<u8>, OpError> {
        let len = self
            .file
            .metadata()
            .map_err(|e| OpError::new("io", format!("stat {}: {e}", self.path.display())))?
            .len();
        if len < self.offset {
            // Replaced under us: reopen and start over, honestly flagged.
            self.file = File::open(&self.path)
                .map_err(|e| OpError::new("io", format!("reopen {}: {e}", self.path.display())))?;
            self.offset = 0;
            self.truncated = true;
            return self.poll_fresh();
        }
        self.poll_fresh()
    }

    /// True once bytes were skipped: the lifetime cap tripped or the file
    /// was replaced mid-tail. Sticky: once set, it never clears.
    #[must_use]
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Bytes returned so far.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.total
    }

    fn poll_fresh(&mut self) -> Result<Vec<u8>, OpError> {
        if self.total >= self.max_total {
            // Cap already tripped: track EOF, return nothing further.
            self.truncated = true;
            self.offset = self
                .file
                .seek(SeekFrom::End(0))
                .map_err(|e| OpError::new("io", format!("seek {}: {e}", self.path.display())))?;
            return Ok(Vec::new());
        }
        self.file
            .seek(SeekFrom::Start(self.offset))
            .map_err(|e| OpError::new("io", format!("seek {}: {e}", self.path.display())))?;
        let mut buf = vec![0u8; self.max_poll];
        let mut read = 0;
        while read < self.max_poll {
            match self.file.read(&mut buf[read..]) {
                Ok(0) => break,
                Ok(n) => read += n,
                Err(e) => {
                    return Err(OpError::new(
                        "io",
                        format!("read {}: {e}", self.path.display()),
                    ));
                }
            }
        }
        buf.truncate(read);
        self.offset += read as u64;
        // Lifetime cap: keep only what fits, skip the rest to EOF.
        // (`read` already bounds `room` to `usize`, so the saturating
        // fallback below never fires; it only satisfies 32-bit targets.)
        let room = self.max_total.saturating_sub(self.total);
        if read as u64 > room {
            buf.truncate(usize::try_from(room).unwrap_or(usize::MAX));
            self.total = self.max_total;
            self.truncated = true;
            self.offset = self
                .file
                .seek(SeekFrom::End(0))
                .map_err(|e| OpError::new("io", format!("seek {}: {e}", self.path.display())))?;
        } else {
            self.total += read as u64;
        }
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tail_file(dir: &std::path::Path, name: &str, seed: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, seed).expect("seed log");
        path
    }

    #[test]
    fn poll_returns_only_new_bytes() {
        let dir = std::env::temp_dir().join(format!("tuiscotti-logtail-a-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = tail_file(&dir, "a.log", b"one\n");
        let mut tail = LogTail::open(&path).expect("open");
        assert_eq!(tail.poll().expect("poll"), b"one\n");
        assert!(tail.poll().expect("poll").is_empty());
        assert!(!tail.truncated());
        std::fs::write(&path, b"one\ntwo\n").expect("append");
        assert_eq!(tail.poll().expect("poll"), b"two\n");
        assert_eq!(tail.total(), 8);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn per_poll_cap_chunks_a_large_write() {
        let dir = std::env::temp_dir().join(format!("tuiscotti-logtail-b-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = tail_file(&dir, "b.log", &[b'x'; 300]);
        let mut tail = LogTail::open_with_caps(&path, 100, 1_000_000).expect("open");
        assert_eq!(tail.poll().expect("poll").len(), 100);
        assert_eq!(tail.poll().expect("poll").len(), 100);
        assert_eq!(tail.poll().expect("poll").len(), 100);
        assert!(tail.poll().expect("poll").is_empty());
        assert!(!tail.truncated());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lifetime_cap_skips_and_flags() {
        let dir = std::env::temp_dir().join(format!("tuiscotti-logtail-c-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = tail_file(&dir, "c.log", &[b'y'; 500]);
        let mut tail = LogTail::open_with_caps(&path, 10_000, 200).expect("open");
        let first = tail.poll().expect("poll");
        assert_eq!(first.len(), 200);
        assert!(tail.truncated());
        assert_eq!(tail.total(), 200);
        // Keeps following (tracks EOF) but returns nothing further.
        std::fs::write(
            &path,
            [b'y'; 500]
                .into_iter()
                .chain([b'z'; 10])
                .collect::<Vec<_>>(),
        )
        .expect("append");
        assert!(tail.poll().expect("poll").is_empty());
        assert!(tail.truncated());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn replaced_file_reopens_and_flags() {
        let dir = std::env::temp_dir().join(format!("tuiscotti-logtail-d-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = tail_file(&dir, "d.log", b"v1-content\n");
        let mut tail = LogTail::open(&path).expect("open");
        assert_eq!(tail.poll().expect("poll"), b"v1-content\n");
        std::fs::write(&path, b"v2\n").expect("replace with shorter");
        assert_eq!(tail.poll().expect("poll"), b"v2\n");
        assert!(tail.truncated());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn zero_caps_rejected() {
        assert!(LogTail::open_with_caps(Path::new("x"), 0, 1).is_err());
        assert!(LogTail::open_with_caps(Path::new("x"), 1, 0).is_err());
    }
}
