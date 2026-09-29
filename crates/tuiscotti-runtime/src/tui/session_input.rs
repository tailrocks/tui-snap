//! [`Session`](super::session::Session) input: text, keys, mouse, focus,
//! resize, signals (R11-core).

use std::sync::mpsc;

use super::error::TuiError;
use super::exit::process_exists;
use super::input_types::{
    Key, KeyEventKind, KeyMods, MouseButton, MouseMods, Signal, Wheel, parse_chord,
};
use super::limits::{MAX_COLS, MAX_ROWS, MIN_COLS, MIN_ROWS};
use super::session::Session;
use super::session_teardown::recv_reply;
use super::worker::{Input, MouseAction, Op};

impl Session {
    /// Send literal text (UTF-8 bytes, no chord interpretation).
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if input is refused or the session is closed.
    pub fn send_text(&self, text: &str) -> Result<(), TuiError> {
        self.send_input(Input::Bytes(text.as_bytes().to_vec()))
    }

    /// Send raw bytes verbatim.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if input is refused or the session is closed.
    pub fn send_bytes(&self, bytes: &[u8]) -> Result<(), TuiError> {
        self.send_input(Input::Bytes(bytes.to_vec()))
    }

    /// Negotiated paste: wrapped in `ESC[200~...ESC[201~` when the
    /// application enabled bracketed paste (2004), sent plain otherwise.
    /// Content containing either delimiter is rejected outright.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if the content holds delimiters or input is refused.
    pub fn paste(&self, text: &str) -> Result<(), TuiError> {
        self.send_input(Input::Paste(text.to_string()))
    }

    /// Parse `chord` ([`parse_chord`]) and send it as a complete press.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` for a bad chord or refused input.
    pub fn press(&self, chord: &str) -> Result<(), TuiError> {
        let (key, mods) = parse_chord(chord)?;
        self.press_key(key, mods)
    }

    /// Send a complete key press.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if input is refused or the session is closed.
    pub fn press_key(&self, key: Key, mods: KeyMods) -> Result<(), TuiError> {
        self.key_event(key, mods, KeyEventKind::Press)
    }

    /// Send a key-down event (no automatic release).
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if input is refused or the session is closed.
    pub fn key_down(&self, key: Key, mods: KeyMods) -> Result<(), TuiError> {
        self.key_event(key, mods, KeyEventKind::Down)
    }

    /// Send a key-repeat tick.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if input is refused or the session is closed.
    pub fn key_repeat(&self, key: Key, mods: KeyMods) -> Result<(), TuiError> {
        self.key_event(key, mods, KeyEventKind::Repeat)
    }

    /// Send a key-release event. Releases emit bytes only with the kitty
    /// keyboard protocol active; otherwise this is a successful no-op.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if input is refused or the session is closed.
    pub fn key_up(&self, key: Key, mods: KeyMods) -> Result<(), TuiError> {
        self.key_event(key, mods, KeyEventKind::Up)
    }

    /// Send a key event of any kind.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` for a bad key or refused input.
    pub fn key_event(&self, key: Key, mods: KeyMods, kind: KeyEventKind) -> Result<(), TuiError> {
        if matches!(key, Key::F(n) if !(1..=12).contains(&n)) {
            return Err(TuiError::InvalidInput(format!(
                "function key out of range: {key:?}"
            )));
        }
        self.send_input(Input::Key { key, mods, kind })
    }

    /// Click: button down immediately followed by button up at `(x, y)`.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if mouse reporting is off or input is refused.
    pub fn click(
        &self,
        button: MouseButton,
        x: u16,
        y: u16,
        mods: MouseMods,
    ) -> Result<(), TuiError> {
        self.mouse_down(button, x, y, mods)?;
        self.mouse_up(button, x, y, mods)
    }

    /// Button press at `(x, y)`.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if mouse reporting is off or input is refused.
    pub fn mouse_down(
        &self,
        button: MouseButton,
        x: u16,
        y: u16,
        mods: MouseMods,
    ) -> Result<(), TuiError> {
        self.send_input(Input::Mouse {
            action: MouseAction::Press(button),
            x,
            y,
            mods,
        })
    }

    /// Button release at `(x, y)`.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if mouse reporting is off or input is refused.
    pub fn mouse_up(
        &self,
        button: MouseButton,
        x: u16,
        y: u16,
        mods: MouseMods,
    ) -> Result<(), TuiError> {
        let _ = button;
        self.send_input(Input::Mouse {
            action: MouseAction::Release,
            x,
            y,
            mods,
        })
    }

    /// Hover (motion with no button held) to `(x, y)`.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if motion reporting is off or input is refused.
    pub fn mouse_move(&self, x: u16, y: u16, mods: MouseMods) -> Result<(), TuiError> {
        self.send_input(Input::Mouse {
            action: MouseAction::Move { held: None },
            x,
            y,
            mods,
        })
    }

    /// Drag step: motion with `button` held, to `(x, y)`.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if drag reporting is off or input is refused.
    pub fn mouse_drag(
        &self,
        button: MouseButton,
        x: u16,
        y: u16,
        mods: MouseMods,
    ) -> Result<(), TuiError> {
        self.send_input(Input::Mouse {
            action: MouseAction::Move { held: Some(button) },
            x,
            y,
            mods,
        })
    }

    /// Wheel event at `(x, y)`.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if mouse reporting is off or input is refused.
    pub fn mouse_wheel(
        &self,
        wheel: Wheel,
        x: u16,
        y: u16,
        mods: MouseMods,
    ) -> Result<(), TuiError> {
        self.send_input(Input::Mouse {
            action: MouseAction::Wheel(wheel),
            x,
            y,
            mods,
        })
    }

    /// Focus-in (`CSI I`). Refused unless the application enabled 1004.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` unless the application enabled focus tracking.
    pub fn focus_in(&self) -> Result<(), TuiError> {
        self.send_input(Input::Focus(true))
    }

    /// Focus-out (`CSI O`). Refused unless the application enabled 1004.
    ///
    /// # Errors
    ///
    /// Returns `TuiError` unless the application enabled focus tracking.
    pub fn focus_out(&self) -> Result<(), TuiError> {
        self.send_input(Input::Focus(false))
    }

    /// Resize the PTY and the emulator together (atomic from the test's
    /// view: one revision, reason `Resize`).
    ///
    /// # Errors
    ///
    /// Returns `TuiError` for out-of-range sizes or refused input.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), TuiError> {
        if !(MIN_COLS..=MAX_COLS).contains(&cols) {
            return Err(TuiError::InvalidInput(format!(
                "cols {cols} outside backend range {MIN_COLS}..={MAX_COLS}"
            )));
        }
        if !(MIN_ROWS..=MAX_ROWS).contains(&rows) {
            return Err(TuiError::InvalidInput(format!(
                "rows {rows} outside backend range {MIN_ROWS}..={MAX_ROWS}"
            )));
        }
        let (tx, rx) = mpsc::channel();
        self.send(Op::Resize {
            cols,
            rows,
            reply: tx,
        })?;
        recv_reply(&rx, "resize")
    }

    /// Deliver a signal to the direct child (Unix only).
    ///
    /// # Errors
    ///
    /// Returns `TuiError` if the child exited or the signal failed.
    #[cfg(unix)]
    pub fn signal(&self, signal: Signal) -> Result<(), TuiError> {
        let pid = self
            .pid
            .ok_or_else(|| TuiError::Signal("child PID unknown on this platform".to_string()))?;
        if self.shared.exit().is_some() {
            return Err(TuiError::ChildExited("child already exited".to_string()));
        }
        // No libc: `kill(1)` exit status follows the `pid_alive` convention
        // (the workspace forbids `unsafe`). A failed signal against a dead
        // pid still maps to `ChildExited`, matching the old ESRCH branch.
        let delivered = std::process::Command::new("kill")
            .arg(format!("-{}", signal.number()))
            .arg(pid.to_string())
            .status()
            .is_ok_and(|s| s.success());
        if delivered {
            return Ok(());
        }
        if !process_exists(pid) {
            return Err(TuiError::ChildExited(format!(
                "child {pid} no longer exists"
            )));
        }
        Err(TuiError::Signal(format!("kill({pid}) failed")))
    }

    /// Non-Unix stub: signals are unsupported.
    #[cfg(not(unix))]
    pub fn signal(&self, _signal: Signal) -> Result<(), TuiError> {
        Err(TuiError::Unsupported("signals require a Unix platform"))
    }
}
