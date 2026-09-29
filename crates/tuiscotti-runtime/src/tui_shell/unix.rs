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

use crate::tui::{CancelToken, ExitWait, Session, Tui, TuiError, WaitError};
use tuiscotti_core::frame::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, UnderlineStyle};
use tuiscotti_core::screen::{Maybe, Observation, Screen};

#[cfg(unix)]
pub(crate) mod guardian_unix {
    use crate::tui_shell::{
        Containment, GuardianReport, MAX_PS_LINES, MAX_SURVIVORS, MAX_SWEEP_TARGETS,
    };
    use std::time::{Duration, Instant};

    #[derive(Debug, Clone)]
    pub(super) struct ProcRow {
        pub(super) pid: u32,
        pub(super) pgid: i32,
        pub(super) sid: i32,
        pub(super) lstart: String,
    }

    pub(crate) fn capture_ids(pid: u32) -> Option<crate::tui_shell::ChildIds> {
        if pid == 0 {
            return None;
        }
        // No libc: single-pid `ps` probe (the workspace forbids `unsafe`),
        // with the same `pgid=,sess=` parse as `reverify` below.
        let out = std::process::Command::new("ps")
            .args(["-o", "pgid=,sess=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut parts = text.split_whitespace();
        let (pgid, sid) = match (parts.next(), parts.next()) {
            (Some(g), Some(s)) => (g.parse::<i32>().ok()?, s.parse::<i32>().ok()?),
            _ => return None,
        };
        let own = own_pgid()?;
        if pgid <= 1 || sid < 0 {
            return None;
        }
        if pgid == own {
            // The child shares OUR group: a group sweep would suicide.
            return None;
        }
        Some(crate::tui_shell::ChildIds {
            pid,
            pgid,
            sid,
            start: lstart_of(pid),
        })
    }

    /// Our own process-group id via `ps` (replaces `getpgrp(2)`; the
    /// workspace forbids `unsafe`).
    fn own_pgid() -> Option<i32> {
        let out = std::process::Command::new("ps")
            .args(["-o", "pgid=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .next()?
            .parse::<i32>()
            .ok()
    }

    pub(super) fn lstart_of(pid: u32) -> Option<String> {
        let out = std::process::Command::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if line.is_empty() { None } else { Some(line) }
    }

    /// One full process-table snapshot, filtered by the caller.
    pub(super) fn snapshot() -> Option<Vec<ProcRow>> {
        let out = std::process::Command::new("ps")
            .args(["-ax", "-o", "pid=,pgid=,sess=,lstart="])
            .output()
            .ok()?;
        if !out.status.success() || out.stdout.len() > 8 << 20 {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut rows = Vec::new();
        for line in text.lines().take(MAX_PS_LINES) {
            let mut parts = line.split_whitespace();
            let (Some(pid), Some(pgid), Some(sid)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let (Ok(pid), Ok(pgid), Ok(sid)) =
                (pid.parse::<u32>(), pgid.parse::<i32>(), sid.parse::<i32>())
            else {
                continue;
            };
            let lstart: String = parts.collect::<Vec<_>>().join(" ");
            rows.push(ProcRow {
                pid,
                pgid,
                sid,
                lstart,
            });
        }
        Some(rows)
    }

    /// Fresh single-pid identity check, used to re-verify each target
    /// immediately before signalling (closes the scan/kill TOCTOU).
    pub(super) fn reverify(pid: u32, pgid: i32, sid: i32) -> bool {
        let out = match std::process::Command::new("ps")
            .args(["-o", "pgid=,sess=", "-p", &pid.to_string()])
            .output()
        {
            Ok(o) => o,
            Err(_) => return false,
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut parts = text.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some(g), Some(s)) => g.parse::<i32>() == Ok(pgid) && s.parse::<i32>() == Ok(sid),
            _ => false,
        }
    }

    /// Guards (any failure refuses, never kills blindly):
    /// 1. no identity -> refuse; 2. any member with a foreign sid -> abort
    ///    (group id reused); 3. never pid 0/1/self; 4. the direct child pid
    ///    only when its start time still matches; 5. every other target
    ///    re-verified (pgid+sid) immediately before the signal.
    pub(crate) fn sweep(
        child: &Option<crate::tui_shell::ChildIds>,
        deadline: Option<Instant>,
        teardown_error: Option<String>,
    ) -> GuardianReport {
        let Some(child) = child.as_ref() else {
            return empty_report(
                None,
                None,
                Containment::Unknown {
                    reason: "child identity unavailable (no pid, exited early, or shared group)"
                        .to_string(),
                },
                teardown_error,
            );
        };
        let own = std::process::id();
        let Some(rows) = snapshot() else {
            return empty_report(
                Some(child.pid),
                Some(child.pgid),
                Containment::Unknown {
                    reason: "process-table snapshot failed".to_string(),
                },
                teardown_error,
            );
        };
        let members: Vec<&ProcRow> = rows.iter().filter(|r| r.pgid == child.pgid).collect();
        if members.iter().any(|m| m.sid != child.sid) {
            return empty_report(
                Some(child.pid),
                Some(child.pgid),
                Containment::Refused {
                    reason: format!(
                        "group {} contains foreign-session members; id may be reused",
                        child.pgid
                    ),
                },
                teardown_error,
            );
        }
        let signals = signal_sweep_targets(child, &members, own);
        let survivors = settle_survivors(child, deadline, own);
        let containment = if survivors.is_empty() {
            Containment::Full
        } else {
            Containment::Partial
        };
        GuardianReport {
            child_pid: Some(child.pid),
            pgid: Some(child.pgid),
            sid_verified: signals.sid_verified,
            start_verified: signals.start_verified,
            signalled: signals.signalled,
            survivors,
            containment,
            teardown_error,
        }
    }

    /// Report for a sweep that signalled nothing (refused/unknown).
    fn empty_report(
        child_pid: Option<u32>,
        pgid: Option<i32>,
        containment: Containment,
        teardown_error: Option<String>,
    ) -> GuardianReport {
        GuardianReport {
            child_pid,
            pgid,
            sid_verified: false,
            start_verified: None,
            signalled: Vec::new(),
            survivors: Vec::new(),
            containment,
            teardown_error,
        }
    }

    /// What the signal pass did, pid by pid.
    struct SweepSignals {
        signalled: Vec<u32>,
        sid_verified: bool,
        start_verified: Option<bool>,
    }

    /// Signal each group member (bounded): never pid 0/1/self, the direct
    /// child pid only when its start time still matches, every other
    /// target re-verified (pgid+sid) immediately before the signal.
    fn signal_sweep_targets(
        child: &crate::tui_shell::ChildIds,
        members: &[&ProcRow],
        own: u32,
    ) -> SweepSignals {
        let mut signalled = Vec::new();
        let mut sid_verified = true;
        let mut start_verified = None;
        for m in members.iter().take(MAX_SWEEP_TARGETS) {
            if m.pid <= 1 || m.pid == own {
                continue;
            }
            if m.pid == child.pid {
                match (&child.start, &m.lstart) {
                    (Some(a), b) if a == b => start_verified = Some(true),
                    (Some(_), _) => continue, // pid reused: refuse this pid
                    (None, _) => start_verified = None,
                }
            }
            if !reverify(m.pid, child.pgid, child.sid) {
                sid_verified = false;
                continue;
            }
            // No libc: `kill -KILL` per re-verified pid (the workspace
            // forbids `unsafe`); delivery failure reads as not-signalled.
            let delivered = std::process::Command::new("kill")
                .arg("-KILL")
                .arg(m.pid.to_string())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if delivered {
                signalled.push(m.pid);
            }
        }
        SweepSignals {
            signalled,
            sid_verified,
            start_verified,
        }
    }

    /// Settle: bounded rescan for survivors (capped at `MAX_SURVIVORS`).
    fn settle_survivors(
        child: &crate::tui_shell::ChildIds,
        deadline: Option<Instant>,
        own: u32,
    ) -> Vec<u32> {
        let settle = deadline
            .map(|d| {
                d.saturating_duration_since(Instant::now())
                    .min(crate::tui_shell::SWEEP_SETTLE)
            })
            .unwrap_or(crate::tui_shell::SWEEP_SETTLE)
            .min(Duration::from_secs(5));
        let start = Instant::now();
        let mut survivors = Vec::new();
        loop {
            let alive: Vec<u32> = snapshot()
                .unwrap_or_default()
                .iter()
                .filter(|r| r.pgid == child.pgid && r.sid == child.sid && r.pid > 1 && r.pid != own)
                .map(|r| r.pid)
                .collect();
            if alive.is_empty() {
                survivors.clear();
                break;
            }
            survivors = alive;
            if start.elapsed() >= settle {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        survivors.truncate(MAX_SURVIVORS);
        survivors
    }
}
