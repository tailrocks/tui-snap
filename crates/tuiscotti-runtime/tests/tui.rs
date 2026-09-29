//! PTY session runtime tests (backlog R06-R11-core): real PTY, real
//! processes (`/bin/cat`, `/bin/sh`, `/bin/sleep`), bounded timeouts.

#![cfg(feature = "pty")]

use std::time::{Duration, Instant};

use tuiscotti_core::screen::Screen;
use tuiscotti_runtime::tui::CancelToken;

fn deadline(secs: u64) -> Instant {
    Instant::now() + Duration::from_secs(secs)
}

fn cancel() -> CancelToken {
    CancelToken::new()
}

/// Plain-text rows of a screen (trailing blanks trimmed per row).
fn rows(screen: &Screen) -> Vec<String> {
    let mut out = Vec::with_capacity(screen.rows() as usize);
    for y in 0..screen.rows() {
        let mut s = String::new();
        for x in 0..screen.cols() {
            let c = screen
                .get(x, y)
                .unwrap_or_else(|| panic!("missing cell {x},{y}"));
            if !c.continuation {
                s.push_str(&c.symbol);
            }
        }
        out.push(s.trim_end().to_string());
    }
    out
}

fn contains(screen: &Screen, needle: &str) -> bool {
    rows(screen).iter().any(|r| r.contains(needle))
}

#[path = "tui/session.rs"]
mod session;

#[path = "tui/input.rs"]
mod input;
