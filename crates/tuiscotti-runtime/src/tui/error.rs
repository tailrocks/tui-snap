//! Session errors, wait failures, and cooperative cancellation.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tuiscotti_core::screen::Observation;

/// Session failure.
#[derive(Debug, Clone)]
pub enum TuiError {
    /// Spawn failed (unresolved binary, PTY open, validation, ...).
    Spawn(String),
    /// Invalid argument (bad size, coordinates, empty input, ...).
    InvalidInput(String),
    /// Unparseable chord.
    Chord(String),
    /// Capability the backend cannot provide. Fails closed, never emulated.
    Unsupported(&'static str),
    /// Input refused: the application has not enabled the mode (mouse
    /// reporting, focus tracking, ...) that would make it meaningful.
    ModeNotEnabled(&'static str),
    /// Paste rejected: content contains the bracketed-paste delimiters.
    PasteRejected(String),
    /// PTY I/O failure.
    Io(String),
    /// Deadlines: wait expired / worker unresponsive.
    Timeout(String),
    /// Input or observation refused: the child already exited.
    ChildExited(String),
    /// Session is closed (or its worker is gone).
    Closed(String),
    /// Teardown itself failed (kill/reap/join errors from finish/close).
    Teardown(String),
    /// Signal delivery failed.
    Signal(String),
    /// `ExitWait::success` / `code` assertion failed.
    Assertion(String),
}

impl std::fmt::Display for TuiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(m) => write!(f, "spawn failed: {m}"),
            Self::InvalidInput(m) => write!(f, "invalid input: {m}"),
            Self::Chord(m) => write!(f, "bad chord: {m}"),
            Self::Unsupported(m) => write!(f, "unsupported: {m}"),
            Self::ModeNotEnabled(m) => write!(f, "mode not enabled: {m}"),
            Self::PasteRejected(m) => write!(f, "paste rejected: {m}"),
            Self::Io(m) => write!(f, "pty io: {m}"),
            Self::Timeout(m) => write!(f, "timeout: {m}"),
            Self::ChildExited(m) => write!(f, "child exited: {m}"),
            Self::Closed(m) => write!(f, "session closed: {m}"),
            Self::Teardown(m) => write!(f, "teardown failed: {m}"),
            Self::Signal(m) => write!(f, "signal failed: {m}"),
            Self::Assertion(m) => write!(f, "exit assertion failed: {m}"),
        }
    }
}

impl std::error::Error for TuiError {}

/// Wait failure. Every variant carries the latest evidence snapshot; a wait
/// never reports success without its condition holding. Evidence is boxed:
/// snapshots are large and only travel on failure paths.
#[derive(Debug, Clone)]
pub enum WaitError {
    Timeout {
        waited: Duration,
        evidence: Box<Observation>,
    },
    Cancelled {
        evidence: Box<Observation>,
    },
    /// wait_frame only: the backend cannot track synchronized frames.
    Unsupported {
        capability: &'static str,
        evidence: Box<Observation>,
    },
    /// The session closed before the condition held.
    Closed {
        evidence: Option<Box<Observation>>,
    },
}

impl WaitError {
    #[must_use]
    pub fn evidence(&self) -> Option<&Observation> {
        match self {
            Self::Timeout { evidence, .. }
            | Self::Cancelled { evidence }
            | Self::Unsupported { evidence, .. } => Some(evidence.as_ref()),
            Self::Closed { evidence } => evidence.as_deref(),
        }
    }
}

impl std::fmt::Display for WaitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout { waited, evidence } => write!(
                f,
                "wait timed out after {waited:?} (evidence at revision {})",
                evidence.revision
            ),
            Self::Cancelled { evidence } => write!(
                f,
                "wait cancelled (evidence at revision {})",
                evidence.revision
            ),
            Self::Unsupported {
                capability,
                evidence,
            } => write!(
                f,
                "wait unsupported ({capability}; evidence at revision {})",
                evidence.revision
            ),
            Self::Closed { .. } => write!(f, "wait failed: session closed"),
        }
    }
}

impl std::error::Error for WaitError {}

/// Cooperative cancellation token shared across threads. Every blocking wait
/// takes one; cancelling unblocks the wait with [`WaitError::Cancelled`].
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}
