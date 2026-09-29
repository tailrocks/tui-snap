//! Setup commands: `init`, `doctor`, `schema`.

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

pub fn cmd_init(dir: &Path, force: bool) -> i32 {
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
    for (rel, body) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("error: mkdir {}: {e}", parent.display());
                return EXIT_OP_ERROR;
            }
        }
        if let Err(e) = std::fs::write(&path, body) {
            eprintln!("error: write {}: {e}", path.display());
            return EXIT_OP_ERROR;
        }
        println!("wrote {}", path.display());
    }
    println!();
    println!("{}", tuiscotti::proto::CONFIG_DOCS);
    0
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

pub fn cmd_doctor() -> i32 {
    let profile = tuiscotti::Profile::default_profile();
    let caps = tuiscotti::proto::capabilities();
    println!("tuisnap {}", env!("CARGO_PKG_VERSION"));
    println!("protocol v{}", tuiscotti::proto::PROTOCOL_VERSION);
    println!();
    println!("[toolchain]");
    println!("  rustc: {}", probe("rustc", &["--version"]));
    println!("  cargo: {}", probe("cargo", &["--version"]));
    println!("  nextest: {}", probe("cargo", &["nextest", "--version"]));
    println!();
    println!("[fonts]");
    println!(
        "  regular sha256: {}",
        tuiscotti::profile::font_sha256(tuiscotti::VENDORED_FONT)
    );
    println!(
        "  fallback faces: {}",
        tuiscotti::VENDORED_FALLBACK_FACES.len()
    );
    println!();
    println!("[profile]");
    println!(
        "  {}: cell {}x{} font_px {} scale {} pad {}",
        profile.name, profile.cell_w, profile.cell_h, profile.font_px, profile.scale, profile.pad
    );
    println!();
    println!("[platform]");
    println!("  os: {}", caps.platform);
    println!("  pty: {}", caps.pty);
    println!();
    println!("[env]");
    for key in [
        "TERM",
        "CI",
        "NEXTEST_PROFILE",
        "TUISNAP_RUNTIME_DIR",
        "TUISNAP_EVIDENCE_DIR",
        "TUISNAP_SNAPSHOT_DIR",
    ] {
        match std::env::var(key) {
            Ok(v) => println!("  {key}={v}"),
            Err(_) => println!("  {key}=(unset)"),
        }
    }
    0
}

pub fn cmd_schema() -> i32 {
    println!("{}", tuiscotti::proto::PROTOCOL_SCHEMA_JSON);
    0
}
