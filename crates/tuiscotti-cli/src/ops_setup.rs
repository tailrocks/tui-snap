//! Setup commands: `init`, `doctor`, `schema`.
//!
//! Every stdout path goes through [`crate::write_stdout`]: no `println!`, so
//! a closed pipe is a clean exit 0 instead of an EPIPE panic (exit 101).

use std::path::Path;

use tuiscotti::proto::EXIT_OP_ERROR;

const SCAFFOLD_TOML: &str = r#"# tui-snap capture + assertion policy. Scheduling lives in
# .config/nextest.toml; review behaviour is Insta's.
[capture]
cols = 120
rows = 40
timeout_ms = 10000

[terminal]
term = "xterm-256color"

[render]
profile = "tuisnap-default"

[gates]
pixel_policy = "exact-decoded-rgba"
"#;

const SCAFFOLD_NEXTEST: &str = r#"# cargo-nextest scheduling only. tui-snap never parses this file.
[profile.default]
test-threads = "num-cpus"
"#;

const SCAFFOLD_TEST: &str = r#"// Example tui-snap visual test. Run: cargo nextest run --profile default
use tuiscotti::runner::TestContext;

#[test]
fn example_view() {
    let ctx = TestContext::current("example").expect("context");
    let screen = tuiscotti::Screen::blank(80, 24);
    let _ = ctx.evidence_dir();
    tuiscotti::assert_snapshot!("example_view", &screen);
}
"#;

pub(crate) fn cmd_init(dir: &Path, force: bool) -> i32 {
    let files: &[(&str, &str)] = &[
        ("tui-snap.toml", SCAFFOLD_TOML),
        (".config/nextest.toml", SCAFFOLD_NEXTEST),
        ("tests/visual.rs", SCAFFOLD_TEST),
    ];
    for (rel, _) in files {
        let path = dir.join(rel);
        if path.exists() && !force {
            eprintln!(
                "error: {} exists (pass --force to overwrite)",
                path.display()
            );
            return EXIT_OP_ERROR;
        }
    }
    let mut buf = String::new();
    for (rel, body) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            return crate::fail_flushed(&buf, &format!("mkdir {}: {e}", parent.display()));
        }
        if let Err(e) = std::fs::write(&path, body) {
            return crate::fail_flushed(&buf, &format!("write {}: {e}", path.display()));
        }
        crate::push_line(&mut buf, &format!("wrote {}", path.display()));
    }
    buf.push('\n');
    crate::push_line(&mut buf, tuiscotti::proto::CONFIG_DOCS);
    crate::write_stdout(&buf)
}

fn probe(program: &str, args: &[&str]) -> String {
    let out = tuiscotti::command::Command::new(program)
        .args(args)
        .timeout(std::time::Duration::from_secs(10))
        .run();
    if out.success() {
        out.stdout_lossy().lines().next().unwrap_or("?").to_string()
    } else {
        "missing".to_string()
    }
}

pub(crate) fn cmd_doctor() -> i32 {
    let profile = tuiscotti::Profile::default_profile();
    let caps = tuiscotti::proto::capabilities();
    let mut buf = format!(
        "tuisnap {}\nprotocol v{}\n\n[toolchain]\n  rustc: {}\n  cargo: {}\n  nextest: {}\n\n[fonts]\n  regular sha256: {}\n  fallback faces: {}\n\n[profile]\n  {}: cell {}x{} font_px {} scale {} pad {}\n\n[platform]\n  os: {}\n  pty: {}\n\n[env]\n",
        env!("CARGO_PKG_VERSION"),
        tuiscotti::proto::PROTOCOL_VERSION,
        probe("rustc", &["--version"]),
        probe("cargo", &["--version"]),
        probe("cargo", &["nextest", "--version"]),
        tuiscotti::profile::font_sha256(tuiscotti::VENDORED_FONT),
        tuiscotti::VENDORED_FALLBACK_FACES.len(),
        profile.name,
        profile.cell_w,
        profile.cell_h,
        profile.font_px,
        profile.scale,
        profile.pad,
        caps.platform,
        caps.pty,
    );
    for key in [
        "TERM",
        "CI",
        "NEXTEST_PROFILE",
        "TUISNAP_RUNTIME_DIR",
        "TUISNAP_EVIDENCE_DIR",
        "TUISNAP_SNAPSHOT_DIR",
    ] {
        match std::env::var(key) {
            Ok(v) => crate::push_line(&mut buf, &format!("  {key}={v}")),
            Err(_) => crate::push_line(&mut buf, &format!("  {key}=(unset)")),
        }
    }
    crate::write_stdout(&buf)
}

pub(crate) fn cmd_schema() -> i32 {
    crate::write_stdout(&format!("{}\n", tuiscotti::proto::PROTOCOL_SCHEMA_JSON))
}
