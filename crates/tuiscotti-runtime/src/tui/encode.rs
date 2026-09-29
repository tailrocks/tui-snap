//! Input encoding, worker side: bytes derived from the live `TermMode`.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions as GridDims;
use alacritty_terminal::term::{Term, TermMode};
use portable_pty::{MasterPty, PtySize};

use super::encode_key::encode_key;
use super::error::TuiError;
use super::input_types::{MouseButton, MouseMods, Wheel};
use super::worker::{Input, MouseAction, WorkerDims};

pub(crate) fn apply_input<T: EventListener>(
    term: &mut Term<T>,
    input: &Input,
    writer: Option<&mut (dyn std::io::Write + Send + 'static)>,
    exited: bool,
) -> Result<(), TuiError> {
    if exited {
        return Err(TuiError::ChildExited("child already exited".to_string()));
    }
    let writer = writer.ok_or_else(|| TuiError::Closed("stdin is closed".to_string()))?;
    let mode = *term.mode();
    let bytes: Option<Vec<u8>> = match input {
        Input::Bytes(b) => Some(b.clone()),
        Input::Paste(text) => Some(encode_paste(text, mode)?),
        Input::Key { key, mods, kind } => encode_key(key, *mods, *kind, mode)?,
        Input::Mouse { action, x, y, mods } => {
            let (cols, rows) = (term.columns(), term.screen_lines());
            Some(encode_mouse(action, *x, *y, *mods, mode, cols, rows)?)
        }
        Input::Focus(focused) => Some(encode_focus(*focused, mode)?),
    };
    match bytes {
        Some(b) if !b.is_empty() => writer
            .write_all(&b)
            .map_err(|e| TuiError::Io(format!("pty write failed: {e}"))),
        _ => Ok(()),
    }
}

pub(crate) fn apply_resize<T: EventListener>(
    master: &dyn MasterPty,
    term: &mut Term<T>,
    cols: u16,
    rows: u16,
) -> Result<(), TuiError> {
    // PTY first: if the kernel refuses, the emulator stays consistent.
    master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| TuiError::Io(format!("pty resize failed: {e}")))?;
    term.resize(WorkerDims {
        cols: cols as usize,
        rows: rows as usize,
    });
    Ok(())
}

// -- paste ---------------------------------------------------------------

const PASTE_START: &str = "\x1b[200~";
const PASTE_END: &str = "\x1b[201~";

fn encode_paste(text: &str, mode: TermMode) -> Result<Vec<u8>, TuiError> {
    if text.contains(PASTE_START) || text.contains(PASTE_END) {
        return Err(TuiError::PasteRejected(
            "content contains bracketed-paste delimiters".to_string(),
        ));
    }
    if mode.contains(TermMode::BRACKETED_PASTE) {
        Ok(format!("{PASTE_START}{text}{PASTE_END}").into_bytes())
    } else {
        Ok(text.as_bytes().to_vec())
    }
}

// -- focus ---------------------------------------------------------------

fn encode_focus(focused: bool, mode: TermMode) -> Result<Vec<u8>, TuiError> {
    if !mode.contains(TermMode::FOCUS_IN_OUT) {
        return Err(TuiError::ModeNotEnabled(
            "focus tracking (DEC 1004) not enabled by the application",
        ));
    }
    Ok(if focused {
        b"\x1b[I".to_vec()
    } else {
        b"\x1b[O".to_vec()
    })
}

// -- mouse ---------------------------------------------------------------

fn button_code(b: MouseButton) -> u8 {
    match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

/// Required live mode bits for one mouse action, plus the error text when
/// the application has not enabled them.
fn mouse_mode_gate(action: &MouseAction) -> (TermMode, &'static str) {
    let any = TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION;
    let required: TermMode = match action {
        MouseAction::Press(_) | MouseAction::Release | MouseAction::Wheel(_) => any,
        MouseAction::Move { held: None } => TermMode::MOUSE_MOTION,
        MouseAction::Move { held: Some(_) } => TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION,
    };
    let what = match action {
        MouseAction::Press(_) | MouseAction::Release | MouseAction::Wheel(_) => {
            "mouse reporting (DEC 1000/1002/1003) not enabled by the application"
        }
        MouseAction::Move { held: None } => {
            "mouse motion reporting (DEC 1003) not enabled by the application"
        }
        MouseAction::Move { held: Some(_) } => {
            "mouse drag reporting (DEC 1002/1003) not enabled by the application"
        }
    };
    (required, what)
}

fn encode_mouse(
    action: &MouseAction,
    x: u16,
    y: u16,
    mods: MouseMods,
    term_mode: TermMode,
    cols: usize,
    rows: usize,
) -> Result<Vec<u8>, TuiError> {
    if x as usize >= cols || y as usize >= rows {
        return Err(TuiError::InvalidInput(format!(
            "mouse ({x},{y}) outside {cols}x{rows} grid"
        )));
    }
    let (required, what) = mouse_mode_gate(action);
    if !term_mode.intersects(required) {
        return Err(TuiError::ModeNotEnabled(what));
    }

    let mut cb: u32 = match action {
        MouseAction::Press(b) => u32::from(button_code(*b)),
        MouseAction::Release => 3,
        MouseAction::Move { held: None } => 3 + 32,
        MouseAction::Move { held: Some(b) } => u32::from(button_code(*b)) + 32,
        MouseAction::Wheel(Wheel::Up) => 64,
        MouseAction::Wheel(Wheel::Down) => 65,
        MouseAction::Wheel(Wheel::Left) => 66,
        MouseAction::Wheel(Wheel::Right) => 67,
    };
    if mods.shift {
        cb += 4;
    }
    if mods.alt {
        cb += 8;
    }
    if mods.ctrl {
        cb += 16;
    }
    let cx = u32::from(x) + 1;
    let cy = u32::from(y) + 1;

    if term_mode.contains(TermMode::SGR_MOUSE) {
        let marker = if matches!(action, MouseAction::Release) {
            'm'
        } else {
            'M'
        };
        return Ok(format!("\x1b[<{cb};{cx};{cy}{marker}").into_bytes());
    }
    if term_mode.contains(TermMode::UTF8_MOUSE) {
        let mut out = b"\x1b[M".to_vec();
        for v in [cb + 32, cx + 32, cy + 32] {
            let ch = char::from_u32(v).ok_or_else(|| {
                TuiError::InvalidInput(format!("mouse coordinate {v} unencodable"))
            })?;
            let mut tmp = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
        }
        return Ok(out);
    }
    // Legacy X10: single bytes, coordinates must fit.
    for (v, name) in [(cb + 32, "button"), (cx + 32, "x"), (cy + 32, "y")] {
        if v > 255 {
            return Err(TuiError::InvalidInput(format!(
                "mouse {name} {v} exceeds legacy X10 encoding"
            )));
        }
    }
    Ok(vec![
        0x1b,
        b'[',
        b'M',
        u8::try_from(cb + 32).unwrap_or(u8::MAX),
        u8::try_from(cx + 32).unwrap_or(u8::MAX),
        u8::try_from(cy + 32).unwrap_or(u8::MAX),
    ])
}
