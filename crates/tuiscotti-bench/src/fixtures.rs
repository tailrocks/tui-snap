//! Fixed benchmark fixtures: three sizes × seven journeys.
//!
//! Every journey renders through the REAL production view functions from
//! `tuiscotti-fixtures` (the same functions headless tests and PTY fixture
//! binaries use) captured with `render_screen`. Nothing is shrunk for speed:
//! 200x60 dense/unicode are the heaviest committed views.

use tuiscotti::Screen;
use tuiscotti::ratatui::{EdgePolicy, render_screen};
use tuiscotti_fixtures::views::{Theme, matrix, menu};

/// Fixed sizes: (cols, rows, label).
pub const SIZES: [(u16, u16, &str); 3] =
    [(80, 24, "80x24"), (120, 40, "120x40"), (200, 60, "200x60")];

/// Journey names; see [`render_journey`].
pub const JOURNEYS: [&str; 7] = [
    "plain", "dense", "unicode", "scroll", "overlay", "cursor", "resize",
];

/// Render one fixed fixture screen:
/// `plain` = home list, `dense` = id/value table, `unicode` = glyph sampler,
/// `scroll` = settings menu stepped down 5 rows, `overlay` = modal dialog,
/// `cursor` = glyph sampler with typed input + hardware cursor,
/// `resize` = home model re-rendered after a geometry change (the alternate
/// size renders first and is discarded; the timed capture is at target size).
///
/// # Errors
///
/// Returns an error when the journey is unknown or capture fails.
pub fn render_journey(journey: &str, cols: u16, rows: u16) -> anyhow::Result<Screen> {
    render_journey_inner(journey, cols, rows, false)
}

/// Deliberately changed twin of [`render_journey`] (one model step ahead):
/// counter +1, selection +1, or one more input char.
///
/// # Errors
///
/// Returns an error when the journey is unknown or capture fails.
pub fn render_journey_changed(journey: &str, cols: u16, rows: u16) -> anyhow::Result<Screen> {
    render_journey_inner(journey, cols, rows, true)
}

/// Plain home screen salted by `salt`: distinct models (hence distinct
/// content hashes) per iteration for cache miss/hit matrices.
///
/// # Errors
///
/// Returns an error when capture fails.
pub fn render_varied(cols: u16, rows: u16, salt: u32) -> anyhow::Result<Screen> {
    let mut model = matrix::Model::new(matrix::Screen::Home, true);
    model.count = i32::try_from(salt & 0x7fff_ffff).unwrap_or(0);
    let capture = render_screen(
        cols,
        rows,
        |f| matrix::render(f, &model),
        EdgePolicy::default(),
    )
    .map_err(|e| anyhow::anyhow!("render varied: {e}"))?;
    Ok(capture.into_screen())
}

fn render_journey_inner(
    journey: &str,
    cols: u16,
    rows: u16,
    changed: bool,
) -> anyhow::Result<Screen> {
    if journey == "resize" {
        pre_render_alt(cols, rows)?;
    }
    let capture = match journey {
        "plain" | "resize" => render_matrix(matrix::Screen::Home, cols, rows, changed, 0)?,
        "dense" => render_matrix(matrix::Screen::Table, cols, rows, changed, 1)?,
        "unicode" => render_matrix(matrix::Screen::Glyphs, cols, rows, changed, 2)?,
        "scroll" => render_menu_scroll(cols, rows, changed)?,
        "overlay" => render_matrix(matrix::Screen::Dialog, cols, rows, changed, 4)?,
        "cursor" => render_matrix(matrix::Screen::Glyphs, cols, rows, changed, 5)?,
        other => return Err(anyhow::anyhow!("unknown journey: {other}")),
    };
    Ok(capture.into_screen())
}

///
/// # Errors
///
/// Returns an error when the alternate-size capture fails.
fn pre_render_alt(cols: u16, rows: u16) -> anyhow::Result<()> {
    // Geometry-change path: paint the alternate size first (discarded).
    let (alt_cols, alt_rows) = if cols == 80 { (120, 40) } else { (80, 24) };
    let model = matrix::Model::new(matrix::Screen::Home, true);
    let _ = render_screen(
        alt_cols,
        alt_rows,
        |f| matrix::render(f, &model),
        EdgePolicy::default(),
    )
    .map_err(|e| anyhow::anyhow!("resize pre-render: {e}"))?;
    let _ = rows;
    Ok(())
}

///
/// # Errors
///
/// Returns an error when capture fails.
fn render_menu_scroll(
    cols: u16,
    rows: u16,
    changed: bool,
) -> anyhow::Result<tuiscotti::ratatui::ScreenCapture> {
    let mut model = menu::Model::demo(Theme::Dark);
    let steps = if changed { 6 } else { 5 };
    for _ in 0..steps {
        let _ = menu::step(&mut model, &menu::MenuKey::Down);
    }
    render_screen(
        cols,
        rows,
        |f| menu::render(f, &model),
        EdgePolicy::default(),
    )
    .map_err(|e| anyhow::anyhow!("render scroll: {e}"))
}

///
/// # Errors
///
/// Returns an error when capture fails.
fn render_matrix(
    screen: matrix::Screen,
    cols: u16,
    rows: u16,
    changed: bool,
    variant: u8,
) -> anyhow::Result<tuiscotti::ratatui::ScreenCapture> {
    let mut model = matrix::Model::new(screen, true);
    match variant {
        1 => {
            if changed {
                model.selected += 1;
            }
        }
        2 => {
            model.input = if changed {
                "test⌨!".to_string()
            } else {
                "test⌨".to_string()
            };
        }
        5 => {
            model.input = if changed {
                "hello!".to_string()
            } else {
                "hello".to_string()
            };
            model.cursor_at_end = true;
        }
        _ => {
            if changed {
                model.count += 1;
            }
        }
    }
    render_screen(
        cols,
        rows,
        |f| matrix::render(f, &model),
        EdgePolicy::default(),
    )
    .map_err(|e| anyhow::anyhow!("render matrix: {e}"))
}
