//! Offline commands: `inspect`, `render`, `diff`, `review`, `accept`,
//! `report`, `import`, `trace`.
//!
//! These never spawn a child and never require the `pty` feature: they work
//! in a `--no-default-features` build.

use std::path::Path;

use tuiscotti::proto::{self, EXIT_OP_ERROR, EXIT_VERIFY_FAIL};

use crate::cli::RenderFormat;

pub fn op_error(e: &proto::OpError) -> i32 {
    eprintln!("error: {e}");
    EXIT_OP_ERROR
}

pub fn cmd_inspect(dir: &Path) -> i32 {
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
        match proto::read_journal(&journal_path) {
            Ok(events) => println!("journal: {} events", events.len()),
            Err(e) => return op_error(&e),
        }
    }
    0
}

pub fn cmd_render(
    input: &Path,
    formats: &[RenderFormat],
    out: &str,
    font_file: Option<&Path>,
) -> i32 {
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
    for format in formats {
        let ext = format.extension();
        let path = format!("{out}.{ext}");
        if let Some(parent) = Path::new(&path).parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("error: mkdir {}: {e}", parent.display());
                    return EXIT_OP_ERROR;
                }
            }
        }
        let write_result = match format {
            RenderFormat::Txt => std::fs::write(&path, frame.text()).map_err(|e| e.to_string()),
            RenderFormat::Ansi => std::fs::write(&path, tuiscotti::render::ansi_dump(&frame))
                .map_err(|e| e.to_string()),
            RenderFormat::Json => std::fs::write(&path, frame.to_json()).map_err(|e| e.to_string()),
            RenderFormat::Svg => {
                std::fs::write(&path, tuiscotti::render::render_svg(&frame, &profile))
                    .map_err(|e| e.to_string())
            }
            RenderFormat::Html => (|| -> Result<(), String> {
                let html = get_renderer!()
                    .render_html(&frame, "frame")
                    .map_err(|e| e.to_string())?;
                std::fs::write(&path, html).map_err(|e| e.to_string())
            })(),
            RenderFormat::Png => (|| -> Result<(), String> {
                let rendered = get_renderer!().render(&frame).map_err(|e| e.to_string())?;
                std::fs::write(&path, &rendered.png).map_err(|e| e.to_string())?;
                std::fs::write(format!("{path}.fidelity.json"), rendered.fidelity.to_json())
                    .map_err(|e| e.to_string())
            })(),
        };
        if let Err(e) = write_result {
            eprintln!("error: render {ext}: {e}");
            return EXIT_OP_ERROR;
        }
        println!("wrote {path}");
    }
    0
}

pub fn cmd_diff(expected: &Path, actual: &Path) -> i32 {
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

pub fn cmd_review(dir: &Path) -> i32 {
    let verdicts = match proto::read_verdicts(dir) {
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
    if failed > 0 { EXIT_VERIFY_FAIL } else { 0 }
}

pub fn cmd_accept(store: &Path, name: &str) -> i32 {
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

pub fn cmd_report(dir: &Path, out: &Path, title: &str) -> i32 {
    let verdicts = match proto::read_verdicts(dir) {
        Ok(v) => v,
        Err(e) => return op_error(&e),
    };
    let html = proto::write_html_report(&verdicts, title);
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

pub fn cmd_import(dir: &Path) -> i32 {
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

pub fn cmd_trace(input: &Path, kind: Option<&str>) -> i32 {
    let events = match proto::read_journal(input) {
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
