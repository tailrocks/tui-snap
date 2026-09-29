//! M4 locator core tests (backlog Q01-Q05, Q08, Q10).

use tuiscotti::frame::{Cell, Cursor};
use tuiscotti::locate::Span;
use tuiscotti::screen::{CaptureProvenance, CaptureReason, Observation, Screen, TermState};

#[path = "locate/pattern.rs"]
mod pattern;
#[path = "locate/scope.rs"]
mod scope;
#[path = "locate/text.rs"]
mod text;
#[path = "locate/viewport.rs"]
mod viewport;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn blank(cols: u16, rows: u16) -> Vec<Cell> {
    (0..rows)
        .flat_map(|y| (0..cols).map(move |x| Cell::blank(x, y)))
        .collect()
}

fn put(cells: &mut [Cell], cols: u16, x: u16, y: u16, text: &str) {
    for (i, ch) in text.chars().enumerate() {
        let idx = y as usize * cols as usize + x as usize + i;
        cells[idx].symbol = ch.to_string();
    }
}

fn put_wide(cells: &mut [Cell], cols: u16, x: u16, y: u16, sym: &str) {
    let idx = y as usize * cols as usize + x as usize;
    cells[idx].symbol = sym.to_string();
    cells[idx].width = 2;
    cells[idx + 1].symbol = String::new();
    cells[idx + 1].width = 0;
    cells[idx + 1].continuation = true;
}

fn finish(cells: Vec<Cell>, cols: u16, rows: u16) -> Screen {
    Screen::validate(cols, rows, 0, 0, cells, Cursor::default()).unwrap()
}

fn obs(screen: Screen, revision: u64) -> Observation {
    Observation::new(
        screen,
        revision,
        CaptureReason::Manual,
        TermState::default(),
        CaptureProvenance::new(0, None, None, 0),
    )
}

fn screen_with(text_rows: &[&str], cols: u16) -> Screen {
    let rows = text_rows.len() as u16;
    let mut cells = blank(cols, rows);
    for (y, row) in text_rows.iter().enumerate() {
        put(&mut cells, cols, 0, y as u16, row);
    }
    finish(cells, cols, rows)
}

/// Scripted observer: yields each observation in turn, repeating the last.
fn script(mut script: Vec<Observation>) -> impl FnMut() -> Observation {
    move || {
        if script.len() > 1 {
            script.remove(0)
        } else {
            script[0].clone()
        }
    }
}

fn long_span(s: &Span) -> bool {
    s.text.chars().count() > 3
}

// ---------------------------------------------------------------------------
// Q01: text match modes
// ---------------------------------------------------------------------------
