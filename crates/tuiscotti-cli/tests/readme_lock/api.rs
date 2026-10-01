//! README API map + macro gates (split from `readme_lock.rs`; shared helpers live in the root).

use ratatui::widgets::Paragraph;
use tuiscotti::Profile;
use tuiscotti::frame::UnderlineStyle;

// ---------------------------------------------------------------------------
// API-map section: every named facade item resolves and behaves.
// ---------------------------------------------------------------------------

fn api_screen() -> Result<tuiscotti::Screen, Box<dyn std::error::Error>> {
    Ok(tuiscotti::ratatui::render_screen(
        30,
        5,
        |f| f.render_widget(Paragraph::new("alpha needle beta"), f.area()),
        tuiscotti::ratatui::EdgePolicy::default(),
    )?
    .into_screen())
}

#[test]
fn readme_api_map_render_and_locate() {
    // ratatui::render_screen -> Screen.
    let screen = api_screen().expect("render api screen");
    assert_eq!((screen.cols(), screen.rows()), (30, 5));

    // locate::Locator text + present/not-present.
    let obs = tuiscotti::screen::Observation::new(
        screen.clone(),
        7,
        tuiscotti::screen::CaptureReason::Manual,
        tuiscotti::screen::TermState::default(),
        tuiscotti::screen::CaptureProvenance::new(0, None, None, 0),
    );
    let span = tuiscotti::locate::Locator::text("needle")
        .resolve_unique(&screen, 7)
        .expect("resolve needle");
    assert_eq!(span.text, "needle");
    assert!(
        tuiscotti::locate::Locator::text("needle")
            .present_now(&obs)
            .expect("present check")
    );
    assert!(
        tuiscotti::locate::Locator::text("no-such-text")
            .not_present_now(&obs)
            .expect("absent check")
    );
    assert!(
        tuiscotti::locate::Locator::regex("n.edle")
            .expect("regex locator")
            .present_now(&obs)
            .expect("regex present")
    );
}

#[test]
fn readme_api_map_command_and_proto() {
    // command::Command piped run.
    let out = tuiscotti::command::Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .run();
    assert!(out.success());
    assert_eq!(out.code(), Some(0));

    // proto typed ops + machine line.
    match tuiscotti::proto::execute(&tuiscotti::proto::Op::Version).expect("version op") {
        tuiscotti::proto::OpResult::Version { protocol, .. } => {
            assert_eq!(protocol, tuiscotti::proto::PROTOCOL_VERSION);
        }
        other => panic!("expected Version, got {other:?}"),
    }
    assert_eq!(
        tuiscotti::proto::capabilities().protocol,
        tuiscotti::proto::PROTOCOL_VERSION
    );
    let (line, ok) = tuiscotti::proto::run_machine_line(r#"{"type":"capabilities"}"#);
    assert!(ok);
    assert!(line.contains(r#""ok":true"#));
    let (bad_line, bad_ok) = tuiscotti::proto::run_machine_line("not json");
    assert!(!bad_ok);
    assert!(bad_line.contains("invalid-input"));
}

#[test]
fn readme_api_map_runner_and_assert() {
    let screen = api_screen().expect("render api screen");

    // runner::TestContext from an injected env (no global state).
    let tmp = tempfile::tempdir().expect("tempdir");
    let ctx = tuiscotti::runner::TestContext::from_map(
        "readme-lock",
        &std::collections::HashMap::new(),
        tmp.path(),
    )
    .expect("test context");
    assert!(ctx.evidence_dir().is_dir());

    // assert helpers: sample render, generation binding, four-tree export.
    let sample = tuiscotti::assert::render_sample(&screen).expect("render sample");
    let generation = tuiscotti::assert::generation_id(&sample.canonical);
    assert_eq!(
        tuiscotti::assert::png_generation(&tuiscotti::assert::png_tag_generation(
            &sample.png,
            &generation
        )),
        Some(generation)
    );
    let paths = tuiscotti::assert::emit_four(&screen, &tmp.path().join("four")).expect("emit four");
    assert!(paths.ansi.is_file() && paths.txt.is_file());
    assert!(paths.png.is_file() && paths.html.is_file());
}

#[test]
fn readme_api_map_observe_export_and_pins() {
    let screen = api_screen().expect("render api screen");
    let tmp = tempfile::tempdir().expect("tempdir");

    // observe + export + mcp.
    assert_eq!(
        tuiscotti::observe::screen_text(&screen),
        tuiscotti::proto::screen_text(&screen)
    );
    let cast =
        tuiscotti::export::cast_v2(&[("hi".to_string(), 0.5)], 80, 24, &tmp.path().join("cast"))
            .expect("write cast");
    assert!(cast.is_file());
    assert!(!tuiscotti::mcp::tools().is_empty());
    let list = tuiscotti::mcp::tools_list_json();
    assert!(
        list.get("tools")
            .and_then(|t| t.as_array())
            .is_some_and(|t| !t.is_empty())
    );

    // Rendering pins.
    assert_eq!(tuiscotti::VENDORED_FALLBACK_FACES.len(), 3);
    let profile = Profile::default_profile();
    assert_eq!((profile.cell_w, profile.cell_h), (10, 21));
    assert!((profile.font_px - 16.0).abs() < f32::EPSILON);
    assert_eq!(profile.scale, 2);
    assert_eq!(tuiscotti::frame::FRAME_VERSION, 3);
    // Schema v3 + additive underline style/color: bool untouched, new keys sparse.
    assert_eq!(UnderlineStyle::default(), UnderlineStyle::None);
    assert!(UnderlineStyle::Curly.is_some());
    assert_eq!(UnderlineStyle::Double.token(), "double-underline");
    let cell = tuiscotti::frame::Cell::blank(0, 0);
    assert!(!cell.mods.underline);
    assert_eq!(cell.mods.underline_style, UnderlineStyle::None);
    assert_eq!(cell.underline_color, tuiscotti::frame::Color::Default);
}

// ---------------------------------------------------------------------------
// Macro gates named in the README: assert_snapshot! / assert_screenshot!
// pass against pre-approved temp snapshots (insta pattern from 01/02).
// ---------------------------------------------------------------------------

#[test]
fn readme_macro_gates_pass_preapproved() {
    use tuiscotti::assert::{Policy, generation_id, png_tag_generation, render_sample};
    use tuiscotti::ratatui::{EdgePolicy, render_screen};
    use tuiscotti::screen::canonical_string;

    let tmp = tempfile::tempdir().expect("tempdir");
    let snaps = tmp.path().join("snaps");
    std::fs::create_dir(&snaps).expect("mkdir");
    let evidence = tmp.path().join("evidence");
    // Explicit dirs (`set_var` is an `unsafe fn` in edition 2024 and cannot
    // be used under the workspace lints); `INSTA_UPDATE` stays ambient.
    let policy = Policy::EvolvingIn {
        snapshots: snaps.clone(),
        evidence: evidence.clone(),
    };

    let screen = render_screen(
        20,
        4,
        |f| f.render_widget(Paragraph::new("readme lock"), f.area()),
        EdgePolicy::default(),
    )
    .expect("render lock screen")
    .into_screen();

    // Pre-approve the canonical text gate.
    let canonical = canonical_string(&screen);
    let generation = generation_id(&canonical);
    std::fs::write(
        snaps.join("readme-lock.snap"),
        format!(
            "---\nsource: tests/readme_lock.rs\ndescription: tuiscotti generation {generation}\n\
             expression: canonical\n---\n{canonical}"
        ),
    )
    .expect("write snap");
    tuiscotti::assert_snapshot!("readme-lock", &screen, &policy);
    assert!(!snaps.join("readme-lock.snap.new").exists());

    // Pre-approve the compound screenshot gate (canonical + tagged PNG).
    let sample = render_sample(&screen).expect("render sample");
    assert_eq!(generation_id(&sample.canonical), generation);
    std::fs::write(
        snaps.join("readme-lock-shot.snap"),
        format!(
            "---\nsource: tests/readme_lock.rs\ndescription: tuiscotti generation {generation}\n\
             expression: canonical\n---\n{}",
            sample.canonical
        ),
    )
    .expect("write snap");
    std::fs::write(
        snaps.join("readme-lock-shot-img.snap"),
        format!(
            "---\nsource: tests/readme_lock.rs\ndescription: tuiscotti generation {generation}\n\
             expression: png_bytes\nextension: png\nsnapshot_kind: binary\n---\n"
        ),
    )
    .expect("write snap");
    std::fs::write(
        snaps.join("readme-lock-shot-img.snap.png"),
        png_tag_generation(&sample.png, &generation),
    )
    .expect("write snap png");
    tuiscotti::assert_screenshot!("readme-lock-shot", &screen, &policy);
    assert!(bundle_has(&evidence, "readme-lock-shot", "image.png"));
    assert!(bundle_has(&evidence, "readme-lock-shot", "complete.json"));
}

/// Whether the evidence root holds `file` inside `scenario`'s bundle partition.
fn bundle_has(evidence: &std::path::Path, scenario: &str, file: &str) -> bool {
    let mut stack = vec![evidence.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n == file)
                && p.strip_prefix(evidence)
                    .is_ok_and(|rel| rel.components().any(|c| c.as_os_str() == scenario))
            {
                return true;
            }
        }
    }
    false
}
