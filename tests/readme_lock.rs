//! README lock (C10): every README code fence compiles and the documented
//! CLI surface matches `--help`.
//!
//! Each test mirrors one README fence statement-for-statement (same calls,
//! temp dirs / runnable programs substituted for placeholders), or asserts
//! one documented CLI command/flag appears in the real help text. If the
//! README drifts from the code again, this file goes red first.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ratatui::widgets::Paragraph;
use tuisnap::snapshot::Store;
use tuisnap::{Profile, Provenance, VENDORED_FACES};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tuisnap"))
}

fn help(args: &[&str]) -> String {
    let out = Command::new(bin())
        .args(args)
        .output()
        .expect("spawn tuisnap help");
    assert!(out.status.success(), "help {args:?} failed");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn home_frame() -> tuisnap::Frame {
    tuisnap::ratatui::draw_frame(
        120,
        40,
        Provenance::now("tuisnap-default", "home", vec![]),
        |f| f.render_widget(Paragraph::new("home"), f.area()),
    )
}

// ---------------------------------------------------------------------------
// Quick start fence: pure view test through Store::check.
// ---------------------------------------------------------------------------

#[test]
fn readme_quickstart_pure_view() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(&tmp.path().join("visual"));
    let profile = Profile::default_profile();
    let frame = home_frame();
    // First run fails with missing-approval (fail-closed), actuals on disk.
    let outcome = store
        .check("home", &frame, &profile, &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(outcome.status.as_str(), "missing-approval");
    assert!(outcome.ensure_matched().is_err());
    assert!(outcome.actual_frame.is_file());
    assert!(outcome.actual_png.is_file());
    // Explicit accept, then the gate passes.
    store.accept("home").unwrap();
    let outcome = store
        .check("home", &frame, &profile, &VENDORED_FACES, 1.0)
        .unwrap();
    outcome.ensure_matched().unwrap();
}

// ---------------------------------------------------------------------------
// Bulk fence: profile.renderer + check_with + report_with.
// ---------------------------------------------------------------------------

#[test]
fn readme_bulk_renderer_reuse() {
    fn bulk(
        store: &tuisnap::snapshot::Store,
        profile: &tuisnap::Profile,
        frame: &tuisnap::Frame,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut renderer = profile.renderer(&tuisnap::VENDORED_FACES)?;
        let outcome = store.check_with(&mut renderer, "home", frame, 1.0)?;
        outcome.ensure_matched()?;
        let report = store.report_with(&mut renderer, 1.0, "my suite")?;
        assert_eq!(report.failed(), 0);
        Ok(())
    }

    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(&tmp.path().join("visual"));
    let profile = Profile::default_profile();
    let frame = home_frame();
    store.accept("home").expect_err("nothing to accept yet");
    // Seed approval, then the bulk fence passes end to end.
    let first = store
        .check("home", &frame, &profile, &VENDORED_FACES, 1.0)
        .unwrap();
    assert!(first.ensure_matched().is_err());
    store.accept("home").unwrap();
    bulk(&store, &profile, &frame).unwrap();
    assert!(store.root().join("report.html").is_file());
}

// ---------------------------------------------------------------------------
// Accept fence: store.accept + actual_names loop.
// ---------------------------------------------------------------------------

#[test]
fn readme_accept_flow() {
    fn accept_reviewed(
        store: &tuisnap::snapshot::Store,
    ) -> Result<(), tuisnap::snapshot::SnapshotError> {
        store.accept("home")?;
        for name in store.actual_names()? {
            store.accept(&name)?;
        }
        Ok(())
    }

    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(&tmp.path().join("visual"));
    let profile = Profile::default_profile();
    let frame = home_frame();
    store
        .check("home", &frame, &profile, &VENDORED_FACES, 1.0)
        .unwrap();
    accept_reviewed(&store).unwrap();
    let outcome = store
        .check("home", &frame, &profile, &VENDORED_FACES, 1.0)
        .unwrap();
    outcome.ensure_matched().unwrap();
}

// ---------------------------------------------------------------------------
// Interactive fence: Tui::new / wait_predicate / press / mouse_wheel /
// wait_stable / frame_from_screen / close. Mirrors the README statements
// against a runnable shell instead of ./my-tui.
// ---------------------------------------------------------------------------

#[test]
#[cfg(feature = "pty")]
fn readme_interactive_tui() {
    use std::time::{Duration, Instant};
    use tuisnap::tui::{CancelToken, Tui};

    // The documented chord shape parses: modifiers + special keys, +-joined.
    let (key, mods) = tuisnap::tui::parse_chord("ctrl+Up").unwrap();
    assert_eq!(key, tuisnap::tui::Key::Up);
    assert!(mods.ctrl);

    let mut s = Tui::new(["/bin/sh", "-c", "printf 'Ready\\n'; sleep 30"])
        .size(120, 40)
        .spawn()
        .unwrap();
    let cancel = CancelToken::new();
    let _obs = s
        .wait_predicate(
            |o| tuisnap::proto::screen_text(&o.screen).contains("Ready"),
            Instant::now() + Duration::from_secs(10),
            &cancel,
        )
        .unwrap();
    s.press("ctrl+Up").unwrap();
    s.send_text("hello").unwrap();
    // Mouse input without app-enabled reporting fails closed, never drops.
    assert!(matches!(
        s.mouse_wheel(
            tuisnap::tui::Wheel::Up,
            10,
            5,
            tuisnap::tui::MouseMods::NONE
        ),
        Err(tuisnap::tui::TuiError::ModeNotEnabled(_))
    ));
    let _settled = s
        .wait_stable(Instant::now() + Duration::from_secs(10), &cancel)
        .unwrap();
    let frame = tuisnap::assert::frame_from_screen(&s.snapshot().unwrap());
    assert_eq!((frame.cols, frame.rows), (120, 40));
    s.close().unwrap();
}

// ---------------------------------------------------------------------------
// API-map section: every named facade item resolves and behaves.
// ---------------------------------------------------------------------------

#[test]
fn readme_api_map_resolves() {
    // ratatui::render_screen -> Screen.
    let screen = tuisnap::ratatui::render_screen(
        30,
        5,
        |f| f.render_widget(Paragraph::new("alpha needle beta"), f.area()),
        tuisnap::ratatui::EdgePolicy::default(),
    )
    .unwrap()
    .into_screen();
    assert_eq!((screen.cols(), screen.rows()), (30, 5));

    // locate::Locator text + present/not-present.
    let obs = tuisnap::screen::Observation::new(
        screen.clone(),
        7,
        tuisnap::screen::CaptureReason::Manual,
        tuisnap::screen::TermState::default(),
        tuisnap::screen::CaptureProvenance::new(0, None, None, 0),
    );
    let span = tuisnap::locate::Locator::text("needle")
        .resolve_unique(&screen, 7)
        .unwrap();
    assert_eq!(span.text, "needle");
    assert!(tuisnap::locate::Locator::text("needle")
        .present_now(&obs)
        .unwrap());
    assert!(tuisnap::locate::Locator::text("no-such-text")
        .not_present_now(&obs)
        .unwrap());
    assert!(tuisnap::locate::Locator::regex("n.edle")
        .unwrap()
        .present_now(&obs)
        .unwrap());

    // command::Command piped run.
    let out = tuisnap::command::Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .run();
    assert!(out.success());
    assert_eq!(out.code(), Some(0));

    // proto typed ops + machine line.
    match tuisnap::proto::execute(&tuisnap::proto::Op::Version).unwrap() {
        tuisnap::proto::OpResult::Version { protocol, .. } => {
            assert_eq!(protocol, tuisnap::proto::PROTOCOL_VERSION)
        }
        other => panic!("expected Version, got {other:?}"),
    }
    assert_eq!(
        tuisnap::proto::capabilities().protocol,
        tuisnap::proto::PROTOCOL_VERSION
    );
    let (line, ok) = tuisnap::proto::run_machine_line(r#"{"type":"capabilities"}"#);
    assert!(ok);
    assert!(line.contains(r#""ok":true"#));
    let (bad_line, bad_ok) = tuisnap::proto::run_machine_line("not json");
    assert!(!bad_ok);
    assert!(bad_line.contains("invalid-input"));

    // runner::TestContext from an injected env (no global state).
    let tmp = tempfile::tempdir().unwrap();
    let ctx = tuisnap::runner::TestContext::from_map(
        "readme-lock",
        &std::collections::HashMap::new(),
        tmp.path(),
    )
    .unwrap();
    assert!(ctx.evidence_dir().is_dir());

    // assert helpers: sample render, generation binding, four-tree export.
    let sample = tuisnap::assert::render_sample(&screen).unwrap();
    let gen = tuisnap::assert::generation_id(&sample.canonical);
    assert_eq!(
        tuisnap::assert::png_generation(&tuisnap::assert::png_tag_generation(&sample.png, &gen)),
        Some(gen)
    );
    let paths = tuisnap::assert::emit_four(&screen, &tmp.path().join("four")).unwrap();
    assert!(paths.ansi.is_file() && paths.txt.is_file());
    assert!(paths.png.is_file() && paths.html.is_file());

    // observe + export + mcp.
    assert_eq!(
        tuisnap::observe::screen_text(&screen),
        tuisnap::proto::screen_text(&screen)
    );
    let cast =
        tuisnap::export::cast_v2(&[("hi".to_string(), 0.5)], 80, 24, &tmp.path().join("cast"))
            .unwrap();
    assert!(cast.is_file());
    assert!(!tuisnap::mcp::tools().is_empty());
    let list = tuisnap::mcp::tools_list_json();
    assert!(list
        .get("tools")
        .and_then(|t| t.as_array())
        .is_some_and(|t| !t.is_empty()));

    // Rendering pins.
    assert_eq!(tuisnap::VENDORED_FALLBACK_FACES.len(), 3);
    let profile = Profile::default_profile();
    assert_eq!((profile.cell_w, profile.cell_h), (10, 21));
    assert_eq!(profile.font_px, 16.0);
    assert_eq!(profile.scale, 2);
    assert_eq!(tuisnap::frame::FRAME_VERSION, 4);
    // Schema v4: styled underlines + underline color are canonical.
    use tuisnap::frame::UnderlineStyle;
    assert_eq!(UnderlineStyle::default(), UnderlineStyle::None);
    assert!(UnderlineStyle::Curly.is_some());
    assert_eq!(UnderlineStyle::Double.token(), "double-underline");
    let cell = tuisnap::frame::Cell::blank(0, 0);
    assert_eq!(cell.mods.underline, UnderlineStyle::None);
    assert_eq!(cell.underline_color, tuisnap::frame::Color::Default);
}

// ---------------------------------------------------------------------------
// Macro gates named in the README: assert_snapshot! / assert_screenshot!
// pass against pre-approved temp snapshots (insta pattern from 01/02).
// ---------------------------------------------------------------------------

#[test]
fn readme_macro_gates_pass_preapproved() {
    use tuisnap::assert::{
        evidence_dir, generation_id, png_tag_generation, render_sample, EVIDENCE_DIR_ENV,
        SNAPSHOT_DIR_ENV,
    };
    use tuisnap::insta_proto::insta_string;
    use tuisnap::ratatui::{render_screen, EdgePolicy};

    std::env::set_var("INSTA_UPDATE", "no");
    let tmp = tempfile::tempdir().unwrap();
    let snaps = tmp.path().join("snaps");
    std::fs::create_dir(&snaps).unwrap();
    std::env::set_var(SNAPSHOT_DIR_ENV, &snaps);
    std::env::set_var(EVIDENCE_DIR_ENV, tmp.path().join("evidence"));

    let screen = render_screen(
        20,
        4,
        |f| f.render_widget(Paragraph::new("readme lock"), f.area()),
        EdgePolicy::default(),
    )
    .unwrap()
    .into_screen();

    // Pre-approve the canonical text gate.
    let canonical = insta_string(&screen);
    let gen = generation_id(&canonical);
    std::fs::write(
        snaps.join("readme-lock.snap"),
        format!(
            "---\nsource: tests/readme_lock.rs\ndescription: tuisnap generation {gen}\n\
             expression: canonical\n---\n{canonical}"
        ),
    )
    .unwrap();
    tuisnap::assert_snapshot!("readme-lock", &screen);
    assert!(!snaps.join("readme-lock.snap.new").exists());

    // Pre-approve the compound screenshot gate (canonical + tagged PNG).
    let sample = render_sample(&screen).unwrap();
    assert_eq!(generation_id(&sample.canonical), gen);
    std::fs::write(
        snaps.join("readme-lock-shot.snap"),
        format!(
            "---\nsource: tests/readme_lock.rs\ndescription: tuisnap generation {gen}\n\
             expression: canonical\n---\n{}",
            sample.canonical
        ),
    )
    .unwrap();
    std::fs::write(
        snaps.join("readme-lock-shot-img.snap"),
        format!(
            "---\nsource: tests/readme_lock.rs\ndescription: tuisnap generation {gen}\n\
             expression: png_bytes\nextension: png\nsnapshot_kind: binary\n---\n"
        ),
    )
    .unwrap();
    std::fs::write(
        snaps.join("readme-lock-shot-img.snap.png"),
        png_tag_generation(&sample.png, &gen),
    )
    .unwrap();
    tuisnap::assert_screenshot!("readme-lock-shot", &screen);
    let dir = evidence_dir();
    assert!(dir.join("readme-lock-shot.png").is_file());
}

// ---------------------------------------------------------------------------
// Grouped fences: check_with + report_with + accept_all + root overrides.
// ---------------------------------------------------------------------------

#[test]
fn readme_grouped_store() {
    fn check_page(
        store: &tuisnap::grouped::GroupedStore,
        profile: &tuisnap::Profile,
        frame: &tuisnap::Frame,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut renderer = profile.renderer(&tuisnap::VENDORED_FACES)?;
        let outcome = store.check_with(&mut renderer, "pages/overview", frame, 1.0)?;
        outcome.ensure_matched()?;
        store.report_with(&mut renderer, 1.0, "my suite")?;
        Ok(())
    }

    let tmp = tempfile::tempdir().unwrap();
    let store = tuisnap::grouped::GroupedStore::new(&tmp.path().join("snapshots"));
    // Documented defaults live outside the approved tree.
    assert_eq!(store.actual_root(), tmp.path().join("snapshots.actual"));
    assert_eq!(store.diff_root(), tmp.path().join("snapshots.diff"));
    assert_eq!(
        store.report_path(),
        tmp.path().join("snapshots.actual").join("report.html")
    );
    // Overrides resolve.
    let custom = tuisnap::grouped::GroupedStore::new(&tmp.path().join("s2"))
        .with_actual_root(&tmp.path().join("a"))
        .with_diff_root(&tmp.path().join("d"))
        .with_report_path(&tmp.path().join("r.html"));
    assert_eq!(custom.actual_root(), tmp.path().join("a"));
    assert_eq!(custom.diff_root(), tmp.path().join("d"));
    assert_eq!(custom.report_path(), tmp.path().join("r.html"));

    let profile = Profile::default_profile();
    let frame = home_frame();
    let first = store
        .check("pages/overview", &frame, &profile, &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(first.status(), tuisnap::snapshot::Status::MissingApproval);

    // Bless recursively from Rust (the documented accept_all fence).
    let accepted = store.accept_all().unwrap();
    assert_eq!(accepted, vec!["pages/overview".to_string()]);
    // Approved tree holds exactly the four artifacts.
    for ext in ["ansi", "txt", "png", "html"] {
        assert!(
            tmp.path()
                .join(format!("snapshots/pages/overview.{ext}"))
                .is_file(),
            "missing approved .{ext}"
        );
    }
    assert!(
        !tmp.path()
            .join("snapshots/pages/overview.frame.json")
            .exists(),
        "approved tree must not hold .frame.json"
    );
    check_page(&store, &profile, &frame).unwrap();
    assert!(store.report_path().is_file());
}

// ---------------------------------------------------------------------------
// Font-fallback fence: FallbackFace + Renderer::with_fallbacks.
// ---------------------------------------------------------------------------

#[test]
fn readme_fallback_chain() {
    fn custom_chain(
        profile: &tuisnap::Profile,
    ) -> Result<tuisnap::Renderer, Box<dyn std::error::Error>> {
        let chain = [tuisnap::FallbackFace {
            bytes: tuisnap::VENDORED_SYMBOLS2_FONT,
            sha256: tuisnap::VENDORED_SYMBOLS2_FONT_SHA256,
            desc: "my extra symbols",
        }];
        Ok(tuisnap::render::Renderer::with_fallbacks(
            profile,
            &tuisnap::VENDORED_FACES,
            &chain,
        )?)
    }

    let profile = Profile::default_profile();
    let mut r = custom_chain(&profile).unwrap();
    let rendered = r.render(&home_frame()).unwrap();
    assert!(!rendered.png.is_empty());
    // A wrong pin refuses to render.
    let bad = tuisnap::render::Renderer::with_fallbacks(
        &profile,
        &VENDORED_FACES,
        &[tuisnap::FallbackFace {
            bytes: tuisnap::VENDORED_SYMBOLS2_FONT,
            sha256: &"0".repeat(64),
            desc: "bad pin",
        }],
    );
    assert!(bad.is_err());
}

// ---------------------------------------------------------------------------
// CLI section: every documented subcommand appears in --help; removed ones
// (accept/check/run) fail with usage error; flags match.
// ---------------------------------------------------------------------------

#[test]
fn readme_cli_help_lists_documented_subcommands() {
    let top = help(&["--help"]);
    for cmd in [
        "init", "doctor", "schema", "capture", "inspect", "render", "diff", "review", "report",
        "import", "session", "record", "trace",
    ] {
        assert!(top.contains(cmd), "--help missing {cmd}:\n{top}");
    }
    // Stale README commands must stay gone (usage error, exit 2).
    for stale in ["accept", "check", "run"] {
        let out = Command::new(bin())
            .arg(stale)
            .output()
            .expect("spawn tuisnap");
        assert_eq!(
            out.status.code(),
            Some(2),
            "`tuisnap {stale}` should be a usage error"
        );
    }
    // Documented flags per subcommand.
    let render = help(&["render", "--help"]);
    for flag in ["--input", "--format", "--out", "--font-file"] {
        assert!(render.contains(flag), "render --help missing {flag}");
    }
    let report = help(&["report", "--help"]);
    for flag in ["--dir", "--out", "--title"] {
        assert!(report.contains(flag), "report --help missing {flag}");
    }
    let session = help(&["session", "--help"]);
    for sub in ["start", "stop", "list", "prune", "attach"] {
        assert!(session.contains(sub), "session --help missing {sub}");
    }
    let cases: &[(&str, &[&str])] = &[
        ("capture", &["--out", "--timeout-ms"]),
        ("inspect", &["--dir"]),
        ("diff", &["--expected", "--actual"]),
        ("review", &["--dir"]),
        ("import", &["--dir"]),
        ("record", &["--out", "--max-events", "--max-bytes"]),
        ("trace", &["--input", "--kind"]),
        ("init", &["--dir", "--force"]),
    ];
    for &(cmd, flags) in cases {
        let text = help(&[cmd, "--help"]);
        for flag in flags {
            assert!(text.contains(flag), "{cmd} --help missing {flag}");
        }
    }
}

#[test]
fn readme_cli_render_and_diff_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("shot.frame.json");
    std::fs::write(&input, home_frame().to_json()).unwrap();
    // Documented render example: png + svg formats, --out prefix.
    let out = Command::new(bin())
        .args([
            "render",
            "--input",
            &input.to_string_lossy(),
            "--format",
            "png",
            "--format",
            "svg",
            "--out",
            &tmp.path().join("shot").to_string_lossy(),
        ])
        .output()
        .expect("spawn render");
    assert!(out.status.success(), "render failed: {out:?}");
    assert!(tmp.path().join("shot.png").is_file());
    assert!(tmp.path().join("shot.svg").is_file());
    assert!(tmp.path().join("shot.png.fidelity.json").is_file());

    // diff exit codes: 0 identical, 4 mismatch.
    let same = Command::new(bin())
        .args([
            "diff",
            "--expected",
            &tmp.path().join("shot.png").to_string_lossy(),
            "--actual",
            &tmp.path().join("shot.png").to_string_lossy(),
        ])
        .output()
        .expect("spawn diff");
    assert_eq!(same.status.code(), Some(0));
    let other = home_frame_diff_png(tmp.path());
    let mismatch = Command::new(bin())
        .args([
            "diff",
            "--expected",
            &tmp.path().join("shot.png").to_string_lossy(),
            "--actual",
            &other.to_string_lossy(),
        ])
        .output()
        .expect("spawn diff");
    assert_eq!(mismatch.status.code(), Some(4));
}

fn home_frame_diff_png(dir: &Path) -> PathBuf {
    let frame = tuisnap::ratatui::draw_frame(
        120,
        40,
        Provenance::now("tuisnap-default", "other", vec![]),
        |f| f.render_widget(Paragraph::new("other"), f.area()),
    );
    let profile = Profile::default_profile();
    let mut renderer = profile.renderer(&VENDORED_FACES).unwrap();
    let rendered = renderer.render(&frame).unwrap();
    let path = dir.join("other.png");
    std::fs::write(&path, &rendered.png).unwrap();
    assert_ne!(
        rendered.png,
        std::fs::read(dir.join("shot.png")).unwrap(),
        "frames must differ for the exit-4 lock"
    );
    path
}

#[test]
fn readme_cli_machine_and_offline_commands() {
    // tuisnap --machine < ops.jsonl : one envelope line, exit 0.
    let mut child = Command::new(bin())
        .arg("--machine")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn --machine");
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"type":"capabilities"}"#)
        .unwrap();
    let out = child.wait_with_output().expect("wait --machine");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(r#""ok":true"#), "machine out: {stdout}");

    // Offline review/report on an empty verdict dir: exit 0.
    let tmp = tempfile::tempdir().unwrap();
    let verdicts = tmp.path().join("verdicts");
    std::fs::create_dir(&verdicts).unwrap();
    let review = Command::new(bin())
        .args(["review", "--dir", &verdicts.to_string_lossy()])
        .output()
        .expect("spawn review");
    assert!(review.status.success());
    let report = tmp.path().join("report.html");
    let rep = Command::new(bin())
        .args([
            "report",
            "--dir",
            &verdicts.to_string_lossy(),
            "--out",
            &report.to_string_lossy(),
        ])
        .output()
        .expect("spawn report");
    assert!(rep.status.success());
    assert!(report.is_file());

    // doctor / schema run clean.
    assert!(Command::new(bin())
        .arg("doctor")
        .output()
        .unwrap()
        .status
        .success());
    let schema = Command::new(bin()).arg("schema").output().unwrap();
    assert!(schema.status.success());
    let schema_text = String::from_utf8_lossy(&schema.stdout).into_owned();
    assert!(
        schema_text.contains("tui-snap op protocol"),
        "schema: {schema_text:.120}"
    );
    assert!(schema_text.contains("capabilities"));
}

// ---------------------------------------------------------------------------
// Pinned facts: toolchain, examples, referenced test.
// ---------------------------------------------------------------------------

#[test]
fn readme_pinned_facts() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let toolchain = std::fs::read_to_string(manifest.join("rust-toolchain.toml")).unwrap();
    assert!(
        toolchain.contains("1.98.1"),
        "README pins toolchain 1.98.1: {toolchain}"
    );
    for n in 1..=8u32 {
        let name = format!(
            "examples/{n:02}-{}.rs",
            [
                "pure-view",
                "styled-shot",
                "piped-cli",
                "interactive-tui",
                "locators-waits",
                "artifacts-review",
                "advanced-profiles",
                "agent-workflow",
            ][n as usize - 1]
        );
        assert!(manifest.join(&name).is_file(), "missing {name}");
    }
    // The fidelity section cites this exact test name.
    let render_tests = std::fs::read_to_string(manifest.join("tests/render.rs")).unwrap();
    assert!(render_tests.contains("fn primary_covered_fixtures_match_pre_fallback_render_bytes"));
}
