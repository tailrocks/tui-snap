//! 01: pure view test — production draw closure → Screen → assert_snapshot.
//!
//! Run: `cargo run --example 01-pure-view`
//!
//! The gate is pre-approved into a temp snapshot dir, so the assertion PASSES
//! deterministically. `INSTA_UPDATE=no` is set in-process: even on drift the
//! example fails without writing pendings anywhere.

use ratatui::widgets::Paragraph;
use tuisnap::assert::{generation_id, SNAPSHOT_DIR_ENV};
use tuisnap::insta_proto::insta_string;
use tuisnap::ratatui::{render_screen, EdgePolicy};

fn main() {
    std::env::set_var("INSTA_UPDATE", "no");

    // Production draw closure: the real render path, not a hand-made grid.
    let shot = render_screen(
        20,
        4,
        |f| f.render_widget(Paragraph::new("hello tui-snap"), f.area()),
        EdgePolicy::default(),
    )
    .unwrap();
    assert!(!shot.has_clips());
    let screen = shot.into_screen();
    // Row 0 carries the paragraph text (canonical form is per-cell lines).
    let mut row0 = String::new();
    for x in 0..screen.cols() {
        row0.push_str(&screen.get(x, 0).unwrap().symbol);
    }
    assert!(row0.contains("hello tui-snap"), "row0 was {row0:?}");

    // Pre-approve: exact canonical bytes under a temp snapshot dir.
    let tmp = tempfile::tempdir().unwrap();
    let snaps = tmp.path().join("snaps");
    std::fs::create_dir(&snaps).unwrap();
    std::env::set_var(SNAPSHOT_DIR_ENV, &snaps);
    let canonical = insta_string(&screen);
    let gen = generation_id(&canonical);
    std::fs::write(
        snaps.join("pure-view.snap"),
        format!(
            "---\nsource: examples/01-pure-view.rs\ndescription: tuisnap generation {gen}\n\
             expression: canonical\n---\n{canonical}"
        ),
    )
    .unwrap();

    tuisnap::assert_snapshot!("pure-view", &screen);
    assert!(!snaps.join("pure-view.snap.new").exists());
    println!("EXAMPLE-01-OK gen={} bytes={}", &gen[..12], canonical.len());
}
