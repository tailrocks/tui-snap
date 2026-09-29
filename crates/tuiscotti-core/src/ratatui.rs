//! Direct Ratatui buffer adapter: production view → canonical [`Frame`].
//!
//! No ANSI, no subprocesses, no PTY in unit tests. Render the real view
//! (widget or draw closure, including stateful widgets) into a `TestBackend`,
//! then convert the buffer — styles and cursor preserved.
//!
//! ```rust,no_run
//! use ratatui::{backend::TestBackend, Terminal, widgets::Paragraph};
//! use tuiscotti_core::{Provenance, ratatui as tuiscotti_core_ratatui};
//!
//! let backend = TestBackend::new(80, 24);
//! let mut term = Terminal::new(backend).unwrap();
//! term.draw(|f| f.render_widget(Paragraph::new("hi"), f.area())).unwrap();
//! let frame = tuiscotti_core_ratatui::capture(
//!     &mut term,
//!     Provenance::now("default", "ratatui", vec![]),
//! );
//! assert!(frame.text().contains("hi"));
//! ```
//!
//! The [`Screen`]-based API below ([`render_screen`], [`screen_from_buffer`],
//! [`screen_from_test_backend`], [`widget_screen`], [`stateful_screen`]) is the
//! M1 path: production draw closures and real `Widget`/`StatefulWidget`
//! renders convert into validated [`Screen`]s with buffer origins and
//! post-draw cursor state preserved, and an explicit [`EdgePolicy`] for
//! wide glyphs at row ends (backlog M05, M06, M03-partial, M07).

use crate::frame::{Cell, Color, Cursor, CursorStyle, Frame, Mods, Provenance, UnderlineStyle};
use ratatui::backend::Backend;
use ratatui::buffer::{Buffer, Cell as RCell};
use ratatui::layout::Position;
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

fn convert_color(c: ratatui::style::Color) -> Color {
    use ratatui::style::Color as C;
    match c {
        C::Reset => Color::Default,
        C::Black => Color::Indexed(0),
        C::Red => Color::Indexed(1),
        C::Green => Color::Indexed(2),
        C::Yellow => Color::Indexed(3),
        C::Blue => Color::Indexed(4),
        C::Magenta => Color::Indexed(5),
        C::Cyan => Color::Indexed(6),
        C::Gray => Color::Indexed(7),
        C::DarkGray => Color::Indexed(8),
        C::LightRed => Color::Indexed(9),
        C::LightGreen => Color::Indexed(10),
        C::LightYellow => Color::Indexed(11),
        C::LightBlue => Color::Indexed(12),
        C::LightMagenta => Color::Indexed(13),
        C::LightCyan => Color::Indexed(14),
        C::White => Color::Indexed(15),
        C::Indexed(i) => Color::Indexed(i),
        C::Rgb(r, g, b) => Color::Rgb(crate::frame::Rgb::new(r, g, b)),
    }
}

fn convert_mods(m: Modifier) -> Mods {
    Mods {
        hidden: m.contains(Modifier::HIDDEN),
        blink: m.intersects(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK),
        bold: m.contains(Modifier::BOLD),
        dim: m.contains(Modifier::DIM),
        italic: m.contains(Modifier::ITALIC),
        // Ratatui exposes no underline STYLE (only the UNDERLINED bit), so
        // any ratatui underline is Single; see the `screen_from_buffer` table.
        underline: m.contains(Modifier::UNDERLINED),
        underline_style: if m.contains(Modifier::UNDERLINED) {
            UnderlineStyle::Single
        } else {
            UnderlineStyle::None
        },
        strikethrough: m.contains(Modifier::CROSSED_OUT),
        reverse: m.contains(Modifier::REVERSED),
    }
}

/// Convert one buffer cell. Wide symbols (width 2) produce the lead cell;
/// the caller emits the continuation follower.
fn lead_cell(x: u16, y: u16, rc: &RCell) -> (Cell, u8) {
    let symbol = rc.symbol().to_string();
    let width = UnicodeWidthStr::width(symbol.as_str()).min(2) as u8;
    let width = width.max(1);
    (
        Cell {
            x,
            y,
            symbol,
            width,
            continuation: false,
            fg: convert_color(rc.fg),
            bg: convert_color(rc.bg),
            mods: convert_mods(rc.modifier),
            underline_color: convert_color(rc.underline_color),
        },
        width,
    )
}

/// Convert a buffer plus explicit cursor state into a [`Frame`].
///
/// `cursor`: `(position, visible)`. Read it from
/// `TestBackend::get_cursor_position` after draw; `None` hides the cursor.
pub fn from_buffer(
    buf: &Buffer,
    cols: u16,
    rows: u16,
    cursor: Option<(Position, bool)>,
    provenance: Provenance,
) -> Frame {
    let mut frame = Frame::blank(cols, rows, provenance);
    // Straightforward row-major conversion with wide-cell continuations.
    for y in 0..rows {
        let mut x = 0u16;
        while x < cols {
            let Some(rc) = buf.cell((x, y)) else {
                x += 1;
                continue;
            };
            // Ratatui marks wide-cell followers with an empty symbol.
            if rc.symbol().is_empty() {
                let mut cont = Cell::blank(x, y);
                cont.fg = convert_color(rc.fg);
                cont.bg = convert_color(rc.bg);
                cont.mods = convert_mods(rc.modifier);
                cont.underline_color = convert_color(rc.underline_color);
                cont.width = 0;
                cont.continuation = true;
                cont.symbol = String::new();
                frame.set(cont);
                x += 1;
                continue;
            }
            let (lead, w) = lead_cell(x, y, rc);
            if w == 2 && x + 1 >= cols {
                // Wide grapheme at the exact row end: no room for its
                // continuation (a terminal would wrap or clip it). Downgrade
                // to width 1 so the frame stays valid; geometry still shows
                // the full symbol in one cell.
                let mut narrow = lead;
                narrow.width = 1;
                frame.set(narrow);
                x += 1;
                continue;
            }
            frame.set(lead);
            if w == 2 && x + 1 < cols {
                let mut cont = Cell::blank(x + 1, y);
                cont.fg = convert_color(rc.fg);
                cont.bg = convert_color(rc.bg);
                cont.mods = convert_mods(rc.modifier);
                cont.underline_color = convert_color(rc.underline_color);
                cont.width = 0;
                cont.continuation = true;
                cont.symbol = String::new();
                frame.set(cont);
                x += 2;
            } else {
                x += 1;
            }
        }
    }
    if let Some((pos, visible)) = cursor {
        frame.cursor = Cursor {
            x: pos.x,
            y: pos.y,
            visible,
            style: CursorStyle::Block,
            blinking: false,
        };
    }
    // Blank filler cells already carry width 1; normalize any untouched cell
    // that the buffer left as default (ratatui guarantees full coverage, but
    // stay total here).
    frame
}

/// Capture the current state of a `TestBackend` terminal: buffer + cursor.
///
/// Reads `get_cursor_position` post-draw so cursor-only changes are gated.
pub fn capture(
    term: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
    provenance: Provenance,
) -> Frame {
    let backend = term.backend_mut();
    let area = backend.buffer().area;
    let cursor = backend
        .get_cursor_position()
        .ok()
        .map(|pos| (pos, backend.cursor_visible()));
    // `Buffer::clone` via re-read: TestBackend exposes `buffer()`.
    let buf = backend.buffer().clone();
    from_buffer(&buf, area.width, area.height, cursor, provenance)
}

/// Render any `Widget` into a [`Frame`] at `cols`×`rows`.
///
/// The hardware cursor is forced hidden: widget unit tests pin content, and
/// a backend-default cursor at (0,0) would make the gate depend on backend
/// defaults instead of the view. Use [`draw_frame`]/[`capture`] for
/// stateful cursor placement.
pub fn widget_frame<W>(widget: W, cols: u16, rows: u16, provenance: Provenance) -> Frame
where
    W: ratatui::widgets::Widget,
{
    let mut frame = draw_frame(cols, rows, provenance, |f| {
        f.render_widget(widget, f.area());
    });
    frame.cursor.visible = false;
    frame
}

/// Render via a draw closure (full-app frames, layouts, stateful widgets).
/// Cursor is captured post-draw, so stateful cursor placement is preserved.
pub fn draw_frame(
    cols: u16,
    rows: u16,
    provenance: Provenance,
    draw: impl FnOnce(&mut ratatui::Frame),
) -> Frame {
    let backend = ratatui::backend::TestBackend::new(cols, rows);
    let mut term = ratatui::Terminal::new(backend).expect("test terminal");
    term.draw(draw).expect("draw");
    capture(&mut term, provenance)
}

// ---------------------------------------------------------------------------
// M1 Screen adapters (backlog M05, M06, M03-partial, M07 edge policy).
// ---------------------------------------------------------------------------

use crate::screen::{Screen, ScreenError};
use ratatui::backend::TestBackend;
use ratatui::widgets::{StatefulWidget, Widget};

/// Policy for a width-2 grapheme landing in the last column of a row, where
/// no room remains for its continuation cell (M07: never cut silently).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgePolicy {
    /// Replace the glyph with [`REPLACEMENT`] (U+FFFD, width 1) and record
    /// each clipped glyph in [`ScreenCapture::clipped`] plus a note. The
    /// screen stays valid and the substitution is visible in evidence.
    #[default]
    ClipWithReplacement,
    /// Fail with a [`ScreenError`] naming the glyph and its position.
    Error,
}

/// Record of one wide glyph clipped at a row end (grid-local coordinates,
/// original symbol preserved for evidence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClippedCell {
    pub x: u16,
    pub y: u16,
    pub symbol: String,
}

/// Substitute emitted by [`EdgePolicy::ClipWithReplacement`]: U+FFFD with
/// display width 1, keeping grid geometry valid.
pub const REPLACEMENT: &str = "�";

/// A validated [`Screen`] plus the record of how it was produced.
///
/// `Screen` itself carries no provenance, so the edge policy applied, every
/// clipped glyph, and any cursor adjustments are recorded here instead of
/// being applied silently (M07/M08).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenCapture {
    /// Validated grid; construction fails rather than producing invalid data.
    pub screen: Screen,
    /// Edge policy this capture was produced under.
    pub policy: EdgePolicy,
    /// Wide glyphs replaced at row ends (empty unless clipped).
    pub clipped: Vec<ClippedCell>,
    /// Human-readable notes: clip summaries, cursor adjustments.
    pub notes: Vec<String>,
}

impl ScreenCapture {
    #[must_use]
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    #[must_use]
    pub fn into_screen(self) -> Screen {
        self.screen
    }

    #[must_use]
    pub fn has_clips(&self) -> bool {
        !self.clipped.is_empty()
    }
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
            // Buffer coordinates are global (offset by the area origin).
            let ax = u16::try_from(u32::from(area.x) + u32::from(gx))
                .map_err(|_| ScreenError(format!("buffer x overflow at grid ({gx},{gy})")))?;
            let ay = u16::try_from(u32::from(area.y) + u32::from(gy))
                .map_err(|_| ScreenError(format!("buffer y overflow at grid ({gx},{gy})")))?;
            let rc = buf
                .cell((ax, ay))
                .ok_or_else(|| ScreenError(format!("buffer missing cell at global ({ax},{ay})")))?;
            let symbol = rc.symbol().to_string();
            let width = UnicodeWidthStr::width(symbol.as_str()).clamp(1, 2) as u8;
            if width == 2 && gx + 1 >= cols {
                match policy {
                    EdgePolicy::Error => {
                        return Err(ScreenError(format!(
                            "wide grapheme {symbol:?} at row end ({gx},{gy}): \
                             no room for its continuation (EdgePolicy::Error)"
                        )));
                    }
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
                        gx += 1;
                        continue;
                    }
                }
            }
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
                gx += 2;
            } else {
                gx += 1;
            }
        }
        gy += 1;
    }
    if !clipped.is_empty() {
        notes.push(format!(
            "EdgePolicy::ClipWithReplacement replaced {} wide glyph(s) at row ends",
            clipped.len()
        ));
    }
    // Cursor positions are global; translate to grid-local. A cursor outside
    // the buffer area cannot be represented on the grid, so it is captured
    // hidden with a note rather than failing the whole capture.
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
