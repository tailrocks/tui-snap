use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions as GridDims;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{ClipboardType, Config as TermConfig, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Color as VteColor, CursorShape, NamedColor, Processor, Rgb as VteRgb,
};

use tuiscotti_core::frame::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, UnderlineStyle};
use tuiscotti_core::screen::{Maybe, Observation, Screen};
use crate::tui::{CancelToken, ExitWait, Session, Tui, TuiError, WaitError};
use super::*;

/// Cap on hyperlinks collected from one replay.
const MAX_REPLAY_LINKS: usize = 1024;


fn replay_modes(mode: &TermMode) -> Vec<u16> {
    let mut out = Vec::new();
    let mut push = |flag: TermMode, n: u16| {
        if mode.contains(flag) {
            out.push(n);
        }
    };
    push(TermMode::APP_CURSOR, 1);
    push(TermMode::INSERT, 4);
    push(TermMode::ORIGIN, 6);
    push(TermMode::LINE_WRAP, 7);
    push(TermMode::LINE_FEED_NEW_LINE, 20);
    push(TermMode::APP_KEYPAD, 66);
    push(TermMode::MOUSE_REPORT_CLICK, 1000);
    push(TermMode::MOUSE_DRAG, 1002);
    push(TermMode::MOUSE_MOTION, 1003);
    push(TermMode::FOCUS_IN_OUT, 1004);
    push(TermMode::UTF8_MOUSE, 1005);
    push(TermMode::SGR_MOUSE, 1006);
    push(TermMode::ALT_SCREEN, 1049);
    push(TermMode::BRACKETED_PASTE, 2004);
    if mode.intersects(TermMode::KITTY_KEYBOARD_PROTOCOL) {
        out.push(57399);
    }
    out
}


fn vte_to_rgb(c: VteRgb) -> Rgb {
    Rgb {
        r: c.r,
        g: c.g,
        b: c.b,
    }
}


/// Full terminal state from a replayed emulator: everything is `Known`.
pub(crate) fn build_replay_state<T: EventListener>(
    term: &Term<T>,
    events: &ReplayEvents,
    cols: u16,
) -> TermSnapshot {
    let palette: Vec<(u8, Rgb)> = (0..256u16)
        .filter_map(|i| term.colors()[i as usize].map(|c| (i as u8, vte_to_rgb(c))))
        .collect();
    let defaults = DefaultColors {
        fg: term.colors()[NamedColor::Foreground].map(vte_to_rgb),
        bg: term.colors()[NamedColor::Background].map(vte_to_rgb),
    };
    let history = term
        .total_lines()
        .saturating_sub(term.screen_lines())
        .min(REPLAY_HISTORY);
    let grid = term.grid();
    let mut scrollback = Vec::with_capacity(history);
    for h in (0..history).rev() {
        scrollback.push(replay_line_text(grid, Line(-(h as i32) - 1), cols));
    }
    let mut seen = HashSet::new();
    let mut hyperlinks = Vec::new();
    for h in (0..history).rev() {
        collect_links(
            grid,
            Line(-(h as i32) - 1),
            cols,
            &mut seen,
            &mut hyperlinks,
        );
    }
    for y in 0..term.screen_lines() {
        collect_links(grid, Line(y as i32), cols, &mut seen, &mut hyperlinks);
    }
    TermSnapshot {
        title: match &events.title {
            Some(t) => Maybe::Known(t.clone()),
            None => Maybe::Unknown,
        },
        bells: Maybe::Known(events.bells),
        modes: Maybe::Known(replay_modes(term.mode())),
        palette: Maybe::Known(palette),
        defaults: Maybe::Known(defaults),
        clipboard: Maybe::Known(events.clipboard.clone()),
        hyperlinks: Maybe::Known(hyperlinks),
        scrollback: Maybe::Known(scrollback),
    }
}


fn replay_line_text(
    grid: &alacritty_terminal::grid::Grid<alacritty_terminal::term::cell::Cell>,
    line: Line,
    cols: u16,
) -> String {
    let mut s = String::new();
    for x in 0..cols {
        let cell = &grid[line][Column(x as usize)];
        if cell
            .flags
            .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        s.push(cell.c);
        if let Some(extra) = cell.zerowidth() {
            s.extend(extra.iter());
        }
    }
    s.trim_end().to_string()
}


fn collect_links(
    grid: &alacritty_terminal::grid::Grid<alacritty_terminal::term::cell::Cell>,
    line: Line,
    cols: u16,
    seen: &mut HashSet<String>,
    out: &mut Vec<Hyperlink>,
) {
    if out.len() >= MAX_REPLAY_LINKS {
        return;
    }
    for x in 0..cols {
        if out.len() >= MAX_REPLAY_LINKS {
            return;
        }
        let cell = &grid[line][Column(x as usize)];
        if let Some(link) = cell.hyperlink() {
            let uri = link.uri().to_string();
            if seen.insert(uri.clone()) {
                out.push(Hyperlink { uri });
            }
        }
    }
}
