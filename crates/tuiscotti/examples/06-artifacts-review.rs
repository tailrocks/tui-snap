//! 06: artifacts + frozen policy — export four, pin two, reject acceptance.
//!
//! Run: `cargo run --example 06-artifacts-review`
//!
//! `emit_four` exports ANSI/TXT/PNG/HTML from one Screen; a frozen root pins
//! approved canonical+PNG and REJECTS acceptance (`frozen_accept` always
//! errors, frozen paths never write). All in temp dirs.

use tuiscotti::assert::{
    FrozenError, assert_frozen_snapshot, check_frozen_screenshot, emit_four, frozen_accept,
    generation_id, import_frozen_v1, png_tag_generation, render_sample,
};
use tuiscotti::insta_proto::insta_string;
use tuiscotti::ratatui::{EdgePolicy, render_screen};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let screen = render_screen(
        20,
        4,
        |f| {
            f.render_widget(
                ratatui::widgets::Paragraph::new("frozen review me"),
                f.area(),
            );
        },
        EdgePolicy::default(),
    )?
    .into_screen();

    // One sample pass → four review artifacts, byte-deterministic.
    let tmp = tempfile::tempdir()?;
    let paths = emit_four(&screen, &tmp.path().join("four"))?;
    assert!(paths.ansi.exists() && paths.txt.exists());
    assert!(paths.png.exists() && paths.html.exists());
    image::load_from_memory(&std::fs::read(&paths.png)?)?;

    // The four-tree round-trips through the read-only importer: 1 scenario.
    let tree = import_frozen_v1(&paths.dir)?;
    assert_eq!(tree.scenarios.len(), 1);
    assert_eq!(tree.scenarios[0].name, "snapshot");

    // Frozen root: approved canonical + tagged PNG, then the read-only gates.
    let root = tmp.path().join("frozen");
    std::fs::create_dir(&root)?;
    let canonical = insta_string(&screen);
    let sample = render_sample(&screen)?;
    let generation = generation_id(&canonical);
    std::fs::write(root.join("review-demo.canonical.txt"), &canonical)?;
    std::fs::write(
        root.join("review-demo.png"),
        png_tag_generation(&sample.png, &generation),
    )?;
    check_frozen_screenshot(&root, "review-demo", &screen)?;
    assert_frozen_snapshot(&root, "review-demo", &screen);

    // Frozen roots never bless: acceptance is rejected, nothing is written.
    match frozen_accept(&root, "review-demo") {
        Err(FrozenError::AcceptRejected { .. }) => {}
        other => return Err(format!("frozen_accept must reject, got {other:?}").into()),
    }
    println!(
        "EXAMPLE-06-OK scenarios={} gen={}",
        tree.scenarios.len(),
        &generation[..12]
    );
    Ok(())
}
