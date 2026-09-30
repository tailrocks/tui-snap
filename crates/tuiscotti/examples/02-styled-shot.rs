//! 02: styled screenshot — canonical + PNG as one sample, evidence on disk.
//!
//! Run: `cargo run --example 02-styled-shot`
//!
//! Like 01, the compound gate is pre-approved (canonical `.snap` + binary PNG
//! `.snap` + sidecar) in a temp dir, so `assert_screenshot!` PASSES. Candidate
//! evidence lands in a temp evidence dir as one partitioned bundle
//! (`<pkg>/<test>/<scenario>/run-*/attempt-*/`) holding every format.

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Paragraph;
use tuiscotti::assert::{Policy, generation_id, png_generation, png_tag_generation, render_sample};
use tuiscotti::ratatui::{EdgePolicy, render_screen};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let snaps = tmp.path().join("snaps");
    std::fs::create_dir(&snaps)?;
    let evidence = tmp.path().join("evidence");
    // Explicit dirs (in-process `set_var` is unavailable: an `unsafe fn` in
    // edition 2024); `INSTA_UPDATE` stays ambient.
    let policy = Policy::EvolvingIn {
        snapshots: snaps.clone(),
        evidence: evidence.clone(),
    };

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
    )?;
    let screen = shot.into_screen();

    // Pre-approve the compound sample: canonical text + tagged PNG bytes.
    let sample = render_sample(&screen)?;
    let generation = generation_id(&sample.canonical);
    assert_eq!(
        png_generation(&png_tag_generation(&sample.png, &generation)),
        Some(generation.clone())
    );
    std::fs::write(
        snaps.join("styled-shot.snap"),
        format!(
            "---\nsource: examples/02-styled-shot.rs\ndescription: tuiscotti generation {generation}\n\
             expression: canonical\n---\n{}",
            sample.canonical
        ),
    )?;
    std::fs::write(
        snaps.join("styled-shot-img.snap"),
        format!(
            "---\nsource: examples/02-styled-shot.rs\ndescription: tuiscotti generation {generation}\n\
             expression: png_bytes\nextension: png\nsnapshot_kind: binary\n---\n"
        ),
    )?;
    let tagged = png_tag_generation(&sample.png, &generation);
    std::fs::write(snaps.join("styled-shot-img.snap.png"), &tagged)?;

    tuiscotti::assert_screenshot!("styled-shot", &screen, &policy);

    // Evidence was written BEFORE the gate ran: discover the partitioned
    // bundle by querying the evidence root for the scenario's `complete.json`
    // (same technique the compound tests use — no hardcoded run/attempt),
    // then check every format landed and the PNG decodes.
    let bundle = find_bundle(&evidence, "styled-shot")?;
    for name in [
        "canonical.txt",
        "image.png",
        "sample.ansi",
        "sample.txt",
        "sample.html",
        "manifest.json",
        "complete.json",
    ] {
        assert!(bundle.join(name).is_file(), "missing bundle file {name}");
    }
    let bytes = std::fs::read(bundle.join("image.png"))?;
    image::load_from_memory(&bytes)?;
    println!(
        "EXAMPLE-02-OK png_bytes={} gen={}",
        bytes.len(),
        &generation[..12]
    );
    Ok(())
}

/// Find the published bundle for `scenario` under the evidence root
/// (exactly one `complete.json` beneath the scenario partition).
fn find_bundle(
    evidence: &std::path::Path,
    scenario: &str,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let mut stack = vec![evidence.to_path_buf()];
    let mut hits = Vec::new();
    while let Some(d) = stack.pop() {
        let entries: Vec<std::path::PathBuf> = std::fs::read_dir(&d)
            .map_err(|e| std::io::Error::other(format!("read {}: {e}", d.display())))?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()?;
        for path in entries {
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "complete.json")
                && path
                    .strip_prefix(evidence)
                    .is_ok_and(|rel| rel.components().any(|c| c.as_os_str() == scenario))
            {
                hits.push(
                    path.parent()
                        .map(std::path::Path::to_path_buf)
                        .ok_or_else(|| std::io::Error::other("bundle parent"))?,
                );
            }
        }
    }
    assert_eq!(hits.len(), 1, "exactly one bundle for {scenario}");
    hits.pop()
        .ok_or_else(|| Box::new(std::io::Error::other("bundle hit")) as Box<dyn std::error::Error>)
}
