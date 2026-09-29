//! README CLI surface + pinned facts (split from `readme_lock.rs`; shared helpers live in the root).

use super::{bin, help, home_frame};
use ratatui::widgets::Paragraph;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tuiscotti::{Profile, Provenance, VENDORED_FACES};

// ---------------------------------------------------------------------------
// CLI section: every documented subcommand appears in --help; removed ones
// (check/run) fail with usage error; flags match.
// ---------------------------------------------------------------------------

#[test]
fn readme_cli_help_lists_documented_subcommands() {
    let top = help(&["--help"]).expect("help text");
    for cmd in [
        "init", "doctor", "schema", "capture", "inspect", "render", "diff", "review", "accept",
        "report", "import", "session", "record", "trace", "machine",
    ] {
        assert!(top.contains(cmd), "--help missing {cmd}:\n{top}");
    }
    // Stale README commands must stay gone (usage error, exit 2).
    for stale in ["check", "run"] {
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
    let render = help(&["render", "--help"]).expect("help text");
    for flag in ["--input", "--format", "--out", "--font-file"] {
        assert!(render.contains(flag), "render --help missing {flag}");
    }
    let report = help(&["report", "--help"]).expect("help text");
    for flag in ["--dir", "--out", "--title"] {
        assert!(report.contains(flag), "report --help missing {flag}");
    }
    let session = help(&["session", "--help"]).expect("help text");
    for sub in ["start", "stop", "list", "prune", "attach"] {
        assert!(session.contains(sub), "session --help missing {sub}");
    }
    let cases: &[(&str, &[&str])] = &[
        ("capture", &["--out", "--timeout-ms"]),
        ("inspect", &["--dir"]),
        ("diff", &["--expected", "--actual"]),
        ("review", &["--dir"]),
        ("accept", &["--store"]),
        ("import", &["--dir"]),
        ("record", &["--out", "--max-events", "--max-bytes"]),
        ("trace", &["--input", "--kind"]),
        ("init", &["--dir", "--force"]),
    ];
    for &(cmd, flags) in cases {
        let text = help(&[cmd, "--help"]).expect("help text");
        for flag in flags {
            assert!(text.contains(flag), "{cmd} --help missing {flag}");
        }
    }
}

#[test]
fn readme_cli_render_and_diff_round_trip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = tmp.path().join("shot.frame.json");
    std::fs::write(&input, home_frame().to_json()).expect("write frame");
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
    let other = home_frame_diff_png(tmp.path()).expect("diff png");
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

fn home_frame_diff_png(dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let frame = tuiscotti::ratatui::draw_frame(
        120,
        40,
        Provenance::now("tuisnap-default", "other", vec![]),
        |f| f.render_widget(Paragraph::new("other"), f.area()),
    );
    let profile = Profile::default_profile();
    let mut renderer = profile.renderer(&VENDORED_FACES)?;
    let raster = renderer.render(&frame)?;
    let path = dir.join("other.png");
    std::fs::write(&path, &raster.png)?;
    assert_ne!(
        raster.png,
        std::fs::read(dir.join("shot.png"))?,
        "frames must differ for the exit-4 lock"
    );
    Ok(path)
}

#[test]
fn readme_cli_machine_and_offline_commands() {
    // tuisnap machine < ops.jsonl : one envelope line, exit 0.
    let mut child = Command::new(bin())
        .arg("machine")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn machine");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(br#"{"type":"capabilities"}"#)
        .expect("write op");
    let out = child.wait_with_output().expect("wait machine");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(r#""ok":true"#), "machine out: {stdout}");

    // Offline review/report on an empty verdict dir: exit 0.
    let tmp = tempfile::tempdir().expect("tempdir");
    let verdicts = tmp.path().join("verdicts");
    std::fs::create_dir(&verdicts).expect("mkdir");
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
    assert!(
        Command::new(bin())
            .arg("doctor")
            .output()
            .expect("run doctor")
            .status
            .success()
    );
    let schema = Command::new(bin())
        .arg("schema")
        .output()
        .expect("run schema");
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
    // This test lives in crates/tuiscotti-cli; pinned facts live at the workspace root.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..");
    let toolchain =
        std::fs::read_to_string(root.join("rust-toolchain.toml")).expect("read toolchain");
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
        assert!(
            root.join("crates/tuiscotti").join(&name).is_file(),
            "missing {name}"
        );
    }
    // The approved-PNG spot test keeps its pinned name (renamed in the g5
    // render-baseline removal; no doc cites the old name anymore). It lives
    // in the `fallback` split part of the render suite.
    let render_tests =
        std::fs::read_to_string(root.join("crates/tuiscotti-fixtures/tests/render/fallback.rs"))
            .expect("read render tests");
    assert!(render_tests.contains("fn approved_pngs_are_exactly_what_the_current_renderer_emits"));
}
