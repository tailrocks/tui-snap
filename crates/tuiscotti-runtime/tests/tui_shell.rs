//! Shell/state/replay/guardian tests (backlog R09, R12, R13, R14): real
//! PTY, real processes, bounded timeouts.

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

/// Pids whose full command line contains `token`.
fn pgrep(token: &str) -> Vec<u32> {
    let out = std::process::Command::new("pgrep")
        .args(["-f", token])
        .output()
        .expect("output succeeds");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .filter_map(|p| p.parse::<u32>().ok())
        .collect()
}

fn pkill(token: &str) {
    let _ = std::process::Command::new("pkill")
        .args(["-f", token])
        .output();
}

/// Process group of `pid` (`None` when the pid is gone/unresolvable).
fn pgid_of(pid: u32) -> Option<i32> {
    let out = std::process::Command::new("ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn wait_gone(token: &str, secs: u64) {
    let dl = deadline(secs);
    while Instant::now() < dl {
        if pgrep(token).is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "processes matching {token:?} still alive: {:?}",
        pgrep(token)
    );
}

fn wait_found(token: &str, secs: u64) -> Vec<u32> {
    let dl = deadline(secs);
    loop {
        let pids = pgrep(token);
        if !pids.is_empty() || Instant::now() >= dl {
            return pids;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[path = "tui_shell/shell.rs"]
mod shell;

#[path = "tui_shell/state.rs"]
mod state;

#[path = "tui_shell/replay.rs"]
mod replay;

#[path = "tui_shell/guardian.rs"]
mod guardian;
