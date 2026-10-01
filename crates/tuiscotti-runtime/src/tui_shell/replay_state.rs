use std::collections::HashSet;

use termpane::DamageGrid;
use termpane::cell::Cell as TermCell;
use termpane::grid::{MouseProtocolEncoding, MouseProtocolMode};
use termpane::snapshot::{GridSnapshot, SnapCell};

use super::{DefaultColors, Hyperlink, ReplayEvents, TermSnapshot};
use tuiscotti_core::frame::Rgb;
use tuiscotti_core::screen::Maybe;

/// Cap on hyperlinks collected from one replay.
const MAX_REPLAY_LINKS: usize = 1024;

fn replay_modes(grid: &DamageGrid) -> Vec<u16> {
    let mut out = Vec::new();
    if grid.application_cursor() {
        out.push(1);
    }
    // Backend gap (O5): modes 4 (IRM), 6 (DECOM), and 20 (LNM) are
    // absorbed untracked, so they never appear here.
    if grid.autowrap() {
        out.push(7);
    }
    if grid.application_keypad() {
        out.push(66);
    }
    match grid.mouse_protocol_mode() {
        MouseProtocolMode::None => {}
        MouseProtocolMode::Press => out.push(1000),
        MouseProtocolMode::PressRelease | MouseProtocolMode::ButtonMotion => out.push(1002),
        MouseProtocolMode::AnyEvent | MouseProtocolMode::AnyMotion => out.push(1003),
    }
    if grid.focus_events() {
        out.push(1004);
    }
    match grid.mouse_protocol_encoding() {
        // Urxvt (1015) is absorbed for parity: the old backend never
        // reported it, so reporting it now would be a new observable.
        MouseProtocolEncoding::Default | MouseProtocolEncoding::Urxvt => {}
        MouseProtocolEncoding::Utf8 => out.push(1005),
        MouseProtocolEncoding::Sgr => out.push(1006),
    }
    if grid.alternate_screen() {
        out.push(1049);
    }
    if grid.bracketed_paste() {
        out.push(2004);
    }
    if grid.kitty_kb_flags() != 0 {
        out.push(57399);
    }
    out
}

/// Full terminal state from a replayed emulator: everything is `Known`.
pub(crate) fn build_replay_state(
    grid: &DamageGrid,
    snapshot: &GridSnapshot,
    events: &ReplayEvents,
    cols: u16,
) -> TermSnapshot {
    // Backend gap (O6): the emulator drops OSC 4, so no palette overrides
    // are ever observed; assertion helpers resolve every index to the
    // nominal xterm default.
    let palette: Vec<(u8, Rgb)> = Vec::new();
    // Backend gap (O7): OSC 10/11 set forms are dropped, so program-set
    // defaults are unobservable; `None` = terminal default.
    let defaults = DefaultColors { fg: None, bg: None };
    let sb_len = grid.scrollback_len();
    let sb_rows = grid.scrollback_rows_at_offset(sb_len, sb_len);
    let mut scrollback = Vec::with_capacity(sb_rows.len());
    for row in &sb_rows {
        scrollback.push(replay_line_text(row, cols));
    }
    let mut seen = HashSet::new();
    let mut hyperlinks = Vec::new();
    for row in &sb_rows {
        collect_links_cells(row, &mut seen, &mut hyperlinks);
    }
    for row in &snapshot.cells {
        collect_links_snap(row, &mut seen, &mut hyperlinks);
    }
    TermSnapshot {
        title: match &events.title {
            Some(t) => Maybe::Known(t.clone()),
            None => Maybe::Unknown,
        },
        bells: Maybe::Known(events.bells),
        modes: Maybe::Known(replay_modes(grid)),
        palette: Maybe::Known(palette),
        defaults: Maybe::Known(defaults),
        clipboard: Maybe::Known(events.clipboard.clone()),
        hyperlinks: Maybe::Known(hyperlinks),
        scrollback: Maybe::Known(scrollback),
    }
}

fn replay_line_text(row: &[TermCell], cols: u16) -> String {
    let mut s = String::new();
    for cell in row.iter().take(cols as usize) {
        if cell.is_wide_continuation {
            continue;
        }
        if cell.contents().is_empty() {
            s.push(' ');
        } else {
            s.push_str(cell.contents());
        }
    }
    s.trim_end().to_string()
}

fn collect_links_cells(row: &[TermCell], seen: &mut HashSet<String>, out: &mut Vec<Hyperlink>) {
    if out.len() >= MAX_REPLAY_LINKS {
        return;
    }
    for cell in row {
        if out.len() >= MAX_REPLAY_LINKS {
            return;
        }
        if let Some(link) = cell.hyperlink.as_ref() {
            push_link_uri(&link.uri, seen, out);
        }
    }
}

fn collect_links_snap(row: &[SnapCell], seen: &mut HashSet<String>, out: &mut Vec<Hyperlink>) {
    if out.len() >= MAX_REPLAY_LINKS {
        return;
    }
    for cell in row {
        if out.len() >= MAX_REPLAY_LINKS {
            return;
        }
        if let Some(uri) = cell.hyperlink_uri.as_deref() {
            push_link_uri(uri, seen, out);
        }
    }
}

/// One hyperlink URI, deduplicated by URI with the replay cap.
fn push_link_uri(uri: &str, seen: &mut HashSet<String>, out: &mut Vec<Hyperlink>) {
    if !uri.is_empty() && seen.insert(uri.to_string()) {
        out.push(Hyperlink {
            uri: uri.to_string(),
        });
    }
}
