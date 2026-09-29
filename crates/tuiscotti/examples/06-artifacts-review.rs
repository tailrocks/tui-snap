//! 06: artifacts + frozen policy — export four, pin two, reject acceptance.
//!
//! Run: `cargo run --example 06-artifacts-review`
//!
//! `emit_four` exports ANSI/TXT/PNG/HTML from one Screen; a frozen root pins
//! approved canonical+PNG and REJECTS acceptance (`frozen_accept` always
//! errors, frozen paths never write). All in temp dirs.

use tuiscotti::assert::{
    assert_frozen_snapshot, check_frozen_screenshot, emit_four, frozen_accept, generation_id,
    import_frozen_v1, png_tag_generation, render_sample, FrozenError,
};
use tuiscotti::insta_proto::insta_string;
use tuiscotti::ratatui::{render_screen, EdgePolicy};

fn main() {
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
    )
    .unwrap()
    .into_screen();

    // One sample pass → four review artifacts, byte-deterministic.
    let tmp = tempfile::tempdir().unwrap();
    let paths = emit_four(&screen, &tmp.path().join("four")).unwrap();
    assert!(paths.ansi.exists() && paths.txt.exists());
    assert!(paths.png.exists() && paths.html.exists());
    image::load_from_memory(&std::fs::read(&paths.png).unwrap()).unwrap();

    // The four-tree round-trips through the read-only importer: 1 scenario.
    let tree = import_frozen_v1(&paths.dir).unwrap();
    assert_eq!(tree.scenarios.len(), 1);
    assert_eq!(tree.scenarios[0].name, "snapshot");

    // Frozen root: approved canonical + tagged PNG, then the read-only gates.
    let root = tmp.path().join("frozen");
    std::fs::create_dir(&root).unwrap();
    let canonical = insta_string(&screen);
    let sample = render_sample(&screen).unwrap();
    let generation = generation_id(&canonical);
    std::fs::write(root.join("review-demo.canonical.txt"), &canonical).unwrap();
    std::fs::write(
        root.join("review-demo.png"),
        png_tag_generation(&sample.png, &generation),
    )
    .unwrap();
    check_frozen_screenshot(&root, "review-demo", &screen).unwrap();
    assert_frozen_snapshot(&root, "review-demo", &screen);

    // Frozen roots never bless: acceptance is rejected, nothing is written.
    match frozen_accept(&root, "review-demo") {
        Err(FrozenError::AcceptRejected { .. }) => {}
        other => panic!("frozen_accept must reject, got {other:?}"),
    }
    println!(
        "EXAMPLE-06-OK scenarios={} gen={}",
        tree.scenarios.len(),
        &generation[..12]
    );
}
