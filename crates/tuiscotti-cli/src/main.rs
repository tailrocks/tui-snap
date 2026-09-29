//! `tuisnap`: capture, inspect, sessions, render, diff, review/report.
//!
//! ```text
//! tuisnap init --dir .                  # scaffold tui-snap.toml + nextest config + example
//! tuisnap doctor                         # toolchain / fonts / profile / env report
//! tuisnap schema                         # print the op-protocol JSON schema
//! tuisnap capture --out shots/home -- ./my-tui --flag
//! tuisnap inspect --dir shots/home      # offline view; never executes
//! tuisnap render --input shot.frame.json --format png --out shot
//! tuisnap diff --expected a.png --actual b.png
//! tuisnap review --dir verdicts          # list verdicts; fails on any fail
//! tuisnap accept --store shots home      # approve one snapshot (explicit, per-name)
//! tuisnap report --dir verdicts --out report.html
//! tuisnap import --dir frozen           # read-only frozen-tree import
//! tuisnap session start --name demo -- ./my-tui
//! tuisnap record --out trace.jsonl -- ./my-tui
//! tuisnap trace --input trace.jsonl
//! tuisnap --machine < ops.jsonl         # typed op protocol over stdio
//! ```
//!
//! Exit statuses: 0 ok; 2 CLI usage error; 3 op error
//! ([`tuiscotti::proto::EXIT_OP_ERROR`]); 4 verification disagreement
//! ([`tuiscotti::proto::EXIT_VERIFY_FAIL`]). `capture`/`record` preserve the
//! child's exit code instead.

use clap::{Parser, Subcommand};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use tuiscotti::proto::{EXIT_OP_ERROR, EXIT_VERIFY_FAIL};

/// Config responsibilities (kept in sync with `proto::CONFIG_DOCS`).
const INIT_HELP: &str = "\
Scaffold tui-snap.toml, nextest config, and an example test.

Config responsibilities:
  tui-snap.toml        Capture + assertion policy (viewport, terminal and
                       render profiles, gates, evidence dir). Owned by
                       tui-snap; read by tests via the Rust API.
  .config/nextest.toml Scheduling only (profiles, retries, threads, groups).
                       Owned by cargo-nextest; tui-snap never parses it.
  insta config         Snapshot review behaviour. Owned by Insta; tui-snap
                       honours it and never auto-accepts in CI.";

// NOTE: no global flags. `--machine` is pre-scanned out of argv before clap
// sees it, so normal usage/error text stays exactly `Usage: tuisnap <COMMAND>`
// (pinned by tests/vertical_slice.rs).
#[derive(Parser, Debug)]
#[command(
    name = "tuisnap",
    version,
    about = "TUI visual regression: capture, inspect, sessions, render, diff, review"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Scaffold tui-snap.toml, nextest config, and an example test.
    #[command(long_about = INIT_HELP)]
    Init {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Report toolchain, fonts, profiles, and environment.
    Doctor,
    /// Print the typed op-protocol JSON schema.
    Schema,
    /// Run a command and collect artifacts (preserves child exit code).
    Capture {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 60_000)]
        timeout_ms: u64,
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// View artifacts offline. Never executes anything in the directory.
    Inspect {
        #[arg(long)]
        dir: PathBuf,
    },
    /// Render a canonical frame.json to offline artifacts.
    Render {
        #[arg(long)]
        input: PathBuf,
        #[arg(long = "format")]
        formats: Vec<String>,
        #[arg(long, default_value = "shot")]
        out: String,
        #[arg(long)]
        font_file: Option<PathBuf>,
    },
    /// Compare two PNGs by decoded pixels (exit 4 on mismatch).
    Diff {
        #[arg(long)]
        expected: PathBuf,
        #[arg(long)]
        actual: PathBuf,
    },
    /// List offline verdicts (exit 4 when any verdict fails).
    Review {
        #[arg(long)]
        dir: PathBuf,
    },
    /// Approve one snapshot: actual → approved (explicit, per-name only;
    /// frozen roots reject).
    Accept {
        /// Snapshot name (e.g. `home`, `pages/overview`).
        name: String,
        /// Snapshot store root (holds `approved/` + `actual/`).
        #[arg(long, default_value = ".")]
        store: PathBuf,
    },
    /// Write a standalone offline HTML report from verdicts.
    Report {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "tuisnap visual report")]
        title: String,
    },
    /// Read-only import of a frozen four-artifact tree (writes nothing).
    Import {
        #[arg(long)]
        dir: PathBuf,
    },
    /// Manage named sessions (versioned endpoints, owner-only runtime dir).
    Session {
        #[command(subcommand)]
        cmd: SessionCmd,
    },
    /// Run a command with bounded event recording (preserves exit code).
    Record {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 10_000)]
        max_events: u64,
        #[arg(long, default_value_t = 10_000_000)]
        max_bytes: u64,
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// View a recorded journal offline.
    Trace {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        kind: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum SessionCmd {
    /// Start a named session (detached child + endpoint file).
    Start {
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t = false)]
        force: bool,
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// Stop a named session and remove its endpoint.
    Stop {
        #[arg(long)]
        name: String,
    },
    /// List named sessions with liveness.
    List,
    /// Remove endpoints whose process already exited.
    Prune,
    /// Attach to a named session (best-effort human view; EOF detaches).
    Attach {
        #[arg(long)]
        name: String,
    },
}

fn main() {
    // Pre-scan `--machine` so clap's usage text never mentions it.
    let mut argv: Vec<String> = std::env::args().collect();
    let machine = extract_flag(&mut argv, "--machine");
    if machine {
        std::process::exit(machine_main());
    }
    let cli = match Cli::try_parse_from(&argv) {
        Ok(cli) => cli,
        Err(e) => e.exit(), // clap usage error, exit 2
    };
    std::process::exit(run(cli));
}

/// Remove all occurrences of `flag` from `argv`; return whether any was found.
fn extract_flag(argv: &mut Vec<String>, flag: &str) -> bool {
    let before = argv.len();
    argv.retain(|a| a != flag);
    argv.len() != before
}

/// Machine mode: Op JSON per line on stdin, envelope JSON per line on stdout.
/// Exit 0 when every op succeeded, else [`EXIT_OP_ERROR`].
fn machine_main() -> i32 {
    let stdin = std::io::stdin();
    let mut all_ok = true;
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("stdin: {e}");
                return EXIT_OP_ERROR;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let (out, ok) = tuiscotti::proto::run_machine_line(&line);
        println!("{out}");
        all_ok &= ok;
    }
    if all_ok {
        0
    } else {
        EXIT_OP_ERROR
    }
}

fn run(cli: Cli) -> i32 {
    match cli.cmd {
        Cmd::Init { dir, force } => cmd_init(&dir, force),
        Cmd::Doctor => cmd_doctor(),
        Cmd::Schema => cmd_schema(),
        Cmd::Capture {
            out,
            timeout_ms,
            argv,
        } => cmd_capture(&out, timeout_ms, argv),
        Cmd::Inspect { dir } => cmd_inspect(&dir),
        Cmd::Render {
            input,
            formats,
            out,
            font_file,
        } => cmd_render(&input, &formats, &out, font_file.as_deref()),
        Cmd::Diff { expected, actual } => cmd_diff(&expected, &actual),
        Cmd::Review { dir } => cmd_review(&dir),
        Cmd::Accept { name, store } => cmd_accept(&store, &name),
        Cmd::Report { dir, out, title } => cmd_report(&dir, &out, &title),
        Cmd::Import { dir } => cmd_import(&dir),
        Cmd::Session { cmd } => cmd_session(cmd),
        Cmd::Record {
            out,
            max_events,
            max_bytes,
            argv,
        } => cmd_record(&out, max_events, max_bytes, argv),
        Cmd::Trace { input, kind } => cmd_trace(&input, kind.as_deref()),
    }
}

fn op_error(e: &tuiscotti::proto::OpError) -> i32 {
    eprintln!("error: {e}");
    EXIT_OP_ERROR
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

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

fn cmd_init(dir: &Path, force: bool) -> i32 {
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

// ---------------------------------------------------------------------------
// doctor / schema
// ---------------------------------------------------------------------------

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

fn cmd_doctor() -> i32 {
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

fn cmd_schema() -> i32 {
    println!("{}", tuiscotti::proto::PROTOCOL_SCHEMA_JSON);
    0
}

// ---------------------------------------------------------------------------
// capture (exit-code preserving)
// ---------------------------------------------------------------------------

fn cmd_capture(out: &Path, timeout_ms: u64, argv: Vec<String>) -> i32 {
    let argv: Vec<String> = argv.into_iter().filter(|a| a != "--").collect();
    if argv.is_empty() {
        eprintln!("error: pass the command after `--`");
        return EXIT_OP_ERROR;
    }
    if let Err(e) = std::fs::create_dir_all(out) {
        eprintln!("error: mkdir {}: {e}", out.display());
        return EXIT_OP_ERROR;
    }
    let result = tuiscotti::command::Command::new(&argv[0])
        .args(&argv[1..])
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .run();
    if let Err(e) = std::fs::write(out.join("stdout.bin"), &result.stdout) {
        eprintln!("error: write stdout.bin: {e}");
        return EXIT_OP_ERROR;
    }
    if let Err(e) = std::fs::write(out.join("stderr.bin"), &result.stderr) {
        eprintln!("error: write stderr.bin: {e}");
        return EXIT_OP_ERROR;
    }
    let manifest = serde_json::json!({
        "argv": argv,
        "termination": format!("{:?}", result.status),
        "code": result.code(),
        "signal": result.signal(),
        "truncated": result.truncated,
        "elapsed_ms": result.elapsed.as_millis() as u64,
        "stdout_bytes": result.stdout.len(),
        "stderr_bytes": result.stderr.len(),
    });
    if let Err(e) = std::fs::write(
        out.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    ) {
        eprintln!("error: write manifest.json: {e}");
        return EXIT_OP_ERROR;
    }
    println!("captured {:?} -> {}", result.status, out.display());
    match result.status {
        tuiscotti::command::Termination::Exit(c) => c,
        _ => EXIT_OP_ERROR,
    }
}

// ---------------------------------------------------------------------------
// inspect (offline only: no Command, no spawn, no import execution)
// ---------------------------------------------------------------------------

fn cmd_inspect(dir: &Path) -> i32 {
    let entries = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: read {}: {e}", dir.display());
            return EXIT_OP_ERROR;
        }
    };
    let mut files: Vec<(String, u64)> = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("error: list {}: {e}", dir.display());
                return EXIT_OP_ERROR;
            }
        };
        let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
        files.push((entry.file_name().to_string_lossy().into_owned(), len));
    }
    files.sort();
    println!(
        "artifacts in {} ({} files, offline view):",
        dir.display(),
        files.len()
    );
    for (name, len) in &files {
        println!("  {name} ({len} bytes)");
    }
    let manifest_path = dir.join("manifest.json");
    if manifest_path.is_file() {
        match std::fs::read_to_string(&manifest_path) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(v) => println!(
                    "manifest: {}",
                    serde_json::to_string(&v).unwrap_or_else(|_| text.clone())
                ),
                Err(_) => println!("manifest: (not JSON, {} bytes)", text.len()),
            },
            Err(e) => {
                eprintln!("error: read manifest.json: {e}");
                return EXIT_OP_ERROR;
            }
        }
    }
    let journal_path = dir.join("journal.jsonl");
    if journal_path.is_file() {
        match tuiscotti::proto::read_journal(&journal_path) {
            Ok(events) => println!("journal: {} events", events.len()),
            Err(e) => return op_error(&e),
        }
    }
    0
}

// ---------------------------------------------------------------------------
// render (offline)
// ---------------------------------------------------------------------------

fn cmd_render(input: &Path, formats: &[String], out: &str, font_file: Option<&Path>) -> i32 {
    if formats.is_empty() {
        eprintln!("error: pass at least one --format (txt|ansi|json|svg|html|png)");
        return EXIT_OP_ERROR;
    }
    let text = match std::fs::read_to_string(input) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: read {}: {e}", input.display());
            return EXIT_OP_ERROR;
        }
    };
    let frame = match tuiscotti::Frame::from_json(&text) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: bad frame JSON: {e}");
            return EXIT_OP_ERROR;
        }
    };
    let mut profile = tuiscotti::Profile::default_profile();
    let owned;
    let faces;
    if let Some(path) = font_file {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("error: read font {}: {e}", path.display());
                return EXIT_OP_ERROR;
            }
        };
        profile = profile.with_font_file(path.display().to_string(), &bytes);
        owned = [bytes.clone(), bytes.clone(), bytes.clone(), bytes];
        faces = tuiscotti::FontFaces {
            regular: owned[0].as_slice(),
            bold: owned[1].as_slice(),
            italic: owned[2].as_slice(),
            bold_italic: owned[3].as_slice(),
        };
    } else {
        faces = tuiscotti::FontFaces {
            regular: tuiscotti::VENDORED_FONT,
            bold: tuiscotti::VENDORED_FONT_BOLD,
            italic: tuiscotti::VENDORED_FONT_ITALIC,
            bold_italic: tuiscotti::VENDORED_FONT_BOLD_ITALIC,
        };
    }
    let mut renderer: Option<tuiscotti::Renderer> = None;
    // Lazily constructed and reused across formats (faces parse once).
    macro_rules! get_renderer {
        () => {{
            if renderer.is_none() {
                renderer =
                    Some(tuiscotti::Renderer::new(&profile, &faces).map_err(|e| e.to_string())?);
            }
            renderer.as_mut().expect("constructed above")
        }};
    }
    for f in formats {
        let path = format!("{out}.{f}");
        if let Some(parent) = Path::new(&path).parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("error: mkdir {}: {e}", parent.display());
                    return EXIT_OP_ERROR;
                }
            }
        }
        let write_result = match f.as_str() {
            "txt" => std::fs::write(&path, frame.text()).map_err(|e| e.to_string()),
            "ansi" => {
                std::fs::write(&path, tuiscotti::render::ansi_dump(&frame)).map_err(|e| e.to_string())
            }
            "json" => std::fs::write(&path, frame.to_json()).map_err(|e| e.to_string()),
            "svg" => std::fs::write(&path, tuiscotti::render::render_svg(&frame, &profile))
                .map_err(|e| e.to_string()),
            "html" => (|| -> Result<(), String> {
                let html = get_renderer!()
                    .render_html(&frame, "frame")
                    .map_err(|e| e.to_string())?;
                std::fs::write(&path, html).map_err(|e| e.to_string())
            })(),
            "png" => (|| -> Result<(), String> {
                let rendered = get_renderer!().render(&frame).map_err(|e| e.to_string())?;
                std::fs::write(&path, &rendered.png).map_err(|e| e.to_string())?;
                std::fs::write(format!("{path}.fidelity.json"), rendered.fidelity.to_json())
                    .map_err(|e| e.to_string())
            })(),
            other => {
                eprintln!("error: unknown format {other:?} (txt|ansi|json|svg|html|png)");
                return EXIT_OP_ERROR;
            }
        };
        if let Err(e) = write_result {
            eprintln!("error: render {f}: {e}");
            return EXIT_OP_ERROR;
        }
        println!("wrote {path}");
    }
    0
}

// ---------------------------------------------------------------------------
// diff / review / report / import (offline)
// ---------------------------------------------------------------------------

fn cmd_diff(expected: &Path, actual: &Path) -> i32 {
    let expected_bytes = match std::fs::read(expected) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: read {}: {e}", expected.display());
            return EXIT_OP_ERROR;
        }
    };
    let actual_bytes = match std::fs::read(actual) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: read {}: {e}", actual.display());
            return EXIT_OP_ERROR;
        }
    };
    match tuiscotti::diff::compare_png(&expected_bytes, &actual_bytes) {
        Ok(v) => {
            println!(
                "pixels_equal={} dims_equal={} score={}",
                v.pixels_equal, v.dims_equal, v.score
            );
            if v.pixels_equal {
                0
            } else {
                EXIT_VERIFY_FAIL
            }
        }
        Err(e) => {
            eprintln!("error: PNG compare failed: {e}");
            EXIT_OP_ERROR
        }
    }
}

fn cmd_review(dir: &Path) -> i32 {
    let verdicts = match tuiscotti::proto::read_verdicts(dir) {
        Ok(v) => v,
        Err(e) => return op_error(&e),
    };
    if verdicts.is_empty() {
        println!("no verdicts in {}", dir.display());
        return 0;
    }
    let mut failed = 0u32;
    for v in &verdicts {
        if v.passed() {
            println!("PASS {}", v.name);
        } else {
            failed += 1;
            if v.detail.is_empty() {
                println!("FAIL {}", v.name);
            } else {
                println!("FAIL {} ({})", v.name, v.detail);
            }
        }
    }
    println!(
        "{} passed, {} failed",
        verdicts.len() - failed as usize,
        failed
    );
    if failed > 0 {
        EXIT_VERIFY_FAIL
    } else {
        0
    }
}

fn cmd_accept(store: &Path, name: &str) -> i32 {
    // Frozen roots (`Policy::Frozen` layout: `<name>.canonical.txt` approvals
    // directly in the root) reject acceptance unconditionally — route through
    // `frozen_accept` so the refusal stays in one place. Checked before
    // `Store::accept` so a planted `actual/` tree inside a frozen root can
    // never bless into it.
    if is_frozen_root(store) {
        return match tuiscotti::assert::frozen_accept(store, name) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("error: {e}");
                EXIT_OP_ERROR
            }
        };
    }
    match tuiscotti::snapshot::Store::new(store).accept(name) {
        Ok(()) => {
            println!("accepted `{name}` in {}", store.display());
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            EXIT_OP_ERROR
        }
    }
}

/// A frozen root holds `<name>.canonical.txt` approvals directly in the root
/// (see `Policy::Frozen`); a classic store holds `approved/`/`actual/`/`diff/`
/// subdirs instead, so the marker never collides.
fn is_frozen_root(store: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(store) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.file_name()
            .to_str()
            .is_some_and(|n| n.ends_with(".canonical.txt"))
    })
}

fn cmd_report(dir: &Path, out: &Path, title: &str) -> i32 {
    let verdicts = match tuiscotti::proto::read_verdicts(dir) {
        Ok(v) => v,
        Err(e) => return op_error(&e),
    };
    let html = tuiscotti::proto::write_html_report(&verdicts, title);
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("error: mkdir {}: {e}", parent.display());
                return EXIT_OP_ERROR;
            }
        }
    }
    if let Err(e) = std::fs::write(out, html) {
        eprintln!("error: write {}: {e}", out.display());
        return EXIT_OP_ERROR;
    }
    let failed = verdicts.iter().filter(|v| !v.passed()).count();
    println!(
        "report: {} ({} verdicts, {} failed)",
        out.display(),
        verdicts.len(),
        failed
    );
    0
}

fn cmd_import(dir: &Path) -> i32 {
    match tuiscotti::assert::import_frozen_v1(dir) {
        Ok(tree) => {
            println!("scenarios: {}", tree.scenarios.len());
            for s in &tree.scenarios {
                println!("  {}", s.name);
            }
            println!("unsupported: {}", tree.unsupported.len());
            for u in &tree.unsupported {
                println!("  {u}");
            }
            0
        }
        Err(e) => {
            eprintln!("error: import failed: {e}");
            EXIT_OP_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// sessions
// ---------------------------------------------------------------------------

fn cmd_session(cmd: SessionCmd) -> i32 {
    match cmd {
        SessionCmd::Start { name, force, argv } => {
            let argv: Vec<String> = argv.into_iter().filter(|a| a != "--").collect();
            match tuiscotti::proto::session_start(&name, &argv, force) {
                Ok(info) => {
                    println!("started: {} (pid {})", info.name, info.pid);
                    0
                }
                Err(e) => op_error(&e),
            }
        }
        SessionCmd::Stop { name } => match tuiscotti::proto::session_stop(&name) {
            Ok(info) => {
                println!("stopped: {} (was {:?})", info.name, info.status);
                0
            }
            Err(e) => op_error(&e),
        },
        SessionCmd::List => match tuiscotti::proto::session_list() {
            Ok(sessions) => {
                if sessions.is_empty() {
                    println!("no sessions");
                }
                for s in sessions {
                    println!(
                        "{} pid={} {:?} started={} argv={:?}",
                        s.name, s.pid, s.status, s.started_unix, s.argv
                    );
                }
                0
            }
            Err(e) => op_error(&e),
        },
        SessionCmd::Prune => match tuiscotti::proto::session_prune() {
            Ok(pruned) => {
                println!("pruned {} session(s)", pruned.len());
                for name in pruned {
                    println!("  {name}");
                }
                0
            }
            Err(e) => op_error(&e),
        },
        SessionCmd::Attach { name } => cmd_session_attach(&name),
    }
}

/// Best-effort human view of a named session: tails the session log as text
/// frames. Assertions remain on `Observation`s, never on this output.
/// Detached process sessions have no input transport (stdin is null), so
/// stdin bytes are drained and discarded; EOF on stdin detaches.
fn cmd_session_attach(name: &str) -> i32 {
    use std::io::Read;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let info = match tuiscotti::proto::session_list() {
        Ok(list) => list.into_iter().find(|s| s.name == *name),
        Err(e) => return op_error(&e),
    };
    let Some(info) = info else {
        eprintln!("error: [not-found] no session {name:?}");
        return EXIT_OP_ERROR;
    };
    let dir = match tuiscotti::proto::runtime_dir() {
        Ok(d) => d,
        Err(e) => return op_error(&e),
    };
    let log_path = dir.join(format!("{name}.log"));
    if !log_path.is_file() {
        eprintln!("error: [not-found] no log for session {name:?}");
        return EXIT_OP_ERROR;
    }
    println!(
        "attached: {} (pid {} {:?}) — best-effort human view; assertions stay on Observations",
        info.name, info.pid, info.status
    );
    println!("stdin is not delivered (process sessions have no input transport); EOF detaches");
    let eof = Arc::new(AtomicBool::new(false));
    let stdin_eof = Arc::clone(&eof);
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        let mut stdin = std::io::stdin().lock();
        let mut discarded: u64 = 0;
        loop {
            match stdin.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => discarded += n as u64,
                Err(_) => break,
            }
        }
        if discarded > 0 {
            eprintln!("note: discarded {discarded} input byte(s): no input transport");
        }
        stdin_eof.store(true, Ordering::SeqCst);
    });
    let mut offset: u64 = 0;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    loop {
        if eof.load(Ordering::SeqCst) {
            println!("detached: stdin EOF");
            return 0;
        }
        let bytes = std::fs::read(&log_path).unwrap_or_default();
        if bytes.len() as u64 > offset {
            use std::io::Write;
            let _ = out.write_all(&bytes[offset as usize..]);
            let _ = out.flush();
            offset = bytes.len() as u64;
        }
        let alive = tuiscotti::proto::session_list()
            .map(|l| {
                l.iter()
                    .any(|s| s.name == *name && s.status == tuiscotti::proto::SessionStatus::Running)
            })
            .unwrap_or(false);
        if !alive {
            println!("detached: session ended");
            return 0;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

// ---------------------------------------------------------------------------
// record / trace
// ---------------------------------------------------------------------------

fn cmd_record(out: &Path, max_events: u64, max_bytes: u64, argv: Vec<String>) -> i32 {
    let argv: Vec<String> = argv.into_iter().filter(|a| a != "--").collect();
    if argv.is_empty() {
        eprintln!("error: pass the command after `--`");
        return EXIT_OP_ERROR;
    }
    let mut rec = match tuiscotti::proto::Recorder::create(out, max_events, max_bytes) {
        Ok(r) => r,
        Err(e) => return op_error(&e),
    };
    let fail = |e: tuiscotti::proto::OpError| {
        eprintln!("error: {e}");
        EXIT_OP_ERROR
    };
    if let Err(e) = rec.record("start", &format!("argv={argv:?}")) {
        return fail(e);
    }
    let result = tuiscotti::command::Command::new(&argv[0])
        .args(&argv[1..])
        .run();
    if let Err(e) = rec.record(
        "output",
        &format!(
            "stdout={} stderr={} truncated={}",
            result.stdout.len(),
            result.stderr.len(),
            result.truncated
        ),
    ) {
        return fail(e);
    }
    if let Err(e) = rec.record(
        "exit",
        &format!(
            "termination={:?} code={:?} signal={:?}",
            result.status,
            result.code(),
            result.signal()
        ),
    ) {
        return fail(e);
    }
    if let Err(e) = rec.record("complete", &format!("events={}", rec.events() + 1)) {
        return fail(e);
    }
    println!("recorded {} events -> {}", rec.events(), out.display());
    match result.status {
        tuiscotti::command::Termination::Exit(c) => c,
        _ => EXIT_OP_ERROR,
    }
}

fn cmd_trace(input: &Path, kind: Option<&str>) -> i32 {
    let events = match tuiscotti::proto::read_journal(input) {
        Ok(e) => e,
        Err(e) => return op_error(&e),
    };
    for ev in events {
        if let Some(k) = kind {
            if ev.kind != k {
                continue;
            }
        }
        println!("{} {} {}", ev.seq, ev.kind, ev.detail);
    }
    0
}
