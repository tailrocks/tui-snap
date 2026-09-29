//! README lock (C10): every README code fence compiles and the documented
//! CLI surface matches `--help`.
//!
//! Each test mirrors one README fence statement-for-statement (same calls,
//! temp dirs / runnable programs substituted for placeholders), or asserts
//! one documented CLI command/flag appears in the real help text. If the
//! README drifts from the code again, this file goes red first.

use std::path::PathBuf;
use std::process::Command;

use ratatui::widgets::Paragraph;
use tuiscotti::Provenance;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tuisnap"))
}

fn help(args: &[&str]) -> std::io::Result<String> {
    let out = Command::new(bin()).args(args).output()?;
    assert!(out.status.success(), "help {args:?} failed");
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn home_frame() -> tuiscotti::Frame {
    tuiscotti::ratatui::draw_frame(
        120,
        40,
        Provenance::now("tuisnap-default", "home", vec![]),
        |f| f.render_widget(Paragraph::new("home"), f.area()),
    )
}

#[path = "readme_lock/fences.rs"]
mod fences;

#[path = "readme_lock/api.rs"]
mod api;

#[path = "readme_lock/grouped.rs"]
mod grouped;

#[path = "readme_lock/cli.rs"]
mod cli;
