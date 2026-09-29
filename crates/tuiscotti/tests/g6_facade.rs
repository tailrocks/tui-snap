//! G6 daily-API tests: the proposed example shapes through the public facade.
//!
//! Everything here uses only top-level `tuiscotti::` names plus the standard
//! `Policy`/`ratatui` paths — the same surface an external consumer crate
//! sees (see also the out-of-workspace consumer check in `/tmp/w2-g6.md`).
//! Hermetic tempdirs via [`tuiscotti::Policy::EvolvingIn`]; approvals are
//! pre-written so green-path assertions hold under every `INSTA_UPDATE` mode.

use std::error::Error as _;
use std::fs;
use std::path::Path;

use tuiscotti::Policy;
use tuiscotti::assert::{generation_id, png_tag_generation, render_sample};
use tuiscotti::insta_proto::insta_string;

fn write_text_snap(
    dir: &Path,
    name: &str,
    generation: &str,
    body: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = format!(
        "---\nsource: tests/g6_facade.rs\ndescription: tuiscotti generation {generation}\nexpression: canonical\n---\n{body}"
    );
    Ok(fs::write(dir.join(format!("{name}.snap")), content)?)
}

fn write_binary_snap(
    dir: &Path,
    name: &str,
    generation: &str,
    sidecar: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let meta = format!(
        "---\nsource: tests/g6_facade.rs\ndescription: tuiscotti generation {generation}\nexpression: png_bytes\nextension: png\nsnapshot_kind: binary\n---\n"
    );
    fs::write(dir.join(format!("{name}.snap")), meta)?;
    Ok(fs::write(dir.join(format!("{name}.snap.png")), sidecar)?)
}

fn hermetic_policy(snaps: &Path, evidence: &Path) -> Policy {
    Policy::EvolvingIn {
        snapshots: snaps.to_path_buf(),
        evidence: evidence.to_path_buf(),
    }
}

struct Clipper;
impl ratatui::widgets::Widget for Clipper {
    fn render(self, area: ratatui::layout::Rect, buf: &mut ratatui::buffer::Buffer) {
        buf[(area.width - 1, 0)].set_symbol("漢");
    }
}
fn render_clipped() -> tuiscotti::Result<tuiscotti::Screen> {
    Ok(tuiscotti::ratatui::render((10, 3), |frame| {
        frame.render_widget(Clipper, frame.area());
    })?)
}

#[test]
fn pure_view_matches_proposed_shape() {
    // Proposed shape:
    //   let screen = tuiscotti::ratatui::render((100, 30), |frame| {
    //       fixtures::render_settings(frame, &model);
    //   })?;
    //   tuiscotti::assert_screenshot!("settings", &screen);
    let screen = tuiscotti::ratatui::render((48, 12), |frame| {
        use ratatui::widgets::{Block, Paragraph};
        frame.render_widget(
            Paragraph::new("Ready").block(Block::bordered().title("Settings")),
            frame.area(),
        );
    })
    .expect("render settings view succeeds");

    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let snaps = tmp.path().join("snaps");
    let evidence = tmp.path().join("evidence");
    fs::create_dir(&snaps).expect("fs::create_dir snaps succeeds");
    fs::create_dir(&evidence).expect("fs::create_dir evidence succeeds");
    let canonical = insta_string(&screen);
    let generation = generation_id(&canonical);
    let sample = render_sample(&screen).expect("render_sample succeeds");
    write_text_snap(&snaps, "g6_settings", &generation, &canonical)
        .expect("write_text_snap succeeds");
    write_binary_snap(
        &snaps,
        "g6_settings-img",
        &generation,
        &png_tag_generation(&sample.png, &generation),
    )
    .expect("write_binary_snap succeeds");

    let policy = hermetic_policy(&snaps, &evidence);
    tuiscotti::assert_screenshot!("g6_settings", &screen, &policy);
    // Same-sample evidence landed before any failure could occur.
    for ext in ["png", "ansi", "txt", "html"] {
        assert!(
            evidence.join(format!("g6_settings.{ext}")).is_file(),
            "missing g6_settings.{ext}"
        );
    }
}

#[test]
#[cfg(feature = "pty")]
fn live_session_matches_proposed_shape() {
    // Proposed shape (adjusted: `expect_exit` takes an explicit Duration —
    // a bare `expect_exit()` would hide the timeout policy; `mut` only for
    // the `&mut self` close):
    //   let mut app = tuiscotti::Tui::cargo_bin("menu-fixture")?
    //       .size(100, 30)
    //       .spawn()?;
    //   app.get_by_text("Ready").expect_visible()?;
    //   app.press("Ctrl+P")?;
    //   app.get_by_text("Settings").click()?;
    //   let screen = app.snapshot()?;
    //   tuiscotti::assert_screenshot!("settings-open", &screen);
    //   app.press("q")?;
    //   app.expect_exit().success()?;
    use std::time::Duration;
    let mut app = tuiscotti::Tui::new(["/bin/sh", "-c", "printf 'Ready\\n'; sleep 30"])
        .size(80, 24)
        .spawn()
        .expect("spawn sh succeeds");
    app.wait_stable_timeout(Duration::from_secs(10))
        .expect("wait_stable succeeds");
    let span = app
        .get_by_text("Ready")
        .expect_visible()
        .expect("expect_visible succeeds");
    assert!(span.text.contains("Ready"), "span: {span}");
    // Scoped composition through the same bound API.
    let scoped = tuiscotti::Locator::within(
        tuiscotti::Locator::region(0, 0, 80, 24),
        tuiscotti::Locator::text("Ready".to_string()),
    );
    app.get_by(scoped)
        .expect_visible()
        .expect("scoped expect_visible succeeds");
    // Harmless input: Enter submits an empty command to sh.
    app.press("Enter").expect("press Enter succeeds");
    app.wait_stable_timeout(Duration::from_secs(10))
        .expect("wait_stable succeeds");
    let screen = app.snapshot().expect("snapshot succeeds");
    let text: String = screen.cells().iter().map(|c| c.symbol.clone()).collect();
    assert!(text.contains("Ready"), "live screen shows Ready");
    // sh never enables mouse reporting: the click is refused with a typed
    // session error, never delivered blindly.
    let err = app
        .get_by_text("Ready")
        .click()
        .expect_err("click without mouse is an error");
    assert!(
        matches!(
            err,
            tuiscotti::ActionError::Session(tuiscotti::TuiError::ModeNotEnabled(_))
        ),
        "unexpected click error: {err:?}"
    );
    app.close().expect("close succeeds");
}

#[test]
#[cfg(feature = "pty")]
fn cargo_bin_missing_is_a_typed_error() {
    // `Tui::cargo_bin(..)?` resolves eagerly: failure surfaces here with the
    // locations tried, not deferred to `spawn()`.
    let err = tuiscotti::Tui::cargo_bin("tuiscotti-no-such-bin-xyz")
        .expect_err("missing bin is an error");
    let msg = err.to_string();
    assert!(msg.contains("tuiscotti-no-such-bin-xyz"), "{msg}");
}

#[test]
fn piped_process_output_is_truthful() {
    // Nonzero exit is data; raw streams stay byte-exact.
    let out = tuiscotti::Command::new("/bin/sh")
        .args(["-c", "printf 'out'; printf 'err' >&2; exit 3"])
        .run();
    assert_eq!(out.status, tuiscotti::Termination::Exit(3));
    assert_eq!(out.code(), Some(3));
    assert_eq!(out.stdout_str().expect("stdout_str succeeds"), "out");
    assert_eq!(out.stderr_str().expect("stderr_str succeeds"), "err");

    // Fallible UTF-8 views fail loudly on raw bytes; the lossy access is
    // explicitly named and never used in equality.
    let raw = tuiscotti::Command::new("/bin/sh")
        .args(["-c", "printf '\\377'"])
        .run();
    assert!(raw.success());
    assert!(raw.stdout_str().is_err());
    assert_eq!(raw.stdout, vec![0o377]);
    assert!(raw.stdout_lossy().contains('\u{FFFD}'));
}

#[test]
#[cfg(feature = "pty")]
fn typed_keys_parse_from_str() {
    use std::str::FromStr;
    let chord = tuiscotti::KeyChord::from_str("Ctrl+P").expect("parse Ctrl+P succeeds");
    assert_eq!(chord.key, tuiscotti::Key::Char('P'));
    assert_eq!(chord.mods, tuiscotti::KeyMods::CTRL);
    assert_eq!(chord.to_string(), "Ctrl+P");
    assert_eq!(
        tuiscotti::Key::from_str("Enter").expect("parse Enter succeeds"),
        tuiscotti::Key::Enter
    );
    // Modifiers are rejected for bare keys: parse a chord instead.
    assert!(tuiscotti::Key::from_str("Ctrl+P").is_err());
    // From for the infallible direction.
    let bare = tuiscotti::KeyChord::from(tuiscotti::Key::Enter);
    assert_eq!(bare.mods, tuiscotti::KeyMods::NONE);
}

#[test]
fn debug_redacts_secrets() {
    let cmd = tuiscotti::Command::new("deploy")
        .env("TOKEN", "super-secret-value")
        .stdin("super-secret-body".as_bytes().to_vec());
    let rendered = format!("{cmd:?}");
    assert!(!rendered.contains("super-secret"), "{rendered}");
    assert!(rendered.contains("TOKEN"), "{rendered}");
    #[cfg(feature = "pty")]
    {
        let tui = tuiscotti::Tui::new(["app"]).env("TOKEN", "super-secret-value");
        let rendered = format!("{tui:?}");
        assert!(!rendered.contains("super-secret"), "{rendered}");
    }
}

#[test]
fn screen_conversions_use_try_from_and_fail_loudly_on_clips() {
    // TryFrom for the fallible Frame -> Screen direction.
    let frame = tuiscotti::assert::frame_from_screen(&tuiscotti::Screen::blank(80, 24));
    let screen = tuiscotti::Screen::try_from(&frame).expect("Screen::try_from succeeds");
    assert_eq!((screen.cols(), screen.rows()), (80, 24));

    // The simple `render` path fails on edge clips instead of substituting
    // silently (production widgets skip unfitting wide glyphs, so the test
    // widget writes the row-end cell directly); the error converts into the
    // facade `Error` via `?`.
    let err = render_clipped().expect_err("clipped render is an error");
    assert!(matches!(err, tuiscotti::Error::Screen(_)), "{err:?}");
    assert!(err.source().is_some());
}

#[test]
fn facade_error_retains_sources() {
    let err = tuiscotti::Error::from(
        tuiscotti::command::cargo_bin_path("x").expect_err("cargo_bin_path is an error"),
    );
    assert!(matches!(err, tuiscotti::Error::Spawn(_)));
    assert!(err.source().is_some());
    let io_err = tuiscotti::Error::from(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
    assert_eq!(io_err.to_string(), "I/O error: gone");
}
