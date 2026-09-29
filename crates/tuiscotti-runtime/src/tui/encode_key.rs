//! Key encoding: kitty `CSI u` when negotiated, legacy xterm otherwise.

use alacritty_terminal::term::TermMode;

use super::error::TuiError;
use super::input_types::{Key, KeyEventKind, KeyMods};

/// xterm modifier parameter: 1 + shift*1 + alt*2 + ctrl*4 (+ super*8 kitty).
fn mods_param(mods: &KeyMods, kitty: bool) -> Result<u8, TuiError> {
    if mods.sup && !kitty {
        return Err(TuiError::Unsupported(
            "super modifier needs the kitty keyboard protocol",
        ));
    }
    Ok(1 + u8::from(mods.shift)
        + 2 * u8::from(mods.alt)
        + 4 * u8::from(mods.ctrl)
        + if kitty { 8 * u8::from(mods.sup) } else { 0 })
}

pub(crate) fn encode_key(
    key: &Key,
    mods: &KeyMods,
    kind: KeyEventKind,
    mode: &TermMode,
) -> Result<Option<Vec<u8>>, TuiError> {
    let kitty = mode.intersects(TermMode::KITTY_KEYBOARD_PROTOCOL);
    if kitty {
        if let Some(bytes) = encode_key_kitty(key, mods, kind)? {
            return Ok(Some(bytes));
        }
        // Kitty cannot express this key (functional table deferred to the
        // shell/paste agent): fall through to legacy encoding.
    }
    if kind == KeyEventKind::Up {
        // Legacy encodings cannot represent releases: successful no-op.
        return Ok(None);
    }
    Ok(Some(encode_key_legacy(key, mods, mode)?))
}

/// Kitty `CSI u` encoding for text keys (`Enter`/`Tab`/`Backspace`/`Escape`
/// by codepoint). Returns `None` for functional keys, which keep legacy
/// encoding until the full kitty functional table lands.
fn encode_key_kitty(
    key: &Key,
    mods: &KeyMods,
    kind: KeyEventKind,
) -> Result<Option<Vec<u8>>, TuiError> {
    let codepoint: u32 = match key {
        Key::Char(c) => (*c).into(),
        Key::Enter => 13,
        Key::Tab => 9,
        Key::Backspace => 127,
        Key::Escape => 27,
        _ => return Ok(None),
    };
    let m = mods_param(mods, true)?;
    let event = match kind {
        KeyEventKind::Press => String::new(),
        KeyEventKind::Down => ":1".to_string(),
        KeyEventKind::Repeat => ":2".to_string(),
        KeyEventKind::Up => ":3".to_string(),
    };
    Ok(Some(format!("\x1b[{codepoint};{m}u{event}").into_bytes()))
}

fn encode_key_legacy(key: &Key, mods: &KeyMods, mode: &TermMode) -> Result<Vec<u8>, TuiError> {
    // Alt-only chords prefix ESC; richer modifier mixes use CSI params or
    // CSI-u, which cannot combine with a bare ESC prefix.
    let alt_only = mods.alt && !mods.ctrl && !mods.shift && !mods.sup;
    if mods.sup {
        return Err(TuiError::Unsupported(
            "super modifier needs the kitty keyboard protocol",
        ));
    }
    let app_cursor = mode.contains(TermMode::APP_CURSOR);

    match key {
        Key::Char(c) => legacy_char_key(*c, mods, alt_only),
        Key::Enter | Key::Tab | Key::Backspace | Key::Escape => {
            legacy_control_key(key, mods, alt_only)
        }
        Key::Up | Key::Down | Key::Right | Key::Left => {
            legacy_arrow_key(key, mods, alt_only, app_cursor)
        }
        Key::Home | Key::End => legacy_home_end_key(key, mods, alt_only),
        Key::Insert | Key::Delete | Key::PageUp | Key::PageDown => {
            legacy_edit_key(key, mods, alt_only)
        }
        Key::F(n) => legacy_function_key(*n, mods, alt_only),
    }
}

/// CSI-u fallback for control keys with modifiers (modifyOtherKeys style).
fn legacy_csi_u(code: u32, mods: &KeyMods) -> Result<Vec<u8>, TuiError> {
    Ok(format!("\x1b[{code};{}u", mods_param(mods, false)?).into_bytes())
}

fn legacy_char_key(c: char, mods: &KeyMods, alt_only: bool) -> Result<Vec<u8>, TuiError> {
    if mods.ctrl && !mods.alt {
        return Ok(vec![ctrl_byte(c)?]);
    }
    if mods.ctrl {
        // Ctrl+Alt and Ctrl+Shift mixes have no legacy form.
        return Err(TuiError::Unsupported(
            "ctrl+alt/shift character chords need the kitty keyboard protocol",
        ));
    }
    if mods.alt && !alt_only {
        return Err(TuiError::Unsupported(
            "alt+shift character chords need the kitty keyboard protocol",
        ));
    }
    // Shift on a character is the caller's case choice; no bytes.
    let mut text = String::new();
    if alt_only {
        text.push('\x1b');
    }
    text.push(c);
    Ok(text.into_bytes())
}

fn legacy_control_key(key: &Key, mods: &KeyMods, alt_only: bool) -> Result<Vec<u8>, TuiError> {
    match key {
        Key::Enter => {
            if mods.is_empty() {
                Ok(vec![b'\r'])
            } else if alt_only {
                Ok(vec![0x1b, b'\r'])
            } else {
                legacy_csi_u(13, mods)
            }
        }
        Key::Tab => {
            if mods.is_empty() {
                Ok(vec![b'\t'])
            } else if *mods == KeyMods::SHIFT {
                Ok(b"\x1b[Z".to_vec())
            } else if alt_only {
                Ok(vec![0x1b, b'\t'])
            } else {
                legacy_csi_u(9, mods)
            }
        }
        Key::Backspace => {
            if mods.is_empty() {
                Ok(vec![0x7f])
            } else if alt_only {
                Ok(vec![0x1b, 0x7f])
            } else {
                legacy_csi_u(127, mods)
            }
        }
        Key::Escape => {
            if mods.is_empty() {
                Ok(vec![0x1b])
            } else {
                legacy_csi_u(27, mods)
            }
        }
        _ => unreachable!("caller gates control keys"),
    }
}

fn legacy_arrow_key(
    key: &Key,
    mods: &KeyMods,
    alt_only: bool,
    app_cursor: bool,
) -> Result<Vec<u8>, TuiError> {
    let letter = match key {
        Key::Up => 'A',
        Key::Down => 'B',
        Key::Right => 'C',
        _ => 'D',
    };
    if mods.is_empty() {
        return Ok(if app_cursor {
            format!("\x1bO{letter}").into_bytes()
        } else {
            format!("\x1b[{letter}").into_bytes()
        });
    }
    if alt_only {
        let base = if app_cursor {
            format!("\x1bO{letter}")
        } else {
            format!("\x1b[{letter}")
        };
        return Ok(format!("\x1b{base}").into_bytes());
    }
    Ok(format!("\x1b[1;{}{letter}", mods_param(mods, false)?).into_bytes())
}

fn legacy_home_end_key(key: &Key, mods: &KeyMods, alt_only: bool) -> Result<Vec<u8>, TuiError> {
    let letter = if matches!(key, Key::Home) { 'H' } else { 'F' };
    if mods.is_empty() {
        return Ok(format!("\x1b[{letter}").into_bytes());
    }
    if alt_only {
        return Ok(format!("\x1b\x1b[{letter}").into_bytes());
    }
    Ok(format!("\x1b[1;{}{letter}", mods_param(mods, false)?).into_bytes())
}

fn legacy_edit_key(key: &Key, mods: &KeyMods, alt_only: bool) -> Result<Vec<u8>, TuiError> {
    let n = match key {
        Key::Insert => 2,
        Key::Delete => 3,
        Key::PageUp => 5,
        _ => 6,
    };
    if mods.is_empty() {
        return Ok(format!("\x1b[{n}~").into_bytes());
    }
    if alt_only {
        return Ok(format!("\x1b\x1b[{n}~").into_bytes());
    }
    Ok(format!("\x1b[{n};{}~", mods_param(mods, false)?).into_bytes())
}

fn legacy_function_key(n: u8, mods: &KeyMods, alt_only: bool) -> Result<Vec<u8>, TuiError> {
    debug_assert!((1..=12).contains(&n));
    if n <= 4 {
        let letter = ['P', 'Q', 'R', 'S'][(n - 1) as usize];
        if mods.is_empty() {
            return Ok(format!("\x1bO{letter}").into_bytes());
        }
        if alt_only {
            return Ok(format!("\x1b\x1bO{letter}").into_bytes());
        }
        return Ok(format!("\x1b[1;{}{letter}", mods_param(mods, false)?).into_bytes());
    }
    let tilde = [15, 17, 18, 19, 20, 21, 23, 24][(n - 5) as usize];
    if mods.is_empty() {
        return Ok(format!("\x1b[{tilde}~").into_bytes());
    }
    if alt_only {
        return Ok(format!("\x1b\x1b[{tilde}~").into_bytes());
    }
    Ok(format!("\x1b[{tilde};{}~", mods_param(mods, false)?).into_bytes())
}

fn ctrl_byte(c: char) -> Result<u8, TuiError> {
    if c == ' ' {
        return Ok(0);
    }
    let upper = c.to_ascii_uppercase();
    if upper.is_ascii_alphabetic() {
        return Ok((upper as u8) & 0x1f);
    }
    if matches!(c, '@' | '[' | '\\' | ']' | '^' | '_' | '?') {
        return Ok((c as u8) & 0x1f);
    }
    Err(TuiError::Unsupported(
        "this ctrl+character chord needs the kitty keyboard protocol",
    ))
}
