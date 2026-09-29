//! Advertised terminal/protocol behavior ([`TerminalProfile`]) for a session.

use super::error::TuiError;

/// Mouse protocols the application may use. The backend tracks all three.
#[derive(Debug, Clone)]
pub struct MouseProfile {
    /// Application may use SGR mouse (1006). Backend tracks it.
    pub sgr: bool,
    /// Application may use UTF-8 mouse (1005). Backend tracks it.
    pub utf8: bool,
    /// Application may use legacy X10 mouse (1000/1002/1003). Tracked.
    pub legacy: bool,
}

/// Application modes the backend tracks (never emulates).
#[derive(Debug, Clone)]
pub struct TrackedModes {
    /// Application may use bracketed paste (2004). Tracked.
    pub bracketed_paste: bool,
    /// Application may use focus tracking (1004). Tracked.
    pub focus_tracking: bool,
    /// Application may use the alternate screen (1049). Tracked.
    pub alt_screen: bool,
}

/// Advertised terminal/protocol behavior for a session.
///
/// `spawn()` rejects any profile claiming a capability the backend cannot
/// implement — capabilities are never silently normalized away.
#[derive(Debug, Clone)]
pub struct TerminalProfile {
    /// `TERM` value exported to the child (child-only).
    pub term: String,
    /// Mouse protocols the application may use.
    pub mouse: MouseProfile,
    /// Parse kitty progressive-enhancement flags (enables them in the
    /// emulator config so applications can negotiate them).
    pub kitty_keyboard: bool,
    /// Application modes the backend tracks.
    pub modes: TrackedModes,
    /// Synchronized output (DEC 2026). **Backend lacks it**: `spawn()`
    /// fails when this is true, and `wait_frame` is unsupported.
    pub synchronized_output: bool,
    /// Per-cell blink (SGR 5/6). **Backend drops it**: `spawn()` fails
    /// when this is true rather than passing on a lossy grid.
    pub cell_blink: bool,
}

impl Default for TerminalProfile {
    fn default() -> Self {
        Self {
            term: "xterm-256color".to_string(),
            mouse: MouseProfile {
                sgr: true,
                utf8: true,
                legacy: true,
            },
            kitty_keyboard: true,
            modes: TrackedModes {
                bracketed_paste: true,
                focus_tracking: true,
                alt_screen: true,
            },
            synchronized_output: false,
            cell_blink: false,
        }
    }
}

impl TerminalProfile {
    /// Reject profiles advertising what the backend lacks.
    ///
    /// # Errors
    ///
    /// Returns [`TuiError::Unsupported`] when the profile claims synchronized
    /// output or per-cell blink.
    pub(crate) fn check(&self) -> Result<(), TuiError> {
        if self.synchronized_output {
            return Err(TuiError::Unsupported(
                "profile advertises synchronized-output (DEC 2026): backend cannot track it",
            ));
        }
        if self.cell_blink {
            return Err(TuiError::Unsupported(
                "profile advertises per-cell blink: backend drops SGR 5/6",
            ));
        }
        Ok(())
    }
}
