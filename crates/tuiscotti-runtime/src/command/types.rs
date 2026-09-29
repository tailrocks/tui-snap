use super::*;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// How a child process run ended.
///
/// Every outcome is distinct: a timeout kill reports [`Termination::Timeout`]
/// even though the OS-level status is signal death, and an output-limit kill
/// reports [`Termination::OutputLimit`]. Only genuine child signal death —
/// where this module did not kill the child — reports [`Termination::Signal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Termination {
    /// Child exited normally with this status code.
    Exit(i32),
    /// Child died from this signal without being killed by this module.
    ///
    /// Unix only; on other platforms signal death is reported as
    /// [`Termination::Exit`] with the process status code.
    Signal(i32),
    /// The configured [`Command::timeout`] elapsed; the child was killed.
    Timeout,
    /// A stream exceeded [`Command::output_limit`]; the child was killed.
    OutputLimit,
    /// The child could not be spawned (missing binary, bad cwd, ...).
    /// Detail is in [`ProcessOutput::error`].
    SpawnError,
}

impl Termination {
    /// True only for `Exit(0)`.
    #[must_use]
    pub fn success(&self) -> bool {
        matches!(self, Termination::Exit(0))
    }

    /// Exit code when the child exited normally.
    #[must_use]
    pub fn code(&self) -> Option<i32> {
        match self {
            Termination::Exit(c) => Some(*c),
            _ => None,
        }
    }

    /// Signal number when the child died from a signal on its own.
    #[must_use]
    pub fn signal(&self) -> Option<i32> {
        match self {
            Termination::Signal(s) => Some(*s),
            _ => None,
        }
    }
}

/// Why a run never produced child output: resolution or spawn failure.
///
/// Typed context for [`Termination::SpawnError`]: the failure kind plus the
/// detail that caused it (searched locations for resolution, the OS message
/// otherwise). Never a bare string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnError {
    kind: SpawnErrorKind,
    detail: String,
    searched: Vec<PathBuf>,
}

/// Spawn-failure kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnErrorKind {
    /// `cargo_bin` resolution found no binary.
    BinaryNotFound,
    /// The OS refused the spawn (missing binary, bad cwd, ...).
    SpawnFailed,
    /// Reaping a live child failed after spawn.
    WaitFailed,
}

impl SpawnError {
    /// Typed failure kind.
    #[must_use]
    pub fn kind(&self) -> SpawnErrorKind {
        self.kind
    }

    /// Human-readable detail (OS message or searched locations).
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Resolution candidates tried, for [`SpawnErrorKind::BinaryNotFound`].
    #[must_use]
    pub fn searched(&self) -> &[PathBuf] {
        &self.searched
    }

    fn not_found(name: &OsStr, var: &str, searched: Vec<PathBuf>) -> Self {
        Self {
            kind: SpawnErrorKind::BinaryNotFound,
            detail: format!(
                "binary `{}` not found; set {var} or build it first (searched: {})",
                Path::new(name)
                    .file_name()
                    .unwrap_or(name)
                    .to_string_lossy(),
                searched
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            searched,
        }
    }

    pub(crate) fn spawn_failed(detail: String) -> Self {
        Self {
            kind: SpawnErrorKind::SpawnFailed,
            detail,
            searched: Vec::new(),
        }
    }

    pub(crate) fn wait_failed(detail: String) -> Self {
        Self {
            kind: SpawnErrorKind::WaitFailed,
            detail,
            searched: Vec::new(),
        }
    }
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.kind {
            SpawnErrorKind::BinaryNotFound => "binary not found",
            SpawnErrorKind::SpawnFailed => "spawn failed",
            SpawnErrorKind::WaitFailed => "wait failed",
        };
        write!(f, "{kind}: {}", self.detail)
    }
}

impl std::error::Error for SpawnError {}

/// Collected result of one [`Command::run`].
///
/// `stdout`/`stderr` hold raw bytes exactly as read: no UTF-8 validation,
/// no newline translation, no cross-stream interleaving.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    /// Raw bytes read from the child's stdout.
    pub stdout: Vec<u8>,
    /// Raw bytes read from the child's stderr.
    pub stderr: Vec<u8>,
    /// How the run ended.
    pub status: Termination,
    /// True iff produced bytes were not captured: a stream exceeded
    /// [`Command::output_limit`], or [`Command::drain_deadline`] expired while
    /// pipes were still open (typically descendants inheriting them).
    ///
    /// Killing the child on [`Termination::Timeout`] does not by itself set
    /// this: it is set only when bytes the child wrote were dropped.
    pub truncated: bool,
    /// Wall time from spawn (or spawn attempt) until output collection ended.
    pub elapsed: Duration,
    /// Typed spawn-failure detail for [`Termination::SpawnError`].
    pub error: Option<SpawnError>,
}

impl ProcessOutput {
    /// True only for `status == Exit(0)`.
    #[must_use]
    pub fn success(&self) -> bool {
        self.status.success()
    }

    /// Exit code when the child exited normally.
    #[must_use]
    pub fn code(&self) -> Option<i32> {
        self.status.code()
    }

    /// Signal number when the child died from a signal on its own.
    #[must_use]
    pub fn signal(&self) -> Option<i32> {
        self.status.signal()
    }

    /// Fallible UTF-8 view of stdout; the raw bytes stay authoritative.
    /// Use [`Self::stdout_lossy`] only when loss is explicitly acceptable —
    /// never in equality checks.
    pub fn stdout_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.stdout)
    }

    /// Fallible UTF-8 view of stderr; the raw bytes stay authoritative.
    /// Use [`Self::stderr_lossy`] only when loss is explicitly acceptable —
    /// never in equality checks.
    pub fn stderr_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.stderr)
    }

    /// Explicitly lossy UTF-8 view of stdout ([`Self::stdout`] is intact).
    #[must_use]
    pub fn stdout_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Explicitly lossy UTF-8 view of stderr ([`Self::stderr`] is intact).
    #[must_use]
    pub fn stderr_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// Resolution of a `cargo_bin` target dir, for error messages.
fn cargo_bin_candidates(name: &OsStr) -> Vec<PathBuf> {
    let mut out = Vec::new();
    // 1. Current executable's directory layout: tests live in
    //    target/<profile>/deps/, binaries in target/<profile>/.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(deps) = exe.parent() {
            out.push(deps.join(name));
            if deps.file_name().is_some_and(|n| n == "deps") {
                if let Some(profile) = deps.parent() {
                    out.push(profile.join(name));
                }
            }
        }
    }
    // 2. target/<profile>/<name> relative to the process working directory
    //    (covers `cargo test` from the package root).
    for profile in ["debug", "release"] {
        out.push(PathBuf::from("target").join(profile).join(name));
    }
    out
}

/// Resolve the path of a cargo-built binary named `name`.
///
/// Lookup order (runtime only, so remapped paths are honored and no stale
/// build-time absolute path is ever baked in — backlog N03):
/// 1. `CARGO_BIN_EXE_<name>` from the process environment, when set.
/// 2. Next to the current executable, then next to its parent when the
///    executable lives in a `deps/` directory (integration-test layout).
/// 3. `target/debug/<name>` and `target/release/<name>` under the cwd.
///
/// Callers that prefer the test crate's compile-time path can instead write
/// `Command::new(env!("CARGO_BIN_EXE_<name>"))` in the test itself; that
/// `env!` must expand in the test crate, not in this library.
///
/// Returns the first candidate that exists, else a typed error listing every
/// location that was searched.
pub fn cargo_bin_path(name: impl AsRef<OsStr>) -> Result<PathBuf, SpawnError> {
    let name = name.as_ref();
    let file = Path::new(name).file_name().unwrap_or(name);
    let var = format!("CARGO_BIN_EXE_{}", file.to_string_lossy());
    let mut searched = Vec::new();
    if let Ok(p) = std::env::var(&var) {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
        searched.push(p);
    }
    for c in cargo_bin_candidates(file) {
        if c.is_file() {
            return Ok(c);
        }
        searched.push(c);
    }
    Err(SpawnError::not_found(file, &var, searched))
}
