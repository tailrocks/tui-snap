//! 02: styled screenshot — canonical + PNG as one sample, evidence on disk.
//!
//! Run: `cargo run --example 02-styled-shot`
//!
//! Like 01, the compound gate is pre-approved (canonical `.snap` + binary PNG
//! `.snap` + sidecar) in a temp dir, so `assert_screenshot!` PASSES. Candidate
//! evidence (`<name>.{png,ansi,txt,html}`) lands in a temp evidence dir.

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Paragraph;
use tuisnap::assert::{
    evidence_dir, generation_id, png_generation, png_tag_generation, render_sample,
    EVIDENCE_DIR_ENV, SNAPSHOT_DIR_ENV,
};
use tuisnap::ratatui::{render_screen, EdgePolicy};

fn main() {
    std::env::set_var("INSTA_UPDATE", "no");
    let tmp = tempfile::tempdir().unwrap();
    let snaps = tmp.path().join("snaps");
    std::fs::create_dir(&snaps).unwrap();
    std::env::set_var(SNAPSHOT_DIR_ENV, &snaps);
    std::env::set_var(EVIDENCE_DIR_ENV, tmp.path().join("evidence"));

    // Styled production view: bold red on blue.
    let shot = render_screen(
        24,
        4,
        |f| {
            f.render_widget(
                Paragraph::new("styled shot").style(
                    Style::default()
                        .fg(Color::Red)
                        .bg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ),
                f.area(),
            );
        },
        EdgePolicy::default(),
    )
    .unwrap();
    let screen = shot.into_screen();

    // Pre-approve the compound sample: canonical text + tagged PNG bytes.
    let sample = render_sample(&screen).unwrap();
    let gen = generation_id(&sample.canonical);
    assert_eq!(
        png_generation(&png_tag_generation(&sample.png, &gen)),
        Some(gen.clone())
    );
    std::fs::write(
        snaps.join("styled-shot.snap"),
        format!(
            "---\nsource: examples/02-styled-shot.rs\ndescription: tuisnap generation {gen}\n\
             expression: canonical\n---\n{}",
            sample.canonical
        ),
    )
    .unwrap();
    std::fs::write(
        snaps.join("styled-shot-img.snap"),
        format!(
            "---\nsource: examples/02-styled-shot.rs\ndescription: tuisnap generation {gen}\n\
             expression: png_bytes\nextension: png\nsnapshot_kind: binary\n---\n"
        ),
    )
    .unwrap();
    let tagged = png_tag_generation(&sample.png, &gen);
    std::fs::write(snaps.join("styled-shot-img.snap.png"), &tagged).unwrap();

    tuisnap::assert_screenshot!("styled-shot", &screen);

    // Evidence was written BEFORE the gate ran: PNG decodes, all four exist.
    let dir = evidence_dir();
    assert!(dir.join("styled-shot.png").exists());
    assert!(dir.join("styled-shot.ansi").exists());
    assert!(dir.join("styled-shot.txt").exists());
    assert!(dir.join("styled-shot.html").exists());
    let bytes = std::fs::read(dir.join("styled-shot.png")).unwrap();
    image::load_from_memory(&bytes).unwrap();
    println!("EXAMPLE-02-OK png_bytes={} gen={}", bytes.len(), &gen[..12]);
}
