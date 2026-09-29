use super::convert::{convert_color, convert_mods};
use super::{ClippedCell, EdgePolicy, REPLACEMENT, ScreenCapture};
use crate::frame::{Cell, Cursor, CursorStyle};
use crate::screen::{Screen, ScreenError};
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell as RCell};
use ratatui::layout::Position;
use ratatui::widgets::{StatefulWidget, Widget};
use unicode_width::UnicodeWidthStr;

/// Look up the buffer cell behind grid `(gx, gy)`. Buffer coordinates are
/// global (offset by the area origin).
fn buffer_cell(buf: &Buffer, gx: u16, gy: u16) -> Result<&RCell, ScreenError> {
    let area = buf.area;
    let ax = u16::try_from(u32::from(area.x) + u32::from(gx))
        .map_err(|_| ScreenError(format!("buffer x overflow at grid ({gx},{gy})")))?;
    let ay = u16::try_from(u32::from(area.y) + u32::from(gy))
        .map_err(|_| ScreenError(format!("buffer y overflow at grid ({gx},{gy})")))?;
    buf.cell((ax, ay))
        .ok_or_else(|| ScreenError(format!("buffer missing cell at global ({ax},{ay})")))
}

/// Handle a width-2 glyph in the last column: fail under [`EdgePolicy::Error`],
/// else push a [`REPLACEMENT`] cell and record the clip. Returns the advanced
/// grid x.
fn clip_row_end(
    policy: EdgePolicy,
    rc: &RCell,
    symbol: String,
    gx: u16,
    gy: u16,
    cells: &mut Vec<Cell>,
    clipped: &mut Vec<ClippedCell>,
) -> Result<u16, ScreenError> {
    match policy {
        EdgePolicy::Error => Err(ScreenError(format!(
            "wide grapheme {symbol:?} at row end ({gx},{gy}): \
             no room for its continuation (EdgePolicy::Error)"
        ))),
        EdgePolicy::ClipWithReplacement => {
            cells.push(Cell {
                x: gx,
                y: gy,
                symbol: REPLACEMENT.to_string(),
                width: 1,
                continuation: false,
                fg: convert_color(rc.fg),
                bg: convert_color(rc.bg),
                mods: convert_mods(rc.modifier),
                underline_color: convert_color(rc.underline_color),
            });
            clipped.push(ClippedCell {
                x: gx,
                y: gy,
                symbol,
            });
            Ok(gx + 1)
        }
    }
}

/// Push a lead cell plus its continuation follower when `width == 2`.
/// Returns the advanced grid x.
fn push_lead(
    rc: &RCell,
    symbol: String,
    width: u8,
    gx: u16,
    gy: u16,
    cells: &mut Vec<Cell>,
) -> u16 {
    let (fg, bg, mods, ucolor) = (
        convert_color(rc.fg),
        convert_color(rc.bg),
        convert_mods(rc.modifier),
        convert_color(rc.underline_color),
    );
    cells.push(Cell {
        x: gx,
        y: gy,
        symbol,
        width,
        continuation: false,
        fg,
        bg,
        mods,
        underline_color: ucolor,
    });
    if width == 2 {
        cells.push(Cell {
            x: gx + 1,
            y: gy,
            symbol: String::new(),
            width: 0,
            continuation: true,
            fg,
            bg,
            mods,
            underline_color: ucolor,
        });
        gx + 2
    } else {
        gx + 1
    }
}

/// Translate a global cursor position to grid-local. A cursor outside the
/// buffer area cannot be represented on the grid, so it is captured hidden
/// with a note rather than failing the whole capture.
fn translate_cursor(
    cursor: Option<(Position, bool)>,
    area: ratatui::layout::Rect,
    cols: u16,
    rows: u16,
    notes: &mut Vec<String>,
) -> Cursor {
    let (ox, oy) = (i32::from(area.x), i32::from(area.y));
    let mut cur = Cursor::default();
    if let Some((pos, visible)) = cursor {
        let lx = i32::from(pos.x) - ox;
        let ly = i32::from(pos.y) - oy;
        if visible && lx >= 0 && ly >= 0 && lx < i32::from(cols) && ly < i32::from(rows) {
            cur = Cursor {
                x: lx as u16,
                y: ly as u16,
                visible: true,
                style: CursorStyle::Block,
                blinking: false,
            };
        } else if visible {
            notes.push(format!(
                "cursor at global ({},{}) is outside buffer area {area:?}: captured hidden",
                pos.x, pos.y
            ));
        }
    }
    cur
}

fn convert_buffer(
    buf: &Buffer,
    cursor: Option<(Position, bool)>,
    policy: EdgePolicy,
) -> Result<ScreenCapture, ScreenError> {
    let area = buf.area;
    let (cols, rows) = (area.width, area.height);
    let (ox, oy) = (i32::from(area.x), i32::from(area.y));
    let mut cells = Vec::with_capacity(cols as usize * rows as usize);
    let mut clipped = Vec::new();
    let mut notes = Vec::new();
    let mut gy: u16 = 0;
    while gy < rows {
        let mut gx: u16 = 0;
        while gx < cols {
            let rc = buffer_cell(buf, gx, gy)?;
            let symbol = rc.symbol().to_string();
            let width = UnicodeWidthStr::width(symbol.as_str()).clamp(1, 2) as u8;
            if width == 2 && gx + 1 >= cols {
                gx = clip_row_end(policy, rc, symbol, gx, gy, &mut cells, &mut clipped)?;
                continue;
            }
            gx = push_lead(rc, symbol, width, gx, gy, &mut cells);
        }
        gy += 1;
    }
    if !clipped.is_empty() {
        notes.push(format!(
            "EdgePolicy::ClipWithReplacement replaced {} wide glyph(s) at row ends",
            clipped.len()
        ));
    }
    let cur = translate_cursor(cursor, area, cols, rows, &mut notes);
    let screen = Screen::validate(cols, rows, ox, oy, cells, cur)?;
    Ok(ScreenCapture {
        screen,
        policy,
        clipped,
        notes,
    })
}

/// Convert a buffer into a validated [`Screen`] (M05 buffer adapter).
///
/// Dimensions and origin come from `buf.area`: a nonzero area origin is
/// preserved as the screen origin, and cells are re-indexed to grid-local
/// coordinates. `cursor` is `(global position, visible)`; it is translated
/// to grid-local, and a visible cursor outside the area is captured hidden
/// with a note.
///
/// Style coverage (M03-partial):
///
/// | Ratatui source | Canonical `Cell`/`Mods` | Status |
/// |---|---|---|
/// | symbol, display width, wide continuations | `symbol`, `width`, `continuation` | preserved |
/// | fg/bg incl Reset/indexed/RGB | `fg`, `bg` | preserved |
/// | BOLD/DIM/ITALIC/UNDERLINED/CROSSED_OUT/REVERSED | `mods` flags | preserved |
/// | HIDDEN | `mods.hidden` | preserved (intent; renderers omit the glyph) |
/// | SLOW_BLINK/RAPID_BLINK | `mods.blink` | preserved (intent; stills freeze phase) |
/// | underline color | `underline_color` | preserved via the `underline-color` cargo feature (`Reset` → `Default`) |
/// | underline style | `mods.underline` | PARTIAL: ratatui 0.30 exposes only the UNDERLINED bit (no style API), so every ratatui underline maps to `Single` |
/// | hyperlinks (OSC 8) | — | NOT exposed: `Buffer`/`Cell` store no link targets |
/// | title, bells, modes, palette, clipboard, graphics | — | NOT exposed by `Buffer`/`TestBackend` |
/// | cursor position + visibility | `Cursor` x/y/visible | preserved (post-draw) |
/// | cursor style / blink | `Block`, non-blinking | NOT exposed by `TestBackend`; defaults recorded |
pub fn screen_from_buffer(
    buf: &Buffer,
    cursor: Option<(Position, bool)>,
    policy: EdgePolicy,
) -> Result<ScreenCapture, ScreenError> {
    convert_buffer(buf, cursor, policy)
}

/// Capture the completed state of a `TestBackend` terminal: buffer + cursor
/// (M05). Reads post-draw cursor position/visibility so cursor-only changes
/// are gated; buffer origin is preserved like [`screen_from_buffer`].
pub fn screen_from_test_backend(
    term: &mut ratatui::Terminal<TestBackend>,
    policy: EdgePolicy,
) -> Result<ScreenCapture, ScreenError> {
    let backend = term.backend();
    let cursor = Some((backend.cursor_position(), backend.cursor_visible()));
    let buf = backend.buffer().clone();
    convert_buffer(&buf, cursor, policy)
}

/// Render a production draw closure into a validated [`Screen`] (M05/M06).
///
/// The closure is the real render path (`FnOnce(&mut ratatui::Frame)`):
/// layouts, stateful widgets, and `set_cursor_position` all work. Cursor
/// state is captured post-draw, so explicit cursor placement survives.
pub fn render_screen(
    cols: u16,
    rows: u16,
    draw: impl FnOnce(&mut ratatui::Frame),
    policy: EdgePolicy,
) -> Result<ScreenCapture, ScreenError> {
    let backend = TestBackend::new(cols, rows);
    let mut term =
        ratatui::Terminal::new(backend).map_err(|e| ScreenError(format!("test terminal: {e}")))?;
    term.draw(draw)
        .map_err(|e| ScreenError(format!("draw: {e}")))?;
    screen_from_test_backend(&mut term, policy)
}

/// Render a production draw closure into a validated [`Screen`] (G6 simple path).
///
/// `size` is `(cols, rows)`; the closure is the real render path
/// (`FnOnce(&mut ratatui::Frame)`). Edge clips fail loudly under
/// [`EdgePolicy::Error`] so the simple path never hides substitutions — use
/// [`render_screen`] with an explicit policy for lenient clipping.
///
/// ```rust
/// # use tuiscotti_core::ratatui as shot;
/// # use tuiscotti_core::screen::Screen;
/// use ratatui::widgets::{Block, Paragraph};
/// let screen = shot::render((100, 30), |frame| {
///     frame.render_widget(Paragraph::new("hi").block(Block::bordered()), frame.area());
/// })?;
/// assert_eq!((screen.cols(), screen.rows()), (100, 30));
/// # Ok::<(), tuiscotti_core::screen::ScreenError>(())
/// ```
pub fn render(
    size: (u16, u16),
    draw: impl FnOnce(&mut ratatui::Frame),
) -> Result<Screen, ScreenError> {
    render_screen(size.0, size.1, draw, EdgePolicy::Error).map(ScreenCapture::into_screen)
}

/// Render any production `Widget` fullscreen into a [`ScreenCapture`] (M05).
///
/// Cursor state is whatever the draw leaves behind (`TestBackend` defaults
/// to hidden); nothing is forced, so the capture reflects production.
pub fn widget_screen<W>(
    widget: W,
    cols: u16,
    rows: u16,
    policy: EdgePolicy,
) -> Result<ScreenCapture, ScreenError>
where
    W: Widget,
{
    render_screen(
        cols,
        rows,
        |f| {
            f.render_widget(widget, f.area());
        },
        policy,
    )
}

/// Render a production `StatefulWidget` with caller-owned state (M06).
///
/// No artificial testing trait: the bound is the real
/// `ratatui::widgets::StatefulWidget`, so actual production render functions
/// work unchanged.
pub fn stateful_screen<W>(
    widget: W,
    state: &mut W::State,
    cols: u16,
    rows: u16,
    policy: EdgePolicy,
) -> Result<ScreenCapture, ScreenError>
where
    W: StatefulWidget,
{
    render_screen(
        cols,
        rows,
        |f| {
            f.render_stateful_widget(widget, f.area(), state);
        },
        policy,
    )
}
