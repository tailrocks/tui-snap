//! 01: pure view test — production draw closure → `Screen` → `assert_snapshot!`.
//!
//! Run: `cargo run --example 01-pure-view`
//!
//! The gate is pre-approved into a temp snapshot dir, so the assertion PASSES
//! deterministically. Hermetic dirs travel on [`Policy::EvolvingIn`] (in-process
//! `set_var` is unavailable: an `unsafe fn` in edition 2024); `INSTA_UPDATE`
//! stays ambient.

use ratatui::widgets::Paragraph;
use tuiscotti::assert::{Policy, generation_id};
use tuiscotti::insta_proto::insta_string;
use tuiscotti::ratatui::{EdgePolicy, render_screen};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Production draw closure: the real render path, not a hand-made grid.
    let shot = render_screen(
        20,
        4,
        |f| f.render_widget(Paragraph::new("hello tui-snap"), f.area()),
        EdgePolicy::default(),
    )?;
    assert!(!shot.has_clips());
    let screen = shot.into_screen();
    // Row 0 carries the paragraph text (canonical form is per-cell lines).
    let mut row0 = String::new();
    for x in 0..screen.cols() {
        row0.push_str(&screen.get(x, 0).ok_or("row0 cell missing")?.symbol);
    }
    assert!(row0.contains("hello tui-snap"), "row0 was {row0:?}");

    // Pre-approve: exact canonical bytes under a temp snapshot dir.
    let tmp = tempfile::tempdir()?;
    let snaps = tmp.path().join("snaps");
    std::fs::create_dir(&snaps)?;
    let policy = Policy::EvolvingIn {
        snapshots: snaps.clone(),
        evidence: tmp.path().join("evidence"),
    };
    let canonical = insta_string(&screen);
    let generation = generation_id(&canonical);
    std::fs::write(
        snaps.join("pure-view.snap"),
        format!(
            "---\nsource: examples/01-pure-view.rs\ndescription: tuisnap generation {generation}\n\
             expression: canonical\n---\n{canonical}"
        ),
    )?;

    tuiscotti::assert_snapshot!("pure-view", &screen, &policy);
    assert!(!snaps.join("pure-view.snap.new").exists());
    println!(
        "EXAMPLE-01-OK gen={} bytes={}",
        &generation[..12],
        canonical.len()
    );
    Ok(())
}
