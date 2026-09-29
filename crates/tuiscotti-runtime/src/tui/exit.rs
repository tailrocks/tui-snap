//! Child exit status, exit assertions, and liveness probes.

use tuiscotti_core::screen::Observation;

use super::error::TuiError;

/// Termination status of the session's direct child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitStatus {
    pub(crate) code: u32,
    pub(crate) signal: Option<String>,
}

impl ExitStatus {
    /// True for a clean code-0 exit with no signal.
    #[must_use]
    pub fn success(&self) -> bool {
        self.signal.is_none() && self.code == 0
    }

    /// Raw exit code.
    #[must_use]
    pub fn code(&self) -> u32 {
        self.code
    }

    /// Killing signal name, if signalled.
    #[must_use]
    pub fn signal(&self) -> Option<&str> {
        self.signal.as_deref()
    }
}

impl std::fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.signal {
            Some(sig) => write!(f, "terminated by {sig}"),
            None => write!(f, "exited with code {}", self.code),
        }
    }
}

impl From<portable_pty::ExitStatus> for ExitStatus {
    fn from(s: portable_pty::ExitStatus) -> Self {
        Self {
            code: s.exit_code(),
            signal: s.signal().map(str::to_string),
        }
    }
}

/// A reaped exit plus the final evidence observation.
#[derive(Debug, Clone)]
pub struct ExitWait {
    /// Reaped child status.
    pub status: ExitStatus,
    /// Final evidence observation.
    pub observation: Observation,
}

impl ExitWait {
    /// Assert successful termination, yielding the final observation.
    ///
    /// # Errors
    ///
    /// Returns `TuiError::Assertion` unless the child exited cleanly.
    pub fn success(self) -> Result<Observation, TuiError> {
        if self.status.success() {
            Ok(self.observation)
        } else {
            Err(TuiError::Assertion(format!(
                "expected successful exit, got {} (revision {})",
                self.status, self.observation.revision
            )))
        }
    }

    /// Assert an exact exit code, yielding the final observation.
    ///
    /// # Errors
    ///
    /// Returns `TuiError::Assertion` unless the code matches exactly.
    pub fn code(self, expected: u32) -> Result<Observation, TuiError> {
        if self.status.signal().is_none() && self.status.code() == expected {
            Ok(self.observation)
        } else {
            Err(TuiError::Assertion(format!(
                "expected exit code {expected}, got {} (revision {})",
                self.status, self.observation.revision
            )))
        }
    }
}

/// True when a process with `pid` exists (Unix: `kill -0` exit status).
/// EPERM targets (a live process owned by another user) read as absent;
/// callers only probe owned children, where EPERM cannot occur.
/// Used to assert teardown reaped the child.
#[cfg(unix)]
#[must_use]
pub fn process_exists(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // No libc: `kill -0` (the workspace forbids `unsafe`).
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .is_ok_and(|s| s.success())
}

/// Non-Unix stub.
#[cfg(not(unix))]
#[must_use]
pub fn process_exists(_pid: u32) -> bool {
    true
}
