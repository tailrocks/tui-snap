//! Typed input vocabulary: keys, chords, mouse, focus, signals.

use super::error::TuiError;

/// A key independent of modifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// A Unicode character (letters, digits, punctuation, space, ...).
    Char(char),
    /// Enter/Return.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Escape.
    Escape,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Insert.
    Insert,
    /// Delete (forward delete).
    Delete,
    /// Function key 1..=12.
    F(u8),
}

/// Extra modifiers beyond ctrl/alt/shift.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyExtMods {
    /// Super/Cmd/Win: needs the kitty keyboard protocol, else `Unsupported`.
    pub sup: bool,
}

/// Modifier set for [`Key`] input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyMods {
    /// Control held.
    pub ctrl: bool,
    /// Alt/Opt held.
    pub alt: bool,
    /// Shift held.
    pub shift: bool,
    /// Extra modifiers.
    pub ext: KeyExtMods,
}

impl KeyMods {
    /// No modifiers.
    pub const NONE: KeyMods = KeyMods {
        ctrl: false,
        alt: false,
        shift: false,
        ext: KeyExtMods { sup: false },
    };
    /// Control only.
    pub const CTRL: KeyMods = KeyMods {
        ctrl: true,
        alt: false,
        shift: false,
        ext: KeyExtMods { sup: false },
    };
    /// Alt only.
    pub const ALT: KeyMods = KeyMods {
        ctrl: false,
        alt: true,
        shift: false,
        ext: KeyExtMods { sup: false },
    };
    /// Shift only.
    pub const SHIFT: KeyMods = KeyMods {
        ctrl: false,
        alt: false,
        shift: true,
        ext: KeyExtMods { sup: false },
    };

    /// True when no modifier is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.ctrl && !self.alt && !self.shift && !self.ext.sup
    }
}

/// Key event kind. Without the kitty keyboard protocol, `Up` (release)
/// emits no bytes — legacy encodings cannot represent releases — while
/// `Press`, `Down`, and `Repeat` all emit the key's legacy bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventKind {
    /// A complete press (down immediately followed by up).
    Press,
    /// Key down (no automatic release).
    Down,
    /// Auto-repeat tick while held.
    Repeat,
    /// Key release.
    Up,
}

/// Parse a typed chord such as `"Ctrl+P"`, `"Alt+Enter"`, `"Shift+F5"`,
/// `"F1"`, or a bare `"a"`.
///
/// Modifiers (case-insensitive, any order): `Ctrl`/`Control`/`Ctl`,
/// `Alt`/`Opt`/`Meta`, `Shift`, `Super`/`Cmd`/`Win`/`Command`. The final
/// segment names the key: `Enter`/`Return`, `Tab`, `Backspace`/`BS`,
/// `Esc`/`Escape`, `Space`, arrows, `Home`/`End`, `PageUp`/`PgUp`,
/// `PageDown`/`PgDn`, `Insert`/`Ins`, `Delete`/`Del`, `F1`..`F12`, or any
/// single character.
///
/// # Errors
///
/// Returns `TuiError::Chord` for empty/unknown modifiers or keys.
pub fn parse_chord(text: &str) -> Result<(Key, KeyMods), TuiError> {
    let bad = |m: String| TuiError::Chord(m);
    let mut parts: Vec<&str> = text.split('+').collect();
    if parts.is_empty() {
        return Err(bad("empty chord".to_string()));
    }
    let name = parts.pop().unwrap_or_default();
    if name.is_empty() {
        return Err(bad(format!("empty key in chord {text:?}")));
    }
    let mut mods = KeyMods::NONE;
    for m in parts {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "ctl" => mods.ctrl = true,
            "alt" | "opt" | "meta" => mods.alt = true,
            "shift" => mods.shift = true,
            "super" | "cmd" | "win" | "windows" | "command" => mods.ext.sup = true,
            "" => return Err(bad(format!("empty modifier in chord {text:?}"))),
            other => return Err(bad(format!("unknown modifier {other:?} in chord {text:?}"))),
        }
    }
    let key = match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Key::Enter,
        "tab" => Key::Tab,
        "backspace" | "bs" => Key::Backspace,
        "esc" | "escape" => Key::Escape,
        "space" => Key::Char(' '),
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        "insert" | "ins" => Key::Insert,
        "delete" | "del" => Key::Delete,
        _ => {
            if let Some(rest) = name.strip_prefix('F').or_else(|| name.strip_prefix('f')) {
                match rest.parse::<u8>() {
                    Ok(n) if (1..=12).contains(&n) => Key::F(n),
                    _ => return Err(bad(format!("bad function key {name:?}"))),
                }
            } else if name.chars().count() == 1 {
                Key::Char(name.chars().next().unwrap_or('?'))
            } else {
                return Err(bad(format!("unknown key {name:?}")));
            }
        }
    };
    Ok((key, mods))
}

/// Mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// Left button.
    Left,
    /// Middle button.
    Middle,
    /// Right button.
    Right,
}

/// Wheel direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wheel {
    /// Wheel up.
    Up,
    /// Wheel down.
    Down,
    /// Wheel left.
    Left,
    /// Wheel right.
    Right,
}

/// Modifier set for mouse input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseMods {
    /// Shift held.
    pub shift: bool,
    /// Alt held.
    pub alt: bool,
    /// Control held.
    pub ctrl: bool,
}

impl MouseMods {
    /// No modifiers.
    pub const NONE: MouseMods = MouseMods {
        shift: false,
        alt: false,
        ctrl: false,
    };
}

/// Process signal for [`Session::signal`](super::session::Session::signal). Unix only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// SIGINT.
    Int,
    /// SIGTERM.
    Term,
    /// SIGKILL.
    Kill,
    /// SIGQUIT.
    Quit,
    /// SIGHUP.
    Hup,
    /// Raw signal number.
    Custom(i32),
}

#[cfg(unix)]
impl Signal {
    pub(crate) fn number(self) -> i32 {
        match self {
            Signal::Int => libc::SIGINT,
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
            Signal::Quit => libc::SIGQUIT,
            Signal::Hup => libc::SIGHUP,
            Signal::Custom(n) => n,
        }
    }
}
