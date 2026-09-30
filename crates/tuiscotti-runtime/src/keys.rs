//! Typed key chords with [`FromStr`] parsing (G6).
//!
//! [`KeyChord`] pairs a [`Key`] with its
//! [`KeyMods`]; chord strings (`"Ctrl+P"`, `"Alt+Enter"`)
//! parse through [`FromStr`], and bare key names parse as
//! [`Key`] directly. Raw bytes and paste stay explicit on
//! [`Session`](crate::tui::Session) (`send_bytes`, `paste`).

use std::str::FromStr;

use crate::tui::{Key, KeyMods, TuiError, parse_chord};

/// A key plus its modifiers, e.g. `Ctrl+P`.
///
/// Parses via [`FromStr`]; see [`parse_chord`] for the
/// accepted grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChord {
    /// The key independent of modifiers.
    pub key: Key,
    /// Modifier set held with the key.
    pub mods: KeyMods,
}

impl KeyChord {
    /// Pair a key with modifiers.
    #[must_use]
    pub fn new(key: Key, mods: KeyMods) -> Self {
        Self { key, mods }
    }
}

impl From<Key> for KeyChord {
    /// A bare key with no modifiers.
    fn from(key: Key) -> Self {
        Self {
            key,
            mods: KeyMods::NONE,
        }
    }
}

impl FromStr for KeyChord {
    type Err = TuiError;

    /// Parse `"Ctrl+P"`, `"Alt+Enter"`, `"F5"`, `"a"`, ... (see [`parse_chord`]).
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (key, mods) = parse_chord(text)?;
        Ok(Self { key, mods })
    }
}

impl std::fmt::Display for KeyChord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.mods.ctrl {
            write!(f, "Ctrl+")?;
        }
        if self.mods.alt {
            write!(f, "Alt+")?;
        }
        if self.mods.shift {
            write!(f, "Shift+")?;
        }
        if self.mods.ext.sup {
            write!(f, "Super+")?;
        }
        write!(f, "{}", self.key)
    }
}

impl FromStr for Key {
    type Err = TuiError;

    /// Parse a bare key name (`"Enter"`, `"F5"`, `"a"`, ...). Modifiers are
    /// rejected: parse chords via [`KeyChord`] instead.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (key, mods) = parse_chord(text)?;
        if mods.is_empty() {
            Ok(key)
        } else {
            Err(TuiError::Chord(format!(
                "modifiers in bare key {text:?}; parse a KeyChord instead"
            )))
        }
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Char(' ') => write!(f, "Space"),
            Self::Char(c) => write!(f, "{c}"),
            Self::Enter => write!(f, "Enter"),
            Self::Tab => write!(f, "Tab"),
            Self::Backspace => write!(f, "Backspace"),
            Self::Escape => write!(f, "Escape"),
            Self::Up => write!(f, "Up"),
            Self::Down => write!(f, "Down"),
            Self::Left => write!(f, "Left"),
            Self::Right => write!(f, "Right"),
            Self::Home => write!(f, "Home"),
            Self::End => write!(f, "End"),
            Self::PageUp => write!(f, "PageUp"),
            Self::PageDown => write!(f, "PageDown"),
            Self::Insert => write!(f, "Insert"),
            Self::Delete => write!(f, "Delete"),
            Self::F(n) => write!(f, "F{n}"),
        }
    }
}
